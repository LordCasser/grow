//! Published prompt ranks plus the prompts still in this cut's retained rows.
//!
//! Capture reads each new source once. A later page seeks only the published
//! ordinals that fall in that page and splices the retained entries by rank.

use super::PagePosition;
use crate::session::history::{
    HistorySourceKey, HistoryTimelineEntry, HistoryTimelinePage, TimelineQuery,
    catalog_from_pieces, page_bounds, page_from_bounds, prompt_piece,
};
use crate::session::storage::SessionUpdate;
use crate::session::storage::jsonl::IndexedHistory;
use acp_transport::protocol as acp;
use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TimelineOverlay {
    base_count: u64,
    inserts: Vec<SuffixInsert>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SuffixInsert {
    /// Published ordinal strictly before this prompt. Equal ranks stay in
    /// source order and occupy consecutive merged ordinals.
    base_rank: u64,
    source: HistorySourceKey,
    message_id: Option<String>,
    display_segment: Option<u32>,
    preview: String,
}

pub(super) fn capture(
    index: &IndexedHistory,
    rows: &[(PagePosition, SessionUpdate)],
) -> io::Result<TimelineOverlay> {
    let entries = retained_entries(rows);
    if entries.is_empty() {
        return Ok(TimelineOverlay {
            base_count: 0,
            inserts: Vec::new(),
        });
    }
    #[cfg(unix)]
    {
        match index.timeline_base_count()? {
            Some(base_count) => overlay_on_base(index, base_count, entries),
            None => Ok(overlay_without_base(entries)),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = index;
        Ok(overlay_without_base(entries))
    }
}

impl TimelineOverlay {
    pub(super) fn page(
        &self,
        index: &IndexedHistory,
        query: TimelineQuery,
        limit: usize,
    ) -> io::Result<HistoryTimelinePage> {
        if self.inserts.is_empty() {
            return match query {
                TimelineQuery::Newest => index.timeline_newest(limit),
                TimelineQuery::Before(ordinal) => index.timeline_before(ordinal, limit),
                TimelineQuery::Around(ordinal) => index.timeline_around_ordinal(ordinal, limit),
            };
        }
        let total = self.base_count + self.inserts.len() as u64;
        let (start, end) = page_bounds(total, query, limit)?;
        let plan = slots(&self.inserts, start, end);
        let base_start = plan.iter().find_map(|slot| match slot {
            Slot::Base(ordinal) => Some(*ordinal),
            Slot::Insert(_) => None,
        });
        let base_end = plan.iter().rev().find_map(|slot| match slot {
            Slot::Base(ordinal) => Some(ordinal + 1),
            Slot::Insert(_) => None,
        });
        let base = match (base_start, base_end) {
            (Some(start), Some(end)) => load_base(index, start, end)?,
            _ => Vec::new(),
        };
        if let (Some(start), Some(end)) = (base_start, base_end)
            && (base.first().is_some_and(|entry| entry.ordinal != start)
                || base.len() as u64 != end - start)
        {
            return Err(invalid("history timeline rank is incomplete"));
        }
        let mut entries = Vec::with_capacity(plan.len());
        for (offset, slot) in plan.into_iter().enumerate() {
            let mut entry = match slot {
                Slot::Insert(index) => self.entry(index),
                Slot::Base(ordinal) => base[(ordinal - base_start.unwrap()) as usize].clone(),
            };
            entry.ordinal = start + offset as u64;
            entries.push(entry);
        }
        Ok(page_from_bounds(total, start, entries))
    }

    fn entry(&self, index: usize) -> HistoryTimelineEntry {
        let insert = &self.inserts[index];
        HistoryTimelineEntry {
            ordinal: 0,
            source: insert.source,
            message_id: insert.message_id.clone(),
            display_segment: insert.display_segment,
            preview: insert.preview.clone(),
        }
    }
}

fn retained_entries(rows: &[(PagePosition, SessionUpdate)]) -> Vec<HistoryTimelineEntry> {
    let mut pieces = Vec::new();
    for (position, update) in rows {
        let SessionUpdate::Acp(notification) = update else {
            continue;
        };
        let acp::SessionUpdate::UserMessageChunk(chunk) = &notification.update else {
            continue;
        };
        let source = HistorySourceKey {
            record: position.record,
            part: position.part,
        };
        if let Some(piece) = prompt_piece(source, chunk) {
            pieces.push(piece);
        }
    }
    pieces.sort_by(|left, right| left.source.cmp(&right.source));
    catalog_from_pieces(pieces)
}

fn overlay_without_base(entries: Vec<HistoryTimelineEntry>) -> TimelineOverlay {
    TimelineOverlay {
        base_count: 0,
        inserts: entries
            .into_iter()
            .map(|entry| SuffixInsert {
                base_rank: 0,
                source: entry.source,
                message_id: entry.message_id,
                display_segment: entry.display_segment,
                preview: entry.preview,
            })
            .collect(),
    }
}

#[cfg(unix)]
fn overlay_on_base(
    index: &IndexedHistory,
    base_count: u64,
    entries: Vec<HistoryTimelineEntry>,
) -> io::Result<TimelineOverlay> {
    let mut inserts = Vec::new();
    for entry in entries {
        if let Some(id) = entry.message_id.as_deref()
            && index.timeline_message_seen(id)?
        {
            continue;
        }
        let (base_rank, exact) = index.timeline_rank_bound(entry.source, entry.display_segment)?;
        if exact {
            continue;
        }
        inserts.push(SuffixInsert {
            base_rank,
            source: entry.source,
            message_id: entry.message_id,
            display_segment: entry.display_segment,
            preview: entry.preview,
        });
    }
    inserts.sort_by(|left, right| {
        left.base_rank
            .cmp(&right.base_rank)
            .then(left.source.cmp(&right.source))
            .then(
                left.display_segment
                    .unwrap_or(0)
                    .cmp(&right.display_segment.unwrap_or(0)),
            )
    });
    Ok(TimelineOverlay {
        base_count,
        inserts,
    })
}

fn load_base(
    index: &IndexedHistory,
    start: u64,
    end: u64,
) -> io::Result<Vec<HistoryTimelineEntry>> {
    #[cfg(unix)]
    return index.timeline_rank_window(start, end);
    #[cfg(not(unix))]
    {
        let _ = (index, start, end);
        Err(invalid("indexed history is unsupported"))
    }
}

enum Slot {
    Insert(usize),
    Base(u64),
}

fn merged(inserts: &[SuffixInsert], index: usize) -> u64 {
    inserts[index].base_rank + index as u64
}

fn slots(inserts: &[SuffixInsert], start: u64, end: u64) -> Vec<Slot> {
    let mut next = 0;
    while next < inserts.len() && merged(inserts, next) < start {
        next += 1;
    }
    let mut plan = Vec::with_capacity((end - start) as usize);
    for ordinal in start..end {
        if next < inserts.len() && merged(inserts, next) == ordinal {
            plan.push(Slot::Insert(next));
            next += 1;
        } else {
            plan.push(Slot::Base(ordinal - next as u64));
        }
    }
    plan
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(base_rank: u64, record: u64) -> SuffixInsert {
        SuffixInsert {
            base_rank,
            source: HistorySourceKey { record, part: 0 },
            message_id: None,
            display_segment: None,
            preview: record.to_string(),
        }
    }

    #[test]
    fn merged_ordinals_insert_an_older_source_between_published_ranks() {
        let inserts = vec![insert(1, 5), insert(1, 6)];
        let plan = slots(&inserts, 0, 4);
        assert!(matches!(
            plan.as_slice(),
            [
                Slot::Base(0),
                Slot::Insert(0),
                Slot::Insert(1),
                Slot::Base(1),
            ]
        ));
        let tail = slots(&inserts, 2, 4);
        assert!(matches!(tail.as_slice(), [Slot::Insert(1), Slot::Base(1)]));
    }
}

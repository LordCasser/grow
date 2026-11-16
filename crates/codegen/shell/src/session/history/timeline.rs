//! Fixed-cut catalog of visible user prompts.
//!
//! Entries match the transcript's UserPrompt occurrences: one message is one
//! entry even when it has several content chunks, combined display segments
//! stay separate, and identical text is kept when the message or source
//! identity differs. Preview text is bounded here so a later query never
//! reopens the message body.

use super::{HistoryRow, HistorySourceKey};
use crate::session::storage::SessionUpdate;
use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};
use std::io;

pub(crate) const TIMELINE_LIMIT: usize = 128;
const PREVIEW_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryTimelineEntry {
    pub ordinal: u64,
    pub source: HistorySourceKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Combined-display bubble within `source`. Absent for a single prompt.
    /// This is not a physical history part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_segment: Option<u32>,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryTimelinePage {
    pub total_entries: u64,
    pub entries: Vec<HistoryTimelineEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older_ordinal: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimelineQuery {
    Newest,
    Before(u64),
    Around(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromptPiece {
    pub source: HistorySourceKey,
    pub message_id: Option<String>,
    pub previews: Vec<String>,
}

pub(crate) fn prompt_piece(
    source: HistorySourceKey,
    chunk: &acp::ContentChunk,
) -> Option<PromptPiece> {
    if hidden(chunk) {
        return None;
    }
    let previews = previews(chunk);
    if previews.is_empty() {
        return None;
    }
    Some(PromptPiece {
        source,
        message_id: message_id(chunk),
        previews,
    })
}

pub(crate) fn prompt_piece_from_text(source: HistorySourceKey, text: &str) -> Option<PromptPiece> {
    let preview = bounded_preview(text);
    if preview.is_empty() {
        None
    } else {
        Some(PromptPiece {
            source,
            message_id: None,
            previews: vec![preview],
        })
    }
}

pub(crate) fn expand_piece(piece: &PromptPiece, ordinal: u64) -> Vec<HistoryTimelineEntry> {
    let combined = piece.previews.len() >= 2;
    piece
        .previews
        .iter()
        .enumerate()
        .map(|(segment, preview)| HistoryTimelineEntry {
            ordinal: ordinal + segment as u64,
            source: piece.source,
            message_id: piece.message_id.clone(),
            display_segment: combined.then_some(segment as u32),
            preview: preview.clone(),
        })
        .collect()
}

/// Collapse chunks of one message and assign dense ordinals in encounter order.
pub(crate) fn catalog_from_pieces(
    pieces: impl IntoIterator<Item = PromptPiece>,
) -> Vec<HistoryTimelineEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();
    for piece in pieces {
        if piece.previews.is_empty() {
            continue;
        }
        if let Some(id) = piece.message_id.as_ref()
            && !seen.insert(id.clone())
        {
            continue;
        }
        let ordinal = entries.len() as u64;
        entries.extend(expand_piece(&piece, ordinal));
    }
    entries
}

pub(crate) fn capture(rows: &[HistoryRow]) -> Vec<HistoryTimelineEntry> {
    let mut pieces = Vec::new();
    for row in rows {
        let SessionUpdate::Acp(notification) = &row.update else {
            continue;
        };
        let acp::SessionUpdate::UserMessageChunk(chunk) = &notification.update else {
            continue;
        };
        if let Some(piece) = prompt_piece(row.source, chunk) {
            pieces.push(piece);
        }
    }
    catalog_from_pieces(pieces)
}

pub(crate) fn page_bounds(
    count: u64,
    query: TimelineQuery,
    limit: usize,
) -> io::Result<(u64, u64)> {
    let limit = (limit.min(TIMELINE_LIMIT) as u64).min(count);
    match query {
        TimelineQuery::Newest => Ok((count.saturating_sub(limit), count)),
        TimelineQuery::Before(ordinal) => {
            if ordinal == 0 || ordinal > count {
                return Err(illegal_ordinal());
            }
            let width = (limit.min(TIMELINE_LIMIT as u64)).min(ordinal);
            Ok((ordinal - width, ordinal))
        }
        TimelineQuery::Around(ordinal) => {
            if count == 0 || ordinal >= count {
                return Err(illegal_ordinal());
            }
            if limit == 0 {
                return Ok((ordinal, ordinal));
            }
            let half = limit / 2;
            let mut start = ordinal.saturating_sub(half);
            let mut end = start.saturating_add(limit).min(count);
            if end.saturating_sub(start) < limit {
                start = end.saturating_sub(limit);
            }
            if ordinal < start {
                start = ordinal;
                end = start.saturating_add(limit).min(count);
            }
            if ordinal >= end {
                end = ordinal + 1;
                start = end.saturating_sub(limit);
            }
            Ok((start, end))
        }
    }
}

pub(crate) fn page_from(
    entries: &[HistoryTimelineEntry],
    query: TimelineQuery,
    limit: usize,
) -> io::Result<HistoryTimelinePage> {
    let (start, end) = page_bounds(entries.len() as u64, query, limit)?;
    Ok(page_from_bounds(
        entries.len() as u64,
        start,
        entries[start as usize..end as usize].to_vec(),
    ))
}

pub(crate) fn page_from_bounds(
    total: u64,
    start: u64,
    entries: Vec<HistoryTimelineEntry>,
) -> HistoryTimelinePage {
    HistoryTimelinePage {
        total_entries: total,
        entries,
        older_ordinal: (start > 0).then_some(start),
    }
}

fn illegal_ordinal() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "history timeline ordinal is outside the published cut",
    )
}

fn bounded_preview(text: &str) -> String {
    text.chars().take(PREVIEW_CHARS).collect()
}

fn hidden(chunk: &acp::ContentChunk) -> bool {
    chunk
        .meta
        .as_ref()
        .and_then(|meta| meta.get("hideFromScrollback"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

fn message_id(chunk: &acp::ContentChunk) -> Option<String> {
    chunk
        .meta
        .as_ref()
        .and_then(|meta| meta.get("messageId"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn text_block(chunk: &acp::ContentChunk) -> Option<&acp::TextContent> {
    match &chunk.content {
        acp::ContentBlock::Text(text) => Some(text),
        _ => None,
    }
}

fn previews(chunk: &acp::ContentChunk) -> Vec<String> {
    let Some(text) = text_block(chunk) else {
        return Vec::new();
    };
    if let Some(segments) = text
        .meta
        .as_ref()
        .and_then(|meta| meta.get("combinedDisplayTexts"))
        .and_then(serde_json::Value::as_array)
    {
        let segments = segments
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|segment| !segment.is_empty())
            .map(bounded_preview)
            .collect::<Vec<_>>();
        if segments.len() >= 2 {
            return segments;
        }
    }
    let body = text
        .meta
        .as_ref()
        .and_then(|meta| meta.get("displayText"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(text.text.as_str());
    let preview = bounded_preview(body);
    if preview.is_empty() {
        Vec::new()
    } else {
        vec![preview]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(record: u64, message: Option<&str>, previews: &[&str]) -> PromptPiece {
        PromptPiece {
            source: HistorySourceKey { record, part: 0 },
            message_id: message.map(str::to_owned),
            previews: previews.iter().map(|text| (*text).to_owned()).collect(),
        }
    }

    fn text_chunk(text: &str, meta: serde_json::Value) -> acp::ContentChunk {
        acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text)))
            .meta(meta.as_object().cloned())
    }

    #[test]
    fn duplicate_text_stays_distinct_and_extra_chunks_collapse() {
        let entries = catalog_from_pieces([
            piece(1, Some("a"), &["same"]),
            piece(2, Some("a"), &["more"]),
            piece(3, Some("b"), &["same"]),
            piece(4, None, &["same"]),
            piece(5, Some("combo"), &["alpha", "beta"]),
        ]);
        let previews: Vec<_> = entries.iter().map(|entry| entry.preview.as_str()).collect();
        assert_eq!(previews, ["same", "same", "same", "alpha", "beta"]);
        assert_eq!(entries[0].message_id.as_deref(), Some("a"));
        assert_eq!(entries[1].message_id.as_deref(), Some("b"));
        assert_eq!(entries[2].message_id, None);
        assert_eq!(entries[2].source, HistorySourceKey { record: 4, part: 0 });
        assert_eq!(entries[3].source, HistorySourceKey { record: 5, part: 0 });
        assert_eq!(entries[4].source, entries[3].source);
        assert_eq!(entries[0].display_segment, None);
        assert_eq!(entries[3].display_segment, Some(0));
        assert_eq!(entries[4].display_segment, Some(1));
        assert_eq!(entries[3].message_id, entries[4].message_id);
    }

    #[test]
    fn hidden_chunks_are_omitted_and_preview_is_bounded() {
        let hidden = text_chunk(
            "secret",
            serde_json::json!({"hideFromScrollback": true, "messageId": "h"}),
        );
        assert!(prompt_piece(HistorySourceKey { record: 1, part: 0 }, &hidden).is_none());
        let long = "你".repeat(161);
        let shown = text_chunk(&long, serde_json::json!({"messageId": "long"}));
        let piece = prompt_piece(HistorySourceKey { record: 2, part: 0 }, &shown).unwrap();
        assert_eq!(piece.previews[0].chars().count(), 160);
        assert!(long.starts_with(&piece.previews[0]));
    }

    #[test]
    fn pages_seek_a_bounded_ordinal_window_and_reject_illegal_ranks() {
        let entries = catalog_from_pieces(
            (0..200).map(|record| piece(record, Some(&record.to_string()), &["x"])),
        );
        let newest = page_from(&entries, TimelineQuery::Newest, 10_000).unwrap();
        assert_eq!(newest.total_entries, 200);
        assert_eq!(newest.entries.len(), TIMELINE_LIMIT);
        assert_eq!(newest.entries[0].ordinal, 72);
        assert_eq!(newest.older_ordinal, Some(72));
        let before = page_from(&entries, TimelineQuery::Before(72), 4).unwrap();
        assert_eq!(
            before
                .entries
                .iter()
                .map(|entry| entry.ordinal)
                .collect::<Vec<_>>(),
            vec![68, 69, 70, 71]
        );
        let around = page_from(&entries, TimelineQuery::Around(0), 4).unwrap();
        assert_eq!(around.entries[0].ordinal, 0);
        assert!(around.entries.iter().any(|entry| entry.ordinal == 0));
        assert!(page_from(&entries, TimelineQuery::Before(0), 4).is_err());
        assert!(page_from(&entries, TimelineQuery::Around(200), 4).is_err());
    }
}

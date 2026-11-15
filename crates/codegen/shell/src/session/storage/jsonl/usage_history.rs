//! Disk pages for usage attribution that is not needed to execute a session.

use chat_state::{UsageAgent, UsageSegment, UsageSegmentId, UsageSegmentPage, UsageTotals};
use redb::{ReadableTable, TableDefinition, WriteTransaction};
use std::io;

type SegmentKey = (u8, u64);

const SEGMENT_IDS: TableDefinition<SegmentKey, ()> = TableDefinition::new("usage_segment_ids");
const SEGMENTS: TableDefinition<(u8, u64, u64), &[u8]> =
    TableDefinition::new("usage_segments_at_publication");
const CHILDREN: TableDefinition<(u8, u64, &str), &[u8]> =
    TableDefinition::new("usage_segment_children");

#[derive(serde::Serialize, serde::Deserialize)]
struct ChildUsage {
    publication_bytes: u64,
    totals: UsageTotals,
}

fn key(id: UsageSegmentId) -> SegmentKey {
    match id {
        UsageSegmentId::Initial => (0, 0),
        UsageSegmentId::Resume(seq) => (1, seq.get()),
    }
}

fn id(key: SegmentKey) -> io::Result<UsageSegmentId> {
    match key {
        (0, 0) => Ok(UsageSegmentId::Initial),
        (1, seq) => Ok(UsageSegmentId::Resume(chat_state::EventSeq::new(seq))),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid usage segment identity",
        )),
    }
}

fn decode(bytes: &[u8]) -> io::Result<UsageSegment> {
    serde_json::from_slice(bytes).map_err(io::Error::other)
}

pub(super) fn publish(
    tx: &WriteTransaction,
    publication_bytes: u64,
    id: UsageSegmentId,
    segment: &UsageSegment,
) -> io::Result<()> {
    let (kind, sequence) = key(id);
    {
        let mut children = tx.open_table(CHILDREN).map_err(io::Error::other)?;
        for (agent, totals) in &segment.by_agent {
            let UsageAgent::Subagent(child_id) = agent else {
                continue;
            };
            let encoded = serde_json::to_vec(&ChildUsage {
                publication_bytes,
                totals: totals.clone(),
            })?;
            if let Some(existing) = children
                .get((kind, sequence, child_id.as_str()))
                .map_err(io::Error::other)?
            {
                let existing: ChildUsage =
                    serde_json::from_slice(existing.value()).map_err(io::Error::other)?;
                if existing.totals != *totals {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "usage child attribution changed after settlement",
                    ));
                }
            } else {
                children
                    .insert((kind, sequence, child_id.as_str()), encoded.as_slice())
                    .map_err(io::Error::other)?;
            }
        }
    }
    let mut segment = segment.clone();
    segment
        .by_agent
        .retain(|agent, _| matches!(agent, UsageAgent::Owner));
    let encoded = serde_json::to_vec(&segment)?;
    tx.open_table(SEGMENT_IDS)
        .map_err(io::Error::other)?
        .insert(key(id), ())
        .map_err(io::Error::other)?;
    tx.open_table(SEGMENTS)
        .map_err(io::Error::other)?
        .insert(
            (key(id).0, key(id).1, publication_bytes),
            encoded.as_slice(),
        )
        .map_err(io::Error::other)?;
    Ok(())
}

fn add_children(
    children: &impl ReadableTable<(u8, u64, &'static str), &'static [u8]>,
    publication_bytes: u64,
    id: UsageSegmentId,
    segment: &mut UsageSegment,
) -> io::Result<usize> {
    let (kind, sequence) = key(id);
    let end = sequence
        .checked_add(1)
        .map(|next| (kind, next, ""))
        .unwrap_or((kind + 1, 0, ""));
    let mut bytes = 0usize;
    for row in children
        .range((kind, sequence, "")..end)
        .map_err(io::Error::other)?
    {
        let (stored_key, value) = row.map_err(io::Error::other)?;
        let (stored_kind, stored_sequence, child_id) = stored_key.value();
        if stored_kind != kind || stored_sequence != sequence {
            continue;
        }
        let stored: ChildUsage = serde_json::from_slice(value.value()).map_err(io::Error::other)?;
        if stored.publication_bytes <= publication_bytes {
            bytes = bytes.saturating_add(value.value().len());
            segment
                .by_agent
                .insert(UsageAgent::Subagent(child_id.to_owned()), stored.totals);
        }
    }
    Ok(bytes)
}

pub(super) fn get(
    tx: &redb::ReadTransaction,
    publication_bytes: u64,
    id: UsageSegmentId,
) -> io::Result<Option<UsageSegment>> {
    let segments = tx.open_table(SEGMENTS).map_err(io::Error::other)?;
    let children = tx.open_table(CHILDREN).map_err(io::Error::other)?;
    let (kind, sequence) = key(id);
    let mut segment = segments
        .range((kind, sequence, 0)..=(kind, sequence, publication_bytes))
        .map_err(io::Error::other)?
        .next_back()
        .map(|entry| decode(entry.map_err(io::Error::other)?.1.value()))
        .transpose()?;
    if let Some(segment) = &mut segment {
        add_children(&children, publication_bytes, id, segment)?;
    }
    Ok(segment)
}

pub(super) fn get_write(
    tx: &WriteTransaction,
    publication_bytes: u64,
    id: UsageSegmentId,
) -> io::Result<Option<UsageSegment>> {
    let segments = tx.open_table(SEGMENTS).map_err(io::Error::other)?;
    let children = tx.open_table(CHILDREN).map_err(io::Error::other)?;
    let (kind, sequence) = key(id);
    let mut segment = segments
        .range((kind, sequence, 0)..=(kind, sequence, publication_bytes))
        .map_err(io::Error::other)?
        .next_back()
        .map(|entry| decode(entry.map_err(io::Error::other)?.1.value()))
        .transpose()?;
    if let Some(segment) = &mut segment {
        add_children(&children, publication_bytes, id, segment)?;
    }
    Ok(segment)
}

pub(super) fn page(
    tx: &redb::ReadTransaction,
    publication_bytes: u64,
    before: Option<UsageSegmentId>,
    byte_limit: usize,
) -> io::Result<UsageSegmentPage> {
    let ids = tx.open_table(SEGMENT_IDS).map_err(io::Error::other)?;
    let segments = tx.open_table(SEGMENTS).map_err(io::Error::other)?;
    let children = tx.open_table(CHILDREN).map_err(io::Error::other)?;
    let mut candidates = match before.map(key) {
        Some(before) => ids.range(..before).map_err(io::Error::other)?,
        None => ids.range::<SegmentKey>(..).map_err(io::Error::other)?,
    };
    let mut page = UsageSegmentPage::default();
    let mut bytes = 0usize;
    while let Some(row) = candidates.next_back() {
        let (segment_key, _) = row.map_err(io::Error::other)?;
        let segment_key = segment_key.value();
        let Some(value) = segments
            .range(
                (segment_key.0, segment_key.1, 0)
                    ..=(segment_key.0, segment_key.1, publication_bytes),
            )
            .map_err(io::Error::other)?
            .next_back()
        else {
            continue;
        };
        let value = value.map_err(io::Error::other)?;
        let mut segment = decode(value.1.value())?;
        add_children(&children, publication_bytes, id(segment_key)?, &mut segment)?;
        let encoded_len = serde_json::to_vec(&segment)?.len();
        if !page.segments.is_empty() && bytes.saturating_add(encoded_len) > byte_limit {
            page.next_before = page.segments.last().map(|(id, _)| *id);
            break;
        }
        bytes = bytes.saturating_add(encoded_len);
        page.segments.push((id(segment_key)?, segment));
        if bytes >= byte_limit {
            page.next_before = Some(id(segment_key)?);
            break;
        }
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use redb::ReadableDatabase;

    #[test]
    fn segment_pages_are_cut_bound_and_byte_continuable() {
        let file = tempfile::tempfile().unwrap();
        let db = redb::Database::builder().create_file(file).unwrap();
        let initial = UsageSegment {
            totals: chat_state::UsageTotals {
                input_tokens: 10,
                ..Default::default()
            },
            by_agent: [(
                UsageAgent::Subagent("child-1".into()),
                UsageTotals {
                    input_tokens: 4,
                    model_calls: 1,
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let resumed = UsageSegment {
            resume_event_seq: Some(chat_state::EventSeq::new(0)),
            totals: chat_state::UsageTotals {
                input_tokens: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        let corrected_initial = UsageSegment {
            totals: chat_state::UsageTotals {
                input_tokens: 11,
                ..Default::default()
            },
            by_agent: [(
                UsageAgent::Subagent("child-2".into()),
                UsageTotals {
                    input_tokens: 8,
                    model_calls: 1,
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let tx = db.begin_write().unwrap();
        publish(&tx, 10, UsageSegmentId::Initial, &initial).unwrap();
        publish(
            &tx,
            10,
            UsageSegmentId::Resume(chat_state::EventSeq::new(0)),
            &resumed,
        )
        .unwrap();
        publish(&tx, 20, UsageSegmentId::Initial, &corrected_initial).unwrap();
        tx.commit().unwrap();

        let tx = db.begin_read().unwrap();
        assert_eq!(
            get(&tx, 10, UsageSegmentId::Initial)
                .unwrap()
                .unwrap()
                .totals
                .input_tokens,
            10
        );
        assert_eq!(
            get(&tx, 20, UsageSegmentId::Initial)
                .unwrap()
                .unwrap()
                .totals
                .input_tokens,
            11
        );
        assert_eq!(
            get(
                &tx,
                10,
                UsageSegmentId::Resume(chat_state::EventSeq::new(0))
            )
            .unwrap()
            .unwrap()
            .totals
            .input_tokens,
            20
        );
        assert!(
            get(&tx, 10, UsageSegmentId::Initial)
                .unwrap()
                .unwrap()
                .by_agent
                .get(&UsageAgent::Subagent("child-2".into()))
                .is_none()
        );
        assert_eq!(
            get(&tx, 20, UsageSegmentId::Initial)
                .unwrap()
                .unwrap()
                .by_agent[&UsageAgent::Subagent("child-2".into())]
                .input_tokens,
            8
        );

        let first = page(&tx, 10, None, 1).unwrap();
        assert_eq!(
            first.segments[0].0,
            UsageSegmentId::Resume(chat_state::EventSeq::new(0))
        );
        let cursor = first.next_before.unwrap();
        let second = page(&tx, 10, Some(cursor), 1).unwrap();
        assert_eq!(second.segments[0].0, UsageSegmentId::Initial);
        assert!(second.segments[0].1.totals.input_tokens == 10);
        assert_eq!(
            second.segments[0].1.by_agent[&UsageAgent::Subagent("child-1".into())].input_tokens,
            4,
        );
    }
}

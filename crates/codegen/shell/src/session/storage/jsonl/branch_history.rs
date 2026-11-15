use super::JsonlStorageAdapter;
use super::tool_history::DomainReference;
use crate::session::history::{HistorySnapshot, JournalFact, JournalRecord};
use crate::session::storage::{self, ContainedDirectory, TIMELINE_FILE};
use chat_state::{
    ImageProjectionEvent, ImageShadowSource, SidebandTimeline, Timeline, TimelineEventKind,
};
use redb::{ReadTransaction, ReadableTable, TableDefinition, WriteTransaction};
use std::{ffi::OsStr, io};

const EXCLUDED: TableDefinition<(u64, u64), u64> = TableDefinition::new("rewind_excluded_ranges");

/// Reuse the canonical projector at an explicit branch transformation. This
/// read runs only when publishing the image projection, never on its reopen.
pub(super) fn project_image(
    directory: &ContainedDirectory,
    session: &str,
    source: DomainReference,
    projection: &ImageProjectionEvent,
    check_cancelled: &impl Fn() -> io::Result<()>,
) -> io::Result<Timeline> {
    let (timeline, _) = read_transformation(
        directory,
        source.record.offset + source.record.len + 1,
        source.seq.get() + 1,
        false,
        check_cancelled,
    )?;
    let mut ledgers = storage::SidebandLedgers::new();
    for shadow in &projection.shadows {
        let ImageShadowSource::Description { result_ref } = &shadow.provenance else {
            continue;
        };
        let id = &result_ref.timeline_id;
        if let std::collections::btree_map::Entry::Vacant(entry) = ledgers.entry(id.clone())
            && let Some(events) = JsonlStorageAdapter::read_sideband_ledger(directory, id)?
        {
            let proof = SidebandTimeline::from_events(events.clone()).map_err(invalid)?;
            let (seq, spawn) = timeline
                .events()
                .iter()
                .find_map(|event| match &event.kind {
                    TimelineEventKind::Sideband(spawn) if &spawn.sideband_id == id => {
                        Some((event.seq.get(), spawn))
                    }
                    _ => None,
                })
                .ok_or_else(|| invalid("missing image description spawn"))?;
            proof
                .validate_parent(session, &timeline, seq, spawn)
                .map_err(invalid)?;
            entry.insert(events);
        }
    }
    storage::validate_image_projection_evidence(projection, &ledgers)?;
    Ok(timeline)
}

pub(super) fn read_transformation(
    directory: &ContainedDirectory,
    end: u64,
    domain: u64,
    display: bool,
    check_cancelled: &impl Fn() -> io::Result<()>,
) -> io::Result<(Timeline, HistorySnapshot)> {
    let name = OsStr::new(TIMELINE_FILE);
    let mut journal = directory.open_regular(name, "branch transformation journal")?;
    let (events, history, prefix) = JsonlStorageAdapter::read_timeline_prefix_with_index(
        &mut journal,
        end,
        &directory.display_path().join(name),
        Some(|record: &JournalRecord, _| {
            check_cancelled()?;
            Ok(display || matches!(record.fact, JournalFact::Domain { .. }))
        }),
    )?;
    if prefix.state.events != domain {
        return Err(invalid(
            "branch transformation differs from its journal coordinate",
        ));
    }
    Ok((Timeline::from_events(events).map_err(invalid)?, history))
}

pub(super) fn publish_display(
    tx: &WriteTransaction,
    directory: &ContainedDirectory,
    session: &str,
    timeline: &Timeline,
    mut history: HistorySnapshot,
    position: u64,
) -> io::Result<()> {
    history.validate(timeline)?;
    history.load_notices(directory, timeline, session)?;
    let ranges = history.rewind_visibility(
        &acp_transport::protocol::SessionId::new(session.to_owned()),
        timeline,
    )?;
    let mut table = tx.open_table(EXCLUDED).map_err(io::Error::other)?;
    for range in ranges {
        table
            .insert((position, range.end), range.start)
            .map_err(io::Error::other)?;
    }
    Ok(())
}

/// Return the nearest range that may contain display rows at or before
/// `anchor`, skipping a discarded branch with two B-tree seeks.
pub(super) fn visible_range(
    tx: &ReadTransaction,
    publication: u64,
    anchor: u64,
) -> io::Result<std::ops::RangeInclusive<u64>> {
    let table = tx.open_table(EXCLUDED).map_err(io::Error::other)?;
    visible_range_in(&table, publication, anchor)
}

pub(super) fn visible_at_write(
    tx: &WriteTransaction,
    publication: Option<u64>,
    position: u64,
) -> io::Result<bool> {
    let Some(publication) = publication else {
        return Ok(true);
    };
    let table = tx.open_table(EXCLUDED).map_err(io::Error::other)?;
    Ok(visible_range_in(&table, publication, position)?.contains(&position))
}

fn visible_range_in(
    table: &impl ReadableTable<(u64, u64), u64>,
    publication: u64,
    anchor: u64,
) -> io::Result<std::ops::RangeInclusive<u64>> {
    let mut upper = anchor;
    if let Some(row) = table
        .range((publication, anchor.saturating_add(1))..=(publication, u64::MAX))
        .map_err(io::Error::other)?
        .next()
    {
        let (_, start) = row.map_err(io::Error::other)?;
        if start.value() <= anchor {
            let Some(before) = start.value().checked_sub(1) else {
                return Ok(1..=0);
            };
            upper = before;
        }
    }
    let lower = table
        .range((publication, 0)..=(publication, upper))
        .map_err(io::Error::other)?
        .next_back()
        .transpose()
        .map_err(io::Error::other)?
        .map_or(0, |(key, _)| key.value().1);
    Ok(lower..=upper)
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

//! Pre-change replay oracle retained for tests and test-support tooling only.
//!
//! This module must never be used for production reads or writes. It models the
//! old updates.jsonl projection-cache and cursor reconciliation behavior.

use super::*;

#[doc(hidden)]
pub struct PreparedReplay<'a> {
    /// Rewind-filtered replay lines, each borrowed from the input transcript.
    pub lines: Vec<&'a str>,
    pub(crate) mark_replay: bool,
    /// Highest `eventId` counter across all live (rewind-filtered) lines, used
    /// to re-seed the process-global event counter on resume so post-load live
    /// events keep monotonically increasing ids (see
    /// [`crate::util::event_id::ensure_event_counter_at_least`]). `None` when no
    /// line carried a parseable `eventId`.
    pub(crate) max_event_seq: Option<u64>,
    pub(crate) total_live: usize,
    /// UI cache coverage for canonical subagent lifecycle facts.
    pub(crate) subagent_projections: SubagentProjectionState,
}

/// Coverage of canonical lifecycle facts already present in the replay cache.
pub(crate) fn collect_subagent_projection_state(filtered: &[&str]) -> SubagentProjectionState {
    use crate::extensions::notification::SessionUpdate as Update;
    let mut state = SubagentProjectionState::default();
    for line in filtered {
        if !line.contains("subagent_spawned") && !line.contains("subagent_finished") {
            continue;
        }
        let Ok(envelope) = serde_json::from_str::<RawLinePeek<'_>>(line) else {
            continue;
        };
        if envelope.method != GROW_SESSION_UPDATE_METHOD {
            continue;
        }
        let Ok(notification) = serde_json::from_str::<SessionNotification>(envelope.params.get())
        else {
            continue;
        };
        match notification.update {
            Update::SubagentSpawned { subagent_id, .. } => {
                state.spawned.insert(subagent_id);
            }
            Update::SubagentFinished { subagent_id, .. } => {
                state.finished.insert(subagent_id);
            }
            _ => {}
        }
    }
    state
}

/// The raw `_meta` object of a canonical persisted envelope, if any, without
/// allocating a `serde_json::Value`.
fn line_meta(line: &str) -> Option<&serde_json::value::RawValue> {
    let env = serde_json::from_str::<RawLinePeek<'_>>(line).ok()?;
    if !matches!(
        env.method,
        ACP_SESSION_UPDATE_METHOD | GROW_SESSION_UPDATE_METHOD
    ) {
        return None;
    }
    serde_json::from_str::<RawParamsPeek<'_>>(env.params.get())
        .ok()?
        .meta
}

/// The `"update":` object key (a protocol key, not an enum discriminant). The
/// structural `params.update` is the FIRST occurrence in a persisted line: the
/// envelope prefix has no `"update":`, and any nested `"update"` (in `_meta` or a
/// tool's `rawInput`/`rawOutput`) is serialized after it, so the first match delimits it.
pub(crate) const UPDATE_KEY: &str = r#""update":"#;

/// Is this persisted line an `available_commands_update`?
///
/// The slash-command catalog is re-advertised in full after every `session/load`,
/// so the historical copies in `updates.jsonl` are redundant on replay and
/// dominate large sessions (~51% of bytes in pathological cases). The lines stay
/// on disk; this only skips forwarding them to the client.
///
/// A cheap [`AVAILABLE_COMMANDS_UPDATE_PREFIX`] substring pre-filter, then a
/// positional confirm that the value at the first [`UPDATE_KEY`] begins with the
/// ACU discriminant. Reads only the prefix (never the huge `availableCommands`
/// array), so it can't be fooled by the discriminant embedded in `_meta` or a
/// tool payload (never the first `"update":`).
pub(crate) fn line_is_available_commands_update(line: &str) -> bool {
    if !line.contains(&*AVAILABLE_COMMANDS_UPDATE_PREFIX) {
        return false;
    }
    line.find(UPDATE_KEY)
        .map(|pos| {
            line[pos + UPDATE_KEY.len()..]
                .trim_start()
                .starts_with(&*AVAILABLE_COMMANDS_UPDATE_PREFIX)
        })
        .unwrap_or(false)
}

// `_meta` protocol field names (not enum discriminants).
/// `_meta` key holding the per-event id used for cursor-based reconnect.
const EVENT_ID_KEY: &str = "eventId";

/// This line's `_meta.eventId`, if any. Cheap peek (no `Value`).
fn line_event_id(line: &str) -> Option<std::borrow::Cow<'_, str>> {
    if !line.contains(EVENT_ID_KEY) {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct EventIdPeek<'a> {
        // `Cow` so an escaped eventId still parses and compares equal
        // (`Option<Cow>` always deserializes owned; `&str` would error).
        #[serde(rename = "eventId", borrow)]
        event_id: Option<std::borrow::Cow<'a, str>>,
    }
    serde_json::from_str::<EventIdPeek<'_>>(line_meta(line)?.get())
        .ok()
        .and_then(|e| e.event_id)
}

/// Does this line's `_meta.eventId` equal `cursor_id`?
fn line_has_event_id(line: &str, cursor_id: &str) -> bool {
    line_event_id(line).as_deref() == Some(cursor_id)
}

#[derive(Debug, Clone)]
pub(crate) enum ReconciledReplayLine<'a> {
    Borrowed(&'a str),
    /// Reconciliation changed the source position or synthesized this row.
    /// A verified exact payload may still borrow its physical JSON bytes.
    Synthesized(std::borrow::Cow<'a, str>),
}

impl ReconciledReplayLine<'_> {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(line) => line,
            Self::Synthesized(line) => line,
        }
    }
}

#[derive(Debug)]
pub(crate) struct RawReconciliation<'a> {
    pub(crate) lines: Vec<ReconciledReplayLine<'a>>,
    pub(crate) changed: bool,
}

pub(crate) fn projection_envelope(
    projection: &crate::session::response_projection::ResponseReplayProjection,
) -> io::Result<String> {
    let update = SessionUpdate::ResponseReplayProjection(Box::new(projection.clone()));
    let envelope = SessionUpdateEnvelope::from_update(&update)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    serde_json::to_string(&envelope)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Rewind-filtered raw JSONL reconciliation. Ordinary ACP/Grow payloads are
/// only classified by peeks; projection records are the sole typed decode.
pub(crate) fn reconcile_raw_replay_lines<'a>(
    session_id: &acp::SessionId,
    timeline: &chat_state::Timeline,
    lines: Vec<&'a str>,
) -> io::Result<RawReconciliation<'a>> {
    reconcile_raw_replay_lines_with_mode(
        session_id,
        timeline,
        lines,
        None,
        crate::session::response_projection::ProjectionPlanningMode::Reconcile,
    )
}

fn reconcile_raw_replay_lines_with_mode<'a>(
    session_id: &acp::SessionId,
    timeline: &chat_state::Timeline,
    lines: Vec<&'a str>,
    response_projections: Option<&[crate::session::response_projection::ResponseReplayProjection]>,
    mode: crate::session::response_projection::ProjectionPlanningMode,
) -> io::Result<RawReconciliation<'a>> {
    let _timer = crate::instrumentation_timer!("session.replay.response_reconcile");
    use crate::session::response_projection::{
        PlannedProjection, ProjectionObservation, plan_response_projections,
        project_admitted_response_view,
    };
    let generated;
    let canonicals = match response_projections {
        // Only the owning load can supply this set, and must discard it if
        // its Timeline changes before replay. Physical rows still require
        // full exact validation against it below.
        Some(canonicals) => canonicals,
        None => {
            let _timer = crate::instrumentation_timer!("session.projection.raw_canonical");
            generated = timeline
                .admitted_response_views()
                .iter()
                .map(|response| {
                    project_admitted_response_view(session_id, *response)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
                })
                .collect::<Result<Vec<_>, _>>()?;
            &generated
        }
    };
    let mut candidate_ids = Vec::with_capacity(lines.len());
    let mut projections = Vec::new();
    let mut kinds = Vec::with_capacity(lines.len());
    for line in &lines {
        let Some((kind, candidate)) = raw_projection_observation(line, &mut projections) else {
            kinds.push(RawObservationKind::Other);
            candidate_ids.push(None);
            continue;
        };
        kinds.push(kind);
        candidate_ids.push(candidate);
    }
    let observations = kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| match kind {
            RawObservationKind::Candidate => ProjectionObservation::Candidate(
                candidate_ids[index]
                    .as_ref()
                    .expect("candidate observation identity"),
            ),
            RawObservationKind::Projection(projection) => {
                ProjectionObservation::Projection(&projections[*projection])
            }
            RawObservationKind::MalformedProjection => ProjectionObservation::MalformedProjection,
            RawObservationKind::Other => ProjectionObservation::Other,
        })
        .collect::<Vec<_>>();
    let plan = plan_response_projections(timeline, canonicals, &observations, mode)?;
    let mut result = Vec::with_capacity(plan.entries.len());
    for entry in plan.entries {
        match entry {
            PlannedProjection::Existing(index) => {
                result.push(ReconciledReplayLine::Borrowed(lines[index]))
            }
            PlannedProjection::Canonical { index, exact } => {
                let line = match exact {
                    Some(physical) => std::borrow::Cow::Borrowed(lines[physical]),
                    None => std::borrow::Cow::Owned(projection_envelope(&canonicals[index])?),
                };
                result.push(ReconciledReplayLine::Synthesized(line));
            }
        }
    }
    Ok(RawReconciliation {
        lines: result,
        changed: plan.changed,
    })
}

enum RawObservationKind {
    Candidate,
    Projection(usize),
    MalformedProjection,
    Other,
}

fn raw_projection_observation(
    line: &str,
    projections: &mut Vec<crate::session::response_projection::ResponseReplayProjection>,
) -> Option<(RawObservationKind, Option<(String, u32)>)> {
    let env = serde_json::from_str::<RawLinePeek<'_>>(line).ok()?;
    if env.method == crate::session::response_projection::RESPONSE_REPLAY_PROJECTION_METHOD {
        let Ok(projection) = serde_json::from_str(env.params.get()) else {
            return Some((RawObservationKind::MalformedProjection, None));
        };
        projections.push(projection);
        return Some((RawObservationKind::Projection(projections.len() - 1), None));
    }
    if env.method != ACP_SESSION_UPDATE_METHOD {
        return None;
    }
    let params = serde_json::from_str::<RawParamsPeek<'_>>(env.params.get()).ok()?;
    let meta = params.meta?;
    #[derive(serde::Deserialize)]
    struct CandidateMeta<'a> {
        #[serde(rename = "samplingRequestId", borrow)]
        request_id: Option<&'a str>,
        #[serde(rename = "samplingAttempt")]
        attempt: Option<u64>,
    }
    let meta = serde_json::from_str::<CandidateMeta<'_>>(meta.get()).ok()?;
    Some((
        RawObservationKind::Candidate,
        Some((
            meta.request_id?.to_owned(),
            u32::try_from(meta.attempt?).ok()?,
        )),
    ))
}

pub(crate) struct PreparedReconciledReplay<'a> {
    pub(crate) lines: Vec<ReconciledReplayLine<'a>>,
    pub(crate) mark_replay: bool,
    pub(crate) max_event_seq: Option<u64>,
    pub(crate) total_live: usize,
    pub(crate) subagent_projections: SubagentProjectionState,
}

/// Seed the process-wide event counter from durable projection identities
/// before a loaded session actor can publish new persistent rows. A Timeline
/// expectation may have reached disk immediately before a crash that prevented
/// its cache row from being appended, so cache-only replay reseeding is too
/// late (and can be skipped entirely by no-replay loads).
pub(crate) fn reseed_event_counter_from_projection_expectations(
    timeline: &chat_state::Timeline,
) -> Option<u64> {
    let max = timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::DisplayProjectionExpected(projection) => projection
                .event_id
                .rsplit_once('-')
                .and_then(|(_, suffix)| suffix.parse::<u64>().ok()),
            _ => None,
        })
        .max();
    if let Some(max) = max {
        crate::util::event_id::ensure_event_counter_at_least(max.saturating_add(1));
    }
    max
}

/// Prepare the replay directly from the one physical snapshot. Canonical
/// projection rows are owned only when synthesized; physical rows remain
/// borrowed from `contents`.
pub(crate) fn prepare_reconciled_replay_lines<'a>(
    session_id: &acp::SessionId,
    timeline: &chat_state::Timeline,
    contents: &'a str,
    response_projections: Option<&[crate::session::response_projection::ResponseReplayProjection]>,
    cursor: Option<&str>,
    read_notice: impl FnMut(
        &chat_state::TimelineEvent,
    ) -> io::Result<Option<crate::extensions::notification::UiNotice>>,
) -> io::Result<PreparedReconciledReplay<'a>> {
    let filtered = filter_rewind_lines(contents.lines().filter(|l| !l.trim().is_empty()).collect());
    let max_event_seq = max_event_seq(&filtered);
    let total_live = filtered
        .iter()
        .filter(|line| !line_is_available_commands_update(line))
        .count();
    let subagent_projections = collect_subagent_projection_state(&filtered);
    let reconciled = reconcile_raw_replay_lines_with_mode(
        session_id,
        timeline,
        filtered.clone(),
        response_projections,
        crate::session::response_projection::ProjectionPlanningMode::Reconcile,
    )?;
    let reconciled = {
        let _timer = crate::instrumentation_timer!("session.replay.communication_restore");
        communication_history::restore(session_id, timeline, reconciled, read_notice)?
    };
    let cursor_pos = cursor
        .and_then(|id| {
            filtered
                .iter()
                .rposition(|line| line_has_event_id(line, id))
        })
        .filter(|&pos| {
            let bounded = filtered[pos + 1..].iter().all(|line| {
                line_is_available_commands_update(line) || line_event_id(line).is_some()
            });
            if !bounded {
                tracing::warn!(
                    "replay: post-cursor tail contains eventId-less lines; full replay instead"
                );
            }
            bounded
        });
    let mark_replay = cursor_pos.is_none() || (cursor.is_some() && reconciled.changed);
    // ACUs participate in cursor resolution and physical positions, but are
    // never forwarded. This mirrors `prepare_replay_lines` without building a
    // second whole contents String.
    let reconciled_lines = reconciled
        .lines
        .into_iter()
        .filter(|line| !line_is_available_commands_update(line.as_str()));
    let lines = if mark_replay {
        reconciled_lines.collect()
    } else {
        let cursor_pos = cursor_pos.expect("incremental replay has a cursor");
        // `mark_replay == false` implies `reconciled.changed == false`, so the
        // plan contains only borrowed physical rows. Never use a pointer lookup
        // for an owned synthesized line; the map is explicitly scoped to this
        // unchanged reconciliation path.
        let physical_positions = filtered
            .iter()
            .enumerate()
            .map(|(position, line)| (((*line).as_ptr(), line.len()), position))
            .collect::<std::collections::HashMap<(*const u8, usize), usize>>();
        reconciled_lines
            .filter(|line| match line {
                ReconciledReplayLine::Borrowed(line) => physical_positions
                    .get(&(line.as_ptr(), line.len()))
                    .is_some_and(|&position| position > cursor_pos),
                ReconciledReplayLine::Synthesized(_) => false,
            })
            .collect()
    };
    Ok(PreparedReconciledReplay {
        lines,
        mark_replay,
        max_event_seq,
        total_live,
        subagent_projections,
    })
}

/// Rewind-filter and reconcile the response-projection cache in memory.
/// Storage-only projection records are synthesized from validated Timeline
/// authority and candidate rows for the exact admitted attempt are removed.
pub(crate) fn reconcile_replay_snapshot(
    session_id: &acp::SessionId,
    timeline: &chat_state::Timeline,
    contents: &str,
) -> io::Result<String> {
    let filtered = filter_rewind_lines(
        contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect(),
    );
    let reconciled = reconcile_raw_replay_lines(session_id, timeline, filtered)?;
    let mut output = String::new();
    for line in reconciled.lines {
        output.push_str(line.as_str());
        output.push('\n');
    }
    Ok(output)
}

fn max_event_seq(filtered: &[&str]) -> Option<u64> {
    let mut max_event_seq: Option<u64> = None;
    for line in filtered {
        if line.contains("eventId")
            && let Ok(env) = serde_json::from_str::<RawLinePeek<'_>>(line)
            && matches!(
                env.method,
                ACP_SESSION_UPDATE_METHOD | GROW_SESSION_UPDATE_METHOD
            )
            && let Ok(pp) = serde_json::from_str::<RawParamsPeek<'_>>(env.params.get())
            && let Some(meta_raw) = pp.meta
            && let Ok(meta) = serde_json::from_str::<serde_json::Value>(meta_raw.get())
            && let Some(seq) = meta
                .get("eventId")
                .and_then(|v| v.as_str())
                .and_then(|s| s.rsplit('-').next())
                .and_then(|c| c.parse::<u64>().ok())
        {
            max_event_seq = Some(max_event_seq.map_or(seq, |m| m.max(seq)));
        }
    }
    max_event_seq
}

/// Rewind-filter, resolve the reconnect cursor, and drop redundant command
/// catalogs. Pure UI replay processing, no agent-state recovery.
///
/// The cursor is resolved before dropping ACUs, because an idle client often
/// reconnects with an ACU's `eventId` as its cursor; resolving against the
/// ACU-inclusive set keeps reconnect incremental instead of a full replay.
///
/// `#[doc(hidden)] pub` (not stable API): production replay uses it, and the
/// session-load memory test drives it to check the peek stays zero-copy.
#[doc(hidden)]
pub fn prepare_replay_lines<'a>(contents: &'a str, cursor: Option<&str>) -> PreparedReplay<'a> {
    let filtered = filter_rewind_lines(contents.lines().filter(|l| !l.trim().is_empty()).collect());

    // Highest event counter across all live rewind-filtered lines.
    let max_event_seq = max_event_seq(&filtered);

    // Resolve the reconnect cursor against the ACU-inclusive set. `mark_replay`
    // is true for a full historical replay (no cursor, or cursor not found).
    //
    // The cursor is refused when a FORWARDED tail line lacks an `eventId`:
    // such a line cannot be covered by a future cursor and has no client-side
    // dedup, so re-delivering it as live would re-apply it. Full replay is
    // the safe fallback — the client swaps it in wholesale. Id-less lines
    // come from older binaries or any emitter outside the stamping
    // chokepoints (see `ensure_event_id_meta`). ACU lines are exempt: they
    // are dropped below, never forwarded.
    let cursor_pos = cursor
        .and_then(|id| filtered.iter().rposition(|l| line_has_event_id(l, id)))
        .filter(|&pos| {
            let bounded = filtered[pos + 1..]
                .iter()
                .all(|l| line_is_available_commands_update(l) || line_event_id(l).is_some());
            if !bounded {
                tracing::warn!(
                    "replay: post-cursor tail contains eventId-less lines; full replay instead"
                );
            }
            bounded
        });
    let mark_replay = cursor_pos.is_none();
    let start = cursor_pos.map_or(0, |pos| pos + 1);

    // Single pass: drop ACUs (kept on disk), collect the post-cursor tail to
    // forward, and count the full ACU-free live set for the skip log.
    let mut lines: Vec<&str> = Vec::with_capacity(filtered.len().saturating_sub(start));
    let mut total_live = 0usize;
    for (i, &line) in filtered.iter().enumerate() {
        if line_is_available_commands_update(line) {
            continue;
        }
        total_live += 1;
        if i >= start {
            lines.push(line);
        }
    }

    PreparedReplay {
        lines,
        mark_replay,
        max_event_seq,
        total_live,
        subagent_projections: collect_subagent_projection_state(&filtered),
    }
}

/// Blank-strip, drop redundant command catalogs, and rewind-filter a raw
/// `updates.jsonl` segment. Shared by the delta-replay path (which has no
/// reconnect cursor); the initial replay path is [`prepare_replay_lines`], which
/// additionally resolves a cursor (and so must see ACUs) before dropping them.
pub(crate) fn filter_delta_replay_lines(lines: Vec<&str>) -> Vec<&str> {
    let live: Vec<&str> = lines
        .into_iter()
        .filter(|l| !l.trim().is_empty() && !line_is_available_commands_update(l))
        .collect();
    filter_rewind_lines(live)
}

pub(crate) fn prepare_reconciled_delta_lines<'a>(
    session_id: &acp::SessionId,
    timeline: &chat_state::Timeline,
    lines: Vec<&'a str>,
) -> io::Result<Vec<ReconciledReplayLine<'a>>> {
    let filtered = filter_delta_replay_lines(lines);
    Ok(reconcile_raw_replay_lines_with_mode(
        session_id,
        timeline,
        filtered,
        None,
        crate::session::response_projection::ProjectionPlanningMode::AlreadyEmitted,
    )?
    .lines)
}

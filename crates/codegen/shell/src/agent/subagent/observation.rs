//! Authorized, read-only observations after coordinator cache loss.

use super::*;
use crate::extensions::session_admin::{BoundedSharedReadError, bounded_shared_read};
use crate::session::storage::jsonl::JsonlStorageAdapter;
use tool_types::SubagentListScope;

pub(crate) fn backend(
    tx: mpsc::UnboundedSender<SubagentEvent>,
    source_session_id: String,
    source_state: Option<chat_state::ChatStateHandle>,
) -> tools::implementations::grow_build::task::backend::ChannelBackend {
    let history_source = source_session_id.clone();
    let history_state = source_state.clone();
    let status_source = source_session_id.clone();
    let status_state = source_state.clone();
    tools::implementations::grow_build::task::backend::ChannelBackend::for_session(
        tx,
        source_session_id.clone(),
    )
    .with_history_query(move |id| {
        inspect_history(source_session_id.clone(), source_state.clone(), id)
    })
    .with_history_list(move |scope| list_history(history_source.clone(), history_state.clone(), scope))
    .with_history_status(move |ids| {
        read_source(
            status_source.clone(),
            status_state.clone(),
            move |storage, source_id, source, events| {
                read_history_statuses(storage, source_id, source, events, &ids)
            },
        )
    })
}

async fn inspect_history(
    source_session_id: String,
    source_state: Option<chat_state::ChatStateHandle>,
    subagent_id: String,
) -> Result<Option<SubagentInspection>, SubagentObservationError> {
    read_source(
        source_session_id,
        source_state,
        move |storage, id, source, events| {
            read_history_from_source(storage, id, source, events, &subagent_id)
        },
    )
    .await?
}

async fn list_history(
    source_session_id: String,
    source_state: Option<chat_state::ChatStateHandle>,
    scope: SubagentListScope,
) -> Result<Vec<tool_types::SubagentSummary>, SubagentObservationError> {
    read_source(
        source_session_id,
        source_state,
        move |storage, id, source, events| {
            read_history_list_from_source(storage, id, source, events, scope)
        },
    )
    .await
}

// One owner for accepted resident facts, pinned read-only storage and worker lifetime.
async fn read_source<T, F>(
    source_session_id: String,
    source_state: Option<chat_state::ChatStateHandle>,
    read: F,
) -> Result<T, SubagentObservationError>
where
    T: Send + 'static,
    F: FnOnce(
            &JsonlStorageAdapter,
            &str,
            &crate::session::storage::jsonl::OpenedSession,
            &[chat_state::TimelineEvent],
        ) -> std::io::Result<T>
        + Send
        + 'static,
{
    let events = match source_state {
        Some(state) => Some(
            state
                .runtime_tree_snapshot()
                .await
                .map_err(|_| SubagentObservationError::Unavailable)?
                .1,
        ),
        None => None,
    };
    bounded_shared_read(move || {
        let storage = JsonlStorageAdapter::new();
        let source = storage
            .open_session_by_id_shared_read(&source_session_id)?
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "source session missing")
            })?;
        let timeline;
        let source_events = if let Some(events) = &events {
            events.as_slice()
        } else {
            timeline = source.reference_timeline()?;
            timeline.events()
        };
        read(&storage, &source_session_id, &source, source_events)
    })
    .await
    .map_err(|error| match error {
        BoundedSharedReadError::TimedOut => SubagentObservationError::TimedOut,
        BoundedSharedReadError::Io(error) => io_observation_error(&error),
        _ => SubagentObservationError::Unavailable,
    })
}

fn io_observation_error(error: &std::io::Error) -> SubagentObservationError {
    if error.kind() == std::io::ErrorKind::InvalidData {
        SubagentObservationError::InvalidEvidence
    } else {
        SubagentObservationError::Unavailable
    }
}

#[cfg(test)]
pub(super) fn read_history(
    storage: &JsonlStorageAdapter,
    source_session_id: &str,
    subagent_id: &str,
) -> std::io::Result<Result<Option<SubagentSnapshot>, SubagentObservationError>> {
    let source = storage
        .open_session_by_id_shared_read(source_session_id)?
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "source session missing")
        })?;
    let source_timeline = source.reference_timeline()?;
    read_history_from_source(
        storage,
        source_session_id,
        &source,
        source_timeline.events(),
        subagent_id,
    )
    .map(|result| result.map(|inspection| inspection.map(|inspection| inspection.snapshot)))
}

// Source events come from the bound ChatState owner (or a validated test
// reader). They have already crossed canonical admission; do not replay the
// resident session's unrelated messages and artifacts on each status query.
fn read_history_from_source(
    storage: &JsonlStorageAdapter,
    source_session_id: &str,
    source: &crate::session::storage::jsonl::OpenedSession,
    source_events: &[chat_state::TimelineEvent],
    subagent_id: &str,
) -> std::io::Result<Result<Option<SubagentInspection>, SubagentObservationError>> {
    use chat_state::{SubagentEvent, SubagentOutcome, TimelineEventKind};
    let (root_id, root) = lifecycle_root(storage, source_session_id, source, source_events)?;
    let root_events = root.as_ref().map_or(source_events, |root| root.events());
    let Some((spawn_seq, spawned_at_ms, spawn)) =
        root_events.iter().find_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))
                if spawn.subagent_id == subagent_id =>
            {
                Some((event.seq, event.at_ms, spawn))
            }
            _ => None,
        })
    else {
        return Ok(Err(SubagentObservationError::Unauthorized));
    };
    if !visible_spawn(spawn, &root_id, source_session_id) {
        return Ok(Err(SubagentObservationError::Unauthorized));
    }
    let Some(terminal) = root_events.iter().find_map(|event| match &event.kind {
        TimelineEventKind::Subagent(SubagentEvent::Ended(terminal))
            if terminal.subagent_id == subagent_id =>
        {
            Some(terminal)
        }
        _ => None,
    }) else {
        return Ok(Err(SubagentObservationError::Unavailable));
    };
    let result = terminal_result(storage, &root_id, spawn_seq, spawn, terminal)?;
    let output = result
        .as_ref()
        .map(|(child, result)| {
            result
                .output_ref
                .as_deref()
                .map(|reference| {
                    load_subagent_output_ref_from_directory(child.directory(), reference)
                })
                .transpose()
        })
        .unwrap_or(Ok(None));
    let status = match terminal.outcome {
        SubagentOutcome::Completed => match output {
            Ok(output) => SubagentSnapshotStatus::Completed {
                output: output.unwrap_or_default(),
                tool_calls: terminal.tool_calls,
                turns: terminal.turns,
                tokens_used: terminal.tokens_used,
                worktree_path: spawn.worktree_path.clone(),
            },
            Err(error) => SubagentSnapshotStatus::CompletedOutputUnavailable {
                error,
                tool_calls: terminal.tool_calls,
                turns: terminal.turns,
                tokens_used: terminal.tokens_used,
                worktree_path: spawn.worktree_path.clone(),
            },
        },
        SubagentOutcome::Failed => SubagentSnapshotStatus::Failed {
            error: terminal
                .error
                .clone()
                .unwrap_or_else(|| "Subagent failed".into()),
        },
        SubagentOutcome::Cancelled => SubagentSnapshotStatus::Cancelled {
            reason: terminal.error.clone(),
        },
    };
    Ok(Ok(Some(SubagentInspection {
        parent_session_id: root_id.to_owned(),
        child_session_id: spawn.child_session_id.clone(),
        fork_parent_prompt_id: spawn.parent_prompt_id.clone(),
        resumed_from: spawn.resumed_from.clone(),
        snapshot: SubagentSnapshot {
            subagent_id: subagent_id.to_owned(),
            description: spawn.description.clone(),
            subagent_type: spawn.subagent_type.clone(),
            status,
            started_at_epoch_ms: spawned_at_ms.max(0) as u64,
            duration_ms: terminal.duration_ms,
        },
    })))
}

fn lifecycle_root(
    storage: &JsonlStorageAdapter,
    source_session_id: &str,
    source: &crate::session::storage::jsonl::OpenedSession,
    source_events: &[chat_state::TimelineEvent],
) -> std::io::Result<(String, Option<chat_state::Timeline>)> {
    use chat_state::{SubagentEvent, TimelineEventKind};
    let seed = source_events.iter().find_map(|event| match &event.kind {
        TimelineEventKind::SubagentSeed(seed) => Some(seed),
        _ => None,
    });
    let Some(seed) = seed else {
        return Ok((source_session_id.to_owned(), None));
    };
    let root_id = seed.parent_timeline_id.as_str();
    if root_id == source_session_id {
        return Ok((root_id.to_owned(), None));
    }
    let root = storage
        .open_session_by_id_shared_read(root_id)?
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "lifecycle root missing"))?
        .reference_timeline()?;
    let source_spawn = root
        .events()
        .iter()
        .find_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))
                if event.seq.get() == seed.parent_spawn_seq =>
            {
                Some((event.seq, spawn))
            }
            _ => None,
        })
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "source spawn missing")
        })?;
    validate_child_session_identity(&source, root_id, source_spawn.1)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    seed
        .validate_spawn_link(root_id, source_spawn.0, source_spawn.1)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    Ok((root_id.to_owned(), Some(root)))
}

fn terminal_result(
    storage: &JsonlStorageAdapter,
    root_id: &str,
    spawn_seq: chat_state::EventSeq,
    spawn: &chat_state::SubagentSpawnEvent,
    terminal: &chat_state::SubagentTerminalEvent,
) -> std::io::Result<
    Option<(
        crate::session::storage::jsonl::OpenedSession,
        chat_state::SubagentResultEvent,
    )>,
> {
    if terminal.result_ref.is_none() {
        if terminal.outcome == chat_state::SubagentOutcome::Completed {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "completed child has no result reference",
            ));
        }
        return Ok(None);
    }
    let child = storage
        .open_session_by_id_shared_read(&spawn.child_session_id)?
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "child session missing")
        })?;
    validate_child_session_identity(&child, root_id, spawn)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let timeline = child.reference_timeline()?;
    let result = timeline
        .validate_subagent_result_link(root_id, spawn_seq, spawn, terminal)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?
        .clone();
    Ok(Some((child, result)))
}

fn read_history_list_from_source(
    storage: &JsonlStorageAdapter,
    source_session_id: &str,
    source: &crate::session::storage::jsonl::OpenedSession,
    source_events: &[chat_state::TimelineEvent],
    scope: SubagentListScope,
) -> std::io::Result<Vec<tool_types::SubagentSummary>> {
    use chat_state::{SubagentEvent, SubagentOutcome, TimelineEventKind};
    use tool_types::{SubagentStatus, SubagentSummary};
    let (root_id, root) = lifecycle_root(storage, source_session_id, source, source_events)?;
    let events = root.as_ref().map_or(source_events, |root| root.events());
    let terminals = events
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Ended(ended)) => {
                Some((ended.subagent_id.as_str(), ended))
            }
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut rows = Vec::new();
    for event in events {
        let TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)) = &event.kind else {
            continue;
        };
        if !visible_spawn(spawn, &root_id, source_session_id) {
            continue;
        }
        let mut row = SubagentSummary {
            subagent_id: spawn.subagent_id.clone(),
            direct_parent_session_id: spawn.security_parent_session_id.clone(),
            subagent_type: spawn.subagent_type.clone(),
            description: spawn.description.clone(),
            status: SubagentStatus::ObservationUnavailable,
            started_at_epoch_ms: Some(event.at_ms.max(0) as u64),
            duration_ms: None,
            error: Some("No current runtime observation or verified terminal".into()),
        };
        if let Some(terminal) = terminals.get(spawn.subagent_id.as_str()) {
            // Active scope drops these rows after merging. The parent's Ended
            // fact is enough to suppress a stale nonterminal runtime row.
            let evidence = match scope {
                SubagentListScope::Active => Ok(None),
                SubagentListScope::All => terminal_result(storage, &root_id, event.seq, spawn, terminal),
            };
            match evidence {
                Ok(_) => {
                    row.status = match terminal.outcome {
                        SubagentOutcome::Completed => SubagentStatus::Completed,
                        SubagentOutcome::Failed => SubagentStatus::Failed,
                        SubagentOutcome::Cancelled => SubagentStatus::Cancelled,
                    };
                    row.duration_ms = Some(terminal.duration_ms);
                    row.error = terminal.error.clone();
                }
                Err(error) => {
                    row.status = if error.kind() == std::io::ErrorKind::InvalidData {
                        SubagentStatus::ObservationInvalidEvidence
                    } else {
                        SubagentStatus::ObservationUnavailable
                    };
                    row.error = Some(error.to_string());
                }
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
pub(super) fn read_history_list(
    storage: &JsonlStorageAdapter,
    source_id: &str,
    scope: SubagentListScope,
) -> std::io::Result<Vec<tool_types::SubagentSummary>> {
    let source = storage
        .open_session_by_id_shared_read(source_id)?
        .expect("fixture source");
    let timeline = source.reference_timeline()?;
    let facts: Vec<_> = timeline.subagent_lifecycle_events().cloned().collect();
    read_history_list_from_source(storage, source_id, &source, &facts, scope)
}

fn visible_spawn(spawn: &chat_state::SubagentSpawnEvent, root_id: &str, source_id: &str) -> bool {
    spawn.workflow_run_id.is_none()
        && resume_security_parent_allows(root_id, source_id, &spawn.security_parent_session_id)
}

fn read_history_statuses(
    storage: &JsonlStorageAdapter,
    source_id: &str,
    source: &crate::session::storage::jsonl::OpenedSession,
    source_events: &[chat_state::TimelineEvent],
    ids: &[String],
) -> std::io::Result<Vec<tool_types::SubagentDetails>> {
    use chat_state::{SubagentEvent, SubagentOutcome, TimelineEventKind};
    use tool_types::{SubagentDetails, SubagentStatistics, SubagentStatus, SubagentSummary};
    let (root_id, root) = lifecycle_root(storage, source_id, source, source_events)?;
    let events = root.as_ref().map_or(source_events, |root| root.events());
    let spawns = events
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))
                if visible_spawn(spawn, &root_id, source_id) =>
            {
                Some((spawn.subagent_id.as_str(), (event, spawn)))
            }
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    let ended = events
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Ended(terminal)) => {
                Some((terminal.subagent_id.as_str(), terminal))
            }
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    Ok(ids
        .iter()
        .map(|id| {
            let Some((event, spawn)) = spawns.get(id.as_str()) else {
                return SubagentDetails::unknown(
                    id.clone(),
                    SubagentStatus::Unauthorized,
                    "Subagent is outside the caller's authorized scope".into(),
                );
            };
            let mut summary = SubagentSummary {
                subagent_id: id.clone(),
                direct_parent_session_id: spawn.security_parent_session_id.clone(),
                subagent_type: spawn.subagent_type.clone(),
                description: spawn.description.clone(),
                status: SubagentStatus::ObservationUnavailable,
                started_at_epoch_ms: Some(event.at_ms.max(0) as u64),
                duration_ms: None,
                error: Some("No current runtime observation or verified terminal".into()),
            };
            let mut statistics = SubagentStatistics::default();
            if let Some(terminal) = ended.get(id.as_str()) {
                match terminal_result(storage, &root_id, event.seq, spawn, terminal) {
                    Ok(_) => {
                        summary.status = match terminal.outcome {
                            SubagentOutcome::Completed => SubagentStatus::Completed,
                            SubagentOutcome::Failed => SubagentStatus::Failed,
                            SubagentOutcome::Cancelled => SubagentStatus::Cancelled,
                        };
                        summary.duration_ms = Some(terminal.duration_ms);
                        summary.error = terminal.error.clone();
                        statistics = SubagentStatistics::terminal(
                            terminal.tool_calls,
                            terminal.turns,
                            terminal.tokens_used,
                        );
                    }
                    Err(error) => {
                        summary.status = if error.kind() == std::io::ErrorKind::InvalidData {
                            SubagentStatus::ObservationInvalidEvidence
                        } else {
                            SubagentStatus::ObservationUnavailable
                        };
                        summary.error = Some(error.to_string());
                    }
                }
            }
            SubagentDetails::from_summary(summary, statistics)
        })
        .collect())
}

#[cfg(test)]
pub(super) fn read_statuses(
    storage: &JsonlStorageAdapter,
    source_id: &str,
    ids: &[String],
) -> std::io::Result<Vec<tool_types::SubagentDetails>> {
    let source = storage
        .open_session_by_id_shared_read(source_id)?
        .expect("fixture source");
    let timeline = source.reference_timeline()?;
    read_history_statuses(storage, source_id, &source, timeline.events(), ids)
}

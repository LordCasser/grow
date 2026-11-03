//! Session-scoped query, inspection, and progress delivery.

use std::sync::Arc;

use futures::FutureExt;
use tokio::sync::oneshot;

use super::super::coordinator_state::{
    BlockingWaiter, CompletedChild, OutputReadRequest, OutputReadTarget, SubagentProgress,
    completed_inspection, completed_snapshot, pending_inspection, pending_snapshot,
    running_inspection, running_seed,
};
use super::super::types::{
    SubagentInspection, SubagentObservationError, SubagentSnapshot, SubagentSnapshotStatus,
};
use super::{ChildControl, ChildRunner, SubagentCoordinator, belongs_to_session};
use tool_types::{
    SubagentDetails, SubagentListScope, SubagentStatistics, SubagentStatus, SubagentSummary,
};

fn send_output_read_error(target: OutputReadTarget, error: SubagentObservationError) {
    match target {
        OutputReadTarget::Query(tx) => {
            let _ = tx.send(Err(error));
        }
        OutputReadTarget::Inspect(tx) => {
            let _ = tx.send(Err(error));
        }
    }
}

impl<R: ChildRunner> SubagentCoordinator<R> {
    pub(super) fn handle_query(
        &mut self,
        id: String,
        parent_session_id: Option<String>,
        block: bool,
        timeout_ms: Option<u64>,
        respond_to: oneshot::Sender<Result<Option<SubagentSnapshot>, SubagentObservationError>>,
    ) {
        let known = self.completed.contains_key(&id)
            || self.active.contains_key(&id)
            || self.suspended.contains_key(&id)
            || self.pending.contains_key(&id);
        let authorized = self.completed.get(&id).is_some_and(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) || self.active.get(&id).is_some_and(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) || self.pending.get(&id).is_some_and(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) || self.suspended.get(&id).is_some_and(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        });
        if (known || parent_session_id.is_some()) && !authorized {
            let _ = respond_to.send(Err(SubagentObservationError::Unauthorized));
            return;
        }
        if let Some(child) = self.completed.get(&id).filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) {
            if child.request.owner.is_workflow() {
                let _ = respond_to.send(Err(SubagentObservationError::Unauthorized));
            } else if child.result.finalization_pending && block {
                self.waiters
                    .entry(id.clone())
                    .or_default()
                    .push(BlockingWaiter {
                        deadline: tokio::time::Instant::now()
                            + std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000)),
                        respond_to,
                    });
            } else {
                self.queue_completed_output_read(
                    child.clone(),
                    OutputReadTarget::Query(respond_to),
                );
            }
            return;
        }
        if let Some(child) = self.active.get(&id).filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) {
            if child.request.owner.is_workflow() {
                let _ = respond_to.send(Err(SubagentObservationError::Unauthorized));
                return;
            }
            if let Some(outcome) = &child.execution_outcome {
                if block {
                    self.waiters.entry(id).or_default().push(BlockingWaiter {
                        deadline: tokio::time::Instant::now()
                            + std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000)),
                        respond_to,
                    });
                    return;
                }
                let _ = respond_to.send(Ok(Some(active_finalization_snapshot(child, outcome))));
            } else if block {
                self.waiters.entry(id).or_default().push(BlockingWaiter {
                    deadline: tokio::time::Instant::now()
                        + std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000)),
                    respond_to,
                });
            } else {
                let result = active_inspection(child).map(|inspection| inspection.snapshot);
                let _ = respond_to.send(result.map(Some));
            }
            return;
        }
        if let Some(child) = self.pending.get(&id).filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) {
            if child.request.owner.is_workflow() {
                let _ = respond_to.send(Err(SubagentObservationError::Unauthorized));
                return;
            }
            if block {
                self.waiters.entry(id).or_default().push(BlockingWaiter {
                    deadline: tokio::time::Instant::now()
                        + std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000)),
                    respond_to,
                });
            } else {
                let _ = respond_to.send(Ok(Some(pending_snapshot(child))));
            }
            return;
        }
        if let Some(child) = self.suspended.get_mut(&id).filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) {
            if child.request.owner.is_workflow() {
                let _ = respond_to.send(Err(SubagentObservationError::Unauthorized));
            } else if block {
                child.waiters.push(BlockingWaiter {
                    deadline: tokio::time::Instant::now()
                        + std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000)),
                    respond_to,
                });
            } else {
                let _ = respond_to.send(Ok(Some(suspended_snapshot(child))));
            }
            return;
        }
        let _ = respond_to.send(Ok(None));
    }

    pub(super) fn handle_inspect(
        &mut self,
        id: String,
        parent_session_id: Option<String>,
        respond_to: oneshot::Sender<Result<Option<SubagentInspection>, SubagentObservationError>>,
    ) {
        if let Some(child) = self.completed.get(&id).filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) {
            if parent_session_id.is_some() && child.request.owner.is_workflow() {
                let _ = respond_to.send(Err(SubagentObservationError::Unauthorized));
                return;
            }
            self.queue_completed_output_read(child.clone(), OutputReadTarget::Inspect(respond_to));
        } else if let Some(child) = self.pending.get(&id).filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) {
            if parent_session_id.is_some() && child.request.owner.is_workflow() {
                let _ = respond_to.send(Err(SubagentObservationError::Unauthorized));
                return;
            }
            let _ = respond_to.send(Ok(Some(pending_inspection(child))));
        } else if let Some(child) = self.active.get(&id).filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) {
            if parent_session_id.is_some() && child.request.owner.is_workflow() {
                let _ = respond_to.send(Err(SubagentObservationError::Unauthorized));
                return;
            }
            let inspection = child.execution_outcome.as_deref().map_or_else(
                || active_inspection(child),
                |outcome| Ok(active_finalization_inspection(child, outcome)),
            );
            let _ = respond_to.send(inspection.map(Some));
        } else if let Some(child) = self.suspended.get(&id).filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id.as_deref(),
            )
        }) {
            if parent_session_id.is_some() && child.request.owner.is_workflow() {
                let _ = respond_to.send(Err(SubagentObservationError::Unauthorized));
            } else {
                let _ = respond_to.send(Ok(Some(suspended_inspection(child))));
            }
        } else {
            let exists = self.completed.contains_key(&id)
                || self.pending.contains_key(&id)
                || self.active.contains_key(&id)
                || self.suspended.contains_key(&id);
            let result = if exists || parent_session_id.is_some() {
                Err(SubagentObservationError::Unauthorized)
            } else {
                Ok(None)
            };
            let _ = respond_to.send(result);
        }
    }

    fn queue_completed_output_read(&mut self, child: CompletedChild, target: OutputReadTarget) {
        if child.result.finalization_pending {
            let snapshot = completed_snapshot(&child, None, false);
            let inspection = completed_inspection(&child, None, false);
            match target {
                OutputReadTarget::Query(tx) => {
                    let _ = tx.send(Ok(Some(snapshot)));
                }
                OutputReadTarget::Inspect(tx) => {
                    let _ = tx.send(Ok(Some(inspection)));
                }
            }
            return;
        }
        let Some(reference) = child.persisted_output_ref.clone() else {
            let snapshot = completed_snapshot(&child, None, false);
            let inspection = completed_inspection(&child, None, false);
            match target {
                OutputReadTarget::Query(tx) => {
                    let _ = tx.send(Ok(Some(snapshot)));
                }
                OutputReadTarget::Inspect(tx) => {
                    let _ = tx.send(Ok(Some(inspection)));
                }
            }
            return;
        };
        let key = format!("{}\0{}", child.request.id, reference);
        if let Some(read) = self.output_read_requests.get_mut(&key) {
            if read.targets.len() >= 32 {
                send_output_read_error(target, SubagentObservationError::Unavailable);
            } else {
                read.targets.push(target);
            }
            return;
        }
        let Ok(permit) = self.output_read_permits.clone().try_acquire_owned() else {
            send_output_read_error(target, SubagentObservationError::Unavailable);
            return;
        };
        let future = self.runner.load_persisted_output_async(
            child.request.clone(),
            child.child_session_id.clone(),
            reference,
            permit,
        );
        self.output_read_requests.insert(
            key.clone(),
            OutputReadRequest {
                child,
                targets: vec![target],
            },
        );
        self.output_reads
            .push(super::super::coordinator_state::TaggedFuture {
                subagent_id: key,
                future: Box::pin(std::panic::AssertUnwindSafe(future).catch_unwind()),
            });
    }

    pub(super) fn finish_output_read(
        &mut self,
        key: &str,
        output: Result<Option<Arc<str>>, SubagentObservationError>,
    ) {
        let Some(read) = self.output_read_requests.remove(key) else {
            return;
        };
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                for target in read.targets {
                    send_output_read_error(target, error);
                }
                return;
            }
        };
        let unavailable = output.is_none();
        let snapshot = completed_snapshot(&read.child, output.as_deref(), unavailable);
        let inspection = completed_inspection(&read.child, output.as_deref(), unavailable);
        for target in read.targets {
            match target {
                OutputReadTarget::Query(tx) => {
                    let _ = tx.send(Ok(Some(snapshot.clone())));
                }
                OutputReadTarget::Inspect(tx) => {
                    let _ = tx.send(Ok(Some(inspection.clone())));
                }
            }
        }
    }

    pub(super) fn ready_snapshot(&self, id: &str) -> Option<SubagentSnapshot> {
        self.completed
            .get(id)
            .filter(|child| !child.request.owner.is_workflow())
            .map(|child| completed_snapshot(child, None, false))
            .or_else(|| {
                self.pending
                    .get(id)
                    .filter(|child| !child.request.owner.is_workflow())
                    .map(pending_snapshot)
            })
    }

    pub(super) fn handle_list_running(
        &mut self,
        parent_session_id: String,
        respond_to: oneshot::Sender<Result<Vec<SubagentInspection>, SubagentObservationError>>,
    ) {
        let inspections = self
            .active
            .values()
            .filter(|child| {
                belongs_to_session(
                    &child.request,
                    &child.immediate_parent_session_id,
                    Some(&parent_session_id),
                ) && !child.request.owner.is_workflow()
            })
            .map(|child| {
                child.execution_outcome.as_deref().map_or_else(
                    || active_listing_inspection(child),
                    |outcome| active_finalization_inspection(child, outcome),
                )
            })
            .collect();
        let _ = respond_to.send(Ok(inspections));
    }

    pub(super) fn handle_list_subagents(
        &self,
        parent_session_id: String,
        scope: SubagentListScope,
        respond_to: oneshot::Sender<Vec<SubagentSummary>>,
    ) {
        let mut summaries = Vec::new();
        for child in self.pending.values().filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                Some(&parent_session_id),
            ) && !child.request.owner.is_workflow()
        }) {
            summaries.push(summary(
                &child.request.id,
                &child.request.subagent_type,
                &child.request.description,
                &child.immediate_parent_session_id,
                SubagentStatus::Initializing,
                epoch_ms_from_instant(child.started_at),
                Some(child.started_at.elapsed().as_millis() as u64),
                None,
            ));
        }
        for child in self.active.values().filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                Some(&parent_session_id),
            ) && !child.request.owner.is_workflow()
        }) {
            let status = child.execution_outcome.as_deref().map_or_else(
                || {
                    if child.control.progress_snapshot().is_some() {
                        SubagentStatus::Running
                    } else {
                        SubagentStatus::RunningProgressUnavailable
                    }
                },
                |_outcome| SubagentStatus::FinalizationPending,
            );
            summaries.push(summary(
                &child.request.id,
                &child.request.subagent_type,
                &child.request.description,
                &child.immediate_parent_session_id,
                status,
                epoch_ms_from_instant(child.started_at),
                Some(child.started_at.elapsed().as_millis() as u64),
                None,
            ));
        }
        for child in self.suspended.values().filter(|child| {
            belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                Some(&parent_session_id),
            ) && !child.request.owner.is_workflow()
        }) {
            summaries.push(summary(
                &child.request.id,
                &child.request.subagent_type,
                &child.request.description,
                &child.immediate_parent_session_id,
                SubagentStatus::Suspended,
                child.started_at_epoch_ms,
                Some(child.duration_ms),
                None,
            ));
        }
        for child in self.completed.values().filter(|child| {
            child.result.finalization_pending
                && belongs_to_session(
                    &child.request,
                    &child.immediate_parent_session_id,
                    Some(&parent_session_id),
                )
                && !child.request.owner.is_workflow()
        }) {
            summaries.push(summary(
                &child.request.id,
                &child.request.subagent_type,
                &child.request.description,
                &child.immediate_parent_session_id,
                SubagentStatus::FinalizationPending,
                epoch_ms_from_instant(child.started_at),
                Some(child.result.duration_ms),
                child.result.error.clone(),
            ));
        }
        if scope == SubagentListScope::All {
            for child in self.completed.values().filter(|child| {
                belongs_to_session(
                    &child.request,
                    &child.immediate_parent_session_id,
                    Some(&parent_session_id),
                ) && !child.request.owner.is_workflow()
            }) {
                summaries.push(completed_summary(child));
            }
        }
        summaries.sort_by(|left, right| left.subagent_id.cmp(&right.subagent_id));
        summaries.dedup_by(|left, right| left.subagent_id == right.subagent_id);
        let _ = respond_to.send(summaries);
    }

    pub(super) fn handle_get_statuses(
        &self,
        ids: Vec<String>,
        parent_session_id: Option<String>,
        respond_to: oneshot::Sender<Vec<Result<Option<SubagentDetails>, SubagentObservationError>>>,
    ) {
        let results = ids
            .iter()
            .map(|id| self.get_status(id, parent_session_id.as_deref()))
            .collect();
        let _ = respond_to.send(results);
    }

    fn get_status(
        &self,
        id: &str,
        parent_session_id: Option<&str>,
    ) -> Result<Option<SubagentDetails>, SubagentObservationError> {
        let known = self.pending.contains_key(id)
            || self.active.contains_key(id)
            || self.suspended.contains_key(id)
            || self.completed.contains_key(id);
        if let Some(child) = self.pending.get(id) {
            if !belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id,
            ) || child.request.owner.is_workflow()
            {
                return Err(SubagentObservationError::Unauthorized);
            }
            return Ok(Some(details(
                summary(
                    id,
                    &child.request.subagent_type,
                    &child.request.description,
                    &child.immediate_parent_session_id,
                    SubagentStatus::Initializing,
                    epoch_ms_from_instant(child.started_at),
                    Some(child.started_at.elapsed().as_millis() as u64),
                    None,
                ),
                SubagentStatistics::default(),
            )));
        }
        if let Some(child) = self.active.get(id) {
            if !belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id,
            ) || child.request.owner.is_workflow()
            {
                return Err(SubagentObservationError::Unauthorized);
            }
            let progress = child.control.progress_snapshot();
            let status = if child.execution_outcome.is_some() {
                SubagentStatus::FinalizationPending
            } else if progress.is_some() {
                SubagentStatus::Running
            } else {
                SubagentStatus::RunningProgressUnavailable
            };
            return Ok(Some(details(
                summary(
                    id,
                    &child.request.subagent_type,
                    &child.request.description,
                    &child.immediate_parent_session_id,
                    status,
                    epoch_ms_from_instant(child.started_at),
                    Some(child.started_at.elapsed().as_millis() as u64),
                    None,
                ),
                progress.map_or_else(SubagentStatistics::default, progress_statistics),
            )));
        }
        if let Some(child) = self.suspended.get(id) {
            if !belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id,
            ) || child.request.owner.is_workflow()
            {
                return Err(SubagentObservationError::Unauthorized);
            }
            return Ok(Some(details(
                summary(
                    id,
                    &child.request.subagent_type,
                    &child.request.description,
                    &child.immediate_parent_session_id,
                    SubagentStatus::Suspended,
                    child.started_at_epoch_ms,
                    Some(child.duration_ms),
                    None,
                ),
                SubagentStatistics::default(),
            )));
        }
        if let Some(child) = self.completed.get(id) {
            if !belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id,
            ) || child.request.owner.is_workflow()
            {
                return Err(SubagentObservationError::Unauthorized);
            }
            return Ok(Some(details(
                completed_summary(child),
                SubagentStatistics::terminal(
                    child.result.tool_calls,
                    child.result.turns,
                    child.result.total_tokens_used,
                ),
            )));
        }
        if known || parent_session_id.is_some() {
            Err(SubagentObservationError::Unauthorized)
        } else {
            Ok(None)
        }
    }
}

fn summary(
    subagent_id: &str,
    subagent_type: &str,
    description: &str,
    direct_parent_session_id: &str,
    status: SubagentStatus,
    started_at_epoch_ms: u64,
    duration_ms: Option<u64>,
    error: Option<String>,
) -> SubagentSummary {
    SubagentSummary {
        subagent_id: subagent_id.to_owned(),
        direct_parent_session_id: direct_parent_session_id.to_owned(),
        subagent_type: subagent_type.to_owned(),
        description: description.to_owned(),
        status,
        started_at_epoch_ms: Some(started_at_epoch_ms),
        duration_ms,
        error,
    }
}

fn details(summary: SubagentSummary, statistics: SubagentStatistics) -> SubagentDetails {
    SubagentDetails::from_summary(summary, statistics)
}

fn progress_statistics(progress: SubagentProgress) -> SubagentStatistics {
    SubagentStatistics {
        tool_call_count: Some(progress.tool_call_count),
        turn_count: Some(progress.turn_count),
        context_tokens: Some(progress.tokens_used),
        context_window_tokens: Some(progress.context_window_tokens),
        context_usage_pct: Some(progress.context_usage_pct),
        error_count: Some(progress.error_count),
        tools_used: Some(progress.tools_used),
        ..SubagentStatistics::default()
    }
}

fn completed_summary(child: &CompletedChild) -> SubagentSummary {
    let status = if child.result.finalization_pending {
        SubagentStatus::FinalizationPending
    } else if child.result.cancelled {
        SubagentStatus::Cancelled
    } else if child.result.success && child.result.output_unavailable {
        SubagentStatus::OutputUnavailable
    } else if child.result.success {
        SubagentStatus::Completed
    } else {
        SubagentStatus::Failed
    };
    summary(
        &child.request.id,
        &child.request.subagent_type,
        &child.request.description,
        &child.immediate_parent_session_id,
        status,
        epoch_ms_from_instant(child.started_at),
        Some(child.result.duration_ms),
        child
            .finalization_error
            .clone()
            .or_else(|| child.result.error.clone()),
    )
}

fn epoch_ms_from_instant(instant: std::time::Instant) -> u64 {
    let elapsed_ms = std::time::Instant::now()
        .saturating_duration_since(instant)
        .as_millis() as u64;
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|now| now.as_millis() as u64)
        .unwrap_or_default()
        .saturating_sub(elapsed_ms)
}

pub(super) fn suspended_snapshot<D>(child: &super::SuspendedChild<D>) -> SubagentSnapshot {
    SubagentSnapshot {
        subagent_id: child.request.id.clone(),
        description: child.request.description.clone(),
        subagent_type: child.request.subagent_type.clone(),
        status: SubagentSnapshotStatus::Suspended {
            operation_id: child.checkpoint.operation_id.clone(),
            checkpoint_seq: child.checkpoint.checkpoint_seq,
        },
        started_at_epoch_ms: child.started_at_epoch_ms,
        duration_ms: child.duration_ms,
    }
}

fn suspended_inspection<D>(child: &super::SuspendedChild<D>) -> SubagentInspection {
    SubagentInspection {
        snapshot: suspended_snapshot(child),
        parent_session_id: child.request.parent_session_id.clone(),
        child_session_id: child.checkpoint.session_id.clone(),
        fork_parent_prompt_id: child.request.parent_prompt_id.clone(),
        resumed_from: child.request.resume_from.clone(),
    }
}

fn active_inspection<C: ChildControl>(
    child: &super::super::coordinator_state::ActiveChild<C>,
) -> Result<SubagentInspection, SubagentObservationError> {
    let progress = child
        .control
        .progress_snapshot()
        .ok_or(SubagentObservationError::Unavailable)?;
    Ok(running_inspection(running_seed(child), progress))
}

fn active_listing_inspection<C: ChildControl>(
    child: &super::super::coordinator_state::ActiveChild<C>,
) -> SubagentInspection {
    let seed = running_seed(child);
    let status = child.control.progress_snapshot().map_or(
        SubagentSnapshotStatus::RunningProgressUnavailable,
        |progress| SubagentSnapshotStatus::Running {
            turn_count: progress.turn_count,
            tool_call_count: progress.tool_call_count,
            tokens_used: progress.tokens_used,
            context_window_tokens: progress.context_window_tokens,
            context_usage_pct: progress.context_usage_pct,
            tools_used: progress.tools_used,
            error_count: progress.error_count,
        },
    );
    SubagentInspection {
        snapshot: SubagentSnapshot {
            subagent_id: seed.subagent_id,
            description: seed.description,
            subagent_type: seed.subagent_type,
            status,
            started_at_epoch_ms: seed.started_at_epoch_ms,
            duration_ms: seed.duration_ms,
        },
        parent_session_id: seed.parent_session_id,
        child_session_id: seed.child_session_id,
        fork_parent_prompt_id: seed.fork_parent_prompt_id,
        resumed_from: seed.resumed_from,
    }
}

pub(super) fn active_finalization_snapshot<C>(
    child: &super::super::coordinator_state::ActiveChild<C>,
    outcome: &str,
) -> SubagentSnapshot {
    let seed = running_seed(child);
    SubagentSnapshot {
        subagent_id: seed.subagent_id,
        description: seed.description,
        subagent_type: seed.subagent_type,
        status: super::super::types::SubagentSnapshotStatus::FinalizationPending {
            outcome: outcome.to_owned(),
            error: None,
        },
        started_at_epoch_ms: seed.started_at_epoch_ms,
        duration_ms: seed.duration_ms,
    }
}

fn active_finalization_inspection<C>(
    child: &super::super::coordinator_state::ActiveChild<C>,
    outcome: &str,
) -> SubagentInspection {
    let seed = running_seed(child);
    SubagentInspection {
        snapshot: active_finalization_snapshot(child, outcome),
        parent_session_id: seed.parent_session_id,
        child_session_id: seed.child_session_id,
        fork_parent_prompt_id: seed.fork_parent_prompt_id,
        resumed_from: seed.resumed_from,
    }
}

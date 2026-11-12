use super::*;
use crate::implementations::grow_build::task::{
    backend::SubagentBackend,
    types::{SubagentObservationError, SubagentSnapshot, SubagentSnapshotStatus},
};
use futures::{StreamExt, stream::FuturesUnordered};
use tool_types::{
    SubagentStatus, SubagentWaitReason, SubagentWaitResult, WaitSubagentInput, WaitSubagentOutput,
};

#[derive(Debug, Default)]
pub struct WaitSubagentTool;
impl ToolMetadata for WaitSubagentTool {
    fn kind(&self) -> ToolKind {
        ToolKind::SubagentWait
    }
    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }
    fn description_template(&self) -> &str {
        "Wait until any selected subagent reaches a terminal state and retrieve its result. Pass one or more subagent_ids. timeout_ms is a positive total call budget (default 30000, capped at {max_wait_ms}). An already finished child returns immediately even while others run. A terminal result uses reason=any_finished. Every return includes actual waited_ms and every selected ID with its latest observed state; unobserved siblings are observation_interrupted. Analyze each returned terminal child's status and result before waiting again on remaining dependencies. Failed or cancelled children also end the wait; reaching the deadline or an observation error does not mean completion. Suspended or finalizing children remain unfinished. Ending the wait does not cancel other children or consume their completion notifications. ${%- if tools.by_kind.subagent_list %} Use ${{ tools.by_kind.subagent_list }} to discover IDs.${%- endif %}${%- if tools.by_kind.subagent_status %} Use ${{ tools.by_kind.subagent_status }} for non-blocking details.${%- endif %} Does not inspect command tasks."
    }
}
impl tool_runtime::Tool for WaitSubagentTool {
    type Args = WaitSubagentInput;
    type Output = WaitSubagentOutput;
    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("wait_subagent").unwrap()
    }
    fn description(&self, _: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new("wait_subagent", self.sanitized_description_template())
    }
    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::Read,
            ..Default::default()
        }
    }
    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: Self::Args,
    ) -> Result<Self::Output, tool_runtime::ToolError> {
        let start = tokio::time::Instant::now();
        let ids = tool_types::resolve_task_ids(&input.subagent_ids);
        if ids.is_empty() || ids.len() > tool_types::MAX_MULTI_WAIT_IDS {
            return Err(tool_runtime::ToolError::invalid_arguments(
                "Provide 1 to 20 subagent_ids.",
            ));
        }
        let budget_ms = input
            .timeout_ms
            .unwrap_or(30_000)
            .min(tool_types::max_wait_block_ms());
        if budget_ms == 0 {
            return Err(tool_runtime::ToolError::invalid_arguments(
                "wait_subagent requires a positive timeout and an enabled wait budget.",
            ));
        }
        let deadline = start + std::time::Duration::from_millis(budget_ms);
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let backend = match tokio::time::timeout_at(deadline, resources.lock()).await {
            Ok(res) => res.require::<SubagentBackendResource>()?.clone(),
            Err(_) => {
                return Ok(finish(
                    start,
                    ids.into_iter()
                        .map(|id| wait_error(id, SubagentObservationError::TimedOut))
                        .collect(),
                    true,
                ));
            }
        };
        let subagents = wait_for_any(backend.backend(), ids, deadline).await;
        Ok(finish(
            start,
            subagents,
            tokio::time::Instant::now() >= deadline,
        ))
    }
}

async fn wait_for_any(
    backend: &dyn SubagentBackend,
    ids: Vec<String>,
    deadline: tokio::time::Instant,
) -> Vec<SubagentWaitResult> {
    let mut observations = vec![None; ids.len()];
    let mut waits = ids
        .iter()
        .zip(&mut observations)
        .map(|(id, observation)| async move {
            let row = wait_one(backend, id.clone(), deadline, observation).await;
            let terminal = row.status.is_terminal();
            *observation = Some(row);
            terminal
        })
        .collect::<FuturesUnordered<_>>();
    while let Some(terminal) = waits.next().await {
        if terminal {
            break;
        }
    }
    // Drop only the remaining observations; the backend still owns child execution.
    drop(waits);
    ids.into_iter()
        .zip(observations)
        .map(|(id, observation)| {
            observation.unwrap_or_else(|| {
                SubagentWaitResult::unknown(
                    id,
                    SubagentStatus::ObservationInterrupted,
                    "Observation stopped because another subagent reached a terminal state; this child's execution state is unconfirmed.".into(),
                )
            })
        })
        .collect()
}

fn finish(
    start: tokio::time::Instant,
    subagents: Vec<SubagentWaitResult>,
    deadline_elapsed: bool,
) -> WaitSubagentOutput {
    let reason = if subagents.iter().any(|row| row.status.is_terminal()) {
        SubagentWaitReason::AnyFinished
    } else if deadline_elapsed {
        SubagentWaitReason::DeadlineElapsed
    } else {
        SubagentWaitReason::ObservationFailed
    };
    WaitSubagentOutput {
        reason,
        waited_ms: start.elapsed().as_millis() as u64,
        subagents,
    }
}
async fn wait_one(
    backend: &dyn SubagentBackend,
    id: String,
    deadline: tokio::time::Instant,
    last: &mut Option<SubagentWaitResult>,
) -> SubagentWaitResult {
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return last
                .take()
                .unwrap_or_else(|| wait_error(id, SubagentObservationError::TimedOut));
        }
        let timeout_ms = remaining.as_millis().max(1) as u64;
        match tokio::time::timeout_at(deadline, backend.query(&id, true, Some(timeout_ms))).await {
            Ok(Ok(Some(snapshot))) => {
                let row = from_snapshot(snapshot);
                if row.status.is_terminal() {
                    return row;
                }
                *last = Some(row);
                // The backend's bounded segment can expire before the call's budget.
                // Continue all nonterminal phases under the original deadline.
            }
            Ok(Ok(None)) => return wait_error(id, SubagentObservationError::Unavailable),
            Ok(Err(error)) => return wait_error(id, error),
            Err(_) => {
                return last
                    .take()
                    .unwrap_or_else(|| wait_error(id, SubagentObservationError::TimedOut));
            }
        }
    }
}
pub(super) fn wait_error(id: String, error: SubagentObservationError) -> SubagentWaitResult {
    let status = match error {
        SubagentObservationError::Unavailable => SubagentStatus::ObservationUnavailable,
        SubagentObservationError::TimedOut => SubagentStatus::ObservationTimedOut,
        SubagentObservationError::InvalidEvidence => SubagentStatus::ObservationInvalidEvidence,
        SubagentObservationError::Unauthorized => SubagentStatus::Unauthorized,
    };
    SubagentWaitResult::unknown(id, status, error.to_string())
}
pub(super) fn from_snapshot(snapshot: SubagentSnapshot) -> SubagentWaitResult {
    use SubagentSnapshotStatus::*;
    let (status, output, error) = match snapshot.status {
        Initializing => (SubagentStatus::Initializing, None, None),
        Running { .. } => (SubagentStatus::Running, None, None),
        RunningProgressUnavailable => (SubagentStatus::RunningProgressUnavailable, None, None),
        Suspended { .. } => (SubagentStatus::Suspended, None, None),
        FinalizationPending { error, .. } => (SubagentStatus::FinalizationPending, None, error),
        Completed { output, .. } => (SubagentStatus::Completed, Some(output), None),
        CompletedOutputUnavailable { error, .. } => {
            (SubagentStatus::OutputUnavailable, None, Some(error))
        }
        Failed { error } => (SubagentStatus::Failed, None, Some(error)),
        Cancelled { reason } => (SubagentStatus::Cancelled, None, reason),
    };
    SubagentWaitResult {
        subagent_id: snapshot.subagent_id,
        status,
        started_at_epoch_ms: Some(snapshot.started_at_epoch_ms),
        duration_ms: Some(snapshot.duration_ms),
        output,
        error,
    }
}

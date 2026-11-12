use super::{GetSubagentResultInput, GetSubagentResultTool, wait::WaitSubagentTool};
use crate::implementations::grow_build::task::backend::{SubagentBackend, SubagentBackendResource};
use crate::implementations::grow_build::task::types::*;
use crate::types::{resources::Resources, tool_metadata::test_ctx};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tool_runtime::Tool;
use tool_types::{SubagentStatus, SubagentWaitReason, WaitSubagentInput};

struct ResultBackend {
    status: SubagentSnapshotStatus,
}
#[async_trait::async_trait]
impl SubagentBackend for ResultBackend {
    async fn spawn(&self, _: SubagentRequest) -> Result<SubagentResult, tool_runtime::ToolError> {
        unreachable!()
    }
    async fn query(
        &self,
        id: &str,
        block: bool,
        timeout: Option<u64>,
    ) -> Result<Option<SubagentSnapshot>, SubagentObservationError> {
        assert!(!block);
        assert!(timeout.is_none());
        match id {
            "foreign" => Err(SubagentObservationError::Unauthorized),
            _ => Ok(Some(SubagentSnapshot {
                subagent_id: id.into(),
                description: "child".into(),
                subagent_type: "explore".into(),
                status: self.status.clone(),
                started_at_epoch_ms: 1,
                duration_ms: 900,
            })),
        }
    }
    async fn cancel(&self, _: &str) -> Result<SubagentCancelOutcome, SubagentObservationError> {
        unreachable!()
    }
    async fn validate_type(&self, _: &str, _: &str) -> SubagentValidateTypeOutcome {
        unreachable!()
    }
}
fn result_ctx(status: SubagentSnapshotStatus) -> tool_runtime::ToolCallContext {
    let mut r = Resources::new();
    r.insert(SubagentBackendResource(Arc::new(ResultBackend { status })));
    test_ctx(r.into_shared())
}

#[derive(Clone)]
enum WaitBehavior {
    TerminalAfter(Duration, SubagentSnapshotStatus),
    Sequence(Vec<(Duration, SubagentSnapshotStatus)>),
    SnapshotThenPending(SubagentSnapshotStatus),
    Pending,
    Unauthorized,
}

struct WaitBackend {
    behaviors: HashMap<String, WaitBehavior>,
    calls: Mutex<HashMap<String, usize>>,
    cancel_calls: Arc<AtomicUsize>,
}
#[async_trait::async_trait]
impl SubagentBackend for WaitBackend {
    async fn spawn(&self, _: SubagentRequest) -> Result<SubagentResult, tool_runtime::ToolError> {
        unreachable!()
    }
    async fn query(
        &self,
        id: &str,
        block: bool,
        timeout: Option<u64>,
    ) -> Result<Option<SubagentSnapshot>, SubagentObservationError> {
        assert!(block);
        assert!(timeout.unwrap() <= 1000);
        let behavior = self.behaviors.get(id).unwrap().clone();
        let call = {
            let mut calls = self.calls.lock().unwrap();
            let call = calls.entry(id.to_owned()).or_default();
            *call += 1;
            *call
        };
        match behavior {
            WaitBehavior::TerminalAfter(delay, status) => {
                tokio::time::sleep(delay).await;
                Ok(Some(snapshot(id, status)))
            }
            WaitBehavior::Sequence(sequence) if call <= sequence.len() => {
                let (delay, status) = sequence[call - 1].clone();
                tokio::time::sleep(delay).await;
                Ok(Some(snapshot(id, status)))
            }
            WaitBehavior::Sequence(_) => std::future::pending().await,
            WaitBehavior::SnapshotThenPending(status) if call == 1 => {
                Ok(Some(snapshot(id, status)))
            }
            WaitBehavior::SnapshotThenPending(_) | WaitBehavior::Pending => {
                std::future::pending().await
            }
            WaitBehavior::Unauthorized => Err(SubagentObservationError::Unauthorized),
        }
    }
    async fn cancel(&self, _: &str) -> Result<SubagentCancelOutcome, SubagentObservationError> {
        self.cancel_calls.fetch_add(1, Ordering::SeqCst);
        Ok(SubagentCancelOutcome::Cancelled)
    }
    async fn validate_type(&self, _: &str, _: &str) -> SubagentValidateTypeOutcome {
        unreachable!()
    }
}
fn snapshot(id: &str, status: SubagentSnapshotStatus) -> SubagentSnapshot {
    SubagentSnapshot {
        subagent_id: id.into(),
        description: "child".into(),
        subagent_type: "explore".into(),
        status,
        started_at_epoch_ms: 1,
        duration_ms: 900,
    }
}
fn wait_ctx(
    behaviors: Vec<(&str, WaitBehavior)>,
) -> (tool_runtime::ToolCallContext, Arc<AtomicUsize>) {
    let cancel_calls = Arc::new(AtomicUsize::new(0));
    let mut r = Resources::new();
    r.insert(SubagentBackendResource(Arc::new(WaitBackend {
        behaviors: behaviors
            .into_iter()
            .map(|(id, behavior)| (id.into(), behavior))
            .collect(),
        calls: Mutex::new(HashMap::new()),
        cancel_calls: cancel_calls.clone(),
    })));
    (test_ctx(r.into_shared()), cancel_calls)
}
#[tokio::test(start_paused = true)]
async fn public_wait_observes_single_child_through_suspension_and_finalization() {
    let (ctx, _) = wait_ctx(vec![(
        "child",
        WaitBehavior::Sequence(vec![
            (
                Duration::from_millis(100),
                SubagentSnapshotStatus::Suspended {
                    operation_id: "suspend".into(),
                    checkpoint_seq: 1,
                },
            ),
            (
                Duration::from_millis(100),
                SubagentSnapshotStatus::FinalizationPending {
                    outcome: "completed".into(),
                    error: None,
                },
            ),
            (
                Duration::from_millis(100),
                SubagentSnapshotStatus::Completed {
                    output: "verified answer".into(),
                    tool_calls: 3,
                    turns: 2,
                    tokens_used: 5,
                    worktree_path: None,
                },
            ),
        ]),
    )]);
    let output = WaitSubagentTool
        .run(
            ctx,
            WaitSubagentInput {
                subagent_ids: vec![" child ".into(), "child".into()],
                timeout_ms: Some(1000),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(output.reason).unwrap(),
        serde_json::json!("any_finished")
    );
    assert_eq!(output.waited_ms, 300);
    assert_eq!(output.subagents.len(), 1);
    assert_eq!(output.subagents[0].subagent_id, "child");
    assert_eq!(output.subagents[0].status, SubagentStatus::Completed);
    assert_eq!(
        output.subagents[0].output.as_deref(),
        Some("verified answer")
    );
}
#[tokio::test(start_paused = true)]
async fn public_wait_returns_on_one_terminal_and_preserves_observed_snapshots() {
    let (ctx, cancel_calls) = wait_ctx(vec![
        (
            "suspended",
            WaitBehavior::SnapshotThenPending(SubagentSnapshotStatus::Suspended {
                operation_id: "suspend".into(),
                checkpoint_seq: 1,
            }),
        ),
        (
            "finalizing",
            WaitBehavior::SnapshotThenPending(SubagentSnapshotStatus::FinalizationPending {
                outcome: "completed".into(),
                error: None,
            }),
        ),
        (
            "done",
            WaitBehavior::TerminalAfter(
                Duration::from_millis(300),
                SubagentSnapshotStatus::Completed {
                    output: "verified answer".into(),
                    tool_calls: 3,
                    turns: 2,
                    tokens_used: 5,
                    worktree_path: None,
                },
            ),
        ),
        ("stalled", WaitBehavior::Pending),
    ]);
    let output = WaitSubagentTool
        .run(
            ctx,
            WaitSubagentInput {
                subagent_ids: vec![
                    "suspended".into(),
                    "finalizing".into(),
                    "done".into(),
                    "stalled".into(),
                ],
                timeout_ms: Some(1000),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(output.reason).unwrap(),
        serde_json::json!("any_finished")
    );
    assert_eq!(output.waited_ms, 300);
    assert_eq!(output.subagents.len(), 4);
    assert_eq!(
        output
            .subagents
            .iter()
            .map(|row| row.subagent_id.as_str())
            .collect::<Vec<_>>(),
        ["suspended", "finalizing", "done", "stalled"]
    );
    assert_eq!(output.subagents[0].status, SubagentStatus::Suspended);
    assert_eq!(
        output.subagents[1].status,
        SubagentStatus::FinalizationPending
    );
    assert_eq!(
        output.subagents[3].status,
        SubagentStatus::ObservationInterrupted
    );
    assert_eq!(
        output.subagents[2].output.as_deref(),
        Some("verified answer")
    );
    assert_eq!(cancel_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        crate::reminders::task_completion::consumed_completion_ids(
            &crate::types::output::ToolOutput::SubagentWait(output)
        ),
        ["done"]
    );
}
#[tokio::test(start_paused = true)]
async fn public_wait_returns_immediate_terminal_even_with_stalled_sibling() {
    for (status, expected_status) in [
        (
            SubagentSnapshotStatus::Completed {
                output: "verified answer".into(),
                tool_calls: 3,
                turns: 2,
                tokens_used: 5,
                worktree_path: None,
            },
            SubagentStatus::Completed,
        ),
        (
            SubagentSnapshotStatus::CompletedOutputUnavailable {
                error: "output unavailable".into(),
                tool_calls: 3,
                turns: 2,
                tokens_used: 5,
                worktree_path: None,
            },
            SubagentStatus::OutputUnavailable,
        ),
        (
            SubagentSnapshotStatus::Failed {
                error: "failed".into(),
            },
            SubagentStatus::Failed,
        ),
        (
            SubagentSnapshotStatus::Cancelled {
                reason: Some("cancelled".into()),
            },
            SubagentStatus::Cancelled,
        ),
    ] {
        let (ctx, cancel_calls) = wait_ctx(vec![
            ("done", WaitBehavior::TerminalAfter(Duration::ZERO, status)),
            ("stalled", WaitBehavior::Pending),
        ]);
        let output = WaitSubagentTool
            .run(
                ctx,
                WaitSubagentInput {
                    subagent_ids: vec!["done".into(), "stalled".into()],
                    timeout_ms: Some(1000),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(output.reason).unwrap(),
            serde_json::json!("any_finished")
        );
        assert_eq!(output.waited_ms, 0);
        assert_eq!(output.subagents[0].status, expected_status);
        assert_eq!(
            output.subagents[1].status,
            SubagentStatus::ObservationInterrupted
        );
        assert_eq!(cancel_calls.load(Ordering::SeqCst), 0);
    }
}
#[tokio::test(start_paused = true)]
async fn public_wait_without_a_terminal_uses_one_shared_deadline() {
    let (ctx, _) = wait_ctx(vec![
        (
            "running",
            WaitBehavior::SnapshotThenPending(SubagentSnapshotStatus::RunningProgressUnavailable),
        ),
        ("blocked", WaitBehavior::Pending),
    ]);
    let output = WaitSubagentTool
        .run(
            ctx,
            WaitSubagentInput {
                subagent_ids: vec!["running".into(), "blocked".into()],
                timeout_ms: Some(1000),
            },
        )
        .await
        .unwrap();
    assert_eq!(output.reason, SubagentWaitReason::DeadlineElapsed);
    assert_eq!(output.waited_ms, 1000);
    assert_eq!(
        output.subagents[0].status,
        SubagentStatus::RunningProgressUnavailable
    );
    assert_eq!(
        output.subagents[1].status,
        SubagentStatus::ObservationTimedOut
    );
}
#[tokio::test(start_paused = true)]
async fn public_wait_observation_error_does_not_preempt_later_terminal() {
    let (ctx, _) = wait_ctx(vec![
        ("foreign", WaitBehavior::Unauthorized),
        (
            "done",
            WaitBehavior::TerminalAfter(
                Duration::from_millis(300),
                SubagentSnapshotStatus::Completed {
                    output: "verified answer".into(),
                    tool_calls: 3,
                    turns: 2,
                    tokens_used: 5,
                    worktree_path: None,
                },
            ),
        ),
    ]);
    let output = WaitSubagentTool
        .run(
            ctx,
            WaitSubagentInput {
                subagent_ids: vec!["foreign".into(), "done".into()],
                timeout_ms: Some(1000),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(output.reason).unwrap(),
        serde_json::json!("any_finished")
    );
    assert_eq!(output.waited_ms, 300);
    assert_eq!(output.subagents[0].status, SubagentStatus::Unauthorized);
    assert_eq!(output.subagents[1].status, SubagentStatus::Completed);
    assert_eq!(
        crate::reminders::task_completion::consumed_completion_ids(
            &crate::types::output::ToolOutput::SubagentWait(output)
        ),
        ["done"]
    );
}
#[tokio::test]
async fn public_wait_rejects_empty_ids_and_nonpositive_budget() {
    for input in [
        WaitSubagentInput {
            subagent_ids: vec![],
            timeout_ms: None,
        },
        WaitSubagentInput {
            subagent_ids: vec!["child".into()],
            timeout_ms: Some(0),
        },
    ] {
        assert!(
            WaitSubagentTool
                .run(wait_ctx(vec![("child", WaitBehavior::Pending)]).0, input)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn public_result_can_be_fetched_repeatedly_from_finished_child() {
    let ctx = result_ctx(SubagentSnapshotStatus::Completed {
        output: "verified answer".into(),
        tool_calls: 3,
        turns: 2,
        tokens_used: 5,
        worktree_path: None,
    });
    for _ in 0..2 {
        let result = GetSubagentResultTool
            .run(
                ctx.clone(),
                GetSubagentResultInput {
                    subagent_id: "child".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.subagent_id, "child");
        assert_eq!(result.status, SubagentStatus::Completed);
        assert_eq!(result.output.as_deref(), Some("verified answer"));
        assert_eq!(
            crate::reminders::task_completion::consumed_completion_ids(
                &crate::types::output::ToolOutput::SubagentResult(result)
            ),
            ["child"]
        );
    }
}

#[tokio::test]
async fn public_result_preserves_running_and_unauthorized_observations() {
    let running = GetSubagentResultTool
        .run(
            result_ctx(SubagentSnapshotStatus::RunningProgressUnavailable),
            GetSubagentResultInput {
                subagent_id: "child".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(running.status, SubagentStatus::RunningProgressUnavailable);
    assert_eq!(running.output, None);
    assert!(
        crate::reminders::task_completion::consumed_completion_ids(
            &crate::types::output::ToolOutput::SubagentResult(running)
        )
        .is_empty()
    );

    let unavailable = GetSubagentResultTool
        .run(
            result_ctx(SubagentSnapshotStatus::CompletedOutputUnavailable {
                error: "answer unavailable".into(),
                tool_calls: 3,
                turns: 2,
                tokens_used: 5,
                worktree_path: None,
            }),
            GetSubagentResultInput {
                subagent_id: "child".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(unavailable.status, SubagentStatus::OutputUnavailable);
    assert!(unavailable.output.is_none());
    assert!(
        crate::reminders::task_completion::consumed_completion_ids(
            &crate::types::output::ToolOutput::SubagentResult(unavailable)
        )
        .is_empty()
    );

    let unauthorized = GetSubagentResultTool
        .run(
            result_ctx(SubagentSnapshotStatus::RunningProgressUnavailable),
            GetSubagentResultInput {
                subagent_id: "foreign".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status, SubagentStatus::Unauthorized);
    assert_eq!(unauthorized.output, None);
}

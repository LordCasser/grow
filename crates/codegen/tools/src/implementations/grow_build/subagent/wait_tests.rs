use super::wait::WaitSubagentTool;
use crate::implementations::grow_build::task::backend::{SubagentBackend, SubagentBackendResource};
use crate::implementations::grow_build::task::types::*;
use crate::types::{resources::Resources, tool_metadata::test_ctx};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tool_runtime::Tool;
use tool_types::{SubagentStatus, SubagentWaitReason, WaitSubagentInput};

struct SegmentedBackend(AtomicUsize);
#[async_trait::async_trait]
impl SubagentBackend for SegmentedBackend {
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
        if id == "stalled" {
            return std::future::pending().await;
        }
        if id == "foreign" {
            return Err(SubagentObservationError::Unauthorized);
        }
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let status = match n {
            0 => SubagentSnapshotStatus::Suspended {
                operation_id: "suspend".into(),
                checkpoint_seq: 1,
            },
            1 => SubagentSnapshotStatus::FinalizationPending {
                outcome: "completed".into(),
                error: None,
            },
            _ => SubagentSnapshotStatus::Completed {
                output: "verified answer".into(),
                tool_calls: 3,
                turns: 2,
                tokens_used: 5,
                worktree_path: None,
            },
        };
        Ok(Some(SubagentSnapshot {
            subagent_id: id.into(),
            description: "child".into(),
            subagent_type: "explore".into(),
            status,
            started_at_epoch_ms: 1,
            duration_ms: 900,
        }))
    }
    async fn cancel(&self, _: &str) -> Result<SubagentCancelOutcome, SubagentObservationError> {
        unreachable!()
    }
    async fn validate_type(&self, _: &str, _: &str) -> SubagentValidateTypeOutcome {
        unreachable!()
    }
}
fn ctx() -> tool_runtime::ToolCallContext {
    let mut r = Resources::new();
    r.insert(SubagentBackendResource(Arc::new(SegmentedBackend(
        AtomicUsize::new(0),
    ))));
    test_ctx(r.into_shared())
}
#[tokio::test(start_paused = true)]
async fn public_wait_keeps_waiting_through_suspension_and_finalization() {
    let output = WaitSubagentTool
        .run(
            ctx(),
            WaitSubagentInput {
                subagent_ids: vec![" child ".into(), "child".into()],
                timeout_ms: Some(1000),
            },
        )
        .await
        .unwrap();
    assert_eq!(output.reason, SubagentWaitReason::AllFinished);
    assert_eq!(output.waited_ms, 300);
    assert_eq!(output.subagents.len(), 1);
    assert_eq!(output.subagents[0].subagent_id, "child");
    assert_eq!(
        output.subagents[0].output.as_deref(),
        Some("verified answer")
    );
    assert_eq!(
        crate::reminders::task_completion::consumed_completion_ids(
            &crate::types::output::ToolOutput::SubagentWait(output)
        ),
        ["child"]
    );
}
#[tokio::test(start_paused = true)]
async fn public_wait_has_one_deadline_and_preserves_partial_terminal_and_failure() {
    let output = WaitSubagentTool
        .run(
            ctx(),
            WaitSubagentInput {
                subagent_ids: vec!["done".into(), "stalled".into(), "foreign".into()],
                timeout_ms: Some(1000),
            },
        )
        .await
        .unwrap();
    assert_eq!(output.reason, SubagentWaitReason::DeadlineElapsed);
    assert_eq!(output.waited_ms, 1000);
    assert_eq!(
        output
            .subagents
            .iter()
            .map(|r| r.status)
            .collect::<Vec<_>>(),
        [
            SubagentStatus::Completed,
            SubagentStatus::ObservationTimedOut,
            SubagentStatus::Unauthorized
        ]
    );
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
        assert!(WaitSubagentTool.run(ctx(), input).await.is_err());
    }
}

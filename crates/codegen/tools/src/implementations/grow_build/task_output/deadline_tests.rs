use super::test_helpers::make_snapshot;
use super::*;
use crate::computer::types::*;
use crate::implementations::grow_build::task::backend::{ChannelBackend, SubagentBackendResource};
use crate::implementations::grow_build::task::types::SubagentEvent;
use crate::types::resources::{Resources, Terminal};
use crate::types::template_renderer::TemplateRenderer;
use crate::types::tool::ToolKind;
use crate::types::tool_metadata::test_ctx;
use std::sync::Arc;
use std::time::Duration;

struct SelectiveTerminal;

#[async_trait::async_trait]
impl TerminalBackend for SelectiveTerminal {
    async fn run(&self, _: TerminalRunRequest) -> Result<TerminalRunResult, ComputerError> {
        unreachable!()
    }
    async fn run_background(
        &self,
        _: TerminalRunRequest,
    ) -> Result<BackgroundHandle, ComputerError> {
        unreachable!()
    }
    async fn kill_task(&self, _: &str) -> KillOutcome {
        unreachable!()
    }
    async fn get_task(&self, id: &str) -> Option<TaskSnapshot> {
        match id {
            "done" => Some(make_snapshot("done", true, Some(0))),
            "slow" => std::future::pending().await,
            _ => None,
        }
    }
    async fn wait_for_completion(&self, id: &str, _: Option<Duration>) -> Option<TaskSnapshot> {
        self.get_task(id).await
    }
    async fn list_tasks(&self) -> Vec<TaskSnapshot> {
        vec![make_snapshot("done", true, Some(0))]
    }
}

fn resources() -> Resources {
    let mut resources = Resources::new();
    resources.insert(Terminal(Arc::new(SelectiveTerminal)));
    resources.insert(TemplateRenderer::new(
        std::collections::HashMap::from([(ToolKind::Read, "read_file".into())]),
        Default::default(),
    ));
    resources
}

#[tokio::test(start_paused = true)]
async fn poll_deadline_includes_terminal_mailbox() {
    let start = tokio::time::Instant::now();
    let result = tool_runtime::Tool::run(
        &GetCommandOutputTool,
        test_ctx(resources().into_shared()),
        TaskOutputToolInput {
            task_ids: vec!["slow".into()],
            timeout_ms: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(start.elapsed(), Duration::from_secs(5));
    let TaskOutputOutput::Result(result) = result else {
        panic!("expected one result")
    };
    assert_eq!(result.status, "observation_timed_out");
}

#[tokio::test(start_paused = true)]
async fn multi_wait_uses_one_shared_deadline_and_keeps_partial_results() {
    let start = tokio::time::Instant::now();
    let result = tool_runtime::Tool::run(
        &GetCommandOutputTool,
        test_ctx(resources().into_shared()),
        TaskOutputToolInput {
            task_ids: vec!["done".into(), "slow".into()],
            timeout_ms: Some(1_000),
        },
    )
    .await
    .unwrap();
    assert_eq!(start.elapsed(), Duration::from_secs(1));
    let TaskOutputOutput::MultiResult(result) = result else {
        panic!("expected multi result")
    };
    assert_eq!(result.mode, "wait_all");
    assert_eq!(result.summary, "1/2 tasks completed (wait_all)");
    assert_eq!(result.results[0].status, "completed");
    assert_eq!(result.results[1].status, "observation_timed_out");
}

#[tokio::test(start_paused = true)]
async fn terminal_only_output_never_falls_back_to_subagent_observation() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let backend = SubagentBackendResource(Arc::new(ChannelBackend::new(tx)));
    let mut resource_set = resources();
    resource_set.insert(backend);
    let result = tool_runtime::Tool::run(
        &GetCommandOutputTool,
        test_ctx(resource_set.into_shared()),
        TaskOutputToolInput {
            task_ids: vec!["child-only-id".into()],
            timeout_ms: None,
        },
    )
    .await
    .unwrap();
    let TaskOutputOutput::TaskNotFound(message) = result else {
        panic!("expected terminal-only not-found result")
    };
    assert!(message.contains("Known task IDs"));
    assert!(!message.to_lowercase().contains("subagent"));
    assert!(
        rx.try_recv().is_err(),
        "terminal lookup must not query subagent backend"
    );
}

#[tokio::test]
async fn missing_terminal_is_an_error_even_when_subagent_backend_exists() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SubagentEvent>();
    let mut resource_set = Resources::new();
    resource_set.insert(SubagentBackendResource(Arc::new(ChannelBackend::new(tx))));
    resource_set.insert(TemplateRenderer::new(
        Default::default(),
        Default::default(),
    ));
    let result = tool_runtime::Tool::run(
        &GetCommandOutputTool,
        test_ctx(resource_set.into_shared()),
        TaskOutputToolInput {
            task_ids: vec!["child-only-id".into()],
            timeout_ms: None,
        },
    )
    .await;
    assert!(result.is_err());
    assert!(
        rx.try_recv().is_err(),
        "missing Terminal must not fall back to subagent query"
    );
}

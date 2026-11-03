//! Terminal command output observation shared by the grow-build tool entry.

pub mod terminal_command;
pub use terminal_command::GetCommandOutputTool;

use std::time::Duration;

use crate::DEFAULT_TOOL_OUTPUT_BYTES;
use crate::implementations::BashTool;
use crate::implementations::grow_build_concise::BashConciseTool;
use crate::implementations::task_output::tool::snapshot_to_result;
use crate::types::requirements::{Expr, ToolParamsRequirement, ToolRequirement};
use crate::types::resources::{SharedResources, Terminal, TruncationCfg};
use crate::types::template_renderer::TemplateRenderer;
#[cfg(test)]
use tool_types::TaskOutputToolInput;
use tool_types::{MultiTaskOutputResult, TaskOutputOutput, TaskOutputResult};

pub(crate) const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn max_wait_block() -> Duration {
    Duration::from_millis(tool_types::max_wait_block_ms())
}

pub(crate) fn capped_wait_timeout(timeout_ms: Option<u64>, cap: Duration) -> Duration {
    timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_WAIT_TIMEOUT)
        .min(cap)
}

fn requested_wait_timeout(timeout_ms: Option<u64>) -> Duration {
    timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_WAIT_TIMEOUT)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaitHint {
    NotRequested,
    Elapsed {
        requested: Duration,
        waited: Duration,
    },
}

fn still_running_wait_hint(hint: WaitHint) -> String {
    let lead = match hint {
        WaitHint::Elapsed { requested, waited } => {
            let waited_label = format_waited_duration(waited);
            if requested > waited {
                format!(
                    "Waited {waited_label} of the {} requested; the command is still running.",
                    format_waited_duration(requested)
                )
            } else {
                format!("Waited the requested {waited_label}; the command is still running.")
            }
        }
        WaitHint::NotRequested => "Use timeout_ms to wait for completion.".to_string(),
    };
    format!("{lead} You will be notified automatically when the command completes.")
}

fn format_waited_duration(duration: Duration) -> String {
    let ms = duration.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{}s", ms / 1000)
    }
}

fn apply_running_wait_hint(mut result: TaskOutputResult, hint: WaitHint) -> TaskOutputResult {
    if result.status == "running" {
        result.output = format!("{}\n\n{}", result.output, still_running_wait_hint(hint));
    }
    result
}

pub(crate) fn is_terminal_status(status: &str) -> bool {
    TaskOutputResult::is_terminal_status(status)
}

pub(crate) use tool_types::MAX_MULTI_WAIT_IDS;

pub(crate) fn background_bash_requires_exprs() -> Vec<Expr<ToolRequirement>> {
    use crate::types::tool_metadata::ToolMetadata;
    [
        (
            ToolMetadata::tool_namespace(&BashTool),
            tool_runtime::Tool::id(&BashTool).as_str().to_owned(),
        ),
        (
            ToolMetadata::tool_namespace(&BashConciseTool),
            tool_runtime::Tool::id(&BashConciseTool).as_str().to_owned(),
        ),
    ]
    .into_iter()
    .map(|(namespace, id)| {
        Expr::Value(ToolRequirement::Tool {
            namespace: namespace.to_string(),
            id,
            if_params: Some(Expr::Value(ToolParamsRequirement {
                key: "enabled_background".to_owned(),
                value: Expr::Value(serde_json::Value::Bool(true)),
            })),
        })
    })
    .collect()
}

pub(crate) fn not_found_result(task_id: &str) -> TaskOutputResult {
    TaskOutputResult {
        task_id: task_id.to_owned(),
        status: "not_found".to_owned(),
        output: format!("Task {task_id} not found."),
        ..Default::default()
    }
}

/// Observe one or more terminal commands under one shared deadline.
pub(crate) async fn run_terminal_tasks(
    task_ids: &[String],
    timeout_ms: Option<u64>,
    resources: SharedResources,
) -> Result<TaskOutputOutput, tool_runtime::ToolError> {
    if task_ids.is_empty() {
        return Err(tool_runtime::ToolError::invalid_arguments(
            "Provide a non-empty task_ids list.".to_string(),
        ));
    }
    if task_ids.len() > MAX_MULTI_WAIT_IDS {
        return Err(tool_runtime::ToolError::invalid_arguments(format!(
            "task_ids exceeds maximum of {MAX_MULTI_WAIT_IDS} entries."
        )));
    }

    let started = tokio::time::Instant::now();
    let waits = tool_types::task_output_waits(timeout_ms);
    let budget = if waits {
        capped_wait_timeout(timeout_ms, max_wait_block())
    } else {
        Duration::from_secs(5)
    };
    let deadline = started + budget;
    let (terminal, read_file_name, max_output_bytes) = {
        let resources = match tokio::time::timeout_at(deadline, resources.lock()).await {
            Ok(resources) => resources,
            Err(_) => {
                let results = task_ids.iter().map(|id| timed_out_result(id)).collect();
                if task_ids.len() == 1 {
                    let mut results: Vec<TaskOutputResult> = results;
                    return Ok(TaskOutputOutput::Result(
                        results.pop().expect("one task ID produces one result"),
                    ));
                }
                return Ok(format_results(results, waits));
            }
        };
        let terminal = resources.require::<Terminal>()?.0.clone();
        let renderer = resources.require::<TemplateRenderer>()?;
        let read_file_name = renderer
            .render("${{ tools.by_kind.read }}")
            .map_err(|error| tool_runtime::ToolError::invalid_arguments(error.to_string()))?;
        let max_output_bytes = resources
            .get::<TruncationCfg>()
            .map(|config| {
                config
                    .0
                    .max_output_bytes_for("get_command_output", DEFAULT_TOOL_OUTPUT_BYTES)
            })
            .unwrap_or(DEFAULT_TOOL_OUTPUT_BYTES);
        (terminal, read_file_name, max_output_bytes)
    };

    let results = futures_util::future::join_all(task_ids.iter().map(|id| {
        observe_terminal_task(
            id,
            &terminal,
            &read_file_name,
            max_output_bytes,
            waits,
            timeout_ms,
            started,
            deadline,
            task_ids.len() == 1,
        )
    }))
    .await;
    if task_ids.len() == 1 {
        let mut results = results;
        let result = results.pop().expect("one task ID produces one result");
        if result.status == "not_found" {
            return Ok(TaskOutputOutput::TaskNotFound(result.output));
        }
        return Ok(TaskOutputOutput::Result(result));
    }
    Ok(format_results(results, waits))
}

fn format_results(results: Vec<TaskOutputResult>, waits: bool) -> TaskOutputOutput {
    let completed = results.iter().filter(|result| result.is_terminal()).count();
    let mode = if waits { "wait_all" } else { "poll" };
    let summary = format!("{completed}/{} tasks completed ({mode})", results.len());
    TaskOutputOutput::MultiResult(MultiTaskOutputResult {
        mode: mode.to_owned(),
        results,
        summary,
    })
}

async fn observe_terminal_task(
    id: &str,
    terminal: &std::sync::Arc<dyn crate::computer::types::TerminalBackend>,
    read_file_name: &str,
    max_output_bytes: usize,
    waits: bool,
    timeout_ms: Option<u64>,
    started: tokio::time::Instant,
    deadline: tokio::time::Instant,
    list_known: bool,
) -> TaskOutputResult {
    let hint = || {
        if waits {
            WaitHint::Elapsed {
                requested: requested_wait_timeout(timeout_ms),
                waited: started.elapsed(),
            }
        } else {
            WaitHint::NotRequested
        }
    };
    let terminal_query = async {
        if waits {
            terminal
                .wait_for_completion(
                    id,
                    Some(deadline.saturating_duration_since(tokio::time::Instant::now())),
                )
                .await
        } else {
            terminal.get_task(id).await
        }
    };
    match tokio::time::timeout_at(deadline, terminal_query).await {
        Ok(Some(snapshot)) => {
            return apply_running_wait_hint(
                snapshot_to_result(snapshot, read_file_name, max_output_bytes),
                hint(),
            );
        }
        Ok(None) => {}
        Err(_) => return timed_out_result(id),
    }

    let mut result = not_found_result(id);
    if list_known {
        match tokio::time::timeout_at(deadline, terminal.list_tasks()).await {
            Ok(known) if known.is_empty() => {
                result.output =
                    format!("Task {id} not found. No background tasks exist in this session.");
            }
            Ok(known) => {
                result.output = format!(
                    "Task {id} not found. Known task IDs: [{}]",
                    known
                        .iter()
                        .map(|task| task.task_id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            Err(_) => return timed_out_result(id),
        }
    }
    result
}

fn timed_out_result(id: &str) -> TaskOutputResult {
    TaskOutputResult {
        task_id: id.to_owned(),
        status: "observation_timed_out".to_owned(),
        output: "Task observation timed out; its execution state is unknown.".to_owned(),
        ..Default::default()
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use crate::types::tool::ToolKind;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use crate::computer::types::{
        BackgroundHandle, KillOutcome, TaskSnapshot, TerminalBackend, TerminalRunRequest,
        TerminalRunResult,
    };
    use crate::types::resources::{Resources, Terminal};
    use crate::types::template_renderer::TemplateRenderer;

    pub(crate) struct MockTerminal {
        snapshot: Option<TaskSnapshot>,
    }

    impl MockTerminal {
        pub(crate) fn with_snapshot(snapshot: TaskSnapshot) -> Self {
            Self {
                snapshot: Some(snapshot),
            }
        }

        pub(crate) fn empty() -> Self {
            Self { snapshot: None }
        }
    }

    #[async_trait::async_trait]
    impl TerminalBackend for MockTerminal {
        async fn run(
            &self,
            _: TerminalRunRequest,
        ) -> Result<TerminalRunResult, crate::computer::types::ComputerError> {
            unimplemented!()
        }
        async fn run_background(
            &self,
            _: TerminalRunRequest,
        ) -> Result<BackgroundHandle, crate::computer::types::ComputerError> {
            unimplemented!()
        }
        async fn kill_task(&self, _: &str) -> KillOutcome {
            unimplemented!()
        }
        async fn get_task(&self, id: &str) -> Option<TaskSnapshot> {
            self.snapshot
                .clone()
                .filter(|snapshot| snapshot.task_id == id)
        }
        async fn wait_for_completion(&self, id: &str, _: Option<Duration>) -> Option<TaskSnapshot> {
            self.get_task(id).await
        }
        async fn list_tasks(&self) -> Vec<TaskSnapshot> {
            self.snapshot.iter().cloned().collect()
        }
    }

    pub(crate) fn make_snapshot(
        task_id: &str,
        completed: bool,
        exit_code: Option<i32>,
    ) -> TaskSnapshot {
        TaskSnapshot {
            task_id: task_id.to_owned(),
            command: "echo hello".to_owned(),
            display_command: None,
            cwd: "/tmp".to_owned(),
            start_time: SystemTime::now(),
            end_time: completed.then(SystemTime::now),
            output: "test output".to_owned(),
            output_file: PathBuf::from(format!("/tmp/{task_id}.log")),
            truncated: false,
            exit_code,
            signal: None,
            completed,
            kind: Default::default(),
            block_waited: false,
            explicitly_killed: false,
            owner_session_id: None,
            goal_id: None,
            goal_definition_revision: None,
            description: None,
            is_backgrounded: false,
        }
    }

    pub(crate) fn resources_with_terminal(snapshot: Option<TaskSnapshot>) -> Resources {
        let mut resources = Resources::new();
        let backend: Arc<dyn TerminalBackend> = match snapshot {
            Some(snapshot) => Arc::new(MockTerminal::with_snapshot(snapshot)),
            None => Arc::new(MockTerminal::empty()),
        };
        resources.insert(Terminal(backend));
        resources.insert(TemplateRenderer::new(
            std::collections::HashMap::from([(ToolKind::Read, "read_file".to_owned())]),
            std::collections::HashMap::new(),
        ));
        resources
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::*;
    use super::*;
    use crate::types::tool_metadata::test_ctx;

    #[test]
    fn capped_wait_timeout_clamps_and_defaults() {
        let cap = Duration::from_millis(tool_types::MAX_WAIT_BLOCK_MS_DEFAULT);
        assert_eq!(capped_wait_timeout(None, cap), DEFAULT_WAIT_TIMEOUT);
        assert_eq!(
            capped_wait_timeout(Some(5_000), cap),
            Duration::from_secs(5)
        );
        assert_eq!(capped_wait_timeout(Some(600_000), cap), cap);
    }

    #[tokio::test]
    async fn terminal_only_tool_returns_task_snapshot() {
        let result = tool_runtime::Tool::run(
            &GetCommandOutputTool,
            test_ctx(
                resources_with_terminal(Some(make_snapshot("cmd-1", false, None))).into_shared(),
            ),
            TaskOutputToolInput {
                task_ids: vec!["cmd-1".into()],
                timeout_ms: None,
            },
        )
        .await
        .unwrap();
        let TaskOutputOutput::Result(result) = result else {
            panic!("expected result")
        };
        assert_eq!(result.status, "running");
        assert_eq!(result.task_id, "cmd-1");
    }

    #[tokio::test]
    async fn terminal_only_tool_does_not_fallback_without_terminal() {
        let resources = crate::types::resources::Resources::new();
        let result = tool_runtime::Tool::run(
            &GetCommandOutputTool,
            test_ctx(resources.into_shared()),
            TaskOutputToolInput {
                task_ids: vec!["child-id".into()],
                timeout_ms: None,
            },
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn terminal_only_tool_rejects_empty_ids() {
        let result = tool_runtime::Tool::run(
            &GetCommandOutputTool,
            test_ctx(resources_with_terminal(None).into_shared()),
            TaskOutputToolInput {
                task_ids: vec![],
                timeout_ms: None,
            },
        )
        .await;
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod deadline_tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::super::acp_session::SessionActor;

impl SessionActor {
    pub(super) async fn finish_restored_deep_research_if_terminal(&self) {
        use crate::session::workflow::tracker::WorkflowRunStatus;
        let Some(run_id) = self
            .behavior
            .lock()
            .deep_research_run_id()
            .map(str::to_owned)
        else {
            return;
        };
        let state = self.workflow_tracker().await.lock().get(&run_id);
        let Some(state) = state.filter(|state| state.status.is_completion_reportable()) else {
            return;
        };
        let outcome = match state.status {
            WorkflowRunStatus::Complete => {
                let summary = state.result_summary.unwrap_or_else(|| {
                    "The process ended after completion, but no persisted report body was available."
                        .to_string()
                });
                let report = summary
                    .split_once("\n\n_Full report: ")
                    .map_or(summary.as_str(), |(report, _)| report)
                    .to_string();
                let status = if report.contains("**Status: Partial**") {
                    "partial"
                } else {
                    "verified"
                };
                workflow::WorkflowOutcome::Completed {
                    result: serde_json::json!({
                        "report": report,
                        "status": status,
                        "path": "scratch/report.md",
                    }),
                }
            }
            WorkflowRunStatus::Cancelled => workflow::WorkflowOutcome::Cancelled,
            WorkflowRunStatus::BudgetLimited => workflow::WorkflowOutcome::BudgetExceeded {
                message: state
                    .pause_message
                    .unwrap_or_else(|| "The research agent budget was exhausted.".to_string()),
            },
            WorkflowRunStatus::Interrupted | WorkflowRunStatus::Failed => {
                workflow::WorkflowOutcome::Failed {
                    error: state.pause_message.unwrap_or_else(|| {
                        "The process restarted before the research run reached a durable terminal report."
                            .to_string()
                    }),
                }
            }
            _ => return,
        };
        self.finish_deep_research_run(&run_id, outcome).await;
    }

    pub(crate) async fn launch_deep_research(
        self: &Arc<Self>,
        query: String,
    ) -> Result<String, String> {
        if query.trim().is_empty() {
            return Err("Deep Research is waiting for a non-empty research query.".to_string());
        }
        if self.behavior.lock().deep_research_run_id().is_some() {
            return Err(
                "Deep Research is already running. Manage the current run or switch behavior first."
                    .to_string(),
            );
        }
        let resolved = crate::session::workflow::registry::resolve_deep_research()
            .map_err(|error| format!("Deep Research workflow unavailable: {error}"))?;
        let spec = crate::session::workflow::manager::LaunchSpec {
            objective: query.clone(),
            args: serde_json::json!({ "query": query }),
            agent_budget: None,
            max_concurrency: None,
            resume_run_id: None,
        };
        let (run_id, outcome_rx) = self
            .workflow_manager
            .lock()
            .await
            .launch(resolved, spec)
            .map_err(|error| format!("Could not start Deep Research: {error}"))?;
        if !self
            .behavior
            .lock()
            .attach_deep_research_run(run_id.clone())
        {
            self.workflow_manager.lock().await.cancel(&run_id);
            return Err("Deep Research behavior changed before the run could start.".to_string());
        }
        let behavior = self.behavior.lock().snapshot();
        let goal = self.goal_tracker.lock().snapshot().cloned();
        if let Err(error) = self.persist_control_snapshot_durably(behavior, goal).await {
            self.behavior.lock().clear_deep_research_run();
            self.workflow_manager.lock().await.cancel(&run_id);
            return Err(format!(
                "Deep Research was cancelled because its ownership could not be persisted: {error}"
            ));
        }
        // WorkflowManager delivers the terminal outcome through the session
        // mailbox. Dropping this secondary observer keeps all Behavior
        // transitions serialized on SessionActor.
        drop(outcome_rx);
        self.send_available_commands_update().await;
        Ok(run_id)
    }

    fn deep_research_report_artifact_path(&self, run_id: &str) -> Option<PathBuf> {
        let path = crate::session::persistence::session_dir(&self.session_info)
            .join("workflows")
            .join(run_id)
            .join("scratch")
            .join("report.md");
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return None;
        }
        std::fs::canonicalize(path).ok()
    }

    pub(super) async fn finish_deep_research_run(
        &self,
        run_id: &str,
        outcome: workflow::WorkflowOutcome,
    ) {
        if matches!(outcome, workflow::WorkflowOutcome::Paused { .. }) {
            return;
        }
        let owned = self.behavior.lock().deep_research_run_id() == Some(run_id);
        if !owned {
            return;
        }
        let query = self
            .workflow_tracker()
            .await
            .lock()
            .get(run_id)
            .map(|run| run.objective.clone())
            .unwrap_or_default();
        let artifact_path = self.deep_research_report_artifact_path(run_id);
        let report = deep_research_terminal_report(&query, &outcome, artifact_path.as_deref());
        let goal = self.goal_tracker.lock().snapshot().cloned();
        if self
            .persist_control_snapshot_durably(
                crate::session::behavior::BehaviorSnapshot::normal(),
                goal,
            )
            .await
            .is_err()
        {
            self.send_host_turn_slash_command_output(&format!(
                "{report}\n\nThe terminal report is available, but the Behavior transition could not be persisted. Select another Behavior to retry."
            ))
            .await;
            return;
        }
        self.send_host_turn_slash_command_output(&report).await;
        {
            let mut behavior = self.behavior.lock();
            behavior.clear_deep_research_run();
            behavior.select_behavior(tool_types::BehaviorId::Normal);
        }
        self.enqueue_current_mode_update(agent_client_protocol::SessionModeId::new(
            tools::types::BehaviorId::Normal.as_id(),
        ));
        self.send_available_commands_update().await;
    }

    pub(crate) async fn cancel_deep_research_with_report(&self, run_id: &str) {
        let query = self
            .workflow_tracker()
            .await
            .lock()
            .get(run_id)
            .map(|run| run.objective.clone())
            .unwrap_or_default();
        self.workflow_manager.lock().await.cancel(run_id);
        self.behavior.lock().clear_deep_research_run();
        let report =
            deep_research_terminal_report(&query, &workflow::WorkflowOutcome::Cancelled, None);
        self.send_host_turn_slash_command_output(&report).await;
    }
    pub(crate) fn named_workflow_snapshot(
        &self,
    ) -> (
        crate::session::workflow::registry::WorkflowRegistry,
        Vec<crate::session::workflow::registry::WorkflowListing>,
        Vec<tools::implementations::grow_build::workflow::WorkflowDiagnostic>,
    ) {
        let cwd = std::path::Path::new(self.session_info.cwd.as_str());
        let registry = crate::session::workflow::registry::WorkflowRegistry::scan(Some(cwd));
        let session_dir = crate::session::persistence::session_dir(&self.session_info);
        match crate::session::workflow::workspace::WorkflowWorkspace::open(&session_dir, cwd) {
            Ok(workspace) => {
                let catalog = workspace.catalog(cwd);
                let listings = catalog
                    .definitions
                    .into_iter()
                    .map(
                        |definition| crate::session::workflow::registry::WorkflowListing {
                            definition_id: definition.definition_id,
                            name: definition.name,
                            description: definition.description,
                            when_to_use: definition.when_to_use,
                            source: definition.scope.as_str(),
                            scope: definition.scope,
                            path: definition.path,
                            status: definition.status,
                            content_hash: definition.content_hash,
                            focused: definition.focused,
                        },
                    )
                    .collect();
                (registry, listings, catalog.diagnostics)
            }
            Err(error) => {
                let mut diagnostics = registry.diagnostics().to_vec();
                diagnostics.push(
                    tools::implementations::grow_build::workflow::WorkflowDiagnostic {
                        scope: tools::implementations::grow_build::workflow::WorkflowScope::Session,
                        path: Some(session_dir.join("workflow-workspace").display().to_string()),
                        code: "workspace_unavailable".into(),
                        message: error.to_string(),
                    },
                );
                let listings = registry.list();
                (registry, listings, diagnostics)
            }
        }
    }

    pub(crate) async fn launch_named_workflow(self: &Arc<Self>, name: &str, input: &str) -> String {
        // This lock is also the special-Behavior admission gate. Recheck the
        // Behavior after acquiring it: a slash command may have been resolved
        // while a concurrent Behavior switch was still committing.
        let mut manager = self.workflow_manager.lock().await;
        let behavior = self.behavior.lock().behavior();
        if behavior != tool_types::BehaviorId::Workflow {
            return format!(
                "Saved Workflow Definitions can only run in Workflow behavior. Use /workflow [prompt] (current: {}).",
                behavior.display_label(),
            );
        }
        let cwd = std::path::Path::new(self.session_info.cwd.as_str());
        let session_dir = crate::session::persistence::session_dir(&self.session_info);
        let mut workspace =
            match crate::session::workflow::workspace::WorkflowWorkspace::open(&session_dir, cwd) {
                Ok(workspace) => workspace,
                Err(error) => return format!("Workflow workspace unavailable: {error}"),
            };
        let candidates: Vec<_> = workspace
            .catalog(cwd)
            .definitions
            .into_iter()
            .filter(|definition| definition.name == name)
            .collect();
        let definition_id = match candidates.as_slice() {
            [definition] => definition.definition_id.clone(),
            [] => return format!("Workflow '{name}' unavailable."),
            _ => {
                return format!(
                    "More than one Definition is named '{name}'. Open /workflows and choose a scoped Definition."
                );
            }
        };
        if let Err(error) = workspace.focus(cwd, &definition_id) {
            return format!("Could not focus Workflow '{name}': {error}");
        }
        let definition = match workspace.resolve(cwd, &definition_id) {
            Ok(definition) => definition,
            Err(error) => return format!("Workflow '{name}' unavailable: {error}"),
        };
        if let Err(error) = workflow::validate_script_with_agent_budget(
            &definition.resolved.script,
            parse_named_workflow_args(input, &definition.resolved.meta.description)
                .0
                .into(),
            workflow::DEFAULT_AGENT_BUDGET,
        ) {
            return format!("Workflow '{name}' failed preflight and was not started: {error}");
        }
        if let Err(error) =
            workspace.record_validated(cwd, &definition_id, &definition.summary.content_hash)
        {
            return format!("Workflow '{name}' changed during preflight: {error}");
        }
        let resolved = definition.resolved;
        let (args, objective) = parse_named_workflow_args(input, &resolved.meta.description);
        let spec = crate::session::workflow::manager::LaunchSpec {
            objective,
            args,
            agent_budget: None,
            max_concurrency: None,
            resume_run_id: None,
        };
        let launched = manager.launch(resolved, spec);
        match launched {
            Ok((run_id, outcome_rx)) => {
                let (display, objective) = manager
                    .tracker()
                    .lock()
                    .get(&run_id)
                    .map(|r| (r.name.clone(), r.objective.clone()))
                    .unwrap_or_else(|| (name.to_string(), String::new()));
                drop(manager);
                let command_line = if input.trim().is_empty() {
                    format!("/{name}")
                } else {
                    format!("/{name} {}", input.trim())
                };
                self.push_workflow_launch_reminder(
                    &display,
                    &run_id,
                    &objective,
                    &command_line,
                    false,
                );
                tokio::spawn(async move {
                    if let Ok(outcome) = outcome_rx.await {
                        tracing::info!(run_id, ?outcome, "named workflow finished");
                    }
                });
                format!(
                    "Workflow '{display}' started in the background. Watch it in /workflows; \
                     the result lands here when it finishes."
                )
            }
            Err(e) => format!("Could not start workflow '{name}': {e}"),
        }
    }

    pub(crate) async fn manage_workflow_run(self: &Arc<Self>, run_id: &str, op: &str) -> String {
        use crate::session::workflow::tracker::WorkflowRunStatus;

        // The manager lock is the public Workflow admission lock shared with
        // Behavior switching. Hold it from the live Behavior check through
        // control so pause/stop cannot execute after a concurrent switch.
        let mut manager = self.workflow_manager.lock().await;
        let behavior = self.behavior.lock().behavior();
        if behavior != tool_types::BehaviorId::Workflow {
            return format!(
                "Workflow Runs can only be managed in Workflow behavior. Use /workflow (current: {}).",
                behavior.display_label()
            );
        }

        const USAGE: &str = "Usage: /workflow <name> [args] to launch a saved workflow, or \
                             /workflow-run <op> [name] to manage \
                             a run — ops: pause, resume, stop. Publish session drafts through the Workflow workspace with an explicit Project or User scope.";
        if op.is_empty() {
            return USAGE.to_string();
        }

        let matches: Vec<(String, WorkflowRunStatus, String)> = {
            let tracker = manager.tracker();
            let tracker = tracker.lock();
            let all: Vec<_> = tracker
                .list()
                .iter()
                .filter(|r| {
                    !r.private && (r.run_id.starts_with(run_id) || r.name.starts_with(run_id))
                })
                .map(|r| (r.run_id.clone(), r.status, r.name.clone()))
                .collect();
            narrow_run_matches(all, run_id, op)
        };
        let (full_id, status, name) = match matches.as_slice() {
            [] if run_id.is_empty() => {
                return "No workflow runs in this session yet.".to_string();
            }
            [] => return format!("No workflow run matches '{run_id}'."),
            [one] => one.clone(),
            many => {
                let rows: Vec<String> = many
                    .iter()
                    .map(|(_, status, name)| format!("  {name} ({})", status.as_str()))
                    .collect();
                return format!(
                    "Several runs could be '{op}' — pick one by name:\n{}\n(/workflow {op} <name>)",
                    rows.join("\n")
                );
            }
        };
        let id_suffix = format!(" {name}");

        match op {
            "pause" => {
                if status != WorkflowRunStatus::Active {
                    return format!("Run '{name}' is not active (status: {}).", status.as_str());
                }
                if !manager.pause(&full_id) {
                    return format!("Run '{name}' is no longer active.");
                }
                format!("Paused {name}. /workflow-run resume{id_suffix} to continue.")
            }
            "stop" => {
                if status.is_terminal() {
                    return format!(
                        "Run '{name}' is already finished (status: {}).",
                        status.as_str()
                    );
                }
                // Private Deep Research runs were filtered out above and are
                // controlled only by the Deep Research Behavior owner.
                if !manager.cancel(&full_id) {
                    return format!("Run '{name}' is already finished.");
                }
                format!("Stopped {name}.")
            }
            "resume" => {
                // Resume is a fresh public-work admission. Serialize it with
                // Behavior selection and re-read both Behavior and run state
                // after acquiring the shared gate; the command may have been
                // parsed before a concurrent switch or terminal event.
                let behavior = self.behavior.lock().behavior();
                if behavior != tool_types::BehaviorId::Workflow {
                    return format!(
                        "Workflow can only be resumed in Workflow behavior (current: {}).",
                        behavior.display_label()
                    );
                }
                let tracker = manager.tracker();
                let (status, objective, agent_budget) = {
                    let tracker = tracker.lock();
                    let Some(run) = tracker.get(&full_id) else {
                        return format!("Workflow run '{name}' disappeared before resume.");
                    };
                    (run.status, run.objective.clone(), run.agent_budget)
                };
                if status == WorkflowRunStatus::Active {
                    return format!("Run '{name}' is already running.");
                }
                if !status.is_resumable() {
                    return format!(
                        "Run '{name}' cannot be resumed (status: {}). Start a new run instead.",
                        status.as_str()
                    );
                }
                if status == WorkflowRunStatus::BudgetLimited {
                    let (used, limit) = {
                        let tracker = manager.tracker();
                        let tracker = tracker.lock();
                        let run = tracker.get(&full_id);
                        (
                            run.as_ref().map_or(0, |r| r.agents_used),
                            run.as_ref().and_then(|r| r.agent_budget),
                        )
                    };
                    let limit = limit.map_or_else(String::new, |l| format!("/{l}"));
                    if used >= workflow::MAX_AGENT_BUDGET {
                        return format!(
                            "Run '{name}' exhausted the maximum agent budget ({used}{limit} agents) \
                             and cannot be resumed. Start a new run instead."
                        );
                    }
                    let suggested = used.saturating_add(64).min(workflow::MAX_AGENT_BUDGET);
                    return format!(
                        "Run '{name}' exhausted its agent budget ({used}{limit} agents). \
                         Resuming keeps all finished work but needs a higher absolute cap — \
                         ask the agent to resume it with an agent budget above {used}, e.g. \
                        \"resume {name} with an agent budget of {suggested}\"."
                    );
                }
                let (script, args) = (
                    manager.script_copy_for(&full_id),
                    manager.args_copy_for(&full_id),
                );
                let Some(script) = script else {
                    return format!("No persisted script for '{name}'; cannot resume.");
                };
                let resolved = match crate::session::workflow::registry::resolve_inline(script) {
                    Ok(r) => r,
                    Err(e) => return format!("Persisted script invalid: {e}"),
                };
                let objective_echo = objective.clone();
                let spec = crate::session::workflow::manager::LaunchSpec {
                    objective,
                    args,
                    agent_budget,
                    max_concurrency: None,
                    resume_run_id: Some(full_id.clone()),
                };
                match manager.launch(resolved, spec) {
                    Ok((rid, outcome_rx)) => {
                        tokio::spawn(async move {
                            if let Ok(outcome) = outcome_rx.await {
                                tracing::info!(run_id = rid, ?outcome, "resumed workflow finished");
                            }
                        });
                        self.push_workflow_launch_reminder(
                            &name,
                            &full_id,
                            &objective_echo,
                            &format!("/workflow-run resume {name}"),
                            true,
                        );
                        format!("Resumed {name} from its journal.")
                    }
                    Err(e) => format!("Could not resume '{name}': {e}"),
                }
            }
            other => format!("Unknown op '{other}'. {USAGE}"),
        }
    }

    pub(crate) async fn workflow_workspace_report(&self) -> String {
        let behavior = self.behavior.lock().behavior();
        if behavior != tool_types::BehaviorId::Workflow {
            return format!(
                "The Workflow workspace is available only in Workflow behavior. Use /workflow [prompt] (current: {}).",
                behavior.display_label()
            );
        }
        let cwd = std::path::Path::new(self.session_info.cwd.as_str());
        let session_dir = crate::session::persistence::session_dir(&self.session_info);
        let workspace =
            match crate::session::workflow::workspace::WorkflowWorkspace::open(&session_dir, cwd) {
                Ok(workspace) => workspace,
                Err(error) => return format!("Workflow workspace unavailable: {error}"),
            };
        let catalog = workspace.catalog(cwd);
        let mut lines = vec![format!(
            "Workflow Workspace — {} Definition(s), {} diagnostic(s)",
            catalog.definitions.len(),
            catalog.diagnostics.len()
        )];
        lines.push("Definitions:".into());
        if catalog.definitions.is_empty() {
            lines.push("  (none)".into());
        } else {
            lines.extend(catalog.definitions.into_iter().map(|definition| {
                let focus = if definition.focused { "*" } else { " " };
                format!(
                    " {focus} {} [{}; {}; {}]",
                    definition.name,
                    definition.scope.as_str(),
                    definition.status,
                    definition.definition_id
                )
            }));
        }
        if !catalog.diagnostics.is_empty() {
            lines.push("Diagnostics:".into());
            lines.extend(catalog.diagnostics.into_iter().map(|diagnostic| {
                let path = diagnostic.path.as_deref().unwrap_or("<no path>");
                format!(
                    "  [{}; {}] {} — {}",
                    diagnostic.scope.as_str(),
                    diagnostic.code,
                    path,
                    diagnostic.message
                )
            }));
        }
        lines.push("Runs:".into());
        let tracker = self.workflow_tracker().await;
        let public_runs: Vec<_> = tracker
            .lock()
            .list()
            .iter()
            .filter(|run| !run.private)
            .map(|run| {
                let provenance = run
                    .definition_scope
                    .zip(run.definition_hash.as_deref())
                    .map(|(scope, hash)| {
                        format!("{}@{}", scope.as_str(), hash.get(..8).unwrap_or(hash))
                    })
                    .unwrap_or_else(|| "unknown source".into());
                format!(
                    "   {} [{}; {}; {provenance}]",
                    run.name,
                    run.status.as_str(),
                    run.run_id
                )
            })
            .collect();
        if public_runs.is_empty() {
            lines.push("  (none)".into());
        } else {
            lines.extend(public_runs);
        }
        lines.join("\n")
    }
}

pub(super) fn deep_research_terminal_report(
    query: &str,
    outcome: &workflow::WorkflowOutcome,
    artifact_path: Option<&Path>,
) -> String {
    use workflow::WorkflowOutcome;
    let artifact = artifact_path.map_or_else(
        || "No complete report artifact was produced for this outcome.".to_string(),
        |path| format!("`{}`", path.display()),
    );
    if let WorkflowOutcome::Completed { result } = outcome
        && let Some(report) = result.get("report").and_then(serde_json::Value::as_str)
        && !report.trim().is_empty()
    {
        let status = match result.get("status").and_then(serde_json::Value::as_str) {
            Some("verified") => "success",
            Some("partial") if report.contains("None of the candidate claims survived") => {
                "verification failed"
            }
            Some("partial") => "partial",
            Some(other) => other,
            None => "completed",
        };
        return format!(
            "# Deep Research Report\n\n## Status\n\n{status}\n\n## Query\n\n{query}\n\n{report}\n\n## Full report\n\n{artifact}\n\n## Termination reason\n\nThe research workflow reached a terminal result."
        );
    }
    let (status, reason, findings) = match outcome {
        WorkflowOutcome::Completed { result } => (
            "completed",
            "The research workflow completed without a dedicated report field.",
            serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string()),
        ),
        WorkflowOutcome::BudgetExceeded { message } => (
            "budget exhausted",
            message.as_str(),
            "No additional verified findings were produced.".to_string(),
        ),
        WorkflowOutcome::Cancelled => (
            "cancelled",
            "The research run was cancelled by the user or by a confirmed Behavior switch.",
            "Only findings already delivered before cancellation should be relied on.".to_string(),
        ),
        WorkflowOutcome::Failed { error } => {
            let status = if error.contains("restart") {
                "interrupted"
            } else {
                "runtime failure"
            };
            (
                status,
                error.as_str(),
                "The runtime failed before it could produce a complete verified report."
                    .to_string(),
            )
        }
        WorkflowOutcome::Paused { message, .. } => (
            "paused",
            message.as_str(),
            "The run remains resumable; this is not a terminal research report.".to_string(),
        ),
    };
    format!(
        "# Deep Research Report\n\n## Status\n\n{status}\n\n## Query\n\n{query}\n\n## Investigation and verification\n\n{findings}\n\n## Limitations\n\nThis is a terminal fallback report generated from the workflow outcome; no additional independently verified evidence was available at termination.\n\n## Full report\n\n{artifact}\n\n## Termination reason\n\n{reason}"
    )
}

pub(crate) fn parse_named_workflow_args(
    input: &str,
    description: &str,
) -> (serde_json::Value, String) {
    let input = input.trim();
    if input.is_empty() {
        return (serde_json::Value::Null, description.to_string());
    }
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(input) {
        let objective = map
            .get("objective")
            .or_else(|| map.get("query"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| input.to_string());
        return (serde_json::Value::Object(map), objective);
    }
    (
        serde_json::json!({ "query": input, "objective": input }),
        input.to_string(),
    )
}

type RunMatch = (
    String,
    crate::session::workflow::tracker::WorkflowRunStatus,
    String,
);

fn narrow_run_matches(mut all: Vec<RunMatch>, selector: &str, op: &str) -> Vec<RunMatch> {
    use crate::session::workflow::tracker::WorkflowRunStatus;
    if !selector.is_empty() {
        let exact: Vec<_> = all
            .iter()
            .filter(|(id, _, name)| id.as_str() == selector || name.as_str() == selector)
            .cloned()
            .collect();
        if !exact.is_empty() {
            all = exact;
        }
    }
    if all.len() > 1 {
        let applicable: Vec<_> = all
            .iter()
            .filter(|(_, status, ..)| match op {
                "pause" => *status == WorkflowRunStatus::Active,
                "resume" => status.is_resumable(),
                "stop" => !status.is_terminal(),
                _ => true,
            })
            .cloned()
            .collect();
        if applicable.len() == 1 {
            return applicable;
        }
    }
    all
}

#[cfg(test)]
mod run_match_tests {
    use super::{deep_research_terminal_report, narrow_run_matches};
    use crate::session::workflow::tracker::WorkflowRunStatus;

    fn run(id: &str, name: &str, status: WorkflowRunStatus) -> super::RunMatch {
        (id.to_string(), status, name.to_string())
    }

    #[test]
    fn exact_name_beats_prefix_of_uniquified_sibling() {
        let all = vec![
            run("wf_1", "deep-research", WorkflowRunStatus::Active),
            run("wf_2", "deep-research-2", WorkflowRunStatus::Active),
        ];
        let picked = narrow_run_matches(all, "deep-research", "stop");
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].2, "deep-research");
    }

    #[test]
    fn prefix_still_narrows_by_op_applicability() {
        let all = vec![
            run("wf_1", "deep-research", WorkflowRunStatus::Complete),
            run("wf_2", "deep-research-2", WorkflowRunStatus::Active),
        ];
        let picked = narrow_run_matches(all, "deep", "stop");
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].2, "deep-research-2");
    }

    #[test]
    fn empty_selector_with_single_applicable_run_resolves() {
        let all = vec![
            run("wf_1", "a", WorkflowRunStatus::Complete),
            run("wf_2", "b", WorkflowRunStatus::UserPaused),
        ];
        let picked = narrow_run_matches(all, "", "resume");
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].2, "b");
    }

    #[test]
    fn failed_run_is_applicable_for_resume_narrowing() {
        let all = vec![
            run("wf_1", "a", WorkflowRunStatus::Complete),
            run("wf_2", "b", WorkflowRunStatus::Failed),
        ];
        let picked = narrow_run_matches(all, "", "resume");
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].2, "b");
    }

    #[test]
    fn ambiguous_stays_ambiguous() {
        let all = vec![
            run("wf_1", "a", WorkflowRunStatus::Active),
            run("wf_2", "b", WorkflowRunStatus::Active),
        ];
        assert_eq!(narrow_run_matches(all, "", "stop").len(), 2);
    }

    #[test]
    fn deep_research_report_preserves_partial_and_verification_failed_statuses() {
        let partial = deep_research_terminal_report(
            "query",
            &workflow::WorkflowOutcome::Completed {
                result: serde_json::json!({
                    "status": "partial",
                    "report": "Some verified material with coverage gaps."
                }),
            },
            Some(std::path::Path::new("/tmp/report.md")),
        );
        assert!(partial.contains("## Status\n\npartial"));
        assert!(partial.contains("## Full report\n\n`/tmp/report.md`"));

        let failed = deep_research_terminal_report(
            "query",
            &workflow::WorkflowOutcome::Completed {
                result: serde_json::json!({
                    "status": "partial",
                    "report": "None of the candidate claims survived independent source verification."
                }),
            },
            None,
        );
        assert!(failed.contains("## Status\n\nverification failed"));
    }

    #[test]
    fn every_deep_research_terminal_fallback_has_the_report_contract() {
        let outcomes = [
            workflow::WorkflowOutcome::BudgetExceeded {
                message: "budget".into(),
            },
            workflow::WorkflowOutcome::Cancelled,
            workflow::WorkflowOutcome::Failed {
                error: "process restart interrupted the run".into(),
            },
            workflow::WorkflowOutcome::Failed {
                error: "runtime unavailable".into(),
            },
        ];
        for outcome in outcomes {
            let report = deep_research_terminal_report("query", &outcome, None);
            for heading in [
                "## Status",
                "## Query",
                "## Investigation and verification",
                "## Limitations",
                "## Full report",
                "## Termination reason",
            ] {
                assert!(report.contains(heading), "missing {heading} in {report}");
            }
        }
    }
}

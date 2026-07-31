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
            WorkflowRunStatus::Complete => xai_workflow::WorkflowOutcome::Completed {
                result: serde_json::json!({
                    "report": state.result_summary.unwrap_or_else(|| {
                        "The process ended after completion, but no persisted report body was available."
                            .to_string()
                    })
                }),
            },
            WorkflowRunStatus::Cancelled => xai_workflow::WorkflowOutcome::Cancelled,
            WorkflowRunStatus::BudgetLimited => xai_workflow::WorkflowOutcome::BudgetExceeded {
                message: state
                    .pause_message
                    .unwrap_or_else(|| "The research agent budget was exhausted.".to_string()),
            },
            WorkflowRunStatus::Interrupted | WorkflowRunStatus::Failed => {
                xai_workflow::WorkflowOutcome::Failed {
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
        self.persist_behavior_state();
        // WorkflowManager delivers the terminal outcome through the session
        // mailbox. Dropping this secondary observer keeps all Behavior
        // transitions serialized on SessionActor.
        drop(outcome_rx);
        Ok(run_id)
    }

    pub(super) async fn finish_deep_research_run(
        &self,
        run_id: &str,
        outcome: xai_workflow::WorkflowOutcome,
    ) {
        if matches!(outcome, xai_workflow::WorkflowOutcome::Paused { .. }) {
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
        let report = deep_research_terminal_report(&query, &outcome);
        self.send_host_turn_slash_command_output(&report).await;
        let mut behavior = self.behavior.lock();
        if behavior.deep_research_run_id() != Some(run_id) {
            return;
        }
        behavior.clear_deep_research_run();
        behavior.select_behavior(None);
        drop(behavior);
        *self.current_prompt_mode.lock() = crate::session::behavior::PromptMode::Agent;
        self.persist_behavior_state();
        self.enqueue_current_mode_update(agent_client_protocol::SessionModeId::new(
            grow_tools::types::SessionMode::Default.as_id(),
        ));
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
            deep_research_terminal_report(&query, &xai_workflow::WorkflowOutcome::Cancelled);
        self.send_host_turn_slash_command_output(&report).await;
        self.persist_behavior_state();
    }
    pub(crate) fn named_workflow_snapshot(
        &self,
    ) -> (
        crate::session::workflow::registry::WorkflowRegistry,
        Vec<crate::session::workflow::registry::WorkflowListing>,
    ) {
        crate::session::workflow::registry::workflow_snapshot(Some(std::path::Path::new(
            self.session_info.cwd.as_str(),
        )))
    }

    pub(crate) async fn launch_named_workflow(
        self: &Arc<Self>,
        registry: &crate::session::workflow::registry::WorkflowRegistry,
        name: &str,
        input: &str,
    ) -> String {
        if self.behavior.lock().is_plan() {
            return "Workflow cannot be launched while Plan behavior is active. Complete or cancel the Plan first.".to_string();
        }
        let resolved = match registry.resolve_by_name(name) {
            Ok(r) => r,
            Err(e) => return format!("Workflow '{name}' unavailable: {e}"),
        };
        let (args, objective) = parse_named_workflow_args(input, &resolved.meta.description);
        let spec = crate::session::workflow::manager::LaunchSpec {
            objective,
            args,
            agent_budget: None,
            max_concurrency: None,
            resume_run_id: None,
        };
        let launched = self.workflow_manager.lock().await.launch(resolved, spec);
        match launched {
            Ok((run_id, outcome_rx)) => {
                let (display, objective) = self
                    .workflow_tracker()
                    .await
                    .lock()
                    .get(&run_id)
                    .map(|r| (r.name.clone(), r.objective.clone()))
                    .unwrap_or_else(|| (name.to_string(), String::new()));
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

        const USAGE: &str = "Usage: /workflow <name> [args] to launch a saved workflow, or \
                             /workflow-run <op> [name] to manage \
                             a run — ops: pause, resume, stop, save.";
        if op.is_empty() {
            return USAGE.to_string();
        }

        let matches: Vec<(String, WorkflowRunStatus, String)> = {
            let tracker = self.workflow_tracker().await;
            let tracker = tracker.lock();
            let all: Vec<_> = tracker
                .list()
                .iter()
                .filter(|r| r.run_id.starts_with(run_id) || r.name.starts_with(run_id))
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
                self.workflow_manager.lock().await.pause(&full_id);
                format!("Paused {name}. /workflow-run resume{id_suffix} to continue.")
            }
            "stop" => {
                if status.is_terminal() {
                    return format!(
                        "Run '{name}' is already finished (status: {}).",
                        status.as_str()
                    );
                }
                let owned_by_deep_research =
                    self.behavior.lock().deep_research_run_id() == Some(full_id.as_str());
                if owned_by_deep_research {
                    self.cancel_deep_research_with_report(&full_id).await;
                    self.behavior.lock().select_behavior(None);
                    *self.current_prompt_mode.lock() = crate::session::behavior::PromptMode::Agent;
                    self.persist_behavior_state();
                    self.enqueue_current_mode_update(agent_client_protocol::SessionModeId::new(
                        grow_tools::types::SessionMode::Default.as_id(),
                    ));
                } else {
                    self.workflow_manager.lock().await.cancel(&full_id);
                }
                format!("Stopped {name}.")
            }
            "resume" => {
                if self.behavior.lock().is_plan() {
                    return "Workflow cannot be resumed while Plan behavior is active. Complete or cancel the Plan first.".to_string();
                }
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
                        let tracker = self.workflow_tracker().await;
                        let tracker = tracker.lock();
                        let run = tracker.get(&full_id);
                        (
                            run.as_ref().map_or(0, |r| r.agents_used),
                            run.as_ref().and_then(|r| r.agent_budget),
                        )
                    };
                    let limit = limit.map_or_else(String::new, |l| format!("/{l}"));
                    if used >= xai_workflow::MAX_AGENT_BUDGET {
                        return format!(
                            "Run '{name}' exhausted the maximum agent budget ({used}{limit} agents) \
                             and cannot be resumed. Start a new run instead."
                        );
                    }
                    let suggested = used.saturating_add(64).min(xai_workflow::MAX_AGENT_BUDGET);
                    return format!(
                        "Run '{name}' exhausted its agent budget ({used}{limit} agents). \
                         Resuming keeps all finished work but needs a higher absolute cap — \
                         ask the agent to resume it with an agent budget above {used}, e.g. \
                         \"resume {name} with an agent budget of {suggested}\"."
                    );
                }
                let (script, args) = {
                    let manager = self.workflow_manager.lock().await;
                    (
                        manager.script_copy_for(&full_id),
                        manager.args_copy_for(&full_id),
                    )
                };
                let Some(script) = script else {
                    return format!("No persisted script for '{name}'; cannot resume.");
                };
                let resolved = match crate::session::workflow::registry::resolve_inline(script) {
                    Ok(r) => r,
                    Err(e) => return format!("Persisted script invalid: {e}"),
                };
                let objective = {
                    let tracker = self.workflow_tracker().await;
                    tracker
                        .lock()
                        .get(&full_id)
                        .map(|r| r.objective.clone())
                        .unwrap_or_default()
                };
                let agent_budget = {
                    let tracker = self.workflow_tracker().await;
                    tracker
                        .lock()
                        .get(&full_id)
                        .and_then(|run| run.agent_budget)
                };
                let objective_echo = objective.clone();
                let spec = crate::session::workflow::manager::LaunchSpec {
                    objective,
                    args,
                    agent_budget,
                    max_concurrency: None,
                    resume_run_id: Some(full_id.clone()),
                };
                match self.workflow_manager.lock().await.launch(resolved, spec) {
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
            "save" => {
                let Some(script) = self.workflow_manager.lock().await.script_copy_for(&full_id)
                else {
                    return format!("No persisted script for '{name}'; nothing to save.");
                };
                let definition_name =
                    match crate::session::workflow::registry::resolve_inline(script.clone()) {
                        Ok(resolved) => resolved.meta.name,
                        Err(error) => return format!("Could not save workflow '{name}': {error}"),
                    };
                if definition_name != name {
                    return format!(
                        "Save is disabled for run '{name}': it is a duplicate-run display handle, \
                         while the script is still named '{definition_name}'. Choose a new unique \
                         meta.name and save the script under that name instead."
                    );
                }
                if crate::session::workflow::registry::BUILTIN_WORKFLOWS
                    .iter()
                    .any(|builtin| builtin.name == definition_name)
                {
                    return format!(
                        "Save is disabled for built-in workflow '{definition_name}', which is \
                         already runnable. To customize it, create a copy with a new unique \
                         meta.name."
                    );
                }
                match crate::session::workflow::registry::save_project_workflow(
                    std::path::Path::new(self.session_info.cwd.as_str()),
                    &definition_name,
                    &script,
                ) {
                    Ok(path) => format!(
                        "Saved workflow '{definition_name}' to {} — runnable by name from now on.",
                        path.display()
                    ),
                    Err(e) => format!("Could not save workflow '{definition_name}': {e}"),
                }
            }
            other => format!("Unknown op '{other}'. {USAGE}"),
        }
    }
}

fn deep_research_terminal_report(query: &str, outcome: &xai_workflow::WorkflowOutcome) -> String {
    use xai_workflow::WorkflowOutcome;
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
            "# Deep Research Report\n\n## Status\n\n{status}\n\n## Query\n\n{query}\n\n## Verified findings\n\n{report}\n\n## Evidence\n\nSee the cited sources and verification notes in the findings above.\n\n## Limitations\n\nAny coverage gaps and uncertainty are recorded in the report body.\n\n## Termination reason\n\nThe research workflow reached a terminal result."
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
        "# Deep Research Report\n\n## Status\n\n{status}\n\n## Query\n\n{query}\n\n## Verified findings\n\n{findings}\n\n## Evidence\n\nNo additional independently verified evidence was available at termination.\n\n## Limitations\n\nThis is a terminal fallback report generated from the workflow outcome.\n\n## Termination reason\n\n{reason}"
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
            &xai_workflow::WorkflowOutcome::Completed {
                result: serde_json::json!({
                    "status": "partial",
                    "report": "Some verified material with coverage gaps."
                }),
            },
        );
        assert!(partial.contains("## Status\n\npartial"));

        let failed = deep_research_terminal_report(
            "query",
            &xai_workflow::WorkflowOutcome::Completed {
                result: serde_json::json!({
                    "status": "partial",
                    "report": "None of the candidate claims survived independent source verification."
                }),
            },
        );
        assert!(failed.contains("## Status\n\nverification failed"));
    }

    #[test]
    fn every_deep_research_terminal_fallback_has_the_report_contract() {
        let outcomes = [
            xai_workflow::WorkflowOutcome::BudgetExceeded {
                message: "budget".into(),
            },
            xai_workflow::WorkflowOutcome::Cancelled,
            xai_workflow::WorkflowOutcome::Failed {
                error: "process restart interrupted the run".into(),
            },
            xai_workflow::WorkflowOutcome::Failed {
                error: "runtime unavailable".into(),
            },
        ];
        for outcome in outcomes {
            let report = deep_research_terminal_report("query", &outcome);
            for heading in [
                "## Status",
                "## Query",
                "## Verified findings",
                "## Evidence",
                "## Limitations",
                "## Termination reason",
            ] {
                assert!(report.contains(heading), "missing {heading} in {report}");
            }
        }
    }
}

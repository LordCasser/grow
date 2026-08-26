use std::sync::Arc;

use super::SessionActor;

fn workflow_completion_source_version(
    state: &crate::session::workflow::tracker::WorkflowRunState,
) -> chat_state::NotificationSourceVersion {
    chat_state::NotificationSourceVersion::Opaque {
        value: format!(
            "workflow-terminal-v1:{}:{}",
            state.execution_epoch,
            state.status.as_str()
        ),
    }
}

impl SessionActor {
    /// Resolve Workflow delivery against the full immutable receipt fold, not
    /// the pending inbox. Returning `None` means this exact terminal boundary
    /// was already admitted (and may already be consumed), so callers must not
    /// regenerate configuration-sensitive payload text.
    async fn unresolved_workflow_completion_identity(
        &self,
        state: &crate::session::workflow::tracker::WorkflowRunState,
    ) -> Result<
        Option<(
            chat_state::NotificationSource,
            chat_state::NotificationSourceVersion,
        )>,
        String,
    > {
        let source = chat_state::NotificationSource::WorkflowCompleted {
            run_id: state.run_id.clone(),
        };
        let source_version = workflow_completion_source_version(state);
        match self
            .chat_state_handle
            .received_notification_id(source.clone(), source_version.clone())
            .await
        {
            Some(Some(_)) => Ok(None),
            Some(None) => Ok(Some((source, source_version))),
            None => Err("Workflow notification receipt fold is unavailable".into()),
        }
    }

    pub(super) async fn reconcile_restored_public_workflow_notifications(
        &self,
    ) -> Result<(), String> {
        let states = self.workflow_tracker().await.lock().list();
        for state in states
            .iter()
            .filter(|state| state.status.is_completion_reportable())
        {
            self.admit_public_workflow_completion(state).await?;
        }
        Ok(())
    }

    pub(super) async fn admit_public_workflow_completion(
        &self,
        state: &crate::session::workflow::tracker::WorkflowRunState,
    ) -> Result<(), String> {
        if !state.status.is_completion_reportable() {
            return Ok(());
        }
        let Some((source, source_version)) =
            self.unresolved_workflow_completion_identity(state).await?
        else {
            return Ok(());
        };
        let prompt_text = self.workflow_completion_notification(state).await;
        self.receive_notification(
            source,
            // Execution epoch plus terminal status is the stable identity of
            // this boundary. A resumable Ended boundary may be followed by a
            // Closed boundary in the same epoch; manifest revision cannot be
            // used because non-lifecycle projections also advance it.
            source_version,
            prompt_text,
        )
        .await
        .map(|_| ())
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
        let session_dir = &self.session_dir;
        match crate::session::workflow::workspace::WorkflowWorkspace::open_in_session(
            &self.session_directory,
            cwd,
        ) {
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
        // Command visibility is presentation, not authorization. Historical
        // Runs intentionally keep `/workflow-run` management available, so a
        // hand-written launch must still pass the session feature gate here.
        if !self.background_workflows_enabled {
            return "Background workflows are disabled for this session ([workflows] enabled = false / GROW_WORKFLOWS=0 / remote flag)."
                .into();
        }
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
        let mut workspace =
            match crate::session::workflow::workspace::WorkflowWorkspace::open_in_session(
                &self.session_directory,
                cwd,
            ) {
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
        let (args, objective) =
            parse_named_workflow_args(input, &definition.resolved.meta.description);
        let validation_script = definition.resolved.script.clone();
        let validation_args = args.clone();
        // Rhai and its Host seam are deliberately synchronous: Host functions
        // wait with `blocking_recv`. Keep the public-Workflow admission guard,
        // but execute preflight off the async session worker.
        match tokio::task::spawn_blocking(move || {
            workflow::validate_script_with_agent_budget(
                &validation_script,
                Some(validation_args),
                workflow::DEFAULT_AGENT_BUDGET,
            )
        })
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                return format!("Workflow '{name}' failed preflight and was not started: {error}");
            }
            Err(error) => {
                return format!(
                    "Workflow '{name}' preflight could not be completed and was not started: \
                     validator task failed: {error}"
                );
            }
        }
        if let Err(error) =
            workspace.record_validated(cwd, &definition_id, &definition.summary.content_hash)
        {
            return format!("Workflow '{name}' changed during preflight: {error}");
        }
        let resolved = definition.resolved;
        let spec = crate::session::workflow::manager::LaunchSpec {
            objective,
            args,
            agent_budget: None,
            max_concurrency: None,
            resume_run_id: None,
        };
        let launched = manager.launch(resolved, spec).await;
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

        const USAGE: &str = "Usage: /workflow-run <name> [args] to launch a saved workflow, or \
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
                    "Several runs could be '{op}' — pick one by name:\n{}\n(/workflow-run {op} <name>)",
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
                if let Err(error) = manager.pause(&full_id).await {
                    return format!("Could not pause '{name}': {error}");
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
                if let Err(error) = manager.cancel(&full_id).await {
                    return format!("Could not stop '{name}': {error}");
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
                match manager.launch(resolved, spec).await {
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
        let workspace =
            match crate::session::workflow::workspace::WorkflowWorkspace::open_in_session(
                &self.session_directory,
                cwd,
            ) {
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
        let runs: Vec<_> = tracker
            .lock()
            .list()
            .iter()
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
        if runs.is_empty() {
            lines.push("  (none)".into());
        } else {
            lines.extend(runs);
        }
        lines.join("\n")
    }
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
    use super::{narrow_run_matches, workflow_completion_source_version};
    use crate::session::workflow::tracker::{WorkflowRunStatus, WorkflowTracker};

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
    fn workflow_completion_retry_identity_ignores_manifest_projection_revisions() {
        let mut tracker = WorkflowTracker::default();
        let mut state = tracker.start_run(
            "wf-replay".into(),
            "replay".into(),
            "verify restore".into(),
            Vec::new(),
            None,
            None,
            crate::session::workflow::tracker::WorkflowRuntimeRoute::for_test(
                "test-model",
                None,
                sampling_types::ModelImageInputKey::new("test-model", "responses", "test-endpoint"),
            )
            .unwrap(),
        );
        let first = workflow_completion_source_version(&state);
        state.revision = state.revision.saturating_add(7);
        assert_eq!(workflow_completion_source_version(&state), first);
    }

    #[tokio::test]
    async fn restored_consumed_workflow_receipt_does_not_regenerate_payload() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                let mut tracker = WorkflowTracker::default();
                let mut state = tracker.start_run(
                    "wf-consumed".into(),
                    "historical".into(),
                    "verify restore idempotence".into(),
                    Vec::new(),
                    None,
                    None,
                    crate::session::workflow::tracker::WorkflowRuntimeRoute::for_test(
                        "test-model",
                        None,
                        sampling_types::ModelImageInputKey::new(
                            "test-model",
                            "responses",
                            "test-endpoint",
                        ),
                    )
                    .unwrap(),
                );
                state.status = WorkflowRunStatus::Complete;
                let source = chat_state::NotificationSource::WorkflowCompleted {
                    run_id: state.run_id.clone(),
                };
                let version = workflow_completion_source_version(&state);
                let receipt = actor
                    .receive_notification(
                        source.clone(),
                        version.clone(),
                        "historical payload rendered with an old tool alias".into(),
                    )
                    .await
                    .expect("historical receipt");
                crate::session::actor::tests::support::begin_test_causal_turn(&actor).await;
                let turn = actor.events.current_turn().expect("causal turn");
                actor
                    .consume_notifications_durably(vec![receipt.clone()], turn, None)
                    .await
                    .expect("consume historical receipt");

                actor
                    .admit_public_workflow_completion(&state)
                    .await
                    .expect("restore must accept the existing receipt without rebuilding body");

                assert!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .expect("pending projection")
                        .is_empty()
                );
                assert_eq!(
                    actor
                        .chat_state_handle
                        .received_notification_id(source, version)
                        .await,
                    Some(Some(receipt))
                );
            })
            .await;
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

    #[tokio::test]
    async fn workflow_run_help_uses_the_host_command_namespace() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Workflow);

                let usage = actor.manage_workflow_run("", "").await;
                assert!(usage.contains("/workflow-run <name> [args]"), "{usage}");
                assert!(!usage.contains("/workflow <name>"), "{usage}");

                let tracker = actor.workflow_tracker().await;
                let mut tracker = tracker.lock();
                for (run_id, name) in [("wf-a", "alpha"), ("wf-b", "beta")] {
                    tracker.start_run(
                        run_id.into(),
                        name.into(),
                        "verify command help".into(),
                        Vec::new(),
                        None,
                        None,
                        crate::session::workflow::tracker::WorkflowRuntimeRoute::for_test(
                            "test-model",
                            None,
                            sampling_types::ModelImageInputKey::new(
                                "test-model",
                                "responses",
                                "test-endpoint",
                            ),
                        )
                        .unwrap(),
                    );
                }
                drop(tracker);

                let ambiguity = actor.manage_workflow_run("", "stop").await;
                assert!(
                    ambiguity.contains("/workflow-run stop <name>"),
                    "{ambiguity}"
                );
                assert!(!ambiguity.contains("/workflow stop"), "{ambiguity}");
            })
            .await;
    }
}

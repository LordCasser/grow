//! Session bring-up concern for the actor: `spawn_session_actor`, the
//! per-session OS thread (`SessionThread` / `spawn_session_on_thread`), and
//! the MCP auto-restart wiring (`SessionRestartActions`).
#![allow(clippy::items_after_test_module)]
use super::*;
use crate::remote::DEFAULT_CONTEXT_WINDOW;
use futures_util::FutureExt as _;

fn permission_audit_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.is_empty())
        .map(|value| tools::util::truncate_line(&value, 240).into_owned())
}

/// Project one permission request into durable/UI audit data without copying
/// arbitrary tool input. Full commands and MCP argument values are valid
/// authorization evidence but are not safe telemetry: they may contain
/// credentials, and `updates.jsonl` is durable and replayed to clients.
fn permission_audit_access_summary(
    event: &workspace::permission::PermissionEvent,
) -> Option<String> {
    let summary = match event.access_kind.as_str() {
        "read" | "grep" | "edit" => event
            .access_detail
            .as_ref()
            .map(|_| "path details redacted".to_owned()),
        "bash" => Some("command details redacted".to_owned()),
        "mcp" => event
            .access_detail
            .as_deref()
            .and_then(|detail| detail.split_whitespace().next())
            .filter(|name| !name.is_empty())
            .map(|name| format!("{name} (arguments redacted)")),
        "web_fetch" => event.access_detail.as_deref().map(|raw| {
            let Ok(mut url) = url::Url::parse(raw) else {
                return "URL details redacted".to_owned();
            };
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_path("");
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }),
        _ => None,
    };
    permission_audit_text(summary)
}

fn subagent_permission_updates(
    event: workspace::permission::PermissionEvent,
) -> Option<(GrowSessionUpdate, GrowSessionUpdate)> {
    use crate::extensions::notification::SubagentPermissionOutcome;

    if event.permission_mode.as_deref() != Some("auto") {
        return None;
    }
    let access_summary = permission_audit_access_summary(&event);
    let outcome = match event.decision.as_str() {
        "allow" => SubagentPermissionOutcome::Approved,
        "timed_out" => SubagentPermissionOutcome::TimedOut,
        "cancelled" => SubagentPermissionOutcome::Cancelled,
        _ if event.classifier_verdict.as_deref() == Some("unavailable") => {
            SubagentPermissionOutcome::Unavailable
        }
        _ => SubagentPermissionOutcome::Denied,
    };
    let source = if event.user_prompted {
        "user_prompt".to_owned()
    } else if event.classifier_verdict.is_some() {
        "main_agent".to_owned()
    } else {
        event
            .decision_reason
            .clone()
            .unwrap_or_else(|| "permission_manager".to_owned())
    };
    // Model and prompt prose may echo the raw request. Durable audit keeps
    // only harness-owned reason codes; detailed evidence remains in the
    // ephemeral permission manager event.
    let reason = permission_audit_text(event.decision_reason.clone());
    let live_access_detail = event.access_detail.clone();
    let live_reason = event.decision_reason.clone();
    let live_classifier_reason = event.classifier_reason.clone();
    let child_session_id = event.subagent_session_id?;
    let durable = GrowSessionUpdate::SubagentPermissionDecision {
        child_session_id,
        subagent_type: event.subagent_type,
        description: event.subagent_description,
        tool_call_id: event.tool_id,
        tool_name: event.tool_name,
        access_kind: event.access_kind,
        access_summary,
        access_detail: None,
        outcome,
        source,
        reason,
        classifier_reason: None,
        latency_ms: event.classifier_latency_ms.or(event.wait_ms),
    };
    let mut live = durable.clone();
    if let GrowSessionUpdate::SubagentPermissionDecision {
        access_detail,
        reason,
        classifier_reason,
        ..
    } = &mut live
    {
        *access_detail = live_access_detail;
        *reason = live_reason;
        *classifier_reason = live_classifier_reason;
    }
    Some((durable, live))
}

#[cfg(test)]
mod permission_audit_tests {
    use super::*;
    use crate::extensions::notification::SubagentPermissionOutcome;

    fn event(decision: &str) -> workspace::permission::PermissionEvent {
        workspace::permission::PermissionEvent {
            audit_sequence: 0,
            tool_id: "tool-1".into(),
            tool_name: "run_terminal_command".into(),
            access_kind: "bash".into(),
            access_detail: Some("cargo test -p shell".into()),
            auto_approved: decision == "allow",
            user_prompted: false,
            decision: decision.into(),
            prompt_outcome: None,
            reject_reason: None,
            timestamp: chrono::Utc::now(),
            subagent_session_id: Some("child-1".into()),
            subagent_type: Some("software-coder".into()),
            subagent_description: Some("verify the change".into()),
            permission_mode: Some("auto".into()),
            requested_permission_mode: Some(
                workspace::permission::types::RequestPermissionMode::Auto,
            ),
            decision_reason: Some("auto_classifier_allow".into()),
            classifier_source: Some("llm".into()),
            classifier_verdict: Some("allow".into()),
            classifier_reason: Some("required by the primary task".into()),
            classifier_latency_ms: Some(18),
            auto_denials_consecutive: Some(0),
            auto_denials_total: Some(0),
            wait_ms: Some(20),
            queue_depth: Some(1),
        }
    }

    #[test]
    fn maps_final_permission_event_to_ui_audit_update() {
        let (durable, live) = subagent_permission_updates(event("allow")).expect("audit update");
        let GrowSessionUpdate::SubagentPermissionDecision {
            child_session_id,
            outcome,
            source,
            latency_ms,
            access_detail,
            classifier_reason,
            ..
        } = durable
        else {
            panic!("unexpected update")
        };
        assert_eq!(child_session_id, "child-1");
        assert_eq!(outcome, SubagentPermissionOutcome::Approved);
        assert_eq!(source, "main_agent");
        assert_eq!(latency_ms, Some(18));
        assert_eq!(access_detail, None);
        assert_eq!(classifier_reason, None);
        let GrowSessionUpdate::SubagentPermissionDecision {
            access_detail,
            classifier_reason,
            ..
        } = live
        else {
            panic!("unexpected live update")
        };
        assert_eq!(access_detail.as_deref(), Some("cargo test -p shell"));
        assert_eq!(
            classifier_reason.as_deref(),
            Some("required by the primary task")
        );
    }

    #[test]
    fn unavailable_and_prompt_timeout_remain_distinct_audit_outcomes() {
        let mut unavailable = event("reject");
        unavailable.classifier_verdict = Some("unavailable".into());
        unavailable.classifier_source = Some("timeout".into());
        unavailable.classifier_reason = Some("permission judgment timed out".into());
        let GrowSessionUpdate::SubagentPermissionDecision { outcome, .. } =
            subagent_permission_updates(unavailable)
                .expect("unavailable update")
                .0
        else {
            panic!("unexpected update")
        };
        assert_eq!(outcome, SubagentPermissionOutcome::Unavailable);

        let mut prompt_timeout = event("timed_out");
        prompt_timeout.user_prompted = true;
        prompt_timeout.classifier_source = None;
        prompt_timeout.classifier_verdict = None;
        let GrowSessionUpdate::SubagentPermissionDecision {
            outcome, source, ..
        } = subagent_permission_updates(prompt_timeout)
            .expect("timeout update")
            .0
        else {
            panic!("unexpected update")
        };
        assert_eq!(outcome, SubagentPermissionOutcome::TimedOut);
        assert_eq!(source, "user_prompt");
    }

    #[test]
    fn ignores_primary_and_non_auto_permission_events() {
        let mut primary = event("allow");
        primary.subagent_session_id = None;
        assert!(subagent_permission_updates(primary).is_none());

        let mut child_ask = event("allow");
        child_ask.permission_mode = Some("ask".into());
        assert!(subagent_permission_updates(child_ask).is_none());
    }

    #[test]
    fn durable_audit_summary_never_copies_tool_secrets() {
        let mut bash = event("allow");
        bash.access_detail = Some("TOKEN=top-secret cargo test --password hunter2".into());
        let bash_json =
            serde_json::to_string(&subagent_permission_updates(bash).unwrap().0).unwrap();
        assert!(bash_json.contains("command details redacted"));
        assert!(!bash_json.contains("top-secret"));
        assert!(!bash_json.contains("hunter2"));

        let mut path = event("allow");
        path.access_kind = "read".into();
        path.access_detail = Some("/private/TOKEN-top-secret/report.md".into());
        path.classifier_reason = Some("Allowed because TOKEN=top-secret is valid".into());
        let path_json =
            serde_json::to_string(&subagent_permission_updates(path).unwrap().0).unwrap();
        assert!(path_json.contains("path details redacted"));
        assert!(!path_json.contains("report.md"));
        assert!(!path_json.contains("TOKEN-top-secret"));
        assert!(!path_json.contains("TOKEN=top-secret"));

        let mut mcp = event("allow");
        mcp.access_kind = "mcp".into();
        mcp.access_detail = Some(
            r#"linear__create_issue {"Authorization":"Bearer secret","password":"hunter2"}"#.into(),
        );
        let mcp_json = serde_json::to_string(&subagent_permission_updates(mcp).unwrap().0).unwrap();
        assert!(mcp_json.contains("linear__create_issue (arguments redacted)"));
        assert!(!mcp_json.contains("Bearer secret"));
        assert!(!mcp_json.contains("hunter2"));

        let mut web = event("allow");
        web.access_kind = "web_fetch".into();
        web.access_detail = Some(
            "https://user:password@example.test/docs?token=query-secret#fragment-secret".into(),
        );
        let web_json = serde_json::to_string(&subagent_permission_updates(web).unwrap().0).unwrap();
        assert!(web_json.contains("https://example.test/"));
        for secret in ["user", "password", "query-secret", "fragment-secret"] {
            assert!(!web_json.contains(secret), "audit leaked {secret}");
        }
    }
}

fn restored_runtime_conflict_actions(
    behavior: tool_types::BehaviorId,
    active_goal: bool,
    public_workflow_active: bool,
) -> (bool, bool) {
    if !public_workflow_active {
        return (false, false);
    }
    match behavior {
        tool_types::BehaviorId::Goal if active_goal => (true, true),
        tool_types::BehaviorId::Plan => (false, true),
        _ => (false, false),
    }
}

fn restored_plan_artifact_is_valid(
    session: &crate::session::storage::ContainedDirectory,
    snapshot: &crate::session::behavior::BehaviorSnapshot,
    phase: crate::session::behavior::PlanPhase,
) -> bool {
    let has_no_artifact =
        snapshot.plan_artifact_revision == 0 && snapshot.plan_artifact_hash.is_none();
    if phase == crate::session::behavior::PlanPhase::Drafting && has_no_artifact {
        return true;
    }
    let Some(hash) = snapshot.plan_artifact_hash.as_deref() else {
        return false;
    };
    snapshot.plan_artifact_revision > 0
        && crate::session::behavior::read_plan_artifact(session, hash).is_ok()
}

#[cfg(test)]
mod goal_restore_reconciliation_tests {
    use super::restored_runtime_conflict_actions;
    use tool_types::BehaviorId;

    #[test]
    fn restored_public_workflow_conflicts_fail_closed_without_deleting_the_run() {
        assert_eq!(
            restored_runtime_conflict_actions(BehaviorId::Goal, true, true),
            (true, true)
        );
        assert_eq!(
            restored_runtime_conflict_actions(BehaviorId::Plan, false, true),
            (false, true)
        );
        assert_eq!(
            restored_runtime_conflict_actions(BehaviorId::Workflow, false, true),
            (false, false)
        );
    }
}

#[cfg(test)]
mod plan_restore_validation_tests {
    use super::restored_plan_artifact_is_valid;
    use crate::session::behavior::{BehaviorCoordinator, BehaviorSnapshot, PlanPhase};
    use tool_types::BehaviorId;

    fn session(dir: &tempfile::TempDir) -> crate::session::storage::ContainedDirectory {
        crate::session::storage::ContainedDirectory::open(
            dir.path(),
            std::path::Path::new(""),
            "Plan restore test session",
            false,
        )
        .unwrap()
    }

    #[test]
    fn fresh_drafting_plan_restores_without_an_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = BehaviorSnapshot::selected(BehaviorId::Plan);
        let session = session(&dir);
        assert!(restored_plan_artifact_is_valid(
            &session,
            &snapshot,
            PlanPhase::Drafting,
        ));
    }

    #[test]
    fn submitted_and_executing_plans_require_the_matching_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let markdown = "# Plan\n\n- implement the change\n";
        let session = session(&dir);
        let mut coordinator =
            BehaviorCoordinator::from_snapshot(BehaviorSnapshot::selected(BehaviorId::Plan));
        coordinator.record_plan_artifact(markdown);
        assert!(coordinator.submit_initial_plan());
        let submitted = coordinator.snapshot();
        assert!(!restored_plan_artifact_is_valid(
            &session,
            &submitted,
            PlanPhase::AwaitingApproval,
        ));
        crate::session::behavior::write_plan_artifact(&session, markdown).unwrap();
        assert!(restored_plan_artifact_is_valid(
            &session,
            &submitted,
            PlanPhase::AwaitingApproval,
        ));

        assert!(coordinator.approve_submitted_plan());
        let executing = coordinator.snapshot();
        assert!(restored_plan_artifact_is_valid(
            &session,
            &executing,
            PlanPhase::Executing,
        ));
    }
}
/// Build the per-session current-thread tokio runtime.
///
/// Construction acquires fds (epoll/kqueue, waker) and fails with
/// `EMFILE`/`EAGAIN` under resource pressure. Extracted so the containment
/// contract — exhaustion returns `Err`, never aborts — is testable
/// (`runtime_containment_tests`).
pub(crate) fn build_session_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}
/// Building the session runtime under fd exhaustion must return `Err`, never
/// panic (under `panic=abort` a panic kills every live session).
///
/// The rlimit is lowered only in a re-exec'd child (the `grow-gix-status`
/// pattern), so parallel tests are unaffected; stdout markers distinguish
/// skip (unenforceable environment) from pass/fail.
#[cfg(all(test, unix))]
mod runtime_containment_tests {
    use super::build_session_runtime;
    /// Env marker dispatching the re-exec'd test binary into child logic.
    const CHILD_ENV: &str = "GROW_SHELL_RUNTIME_CONTAINMENT_CHILD";
    const PASS_MARK: &str = "runtime-build-contained:";
    const SKIP_MARK: &str = "skip-child:";
    /// Child: lower RLIMIT_NOFILE, fill the fd table, assert `Err`.
    fn run_child() -> ! {
        let mut lim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) } != 0 {
            println!("{SKIP_MARK} getrlimit failed");
            std::process::exit(0);
        }
        lim.rlim_cur = 64.min(lim.rlim_max);
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lim) } != 0 {
            println!("{SKIP_MARK} setrlimit failed");
            std::process::exit(0);
        }
        let mut held = Vec::new();
        loop {
            let fd = unsafe { libc::dup(0) };
            if fd < 0 {
                break;
            }
            held.push(fd);
            if held.len() > 4096 {
                println!("{SKIP_MARK} fd limit not enforced");
                std::process::exit(0);
            }
        }
        match build_session_runtime() {
            Err(e) => {
                println!("{PASS_MARK} {e}");
                std::process::exit(0);
            }
            Ok(_) => {
                println!("{SKIP_MARK} runtime built despite full fd table");
                std::process::exit(0);
            }
        }
    }
    /// Doubles as the child entry point when `CHILD_ENV` is set.
    #[test]
    fn child_entry_runtime_build_under_fd_exhaustion() {
        if std::env::var_os(CHILD_ENV).is_some() {
            run_child();
        }
    }
    #[test]
    fn runtime_build_failure_is_contained() {
        let filter = module_path!()
            .split_once("::")
            .map(|(_, rest)| rest)
            .unwrap_or_default();
        let exe = std::env::current_exe().expect("current_exe");
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--exact")
            .arg(format!(
                "{filter}::child_entry_runtime_build_under_fd_exhaustion"
            ))
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CHILD_ENV, "1")
            .stdin(std::process::Stdio::null());
        tty_utils::detach_std_command(&mut cmd);
        let out = cmd.output().expect("spawn child test process");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success() && !stderr.contains("panicked at"),
            "child aborted/panicked instead of containing the failure \
             (status: {:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
            out.status
        );
        if stdout.contains(SKIP_MARK) {
            eprintln!("skipped: {stdout}");
            return;
        }
        assert!(
            stdout.contains(PASS_MARK),
            "no pass/skip marker (filter matched nothing?)\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}
/// Spawns a session actor. The permission-event receiver, when this session
/// owns the shared manager, is consumed internally by the primary session's
/// passive audit bridge.
pub(crate) enum TimelineBootstrap {
    Fresh { session_rules: Option<String> },
    Existing(Vec<chat_state::TimelineEvent>),
}

impl TimelineBootstrap {
    pub(crate) fn is_fresh(&self) -> bool {
        matches!(self, Self::Fresh { .. })
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Fresh { .. } => "fresh",
            Self::Existing(_) => "existing",
        }
    }
}

#[tracing::instrument(
    name = "session.spawn",
    skip_all,
    fields(
        session_id = %session_info.id.0,
        client_type = ?client_type,
        start_type = timeline_bootstrap.label(),
    ),
)]
pub(crate) async fn spawn_session_actor(
    session_info: SessionInfo,
    session_dir: std::path::PathBuf,
    gateway: GatewaySender,
    sampling_config: SamplingConfig,
    credentials: chat_state::Credentials,
    auth_method_id: crate::agent::auth_method::SharedAuthMethodId,
    mut tool_context: ToolContext,
    mcp_servers: Vec<acp::McpServer>,
    initial_client_mcp_servers: Vec<acp::McpServer>,
    mcp_meta_config_map: McpMetaConfigMap,
    parent_mcp_pool: Option<crate::session::mcp_servers::SharedMcpPool>,
    acp_mcp_servers: Vec<crate::session::mcp_servers::AcpServerEntry>,
    support_permission: bool,
    auto_update: Option<bool>,
    persistence: PersistenceHandle,
    session_title_route: Option<crate::session::actor::summary::SessionTitleRoute>,
    timeline_bootstrap: TimelineBootstrap,
    rewind_points_source: Option<workspace::session::file_state::PinnedRewindSource>,
    fs_notify_config: Option<ClientFsConfig>,
    mut startup_hints: StartupHints,
    client_type: ClientType,
    permission_prompt_timeout: std::time::Duration,
    auto_compact_threshold_percent: u8,
    system_prompt_label: String,
    compaction_verbatim_input: bool,
    compaction_pre_prune: bool,
    compaction_pre_prune_token_budget: Option<u64>,
    buffering_settings: Option<BufferingSettings>,
    origin_client: Option<crate::http::OriginClientInfo>,
    codebase_indexes: std::sync::Arc<parking_lot::Mutex<CodebaseIndexManager>>,
    code_nav_enabled: bool,
    fs_watch_caps: fs_watch::FsWatchCapabilities,
    client_terminal_capable: bool,
    client_fs_capable: bool,
    gateway_enabled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    agent_definition: AgentDefinition,
    skills_config: SkillsConfig,
    preloaded_skills: Option<Vec<tools::implementations::skills::types::SkillInfo>>,
    incremental_bash_output: bool,
    persisted_signals: Option<crate::session::signals::SessionSignals>,
    persisted_behavior: Option<crate::session::behavior::BehaviorSnapshot>,
    persisted_goal_mode: Option<crate::session::goal_tracker::GoalState>,
    persisted_control_revision: u64,
    persisted_workflow_runs: Vec<crate::session::workflow::store::RestoredWorkflowRun>,
    persisted_announcement_state: Option<crate::session::announcement_state::AnnouncementState>,
    memory_config: Option<crate::config::MemoryConfig>,
    session_model_id: crate::agent::models::ModelId,
    session_permission_mode: crate::util::config::PermissionMode,
    session_client_identifier: Option<String>,
    inference_idle_timeout_secs: u64,
    max_retries: Option<u32>,
    web_fetch_config: tools::implementations::grow_build::web_fetch::WebFetchConfig,
    app_builder_deployer_config: tools::implementations::grow_build::deploy_app::AppBuilderDeployerConfig,
    write_file_enabled: bool,
    goal_enabled: bool,
    background_workflows_enabled: bool,
    subagents_enabled: bool,
    subagents_max_depth: u32,
    subagent_classifier_input: crate::config::SubagentClassifierInput,
    ask_user_question_enabled: bool,
    client_hooks: crate::extensions::hooks::ClientHooks,
    prompt_display_cwd: Option<String>,
    subagent_toggle: std::collections::HashMap<String, bool>,
    workflow_cli_agents: Vec<agent::config::AgentDefinition>,
    workflow_cli_overrides: crate::agent::config::CliAgentOverrides,
    workflow_file_tool_overrides: Option<Vec<tools::registry::types::ToolConfig>>,
    frozen_subagent_names: Option<Vec<String>>,
    prompt_audience: agent::prompt::context::PromptAudience,
    respect_gitignore: bool,
    path_not_found_hints: bool,
    tool_params_json: crate::session::agent_rebuild::ResolvedToolParamsJson,
    plugin_registry: Option<std::sync::Arc<agent::plugins::PluginRegistry>>,
    plugin_registry_handle: Option<agent::plugins::SharedPluginRegistryHandle>,
    models_manager: crate::agent::models::ModelsManager,
    inherited_permission_handle: Option<workspace::permission::PermissionHandle>,
    api_key_provider: Option<tools::types::SharedApiKeyProvider>,
    image_description_model: Option<String>,
    hook_registry_override: Option<std::sync::Arc<::hooks::discovery::HookRegistry>>,
    workspace_ops: workspace::WorkspaceOps,
    cli_permission_rules: Vec<workspace::permission::types::PermissionRule>,
    remote_settings: Option<crate::util::config::RemoteSettings>,
    laziness_debug_log: Option<std::path::PathBuf>,
    parent_terminal_backend: Option<std::sync::Arc<dyn tools::computer::types::TerminalBackend>>,
    parent_scheduler_handle: Option<
        tools::implementations::grow_build::scheduler::types::SchedulerHandle,
    >,
    max_turns: Option<usize>,
) -> Result<(SessionHandle, tokio::sync::oneshot::Receiver<()>), agent::AgentBuildError> {
    if max_turns == Some(0) {
        return Err(agent::AgentBuildError::InvalidConfig(
            "max_turns must be greater than 0".to_string(),
        ));
    }
    let session_directory = match persistence.session_directory() {
        Some(directory) => directory,
        None => {
            #[cfg(test)]
            {
                std::fs::create_dir_all(&session_dir).map_err(|error| {
                    agent::AgentBuildError::InvalidConfig(format!(
                        "failed to create test session directory: {error}"
                    ))
                })?;
                std::sync::Arc::new(
                    crate::session::storage::ContainedDirectory::open(
                        &session_dir,
                        std::path::Path::new(""),
                        "test session entity",
                        false,
                    )
                    .map_err(|error| {
                        agent::AgentBuildError::InvalidConfig(format!(
                            "failed to pin test session directory: {error}"
                        ))
                    })?,
                )
            }
            #[cfg(not(test))]
            {
                return Err(agent::AgentBuildError::InvalidConfig(
                    "session persistence did not provide an identity-bound directory capability"
                        .to_string(),
                ));
            }
        }
    };
    let (
        resumed_timeline,
        validated_timeline,
        mut conversation,
        fresh_session_rules,
        mut restored_control_intents,
    ) = match timeline_bootstrap {
        TimelineBootstrap::Fresh { session_rules } => (
            None,
            None,
            Vec::new(),
            Some(session_rules),
            std::collections::HashMap::new(),
        ),
        TimelineBootstrap::Existing(events) => {
            let mut receipts = std::collections::HashMap::new();
            let durable_receipts =
                crate::session::control::SessionControlSnapshot::durable_receipts_from_timeline(
                    &events,
                )
                .and_then(|mut receipts| {
                    receipts.extend(crate::session::persistence::durable_model_control_receipts(
                        &events,
                    )?);
                    Ok(receipts)
                })
                .map_err(|error| {
                    agent::AgentBuildError::InvalidConfig(format!(
                        "invalid persisted control receipt: {error}"
                    ))
                })?;
            for receipt in durable_receipts {
                AdmissionState::restore_terminal_control_intent(
                    &mut receipts,
                    receipt.domain,
                    &receipt.intent,
                    ControlIntentTerminal {
                        phase: crate::extensions::notification::ControlPhase::Applied,
                        target: receipt.target,
                        message: None,
                        ui_terminal_durable: false,
                    },
                )
                .map_err(|error| agent::AgentBuildError::InvalidConfig(error))?;
            }
            let timeline = chat_state::Timeline::from_events(events).map_err(|error| {
                agent::AgentBuildError::InvalidConfig(format!(
                    "invalid persisted conversation timeline: {error}"
                ))
            })?;
            let surface = timeline.surface().to_vec();
            (
                Some(timeline.clone()),
                Some(timeline),
                surface,
                None,
                receipts,
            )
        }
    };
    let mut control_receipt_error = None;
    let _ = crate::session::storage::stream_replay_grow_notifications_in(
        session_directory.as_ref(),
        |notification| {
            let GrowSessionUpdate::ControlStateUpdate(update) = notification.update else {
                return;
            };
            if !matches!(
                update.phase,
                crate::extensions::notification::ControlPhase::Applied
                    | crate::extensions::notification::ControlPhase::Rejected
                    | crate::extensions::notification::ControlPhase::Superseded
            ) {
                return;
            }
            if let Some(intent) = update.intent.as_ref() {
                let target = update
                    .desired
                    .clone()
                    .unwrap_or_else(|| update.current.clone());
                if target.domain() != update.domain {
                    control_receipt_error = Some(
                        "persisted control terminal target does not match its domain".to_string(),
                    );
                    return;
                }
                if let Err(error) = AdmissionState::restore_terminal_control_intent(
                    &mut restored_control_intents,
                    update.domain,
                    intent,
                    ControlIntentTerminal {
                        phase: update.phase,
                        target,
                        message: update.message.clone(),
                        ui_terminal_durable: true,
                    },
                ) {
                    control_receipt_error = Some(error);
                }
            }
        },
    )
    .map_err(|error| {
        agent::AgentBuildError::InvalidConfig(format!(
            "failed to restore durable control receipts: {error}"
        ))
    })?;
    if let Some(error) = control_receipt_error {
        return Err(agent::AgentBuildError::InvalidConfig(error));
    }
    if validated_timeline.is_some()
        && !matches!(conversation.first(), Some(ConversationItem::System(_)))
    {
        return Err(agent::AgentBuildError::InvalidConfig(
            "persisted Timeline has no stable System head".to_string(),
        ));
    }
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    tracing::info!(
        "Session '{}' created with {} MCP servers",
        session_info.id.0,
        mcp_servers.len()
    );
    let _ = support_permission;
    let owns_permission_manager = inherited_permission_handle.is_none();
    let (permissions, permission_events_rx, deny_read_globs) = if let Some(handle) =
        inherited_permission_handle
    {
        let deny_read_globs = handle.deny_read_globs();
        (handle, None, deny_read_globs)
    } else {
        let web_fetch_allowed_domains = match &web_fetch_config {
            WebFetchConfig::Enabled { params } => params.allowed_domains(),
            WebFetchConfig::Disabled => vec![],
        };
        let project_trusted =
            crate::agent::folder_trust::project_scope_allowed(tool_context.cwd.as_path());
        let mut permission_config = workspace::permission::resolution::resolve_permission_config(
            tool_context.cwd.as_path(),
            project_trusted,
        )
        .await;
        if !cli_permission_rules.is_empty() {
            match &mut permission_config {
                Some(config) => {
                    let mut merged = cli_permission_rules;
                    merged.append(&mut config.rules);
                    config.rules = merged;
                }
                None => {
                    permission_config = Some(workspace::permission::types::PermissionConfig::new(
                        cli_permission_rules,
                    ));
                }
            }
        }
        let deny_read_globs = permission_config
            .as_ref()
            .map(workspace::permission::resolution::deny_read_globs_from_config)
            .unwrap_or_default();
        let (permissions, permission_events_rx) = workspace::permission::spawn_permission_manager(
            session_info.id.clone(),
            gateway.clone(),
            tool_context.cwd.clone(),
            client_type,
            permission_prompt_timeout,
            permission_config,
            deny_read_globs.clone(),
            web_fetch_allowed_domains,
            session_permission_mode,
            session_client_identifier.clone(),
            crate::util::config::remember_tool_approvals_from_disk(),
        );
        (permissions, Some(permission_events_rx), deny_read_globs)
    };
    let initial_prompt_index = conversation
        .iter()
        .filter(|item| matches!(item, ConversationItem::User(_)))
        .count();
    let initial_conversation_len = conversation.len();
    let initial_user_count = initial_prompt_index.saturating_sub(1) as u32;
    let initial_assistant_count = conversation
        .iter()
        .filter(|item| matches!(item, ConversationItem::Assistant(_)))
        .count() as u32;
    let (initial_tool_call_count, initial_tools_used, initial_models_used) =
        if persisted_signals.is_none() {
            let mut tool_call_count: u32 = 0;
            let mut tools_used = std::collections::HashSet::new();
            let mut models_used = std::collections::HashSet::new();
            for item in &conversation {
                if let ConversationItem::Assistant(assistant) = item {
                    tool_call_count += assistant.tool_calls.len() as u32;
                    for tc in &assistant.tool_calls {
                        tools_used.insert(tc.name.clone());
                    }
                    if let Some(model_id) = &assistant.model_id {
                        models_used.insert(model_id.clone());
                    }
                }
            }
            (
                tool_call_count,
                tools_used.into_iter().collect::<Vec<_>>(),
                models_used.into_iter().collect::<Vec<_>>(),
            )
        } else {
            (0, Vec::new(), Vec::new())
        };
    let primary_model_id = session_model_id.0.to_string();
    let embed_base_url = sampling_config.base_url.clone();
    let embed_api_key = sampling_config.api_key.clone();
    let context_window_override = std::env::var("GROW_DEBUG_CONTEXT_WINDOW")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .and_then(std::num::NonZeroU64::new);
    let baseline_context_window = std::num::NonZeroU64::new(sampling_config.context_window)
        .unwrap_or_else(|| {
            std::num::NonZeroU64::new(DEFAULT_CONTEXT_WINDOW)
                .expect("DEFAULT_CONTEXT_WINDOW is non-zero")
        });
    if let Some(cw) = context_window_override {
        tracing::warn!(
            override_context_window = cw.get(),
            original_context_window = baseline_context_window.get(),
            "GROW_DEBUG_CONTEXT_WINDOW override active"
        );
    }
    let chat_state_sampling_config = sampling_types::SamplingConfig {
        base_url: sampling_config.base_url.clone(),
        model: sampling_config.model.clone(),
        output_limit: sampling_config.output_limit,
        temperature: sampling_config.temperature,
        top_p: sampling_config.top_p,
        api_backend: sampling_config.api_backend.clone(),
        extra_headers: sampling_config.extra_headers.clone(),
        query_params: sampling_config.query_params.clone(),
        env_http_headers: sampling_config.env_http_headers.clone(),
        context_window: context_window_override.unwrap_or(baseline_context_window),
        reasoning_effort: sampling_config.reasoning_effort,
        stream_tool_calls: Some(sampling_config.stream_tool_calls),
    };
    let (chat_state_event_tx, chat_state_event_rx) = mpsc::unbounded_channel();
    let timeline_persistence = Box::new(
        super::timeline_persistence::ChannelTimelinePersistence::new(persistence.tx.clone()),
    );
    let chat_state_handle = if let Some(timeline) = validated_timeline {
        chat_state::ChatStateActor::spawn_from_validated_timeline(
            timeline,
            chat_state_sampling_config,
            timeline_persistence,
            chat_state_event_tx,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .map_err(|error| {
            agent::AgentBuildError::InvalidConfig(format!(
                "invalid persisted conversation timeline: {error}"
            ))
        })?
    } else {
        chat_state::ChatStateActor::spawn(
            conversation.clone(),
            chat_state_sampling_config,
            timeline_persistence,
            chat_state_event_tx,
            tokio_util::sync::CancellationToken::new(),
        )
    };
    let workflow_alpha_test_key = credentials.alpha_test_key.clone();
    chat_state_handle.update_credentials(credentials);
    let state = TokioMutex::new(AdmissionState {
        foreground: ForegroundState::Idle,
        termination: TerminationState::Open,
        pending_inputs: VecDeque::new(),
        combine_edit_holds: std::collections::HashSet::new(),
        notifications_suppressed: false,
        rewindable: false,
        nudges_used_this_session: 0,
        recent_terminals: VecDeque::new(),
        pending_manual_compact: None,
        pending_step_controls: PendingStepControls::default(),
        applying_step_control: None,
        behavior_control_revision: 0,
        pending_behavior_control: None,
        applying_behavior_control: None,
        behavior_control_worker_active: false,
        behavior_control_foreground_claimed: false,
        control_intents: restored_control_intents,
        terminal_preemption_pending: false,
    });
    let mcp_strategy = match std::env::var("MCP_INIT_STRATEGY") {
        Ok(v) if !v.trim().is_empty() => McpInitStrategy::from(v),
        _ if startup_hints.non_interactive => McpInitStrategy::Blocking,
        _ => McpInitStrategy::Progressive,
    };
    let file_state_tracker = Arc::new(match rewind_points_source {
        Some(source) => FileStateTracker::with_lazy_source(source),
        None => FileStateTracker::new(),
    });
    let file_state_handle = FileStateHandle::new(file_state_tracker.clone());
    let mut tool_context = tool_context.with_file_state_handle(file_state_handle);
    let index_root_for_session =
        workspace::session::git::find_git_root_from_path(tool_context.cwd.as_path())
            .unwrap_or_else(|_| tool_context.cwd.to_path_buf());
    let chat_state_handle_for_handle = chat_state_handle.clone();
    let hunk_tracker_handle_for_bridge = tool_context.hunk_tracker_handle.clone();
    let hunk_tracker_handle = tool_context.hunk_tracker_handle.clone();
    let prompt_index_for_bridge = tool_context.prompt_index.clone();
    // Goal state is durable user data, independent of whether the current
    // harness can drive it. Restore it first; capability reconciliation below
    // pauses an active Goal when its runtime is unavailable instead of
    // silently deleting the objective. Invalid payloads were already rejected
    // by the Control loader and must never degrade into an empty Goal here.
    let restored_goal_tracker = match persisted_goal_mode {
        Some(goal) => Some(
            crate::session::goal_tracker::GoalTracker::from_snapshot(goal).ok_or_else(|| {
                agent::AgentBuildError::InvalidConfig(
                    "invalid persisted Goal control payload".to_string(),
                )
            })?,
        ),
        None => None,
    };
    let goal_was_restored = restored_goal_tracker.is_some();
    let goal_tracker = Arc::new(parking_lot::Mutex::new(
        restored_goal_tracker.unwrap_or_default(),
    ));
    let goal_usage_window = startup_hints.goal_usage_window.clone().unwrap_or_else(|| {
        let active_goal_id = goal_tracker.lock().snapshot().and_then(|goal| {
            (goal.status == crate::session::goal_tracker::GoalStatus::Active)
                .then(|| goal.goal_id.clone())
        });
        super::goal_support::GoalUsageWindow::new(cmd_tx.clone(), active_goal_id)
    });
    let persisted_behavior = persisted_behavior
        .map(|snapshot| {
            let valid = snapshot.runtime_fields_match_selection()
                && match &snapshot.state {
                    crate::session::behavior::BehaviorState::Plan(phase) => {
                        restored_plan_artifact_is_valid(&session_directory, &snapshot, *phase)
                    }
                    _ => true,
                };
            if valid {
                Ok(snapshot)
            } else {
                Err(agent::AgentBuildError::InvalidConfig(format!(
                    "invalid persisted Behavior control state: {:?}",
                    snapshot.state
                )))
            }
        })
        .transpose()?;
    let behavior = {
        let tracker = if let Some(snapshot) = persisted_behavior {
            crate::session::behavior::BehaviorCoordinator::from_snapshot(snapshot)
        } else {
            crate::session::behavior::BehaviorCoordinator::new()
        };
        Arc::new(parking_lot::Mutex::new(tracker))
    };
    let task_output_tool_name = Arc::new(std::sync::OnceLock::new());
    let read_tool_name = Arc::new(std::sync::OnceLock::new());
    let tools_notification_handle = crate::tools::notification_bridge::spawn_notification_bridge(
        crate::tools::notification_bridge::NotificationBridgeConfig {
            gateway: gateway.clone(),
            session_id: session_info.id.clone(),
            hunk_tracker_handle: hunk_tracker_handle_for_bridge.clone(),
            file_state_tracker: file_state_tracker.clone(),
            prompt_index: prompt_index_for_bridge,
            cwd: tool_context.cwd.as_path().to_path_buf(),
            gateway_enabled: gateway_enabled.clone(),
            persistence: persistence.clone(),
            incremental_bash_output,
            behavior: behavior.clone(),
            session_cmd_tx: cmd_tx.clone(),
            task_output_tool_name: task_output_tool_name.clone(),
            read_tool_name: read_tool_name.clone(),
        },
    );
    let tool_context_for_handle = tool_context.clone();
    let terminal_backend_kind = select_terminal_backend_kind(
        startup_hints.is_subagent,
        parent_terminal_backend.is_some(),
        client_terminal_capable,
        tool_context.gateway.is_some(),
    );
    let effective_cfg = matches!(
        terminal_backend_kind,
        TerminalBackendKind::LocalNonPersistent
    )
    .then(crate::config::load_effective_config)
    .and_then(Result::ok);
    let resolve_search_shadows = || {
        let (find_bfs, grep_ugrep) =
            crate::util::config::resolve_search_tools_enabled(effective_cfg.as_ref());
        tools::computer::local::SearchShadowConfig {
            find_bfs,
            grep_ugrep,
        }
    };
    let resolve_policy = || crate::util::config::resolve_shell_env_policy(effective_cfg.as_ref());
    let terminal_backend: std::sync::Arc<dyn tools::computer::types::TerminalBackend> =
        match terminal_backend_kind {
            TerminalBackendKind::ReuseParent => parent_terminal_backend
                .expect("ReuseParent is only selected when a parent backend is present"),
            TerminalBackendKind::AcpClient => {
                std::sync::Arc::new(crate::terminal::AcpTerminalAdapter::new(
                    tool_context.gateway.clone().unwrap(),
                    tool_context.session_id.clone().unwrap(),
                )) as std::sync::Arc<dyn tools::computer::types::TerminalBackend>
            }
            TerminalBackendKind::LocalNonPersistent => {
                let login_shell_capture =
                    crate::util::config::resolve_login_shell_capture(effective_cfg.as_ref());
                std::sync::Arc::new(LocalTerminalBackend::new_local_with_login_shell_capture(
                    resolve_search_shadows(),
                    login_shell_capture,
                    resolve_policy(),
                    tool_context.process_scope.clone(),
                ))
            }
        };
    if matches!(
        terminal_backend_kind,
        TerminalBackendKind::LocalNonPersistent
    ) {
        terminal_backend
            .warm_shell(tool_context.cwd.as_path())
            .await;
    }
    let fs_backend: std::sync::Arc<dyn tools::computer::types::AsyncFileSystem> =
        if client_fs_capable && tool_context.gateway.is_some() {
            std::sync::Arc::new(workspace::file_system::AcpFsAdapter::new(
                tool_context.gateway.clone().unwrap(),
                tool_context.session_id.clone().unwrap(),
            ))
        } else {
            std::sync::Arc::new(tools::computer::local::LocalFs)
        };
    let resources_persistence =
        crate::session::storage::resources_persistence(session_directory.clone());
    let agent_selector = agent_definition.selector_identity();
    let agent_profile = crate::session::handle::SessionAgentProfile::new(
        agent_selector.clone(),
        agent_definition.subagent_filter(),
    );
    let harness_metrics = {
        let plugin_names = plugin_registry
            .as_ref()
            .map(|reg| {
                reg.active_plugins()
                    .iter()
                    .map(|p| p.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        Some(super::diagnostics::SessionHarnessMetrics {
            session_id: session_info.id.0.to_string(),
            client_identifier: session_client_identifier.clone(),
            model_id: session_model_id.0.to_string(),
            agent_name: agent_selector,
            permission_mode: if session_permission_mode.is_auto()
                && !crate::util::config::auto_permission_mode_enabled_from_disk()
            {
                ::diagnostics::enums::PermissionMode::Ask
            } else {
                session_permission_mode
            },
            mcp_server_names: mcp_servers
                .iter()
                .map(|s| mcp_server_name(s).to_owned())
                .collect(),
            lsp_server_names: tool_context.lsp_server_names.clone(),
            memory_enabled: memory_config.is_some(),
            auto_update,
            cwd: tool_context.cwd.as_str().to_owned(),
            skills_config: skills_config.clone(),
            plugin_registry: plugin_registry.clone(),
            plugin_names,
        })
    };
    let memory_flush_before_compaction = memory_config.as_ref().is_some_and(|mc| mc.flush.enabled);
    let compaction_wall_clock_budget_secs =
        crate::util::config::resolve_compaction_wall_clock_budget_secs(
            remote_settings
                .as_ref()
                .and_then(|r| r.compaction_wall_clock_budget_secs),
        );
    let (user_question_tx, user_question_rx) = tokio::sync::mpsc::unbounded_channel::<
        tools::implementations::grow_build::ask_user_question::types::UserQuestionRequest,
    >();
    let memory_storage_for_session = memory_config.as_ref().filter(|mc| mc.enabled).map(|mc| {
        if mc.flat_memory_root
            && let Some(ref root) = mc.root_dir_override
        {
            return memory::MemoryStorage::new_flat(tool_context.cwd.as_path(), root);
        }
        memory::MemoryStorage::new(tool_context.cwd.as_path(), mc.root_dir_override.as_deref())
    });
    let memory_initial_injection_config = memory_config
        .as_ref()
        .map_or_else(Default::default, |mc| mc.initial_injection.clone());
    let mut memory_backend_params_for_session: Option<memory::MemoryBackendParams> = None;
    let mut memory_search_counter: Option<std::sync::Arc<std::sync::atomic::AtomicU64>> = None;
    let memory_backend_for_spec: Option<
        std::sync::Arc<dyn tools::types::memory_backend::MemoryBackend>,
    > = if let Some(ref storage) = memory_storage_for_session {
        if let Err(e) = storage.ensure_initialized() {
            tracing::warn!(
                target: ::diagnostics::memory_log::TARGET,
                error = %e,
                "MEMORY_INIT: ensure_initialized failed, continuing without template files"
            );
        }
        let watcher_config = memory_config
            .as_ref()
            .map(|mc| &mc.watcher)
            .cloned()
            .unwrap_or_default();
        let watcher = if watcher_config.enabled {
            memory::watcher::MemoryFileWatcher::start(storage.global_dir()).map(std::sync::Arc::new)
        } else {
            None
        };
        let embed_credentials = memory::EndpointScopedCredentials::none();
        let params = memory::MemoryBackendParams {
            session_id: session_info.id.to_string(),
            embed_config: memory_config.as_ref().map(|mc| mc.embedding.clone()),
            embed_base_url: embed_base_url.clone(),
            embed_api_key: embed_api_key.clone(),
            search_config: memory_config
                .as_ref()
                .map_or_else(Default::default, |mc| mc.search.clone()),
            watcher,
            stale_claim_secs: watcher_config.stale_claim_secs,
            search_source: "tool",
            embedding_credentials: embed_credentials,
        };
        let backend = memory::MemoryBackendImpl::from_session_params(storage.clone(), &params);
        memory_search_counter = Some(backend.search_counter.clone());
        let watcher_started = params.watcher.is_some();
        let backend: std::sync::Arc<dyn tools::types::memory_backend::MemoryBackend> =
            std::sync::Arc::new(backend);
        memory_backend_params_for_session = Some(params);
        if watcher_config.enabled && !watcher_started {
            tracing::warn!(
                target: ::diagnostics::memory_log::TARGET,
                "MEMORY_INIT: watcher was configured but failed to start \
                 (directory may not exist or OS watcher unavailable)"
            );
        }
        tracing::info!(
            target: ::diagnostics::memory_log::TARGET,
            workspace = %storage.workspace_dir().display(),
            global = %storage.global_dir().display(),
            watcher_config_enabled = watcher_config.enabled,
            watcher_started,
            "MEMORY_INIT: storage + backend created"
        );
        let mc = memory_config.as_ref();
        let total_chunks = storage.total_chunk_count();
        ::diagnostics::session_ctx::log_event(::diagnostics::memory_events::MemorySessionInit {
            session_id: session_info.id.to_string(),
            memory_enabled: true,
            watcher_config_enabled: watcher_config.enabled,
            watcher_started,
            temporal_decay_enabled: mc.is_none_or(|c| c.search.temporal_decay.enabled),
            mmr_enabled: mc.is_some_and(|c| c.search.mmr.enabled),
            mmr_lambda: mc.map_or(0.7, |c| c.search.mmr.lambda),
            half_life_days: mc.map_or(30.0, |c| c.search.temporal_decay.half_life_days),
            embedding_dimensions: mc.map_or(1024, |c| c.embedding.dimensions),
            total_chunks,
            total_files: storage.list_memory_files().map_or(0, |f| f.len()),
            has_global_memory_md: storage.global_memory_file().exists(),
            has_workspace_memory_md: storage.workspace_memory_file().exists(),
        });
        Some(backend)
    } else {
        tracing::debug!(
            target: ::diagnostics::memory_log::TARGET,
            "MEMORY_INIT: memory disabled, no storage created"
        );
        None
    };
    let context_window_tokens = context_window_override
        .map(|c| c.get())
        .unwrap_or(sampling_config.context_window);
    let inherited_mcp_eligibility = parent_mcp_pool
        .as_ref()
        .map(crate::session::mcp_servers::SharedMcpPool::eligibility);
    let mcp_state = {
        let mut state = McpState::new_with_meta(mcp_servers.clone(), mcp_meta_config_map);
        if let Some(ref pool) = parent_mcp_pool {
            state.import_shared_clients(pool);
            tracing::info!(
                session_id = %session_info.id.0,
                shared_clients = state.shared_clients.len(),
                "Imported shared MCP clients from parent pool"
            );
        }
        if !acp_mcp_servers.is_empty() {
            let invoker = std::sync::Arc::new(crate::session::acp_mcp::GatewayAcpInvoker::new(
                gateway.clone(),
            ));
            let acp_server_count = acp_mcp_servers.len();
            state.set_acp_servers(acp_mcp_servers, invoker);
            tracing::info!(
                session_id = %session_info.id.0,
                acp_mcp_servers = acp_server_count,
                "Registered in-process SDK MCP servers (grow/mcp/sdk_call)"
            );
        }
        Arc::new(TokioMutex::new(state))
    };
    let tool_metadata_snapshot = Arc::new(std::sync::Mutex::new(Default::default()));
    let (context_recall_backend, context_recall_receiver) =
        crate::session::actor::context_recall::context_recall_channel();
    let shared_plugin_registry =
        std::sync::Arc::new(parking_lot::RwLock::new(plugin_registry.clone()));
    let frozen_workflow_skills = startup_hints
        .workflow_run_id
        .as_ref()
        .and(preloaded_skills.clone());
    let rebuild_spec = std::sync::Arc::new(crate::session::agent_rebuild::AgentRebuildSpec {
        working_directory: tool_context.cwd.as_path().to_path_buf(),
        terminal_backend: terminal_backend.clone(),
        fs_backend: fs_backend.clone(),
        tools_notification_handle: tools_notification_handle.clone(),
        resources_persistence: resources_persistence.clone(),
        resource_domain: std::sync::Arc::new(tools::registry::types::SessionResourceDomain::new()),
        session_env: tool_context.session_env.clone(),
        models_manager: models_manager.clone(),
        memory_runtime: std::sync::Arc::new(parking_lot::RwLock::new(memory_backend_for_spec)),
        context_recall_backend,
        coordination_backend: parking_lot::RwLock::new(None),
        web_fetch_config: web_fetch_config.clone(),
        app_builder_deployer_config: app_builder_deployer_config.clone(),
        write_file_enabled,
        subagents_enabled,
        subagent_toggle: subagent_toggle.clone(),
        background_workflows_enabled,
        ask_user_question_enabled,
        prompt_audience,
        skills_config: skills_config.clone(),
        context_window_tokens: Arc::new(std::sync::atomic::AtomicU64::new(context_window_tokens)),
        prompt_working_directory: prompt_display_cwd.clone(),
        lsp: tool_context.lsp.clone(),
        plugin_registry: shared_plugin_registry.clone(),
        preloaded_subagent_names: frozen_subagent_names,
        frozen_skills: frozen_workflow_skills,
        file_tool_overrides: workflow_file_tool_overrides.clone(),
        tool_params_json: tool_params_json.clone(),
        subagent_event_tx: tool_context.subagent_event_tx.clone(),
        user_question_tx: user_question_tx.clone(),
        subagent_depth: tool_context.subagent_depth,
        subagents_max_depth,
        initial_subagent_capability_mode: startup_hints
            .is_subagent
            .then_some(agent_definition.capability_mode.unwrap_or_default()),
        workflow_owned: startup_hints.workflow_run_id.is_some(),
        session_id_str: session_info.id.0.to_string(),
        blocking_wait_depth: tool_context.blocking_wait_depth.clone(),
        respect_gitignore,
        path_not_found_hints,
        is_non_interactive: startup_hints.non_interactive,
        system_prompt_label,
        owner_session_id: Some(session_info.id.0.to_string()),
        parent_scheduler_handle: if startup_hints.is_subagent {
            parent_scheduler_handle
        } else {
            None
        },
    });
    let pending_interactions: crate::session::pending_interaction::PendingInteractions =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let agent = rebuild_spec
        .build_agent_with_initial_overrides(
            agent_definition,
            persisted_announcement_state
                .as_ref()
                .filter(|s| !s.announced_skill_names.is_empty())
                .map(|s| s.announced_skill_names.clone()),
            preloaded_skills,
        )
        .await
        .map_err(|e| {
            tracing::error!(
                session_id = %session_info.id.0,
                error = %e,
                "Agent building failed, please check your config"
            );
            e
        })?;
    let (subagent_capabilities, delegable_capability_ceiling) = if startup_hints.is_subagent {
        let initial_mode = agent.definition().capability_mode.unwrap_or_default();
        let bound_mcp_client_ids = mcp_state.lock().await.shared_client_ids();
        let state = crate::session::subagent_capability::SubagentCapabilityState::from_bridge(
            agent.tool_bridge(),
            agent
                .definition()
                .authored_capability_tools
                .as_ref()
                .unwrap_or(&agent.definition().tool_config),
            initial_mode,
            inherited_mcp_eligibility,
            bound_mcp_client_ids.clone(),
        )
        .await;
        (
            Some(state),
            Some(
                crate::session::subagent_capability::DelegableCapabilityCeiling::new(
                    initial_mode,
                    bound_mcp_client_ids,
                ),
            ),
        )
    } else {
        (None, None)
    };
    let selected_behavior = behavior.lock().behavior();
    let selected_behavior_unavailable = match selected_behavior {
        tool_types::BehaviorId::Plan => agent
            .tool_bridge()
            .tool_for_kind(tools::types::tool::ToolKind::PlanControl)
            .await
            .is_none(),
        // Workflow support belongs to the Shell-owned runtime. Do not infer
        // it from this Agent definition's authored tool list; only a disabled
        // runtime or a subagent session can make the restored choice invalid.
        tool_types::BehaviorId::Workflow => {
            !background_workflows_enabled || startup_hints.is_subagent
        }
        tool_types::BehaviorId::Goal => false,
        tool_types::BehaviorId::Clarify | tool_types::BehaviorId::Normal => false,
    };
    let mut behavior_normalized = selected_behavior_unavailable;
    if selected_behavior_unavailable {
        behavior
            .lock()
            .select_behavior(tool_types::BehaviorId::Normal);
    }
    let resolved_task_output =
        tools::reminders::task_completion::resolve_task_output_tool_name(agent.tool_bridge()).await;
    let resolved_read =
        tools::reminders::task_completion::resolve_read_tool_name(agent.tool_bridge()).await;
    let _ = task_output_tool_name.set(resolved_task_output.clone());
    let _ = read_tool_name.set(resolved_read);
    tool_context.task_output_tool_name = resolved_task_output;
    let scheduler_handle_for_handle = {
        let toolset = agent.tool_bridge().toolset();
        let res = toolset.resources.lock().await;
        res.get::<tools::implementations::grow_build::scheduler::types::SchedulerHandle>()
            .cloned()
    };
    if let Err(e) = workspace_ops
        .bind_local_session(
            &session_info.id.0,
            tool_context.cwd.as_path().to_path_buf(),
            tool_context.hunk_tracker_handle.clone(),
            agent.tool_bridge().toolset(),
            None,
        )
        .await
    {
        tracing::warn!(error = %e, "failed to bind local session toolset");
    }
    let system_prompt = agent.system_prompt().to_string();
    let mut initial_context_changed = false;
    if resumed_timeline.is_some()
        && !startup_hints.preserve_inherited_system
        && !conversation_has_project_instructions(&conversation)
        && let Some(agents_md_reminder) = agent.agents_md_user_reminder()
    {
        let insert_at = conversation.len().min(1);
        conversation.insert(
            insert_at,
            ConversationItem::project_instructions(agents_md_reminder),
        );
        if let Some(ref mut len) = startup_hints.inherited_prefix_len {
            *len += 1;
        }
        initial_context_changed = true;
    }
    if let Some(section) = agent.agents_md_section()
        && should_set_classifier_project_instructions(
            owns_permission_manager,
            Some(section.as_str()),
        )
    {
        let body = agents_md_classifier_body(&section);
        if !body.is_empty() {
            permissions.set_project_instructions(Some(body));
        }
    }
    if initial_context_changed {
        let (_, source_surface_revision) = chat_state_handle
            .get_conversation_with_revision()
            .await
            .ok_or_else(|| {
                agent::AgentBuildError::InvalidConfig(
                    "chat state actor stopped before initial context commit".into(),
                )
            })?;
        chat_state_handle
            .replace_context_durably(conversation, source_surface_revision)
            .await
            .map_err(|error| {
                agent::AgentBuildError::InvalidConfig(format!(
                    "initial context was not durably recorded: {error}"
                ))
            })?;
    }
    let (signals_handle, signals_actor) = crate::session::signals::SessionSignalsActor::new();
    tokio::spawn(signals_actor.run());
    if let Some(persisted) = persisted_signals {
        signals_handle.restore_signals(persisted);
    } else {
        signals_handle.seed_counts(
            initial_user_count,
            initial_assistant_count,
            initial_tool_call_count,
            initial_tools_used,
            initial_models_used,
        );
    }
    signals_handle.set_primary_model(&primary_model_id);
    signals_handle.set_tracing_config(inference_idle_timeout_secs);
    let force_compact = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let resolved_workspace_root =
        workspace::session::git::find_git_root_from_path(std::path::Path::new(&session_info.cwd))
            .ok()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| session_info.cwd.clone());
    let current_prompt_id = std::sync::Arc::new(std::sync::Mutex::new(None));
    let permissions_for_handle = permissions.clone();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<SessionEvent>();
    let mut sampler_config_initial = sampling_config.clone();
    sampler_config_initial.idle_timeout_secs = Some(inference_idle_timeout_secs);
    let task_output_budgeted = tool_context.task_output_token_budget.is_some();
    let retry_only_before_output =
        task_output_budgeted || tool_context.sampler_retry_only_before_output;
    if retry_only_before_output {
        sampler_config_initial.doom_loop_recovery = None;
    }
    let sampler_retry_policy = sampler::RetryPolicy {
        max_retries: max_retries.unwrap_or(5),
        rate_limit_retry_threshold: 2,
        retry_only_before_output,
    };
    let (sampler_event_tx, sampler_event_rx) =
        tokio::sync::mpsc::unbounded_channel::<sampler::SamplingEvent>();
    let sampler_owner = sampler::SamplerActor::spawn_owned(
        sampler_config_initial,
        sampler_retry_policy,
        sampler_event_tx,
    );
    let sampler_handle = sampler_owner.handle();
    let mut hook_discovery_errors: Vec<::hooks::error::HookError> = Vec::new();
    let built_hook_registry: Option<Arc<::hooks::discovery::HookRegistry>> =
        if let Some(override_reg) = hook_registry_override {
            Some(override_reg)
        } else {
            let cwd_path = std::path::Path::new(&session_info.cwd);
            let project_trusted = crate::agent::folder_trust::resolve_and_record(
                cwd_path,
                remote_settings.as_ref(),
                false,
            );
            let git_root = workspace::session::git::find_git_root_from_path(cwd_path).ok();
            let (registry, errors) =
                crate::util::hooks::discover_hooks(git_root.as_deref(), project_trusted);
            for e in &errors {
                tracing::warn!(error = ?e, "hook loading error");
            }
            hook_discovery_errors = errors;
            if registry.is_empty() {
                None
            } else {
                tracing::info!(hook_count = registry.len(), "loaded hooks");
                Some(Arc::new(registry))
            }
        };
    let hook_registry_for_handle = built_hook_registry.clone();
    let workspace_ops_for_handle = workspace_ops.clone();
    #[allow(clippy::arc_with_non_send_sync)]
    let mut _hook_load_errors: Vec<String> = hook_discovery_errors
        .iter()
        .map(|e| e.to_string())
        .collect();
    let (goal_command_tx, goal_command_rx) = tokio::sync::mpsc::unbounded_channel::<
        tools::implementations::grow_build::update_goal::GoalCommand,
    >();
    let workflow_session_directory = session_directory.clone();
    let (workflow_store, workflow_snapshots) =
        crate::session::workflow::store::WorkflowRunStore::from_restored(
            Some(workflow_session_directory.clone()),
            persistence.tx.clone(),
            persisted_workflow_runs,
            resumed_timeline.as_ref(),
        );
    let restored_behavior = behavior.lock().behavior();
    let public_workflow_active = workflow_snapshots
        .iter()
        .any(|run| run.status == crate::session::workflow::tracker::WorkflowRunStatus::Active);
    let active_goal =
        goal_tracker.lock().status() == Some(crate::session::goal_tracker::GoalStatus::Active);
    let (pause_goal, reset_behavior) =
        restored_runtime_conflict_actions(restored_behavior, active_goal, public_workflow_active);
    if pause_goal {
        goal_tracker.lock().pause_with_message(
            crate::session::goal_tracker::GoalPauseReason::RuntimeUnavailable,
            "Recovered a non-terminal public Workflow alongside this Goal. Stop or finish the Workflow, then restart the Goal."
                .to_string(),
        );
        goal_usage_window.sync(None);
    }
    if reset_behavior {
        behavior
            .lock()
            .select_behavior(tool_types::BehaviorId::Normal);
        behavior_normalized = true;
    }
    let workflow_tracker = Arc::new(parking_lot::Mutex::new(
        crate::session::workflow::tracker::WorkflowTracker::from_snapshot(workflow_snapshots)
            .map_err(|error| agent::AgentBuildError::InvalidConfig(error.into()))?,
    ));
    let workflow_notify = crate::session::workflow::notify::WorkflowNotifySender::new(
        session_info.id.clone(),
        gateway.clone(),
        persistence.tx.clone(),
        workflow_store.clone(),
    );
    for state in workflow_tracker.lock().snapshot() {
        workflow_notify.emit(&state, state.elapsed_ms_floor, 0);
    }
    let mut workflow_default_sampler = sampling_config.clone();
    workflow_default_sampler.idle_timeout_secs = Some(inference_idle_timeout_secs);
    workflow_default_sampler.max_retries = max_retries;
    let workflow_agent_catalog_source =
        crate::session::workflow::tracker::WorkflowAgentCatalogSource::new(
            std::path::PathBuf::from(session_info.cwd.as_str()),
            shared_plugin_registry.clone(),
            workflow_cli_agents,
            workflow_cli_overrides,
            subagent_toggle.clone(),
            workflow_file_tool_overrides,
            agent.definition().selector_identity(),
            skills_config.clone(),
        );
    let workflow_next_run_route = crate::session::workflow::tracker::WorkflowRuntimeRoute::capture(
        session_model_id.0.to_string(),
        workflow_default_sampler,
        &models_manager,
        workflow_alpha_test_key,
        agent.definition().subagent_filter(),
    )
    .map_err(|error| agent::AgentBuildError::InvalidConfig(error.into()))?;
    let workflow_manager = Arc::new(tokio::sync::Mutex::new(
        crate::session::workflow::manager::WorkflowManager::new(
            session_info.id.0.to_string(),
            Some(workflow_session_directory.clone()),
            std::path::PathBuf::from(session_info.cwd.as_str()),
            workflow_tracker.clone(),
            workflow_store,
            workflow_notify,
            tool_context.subagent_event_tx.clone().unwrap_or_else(|| {
                tracing::warn!(
                    "workflow manager: no subagent coordinator; agent() spawns will fail"
                );
                tokio::sync::mpsc::unbounded_channel().0
            }),
            Arc::new(|name: &str, fields: &serde_json::Value, replayed: bool| {
                if !replayed {
                    tracing::info!(event = name, %fields, "workflow diagnostics");
                }
            }),
            cmd_tx.clone(),
            chat_state_handle.clone(),
            std::collections::HashMap::new(),
            workflow_next_run_route,
            workflow_agent_catalog_source,
        ),
    ));
    let (workflow_tx, mut workflow_rx) = tokio::sync::mpsc::unbounded_channel::<
        tools::implementations::grow_build::workflow::WorkflowEnvelope,
    >();
    let workflow_service_shutdown = tokio_util::sync::CancellationToken::new();
    let workflow_service_shutdown_signal = workflow_service_shutdown.clone();
    let workflow_worker = {
        let manager = workflow_manager.clone();
        let panic_manager = manager.clone();
        let behavior = behavior.clone();
        let workflow_cmd_tx = cmd_tx.clone();
        let panic_cmd_tx = workflow_cmd_tx.clone();
        let launch_cwd = std::path::PathBuf::from(session_info.cwd.as_str());
        let launch_session_directory = workflow_session_directory.clone();
        let panic_shutdown = workflow_service_shutdown_signal.clone();
        tokio::spawn(async move {
            let worker_result = std::panic::AssertUnwindSafe(async move {
            use crate::session::workflow::{registry, workspace::WorkflowWorkspace};
            use tools::implementations::grow_build::workflow::{
                WorkflowAck, WorkflowRunControl, WorkflowToolInput, WorkflowToolOutput,
            };
            loop {
                let (req, ack) = tokio::select! {
                    envelope = workflow_rx.recv() => match envelope {
                        Some(envelope) => envelope,
                        None => break,
                    },
                    _ = workflow_service_shutdown_signal.cancelled() => {
                        while let Ok((_, ack)) = workflow_rx.try_recv() {
                            reject_workflow_envelope(ack);
                        }
                        break;
                    }
                };
                if !background_workflows_enabled {
                    let _ = ack.send(WorkflowAck::Rejected {
                        code: "workflows_disabled",
                        detail: "Background workflows are disabled for this session \
                                 ([workflows] enabled = false / GROW_WORKFLOWS=0 / remote flag)."
                            .into(),
                    });
                    continue;
                }
                let admitted_behavior = req.admitted_behavior;
                let input = req.input;
                if let Err(detail) = input.validate() {
                    let _ = ack.send(WorkflowAck::Rejected {
                        code: "workflow_invalid_input",
                        detail,
                    });
                    continue;
                }
                if admitted_behavior != tool_types::BehaviorId::Workflow {
                    let _ = ack.send(WorkflowAck::Rejected {
                        code: "workflow_behavior_required",
                        detail: "Public Workflow actions require a turn admitted in Workflow behavior. Use /workflow [prompt]."
                            .into(),
                    });
                    continue;
                }
                // This is the same admission lock used by Behavior switching.
                // Keep it through every Workspace mutation and Run control so
                // the live Behavior check and the side effect are one ordered
                // operation rather than a check-then-act race.
                let mut workflow_admission = manager.lock().await;
                if let Err(error) = workflow_admission.ensure_open_for_ingress() {
                    let _ = ack.send(WorkflowAck::Rejected {
                        code: "workflow_session_shutting_down",
                        detail: error.to_string(),
                    });
                    continue;
                }
                if behavior.lock().behavior() != tool_types::BehaviorId::Workflow {
                    let _ = ack.send(WorkflowAck::Rejected {
                        code: "workflow_behavior_required",
                        detail: "Public Workflow actions require live Workflow behavior. Use /workflow [prompt]."
                            .into(),
                    });
                    continue;
                }
                let output: Result<WorkflowToolOutput, (&'static str, String)> = async {
                    let mut workspace = WorkflowWorkspace::open_in_session(
                        &launch_session_directory,
                        &launch_cwd,
                    )
                        .map_err(|error| ("workflow_workspace_failed", error.to_string()))?;
                    match input {
                        WorkflowToolInput::Search { query, limit } => {
                            let catalog = workspace.search(&launch_cwd, &query, limit.unwrap_or(10));
                            let count = catalog.definitions.len();
                            let diagnostic_count = catalog.diagnostics.len();
                            Ok(WorkflowToolOutput::Search {
                                matches: catalog.definitions,
                                diagnostics: catalog.diagnostics,
                                message: format!(
                                    "Found {count} Workflow Definition candidate(s) and {diagnostic_count} diagnostic(s). Inspect ambiguous candidates before drafting or running."
                                ),
                            })
                        }
                        WorkflowToolInput::Inspect {
                            definition_id,
                            include_source,
                        } => {
                            workspace
                                .focus(&launch_cwd, &definition_id)
                                .map_err(|error| ("workflow_focus_failed", error.to_string()))?;
                            let definition = workspace
                                .resolve(&launch_cwd, &definition_id)
                                .map_err(|error| ("workflow_resolve_failed", error.to_string()))?;
                            let source = include_source.then(|| definition.resolved.script.clone());
                            Ok(WorkflowToolOutput::Inspect {
                                definition: definition.summary,
                                source,
                                message: "Definition inspected and set as the explicit Workflow focus."
                                    .into(),
                            })
                        }
                        WorkflowToolInput::Draft { name, source } => {
                            let definition = workspace
                                .draft(
                                    &launch_cwd,
                                    name.as_deref(),
                                    source,
                                )
                                .map_err(|error| ("workflow_draft_failed", error.to_string()))?;
                            Ok(WorkflowToolOutput::Draft {
                                definition: definition.summary,
                                message: "Session draft created and focused. Edits and validation affect only this draft; existing Runs remain immutable."
                                    .into(),
                            })
                        }
                        WorkflowToolInput::Validate {
                            definition_id,
                            args,
                            agent_budget,
                        } => {
                            let definition = workspace
                                .resolve(&launch_cwd, &definition_id)
                                .map_err(|error| ("workflow_resolve_failed", error.to_string()))?;
                            let script = definition.resolved.script.clone();
                            let hash = definition.summary.content_hash.clone();
                            // Validation executes a synchronous Rhai preflight
                            // on the blocking pool. Do not hold the shared
                            // Workflow admission lock across it: teardown
                            // must be able to close the generation promptly.
                            drop(workflow_admission);
                            let report = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                tokio::task::spawn_blocking(move || {
                                    workflow::validate_script_with_agent_budget(
                                        &script,
                                        args,
                                        agent_budget.unwrap_or(workflow::DEFAULT_AGENT_BUDGET),
                                    )
                                }),
                            )
                            .await
                            .map_err(|_| {
                                (
                                    "workflow_validation_failed",
                                    "Workflow preflight timed out after 30 seconds.".into(),
                                )
                            })?
                            .map_err(|error| {
                                ("workflow_validation_failed", format!("validator panicked: {error}"))
                            })?
                            .map_err(|error| {
                                ("workflow_validation_failed", error.to_string())
                            })?;
                            let mut workflow_admission = manager.lock().await;
                            if let Err(error) = workflow_admission.ensure_open_for_ingress() {
                                return Err(("workflow_session_shutting_down", error.to_string()));
                            }
                            if behavior.lock().behavior() != tool_types::BehaviorId::Workflow {
                                return Err((
                                    "workflow_behavior_required",
                                    "Workflow behavior changed while validation was running.".into(),
                                ));
                            }
                            let current = workspace
                                .resolve(&launch_cwd, &definition_id)
                                .map_err(|error| ("workflow_resolve_failed", error.to_string()))?;
                            if current.summary.content_hash != hash {
                                return Err((
                                    "workflow_validation_stale",
                                    "Workflow Definition changed while validation was running; retry validation.".into(),
                                ));
                            }
                            workspace
                                .record_validated(&launch_cwd, &definition_id, &hash)
                                .map_err(|error| ("workflow_workspace_failed", error.to_string()))?;
                            let definition = workspace
                                .resolve(&launch_cwd, &definition_id)
                                .map_err(|error| ("workflow_resolve_failed", error.to_string()))?;
                            Ok(WorkflowToolOutput::Validated {
                                definition: definition.summary,
                                phases: report.phases,
                                summary: report.outcome_summary,
                                message: "Current Definition hash passed the canned-host preflight."
                                    .into(),
                            })
                        }
                        WorkflowToolInput::Run {
                            definition_id,
                            args,
                            max_concurrency,
                            agent_budget,
                        } => {
                            let mut workflow_admission = Some(workflow_admission);
                            let mut definition = workspace
                                .resolve(&launch_cwd, &definition_id)
                                .map_err(|error| ("workflow_resolve_failed", error.to_string()))?;
                            if !definition.summary.status.contains("validated") {
                                let script = definition.resolved.script.clone();
                                let probe_args = args.clone();
                                let definition_hash = definition.summary.content_hash.clone();
                                drop(workflow_admission.take());
                                tokio::time::timeout(
                                    std::time::Duration::from_secs(30),
                                    tokio::task::spawn_blocking(move || {
                                        workflow::validate_script_with_agent_budget(
                                            &script,
                                            probe_args,
                                            agent_budget.unwrap_or(workflow::DEFAULT_AGENT_BUDGET),
                                        )
                                    }),
                                )
                                .await
                                .map_err(|_| {
                                    (
                                        "workflow_validation_failed",
                                        "Workflow preflight timed out after 30 seconds.".into(),
                                    )
                                })?
                                .map_err(|error| {
                                    ("workflow_validation_failed", format!("validator panicked: {error}"))
                                })?
                                .map_err(|error| {
                                    ("workflow_validation_failed", error.to_string())
                                })?;
                                workflow_admission = Some(manager.lock().await);
                                if let Err(error) = workflow_admission
                                    .as_ref()
                                    .expect("Workflow admission guard restored after preflight")
                                    .ensure_open_for_ingress()
                                {
                                    return Err(("workflow_session_shutting_down", error.to_string()));
                                }
                                if behavior.lock().behavior() != tool_types::BehaviorId::Workflow {
                                    return Err((
                                        "workflow_behavior_required",
                                        "Workflow behavior changed while validation was running.".into(),
                                    ));
                                }
                                let current = workspace
                                    .resolve(&launch_cwd, &definition_id)
                                    .map_err(|error| ("workflow_resolve_failed", error.to_string()))?;
                                if current.summary.content_hash != definition_hash {
                                    return Err((
                                        "workflow_validation_stale",
                                        "Workflow Definition changed while validation was running; retry the Run.".into(),
                                    ));
                                }
                                workspace
                                    .record_validated(
                                        &launch_cwd,
                                        &definition_id,
                                        &definition.summary.content_hash,
                                    )
                                    .map_err(|error| {
                                        ("workflow_workspace_failed", error.to_string())
                                    })?;
                                definition = workspace
                                    .resolve(&launch_cwd, &definition_id)
                                    .map_err(|error| {
                                        ("workflow_resolve_failed", error.to_string())
                                    })?;
                            } else {
                                workspace
                                    .focus(&launch_cwd, &definition_id)
                                    .map_err(|error| {
                                        ("workflow_focus_failed", error.to_string())
                                    })?;
                                definition = workspace
                                    .resolve(&launch_cwd, &definition_id)
                                    .map_err(|error| {
                                        ("workflow_resolve_failed", error.to_string())
                                    })?;
                            }
                            let definition_summary = definition.summary.clone();
                            let objective = args
                                .as_ref()
                                .and_then(|value| value.get("objective"))
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_owned)
                                .unwrap_or_else(|| definition.resolved.meta.description.clone());
                            let spec = crate::session::workflow::manager::LaunchSpec {
                                objective,
                                args: args.unwrap_or(serde_json::Value::Null),
                                agent_budget,
                                max_concurrency,
                                resume_run_id: None,
                            };
                            let (run_id, outcome_rx) = workflow_admission
                                .as_mut()
                                .expect("Workflow admission guard restored before launch")
                                .launch(definition.resolved, spec)
                                .await
                                .map_err(|error| ("workflow_launch_failed", error.to_string()))?;
                            let run_handle = workflow_admission
                                .as_ref()
                                .expect("Workflow admission guard restored before inspection")
                                .tracker()
                                .lock()
                                .get(&run_id)
                                .map(|run| run.name)
                                .unwrap_or_else(|| definition_summary.name.clone());
                            let logged_run_id = run_id.clone();
                            tokio::spawn(async move {
                                if let Ok(outcome) = outcome_rx.await {
                                    tracing::info!(run_id = logged_run_id, ?outcome, "background workflow finished");
                                }
                            });
                            Ok(WorkflowToolOutput::RunStarted {
                                content_hash: definition_summary.content_hash.clone(),
                                definition: definition_summary,
                                run_id,
                                run_handle,
                                message: "Workflow Run started from an immutable Definition snapshot. Later edits affect only the next Run."
                                    .into(),
                            })
                        }
                        WorkflowToolInput::Publish {
                            definition_id,
                            scope,
                        } => {
                            let definition = workspace
                                .publish(&launch_cwd, &definition_id, scope)
                                .map_err(|error| ("workflow_publish_failed", error.to_string()))?;
                            let path = definition.summary.path.clone().unwrap_or_default();
                            let message = if definition.summary.focused {
                                format!(
                                    "Draft published atomically to {path}; the saved Definition is now focused."
                                )
                            } else {
                                format!(
                                    "Draft published atomically to {path}; a newer session draft edit was preserved and remains focused."
                                )
                            };
                            Ok(WorkflowToolOutput::Published {
                                definition: definition.summary,
                                message,
                            })
                        }
                        WorkflowToolInput::Discard { definition_id } => {
                            workspace
                                .discard(&definition_id)
                                .map_err(|error| ("workflow_discard_failed", error.to_string()))?;
                            let focused_definition = workspace
                                .catalog(&launch_cwd)
                                .definitions
                                .into_iter()
                                .find(|definition| definition.focused);
                            Ok(WorkflowToolOutput::Discarded {
                                definition_id,
                                focused_definition,
                                message: "Session Workflow draft discarded; saved Definitions and existing Runs were unchanged."
                                    .into(),
                            })
                        }
                        WorkflowToolInput::ControlRun {
                            run_id,
                            operation,
                            agent_budget,
                        } => {
                            let tracker = workflow_admission.tracker();
                            let matches: Vec<_> = tracker
                                .lock()
                                .list()
                                .iter()
                                .filter(|run| run.run_id == run_id || run.name == run_id)
                                .cloned()
                                .collect();
                            let state = match matches.as_slice() {
                                [state] => state.clone(),
                                [] => {
                                    return Err((
                                        "workflow_run_not_found",
                                        format!("No Workflow Run matches '{run_id}'"),
                                    ));
                                }
                                _ => {
                                    return Err((
                                        "workflow_run_ambiguous",
                                        format!("More than one Workflow Run matches '{run_id}'"),
                                    ));
                                }
                            };
                            let final_state = match operation {
                                WorkflowRunControl::Pause => {
                                    Some(workflow_admission.pause(&state.run_id).await.map_err(
                                        |error| ("workflow_pause_failed", error),
                                    )?)
                                }
                                WorkflowRunControl::Stop => {
                                    Some(workflow_admission.cancel(&state.run_id).await.map_err(
                                        |error| ("workflow_stop_failed", error),
                                    )?)
                                }
                                WorkflowRunControl::Resume => {
                                    let script = workflow_admission.script_copy_for(&state.run_id).ok_or((
                                        "workflow_resume_failed",
                                        "Immutable Run script is missing".into(),
                                    ))?;
                                    let args = workflow_admission.args_copy_for(&state.run_id);
                                    let mut resolved = registry::resolve_inline(script).map_err(|error| {
                                        ("workflow_resume_failed", error.to_string())
                                    })?;
                                    if let Some(definition_id) = state.definition_id.clone() {
                                        resolved.definition_id = definition_id;
                                    }
                                    if let Some(scope) = state.definition_scope {
                                        resolved.scope = scope;
                                    }
                                    if let Some(hash) = state.definition_hash.clone() {
                                        resolved.content_hash = hash;
                                    }
                                    let spec = crate::session::workflow::manager::LaunchSpec {
                                        objective: state.objective.clone(),
                                        args,
                                        agent_budget: agent_budget.or(state.agent_budget),
                                        max_concurrency: Some(state.max_concurrency),
                                        resume_run_id: Some(state.run_id.clone()),
                                    };
                                    let (resumed_id, outcome_rx) = workflow_admission
                                        .launch(resolved, spec)
                                        .await
                                        .map_err(|error| {
                                            ("workflow_resume_failed", error.to_string())
                                        })?;
                                    let logged_run_id = resumed_id.clone();
                                    tokio::spawn(async move {
                                        if let Ok(outcome) = outcome_rx.await {
                                            tracing::info!(run_id = logged_run_id, ?outcome, "resumed workflow finished");
                                        }
                                    });
                                    workflow_admission.tracker().lock().get(&resumed_id)
                                }
                            }
                            .ok_or((
                                "workflow_control_failed",
                                "Run state disappeared while applying control".into(),
                            ))?;
                            Ok(WorkflowToolOutput::RunControlled {
                                run_id: final_state.run_id,
                                run_handle: final_state.name,
                                status: final_state.status.as_str().into(),
                                definition_id: final_state.definition_id,
                                definition_scope: final_state.definition_scope,
                                content_hash: final_state.definition_hash,
                                message: format!(
                                    "Workflow Run is now {}.",
                                    final_state.status.as_str()
                                ),
                            })
                        }
                    }
                }
                .await;
                let _ = match output {
                    Ok(output) => {
                        let _ = workflow_cmd_tx
                            .send(crate::session::commands::SessionCommand::AdvertiseCommands);
                        ack.send(WorkflowAck::Completed(output))
                    }
                    Err((code, detail)) => ack.send(WorkflowAck::Rejected { code, detail }),
                };
            }
            })
            .catch_unwind()
            .await;
            if worker_result.is_err() {
                panic_manager.lock().await.close_admission();
                panic_shutdown.cancel();
                let _ = panic_cmd_tx.send(
                    crate::session::commands::SessionCommand::WorkflowTerminalFailure {
                        run_id: "workflow-worker".into(),
                        error: "Workflow ingress worker panicked; session entered fatal teardown"
                            .into(),
                    },
                );
            }
        })
    };
    let mut effective_config = crate::config::load_effective_config()
        .ok()
        .and_then(|raw| crate::agent::config::Config::new_from_toml_cfg(&raw).ok())
        .unwrap_or_default();
    effective_config.remote_settings = remote_settings.clone();
    let goal_behavior_available = goal_enabled;
    if behavior.lock().behavior() == tool_types::BehaviorId::Goal && !goal_behavior_available {
        behavior
            .lock()
            .select_behavior(tool_types::BehaviorId::Normal);
        behavior_normalized = true;
    }
    let doom_loop_recovery = effective_config.resolve_doom_loop_recovery();
    let model_route = crate::session::handle::SessionModelRoute::new(
        session_model_id.clone(),
        sampling_config.clone(),
    );
    let workflow_run_id = startup_hints.workflow_run_id.clone();
    let session = Arc::new_cyclic(|weak: &std::sync::Weak<SessionActor>| SessionActor {
        session_info: session_info.clone(),
        control_epoch: uuid::Uuid::now_v7().to_string(),
        #[cfg(test)]
        test_session_dir_guard: None,
        session_dir: session_dir.clone(),
        session_directory: session_directory.clone(),
        notification_artifact_gate: TokioMutex::new(()),
        auth_method_id,
        model_auth_memo: std::cell::RefCell::new(None),
        state,
        step_control_gate: TokioMutex::new(()),
        goal_transaction_gate: TokioMutex::new(()),
        notifications: NotificationSender {
            gateway: gateway.clone(),
            gateway_enabled: gateway_enabled.clone(),
            persistence_tx: persistence.tx.clone(),
        },
        permissions,
        tool_context,
        deny_read_globs,
        mcp_state: mcp_state.clone(),
        mcp: McpSessionState {
            strategy: mcp_strategy,
            initial_client_servers: initial_client_mcp_servers.clone(),
            tool_metadata_snapshot,
            announced_servers: Mutex::new(
                persisted_announcement_state
                    .as_ref()
                    .map(|s| {
                        crate::session::announcement_state::from_persisted_fingerprints(
                            &s.mcp_server_fingerprints,
                        )
                    })
                    .unwrap_or_default(),
            ),
            reminder_mode: McpReminderMode::from_env(),
            reminder_dirty: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            connecting_reminder_injected: std::cell::Cell::new(false),
            handshakes_done: Arc::new(tokio::sync::Notify::new()),
        },
        chat_state_handle: chat_state_handle.clone(),
        unattributed_background_usage: std::sync::atomic::AtomicBool::new(false),
        current_prompt_id: current_prompt_id.clone(),
        pending_interactions: pending_interactions.clone(),
        compactions_remaining: std::cell::Cell::new(sampling_config.compactions_remaining),
        compaction_at_tokens: std::cell::Cell::new(sampling_config.compaction_at_tokens),
        doom_loop_recovery,
        doom_loop_turn_tally: Default::default(),
        file_state_tracker,
        rewind_pending_prompt: std::sync::Mutex::new(None),
        startup_hints,
        subagent_capabilities,
        compaction: super::compaction_config::CompactionConfig {
            lease: Default::default(),
            threshold_percent: std::cell::Cell::new(auto_compact_threshold_percent),
            memory_flush_enabled: memory_flush_before_compaction,
            wall_clock_budget_secs: compaction_wall_clock_budget_secs,
            force_compact: force_compact.clone(),
            context_window_override,
            count: std::sync::atomic::AtomicU64::new(0),
            auto_compact_suppressed: std::sync::atomic::AtomicU8::new(0),
            previous_model: std::cell::Cell::new(None),
            verbatim_input: compaction_verbatim_input,
            pre_prune: std::cell::Cell::new(compaction_pre_prune),
            pre_prune_token_budget: std::cell::Cell::new(compaction_pre_prune_token_budget),
            cancel: Default::default(),
        },
        sideband_cancel: tokio_util::sync::CancellationToken::new(),
        finalizer_sideband_cancel: tokio_util::sync::CancellationToken::new(),
        sideband_repair_cancel: tokio_util::sync::CancellationToken::new(),
        durable_ui_cancel: tokio_util::sync::CancellationToken::new(),
        sideband_fail_stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        sideband_admission_gate: tokio::sync::Mutex::new(()),
        session_activities: SessionActivityTracker::new(),
        coordination_inquiries: std::cell::RefCell::new(std::collections::VecDeque::new()),
        coordination_inquiry_active: std::cell::Cell::new(false),
        mcp_dispatcher_worker: TaskSlot::new(),
        mcp_initialization_worker: TaskSlot::new(),
        project_discovery_worker: TaskSlot::new(),
        fs_watch_handle: std::cell::RefCell::new(None),
        background_service_shutdown: CancellationToken::new(),
        user_question_worker: TaskSlot::new(),
        context_recall_worker: TaskSlot::new(),
        notification_reconciliation_worker: TaskSlot::new(),
        memory_reindex_worker: TaskSlot::new(),
        step_control_worker: TaskSlot::new(),
        behavior_control_worker: TaskSlot::new(),
        memory: super::memory_state::SessionMemory {
            flush_config: memory_config.as_ref().map_or_else(
                || crate::config::MemoryFlushConfig {
                    enabled: false,
                    ..Default::default()
                },
                |mc| mc.flush.clone(),
            ),
            is_flushing: std::sync::atomic::AtomicBool::new(false),
            is_dreaming: std::sync::atomic::AtomicBool::new(false),
            last_flush_compaction: std::sync::atomic::AtomicU64::new(0),
            storage: std::cell::RefCell::new(memory_storage_for_session),
            save_on_end: memory_config
                .as_ref()
                .is_none_or(|mc| mc.session.save_on_end),
            backend_params: memory_backend_params_for_session,
            initial_injection_config: memory_initial_injection_config,
            context_injected: std::sync::atomic::AtomicBool::new(false),
            flush_count: std::sync::atomic::AtomicU64::new(0),
            last_flush_content: std::cell::RefCell::new(None),
            flush_success_count: std::sync::atomic::AtomicU64::new(0),
            flush_error_count: std::sync::atomic::AtomicU64::new(0),
            search_counter: std::cell::RefCell::new(memory_search_counter),
            injection_count: std::sync::atomic::AtomicU64::new(0),
            compaction_recovery_count: std::sync::atomic::AtomicU64::new(0),
            chunks_added: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            dream_config: memory_config
                .as_ref()
                .map_or_else(Default::default, |mc| mc.dream),
            dream_count: std::sync::atomic::AtomicU64::new(0),
            dream_success_count: std::sync::atomic::AtomicU64::new(0),
            dream_error_count: std::sync::atomic::AtomicU64::new(0),
        },
        session_start: std::time::Instant::now(),
        inference_idle_timeout: std::cell::Cell::new(Duration::from_secs(
            inference_idle_timeout_secs,
        )),
        max_turns,
        max_retries: std::cell::Cell::new(sampler::resolve_max_retries(max_retries)),
        subagent_classifier_input,
        pending_interjections: InterjectionBuffer::new(),
        completion_delivery: Default::default(),
        pending_system_reminders: Mutex::new(Vec::new()),
        idle_flush_timeout: memory_config
            .as_ref()
            .and_then(|mc| mc.flush.idle_timeout_secs)
            .map(std::time::Duration::from_secs),
        dream_check_timeout: memory_config
            .as_ref()
            .filter(|mc| mc.dream.enabled)
            .and_then(|mc| mc.dream.check_interval_secs)
            .filter(|&s| s > 0)
            .map(std::time::Duration::from_secs),
        last_idle_flush_conversation_len: std::sync::atomic::AtomicUsize::new(
            initial_conversation_len,
        ),
        event_tx,
        idle_arbiter: Arc::new(tokio::sync::Notify::new()),
        buffering_settings,
        client_identifier: session_client_identifier.clone(),
        origin_client: origin_client.clone(),
        signals_handle: signals_handle.clone(),
        agent: std::cell::RefCell::new(agent),
        agent_profile: agent_profile.clone(),
        last_reported_branch: Arc::new(Mutex::new(None)),
        git_head_enabled: fs_watch_caps.git_head,
        models_manager,
        owns_permission_manager,
        permission_audit_bridge: parking_lot::Mutex::new(None),
        display_cwd: {
            let lock = std::sync::OnceLock::new();
            if let Some(ref cwd) = prompt_display_cwd {
                let _ = lock.set(cwd.clone());
            }
            lock
        },
        model_route: model_route.clone(),
        active_skill: parking_lot::Mutex::new(None),
        turn_behavior: Arc::new(parking_lot::Mutex::new(behavior.lock().behavior())),
        behavior: behavior.clone(),
        control_revision: Arc::new(std::sync::atomic::AtomicU64::new(
            persisted_control_revision,
        )),
        goal_enabled,
        background_workflows_enabled,
        // Fail closed until the live tool bridge proves all Goal tools exist.
        goal_runtime_available: std::sync::atomic::AtomicBool::new(false),
        goal_drive: TaskSlot::new(),
        goal_tracker,
        goal_usage_window: goal_usage_window.clone(),
        goal_turn_task_ids: parking_lot::Mutex::new(std::collections::HashMap::new()),
        goal_command_rx: std::cell::RefCell::new(Some(goal_command_rx)),
        goal_command_tx,
        workflow_manager: workflow_manager.clone(),
        workflow_tx: workflow_tx.clone(),
        workflow_worker: {
            let slot = TaskSlot::new();
            slot.arm(workflow_worker);
            slot
        },
        workflow_service_shutdown,
        user_input_generation: std::sync::atomic::AtomicU64::new(0),
        laziness_debug_log: laziness_debug_log.map(|p| std::sync::Arc::from(p.as_path())),
        deferred_prefix: TaskSlot::new(),
        restored_plan_approval: TaskSlot::new(),
        idle_prompt_extension: Some(IdlePromptExtension::new(weak.clone())),
        last_announced_local_date: std::cell::Cell::new(chrono::Local::now().date_naive()),
        last_search_prompt_index: std::sync::atomic::AtomicI64::new(-1),
        last_api_request_at: std::sync::atomic::AtomicI64::new(0),
        hooks: HookSessionState {
            registry: std::cell::RefCell::new(built_hook_registry),
            client_hooks: std::cell::RefCell::new(client_hooks),
            resolved_workspace_root,
            vcs_kind: {
                let root = std::path::Path::new(&session_info.cwd);
                match workspace::session::git::discover_git_root(root) {
                    workspace::session::git::GitDiscoveryResult::Found(git_root) => {
                        workspace::session::git::detect_vcs_kind(&git_root)
                    }
                    _ => workspace::session::git::VcsKind::None,
                }
            },
            load_errors: std::cell::RefCell::new(_hook_load_errors),
        },
        plugin_registry: shared_plugin_registry.clone(),
        plugin_registry_handle,
        events: crate::session::events::EventTracker::new(chat_state_handle.clone()),
        current_turn_number: std::cell::Cell::new(0),
        last_recap_main_turn: std::cell::Cell::new(0),
        recap_in_flight: std::cell::Cell::new(false),
        recap_epoch: std::cell::Cell::new(0),
        session_turn_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        turn_stream_drained: parking_lot::Mutex::new(None),
        sampler_handle,
        sampler_owner: std::cell::RefCell::new(Some(sampler_owner)),
        sampler_event_drainer: TaskSlot::new(),
        rebuild_spec: rebuild_spec.clone(),
        image_description_model: parking_lot::RwLock::new(image_description_model),
        session_title_route: std::cell::RefCell::new(session_title_route),
        image_describe_cache: Arc::new(crate::session::image_describe::ImageDescribeCache::new()),
        workspace_ops: workspace_ops.clone(),
    });
    session.recover_pending_rewind().await.map_err(|error| {
        agent::AgentBuildError::IoError(std::io::Error::other(format!(
            "failed to recover pending rewind transaction: {error}"
        )))
    })?;
    let initialized_fresh_context = if let Some(session_rules) = fresh_session_rules {
        session
            .initialize_fresh_context_durably(system_prompt.clone(), session_rules)
            .await
            .map_err(|error| {
                agent::AgentBuildError::IoError(std::io::Error::other(format!(
                    "initial model context was not durably recorded: {error}"
                )))
            })?;
        true
    } else {
        false
    };
    if persisted_control_revision == 0 || resumed_timeline.is_some() {
        let (agent_name, role_prompt, capability_catalog) = {
            let agent = session.agent.borrow();
            (
                agent.definition().selector_identity(),
                agent.role_prompt().map(str::to_owned),
                session
                    .subagent_capabilities
                    .as_ref()
                    .map(|capabilities| capabilities.native_catalog_prompt()),
            )
        };
        session
            .persist_agent_transition_durably(
                &agent_name,
                role_prompt.as_deref(),
                capability_catalog.as_deref(),
            )
            .await
            .map_err(|error| {
                agent::AgentBuildError::IoError(std::io::Error::other(format!(
                    "initial Agent role was not durably recorded: {error}"
                )))
            })?;
    }
    session
        .repair_missing_control_contexts_durably()
        .await
        .map_err(|error| {
            agent::AgentBuildError::IoError(std::io::Error::other(format!(
                "active Control context was not restored: {error}"
            )))
        })?;
    if initialized_fresh_context {
        let prefix_session = session.clone();
        session
            .deferred_prefix
            .arm(tokio::task::spawn_local(async move {
                prefix_session.build_prefix_background().await
            }));
    }
    session
        .context_recall_worker
        .arm(crate::session::actor::context_recall::serve_context_recall(
            &session,
            context_recall_receiver,
            session.background_service_shutdown.clone(),
        ));
    // A restored Active Goal must never reach the idle arbiter until the live
    // bridge proves that every required Goal tool is actually registered.
    session.refresh_goal_runtime_availability().await;
    // Reconstruct provider admission from the complete durable Goal snapshot,
    // including whether incomplete usage has been explicitly acknowledged.
    // The initial window constructor only carries ownership across parent /
    // child wiring and cannot infer this lifecycle distinction by id alone.
    session.sync_goal_usage_window();
    session
        .reconcile_restored_plan_handoff_notification()
        .await
        .map_err(|error| {
            agent::AgentBuildError::IoError(std::io::Error::other(format!(
                "restored Plan handoff notification was not reconciled: {error}"
            )))
        })?;
    session
        .reconcile_restored_public_workflow_notifications()
        .await
        .map_err(|error| {
            agent::AgentBuildError::IoError(std::io::Error::other(format!(
                "restored Workflow terminal notification was not reconciled: {error}"
            )))
        })?;
    if behavior_normalized || goal_was_restored {
        let behavior = session.behavior.lock().snapshot();
        let goal = session.goal_tracker.lock().snapshot().cloned();
        let persisted = if behavior_normalized {
            session
                .persist_behavior_transition_durably(behavior, goal)
                .await
        } else {
            session
                .persist_control_snapshot_durably(behavior, goal)
                .await
        };
        persisted.map_err(|error| {
            agent::AgentBuildError::IoError(std::io::Error::other(format!(
                "reconciled session control state was not durably recorded: {error}"
            )))
        })?;
    }
    if goal_was_restored {
        let tokens_used = session.goal_tokens_used();
        // The durable write above also captures any fail-closed sanitization
        // performed by `from_snapshot`; do not enqueue a second, weaker copy.
        session
            .goal_notify_sender()
            .emit_goal_updated(&session.goal_tracker.lock(), tokens_used);
    }
    {
        let drainer_session = session.clone();
        let mut sampler_event_rx = sampler_event_rx;
        let drainer = tokio::task::spawn_local(async move {
            while let Some(event) = sampler_event_rx.recv().await {
                drainer_session.handle_sampling_event(event).await;
            }
            tracing::debug!("sampler event drainer exiting (channel closed)");
        });
        session.sampler_event_drainer.arm(drainer);
    }
    {
        let Some(mut goal_command_rx) = session.goal_command_rx.borrow_mut().take() else {
            unreachable!("goal_command_rx must be Some at session spawn");
        };
        let session_cmd_tx = cmd_tx.clone();
        tokio::task::spawn_local(async move {
            while let Some(command) = goal_command_rx.recv().await {
                if session_cmd_tx
                    .send(SessionCommand::GoalControl { command })
                    .is_err()
                {
                    break;
                }
            }
            tracing::debug!("goal command drainer exiting (channel closed)");
        });
    }
    {
        let snapshot = session.mcp.tool_metadata_snapshot.clone();
        let tool_index = crate::session::tool_index::Bm25ToolSearchIndex::new(snapshot);
        session
            .agent
            .borrow()
            .tool_bridge()
            .update_resource(tools::types::tool_index::ToolIndex(std::sync::Arc::new(
                tool_index,
            )))
            .await;
    }
    session.inject_deny_read_globs().await;
    // The primary may be in Ask while globally configured subagents use Auto;
    // the wiring method resolves both cases and keeps the classifier on the
    // primary model/config rather than the child's model.
    session.wire_permission_auto_llm_classifier().await;
    if let Some(mut permission_events_rx) = permission_events_rx {
        let weak_session = Arc::downgrade(&session);
        let bridge = tokio::task::spawn_local(async move {
            while let Some(event) = permission_events_rx.recv().await {
                let Some(session) = weak_session.upgrade() else {
                    break;
                };
                let audit_sequence = event.audit_sequence;
                if let Some((durable, live)) = subagent_permission_updates(event) {
                    if let Err(error) = session.send_grow_passive_notification(durable, live).await
                    {
                        tracing::error!(
                            %error,
                            "failed to durably append subagent permission audit event"
                        );
                        return Err(format!(
                            "subagent permission audit event was not durable: {error}"
                        ));
                    }
                }
                session
                    .permissions
                    .mark_audit_event_processed(audit_sequence);
            }
            tracing::debug!("permission audit bridge exiting (channel closed)");
            Ok(())
        });
        *session.permission_audit_bridge.lock() = Some(bridge);
    }
    session
        .agent
        .borrow()
        .tool_bridge()
        .update_resource(
            tools::implementations::grow_build::workflow::WorkflowHandle {
                sender: session.workflow_tx.clone(),
                admitted_behavior: session.turn_behavior.clone(),
            },
        )
        .await;
    session
        .agent
        .borrow()
        .tool_bridge()
        .update_resource(
            tools::implementations::grow_build::update_goal::GoalRuntimeHandle(
                session.goal_command_tx.clone(),
            ),
        )
        .await;
    if let Some(ref display_cwd) = prompt_display_cwd {
        session
            .agent
            .borrow()
            .tool_bridge()
            .set_display_cwd(std::path::PathBuf::from(display_cwd))
            .await;
    }
    if let Some(storage) = session.memory.storage() {
        memory::init_sqlite_vec();
        let index_config = memory_config
            .as_ref()
            .map_or_else(Default::default, |mc| mc.index.clone());
        let embed_config = memory_config
            .as_ref()
            .map(|mc| mc.embedding.clone())
            .unwrap_or_default();
        let embed_dims = embed_config.dimensions;
        let sampling_base_url = embed_base_url.clone();
        let sampling_api_key = embed_api_key.clone();
        let session_id_for_reindex = session_info.id.to_string();
        let chunks_added_counter = session.memory.chunks_added.clone();
        let gc_max_age = memory_config.as_ref().map_or(30, |mc| mc.gc.max_age_days);
        let shutdown = session.background_service_shutdown.clone();
        let worker = tokio::task::spawn_local(async move {
            let reindex_start = std::time::Instant::now();
            let reindex_storage = storage.clone();
            let reindex_shutdown = shutdown.clone();
            let embed_index_config = index_config.clone();
            let reindex = tokio::task::spawn_blocking(move || {
                match reindex_storage.gc(gc_max_age) {
                    Ok(removed) if removed > 0 => tracing::info!(
                        target: ::diagnostics::memory_log::TARGET,
                        removed,
                        "MEMORY_GC: cleaned orphaned workspace directories"
                    ),
                    Err(error) => tracing::debug!(
                        target: ::diagnostics::memory_log::TARGET,
                        %error,
                        "MEMORY_GC: failed"
                    ),
                    _ => {}
                }
                if reindex_shutdown.is_cancelled() {
                    return Ok((0, 0, 0, 0));
                }
                let db_path = reindex_storage.workspace_dir().join("index.sqlite");
                let mut index = memory::MemoryIndex::open_or_create(
                    &db_path,
                    reindex_storage.clone(),
                    index_config,
                    embed_dims,
                )
                .map_err(|error| error.to_string())?;
                let files = reindex_storage
                    .list_memory_files()
                    .map_err(|error| error.to_string())?;
                let (mut added, mut updated, mut removed) = (0, 0, 0);
                for file in &files {
                    if reindex_shutdown.is_cancelled() {
                        break;
                    }
                    let source = reindex_storage.classify_source(file);
                    if let Ok(stats) = index.reindex_file(file, source) {
                        added += stats.added;
                        updated += stats.updated;
                        removed += stats.removed;
                    }
                }
                Ok::<_, String>((files.len(), added, updated, removed))
            });
            let (file_count, total_added, total_updated, total_removed) = match reindex.await {
                Ok(Ok(stats)) => stats,
                Ok(Err(error)) => {
                    tracing::warn!(
                        target: ::diagnostics::memory_log::TARGET,
                        %error,
                        "MEMORY_REINDEX: initial reindex failed"
                    );
                    return;
                }
                Err(error) => {
                    tracing::warn!(
                        target: ::diagnostics::memory_log::TARGET,
                        %error,
                        "MEMORY_REINDEX: blocking owner failed"
                    );
                    return;
                }
            };
            if shutdown.is_cancelled() {
                return;
            }
            let embedded_count = if let Some(api_key) = sampling_api_key {
                let db_path = storage.workspace_dir().join("index.sqlite");
                match memory::MemoryIndex::open_or_create(
                    &db_path,
                    storage.clone(),
                    embed_index_config,
                    embed_dims,
                ) {
                    Ok(index) => {
                        if let Some(provider) =
                            memory::embedding::ApiEmbeddingProvider::from_session(
                                &embed_config,
                                sampling_base_url,
                                api_key,
                            )
                        {
                            tokio::select! {
                                biased;
                                _ = shutdown.cancelled() => 0,
                                embedded = memory::embed_missing_chunks(&index, &provider) => embedded,
                            }
                        } else {
                            0
                        }
                    }
                    Err(error) => {
                        tracing::warn!(
                            target: ::diagnostics::memory_log::TARGET,
                            %error,
                            "MEMORY_REINDEX: failed to reopen index for embeddings"
                        );
                        0
                    }
                }
            } else {
                0
            };
            if shutdown.is_cancelled() {
                return;
            }
            tracing::info!(
                target: ::diagnostics::memory_log::TARGET,
                files = file_count,
                "MEMORY_REINDEX: background reindex complete"
            );
            ::diagnostics::session_ctx::log_event(::diagnostics::memory_events::MemoryReindex {
                session_id: session_id_for_reindex,
                source: "init".to_owned(),
                added: total_added,
                updated: total_updated,
                removed: total_removed,
                embedded: embedded_count,
                duration_ms: reindex_start.elapsed().as_millis() as u64,
                trigger: "init".to_owned(),
            });
            chunks_added_counter
                .fetch_add(total_added as u64, std::sync::atomic::Ordering::Relaxed);
        });
        session.memory_reindex_worker.arm(worker);
    }
    {
        use acp_transport::AcpClientHandler as _;
        use tools::implementations::grow_build::ask_user_question::{
            AskUserQuestionExtRequest, AskUserQuestionExtResponse, UserQuestionError,
            UserQuestionResponse,
        };
        let gateway = session.notifications.gateway.clone();
        let session_id = session.session_info.id.clone();
        let behavior = session.behavior.clone();
        let pending_interactions = session.pending_interactions.clone();
        let weak_session = Arc::downgrade(&session);
        let shutdown = session.background_service_shutdown.clone();
        let mut user_question_rx = user_question_rx;
        let worker = tokio::task::spawn_local(async move {
            loop {
                let mut request = tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break,
                    request = user_question_rx.recv() => {
                        let Some(request) = request else { break };
                        request
                    }
                };
                use tools::implementations::grow_build::ask_user_question::AskUserQuestionMode;
                let mode = match behavior.lock().behavior() {
                    tool_types::BehaviorId::Plan => AskUserQuestionMode::Plan,
                    _ => AskUserQuestionMode::Default,
                };
                let ext_req = AskUserQuestionExtRequest {
                    session_id: session_id.0.to_string(),
                    tool_call_id: request.tool_call_id.clone(),
                    questions: request.questions.clone(),
                    mode,
                };
                debug_assert!(
                    !ext_req.session_id.is_empty(),
                    "ask_user_question reverse-request must carry a non-empty sessionId (design §5.4)"
                );
                let ext_request = agent_client_protocol::schema::v1::ExtRequest::new(
                    "grow/ask_user_question",
                    serde_json::value::to_raw_value(&ext_req)
                        .expect("AskUserQuestionExtRequest serialization should not fail")
                        .into(),
                );
                let questions_for_response = request.questions.clone();
                let tool_call_id = request.tool_call_id.clone();
                let _pending_guard =
                    crate::session::pending_interaction::PendingInteractionGuard::new(
                        pending_interactions.clone(),
                        gateway.clone(),
                        session_id.clone(),
                        tool_call_id.clone(),
                        crate::session::pending_interaction::PendingKind::Question,
                    );
                let hook_completed = if let Some(session) = weak_session.upgrade() {
                    tokio::select! {
                        biased;
                        _ = shutdown.cancelled() => false,
                        () = request.result_tx.closed() => false,
                        _ = session.dispatch_notification_hook(
                            "elicitation_dialog",
                            Some("User question requested".into()),
                            None,
                            Some("info".into()),
                        ) => true,
                    }
                } else {
                    false
                };
                if !hook_completed {
                    let _ = request.result_tx.send(Ok(UserQuestionResponse::Cancelled));
                    break;
                }
                let result = tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => {
                        Ok(UserQuestionResponse::Cancelled)
                    }
                    () = request.result_tx.closed() => {
                        tracing::info!(
                            %tool_call_id,
                            "ask_user_question tool receiver closed (timeout or cancel); abandoning ACP wait"
                        );
                        Ok(UserQuestionResponse::Cancelled)
                    }
                    acp_result = gateway.ext_method(ext_request) => {
                        match acp_result {
                            Ok(raw) => {
                                match serde_json::from_str::<AskUserQuestionExtResponse>(
                                    raw.0.get(),
                                ) {
                                    Ok(typed) => {
                                        Ok(typed.into_response(questions_for_response))
                                    }
                                    Err(e) => Err(UserQuestionError::MalformedResponse(
                                        e.to_string(),
                                    )),
                                }
                            }
                            Err(e) => Err(UserQuestionError::TransportError(e.to_string())),
                        }
                    }
                };
                let _ = request.result_tx.send(result);
                if shutdown.is_cancelled() {
                    break;
                }
            }
        });
        session.user_question_worker.arm(worker);
    }
    let (session_done_tx, session_done_rx) = tokio::sync::oneshot::channel::<()>();
    let diagnostics_ctx = ::diagnostics::session_ctx::DiagnosticCtx::new(
        session.session_info.id.0.to_string(),
        session.tool_context.prompt_index.clone(),
    );
    if let Some(metrics) = harness_metrics {
        let hooks: Vec<super::diagnostics::HookRegInfo> = session
            .hooks
            .registry
            .borrow()
            .as_ref()
            .map(|reg| {
                reg.all_hooks()
                    .iter()
                    .map(|s| super::diagnostics::HookRegInfo::from_spec(s))
                    .collect()
            })
            .unwrap_or_default();
        tokio::spawn(async move {
            let ev = metrics.into_event(hooks).await;
            ::diagnostics::session_ctx::log_event(ev);
        });
    }
    if session.goal_tracker.lock().status()
        == Some(crate::session::goal_tracker::GoalStatus::Active)
    {
        session.idle_arbiter.notify_one();
    }
    {
        // The persisted adapter already owns the cross-process writer epoch.
        // Reconcile in bounded background batches so maintenance never extends
        // the session spawn critical path; the actor-local gate serializes each
        // batch with live notification admission and resolution. The worker is
        // Session-owned so payload deletion cannot cross the final frontier.
        let weak_session = Arc::downgrade(&session);
        let shutdown = session.background_service_shutdown.clone();
        let worker = tokio::task::spawn_local(async move {
            let Some(session) = weak_session.upgrade() else {
                return;
            };
            session.reconcile_notification_payloads(&shutdown).await;
        });
        session.notification_reconciliation_worker.arm(worker);
    }
    tokio::task::spawn_local(async move {
        ::diagnostics::session_ctx::with_session_ctx(
            diagnostics_ctx,
            run_session(
                session,
                cmd_rx,
                chat_state_event_rx,
                event_rx,
                fs_notify_config,
                codebase_indexes,
                index_root_for_session,
                fs_watch_caps,
            ),
        )
        .await;
        let _ = session_done_tx.send(());
    });
    Ok((
        SessionHandle {
            lifecycle_owner: std::sync::Arc::new(
                crate::session::handle::SessionLifecycleOwner::new(&cmd_tx),
            ),
            cmd_tx,
            goal_usage_window,
            persistence_tx: persistence.tx.clone(),
            current_prompt_id,
            pending_interactions,
            info: session_info,
            max_turns,
            permission_prompt_timeout,
            hunk_tracker_handle,
            chat_state_handle: chat_state_handle_for_handle,
            signals_handle,
            gateway_enabled,
            mcp_servers,
            initial_client_mcp_servers,
            display_cwd: None,
            tool_context: tool_context_for_handle,
            model_route,
            permission_mode: session_permission_mode,
            origin_client: origin_client.clone(),
            code_nav_enabled,
            ask_user_question_enabled,
            behavior: behavior.clone(),
            workflow_tracker,
            force_compact,
            permission_handle: permissions_for_handle,
            delegable_capability_ceiling,
            agent_profile,
            workflow_run_id,
            plugin_registry: shared_plugin_registry.clone(),
            hook_registry: hook_registry_for_handle,
            workspace_ops: workspace_ops_for_handle,
            terminal_backend: Some(terminal_backend.clone()),
            tools_notification_handle: Some(tools_notification_handle.clone()),
            scheduler_handle: scheduler_handle_for_handle,
        },
        session_done_rx,
    ))
}

fn reject_workflow_envelope(
    ack: tokio::sync::oneshot::Sender<tools::implementations::grow_build::workflow::WorkflowAck>,
) {
    let _ = ack.send(
        tools::implementations::grow_build::workflow::WorkflowAck::Rejected {
            code: "workflow_session_shutting_down",
            detail: "Session is shutting down; Workflow admission is closed.".into(),
        },
    );
}
/// Handle for a session's dedicated thread. Stored separately from `SessionHandle`
/// (which derives `Clone`) because `JoinHandle` is not `Clone`.
#[derive(Clone)]
pub struct SessionThread {
    owner: std::sync::Arc<SessionThreadOwner>,
}
enum SessionThreadState {
    Running(std::thread::JoinHandle<()>),
    Joining,
    Joined { panicked: bool },
}
struct SessionThreadOwner {
    state: std::sync::Mutex<SessionThreadState>,
}
fn session_thread_reaper() -> &'static std::sync::mpsc::Sender<std::thread::JoinHandle<()>> {
    static REAPER: std::sync::OnceLock<std::sync::mpsc::Sender<std::thread::JoinHandle<()>>> =
        std::sync::OnceLock::new();
    REAPER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<std::thread::JoinHandle<()>>();
        std::thread::Builder::new()
            .name("session-reaper".into())
            .spawn(move || {
                while let Ok(handle) = rx.recv() {
                    let _ = handle.join();
                }
            })
            .expect("session thread reaper must start");
        tx
    })
}
impl Drop for SessionThreadOwner {
    fn drop(&mut self) {
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        let SessionThreadState::Running(handle) =
            std::mem::replace(state, SessionThreadState::Joining)
        else {
            return;
        };
        // The last logical owner disappearing must never detach a live session
        // thread. One process-owned reaper serializes joins off the Tokio
        // runtime. If the reaper itself is unavailable, fail closed and join
        // synchronously rather than exposing a second writer admission.
        if let Err(error) = session_thread_reaper().send(handle) {
            let _ = error.0.join();
        }
    }
}
impl SessionThread {
    fn new(join_handle: std::thread::JoinHandle<()>) -> Self {
        Self {
            owner: std::sync::Arc::new(SessionThreadOwner {
                state: std::sync::Mutex::new(SessionThreadState::Running(join_handle)),
            }),
        }
    }
    /// Check if the session thread has exited (panicked or finished).
    pub fn is_finished(&self) -> bool {
        let state = self
            .owner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match &*state {
            SessionThreadState::Running(handle) => handle.is_finished(),
            SessionThreadState::Joining => false,
            SessionThreadState::Joined { .. } => true,
        }
    }
    /// Consume and join a finished session thread so panic is not mistaken for
    /// a clean writer shutdown. Callers must check `is_finished` first.
    pub fn join(self) -> std::thread::Result<()> {
        let handle = {
            let mut state = self
                .owner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match std::mem::replace(&mut *state, SessionThreadState::Joining) {
                SessionThreadState::Running(handle) => handle,
                SessionThreadState::Joined { panicked } => {
                    *state = SessionThreadState::Joined { panicked };
                    return if panicked {
                        Err(Box::new("session thread panicked".to_string()))
                    } else {
                        Ok(())
                    };
                }
                SessionThreadState::Joining => {
                    *state = SessionThreadState::Joining;
                    return Err(Box::new(
                        "session thread join already in progress".to_string(),
                    ));
                }
            }
        };
        let result = handle.join();
        let panicked = result.is_err();
        *self
            .owner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = SessionThreadState::Joined { panicked };
        result
    }
    /// Construct from a raw `JoinHandle`. Used in tests.
    #[cfg(test)]
    pub fn from_handle(handle: std::thread::JoinHandle<()>) -> Self {
        Self::new(handle)
    }
}
/// Return type from the session thread's initialization, sent via oneshot.
struct SessionInitResult {
    handle: SessionHandle,
}

/// Join a session thread whose initialization did not produce a usable
/// handle. Dropping a `std::thread::JoinHandle` detaches it; that would expose
/// the caller to a retry while the old persistence owner is still unwinding,
/// which is especially visible as file-sharing violations on Windows.
async fn join_failed_session_init_thread(
    session_thread: SessionThread,
) -> Result<std::thread::Result<()>, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || session_thread.join()).await
}
/// Spawn a session actor on a dedicated thread with its own tokio runtime and `LocalSet`.
///
/// The entire `spawn_session_actor` body runs on the session thread — the `!Send`
/// `SessionActor` is constructed there and never crosses a thread boundary. The
/// `Send` construction parameters are moved into the thread, and the `Send` results
/// (`SessionHandle`) are sent back to the caller via a oneshot channel. The
/// owning primary session consumes permission events through its
/// local audit bridge.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn spawn_session_on_thread(
    session_info: SessionInfo,
    session_dir: std::path::PathBuf,
    gateway: GatewaySender,
    sampling_config: SamplingConfig,
    credentials: chat_state::Credentials,
    auth_method_id: crate::agent::auth_method::SharedAuthMethodId,
    tool_context: ToolContext,
    mcp_servers: Vec<acp::McpServer>,
    initial_client_mcp_servers: Vec<acp::McpServer>,
    mcp_meta_config_map: McpMetaConfigMap,
    parent_mcp_pool: Option<crate::session::mcp_servers::SharedMcpPool>,
    acp_mcp_servers: Vec<crate::session::mcp_servers::AcpServerEntry>,
    support_permission: bool,
    auto_update: Option<bool>,
    persistence: PersistenceHandle,
    session_title_route: Option<crate::session::actor::summary::SessionTitleRoute>,
    timeline_bootstrap: TimelineBootstrap,
    rewind_points_source: Option<workspace::session::file_state::PinnedRewindSource>,
    fs_notify_config: Option<ClientFsConfig>,
    startup_hints: StartupHints,
    client_type: ClientType,
    permission_prompt_timeout: std::time::Duration,
    auto_compact_threshold_percent: u8,
    system_prompt_label: String,
    compaction_verbatim_input: bool,
    compaction_pre_prune: bool,
    compaction_pre_prune_token_budget: Option<u64>,
    buffering_settings: Option<BufferingSettings>,
    origin_client: Option<crate::http::OriginClientInfo>,
    codebase_indexes: std::sync::Arc<parking_lot::Mutex<CodebaseIndexManager>>,
    code_nav_enabled: bool,
    fs_watch_caps: fs_watch::FsWatchCapabilities,
    client_terminal_capable: bool,
    client_fs_capable: bool,
    gateway_enabled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    agent_definition: AgentDefinition,
    skills_config: SkillsConfig,
    preloaded_skills: Option<Vec<tools::implementations::skills::types::SkillInfo>>,
    incremental_bash_output: bool,
    persisted_signals: Option<crate::session::signals::SessionSignals>,
    persisted_behavior: Option<crate::session::behavior::BehaviorSnapshot>,
    persisted_goal_mode: Option<crate::session::goal_tracker::GoalState>,
    persisted_control_revision: u64,
    persisted_workflow_runs: Vec<crate::session::workflow::store::RestoredWorkflowRun>,
    persisted_announcement_state: Option<crate::session::announcement_state::AnnouncementState>,
    memory_config: Option<crate::config::MemoryConfig>,
    session_model_id: crate::agent::models::ModelId,
    session_permission_mode: crate::util::config::PermissionMode,
    session_client_identifier: Option<String>,
    inference_idle_timeout_secs: u64,
    max_retries: Option<u32>,
    web_fetch_config: tools::implementations::grow_build::web_fetch::WebFetchConfig,
    app_builder_deployer_config: tools::implementations::grow_build::deploy_app::AppBuilderDeployerConfig,
    write_file_enabled: bool,
    goal_enabled: bool,
    background_workflows_enabled: bool,
    subagents_enabled: bool,
    subagents_max_depth: u32,
    subagent_classifier_input: crate::config::SubagentClassifierInput,
    ask_user_question_enabled: bool,
    client_hooks: crate::extensions::hooks::ClientHooks,
    prompt_display_cwd: Option<String>,
    subagent_toggle: std::collections::HashMap<String, bool>,
    workflow_cli_agents: Vec<agent::config::AgentDefinition>,
    workflow_cli_overrides: crate::agent::config::CliAgentOverrides,
    workflow_file_tool_overrides: Option<Vec<tools::registry::types::ToolConfig>>,
    frozen_subagent_names: Option<Vec<String>>,
    prompt_audience: agent::prompt::context::PromptAudience,
    respect_gitignore: bool,
    path_not_found_hints: bool,
    tool_params_json: crate::session::agent_rebuild::ResolvedToolParamsJson,
    plugin_registry: Option<std::sync::Arc<agent::plugins::PluginRegistry>>,
    plugin_registry_handle: Option<agent::plugins::SharedPluginRegistryHandle>,
    models_manager: crate::agent::models::ModelsManager,
    inherited_permission_handle: Option<workspace::permission::PermissionHandle>,
    api_key_provider: Option<tools::types::SharedApiKeyProvider>,
    image_description_model: Option<String>,
    hook_registry_override: Option<std::sync::Arc<::hooks::discovery::HookRegistry>>,
    workspace_ops: workspace::WorkspaceOps,
    cli_permission_rules: Vec<workspace::permission::types::PermissionRule>,
    remote_settings: Option<crate::util::config::RemoteSettings>,
    laziness_debug_log: Option<std::path::PathBuf>,
    parent_terminal_backend: Option<std::sync::Arc<dyn tools::computer::types::TerminalBackend>>,
    parent_scheduler_handle: Option<
        tools::implementations::grow_build::scheduler::types::SchedulerHandle,
    >,
    max_turns: Option<usize>,
    on_thread_spawned: Option<&dyn Fn(SessionThread)>,
) -> Result<(SessionHandle, SessionThread), acp::Error> {
    let (init_tx, init_rx) =
        tokio::sync::oneshot::channel::<Result<SessionInitResult, agent::AgentBuildError>>();
    let sid = session_info.id.0.to_string();
    let thread_name = format!("ses-{}", &sid[..sid.len().min(8)]);
    const SESSION_THREAD_STACK_SIZE: usize = 8 * 1024 * 1024;
    let join_handle = std::thread::Builder::new()
        .name(thread_name)
        .stack_size(SESSION_THREAD_STACK_SIZE)
        .spawn(move || {
            let rt = match build_session_runtime() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "failed to build session runtime (resource exhaustion?)"
                    );
                    let _ = init_tx.send(Err(agent::AgentBuildError::RuntimeBuild(e)));
                    return;
                }
            };
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                let (handle, session_done_rx) = match spawn_session_actor(
                    session_info,
                    session_dir,
                    gateway,
                    sampling_config,
                    credentials,
                    auth_method_id,
                    tool_context,
                    mcp_servers,
                    initial_client_mcp_servers,
                    mcp_meta_config_map,
                    parent_mcp_pool,
                    acp_mcp_servers,
                    support_permission,
                    auto_update,
                    persistence,
                    session_title_route,
                    timeline_bootstrap,
                    rewind_points_source,
                    fs_notify_config,
                    startup_hints,
                    client_type,
                    permission_prompt_timeout,
                    auto_compact_threshold_percent,
                    system_prompt_label,
                    compaction_verbatim_input,
                    compaction_pre_prune,
                    compaction_pre_prune_token_budget,
                    buffering_settings,
                    origin_client,
                    codebase_indexes,
                    code_nav_enabled,
                    fs_watch_caps,
                    client_terminal_capable,
                    client_fs_capable,
                    gateway_enabled,
                    agent_definition,
                    skills_config,
                    preloaded_skills,
                    incremental_bash_output,
                    persisted_signals,
                    persisted_behavior,
                    persisted_goal_mode,
                    persisted_control_revision,
                    persisted_workflow_runs,
                    persisted_announcement_state,
                    memory_config,
                    session_model_id,
                    session_permission_mode,
                    session_client_identifier,
                    inference_idle_timeout_secs,
                    max_retries,
                    web_fetch_config,
                    app_builder_deployer_config,
                    write_file_enabled,
                    goal_enabled,
                    background_workflows_enabled,
                    subagents_enabled,
                    subagents_max_depth,
                    subagent_classifier_input,
                    ask_user_question_enabled,
                    client_hooks,
                    prompt_display_cwd,
                    subagent_toggle,
                    workflow_cli_agents,
                    workflow_cli_overrides,
                    workflow_file_tool_overrides,
                    frozen_subagent_names,
                    prompt_audience,
                    respect_gitignore,
                    path_not_found_hints,
                    tool_params_json,
                    plugin_registry,
                    plugin_registry_handle,
                    models_manager,
                    inherited_permission_handle,
                    api_key_provider,
                    image_description_model,
                    hook_registry_override,
                    workspace_ops,
                    cli_permission_rules,
                    remote_settings,
                    laziness_debug_log,
                    parent_terminal_backend,
                    parent_scheduler_handle,
                    max_turns,
                )
                .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };
                let _ = init_tx.send(Ok(SessionInitResult { handle }));
                let _ = session_done_rx.await;
            });
        });
    let join_handle = match join_handle {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(
                error = %e,
                "failed to spawn session thread (thread/PID limit or memory pressure?)"
            );
            return Err(
                acp::Error::internal_error().data(format!("failed to spawn session thread: {e}"))
            );
        }
    };
    let session_thread = SessionThread::new(join_handle);
    if let Some(register) = on_thread_spawned {
        register(session_thread.clone());
    }
    match init_rx.await {
        Ok(Ok(init)) => Ok((init.handle, session_thread)),
        Ok(Err(error)) => {
            let joined = join_failed_session_init_thread(session_thread)
                .await
                .map_err(|join_error| {
                    acp::Error::internal_error().data(format!(
                        "session initialization failed and its thread could not be joined: {join_error}"
                    ))
                })?;
            if joined.is_err() {
                tracing::error!("Session thread panicked after reporting an initialization error");
                return Err(acp::Error::internal_error().data(format!(
                    "session initialization failed: {error}; session thread panicked while unwinding"
                )));
            }
            Err(acp::Error::internal_error()
                .data(format!("session initialization failed: {error}")))
        }
        Err(_) => {
            let joined = join_failed_session_init_thread(session_thread)
                .await
                .map_err(|join_error| {
                    acp::Error::internal_error().data(format!(
                        "session initialization channel closed and its thread could not be joined: {join_error}"
                    ))
                })?;
            if joined.is_err() {
                tracing::error!("Session thread panicked during initialization");
                Err(acp::Error::internal_error()
                    .data("session thread panicked during initialization"))
            } else {
                tracing::error!("Session thread exited without publishing initialization");
                Err(acp::Error::internal_error()
                    .data("session thread exited without publishing initialization"))
            }
        }
    }
}
/// Production [`crate::session::mcp_restart::RestartActions`] impl.
///
/// Captured by the dispatcher task at session startup when
/// `mcp.auto_restart=true`. Holds an `Arc<SessionActor>` plus the
/// dispatcher's `SharedShutdownState` so:
///
/// - `is_stdio_server_configured` resolves against
///   [`SessionActor::is_stdio_server_configured`] (which reads
///   `McpState::configs`).
/// - `is_in_shutting_down` peeks at the dispatcher's set.
/// - `respawn_stdio` delegates to
///   [`SessionActor::respawn_stdio`] (re-runs `start_mcp_server`,
///   handshake, liveness arm, owned_clients swap).
/// - `push_status` forwards directly via the session's gateway.
pub(crate) struct SessionRestartActions {
    session: Arc<SessionActor>,
    shutdown: crate::session::mcp_dispatcher::SharedShutdownState,
}
impl SessionRestartActions {
    pub(crate) fn new(
        session: Arc<SessionActor>,
        shutdown: crate::session::mcp_dispatcher::SharedShutdownState,
    ) -> Self {
        Self { session, shutdown }
    }
}
#[async_trait::async_trait(?Send)]
impl crate::session::mcp_restart::RestartActions for SessionRestartActions {
    async fn is_stdio_server_configured(&self, server: &str) -> bool {
        self.session.is_stdio_server_configured(server).await
    }
    fn is_in_shutting_down(&self, server: &str) -> bool {
        self.shutdown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_shutting_down(server)
    }
    async fn respawn_stdio(&self, server: &str) -> Result<(), String> {
        self.session.respawn_stdio(server).await
    }
    async fn is_http_server_configured(&self, server: &str) -> bool {
        self.session.is_http_server_configured(server).await
    }
    async fn reset_http_client(&self, server: &str) -> Result<(), String> {
        self.session.reset_http_client(server).await
    }
    fn unregister_server_tools(&self, server: &str) {
        self.session.unregister_server_tools(server);
    }
    fn push_status(&self, payload: &crate::session::mcp_dispatcher::McpServerStatusPayload) {
        crate::session::mcp_restart::forward_status(&self.session.notifications.gateway, payload);
    }
    fn begin_restart(&self, server: &str) -> bool {
        self.shutdown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .begin_restart(server.to_string())
    }
    fn end_restart(&self, server: &str) {
        self.shutdown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .end_restart(server);
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalBackendKind {
    ReuseParent,
    AcpClient,
    LocalNonPersistent,
}
fn select_terminal_backend_kind(
    is_subagent: bool,
    has_parent_backend: bool,
    client_terminal_capable: bool,
    has_gateway: bool,
) -> TerminalBackendKind {
    if is_subagent && has_parent_backend {
        TerminalBackendKind::ReuseParent
    } else if client_terminal_capable && has_gateway {
        TerminalBackendKind::AcpClient
    } else {
        TerminalBackendKind::LocalNonPersistent
    }
}
#[cfg(test)]
mod terminal_backend_select_tests {
    use super::{TerminalBackendKind, select_terminal_backend_kind};
    #[test]
    fn subagent_with_parent_reuses_parent() {
        assert_eq!(
            select_terminal_backend_kind(true, true, true, true),
            TerminalBackendKind::ReuseParent
        );
    }
    #[test]
    fn subagent_without_parent_falls_through() {
        assert_eq!(
            select_terminal_backend_kind(true, false, true, true),
            TerminalBackendKind::AcpClient
        );
        assert_eq!(
            select_terminal_backend_kind(true, false, false, true),
            TerminalBackendKind::LocalNonPersistent
        );
    }
    #[test]
    fn non_subagent_never_reuses_parent() {
        assert_eq!(
            select_terminal_backend_kind(false, true, false, false),
            TerminalBackendKind::LocalNonPersistent
        );
    }
    #[test]
    fn client_terminal_uses_acp_only_with_gateway() {
        assert_eq!(
            select_terminal_backend_kind(false, false, true, true),
            TerminalBackendKind::AcpClient
        );
        assert_eq!(
            select_terminal_backend_kind(false, false, true, false),
            TerminalBackendKind::LocalNonPersistent
        );
    }
    #[test]
    fn local_session_uses_non_persistent_backend() {
        assert_eq!(
            select_terminal_backend_kind(false, false, false, false),
            TerminalBackendKind::LocalNonPersistent
        );
    }
}

#[cfg(test)]
mod workflow_ingress_shutdown_tests {
    use super::reject_workflow_envelope;
    use tools::implementations::grow_build::workflow::WorkflowAck;

    #[tokio::test]
    async fn queued_envelope_is_rejected_at_worker_shutdown() {
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        reject_workflow_envelope(ack_tx);
        assert!(matches!(
            ack_rx.await.expect("shutdown rejection"),
            WorkflowAck::Rejected {
                code: "workflow_session_shutting_down",
                ..
            }
        ));
    }
}

#[cfg(test)]
mod failed_session_init_join_tests {
    use super::{SessionThread, SessionThreadState, join_failed_session_init_thread};

    #[tokio::test]
    async fn failed_initialization_waits_for_thread_teardown() {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = release_rx.recv();
        });
        let join = join_failed_session_init_thread(SessionThread::new(thread));
        tokio::pin!(join);

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut join)
                .await
                .is_err(),
            "initialization failure must not expose a retry boundary before teardown"
        );
        let _ = release_tx.send(());
        assert!(join.await.expect("blocking join task").is_ok());
    }

    #[tokio::test]
    async fn failed_initialization_observes_thread_panic() {
        let thread = std::thread::spawn(|| panic!("init panic fixture"));
        assert!(
            join_failed_session_init_thread(SessionThread::new(thread))
                .await
                .expect("blocking join task")
                .is_err(),
            "panic must be joined and classified instead of detached"
        );
    }

    #[tokio::test]
    async fn cancelling_init_waiter_cannot_detach_the_registered_thread() {
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _ = release_rx.recv();
        });
        let registered = SessionThread::new(thread);
        let waiter = tokio::spawn(join_failed_session_init_thread(registered.clone()));

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let joining = {
                    let state = registered
                        .owner
                        .state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    matches!(&*state, SessionThreadState::Joining)
                };
                if joining {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the blocking join must claim the shared owner");

        waiter.abort();
        let _ = waiter.await;
        assert!(
            !registered.is_finished(),
            "cancelling the async waiter must leave the SessionId draining"
        );

        let _ = release_tx.send(());
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !registered.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the uncancellable blocking join must publish its terminal state");
        assert!(registered.join().is_ok());
    }
}

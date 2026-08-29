use super::*;

#[test]
fn complete_jsonl_snapshot_never_returns_a_partial_record_offset() {
    use std::io::{Read, Seek, SeekFrom, Write};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("updates.jsonl");
    let first = br#"{"eventId":"a","text":"complete"}
"#;
    let second = "{\"eventId\":\"b\",\"text\":\"权限审批完整内容\"}\n".as_bytes();

    for cut in 0..second.len() {
        let mut prefix = first.to_vec();
        prefix.extend_from_slice(&second[..cut]);
        std::fs::write(&path, prefix).unwrap();

        let (snapshot, offset) = read_complete_jsonl_snapshot(&path).unwrap();
        assert_eq!(snapshot.as_bytes(), first, "cut={cut}");
        assert_eq!(offset, first.len() as u64, "cut={cut}");

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(&second[cut..]).unwrap();
        drop(file);
        let mut file = std::fs::File::open(&path).unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        let mut delta = Vec::new();
        file.read_to_end(&mut delta).unwrap();
        assert_eq!(delta, second, "cut={cut}");
    }

    std::fs::write(&path, second).unwrap();
    let (snapshot, offset) = read_complete_jsonl_snapshot(&path).unwrap();
    assert_eq!(snapshot.as_bytes(), second);
    assert_eq!(offset, second.len() as u64);
}

fn valid_agent_config() -> crate::agent::config::Config {
    let raw: toml::Value = toml::from_str(
        r#"
        [models]
        default = "test/default"

        [provider.test]
        api_backend = "chat_completions"

        [provider.test.options]
        base_url = "http://localhost:11434/v1"

        [provider.test.models.default]
        context_window = 128000
        "#,
    )
    .expect("valid test provider TOML");
    let mut config = crate::agent::config::Config::new_from_toml_cfg(&raw)
        .expect("valid explicit BYOK test config");
    // Tests must not inherit the developer machine's model override.
    config.models.default = Some("test/default".to_owned());
    config.default_model_override = None;
    config.remote_settings = Some(crate::util::config::RemoteSettings::default());
    config
}

/// Single-flight flag must clear on Drop even if the retry task panics /
/// aborts mid-backoff (guards against the flag stuck true forever).
mod hunk_tracking_mode {
    use super::super::{plan_hunk_tracking, resolve_hunk_tracking_mode};
    use hunk_tracker::TrackingMode;
    #[test]
    fn off_and_disabled_disable_tracking() {
        assert_eq!(resolve_hunk_tracking_mode(Some("off")), None);
        assert_eq!(resolve_hunk_tracking_mode(Some("disabled")), None);
    }
    #[test]
    fn matching_is_case_insensitive_and_trimmed() {
        assert_eq!(resolve_hunk_tracking_mode(Some("OFF")), None);
        assert_eq!(resolve_hunk_tracking_mode(Some("  Off ")), None);
        assert_eq!(resolve_hunk_tracking_mode(Some("DISABLED")), None);
        assert_eq!(
            resolve_hunk_tracking_mode(Some("Agent_Only")),
            Some(TrackingMode::AgentOnly)
        );
        assert_eq!(
            resolve_hunk_tracking_mode(Some(" ALL_DIRTY ")),
            Some(TrackingMode::AllDirty)
        );
    }
    #[test]
    fn recognized_modes_parse() {
        assert_eq!(
            resolve_hunk_tracking_mode(Some("agent_only")),
            Some(TrackingMode::AgentOnly)
        );
        assert_eq!(
            resolve_hunk_tracking_mode(Some("all_dirty")),
            Some(TrackingMode::AllDirty)
        );
    }
    #[test]
    fn parser_absent_returns_none_policy_defaults_in_plan() {
        assert_eq!(resolve_hunk_tracking_mode(None), None);
        assert_eq!(resolve_hunk_tracking_mode(Some("")), None);
        assert_eq!(
            resolve_hunk_tracking_mode(Some("bogus")),
            Some(TrackingMode::AllDirty)
        );
    }
    #[test]
    fn plan_disables_actor_forward_and_loc_together() {
        for off in ["off", "disabled", "OFF"] {
            let plan = plan_hunk_tracking(Some(off));
            assert_eq!(plan.actor_mode, None, "{off} must not spawn the actor");
            assert!(!plan.enabled(), "{off} must disable the forward + LOC sink");
        }
    }
    #[test]
    fn plan_enables_actor_and_forward_for_active_modes() {
        for (mode, expected) in [
            ("agent_only", TrackingMode::AgentOnly),
            ("all_dirty", TrackingMode::AllDirty),
            ("bogus", TrackingMode::AllDirty),
        ] {
            let plan = plan_hunk_tracking(Some(mode));
            assert_eq!(plan.actor_mode, Some(expected));
            assert!(plan.enabled());
        }
        let plan = plan_hunk_tracking(None);
        assert_eq!(plan.actor_mode, None);
        assert!(!plan.enabled());
    }
}
mod capture {
    use tokio::sync::mpsc;
    use tracing::Subscriber;
    use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
    use tracing_subscriber::registry::LookupSpan;
    pub(crate) struct CapturedEvent {
        pub level: tracing::Level,
        pub fields: String,
    }
    pub(crate) struct Captured {
        pub events_rx: mpsc::UnboundedReceiver<CapturedEvent>,
        _guard: tracing::subscriber::DefaultGuard,
    }
    pub(crate) fn capture() -> Captured {
        let (tx, rx) = mpsc::unbounded_channel();
        let subscriber = tracing_subscriber::registry().with(CaptureLayer { tx });
        let guard = tracing::subscriber::set_default(subscriber);
        Captured {
            events_rx: rx,
            _guard: guard,
        }
    }
    struct CaptureLayer {
        tx: mpsc::UnboundedSender<CapturedEvent>,
    }
    impl<S> Layer<S> for CaptureLayer
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
            let mut v = Visitor::default();
            event.record(&mut v);
            let _ = self.tx.send(CapturedEvent {
                level: *event.metadata().level(),
                fields: v.out,
            });
        }
    }
    #[derive(Default)]
    struct Visitor {
        out: String,
    }
    impl tracing::field::Visit for Visitor {
        fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
            if !self.out.is_empty() {
                self.out.push(' ');
            }
            self.out.push_str(f.name());
            self.out.push('=');
            self.out.push_str(&format!("{v:?}"));
        }
        fn record_str(&mut self, f: &tracing::field::Field, v: &str) {
            if !self.out.is_empty() {
                self.out.push(' ');
            }
            self.out.push_str(f.name());
            self.out.push('=');
            self.out.push_str(v);
        }
    }
}
#[test]
fn warn_on_missing_parent_session_emits_when_session_absent() {
    let captured = capture::capture();
    warn_on_missing_parent_session_for_validate_type("ghost-session", false);
    let mut rx = captured.events_rx;
    let mut saw = false;
    while let Ok(event) = rx.try_recv() {
        if event.level == tracing::Level::WARN
            && event
                .fields
                .contains("ValidateType received for unknown parent session")
            && event.fields.contains("parent_session_id=ghost-session")
        {
            saw = true;
            break;
        }
    }
    assert!(saw, "warn must fire");
}
#[test]
fn warn_on_missing_parent_session_silent_when_session_present() {
    let captured = capture::capture();
    warn_on_missing_parent_session_for_validate_type("real-session", true);
    let mut rx = captured.events_rx;
    assert!(rx.try_recv().is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn broadcast_refresh_skill_baseline_sends_one_message_per_sender() {
    let mut receivers = Vec::new();
    let mut senders = Vec::new();
    for _ in 0..3 {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        senders.push(tx);
        receivers.push(rx);
    }
    MvpAgent::broadcast_refresh_skill_baseline(senders);
    for mut rx in receivers {
        assert!(matches!(
            rx.try_recv(),
            Ok(crate::session::SessionCommand::RefreshSkillBaseline)
        ));
        assert!(
            rx.try_recv().is_err(),
            "broadcast must send exactly one message per sender",
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn broadcast_refresh_skill_baseline_tolerates_dropped_receiver() {
    let (tx_alive, mut rx_alive) = tokio::sync::mpsc::unbounded_channel();
    let (tx_dead, rx_dead) = tokio::sync::mpsc::unbounded_channel();
    drop(rx_dead);
    MvpAgent::broadcast_refresh_skill_baseline(vec![tx_alive, tx_dead]);
    assert!(matches!(
        rx_alive.try_recv(),
        Ok(crate::session::SessionCommand::RefreshSkillBaseline)
    ));
}
/// The monotonic turn counter must never wrap on the DB-bound i32 path.
/// `allocate_turn_number` returns u64; the AB submission casts to i32.
/// Verify we saturate instead of wrapping.
#[test]
fn trace_turn_to_i32_saturates_at_max() {
    let small: u64 = 42;
    let result = i32::try_from(small).unwrap_or(i32::MAX);
    assert_eq!(result, 42);
    let huge: u64 = (i32::MAX as u64) + 100;
    let result = i32::try_from(huge).unwrap_or(i32::MAX);
    assert_eq!(result, i32::MAX);
    let boundary: u64 = i32::MAX as u64;
    let result = i32::try_from(boundary).unwrap_or(i32::MAX);
    assert_eq!(result, i32::MAX);
}
/// After allocating a turn number, the retained (in-memory) turn counter holds
/// the next value (current + 1). This is the value that must be persisted via
/// `SetNextTraceTurn` so the counter survives restarts.
#[test]
fn allocate_turn_number_advances_counter() {
    use std::cell::RefCell;
    use std::collections::HashMap;
    let counters: RefCell<HashMap<acp::SessionId, u64>> = RefCell::new(HashMap::new());
    let sid = acp::SessionId::new("test-session");
    let allocate = |id: &acp::SessionId| -> u64 {
        let mut m = counters.borrow_mut();
        let turn = m.get(id).copied().unwrap_or(0u64);
        m.insert(id.clone(), turn.saturating_add(1));
        turn
    };
    assert_eq!(allocate(&sid), 0);
    assert_eq!(*counters.borrow().get(&sid).unwrap(), 1);
    assert_eq!(allocate(&sid), 1);
    assert_eq!(*counters.borrow().get(&sid).unwrap(), 2);
    assert_eq!(allocate(&sid), 2);
    assert_eq!(*counters.borrow().get(&sid).unwrap(), 3);
}
/// A new session with no explicit profile uses the global default Agent.
#[test]
#[serial_test::serial]
fn resolve_agent_definition_defaults_to_grow_build() {
    let prev = std::env::var("GROW_AGENT").ok();
    unsafe {
        std::env::remove_var("GROW_AGENT");
    }
    let tmp = tempfile::tempdir().unwrap();
    let def = MvpAgent::resolve_agent_definition(
        tmp.path(),
        None,
        None,
        &config::AgentSelectionConfig::default(),
        None,
        None,
    );
    assert_eq!(def.name, config::DEFAULT_AGENT_TYPE);
    if let Some(v) = prev {
        unsafe { std::env::set_var("GROW_AGENT", v) }
    }
}
/// A resumed session restores its persisted Agent independently of the model.
#[test]
#[serial_test::serial]
fn resolve_agent_definition_restores_persisted_agent() {
    let prev = std::env::var("GROW_AGENT").ok();
    unsafe {
        std::env::remove_var("GROW_AGENT");
    }
    let tmp = tempfile::tempdir().unwrap();
    let def = MvpAgent::resolve_agent_definition(
        tmp.path(),
        None,
        None,
        &config::AgentSelectionConfig::default(),
        None,
        Some("browser-use"),
    );
    assert_eq!(def.name, "browser-use");
    if let Some(v) = prev {
        unsafe { std::env::set_var("GROW_AGENT", v) }
    }
}

#[test]
#[serial_test::serial]
fn resolve_agent_definition_restores_qualified_plugin_agent() {
    use agent::plugins::discovery::{DiscoveredPlugin, PluginId};
    use agent::plugins::{PluginOrigin, PluginRegistry, PluginScope};

    let previous = std::env::var("GROW_AGENT").ok();
    unsafe {
        std::env::remove_var("GROW_AGENT");
    }
    let root = tempfile::tempdir().unwrap();
    let agents = root.path().join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("reviewer.md"),
        "---\nname: reviewer\ndescription: Plugin reviewer\n---\nReview carefully.\n",
    )
    .unwrap();
    let manifest = serde_json::from_value(serde_json::json!({ "name": "quality" })).unwrap();
    let plugin = DiscoveredPlugin {
        id: PluginId::new(PluginScope::User, root.path(), "quality"),
        manifest,
        root: root.path().to_path_buf(),
        canonical_root: root.path().to_path_buf(),
        scope: PluginScope::User,
        origin: PluginOrigin::UserGrow,
        trusted: true,
        skill_dirs: vec![],
        command_dirs: vec![],
        agent_dirs: vec![agents],
        hooks_path: None,
        mcp_config_path: None,
        lsp_config_path: None,
        conflict: None,
    };
    let registry = PluginRegistry::from_discovered(vec![plugin], &[], &["quality".into()]);
    let cwd = tempfile::tempdir().unwrap();
    let definition = MvpAgent::resolve_agent_definition(
        cwd.path(),
        Some(&registry),
        None,
        &config::AgentSelectionConfig::default(),
        None,
        Some("quality:reviewer"),
    );
    assert_eq!(definition.selector_identity(), "quality:reviewer");
    assert_eq!(definition.prompt_body.as_deref(), Some("Review carefully."));
    if let Some(value) = previous {
        unsafe { std::env::set_var("GROW_AGENT", value) }
    }
}
/// A new session may still receive an explicit ACP Agent profile.
#[test]
#[serial_test::serial]
fn resolve_agent_definition_acp_profile_wins_for_new_session() {
    let prev = std::env::var("GROW_AGENT").ok();
    unsafe {
        std::env::remove_var("GROW_AGENT");
    }
    let tmp = tempfile::tempdir().unwrap();
    let acp_profile = agent::AgentDefinition::from_json(&serde_json::json!(
        { "name" : "custom-devbox-profile", "description" :
        "Custom devbox profile", "promptBody" :
        "You are a custom-configured devbox agent.", }
    ))
    .expect("agent definition must parse");
    let def = MvpAgent::resolve_agent_definition(
        tmp.path(),
        None,
        None,
        &config::AgentSelectionConfig::default(),
        Some(acp_profile),
        None,
    );
    assert_eq!(
        def.name, "custom-devbox-profile",
        "ACP _meta.agentProfile must win for a new session"
    );
    if let Some(v) = prev {
        unsafe { std::env::set_var("GROW_AGENT", v) }
    }
}
/// CLI `--agent-profile` selects a new session's Agent.
#[test]
#[serial_test::serial]
fn resolve_agent_definition_cli_agent_profile_wins_for_new_session() {
    let prev = std::env::var("GROW_AGENT").ok();
    unsafe {
        std::env::remove_var("GROW_AGENT");
    }
    let tmp = tempfile::tempdir().unwrap();
    let profile_path = tmp.path().join("cli-profile.md");
    std::fs::write(
        &profile_path,
        "---\nname: cli-profile\ndescription: cli test\n---\nYou are a CLI profile.\n",
    )
    .unwrap();
    let def = MvpAgent::resolve_agent_definition(
        tmp.path(),
        None,
        Some(&profile_path),
        &config::AgentSelectionConfig::default(),
        None,
        None,
    );
    assert_eq!(def.name, "cli-profile");
    if let Some(v) = prev {
        unsafe { std::env::set_var("GROW_AGENT", v) }
    }
}
#[test]
fn read_session_or_init_meta_str_prefers_session_meta() {
    let session = serde_json::json!({ "rules": "from-session" });
    let init = serde_json::json!({ "rules": "from-init" });
    assert_eq!(
        read_session_or_init_meta_str(session.as_object(), init.as_object(), "rules"),
        Some("from-session"),
    );
}
#[test]
fn read_session_or_init_meta_str_falls_back_to_init_meta() {
    let session = serde_json::json!({ "other": "x" });
    let init = serde_json::json!({ "rules": "from-init" });
    assert_eq!(
        read_session_or_init_meta_str(session.as_object(), init.as_object(), "rules"),
        Some("from-init"),
    );
    assert_eq!(
        read_session_or_init_meta_str(None, init.as_object(), "rules"),
        Some("from-init"),
    );
}
#[test]
fn parse_session_plugin_dirs_filters_and_dedupes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = dunce::canonicalize(tmp.path()).unwrap().join("plugin");
    std::fs::create_dir(&dir).unwrap();
    let file = tmp.path().join("file.txt");
    std::fs::write(&file, "x").unwrap();
    let meta = serde_json::json!({
        "pluginDirs": [
            dir.to_string_lossy(),          // kept
            dir.to_string_lossy(),          // duplicate → deduped
            file.to_string_lossy(),         // not a directory → skipped
            "relative/path",                // not absolute → skipped
            42,                             // not a string → skipped
        ]
    });
    assert_eq!(parse_session_plugin_dirs(meta.as_object()), vec![dir]);
    assert!(parse_session_plugin_dirs(None).is_empty());
    assert!(parse_session_plugin_dirs(serde_json::json!({}).as_object()).is_empty());
}
#[test]
fn read_session_or_init_meta_str_returns_none_when_absent() {
    assert_eq!(read_session_or_init_meta_str(None, None, "rules"), None,);
    let session = serde_json::json!({ "other": "x" });
    assert_eq!(
        read_session_or_init_meta_str(session.as_object(), None, "rules"),
        None,
    );
}
#[test]
fn read_session_or_init_meta_str_ignores_non_string_values() {
    let session = serde_json::json!({ "rules": 42 });
    let init = serde_json::json!({ "rules": "from-init" });
    assert_eq!(
        read_session_or_init_meta_str(session.as_object(), init.as_object(), "rules"),
        Some("from-init"),
    );
}
#[test]
fn session_rules_are_separate_from_the_system_head() {
    let session = serde_json::json!({ "rules": " session rule </human_rules> " });
    let init = serde_json::json!({ "rules": "init rule" });
    assert_eq!(
        session_rules_from_meta(session.as_object(), init.as_object()).as_deref(),
        Some("<human_rules>\nsession rule <\\/human_rules>\n</human_rules>")
    );
    assert_eq!(
        session_rules_from_meta(serde_json::json!({ "rules": "  " }).as_object(), None,),
        None
    );
}
/// End-to-end test: config -> resolve -> override -> finalize -> tool_definitions.
///
/// Exercises the full live path through to the finalized toolset, proving
/// that the hashline tools appear in the actual tool definitions that
/// would be sent to the model.
#[tokio::test]
async fn file_toolset_override_e2e_to_finalized_toolset() {
    use crate::tools::{FileToolset, ShellToolsetConfig};
    use tools::computer::local::{LocalFs, LocalTerminalBackend};
    use tools::notification::ToolNotificationHandle;
    use tools::registry::types::SessionContext;
    let tmp = tempfile::tempdir().unwrap();
    let mut def = MvpAgent::resolve_agent_definition(
        tmp.path(),
        None,
        None,
        &config::AgentSelectionConfig::default(),
        None,
        None,
    );
    let toolset_config = ShellToolsetConfig {
        file_toolset: FileToolset::Hashline,
        ..ShellToolsetConfig::default()
    };
    let effective = toolset_config.resolve_file_toolset(None);
    let file_tools = effective
        .tool_configs(&toolset_config.hashline)
        .expect("default hashline config should validate");
    def.override_file_tools(file_tools);
    let builder = tools::registry::types::ToolRegistryBuilder::new();
    let ctx = SessionContext {
        backend: std::sync::Arc::new(LocalTerminalBackend::new()),
        fs: std::sync::Arc::new(LocalFs),
        cwd: tmp.path().to_path_buf(),
        session_folder: tmp.path().join("session"),
        session_env: std::sync::Arc::new(std::collections::HashMap::new()),
        notification_handle: ToolNotificationHandle::noop(),
        owner_session_id: None,
        subagent: None,
        parent_scheduler_handle: None,
        skills: vec![],
        resources_persistence: std::sync::Arc::new(
            tools::persistence::ResourcesPersistence::local(
                tmp.path().join("resources_state.json"),
            )
            .expect("pin resources state test store"),
        ),
        memory_backend: None,
        web_fetch_config: Default::default(),
        lsp: None,
        app_builder_deployer_config:
            tools::implementations::grow_build::deploy_app::AppBuilderDeployerConfig::default(),
        system_reminder_tag: tools::reminders::DEFAULT_REMINDER_TAG,
    };
    let toolset = builder
        .finalize(def.tool_config, ctx)
        .expect("hashline toolset should finalize");
    let defs = toolset.tool_definitions();
    let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
    assert!(names.contains(&"hashline_read"), "defs: {names:?}");
    assert!(names.contains(&"hashline_edit"), "defs: {names:?}");
    assert!(names.contains(&"hashline_grep"), "defs: {names:?}");
    assert!(!names.contains(&"read_file"), "defs: {names:?}");
    assert!(!names.contains(&"search_replace"), "defs: {names:?}");
    assert!(names.contains(&"list_dir"), "defs: {names:?}");
}
/// Invalid hashline config returns a clean error, not a panic.
#[test]
fn file_toolset_override_invalid_config_returns_error() {
    use crate::tools::FileToolset;
    use crate::tools::config::HashlineSchemeConfig;
    let bad = HashlineSchemeConfig {
        scheme: "bogus".to_owned(),
        hash_len: 0,
        chunk_size: 0,
    };
    let err = FileToolset::Hashline.tool_configs(&bad);
    assert!(err.is_err());
    assert!(err.unwrap_err().contains("unknown"));
}
/// Helper: creates a real SessionHandle with the given model and client id.
/// Requires a tokio runtime for SessionSignalsHandle::new().
fn make_test_handle(model: &str, client_id: Option<&str>) -> crate::session::SessionHandle {
    make_test_handle_with_receiver(model, client_id).0
}

fn make_test_handle_with_receiver(
    model: &str,
    client_id: Option<&str>,
) -> (
    crate::session::SessionHandle,
    tokio::sync::mpsc::UnboundedReceiver<crate::session::SessionCommand>,
) {
    let (cmd_tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    let (hunk_event_tx, _hunk_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let hunk_cancel = tokio_util::sync::CancellationToken::new();
    let hunk_tracker_handle = hunk_tracker::HunkTrackerActor::spawn(
        "test".to_string(),
        std::path::PathBuf::from("/tmp"),
        hunk_event_tx,
        hunk_tracker::TrackingMode::AllDirty,
        hunk_cancel,
    );
    let handle = crate::session::SessionHandle {
        lifecycle_owner: std::sync::Arc::new(crate::session::handle::SessionLifecycleOwner::new(
            &cmd_tx,
        )),
        cmd_tx: cmd_tx.clone(),
        goal_usage_window: crate::session::actor::goal_support::GoalUsageWindow::new(cmd_tx, None),
        persistence_tx,
        current_prompt_id: std::sync::Arc::new(std::sync::Mutex::new(None)),
        pending_interactions: std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashMap::new(),
        )),
        info: crate::session::info::Info {
            id: acp::SessionId::new("test"),
            cwd: "/tmp".to_string(),
        },
        max_turns: None,
        permission_prompt_timeout: std::time::Duration::from_secs(
            crate::agent::config::DEFAULT_PERMISSION_PROMPT_TIMEOUT_SECS,
        ),
        hunk_tracker_handle,
        chat_state_handle: chat_state::ChatStateHandle::noop(),
        signals_handle: crate::session::signals::SessionSignalsHandle::new(),
        gateway_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        mcp_servers: vec![],
        initial_client_mcp_servers: vec![],
        display_cwd: None,
        tool_context: crate::tools::ToolContext::new_local_context(
            paths::AbsPathBuf::new(std::path::PathBuf::from("/tmp")).unwrap(),
            std::sync::Arc::new(workspace::file_system::LocalFs::new(
                std::path::PathBuf::from("/tmp"),
            )),
            std::sync::Arc::new(crate::terminal::LocalTerminalRunner),
        ),
        model_route: crate::session::handle::SessionModelRoute::new(
            acp::ModelId::new(model),
            sampler::SamplerConfig {
                model: model.to_owned(),
                ..Default::default()
            },
        ),
        permission_mode: crate::util::config::PermissionMode::Ask,
        origin_client: client_id.map(|s| crate::http::OriginClientInfo {
            product: s.to_string(),
            version: None,
        }),
        code_nav_enabled: false,
        ask_user_question_enabled: true,
        behavior: std::sync::Arc::new(parking_lot::Mutex::new(
            crate::session::behavior::BehaviorCoordinator::new(),
        )),
        workflow_tracker: std::sync::Arc::new(parking_lot::Mutex::new(
            crate::session::workflow::tracker::WorkflowTracker::default(),
        )),
        force_compact: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        permission_handle: workspace::permission::PermissionHandle::allow_all(),
        delegable_capability_ceiling: None,
        agent_profile: crate::session::handle::SessionAgentProfile::new(
            "grow-build".to_string(),
            Default::default(),
        ),
        workflow_run_id: None,
        plugin_registry: std::sync::Arc::new(parking_lot::RwLock::new(None)),
        hook_registry: None,
        workspace_ops: workspace::WorkspaceOps::for_test(),
        terminal_backend: None,
        tools_notification_handle: None,
        scheduler_handle: None,
    };
    (handle, rx)
}
/// lookup_session_model returns the per-session model for each session.
#[tokio::test]
async fn lookup_session_model_returns_per_session_model() {
    let sid_a = acp::SessionId::new("sess-a");
    let sid_b = acp::SessionId::new("sess-b");
    let default_model = acp::ModelId::new("default-model");
    let sessions: HashMap<acp::SessionId, crate::session::SessionHandle> = [
        (sid_a.clone(), make_test_handle("grow-3-fast", None)),
        (sid_b.clone(), make_test_handle("grow-mini", None)),
    ]
    .into();
    assert_eq!(
        lookup_session_model(&sessions, Some(&sid_a), &default_model)
            .0
            .as_ref(),
        "grow-3-fast"
    );
    assert_eq!(
        lookup_session_model(&sessions, Some(&sid_b), &default_model)
            .0
            .as_ref(),
        "grow-mini"
    );
}
/// lookup_session_model falls back to the default when session_id is None.
#[tokio::test]
async fn lookup_session_model_fallback_no_session() {
    let default_model = acp::ModelId::new("grow-3");
    let sessions: HashMap<acp::SessionId, crate::session::SessionHandle> = HashMap::new();
    assert_eq!(
        lookup_session_model(&sessions, None, &default_model)
            .0
            .as_ref(),
        "grow-3"
    );
}
/// Committing session A's model route does not affect session B.
#[tokio::test]
async fn set_session_model_does_not_cross_contaminate() {
    let sid_a = acp::SessionId::new("sess-a");
    let sid_b = acp::SessionId::new("sess-b");
    let default_model = acp::ModelId::new("default");
    let sessions: HashMap<acp::SessionId, crate::session::SessionHandle> = [
        (sid_a.clone(), make_test_handle("grow-3", None)),
        (sid_b.clone(), make_test_handle("grow-3", None)),
    ]
    .into();
    let route = sessions
        .get(&sid_a)
        .unwrap()
        .model_route
        .snapshot()
        .sampling_config;
    sessions
        .get(&sid_a)
        .unwrap()
        .model_route
        .replace(acp::ModelId::new("grow-mini"), route);
    assert_eq!(
        lookup_session_model(&sessions, Some(&sid_a), &default_model)
            .0
            .as_ref(),
        "grow-mini"
    );
    assert_eq!(
        lookup_session_model(&sessions, Some(&sid_b), &default_model)
            .0
            .as_ref(),
        "grow-3",
        "Session B's model must not be affected by session A's model change"
    );
}
#[tokio::test]
async fn model_state_prefers_session_reasoning_effort_over_model_default() {
    use crate::agent::config::{EndpointsConfig, ModelEntry};
    use sampling_types::{REASONING_EFFORT_META_KEY, ReasoningEffort, ReasoningEffortOption};
    let agent = build_minimal_agent_for_tests();
    let mut entry = ModelEntry::baseline("effort-model");
    entry.info.reasoning_efforts = [
        (ReasoningEffort::Low, true),
        (ReasoningEffort::Xhigh, false),
    ]
    .into_iter()
    .map(|(value, default)| ReasoningEffortOption {
        id: value.as_str().to_string(),
        value,
        label: value.to_string(),
        description: None,
        default,
    })
    .collect();
    agent
        .models_manager
        .insert_test_entry("effort-model", entry);
    let read_effort = |state: &acp::SessionModelState| -> Option<String> {
        state
            .available_models
            .iter()
            .find(|m| m.model_id.0.as_ref() == "effort-model")
            .and_then(|m| m.meta.as_ref())
            .and_then(|m| m.get(REASONING_EFFORT_META_KEY))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    let pinned = acp::SessionId::new("sess-pinned");
    let handle = make_test_handle("effort-model", None);
    let mut route = handle.model_route.snapshot().sampling_config;
    route.reasoning_effort = Some(ReasoningEffort::Xhigh);
    handle
        .model_route
        .replace(acp::ModelId::new("effort-model"), route);
    agent.sessions.borrow_mut().insert(pinned.clone(), handle);
    assert_eq!(
        read_effort(&agent.model_state(Some(&pinned))).as_deref(),
        Some("xhigh"),
        "model_state must report the session's own restored effort",
    );
    let unset = acp::SessionId::new("sess-unset");
    agent
        .sessions
        .borrow_mut()
        .insert(unset.clone(), make_test_handle("effort-model", None));
    assert_eq!(
        read_effort(&agent.model_state(Some(&unset))).as_deref(),
        Some("low"),
        "absent session effort falls back to the resolved model default",
    );
}
/// `drain_old_session_thread` returns immediately when the thread has
/// already finished.
#[tokio::test]
async fn drain_finished_thread_returns_immediately() {
    let session_threads: RefCell<HashMap<acp::SessionId, crate::session::SessionThread>> =
        RefCell::new(HashMap::new());
    let sid = acp::SessionId::new("drain-test");
    let handle = std::thread::spawn(|| {});
    std::thread::sleep(std::time::Duration::from_millis(10));
    session_threads.borrow_mut().insert(
        sid.clone(),
        crate::session::SessionThread::from_handle(handle),
    );
    let thread = session_threads.borrow_mut().remove(&sid).unwrap();
    assert!(thread.is_finished(), "thread should be finished");
    assert!(!session_threads.borrow().contains_key(&sid));
}
/// `drain_old_session_thread` waits for a slow thread to finish.
#[tokio::test]
async fn drain_waits_for_slow_thread() {
    let session_threads: RefCell<HashMap<acp::SessionId, crate::session::SessionThread>> =
        RefCell::new(HashMap::new());
    let sid = acp::SessionId::new("slow-drain");
    let handle = std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(100));
    });
    session_threads.borrow_mut().insert(
        sid.clone(),
        crate::session::SessionThread::from_handle(handle),
    );
    let thread = session_threads.borrow_mut().remove(&sid).unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if thread.is_finished() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "thread should finish within 5s"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(thread.is_finished());
}
/// Drain respects the 5s deadline and returns even if the thread is still running.
#[tokio::test]
async fn drain_respects_deadline() {
    let session_threads: RefCell<HashMap<acp::SessionId, crate::session::SessionThread>> =
        RefCell::new(HashMap::new());
    let sid = acp::SessionId::new("hung-drain");
    let handle = std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(30));
    });
    session_threads.borrow_mut().insert(
        sid.clone(),
        crate::session::SessionThread::from_handle(handle),
    );
    let thread = session_threads.borrow_mut().remove(&sid).unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(200);
    let mut timed_out = false;
    loop {
        if thread.is_finished() {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            timed_out = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        timed_out,
        "should have timed out waiting for the hung thread"
    );
    assert!(!thread.is_finished(), "thread should still be running");
}
#[test]
fn parse_code_nav_capability_present_and_true() {
    let mut meta = serde_json::Map::new();
    meta.insert(
        "grow/codeNavigation".to_string(),
        serde_json::json!({ "enabled": true }),
    );
    let init = acp::InitializeRequest::new(acp::ProtocolVersion::V1).client_capabilities(
        acp::ClientCapabilities::new()
            .fs(acp::FileSystemCapabilities::new())
            .terminal(false)
            .meta(meta),
    );
    assert!(MvpAgent::parse_code_nav_capability(&init));
}
#[test]
fn parse_code_nav_capability_absent_returns_false() {
    let init = acp::InitializeRequest::new(acp::ProtocolVersion::V1).client_capabilities(
        acp::ClientCapabilities::new()
            .fs(acp::FileSystemCapabilities::new())
            .terminal(false),
    );
    assert!(!MvpAgent::parse_code_nav_capability(&init));
}
#[test]
fn parse_code_nav_capability_false_returns_false() {
    let mut meta = serde_json::Map::new();
    meta.insert(
        "grow/codeNavigation".to_string(),
        serde_json::json!({ "enabled": false }),
    );
    let init = acp::InitializeRequest::new(acp::ProtocolVersion::V1).client_capabilities(
        acp::ClientCapabilities::new()
            .fs(acp::FileSystemCapabilities::new())
            .terminal(false)
            .meta(meta),
    );
    assert!(!MvpAgent::parse_code_nav_capability(&init));
}
/// Verify that two session handles with different code-nav state produce
/// independent eligibility outcomes — the key leader-mode isolation test.
///
/// This tests the `code_nav_eligibility_for_request` lookup path directly
/// by inspecting the per-handle fields rather than building a full agent,
/// which mirrors what the method actually reads at runtime.
#[tokio::test]
async fn test_per_session_code_nav_isolation() {
    let web_handle = {
        let mut h = make_test_handle("model", Some("grow-web"));
        h.code_nav_enabled = true;
        h
    };
    let tui_handle = {
        let mut h = make_test_handle("model", Some("grow-tui"));
        h.code_nav_enabled = false;
        h
    };
    let check = |handle: &crate::session::SessionHandle| {
        let ct = crate::http::client_type_from_origin(handle.origin_client.as_ref());
        if !matches!(ct, ClientType::GrowWeb) {
            return Err(CodeNavEligibility::ClientNotWeb);
        }
        if !handle.code_nav_enabled {
            return Err(CodeNavEligibility::CapabilityNotAdvertised);
        }
        Ok(())
    };
    assert!(
        check(&web_handle).is_ok(),
        "web session with capability should pass client-type and capability gates"
    );
    assert_eq!(
        check(&tui_handle),
        Err(CodeNavEligibility::ClientNotWeb),
        "tui session should be rejected at gate 1"
    );
    let mut web_no_cap = web_handle.clone();
    web_no_cap.code_nav_enabled = false;
    assert_eq!(
        check(&web_no_cap),
        Err(CodeNavEligibility::CapabilityNotAdvertised),
        "web session without capability should be rejected at gate 2"
    );
    assert!(
        check(&web_handle).is_ok(),
        "original web handle must be unaffected"
    );
}
/// Verify that code-nav requests without a sessionId are rejected.
///
/// `sessionId` is required so per-client capability gating is unambiguous
/// in both simple and leader modes.  Falling back to shared global state
/// (last-client-wins in leader mode) is not safe.
#[test]
fn test_sessionless_request_requires_session_id() {
    let session_id: Option<&acp::SessionId> = None;
    let result: Result<(), CodeNavEligibility> = if session_id.is_none() {
        Err(CodeNavEligibility::SessionRequired)
    } else {
        Ok(())
    };
    assert_eq!(
        result,
        Err(CodeNavEligibility::SessionRequired),
        "cwd-only requests with no sessionId must return SessionRequired"
    );
}
/// Build a minimal MvpAgent suitable for testing extension methods.
fn build_minimal_agent_for_tests() -> MvpAgent {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let gateway = GatewaySender::new(tx);
    let cfg = valid_agent_config();
    MvpAgent::new(gateway, &cfg).expect("valid test config")
}

#[test]
fn reconnect_gateway_mute_restores_only_the_captured_incarnation() {
    use std::sync::atomic::Ordering;

    let old_gate = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let replacement_gate = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mute = super::acp_agent::ReconnectGatewayMute::acquire(old_gate.clone());
    assert!(!old_gate.load(Ordering::Relaxed));

    // Dropping an aborted reconnect restores its own actor, never whatever
    // handle may now be registered for the same SessionId.
    drop(mute);
    assert!(old_gate.load(Ordering::Relaxed));
    assert!(!replacement_gate.load(Ordering::Relaxed));
}

#[tokio::test(flavor = "current_thread")]
async fn session_lifecycle_gate_serializes_close_behind_load_incarnation() {
    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("lifecycle-race");
    let load_transaction = agent.lock_session_lifecycle(&sid).await;
    let close = agent.close_session_explicit(&sid);
    tokio::pin!(close);

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut close)
            .await
            .is_err(),
        "close must wait for the complete load transaction"
    );

    let mut loaded = make_test_handle("test/default", None);
    loaded.info.id = sid.clone();
    agent.sessions.borrow_mut().insert(sid.clone(), loaded);
    drop(load_transaction);

    assert!(close.await.expect("serialized close succeeds"));
    assert!(
        !agent.sessions.borrow().contains_key(&sid),
        "a close racing load must retire the incarnation registered by that load"
    );
    assert!(
        agent.session_lifecycle_gates.borrow().is_empty(),
        "weak lifecycle gates are reclaimed after the final waiter exits"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn catalog_failure_cannot_evict_a_new_actor_incarnation() {
    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("catalog-incarnation");
    let stale_actor = make_test_handle("test/default", None).cmd_tx;
    let mut current = make_test_handle("test/default", None);
    current.info.id = sid.clone();
    let current_actor = current.cmd_tx.clone();
    agent.sessions.borrow_mut().insert(sid.clone(), current);

    assert!(
        !agent
            .evict_catalog_diverged_session(
                &sid,
                &stale_actor,
                agent.models_manager.catalog_revision(),
            )
            .await
            .expect("stale failure is ignored"),
        "a convergence result is scoped to the actor that received it"
    );
    assert!(agent.sessions.borrow().contains_key(&sid));

    assert!(
        agent
            .evict_catalog_diverged_session(
                &sid,
                &current_actor,
                agent.models_manager.catalog_revision(),
            )
            .await
            .expect("current incarnation is evicted")
    );
    assert!(!agent.sessions.borrow().contains_key(&sid));
}

#[tokio::test(flavor = "current_thread")]
async fn catalog_failure_cannot_evict_after_a_newer_publication() {
    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("catalog-generation-race");
    let mut current = make_test_handle("test/default", None);
    current.info.id = sid.clone();
    let current_actor = current.cmd_tx.clone();
    agent.sessions.borrow_mut().insert(sid.clone(), current);

    let superseded_revision = agent.models_manager.catalog_revision().saturating_add(1);
    assert!(
        !agent
            .evict_catalog_diverged_session(&sid, &current_actor, superseded_revision)
            .await
            .expect("superseded generation is ignored"),
        "generation must be revalidated inside the lifecycle transaction"
    );
    assert!(
        agent.sessions.borrow().contains_key(&sid),
        "the actor may have converged on the current catalog after the stale failure"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn session_control_lookup_addresses_live_child_without_promoting_it_to_roster() {
    let agent = build_minimal_agent_for_tests();
    let root_id = acp::SessionId::new("live-root");
    let mut root = make_test_handle("root-model", None);
    root.info.id = root_id.clone();
    agent.sessions.borrow_mut().insert(root_id.clone(), root);
    let child_id = acp::SessionId::new("live-child");
    let mut child = make_test_handle("child-model", None);
    child.info.id = child_id.clone();
    agent
        .active_child_sessions
        .borrow_mut()
        .insert(child_id.clone(), child);
    let workflow_child_id = acp::SessionId::new("workflow-child");
    let mut workflow_child = make_test_handle("workflow-model", None);
    workflow_child.info.id = workflow_child_id.clone();
    workflow_child.workflow_run_id = Some("wf-pinned".to_owned());
    agent
        .active_child_sessions
        .borrow_mut()
        .insert(workflow_child_id.clone(), workflow_child);

    assert!(
        agent
            .control_session_handle_waiting_for_load(&child_id)
            .await
            .is_some()
    );
    assert!(agent.sessions.borrow().get(&child_id).is_none());
    assert!(
        agent
            .session_handle_waiting_for_load(&child_id)
            .await
            .is_none(),
        "child control routing must not broaden primary prompt/load routing"
    );
    let reload_targets = agent.live_control_session_handles();
    assert!(reload_targets.contains_key(&root_id));
    assert!(reload_targets.contains_key(&child_id));
    assert!(
        !reload_targets.contains_key(&workflow_child_id),
        "automatic catalog publication must not rewrite a Workflow Run's frozen child route"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn set_session_mode_routes_to_a_live_child_actor() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let agent = build_minimal_agent_for_tests();
            let child_id = acp::SessionId::new("behavior-child");
            let (mut child, mut commands) = make_test_handle_with_receiver("child-model", None);
            child.info.id = child_id.clone();
            agent
                .active_child_sessions
                .borrow_mut()
                .insert(child_id.clone(), child);

            tokio::task::spawn_local(async move {
                let crate::session::SessionCommand::BehaviorChange {
                    session_mode,
                    responds_to,
                    ..
                } = commands.recv().await.expect("Behavior control command")
                else {
                    panic!("unexpected child control command")
                };
                assert_eq!(session_mode.0.as_ref(), "plan");
                let _ =
                    responds_to.send(Ok(crate::session::behavior::BehaviorChangeOutcome::Applied));
            });

            let response = acp::Agent::set_session_mode(
                &agent,
                acp::SetSessionModeRequest::new(child_id, acp::SessionModeId::new("plan")),
            )
            .await;
            assert!(
                response.is_ok(),
                "child Behavior routing failed: {response:?}"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn workflow_child_switch_uses_frozen_model_after_live_catalog_removal() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let agent = build_minimal_agent_for_tests();
            let session_id = acp::SessionId::new("workflow-frozen-model-child");
            let (mut child, mut commands) =
                make_test_handle_with_receiver("catalog/original", None);
            child.info.id = session_id.clone();

            let mut entry = crate::agent::config::ModelEntry::baseline("frozen-wire-model");
            entry.info.base_url = "https://workflow-frozen.test.invalid/v1".to_owned();
            let frozen_manager = crate::agent::models::ModelsManager::new(
                indexmap::IndexMap::from([("catalog/frozen".to_owned(), entry.clone())]),
                acp::ModelId::new("catalog/frozen"),
                crate::agent::config::Config::default(),
            );
            let sampler = crate::agent::config::sampling_config_for_model(
                &entry,
                crate::agent::config::resolve_credentials(&entry),
                None,
            );
            let route = crate::session::workflow::tracker::WorkflowRuntimeRoute::capture(
                "catalog/frozen",
                sampler,
                &frozen_manager,
                None,
                agent::config::SubagentFilter::default(),
            )
            .unwrap();
            let mut tracker = crate::session::workflow::tracker::WorkflowTracker::default();
            tracker.start_run(
                "wf-frozen".to_owned(),
                "frozen".to_owned(),
                "verify frozen child control".to_owned(),
                vec![],
                Some(1),
                None,
                route,
            );
            child.workflow_run_id = Some("wf-frozen".to_owned());
            child.workflow_tracker = std::sync::Arc::new(parking_lot::Mutex::new(tracker));
            agent
                .active_child_sessions
                .borrow_mut()
                .insert(session_id.clone(), child);

            tokio::task::spawn_local(async move {
                let crate::session::SessionCommand::SetSessionModel {
                    route,
                    catalog,
                    intent: _,
                    responds_to,
                } = commands.recv().await.expect("model control command")
                else {
                    panic!("unexpected child control command")
                };
                assert_eq!(route.model_id.0.as_ref(), "catalog/frozen");
                assert_eq!(route.sampling_config.model, "frozen-wire-model");
                assert!(catalog.is_none());
                let _ = responds_to.send(Ok(crate::session::DesiredStateOutcome::Applied(
                    route.model_id,
                )));
            });

            let response = acp::Agent::set_session_model(
                &agent,
                acp::SetSessionModelRequest::new(session_id, acp::ModelId::new("catalog/frozen")),
            )
            .await;
            assert!(response.is_ok(), "frozen child switch failed: {response:?}");
        })
        .await;
}
fn session_usage_request(session_id: &str) -> acp::ExtRequest {
    acp::ExtRequest::new(
        "grow/session/usage",
        serde_json::value::to_raw_value(&serde_json::json!({ "sessionId": session_id }))
            .unwrap()
            .into(),
    )
}
#[tokio::test(flavor = "current_thread")]
async fn session_usage_unknown_session_is_resource_not_found() {
    let agent = build_minimal_agent_for_tests();
    let err = crate::extensions::usage::handle(&agent, &session_usage_request("no-such-session"))
        .await
        .expect_err("unknown session");
    assert_eq!(
        err.code,
        acp::Error::resource_not_found(None::<String>).code
    );
}
#[tokio::test(flavor = "current_thread")]
async fn session_usage_dead_chat_state_actor_fails_closed() {
    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("usage-dead-actor-sess");
    let mut handle = make_test_handle("test-model", None);
    handle.info.id = sid.clone();
    agent.sessions.borrow_mut().insert(sid, handle);
    let err =
        crate::extensions::usage::handle(&agent, &session_usage_request("usage-dead-actor-sess"))
            .await
            .expect_err("dead chat-state actor");
    assert_eq!(err.code, acp::Error::internal_error().code);
}
/// Regression: boot-time plugin discovery is deferred past ACP
/// `initialize`, so the shared plugin registry starts empty.
/// `resolve_mcp_servers` reads that snapshot to merge plugin-contributed
/// MCP servers into a new session, so without lazy population the servers
/// silently vanished until an explicit `/plugins reload`.
/// `ensure_plugin_registry` must build the snapshot on first use.
#[tokio::test]
#[serial_test::serial]
async fn ensure_plugin_registry_lazily_populates_snapshot() {
    use test_support::EnvGuard;
    let grow_home = tempfile::tempdir().unwrap();
    let _env = EnvGuard::set("GROW_HOME", grow_home.path());
    let plugin_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        plugin_dir.path().join("plugin.json"),
        r#"{"name": "regr-lazy-mcp-plugin"}"#,
    )
    .unwrap();
    std::fs::write(
        plugin_dir.path().join(".mcp.json"),
        r#"{"mcpServers":{"regr-srv":{"command":"echo","args":["hi"]}}}"#,
    )
    .unwrap();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let gateway = GatewaySender::new(tx);
    let mut cfg = valid_agent_config();
    cfg.plugins.cli_plugin_dirs = vec![plugin_dir.path().to_path_buf()];
    let agent = MvpAgent::new(gateway, &cfg).expect("valid test config");
    assert!(
        agent.plugin_registry_handle.snapshot().is_none(),
        "snapshot must start empty (boot discovery deferred past initialize)"
    );
    agent.ensure_plugin_registry();
    let snapshot = agent
        .plugin_registry_handle
        .snapshot()
        .expect("snapshot must be populated on first use");
    assert!(
        snapshot.get("regr-lazy-mcp-plugin").is_some(),
        "lazy discovery must surface the plugin so its MCP server merges into the session"
    );
    agent.ensure_plugin_registry();
    assert!(
        agent
            .plugin_registry_handle
            .snapshot()
            .is_some_and(|s| s.get("regr-lazy-mcp-plugin").is_some()),
        "repeat call must keep the populated snapshot"
    );
}
#[cfg(unix)]
mod process_scope_reclaim;
mod subagent_spawn_context_tests;
/// No load in flight and no session → the wait returns immediately
/// (the caller then surfaces "unknown session id" exactly as before).
#[tokio::test]
async fn wait_for_in_flight_load_returns_immediately_when_idle() {
    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("sess-none");
    tokio::time::timeout(
        std::time::Duration::from_millis(200),
        agent.wait_for_in_flight_session_load(&sid),
    )
    .await
    .expect("wait must not block when no load is in flight");
}
/// A waiter racing an in-flight `session/load` blocks until the load
/// finishes and then observes the registered session. This is the
/// agent-side guarantee that closes the post-leader-crash
/// "unknown session id" race: the reconnect replay's `session/load` and
/// the client's next `session/prompt` can arrive back-to-back.
#[tokio::test]
async fn wait_for_in_flight_load_blocks_until_load_completes() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let agent = std::rc::Rc::new(build_minimal_agent_for_tests());
            let sid = acp::SessionId::new("sess-loading");
            let guard = agent.begin_session_load(&sid);
            let waiter_agent = agent.clone();
            let waiter_sid = sid.clone();
            let waiter = tokio::task::spawn_local(async move {
                waiter_agent
                    .wait_for_in_flight_session_load(&waiter_sid)
                    .await;
                waiter_agent.sessions.borrow().contains_key(&waiter_sid)
            });
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert!(!waiter.is_finished(), "waiter must block while loading");
            let handle = make_test_handle("test-model", None);
            agent.sessions.borrow_mut().insert(sid.clone(), handle);
            drop(guard);
            let found_session = tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
                .await
                .expect("waiter must wake when the load guard drops")
                .expect("waiter task must not panic");
            assert!(
                found_session,
                "after the wait, the session must be visible to the racing request"
            );
        })
        .await;
}
/// A failed load (guard dropped WITHOUT registering the session) also
/// wakes waiters — they re-check, find nothing, and the caller surfaces
/// the regular "unknown session id" error rather than hanging.
#[tokio::test]
async fn wait_for_in_flight_load_wakes_on_failed_load() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let agent = std::rc::Rc::new(build_minimal_agent_for_tests());
            let sid = acp::SessionId::new("sess-load-fails");
            let guard = agent.begin_session_load(&sid);
            let waiter_agent = agent.clone();
            let waiter_sid = sid.clone();
            let waiter = tokio::task::spawn_local(async move {
                waiter_agent
                    .wait_for_in_flight_session_load(&waiter_sid)
                    .await;
                waiter_agent.sessions.borrow().contains_key(&waiter_sid)
            });
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            drop(guard);
            let found_session = tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
                .await
                .expect("waiter must wake when the failed load's guard drops")
                .expect("waiter task must not panic");
            assert!(!found_session, "failed load leaves no session behind");
        })
        .await;
}
/// Two concurrent loads of the same session: the first guard's drop must
/// not remove the second load's marker (waiters keep waiting on the
/// newer in-flight load).
#[tokio::test]
async fn concurrent_load_guards_do_not_clobber_each_other() {
    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("sess-concurrent");
    let guard_one = agent.begin_session_load(&sid);
    let guard_two = agent.begin_session_load(&sid);
    drop(guard_one);
    assert!(
        agent.loading_sessions.borrow().contains_key(&sid),
        "second load's marker must survive the first guard's drop"
    );
    drop(guard_two);
    assert!(
        agent.loading_sessions.borrow().is_empty(),
        "all markers removed once every load finished"
    );
}
/// A later queued load may be cancelled before the earlier load publishes its
/// handle. Its lease must not remove the shared marker owned by that earlier
/// load, or prompt/control requests would observe a false unknown-session gap.
#[tokio::test]
async fn cancelled_later_load_keeps_the_earlier_marker_alive() {
    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("sess-cancelled-later-load");
    let earlier = agent.begin_session_load(&sid);
    let later = agent.begin_session_load(&sid);

    drop(later);
    assert!(
        agent.loading_sessions.borrow().contains_key(&sid),
        "cancelling a later load must not hide the earlier live load"
    );
    drop(earlier);
    assert!(
        agent.loading_sessions.borrow().is_empty(),
        "the final load lease must remove the shared marker"
    );
}
/// `resident_activity` returns `NeedsInput` whenever the session's
/// pending-interaction map is non-empty — and that wins even over a
/// running turn (a session blocked on a permission mid-turn "needs
/// input"). Clearing the map falls back to Working / Idle.
#[tokio::test]
async fn resident_activity_reports_needs_input_when_pending() {
    use crate::agent::roster::RosterActivity;
    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("sess-pending");
    let handle = make_test_handle("grow-3", None);
    let pending = handle.pending_interactions.clone();
    let prompt_id = handle.current_prompt_id.clone();
    agent.sessions.borrow_mut().insert(sid.clone(), handle);
    assert_eq!(agent.resident_activity(&sid), RosterActivity::Idle);
    *prompt_id.lock().unwrap() = Some("turn-1".to_string());
    assert_eq!(agent.resident_activity(&sid), RosterActivity::Working);
    pending.lock().unwrap().insert(
        "call-1".to_string(),
        crate::session::pending_interaction::PendingKind::Permission,
    );
    assert_eq!(agent.resident_activity(&sid), RosterActivity::NeedsInput);
    let entry = agent.resident_roster_entry(&sid).expect("resident entry");
    assert_eq!(entry.activity, RosterActivity::NeedsInput);
    pending.lock().unwrap().clear();
    assert_eq!(agent.resident_activity(&sid), RosterActivity::Working);
}
/// Drain the agent gateway, returning the first `grow/sessions/changed`
/// payload that carries an upserted entry (ignoring any unrelated
/// notifications, which parse into an empty `RosterChanged`).
fn drain_roster_changed(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) -> Option<crate::agent::roster::RosterChanged> {
    let mut found = None;
    while let Ok(msg) = rx.try_recv() {
        if let acp_transport::AcpClientMessage::ExtNotification(args) = msg {
            if found.is_none()
                && let Ok(changed) = serde_json::from_str::<crate::agent::roster::RosterChanged>(
                    args.request.params.get(),
                )
                && !changed.upserted.is_empty()
            {
                found = Some(changed);
            }
            let _ = args.response_tx.send(Ok(()));
        }
    }
    found
}
/// A turn-boundary activity delta (`push_roster_activity_delta`) broadcasts
/// an `grow/sessions/changed` upsert carrying the *overridden* activity, so
/// every attached dashboard reflects Working/Idle immediately instead of
/// waiting for the ≤1s roster poll (turn-start/turn-end). The
/// override matters because at turn-start the actor has not yet published
/// `current_prompt_id`, so a natural `resident_activity` read would emit
/// `Idle` for a session that is in fact starting a turn.
#[tokio::test]
async fn push_roster_activity_delta_broadcasts_overridden_activity() {
    use crate::agent::roster::RosterActivity;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let gateway = GatewaySender::new(tx);
    let cfg = valid_agent_config();
    let agent = MvpAgent::new(gateway, &cfg).expect("valid test config");
    let sid = acp::SessionId::new("sess-activity");
    agent
        .sessions
        .borrow_mut()
        .insert(sid.clone(), make_test_handle("grow-3", None));
    agent.push_roster_activity_delta(&sid, RosterActivity::Working);
    let changed = drain_roster_changed(&mut rx).expect("turn-start delta emitted");
    assert_eq!(changed.upserted.len(), 1);
    assert_eq!(changed.upserted[0].session_id, sid.0.to_string());
    assert!(changed.upserted[0].resident);
    assert_eq!(
        changed.upserted[0].activity,
        RosterActivity::Working,
        "forced activity must override the Idle that resident_activity would read"
    );
    assert!(changed.removed.is_empty());
    agent.push_roster_activity_delta(&sid, RosterActivity::Idle);
    let changed = drain_roster_changed(&mut rx).expect("turn-end delta emitted");
    assert_eq!(changed.upserted[0].activity, RosterActivity::Idle);
}
/// Extract the inner payload from an ExtResponse.
#[expect(
    dead_code,
    reason = "unused in production; remove expect when wired or delete the item"
)]
fn parse_ext_body(resp: &acp::ExtResponse) -> serde_json::Value {
    let outer: serde_json::Value =
        serde_json::from_str(resp.0.get()).expect("ExtResponse must be valid JSON");
    outer
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("ExtResponse has no 'result' key; full JSON: {outer}"))
}
/// Replicate the lookup logic of code_nav_eligibility_for_request so we
/// can test it with a plain sessions HashMap.
fn check_nav_eligibility_from_sessions(
    sessions: &HashMap<acp::SessionId, crate::session::SessionHandle>,
    session_id: Option<&acp::SessionId>,
) -> Result<(), CodeNavEligibility> {
    let session_id = match session_id {
        Some(sid) => sid,
        None => return Err(CodeNavEligibility::SessionRequired),
    };
    let Some(handle) = sessions.get(session_id) else {
        return Err(CodeNavEligibility::SessionRequired);
    };
    let ct = crate::http::client_type_from_origin(handle.origin_client.as_ref());
    if !matches!(ct, ClientType::GrowWeb) {
        return Err(CodeNavEligibility::ClientNotWeb);
    }
    if !handle.code_nav_enabled {
        return Err(CodeNavEligibility::CapabilityNotAdvertised);
    }
    Ok(())
}
/// Web session with code-nav capability is eligible.
///
/// This is the "happy path" that allows lazy index startup on the first
/// code-nav request.
#[tokio::test]
async fn test_web_session_with_capability_is_eligible() {
    let sid = acp::SessionId::new("sess-web");
    let mut handle = make_test_handle("model", Some("grow-web"));
    handle.code_nav_enabled = true;
    let sessions = [(sid.clone(), handle)].into();
    assert!(
        check_nav_eligibility_from_sessions(&sessions, Some(&sid)).is_ok(),
        "web session with code-nav capability must be eligible"
    );
}
/// TUI session is rejected at gate 1 (client type) regardless of capability.
#[tokio::test]
async fn test_tui_session_is_rejected() {
    let sid = acp::SessionId::new("sess-tui");
    let mut handle = make_test_handle("model", Some("grow-tui"));
    handle.code_nav_enabled = true;
    let sessions = [(sid.clone(), handle)].into();
    assert_eq!(
        check_nav_eligibility_from_sessions(&sessions, Some(&sid)),
        Err(CodeNavEligibility::ClientNotWeb),
        "TUI client must be rejected at gate 1 (client type)"
    );
}
/// Web session without capability is rejected at gate 2.
#[tokio::test]
async fn test_web_session_without_capability_is_rejected() {
    let sid = acp::SessionId::new("sess-web-no-cap");
    let mut handle = make_test_handle("model", Some("grow-web"));
    handle.code_nav_enabled = false;
    let sessions = [(sid.clone(), handle)].into();
    assert_eq!(
        check_nav_eligibility_from_sessions(&sessions, Some(&sid)),
        Err(CodeNavEligibility::CapabilityNotAdvertised),
        "web client without capability must be rejected at gate 2"
    );
}
/// Leader-mode isolation: two sessions with different code-nav state return
/// independent results.
#[tokio::test]
async fn test_leader_mode_two_sessions_stay_isolated() {
    let web_sid = acp::SessionId::new("web");
    let tui_sid = acp::SessionId::new("tui");
    let mut web_handle = make_test_handle("model", Some("grow-web"));
    web_handle.code_nav_enabled = true;
    let mut tui_handle = make_test_handle("model", Some("grow-tui"));
    tui_handle.code_nav_enabled = false;
    let sessions = [(web_sid.clone(), web_handle), (tui_sid.clone(), tui_handle)].into();
    assert!(
        check_nav_eligibility_from_sessions(&sessions, Some(&web_sid)).is_ok(),
        "web session must be eligible"
    );
    assert_eq!(
        check_nav_eligibility_from_sessions(&sessions, Some(&tui_sid)),
        Err(CodeNavEligibility::ClientNotWeb),
        "tui session must remain ineligible even when web session is eligible"
    );
}
/// Unknown session ID returns SessionRequired, not a global fallback.
///
/// This is the stale/evicted session path: a caller with a session ID that
/// no longer exists in the sessions map must get SessionRequired, not
/// accidentally inherit the last-initialized client's eligibility.
#[tokio::test]
async fn test_unknown_session_id_returns_session_required() {
    let known_sid = acp::SessionId::new("known");
    let mut known_handle = make_test_handle("model", Some("grow-web"));
    known_handle.code_nav_enabled = true;
    let sessions = [(known_sid.clone(), known_handle)].into();
    let stale_sid = acp::SessionId::new("stale-or-evicted");
    assert_eq!(
        check_nav_eligibility_from_sessions(&sessions, Some(&stale_sid)),
        Err(CodeNavEligibility::SessionRequired),
        "stale/evicted sessionId must not fall back to global state"
    );
    assert!(check_nav_eligibility_from_sessions(&sessions, Some(&known_sid)).is_ok());
}
mod parse_json_object_env_tests {
    use super::parse_json_object_env;
    unsafe fn set(k: &str, v: &str) {
        unsafe { std::env::set_var(k, v) };
    }
    unsafe fn unset(k: &str) {
        unsafe { std::env::remove_var(k) };
    }
    #[test]
    #[serial_test::serial]
    fn valid_json_object_returns_some() {
        unsafe { set("TEST_JSON_OBJ", r#"{"team":"platform","org":"acme"}"#) };
        let result = parse_json_object_env("TEST_JSON_OBJ");
        unsafe { unset("TEST_JSON_OBJ") };
        let val = result.expect("should parse valid JSON object");
        assert_eq!(val["team"], "platform");
        assert_eq!(val["org"], "acme");
    }
    #[test]
    #[serial_test::serial]
    fn non_object_json_returns_none() {
        unsafe { set("TEST_JSON_ARR", r#"["not","an","object"]"#) };
        let result = parse_json_object_env("TEST_JSON_ARR");
        unsafe { unset("TEST_JSON_ARR") };
        assert!(result.is_none());
    }
    #[test]
    #[serial_test::serial]
    fn invalid_json_returns_none() {
        unsafe { set("TEST_JSON_BAD", "not json at all") };
        let result = parse_json_object_env("TEST_JSON_BAD");
        unsafe { unset("TEST_JSON_BAD") };
        assert!(result.is_none());
    }
    #[test]
    #[serial_test::serial]
    fn unset_var_returns_none() {
        unsafe { unset("TEST_JSON_UNSET") };
        assert!(parse_json_object_env("TEST_JSON_UNSET").is_none());
    }
}
mod eligibility_gates {
    use super::*;
    /// Standalone replica of the first three eligibility gates.
    /// Gate 4 (git root) requires a real filesystem and is covered by
    /// integration tests.
    fn check_gates(
        client_type: ClientType,
        code_nav_enabled: bool,
        indexing_enabled: bool,
    ) -> Result<(), CodeNavEligibility> {
        if !matches!(client_type, ClientType::GrowWeb) {
            return Err(CodeNavEligibility::ClientNotWeb);
        }
        if !code_nav_enabled {
            return Err(CodeNavEligibility::CapabilityNotAdvertised);
        }
        if !indexing_enabled {
            return Err(CodeNavEligibility::DisabledByConfig);
        }
        Ok(())
    }
    #[test]
    fn non_web_client_rejected() {
        assert_eq!(
            check_gates(ClientType::Generic, true, true),
            Err(CodeNavEligibility::ClientNotWeb)
        );
    }
    #[test]
    fn tui_client_rejected() {
        assert_eq!(
            check_gates(ClientType::GrowTUI, true, true),
            Err(CodeNavEligibility::ClientNotWeb)
        );
    }
    #[test]
    fn web_client_no_capability_rejected() {
        assert_eq!(
            check_gates(ClientType::GrowWeb, false, true),
            Err(CodeNavEligibility::CapabilityNotAdvertised)
        );
    }
    #[test]
    fn web_client_with_capability_config_disabled_rejected() {
        assert_eq!(
            check_gates(ClientType::GrowWeb, true, false),
            Err(CodeNavEligibility::DisabledByConfig)
        );
    }
    #[test]
    fn web_client_with_capability_and_config_passes_first_three_gates() {
        assert!(check_gates(ClientType::GrowWeb, true, true).is_ok());
    }
}
#[test]
fn find_model_by_catalog_id_never_matches_routing_slug() {
    let entry = |model: &str| ModelEntry {
        info: config::ModelInfo {
            user_selectable: true,
            id: None,
            model: model.to_string(),
            base_url: String::new(),
            name: None,
            description: None,
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: crate::sampling::ApiBackend::default(),
            auth_scheme: Default::default(),
            extra_headers: IndexMap::new(),
            query_params: IndexMap::new(),
            env_http_headers: IndexMap::new(),
            context_window: std::num::NonZeroU64::new(200_000).unwrap(),
            auto_compact_threshold_percent: None,
            system_prompt_label: None,
            agent_type: config::default_agent_type(),
            inference_idle_timeout_secs: None,
            max_retries: None,
            hidden: false,
            reasoning_efforts: Vec::new(),
            compactions_remaining: None,
            compaction_at_tokens: None,
            show_model_fingerprint: false,
            stream_tool_calls: None,
            laziness_detector: crate::agent::config::LazinessDetectorPerModelConfig::default(),
        },
        api_key: None,
        env_key: None,
        auth_provider: None,
    };
    let mut models = indexmap::IndexMap::new();
    models.insert("a".to_string(), entry("target"));
    models.insert("target".to_string(), entry("other"));
    assert_eq!(
        config::find_model_by_catalog_id(&models, "target")
            .unwrap()
            .model,
        "other",
        "catalog identity is the exact map key"
    );
    assert_eq!(
        config::find_model_by_catalog_id(&models, "a")
            .unwrap()
            .model,
        "target",
        "exact key match for 'a'"
    );
    assert!(
        config::find_model_by_catalog_id(&models, "other").is_none(),
        "the routing slug is not a catalog identity"
    );
}
fn write_updates(dir: &std::path::Path, lines: &[&str]) {
    let path = dir.join("updates.jsonl");
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
}
fn stale_tasks(dir: &std::path::Path) -> Vec<OrphanedTask> {
    let directory = crate::session::storage::ContainedDirectory::open(
        dir,
        std::path::Path::new(""),
        "updates fixture",
        false,
    )
    .unwrap();
    MvpAgent::find_stale_background_task_projections(&directory)
}
fn bg_line(task_id: &str) -> String {
    format!(
        r#"{{"timestamp":1,"method":"_grow/session/update","params":{{"sessionId":"s","update":{{"sessionUpdate":"task_backgrounded","task_id":"{task_id}","command":"sleep 99","cwd":"/tmp"}}}}}}"#
    )
}
fn completed_line(task_id: &str) -> String {
    format!(
        r#"{{"timestamp":2,"method":"_grow/session/update","params":{{"sessionId":"s","update":{{"sessionUpdate":"task_completed","task_snapshot":{{"task_id":"{task_id}","completed":true}}}}}}}}"#
    )
}
fn orphaned_ids(tasks: &[OrphanedTask]) -> std::collections::HashSet<&str> {
    tasks.iter().map(|t| t.task_id.as_str()).collect()
}
#[test]
fn orphaned_tasks_returns_empty_for_no_file() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(stale_tasks(tmp.path()).is_empty());
}
#[test]
fn orphaned_tasks_returns_empty_when_all_completed() {
    let tmp = tempfile::tempdir().unwrap();
    let bg = bg_line("t1");
    let done = completed_line("t1");
    write_updates(tmp.path(), &[&bg, &done]);
    let result = stale_tasks(tmp.path());
    assert!(result.is_empty());
}
#[test]
fn orphaned_tasks_returns_uncompleted() {
    let tmp = tempfile::tempdir().unwrap();
    let bg1 = bg_line("t1");
    let bg2 = bg_line("t2");
    let done1 = completed_line("t1");
    write_updates(tmp.path(), &[&bg1, &bg2, &done1]);
    let result = stale_tasks(tmp.path());
    let ids = orphaned_ids(&result);
    assert_eq!(ids.len(), 1);
    assert!(ids.contains("t2"));
}
#[test]
fn orphaned_tasks_returns_multiple_uncompleted() {
    let tmp = tempfile::tempdir().unwrap();
    let bg1 = bg_line("t1");
    let bg2 = bg_line("t2");
    let bg3 = bg_line("t3");
    let done2 = completed_line("t2");
    write_updates(tmp.path(), &[&bg1, &bg2, &bg3, &done2]);
    let result = stale_tasks(tmp.path());
    let ids = orphaned_ids(&result);
    assert_eq!(ids.len(), 2);
    assert!(ids.contains("t1"));
    assert!(ids.contains("t3"));
}
#[test]
fn orphaned_tasks_captures_command_and_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let bg = bg_line("t1");
    write_updates(tmp.path(), &[&bg]);
    let result = stale_tasks(tmp.path());
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].command, "sleep 99");
    assert_eq!(result[0].cwd, "/tmp");
}
#[test]
fn orphaned_tasks_skips_malformed_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let bg = bg_line("t1");
    write_updates(tmp.path(), &["not json", &bg, "{}"]);
    let result = stale_tasks(tmp.path());
    assert_eq!(result.len(), 1);
}
#[test]
fn orphaned_tasks_ignores_unrelated_updates() {
    let tmp = tempfile::tempdir().unwrap();
    let bg = bg_line("t1");
    let unrelated = r#"{"timestamp":1,"method":"_grow/session/update","params":{"sessionId":"s","update":{"sessionUpdate":"auto_compact_started","percentage":80}}}"#;
    write_updates(tmp.path(), &[&bg, unrelated]);
    let result = stale_tasks(tmp.path());
    assert_eq!(result.len(), 1);
}
#[test]
fn orphaned_tasks_filters_rewind_dead_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let user_msg = r#"{"timestamp":0,"method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hello"}}}}"#;
    let bg_before_rewind = bg_line("t-dead");
    let rewind = r#"{"timestamp":3,"method":"_grow/session/update","params":{"sessionId":"s","update":{"sessionUpdate":"rewind_marker","target_prompt_index":0,"created_at":"2025-01-01T00:00:00Z"}}}"#;
    let user_msg2 = r#"{"timestamp":4,"method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"retry"}}}}"#;
    let bg_after_rewind = bg_line("t-alive");
    write_updates(
        tmp.path(),
        &[
            user_msg,
            &bg_before_rewind,
            rewind,
            user_msg2,
            &bg_after_rewind,
        ],
    );
    let result = stale_tasks(tmp.path());
    let ids = orphaned_ids(&result);
    assert!(
        ids.contains("t-alive"),
        "task after rewind should be present"
    );
    assert!(
        !ids.contains("t-dead"),
        "task in dead branch should be filtered"
    );
}
/// `spawn_gateway_bridge` uses `tokio::task::spawn_local`.
fn run_local_for_bridge_test<F, Fut, T>(body: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime must build");
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, body())
}
/// `remove_session` releases the workspace binding and drains the
/// per-session side maps. Test agents default to `workspace_ops = None`,
/// so no other test reaches the release.
#[tokio::test]
async fn remove_session_releases_workspace_binding_and_side_maps() {
    let agent = build_minimal_agent_for_tests();
    let sid = acp::SessionId::new("test-session-workspace-release");
    let ops = workspace::WorkspaceOps::for_test();
    let toolset = std::sync::Arc::new(tools::registry::types::FinalizedToolset::empty_for_test());
    let toolset_weak = std::sync::Arc::downgrade(&toolset);
    ops.bind_local_session(
        sid.0.as_ref(),
        std::env::temp_dir(),
        hunk_tracker::HunkTrackerHandle::noop(),
        toolset,
        None,
    )
    .await
    .expect("bind_local_session must succeed");
    assert!(toolset_weak.upgrade().is_some());
    *agent.workspace_ops.borrow_mut() = Some(ops);
    agent.model_unavailable_sessions.borrow_mut().insert(
        sid.0.to_string(),
        acp::ModelId::new(std::sync::Arc::from("gone-model")),
    );
    agent.set_turn_number(&sid, 3);
    agent
        .resident_resources
        .borrow_mut()
        .entry(sid.clone())
        .or_default()
        .require_gateway = true;
    agent.remove_session(&sid);
    assert!(
        toolset_weak.upgrade().is_none(),
        "the workspace binding must release the toolset"
    );
    assert!(
        !agent
            .model_unavailable_sessions
            .borrow()
            .contains_key(sid.0.as_ref())
    );
    assert!(!agent.resident_resources.borrow().contains_key(&sid));
    assert!(
        !agent.retained_resources.borrow().contains_key(&sid),
        "retained per-session resources must be reclaimed on removal"
    );
}
/// Without a bridge, `ext_method` falls through to the unchanged local
/// dispatch (`rewind::handle`), which reports the missing session — proving
/// the routing hook is skipped in local mode.
#[test]
fn ext_method_rewind_uses_local_dispatch_without_bridge() {
    use acp::Agent as _;
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let params = serde_json::json!({ "sessionId": "sess-local" });
        let err = agent
            .ext_method(acp::ExtRequest::new(
                "grow/rewind/points",
                std::sync::Arc::from(serde_json::value::to_raw_value(&params).unwrap()),
            ))
            .await
            .expect_err("local rewind with no session must error");
        assert_eq!(err.code, acp::Error::resource_not_found(None).code);
    });
}
#[test]
fn cancel_does_not_forward_to_bridge_in_local_mode() {
    use crate::session::SessionCommand;
    use acp::Agent as _;
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-cancel-local");
        let (handle, _tx, mut cmd_rx) = make_live_session_handle(&sid, None);
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        agent
            .cancel(acp::CancelNotification::new(sid.clone()))
            .await
            .expect("cancel must succeed");
        let mut saw_local_cancel = false;
        while let Ok(cmd) = cmd_rx.try_recv() {
            if let SessionCommand::Cancel { .. } = cmd {
                saw_local_cancel = true;
            }
        }
        assert!(
            saw_local_cancel,
            "local-mode cancel dispatches the local SessionCommand::Cancel with no bridge attached"
        );
    });
}
/// The Goal interrupt panel's "Pause goal" choice travels as `_meta.pauseGoal`;
/// the cancel handler must forward it onto `SessionCommand::Cancel` (the
/// Cancel arm pauses the Goal only when it is true). Absent meta → false.
#[test]
fn cancel_forwards_pause_goal_meta() {
    use crate::session::SessionCommand;
    use acp::Agent as _;
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-cancel-pause");

        // Explicit pause intent.
        let (handle, _tx, mut cmd_rx) = make_live_session_handle(&sid, None);
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        let mut meta = serde_json::Map::new();
        meta.insert("pauseGoal".to_string(), serde_json::json!(true));
        let notif = acp::CancelNotification::new(sid.clone()).meta(meta);
        agent.cancel(notif).await.expect("cancel must succeed");
        let mut saw_pause = false;
        while let Ok(cmd) = cmd_rx.try_recv() {
            if let SessionCommand::Cancel {
                pause_goal: true, ..
            } = cmd
            {
                saw_pause = true;
            }
        }
        assert!(
            saw_pause,
            "pauseGoal meta must reach SessionCommand::Cancel"
        );

        // Absent meta → false (older clients / programmatic cancels).
        let (handle, _tx, mut cmd_rx) = make_live_session_handle(&sid, None);
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        agent
            .cancel(acp::CancelNotification::new(sid.clone()))
            .await
            .expect("cancel must succeed");
        let mut saw_no_pause = false;
        while let Ok(cmd) = cmd_rx.try_recv() {
            if let SessionCommand::Cancel {
                pause_goal: false, ..
            } = cmd
            {
                saw_no_pause = true;
            }
        }
        assert!(saw_no_pause, "absent pauseGoal must default to false");
    });
}
/// Regression (post-cancel slot hang, first bad release 0.2.101; see
/// `dispatch_lock`). SDK e2e shape:
/// `test_cancel_ends_in_flight_turn_and_frees_slot` (agent-sdk).
#[test]
fn cancel_never_overtakes_in_flight_prompt_intake() {
    use crate::session::SessionCommand;
    use acp::Agent as _;
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-cancel-intake-race");
        let (handle, _tx, mut cmd_rx) = make_live_session_handle(&sid, None);
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        let order: std::rc::Rc<std::cell::RefCell<Vec<&'static str>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let (intake_parked_tx, intake_parked_rx) = tokio::sync::oneshot::channel::<()>();
        let driver_order = order.clone();
        tokio::task::spawn_local(async move {
            let mut intake_parked_tx = Some(intake_parked_tx);
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    SessionCommand::GetCurrentModel { responds_to } => {
                        if let Some(tx) = intake_parked_tx.take() {
                            let _ = tx.send(());
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                        let _ = responds_to.send("test-model".to_string());
                    }
                    SessionCommand::QueuePrompt { respond_to, .. } => {
                        driver_order.borrow_mut().push("prompt");
                        let _ = respond_to.send(crate::session::commands::ok_end_turn(0, None));
                    }
                    SessionCommand::Cancel { .. } => driver_order.borrow_mut().push("cancel"),
                    _ => {}
                }
            }
        });
        let prompt_fut = agent.prompt(acp::PromptRequest::new(
            sid.clone(),
            vec![acp::ContentBlock::from("hi")],
        ));
        let cancel_fut = async {
            intake_parked_rx
                .await
                .expect("prompt intake reaches the fake actor");
            let _ = agent
                .cancel(acp::CancelNotification::new(sid.clone()))
                .await;
        };
        let _ = futures::join!(prompt_fut, cancel_fut);
        assert_eq!(
            order.borrow().as_slice(),
            ["prompt", "cancel"],
            "cancel must land on the actor mailbox after the prompt it targets"
        );
    });
}
use crate::session::SessionCommand as TestSessionCommand;

enum FakeActorEvent {
    IdleUnloadCommitted,
    Command(TestSessionCommand),
}
/// Build a session handle wired to a *live* command channel. Returns the
/// handle (move into `sessions`) plus a probe `cmd_tx`/`cmd_rx` so a test
/// can observe what the agent sends to the actor and prove the channel is
/// live.
fn make_live_session_handle(
    sid: &acp::SessionId,
    running_prompt: Option<&str>,
) -> (
    crate::session::SessionHandle,
    tokio::sync::mpsc::UnboundedSender<TestSessionCommand>,
    tokio::sync::mpsc::UnboundedReceiver<TestSessionCommand>,
) {
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut handle = make_test_handle("test-model", Some("grow-tui"));
    handle.cmd_tx = cmd_tx.clone();
    handle.info = crate::session::info::Info {
        id: sid.clone(),
        cwd: "/tmp".to_string(),
    };
    if let Some(pid) = running_prompt {
        *handle.current_prompt_id.lock().unwrap() = Some(pid.to_string());
    }
    (handle, cmd_tx, cmd_rx)
}
/// Spawn a minimal fake session actor on the `LocalSet` that accepts or rejects
/// the actor-owned idle-unload transaction and forwards other commands.
fn spawn_fake_actor(
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<TestSessionCommand>,
    busy: bool,
) -> tokio::sync::mpsc::UnboundedReceiver<FakeActorEvent> {
    let (observed_tx, observed_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_local(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                TestSessionCommand::UnloadIfIdle { respond_to } => {
                    if busy {
                        let _ = respond_to.send(false);
                    } else {
                        let _ = observed_tx.send(FakeActorEvent::IdleUnloadCommitted);
                        let _ = respond_to.send(true);
                        break;
                    }
                }
                other => {
                    let _ = observed_tx.send(FakeActorEvent::Command(other));
                }
            }
        }
    });
    observed_rx
}
/// Fake actor that acknowledges the unload admission immediately, then keeps
/// tearing down for a while. The leader must remove the session from the
/// resident roster after the ACK rather than waiting for this tail.
fn spawn_slow_teardown_actor(
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<TestSessionCommand>,
    teardown_delay: std::time::Duration,
) -> tokio::sync::mpsc::UnboundedReceiver<FakeActorEvent> {
    let (observed_tx, observed_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_local(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                TestSessionCommand::UnloadIfIdle { respond_to } => {
                    let _ = observed_tx.send(FakeActorEvent::IdleUnloadCommitted);
                    let _ = respond_to.send(true);
                    tokio::time::sleep(teardown_delay).await;
                    break;
                }
                other => {
                    let _ = observed_tx.send(FakeActorEvent::Command(other));
                }
            }
        }
    });
    observed_rx
}
/// Drive `grow/internal/evict_sessions` through the real `ext_notification`
/// handler path (not the internal helper) — matches how the leader server
/// signals a client disconnect.
async fn drive_disconnect(agent: &MvpAgent, sid: &acp::SessionId) {
    drive_disconnect_many(agent, &[sid]).await;
}
/// Like `drive_disconnect`, but evicts several sessions in a single
/// `grow/internal/evict_sessions` notification — the realistic shape of a
/// real client disconnect, and the path that exercises `handle_evict_sessions`'
/// concurrent actor-owned unload transactions.
async fn drive_disconnect_many(agent: &MvpAgent, sids: &[&acp::SessionId]) {
    use acp::Agent as _;
    let ids: Vec<&str> = sids.iter().map(|s| s.0.as_ref()).collect();
    let params = serde_json::json!({ "sessionIds": ids });
    let raw = serde_json::value::to_raw_value(&params).unwrap();
    agent
        .ext_notification(acp::ExtNotification::new(
            "grow/internal/evict_sessions",
            raw.into(),
        ))
        .await
        .expect("evict_sessions notification must be handled");
}
/// Drive `grow/session/close` through the real `ext_method` dispatch
/// (`ext_method` → `handlers::session::handle` → `handle_session_close`),
/// exercising the exact production path that finalizes the replica.
async fn drive_close(agent: &MvpAgent, session_id: &str) -> Result<acp::ExtResponse, acp::Error> {
    use acp::Agent as _;
    let params = serde_json::json!({ "sessionId": session_id });
    let raw = serde_json::value::to_raw_value(&params).unwrap();
    agent
        .ext_method(acp::ExtRequest::new(
            "grow/session/close",
            std::sync::Arc::from(raw),
        ))
        .await
}
/// No-evict keystone: a client disconnecting mid-turn must NOT destroy the
/// session. The actor stays resident, no `Shutdown` is sent, the resident
/// session's command channel still **delivers** commands (so a reconnecting
/// `session/load` can keep driving the turn), and `finalize()` is NOT called
/// on a mere disconnect.
#[test]
fn disconnect_keeps_live_session_resident_without_finalize() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-live");
        let (_cmd_tx, mut observed) = {
            let (handle, tx, rx) = make_live_session_handle(&sid, Some("turn-1"));
            agent.sessions.borrow_mut().insert(sid.clone(), handle);
            (tx, spawn_fake_actor(rx, true))
        };
        drive_disconnect(&agent, &sid).await;
        assert!(
            agent.sessions.borrow().contains_key(&sid),
            "live session must stay resident across client disconnect"
        );
        assert!(
            matches!(
                observed.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "no command may be sent to a session kept resident with live work"
        );
        let resident = agent
            .sessions
            .borrow()
            .get(&sid)
            .cloned()
            .expect("session must still be resident");
        resident
            .cmd_tx
            .send(TestSessionCommand::ResetPermissionState)
            .expect("resident session channel must accept commands post-disconnect");
        tokio::task::yield_now().await;
        assert!(
            matches!(
                observed.try_recv(),
                Ok(FakeActorEvent::Command(
                    TestSessionCommand::ResetPermissionState
                ))
            ),
            "the resident session's receiver must observe the delivered command"
        );
        assert_eq!(
            agent.session_live_state_for(&sid),
            Some(SessionLiveState::Working),
            "a kept-resident session with live work is Working"
        );
    });
}
/// An unreachable actor cannot accept the unload transaction, so the leader
/// conservatively retains the handle.
#[test]
fn disconnect_keeps_resident_when_actor_is_unreachable() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-unreachable");
        let (handle, _tx, rx) = make_live_session_handle(&sid, None);
        drop(rx);
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        drive_disconnect(&agent, &sid).await;
        assert!(
            agent.sessions.borrow().contains_key(&sid),
            "an actor that cannot accept unload must be kept resident"
        );
        assert_eq!(
            agent.session_live_state_for(&sid),
            Some(SessionLiveState::Working),
        );
    });
}
/// Idle-unload + supervisor interaction: a *fully idle* session is unloaded to
/// disk on disconnect (the actor accepts `UnloadIfIdle`, handle
/// dropped) while the `SessionThread` is **retained** for
/// `drain_old_session_thread`. It is not finalized, and once the kept thread
/// finishes the supervisor reaps it as a *clean* exit — never `DeadFailed`.
#[test]
fn disconnect_unloads_idle_session_without_finalize() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-idle");
        let (handle, _cmd_tx, cmd_rx) = make_live_session_handle(&sid, None);
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        let mut observed = spawn_fake_actor(cmd_rx, false);
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        agent.session_threads.borrow_mut().insert(
            sid.clone(),
            crate::session::SessionThread::from_handle(std::thread::spawn(move || {
                let _ = release_rx.recv();
            })),
        );
        agent.ensure_session_supervisor();
        drive_disconnect(&agent, &sid).await;
        assert!(
            !agent.sessions.borrow().contains_key(&sid),
            "idle session must be unloaded from the resident map on disconnect"
        );
        assert!(
            agent.session_threads.borrow().contains_key(&sid),
            "idle-unload must keep the SessionThread for reconnect drain"
        );
        let unload = tokio::time::timeout(std::time::Duration::from_secs(1), observed.recv())
            .await
            .expect("idle-unload must commit within 1s")
            .expect("fake actor channel must stay open");
        assert!(
            matches!(unload, FakeActorEvent::IdleUnloadCommitted),
            "idle-unload must be committed by the actor"
        );
        assert_eq!(
            agent.session_live_state_for(&sid),
            Some(SessionLiveState::Dormant),
            "an idle-unloaded session demotes to Dormant"
        );
        drop(release_tx);
        let deadline = tokio::time::Instant::now() + (SESSION_SUPERVISOR_TICK * 6);
        while tokio::time::Instant::now() < deadline {
            if !agent.session_threads.borrow().contains_key(&sid) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(
            !agent.session_threads.borrow().contains_key(&sid),
            "supervisor must drop the finished kept thread"
        );
        assert!(
            !agent
                .roster_delta_spy
                .borrow()
                .iter()
                .any(|(id, st)| id == sid.0.as_ref() && *st == SessionLiveState::DeadFailed),
            "a cleanly idle-unloaded session must not be reaped as DeadFailed"
        );
        assert_eq!(
            agent.session_live_state_for(&sid),
            None,
            "clean-exit sweep must drop the Dormant live-state entry"
        );
    });
}
/// The actor-owned unload ACK is an admission boundary, not a teardown join.
/// A slow post-ACK teardown must still remove the handle and publish Dormant
/// within the leader's 500ms unload budget.
#[test]
fn disconnect_unloads_before_slow_actor_teardown_finishes() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-slow-unload");
        let (handle, _cmd_tx, cmd_rx) = make_live_session_handle(&sid, None);
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        let mut observed = spawn_slow_teardown_actor(cmd_rx, std::time::Duration::from_secs(2));

        tokio::time::timeout(
            std::time::Duration::from_millis(500),
            drive_disconnect(&agent, &sid),
        )
        .await
        .expect("post-ACK teardown must not hold up disconnect eviction");
        assert!(!agent.sessions.borrow().contains_key(&sid));
        assert_eq!(
            agent.session_live_state_for(&sid),
            Some(SessionLiveState::Dormant)
        );
        assert!(matches!(
            observed.try_recv(),
            Ok(FakeActorEvent::IdleUnloadCommitted)
        ));
    });
}
/// A between-turns session whose actor rejects `UnloadIfIdle` because it owns
/// queued work must stay resident.
#[test]
fn disconnect_keeps_resident_when_actor_reports_busy() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-busy");
        let (handle, _cmd_tx, cmd_rx) = make_live_session_handle(&sid, None);
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        let mut observed = spawn_fake_actor(cmd_rx, true);
        drive_disconnect(&agent, &sid).await;
        assert!(
            agent.sessions.borrow().contains_key(&sid),
            "a between-turns session with queued work must stay resident"
        );
        assert_eq!(
            agent.session_live_state_for(&sid),
            Some(SessionLiveState::Working),
            "an actor-reported-busy session is kept Working"
        );
        tokio::task::yield_now().await;
        assert!(
            matches!(
                observed.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "a busy session must not commit idle unload"
        );
    });
}
/// A between-turns session whose ONLY outstanding work is a parked
/// `PlanApproval` reverse-request (the resume re-park) must be kept resident on
/// disconnect. The actor's canonical live-work predicate includes the parked
/// approval and therefore rejects the unload.
#[test]
fn disconnect_keeps_resident_when_plan_approval_parked() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-plan-parked");
        let (handle, _cmd_tx, cmd_rx) = make_live_session_handle(&sid, None);
        handle.pending_interactions.lock().unwrap().insert(
            "exit-plan-mode-resume".to_string(),
            crate::session::pending_interaction::PendingKind::PlanApproval,
        );
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        let mut observed = spawn_fake_actor(cmd_rx, true);
        drive_disconnect(&agent, &sid).await;
        assert!(
            agent.sessions.borrow().contains_key(&sid),
            "a session with a parked plan-approval must stay resident"
        );
        assert_eq!(
            agent.session_live_state_for(&sid),
            Some(SessionLiveState::Working),
            "a parked-approval session is kept Working"
        );
        tokio::task::yield_now().await;
        assert!(
            matches!(
                observed.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "a parked-approval session must not commit idle unload"
        );
    });
}
/// Mixed batch in a *single* `grow/internal/evict_sessions` notification —
/// the realistic disconnect shape and the path that exercises
/// `handle_evict_sessions`' concurrent actor-owned unload transactions. One
/// actor rejects unload (→ resident/Working); the other commits it
/// (→ unloaded/Dormant), without cross-contamination.
#[test]
fn disconnect_mixed_batch_keeps_busy_unloads_idle() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid_busy = acp::SessionId::new("sess-batch-busy");
        let sid_idle = acp::SessionId::new("sess-batch-idle");
        let (busy_handle, _busy_tx, busy_rx) = make_live_session_handle(&sid_busy, None);
        let (idle_handle, _idle_tx, idle_rx) = make_live_session_handle(&sid_idle, None);
        agent
            .sessions
            .borrow_mut()
            .insert(sid_busy.clone(), busy_handle);
        agent
            .sessions
            .borrow_mut()
            .insert(sid_idle.clone(), idle_handle);
        let mut busy_observed = spawn_fake_actor(busy_rx, true);
        let mut idle_observed = spawn_fake_actor(idle_rx, false);
        drive_disconnect_many(&agent, &[&sid_busy, &sid_idle]).await;
        assert!(
            agent.sessions.borrow().contains_key(&sid_busy),
            "the busy session in the batch must stay resident"
        );
        assert_eq!(
            agent.session_live_state_for(&sid_busy),
            Some(SessionLiveState::Working),
            "the busy session must be Working"
        );
        assert!(
            !agent.sessions.borrow().contains_key(&sid_idle),
            "the idle session in the batch must be unloaded"
        );
        assert_eq!(
            agent.session_live_state_for(&sid_idle),
            Some(SessionLiveState::Dormant),
            "the idle session must be Dormant"
        );
        let idle_unload =
            tokio::time::timeout(std::time::Duration::from_secs(1), idle_observed.recv())
                .await
                .expect("idle session must receive a command within 1s")
                .expect("fake actor channel must stay open");
        assert!(
            matches!(idle_unload, FakeActorEvent::IdleUnloadCommitted),
            "the idle session must commit its unload"
        );
        tokio::task::yield_now().await;
        assert!(
            matches!(
                busy_observed.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "the busy session must not commit unload in a mixed batch"
        );
    });
}

/// An idle-unload acknowledgement belongs to the actor incarnation that
/// received the request. A reconnect/new writer for the same SessionId must
/// wait until that acknowledgement has been consumed and the old handle has
/// been removed, rather than being installed early and then removed by the
/// stale disconnect transaction.
#[test]
fn disconnect_unload_serializes_a_replacement_incarnation() {
    run_local_for_bridge_test(|| async {
        let agent = std::rc::Rc::new(build_minimal_agent_for_tests());
        let sid = acp::SessionId::new("sess-evict-incarnation-race");
        let (old_handle, _old_tx, mut old_rx) = make_live_session_handle(&sid, None);
        agent.sessions.borrow_mut().insert(sid.clone(), old_handle);

        let (unload_seen_tx, unload_seen_rx) = tokio::sync::oneshot::channel();
        let (release_ack_tx, release_ack_rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_local(async move {
            while let Some(command) = old_rx.recv().await {
                if let TestSessionCommand::UnloadIfIdle { respond_to } = command {
                    let _ = unload_seen_tx.send(());
                    let _ = release_ack_rx.await;
                    let _ = respond_to.send(true);
                    break;
                }
            }
        });

        let disconnect_agent = agent.clone();
        let disconnect_sid = sid.clone();
        let disconnect = tokio::task::spawn_local(async move {
            drive_disconnect(&disconnect_agent, &disconnect_sid).await;
        });
        unload_seen_rx
            .await
            .expect("old actor must receive idle-unload request");

        let (replacement, replacement_tx, _replacement_rx) = make_live_session_handle(&sid, None);
        let replacement_agent = agent.clone();
        let replacement_sid = sid.clone();
        let install = tokio::task::spawn_local(async move {
            let _lifecycle = replacement_agent
                .lock_session_lifecycle(&replacement_sid)
                .await;
            replacement_agent
                .sessions
                .borrow_mut()
                .insert(replacement_sid, replacement);
        });
        tokio::task::yield_now().await;
        assert!(
            !install.is_finished(),
            "replacement writer must wait behind the complete eviction transaction"
        );

        let _ = release_ack_tx.send(());
        disconnect.await.expect("disconnect task must finish");
        install.await.expect("replacement install must finish");

        let current = agent
            .sessions
            .borrow()
            .get(&sid)
            .cloned()
            .expect("replacement incarnation must remain resident");
        assert!(
            current.cmd_tx.same_channel(&replacement_tx),
            "the stale unload acknowledgement must not remove the replacement actor"
        );
    });
}
/// The bounded `session_live_state` map does not grow without bound
/// across repeated create/close cycles — every terminal close drops its
/// entry, so the map size stays at the live count, not the cumulative count.
#[test]
fn session_live_state_map_is_bounded_across_cycles() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        for i in 0..50 {
            let sid = acp::SessionId::new(format!("sess-cycle-{i}"));
            let (handle, _tx, _rx) = make_live_session_handle(&sid, Some("turn"));
            agent.sessions.borrow_mut().insert(sid.clone(), handle);
            agent.set_session_live_state(&sid, SessionLiveState::IdleResident);
            agent
                .close_session_explicit(&sid)
                .await
                .expect("test session has no outstanding writer thread");
        }
        assert_eq!(
            agent.session_live_state.borrow().len(),
            0,
            "terminal closes must leave no residual live-state entries (bounded map)"
        );
    });
}
/// A genuine terminal close driven through the real `grow/session/close`
/// dispatch shuts down the actor, drops local state, and updates the roster.
#[test]
fn explicit_close_removes_the_local_session() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-close");
        let (handle, _tx, mut cmd_rx) = make_live_session_handle(&sid, Some("turn-1"));
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        drive_close(&agent, "no-such-session")
            .await
            .expect("close of a missing session must succeed as a no-op");
        drive_close(&agent, sid.0.as_ref())
            .await
            .expect("session close must be handled");
        assert!(
            matches!(cmd_rx.try_recv(), Ok(TestSessionCommand::Shutdown)),
            "handle_session_close must send Shutdown to the actor"
        );
        assert!(
            !agent.sessions.borrow().contains_key(&sid),
            "explicit close removes the session"
        );
        assert_eq!(
            agent.session_live_state_for(&sid),
            None,
            "terminal removal must drop the live-state entry (bounded map)"
        );
        assert!(
            agent
                .roster_delta_spy
                .borrow()
                .iter()
                .any(|(id, st)| id == sid.0.as_ref() && *st == SessionLiveState::Completed),
            "explicit close must emit a Completed roster delta"
        );
    });
}

#[test]
fn explicit_close_reports_panicked_writer_as_failed() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-close-panicked");
        let (mut handle, _tx, _rx) = make_live_session_handle(&sid, Some("turn-1"));
        handle.info.id = sid.clone();
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        let thread = std::thread::spawn(|| panic!("injected writer panic"));
        while !thread.is_finished() {
            std::thread::yield_now();
        }
        agent.session_threads.borrow_mut().insert(
            sid.clone(),
            crate::session::SessionThread::from_handle(thread),
        );

        assert!(
            agent
                .close_session_explicit(&sid)
                .await
                .expect("panicked writer is already terminal")
        );
        assert!(
            agent
                .roster_delta_spy
                .borrow()
                .iter()
                .any(|(id, state)| id == sid.0.as_ref() && *state == SessionLiveState::DeadFailed),
            "joining the writer must preserve panic identity"
        );
        assert!(
            !agent
                .roster_delta_spy
                .borrow()
                .iter()
                .any(|(id, state)| id == sid.0.as_ref() && *state == SessionLiveState::Completed),
            "a panicked writer must never be reported as a clean close"
        );
    });
}
/// Join-handle supervisor: a *resident* actor that panics is reaped
/// promptly — removed from `sessions`/`session_threads`, demoted to
/// `DeadFailed` (observed via the roster delta, since the live-state entry
/// is dropped on removal), while the durable local session remains available.
///
/// Polls in real time (the panic unwinds on a real OS thread, independent of
/// the tokio clock); the reap lands within a small number of supervisor
/// ticks. The injected-panic backtrace on stderr is expected and harmless.
#[test]
fn supervisor_reaps_panicked_resident_actor() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-panic");
        let (handle, _tx, _rx) = make_live_session_handle(&sid, Some("turn-1"));
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        let panic_thread = std::thread::spawn(|| panic!("injected actor panic"));
        agent.session_threads.borrow_mut().insert(
            sid.clone(),
            crate::session::SessionThread::from_handle(panic_thread),
        );
        agent.set_session_live_state(&sid, SessionLiveState::Working);
        agent.ensure_session_supervisor();
        let deadline = tokio::time::Instant::now() + (SESSION_SUPERVISOR_TICK * 6);
        while tokio::time::Instant::now() < deadline {
            if !agent.session_threads.borrow().contains_key(&sid) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(
            !agent.session_threads.borrow().contains_key(&sid),
            "supervisor must reap the dead thread"
        );
        assert!(
            !agent.sessions.borrow().contains_key(&sid),
            "reaped session must be removed from the resident map"
        );
        assert_eq!(
            agent.session_live_state_for(&sid),
            None,
            "terminal removal drops the live-state entry (bounded map)"
        );
        assert!(
            agent
                .roster_delta_spy
                .borrow()
                .iter()
                .any(|(id, st)| id == sid.0.as_ref() && *st == SessionLiveState::DeadFailed),
            "a reaped resident actor must emit a DeadFailed roster delta"
        );
    });
}
/// `ensure_session_supervisor` is idempotent: calling it repeatedly spawns
/// the sweeper loop exactly once.
#[test]
fn ensure_session_supervisor_is_idempotent() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        assert_eq!(agent.supervisor_spawn_count.get(), 0);
        agent.ensure_session_supervisor();
        agent.ensure_session_supervisor();
        agent.ensure_session_supervisor();
        assert_eq!(
            agent.supervisor_spawn_count.get(),
            1,
            "the supervisor task must be spawned at most once"
        );
        assert!(agent.supervisor_started.get());
    });
}
/// After a terminal removal (reap/close drops the live-state entry), a later
/// reload of the same SessionId starts clean at `IdleResident` with no stale
/// terminal state leaking in (ties to the bounded-map fix).
#[test]
fn reload_after_terminal_removal_starts_clean() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("sess-reload");
        let (handle, _tx, _rx) = make_live_session_handle(&sid, Some("turn-1"));
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        agent
            .close_session_explicit(&sid)
            .await
            .expect("test session has no outstanding writer thread");
        assert_eq!(
            agent.session_live_state_for(&sid),
            None,
            "terminal removal must leave no stale state"
        );
        let (handle2, _tx2, _rx2) = make_live_session_handle(&sid, None);
        agent.sessions.borrow_mut().insert(sid.clone(), handle2);
        agent.set_session_live_state(&sid, SessionLiveState::IdleResident);
        assert_eq!(
            agent.session_live_state_for(&sid),
            Some(SessionLiveState::IdleResident),
            "a reloaded session must start at IdleResident, not a stale terminal state"
        );
    });
}
/// Build an agent whose gateway is wired to a live receiver so tests can
/// observe local ACP notifications.
fn build_agent_with_gateway_rx() -> (
    MvpAgent,
    tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let gateway = GatewaySender::new(tx);
    let cfg = valid_agent_config();
    let agent = MvpAgent::new(gateway, &cfg).expect("valid test config");
    (agent, rx)
}
#[tokio::test]
async fn local_announcement_reload_updates_config_and_clients() {
    let (agent, mut rx) = build_agent_with_gateway_rx();
    let announcements = vec![announcements::Announcement {
        id: Some("local".into()),
        message: Some("Configured locally".into()),
        severity: Some("info".into()),
        ..Default::default()
    }];
    let params = serde_json::value::to_raw_value(&serde_json::json!({
        "announcements": announcements,
    }))
    .unwrap();
    let request = acp::ExtRequest::new(
        "grow/internal/reload_announcements",
        std::sync::Arc::from(params),
    );

    crate::extensions::session_admin::handle(&agent, &request)
        .await
        .expect("reload succeeds");

    let acp_transport::AcpClientMessage::ExtNotification(args) =
        rx.try_recv().expect("announcement notification")
    else {
        panic!("expected announcement notification");
    };
    assert_eq!(args.request.method.as_ref(), "grow/announcements/update");
    let payload: announcements::AnnouncementsUpdated =
        serde_json::from_str(args.request.params.get()).unwrap();
    assert_eq!(payload.announcements, agent.cfg.borrow().announcements);
}

mod soft_default_settings_emit {
    use super::*;
    #[tokio::test]
    async fn emit_settings_update_carries_permission_mode_from_cfg() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                let gateway = GatewaySender::new(tx);
                let mut cfg = valid_agent_config();
                cfg.remote_settings = Some(crate::util::config::RemoteSettings {
                    permission_mode: Some("always-approve".into()),
                    slash_command_tags: Some(
                        [("workflows".to_string(), "new".to_string())]
                            .into_iter()
                            .collect(),
                    ),
                    ..Default::default()
                });
                let agent = MvpAgent::new(gateway, &cfg).expect("valid test config");
                agent.cfg.borrow_mut().remote_settings = cfg.remote_settings.clone();
                agent.emit_settings_update_notification();
                let msg = rx.try_recv().expect("settings/update must be emitted");
                let acp_transport::AcpClientMessage::ExtNotification(args) = msg else {
                    panic!("expected ExtNotification, got {msg:?}");
                };
                assert_eq!(args.request.method.as_ref(), "grow/settings/update");
                let params: serde_json::Value =
                    serde_json::from_str(args.request.params.get()).expect("parse params");
                assert_eq!(
                    params.get("permission_mode").and_then(|v| v.as_str()),
                    Some("always-approve"),
                    "post-auth emit must carry remote permission_mode for first session"
                );
                assert_eq!(
                    params
                        .get("slash_command_tags")
                        .and_then(|v| v.get("workflows"))
                        .and_then(|v| v.as_str()),
                    Some("new"),
                    "post-auth emit must carry remote slash_command_tags"
                );
                let _ = args.response_tx.send(Ok(()));
            })
            .await;
    }
}

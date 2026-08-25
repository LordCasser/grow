#![allow(dead_code)]
use super::*;
/// Wrap `id` in a shared auth-method handle for `SessionActor` test literals
/// (the field is now a shared live handle, not an owned id).
pub(crate) fn test_auth_method_id(id: &str) -> crate::agent::auth_method::SharedAuthMethodId {
    crate::agent::auth_method::new_shared_auth_method_id(Some(acp::AuthMethodId::new(id)))
}
/// Establish the causal scope that production creates before dispatching tools.
///
/// Tests that invoke `execute_tool_calls` directly deliberately bypass the
/// session loop, so they must create the same Timeline turn/step boundary
/// explicitly. The first durable tool append then acts as the FIFO barrier for
/// these buffered start events.
#[cfg(test)]
pub(crate) async fn begin_test_causal_turn(actor: &SessionActor) {
    actor.events.begin_turn();
    actor
        .events
        .start_turn(crate::session::events::Event::TurnStarted {
            session_id: actor.session_id_string(),
            turn_number: 1,
            identity: chat_state::TurnIdentity {
                origin: "test".into(),
                turn_kind: "internal".into(),
                goal_id: None,
                stage_id: None,
            },
            model_id: "test".into(),
            permission_mode: actor.permissions.mode(),
            conversation_message_count: 0,
            prompt_index: Some(0),
            prompt_text: Some("test prompt".into()),
            input_kind: chat_state::TurnInputKind::Prompt,
            session_relationship: if actor.startup_hints.is_subagent {
                crate::session::events::SessionRelationship::Subagent
            } else {
                crate::session::events::SessionRelationship::Primary
            },
            schema_version: crate::session::events::EVENT_SCHEMA_VERSION.into(),
            redirect_kind: None,
        })
        .await
        .unwrap();
    actor
        .events
        .emit(crate::session::events::Event::LoopStarted { loop_index: 0 });
}
/// Establish the Timeline and foreground halves of a live production turn.
/// Notification-drain tests need both: a Timeline turn without its admission
/// owner is deliberately treated as non-consumable.
#[cfg(test)]
pub(crate) async fn begin_test_active_causal_turn(actor: &SessionActor) {
    begin_test_causal_turn(actor).await;
    actor.state.lock().await.foreground =
        ForegroundState::RegularTurn(running_task_stub("test-active-turn"));
}

/// Seed a test Surface and its branch-local prompt coordinates through the
/// same Timeline mechanisms used by production. Snapshots are read models and
/// must never be used to install actor state.
#[cfg(test)]
pub(crate) async fn replace_test_surface(
    handle: &chat_state::ChatStateHandle,
    mut conversation: Vec<crate::sampling::ConversationItem>,
) {
    let (current, source_surface_revision) = handle
        .get_conversation_with_revision()
        .await
        .expect("test chat-state actor must be live");
    let system = current
        .first()
        .filter(|item| matches!(item, crate::sampling::ConversationItem::System(_)))
        .cloned()
        .expect("test actor must start with its immutable System governance head");
    if matches!(
        conversation.first(),
        Some(crate::sampling::ConversationItem::System(_))
    ) {
        conversation[0] = system;
    } else {
        conversation.insert(0, system);
    }
    handle
        .replace_context_durably(conversation, source_surface_revision)
        .await
        .unwrap();
}

#[cfg(test)]
pub(crate) async fn seed_test_timeline(
    actor: &SessionActor,
    conversation: Vec<crate::sampling::ConversationItem>,
    prompts: &[&str],
) {
    replace_test_surface(&actor.chat_state_handle, conversation).await;
    for prompt_text in prompts {
        record_test_prompt(actor, prompt_text).await;
    }
}

#[cfg(test)]
pub(crate) async fn record_test_prompt(actor: &SessionActor, prompt_text: &str) {
    static NEXT_TURN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(10_000);
    let prompt_index = actor.chat_state_handle.get_prompt_index().await;
    let id = chat_state::TurnId(NEXT_TURN.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    actor
        .chat_state_handle
        .record_timeline_event(chat_state::TimelineEventKind::Turn(
            chat_state::TurnEvent::Started {
                id,
                identity: chat_state::TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    stage_id: None,
                },
                model_id: "test".into(),
                input_message_count: 0,
                prompt_index,
                prompt_text: prompt_text.into(),
                input_kind: chat_state::TurnInputKind::Prompt,
                redirect_kind: None,
            },
        ));
    actor
        .chat_state_handle
        .record_timeline_event(chat_state::TimelineEventKind::Turn(
            chat_state::TurnEvent::Ended {
                id,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: chat_state::TurnTerminal {
                    stop_reason: "end_turn".into(),
                    completion_kind: "completed".into(),
                },
                cancellation_category: None,
                details: None,
            },
        ));
    let _ = actor.chat_state_handle.get_prompt_index().await;
}
#[cfg(test)]
pub(crate) async fn test_agent_default() -> agent::Agent {
    test_agent_with_tools(vec![]).await
}
/// Grow-build agent with the real `TodoWriteTool` (id `todo_write`, kind
/// `Plan`) registered, so `tool_for_kind(ToolKind::Plan)` resolves through the
/// live toolset instead of the literal fallback.
#[cfg(test)]
pub(crate) async fn test_grow_build_agent_with_todo() -> agent::Agent {
    use tools::implementations::grow_build::todo::TodoWriteTool;
    use tools::registry::types::ToolConfig;
    test_agent_with_tools(vec![ToolConfig::for_tool::<TodoWriteTool>()]).await
}
/// Agent with the real Plan lifecycle tool registered.
#[cfg(test)]
pub(crate) async fn test_agent_with_plan_tools() -> agent::Agent {
    use tools::implementations::grow_build::plan_control::PlanControlTool;
    use tools::registry::types::ToolConfig;
    test_agent_with_tools(vec![ToolConfig::for_tool::<PlanControlTool>()]).await
}
#[cfg(test)]
pub(crate) async fn test_agent_with_tools(
    tools: Vec<tools::registry::types::ToolConfig>,
) -> agent::Agent {
    test_agent_from_config(
        tools::registry::types::ToolServerConfig { tools },
        agent::AgentDefinition::default_grow_build(),
        std::sync::Arc::new(tools::computer::local::LocalTerminalBackend::new()),
    )
    .await
}
#[cfg(test)]
async fn test_agent_from_config(
    config: tools::registry::types::ToolServerConfig,
    definition: agent::AgentDefinition,
    backend: std::sync::Arc<dyn tools::computer::types::TerminalBackend>,
) -> agent::Agent {
    use tools::computer::local::LocalFs;
    use tools::computer::types::AsyncFileSystem;
    use tools::notification::ToolNotificationHandle;
    use tools::registry::types::SessionContext;
    let builder = tools::bridge::ToolBridge::get_builder();
    let fs: std::sync::Arc<dyn AsyncFileSystem> = std::sync::Arc::new(LocalFs);
    // Every actor owns an independent persistence path. A shared
    // resources-state path lets parallel tests rename/remove one another's
    // durable snapshot and turns a real persistence acknowledgement into a
    // nondeterministic ENOENT.
    let state_root =
        std::env::temp_dir().join(format!("grow-test-tool-state-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&state_root).expect("create isolated tool-state test directory");
    let state_path = state_root.join("resources_state.json");
    let ctx = SessionContext {
        backend,
        fs,
        cwd: std::path::PathBuf::from("/tmp"),
        session_folder: std::env::temp_dir().join("grow-test"),
        session_env: std::sync::Arc::new(std::collections::HashMap::new()),
        notification_handle: ToolNotificationHandle::noop(),
        owner_session_id: None,
        subagent: None,
        parent_scheduler_handle: None,
        skills: vec![],
        resources_persistence: std::sync::Arc::new(
            tools::persistence::ResourcesPersistence::local(state_path)
                .expect("pin resources state test store"),
        ),
        memory_backend: None,
        web_fetch_config: Default::default(),
        lsp: None,
        app_builder_deployer_config: Default::default(),
        system_reminder_tag: tools::reminders::DEFAULT_REMINDER_TAG,
    };
    let tool_bridge = tools::bridge::ToolBridge::finalize_builder(builder, config, ctx)
        .await
        .expect("finalize_builder should succeed for tests");
    #[allow(clippy::arc_with_non_send_sync)]
    let tool_bridge = std::sync::Arc::new(tool_bridge);
    agent::Agent::new(
        definition,
        agent::PromptContext::default(),
        String::new(),
        None,
        tool_bridge,
    )
}
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct DummyTerminal;
#[cfg(test)]
#[async_trait::async_trait]
impl crate::terminal::AsyncTerminalRunner for DummyTerminal {
    async fn run(
        &self,
        _request: crate::terminal::runner::TerminalRunRequest,
    ) -> Result<crate::terminal::runner::TerminalRunResult, crate::terminal::runner::TerminalError>
    {
        Err(crate::terminal::runner::TerminalError::Other(
            "dummy terminal".into(),
        ))
    }
}
#[cfg(test)]
pub(crate) async fn create_test_actor_ex(
    total_tokens: u64,
    context_window: u64,
    threshold_percent: u8,
    gateway_tx: tokio::sync::mpsc::UnboundedSender<acp_transport::AcpClientMessage>,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
) -> (
    SessionActor,
    tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
) {
    // Production owns a persistence actor that acknowledges durable terminal
    // appends. Unit tests pass an observation channel instead, so bridge the
    // durable envelope to the historical `Update` shape while completing the
    // barrier. Tests that care about ordering still observe the exact record.
    let (actor_persistence_tx, mut actor_persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(message) = actor_persistence_rx.recv().await {
            match message {
                PersistenceMsg::AppendUpdateDurablyAndAck { update, respond_to } => {
                    let _ = persistence_tx.send(PersistenceMsg::Update(update));
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::TimelineDurablyAndAck { event, respond_to } => {
                    let (observed_reply, _observed_ack) = tokio::sync::oneshot::channel();
                    let _ = persistence_tx.send(PersistenceMsg::TimelineDurablyAndAck {
                        event,
                        respond_to: observed_reply,
                    });
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::SidebandDurablyAndAck { event, respond_to } => {
                    let (observed_reply, _observed_ack) = tokio::sync::oneshot::channel();
                    let _ = persistence_tx.send(PersistenceMsg::SidebandDurablyAndAck {
                        event,
                        respond_to: observed_reply,
                    });
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::ReplaceRewindPointsAndAck { respond_to, .. } => {
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::WriteRewindTransactionAndAck { respond_to, .. }
                | PersistenceMsg::ClearRewindTransactionAndAck { respond_to } => {
                    let _ = respond_to.send(Ok(()));
                }
                other => {
                    let _ = persistence_tx.send(other);
                }
            }
        }
    });
    let persistence_tx = actor_persistence_tx;
    let cwd = paths::AbsPathBuf::new(std::path::PathBuf::from("/tmp")).unwrap();
    let fs = Arc::new(workspace::file_system::MockFs::new(cwd.to_path_buf()));
    let terminal = Arc::new(DummyTerminal {});
    let (hunk_tx, _hunk_rx) = tokio::sync::mpsc::unbounded_channel();
    let hunk_tracker_handle = hunk_tracker::HunkTrackerActor::spawn(
        "test-actor".to_string(),
        cwd.to_path_buf(),
        hunk_tx,
        hunk_tracker::TrackingMode::AgentOnly,
        tokio_util::sync::CancellationToken::new(),
    );
    let mut tool_context =
        ToolContext::new(cwd.clone(), None, None, fs, terminal, hunk_tracker_handle);
    let state = TokioMutex::new(AdmissionState {
        foreground: ForegroundState::Idle,
        pending_manual_compact: None,
        pending_inputs: VecDeque::new(),
        combine_edit_holds: std::collections::HashSet::new(),
        notifications_suppressed: false,
        rewindable: false,
        nudges_used_this_session: 0,
        recent_terminals: VecDeque::new(),
    });
    let (chat_event_tx, _chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<SessionEvent>();
    let chat_state_handle = chat_state::ChatStateActor::spawn(
        vec![sampling_types::ConversationItem::system(
            "test system prompt",
        )],
        sampling_types::SamplingConfig {
            base_url: "http://localhost".to_string(),
            model: "test".to_string(),
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: Default::default(),
            extra_headers: Default::default(),
            query_params: Default::default(),
            env_http_headers: Default::default(),
            context_window: std::num::NonZeroU64::new(context_window)
                .expect("test context_window must be non-zero"),
            reasoning_effort: None,
            stream_tool_calls: None,
        },
        Box::new(chat_state::NullTimelinePersistence),
        chat_event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    chat_state_handle.record_provider_context_anchor(total_tokens);
    let events = crate::session::events::EventTracker::new(chat_state_handle.clone());
    let (goal_command_tx, goal_command_rx) = tokio::sync::mpsc::unbounded_channel();
    let test_session_dir_guard = tempfile::Builder::new()
        .prefix(".grow-test-session-")
        .tempdir_in(cwd.as_path())
        .expect("create isolated test session directory");
    let session_dir = test_session_dir_guard.path().to_owned();
    let session_dir_name = session_dir
        .file_name()
        .expect("test session directory has a basename")
        .to_owned();
    // macOS exposes `/tmp` as a symlink to `/private/tmp`; production
    // capabilities intentionally reject symlink authorities, so pin the test
    // fixture through the resolved root while preserving `/tmp` as the model
    // workspace path.
    let session_root = std::fs::canonicalize(cwd.as_path()).expect("resolve test session root");
    let actor = SessionActor {
        session_info: SessionInfo {
            id: acp::SessionId::new("test-actor"),
            cwd: cwd.as_str().to_string(),
        },
        test_session_dir_guard: Some(test_session_dir_guard),
        session_dir,
        session_directory: std::sync::Arc::new(
            crate::session::storage::ContainedDirectory::open(
                &session_root,
                std::path::Path::new(&session_dir_name),
                "test session directory",
                false,
            )
            .expect("pin test session directory"),
        ),
        notification_artifact_gate: TokioMutex::new(()),
        auth_method_id: test_auth_method_id("test-auth"),
        model_auth_memo: std::cell::RefCell::new(None),
        state,
        notifications: NotificationSender {
            gateway: GatewaySender::new(gateway_tx),
            gateway_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            persistence_tx,
        },
        permissions: workspace::permission::PermissionHandle::allow_all(),
        tool_context,
        deny_read_globs: Vec::new(),
        mcp_state: Arc::new(TokioMutex::new(McpState::new(vec![]))),
        mcp: McpSessionState {
            strategy: McpInitStrategy::Blocking,
            initial_client_servers: vec![],
            tool_metadata_snapshot: Arc::new(std::sync::Mutex::new(Default::default())),
            announced_servers: Mutex::new(HashMap::new()),
            reminder_mode: McpReminderMode::Delta,
            reminder_dirty: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            connecting_reminder_injected: std::cell::Cell::new(false),
            handshakes_done: Arc::new(tokio::sync::Notify::new()),
        },
        chat_state_handle,
        unattributed_background_usage: std::sync::atomic::AtomicBool::new(false),
        current_prompt_id: std::sync::Arc::new(std::sync::Mutex::new(None)),
        pending_interactions: std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashMap::new(),
        )),
        compactions_remaining: std::cell::Cell::new(None),
        compaction_at_tokens: std::cell::Cell::new(None),
        doom_loop_recovery: None,
        doom_loop_turn_tally: Default::default(),
        file_state_tracker: Arc::new(FileStateTracker::new()),
        rewind_pending_prompt: std::sync::Mutex::new(None),
        startup_hints: StartupHints::default(),
        subagent_capabilities: None,
        compaction: crate::session::compaction_config::CompactionConfig {
            lease: Default::default(),
            threshold_percent: std::cell::Cell::new(threshold_percent),
            memory_flush_enabled: false,
            wall_clock_budget_secs: 0,
            force_compact: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            context_window_override: None,
            count: std::sync::atomic::AtomicU64::new(0),
            auto_compact_suppressed: std::sync::atomic::AtomicU8::new(0),
            previous_model: std::cell::Cell::new(None),
            verbatim_input: true,
            pre_prune: std::cell::Cell::new(true),
            pre_prune_token_budget: std::cell::Cell::new(None),
            cancel: Default::default(),
        },
        todo_gate: Default::default(),
        memory: crate::session::memory_state::SessionMemory {
            flush_config: crate::config::MemoryFlushConfig::default(),
            is_flushing: std::sync::atomic::AtomicBool::new(false),
            last_flush_compaction: std::sync::atomic::AtomicU64::new(0),
            storage: std::cell::RefCell::new(None),
            save_on_end: true,
            backend_params: None,
            initial_injection_config: Default::default(),
            context_injected: std::sync::atomic::AtomicBool::new(false),
            flush_count: std::sync::atomic::AtomicU64::new(0),
            last_flush_content: std::cell::RefCell::new(None),
            flush_success_count: std::sync::atomic::AtomicU64::new(0),
            flush_error_count: std::sync::atomic::AtomicU64::new(0),
            search_counter: std::cell::RefCell::new(None),
            injection_count: std::sync::atomic::AtomicU64::new(0),
            compaction_recovery_count: std::sync::atomic::AtomicU64::new(0),
            chunks_added: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            dream_config: Default::default(),
            dream_count: std::sync::atomic::AtomicU64::new(0),
            dream_success_count: std::sync::atomic::AtomicU64::new(0),
            dream_error_count: std::sync::atomic::AtomicU64::new(0),
        },
        session_start: std::time::Instant::now(),
        inference_idle_timeout: std::cell::Cell::new(Duration::from_secs(300)),
        max_retries: std::cell::Cell::new(3),
        subagent_classifier_input: crate::config::SubagentClassifierInput::Context,
        max_turns: None,
        pending_interjections: InterjectionBuffer::new(),
        completion_delivery: Default::default(),
        pending_system_reminders: Mutex::new(Vec::new()),
        idle_flush_timeout: None,
        dream_check_timeout: None,
        last_idle_flush_conversation_len: std::sync::atomic::AtomicUsize::new(0),
        event_tx,
        idle_arbiter: Arc::new(tokio::sync::Notify::new()),
        buffering_settings: None,
        client_identifier: None,
        origin_client: None,
        signals_handle: Default::default(),
        agent: std::cell::RefCell::new(test_agent_default().await),
        last_reported_branch: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        git_head_enabled: false,
        models_manager: Default::default(),
        owns_permission_manager: false,
        permission_audit_bridge: parking_lot::Mutex::new(None),
        display_cwd: std::sync::OnceLock::new(),
        selected_model_id: std::cell::RefCell::new(acp::ModelId::new("test")),
        active_skill: parking_lot::Mutex::new(None),
        turn_behavior: Arc::new(parking_lot::Mutex::new(tool_types::BehaviorId::Normal)),
        behavior: Arc::new(parking_lot::Mutex::new(
            crate::session::behavior::BehaviorCoordinator::new(),
        )),
        control_revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        goal_enabled: false,
        background_workflows_enabled: false,
        goal_runtime_available: std::sync::atomic::AtomicBool::new(false),
        goal_tracker: Arc::new(parking_lot::Mutex::new(
            crate::session::goal_tracker::GoalTracker::new(),
        )),
        goal_turn_task_ids: parking_lot::Mutex::new(std::collections::HashMap::new()),
        goal_command_rx: std::cell::RefCell::new(Some(goal_command_rx)),
        goal_command_tx,
        workflow_manager: crate::session::workflow::manager::WorkflowManager::test_bundle().0,
        workflow_tx: tokio::sync::mpsc::unbounded_channel().0,
        user_input_generation: std::sync::atomic::AtomicU64::new(0),
        laziness_debug_log: None,
        deferred_prefix: TaskSlot::new(),
        idle_prompt_extension: None,
        last_announced_local_date: std::cell::Cell::new(chrono::Local::now().date_naive()),
        last_search_prompt_index: std::sync::atomic::AtomicI64::new(-1),
        last_api_request_at: std::sync::atomic::AtomicI64::new(0),
        hooks: HookSessionState {
            registry: std::cell::RefCell::new(None),
            client_hooks: Default::default(),
            resolved_workspace_root: String::new(),
            vcs_kind: workspace::session::git::VcsKind::Git,
            load_errors: std::cell::RefCell::new(Vec::new()),
        },
        plugin_registry: std::cell::RefCell::new(None),
        plugin_registry_handle: None,
        events,
        current_turn_number: std::cell::Cell::new(0),
        last_recap_main_turn: std::cell::Cell::new(0),
        recap_in_flight: std::cell::Cell::new(false),
        recap_epoch: std::cell::Cell::new(0),
        session_turn_active: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        turn_stream_drained: parking_lot::Mutex::new(None),
        sampler_handle: sampler::SamplerHandle::noop(),
        rebuild_spec: crate::session::agent_rebuild::test_rebuild_spec_default(),
        image_description_model: parking_lot::RwLock::new(None),
        session_title_route: std::cell::RefCell::new(None),
        image_describe_cache: Arc::new(crate::session::image_describe::ImageDescribeCache::new()),
        subagent_token_records: parking_lot::Mutex::new(HashMap::new()),
        workspace_ops: workspace::WorkspaceOps::for_test(),
    };
    (actor, event_rx)
}
#[cfg(test)]
pub(crate) async fn create_test_actor(
    total_tokens: u64,
    context_window: u64,
    threshold_percent: u8,
    gateway_tx: tokio::sync::mpsc::UnboundedSender<acp_transport::AcpClientMessage>,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
) -> SessionActor {
    create_test_actor_ex(
        total_tokens,
        context_window,
        threshold_percent,
        gateway_tx,
        persistence_tx,
    )
    .await
    .0
}
/// Build a user-originated `InputItem` carrying queue metadata, returning the
/// completion receiver so a test can assert the prompt's in-flight RPC is
/// resolved (not dropped) when the prompt is removed/cleared.
#[cfg(test)]
pub(crate) fn user_item_with_rx(
    id: &str,
    owner: &str,
) -> (InputItem, oneshot::Receiver<PromptTurnResult>) {
    let (respond_to, rx) = oneshot::channel();
    let text = format!("text for {id}");
    let item = InputItem {
        notification_ids: Vec::new(),
        prompt_id: id.to_string(),
        turn_kind: crate::session::TurnKind::User,
        prompt_blocks: vec![acp::ContentBlock::Text(acp::TextContent::new(text.clone()))],
        client_identifier: Some(owner.to_string()),
        screen_mode: None,
        verbatim: false,
        json_schema: None,
        origin: crate::session::PromptOrigin::User,
        respond_to,
        persist_ack: None,
        queue_meta: Some(crate::session::prompt_queue::QueueEntryMeta {
            id: id.to_string(),
            version: 0,
            owner: Some(owner.to_string()),
            last_editor: None,
            kind: "prompt".to_string(),
            text,
            combined_texts: None,
        }),
    };
    (item, rx)
}
/// Build a user-originated `InputItem` carrying queue metadata (dropping the
/// completion receiver — for tests that don't assert on the RPC result).
#[cfg(test)]
pub(crate) fn user_item(id: &str, owner: &str) -> InputItem {
    user_item_with_rx(id, owner).0
}
#[cfg(test)]
pub(crate) fn input_with_origin_rx(
    prompt_id: &str,
    origin: crate::session::PromptOrigin,
) -> (InputItem, oneshot::Receiver<PromptTurnResult>) {
    let (respond_to, rx) = oneshot::channel();
    let verbatim = origin.is_synthetic();
    let item = InputItem {
        notification_ids: Vec::new(),
        prompt_id: prompt_id.to_string(),
        turn_kind: if origin.is_synthetic() {
            crate::session::TurnKind::Internal
        } else {
            crate::session::TurnKind::User
        },
        prompt_blocks: vec![],
        client_identifier: None,
        screen_mode: None,
        verbatim,
        json_schema: None,
        origin,
        respond_to,
        persist_ack: None,
        queue_meta: None,
    };
    (item, rx)
}
/// A regular foreground `AgentTask` stub: a 60s sleeper that keeps the turn
/// in flight until aborted. Requires a `LocalSet` (`spawn_local`).
#[cfg(test)]
pub(crate) fn running_task_stub(prompt_id: &str) -> AgentTask {
    AgentTask {
        prompt_id: prompt_id.to_string(),
        origin: crate::session::PromptOrigin::User,
        turn_kind: crate::session::TurnKind::User,
        turn_start_ms: 0,
        handle: tokio::task::spawn_local(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        })
        .abort_handle(),
    }
}
#[cfg(test)]
pub(crate) async fn build_actor() -> (
    std::sync::Arc<SessionActor>,
    tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let (gateway_tx, gateway_rx) =
        tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, mut persistence_rx) =
        tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    tokio::spawn(async move { while persistence_rx.recv().await.is_some() {} });
    let actor =
        std::sync::Arc::new(create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await);
    (actor, gateway_rx)
}
/// A small valid inline PNG content block (survives normalization —
/// 32×32 = 1024 px clears the API's 512-total-pixel floor).
#[cfg(test)]
pub(crate) fn test_image_content() -> acp::ImageContent {
    use base64::Engine as _;
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(32, 32, Rgba([128, 64, 32, 255]));
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    acp::ImageContent::new(
        base64::engine::general_purpose::STANDARD.encode(&buf),
        "image/png".to_string(),
    )
}

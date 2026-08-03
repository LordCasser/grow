use super::support::*;
use super::*;
use crate::terminal::AsyncTerminalRunner;
use crate::terminal::runner::{TerminalError, TerminalRunRequest, TerminalRunResult};
use paths::AbsPathBuf;
use tokio::sync::mpsc;
use workspace::file_system::MockFs;
use workspace::permission::PermissionHandle;
#[derive(Debug)]
struct DummyTerminal;
#[async_trait::async_trait]
impl AsyncTerminalRunner for DummyTerminal {
    async fn run(&self, _request: TerminalRunRequest) -> Result<TerminalRunResult, TerminalError> {
        Err(TerminalError::Other("dummy terminal".into()))
    }
}
fn agent_msg_update(text: &str) -> acp::SessionUpdate {
    acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
        acp::TextContent::new(text.to_string()),
    )))
}
fn extract_text(n: &acp::SessionNotification) -> Option<String> {
    match &n.update {
        acp::SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
            acp::ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        },
        _ => None,
    }
}
pub(super) struct ReplaySendUpdateFixture {
    pub(super) actor: SessionActor,
    pub(super) event_rx: mpsc::UnboundedReceiver<SessionEvent>,
    sent: Arc<tokio::sync::Mutex<Vec<acp::SessionNotification>>>,
    persistence_rx: mpsc::UnboundedReceiver<PersistenceMsg>,
}
pub(super) async fn make_replay_send_update_fixture() -> ReplaySendUpdateFixture {
    let (gateway_tx, mut gateway_rx) = mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let gateway = GatewaySender::new(gateway_tx);
    let sent = Arc::new(tokio::sync::Mutex::new(
        Vec::<acp::SessionNotification>::new(),
    ));
    let sent_for_task = sent.clone();
    tokio::task::spawn_local(async move {
        while let Some(msg) = gateway_rx.recv().await {
            if let acp_transport::AcpClientMessage::SessionNotification(args) = msg {
                sent_for_task.lock().await.push(args.request);
                let _ = args.response_tx.send(Ok(()));
            }
        }
    });
    let (persistence_tx, persistence_rx) = mpsc::unbounded_channel::<PersistenceMsg>();
    let cwd = AbsPathBuf::new(std::path::PathBuf::from("/tmp")).unwrap();
    let fs = Arc::new(MockFs::new(cwd.to_path_buf()));
    let terminal = Arc::new(DummyTerminal {});
    let (hunk_tx, _hunk_rx) = tokio::sync::mpsc::unbounded_channel();
    let hunk_tracker_handle = hunk_tracker::HunkTrackerActor::spawn(
        "test-session".to_string(),
        cwd.to_path_buf(),
        hunk_tx,
        hunk_tracker::TrackingMode::AgentOnly,
        tokio_util::sync::CancellationToken::new(),
    );
    let tool_context = ToolContext::new(cwd.clone(), None, None, fs, terminal, hunk_tracker_handle);
    let state = TokioMutex::new(State {
        running_task: None,
        pending_inputs: VecDeque::new(),
        combine_edit_holds: std::collections::HashSet::new(),
        pending_notifications: Vec::new(),
        notifications_suppressed: false,
        rewindable: false,
        nudges_used_this_session: 0,
    });
    let (event_tx, event_rx) = mpsc::unbounded_channel::<SessionEvent>();
    let actor = SessionActor {
        session_info: SessionInfo {
            id: acp::SessionId::new("test-session"),
            cwd: cwd.as_str().to_string(),
        },
        auth_method_id: test_auth_method_id("test-auth"),
        model_auth_memo: std::cell::RefCell::new(None),
        state,
        notifications: NotificationSender {
            gateway,
            gateway_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            persistence_tx,
        },
        permissions: PermissionHandle::allow_all(),
        tool_context,
        deny_read_globs: Vec::new(),
        mcp_state: Arc::new(TokioMutex::new(McpState::new(vec![]))),
        mcp_strategy: McpInitStrategy::Blocking,
        chat_state_handle: chat_state::ChatStateHandle::noop(),
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
        forked_tool_override: None,
        compaction: crate::session::compaction_config::CompactionConfig {
            threshold_percent: std::cell::Cell::new(85),
            force_compact: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            context_window_override: None,
            count: std::sync::atomic::AtomicU64::new(0),
            auto_compact_suppressed: std::sync::atomic::AtomicU8::new(0),
            previous_model: std::cell::Cell::new(None),
            compaction_mode: chat_state::CompactionMode::Transcript,
            verbatim_input: true,
            tool_choice: crate::util::config::CompactionToolChoice::Auto,
            prefire: crate::session::compaction_config::PrefireState::default(),
            prefix_released: std::sync::atomic::AtomicBool::new(false),
            cancel: Default::default(),
        },
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
        inference_idle_timeout: Duration::from_secs(300),
        max_retries: 3,
        max_turns: None,
        pending_interjections: InterjectionBuffer::new(),
        pending_system_reminders: Mutex::new(Vec::new()),
        idle_flush_timeout: None,
        dream_check_timeout: None,
        last_idle_flush_conversation_len: std::sync::atomic::AtomicUsize::new(0),
        event_tx,
        buffering_settings: Some(BufferingSettings {
            max_items: 100,
            max_bytes: 1_000_000,
            max_duration_ms: 50,
        }),
        client_identifier: None,
        origin_client: None,
        signals_handle: Default::default(),
        agent: std::cell::RefCell::new(test_agent_default().await),
        last_reported_branch: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        git_head_enabled: false,
        models_manager: Default::default(),
        display_cwd: std::sync::OnceLock::new(),
        active_agent_type: parking_lot::Mutex::new(None),
        active_skill: parking_lot::Mutex::new(None),
        current_prompt_mode: Arc::new(parking_lot::Mutex::new(PromptMode::Agent)),
        turn_start_prompt_mode: parking_lot::Mutex::new(PromptMode::Agent),
        turn_prompt_mode: Arc::new(parking_lot::Mutex::new(PromptMode::Agent)),
        behavior: Arc::new(parking_lot::Mutex::new(
            crate::session::behavior::BehaviorController::new(std::path::PathBuf::from(
                "/tmp/test-session",
            )),
        )),
        goal_enabled: false,
        background_workflows_enabled: false,
        goal_harness_enabled: std::sync::atomic::AtomicBool::new(false),
        goal_harness_availability_reconciled: std::sync::atomic::AtomicBool::new(false),
        goal_tracker: Arc::new(parking_lot::Mutex::new(
            crate::session::goal_tracker::GoalTracker::new(std::path::PathBuf::from(
                "/tmp/test-session",
            )),
        )),
        goal_turn_task_ids: parking_lot::Mutex::new(std::collections::HashSet::new()),
        goal_continuation_streak: std::sync::atomic::AtomicU32::new(0),
        goal_blocked_streak: std::sync::atomic::AtomicU32::new(0),
        goal_update_rx: std::cell::RefCell::new(None),
        goal_update_tx: tokio::sync::mpsc::unbounded_channel().0,
        workflow_manager: crate::session::workflow::manager::WorkflowManager::test_bundle().0,
        workflow_launch_tx: tokio::sync::mpsc::unbounded_channel().0,
        goal_classifier_enabled: false,
        goal_planner_enabled: false,
        goal_summary_enabled: false,
        goal_verifier_skeptic_count: 1,
        goal_role_models: Default::default(),
        goal_use_current_model_only: false,
        goal_classifier_max_runs: crate::session::goal_classifier::GOAL_CLASSIFIER_MAX_RUNS_DEFAULT,
        goal_strategist_every: 5,
        goal_reverify_after: crate::session::acp_session::GOAL_REVERIFY_AFTER_DEFAULT,
        goal_plan_reconciled: std::sync::atomic::AtomicBool::new(false),
        pending_classifier_completions: parking_lot::Mutex::new(std::collections::VecDeque::new()),
        goal_classifier_in_flight: std::sync::atomic::AtomicBool::new(false),
        managed_mcp_handle: Default::default(),
        initial_client_mcp_servers: vec![],
        tool_metadata_snapshot: Arc::new(std::sync::Mutex::new(Default::default())),
        mcp_announced_servers: Mutex::new(HashMap::new()),
        mcp_reminder_mode: McpReminderMode::Delta,
        mcp_reminder_dirty: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        mcp_connecting_reminder_injected: std::cell::Cell::new(false),
        mcp_handshakes_done: Arc::new(tokio::sync::Notify::new()),
        user_input_generation: std::sync::atomic::AtomicU64::new(0),
        laziness_debug_log: None,
        deferred_prefix: TaskSlot::new(),
        idle_prompt_extension: None,
        last_announced_local_date: std::cell::Cell::new(chrono::Local::now().date_naive()),
        prefix_carries_fallback_date: std::cell::Cell::new(false),
        last_search_prompt_index: std::sync::atomic::AtomicI64::new(-1),
        last_api_request_at: std::sync::atomic::AtomicI64::new(0),
        hook_registry: std::cell::RefCell::new(None),
        client_hooks: Default::default(),
        hook_resolved_workspace_root: String::new(),
        vcs_kind: workspace::session::git::VcsKind::Git,
        hook_load_errors: std::cell::RefCell::new(Vec::new()),
        plugin_registry: std::cell::RefCell::new(None),
        plugin_registry_handle: None,
        events: crate::session::events::EventTracker::new(std::path::Path::new("/tmp")),
        current_turn_number: std::cell::Cell::new(0),
        last_recap_main_turn: std::cell::Cell::new(0),
        recap_in_flight: std::cell::Cell::new(false),
        recap_epoch: std::cell::Cell::new(0),
        session_turn_active: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        turn_stream_drained: parking_lot::Mutex::new(None),
        sampler_handle: sampler::SamplerHandle::noop(),
        rebuild_spec: crate::session::agent_rebuild::test_rebuild_spec_default(),
        image_description_model: None,
        image_describe_cache: Arc::new(crate::session::image_describe::ImageDescribeCache::new()),
        subagent_token_records: parking_lot::Mutex::new(HashMap::new()),
        workspace_ops: workspace::WorkspaceOps::for_test(),
    };
    ReplaySendUpdateFixture {
        actor,
        event_rx,
        sent,
        persistence_rx,
    }
}
#[tokio::test(flavor = "current_thread")]
async fn send_update_buffers_streaming_chunks_and_flush_sends_merged_notification() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let ReplaySendUpdateFixture {
                actor,
                mut event_rx,
                sent,
                mut persistence_rx,
            } = make_replay_send_update_fixture().await;
            actor.send_update(agent_msg_update("he"), Some(1)).await;
            actor.send_update(agent_msg_update("llo"), Some(2)).await;
            assert!(
                sent.lock().await.is_empty(),
                "buffering enabled: no outbound notifications expected yet"
            );
            assert!(
                persistence_rx.try_recv().is_err(),
                "buffering enabled: nothing should be persisted until emitted"
            );
            let mut replay_buffer = ReplayBuffer::new(actor.buffering_settings.clone());
            while let Ok(event) = event_rx.try_recv() {
                let SessionEvent::Notification(notification) = event else {
                    unreachable!("send_update should only enqueue replay notifications")
                };
                let _ = replay_buffer.consume_chunk(notification);
            }
            let flushed = replay_buffer
                .flush()
                .expect("flush should emit pending chunk");
            actor.emit_buffered(flushed).await;
            tokio::task::yield_now().await;
            let sent_msgs = sent.lock().await.clone();
            assert_eq!(sent_msgs.len(), 1);
            assert_eq!(extract_text(&sent_msgs[0]).as_deref(), Some("hello"));
            let mut persisted = vec![];
            while let Ok(msg) = persistence_rx.try_recv() {
                persisted.push(msg);
            }
            let persisted_updates = persisted
                .into_iter()
                .filter(|m| matches!(m, PersistenceMsg::Update(_)))
                .count();
            assert_eq!(persisted_updates, 1);
        })
        .await;
}
/// A cancel must flush the actor-owned replay buffer so the tail of a streamed
/// reasoning response reaches local session persistence.
#[tokio::test(flavor = "current_thread")]
async fn cancel_flushes_buffered_chunks_to_persistence() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let ReplaySendUpdateFixture {
                actor,
                mut event_rx,
                sent: _sent,
                mut persistence_rx,
            } = make_replay_send_update_fixture().await;
            actor
                .send_update(agent_msg_update("partial reasoning tail"), Some(1))
                .await;
            assert!(
                persistence_rx.try_recv().is_err(),
                "no Update should land while the chunk is still in flight",
            );
            let mut replay_buffer = ReplayBuffer::new(actor.buffering_settings.clone());
            while let Ok(event) = event_rx.try_recv() {
                if let SessionEvent::Notification(notification) = event {
                    let _ = replay_buffer.consume_chunk(notification);
                }
            }
            assert!(
                persistence_rx.try_recv().is_err(),
                "chunk should still be buffered, not yet persisted",
            );
            if let Some(notification) = replay_buffer.flush() {
                actor.emit_buffered(notification).await;
            }
            tokio::task::yield_now().await;
            let mut got_chunk_text: Option<String> = None;
            while let Ok(msg) = persistence_rx.try_recv() {
                if let PersistenceMsg::Update(update) = msg
                    && let crate::session::storage::SessionUpdate::Acp(notif) = update
                    && let acp::SessionUpdate::AgentMessageChunk(chunk) = &notif.update
                    && let acp::ContentBlock::Text(t) = &chunk.content
                {
                    got_chunk_text = Some(t.text.clone());
                    break;
                }
            }
            assert_eq!(
                got_chunk_text.as_deref(),
                Some("partial reasoning tail"),
                "cancel flush must persist the buffered reasoning chunk"
            );
            drop(actor);
        })
        .await;
}
/// Negative control for `cancel_flushes_buffered_chunks_to_persistence`:
/// without the flush, a buffered chunk does NOT reach persistence on its
/// own — proving the flush call is load-bearing in the cancel path.
#[tokio::test(flavor = "current_thread")]
async fn buffered_chunk_does_not_reach_persistence_without_explicit_flush() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let ReplaySendUpdateFixture {
                actor,
                mut event_rx,
                sent: _sent,
                mut persistence_rx,
            } = make_replay_send_update_fixture().await;
            actor
                .send_update(agent_msg_update("would-be lost reasoning"), Some(1))
                .await;
            let mut replay_buffer = ReplayBuffer::new(actor.buffering_settings.clone());
            while let Ok(event) = event_rx.try_recv() {
                if let SessionEvent::Notification(notification) = event {
                    let _ = replay_buffer.consume_chunk(notification);
                }
            }
            tokio::task::yield_now().await;
            let mut saw_update = false;
            while let Ok(msg) = persistence_rx.try_recv() {
                if matches!(msg, PersistenceMsg::Update(_)) {
                    saw_update = true;
                    break;
                }
            }
            assert!(
                !saw_update,
                "without an explicit flush, the buffered chunk must \
                     remain stranded in `replay_buffer.pending` — this is \
                     the cancel path would otherwise lose it",
            );
            drop(actor);
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn available_commands_update_is_forwarded_but_not_persisted() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let ReplaySendUpdateFixture {
                actor,
                event_rx: _event_rx,
                sent,
                mut persistence_rx,
            } = make_replay_send_update_fixture().await;
            let session_id = acp::SessionId::new("test-session");
            actor
                .emit_notification_direct(
                    acp::SessionNotification::new(
                        session_id.clone(),
                        acp::SessionUpdate::AvailableCommandsUpdate(
                            acp::AvailableCommandsUpdate::new(vec![]),
                        ),
                    ),
                )
                .await;
            actor
                .emit_notification_direct(
                    acp::SessionNotification::new(session_id, agent_msg_update("hello")),
                )
                .await;
            for _ in 0..50 {
                if sent.lock().await.len() >= 2 {
                    break;
                }
                tokio::task::yield_now().await;
            }
            assert_eq!(
                sent.lock().await.len(),
                2,
                "both updates must be forwarded to the live client (command palette must stay current)",
            );
            let mut persisted = vec![];
            while let Ok(msg) = persistence_rx.try_recv() {
                if let PersistenceMsg::Update(
                    crate::session::storage::SessionUpdate::Acp(n),
                ) = msg {
                    persisted.push(n);
                }
            }
            assert_eq!(
                persisted.len(),
                1,
                "exactly one update must be persisted; available_commands_update must be skipped",
            );
            assert!(
                matches!(persisted[0].update, acp::SessionUpdate::AgentMessageChunk(_)),
                "the persisted update must be the agent message, not available_commands_update",
            );
            drop(actor);
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn completed_event_releases_stream_drain_barrier() {
    use sampler::{InferenceLatencyStats, RequestId, SamplingChannel, SamplingEvent};
    use sampling_types::{ConversationItem, ConversationResponse};
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let fixture = make_replay_send_update_fixture().await;
            let actor = Arc::new(fixture.actor);
            *actor
                .current_prompt_id
                .lock()
                .expect("current_prompt_id mutex poisoned") = Some("prompt-barrier".to_string());
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            *actor.turn_stream_drained.lock() = Some(tx);
            let req = RequestId::random();
            actor
                .handle_sampling_event(SamplingEvent::StreamStarted {
                    request_id: req.clone(),
                    timestamp_ms: 0,
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::ChannelToken {
                    request_id: req.clone(),
                    channel: SamplingChannel::Text,
                    text: "the scrollback blo".to_string(),
                    chunk_index: 0,
                })
                .await;
            assert!(
                actor.turn_stream_drained.lock().is_some(),
                "a mid-stream text chunk must NOT release the stream-drain barrier"
            );
            actor
                .handle_sampling_event(SamplingEvent::Completed {
                    request_id: req,
                    response: Box::new(ConversationResponse {
                        items: vec![ConversationItem::assistant("blocks".to_string())],
                        usage: None,
                        stop_reason: None,
                        cost_usd_ticks: None,
                        message_chunks_emitted: 1,
                        doom_loop_signals: Vec::new(),
                        stop_message: None,
                        message_id: None,
                        raw_stop_reason: None,
                        stop_sequence: None,
                    }),
                    metrics: InferenceLatencyStats::default(),
                })
                .await;
            assert!(
                actor.turn_stream_drained.lock().is_none(),
                "Completed must take the stream-drain barrier sender"
            );
            assert!(
                rx.await.is_ok(),
                "Completed must fire the stream-drain barrier so \
                 run_turn_via_sampler can proceed to emit tool calls in order"
            );
        })
        .await;
}
/// Observe-only (`max_retries = 0`): a first completion carrying confident
/// signals had NOTHING discarded, so it must not be classified as a
/// budget-spent accept — no tally or counters; the
/// signals stay warn-only on the accepted response.
#[tokio::test(flavor = "current_thread")]
async fn observe_only_confident_completion_stays_warn_only() {
    use sampler::{RequestId, SamplingChannel, SamplingEvent};
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let mut fixture = make_replay_send_update_fixture().await;
            fixture.actor.doom_loop_recovery = Some(sampling_types::DoomLoopRecoveryPolicy {
                max_threshold: 8,
                max_retries: 0,
            });
            let actor = Arc::new(fixture.actor);
            *actor
                .current_prompt_id
                .lock()
                .expect("current_prompt_id mutex poisoned") = Some("prompt-observe".to_string());
            let req = RequestId::random();
            actor
                .handle_sampling_event(SamplingEvent::StreamStarted {
                    request_id: req.clone(),
                    timestamp_ms: 0,
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::ChannelToken {
                    request_id: req.clone(),
                    channel: SamplingChannel::Reasoning,
                    text: "loop loop loop".to_string(),
                    chunk_index: 0,
                })
                .await;
            let response = sampling_types::ConversationResponse {
                items: vec![sampling_types::ConversationItem::assistant(
                    "answer kept as-is",
                )],
                stop_reason: None,
                usage: None,
                cost_usd_ticks: None,
                message_chunks_emitted: 1,
                doom_loop_signals: vec![sampling_types::doom_loop::DoomLoopSignal::parse(
                    "tail_repetition:8@thinking",
                )],
                stop_message: None,
                message_id: None,
                raw_stop_reason: None,
                stop_sequence: None,
            };
            actor
                .handle_sampling_event(SamplingEvent::Completed {
                    request_id: req,
                    response: Box::new(response),
                    metrics: Default::default(),
                })
                .await;
            let tally = actor.doom_loop_turn_tally.lock().clone();
            assert!(!tally.fired(), "no resample happened: nothing to report");
            assert!(!tally.accepted_after_budget);
            assert_eq!(tally.attempts, 0);
            let signals = actor
                .signals_handle()
                .snapshot()
                .await
                .expect("signals snapshot");
            assert_eq!(signals.doom_loop_recovery_attempts, 0);
            assert_eq!(signals.doom_loop_recovery_accepted_after_budget, 0);
            assert_eq!(signals.doom_loop_recovery_top_trigger, None);
        })
        .await;
}
/// A recovered turn updates the per-turn tally and structured counters for
/// resampled and budget-accepted doom-loop generations.
#[tokio::test(flavor = "current_thread")]
async fn doom_loop_recovery_updates_tally_and_counters() {
    use sampler::{RequestId, SamplingChannel, SamplingErrorKind, SamplingEvent};
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let mut fixture = make_replay_send_update_fixture().await;
            fixture.actor.doom_loop_recovery =
                Some(sampling_types::DoomLoopRecoveryPolicy::default());
            let actor = Arc::new(fixture.actor);
            *actor
                .current_prompt_id
                .lock()
                .expect("current_prompt_id mutex poisoned") = Some("prompt-doom".to_string());
            let req = RequestId::random();
            actor
                .handle_sampling_event(SamplingEvent::StreamStarted {
                    request_id: req.clone(),
                    timestamp_ms: 0,
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::ChannelToken {
                    request_id: req.clone(),
                    channel: SamplingChannel::Reasoning,
                    text: "loop loop loop".to_string(),
                    chunk_index: 0,
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::Retrying {
                    request_id: req.clone(),
                    attempt: 1,
                    max_retries: 2,
                    kind: SamplingErrorKind::DoomLoopDetected,
                    reason: "doom loop detected: tail_repetition:8@thinking".to_string(),
                    doom_loop_triggers: Some(vec!["tail_repetition:8@thinking".to_string()]),
                    doom_loop_aborted_at_chunk: Some(421),
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::StreamStarted {
                    request_id: req.clone(),
                    timestamp_ms: 1,
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::ChannelToken {
                    request_id: req.clone(),
                    channel: SamplingChannel::Reasoning,
                    text: "still looping".to_string(),
                    chunk_index: 0,
                })
                .await;
            let response = sampling_types::ConversationResponse {
                items: vec![sampling_types::ConversationItem::assistant(
                    "still looping answer",
                )],
                stop_reason: None,
                usage: None,
                cost_usd_ticks: None,
                message_chunks_emitted: 1,
                doom_loop_signals: vec![sampling_types::doom_loop::DoomLoopSignal::parse(
                    "tail_repetition:4@thinking",
                )],
                stop_message: None,
                message_id: None,
                raw_stop_reason: None,
                stop_sequence: None,
            };
            actor
                .handle_sampling_event(SamplingEvent::Completed {
                    request_id: req,
                    response: Box::new(response),
                    metrics: Default::default(),
                })
                .await;
            let tally = actor.doom_loop_turn_tally.lock().clone();
            assert_eq!(tally.attempts, 1);
            assert!(tally.accepted_after_budget);
            assert_eq!(
                tally.top_trigger.as_deref(),
                Some("tail_repetition:4@thinking"),
                "tightest across resample + accept"
            );
            let signals = actor
                .signals_handle()
                .snapshot()
                .await
                .expect("signals snapshot");
            assert_eq!(signals.doom_loop_recovery_attempts, 1);
            assert_eq!(signals.doom_loop_recovery_accepted_after_budget, 1);
            assert_eq!(signals.doom_loop_recovery_aborted_chunks, 421);
            assert_eq!(
                signals.doom_loop_recovery_top_trigger.as_deref(),
                Some("tail_repetition:4@thinking")
            );
        })
        .await;
}

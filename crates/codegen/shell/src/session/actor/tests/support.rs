#![allow(dead_code)]
use super::*;

#[cfg(test)]
pub(crate) async fn admit_test_human_input(actor: &SessionActor, label: &str) -> Vec<String> {
    let input_id = format!("test-input-{label}");
    let payload = crate::session::input_inbox::tests::text_payload(label, label);
    let payload_ref =
        crate::session::input_inbox::write_payload(&actor.session_directory, &payload)
            .expect("test input payload must be persisted before admission");
    actor
        .chat_state_handle
        .submit_input_durably(
            input_id.clone(),
            chat_state::InputIntent::Prompt,
            payload_ref,
        )
        .await
        .unwrap();
    let occurrence_id = format!("test-hook-{label}");
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Hook(
            chat_state::HookEvent::Triggered {
                occurrence_id: occurrence_id.clone(),
                event: chat_state::HookEventType::UserPromptSubmit,
                gate: chat_state::HookGateKind::Prompt,
                cause: chat_state::HookCause::Input {
                    input_id: input_id.clone(),
                },
                config_generation: 1,
                handlers: Vec::new(),
            },
        ))
        .await
        .unwrap();
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Hook(
            chat_state::HookEvent::Completed {
                occurrence_id,
                decision: chat_state::HookAggregateDecision::Prompt {
                    decision: chat_state::HookGateDecision::Allow,
                },
            },
        ))
        .await
        .unwrap();
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Input(
            chat_state::InputEvent::AdmissionResolved {
                input_id: input_id.clone(),
                decision: chat_state::InputAdmissionDecision::Allow,
                route: Some(chat_state::InputRoute::Fifo),
                supersedes: Vec::new(),
            },
        ))
        .await
        .unwrap();
    vec![input_id]
}
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
    begin_test_causal_turn_with_identity(
        actor,
        chat_state::TurnIdentity {
            origin: "test".into(),
            turn_kind: "internal".into(),
            goal_id: None,
            goal_definition_revision: None,
            stage_id: None,
        },
        Vec::new(),
    )
    .await;
}

#[cfg(test)]
async fn begin_test_causal_turn_with_identity(
    actor: &SessionActor,
    identity: chat_state::TurnIdentity,
    input_ids: Vec<String>,
) {
    actor.events.begin_turn();
    actor
        .events
        .start_turn(crate::session::events::Event::TurnStarted {
            prompt_id: "fixture-prompt".into(),
            session_id: actor.session_id_string(),
            turn_number: 1,
            input_ids,
            identity,
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

#[cfg(test)]
pub(crate) async fn begin_test_active_causal_turn_with_origin(
    actor: &SessionActor,
    prompt_id: &str,
    origin: crate::session::PromptOrigin,
    turn_kind: crate::session::TurnKind,
) {
    let input_ids = if turn_kind == crate::session::TurnKind::User {
        admit_test_human_input(actor, prompt_id).await
    } else {
        Vec::new()
    };
    begin_test_causal_turn_with_identity(actor, origin.turn_identity(turn_kind), input_ids).await;
    let mut task = running_task_stub(prompt_id);
    task.origin = origin;
    task.turn_kind = turn_kind;
    actor.state.lock().await.foreground = ForegroundState::RegularTurn(task);
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
                prompt_id: format!("test-prompt-{}", id.0),
                input_ids: Vec::new(),
                identity: chat_state::TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
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
                    source: chat_state::TurnTerminalSource::Host,
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
        cwd: std::env::temp_dir(),
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
    let (actor, events, _) = create_test_actor_ex_inner(
        total_tokens,
        context_window,
        threshold_percent,
        gateway_tx,
        persistence_tx,
        false,
        ProjectionBarrier::Inject(None),
        None,
        None,
    )
    .await;
    (actor, events)
}

#[cfg(test)]
pub(crate) async fn create_test_actor_with_planning_driver(
    total_tokens: u64,
    context_window: u64,
    threshold_percent: u8,
    gateway_tx: tokio::sync::mpsc::UnboundedSender<acp_transport::AcpClientMessage>,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
) -> std::sync::Arc<SessionActor> {
    let (actor, planning_rx) = create_test_actor_with_planning_receiver(
        total_tokens,
        context_window,
        threshold_percent,
        gateway_tx,
        persistence_tx,
    )
    .await;
    let actor = std::sync::Arc::new(actor);
    start_test_planning_driver(&actor, planning_rx);
    actor
}

#[cfg(test)]
pub(crate) async fn create_test_actor_with_planning_receiver(
    total_tokens: u64,
    context_window: u64,
    threshold_percent: u8,
    gateway_tx: tokio::sync::mpsc::UnboundedSender<acp_transport::AcpClientMessage>,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
) -> (
    SessionActor,
    tokio::sync::mpsc::UnboundedReceiver<
        tools::implementations::grow_build::todo::runtime::PlanningCommand,
    >,
) {
    let (actor, _events, planning_rx) = create_test_actor_ex_inner(
        total_tokens,
        context_window,
        threshold_percent,
        gateway_tx,
        persistence_tx,
        true,
        ProjectionBarrier::Inject(None),
        None,
        None,
    )
    .await;
    (actor, planning_rx)
}

#[cfg(test)]
pub(crate) fn start_test_planning_driver(
    actor: &std::sync::Arc<SessionActor>,
    mut planning_rx: tokio::sync::mpsc::UnboundedReceiver<
        tools::implementations::grow_build::todo::runtime::PlanningCommand,
    >,
) {
    let weak_actor = std::sync::Arc::downgrade(&actor);
    tokio::task::spawn_local(async move {
        while let Some(command) = planning_rx.recv().await {
            let Some(actor) = weak_actor.upgrade() else {
                break;
            };
            actor.handle_planning_command(command).await;
        }
    });
}

#[cfg(test)]
pub(crate) async fn install_test_planning_resources(
    actor: &std::sync::Arc<SessionActor>,
    prompt_id: &str,
) {
    actor
        .agent
        .borrow()
        .tool_bridge()
        .update_resource(
            tools::implementations::grow_build::todo::runtime::PlanningRuntimeHandle(
                actor.planning_command_tx.clone(),
            ),
        )
        .await;
    actor
        .agent
        .borrow()
        .tool_bridge()
        .update_resource(
            tools::implementations::grow_build::task::types::CurrentPromptIdResource(
                prompt_id.to_owned(),
            ),
        )
        .await;
}

/// Test-only `TimelinePersistence` for chat-state whose acknowledgement of
/// `ImageProjection` commits can be made to fail permanently.
///
/// Every other event behaves exactly like [`chat_state::NullTimelinePersistence`]:
/// acknowledged immediately and kept nowhere. Image projections are the only
/// events this harness observes, so a test can prove that the shell never
/// resubmits from an in-memory-only projection.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct ImageProjectionPersistence {
    storage: std::sync::Arc<
        std::sync::Mutex<
            Option<(
                crate::session::storage::jsonl::JsonlStorageAdapter,
                SessionInfo,
            )>,
        >,
    >,
    fail: std::sync::Arc<std::sync::atomic::AtomicBool>,
    hold: std::sync::Arc<std::sync::atomic::AtomicBool>,
    hold_sideband: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pending_sideband_ack:
        std::sync::Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<std::io::Result<()>>>>>,
    pending_ack:
        std::sync::Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<std::io::Result<()>>>>>,
    committed: std::sync::Arc<std::sync::Mutex<Vec<(chat_state::TimelineEvent, bool)>>>,
}

#[cfg(test)]
impl ImageProjectionPersistence {
    pub(crate) fn persist_to_storage(
        &self,
        storage: crate::session::storage::jsonl::JsonlStorageAdapter,
        info: SessionInfo,
    ) {
        *self.storage.lock().unwrap() = Some((storage, info));
    }

    /// Reject every later `ImageProjection` commit with a permanent I/O error.
    pub(crate) fn fail_projection_commits(&self) {
        self.fail.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn hold_next_sideband_attempt(&self) {
        self.hold_sideband
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn delay_sideband_ack(
        &self,
        reply: tokio::sync::oneshot::Sender<std::io::Result<()>>,
    ) -> Option<tokio::sync::oneshot::Sender<std::io::Result<()>>> {
        if self
            .hold_sideband
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            *self.pending_sideband_ack.lock().unwrap() = Some(reply);
            None
        } else {
            Some(reply)
        }
    }

    pub(crate) fn sideband_ack_is_pending(&self) -> bool {
        self.pending_sideband_ack.lock().unwrap().is_some()
    }

    pub(crate) fn release_sideband_ack(&self) {
        if let Some(reply) = self.pending_sideband_ack.lock().unwrap().take() {
            let _ = reply.send(Ok(()));
        }
    }

    pub(crate) fn hold_projection_acknowledgement(&self) {
        self.hold.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn release_projection_acknowledgement(&self) {
        let reply = self
            .pending_ack
            .lock()
            .unwrap()
            .take()
            .expect("held projection ACK");
        let mut events = self.committed.lock().unwrap();
        let (_, acknowledged) = events
            .iter_mut()
            .rev()
            .find(|(event, _)| {
                matches!(
                    event.kind,
                    chat_state::TimelineEventKind::ImageProjection(_)
                )
            })
            .expect("held projection event");
        *acknowledged = true;
        self.hold.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = reply.send(Ok(()));
    }

    /// Image projections the actor tried to persist, with their acknowledge
    /// outcome. A permanently failed attempt still appears exactly once.
    pub(crate) fn attempted_projections(&self) -> Vec<(chat_state::TimelineEvent, bool)> {
        self.committed
            .lock()
            .expect("image projection log must not be poisoned")
            .iter()
            .filter(|(event, _)| {
                matches!(
                    event.kind,
                    chat_state::TimelineEventKind::ImageProjection(_)
                )
            })
            .cloned()
            .collect()
    }

    /// The model-visible Surface implied by the *durable* prefix only.
    pub(crate) fn durable_surface(&self) -> Vec<ConversationItem> {
        let events = self
            .committed
            .lock()
            .expect("image projection log must not be poisoned")
            .iter()
            .filter(|(_, acknowledged)| *acknowledged)
            .map(|(event, _)| event.clone())
            .collect::<Vec<_>>();
        chat_state::Timeline::from_events(events)
            .expect("acknowledged events form a valid Timeline prefix")
            .surface()
            .to_vec()
    }
}

#[cfg(test)]
impl chat_state::TimelinePersistence for ImageProjectionPersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let (reply, receiver) = tokio::sync::oneshot::channel();
        let failed = matches!(
            event.kind,
            chat_state::TimelineEventKind::ImageProjection(_)
        ) && self.fail.load(std::sync::atomic::Ordering::SeqCst);
        let held = !failed
            && matches!(
                event.kind,
                chat_state::TimelineEventKind::ImageProjection(_)
            )
            && self.hold.load(std::sync::atomic::Ordering::SeqCst);
        if held {
            *self.pending_ack.lock().unwrap() = Some(reply);
            self.committed.lock().unwrap().push((event.clone(), false));
            return receiver;
        }
        self.committed
            .lock()
            .expect("image projection log must not be poisoned")
            .push((event.clone(), !failed));
        let result = if failed {
            Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "simulated full disk while persisting an image projection",
            ))
        } else {
            Ok(())
        };
        if result.is_ok()
            && let Some((storage, info)) = self.storage.lock().unwrap().clone()
        {
            let event = event.clone();
            tokio::spawn(async move {
                use crate::session::storage::StorageAdapter as _;
                let _ = reply.send(storage.append_timeline_event_durable(&info, &event).await);
            });
        } else {
            let _ = reply.send(result);
        }
        receiver
    }

    fn flush(&mut self) {}
}

/// Actor whose chat-state Timeline persistence observes image projections and
/// can fail them permanently; used to prove the shell fails closed instead of
/// resubmitting a lossy in-memory request.
#[cfg(test)]
pub(crate) async fn create_test_actor_with_image_projection_persistence(
    total_tokens: u64,
    context_window: u64,
    threshold_percent: u8,
    gateway_tx: tokio::sync::mpsc::UnboundedSender<acp_transport::AcpClientMessage>,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
) -> (SessionActor, ImageProjectionPersistence) {
    let image_projection_persistence = ImageProjectionPersistence::default();
    let (actor, _, _) = create_test_actor_ex_inner(
        total_tokens,
        context_window,
        threshold_percent,
        gateway_tx,
        persistence_tx,
        true,
        ProjectionBarrier::Inject(None),
        Some(image_projection_persistence.clone()),
        None,
    )
    .await;
    (actor, image_projection_persistence)
}

#[cfg(test)]
pub(crate) async fn create_test_actor_ex_with_projection_error(
    total_tokens: u64,
    context_window: u64,
    threshold_percent: u8,
    gateway_tx: tokio::sync::mpsc::UnboundedSender<acp_transport::AcpClientMessage>,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
    projection_error: Option<crate::session::storage::AppendUpdateError>,
) -> (
    SessionActor,
    tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
) {
    let (actor, events, _) = create_test_actor_ex_inner(
        total_tokens,
        context_window,
        threshold_percent,
        gateway_tx,
        persistence_tx,
        true,
        ProjectionBarrier::Inject(projection_error),
        None,
        None,
    )
    .await;
    (actor, events)
}

#[cfg(test)]
pub(crate) async fn create_test_actor_ex_with_physical_projection(
    total_tokens: u64,
    context_window: u64,
    threshold_percent: u8,
    gateway_tx: tokio::sync::mpsc::UnboundedSender<acp_transport::AcpClientMessage>,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
) -> (
    SessionActor,
    tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
) {
    let (actor, events, _) = create_test_actor_ex_inner(
        total_tokens,
        context_window,
        threshold_percent,
        gateway_tx,
        persistence_tx,
        true,
        ProjectionBarrier::Forward,
        None,
        None,
    )
    .await;
    (actor, events)
}

#[cfg(test)]
enum ProjectionBarrier {
    Inject(Option<crate::session::storage::AppendUpdateError>),
    Forward,
}

#[cfg(test)]
struct AdmissionAckPersistence {
    projection_barrier: ProjectionBarrier,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
}

#[cfg(test)]
impl chat_state::TimelinePersistence for AdmissionAckPersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let (respond_to, receiver) = tokio::sync::oneshot::channel();
        let is_response_admission = event
            .messages()
            .is_some_and(|messages| messages.response_admission.is_some());
        if is_response_admission
            && matches!(&self.projection_barrier, ProjectionBarrier::Inject(Some(_)))
        {
            let ProjectionBarrier::Inject(Some(error)) = std::mem::replace(
                &mut self.projection_barrier,
                ProjectionBarrier::Inject(None),
            ) else {
                unreachable!("matched an injected response admission failure")
            };
            let _ = respond_to.send(Err(error.into_io_error()));
        } else {
            match &self.projection_barrier {
                ProjectionBarrier::Inject(_) => {
                    let _ = respond_to.send(Ok(()));
                }
                ProjectionBarrier::Forward => {
                    let message = PersistenceMsg::TimelineDurablyAndAck {
                        event: event.clone(),
                        respond_to,
                    };
                    if let Err(error) = self.persistence_tx.send(message) {
                        if let PersistenceMsg::TimelineDurablyAndAck { respond_to, .. } = error.0 {
                            let _ = respond_to.send(Err(std::io::Error::new(
                                std::io::ErrorKind::BrokenPipe,
                                "test persistence observation channel closed",
                            )));
                        }
                    }
                }
            }
        }
        receiver
    }

    fn flush(&mut self) {}
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum SkillCommitAckFault {
    None,
    FailPermanent,
    PersistThenCancel,
}

#[cfg(test)]
struct SkillCommitAckPersistence {
    fault: std::sync::Mutex<SkillCommitAckFault>,
    injected: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    durable_events: std::sync::Arc<std::sync::Mutex<Vec<chat_state::TimelineEvent>>>,
    cancellation_token: tokio_util::sync::CancellationToken,
}

#[cfg(test)]
pub(crate) struct SkillCommitAckProbe {
    pub(crate) injected: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub(crate) durable_events: std::sync::Arc<std::sync::Mutex<Vec<chat_state::TimelineEvent>>>,
}

#[cfg(test)]
impl chat_state::TimelinePersistence for SkillCommitAckPersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let (respond_to, receiver) = tokio::sync::oneshot::channel();
        let is_skill_group = matches!(
            &event.kind,
            chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                skill_loads,
                ..
            }) if !skill_loads.is_empty()
        );
        if is_skill_group {
            match std::mem::replace(
                &mut *self.fault.lock().expect("skill ACK fault mutex poisoned"),
                SkillCommitAckFault::None,
            ) {
                SkillCommitAckFault::None => {
                    self.durable_events
                        .lock()
                        .expect("skill durable event mutex poisoned")
                        .push(event.clone());
                    let _ = respond_to.send(Ok(()));
                }
                SkillCommitAckFault::FailPermanent => {
                    self.injected
                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    let _ = respond_to.send(Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "injected permanent user skill group persistence failure",
                    )));
                }
                SkillCommitAckFault::PersistThenCancel => {
                    self.injected
                        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                    self.durable_events
                        .lock()
                        .expect("skill durable event mutex poisoned")
                        .push(event.clone());
                    drop(respond_to);
                    self.cancellation_token.cancel();
                }
            }
        } else {
            self.durable_events
                .lock()
                .expect("skill durable event mutex poisoned")
                .push(event.clone());
            let _ = respond_to.send(Ok(()));
        }
        receiver
    }

    fn flush(&mut self) {}
}

#[cfg(test)]
pub(crate) async fn create_test_actor_ex_with_skill_commit_ack_fault(
    total_tokens: u64,
    context_window: u64,
    threshold_percent: u8,
    gateway_tx: tokio::sync::mpsc::UnboundedSender<acp_transport::AcpClientMessage>,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
    fault: SkillCommitAckFault,
) -> (
    SessionActor,
    tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
    std::sync::Arc<SkillCommitAckProbe>,
) {
    let injected = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let durable_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let (actor, events, _) = create_test_actor_ex_inner(
        total_tokens,
        context_window,
        threshold_percent,
        gateway_tx,
        persistence_tx,
        false,
        ProjectionBarrier::Inject(None),
        None,
        Some((
            fault,
            injected.clone(),
            durable_events.clone(),
            cancellation_token,
        )),
    )
    .await;
    (
        actor,
        events,
        std::sync::Arc::new(SkillCommitAckProbe {
            injected,
            durable_events,
        }),
    )
}

#[cfg(test)]
async fn create_test_actor_ex_inner(
    total_tokens: u64,
    context_window: u64,
    threshold_percent: u8,
    gateway_tx: tokio::sync::mpsc::UnboundedSender<acp_transport::AcpClientMessage>,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
    bridge_projection_events: bool,
    projection_barrier: ProjectionBarrier,
    image_projection_persistence: Option<ImageProjectionPersistence>,
    skill_commit_ack: Option<(
        SkillCommitAckFault,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::Arc<std::sync::Mutex<Vec<chat_state::TimelineEvent>>>,
        tokio_util::sync::CancellationToken,
    )>,
) -> (
    SessionActor,
    tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
    tokio::sync::mpsc::UnboundedReceiver<
        tools::implementations::grow_build::todo::runtime::PlanningCommand,
    >,
) {
    // Ordinary unit-test persistence messages use a bridge to the historical
    // Update shape. Forward mode sends the Timeline event and its original ACK
    // through that bridge so tests can exercise the physical commit barrier.
    let physical_timeline = matches!(&projection_barrier, ProjectionBarrier::Forward);
    let (planning_command_tx, planning_command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (actor_persistence_tx, mut actor_persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(message) = actor_persistence_rx.recv().await {
            match message {
                PersistenceMsg::AppendUpdateDurablyAndAck { update, respond_to } => {
                    let _ = persistence_tx.send(PersistenceMsg::Update(update));
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::AppendUpdateBufferedAndAck { update, respond_to } => {
                    let result =
                        persistence_tx
                            .send(PersistenceMsg::Update(update))
                            .map_err(|_| {
                                crate::session::storage::AppendUpdateError::NotCommitted(
                                    std::io::Error::new(
                                        std::io::ErrorKind::BrokenPipe,
                                        "test persistence observation channel closed",
                                    ),
                                )
                            });
                    let _ = respond_to.send(result);
                }
                PersistenceMsg::ReplayBarrier {
                    respond_to,
                    release,
                } => {
                    let ready = respond_to.send(Ok(())).is_ok();
                    if ready {
                        let _ = release.await;
                    }
                }
                PersistenceMsg::TimelineDurablyAndAck { event, respond_to } => {
                    if physical_timeline {
                        let _ = persistence_tx
                            .send(PersistenceMsg::TimelineDurablyAndAck { event, respond_to });
                    } else {
                        let (observed_reply, _observed_ack) = tokio::sync::oneshot::channel();
                        let _ = persistence_tx.send(PersistenceMsg::TimelineDurablyAndAck {
                            event,
                            respond_to: observed_reply,
                        });
                        let _ = respond_to.send(Ok(()));
                    }
                }
                PersistenceMsg::SidebandDurablyAndAck { event, respond_to } => {
                    let (observed_reply, _observed_ack) = tokio::sync::oneshot::channel();
                    let _ = persistence_tx.send(PersistenceMsg::SidebandDurablyAndAck {
                        event,
                        respond_to: observed_reply,
                    });
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::SamplingBarrier { respond_to, .. } => {
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
    let cwd = paths::AbsPathBuf::new(std::env::temp_dir()).unwrap();
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
        termination: TerminationState::Open,
        runtime_suspend: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        pending_manual_compact: None,
        pending_step_controls: PendingStepControls::default(),
        applying_step_control: None,
        behavior_control_revision: 0,
        behavior_availability_revision: 0,
        pending_behavior_control: None,
        applying_behavior_control: None,
        behavior_control_worker_active: false,
        behavior_control_foreground_claimed: false,
        control_intents: std::collections::HashMap::new(),
        terminal_preemption_pending: false,
        pending_inputs: VecDeque::new(),
        queue_edit_holds: std::collections::HashMap::new(),
        notifications_suppressed: false,
        rewindable: false,
        nudges_used_this_session: 0,
        recent_terminals: VecDeque::new(),
    });
    let (chat_event_tx, _chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, mut raw_event_rx) = tokio::sync::mpsc::unbounded_channel::<SessionEvent>();
    let observed_event_rx = if bridge_projection_events {
        let (observed_event_tx, observed_event_rx) =
            tokio::sync::mpsc::unbounded_channel::<SessionEvent>();
        tokio::spawn(async move {
            while let Some(event) = raw_event_rx.recv().await {
                match event {
                    SessionEvent::FlushReplay {
                        respond_to: Some(respond_to),
                    } => {
                        let _ = respond_to.send(());
                    }
                    event => {
                        let _ = observed_event_tx.send(event);
                    }
                }
            }
        });
        observed_event_rx
    } else {
        raw_event_rx
    };
    let chat_state_cancellation_token = skill_commit_ack
        .as_ref()
        .map(|(_, _, _, cancellation_token)| cancellation_token.clone())
        .unwrap_or_else(tokio_util::sync::CancellationToken::new);
    let timeline_persistence: Box<dyn chat_state::TimelinePersistence> =
        match image_projection_persistence {
            Some(persistence) => Box::new(persistence),
            None => match skill_commit_ack {
                None => Box::new(AdmissionAckPersistence {
                    projection_barrier,
                    persistence_tx: persistence_tx.clone(),
                }),
                Some((fault, injected, durable_events, cancellation_token)) => {
                    Box::new(SkillCommitAckPersistence {
                        fault: std::sync::Mutex::new(fault),
                        injected,
                        durable_events,
                        cancellation_token,
                    })
                }
            },
        };
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
        timeline_persistence,
        chat_event_tx,
        chat_state_cancellation_token,
    );
    chat_state_handle.record_provider_context_anchor(total_tokens);
    let events = crate::session::events::EventTracker::new(chat_state_handle.clone());
    let (goal_command_tx, goal_command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (goal_usage_tx, _goal_usage_rx) = tokio::sync::mpsc::unbounded_channel();
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
    let control_epoch = uuid::Uuid::now_v7().to_string();
    let runtime_observation = Arc::new(parking_lot::RwLock::new(
        crate::session::actor::RuntimeObservationSnapshot::new(
            acp::SessionId::new("test-actor"),
            control_epoch.clone(),
        ),
    ));
    let rebuild_spec = crate::session::agent_rebuild::test_rebuild_spec_default();
    let mut test_agent = test_agent_default().await;
    test_agent
        .activate_resource_domain(&rebuild_spec.resource_domain)
        .await
        .expect("test actor resource domain must be active before Agent rebuilds");
    let actor = SessionActor {
        session_info: SessionInfo {
            id: acp::SessionId::new("test-actor"),
            cwd: cwd.as_str().to_string(),
        },
        control_epoch,
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
        input_admission_gate: TokioMutex::new(()),
        input_artifact_gate: TokioMutex::new(()),
        runtime_observation,
        auth_method_id: test_auth_method_id("test-auth"),
        model_auth_memo: std::cell::RefCell::new(None),
        state,
        step_control_gate: TokioMutex::new(()),
        goal_transaction_gate: TokioMutex::new(()),
        notifications: NotificationSender {
            gateway: GatewaySender::new(gateway_tx),
            gateway_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            persistence_tx,
            preview_gateway_budget: sampler::PreviewEventBudget::default(),
        },
        permissions: workspace::permission::PermissionHandle::allow_all(),
        tool_context,
        deny_read_globs: Vec::new(),
        mcp_state: Arc::new(TokioMutex::new(McpState::new(vec![]))),
        mcp: McpSessionState {
            publication_bridge: Arc::new(TokioMutex::new(test_agent.tool_bridge().clone())),
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
        pending_interactions: crate::session::pending_interaction::PendingInteractions::new(),
        compactions_remaining: std::cell::Cell::new(None),
        compaction_at_tokens: std::cell::Cell::new(None),
        doom_loop_recovery: None,
        doom_loop_turn_tally: Default::default(),
        pending_sampling_failure: Default::default(),
        file_state_tracker: Arc::new(FileStateTracker::new()),
        rewind_pending_prompt: std::sync::Mutex::new(None),
        startup_hints: StartupHints::default(),
        subagent_capabilities: None,
        compaction: crate::session::compaction_config::CompactionConfig {
            background: Default::default(),
            pending_async_notice: std::cell::Cell::new(None),
            background_failed: std::cell::Cell::new(false),
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
        background_service_shutdown: tokio_util::sync::CancellationToken::new(),
        user_question_worker: TaskSlot::new(),
        context_recall_worker: TaskSlot::new(),
        notification_reconciliation_worker: TaskSlot::new(),
        memory_reindex_worker: TaskSlot::new(),
        step_control_worker: TaskSlot::new(),
        behavior_control_worker: TaskSlot::new(),
        memory: crate::session::memory_state::SessionMemory {
            flush_config: crate::config::MemoryFlushConfig::default(),
            is_flushing: std::sync::atomic::AtomicBool::new(false),
            is_dreaming: std::sync::atomic::AtomicBool::new(false),
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
        parent_message_interrupt: tokio::sync::watch::channel(false).0,
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
        agent: std::cell::RefCell::new(test_agent),
        agent_profile: crate::session::handle::SessionAgentProfile::new(
            "grow-build".into(),
            Default::default(),
        ),
        last_reported_branch: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        git_head_enabled: false,
        models_manager: Default::default(),
        owns_permission_manager: false,
        permission_audit_bridge: parking_lot::Mutex::new(None),
        display_cwd: std::sync::OnceLock::new(),
        model_route: crate::session::handle::SessionModelRoute::new(
            crate::agent::models::ModelId::new("test"),
            sampler::SamplerConfig {
                base_url: "http://localhost".into(),
                model: "test".into(),
                context_window,
                ..Default::default()
            },
        ),
        active_skill: parking_lot::Mutex::new(None),
        turn_behavior: Arc::new(parking_lot::Mutex::new(tool_types::BehaviorId::Normal)),
        behavior: Arc::new(parking_lot::Mutex::new(
            crate::session::behavior::BehaviorCoordinator::new(),
        )),
        control_revision: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        goal_enabled: false,
        background_workflows_enabled: false,
        goal_runtime_available: std::sync::atomic::AtomicBool::new(false),
        goal_drive: TaskSlot::new(),
        goal_tracker: Arc::new(parking_lot::Mutex::new(
            crate::session::goal_tracker::GoalTracker::new(),
        )),
        goal_usage_window: crate::session::actor::goal_support::GoalUsageWindow::new(
            goal_usage_tx,
            None,
        ),
        goal_turn_task_ids: parking_lot::Mutex::new(std::collections::HashMap::new()),
        goal_command_rx: std::cell::RefCell::new(Some(goal_command_rx)),
        goal_command_tx,
        planning_command_tx,
        planning_delivery: TaskSlot::new(),
        runtime_suspension_worker: TaskSlot::new(),
        runtime_suspension_fault: std::cell::RefCell::new(None),
        runtime_suspended_waiters: std::cell::RefCell::new(Vec::new()),
        planning_delivery_dirty: std::cell::Cell::new(false),
        runtime_release_waiters: std::cell::RefCell::new(Vec::new()),
        restoration_release: std::cell::RefCell::new(None),
        runtime_completion_watchers: std::cell::RefCell::new(Vec::new()),
        restored_response: std::cell::RefCell::new(None),
        restored_response_for_stop_gate: std::cell::RefCell::new(None),
        turn_continuation: std::cell::RefCell::new(None),
        sampling_continuation: std::cell::RefCell::new(None),
        workflow_manager: crate::session::workflow::manager::WorkflowManager::test_bundle().0,
        workflow_tx: tokio::sync::mpsc::unbounded_channel().0,
        workflow_worker: TaskSlot::new(),
        workflow_service_shutdown: tokio_util::sync::CancellationToken::new(),
        user_input_generation: std::sync::atomic::AtomicU64::new(0),
        laziness_debug_log: None,
        deferred_prefix: TaskSlot::new(),
        restored_plan_approval: TaskSlot::new(),
        idle_prompt_extension: None,
        last_announced_local_date: std::cell::Cell::new(chrono::Local::now().date_naive()),
        last_search_prompt_index: std::sync::atomic::AtomicI64::new(-1),
        last_api_request_at: std::sync::atomic::AtomicI64::new(0),
        hooks: HookSessionState {
            registry: std::cell::RefCell::new(None),
            client_hooks: Default::default(),
            generation: std::cell::Cell::new(1),
            resolved_workspace_root: String::new(),
            vcs_kind: workspace::session::git::VcsKind::Git,
            load_errors: std::cell::RefCell::new(Vec::new()),
        },
        plugin_registry: std::sync::Arc::new(parking_lot::RwLock::new(None)),
        plugin_registry_handle: None,
        events,
        current_turn_number: std::cell::Cell::new(0),
        last_recap_main_turn: std::cell::Cell::new(0),
        recap_in_flight: std::cell::Cell::new(false),
        recap_epoch: std::cell::Cell::new(0),
        session_turn_active: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        turn_stream_drained: parking_lot::Mutex::new(None),
        sampling_preview: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        replay_preview_resync: parking_lot::Mutex::new(None),
        sampler_handle: sampler::SamplerHandle::noop(),
        sampler_owner: std::cell::RefCell::new(None),
        sampler_event_drainer: TaskSlot::new(),
        rebuild_spec,
        image_description_model: parking_lot::RwLock::new(None),
        session_title_route: std::cell::RefCell::new(None.into()),
        image_projection_gate: tokio::sync::RwLock::new(()),
        image_describe_cache: Arc::new(crate::session::image_describe::ImageDescribeCache::new()),
        workspace_ops: workspace::WorkspaceOps::for_test(),
    };
    (actor, observed_event_rx, planning_command_rx)
}
#[cfg(test)]
pub(crate) async fn create_test_actor(
    total_tokens: u64,
    context_window: u64,
    threshold_percent: u8,
    gateway_tx: tokio::sync::mpsc::UnboundedSender<acp_transport::AcpClientMessage>,
    persistence_tx: tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
) -> SessionActor {
    let (actor, _) = create_test_actor_ex_with_projection_error(
        total_tokens,
        context_window,
        threshold_percent,
        gateway_tx,
        persistence_tx,
        None,
    )
    .await;
    actor
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
        input_ids: Vec::new(),
        notification_ids: Vec::new(),
        prompt_id: id.to_string(),
        turn_kind: crate::session::TurnKind::User,
        prompt_blocks: vec![acp::ContentBlock::Text(acp::TextContent::new(text.clone()))],
        client_identifier: Some(owner.to_string()),
        screen_mode: None,
        verbatim: false,
        json_schema: None,
        origin: crate::session::PromptOrigin::User,
        host_command: None,
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
        input_ids: Vec::new(),
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
        host_command: None,
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
    let (stopped_tx, stopped_rx) = tokio::sync::oneshot::channel();
    AgentTask {
        stopped: futures_util::FutureExt::shared(stopped_rx),
        prompt_id: prompt_id.to_string(),
        origin: crate::session::PromptOrigin::User,
        turn_kind: crate::session::TurnKind::User,
        turn_start_ms: 0,
        usage_epoch: 0,
        handle: tokio::task::spawn_local(async move {
            let _stopped = stopped_tx;
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        })
        .abort_handle(),
        steering_open: true,
    }
}

/// Install the foreground ownership half that production establishes before
/// `run_task` invokes `handle_prompt`. Direct turn tests bypass the admission
/// queue, but terminal settlement must still exercise the real ownership
/// transfer instead of a test-only fallback in production code.
#[cfg(test)]
pub(crate) async fn install_test_foreground(actor: &SessionActor, prompt_id: &str) {
    let mut state = actor.state.lock().await;
    assert!(
        state.foreground.is_idle(),
        "test foreground is already owned"
    );
    state.foreground = ForegroundState::RegularTurn(running_task_stub(prompt_id));
}
/// The direct-actor fixture simulates successful presentation commits. Model
/// the persistence owner's post-commit delivery instead of dropping its queue.
#[cfg(test)]
async fn drain_successful_test_persistence(
    mut receiver: tokio::sync::mpsc::UnboundedReceiver<PersistenceMsg>,
) {
    while let Some(message) = receiver.recv().await {
        if let PersistenceMsg::AuxiliaryGrow {
            delivery,
            respond_to,
            ..
        } = message
        {
            if let Some(delivery) = delivery {
                delivery.publish();
            }
            if let Some(respond_to) = respond_to {
                let _ = respond_to.send(Ok(()));
            }
        }
    }
}

#[cfg(test)]
pub(crate) async fn build_actor() -> (
    std::sync::Arc<SessionActor>,
    tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let (gateway_tx, gateway_rx) =
        tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, persistence_rx) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    tokio::spawn(drain_successful_test_persistence(persistence_rx));
    let (actor, _) = create_test_actor_ex(0, 256_000, 85, gateway_tx, persistence_tx).await;
    (std::sync::Arc::new(actor), gateway_rx)
}

#[cfg(test)]
pub(crate) async fn build_actor_with_planning_driver() -> (
    std::sync::Arc<SessionActor>,
    tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let (gateway_tx, gateway_rx) =
        tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, persistence_rx) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    tokio::spawn(drain_successful_test_persistence(persistence_rx));
    let actor =
        create_test_actor_with_planning_driver(0, 256_000, 85, gateway_tx, persistence_tx).await;
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

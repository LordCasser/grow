use super::support::*;
use super::*;
use crate::session::input_inbox::{InputPayload, InputRequestRoute};
use std::sync::Arc;
use test_support::{MockInferenceServer, ScriptedResponse};

const BODY: &str = "UNIQUE_SKILL_GUIDANCE_BODY";

fn prepared_body(args: &str) -> String {
    format!("{BODY}\n\n**ARGUMENTS:** {args}")
}

fn review_skill(path: &str, body: &str) -> tools::implementations::skills::types::SkillInfo {
    tools::implementations::skills::types::SkillInfo {
        name: "review".into(),
        description: "Review a change".into(),
        path: path.into(),
        scope: tools::implementations::skills::types::SkillScope::Local,
        body: Some(body.into()),
        ..Default::default()
    }
}

fn prompt_payload(prompt_id: &str, text: &str) -> InputPayload {
    InputPayload::Prompt {
        prompt_id: prompt_id.into(),
        requested_route: InputRequestRoute::Fifo,
        prompt_blocks: vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
        skill_invocations: Vec::new(),
        client_identifier: None,
        screen_mode: None,
        verbatim: false,
        json_schema: None,
        origin: crate::session::PromptOrigin::User,
        turn_kind: crate::session::TurnKind::User,
        queue: None,
    }
}

async fn configured_actor(
    server: &MockInferenceServer,
    skill: tools::implementations::skills::types::SkillInfo,
) -> Arc<SessionActor> {
    configured_actor_with_hook_response(server, skill, None).await
}

async fn configured_actor_with_hook_response(
    server: &MockInferenceServer,
    skill: tools::implementations::skills::types::SkillInfo,
    hook_decision: Option<&'static str>,
) -> Arc<SessionActor> {
    configured_actor_with_skills_and_hook_response(server, vec![skill], hook_decision).await
}

async fn configured_actor_with_skills_and_hook_response(
    server: &MockInferenceServer,
    skills: Vec<tools::implementations::skills::types::SkillInfo>,
    hook_decision: Option<&'static str>,
) -> Arc<SessionActor> {
    configured_actor_with_skills_and_commit_fault(server, skills, hook_decision, None)
        .await
        .0
}

async fn configured_actor_with_skills_and_commit_fault(
    server: &MockInferenceServer,
    skills: Vec<tools::implementations::skills::types::SkillInfo>,
    hook_decision: Option<&'static str>,
    commit_fault: Option<SkillCommitAckFault>,
) -> (Arc<SessionActor>, Option<Arc<SkillCommitAckProbe>>) {
    configured_actor_with_skills_commit_fault_and_hook_gate(
        server,
        skills,
        hook_decision,
        commit_fault,
        None,
        None,
    )
    .await
    .0
}

async fn configured_actor_with_skills_commit_fault_and_hook_gate(
    server: &MockInferenceServer,
    skills: Vec<tools::implementations::skills::types::SkillInfo>,
    hook_decision: Option<&'static str>,
    commit_fault: Option<SkillCommitAckFault>,
    hook_gate: Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>,
    permission_decision: Option<acp::PermissionOptionKind>,
) -> (
    (Arc<SessionActor>, Option<Arc<SkillCommitAckProbe>>),
    Option<Arc<tokio::sync::Notify>>,
) {
    let hook_reached = hook_gate.as_ref().map(|(reached, _)| reached.clone());
    let (gateway_tx, gateway_rx) =
        tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    tokio::task::spawn_local(async move {
        let mut gateway_rx = gateway_rx;
        while let Some(message) = gateway_rx.recv().await {
            match message {
                acp_transport::AcpClientMessage::SessionNotification(notification) => {
                    let _ = notification.response_tx.send(Ok(()));
                }
                acp_transport::AcpClientMessage::RequestPermission(request) => {
                    let kind = permission_decision
                        .as_ref()
                        .expect("unexpected permission request");
                    let selected = request
                        .request
                        .options
                        .iter()
                        .find(|option| &option.kind == kind)
                        .expect("requested permission option is available");
                    let response = acp::RequestPermissionResponse::new(
                        acp::RequestPermissionOutcome::Selected(
                            acp::SelectedPermissionOutcome::new(selected.option_id.clone()),
                        ),
                    );
                    let _ = request.response_tx.send(Ok(response));
                }
                acp_transport::AcpClientMessage::ExtMethod(request) => {
                    if let Some((reached, release)) = &hook_gate {
                        reached.notify_one();
                        release.notified().await;
                    }
                    let decision = hook_decision.unwrap_or("allow");
                    let response: Arc<serde_json::value::RawValue> =
                        serde_json::value::to_raw_value(&serde_json::json!({
                            "decision": decision,
                            "systemMessage": "denied by skill pipeline test",
                        }))
                        .unwrap()
                        .into();
                    let _ = request
                        .response_tx
                        .send(Ok(acp::ExtResponse::new(response)));
                }
                _ => {}
            }
        }
    });
    let (persistence_tx, mut persistence_rx) =
        tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    tokio::task::spawn_local(async move { while persistence_rx.recv().await.is_some() {} });

    let (mut actor, injected) = match commit_fault {
        Some(fault) => {
            let (actor, _, injected) = create_test_actor_ex_with_skill_commit_ack_fault(
                0,
                256_000,
                85,
                gateway_tx.clone(),
                persistence_tx,
                fault,
            )
            .await;
            (actor, Some(injected))
        }
        None => (
            create_test_actor(0, 256_000, 85, gateway_tx.clone(), persistence_tx).await,
            None,
        ),
    };
    if permission_decision.is_some() {
        use workspace::permission::types::{
            PatternMode, PermissionConfig, PermissionRule, RuleAction, ToolFilter,
        };
        let config = PermissionConfig::new(vec![PermissionRule {
            action: RuleAction::Ask,
            tool: ToolFilter::Read,
            pattern: Some("**/reference.md".into()),
            pattern_mode: PatternMode::Glob,
        }]);
        let (permissions, mut events) = workspace::permission::spawn_permission_manager(
            actor.session_info.id.clone(),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            actor.tool_context.cwd.clone(),
            workspace::permission::ClientType::GrowPager,
            std::time::Duration::from_secs(5),
            Some(config),
            vec![],
            vec![],
            ::diagnostics::enums::PermissionMode::Ask,
            None,
            false,
        );
        actor.permissions = permissions;
        tokio::task::spawn_local(async move { while events.recv().await.is_some() {} });
    }
    let tool_config = tools::registry::types::ToolConfig::for_tool::<
        tools::implementations::grow_build::LoadSkillTool,
    >();
    *actor.agent.borrow_mut() = test_agent_with_tools(vec![
        tool_config,
        // The mock classifies headerless requests with at least two tools as
        // foreground work; keep these real actor turns on that route.
        tools::registry::types::ToolConfig::for_tool::<
            tools::implementations::grow_build::read_file::ReadFileTool,
        >(),
    ])
    .await;
    let bridge = actor.agent.borrow().tool_bridge().clone();
    bridge
        .seed_skill_discovery(
            Some(actor.tool_context.cwd.as_path().to_path_buf()),
            None,
            skills,
            None,
            Some(256_000),
        )
        .await;
    actor
        .workspace_ops
        .bind_local_session(
            &actor.session_id_string(),
            actor.tool_context.cwd.as_path().to_path_buf(),
            actor.tool_context.hunk_tracker_handle.clone(),
            actor.agent.borrow().tool_bridge().toolset(),
            None,
        )
        .await
        .expect("bind test tool session");

    let sampling = sampler::SamplerConfig {
        api_key: Some("test-key".into()),
        base_url: server.url(),
        model: "test".into(),
        api_backend: sampler::ApiBackend::Responses,
        context_window: 256_000,
        max_retries: Some(0),
        idle_timeout_secs: Some(30),
        ..Default::default()
    };
    let (sampler_event_tx, mut sampler_event_rx) =
        tokio::sync::mpsc::unbounded_channel::<sampler::SamplingEvent>();
    actor.sampler_handle = sampler::SamplerActor::spawn(
        sampling,
        sampler::RetryPolicy {
            max_retries: 0,
            rate_limit_retry_threshold: 0,
            ..Default::default()
        },
        sampler_event_tx,
    );
    actor.max_retries.set(1);
    let mut route = actor
        .chat_state_handle
        .get_sampling_config()
        .await
        .expect("test sampling config");
    route.base_url = server.url();
    route.api_backend = sampling_types::ApiBackend::Responses;
    route.model = "test".into();
    actor.chat_state_handle.replace_sampling_route(route);
    let mut credentials = actor.chat_state_handle.get_credentials().await;
    credentials.api_key = Some("test-key".into());
    actor.chat_state_handle.update_credentials(credentials);

    let actor = Arc::new(actor);
    let event_actor = actor.clone();
    tokio::task::spawn_local(async move {
        while let Some(event) = sampler_event_rx.recv().await {
            event_actor.handle_sampling_event(event).await;
        }
    });
    ((actor, injected), hook_reached)
}

fn install_load_skill_hook(actor: &SessionActor, events: &[::hooks::event::HookEventName]) {
    let mut client_hooks = crate::extensions::hooks::ClientHooks::new();
    for event in events {
        client_hooks.insert(
            *event,
            vec![crate::extensions::hooks::ClientHookGroup {
                matcher: Some(::hooks::matcher::HookMatcher::new("load_skill").unwrap()),
                callback_ids: vec![format!("skill_hook_{event:?}")],
                timeout: None,
            }],
        );
    }
    *actor.hooks.client_hooks.borrow_mut() = client_hooks;
}

fn install_load_skill_deny_hook(actor: &SessionActor) {
    install_load_skill_hook(actor, &[::hooks::event::HookEventName::PreToolUse]);
}

async fn admit_review_prompt(actor: &SessionActor, prompt_id: &str) -> String {
    admit_prompt(actor, prompt_id, "/review inspect").await
}

async fn admit_prompt(actor: &SessionActor, prompt_id: &str, text: &str) -> String {
    actor
        .admit_human_input(
            chat_state::InputIntent::Prompt,
            prompt_payload(prompt_id, text),
            Some(prompt_id.into()),
            chat_state::InputRoute::Fifo,
            Vec::new(),
        )
        .await
        .expect("explicit skill prompt admitted")
        .input_id
}

async fn run_prompt(
    actor: &Arc<SessionActor>,
    prompt_id: &str,
    input_id: String,
) -> PromptTurnResult {
    run_prompt_text(actor, prompt_id, input_id, "/review inspect").await
}

async fn run_prompt_text(
    actor: &Arc<SessionActor>,
    prompt_id: &str,
    input_id: String,
    text: &str,
) -> PromptTurnResult {
    install_test_foreground(actor, prompt_id).await;
    actor
        .handle_prompt(
            prompt_id,
            vec![input_id],
            crate::session::PromptOrigin::User,
            Vec::new(),
            crate::session::TurnKind::User,
            vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
            tool_types::BehaviorId::Normal,
            None,
            None,
            false,
            None,
            None,
        )
        .await
}

fn body_occurs(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::String(value) => {
            value.contains(needle)
                || serde_json::from_str::<serde_json::Value>(value)
                    .is_ok_and(|decoded| body_occurs(&decoded, needle))
        }
        serde_json::Value::Array(values) => values.iter().any(|value| body_occurs(value, needle)),
        serde_json::Value::Object(values) => {
            values.values().any(|value| body_occurs(value, needle))
        }
        _ => false,
    }
}

async fn begin_skill_compaction(
    actor: &SessionActor,
    id: &str,
) -> (chat_state::SurfaceRange, usize) {
    let handle = &actor.chat_state_handle;
    let materialized = handle
        .materialize_timeline(actor.session_id_string())
        .await
        .expect("current Surface materializes");
    let start = usize::from(matches!(
        materialized.surface.first(),
        Some(ConversationItem::System(_))
    ));
    let shadowed = materialized.surface_ids[start..].to_vec();
    assert!(!shadowed.is_empty(), "compaction requires a Surface body");
    let target = chat_state::SurfaceRange {
        start: shadowed[0],
        end: *shadowed.last().unwrap(),
        shadowed,
    };
    let input_ref = materialized.input_ref.clone();
    let source_items = materialized.surface.len();
    let prompt_index = handle.get_prompt_index().await;
    handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
            chat_state::CompactionEvent::Started {
                mode: chat_state::CompactionMode::Foreground,
                id: id.into(),
                source_items,
                prompt_index,
            },
        ))
        .await
        .expect("compaction start persists");
    let sideband_id = uuid::Uuid::now_v7().to_string();
    handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Sideband(
            chat_state::SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose: chat_state::SidebandPurpose::CompactionSummary,
                source_refs: vec![input_ref.clone()],
            },
        ))
        .await
        .expect("summary source persists");
    handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
            chat_state::CompactionEvent::Summary {
                id: id.into(),
                input_ref,
                result_ref: chat_state::TimelineRangeRef {
                    timeline_id: sideband_id,
                    first_seq: 2,
                    last_seq: 2,
                },
                target: target.clone(),
                source_tokens: 100,
                summary_chars: 7,
            },
        ))
        .await
        .expect("summary link persists");
    (target, source_items)
}

#[tokio::test(flavor = "current_thread")]
async fn queued_and_direct_skill_steers_keep_images_literal_text_and_atomic_identity() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for queued in [false, true] {
                let server = MockInferenceServer::start().await.unwrap();
                let mut first_events =
                    test_support::sse::responses_api_script_exact("first answer", "test");
                // The fixture blocks its last event. Responses completes before
                // [DONE], so gate response.completed while steering is still open.
                assert_eq!(first_events.pop().unwrap().data, "[DONE]");
                let mut first = server.expect_response_blocked(
                    "foreground before skill steer",
                    test_support::InferenceRequestMatcher::foreground(
                        test_support::InferenceEndpoint::Responses,
                    ),
                    ScriptedResponse::sse(first_events),
                );
                server.enqueue_response(
                    "/v1/responses",
                    ScriptedResponse::sse(test_support::sse::responses_api_script_exact(
                        "steered answer",
                        "test",
                    )),
                );
                let actor =
                    configured_actor(&server, review_skill("/skills/review/SKILL.md", BODY)).await;
                let initial_input =
                    admit_prompt(&actor, "active-skill-steer", "original task").await;
                let running_actor = actor.clone();
                let turn = tokio::task::spawn_local(async move {
                    run_prompt_text(
                        &running_actor,
                        "active-skill-steer",
                        initial_input,
                        "original task",
                    )
                    .await
                });
                tokio::time::timeout(std::time::Duration::from_secs(5), first.wait_blocked())
                    .await
                    .unwrap();
                let text = "/review inspect <user_query>literal user text</user_query>";
                let image = test_image_content();
                let expected_display_blocks = vec![
                    acp::ContentBlock::Text(acp::TextContent::new(text)),
                    acp::ContentBlock::Image(image.clone()),
                ];
                let input_id = if queued {
                    let (mut item, completion) = user_item_with_rx("queued-skill-steer", "pager");
                    item.prompt_blocks = vec![
                        acp::ContentBlock::Text(acp::TextContent::new(text)),
                        acp::ContentBlock::Image(image),
                    ];
                    item.queue_meta.as_mut().unwrap().text = text.into();
                    let mut payload = prompt_payload(&item.prompt_id, text);
                    let InputPayload::Prompt {
                        prompt_blocks,
                        queue,
                        ..
                    } = &mut payload;
                    *prompt_blocks = item.prompt_blocks.clone();
                    *queue = Some(item.queue_meta.as_ref().unwrap().into());
                    let admitted = actor
                        .admit_human_input(
                            chat_state::InputIntent::Followup,
                            payload,
                            Some(item.prompt_id.clone()),
                            chat_state::InputRoute::Fifo,
                            Vec::new(),
                        )
                        .await
                        .unwrap();
                    let input_id = admitted.input_id;
                    item.input_ids = vec![input_id.clone()];
                    actor.state.lock().await.pending_inputs.push_back(item);
                    actor
                        .handle_steer_queued_prompt(
                            "active-skill-steer",
                            "queued-skill-steer",
                            0,
                            Some("pager"),
                            None,
                        )
                        .await;
                    tokio::time::timeout(std::time::Duration::from_secs(5), completion)
                        .await
                        .expect("the queued steer must be acknowledged before response release")
                        .unwrap()
                        .unwrap();
                    input_id
                } else {
                    actor
                        .admit_human_steer(
                            "active-skill-steer",
                            text.into(),
                            vec![image],
                            Some("direct-skill-steer".into()),
                        )
                        .await
                        .unwrap();
                    actor.pending_interjections.snapshot()[0]
                        .requeue
                        .as_ref()
                        .unwrap()
                        .input_ids[0]
                        .clone()
                };
                first.release();
                let result = tokio::time::timeout(std::time::Duration::from_secs(10), turn)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert_eq!(result.stop_reason, acp::StopReason::EndTurn);
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let consumed = events
                    .iter()
                    .filter_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Consumed {
                                input_ids,
                                item,
                                skill_loads,
                                display_echo,
                                ..
                            },
                        ) if input_ids == std::slice::from_ref(&input_id) => {
                            Some((item, skill_loads, display_echo))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(consumed.len(), 1);
                let (item, loads, echo) = consumed[0];
                assert_eq!(loads.len(), 1);
                assert!(loads[0].prepared_body.contains(BODY));
                assert!(item.text_content().contains(text));
                assert!(!item.text_content().contains(BODY));
                let image_groups =
                    sampling_types::conversation_image_groups(std::slice::from_ref(item));
                assert_eq!(image_groups.len(), 1);
                assert_eq!(image_groups[0].image_count(), 1);
                let echo = echo.as_ref().expect("steer has a durable display echo");
                let expected_echo = super::super::user_display_echo(
                    echo.message_id.clone(),
                    echo.prompt_index,
                    &expected_display_blocks,
                );
                assert_eq!(echo.block_count, 2);
                assert_eq!(
                    echo.blake3, expected_echo.blake3,
                    "display binds the literal text and image rather than parsing user_query tags"
                );
                assert!(actor.pending_interjections.is_empty());
                assert!(actor.state.lock().await.pending_inputs.is_empty());
                let requests = server.request_bodies();
                assert_eq!(
                    requests.len(),
                    2,
                    "queued={queued}; result={result:?}; timeline={events:#?}; requests={}",
                    server.request_log_summary()
                );
                assert!(!body_occurs(&requests[0], BODY));
                assert!(body_occurs(&requests[1], BODY));
                assert!(body_occurs(&requests[1], "literal user text"));
                assert!(body_occurs(&requests[1], "data:image/png;base64,"));
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Turn(
                                chat_state::TurnEvent::Started { .. }
                            )
                        ))
                        .count(),
                    1,
                    "steering keeps the original Turn"
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start()
                .await
                .expect("mock inference server");
            server.enqueue_response(
                "/v1/responses",
                ScriptedResponse::sse(test_support::sse::responses_api_script_exact(
                    "review complete",
                    "test",
                )),
            );
            let actor = configured_actor_with_hook_response(
                &server,
                review_skill("/skills/review/SKILL.md", BODY),
                Some("allow"),
            )
            .await;
            install_load_skill_hook(
                &actor,
                &[
                    ::hooks::event::HookEventName::PreToolUse,
                    ::hooks::event::HookEventName::PostToolUse,
                ],
            );
            let prompt_id = "user-skill-prompt";
            let input_id = admit_review_prompt(&actor, prompt_id).await;

            let result = run_prompt(&actor, prompt_id, input_id.clone())
                .await
                .expect("user skill pipeline succeeds");
            assert_eq!(
                result.stop_reason,
                acp::StopReason::EndTurn,
                "result={result:#?}; events={:#?}; requests={:#?}",
                actor.chat_state_handle.timeline_events().await,
                server.request_bodies()
            );

            let expected_body = prepared_body("inspect");
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let requests = server.request_bodies();
            assert_eq!(requests.len(), 1, "requests={requests:#?}");
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Messages(messages)
                            if messages.response_admission.is_some()
                    ))
                    .count(),
                1,
                "the response must be canonically admitted: {events:#?}"
            );
            let consumed = events
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                        input_ids,
                        item,
                        skill_loads,
                        ..
                    }) if input_ids == std::slice::from_ref(&input_id) => Some((item, skill_loads)),
                    _ => None,
                })
                .expect("one atomic consumed event");
            assert_eq!(consumed.1.len(), 1);
            assert_eq!(consumed.1[0].prepared_body.as_ref(), expected_body.as_str());
            assert!(consumed.0.text_content().contains("/review inspect"));
            assert!(!consumed.0.text_content().contains(BODY));
            let surface_before_completion = actor.chat_state_handle.get_conversation().await;
            assert!(
                surface_before_completion.iter().any(|item| {
                    matches!(item, ConversationItem::SkillLoad(skill)
                    if skill.source == sampling_types::SkillLoadSource::User
                        && skill.prepared_body.as_ref() == expected_body.as_str())
                }),
                "surface={surface_before_completion:#?}; timeline={events:#?}"
            );
            assert!(
                requests
                    .iter()
                    .any(|request| body_occurs(request, &expected_body))
            );
            let call_id = &consumed.1[0].call_id;
            let event_seq = |predicate: &dyn Fn(&chat_state::TimelineEventKind) -> bool| {
                events
                    .iter()
                    .find(|event| predicate(&event.kind))
                    .map(|event| event.seq.get())
                    .unwrap_or_else(|| panic!("expected ordered skill event: {events:#?}"))
            };
            let tool_started = event_seq(&|kind| matches!(
                kind,
                chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Started {
                    call_id: id,
                    name,
                    ..
                }) if id == call_id && name == "load_skill"
            ));
            let pre_hook_started = event_seq(&|kind| matches!(
                kind,
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                    event: chat_state::HookEventType::PreToolUse,
                    cause: chat_state::HookCause::Tool { call_id: id },
                    ..
                }) if id == call_id
            ));
            let pre_hook_completed = event_seq(&|kind| matches!(
                kind,
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                    decision: chat_state::HookAggregateDecision::Tool {
                        decision: chat_state::HookGateDecision::Allow,
                    },
                    ..
                })
            ));
            let input_consumed = event_seq(&|kind| matches!(
                kind,
                chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                    input_ids,
                    skill_loads,
                    ..
                }) if input_ids.contains(&input_id) && skill_loads.len() == 1
            ));
            let tool_completed = event_seq(&|kind| matches!(
                kind,
                chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Completed {
                    call_id: id,
                    name,
                    outcome,
                    ..
                }) if id == call_id && name == "load_skill" && outcome == "completed"
            ));
            let post_hook_started = event_seq(&|kind| matches!(
                kind,
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                    event: chat_state::HookEventType::PostToolUse,
                    cause: chat_state::HookCause::Tool { call_id: id },
                    ..
                }) if id == call_id
            ));
            assert!(
                tool_started < pre_hook_started
                    && pre_hook_started < pre_hook_completed
                    && pre_hook_completed < input_consumed
                    && input_consumed < tool_completed
                    && tool_completed < post_hook_started,
                "Hook and publication order must follow the real dispatch lifecycle: started={tool_started}, pre={pre_hook_started}/{pre_hook_completed}, consumed={input_consumed}, completed={tool_completed}, post={post_hook_started}; events={events:#?}"
            );
            actor.handle_completion(prompt_id.into(), Ok(result)).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn complete_request_headroom_rejects_user_skill_group_before_sampling() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start()
                .await
                .expect("mock inference server");
            let large_body = "oversized-guidance ".repeat(8_000);
            let actor = configured_actor(
                &server,
                review_skill("/skills/review/SKILL.md", &large_body),
            )
            .await;
            let mut route = actor
                .chat_state_handle
                .get_sampling_config()
                .await
                .expect("sampling route");
            route.context_window = std::num::NonZeroU64::new(4_096).unwrap();
            actor.chat_state_handle.replace_sampling_route(route);
            let route = actor
                .chat_state_handle
                .get_sampling_config()
                .await
                .expect("updated sampling route");
            assert_eq!(route.context_window.get(), 4_096);

            let prompt_id = "skill-group-headroom-rejection";
            let text = "/review inspect";
            let input_id = admit_prompt(&actor, prompt_id, text).await;
            let result = run_prompt_text(&actor, prompt_id, input_id.clone(), text).await;
            assert!(
                result.is_err(),
                "the complete user-plus-skill request must fail headroom admission: {result:#?}"
            );
            assert!(
                server.request_bodies().is_empty(),
                "no provider request can receive a group that exceeds context headroom"
            );
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(
                !events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                        input_ids,
                        skill_loads,
                        ..
                    }) if input_ids.contains(&input_id) && !skill_loads.is_empty()
                )),
                "headroom rejection cannot publish a partial group: {events:#?}"
            );
            let surface = actor.chat_state_handle.get_conversation().await;
            assert!(surface.iter().all(|item| {
                !matches!(item, ConversationItem::SkillLoad(_))
                    && !item.text_content().contains("oversized-guidance")
            }));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn user_skill_remains_pinned_until_first_accepted_response_then_compacts() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start()
                .await
                .expect("mock inference server");
            let mut response = server.expect_response_blocked(
                "first response after user skill",
                test_support::InferenceRequestMatcher::foreground(
                    test_support::InferenceEndpoint::Responses,
                ),
                ScriptedResponse::sse(test_support::sse::responses_api_script_exact(
                    "skill consumed",
                    "test",
                )),
            );
            let actor =
                configured_actor(&server, review_skill("/skills/review/SKILL.md", BODY)).await;
            let prompt_id = "skill-first-response-pin";
            let text = "/review inspect";
            let input_id = admit_prompt(&actor, prompt_id, text).await;
            install_test_foreground(&actor, prompt_id).await;
            let turn_actor = actor.clone();
            let prompt_id_owned = prompt_id.to_string();
            let turn = tokio::task::spawn_local(async move {
                turn_actor
                    .handle_prompt(
                        &prompt_id_owned,
                        vec![input_id],
                        crate::session::PromptOrigin::User,
                        Vec::new(),
                        crate::session::TurnKind::User,
                        vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
                        tool_types::BehaviorId::Normal,
                        None,
                        None,
                        false,
                        None,
                        None,
                    )
                    .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(10), response.wait_blocked())
                .await
                .expect("foreground response reaches its terminal barrier");
            let pinned_surface = actor.chat_state_handle.get_conversation().await;
            assert!(pinned_surface.iter().any(|item| matches!(
                item,
                ConversationItem::SkillLoad(skill)
                    if skill.source == sampling_types::SkillLoadSource::User
                        && skill.prepared_body.as_ref() == prepared_body("inspect")
            )));

            let (target, _source_items) =
                begin_skill_compaction(&actor, "before-first-response").await;
            assert!(
                matches!(
                    actor
                        .chat_state_handle
                        .replace_compaction_range(
                            target,
                            vec![ConversationItem::user_meta("early summary")],
                        )
                        .await,
                    Err(chat_state::TimelineWriteError::Invalid(
                        chat_state::TimelineError::UnconsumedSkillLoadProtected
                    ))
                ),
                "unaccepted responses must leave fresh Skill body pinned"
            );
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
                    chat_state::CompactionEvent::Failed {
                        id: "before-first-response".into(),
                        duration_ms: 1,
                        error: "fresh skill is pinned".into(),
                    },
                ))
                .await
                .unwrap();
            response.release();
            let result = tokio::time::timeout(std::time::Duration::from_secs(10), turn)
                .await
                .expect("provider response is released")
                .expect("turn task joins")
                .expect("turn succeeds after accepted response");
            actor.handle_completion(prompt_id.into(), Ok(result)).await;

            let (target, source_items) =
                begin_skill_compaction(&actor, "after-first-response").await;
            actor
                .chat_state_handle
                .replace_compaction_range(
                    target,
                    vec![ConversationItem::user_meta("accepted summary")],
                )
                .await
                .expect("consumed Skill is now eligible for ordinary compaction");
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
                    chat_state::CompactionEvent::Completed {
                        id: "after-first-response".into(),
                        source_items,
                        result_items: 1,
                        duration_ms: 1,
                    },
                ))
                .await
                .unwrap();
            let compacted = actor.chat_state_handle.get_conversation().await;
            assert!(
                compacted
                    .iter()
                    .all(|item| !matches!(item, ConversationItem::SkillLoad(_)))
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn repeated_user_skill_references_publish_one_atomic_group_with_distinct_calls() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start()
                .await
                .expect("mock inference server");
            server.enqueue_response(
                "/v1/responses",
                ScriptedResponse::sse(test_support::sse::responses_api_script_exact(
                    "both references loaded",
                    "test",
                )),
            );
            let actor =
                configured_actor(&server, review_skill("/skills/review/SKILL.md", BODY)).await;
            let prompt_id = "repeated-user-skill";
            let text = "/review inspect /review summarize";
            let input_id = admit_prompt(&actor, prompt_id, text).await;
            let result = run_prompt_text(&actor, prompt_id, input_id.clone(), text)
                .await
                .expect("repeated skill references load");
            assert_eq!(result.stop_reason, acp::StopReason::EndTurn);

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let committed = events
                .iter()
                .filter_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                        input_ids,
                        skill_loads,
                        ..
                    }) if input_ids == std::slice::from_ref(&input_id)
                        && !skill_loads.is_empty() =>
                    {
                        Some(skill_loads)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(committed.len(), 1, "events={events:#?}");
            let loads = committed[0];
            assert_eq!(loads.len(), 2, "events={events:#?}");
            assert_eq!(loads[0].input_id, input_id);
            assert_eq!(loads[1].input_id, input_id);
            assert_eq!((loads[0].reference_index, loads[1].reference_index), (0, 1));
            assert_eq!(
                (loads[0].skill_name.as_str(), loads[1].skill_name.as_str()),
                ("review", "review")
            );
            assert_eq!(
                (loads[0].args.as_str(), loads[1].args.as_str()),
                ("inspect", "summarize")
            );
            assert_ne!(loads[0].call_id, loads[1].call_id);
            assert_eq!(loads[0].prepared_body.as_ref(), prepared_body("inspect"));
            assert_eq!(loads[1].prepared_body.as_ref(), prepared_body("summarize"));

            for load in loads {
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Started {
                                call_id,
                                name,
                                ..
                            }) if call_id == &load.call_id && name == "load_skill"
                        ))
                        .count(),
                    1,
                    "each reference must have one real dispatched call: {events:#?}"
                );
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Completed {
                                call_id,
                                name,
                                ..
                            }) if call_id == &load.call_id && name == "load_skill"
                        ))
                        .count(),
                    1,
                    "each reference must have one real result: {events:#?}"
                );
            }
            let requests = server.request_bodies();
            assert_eq!(requests.len(), 1);
            assert!(body_occurs(&requests[0], &prepared_body("inspect")));
            assert!(body_occurs(&requests[0], &prepared_body("summarize")));
            actor.handle_completion(prompt_id.into(), Ok(result)).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn denied_user_skill_is_handled_without_loading_and_next_input_can_continue() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start()
                .await
                .expect("mock inference server");
            server.enqueue_response(
                "/v1/responses",
                ScriptedResponse::sse(test_support::sse::responses_api_script_exact(
                    "follow-up complete",
                    "test",
                )),
            );
            let actor = configured_actor_with_hook_response(
                &server,
                review_skill("/skills/review/SKILL.md", BODY),
                Some("deny"),
            )
            .await;
            install_load_skill_deny_hook(&actor);

            let skill_prompt_id = "denied-user-skill";
            let skill_input_id = admit_review_prompt(&actor, skill_prompt_id).await;
            let result = run_prompt(&actor, skill_prompt_id, skill_input_id.clone()).await;
            assert!(
                result.is_err(),
                "PreToolUse denial must fail explicit loading: {result:#?}"
            );

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let handled = events
                .iter()
                .filter_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Handled {
                        input_ids,
                        ..
                    }) if input_ids == std::slice::from_ref(&skill_input_id) => Some(input_ids),
                    _ => None,
                })
                .count();
            assert_eq!(handled, 1, "denied input must be durably Handled once");
            assert!(!events.iter().any(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                    input_ids,
                    ..
                }) if input_ids.contains(&skill_input_id)
            )));
            assert!(
                events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                        event: chat_state::HookEventType::PreToolUse,
                        cause: chat_state::HookCause::Tool { call_id },
                        ..
                    }) if call_id.starts_with("skill-")
                )),
                "the existing PreToolUse gate must own this denial"
            );
            assert!(
                server.request_bodies().is_empty(),
                "denial precedes sampling"
            );
            let surface = actor.chat_state_handle.get_conversation().await;
            assert!(
                !surface.iter().any(|item| {
                    matches!(item, ConversationItem::SkillLoad(_))
                        || item.text_content().contains(BODY)
                }),
                "denied body must not reach Surface: {surface:#?}"
            );
            actor
                .handle_completion(skill_prompt_id.into(), result)
                .await;

            let next_prompt_id = "after-denied-skill";
            let next_input_id = actor
                .admit_human_input(
                    chat_state::InputIntent::Prompt,
                    prompt_payload(next_prompt_id, "ordinary follow-up"),
                    Some(next_prompt_id.into()),
                    chat_state::InputRoute::Fifo,
                    Vec::new(),
                )
                .await
                .expect("follow-up input admitted")
                .input_id;
            let next = run_prompt_text(&actor, next_prompt_id, next_input_id, "ordinary follow-up")
                .await
                .expect("later input can proceed after denial");
            assert_eq!(next.stop_reason, acp::StopReason::EndTurn);
            assert_eq!(server.request_bodies().len(), 1);
            assert!(body_occurs(
                &server.request_bodies()[0],
                "ordinary follow-up"
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_while_pretool_hook_is_pending_keeps_skill_group_unpublished() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start()
                .await
                .expect("mock inference server");
            let hook_reached = Arc::new(tokio::sync::Notify::new());
            let release_hook = Arc::new(tokio::sync::Notify::new());
            let ((actor, _), _) = configured_actor_with_skills_commit_fault_and_hook_gate(
                &server,
                vec![review_skill("/skills/review/SKILL.md", BODY)],
                Some("allow"),
                None,
                Some((hook_reached.clone(), release_hook.clone())),
                None,
            )
            .await;
            install_load_skill_hook(&actor, &[::hooks::event::HookEventName::PreToolUse]);

            let prompt_id = "cancel-during-skill-preparation";
            let text = "/review inspect";
            let input_id = admit_prompt(&actor, prompt_id, text).await;
            let (start_tx, start_rx) = tokio::sync::oneshot::channel();
            let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();
            let task = AgentTask::new_prompt(
                actor.clone(),
                prompt_id.into(),
                vec![input_id.clone()],
                crate::session::PromptOrigin::User,
                None,
                Vec::new(),
                crate::session::TurnKind::User,
                vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
                tool_types::BehaviorId::Normal,
                None,
                None,
                false,
                None,
                Some(start_rx),
                None,
                completion_tx,
                None,
            );
            let producer = task.handle.clone();
            actor.state.lock().await.foreground = ForegroundState::RegularTurn(task);
            *actor.current_prompt_id.lock().unwrap() = Some(prompt_id.into());
            start_tx.send(()).expect("start actual foreground producer");

            tokio::time::timeout(std::time::Duration::from_secs(10), hook_reached.notified())
                .await
                .expect("real PreToolUse request reaches the blocked client hook");
            let events_before_cancel = actor.chat_state_handle.timeline_events().await.unwrap();
            let (occurrence_id, call_id) = events_before_cancel
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                        occurrence_id,
                        event: chat_state::HookEventType::PreToolUse,
                        cause: chat_state::HookCause::Tool { call_id },
                        ..
                    }) => Some((occurrence_id.clone(), call_id.clone())),
                    _ => None,
                })
                .expect("the real tool lifecycle reached PreToolUse");
            assert!(events_before_cancel.iter().any(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Started {
                    call_id: started_call,
                    name,
                    ..
                }) if started_call == &call_id && name == "load_skill"
            )));

            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.cancel_running_task(false, false, false, Some("esc".into())),
            )
            .await
            .expect("cancel does not wait on the blocked hook")
            .expect("cancel succeeds");
            release_hook.notify_one();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while !producer.is_finished() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("the actual foreground producer is aborted");

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(
                !events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                        input_ids,
                        skill_loads,
                        ..
                    }) if input_ids.contains(&input_id) && !skill_loads.is_empty()
                )),
                "cancellation before preparation completes cannot publish the group: {events:#?}"
            );
            assert!(
                !events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                        occurrence_id: completed_occurrence,
                        ..
                    }) if completed_occurrence == &occurrence_id
                )),
                "the cancelled hook has no fabricated completion: {events:#?}"
            );
            assert!(server.request_bodies().is_empty());
            let surface = actor.chat_state_handle.get_conversation().await;
            assert!(surface.iter().all(|item| {
                !matches!(item, ConversationItem::SkillLoad(_))
                    && !item.text_content().contains(BODY)
            }));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn second_user_skill_preparation_failure_does_not_publish_partial_group_and_fifo_continues() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start()
                .await
                .expect("mock inference server");
            server.enqueue_response(
                "/v1/responses",
                ScriptedResponse::sse(test_support::sse::responses_api_script_exact(
                    "ordinary follow-up complete",
                    "test",
                )),
            );
            let mut unavailable = review_skill("virtual://second/SKILL.md", "");
            unavailable.name = "unavailable".into();
            unavailable.body = None;
            let actor = configured_actor_with_skills_and_hook_response(
                &server,
                vec![review_skill("/skills/review/SKILL.md", BODY), unavailable],
                None,
            )
            .await;

            let failed_prompt_id = "second-skill-preparation-fails";
            let failed_text = "/review inspect /unavailable more";
            let failed_input_id = admit_prompt(&actor, failed_prompt_id, failed_text).await;
            let failure = run_prompt_text(
                &actor,
                failed_prompt_id,
                failed_input_id.clone(),
                failed_text,
            )
            .await;
            assert!(
                failure.is_err(),
                "second preparation must fail: {failure:#?}"
            );

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Input(chat_state::InputEvent::Handled {
                            input_ids,
                            ..
                        }) if input_ids == std::slice::from_ref(&failed_input_id)
                    ))
                    .count(),
                1,
                "failed explicit input should be durably handled once: {events:#?}"
            );
            assert!(
                !events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                        input_ids,
                        ..
                    }) if input_ids.contains(&failed_input_id)
                )),
                "a failed group cannot consume its input: {events:#?}"
            );
            assert!(
                !events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                        skill_loads,
                        ..
                    }) if !skill_loads.is_empty()
                )),
                "no subset of the failed group may be published: {events:#?}"
            );
            let surface = actor.chat_state_handle.get_conversation().await;
            let first_call_id = format!(
                "skill-{}",
                blake3::hash(format!("{failed_input_id}:0").as_bytes()).to_hex()
            );
            assert!(
                !surface.iter().any(|item| {
                    matches!(item, ConversationItem::SkillLoad(_))
                        || item.text_content().contains(BODY)
                }),
                "first skill body must not become model-visible: {surface:#?}"
            );
            assert!(
                !surface.iter().any(|item| matches!(
                    item,
                    ConversationItem::ToolResult(result)
                        if result.tool_call_id == first_call_id
                )),
                "the first preparation must not leave an orphan success result: {surface:#?}"
            );
            assert!(
                server.request_bodies().is_empty(),
                "the provider must not see a partial group"
            );

            actor
                .handle_completion(failed_prompt_id.into(), failure)
                .await;
            let next_prompt_id = "fifo-after-skill-group-failure";
            let next_input_id = admit_prompt(&actor, next_prompt_id, "ordinary follow-up").await;
            let next = run_prompt_text(&actor, next_prompt_id, next_input_id, "ordinary follow-up")
                .await
                .expect("next FIFO input continues after skill preparation failure");
            assert_eq!(next.stop_reason, acp::StopReason::EndTurn);
            let requests = server.request_bodies();
            assert_eq!(requests.len(), 1, "only the ordinary input is sampled");
            assert!(body_occurs(&requests[0], "ordinary follow-up"));
            assert!(!body_occurs(&requests[0], BODY));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn unconfirmed_user_skill_group_stops_before_provider_without_partial_publication() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for fault in [
                SkillCommitAckFault::FailPermanent,
                SkillCommitAckFault::PersistThenCancel,
            ] {
                let server = MockInferenceServer::start()
                    .await
                    .expect("mock inference server");
                let mut format = review_skill("/skills/format/SKILL.md", "FORMAT_GUIDANCE_BODY");
                format.name = "format".into();
                let (actor, fault_injections) = configured_actor_with_skills_and_commit_fault(
                    &server,
                    vec![
                        review_skill("/skills/review/SKILL.md", BODY),
                        format,
                    ],
                    None,
                    Some(fault),
                )
                .await;
                let prompt_id = match fault {
                    SkillCommitAckFault::FailPermanent => "skill-group-commit-failure",
                    SkillCommitAckFault::PersistThenCancel => "skill-group-ack-unknown-cancelled",
                    SkillCommitAckFault::None => unreachable!(),
                };
                let text = "/review inspect /format rust";
                let input_id = admit_prompt(&actor, prompt_id, text).await;
                let failure = run_prompt_text(&actor, prompt_id, input_id.clone(), text).await;
                let error = failure.as_ref().expect_err("commit failure or cancellation is fail-stop");
                assert_eq!(
                    fault_injections
                        .as_ref()
                        .expect("fault-enabled actor returns its injection probe")
                        .injected
                        .load(std::sync::atomic::Ordering::Acquire),
                    1,
                    "the persistence fault must hit the actual ChatState Timeline append"
                );
                assert!(
                    crate::session::commands::is_fatal_turn_boundary_error(error),
                    "fault={fault:?}; failure={failure:#?}"
                );
                let request_bodies = server.request_bodies();
                assert!(
                    request_bodies.is_empty(),
                    "provider cannot run before durable group acknowledgement; fault={fault:?}; requests={request_bodies:#?}; log={}",
                    server.request_log_summary()
                );

                let events = fault_injections
                    .as_ref()
                    .expect("fault-enabled actor returns its persistence probe")
                    .durable_events
                    .lock()
                    .expect("durable event log mutex poisoned")
                    .clone();
                let timeline = chat_state::Timeline::from_events(events.clone())
                    .expect("the durable prefix remains a valid Timeline");
                assert!(!events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Handled {
                        input_ids,
                        ..
                    }) if input_ids.contains(&input_id)
                )), "unknown ownership must not be settled as Handled: {events:#?}");
                for event in &events {
                    if let chat_state::TimelineEventKind::Input(
                        chat_state::InputEvent::Consumed {
                            input_ids,
                            skill_loads,
                            ..
                        },
                    ) = &event.kind
                        && input_ids.contains(&input_id)
                    {
                        assert_eq!(
                            skill_loads.len(),
                            2,
                            "an acknowledged input group is all-or-none: {events:#?}"
                        );
                    }
                }
                let surface = timeline.surface();
                let visible_load_count = surface
                    .iter()
                    .filter(|item| matches!(item, ConversationItem::SkillLoad(_)))
                    .count();
                assert!(
                    visible_load_count == 0 || visible_load_count == 2,
                    "the frozen pair cannot be partially visible: {surface:#?}"
                );
                for reference_index in 0..2 {
                    let call_id = format!(
                        "skill-{}",
                        blake3::hash(format!("{input_id}:{reference_index}").as_bytes()).to_hex()
                    );
                    assert_eq!(
                        events
                            .iter()
                            .filter(|event| matches!(
                                &event.kind,
                                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                                    cause: chat_state::HookCause::Tool { call_id: hooked_call },
                                    event: chat_state::HookEventType::PreToolUse,
                                    ..
                                }) if hooked_call == &call_id
                            ))
                            .count(),
                        1,
                        "each deterministic call reaches the hook once before fail-stop: {events:#?}"
                    );
                }
                if matches!(fault, SkillCommitAckFault::PersistThenCancel) {
                    let (persistence, _persistence_records) = chat_state::MockTimelinePersistence::new();
                    let (event_tx, _event_rx) =
                        tokio::sync::mpsc::unbounded_channel();
                    let cold = chat_state::ChatStateActor::spawn_from_timeline(
                        events.clone(),
                        sampling_types::SamplingConfig {
                            base_url: "http://localhost".into(),
                            model: "test".into(),
                            output_limit: None,
                            temperature: None,
                            top_p: None,
                            api_backend: Default::default(),
                            extra_headers: Default::default(),
                            query_params: Default::default(),
                            env_http_headers: Default::default(),
                            context_window: std::num::NonZeroU64::new(256_000).unwrap(),
                            reasoning_effort: None,
                            stream_tool_calls: None,
                        },
                        Box::new(persistence),
                        event_tx,
                        tokio_util::sync::CancellationToken::new(),
                    )
                    .await
                    .expect("cold restore must use the exact persisted-before-cancel prefix");
                    let cold_events = cold.timeline_events().await.unwrap();
                    assert_eq!(
                        cold_events
                            .iter()
                            .filter(|event| matches!(
                                &event.kind,
                                chat_state::TimelineEventKind::Input(
                                    chat_state::InputEvent::Consumed {
                                        input_ids,
                                        skill_loads,
                                        ..
                                    }
                                ) if input_ids.contains(&input_id) && skill_loads.len() == 2
                            ))
                            .count(),
                        1,
                        "recovery must not duplicate an unknown but persisted group"
                    );
                    for reference_index in 0..2 {
                        let call_id = format!(
                            "skill-{}",
                            blake3::hash(
                                format!("{input_id}:{reference_index}").as_bytes()
                            )
                            .to_hex()
                        );
                        assert_eq!(
                            cold_events
                                .iter()
                                .filter(|event| matches!(
                                    &event.kind,
                                    chat_state::TimelineEventKind::Hook(
                                        chat_state::HookEvent::Triggered {
                                            cause: chat_state::HookCause::Tool {
                                                call_id: hooked_call
                                            },
                                            event: chat_state::HookEventType::PreToolUse,
                                            ..
                                        }
                                    ) if hooked_call == &call_id
                                ))
                                .count(),
                            1,
                            "cold recovery must retain one hook record per deterministic call"
                        );
                    }
                    let cold_surface = cold.get_conversation().await;
                    assert_eq!(
                        cold_surface
                            .iter()
                            .filter(|item| matches!(item, ConversationItem::SkillLoad(_)))
                            .count(),
                        0,
                        "cold recovery settles committed guidance before closing the interrupted turn"
                    );
                    assert!(cold_surface.iter().all(|item| {
                        !item.text_content().contains(BODY)
                            && !item.text_content().contains("FORMAT_GUIDANCE_BODY")
                    }));
                    assert!(cold_events.iter().any(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Messages(chat_state::MessageEvent {
                            cause: chat_state::MessageCause::SkillLoadSettlement,
                            ..
                        })
                    )));
                } else {
                    assert!(!events.iter().any(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Consumed { input_ids, skill_loads, .. }
                        ) if input_ids.contains(&input_id) && !skill_loads.is_empty()
                    )), "a definite failure does not durably append the group: {events:#?}");
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn model_load_skill_pipeline_publishes_one_result_and_one_skill_item() {
    assert_model_skill_load_pipeline(false, true, true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn model_skill_resource_pipeline_commits_only_requested_literal_resource() {
    assert_model_skill_load_pipeline(true, true, true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn model_disabled_skill_pipeline_reports_failure_without_publishing_guidance() {
    assert_model_skill_load_pipeline(false, false, true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn model_skill_resource_permission_denial_publishes_no_guidance() {
    assert_model_skill_load_pipeline(true, true, false).await;
}

async fn assert_model_skill_load_pipeline(resource: bool, enabled: bool, permit_resource: bool) {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start()
                .await
                .expect("mock inference server");
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("SKILL.md");
            std::fs::write(&path, BODY).unwrap();
            let literal_resource = "---\nreference: true\n---\nRESOURCE_GUIDANCE_ONLY $ARGUMENTS";
            std::fs::write(dir.path().join("reference.md"), literal_resource).unwrap();
            let (arguments, expected_args, expected_body) = if resource {
                (r#"{"name":"review","resource":"reference.md"}"#, "", format!("<skill_resource name=\"review\" resource=\"reference.md\">\n{literal_resource}\n</skill_resource>"))
            } else {
                (r#"{"name":"review","args":"focus"}"#, "focus", prepared_body("focus"))
            };
            server.enqueue_response(
                "/v1/responses",
                ScriptedResponse::sse(
                    test_support::sse::responses_api_reasoning_then_tool_call_events(
                        "load review guidance",
                        "model-skill-call",
                        "load_skill",
                        arguments,
                        "test",
                    ),
                ),
            );
            server.enqueue_response(
                "/v1/responses",
                ScriptedResponse::json(
                    400,
                    serde_json::json!({
                        "type": "error",
                        "error": {"type": "invalid_request_error", "message": "stop after skill load"}
                    }),
                ),
            );
            let mut selected = review_skill(path.to_str().unwrap(), BODY);
            selected.enabled = enabled;
            let ((actor, _), _) = configured_actor_with_skills_commit_fault_and_hook_gate(
                &server, vec![selected], None, None, None,
                resource.then_some(if permit_resource { acp::PermissionOptionKind::AllowOnce } else { acp::PermissionOptionKind::RejectOnce }),
            ).await;
            let prompt_id = "model-skill-prompt";
            let input_id = actor
                .admit_human_input(
                    chat_state::InputIntent::Prompt,
                    prompt_payload(prompt_id, "review this"),
                    Some(prompt_id.into()),
                    chat_state::InputRoute::Fifo,
                    Vec::new(),
                )
                .await
                .expect("ordinary prompt is admitted before model sampling")
                .input_id;
            install_test_foreground(&actor, prompt_id).await;
            let result = actor
                .handle_prompt(
                    prompt_id,
                    vec![input_id],
                    crate::session::PromptOrigin::User,
                    Vec::new(),
                    crate::session::TurnKind::User,
                    vec![acp::ContentBlock::Text(acp::TextContent::new(
                        "review this",
                    ))],
                    tool_types::BehaviorId::Normal,
                    None,
                    None,
                    false,
                    None,
                    None,
                )
                .await;
            if resource && !permit_resource {
                assert!(result.is_ok(), "permission rejection ends the turn cleanly: {result:?}");
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert!(!events.iter().any(|event| matches!(&event.kind,
                    chat_state::TimelineEventKind::Messages(messages)
                        if messages.cause == chat_state::MessageCause::SkillLoadModel)));
                assert_eq!(server.request_bodies().len(), 1, "primary permission rejection must not resample");
                let surface = actor.chat_state_handle.get_conversation().await;
                assert!(surface.iter().any(|item| item.text_content().contains("rejected") || item.text_content().contains("denied")));
                assert!(surface.iter().all(|item| !item.text_content().contains(literal_resource)));
                assert!(format!("{events:?}").contains("Rejected"), "a durable permission rejection is required");
                actor.handle_completion(prompt_id.into(), result).await;
                return;
            }
            let error = match result.as_ref() {
                Err(error) => error,
                Ok(_) => {
                    let surface = actor.chat_state_handle.get_conversation().await;
                    panic!(
                        "expected scripted second request failure, got result={result:#?}; surface={surface:#?}"
                    );
                }
            };
            let failure_debug = format!("{error:?}");
            let surface_at_failure = actor.chat_state_handle.get_conversation().await;
            assert!(
                failure_debug.contains("stop after skill load"),
                "expected the scripted HTTP 400 error, got result={result:#?}; surface={surface_at_failure:#?}"
            );

            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            if !enabled || !permit_resource {
                assert!(!events.iter().any(|event| matches!(&event.kind,
                    chat_state::TimelineEventKind::Messages(messages)
                        if messages.cause == chat_state::MessageCause::SkillLoadModel)));
                let requests = server.request_bodies();
                assert_eq!(requests.len(), 2);
                let followup = requests[1].to_string();
                if !enabled {
                    assert!(followup.contains("disabled by configuration"), "{followup}");
                    assert!(followup.contains("do not bypass"), "{followup}");
                } else {
                    assert!(followup.contains("denied") || followup.contains("rejected"), "{followup}");
                    assert!(!followup.contains(literal_resource));
                }
                assert!(!followup.contains(BODY));
                actor.handle_completion(prompt_id.into(), result).await;
                return;
            }
            let published_events = events
                .iter()
                .filter_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Messages(messages)
                        if messages.cause == chat_state::MessageCause::SkillLoadModel =>
                    {
                        Some(&messages.items)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(published_events.len(), 1, "timeline={events:#?}");
            assert_eq!(published_events[0].len(), 2);
            assert!(matches!(
                &published_events[0][0],
                ConversationItem::ToolResult(result)
                    if result.tool_call_id == "model-skill-call"
            ));
            assert!(matches!(
                &published_events[0][1],
                ConversationItem::SkillLoad(skill)
                    if skill.call_id == "model-skill-call"
                        && skill.source == sampling_types::SkillLoadSource::Model
                        && skill.skill_name == "review"
                        && skill.args == expected_args
                        && skill.prepared_body.as_ref() == expected_body.as_str()
            ));
            let requests = server.request_bodies();
            assert_eq!(
                requests.len(),
                2,
                "a sampling failure before the follow-up request is not the test scenario"
            );
            assert!(
                requests
                    .get(1)
                    .is_some_and(|request| body_occurs(request, &expected_body)),
                "the follow-up request must include the published body: {requests:#?}"
            );

            // handle_prompt reaches the failed follow-up request, ends the
            // turn, and settles any guidance that was never consumed.
            let before_completion = actor.chat_state_handle.get_conversation().await;
            assert!(
                !before_completion
                    .iter()
                    .any(|item| matches!(item, ConversationItem::SkillLoad(_)))
            );
            assert_eq!(
                before_completion
                    .iter()
                    .filter_map(|item| match item {
                        ConversationItem::ToolResult(result)
                            if result.tool_call_id == "model-skill-call" =>
                        {
                            Some(result.content.as_ref())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                vec!["Loaded skill content was not consumed before execution ended"]
            );

            actor.handle_completion(prompt_id.into(), result).await;
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(
                !conversation
                    .iter()
                    .any(|item| matches!(item, ConversationItem::SkillLoad(_)))
            );
            assert_eq!(
                conversation
                    .iter()
                    .filter_map(|item| match item {
                        ConversationItem::ToolResult(result)
                            if result.tool_call_id == "model-skill-call" =>
                        {
                            Some(result.content.as_ref())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                vec!["Loaded skill content was not consumed before execution ended"]
            );
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Messages(messages)
                            if messages.cause == chat_state::MessageCause::SkillLoadModel
                    ))
                    .count(),
                1
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Messages(messages)
                            if messages.cause == chat_state::MessageCause::SkillLoadSettlement
                    ))
                    .count(),
                1,
                "completion must not settle the same loaded skill twice"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replayed_user_skill_input_does_not_switch_source_or_leave_result() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start()
                .await
                .expect("mock inference server");
            let actor = configured_actor(
                &server,
                review_skill("/skills/review-old/SKILL.md", "OLD BODY"),
            )
            .await;
            let prompt_id = "replayed-skill-prompt";
            let original_payload = prompt_payload(prompt_id, "/review inspect");
            let input_id = actor
                .admit_human_input(
                    chat_state::InputIntent::Prompt,
                    original_payload.clone(),
                    Some(prompt_id.into()),
                    chat_state::InputRoute::Fifo,
                    Vec::new(),
                )
                .await
                .expect("first input admission")
                .input_id;

            actor
                .agent
                .borrow()
                .tool_bridge()
                .update_skill_baseline(vec![review_skill(
                    "/skills/review-new/SKILL.md",
                    "NEW BODY",
                )])
                .await;
            let replay = actor
                .admit_human_input(
                    chat_state::InputIntent::Prompt,
                    original_payload,
                    Some(prompt_id.into()),
                    chat_state::InputRoute::Fifo,
                    Vec::new(),
                )
                .await
                .expect("same prompt identity is idempotent");
            assert!(replay.replayed);
            assert_eq!(replay.input_id, input_id);

            let result = run_prompt(&actor, prompt_id, input_id.clone()).await;
            assert!(
                result.is_err(),
                "changed source must fail closed: {result:?}"
            );
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let submitted_payload = events
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted {
                        input_id: id,
                        payload_ref,
                        ..
                    }) if id == &input_id => Some(payload_ref),
                    _ => None,
                })
                .expect("original input remains durable");
            let payload = crate::session::input_inbox::read_payload(
                &actor.session_directory,
                submitted_payload,
            )
            .unwrap();
            let InputPayload::Prompt {
                skill_invocations, ..
            } = payload;
            assert_eq!(skill_invocations.len(), 1);
            assert_eq!(
                skill_invocations[0].skill.as_ref().unwrap().path,
                "/skills/review-old/SKILL.md"
            );
            let surface = actor.chat_state_handle.get_conversation().await;
            assert!(
                !surface
                    .iter()
                    .any(|item| matches!(item, ConversationItem::ToolResult(_)))
            );
            assert!(
                !surface
                    .iter()
                    .any(|item| matches!(item, ConversationItem::SkillLoad(_)))
            );
            assert!(
                !surface
                    .iter()
                    .any(|item| item.text_content().contains("OLD BODY")
                        || item.text_content().contains("NEW BODY"))
            );
        })
        .await;
}

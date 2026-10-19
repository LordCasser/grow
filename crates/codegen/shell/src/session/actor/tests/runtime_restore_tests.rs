use super::*;
use chat_state::{TimelineEventKind as K, session_runtime::*};

#[test]
fn accepted_final_response_resumes_original_turn_without_provider_or_prompt_replay() {
    std::thread::Builder::new().stack_size(48 * 1024 * 1024).spawn(|| {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(
            tokio::task::LocalSet::new().run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, mut persistence_rx) =
                    tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
                tokio::task::spawn_local(async move {
                    while persistence_rx.recv().await.is_some() {}
                });
                let (mut actor, _session_events) = support::create_test_actor_ex_with_projection_error(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                    None,
                )
                .await;
                let (workflow_manager, workflow_tracker, _subagent_events) =
                    crate::session::workflow::manager::WorkflowManager::test_bundle_with_timeline(
                        actor.chat_state_handle.clone(),
                        actor.session_id_string(),
                        actor.session_directory.clone(),
                    );
                let workflow_script = r#"let meta = #{ name: "restore-test", description: "d" };
complete("restored workflow");"#;
                workflow_manager
                    .lock()
                    .await
                    .test_seed_workflow_run("wf_restore_gate", workflow_script)
                    .await;
                actor.workflow_manager = workflow_manager;
                let actor = std::sync::Arc::new(actor);
                let turn = chat_state::TurnId(41);
                let step = chat_state::StepId { turn, index: 0 };
                for kind in [
                    K::Turn(chat_state::TurnEvent::Started {
                        prompt_id: "original-prompt".into(),
                        id: turn, input_ids: vec![], identity: chat_state::TurnIdentity { origin: "user".into(), turn_kind: "internal".into(), goal_id: None, goal_definition_revision: None, stage_id: None },
                        model_id: "test".into(), input_message_count: 1, prompt_index: 0, prompt_text: "original task".into(), input_kind: chat_state::TurnInputKind::Prompt, redirect_kind: None,
                    }),
                    K::Step(chat_state::StepEvent::Started { id: step }),
                    K::Request(chat_state::RequestEvent::Started { id: "original-request".into(), turn, step, model_id: "test".into(), input_message_count: 1, tool_count: 0 }),
                    K::Request(chat_state::RequestEvent::Completed { id: "original-request".into(), duration_ms: 1, time_to_first_token_ms: None, usage: chat_state::RequestUsage::default(), response_message_count: 1, attempt: 1, provider_terminal: Some(sampling_types::ProviderTerminal::ChatCompletions { finish_reason: "stop".into() }) }),
                ] { actor.chat_state_handle.record_timeline_event_durably(kind).await.unwrap(); }
                let identity = chat_state::ResponseAdmissionIdentity { request_id: "original-request".into(), attempt: 1 };
                actor.chat_state_handle.push_response_durably_with_identity(vec![ConversationItem::assistant("existing final answer")], identity.clone(), None).await.unwrap();
                for hook in [
                    chat_state::HookEvent::Triggered {
                        occurrence_id: "restored-stop".into(),
                        event: chat_state::HookEventType::Stop,
                        gate: chat_state::HookGateKind::Stop,
                        cause: chat_state::HookCause::Turn { turn },
                        config_generation: 1,
                        handlers: vec![],
                    },
                    chat_state::HookEvent::Completed {
                        occurrence_id: "restored-stop".into(),
                        decision: chat_state::HookAggregateDecision::Stop {
                            decision: chat_state::HookStopDecision::AllowStop,
                        },
                    },
                ] {
                    actor.chat_state_handle.record_timeline_event_durably(K::Hook(hook)).await.unwrap();
                }
                actor.state.lock().await.runtime_suspend.store(true, std::sync::atomic::Ordering::Release);
                actor.chat_state_handle.record_timeline_event_durably(K::Runtime(SessionRuntimeEvent::SuspendRequested { operation_id: "op".into(), root_session_id: actor.session_id_string() })).await.unwrap();
                actor
                    .workflow_manager
                    .lock()
                    .await
                    .test_suspend_seeded_run("wf_restore_gate")
                    .await;
                actor.chat_state_handle.record_timeline_event_durably(K::Runtime(SessionRuntimeEvent::CommandsSettled { operation_id: "op".into(), commands: vec![] })).await.unwrap();
                actor.chat_state_handle.checkpoint_runtime("op".into(), actor.runtime_source_hash().await.unwrap(), Some(TurnContinuation {
                    turn_id: turn, prompt_id: "original-prompt".into(), prompt_index: 0, origin: serde_json::to_string(&PromptOrigin::User).unwrap(), next_step_index: 1,
                    tool_turns_used: 1, tool_calls_used: 0, stop_continuations_used: 0, output_schema: None, resume_point: chat_state::session_runtime::TurnResumePoint::Model, task_output_usage: None,
                    accepted_response: Some(AcceptedResponseContinuation { identity, stop_reason: Some(sampling_types::StopReason::Stop), stop_message: None }), structured_output_retries: 0,
                    sampling_attempts_used: 0, sampling_max_attempts: None, sampling_replay_safe: true, sampling_deadline_ms: None, operations: vec![],
                }), Some(step), tokio::time::Instant::now() + std::time::Duration::from_secs(5)).await.unwrap();
                let (completed, mut completion) = tokio::sync::mpsc::unbounded_channel::<(
                    String,
                    crate::session::commands::PromptTurnResult,
                )>();
                actor.prepare_runtime_restore("op", "instance", completed).await.unwrap();
                assert!(completion.try_recv().is_err());
                assert!(actor.state.lock().await.runtime_suspend.load(std::sync::atomic::Ordering::Acquire));
                actor.commit_runtime_resume("op".into(), "instance".into()).await.unwrap();
                assert!(completion.try_recv().is_err());
                workflow_tracker.lock().set_definition_provenance(
                    "wf_restore_gate",
                    tools::implementations::grow_build::workflow::WorkflowDefinitionId::new(
                        "session:restore-test",
                    ),
                    tools::implementations::grow_build::workflow::WorkflowScope::Session,
                    "0".repeat(64),
                );
                let workflow_preflight_error = actor
                    .release_runtime_restore("instance")
                    .await
                    .expect_err("a source drift must keep the original Turn behind the Workflow gate");
                assert!(workflow_preflight_error.contains("source hash"), "{workflow_preflight_error}");
                assert!(actor.state.lock().await.runtime_suspend.load(std::sync::atomic::Ordering::Acquire));
                assert!(actor.restoration_release.borrow().is_some());
                assert!(completion.try_recv().is_err());
                let suspended_workflow = workflow_tracker.lock().get("wf_restore_gate").unwrap();
                assert!(suspended_workflow.execution_suspended);
                assert_eq!(suspended_workflow.execution_epoch, 0);

                workflow_tracker.lock().set_definition_provenance(
                    "wf_restore_gate",
                    tools::implementations::grow_build::workflow::WorkflowDefinitionId::new(
                        "session:restore-test",
                    ),
                    tools::implementations::grow_build::workflow::WorkflowScope::Session,
                    crate::session::workflow::registry::content_hash(workflow_script),
                );
                actor.release_runtime_restore("instance").await.unwrap();
                let (prompt_id, result) = tokio::time::timeout(std::time::Duration::from_secs(5), completion.recv()).await.unwrap().unwrap();
                assert_eq!(prompt_id, "original-prompt");
                assert!(result.is_ok(), "{result:?}");
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(events.iter().filter(|event| matches!(event.kind, K::Hook(chat_state::HookEvent::Triggered { event: chat_state::HookEventType::Stop, .. }))).count(), 1, "restoration must reuse the exact completed Stop decision rather than run it again");
                assert_eq!(events.iter().filter(|event| matches!(event.kind, K::Turn(chat_state::TurnEvent::Started { .. }))).count(), 1);
                assert_eq!(events.iter().filter(|event| matches!(event.kind, K::Request(chat_state::RequestEvent::Started { .. }))).count(), 1);
                assert_eq!(events.iter().filter(|event| matches!(event.kind, K::Turn(chat_state::TurnEvent::Ended { id, .. }) if id == turn)).count(), 1);
                assert_eq!(actor.chat_state_handle.get_conversation().await.iter().filter(|item| matches!(item, ConversationItem::Assistant(_))).count(), 1);
                assert_eq!(events.iter().filter(|event| matches!(&event.kind, K::Workflow(chat_state::WorkflowEvent::Resumed { run_id, .. }) if run_id == "wf_restore_gate")).count(), 1);
                let workflow_finished = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    loop {
                        if workflow_tracker
                            .lock()
                            .get("wf_restore_gate")
                            .is_some_and(|state| state.status == crate::session::workflow::tracker::WorkflowRunStatus::Complete)
                        {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                }).await;
                if workflow_finished.is_err() {
                    let current = workflow_tracker.lock().get("wf_restore_gate");
                    let workflow_facts = actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .into_iter()
                        .filter_map(|event| match event.kind {
                            K::Workflow(workflow) => Some(workflow),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    panic!(
                        "same restored Workflow Run did not finish; state={current:?}; Workflow facts={workflow_facts:?}"
                    );
                }
                assert_eq!(workflow_tracker.lock().list().len(), 1);
            })
        );
    }).unwrap().join().unwrap();
}

#[test]
fn suspended_workflow_rebinds_the_same_open_child_before_original_turn_releases() {
    std::thread::Builder::new().stack_size(48 * 1024 * 1024).spawn(|| {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(
            tokio::task::LocalSet::new().run_until(async {
                use crate::session::workflow::{
                    manager::{LaunchSpec, WorkflowManager},
                    registry::resolve_inline,
                };
                use tools::implementations::grow_build::task::types::{
                    SubagentEvent, SubagentSnapshot, SubagentSnapshotStatus,
                };

                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, mut persistence_rx) =
                    tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
                tokio::task::spawn_local(async move {
                    while persistence_rx.recv().await.is_some() {}
                });
                let (mut actor, _session_events) = support::create_test_actor_ex_with_projection_error(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                    None,
                )
                .await;
                let (workflow_manager, workflow_tracker, mut subagent_events) =
                    WorkflowManager::test_bundle_with_timeline(
                        actor.chat_state_handle.clone(),
                        actor.session_id_string(),
                        actor.session_directory.clone(),
                    );
                actor.workflow_manager = workflow_manager;
                let actor = std::sync::Arc::new(actor);

                let script = r#"let meta = #{ name: "restore-child", description: "d" };
let child = agent("wait for the existing child");
complete(child.output);"#;
                let resolved = resolve_inline(script.to_owned()).unwrap();
                let (run_id, _initial_outcome) = actor
                    .workflow_manager
                    .lock()
                    .await
                    .launch(
                        resolved,
                        LaunchSpec {
                            objective: "restore the same child wait".into(),
                            args: serde_json::json!({"fixed": true}),
                            agent_budget: None,
                            max_concurrency: None,
                            resume_run_id: None,
                        },
                    )
                    .await
                    .unwrap();

                let initial_spawn = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    subagent_events.recv(),
                )
                .await
                .unwrap()
                .expect("workflow must dispatch its awaited child");
                let SubagentEvent::Spawn(spawn) = initial_spawn else {
                    panic!("expected initial child Spawn event");
                };
                let child_id = spawn.id.clone();
                let child_request = &spawn.request;
                actor
                    .chat_state_handle
                    .record_timeline_event_durably(K::Subagent(
                        chat_state::SubagentEvent::Spawned(chat_state::SubagentSpawnEvent {
                            subagent_id: child_id.clone(),
                            child_session_id: child_id.clone(),
                            origin_tool_call_id: child_request.origin_tool_call_id.clone(),
                            security_parent_session_id: child_request.parent_session_id.clone(),
                            parent_step_id: child_request.parent_step_id.clone(),
                            subagent_type: child_request.subagent_type.clone(),
                            description: child_request.description.clone(),
                            prompt: child_request.prompt.clone(),
                            execution_recipe: child_request.execution_recipe(),
                            context_source: chat_state::SubagentContextSource::New,
                            source_ref: None,
                            context_normalized: false,
                            resumed_from: child_request.resume_from.clone(),
                            parent_prompt_id: child_request.parent_prompt_id.clone(),
                            capability_mode: None,
                            permission_mode: None,
                            effective_permission_mode: None,
                            workflow_run_id: child_request.owner.workflow_run_id().map(str::to_owned),
                            goal_id: child_request.owner.goal_id().map(str::to_owned),
                            goal_definition_revision: child_request
                                .owner
                                .goal_definition_revision(),
                            surface_completion: child_request.surface_completion,
                            child_cwd: child_request
                                .cwd
                                .clone()
                                .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned()),
                            worktree_path: None,
                            effective_model_id: child_request
                                .runtime_overrides
                                .model
                                .clone()
                                .unwrap_or_else(|| "test-model".into()),
                            model_transport_key: child_request
                                .runtime_overrides
                                .model_transport_key
                                .clone()
                                .unwrap_or_else(|| {
                                    sampling_types::ModelImageInputKey::new(
                                        "test-model",
                                        "responses",
                                        "test-endpoint",
                                    )
                                }),
                            reasoning_effort: None,
                        }),
                    ))
                    .await
                    .unwrap();

                let turn = chat_state::TurnId(52);
                let step = chat_state::StepId { turn, index: 0 };
                for kind in [
                    K::Turn(chat_state::TurnEvent::Started {
                        prompt_id: "original-child-prompt".into(),
                        id: turn,
                        input_ids: vec![],
                        identity: chat_state::TurnIdentity {
                            origin: "user".into(),
                            turn_kind: "internal".into(),
                            goal_id: None,
                            goal_definition_revision: None,
                            stage_id: None,
                        },
                        model_id: "test".into(),
                        input_message_count: 1,
                        prompt_index: 0,
                        prompt_text: "original task".into(),
                        input_kind: chat_state::TurnInputKind::Prompt,
                        redirect_kind: None,
                    }),
                    K::Step(chat_state::StepEvent::Started { id: step }),
                    K::Request(chat_state::RequestEvent::Started {
                        id: "original-child-request".into(),
                        turn,
                        step,
                        model_id: "test".into(),
                        input_message_count: 1,
                        tool_count: 0,
                    }),
                    K::Request(chat_state::RequestEvent::Completed {
                        id: "original-child-request".into(),
                        duration_ms: 1,
                        time_to_first_token_ms: None,
                        usage: chat_state::RequestUsage::default(),
                        response_message_count: 1,
                        attempt: 1,
                        provider_terminal: Some(sampling_types::ProviderTerminal::ChatCompletions {
                            finish_reason: "stop".into(),
                        }),
                    }),
                ] {
                    actor
                        .chat_state_handle
                        .record_timeline_event_durably(kind)
                        .await
                        .unwrap();
                }
                let identity = chat_state::ResponseAdmissionIdentity {
                    request_id: "original-child-request".into(),
                    attempt: 1,
                };
                actor
                    .chat_state_handle
                    .push_response_durably_with_identity(
                        vec![ConversationItem::assistant("accepted final")],
                        identity.clone(),
                        None,
                    )
                    .await
                    .unwrap();
                for hook in [
                    chat_state::HookEvent::Triggered {
                        occurrence_id: "workflow-child-stop".into(),
                        event: chat_state::HookEventType::Stop,
                        gate: chat_state::HookGateKind::Stop,
                        cause: chat_state::HookCause::Turn { turn },
                        config_generation: 1,
                        handlers: vec![],
                    },
                    chat_state::HookEvent::Completed {
                        occurrence_id: "workflow-child-stop".into(),
                        decision: chat_state::HookAggregateDecision::Stop {
                            decision: chat_state::HookStopDecision::AllowStop,
                        },
                    },
                ] {
                    actor
                        .chat_state_handle
                        .record_timeline_event_durably(K::Hook(hook))
                        .await
                        .unwrap();
                }

                actor.state.lock().await.runtime_suspend.store(
                    true,
                    std::sync::atomic::Ordering::Release,
                );
                actor
                    .chat_state_handle
                    .record_timeline_event_durably(K::Runtime(
                        SessionRuntimeEvent::SuspendRequested {
                            operation_id: "workflow-child-op".into(),
                            root_session_id: actor.session_id_string(),
                        },
                    ))
                    .await
                    .unwrap();
                actor
                    .workflow_manager
                    .lock()
                    .await
                    .suspend_all_and_drain(
                        tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                    )
                    .await
                    .unwrap();
                assert!(workflow_tracker
                    .lock()
                    .get(&run_id)
                    .is_some_and(|state| state.execution_suspended));

                actor
                    .chat_state_handle
                    .record_timeline_event_durably(K::Runtime(
                        SessionRuntimeEvent::CommandsSettled {
                            operation_id: "workflow-child-op".into(),
                            commands: vec![],
                        },
                    ))
                    .await
                    .unwrap();
                actor
                    .chat_state_handle
                    .checkpoint_runtime(
                        "workflow-child-op".into(),
                        actor.runtime_source_hash().await.unwrap(),
                        Some(TurnContinuation {
                            turn_id: turn,
                            prompt_id: "original-child-prompt".into(),
                            prompt_index: 0,
                            origin: serde_json::to_string(&PromptOrigin::User).unwrap(),
                            next_step_index: 1,
                            tool_turns_used: 1,
                            tool_calls_used: 0,
                            stop_continuations_used: 0,
                            output_schema: None,
                            resume_point: chat_state::session_runtime::TurnResumePoint::Model,
                            task_output_usage: None,
                            accepted_response: Some(AcceptedResponseContinuation {
                                identity,
                                stop_reason: Some(sampling_types::StopReason::Stop),
                                stop_message: None,
                            }),
                            structured_output_retries: 0,
                            sampling_attempts_used: 0,
                            sampling_max_attempts: None,
                            sampling_replay_safe: true,
                            sampling_deadline_ms: None,
                            operations: vec![],
                        }),
                        Some(step),
                        tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                    )
                    .await
                    .unwrap();
                let (completed, mut completion) = tokio::sync::mpsc::unbounded_channel::<(
                    String,
                    crate::session::commands::PromptTurnResult,
                )>();
                actor
                    .prepare_runtime_restore("workflow-child-op", "workflow-child-instance", completed)
                    .await
                    .unwrap();
                actor
                    .commit_runtime_resume(
                        "workflow-child-op".into(),
                        "workflow-child-instance".into(),
                    )
                    .await
                    .unwrap();
                assert!(completion.try_recv().is_err(), "the original Turn stays gated before Workflow restoration");

                actor.release_runtime_restore("workflow-child-instance").await.unwrap();
                let query = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    subagent_events.recv(),
                )
                .await
                .unwrap()
                .expect("restored Workflow must query its original child");
                let SubagentEvent::Query(query) = query else {
                    panic!("restore must query the existing child without dispatching again");
                };
                assert_eq!(query.subagent_id, child_id, "journal operation must retain child identity");
                query
                    .respond_to
                    .send(Ok(Some(SubagentSnapshot {
                        subagent_id: child_id.clone(),
                        description: "existing workflow child".into(),
                        subagent_type: "general-purpose".into(),
                        status: SubagentSnapshotStatus::Running {
                            turn_count: 1,
                            tool_call_count: 0,
                            tokens_used: 0,
                            context_window_tokens: 1000,
                            context_usage_pct: 0,
                            tools_used: vec![],
                            error_count: 0,
                        },
                        started_at_epoch_ms: 1,
                        duration_ms: 1,
                    })))
                    .unwrap();
                let (prompt_id, result) = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    completion.recv(),
                )
                .await
                .expect("the original Turn must resume after Workflow owner installation")
                .expect("the original Turn completion route must remain open");
                assert_eq!(prompt_id, "original-child-prompt");
                assert!(result.is_ok(), "{result:?}");

                let second_query = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    subagent_events.recv(),
                )
                .await
                .unwrap()
                .expect("an unresolved child remains awaited after rebinding");
                let SubagentEvent::Query(second_query) = second_query else {
                    panic!("restored run must not dispatch a replacement child");
                };
                assert_eq!(second_query.subagent_id, child_id);
                // The controlled coordinator still owns an open child. Retire
                // this test's physical Workflow execution by suspending it;
                // cancellation cannot claim a terminal child result here.
                actor.state.lock().await.runtime_suspend.store(
                    true,
                    std::sync::atomic::Ordering::Release,
                );
                actor
                    .chat_state_handle
                    .record_timeline_event_durably(K::Runtime(
                        SessionRuntimeEvent::SuspendRequested {
                            operation_id: "workflow-child-cleanup".into(),
                            root_session_id: actor.session_id_string(),
                        },
                    ))
                    .await
                    .expect("Workflow cleanup must belong to a real session suspension");
                actor
                    .workflow_manager
                    .lock()
                    .await
                    .suspend_all_and_drain(
                        tokio::time::Instant::now() + std::time::Duration::from_secs(2),
                    )
                    .await
                    .expect("retire the restored Workflow without terminating its original child");
                assert!(workflow_tracker
                    .lock()
                    .get(&run_id)
                    .is_some_and(|state| state.execution_suspended));
                drop(second_query);
                drop(spawn);
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(events.iter().filter(|event| matches!(&event.kind, K::Subagent(chat_state::SubagentEvent::Spawned(spawn)) if spawn.subagent_id == child_id)).count(), 1);
                assert_eq!(events.iter().filter(|event| matches!(&event.kind, K::Workflow(chat_state::WorkflowEvent::Resumed { run_id: resumed, .. }) if resumed == &run_id)).count(), 1);
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(&event.kind, K::Turn(chat_state::TurnEvent::Started { .. })))
                        .count(),
                    1
                );
            })
        );
    }).unwrap().join().unwrap();
}

use super::support::*;
use super::*;

fn install_client_hook(
    actor: &SessionActor,
    event: ::hooks::event::HookEventName,
    callback_ids: &[&str],
) {
    let mut client_hooks = crate::extensions::hooks::ClientHooks::new();
    client_hooks.insert(
        event,
        vec![crate::extensions::hooks::ClientHookGroup {
            matcher: None,
            callback_ids: callback_ids.iter().map(|s| s.to_string()).collect(),
            timeout: None,
        }],
    );
    *actor.hooks.client_hooks.borrow_mut() = client_hooks;
}

/// Acks UI notifications so `deny_tool` cannot block the gate.
fn spawn_deny_responder(
    gateway_rx: tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
    reason: &'static str,
) {
    let mut gateway_rx = gateway_rx;
    tokio::task::spawn_local(async move {
        while let Some(msg) = gateway_rx.recv().await {
            match msg {
                acp_transport::AcpClientMessage::ExtMethod(args) => {
                    let deny: Arc<serde_json::value::RawValue> =
                        serde_json::value::to_raw_value(&serde_json::json!({
                            "decision": "deny",
                            "systemMessage": reason,
                        }))
                        .unwrap()
                        .into();
                    let _ = args.response_tx.send(Ok(acp::ExtResponse::new(deny)));
                }
                acp_transport::AcpClientMessage::SessionNotification(args) => {
                    let _ = args.response_tx.send(Ok(()));
                }
                _ => {}
            }
        }
    });
}

/// Client hooks fire even with no on-disk hook registry: `notify_client_hooks`
/// reads `client_hooks` (never `hook_registry`) and its call sites sit outside the
/// file-registry guard.
#[tokio::test(flavor = "current_thread")]
async fn client_hooks_fire_without_file_registry() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, mut gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;

            assert!(
                actor.hooks.registry.borrow().is_none(),
                "fixture must have no file registry for this invariant"
            );
            install_client_hook(&actor, ::hooks::event::HookEventName::Stop, &["cb_0"]);

            actor.fire_hook(
                ::hooks::event::HookEventName::Stop,
                None,
                ::hooks::event::HookPayload::Stop {
                    reason: "end_turn".to_string(),
                    stop_hook_active: false,
                    last_assistant_message: None,
                    background_tasks: None,
                    session_crons: None,
                },
            );

            let msg = gateway_rx
                .try_recv()
                .expect("client hook must fire with no file registry");
            let acp_transport::AcpClientMessage::ExtNotification(args) = msg else {
                panic!("expected an grow/hooks/event ext notification");
            };
            assert_eq!(args.request.method.as_ref(), "grow/hooks/event");
            let params: serde_json::Value =
                serde_json::from_str(args.request.params.get()).unwrap();
            assert_eq!(params["hookCallbackId"], "cb_0");
            assert_eq!(params["hookEventName"], "stop");
        })
        .await;
}

/// A `use_tool` call whose wire `function.name` is the dispatcher surfaces to PreToolUse
/// hooks as its resolved target, so a matcher keyed on the qualified MCP name
/// (`linear__save_issue`) gates the dispatch. Drives the real `prepare_tool_call` path;
/// the deny fires only if the resolved name reached the envelope.
#[tokio::test(flavor = "current_thread")]
async fn pre_tool_use_resolves_meta_dispatch_tool_name_end_to_end() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            // The toolset must know `use_tool` so it parses to `ToolInput::UseTool`.
            *actor.agent.borrow_mut() =
                test_agent_with_tools(vec![tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::use_tool::UseTool,
                >()])
                .await;

            let mut client_hooks = crate::extensions::hooks::ClientHooks::new();
            client_hooks.insert(
                ::hooks::event::HookEventName::PreToolUse,
                vec![crate::extensions::hooks::ClientHookGroup {
                    matcher: Some(
                        ::hooks::matcher::HookMatcher::new("linear__save_issue").unwrap(),
                    ),
                    callback_ids: vec!["cb_0".to_string()],
                    timeout: None,
                }],
            );
            *actor.hooks.client_hooks.borrow_mut() = client_hooks;
            spawn_deny_responder(gateway_rx, "nope");

            begin_test_causal_turn(&actor).await;

            let call = ToolCallResponse {
                id: "call_1".to_string(),
                kind: "function".to_string(),
                function: crate::sampling::types::ToolCallFunction::new(
                    "use_tool",
                    r#"{"tool_name":"linear__save_issue","tool_input":{}}"#,
                ),
            };
            actor
                .events
                .tool_started(
                    call.function.name.clone(),
                    call.id.clone(),
                    serde_json::from_str(&call.function.arguments).ok(),
                )
                .await
                .expect("the direct preflight test must durably open its causal tool");

            let mut deferred = Vec::new();
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.prepare_tool_call(call, &mut deferred),
            )
            .await
            .expect("prepare_tool_call must not hang")
            .expect("prepare_tool_call must not error");
            assert!(
                matches!(
                    result,
                    ToolPreflight::Resolved {
                        loop_result: ToolLoop::HookDenied { .. },
                        ..
                    }
                ),
                "a hook matched on the resolved tool must gate the use_tool dispatch; \
                 got {result:?}"
            );
        })
        .await;
}

/// Reproduces the prod inheritance seam (subagent.rs `ctx.client_hooks.clone()`) by
/// cloning the parent's hooks into a child `SessionActor`, so the subagent call hits
/// the parent's PreToolUse gate carrying the `subagentType`.
#[tokio::test(flavor = "current_thread")]
async fn subagent_inherits_parent_pre_tool_use_client_hook() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (parent_gateway_tx, _parent_gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (parent_persistence_tx, _parent_persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let parent =
                create_test_actor(0, 256_000, 85, parent_gateway_tx, parent_persistence_tx).await;

            install_client_hook(
                &parent,
                ::hooks::event::HookEventName::PreToolUse,
                &["cb_0"],
            );

            let (child_gateway_tx, mut child_gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (child_persistence_tx, _child_persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let subagent =
                create_test_actor(0, 256_000, 85, child_gateway_tx, child_persistence_tx).await;

            assert!(
                subagent.hooks.client_hooks.borrow().is_empty(),
                "the subagent starts with no hooks of its own"
            );
            *subagent.hooks.client_hooks.borrow_mut() = parent.hooks.client_hooks.borrow().clone();

            let seen_subagent_type = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
            let seen = seen_subagent_type.clone();
            tokio::task::spawn_local(async move {
                while let Some(msg) = child_gateway_rx.recv().await {
                    match msg {
                        acp_transport::AcpClientMessage::ExtMethod(args) => {
                            let params: serde_json::Value =
                                serde_json::from_str(args.request.params.get()).unwrap();
                            *seen.lock().unwrap() =
                                params["subagentType"].as_str().map(str::to_string);
                            let deny: Arc<serde_json::value::RawValue> =
                                serde_json::value::to_raw_value(&serde_json::json!({
                                    "decision": "deny",
                                }))
                                .unwrap()
                                .into();
                            let _ = args.response_tx.send(Ok(acp::ExtResponse::new(deny)));
                        }
                        acp_transport::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });

            let call = ToolCallResponse {
                id: "call_1".to_string(),
                kind: "function".to_string(),
                function: crate::sampling::types::ToolCallFunction::new(
                    "run_terminal_command",
                    "{}",
                ),
            };
            let tool_call_id = acp::ToolCallId::new("call_1");
            let envelope = subagent.make_hook_envelope(
                ::hooks::event::HookEventName::PreToolUse,
                None,
                ::hooks::event::HookPayload::PreToolUse {
                    tool_name: call.function.name.clone(),
                    tool_use_id: call.id.clone(),
                    tool_input: serde_json::json!({}),
                    tool_input_truncated: false,
                    subagent_type: Some("code-reviewer".to_string()),
                },
            );

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                subagent.run_pre_tool_use_client_hook(&call, &tool_call_id, &envelope),
            )
            .await
            .expect("the gate must not hang")
            .expect("the gate must not error");

            assert!(
                matches!(result, Some(ToolLoop::HookDenied { .. })),
                "a subagent tool call must be blocked by the parent's inherited PreToolUse hook"
            );
            assert_eq!(
                seen_subagent_type.lock().unwrap().as_deref(),
                Some("code-reviewer"),
                "the parent's hook must observe the subagent's type on the dispatch"
            );
        })
        .await;
}

/// A slow/hung callback must not starve a later deny: with the first-registered callback
/// never replying and the second denying, the durable production dispatcher resolves
/// the Tool gate quickly (a sequential gate would block on the hung one's full timeout).
#[tokio::test(flavor = "current_thread")]
async fn pre_tool_use_slow_callback_does_not_starve_a_deny() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, mut gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;

            install_client_hook(
                &actor,
                ::hooks::event::HookEventName::PreToolUse,
                &["slow_cb", "deny_cb"],
            );
            begin_test_causal_turn(&actor).await;

            tokio::task::spawn_local(async move {
                let mut held = Vec::new();
                while let Some(msg) = gateway_rx.recv().await {
                    match msg {
                        acp_transport::AcpClientMessage::ExtMethod(args) => {
                            let params: serde_json::Value =
                                serde_json::from_str(args.request.params.get()).unwrap();
                            if params["hookCallbackId"] == "deny_cb" {
                                let deny: Arc<serde_json::value::RawValue> =
                                    serde_json::value::to_raw_value(&serde_json::json!({
                                        "decision": "deny",
                                    }))
                                    .unwrap()
                                    .into();
                                let _ = args.response_tx.send(Ok(acp::ExtResponse::new(deny)));
                            } else {
                                held.push(args.response_tx);
                            }
                        }
                        acp_transport::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });

            let call = ToolCallResponse {
                id: "call_1".to_string(),
                kind: "function".to_string(),
                function: crate::sampling::types::ToolCallFunction::new(
                    "run_terminal_command",
                    "{}",
                ),
            };
            actor
                .events
                .tool_started(
                    call.function.name.clone(),
                    call.id.clone(),
                    serde_json::from_str(&call.function.arguments).ok(),
                )
                .await
                .expect("the production hook dispatcher requires an open causal tool");
            let envelope = actor.make_hook_envelope(
                ::hooks::event::HookEventName::PreToolUse,
                None,
                ::hooks::event::HookPayload::PreToolUse {
                    tool_name: call.function.name.clone(),
                    tool_use_id: call.id.clone(),
                    tool_input: serde_json::json!({}),
                    tool_input_truncated: false,
                    subagent_type: None,
                },
            );

            // 5s ceiling is well under the hung callback's 30s per-callback timeout, so a
            // pass proves the deny was not serialized behind the slow callback.
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.dispatch_hook_occurrence(
                    ::hooks::event::HookEventName::PreToolUse,
                    chat_state::HookCause::Tool {
                        call_id: call.id.clone(),
                    },
                    envelope,
                    ::hooks::event::GateKind::Tool,
                ),
            )
            .await
            .expect("a deny must resolve without waiting on the hung callback")
            .expect("the hook lifecycle must remain durable");
            assert!(matches!(
                result.into_tool_decision(),
                ::hooks::result::HookDecision::Deny { .. }
            ));
        })
        .await;
}

/// PostToolUse and PostToolUseFailure must never both fire for one tool call: a hard
/// dispatch error fires only PostToolUseFailure; a successful dispatch fires only
/// PostToolUse.
#[tokio::test(flavor = "current_thread")]
async fn post_tool_use_and_failure_never_double_fire() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, mut gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            // `get_goal` is valid but its test bridge has no Goal runtime resource,
            // so dispatch reaches the real tool and returns a structured failure.
            *actor.agent.borrow_mut() =
                test_agent_with_tools(vec![tools::registry::types::ToolConfig::for_tool::<
                    tools::implementations::grow_build::update_goal::GetGoalTool,
                >()])
                .await;
            begin_test_causal_turn(&actor).await;

            let mut client_hooks = crate::extensions::hooks::ClientHooks::new();
            for event in [
                ::hooks::event::HookEventName::PostToolUse,
                ::hooks::event::HookEventName::PostToolUseFailure,
            ] {
                client_hooks.insert(
                    event,
                    vec![crate::extensions::hooks::ClientHookGroup {
                        matcher: None,
                        callback_ids: vec!["cb".to_string()],
                        timeout: None,
                    }],
                );
            }
            *actor.hooks.client_hooks.borrow_mut() = client_hooks;

            let drain =
                |rx: &mut tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>| {
                    let mut events = Vec::new();
                    while let Ok(msg) = rx.try_recv() {
                        if let acp_transport::AcpClientMessage::ExtNotification(args) = msg
                            && args.request.method.as_ref() == "grow/hooks/event"
                        {
                            let params: serde_json::Value =
                                serde_json::from_str(args.request.params.get()).unwrap();
                            if let Some(name) = params["hookEventName"].as_str() {
                                events.push(name.to_string());
                            }
                        }
                    }
                    events
                };

            let todo_call = |id: &str| crate::sampling::types::ToolCallResponse {
                id: id.to_string(),
                kind: "function".to_string(),
                function: crate::sampling::types::ToolCallFunction::new(
                    "todo_write",
                    r#"{"todos":[{"id":"t1","content":"do","status":"completed"}]}"#,
                ),
            };

            // Failure: the finalized bridge is the execution authority; the
            // registered Goal tool fails because this actor has no Goal runtime.
            actor
                .execute_tool_calls(vec![crate::sampling::types::ToolCallResponse {
                    id: "call_err".to_string(),
                    kind: "function".to_string(),
                    function: crate::sampling::types::ToolCallFunction::new("get_goal", "{}"),
                }])
                .await
                .expect("execute_tool_calls must not error");
            assert_eq!(
                drain(&mut gateway_rx),
                ["post_tool_use_failure"],
                "an errored tool must fire only PostToolUseFailure, never PostToolUse"
            );

            *actor.agent.borrow_mut() = test_grow_build_agent_with_todo().await;
            actor
                .execute_tool_calls(vec![todo_call("call_ok")])
                .await
                .expect("execute_tool_calls must not error");
            assert_eq!(
                drain(&mut gateway_rx),
                ["post_tool_use"],
                "a successful tool must fire PostToolUse exactly once, never PostToolUseFailure"
            );
        })
        .await;
}

/// A `pre_tool_use` deny must NOT cancel the turn: `execute_tool_calls` feeds the deny
/// reason back as the blocked tool's `tool_result` and returns `ToolLoop::Continue`, so
/// the model re-samples with the reason in context.
///
/// Regression guard: the deny once surfaced as `ToolLoop::HookDenied`, which
/// `execute_tool_calls` treated as a terminal result, cancelling the whole turn.
#[tokio::test(flavor = "current_thread")]
async fn pre_tool_use_deny_feeds_reason_back_and_continues_turn() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            // The tool bridge must know `todo_write` so the call reaches the gate
            // rather than short-circuiting as an unknown tool.
            *actor.agent.borrow_mut() = test_grow_build_agent_with_todo().await;
            begin_test_causal_turn(&actor).await;

            install_client_hook(&actor, ::hooks::event::HookEventName::PreToolUse, &["cb_0"]);
            spawn_deny_responder(gateway_rx, "use read_file instead");

            let call = ToolCallResponse {
                id: "call_1".to_string(),
                kind: "function".to_string(),
                function: crate::sampling::types::ToolCallFunction::new(
                    "todo_write",
                    r#"{"todos":[{"id":"t1","content":"do","status":"completed"}]}"#,
                ),
            };

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.execute_tool_calls(vec![call]),
            )
            .await
            .expect("execute_tool_calls must not hang")
            .expect("execute_tool_calls must not error");

            assert!(
                matches!(result, ToolLoop::Continue),
                "a pre_tool_use deny must continue the turn, got {result:?}"
            );

            let conv = actor.chat_state_handle.get_conversation().await;
            assert!(
                conv.iter()
                    .any(|c| c.text_content().contains("use read_file instead")),
                "the deny reason must be fed back as the tool_result"
            );
        })
        .await;
}

/// The Stop client gate collects every deny as a block (no short-circuit).
#[tokio::test(flavor = "current_thread")]
async fn stop_client_gate_collects_denies() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, mut gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;

            install_client_hook(
                &actor,
                ::hooks::event::HookEventName::Stop,
                &["cb_block", "cb_allow"],
            );

            tokio::task::spawn_local(async move {
                while let Some(msg) = gateway_rx.recv().await {
                    match msg {
                        acp_transport::AcpClientMessage::ExtMethod(args) => {
                            let params: serde_json::Value =
                                serde_json::from_str(args.request.params.get()).unwrap();
                            let response = if params["hookCallbackId"] == "cb_block" {
                                serde_json::json!({
                                    "decision": "deny",
                                    "systemMessage": "finish the tests first",
                                })
                            } else {
                                serde_json::json!({})
                            };
                            let response_params: Arc<serde_json::value::RawValue> =
                                serde_json::value::to_raw_value(&response).unwrap().into();
                            let _ = args
                                .response_tx
                                .send(Ok(acp::ExtResponse::new(response_params)));
                        }
                        acp_transport::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });

            let envelope = actor.make_hook_envelope(
                ::hooks::event::HookEventName::Stop,
                Some("prompt-1".to_string()),
                ::hooks::event::HookPayload::Stop {
                    reason: "end_turn".to_string(),
                    stop_hook_active: true,
                    last_assistant_message: Some("I'm done".to_string()),
                    background_tasks: None,
                    session_crons: None,
                },
            );
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.run_stop_client_hooks(&envelope),
            )
            .await
            .expect("the stop gate must not hang");

            assert_eq!(result.blocks.len(), 1, "only the denying callback blocks");
            assert_eq!(result.blocks[0].hook_name, "client:cb_block");
            assert_eq!(result.blocks[0].reason, "finish the tests first");
            assert!(result.prevent_continuation.is_none());
            assert!(result.additional_context.is_empty());
        })
        .await;
}

/// `continue: false` becomes a force-stop (with `stopReason`) and `additionalContext`
/// becomes non-error feedback, matching what file hooks express.
#[tokio::test(flavor = "current_thread")]
async fn stop_client_gate_carries_continue_false_and_context() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, mut gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;

            install_client_hook(
                &actor,
                ::hooks::event::HookEventName::Stop,
                &["cb_stop", "cb_ctx"],
            );

            tokio::task::spawn_local(async move {
                while let Some(msg) = gateway_rx.recv().await {
                    match msg {
                        acp_transport::AcpClientMessage::ExtMethod(args) => {
                            let params: serde_json::Value =
                                serde_json::from_str(args.request.params.get()).unwrap();
                            let response = if params["hookCallbackId"] == "cb_stop" {
                                serde_json::json!({ "continue": false, "stopReason": "budget" })
                            } else {
                                serde_json::json!({ "additionalContext": "run the linter" })
                            };
                            let response_params: Arc<serde_json::value::RawValue> =
                                serde_json::value::to_raw_value(&response).unwrap().into();
                            let _ = args
                                .response_tx
                                .send(Ok(acp::ExtResponse::new(response_params)));
                        }
                        acp_transport::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });

            let envelope = actor.make_hook_envelope(
                ::hooks::event::HookEventName::Stop,
                Some("prompt-1".to_string()),
                ::hooks::event::HookPayload::Stop {
                    reason: "end_turn".to_string(),
                    stop_hook_active: false,
                    last_assistant_message: None,
                    background_tasks: None,
                    session_crons: None,
                },
            );
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.run_stop_client_hooks(&envelope),
            )
            .await
            .expect("the stop gate must not hang");

            assert!(result.blocks.is_empty());
            let prevent = result
                .prevent_continuation
                .expect("continue:false captured");
            assert_eq!(prevent.hook_name, "client:cb_stop");
            assert_eq!(prevent.reason, "budget");
            assert_eq!(result.additional_context, ["run the linter"]);
        })
        .await;
}

/// End-to-end through `run_stop_gate`: a client deny becomes `KeepWorking`, no hooks
/// allows the stop, and the consecutive-block cap overrides the gate.
#[tokio::test(flavor = "current_thread")]
async fn run_stop_gate_keep_working_and_cap() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            begin_test_causal_turn(&actor).await;

            let decision = actor.run_stop_gate("prompt-1", 0).await;
            assert!(matches!(decision, StopGateDecision::AllowStop));

            install_client_hook(&actor, ::hooks::event::HookEventName::Stop, &["cb_0"]);

            spawn_deny_responder(gateway_rx, "keep working");

            let decision = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.run_stop_gate("prompt-1", 0),
            )
            .await
            .expect("the stop gate must not hang");
            match decision {
                StopGateDecision::KeepWorking { feedback } => {
                    assert!(
                        feedback.contains("Stop hook feedback:")
                            && feedback.contains("keep working"),
                        "feedback must carry the deny message, got: {feedback}"
                    );
                }
                _ => panic!("a client deny must keep the agent working"),
            }

            let decision = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.run_stop_gate("prompt-1", MAX_STOP_HOOK_CONTINUATIONS_PER_TURN),
            )
            .await
            .expect("the capped gate must not hang");
            assert!(matches!(decision, StopGateDecision::AllowStop));
        })
        .await;
}

fn file_registry_with_stop_spec(
    event: ::hooks::event::HookEventName,
    script: &str,
) -> ::hooks::discovery::HookRegistry {
    let (mut registry, _) = ::hooks::discovery::load_hooks(None, None);
    registry.append_specs(vec![::hooks::config::HookSpec {
        name: "global/test-stop-hook".into(),
        event,
        handler_type: ::hooks::config::HandlerType::Command,
        configured_matcher: None,
        matcher: None,
        enabled: true,
        command: Some(std::path::PathBuf::from(script)),
        command_raw: Some(script.to_string()),
        url: None,
        url_raw: None,
        timeout_ms: 5000,
        on_failure: ::hooks::config::OnFailure::Allow,
        source_dir: std::path::PathBuf::from("/tmp"),
        extra_env: std::collections::HashMap::new(),
        layer: ::hooks::config::HookProvenance::File,
    }]);
    registry
}

/// A file-hook force-stop skips the client run gate (its signals would be discarded)
/// but still delivers the observe `grow/hooks/event` notification.
#[tokio::test(flavor = "current_thread")]
async fn file_force_stop_skips_client_gate_but_notifies() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, mut gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.hooks.resolved_workspace_root = "/tmp".to_string();
            begin_test_causal_turn(&actor).await;

            *actor.hooks.registry.borrow_mut() =
                Some(std::sync::Arc::new(file_registry_with_stop_spec(
                    ::hooks::event::HookEventName::Stop,
                    r#"echo '{"continue":false,"stopReason":"budget exhausted"}'"#,
                )));
            install_client_hook(
                &actor,
                ::hooks::event::HookEventName::Stop,
                &["cb_observer"],
            );

            let run_requests = std::rc::Rc::new(std::cell::Cell::new(0u32));
            let observe_events = std::rc::Rc::new(std::cell::Cell::new(0u32));
            let (runs, observes) = (run_requests.clone(), observe_events.clone());
            tokio::task::spawn_local(async move {
                while let Some(msg) = gateway_rx.recv().await {
                    match msg {
                        acp_transport::AcpClientMessage::ExtMethod(args) => {
                            if args.request.method.as_ref() == "grow/hooks/run" {
                                runs.set(runs.get() + 1);
                            }
                            let empty: Arc<serde_json::value::RawValue> =
                                serde_json::value::to_raw_value(&serde_json::json!({}))
                                    .unwrap()
                                    .into();
                            let _ = args.response_tx.send(Ok(acp::ExtResponse::new(empty)));
                        }
                        acp_transport::AcpClientMessage::ExtNotification(args) => {
                            if args.request.method.as_ref() == "grow/hooks/event" {
                                observes.set(observes.get() + 1);
                            }
                        }
                        acp_transport::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });

            let decision = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.run_stop_gate("prompt-1", 0),
            )
            .await
            .expect("the stop gate must not hang");
            assert!(
                matches!(decision, StopGateDecision::AllowStop),
                "a file force-stop must end the turn"
            );
            // Yield so the fire-and-forget notification lands.
            tokio::task::yield_now().await;
            assert_eq!(run_requests.get(), 0, "the client run gate must be skipped");
            assert_eq!(
                observe_events.get(),
                1,
                "client callbacks must still see the turn end as an observe event"
            );
        })
        .await;
}

/// Two client callbacks both force-stop; attribution follows registration order even
/// when that callback responds last (completion order must not decide it).
#[tokio::test(flavor = "current_thread")]
async fn client_force_stop_attribution_is_registration_ordered() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, mut gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;

            install_client_hook(
                &actor,
                ::hooks::event::HookEventName::Stop,
                &["cb_first", "cb_second"],
            );

            tokio::task::spawn_local(async move {
                while let Some(msg) = gateway_rx.recv().await {
                    match msg {
                        acp_transport::AcpClientMessage::ExtMethod(args) => {
                            let params: serde_json::Value =
                                serde_json::from_str(args.request.params.get()).unwrap();
                            let is_first = params["hookCallbackId"] == "cb_first";
                            tokio::task::spawn_local(async move {
                                if is_first {
                                    // The registration-order winner replies last.
                                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                }
                                let reason = if is_first {
                                    "from-first"
                                } else {
                                    "from-second"
                                };
                                let body: Arc<serde_json::value::RawValue> =
                                    serde_json::value::to_raw_value(&serde_json::json!({
                                        "continue": false,
                                        "stopReason": reason,
                                    }))
                                    .unwrap()
                                    .into();
                                let _ = args.response_tx.send(Ok(acp::ExtResponse::new(body)));
                            });
                        }
                        acp_transport::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });

            let envelope = actor.make_hook_envelope(
                ::hooks::event::HookEventName::Stop,
                Some("prompt-1".to_string()),
                ::hooks::event::HookPayload::Stop {
                    reason: "end_turn".to_string(),
                    stop_hook_active: false,
                    last_assistant_message: None,
                    background_tasks: None,
                    session_crons: None,
                },
            );
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.run_stop_client_hooks(&envelope),
            )
            .await
            .expect("the stop gate must not hang");

            let prevent = result.prevent_continuation.expect("force-stop captured");
            assert_eq!(
                prevent.hook_name, "client:cb_first",
                "attribution must follow registration order, not completion order"
            );
            assert_eq!(prevent.reason, "from-first");
        })
        .await;
}

/// A subagent session gates on `SubagentStop` specs (not `Stop`), with the gate-phase
/// payload.
#[tokio::test(flavor = "current_thread")]
async fn subagent_session_gates_on_subagent_stop() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (gateway_tx, mut gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            actor.startup_hints.is_subagent = true;
            actor.hooks.resolved_workspace_root = "/tmp".to_string();
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::SubagentSeed(
                    chat_state::SubagentSeedEvent {
                        parent_timeline_id: "test-parent-timeline".into(),
                        parent_spawn_seq: 1,
                        subagent_id: actor.session_id_string(),
                        security_parent_session_id: "test-parent-session".into(),
                        context_source: chat_state::SubagentContextSource::New,
                        source_ref: None,
                        normalized: false,
                    },
                ))
                .await
                .expect("the subagent stop test must durably seed its child identity");
            begin_test_causal_turn(&actor).await;

            *actor.hooks.registry.borrow_mut() =
                Some(std::sync::Arc::new(file_registry_with_stop_spec(
                    ::hooks::event::HookEventName::SubagentStop,
                    r#"echo '{"decision":"block","reason":"verify the summary"}'"#,
                )));

            tokio::task::spawn_local(async move {
                while let Some(msg) = gateway_rx.recv().await {
                    if let acp_transport::AcpClientMessage::SessionNotification(args) = msg {
                        let _ = args.response_tx.send(Ok(()));
                    }
                }
            });

            let decision = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.run_stop_gate("prompt-1", 0),
            )
            .await
            .expect("the subagent stop gate must not hang");
            match decision {
                StopGateDecision::KeepWorking { feedback } => {
                    assert!(
                        feedback.contains("verify the summary"),
                        "the SubagentStop block reason must become feedback, got: {feedback}"
                    );
                }
                other => {
                    panic!("a SubagentStop block must keep the subagent working, got {other:?}")
                }
            }
        })
        .await;
}

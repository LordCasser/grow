//! ExitPlan remains a batch barrier: if an earlier call is cancelled, the
//! approval protocol must not run. Plan content is submitted inline.

use super::support::*;
use super::*;
use agent_client_protocol as acp;

const SUBMITTED_PLAN: &str = "# Submitted plan\n- verify the result";

fn ext_response(outcome: &str) -> Arc<serde_json::value::RawValue> {
    serde_json::value::to_raw_value(&serde_json::json!({ "outcome": outcome }))
        .unwrap()
        .into()
}

fn exit_plan_mode_call(id: &str) -> ToolCallResponse {
    ToolCallResponse {
        id: id.to_string(),
        kind: "function".to_string(),
        function: crate::sampling::types::ToolCallFunction::new(
            "exit_plan_mode",
            serde_json::json!({ "plan": SUBMITTED_PLAN }).to_string(),
        ),
    }
}

fn bash_call(id: &str) -> ToolCallResponse {
    ToolCallResponse {
        id: id.to_string(),
        kind: "function".to_string(),
        function: crate::sampling::types::ToolCallFunction::new(
            "run_terminal_cmd",
            r#"{"command":"echo mixed-batch-reject","description":"probe mixed-batch permission cancel"}"#,
        ),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn mixed_permission_cancel_skips_exit_reverse_request() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            use grow_paths::AbsPathBuf;
            use grow_tools::implementations::grow_build::enter_plan_mode::EnterPlanModeTool;
            use grow_tools::implementations::grow_build::exit_plan_mode::ExitPlanModeTool;
            use grow_tools::registry::types::ToolConfig;
            use grow_workspace::permission::{ClientType, spawn_permission_manager};

            let (gateway_tx, mut gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<xai_acp_lib::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let mut actor =
                create_test_actor(0, 256_000, 85, gateway_tx.clone(), persistence_tx).await;
            // Disable background bash so finalize does not require companion
            // output and cancellation tools.
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![
                ToolConfig {
                    id: "Grow:run_terminal_cmd".into(),
                    params: Some(
                        serde_json::from_value(serde_json::json!({
                            "enabled_background": false
                        }))
                        .unwrap(),
                    ),
                    name_override: None,
                    params_name_overrides: None,
                    description_override: None,
                    behavior_version: None,
                    kind: None,
                },
                ToolConfig::for_tool::<EnterPlanModeTool>(),
                ToolConfig::for_tool::<ExitPlanModeTool>(),
            ])
            .await;

            let dir = tempfile::tempdir().unwrap();
            {
                let mut tracker = actor.plan_mode.lock();
                *tracker =
                    crate::session::plan_mode::BehaviorController::new(dir.path().to_path_buf());
                tracker.activate_from_tool();
            }

            let cwd = AbsPathBuf::new(std::path::PathBuf::from(actor.session_info.cwd.clone()))
                .unwrap_or_else(|_| AbsPathBuf::new(std::path::PathBuf::from("/tmp")).unwrap());
            let (perms, _ev) = spawn_permission_manager(
                actor.session_info.id.clone(),
                xai_acp_lib::AcpAgentGatewaySender::new(gateway_tx),
                cwd,
                ClientType::Generic,
                None,
                vec![],
                vec![],
                false,
                None,
                false,
            );
            actor.permissions = perms;

            let exit_fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let exit_fired_task = exit_fired.clone();
            let responder = tokio::task::spawn_local(async move {
                while let Some(msg) = gateway_rx.recv().await {
                    match msg {
                        xai_acp_lib::AcpClientMessage::RequestPermission(args) => {
                            let _ = args
                                .response_tx
                                .send(Ok(acp::RequestPermissionResponse::new(
                                    acp::RequestPermissionOutcome::Cancelled,
                                )));
                        }
                        xai_acp_lib::AcpClientMessage::ExtMethod(args) => {
                            if args.request.method.as_ref() == "grow/exit_plan_mode" {
                                exit_fired_task.store(true, std::sync::atomic::Ordering::SeqCst);
                                let _ = args
                                    .response_tx
                                    .send(Ok(acp::ExtResponse::new(ext_response("approved"))));
                            }
                        }
                        xai_acp_lib::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });

            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                actor.execute_tool_calls(vec![
                    bash_call("call_bash_reject"),
                    exit_plan_mode_call("call_exit"),
                ]),
            )
            .await
            .expect("execute_tool_calls must not hang")
            .expect("execute_tool_calls must not error");

            assert!(
                !exit_fired.load(std::sync::atomic::Ordering::SeqCst),
                "exit must not reverse-request after an earlier permission cancel"
            );
            responder.abort();
        })
        .await;
}

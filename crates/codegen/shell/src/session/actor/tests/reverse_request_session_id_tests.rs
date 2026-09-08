//! Regression guard: every blocking reverse-request
//! (permission / `ask_user_question` / plan-approval) must carry a
//! non-empty `sessionId`, otherwise Tier-2 routing silently drops it
//! (`server.rs`). The invariant holds today; these tests pin it.
use tools::implementations::grow_build::ask_user_question::{
    AskUserQuestionExtRequest, AskUserQuestionMode,
};
use tools::implementations::grow_build::plan_control::PlanApprovalExtRequest;

#[test]
fn ask_user_question_request_carries_session_id() {
    let req = AskUserQuestionExtRequest {
        session_id: "sess-abc".to_string(),
        tool_call_id: "call-1".to_string(),
        questions: vec![],
        mode: AskUserQuestionMode::Default,
    };
    assert!(!req.session_id.is_empty());
    // Wire format is camelCase (`sessionId`); Tier-2 routing reads it.
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["sessionId"], "sess-abc");
    assert!(!json["sessionId"].as_str().unwrap().is_empty());
}

#[test]
fn plan_approval_request_carries_session_id() {
    let req = PlanApprovalExtRequest {
        session_id: "sess-xyz".to_string(),
        tool_call_id: "call-2".to_string(),
        plan_content: "plan".to_string(),
    };
    assert!(!req.session_id.is_empty());
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["sessionId"], "sess-xyz");
    assert!(!json["sessionId"].as_str().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_question_does_not_stop_worker() {
    use super::support::*;
    use tools::implementations::grow_build::ask_user_question::{
        UserQuestionRequest, UserQuestionResponse,
    };
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway_tx, mut gateway_rx) = tokio::sync::mpsc::unbounded_channel();
            let (persistence_tx, _prx) = tokio::sync::mpsc::unbounded_channel();
            let actor = std::sync::Arc::new(
                create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await,
            );
            tokio::task::spawn_local(async move {
                while let Some(message) = gateway_rx.recv().await {
                    match message {
                        acp_transport::AcpClientMessage::ExtMethod(args) => {
                            let raw = serde_json::value::to_raw_value(
                                &serde_json::json!({"outcome":"cancelled"}),
                            )
                            .unwrap();
                            let _ = args
                                .response_tx
                                .send(Ok(acp_transport::protocol::ExtResponse::new(raw.into())));
                        }
                        acp_transport::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let (first_tx, first_rx) = tokio::sync::oneshot::channel();
            drop(first_rx);
            tx.send(UserQuestionRequest {
                tool_call_id: "cancelled".into(),
                questions: vec![],
                result_tx: first_tx,
            })
            .unwrap();
            let (next_tx, next_rx) = tokio::sync::oneshot::channel();
            tx.send(UserQuestionRequest {
                tool_call_id: "next".into(),
                questions: vec![],
                result_tx: next_tx,
            })
            .unwrap();
            super::super::spawn::start_user_question_worker(&actor, rx);
            let response = tokio::time::timeout(std::time::Duration::from_secs(2), next_rx)
                .await
                .unwrap()
                .expect("worker must serve the next request")
                .unwrap();
            assert!(matches!(response, UserQuestionResponse::Cancelled));
            actor.background_service_shutdown.cancel();
        })
        .await;
}

use super::*;

use crate::session::behavior::{
    BehaviorCoordinator, RetainedPlanAction, RetainedPlanDecisionOutcome, RetainedPlanStatus,
};

async fn query_status(
    agent: &MvpAgent,
    session_id: &acp::SessionId,
    decision_id: &str,
) -> serde_json::Value {
    let request = acp::ExtRequest::new(
        "grow/plan/decision_status",
        std::sync::Arc::from(
            serde_json::value::to_raw_value(&serde_json::json!({
                "sessionId": session_id.to_string(),
                "decisionId": decision_id,
            }))
            .unwrap(),
        ),
    );
    let response = crate::extensions::session_admin::handle(agent, &request)
        .await
        .unwrap();
    serde_json::from_str(response.0.get()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn plan_decision_status_is_unconfirmed_until_control_ack_commits() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut behavior = BehaviorCoordinator::new();
            behavior.select_behavior(tool_types::BehaviorId::Plan);
            behavior.record_plan_artifact("# deferred plan");
            assert!(behavior.submit_initial_plan());
            let submission_id = behavior.plan_submission_id().unwrap();
            let submitted = behavior.snapshot();

            let mut timeline =
                chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::system(
                    "test system",
                )])
                .unwrap();
            timeline
                .record(
                    crate::session::control::SessionControlSnapshot::new(
                        1,
                        "grow-build",
                        submitted,
                        None,
                    )
                    .timeline_kind_with_model_context_items(Vec::new())
                    .unwrap(),
                )
                .unwrap();

            assert_eq!(
                behavior.decide_submitted_plan_if(
                    &behavior.snapshot(),
                    submission_id,
                    "defer-while-writing",
                    RetainedPlanAction::Defer,
                    Some("later".into()),
                ),
                RetainedPlanDecisionOutcome::Applied(RetainedPlanStatus::Deferred)
            );
            let deferred_snapshot = behavior.snapshot();
            let pending_control = crate::session::control::SessionControlSnapshot::new(
                2,
                "grow-build",
                deferred_snapshot,
                None,
            )
            .timeline_kind_with_model_context_items(Vec::new())
            .unwrap();

            let (persistence, mut persistence_rx) =
                chat_state::MockTimelinePersistence::new_with_manual_timeline_ack();
            let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
            let chat_state = chat_state::ChatStateActor::spawn_from_timeline(
                timeline.events().to_vec(),
                sampling_types::SamplingConfig {
                    base_url: "https://api.test/v1".into(),
                    model: "test/default".into(),
                    output_limit: None,
                    temperature: None,
                    top_p: None,
                    api_backend: Default::default(),
                    extra_headers: Default::default(),
                    query_params: Default::default(),
                    env_http_headers: Default::default(),
                    context_window: std::num::NonZeroU64::new(128_000).unwrap(),
                    reasoning_effort: None,
                    stream_tool_calls: None,
                },
                Box::new(persistence),
                event_tx,
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("spawn test ChatState writer");
            // Keep the mock persistence receiver alive for the whole test: it
            // owns the ACK channel that represents the durable writer boundary.
            let _persistence_rx = &mut persistence_rx;

            let (mut agent, _gateway_rx) = build_agent_with_gateway_rx();
            let session_id = acp::SessionId::new("plan-decision-status");
            let mut session = make_test_handle("test/default", None);
            session.info.id = session_id.clone();
            session.chat_state_handle = chat_state.clone();
            agent
                .sessions
                .borrow_mut()
                .insert(session_id.clone(), session);

            let absent = query_status(&agent, &session_id, "defer-while-writing").await;
            assert_eq!(absent["status"], "unconfirmed");
            assert!(absent.get("receipt").is_none());

            let write_handle = chat_state.clone();
            let mut write = tokio::task::spawn_local(async move {
                write_handle
                    .record_timeline_event_durably(pending_control)
                    .await
            });
            let first_ack = persistence_rx
                .next_timeline_ack()
                .await
                .expect("first Control persistence ACK handle");
            let pending = chat_state
                .pending_timeline_commit()
                .expect("Control event remains pending until ACK");
            let pending_event = serde_json::to_value(&pending.event).unwrap();
            assert!(matches!(
                &pending.event.kind,
                chat_state::TimelineEventKind::Control(control)
                    if control.snapshot["behavior"]["retained_plan"]["decisions"][0]["decision_id"]
                        == "defer-while-writing"
            ));

            let during_write = tokio::time::timeout(
                std::time::Duration::from_millis(250),
                query_status(&agent, &session_id, "defer-while-writing"),
            )
            .await
            .expect("in-flight status query must be bounded");
            assert_eq!(during_write["status"], "unconfirmed");
            assert!(during_write.get("receipt").is_none());
            drop(first_ack); // The writer cannot treat a lost ACK as rejection.

            let retry_ack = persistence_rx
                .next_timeline_ack()
                .await
                .expect("same Control event is retried after its ACK is lost");
            let pending_retry = chat_state
                .pending_timeline_commit()
                .expect("retry remains unconfirmed until its ACK");
            assert_eq!(
                serde_json::to_value(&pending_retry.event).unwrap(),
                pending_event
            );
            assert!(pending_retry.acknowledgement_lost);

            let while_retrying = query_status(&agent, &session_id, "defer-while-writing").await;
            assert_eq!(while_retrying["status"], "unconfirmed");

            retry_ack
                .send(Ok(()))
                .expect("durable retry ACK reaches the ChatState owner");
            write.await.unwrap().expect("defer Control write commits");
            assert!(chat_state.pending_timeline_commit().is_none());

            let committed = query_status(&agent, &session_id, "defer-while-writing").await;
            assert_eq!(committed["status"], "accepted");
            assert_eq!(committed["receipt"]["decision_id"], "defer-while-writing");
            assert_eq!(committed["receipt"]["submission_id"], submission_id);
        })
        .await;
}

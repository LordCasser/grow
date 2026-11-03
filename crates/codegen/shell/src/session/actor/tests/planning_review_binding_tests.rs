use super::*;
use tools::implementations::grow_build::todo::{PlanningAction, TodoWriteInput};

fn timeline_result_ref() -> String {
    serde_json::to_string(&chat_state::TimelineRangeRef {
        timeline_id: "task-1".into(),
        first_seq: 1,
        last_seq: 1,
    })
    .expect("timeline range reference serializes")
}

fn binding_from_summary(summary: &str, link_id: &str) -> serde_json::Value {
    summary
        .lines()
        .filter_map(|line| line.strip_prefix("    review binding: "))
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|binding| binding.get("linkId").and_then(|value| value.as_str()) == Some(link_id))
        .expect("summary contains the link's review binding")
}

async fn write_bound_review(
    actor: &std::sync::Arc<SessionActor>,
    prompt_id: &str,
    review: DeliveryReview,
) -> Result<(), String> {
    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle_planning_command(PlanningCommand::Write {
            prompt_id: prompt_id.into(),
            input: TodoWriteInput {
                merge: true,
                todos: Vec::new(),
                actions: vec![PlanningAction::Review { review }],
            },
            respond_to,
        })
        .await;
    response
        .await
        .expect("planning actor must acknowledge")
        .map(|_| ())
}

async fn terminal_result_plan(actor: &SessionActor) -> (tool_types::LocalPlan, String) {
    let snapshot = actor
        .chat_state_handle
        .planning_state()
        .await
        .expect("read plan")
        .expect("plan exists");
    let result_id = timeline_result_ref();
    let mut plan = snapshot.plan;
    plan.revision += 1;
    plan.links[0].result_id = Some(result_id.clone());
    plan.mark_link_terminal("task-1", "child result arrived")
        .expect("fixture link should become terminal");
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
            chat_state::timeline::PlanningEvent {
                previous: Some(snapshot.seq),
                plan: Box::new(plan.clone()),
            },
        ))
        .await
        .expect("terminal result fact must be durable before review");
    (plan, result_id)
}

fn review_from_binding(
    binding: &serde_json::Value,
    decision: ReviewDecision,
    reason: Option<&str>,
) -> DeliveryReview {
    serde_json::from_value({
        let mut value = binding.clone();
        let object = value.as_object_mut().expect("binding is a JSON object");
        object.insert("decision".into(), serde_json::json!(decision));
        object.insert("reason".into(), serde_json::json!(reason));
        object.insert(
            "verificationRefs".into(),
            serde_json::json!(["review:test"]),
        );
        value
    })
    .expect("binding plus review decision deserializes")
}

#[tokio::test(flavor = "current_thread")]
async fn review_binding_from_todo_summary_accepts_and_cold_replays() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (decision, reason) in [
                (ReviewDecision::Accepted, None),
                (
                    ReviewDecision::Released,
                    Some("execution explicitly released"),
                ),
            ] {
                let actor = linked_actor().await;
                crate::session::actor::tests::support::install_test_foreground(
                    &actor,
                    "planning-review-test",
                )
                .await;
                let (plan, result_id) = terminal_result_plan(&actor).await;
                let summary = tools::implementations::grow_build::todo::todo_write_success(&plan)
                    .summary_for_prompt;
                let binding = binding_from_summary(&summary, "task-1");
                assert_eq!(binding["linkId"], "task-1");
                assert_eq!(binding["resultId"], result_id);
                assert_eq!(binding["reviewedSourceRevision"], plan.revision);
                let context = actor.local_planning_context().await.unwrap().unwrap();
                let (_, json) = context.split_once('\n').unwrap();
                let context: serde_json::Value = serde_json::from_str(json).unwrap();
                assert_eq!(context["links"][0]["review_binding"], binding);

                write_bound_review(
                    &actor,
                    "planning-review-test",
                    review_from_binding(&binding, decision, reason),
                )
                .await
                .expect("bound review should commit");
                assert!(
                    actor
                        .planning_completion_blockers()
                        .await
                        .unwrap()
                        .is_empty()
                );

                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let replay = chat_state::Timeline::from_events(events)
                    .expect("cold replay retains valid review history");
                let replayed_plan = &replay.planning_snapshot().expect("planning snapshot").plan;
                assert_eq!(replayed_plan.reviews.len(), 1);
                assert_eq!(replayed_plan.reviews[0].decision, decision);
                assert_eq!(
                    replayed_plan.reviews[0].result_id.as_deref(),
                    Some(result_id.as_str())
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn mismatched_review_result_is_rejected_with_exact_binding_before_commit() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor().await;
            crate::session::actor::tests::support::install_test_foreground(
                &actor,
                "planning-review-test",
            )
            .await;
            let (plan, result_id) = terminal_result_plan(&actor).await;
            let binding = binding_from_summary(
                &tools::implementations::grow_build::todo::todo_write_success(&plan)
                    .summary_for_prompt,
                "task-1",
            );
            let binding_text = serde_json::to_string(&binding).unwrap();

            for (bad_result, decision) in [
                (None, ReviewDecision::Accepted),
                (None, ReviewDecision::Released),
                (Some("1".to_owned()), ReviewDecision::Released),
            ] {
                let before = actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap();
                let error = write_bound_review(
                    &actor,
                    "planning-review-test",
                    DeliveryReview {
                        link_id: "task-1".into(),
                        result_id: bad_result.clone(),
                        reviewed_source_revision: before.plan.revision,
                        decision,
                        reason: Some("reviewed result".into()),
                        verification_refs: Vec::new(),
                    },
                )
                .await
                .expect_err("incorrect result reference must be rejected");
                assert_eq!(
                    error,
                    format!(
                        "review resultId does not match the delegated result; review binding: {binding_text}"
                    )
                );
                let after = actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(after.seq, before.seq, "rejected review must not commit");
                assert_eq!(after.plan.links[0].result_id.as_deref(), Some(result_id.as_str()));
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn rejected_review_for_link_outside_summary_still_reports_its_binding() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor().await;
            crate::session::actor::tests::support::install_test_foreground(
                &actor,
                "planning-review-test",
            )
            .await;
            let snapshot = actor.chat_state_handle.planning_state().await.unwrap().unwrap();
            let mut plan = snapshot.plan;
            let target_result = serde_json::to_string(&chat_state::TimelineRangeRef {
                timeline_id: "task-9".into(),
                first_seq: 9,
                last_seq: 9,
            })
            .unwrap();
            for index in 2..=9 {
                plan.links.push(PlanLink {
                    id: format!("task-{index}"),
                    parent_step_id: "step-1".into(),
                    child_session_id: format!("task-{index}"),
                    terminal: index == 9,
                    required: true,
                    pending_source_revision: None,
                    review_required_revision: plan.revision,
                    result_id: (index == 9).then(|| target_result.clone()),
                    dispatch_failure: None,
                });
            }
            plan.revision += 1;
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: Some(snapshot.seq),
                        plan: Box::new(plan.clone()),
                    },
                ))
                .await
                .expect("expanded plan must be durable");
            let summary = tools::implementations::grow_build::todo::todo_write_success(&plan)
                .summary_for_prompt;
            assert!(!summary.contains("\"linkId\":\"task-9\""));
            let binding = tools::implementations::grow_build::todo::review_binding(
                &plan,
                plan.links.iter().find(|link| link.id == "task-9").unwrap(),
            );
            let binding_text = serde_json::to_string(&binding).unwrap();
            let before = actor.chat_state_handle.planning_state().await.unwrap().unwrap();
            let error = write_bound_review(
                &actor,
                "planning-review-test",
                DeliveryReview {
                    link_id: "task-9".into(),
                    result_id: Some("9".into()),
                    reviewed_source_revision: before.plan.revision,
                    decision: ReviewDecision::Accepted,
                    reason: Some("reviewed result".into()),
                    verification_refs: Vec::new(),
                },
            )
            .await
            .expect_err("mismatched ninth binding must be rejected");
            assert_eq!(
                error,
                format!(
                    "review resultId does not match the delegated result; review binding: {binding_text}"
                )
            );
            let after = actor.chat_state_handle.planning_state().await.unwrap().unwrap();
            assert_eq!(after.seq, before.seq);
        })
        .await;
}

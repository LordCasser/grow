// Audit probe: PASS means the actor-level scope-switch rejection was reproduced.
use super::*;

#[tokio::test(flavor = "current_thread")]
async fn audit_released_undispatched_link_can_validate_new_scope_but_actor_rejects_it() {
    use crate::session::actor::tests::support::install_test_foreground;
    use tools::implementations::grow_build::todo::{PlanningAction, TodoWriteInput};

    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor().await;
            install_test_foreground(&actor, "planning-scope-audit").await;

            record_undispatched(&actor, "coordinator admission closed")
                .await
                .expect("dispatch rejection must be recorded");

            let before_review = actor
                .chat_state_handle
                .planning_state()
                .await
                .expect("read local plan")
                .expect("linked actor has a plan");
            let link = &before_review.plan.links[0];
            assert_eq!(
                link.dispatch_failure.as_deref(),
                Some("coordinator admission closed")
            );
            assert!(!link.terminal, "dispatch rejection is not child completion");
            assert!(link.result_id.is_none());

            let (respond_to, response) = tokio::sync::oneshot::channel();
            actor
                .handle_planning_command(PlanningCommand::Write {
                    prompt_id: "planning-scope-audit".into(),
                    input: TodoWriteInput {
                        merge: true,
                        todos: Vec::new(),
                        actions: vec![PlanningAction::Review {
                            review: DeliveryReview {
                                link_id: "task-1".into(),
                                result_id: None,
                                reviewed_source_revision: before_review.plan.revision,
                                decision: ReviewDecision::Released,
                                reason: Some("task was rejected before dispatch".into()),
                                verification_refs: Vec::new(),
                            },
                        }],
                    },
                    respond_to,
                })
                .await;
            response
                .await
                .expect("released review must answer")
                .expect("released review must commit");

            let released = actor
                .chat_state_handle
                .planning_state()
                .await
                .expect("read released local plan")
                .expect("released plan remains present");
            assert!(
                actor
                    .planning_completion_blockers()
                    .await
                    .expect("completion blockers must be readable")
                    .is_empty(),
                "a released undispatched link has no outstanding completion blocker"
            );

            let next = tool_types::LocalPlan::new(actor.session_id_string(), "work:next");
            next.validate_transition_from(&released.plan)
                .expect("the planning model permits a new scope after release");

            let (respond_to, response) = tokio::sync::oneshot::channel();
            actor
                .handle_planning_command(PlanningCommand::Write {
                    prompt_id: "planning-scope-audit".into(),
                    input: TodoWriteInput {
                        merge: true,
                        todos: Vec::new(),
                        actions: vec![PlanningAction::SelectScope {
                            scope_id: "work:next".into(),
                        }],
                    },
                    respond_to,
                })
                .await;
            let error = response
                .await
                .expect("scope selection must answer")
                .expect_err("actor guard is expected to reject the released link");
            assert_eq!(
                error,
                "dispose old executions, waits and mandatory deliveries before switching scope"
            );

            let after_rejection = actor
                .chat_state_handle
                .planning_state()
                .await
                .expect("read plan after rejected selection")
                .expect("original plan remains present");
            assert_eq!(after_rejection.seq, released.seq);
            assert_eq!(after_rejection.plan, released.plan);
        })
        .await;
}

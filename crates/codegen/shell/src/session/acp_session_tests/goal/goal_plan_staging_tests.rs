//! Tier 3 contract tests for the planner staging channel (Task 3).
//!
//! The planner commits through `submit_goal_plan_section` /
//! `finalize_goal_plan` instead of returning a Markdown document. These
//! tests drive the actor-side state machine directly through
//! `handle_goal_plan_command` and `handle_goal_stage_completed`; the
//! resume request wiring is asserted through the pure
//! `build_goal_stage_request` constructor (Reject Conditions fallback:
//! e2e subagent tool loops need a real model).

use super::goal::planner_prompt;
use super::support::*;
use super::*;
use crate::session::acp_session::goal::GoalPlanStaging;
use crate::session::goal_board::parse_goal_board;
use crate::session::goal_tracker::{GoalPhase, GoalStatus, StageLease};
use crate::session::replay_events::{GoalStageCompletion, GoalStageKind};
use tokio::sync::oneshot;
use tool_types::{GoalPlanSectionPayload, GoalPlanTaskSpec, GoalTaskStatus};
use tools::implementations::grow_build::task::types::GoalStageResume;
use tools::implementations::grow_build::update_goal::{GoalPlanCommand, StageToken};

async fn planning_actor() -> (std::sync::Arc<SessionActor>, StageLease) {
    let (gateway_tx, _gateway_rx) =
        tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, _persistence_rx) =
        tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    let actor =
        std::sync::Arc::new(create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await);
    {
        let mut tracker = actor.goal_tracker.lock();
        tracker.create_goal(
            "goal-1".into(),
            "ship safely".into(),
            None,
            0,
            "now".into(),
            None,
        );
    }
    let lease = actor
        .goal_tracker
        .lock()
        .claim_stage(GoalPhase::Planning)
        .expect("planning lease");
    (actor, lease)
}

fn plan_tasks() -> GoalPlanSectionPayload {
    GoalPlanSectionPayload::PlanTasks {
        tasks: vec![GoalPlanTaskSpec {
            summary: "Task A: implement the change".into(),
            status: Some(GoalTaskStatus::InProgress),
            scope: Some("runtime".into()),
            acceptance: Some("tests pass".into()),
            evidence: None,
            gap: None,
            children: vec![GoalPlanTaskSpec {
                summary: "FS-1: add regression coverage".into(),
                status: None,
                scope: None,
                acceptance: None,
                evidence: None,
                gap: None,
                children: Vec::new(),
            }],
        }],
    }
}

fn acceptance() -> GoalPlanSectionPayload {
    GoalPlanSectionPayload::GoalAcceptance {
        items: vec!["tests pass".into()],
    }
}

async fn submit(
    actor: &std::sync::Arc<SessionActor>,
    stage: StageToken,
    section: GoalPlanSectionPayload,
) -> Result<tools::implementations::grow_build::update_goal::SubmitGoalPlanSectionOutput, String> {
    let (tx, rx) = oneshot::channel();
    actor
        .handle_goal_plan_command(GoalPlanCommand::SubmitPlanSection {
            stage,
            section,
            respond_to: tx,
        })
        .await;
    rx.await.expect("submit response")
}

async fn finalize(
    actor: &std::sync::Arc<SessionActor>,
    stage: StageToken,
) -> Result<tools::implementations::grow_build::update_goal::FinalizeGoalPlanOutput, String> {
    let (tx, rx) = oneshot::channel();
    actor
        .handle_goal_plan_command(GoalPlanCommand::FinalizePlan {
            stage,
            respond_to: tx,
        })
        .await;
    rx.await.expect("finalize response")
}

#[tokio::test(flavor = "current_thread")]
async fn staged_sections_finalize_into_a_committed_board_with_host_assigned_ids() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, lease) = planning_actor().await;
            let stage = StageToken(lease.stage_id);

            // External naming ("Task A", "FS-1") appears only as summary
            // text; the host assigns T1/T1.1.
            let output = submit(&actor, stage, plan_tasks()).await.unwrap();
            assert_eq!(output.accepted_sections, ["plan_tasks"]);
            assert!(output.issues.is_empty());

            let output = submit(&actor, stage, acceptance()).await.unwrap();
            assert_eq!(output.accepted_sections, ["plan_tasks", "goal_acceptance"]);
            assert!(output.issues.is_empty());

            let finalize = finalize(&actor, stage).await.unwrap();
            assert_eq!(finalize.view.tasks.len(), 2);
            assert_eq!(finalize.view.tasks[0].id, "T1");
            assert_eq!(finalize.view.tasks[1].id, "T1.1");
            assert_eq!(finalize.view.tasks[1].parent_id.as_deref(), Some("T1"));
            assert!(finalize.summary.contains("committed"));

            {
                let tracker = actor.goal_tracker.lock();
                let goal = tracker.snapshot().expect("goal exists");
                assert_eq!(goal.phase, GoalPhase::Executing);
                assert_eq!(goal.planner_failures, 0);
                assert!(goal.in_flight_stage.is_none());
                assert!(parse_goal_board(&goal.objective, goal.board.markdown.clone()).is_ok());
            }
            assert!(
                actor.goal_plan_staging.lock().unwrap().is_none(),
                "a committed board clears the staging"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_sections_return_structured_issues_without_charging_failures() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, lease) = planning_actor().await;
            let stage = StageToken(lease.stage_id);

            let invalid = GoalPlanSectionPayload::PlanTasks {
                tasks: vec![GoalPlanTaskSpec {
                    summary: "   ".into(),
                    status: None,
                    scope: None,
                    acceptance: None,
                    evidence: None,
                    gap: None,
                    children: Vec::new(),
                }],
            };
            let output = submit(&actor, stage, invalid).await.unwrap();
            assert!(
                output.accepted_sections.is_empty(),
                "an invalid section must not be accepted"
            );
            assert_eq!(output.issues.len(), 1);
            assert_eq!(output.issues[0].path, "tasks[0].summary");
            assert!(output.issues[0].reason.contains("must not be empty"));

            {
                let tracker = actor.goal_tracker.lock();
                let goal = tracker.snapshot().expect("goal exists");
                assert_eq!(goal.phase, GoalPhase::Planning);
                assert_eq!(
                    goal.planner_failures, 0,
                    "per-item validation never charges planner_failures"
                );
            }

            // Fix-resubmit: the corrected section is accepted and replaces
            // nothing (no prior acceptance existed).
            let output = submit(&actor, stage, plan_tasks()).await.unwrap();
            assert_eq!(output.accepted_sections, ["plan_tasks"]);
            assert!(output.issues.is_empty());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn planner_infra_failure_records_staging_and_next_spawn_resumes() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, lease) = planning_actor().await;
            // An accepted section before the failure must survive the respawn.
            let stage = StageToken(lease.stage_id);
            submit(&actor, stage, plan_tasks()).await.unwrap();

            actor
                .handle_goal_stage_completed(GoalStageCompletion {
                    lease: lease.clone(),
                    subagent_id: Some("prior-planner-1".into()),
                    kind: GoalStageKind::Planner(Err("planner infra failure".into())),
                })
                .await;

            {
                let staging = actor.goal_plan_staging.lock().unwrap();
                let staging = staging.as_ref().expect("staging recorded");
                assert_eq!(staging.goal_id, "goal-1");
                assert_eq!(
                    staging.prior_subagent_id.as_deref(),
                    Some("prior-planner-1")
                );
                assert_eq!(staging.last_error.as_deref(), Some("planner infra failure"));
                assert_eq!(
                    staging.accepted_sections.len(),
                    1,
                    "accepted sections survive an infra failure"
                );
            }
            {
                let tracker = actor.goal_tracker.lock();
                let goal = tracker.snapshot().expect("goal exists");
                assert_eq!(goal.planner_failures, 1);
                assert!(goal.in_flight_stage.is_none());
            }

            // The next spawn decision resumes the prior planner with the
            // failure reason and accepted sections in its prompt, and the
            // constructed request carries the agreeing resume fields.
            let goal = actor.goal_tracker.lock().snapshot().cloned().expect("goal");
            let resume = SessionActor::planner_resume_for(&actor, &goal)
                .expect("matching staging yields a resume target");
            assert_eq!(resume.prior_subagent_id, "prior-planner-1");
            assert_eq!(resume.accepted_sections.len(), 1);

            let prompt = planner_prompt(&goal, Some(&resume));
            assert!(prompt.contains("planner infra failure"));
            assert!(prompt.contains("plan_tasks: 1 top-level task(s)"));
            assert!(prompt.contains("finalize_goal_plan"));

            let request = SessionActor::build_goal_stage_request(
                "parent".into(),
                Some("/tmp".into()),
                &lease,
                tools::implementations::grow_build::update_goal::GoalContextSnapshot {
                    role:
                        tools::implementations::grow_build::task::types::GoalSubagentRole::Planner,
                    view: crate::session::acp_session::goal::goal_view_from_snapshot(&goal, 0),
                },
                prompt,
                "Goal planner".into(),
                tokio_util::sync::CancellationToken::new(),
                false,
                None,
                Some(GoalStageResume {
                    prior_subagent_id: "prior-planner-1".into(),
                }),
            )
            .expect("valid goal stage request");
            assert_eq!(request.resume_from.as_deref(), Some("prior-planner-1"));
            assert_eq!(
                request
                    .goal_stage_resume
                    .as_ref()
                    .map(|r| r.prior_subagent_id.as_str()),
                Some("prior-planner-1")
            );
            assert!(
                !request.fork_context,
                "resume takes precedence over forking"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn planner_that_ends_without_finalizing_is_treated_as_a_respawn() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, lease) = planning_actor().await;
            actor
                .handle_goal_stage_completed(GoalStageCompletion {
                    lease: lease.clone(),
                    subagent_id: Some("prior-planner-2".into()),
                    kind: GoalStageKind::Planner(Ok(())),
                })
                .await;

            {
                let tracker = actor.goal_tracker.lock();
                let goal = tracker.snapshot().expect("goal exists");
                assert_eq!(goal.phase, GoalPhase::Planning);
                assert_eq!(goal.planner_failures, 1);
            }
            let staging = actor.goal_plan_staging.lock().unwrap();
            let staging = staging.as_ref().expect("staging recorded");
            assert_eq!(
                staging.prior_subagent_id.as_deref(),
                Some("prior-planner-2")
            );
            assert!(
                staging
                    .last_error
                    .as_deref()
                    .unwrap()
                    .contains("without finalizing")
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn three_respawn_failures_pause_the_goal() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, lease) = planning_actor().await;
            // The helper holds one lease; release it so each respawn round
            // can claim a fresh one (planner_failed releases its lease).
            assert!(actor.goal_tracker.lock().release_stage(&lease));
            for round in 1..=3 {
                let lease = actor
                    .goal_tracker
                    .lock()
                    .claim_stage(GoalPhase::Planning)
                    .expect("planning lease after release");
                actor
                    .handle_goal_stage_completed(GoalStageCompletion {
                        lease,
                        subagent_id: Some(format!("planner-{round}")),
                        kind: GoalStageKind::Planner(Err("planner infra failure".into())),
                    })
                    .await;
            }
            let tracker = actor.goal_tracker.lock();
            let goal = tracker.snapshot().expect("goal exists");
            assert_eq!(goal.status, GoalStatus::Paused);
            assert_eq!(goal.planner_failures, 3);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn late_and_mismatched_stage_commands_are_rejected_without_state_changes() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, lease) = planning_actor().await;
            let stage = StageToken(lease.stage_id);
            submit(&actor, stage, plan_tasks()).await.unwrap();
            submit(&actor, stage, acceptance()).await.unwrap();
            finalize(&actor, stage).await.unwrap();

            // Late Submit/Finalize after the stage committed.
            let error = submit(&actor, stage, acceptance()).await.unwrap_err();
            assert!(error.contains("expired") || error.contains("in flight"));
            let error = finalize(&actor, stage).await.unwrap_err();
            assert!(error.contains("expired") || error.contains("in flight"));

            // Mismatched stage id while a stage is live.
            let (tx, rx) = oneshot::channel();
            actor
                .handle_goal_command(tools::implementations::grow_build::update_goal::GoalCommand::Replan {
                    input: tools::implementations::grow_build::update_goal::RequestGoalReplanInput {
                        expected_plan_revision: 1,
                        expected_board_revision: 1,
                        guidance: "redo".into(),
                        reason: "test".into(),
                    },
                    respond_to: tx,
                })
                .await;
            assert!(rx.await.unwrap().is_ok());
            let new_lease = actor
                .goal_tracker
                .lock()
                .claim_stage(GoalPhase::Planning)
                .expect("new planning lease");
            let wrong_stage = StageToken(new_lease.stage_id.wrapping_add(7));
            let error = submit(&actor, wrong_stage, plan_tasks()).await.unwrap_err();
            assert!(error.contains("mismatch") || error.contains("expired"));

            {
                let tracker = actor.goal_tracker.lock();
                let goal = tracker.snapshot().expect("goal exists");
                assert_eq!(goal.phase, GoalPhase::Planning);
                assert_eq!(goal.planner_failures, 0);
                assert!(
                    goal.board.markdown.contains("T1"),
                    "request_replan preserves the prior board as planner evidence"
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn replan_invalidates_staging_and_late_commands_are_rejected() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, lease) = planning_actor().await;
            let stage = StageToken(lease.stage_id);
            submit(&actor, stage, plan_tasks()).await.unwrap();
            assert!(actor.goal_plan_staging.lock().unwrap().is_some());

            let (tx, rx) = oneshot::channel();
            actor
                .handle_goal_command(tools::implementations::grow_build::update_goal::GoalCommand::Replan {
                    input: tools::implementations::grow_build::update_goal::RequestGoalReplanInput {
                        expected_plan_revision: 1,
                        expected_board_revision: 0,
                        guidance: "redo".into(),
                        reason: "test".into(),
                    },
                    respond_to: tx,
                })
                .await;
            assert!(rx.await.unwrap().is_ok());
            assert!(
                actor.goal_plan_staging.lock().unwrap().is_none(),
                "replan drops the staging"
            );

            // The old stage token no longer resolves to a live lease.
            let error = submit(&actor, stage, acceptance()).await.unwrap_err();
            assert!(error.contains("expired") || error.contains("in flight"));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn finalize_without_required_sections_reports_missing_section_issues() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, lease) = planning_actor().await;
            let stage = StageToken(lease.stage_id);
            submit(&actor, stage, plan_tasks()).await.unwrap();

            let error = finalize(&actor, stage).await.unwrap_err();
            assert!(error.contains("sections"));
            assert!(error.contains("goal_acceptance"));

            {
                let tracker = actor.goal_tracker.lock();
                let goal = tracker.snapshot().expect("goal exists");
                assert_eq!(goal.phase, GoalPhase::Planning);
                assert_eq!(goal.planner_failures, 0);
            }
            // Staging survives a rejected finalize so the planner can fix
            // and retry without losing accepted work.
            assert_eq!(
                actor
                    .goal_plan_staging
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|s| s.accepted_sections.len()),
                Some(1)
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn resubmitting_the_same_kind_replaces_only_on_acceptance() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, lease) = planning_actor().await;
            let stage = StageToken(lease.stage_id);
            submit(&actor, stage, plan_tasks()).await.unwrap();

            // A rejected replacement leaves the previously accepted section
            // untouched.
            let invalid = GoalPlanSectionPayload::PlanTasks {
                tasks: vec![GoalPlanTaskSpec {
                    summary: String::new(),
                    status: None,
                    scope: None,
                    acceptance: None,
                    evidence: None,
                    gap: None,
                    children: Vec::new(),
                }],
            };
            let output = submit(&actor, stage, invalid).await.unwrap();
            assert_eq!(output.accepted_sections, ["plan_tasks"]);
            assert!(!output.issues.is_empty());
            {
                // Scope the staging guard: the next submit needs the lock.
                let staging = actor.goal_plan_staging.lock().unwrap();
                let staging = staging.as_ref().expect("staging exists");
                assert_eq!(staging.accepted_sections.len(), 1);
            }

            // A fixed replacement is accepted and replaces in place.
            let mut fixed = plan_tasks();
            if let GoalPlanSectionPayload::PlanTasks { tasks } = &mut fixed {
                tasks[0].summary = "Task B: revised".into();
            }
            let output = submit(&actor, stage, fixed).await.unwrap();
            assert_eq!(output.accepted_sections, ["plan_tasks"]);
            assert!(output.issues.is_empty());
            {
                let staging = actor.goal_plan_staging.lock().unwrap();
                let staging = staging.as_ref().expect("staging exists");
                assert_eq!(staging.accepted_sections.len(), 1);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn slash_goal_pause_invalidates_planner_staging() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, lease) = planning_actor().await;
            let stage = StageToken(lease.stage_id);
            // Accepted sections plus a prior subagent id, as a failed
            // planner stage leaves behind.
            submit(&actor, stage, plan_tasks()).await.unwrap();
            actor
                .handle_goal_stage_completed(GoalStageCompletion {
                    lease,
                    subagent_id: Some("prior-planner-1".into()),
                    kind: GoalStageKind::Planner(Err("planner infra failure".into())),
                })
                .await;
            assert!(actor.goal_plan_staging.lock().unwrap().is_some());

            // Drive the real slash path: `/goal pause` resolves to
            // `BuiltinAction::GoalPause` in slash_exec.rs, which pauses the
            // tracker, cancels the stage, invalidates the staging, and
            // emits GoalUpdated.
            let result = actor
                .execute_builtin_slash_command(
                    crate::session::slash_commands::BuiltinAction::GoalPause,
                )
                .await;
            assert!(result.is_ok(), "slash pause must complete: {result:?}");

            {
                let tracker = actor.goal_tracker.lock();
                let goal = tracker.snapshot().expect("goal exists");
                assert_eq!(goal.status, GoalStatus::Paused);
            }
            assert!(
                actor.goal_plan_staging.lock().unwrap().is_none(),
                "/goal pause must invalidate planner staging"
            );
            let goal = actor.goal_tracker.lock().snapshot().cloned().expect("goal");
            assert!(
                SessionActor::planner_resume_for(&actor, &goal).is_none(),
                "a paused goal must not resume from stale staging"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn stale_staging_is_dropped_by_the_resume_decision() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _lease) = planning_actor().await;
            // Plant staging whose plan_revision no longer matches the goal
            // (simulating an edit/replan that advanced the revision).
            *actor.goal_plan_staging.lock().unwrap() = Some(GoalPlanStaging {
                goal_id: "goal-1".into(),
                plan_revision: 999,
                accepted_sections: vec![plan_tasks()],
                prior_subagent_id: Some("prior".into()),
                last_error: Some("failed".into()),
            });
            let goal = actor.goal_tracker.lock().snapshot().cloned().expect("goal");
            assert!(
                SessionActor::planner_resume_for(&actor, &goal).is_none(),
                "a revision mismatch must not resume"
            );
            assert!(
                actor.goal_plan_staging.lock().unwrap().is_none(),
                "stale staging is dropped so it can never be misused"
            );
        })
        .await;
}

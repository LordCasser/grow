//! Actor-level coverage for the B1 GoalStage refactor: the turn-end drain
//! must stay mailbox-fast (`completed: true` proposals schedule a background
//! verification stage instead of awaiting the model work inline), and a
//! stage completion that arrives after the goal was paused / cleared is
//! lease-dropped by the mailbox commit.
//!
//! Same single-thread + LocalSet pattern as `goal_planner_e2e_tests`.

use super::support::*;
use super::*;
use std::sync::Arc as StdArc;
use std::sync::atomic::{AtomicUsize, Ordering as SeqOrd};
use tempfile::TempDir;
use test_support::sse::responses_api_script_exact;
use tools::implementations::grow_build::task::types::{SubagentEvent, SubagentResult};
use tools::implementations::grow_build::update_goal::{
    RejectReason, UpdateGoalAck, UpdateGoalEnvelope, UpdateGoalInput,
};

/// Coordinator stub for the verification stage: on each Spawn, parse the
/// `{VERDICT_FILE}` / `{DETAILS_FILE}` paths out of the rendered prompt,
/// write a verdict (`refuted:false` → Achieved, `refuted:true` →
/// NotAchieved; inline `details_md`), optionally block on `release`, then
/// reply with the "Not Refuted" terminal token.
fn spawn_verifier_coordinator(
    mut release: Option<tokio::sync::oneshot::Receiver<()>>,
    refuted: bool,
) -> (
    tokio::sync::mpsc::UnboundedSender<SubagentEvent>,
    StdArc<AtomicUsize>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SubagentEvent>();
    let spawn_count = StdArc::new(AtomicUsize::new(0));
    let count_task = StdArc::clone(&spawn_count);
    tokio::task::spawn_local(async move {
        while let Some(ev) = rx.recv().await {
            if let SubagentEvent::Spawn(req) = ev {
                count_task.fetch_add(1, SeqOrd::SeqCst);
                if let Some(verdict_path) =
                    crate::session::goal_classifier::parse_verdict_path_from_prompt(&req.prompt)
                {
                    let _ = tokio::fs::write(
                        &verdict_path,
                        &format!(
                            "{{\"refuted\":{refuted},\"evidence\":\"diff hunk src/foo.rs:1\",\
                             \"confidence\":\"medium\",\"details_md\":\"# Skeptic\\n\\nlooks good\"}}"
                        ),
                    )
                    .await;
                }
                if let Some(details_path) =
                    crate::session::goal_classifier::parse_skeptic_details_path_from_prompt(
                        &req.prompt,
                    )
                {
                    let _ =
                        tokio::fs::write(&details_path, "# Skeptic details\nnot refuted body\n")
                            .await;
                }
                // Hold the stage's model work until the test releases it.
                // `take()` (not `as_mut()`): awaiting `&mut Receiver` does
                // not compile — `Receiver` contains `UnsafeCell`, so it is
                // not `UnwindSafe` and the std `Future for &mut F` blanket
                // impl does not apply. The tests spawn exactly one skeptic,
                // so consuming the release channel is lossless (dropping the
                // sender also releases).
                if let Some(release) = release.take() {
                    let _ = release.await;
                }
                let result = SubagentResult {
                    success: true,
                    output: StdArc::from("Not Refuted"),
                    subagent_id: req.id.clone(),
                    child_session_id: req.id.clone(),
                    ..Default::default()
                };
                let _ = req.result_tx.send(result);
            }
        }
    });
    (tx, spawn_count)
}

/// Build a `SessionActor` with the goal harness + classifier enabled, a
/// fresh tracker under a unique tempdir, and the supplied coordinator.
async fn make_stage_actor(
    coordinator_tx: Option<tokio::sync::mpsc::UnboundedSender<SubagentEvent>>,
) -> (
    StdArc<SessionActor>,
    TempDir,
    tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
) {
    let tmp = TempDir::new().expect("tempdir");
    let (gateway_tx, _gateway_rx) =
        tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, _persistence_rx) =
        tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    let (mut actor, event_rx) =
        create_test_actor_ex(0, 256_000, 85, gateway_tx, persistence_tx).await;
    actor.events = crate::session::events::EventTracker::new(tmp.path());
    actor.goal_enabled = true;
    actor.goal_classifier_enabled = true;
    set_goal_harness_for_tests(&actor);
    actor.goal_tracker = StdArc::new(parking_lot::Mutex::new(
        crate::session::goal_tracker::GoalTracker::new(tmp.path().to_path_buf()),
    ));
    if let Some(tx) = coordinator_tx {
        actor.tool_context.subagent_event_tx = Some(tx);
    }
    (StdArc::new(actor), tmp, event_rx)
}

fn create_test_goal(actor: &SessionActor) {
    actor.goal_tracker.lock().create_goal(
        "g-stage-test".into(),
        "stage test objective".into(),
        None,
        0,
        "2026-01-01T00:00:00Z".into(),
        None,
    );
}

/// Envelope for a `completed: true` update with a live ack channel.
fn completed_envelope() -> (
    UpdateGoalEnvelope,
    tokio::sync::oneshot::Receiver<UpdateGoalAck>,
) {
    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<UpdateGoalAck>();
    (
        (
            UpdateGoalInput {
                completed: Some(true),
                message: Some("done".into()),
                blocked_reason: None,
            },
            ack_tx,
        ),
        ack_rx,
    )
}

/// Drain a `completed: true` proposal at turn-end and require the drain to
/// return promptly while the verification stage is still running (B1: the
/// mailbox must not block on verification model work). Returns the live ack
/// receiver, which must stay unresolved until the mailbox commit.
async fn drain_completed_and_expect_stage_in_flight(
    actor: &StdArc<SessionActor>,
) -> tokio::sync::oneshot::Receiver<UpdateGoalAck> {
    let current_tokens = actor.chat_state_handle.get_total_tokens().await as i64;
    let (envelope, mut ack_rx) = completed_envelope();
    let drain = StdArc::clone(actor);
    tokio::time::timeout(std::time::Duration::from_millis(500), async move {
        drain
            .drain_goal_updates_with_extra(current_tokens, DrainPurpose::TurnEnd, vec![envelope])
            .await;
    })
    .await
    .expect("turn-end drain must return promptly instead of awaiting verification");
    assert!(
        actor
            .goal_classifier_in_flight
            .load(std::sync::atomic::Ordering::SeqCst),
        "in-flight CAS must be held while the verification stage runs"
    );
    assert!(
        actor
            .goal_tracker
            .lock()
            .snapshot()
            .expect("goal must still exist")
            .verifying_in_flight,
        "verifying latch must be set while the verification stage runs"
    );
    assert!(
        ack_rx.try_recv().is_err(),
        "the tool ack must not resolve until the mailbox commits the stage"
    );
    ack_rx
}

/// Wait (bounded) until the verification stage has spawned its skeptic —
/// the stage does async setup (tool names, prompt render) before the spawn,
/// so the spawn count may lag the drain's return.
async fn wait_for_stage_spawn(spawn_count: &AtomicUsize) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if spawn_count.load(SeqOrd::SeqCst) >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the verification stage must spawn its skeptic");
}

/// Receive the stage's completion event and hand it to the mailbox commit
/// handler (driving the run_loop event arm's behavior directly). Intermediate
/// notifications (e.g. the `/goal pause` AgentMessageChunk system log) are
/// filtered so the commit runs against the stage completion alone.
async fn commit_next_stage_event(
    actor: &StdArc<SessionActor>,
    event_rx: &mut tokio::sync::mpsc::UnboundedReceiver<SessionEvent>,
) {
    let completion = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match event_rx.recv().await {
                Some(SessionEvent::GoalStageCompleted(completion)) => return completion,
                Some(_other) => continue,
                None => panic!("event channel closed"),
            }
        }
    })
    .await
    .expect("stage completion event must arrive");
    actor.handle_goal_stage_completed(completion).await;
}

/// The happy path: a `completed: true` proposal schedules a background
/// verification stage; when the stage finishes, the mailbox commits the
/// Achieved verdict, completes the goal and resolves the tool ack.
#[tokio::test(flavor = "current_thread")]
async fn verifier_stage_commits_achieved_when_lease_valid() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let (coordinator_tx, spawn_count) = spawn_verifier_coordinator(Some(release_rx), false);
            let (actor, _tmp, mut event_rx) = make_stage_actor(Some(coordinator_tx)).await;
            *actor.agent.borrow_mut() = test_agent_with_goal_tool().await;
            create_test_goal(&actor);

            let ack_rx = drain_completed_and_expect_stage_in_flight(&actor).await;
            wait_for_stage_spawn(&spawn_count).await;
            assert_eq!(
                spawn_count.load(SeqOrd::SeqCst),
                1,
                "exactly one skeptic spawn for the stage"
            );

            // Release the stage; its completion arrives against an
            // unchanged Active goal → the lease holds → the mailbox commits
            // the Achieved verdict.
            drop(release_tx);
            commit_next_stage_event(&actor, &mut event_rx).await;

            assert_eq!(
                actor.goal_tracker.lock().status(),
                Some(crate::session::goal_tracker::GoalStatus::Complete),
                "a valid-lease Achieved verdict must complete the goal"
            );
            assert!(
                !actor
                    .goal_classifier_in_flight
                    .load(std::sync::atomic::Ordering::SeqCst),
                "in-flight CAS must be released after the commit"
            );
            assert!(
                !actor
                    .goal_tracker
                    .lock()
                    .snapshot()
                    .expect("completed goal still has an orchestration")
                    .verifying_in_flight,
                "verifying latch must be cleared after the stage finishes"
            );
            let ack = tokio::time::timeout(std::time::Duration::from_secs(2), ack_rx)
                .await
                .expect("commit must resolve the tool ack")
                .expect("ack channel must be live");
            match ack {
                UpdateGoalAck::ClassifierAchieved { details_path } => {
                    assert!(
                        !details_path.is_empty(),
                        "the achieved ack points at the details file"
                    );
                }
                other => panic!("expected ClassifierAchieved ack, got {other:?}"),
            }
        })
        .await;
}

/// While the verification stage is held mid-model-work, `/goal pause` and
/// the `/compact` admission must be accepted promptly (the mailbox is not
/// blocked on verification). The stage's completion then arrives against a
/// paused goal and is lease-dropped: the goal stays paused and the tool
/// ack is resolved as `Rejected(StatusChangedDuringClassifier)`.
#[tokio::test(flavor = "current_thread")]
async fn mailbox_commands_accepted_while_stage_runs_and_pause_drops_completion() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let (coordinator_tx, spawn_count) = spawn_verifier_coordinator(Some(release_rx), false);
            let (actor, _tmp, mut event_rx) = make_stage_actor(Some(coordinator_tx)).await;
            *actor.agent.borrow_mut() = test_agent_with_goal_tool().await;
            create_test_goal(&actor);

            let ack_rx = drain_completed_and_expect_stage_in_flight(&actor).await;
            wait_for_stage_spawn(&spawn_count).await;
            assert_eq!(
                spawn_count.load(SeqOrd::SeqCst),
                1,
                "exactly one skeptic spawn for the stage"
            );

            // The verification stage is held. `/goal pause` must be accepted
            // within a second (the mailbox is not queued behind verification).
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                actor
                    .clone()
                    .execute_out_of_band_slash_command("/goal pause".to_string())
                    .await
                    .expect("pause command must be accepted");
            })
            .await
            .expect("pause must be accepted while the verification stage runs");
            assert_eq!(
                actor.goal_tracker.lock().status(),
                Some(crate::session::goal_tracker::GoalStatus::UserPaused)
            );

            // A `/compact` admission must also be accepted promptly while the
            // stage still runs (here: behind a running turn → Scheduled).
            {
                let mut state = actor.state.lock().await;
                state.running_task = Some(running_task_stub("running"));
            }
            let (compact_tx, compact_rx) = tokio::sync::oneshot::channel();
            let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                actor
                    .admit_manual_compaction(None, completion_tx, Some(compact_tx))
                    .await;
            })
            .await
            .expect("compact admission must be accepted while the verification stage runs");
            assert_eq!(
                compact_rx
                    .await
                    .expect("compact admission must respond")
                    .expect("compact admission must succeed"),
                crate::session::CompactConversationStatus::Scheduled
            );

            // Release the stage. Its completion arrives against a paused
            // goal → the lease is dead → the outcome is dropped.
            drop(release_tx);
            commit_next_stage_event(&actor, &mut event_rx).await;

            assert_eq!(
                actor.goal_tracker.lock().status(),
                Some(crate::session::goal_tracker::GoalStatus::UserPaused),
                "a paused goal must not be completed by a stale stage"
            );
            assert!(
                !actor
                    .goal_classifier_in_flight
                    .load(std::sync::atomic::Ordering::SeqCst),
                "in-flight CAS must be released even for a dropped completion"
            );
            let ack = tokio::time::timeout(std::time::Duration::from_secs(2), ack_rx)
                .await
                .expect("commit must resolve the tool ack")
                .expect("ack channel must be live");
            match ack {
                UpdateGoalAck::Rejected {
                    reason: RejectReason::StatusChangedDuringClassifier,
                    ..
                } => {}
                other => panic!("expected Rejected(StatusChangedDuringClassifier), got {other:?}"),
            }
        })
        .await;
}

/// A stage completion arriving after the goal was cleared is lease-dropped:
/// the goal stays cleared, the in-flight CAS is released and the tool ack
/// is resolved as `Rejected(StatusChangedDuringClassifier)`.
#[tokio::test(flavor = "current_thread")]
async fn stage_completion_after_clear_is_lease_dropped() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let (coordinator_tx, spawn_count) = spawn_verifier_coordinator(Some(release_rx), false);
            let (actor, _tmp, mut event_rx) = make_stage_actor(Some(coordinator_tx)).await;
            *actor.agent.borrow_mut() = test_agent_with_goal_tool().await;
            create_test_goal(&actor);

            let ack_rx = drain_completed_and_expect_stage_in_flight(&actor).await;
            wait_for_stage_spawn(&spawn_count).await;
            assert_eq!(
                spawn_count.load(SeqOrd::SeqCst),
                1,
                "exactly one skeptic spawn for the stage"
            );

            // The goal is cleared while the stage runs.
            actor.goal_tracker.lock().clear();
            assert!(
                actor.goal_tracker.lock().snapshot().is_none(),
                "clear must remove the orchestration"
            );

            drop(release_tx);
            commit_next_stage_event(&actor, &mut event_rx).await;

            assert!(
                actor.goal_tracker.lock().snapshot().is_none(),
                "a cleared goal must stay cleared (stale stage must not recreate it)"
            );
            assert!(
                !actor
                    .goal_classifier_in_flight
                    .load(std::sync::atomic::Ordering::SeqCst),
                "in-flight CAS must be released even for a dropped completion"
            );
            let ack = tokio::time::timeout(std::time::Duration::from_secs(2), ack_rx)
                .await
                .expect("commit must resolve the tool ack")
                .expect("ack channel must be live");
            match ack {
                UpdateGoalAck::Rejected {
                    reason: RejectReason::StatusChangedDuringClassifier,
                    ..
                } => {}
                other => panic!("expected Rejected(StatusChangedDuringClassifier), got {other:?}"),
            }
        })
        .await;
}

/// Full non-workflow Goal actor for `handle_prompt` integration tests: mock
/// inference server + sampler + gateway/persistence drainers + bound local
/// session. The actor owns a planned Active goal (`plan_file` set) so the
/// GoalSummary branch skips the planner. The caller enqueues model responses
/// on `server` before driving a turn.
async fn spawn_implementer_actor(
    server: &test_support::MockInferenceServer,
) -> StdArc<SessionActor> {
    let (sampler_event_tx, sampler_event_rx) =
        tokio::sync::mpsc::unbounded_channel::<sampler::SamplingEvent>();
    let sampling_cfg = sampler::SamplerConfig {
        api_key: Some("test-key".to_string()),
        base_url: server.url(),
        model: "test".to_string(),
        api_backend: sampler::ApiBackend::Responses,
        context_window: 256_000,
        max_retries: Some(0),
        idle_timeout_secs: Some(30),
        ..Default::default()
    };
    let sampler_handle = sampler::SamplerActor::spawn(
        sampling_cfg,
        sampler::RetryPolicy {
            max_retries: 0,
            rate_limit_retry_threshold: 0,
            ..Default::default()
        },
        sampler_event_tx,
    );

    // The turn completion path awaits the gateway notification ack and the
    // persistence FlushAndAck barrier; without these drainers `handle_prompt`
    // never returns.
    let (gateway_tx, mut gateway_rx) =
        tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    tokio::task::spawn_local(async move {
        while let Some(msg) = gateway_rx.recv().await {
            if let acp_transport::AcpClientMessage::SessionNotification(args) = msg {
                let _ = args.response_tx.send(Ok(()));
            }
        }
    });
    let (persistence_tx, mut persistence_rx) =
        tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    tokio::task::spawn_local(async move {
        while let Some(msg) = persistence_rx.recv().await {
            if let PersistenceMsg::FlushAndAck { respond_to } = msg {
                let _ = respond_to.send(());
            }
        }
    });
    let mut actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
    actor.sampler_handle = sampler_handle;
    actor.events = crate::session::events::EventTracker::new(std::path::Path::new("/tmp"));
    actor.goal_enabled = true;
    actor.goal_classifier_enabled = true;
    set_goal_harness_for_tests(&actor);
    actor.goal_tracker = StdArc::new(parking_lot::Mutex::new(
        crate::session::goal_tracker::GoalTracker::new(std::path::PathBuf::from("/tmp")),
    ));
    create_test_goal(&actor);
    {
        let mut tracker = actor.goal_tracker.lock();
        let mut snapshot = tracker.snapshot_mut().unwrap();
        // A planned Goal: the GoalSummary branch skips the planner and
        // renders the continuation.
        snapshot.plan_file = Some(std::path::PathBuf::from("/tmp/goal/plan.md"));
    }
    actor
        .behavior
        .lock()
        .select_behavior(Some(tool_types::BehaviorId::Goal));
    *actor.current_prompt_mode.lock() = crate::session::behavior::PromptMode::Goal;
    *actor.agent.borrow_mut() = test_agent_with_goal_tool().await;

    let mut cfg = actor
        .chat_state_handle
        .get_sampling_config()
        .await
        .expect("test actor has sampling config");
    cfg.base_url = server.url();
    cfg.api_backend = sampling_types::ApiBackend::Responses;
    cfg.model = "test".to_string();
    actor.chat_state_handle.update_sampling_config(cfg);
    let mut creds = actor.chat_state_handle.get_credentials().await;
    creds.api_key = Some("test-key".to_string());
    actor.chat_state_handle.update_credentials(creds);
    actor
        .workspace_ops
        .bind_local_session(
            &actor.session_id_string(),
            actor.tool_context.cwd.as_path().to_path_buf(),
            actor.tool_context.hunk_tracker_handle.clone(),
            actor.agent.borrow().tool_bridge().toolset(),
            None,
        )
        .expect("bind_local_session");

    let actor = StdArc::new(actor);
    {
        let drainer = StdArc::clone(&actor);
        tokio::task::spawn_local(async move {
            let mut sampler_event_rx = sampler_event_rx;
            while let Some(event) = sampler_event_rx.recv().await {
                drainer.handle_sampling_event(event).await;
            }
        });
    }
    actor
}

/// A3 config lock — non-workflow side. A foreground Goal prompt is exactly
/// one implementer cycle: the model completes one round, the turn loop must
/// NOT continue in-turn (`run_goal_round_end` is the workflow engine's
/// evaluator and never runs here), and the turn-end hook schedules the next
/// cycle as a fresh GoalSummary prompt whose blocks are the rendered
/// continuation directive — never the queue placeholder.
///
/// The prompt is driven as a `goal-summary-*` turn with the command-plane
/// placeholder blocks and an already-planned Goal, which also locks W7: the
/// `handle_prompt` GoalSummary branch replaces the placeholder with the
/// directive rendered by `render_goal_continuation` (same source as the
/// turn-end continuation) before the model request is built.
#[test]
fn non_workflow_foreground_runs_single_implementer_cycle() {
    let handle = std::thread::Builder::new()
        .name("non_workflow_foreground_runs_single_implementer_cycle".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let local = tokio::task::LocalSet::new();
                    local
                        .run_until(async {
                            let server = test_support::MockInferenceServer::start()
                                .await
                                .expect("mock inference server");
                            server.enqueue_response(
                                "/v1/responses",
                                test_support::ScriptedResponse::sse(responses_api_script_exact(
                                    "implementer round done",
                                    "test",
                                )),
                            );

                            let actor = spawn_implementer_actor(&server).await;

                            assert!(
                                !actor.goal_runs_on_workflow_engine(),
                                "the default test actor is the non-workflow foreground path"
                            );

                            let outcome = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                actor.handle_prompt(
                                    "goal-summary-single-cycle",
                                    vec![acp::ContentBlock::Text(acp::TextContent::new(
                                        super::slash_exec::GOAL_CYCLE_PLACEHOLDER,
                                    ))],
                                    crate::session::behavior::PromptMode::Goal,
                                    None,
                                    None,
                                    true,
                                    None,
                                    None,
                                    None,
                                ),
                            )
                            .await
                            .expect("implementer turn must finish");
                            assert!(outcome.is_ok(), "turn must not error: {outcome:?}");

                            let model_requests = server
                                .requests()
                                .iter()
                                .filter(|e| e.path == "/v1/responses")
                                .count();
                            assert_eq!(
                                model_requests, 1,
                                "non-workflow foreground must be exactly one implementer \
                                 cycle; an in-turn Continue would drive a second request"
                            );

                            // W7: the placeholder was replaced by the rendered
                            // continuation directive before the model input was
                            // committed.
                            let conversation = actor.chat_state_handle.get_conversation().await;
                            let joined: String = conversation
                                .iter()
                                .map(|item| item.text_content())
                                .collect();
                            assert!(
                                joined.contains("stage test objective"),
                                "the continuation directive rendered in the turn must carry \
                                 the objective, got: {joined:?}"
                            );
                            assert!(
                                !joined.contains(super::slash_exec::GOAL_CYCLE_PLACEHOLDER),
                                "the queue placeholder must never reach the conversation"
                            );

                            // Turn-end hook (event loop): the next cycle is a
                            // fresh GoalSummary prompt, not an in-turn Continue.
                            actor.handle_turn_end(true, false).await;
                            let state = actor.state.lock().await;
                            assert_eq!(state.pending_inputs.len(), 1);
                            assert_eq!(
                                state.pending_inputs[0].origin,
                                crate::session::PromptOrigin::GoalSummary
                            );
                            let queued_text = state.pending_inputs[0]
                                .prompt_blocks
                                .iter()
                                .find_map(|block| match block {
                                    acp::ContentBlock::Text(text) => Some(text.text.as_str()),
                                    _ => None,
                                })
                                .expect("Goal-cycle text");
                            assert!(
                                queued_text.contains("stage test objective"),
                                "the queued next cycle must carry the rendered directive, \
                                 got: {queued_text:?}"
                            );
                            assert_ne!(queued_text, super::slash_exec::GOAL_CYCLE_PLACEHOLDER);
                        })
                        .await;
                })
        })
        .expect("spawn test thread");
    handle.join().expect("test thread panicked");
}

/// A3 config lock — workflow side. With `background_workflows_enabled` the
/// Goal turn keeps `run_goal_round_end` as its in-turn evaluator: calling it
/// on an Active goal reaches `evaluate_goal_round` (observable here as the
/// bounded evaluator failure auto-pausing the goal with
/// `GoalPauseReason::Infra`), which the non-workflow path never does.
#[tokio::test(flavor = "current_thread")]
async fn workflow_engine_path_keeps_in_turn_round_end_evaluator() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (mut actor, _tmp, _event_rx) = make_stage_actor(None).await;
            *actor.agent.borrow_mut() = test_agent_with_goal_tool().await;
            create_test_goal(&actor);

            assert!(
                !actor.goal_runs_on_workflow_engine(),
                "the default test actor is the non-workflow foreground path"
            );
            StdArc::get_mut(&mut actor)
                .expect("actor must be uniquely owned")
                .background_workflows_enabled = true;
            assert!(
                actor.goal_runs_on_workflow_engine(),
                "the workflow engine must flip the goal round-end path"
            );

            // No sampler is wired in this test actor, so the in-turn
            // evaluator fails fast (bounded retries); the workflow path's
            // failure handling pauses the goal and ends the turn.
            let decision = actor.run_goal_round_end().await;
            assert!(
                matches!(decision, GoalRoundDecision::EndTurn),
                "workflow round-end must end the turn when the evaluator fails bounded"
            );
            assert_eq!(
                actor.goal_tracker.lock().status(),
                Some(crate::session::goal_tracker::GoalStatus::InfraPaused),
                "run_goal_round_end must run the in-turn evaluator on the workflow path"
            );
        })
        .await;
}

/// Cycle-boundary gate (Issue 1): while a verification stage is in flight the
/// turn-end hook must NOT queue the next implementer cycle — the pending
/// verdict owns that decision. After an `Achieved` commit the goal is
/// Complete and no continuation is queued.
#[tokio::test(flavor = "current_thread")]
async fn turn_end_gates_continuation_while_verifier_in_flight() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let (coordinator_tx, spawn_count) = spawn_verifier_coordinator(Some(release_rx), false);
            let (actor, _tmp, mut event_rx) = make_stage_actor(Some(coordinator_tx)).await;
            *actor.agent.borrow_mut() = test_agent_with_goal_tool().await;
            create_test_goal(&actor);

            let ack_rx = drain_completed_and_expect_stage_in_flight(&actor).await;
            wait_for_stage_spawn(&spawn_count).await;

            // The implementer turn ends while the verifier stage runs: the
            // next cycle must be held back (no GoalSummary queued), and the
            // in-flight CAS stays held.
            actor.clone().handle_turn_end(true, false).await;
            {
                let state = actor.state.lock().await;
                assert!(
                    !state
                        .pending_inputs
                        .iter()
                        .any(|i| { matches!(i.origin, crate::session::PromptOrigin::GoalSummary) }),
                    "no next cycle may be queued while the verifier stage is in flight"
                );
            }
            assert!(
                actor
                    .goal_classifier_in_flight
                    .load(std::sync::atomic::Ordering::SeqCst),
                "in-flight CAS must stay held while the stage runs"
            );

            // Achieved commit: goal completes, no continuation is queued.
            drop(release_tx);
            commit_next_stage_event(&actor, &mut event_rx).await;
            assert_eq!(
                actor.goal_tracker.lock().status(),
                Some(crate::session::goal_tracker::GoalStatus::Complete),
                "an Achieved verdict must complete the goal"
            );
            {
                let state = actor.state.lock().await;
                assert!(
                    !state
                        .pending_inputs
                        .iter()
                        .any(|i| { matches!(i.origin, crate::session::PromptOrigin::GoalSummary) }),
                    "an Achieved commit must not queue a continuation"
                );
            }
            let ack = tokio::time::timeout(std::time::Duration::from_secs(2), ack_rx)
                .await
                .expect("commit must resolve the tool ack")
                .expect("ack channel must be live");
            assert!(
                matches!(ack, UpdateGoalAck::ClassifierAchieved { .. }),
                "expected ClassifierAchieved ack, got {ack:?}"
            );
        })
        .await;
}

/// Cycle-boundary gate (Issue 1), NotAchieved side: after the stage rejects
/// the completion while the goal is still Active, exactly one next cycle is
/// queued — carrying the post-verification state.
#[tokio::test(flavor = "current_thread")]
async fn verifier_not_achieved_requeues_next_cycle() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let (coordinator_tx, spawn_count) = spawn_verifier_coordinator(Some(release_rx), true);
            let (actor, _tmp, mut event_rx) = make_stage_actor(Some(coordinator_tx)).await;
            *actor.agent.borrow_mut() = test_agent_with_goal_tool().await;
            create_test_goal(&actor);

            let ack_rx = drain_completed_and_expect_stage_in_flight(&actor).await;
            wait_for_stage_spawn(&spawn_count).await;

            // Turn end while the stage runs: gated (no premature cycle).
            actor.clone().handle_turn_end(true, false).await;
            {
                let state = actor.state.lock().await;
                assert!(
                    !state
                        .pending_inputs
                        .iter()
                        .any(|i| { matches!(i.origin, crate::session::PromptOrigin::GoalSummary) }),
                    "no next cycle while the verifier stage is in flight"
                );
            }

            // Refuted (NotAchieved) commit while still Active: the next
            // finite implementer cycle is queued exactly once.
            drop(release_tx);
            commit_next_stage_event(&actor, &mut event_rx).await;
            assert_eq!(
                actor.goal_tracker.lock().status(),
                Some(crate::session::goal_tracker::GoalStatus::Active),
                "a NotAchieved verdict keeps the goal Active"
            );
            {
                let state = actor.state.lock().await;
                let goal_summaries: Vec<_> = state
                    .pending_inputs
                    .iter()
                    .filter(|i| matches!(i.origin, crate::session::PromptOrigin::GoalSummary))
                    .collect();
                assert_eq!(
                    goal_summaries.len(),
                    1,
                    "a NotAchieved commit must queue exactly one next cycle"
                );
            }
            let ack = tokio::time::timeout(std::time::Duration::from_secs(2), ack_rx)
                .await
                .expect("commit must resolve the tool ack")
                .expect("ack channel must be live");
            assert!(
                matches!(ack, UpdateGoalAck::ClassifierNotAchieved { .. }),
                "expected ClassifierNotAchieved ack, got {ack:?}"
            );
        })
        .await;
}

/// Gate 2: a user prompt under a paused Goal is exactly one finite
/// interaction turn — the model answers, the turn ends idle, the Goal stays
/// paused, and NO autonomous cycle is queued by the turn-end hook.
#[test]
fn paused_goal_user_prompt_runs_finite_interaction_without_continuation() {
    let handle = std::thread::Builder::new()
        .name("paused_goal_user_prompt_runs_finite_interaction_without_continuation".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let local = tokio::task::LocalSet::new();
                    local
                        .run_until(async {
                            let server = test_support::MockInferenceServer::start()
                                .await
                                .expect("mock inference server");
                            server.enqueue_response(
                                "/v1/responses",
                                test_support::ScriptedResponse::sse(responses_api_script_exact(
                                    "user interaction answered",
                                    "test",
                                )),
                            );
                            let actor = spawn_implementer_actor(&server).await;
                            assert!(
                                actor
                                    .goal_tracker
                                    .lock()
                                    .pause(crate::session::goal_tracker::GoalPauseReason::User),
                                "the goal must be paused before the interaction"
                            );

                            // The user's ordinary message runs as a finite
                            // interaction turn (paused gate was removed).
                            let outcome = tokio::time::timeout(
                                std::time::Duration::from_secs(30),
                                actor.handle_prompt(
                                    "user-paused-interaction",
                                    vec![acp::ContentBlock::Text(acp::TextContent::new(
                                        "What is the current state of this goal?",
                                    ))],
                                    crate::session::behavior::PromptMode::Goal,
                                    None,
                                    None,
                                    true,
                                    None,
                                    None,
                                    None,
                                ),
                            )
                            .await
                            .expect("interaction turn must finish");
                            assert!(outcome.is_ok(), "interaction must not error: {outcome:?}");

                            let model_requests = server
                                .requests()
                                .iter()
                                .filter(|e| e.path == "/v1/responses")
                                .count();
                            assert_eq!(
                                model_requests, 1,
                                "a paused interaction is exactly one finite turn"
                            );

                            // The Goal stays paused; no continuation is queued.
                            assert_eq!(
                                actor.goal_tracker.lock().status(),
                                Some(crate::session::goal_tracker::GoalStatus::UserPaused),
                                "the Goal must remain paused after the interaction"
                            );
                            {
                                let state = actor.state.lock().await;
                                assert!(
                                    state.running_task.is_none(),
                                    "the interaction turn must end idle"
                                );
                                assert!(
                                    !state.pending_inputs.iter().any(|i| {
                                        matches!(
                                            i.origin,
                                            crate::session::PromptOrigin::GoalSummary
                                        )
                                    }),
                                    "a paused interaction must not queue a Goal cycle"
                                );
                            }

                            // Turn-end hook: a paused goal never auto-continues.
                            actor.clone().handle_turn_end(true, false).await;
                            {
                                let state = actor.state.lock().await;
                                assert!(
                                    !state.pending_inputs.iter().any(|i| {
                                        matches!(
                                            i.origin,
                                            crate::session::PromptOrigin::GoalSummary
                                        )
                                    }),
                                    "the turn-end hook must not schedule a cycle for a paused Goal"
                                );
                            }
                        })
                        .await;
                })
        })
        .expect("spawn test thread");
    handle.join().expect("test thread panicked");
}

/// Gate 3 (shell side): `/goal pause` ends the foreground implementer turn —
/// `cancel_turn_for_goal_pause` tears down the running task and emits the
/// durable `TurnCompleted` terminal (cancelled) that lets the Pager leave
/// "LLM Response". Background tasks are preserved (no kill flag).
#[tokio::test(flavor = "current_thread")]
async fn goal_pause_cancels_turn_and_emits_durable_terminal() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _tmp, _event_rx) = make_stage_actor(None).await;
            *actor.agent.borrow_mut() = test_agent_with_goal_tool().await;
            create_test_goal(&actor);
            assert!(
                actor
                    .goal_tracker
                    .lock()
                    .pause(crate::session::goal_tracker::GoalPauseReason::User),
                "the goal must be paused first (slash order: state then cancel)"
            );

            // An implementer turn is in flight.
            *actor
                .current_prompt_id
                .lock()
                .expect("current_prompt_id mutex poisoned") = Some("user-implementer".to_string());
            {
                let mut state = actor.state.lock().await;
                state.running_task = Some(crate::session::acp_session::AgentTask {
                    prompt_id: "user-implementer".into(),
                    turn_start_ms: 0,
                    handle: tokio::task::spawn_local(async {
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    })
                    .abort_handle(),
                });
            }

            let mut replay_buffer = crate::agent::update_chunk_merge::ReplayBuffer::new(None);
            actor.cancel_turn_for_goal_pause(&mut replay_buffer).await;

            let state = actor.state.lock().await;
            assert!(
                state.running_task.is_none(),
                "the foreground implementer turn must be cancelled"
            );
            assert!(
                state
                    .recent_terminals
                    .iter()
                    .any(|t| t.prompt_id == "user-implementer" && t.stop_reason == "cancelled"),
                "a durable cancelled terminal must be emitted for the paused turn \
                 (the Pager's first-wins finalizer exits LLM Response on it)"
            );
            // Pause preserves background tasks: the turn cancel ran with
            // `kill_background_tasks = false`; any background reservation
            // survives as a deferred completion (asserted by the
            // cancel-while-wait suite).
        })
        .await;
}

/// Interruptibility (sampling): a user steering / send-now soft-preempts the
/// Goal implementer's in-flight sampler request — the request is cancelled
/// and `run_turn_via_sampler` returns `Steered` so the main agent can
/// immediately process the user message (the turn loop rebuilds the request).
#[test]
fn goal_sampling_soft_preempted_by_steering() {
    let handle = std::thread::Builder::new()
        .name("goal_sampling_soft_preempted_by_steering".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let local = tokio::task::LocalSet::new();
                    local
                        .run_until(async {
                            let server = test_support::MockInferenceServer::start()
                                .await
                                .expect("mock inference server");
                            let actor = spawn_implementer_actor(&server).await;

                            let request = sampling_types::ConversationRequest {
                                items: vec![sampling_types::ConversationItem::user(
                                    "implement this step",
                                )],
                                ..Default::default()
                            };
                            // Steering (send-now) is queued before the sampler
                            // request submits: the sampler's `tokio::select!`
                            // (biased) prefers the pending interjection over
                            // continuing the request, so it cancels and
                            // returns `Steered` — the main agent responds to
                            // the user immediately instead of finishing the
                            // in-flight sample. (Mid-flight cancel delivery is
                            // exercised by the sampler-layer tests; this pins
                            // the steering-preempts-sampling decision.)
                            actor.pending_interjections.push(
                                crate::session::acp_session::interjection::PendingInterjection {
                                    text: "stop and answer me".into(),
                                    attachments: vec![],
                                },
                            );
                            let sampler = StdArc::clone(&actor);
                            let sampler_handle = tokio::task::spawn_local(async move {
                                sampler.run_turn_via_sampler(request).await
                            });

                            let outcome = tokio::time::timeout(
                                std::time::Duration::from_secs(10),
                                sampler_handle,
                            )
                            .await
                            .expect("sampler must return promptly after steering")
                            .expect("sampler task must not panic")
                            .expect("run_turn_via_sampler must succeed");
                            assert!(
                                matches!(
                                    outcome,
                                    crate::session::acp_session::SamplerTurnOutcome::Steered
                                ),
                                "steering must soft-preempt the Goal sampler (expected Steered)"
                            );
                        })
                        .await;
                })
        })
        .expect("spawn test thread");
    handle.join().expect("test thread panicked");
}

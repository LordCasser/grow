//! A subagent child's terminal commit is appended by its own session actor
//! after the SessionEnd lifecycle, as the child Timeline's last normal fact.
//!
//! The runner-level ordering (real child public lifecycle through the
//! subagent runner) is covered by the dedicated CLI-isolated test
//! `test_subagent_session_end_order.rs`; this file drives the session actor
//! mailbox directly so the invariant stays covered in the default test run.
//! Failure regressions cover the cancelled outcome, an expired commit
//! deadline, a lost ACK receiver, and a persistently failing SessionEnd
//! hook write.

use super::support::*;
use super::*;

fn run_with_session_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(body)
        .unwrap()
        .join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
}

/// Fixture identity shared by every commit in this file. The seed's
/// `subagent_id` must equal the committed result's or the Timeline validator
/// answers `SubagentSeedMismatch`; the remaining fields copy the legal seed
/// fixture used by the prompt-queue child tests.
const CHILD_SUBAGENT_ID: &str = "terminal-commit-child";
const PARENT_TIMELINE_ID: &str = "terminal-commit-parent";

fn child_seed() -> chat_state::SubagentSeedEvent {
    chat_state::SubagentSeedEvent {
        parent_timeline_id: PARENT_TIMELINE_ID.into(),
        parent_spawn_seq: 1,
        subagent_id: CHILD_SUBAGENT_ID.into(),
        security_parent_session_id: PARENT_TIMELINE_ID.into(),
        context_source: chat_state::SubagentContextSource::New,
        source_ref: None,
        normalized: false,
    }
}

fn fixture_result_event() -> chat_state::SubagentResultEvent {
    chat_state::SubagentResultEvent {
        subagent_id: CHILD_SUBAGENT_ID.into(),
        outcome: chat_state::SubagentOutcome::Completed,
        duration_ms: 12,
        tool_calls: 1,
        turns: 1,
        tokens_used: 34,
        error: None,
        output_ref: None,
        planning_summary: None,
    }
}

/// Every Timeline event the local persistence stub has accepted as durable.
/// Recorded before the ACK is sent, it survives a poisoned writer's exit and
/// is the durable evidence where the in-memory ChatState query is no longer
/// available.
type RecordedDurableTimeline = std::sync::Arc<parking_lot::Mutex<Vec<chat_state::TimelineEvent>>>;

/// A subagent-hint session actor running the real `run_session` loop. Uses
/// the physical projection fixture so every Timeline write crosses the local
/// persistence stub below. The child's legal `SubagentSeed` is recorded
/// durably before the loop starts, matching the real runner's entry premise:
/// the seed always precedes any prompt, output, or terminal result.
async fn terminal_commit_actor() -> (
    std::sync::Arc<SessionActor>,
    tokio::task::JoinHandle<()>,
    tokio::sync::mpsc::UnboundedSender<SessionCommand>,
    tokio::sync::mpsc::UnboundedSender<chat_state::ChatStateEvent>,
    RecordedDurableTimeline,
) {
    terminal_commit_actor_with_session_end_hook_failure(false).await
}

/// Same fixture with the stub rejecting exactly the SessionEnd hook
/// `Triggered` durable write using a permanent I/O error; the seed and every
/// other write still succeed.
async fn terminal_commit_actor_with_session_end_hook_failure(
    fail_session_end_hook_trigger: bool,
) -> (
    std::sync::Arc<SessionActor>,
    tokio::task::JoinHandle<()>,
    tokio::sync::mpsc::UnboundedSender<SessionCommand>,
    tokio::sync::mpsc::UnboundedSender<chat_state::ChatStateEvent>,
    RecordedDurableTimeline,
) {
    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    let (mut actor, event_rx) =
        create_test_actor_ex_with_physical_projection(0, 256_000, 85, gateway_tx, persistence_tx)
            .await;
    actor.startup_hints.is_subagent = true;
    let recorded: RecordedDurableTimeline =
        std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let stub_recorded = recorded.clone();
    tokio::task::spawn_local(async move {
        while let Some(message) = persistence_rx.recv().await {
            match message {
                PersistenceMsg::TimelineDurablyAndAck { event, respond_to } => {
                    let is_session_end_trigger = matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                            event: chat_state::HookEventType::SessionEnd,
                            ..
                        },)
                    );
                    if fail_session_end_hook_trigger && is_session_end_trigger {
                        let _ = respond_to.send(Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "injected session-end hook persistence failure",
                        )));
                        continue;
                    }
                    stub_recorded.lock().push(event);
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::SidebandDurablyAndAck { respond_to, .. }
                | PersistenceMsg::ReplaceRewindPointsAndAck { respond_to, .. }
                | PersistenceMsg::WriteRewindTransactionAndAck { respond_to, .. }
                | PersistenceMsg::ClearRewindTransactionAndAck { respond_to } => {
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::AppendUpdateDurablyAndAck { respond_to, .. }
                | PersistenceMsg::AppendUpdateBufferedAndAck { respond_to, .. } => {
                    let _ = respond_to.send(Ok(()));
                }
                PersistenceMsg::FlushAndAck { respond_to } => {
                    let _ = respond_to.send(());
                }
                _ => {}
            }
        }
    });
    // The Timeline validator rejects a `SubagentResult` whose child never
    // seeded (`MissingSubagentSeed`), and the real runner always records the
    // seed before the child produces anything. The stub's durable record of
    // this write is also the positive control for the evidence below.
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::SubagentSeed(child_seed()))
        .await
        .expect("the child identity seed must be durable before the session loop starts");
    assert!(
        recorded_has_durable_seed(&recorded_durable_events(&recorded)),
        "the persistence stub must record accepted durable Timeline writes"
    );
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
    let (chat_event_tx, chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
    let root = actor.session_dir.clone();
    let run_actor = std::sync::Arc::new(actor);
    let task_actor = run_actor.clone();
    let task = tokio::task::spawn_local(async move {
        super::super::run_loop::run_session(
            task_actor,
            cmd_rx,
            chat_event_rx,
            event_rx,
            None,
            std::sync::Arc::new(parking_lot::Mutex::new(CodebaseIndexManager::default())),
            root,
            Default::default(),
        )
        .await;
    });
    let (responds_to, ready) = tokio::sync::oneshot::channel();
    cmd_tx
        .send(SessionCommand::GetActiveAgent { responds_to })
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), ready)
        .await
        .expect("terminal commit mailbox startup must complete")
        .unwrap();
    (run_actor, task, cmd_tx, chat_event_tx, recorded)
}

#[test]
fn terminal_commit_records_session_end_before_the_last_canonical_result() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let (actor, loop_task, commands, _chat_events, recorded) =
                terminal_commit_actor().await;
            let result_ref =
                commit_result_and_await_exit(&commands, loop_task, fixture_result_event()).await;

            assert_eq!(result_ref.timeline_id, actor.session_id_string());
            assert_eq!(result_ref.first_seq, result_ref.last_seq);
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let result_index = assert_single_last_result(&events);
            assert_eq!(
                events[result_index].seq.get(),
                result_ref.first_seq,
                "the ACK must reference the committed result event"
            );
            assert_session_end_precedes_result(&events, result_index);
            assert_eq!(
                recorded_result_count(&recorded_durable_events(&recorded)),
                1,
                "the persistence stub must have accepted the result exactly once"
            );
        }));
    });
}

/// Drive one terminal commit to its durable ACK and the actor's full exit.
async fn commit_result_and_await_exit(
    commands: &tokio::sync::mpsc::UnboundedSender<SessionCommand>,
    loop_task: tokio::task::JoinHandle<()>,
    result: chat_state::SubagentResultEvent,
) -> chat_state::TimelineRangeRef {
    let (respond_to, ack) = tokio::sync::oneshot::channel();
    commands
        .send(SessionCommand::CommitSubagentResult {
            result,
            deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(30),
            respond_to,
        })
        .unwrap();
    let result_ref = tokio::time::timeout(std::time::Duration::from_secs(20), ack)
        .await
        .expect("the actor must ACK its terminal commit")
        .unwrap()
        .expect("the terminal commit must be durable");
    tokio::time::timeout(std::time::Duration::from_secs(10), loop_task)
        .await
        .unwrap()
        .unwrap();
    result_ref
}

/// Snapshot the durable evidence the persistence stub has accepted so far.
fn recorded_durable_events(recorded: &RecordedDurableTimeline) -> Vec<chat_state::TimelineEvent> {
    recorded.lock().clone()
}

fn recorded_has_durable_seed(events: &[chat_state::TimelineEvent]) -> bool {
    events
        .iter()
        .any(|event| matches!(&event.kind, chat_state::TimelineEventKind::SubagentSeed(_)))
}

fn recorded_result_count(events: &[chat_state::TimelineEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                chat_state::TimelineEventKind::SubagentResult(_)
            )
        })
        .count()
}

fn recorded_session_end_trigger_count(events: &[chat_state::TimelineEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                    event: chat_state::HookEventType::SessionEnd,
                    ..
                },)
            )
        })
        .count()
}

/// The canonical result is committed exactly once and is the child
/// Timeline's last fact. Returns the result's event index.
fn assert_single_last_result(events: &[chat_state::TimelineEvent]) -> usize {
    let result_positions: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            matches!(
                &event.kind,
                chat_state::TimelineEventKind::SubagentResult(_)
            )
        })
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        result_positions.len(),
        1,
        "the canonical result must be committed exactly once"
    );
    let result_index = result_positions[0];
    assert_eq!(
        result_index,
        events.len() - 1,
        "the result must be the child Timeline's last fact"
    );
    result_index
}

/// The SessionEnd occurrence is recorded exactly once before the canonical
/// result, and its `Completed` aggregate also precedes the result.
fn assert_session_end_precedes_result(events: &[chat_state::TimelineEvent], result_index: usize) {
    let session_end_count = events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                    event: chat_state::HookEventType::SessionEnd,
                    ..
                },)
            )
        })
        .count();
    assert_eq!(
        session_end_count, 1,
        "SessionEnd must be recorded exactly once"
    );
    let (hook_index, occurrence_id) = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match &event.kind {
            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                occurrence_id,
                event: chat_state::HookEventType::SessionEnd,
                ..
            }) => Some((index, occurrence_id.clone())),
            _ => None,
        })
        .expect("the SessionEnd occurrence must be recorded");
    assert!(
        hook_index < result_index,
        "SessionEnd must be recorded before the canonical result"
    );
    let completed_index = events
        .iter()
        .position(|event| {
            matches!(
                &event.kind,
                chat_state::TimelineEventKind::Hook(
                    chat_state::HookEvent::Completed {
                        occurrence_id: completed_id,
                        ..
                    },
                ) if completed_id == &occurrence_id
            )
        })
        .expect("the SessionEnd occurrence must complete");
    assert!(
        completed_index < result_index,
        "the SessionEnd aggregate must precede the canonical result"
    );
}

#[test]
fn cancelled_result_keeps_the_same_terminal_commit_order() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let (actor, loop_task, commands, _chat_events, recorded) =
                terminal_commit_actor().await;
            let mut result_event = fixture_result_event();
            result_event.outcome = chat_state::SubagentOutcome::Cancelled;
            result_event.error = Some("cancelled by the parent run".into());
            let result_ref = commit_result_and_await_exit(&commands, loop_task, result_event).await;

            assert_eq!(result_ref.timeline_id, actor.session_id_string());
            assert_eq!(result_ref.first_seq, result_ref.last_seq);
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let result_index = assert_single_last_result(&events);
            assert_eq!(
                events[result_index].seq.get(),
                result_ref.first_seq,
                "the ACK must reference the committed cancelled result"
            );
            assert_session_end_precedes_result(&events, result_index);
            assert_eq!(
                recorded_result_count(&recorded_durable_events(&recorded)),
                1,
                "the cancelled result must be the only durably accepted result"
            );
        }));
    });
}

#[test]
fn expired_deadline_appends_no_result_and_acks_the_typed_failure() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let (_actor, loop_task, commands, _chat_events, recorded) =
                terminal_commit_actor().await;
            let (respond_to, ack) = tokio::sync::oneshot::channel();
            commands
                .send(SessionCommand::CommitSubagentResult {
                    result: fixture_result_event(),
                    deadline: tokio::time::Instant::now() - std::time::Duration::from_secs(1),
                    respond_to,
                })
                .unwrap();
            let ack_result = tokio::time::timeout(std::time::Duration::from_secs(20), ack)
                .await
                .expect("the actor must resolve an expired-deadline commit")
                .unwrap();
            assert!(
                matches!(
                    ack_result,
                    Err(chat_state::TimelineWriteError::DeadlineExceeded)
                ),
                "an expired deadline must ACK the typed deadline failure instead of success, got {ack_result:?}"
            );
            tokio::time::timeout(std::time::Duration::from_secs(10), loop_task)
                .await
                .unwrap()
                .unwrap();
            // A deadline failure poisons the Timeline writer, so the
            // in-memory ChatState query is unavailable after exit. The
            // durable evidence is the stub's own record: the seed was
            // accepted and the canonical result never reached persistence.
            let durable = recorded_durable_events(&recorded);
            assert!(
                recorded_has_durable_seed(&durable),
                "the seed must have been durably accepted before the expired commit"
            );
            assert_eq!(
                recorded_result_count(&durable),
                0,
                "an expired deadline must not append the canonical result"
            );
        }));
    });
}

#[test]
fn dropped_ack_receiver_neither_stalls_the_actor_nor_duplicates_the_result() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let (_actor, loop_task, commands, _chat_events, recorded) =
                terminal_commit_actor().await;
            let (respond_to, ack) = tokio::sync::oneshot::channel();
            drop(ack);
            commands
                .send(SessionCommand::CommitSubagentResult {
                    result: fixture_result_event(),
                    deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(30),
                    respond_to,
                })
                .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(10), loop_task)
                .await
                .expect("a dropped acknowledgement receiver must not stall the actor")
                .unwrap();
            let durable = recorded_durable_events(&recorded);
            assert_eq!(
                recorded_result_count(&durable),
                1,
                "the commit must reach persistence exactly once despite the lost ACK"
            );
            assert!(
                durable.last().is_some_and(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::SubagentResult(_)
                )),
                "the accepted result must be the durable record's last event"
            );
        }));
    });
}

#[test]
fn failed_session_end_hook_write_withholds_result_and_commit_success_ack() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let (_actor, loop_task, commands, _chat_events, recorded) =
                terminal_commit_actor_with_session_end_hook_failure(true).await;
            let (respond_to, ack) = tokio::sync::oneshot::channel();
            commands
                .send(SessionCommand::CommitSubagentResult {
                    result: fixture_result_event(),
                    deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(30),
                    respond_to,
                })
                .unwrap();
            // The SessionEnd lifecycle could not become durable, so the
            // actor fail-stops before the commit: the ACK responder is
            // dropped and the waiting runner is left with the unknown
            // outcome instead of a success.
            let ack_outcome = tokio::time::timeout(std::time::Duration::from_secs(20), ack)
                .await
                .expect("the dropped commit responder must release the waiting runner");
            assert!(
                !matches!(ack_outcome, Ok(Ok(_))),
                "a failed session-end hook must never ACK the commit as success, got {ack_outcome:?}"
            );
            // The actor retires on its own fail-stop path instead of
            // stalling on the unacknowledged commit.
            tokio::time::timeout(std::time::Duration::from_secs(10), loop_task)
                .await
                .expect("the actor must retire after the hook persistence failure")
                .unwrap();
            let durable = recorded_durable_events(&recorded);
            assert!(
                recorded_has_durable_seed(&durable),
                "only the session-end hook write may fail; the seed stays durable"
            );
            assert_eq!(
                recorded_session_end_trigger_count(&durable),
                0,
                "the rejected session-end occurrence must not be recorded or retried"
            );
            assert_eq!(
                recorded_result_count(&durable),
                0,
                "a failed session-end hook must withhold the canonical result"
            );
        }));
    });
}

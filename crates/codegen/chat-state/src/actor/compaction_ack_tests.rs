//! Compaction ACK-loss probes against a persistence sink that has already
//! retained the first write. The retry must use the same immutable event, so
//! the sink stores one logical Timeline fact and the live actor can safely
//! acknowledge it.

use super::*;
use crate::persistence::TimelinePersistence;
use std::io;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[derive(Clone, Copy)]
enum LostAckTarget {
    Replacement,
    Completed,
}

#[derive(Clone, Copy)]
enum HeldAckTarget {
    Replacement,
    Completed,
    Promoted,
}

#[derive(Default)]
struct RetainedTimeline {
    events: Vec<crate::TimelineEvent>,
    attempts: Vec<crate::TimelineEvent>,
}

struct RetainThenLoseAck {
    target: LostAckTarget,
    state: Arc<Mutex<RetainedTimeline>>,
    lost: bool,
}

struct RetainAndHoldAck {
    target: HeldAckTarget,
    state: Arc<Mutex<RetainedTimeline>>,
    first_target_write: Arc<tokio::sync::Notify>,
    held_ack: Arc<Mutex<Option<oneshot::Sender<io::Result<()>>>>>,
}

impl RetainAndHoldAck {
    fn targets(&self, event: &crate::TimelineEvent) -> bool {
        match (&self.target, &event.kind) {
            (HeldAckTarget::Replacement, crate::TimelineEventKind::Messages(message)) => {
                message.cause == crate::MessageCause::Compaction
            }
            (
                HeldAckTarget::Completed,
                crate::TimelineEventKind::Compaction(crate::CompactionEvent::Completed { .. }),
            ) => true,
            (
                HeldAckTarget::Promoted,
                crate::TimelineEventKind::Compaction(crate::CompactionEvent::Promoted { .. }),
            ) => true,
            _ => false,
        }
    }
}

impl TimelinePersistence for RetainAndHoldAck {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &crate::TimelineEvent,
    ) -> oneshot::Receiver<io::Result<()>> {
        let (reply, receiver) = oneshot::channel();
        let mut state = self.state.lock().unwrap();
        state.attempts.push(event.clone());
        if let Some(existing) = state
            .events
            .iter()
            .find(|existing| existing.seq == event.seq)
        {
            assert_eq!(
                serde_json::to_value(existing).unwrap(),
                serde_json::to_value(event).unwrap(),
                "retry after an unconfirmed ACK must preserve the retained event"
            );
        } else {
            state.events.push(event.clone());
        }
        let is_target = self.targets(event);
        let mut held_ack = self.held_ack.lock().unwrap();
        if is_target && held_ack.is_none() {
            self.first_target_write.notify_one();
            *held_ack = Some(reply);
        } else {
            let _ = reply.send(Ok(()));
        }
        receiver
    }

    fn flush(&mut self) {}
}

impl RetainThenLoseAck {
    fn targets(&self, event: &crate::TimelineEvent) -> bool {
        match (&self.target, &event.kind) {
            (LostAckTarget::Replacement, crate::TimelineEventKind::Messages(message)) => {
                message.cause == crate::MessageCause::Compaction
            }
            (
                LostAckTarget::Completed,
                crate::TimelineEventKind::Compaction(crate::CompactionEvent::Completed { .. }),
            ) => true,
            _ => false,
        }
    }
}

impl TimelinePersistence for RetainThenLoseAck {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &crate::TimelineEvent,
    ) -> oneshot::Receiver<io::Result<()>> {
        let (reply, receiver) = oneshot::channel();
        let mut state = self.state.lock().unwrap();
        state.attempts.push(event.clone());
        if let Some(existing) = state
            .events
            .iter()
            .find(|existing| existing.seq == event.seq)
        {
            assert_eq!(
                serde_json::to_value(existing).unwrap(),
                serde_json::to_value(event).unwrap(),
                "a retry must preserve the retained event's exact identity and payload"
            );
        } else {
            state.events.push(event.clone());
        }
        let lose_this_ack = self.targets(event) && !self.lost;
        if lose_this_ack {
            self.lost = true;
            // The event is already in the simulated durable log. Closing the
            // receipt channel forces ChatState to retry this exact event.
            drop(reply);
        } else {
            let _ = reply.send(Ok(()));
        }
        receiver
    }

    fn flush(&mut self) {}
}

async fn spawn_ack_loss_actor(
    target: LostAckTarget,
) -> (crate::handle::ChatStateHandle, Arc<Mutex<RetainedTimeline>>) {
    let state = Arc::new(Mutex::new(RetainedTimeline::default()));
    let persistence = RetainThenLoseAck {
        target,
        state: state.clone(),
        lost: false,
    };
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        vec![
            ConversationItem::system("system"),
            ConversationItem::user("old prompt"),
            ConversationItem::assistant("old answer"),
        ],
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    // Cross the bootstrap barrier so all initial Surface facts are retained.
    handle.get_conversation().await;
    (handle, state)
}

async fn spawn_held_ack_actor(
    target: HeldAckTarget,
) -> (
    crate::handle::ChatStateHandle,
    tokio_util::sync::CancellationToken,
    Arc<Mutex<RetainedTimeline>>,
    Arc<tokio::sync::Notify>,
    Arc<Mutex<Option<oneshot::Sender<io::Result<()>>>>>,
) {
    let state = Arc::new(Mutex::new(RetainedTimeline::default()));
    let first_target_write = Arc::new(tokio::sync::Notify::new());
    let held_ack = Arc::new(Mutex::new(None));
    let persistence = RetainAndHoldAck {
        target,
        state: state.clone(),
        first_target_write: first_target_write.clone(),
        held_ack: held_ack.clone(),
    };
    let cancellation = tokio_util::sync::CancellationToken::new();
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        vec![
            ConversationItem::system("system"),
            ConversationItem::user("old prompt"),
            ConversationItem::assistant("old answer"),
        ],
        test_config(),
        Box::new(persistence),
        event_tx,
        cancellation.clone(),
    );
    handle.get_conversation().await;
    (handle, cancellation, state, first_target_write, held_ack)
}

fn assert_exactly_one_retained_retry(
    state: &Arc<Mutex<RetainedTimeline>>,
    target: impl Fn(&crate::TimelineEvent) -> bool,
) -> Vec<crate::TimelineEvent> {
    let state = state.lock().unwrap();
    let attempted = state
        .attempts
        .iter()
        .filter(|event| target(event))
        .collect::<Vec<_>>();
    assert_eq!(
        attempted.len(),
        2,
        "one lost ACK must cause one exact retry"
    );
    assert_eq!(
        serde_json::to_value(attempted[0]).unwrap(),
        serde_json::to_value(attempted[1]).unwrap(),
        "the retry must be byte-equivalent at the typed event level"
    );
    let retained = state
        .events
        .iter()
        .filter(|event| target(event))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        retained.len(),
        1,
        "the durable sink retains one logical fact"
    );
    state.events.clone()
}

#[tokio::test]
async fn compaction_replacement_reconciles_a_lost_ack_before_surface_publication() {
    let (handle, state) = spawn_ack_loss_actor(LostAckTarget::Replacement).await;
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                mode: crate::CompactionMode::Foreground,
                id: "replacement-ack-loss".into(),
                source_items: 3,
                prompt_index: 0,
            },
        ))
        .await
        .unwrap();
    let target = record_compaction_summary(&handle, "replacement-ack-loss").await;

    handle
        .replace_compaction_range(
            target,
            vec![ConversationItem::user_meta("replacement summary")],
        )
        .await
        .expect("exact retry ACK allows the replacement to publish");

    let live_surface = handle.get_conversation().await;
    assert!(
        live_surface
            .iter()
            .any(|item| item.text_content() == "replacement summary")
    );
    let events = assert_exactly_one_retained_retry(&state, |event| {
        matches!(
            &event.kind,
            crate::TimelineEventKind::Messages(message)
                if message.cause == crate::MessageCause::Compaction
        )
    });
    let replayed = crate::Timeline::from_events(events).expect("retained facts replay");
    assert_eq!(
        serde_json::to_value(replayed.surface()).unwrap(),
        serde_json::to_value(&live_surface).unwrap()
    );
}

#[tokio::test]
async fn completed_terminal_reconciles_a_lost_ack_without_duplicate_completion() {
    let (handle, state) = spawn_ack_loss_actor(LostAckTarget::Completed).await;
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                mode: crate::CompactionMode::Foreground,
                id: "terminal-ack-loss".into(),
                source_items: 3,
                prompt_index: 0,
            },
        ))
        .await
        .unwrap();
    let target = record_compaction_summary(&handle, "terminal-ack-loss").await;
    handle
        .replace_compaction_range(
            target,
            vec![ConversationItem::user_meta("terminal summary")],
        )
        .await
        .unwrap();
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Completed {
                id: "terminal-ack-loss".into(),
                source_items: 3,
                result_items: 1,
                duration_ms: 1,
            },
        ))
        .await
        .expect("exact retry ACK confirms the terminal");

    let events = assert_exactly_one_retained_retry(&state, |event| {
        matches!(
            &event.kind,
            crate::TimelineEventKind::Compaction(crate::CompactionEvent::Completed { id, .. })
                if id == "terminal-ack-loss"
        )
    });
    let replayed = crate::Timeline::from_events(events).expect("terminal facts replay");
    assert!(
        replayed
            .surface()
            .iter()
            .any(|item| item.text_content() == "terminal summary")
    );
}

async fn begin_compaction(
    handle: &crate::handle::ChatStateHandle,
    id: &str,
) -> crate::SurfaceRange {
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                mode: crate::CompactionMode::Foreground,
                id: id.into(),
                source_items: 3,
                prompt_index: 0,
            },
        ))
        .await
        .unwrap();
    record_compaction_summary(handle, id).await
}

fn replay_and_settle(events: Vec<crate::TimelineEvent>) -> crate::Timeline {
    let mut replayed = crate::Timeline::from_events(events).expect("retained facts replay");
    replayed
        .settle_open_compaction("process_interrupted")
        .expect("interrupted compaction can be settled");
    replayed
}

#[tokio::test]
async fn retained_replacement_with_unconfirmed_ack_poison_closes_writer_and_replay_completes_once()
{
    let (handle, cancellation, state, first_write, _held_ack) =
        spawn_held_ack_actor(HeldAckTarget::Replacement).await;
    let target = begin_compaction(&handle, "replacement-deadline").await;
    let handle_for_write = handle.clone();
    let write = tokio::spawn(async move {
        handle_for_write
            .replace_compaction_range(
                target,
                vec![ConversationItem::user_meta("replacement retained")],
            )
            .await
    });
    first_write.notified().await;
    cancellation.cancel();
    assert!(matches!(
        write.await.unwrap(),
        Err(crate::TimelineWriteError::Cancelled)
    ));

    let attempts_before = state.lock().unwrap().attempts.len();
    let rejected = handle
        .record_timeline_event_durably(crate::TimelineEventKind::Observation(
            crate::ObservationEvent {
                scope: "compaction_ack_test".into(),
                name: "must_not_follow_unconfirmed_replacement".into(),
                turn: None,
                step: None,
                data: None,
            },
        ))
        .await;
    assert!(
        rejected.is_err(),
        "cancelled writer epoch must reject later mutation"
    );
    assert_eq!(state.lock().unwrap().attempts.len(), attempts_before);

    let events = state.lock().unwrap().events.clone();
    let replayed = replay_and_settle(events);
    assert!(
        replayed
            .surface()
            .iter()
            .any(|item| item.text_content() == "replacement retained")
    );
    assert_eq!(
        replayed
            .events()
            .iter()
            .filter(|event| matches!(
                &event.kind,
                crate::TimelineEventKind::Compaction(crate::CompactionEvent::Completed {
                    id,
                    ..
                }) if id == "replacement-deadline"
            ))
            .count(),
        1,
        "cold recovery completes the physically retained replacement exactly once"
    );
}

#[tokio::test(start_paused = true)]
async fn retained_completed_with_unconfirmed_ack_times_out_and_replays_once() {
    let (handle, _cancellation, state, first_write, _held_ack) =
        spawn_held_ack_actor(HeldAckTarget::Completed).await;
    let target = begin_compaction(&handle, "completed-deadline").await;
    handle
        .replace_compaction_range(
            target,
            vec![ConversationItem::user_meta("completed retained")],
        )
        .await
        .unwrap();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    let handle_for_write = handle.clone();
    let write = tokio::spawn(async move {
        handle_for_write
            .record_timeline_event_durably_before(
                crate::TimelineEventKind::Compaction(crate::CompactionEvent::Completed {
                    id: "completed-deadline".into(),
                    source_items: 3,
                    result_items: 1,
                    duration_ms: 1,
                }),
                deadline,
            )
            .await
    });
    first_write.notified().await;
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    assert!(matches!(
        write.await.unwrap(),
        Err(crate::TimelineWriteError::DeadlineExceeded)
    ));

    let attempts_before = state.lock().unwrap().attempts.len();
    let rejected = handle
        .record_timeline_event_durably(crate::TimelineEventKind::Observation(
            crate::ObservationEvent {
                scope: "compaction_ack_test".into(),
                name: "must_not_follow_unconfirmed_terminal".into(),
                turn: None,
                step: None,
                data: None,
            },
        ))
        .await;
    assert!(
        rejected.is_err(),
        "deadline-expired writer epoch must be poisoned"
    );
    assert_eq!(state.lock().unwrap().attempts.len(), attempts_before);

    let mut replayed = crate::Timeline::from_events(state.lock().unwrap().events.clone())
        .expect("physically retained terminal replays");
    assert!(
        replayed
            .settle_open_compaction("process_interrupted")
            .unwrap()
            .is_none()
    );
    assert!(
        replayed
            .surface()
            .iter()
            .any(|item| item.text_content() == "completed retained")
    );
    assert_eq!(
        replayed
            .events()
            .iter()
            .filter(|event| matches!(
                &event.kind,
                crate::TimelineEventKind::Compaction(crate::CompactionEvent::Completed {
                    id,
                    ..
                }) if id == "completed-deadline"
            ))
            .count(),
        1,
        "replay keeps the already retained Completed fact instead of adding another"
    );
}

#[tokio::test]
async fn acknowledged_compaction_promotion_reuses_the_original_durable_event() {
    let (handle, state) = spawn_ack_loss_actor(LostAckTarget::Replacement).await;
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                mode: crate::CompactionMode::Background,
                id: "promotion-acknowledged".into(),
                source_items: 3,
                prompt_index: 0,
            },
        ))
        .await
        .unwrap();
    let first = handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Promoted {
                id: "promotion-acknowledged".into(),
            },
        ))
        .await
        .unwrap();
    let retry = handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Promoted {
                id: "promotion-acknowledged".into(),
            },
        ))
        .await
        .unwrap();

    assert_eq!(
        first.seq, retry.seq,
        "ACKed retry returns the original sequence"
    );
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&retry).unwrap(),
        "ACKed retry returns the original event"
    );
    let state = state.lock().unwrap();
    assert_eq!(
        state
            .events
            .iter()
            .filter(|event| matches!(
                &event.kind,
                crate::TimelineEventKind::Compaction(crate::CompactionEvent::Promoted { id })
                    if id == "promotion-acknowledged"
            ))
            .count(),
        1,
        "the acknowledged promotion remains one durable event"
    );
}

#[tokio::test]
async fn compaction_promotion_retry_cannot_borrow_a_stale_promotion() {
    for invalidation in ["foreign", "summary", "terminal"] {
        let (handle, state) = spawn_ack_loss_actor(LostAckTarget::Replacement).await;
        for kind in [
            crate::CompactionEvent::Started {
                mode: crate::CompactionMode::Background,
                id: "original-generation".into(),
                source_items: 3,
                prompt_index: 0,
            },
            crate::CompactionEvent::Promoted {
                id: "original-generation".into(),
            },
        ] {
            handle
                .record_timeline_event_durably(crate::TimelineEventKind::Compaction(kind))
                .await
                .unwrap();
        }
        if invalidation == "summary" {
            record_compaction_summary(&handle, "original-generation").await;
        } else if invalidation == "terminal" {
            handle
                .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
                    crate::CompactionEvent::Failed {
                        id: "original-generation".into(),
                        duration_ms: 1,
                        error: "cancelled".into(),
                    },
                ))
                .await
                .unwrap();
        }
        let result = handle
            .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
                crate::CompactionEvent::Promoted {
                    id: if invalidation == "foreign" {
                        "different-generation"
                    } else {
                        "original-generation"
                    }
                    .into(),
                },
            ))
            .await;
        assert!(
            matches!(result, Err(crate::TimelineWriteError::Invalid(_))),
            "{invalidation}: an existing promotion cannot bypass its current lifecycle boundary"
        );
        assert_eq!(
            state
                .lock()
                .unwrap()
                .events
                .iter()
                .filter(|event| matches!(
                    &event.kind,
                    crate::TimelineEventKind::Compaction(crate::CompactionEvent::Promoted { .. })
                ))
                .count(),
            1,
            "{invalidation}: only the original promotion is durable"
        );
    }
}

#[tokio::test]
async fn dropped_promotion_waiter_does_not_acknowledge_a_queued_retry_before_original_ack() {
    let (handle, _cancellation, state, first_write, held_ack) =
        spawn_held_ack_actor(HeldAckTarget::Promoted).await;
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                mode: crate::CompactionMode::Background,
                id: "promotion-held-ack".into(),
                source_items: 3,
                prompt_index: 0,
            },
        ))
        .await
        .unwrap();

    let first_handle = handle.clone();
    let first = tokio::spawn(async move {
        first_handle
            .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
                crate::CompactionEvent::Promoted {
                    id: "promotion-held-ack".into(),
                },
            ))
            .await
    });
    first_write.notified().await;
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());

    let retry = handle.record_timeline_event_durably(crate::TimelineEventKind::Compaction(
        crate::CompactionEvent::Promoted {
            id: "promotion-held-ack".into(),
        },
    ));
    tokio::pin!(retry);
    tokio::select! {
        biased;
        result = &mut retry => panic!("retry bypassed pending original ACK: {result:?}"),
        _ = std::future::ready(()) => {}
    }

    held_ack
        .lock()
        .unwrap()
        .take()
        .expect("the persistence sink holds the first promotion ACK")
        .send(Ok(()))
        .unwrap();
    let returned = retry.await.expect("confirmed promotion is reused");
    let promotion = state
        .lock()
        .unwrap()
        .events
        .iter()
        .find(|event| {
            matches!(
                &event.kind,
                crate::TimelineEventKind::Compaction(crate::CompactionEvent::Promoted { id })
                    if id == "promotion-held-ack"
            )
        })
        .cloned()
        .expect("the physical sink retained the promotion once");
    assert_eq!(promotion.seq, returned.seq);
    assert_eq!(
        serde_json::to_value(promotion).unwrap(),
        serde_json::to_value(returned).unwrap()
    );
    assert_eq!(
        state
            .lock()
            .unwrap()
            .events
            .iter()
            .filter(|event| matches!(
                &event.kind,
                crate::TimelineEventKind::Compaction(crate::CompactionEvent::Promoted { id })
                    if id == "promotion-held-ack"
            ))
            .count(),
        1,
        "the retry is satisfied by one original promoted event"
    );
}

#[tokio::test]
async fn failed_pending_promotion_ack_rejects_a_queued_same_id_retry() {
    let (handle, _cancellation, state, first_write, held_ack) =
        spawn_held_ack_actor(HeldAckTarget::Promoted).await;
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                mode: crate::CompactionMode::Background,
                id: "promotion-failed-ack".into(),
                source_items: 3,
                prompt_index: 0,
            },
        ))
        .await
        .unwrap();

    let first_handle = handle.clone();
    let first = tokio::spawn(async move {
        first_handle
            .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
                crate::CompactionEvent::Promoted {
                    id: "promotion-failed-ack".into(),
                },
            ))
            .await
    });
    first_write.notified().await;
    let retry = handle.record_timeline_event_durably(crate::TimelineEventKind::Compaction(
        crate::CompactionEvent::Promoted {
            id: "promotion-failed-ack".into(),
        },
    ));
    tokio::pin!(retry);
    tokio::select! {
        biased;
        result = &mut retry => panic!("retry bypassed pending original ACK: {result:?}"),
        _ = std::future::ready(()) => {}
    }

    held_ack
        .lock()
        .unwrap()
        .take()
        .expect("the persistence sink holds the first promotion ACK")
        .send(Err(io::Error::new(
            io::ErrorKind::StorageFull,
            "promotion was retained but never acknowledged",
        )))
        .unwrap();
    assert!(matches!(
        first.await.unwrap(),
        Err(crate::TimelineWriteError::Persistence(error))
            if error.kind() == io::ErrorKind::StorageFull
    ));
    assert!(
        retry.await.is_err(),
        "a queued retry cannot turn a failed promotion ACK into success"
    );
    assert_eq!(
        state
            .lock()
            .unwrap()
            .events
            .iter()
            .filter(|event| matches!(
                &event.kind,
                crate::TimelineEventKind::Compaction(crate::CompactionEvent::Promoted { id })
                    if id == "promotion-failed-ack"
            ))
            .count(),
        1,
        "the failed ACK leaves one physical fact for cold recovery to reconcile"
    );
}

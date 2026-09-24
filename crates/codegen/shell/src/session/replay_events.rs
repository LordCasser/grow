use acp_transport::protocol as acp;
use tokio::sync::{mpsc, oneshot};

use crate::extensions::notification::SessionNotification as GrowSessionNotification;
use acp::SessionNotification as AcpSessionNotification;

/// Notification destined for the high-frequency event ReplayBuffer).
/// Variants tag the inner protocol surface because the
/// merge rules and wire envelopes differ, but routing through `event_tx`
/// and `ReplayBuffer` is by design -- anything that goes here gets
/// debounced + merged, and emerges through `emit_buffered` without
/// firing per-chunk hooks or persistence writes.
///
/// One-shot Grow events (RetryState, ImageCompressed,
/// etc.) take the direct `send_grow_notification` path for per-event hooks and persistence.
#[derive(Debug, Clone)]
pub(crate) enum SessionNotification {
    Acp(Box<AcpSessionNotification>),
    Grow(Box<GrowSessionNotification>),
}

impl SessionNotification {
    pub(crate) fn session_id(&self) -> &acp::SessionId {
        match self {
            Self::Acp(n) => &n.session_id,
            Self::Grow(n) => &n.session_id,
        }
    }

    /// Returns true if this notification is a streaming chunk that
    /// should be buffered for merging + debouncing.
    pub(crate) fn is_streaming_chunk(&self) -> bool {
        match self {
            Self::Acp(n) => matches!(
                n.update,
                acp::SessionUpdate::AgentMessageChunk(_) | acp::SessionUpdate::AgentThoughtChunk(_)
            ),
            Self::Grow(n) => matches!(
                n.update,
                crate::extensions::notification::SessionUpdate::ToolCallDeltaChunk { .. }
            ),
        }
    }

    /// Extract `agentTimestampMs` from the notification's meta, if set.
    pub(crate) fn agent_timestamp_ms(&self) -> Option<u64> {
        match self {
            Self::Acp(n) => n
                .meta
                .as_ref()
                .and_then(|m| m.get("agentTimestampMs"))
                .and_then(|v| v.as_u64()),
            Self::Grow(n) => n
                .meta
                .as_ref()
                .and_then(|m| m.get("agentTimestampMs"))
                .and_then(|v| v.as_u64()),
        }
    }

    /// Returns true if this notification can be merged with `prev`'s pending slot based on their timestamps.
    pub(crate) fn is_in_timestamp_window(&self, prev: &Self, max_duration_ms: u64) -> bool {
        match (prev.agent_timestamp_ms(), self.agent_timestamp_ms()) {
            // ACP events have timestamps, so we can window-check.
            (Some(prev_ts), Some(incoming_ts)) => incoming_ts <= prev_ts + max_duration_ms,
            // Either side missing the agentTimestampMs meta means we can't window-check.
            _ => true,
        }
    }
}

impl From<AcpSessionNotification> for SessionNotification {
    fn from(n: AcpSessionNotification) -> Self {
        Self::Acp(Box::new(n))
    }
}

impl From<GrowSessionNotification> for SessionNotification {
    fn from(n: GrowSessionNotification) -> Self {
        Self::Grow(Box::new(n))
    }
}

#[cfg(test)]
impl SessionNotification {
    /// Test-only: borrow the inner ACP notification, panicking if this
    /// is not the `Acp` variant.
    pub(crate) fn expect_acp(&self) -> &AcpSessionNotification {
        match self {
            Self::Acp(n) => n,
            Self::Grow(_) => panic!("expected Acp notification, got Grow"),
        }
    }

    /// Test-only: move the inner ACP notification out, panicking if
    /// this is not the `Acp` variant.
    pub(crate) fn into_acp(self) -> AcpSessionNotification {
        match self {
            Self::Acp(n) => *n,
            Self::Grow(_) => panic!("expected Acp notification, got Grow"),
        }
    }
}

#[derive(Debug)]
pub(crate) enum SessionEvent {
    Notification(SessionNotification),
    /// The sampler drainer holds fragment credits until earlier translated
    /// notifications have been consumed from this FIFO.
    PreviewDrained {
        respond_to: oneshot::Sender<()>,
    },
    /// A deferred task completion became ready outside the
    /// actor mailbox. Wakes the actor so an idle session can synthesize a
    /// model turn; an active turn drains it at its next safe boundary.
    ForegroundWake,
    /// The detached manual-compaction owner has fully settled its durable
    /// transaction and emitted its terminal UI result. Only the main actor may
    /// release `ForegroundState::Compaction` or admit the next idle owner.
    ManualCompactionFinished {
        /// A panic means the detached owner could not produce its normal
        /// durable/UI terminal. The actor must fail-stop the session after
        /// releasing foreground ownership instead of silently continuing.
        failure: Option<String>,
    },
    FlushReplay {
        respond_to: Option<oneshot::Sender<()>>,
    },
    /// Ordered response-projection barrier. Because this crosses the same
    /// event FIFO as candidate and interleaved notifications, persistence
    /// cannot commit the canonical projection before prior preview traffic.
    ResponseProjection {
        projection: crate::session::response_projection::ResponseReplayProjection,
        respond_to: oneshot::Sender<Result<(), crate::session::storage::AppendUpdateError>>,
    },
    /// A detached control worker panicked or crossed a fatal persistence
    /// boundary. The main actor owns teardown; workers may only report the
    /// failure, never dismantle shared session authorities themselves.
    ControlWorkerFailed {
        message: String,
    },
}

impl SessionEvent {
    pub(crate) fn flush_with_ack() -> (Self, oneshot::Receiver<()>) {
        let (tx, rx) = oneshot::channel();
        (
            Self::FlushReplay {
                respond_to: Some(tx),
            },
            rx,
        )
    }
}

/// Release one sampler fragment's credits only after its translated events
/// have passed the session actor's event FIFO. A stopped actor closes the
/// budget so a provider waiting on capacity exits.
pub(crate) async fn release_preview_after_actor(
    event_tx: &mpsc::UnboundedSender<SessionEvent>,
    budget: &sampler::PreviewEventBudget,
    cost: u32,
) -> Result<(), ()> {
    let (respond_to, ack) = oneshot::channel();
    if event_tx
        .send(SessionEvent::PreviewDrained { respond_to })
        .is_err()
        || ack.await.is_err()
    {
        budget.close();
        return Err(());
    }
    budget.release_cost(cost);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlushReplayError {
    EventChannelClosed,
    Timeout,
}

/// Flush replay-buffered notifications through the session actor loop.
///
/// This must only be used from callers that are *outside* `run_session()`.
/// The `FlushComplete` command runs inside the actor loop and therefore
/// flushes `replay_buffer` inline to avoid waiting on a mailbox event that
/// the same loop would need to process.
pub(crate) async fn flush_replay_actor(
    event_tx: &mpsc::UnboundedSender<SessionEvent>,
) -> Result<(), FlushReplayError> {
    let (event, rx) = SessionEvent::flush_with_ack();
    event_tx
        .send(event)
        .map_err(|_| FlushReplayError::EventChannelClosed)?;
    tokio::time::timeout(std::time::Duration::from_secs(5), rx)
        .await
        .map_err(|_| FlushReplayError::Timeout)?
        .map_err(|_| FlushReplayError::EventChannelClosed)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn flush_replay_actor_acknowledges() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (event_tx, mut event_rx) = mpsc::unbounded_channel::<SessionEvent>();
                let (event, rx) = SessionEvent::flush_with_ack();
                event_tx.send(event).expect("event send should succeed");

                let event = event_rx.recv().await.expect("event should arrive");

                match event {
                    SessionEvent::FlushReplay { respond_to } => {
                        let tx = respond_to.expect("flush replay should carry ack sender");
                        tx.send(()).expect("ack send should succeed");
                    }
                    other => panic!("unexpected event: {other:?}"),
                }

                rx.await.expect("ack should be received");
            })
            .await;
    }

    #[tokio::test]
    async fn preview_credits_wait_for_actor_fence_and_close_on_lost_ack() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<SessionEvent>();
        let budget = sampler::PreviewEventBudget::default();
        let permit = budget.acquire(4096).await.unwrap();
        permit.forget();
        event_tx.send(SessionEvent::ForegroundWake).unwrap();

        let release = tokio::spawn({
            let event_tx = event_tx.clone();
            let budget = budget.clone();
            async move { release_preview_after_actor(&event_tx, &budget, 4096).await }
        });
        assert!(matches!(
            event_rx.recv().await,
            Some(SessionEvent::ForegroundWake)
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), budget.acquire(1))
                .await
                .is_err()
        );
        let SessionEvent::PreviewDrained { respond_to } = event_rx.recv().await.unwrap() else {
            panic!("expected preview fence");
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), budget.acquire(1))
                .await
                .is_err()
        );
        respond_to.send(()).unwrap();
        assert!(release.await.unwrap().is_ok());
        drop(budget.acquire(1).await.unwrap());

        let permit = budget.acquire(4096).await.unwrap();
        permit.forget();
        let release = tokio::spawn({
            let event_tx = event_tx.clone();
            let budget = budget.clone();
            async move { release_preview_after_actor(&event_tx, &budget, 4096).await }
        });
        let SessionEvent::PreviewDrained { respond_to } = event_rx.recv().await.unwrap() else {
            panic!("expected preview fence");
        };
        drop(respond_to);
        assert!(release.await.unwrap().is_err());
        assert!(budget.acquire(1).await.is_err());
    }
}

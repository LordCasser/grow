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
}

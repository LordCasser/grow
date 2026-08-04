use agent_client_protocol as acp;
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
/// One-shot Grow events (RetryState, ImageCompressed, HookExecution,
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
    /// A deferred task or Goal-stage completion became ready outside the
    /// actor mailbox. Wakes the actor so an idle session can synthesize a
    /// model turn; an active turn drains it at its next safe boundary.
    ForegroundWake,
    /// A background Goal stage (verifier / strategist / summarizer) finished
    /// its model work. The mailbox validates the captured lease and commits
    /// the pure state part; a stale completion (goal paused / cleared /
    /// revised, or foreground context changed) is dropped with diagnostics.
    /// The stage task never awaits the mailbox, so the mailbox is never
    /// blocked on model work (B1: verification now runs off the actor loop).
    GoalStageCompleted(GoalStageCompletion),
    FlushReplay {
        respond_to: Option<oneshot::Sender<()>>,
    },
}

/// Payload of [`SessionEvent::GoalStageCompleted`]. Carries the lease the
/// stage was scheduled under (goal identity + definition revision +
/// autonomy generation + foreground completion generation) plus the stage's
/// outcome (or a string error when the stage task itself failed).
#[derive(Debug)]
pub(crate) struct GoalStageCompletion {
    /// Monotonic per-session stage ordinal, assigned at schedule time.
    pub(crate) stage_id: u64,
    pub(crate) goal_id: String,
    pub(crate) definition_revision: u64,
    pub(crate) autonomy_generation: u64,
    /// `completion_delivery.generation()` captured at schedule time. The
    /// verifier commit drops the outcome if the foreground changed (user
    /// steering or a deferred completion became ready) while the stage ran.
    pub(crate) foreground_generation: u64,
    pub(crate) kind: GoalStageKind,
}

/// Discriminates the three background Goal stages. Each carries the outcome
/// its model work produced; `Err` means the stage task failed (panic /
/// join error) and is treated exactly like the stage's fail-closed variant
/// at commit time.
#[derive(Debug)]
pub(crate) enum GoalStageKind {
    /// Independent verification of an `update_goal(completed: true)`
    /// proposal. `ack` resolves the tool's `UpdateGoalAck` once the mailbox
    /// commits (or rejects) the outcome — the ack contract is unchanged,
    /// only the resolution point moved off the drain.
    Verifier {
        attempt: u32,
        max_runs: u32,
        latency_ms: u64,
        outcome: Result<crate::session::goal_classifier::GoalClassifierOutcome, String>,
        ack: Option<
            tokio::sync::oneshot::Sender<
                tools::implementations::grow_build::update_goal::UpdateGoalAck,
            >,
        >,
    },
    /// Stall-triggered strategist subagent (fail-OPEN). The commit records
    /// the recommendation only when the lease holds; otherwise the
    /// strategist cap bonus claimed at schedule time is revoked.
    Strategist {
        attempt: u32,
        consecutive_failures: u32,
        outcome: Result<crate::session::goal_strategist::GoalStrategistOutcome, String>,
    },
    /// One-shot closing summarizer after a verified achievement (fail-OPEN).
    Summarizer {
        attempt: u32,
        outcome: Result<crate::session::goal_summarizer::GoalSummarizerOutcome, String>,
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

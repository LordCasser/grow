//! Timeline persistence port and test implementations.
//!
//! The actor owns persistence exclusively (`Box<dyn TimelinePersistence>`), so the
//! trait uses `&mut self` — no locks, no atomics, no shared state.
//! The mock uses a channel to report records to the test, keeping everything
//! in the actor / message-passing paradigm.

use std::io;

use tokio::sync::{mpsc, oneshot};

use crate::TimelineEvent;

/// Append-only persistence boundary owned by the Timeline actor.
///
/// The actor owns this exclusively via `Box<dyn TimelinePersistence>`, so all
/// methods take `&mut self` — no interior mutability needed.
///
/// The real implementation wraps an `mpsc::UnboundedSender<PersistenceMsg>`
/// (which only needs `&self` to send, but `&mut self` is still correct
/// because the actor is the sole owner).
pub trait TimelinePersistence: Send + 'static {
    /// Durably append one immutable fact and acknowledge its commit.
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &TimelineEvent,
    ) -> oneshot::Receiver<io::Result<()>>;

    /// Flush pending writes to disk.
    fn flush(&mut self);
}

// ============================================================================
// Mock (test double) — channel-based, no locks, no atomics
// ============================================================================

/// A record of a persistence call, sent over a channel to the test.
#[derive(Debug, Clone)]
pub enum PersistenceRecord {
    /// An immutable timeline event was appended.
    Timeline(TimelineEvent),
    /// A flush was requested.
    Flush,
}

/// Test implementation: sends every call as a [`PersistenceRecord`] over a
/// channel. The test holds the [`MockPersistenceReceiver`] to inspect what
/// the actor did. No locks, no atomics — just message passing.
pub struct MockTimelinePersistence {
    tx: mpsc::UnboundedSender<PersistenceRecord>,
    timeline_ack_tx: Option<mpsc::UnboundedSender<oneshot::Sender<io::Result<()>>>>,
    automatic_acks_remaining: usize,
}

/// Receiver side of the mock. Held by the test to drain and inspect records.
pub struct MockPersistenceReceiver {
    rx: mpsc::UnboundedReceiver<PersistenceRecord>,
    timeline_ack_rx: Option<mpsc::UnboundedReceiver<oneshot::Sender<io::Result<()>>>>,
}

impl MockTimelinePersistence {
    /// Create a paired (mock, receiver). Give the mock to the actor, keep the
    /// receiver in the test.
    pub fn new() -> (Self, MockPersistenceReceiver) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Self {
                tx,
                timeline_ack_tx: None,
                automatic_acks_remaining: 0,
            },
            MockPersistenceReceiver {
                rx,
                timeline_ack_rx: None,
            },
        )
    }

    /// Create a mock whose durable Timeline acknowledgement is test-controlled.
    pub fn new_with_manual_timeline_ack() -> (Self, MockPersistenceReceiver) {
        Self::new_with_manual_timeline_ack_after(0)
    }

    /// Create a manual-ack mock while automatically acknowledging an initial
    /// bootstrap prefix. This lets actor tests control the first live write
    /// without weakening the durability of seed events.
    pub fn new_with_manual_timeline_ack_after(
        automatic_acks: usize,
    ) -> (Self, MockPersistenceReceiver) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (timeline_ack_tx, timeline_ack_rx) = mpsc::unbounded_channel();
        (
            Self {
                tx,
                timeline_ack_tx: Some(timeline_ack_tx),
                automatic_acks_remaining: automatic_acks,
            },
            MockPersistenceReceiver {
                rx,
                timeline_ack_rx: Some(timeline_ack_rx),
            },
        )
    }
}

impl MockPersistenceReceiver {
    /// Drain all pending records from the channel.
    pub fn drain(&mut self) -> Vec<PersistenceRecord> {
        let mut records = Vec::new();
        while let Ok(record) = self.rx.try_recv() {
            records.push(record);
        }
        records
    }

    pub async fn next_timeline_ack(&mut self) -> Option<oneshot::Sender<io::Result<()>>> {
        match &mut self.timeline_ack_rx {
            Some(rx) => tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
                .await
                .ok()
                .flatten(),
            None => None,
        }
    }
}

impl TimelinePersistence for MockTimelinePersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &TimelineEvent,
    ) -> oneshot::Receiver<io::Result<()>> {
        let (reply, receiver) = oneshot::channel();
        let result = self
            .tx
            .send(PersistenceRecord::Timeline(event.clone()))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "mock persistence closed"));
        if let Err(error) = result {
            let _ = reply.send(Err(error));
        } else if self.automatic_acks_remaining > 0 {
            self.automatic_acks_remaining -= 1;
            let _ = reply.send(Ok(()));
        } else if let Some(ack_tx) = &self.timeline_ack_tx {
            let _ = ack_tx.send(reply);
        } else {
            let _ = reply.send(Ok(()));
        }
        receiver
    }

    fn flush(&mut self) {
        let _ = self.tx.send(PersistenceRecord::Flush);
    }
}

// ============================================================================
// Null (noop) — for benchmarks / scenarios where persistence is unwanted
// ============================================================================

/// No-op implementation: discards everything (for benchmarks / noop scenarios).
pub struct NullTimelinePersistence;

impl TimelinePersistence for NullTimelinePersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        _event: &TimelineEvent,
    ) -> oneshot::Receiver<io::Result<()>> {
        let (reply, receiver) = oneshot::channel();
        let _ = reply.send(Ok(()));
        receiver
    }
    fn flush(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_persistence_records_flush() {
        let (mut mock, mut rx) = MockTimelinePersistence::new();
        mock.flush();
        mock.flush();
        let records = rx.drain();
        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .all(|r| matches!(r, PersistenceRecord::Flush))
        );
    }

    #[test]
    fn null_persistence_does_not_panic() {
        let mut null = NullTimelinePersistence;
        null.flush();
    }
}

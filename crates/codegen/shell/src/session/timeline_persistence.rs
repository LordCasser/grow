//! Production `TimelinePersistence` implementation backed by the session persistence channel.
//!
//! Wraps an `mpsc::UnboundedSender<PersistenceMsg>` and translates
//! `TimelinePersistence` trait calls into the appropriate `PersistenceMsg` variants.

use std::io;

use chat_state::{TimelineEvent, TimelinePersistence};
use tokio::sync::{mpsc, oneshot};

use super::persistence::PersistenceMsg;

/// Production `TimelinePersistence` that sends to the session persistence actor.
///
/// Timeline events are the sole durable conversation representation.
pub struct ChannelTimelinePersistence {
    tx: mpsc::UnboundedSender<PersistenceMsg>,
    directory: Option<std::sync::Arc<crate::session::storage::ContainedDirectory>>,
}

impl ChannelTimelinePersistence {
    /// Create a new `ChannelTimelinePersistence` wrapping the given persistence channel.
    pub fn new(tx: mpsc::UnboundedSender<PersistenceMsg>) -> Self {
        Self { tx, directory: None }
    }

    pub(super) fn for_session(handle: &super::persistence::PersistenceHandle) -> Self {
        let mut persistence = Self::new(handle.tx.clone());
        persistence.directory = handle.session_directory();
        persistence
    }
}

impl TimelinePersistence for ChannelTimelinePersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &TimelineEvent,
    ) -> oneshot::Receiver<io::Result<()>> {
        let (respond_to, receiver) = oneshot::channel();
        if self
            .tx
            .send(PersistenceMsg::TimelineDurablyAndAck {
                event: event.clone(),
                respond_to,
            })
            .is_err()
        {
            let (reply, receiver) = oneshot::channel();
            let _ = reply.send(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "session persistence actor unavailable",
            )));
            return receiver;
        }
        receiver
    }

    fn flush(&mut self) {
        let _ = self.tx.send(PersistenceMsg::Flush);
    }

    fn retains_complete_history(&self) -> bool {
        self.directory.is_none()
    }

    fn read_events(&self, from_seq: u64, end_seq: u64) -> Option<tokio::task::JoinHandle<io::Result<Vec<TimelineEvent>>>> {
        let directory = self.directory.clone()?;
        Some(tokio::task::spawn_blocking(move || {
            crate::session::storage::jsonl::JsonlStorageAdapter::read_domain_history_range(&directory, from_seq..end_seq)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn channel_persistence_sends_acknowledged_timeline_events() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut persistence = ChannelTimelinePersistence::new(tx);
        let timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user("test")])
                .unwrap();
        let acknowledgement = persistence.persist_timeline_event_and_ack(&timeline.events()[0]);
        assert!(matches!(
            rx.recv().await.unwrap(),
            PersistenceMsg::TimelineDurablyAndAck { .. }
        ));
        drop(acknowledgement);
    }

    #[tokio::test]
    async fn channel_persistence_sends_flush() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut persistence = ChannelTimelinePersistence::new(tx);
        persistence.flush();
        let msg = rx.recv().await.unwrap();
        assert!(matches!(msg, PersistenceMsg::Flush));
    }
}

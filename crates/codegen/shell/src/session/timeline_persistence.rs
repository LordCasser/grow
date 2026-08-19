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
}

impl ChannelTimelinePersistence {
    /// Create a new `ChannelTimelinePersistence` wrapping the given persistence channel.
    pub fn new(tx: mpsc::UnboundedSender<PersistenceMsg>) -> Self {
        Self { tx }
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

//! Production `ChatPersistence` implementation backed by the existing persistence channel.
//!
//! Wraps an `mpsc::UnboundedSender<PersistenceMsg>` and translates
//! `ChatPersistence` trait calls into the appropriate `PersistenceMsg` variants.

use std::io;

use chat_state::{ChatPersistence, TimelineEvent};
use sampling_types::ConversationItem;
use tokio::sync::{mpsc, oneshot};

use super::persistence::PersistenceMsg;

/// Production `ChatPersistence` that sends to the existing session persistence channel.
///
/// Translates:
/// - `persist_message` → `PersistenceMsg::Chat`
/// - `replace_history` → `PersistenceMsg::ReplaceChatHistory`
/// - `flush` → `PersistenceMsg::Flush`
pub struct ChannelChatPersistence {
    tx: mpsc::UnboundedSender<PersistenceMsg>,
}

impl ChannelChatPersistence {
    /// Create a new `ChannelChatPersistence` wrapping the given persistence channel.
    pub fn new(tx: mpsc::UnboundedSender<PersistenceMsg>) -> Self {
        Self { tx }
    }
}

impl ChatPersistence for ChannelChatPersistence {
    fn persist_timeline_event(&mut self, event: &TimelineEvent) {
        let _ = self.tx.send(PersistenceMsg::Timeline(event.clone()));
    }

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

    fn persist_message(&mut self, item: &ConversationItem) {
        let _ = self.tx.send(PersistenceMsg::Chat(item.clone()));
    }

    fn replace_history(&mut self, items: &[ConversationItem]) {
        let _ = self
            .tx
            .send(PersistenceMsg::ReplaceChatHistory(items.to_vec()));
    }

    fn flush(&mut self) {
        let _ = self.tx.send(PersistenceMsg::Flush);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn channel_persistence_sends_chat_messages() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut persistence = ChannelChatPersistence::new(tx);
        let item = ConversationItem::user("test");
        persistence.persist_message(&item);
        let msg = rx.recv().await.unwrap();
        assert!(matches!(msg, PersistenceMsg::Chat(_)));
    }

    #[tokio::test]
    async fn channel_persistence_sends_timeline_events() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut persistence = ChannelChatPersistence::new(tx);
        let timeline =
            chat_state::Timeline::from_seed(vec![ConversationItem::user("test")]).unwrap();
        persistence.persist_timeline_event(&timeline.events()[0]);
        assert!(matches!(
            rx.recv().await.unwrap(),
            PersistenceMsg::Timeline(_)
        ));
    }

    #[tokio::test]
    async fn channel_persistence_sends_replace_history() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut persistence = ChannelChatPersistence::new(tx);
        persistence.replace_history(&[ConversationItem::system("compacted")]);
        let msg = rx.recv().await.unwrap();
        assert!(matches!(msg, PersistenceMsg::ReplaceChatHistory(_)));
    }

    #[tokio::test]
    async fn channel_persistence_sends_flush() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut persistence = ChannelChatPersistence::new(tx);
        persistence.flush();
        let msg = rx.recv().await.unwrap();
        assert!(matches!(msg, PersistenceMsg::Flush));
    }
}

//! Inherited display history is rebound to a new model seed, never replayed
//! as the source session's input, response or execution authority.
use super::*;
use crate::session::storage::SessionUpdate;

#[derive(Debug, Default)]
pub(crate) struct SeedPresentation {
    pub source_ids: Vec<chat_state::SurfaceId>,
    pub updates: Vec<SessionUpdate>,
}

impl SeedPresentation {
    pub(crate) fn for_resume(
        source_ids: Vec<chat_state::SurfaceId>,
        updates: Vec<SessionUpdate>,
    ) -> Self {
        let updates = updates
            .into_iter()
            .filter_map(|mut update| {
                match &mut update {
                    SessionUpdate::Acp(notification) => {
                        match &mut notification.update {
                            acp::SessionUpdate::UserMessageChunk(chunk) => {
                                if let Some(meta) = chunk.meta.as_mut() {
                                    for key in [
                                        "grow/consumedInputEcho",
                                        "messageId",
                                        "promptIndex",
                                        "permissionEvidence",
                                    ] {
                                        meta.remove(key);
                                    }
                                }
                            }
                            acp::SessionUpdate::AgentMessageChunk(_)
                            | acp::SessionUpdate::AgentThoughtChunk(_)
                            | acp::SessionUpdate::ToolCall(_)
                            | acp::SessionUpdate::ToolCallUpdate(_) => {}
                            _ => return None,
                        }
                        if let Some(meta) = notification.meta.as_mut() {
                            for key in [
                                "eventId",
                                "grow/displayProjectionSource",
                                "grow/displayProjectionRevision",
                            ] {
                                meta.remove(key);
                            }
                        }
                    }
                    SessionUpdate::Grow(notification) => {
                        if !matches!(
                            &notification.update,
                            crate::extensions::notification::SessionUpdate::UiNotice(_)
                        ) {
                            return None;
                        }
                        if let Some(meta) = notification
                            .meta
                            .as_mut()
                            .and_then(serde_json::Value::as_object_mut)
                        {
                            meta.remove("eventId");
                        }
                    }
                    #[cfg(any(test, feature = "test-support"))]
                    SessionUpdate::ResponseReplayProjection(_) => {
                        unreachable!("prepared history expands responses")
                    }
                }
                Some(update)
            })
            .collect();
        Self {
            source_ids,
            updates,
        }
    }

    pub(crate) fn into_facts(self, seed: &chat_state::Timeline) -> io::Result<Vec<HistoryFact>> {
        let positions = self
            .source_ids
            .iter()
            .enumerate()
            .map(|(index, id)| ((id.event.get(), id.item as usize), index))
            .collect::<BTreeMap<_, _>>();
        let mut facts = Vec::new();
        for mut update in self.updates {
            let source = if let SessionUpdate::Acp(notification) = &mut update {
                let meta = notification.meta.get_or_insert_with(Default::default);
                let source = meta
                    .get("grow/historyMessageEvent")
                    .and_then(serde_json::Value::as_u64)
                    .zip(
                        meta.get("grow/historyMessageItem")
                            .and_then(serde_json::Value::as_u64),
                    );
                for key in ["samplingRequestId", "samplingAttempt", "responseProjection"] {
                    meta.remove(key);
                }
                source
            } else {
                None
            };
            if let Some(mut fact) = HistoryFact::from_update(&update)? {
                if let Some((event, ordinal)) = source
                    && let Some(index) = positions.get(&(event, ordinal as usize))
                {
                    let reference = match &fact.content {
                        HistoryContent::Assistant(chunk) => {
                            Some((TextChannel::Assistant, chunk.meta.clone()))
                        }
                        HistoryContent::Reasoning(chunk) => {
                            Some((TextChannel::Reasoning, chunk.meta.clone()))
                        }
                        _ => None,
                    };
                    if let Some((channel, chunk_meta)) = reference {
                        let source = seed
                            .surface_ids()
                            .get(*index)
                            .ok_or_else(|| invalid("inherited history seed source is absent"))?;
                        fact.content = HistoryContent::Message {
                            event: source.event.get(),
                            item: source.item as usize,
                            channel,
                            chunk_meta,
                        };
                    }
                }
                if let Some(meta) = fact.meta.as_mut() {
                    for key in [
                        "grow/historyCursor",
                        "samplingRequestId",
                        "samplingAttempt",
                        "responseProjection",
                        "grow/historyMessageEvent",
                        "grow/historyMessageItem",
                    ] {
                        meta.remove(key);
                    }
                }
                facts.push(fact);
            }
        }
        Ok(facts)
    }
}

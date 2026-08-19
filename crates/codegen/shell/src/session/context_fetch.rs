//! Session adapter for read-only reprojection of compacted Timeline context.

use chat_state::ChatStateHandle;
use sampling_types::ConversationItem;
use tools::implementations::context_fetch::{ContextFetchBackend, ContextFetchInput};

pub(crate) struct TimelineContextFetchBackend {
    timeline_id: String,
    chat_state: ChatStateHandle,
}

impl TimelineContextFetchBackend {
    pub(crate) fn new(timeline_id: String, chat_state: ChatStateHandle) -> Self {
        Self {
            timeline_id,
            chat_state,
        }
    }
}

#[async_trait::async_trait]
impl ContextFetchBackend for TimelineContextFetchBackend {
    async fn fetch(
        &self,
        input: &ContextFetchInput,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if input.timeline_id != self.timeline_id {
            return Err(std::io::Error::other(
                "the context reference belongs to a different Timeline",
            )
            .into());
        }

        let (items, total) = self
            .chat_state
            .fetch_compacted_context(input.first_seq, input.offset, input.limit)
            .await
            .ok_or_else(|| std::io::Error::other("chat-state actor is unavailable"))??;
        Ok(format_context_page(input, &items, total)?)
    }
}

fn format_context_page(
    input: &ContextFetchInput,
    items: &[ConversationItem],
    total: usize,
) -> Result<String, serde_json::Error> {
    let end = input.offset.saturating_add(items.len()).min(total);
    let mut output = format!(
        "<context-reprojection timeline_id={:?} summary_seq={} offset={} end={} total_items={}>\n",
        input.timeline_id, input.first_seq, input.offset, end, total
    );
    if items.is_empty() {
        output.push_str("No items in this page.\n");
    } else {
        for (page_index, item) in items.iter().enumerate() {
            let item_index = input.offset.saturating_add(page_index);
            output.push_str(&format!(
                "\n## Shadowed item {item_index}\n```json\n{}\n```\n",
                serde_json::to_string_pretty(item)?
            ));
        }
    }

    if end < total {
        let next = ContextFetchInput {
            timeline_id: input.timeline_id.clone(),
            first_seq: input.first_seq,
            last_seq: input.last_seq,
            offset: end,
            limit: input.limit,
        };
        output.push_str(&format!(
            "\nMore shadowed context is available. Fetch the next page with:\n```json\n{}\n```\n",
            serde_json::to_string_pretty(&next)?
        ));
    }
    output.push_str("</context-reprojection>");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_renderer_preserves_roles_and_emits_next_reference() {
        let input = ContextFetchInput {
            timeline_id: "session-1".into(),
            first_seq: 9,
            last_seq: 9,
            offset: 1,
            limit: 1,
        };
        let rendered = format_context_page(
            &input,
            &[ConversationItem::assistant("original answer")],
            3,
        )
        .unwrap();

        assert!(rendered.contains("Shadowed item 1"));
        assert!(rendered.contains("original answer"));
        assert!(rendered.contains("\"offset\": 2"));
        assert!(rendered.contains("\"first_seq\": 9"));
    }
}

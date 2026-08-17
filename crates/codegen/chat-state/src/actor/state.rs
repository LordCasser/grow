//! Internal state types for the ChatStateActor.

use std::collections::BTreeSet;

use sampling_types::{
    ConversationItem, DanglingToolCallReason, SamplingConfig, TokenUsage,
    dedup_duplicate_tool_results, repair_dangling_tool_calls,
};

use crate::types::Credentials;
use crate::usage::UsageLedger;
use crate::{EventSeq, Timeline};

/// Bytes/4 estimate of the system prompt portion of a [`ConversationItem`].
/// Returns 0 for non-system items so callers can pipe through whatever they
/// have without unwrapping.
pub fn estimate_system_message_tokens(item: &ConversationItem) -> u64 {
    match item {
        ConversationItem::System(s) => token_estimation::estimate_tokens(&s.content),
        _ => 0,
    }
}

/// Bytes/4 estimate of one tool definition (name + description + the
/// JSON-serialized parameters).
pub fn estimate_tool_definition_tokens(td: &sampling_types::ToolDefinition) -> u64 {
    let name_len = td.function.name.len();
    let desc_len = td.function.description.as_deref().map_or(0, |d| d.len());
    let params_len = td.function.parameters.to_string().len();
    ((name_len + desc_len + params_len) as u64) / token_estimation::BYTES_PER_TOKEN
}

/// Sum [`estimate_tool_definition_tokens`] across a slice.
pub fn estimate_tool_definitions_tokens(tds: &[sampling_types::ToolDefinition]) -> u64 {
    tds.iter().map(estimate_tool_definition_tokens).sum()
}

/// Bytes/4 estimate for a single [`ConversationItem`].
///
/// Images are counted at [`token_estimation::IMAGE_TOKEN_ESTIMATE`] each.
/// Shared by [`estimate_conversation_tokens`] and [`estimate_messages_tokens`]
/// so the per-variant arithmetic stays in one place.
pub fn estimate_item_tokens(item: &ConversationItem) -> u64 {
    use sampling_types::ContentPart;
    match item {
        ConversationItem::System(s) => token_estimation::estimate_tokens(&s.content),
        ConversationItem::User(u) => {
            let mut bytes: usize = 0;
            let mut images: u64 = 0;
            for p in &u.content {
                match p {
                    ContentPart::Text { text } => bytes += text.len(),
                    ContentPart::Image { .. } => images += 1,
                }
            }
            (bytes as u64) / token_estimation::BYTES_PER_TOKEN
                + token_estimation::estimate_image_tokens(images)
        }
        ConversationItem::Assistant(a) => {
            let bytes = a.content.len()
                + a.tool_calls
                    .iter()
                    .map(|tc| tc.arguments.len())
                    .sum::<usize>();
            (bytes as u64) / token_estimation::BYTES_PER_TOKEN
        }
        ConversationItem::ToolResult(tr) => token_estimation::estimate_tokens(&tr.content),
        ConversationItem::BackendToolCall(b) => {
            token_estimation::estimate_tokens(&b.text_summary())
        }
        ConversationItem::Reasoning(r) => {
            // Summary + content text follow the standard bytes-per-token
            // estimate; encrypted blobs are base64 and don't survive
            // tokenization 1:1, so estimate at len/4 as well.
            let text_bytes = sampling_types::reasoning_item_text(r).len();
            let enc_bytes = r.encrypted_content.as_deref().map(str::len).unwrap_or(0);
            ((text_bytes + enc_bytes) as u64) / token_estimation::BYTES_PER_TOKEN
        }
    }
}

/// Estimate token footprint: text bytes / 4, images at the per-image
/// constant defined by [`token_estimation::IMAGE_TOKEN_ESTIMATE`].
pub fn estimate_conversation_tokens(items: &[ConversationItem]) -> u64 {
    items.iter().map(estimate_item_tokens).sum()
}

/// grow-build's [`ItemTokenCounter`](compaction::ItemTokenCounter)
/// for the shared compaction engine: the bytes/4 estimate grow-build already
/// uses to drive its compaction triggers, exposed through the seam so the
/// shared budgeting math gets the *same* trusted count.
///
/// Where another host plugs a real BPE tokenizer into the same seam,
/// grow-build estimates instead, reusing [`estimate_item_tokens`] so the
/// per-variant arithmetic (images, reasoning blobs, tool-call args) stays in
/// one place.
pub struct EstimatedItemTokenCounter;

impl compaction::ItemTokenCounter<ConversationItem> for EstimatedItemTokenCounter {
    fn count_item_tokens(&self, item: &ConversationItem) -> u32 {
        // The estimate is a `u64`; a single item never approaches `u32::MAX`
        // tokens, but saturate rather than wrap if one somehow does.
        estimate_item_tokens(item).try_into().unwrap_or(u32::MAX)
    }
}

/// Bytes/4 estimate of every non-system item in `items`.
pub fn estimate_messages_tokens(items: &[ConversationItem]) -> u64 {
    items
        .iter()
        .filter(|i| !matches!(i, ConversationItem::System(_)))
        .map(estimate_item_tokens)
        .sum()
}

/// Internal mutable state for the ChatStateActor.
///
/// All fields are owned exclusively by the actor task — no locks needed.
pub(crate) struct ChatState {
    /// Durable conversation facts plus the current model-visible projection.
    pub timeline: Timeline,
    /// Current sampling configuration (model, context window, etc.).
    pub sampling_config: SamplingConfig,
    /// Current prompt index (incremented per user turn).
    pub prompt_index: usize,
    /// Cached prompt texts for rewind preview.
    pub prompt_texts: Vec<String>,
    /// Accumulated token usage.
    pub total_tokens: u64,
    /// Timestamp when the current stream started (epoch ms).
    pub stream_start_ms: Option<i64>,
    /// Timestamp when the current turn started (epoch ms).
    pub turn_start_ms: Option<i64>,
    /// File paths the agent has edited.
    pub agent_edited_paths: BTreeSet<String>,
    /// Prompt index at which the last compaction occurred.
    pub last_compaction_prompt_index: Option<usize>,
    /// Opaque credential secrets (api key, optional extra auth, client version).
    /// Stored opaquely — the actor never interprets them.
    pub credentials: Credentials,
    /// Bytes/4 estimate of tokens added since the last `record_token_usage`.
    /// Used by `check_preflight_overflow` to detect context window overflows
    /// between model responses.
    pub estimated_tokens_since_model: u64,
    /// Bytes/4 estimate of the conversation as of the last `record_token_usage`
    /// (or last reseed). `total_tokens − estimate_at_last_response` is the
    /// provider-side overhead carried across compaction.
    pub estimate_at_last_response: u64,
    /// Per-turn token usage from the most recent model response.
    /// Stashed by `record_last_turn_usage()` and read at `PromptResponse`
    /// construction to enrich `_meta` with `inputTokens` / `outputTokens` /
    /// `cachedReadTokens`. `None` means no model turn has completed yet
    /// in this session (or this is a freshly restored session that did not
    /// persist last_turn_usage). Always overwritten by the most recent turn —
    /// historical turns are not retained here.
    pub last_turn_usage: Option<TokenUsage>,
    /// Usage for the open prompt (cleared on next prompt; not persisted).
    pub prompt_usage: Option<UsageLedger>,
    /// Lifetime session usage (not persisted).
    pub session_usage: UsageLedger,
    /// Event-sequence turn capture state. `Some` = capture active, `None` = inactive.
    /// Cleared on `TakeTurnMessages` (consumed), `BeginTurnCapture` (new turn),
    /// and `TruncateToPromptIndex` (rewind abandons the turn).
    pub(super) turn_capture: Option<TurnCaptureState>,
}

/// Tracks which append events belong to the current turn.
///
/// Surface replacements do not invalidate this cursor because accepted
/// events are immutable. This is the primary reason turn capture belongs on
/// the timeline rather than on projection offsets.
pub(super) struct TurnCaptureState {
    /// First event sequence owned by this turn.
    pub turn_start_seq: EventSeq,
    /// Whether compaction occurred during this capture.
    pub compaction_occurred: bool,
}

impl ChatState {
    /// Create a new `ChatState` with the given conversation and sampling config,
    /// all other fields defaulted.
    ///
    /// Repairs any dangling tool calls in the initial conversation. This handles
    /// the race condition where the process was killed mid-tool-execution and
    /// `chat_history.jsonl` has an assistant message with tool call IDs that
    /// lack matching `ToolResult` entries. Without this, the in-memory state
    /// would carry broken conversation history until the next `build_request`.
    pub fn new(mut conversation: Vec<ConversationItem>, sampling_config: SamplingConfig) -> Self {
        let deduped = dedup_duplicate_tool_results(&mut conversation);
        if deduped > 0 {
            tracing::info!(
                deduped_count = deduped,
                "Removed duplicate tool results in initial conversation"
            );
        }
        let repaired =
            repair_dangling_tool_calls(&mut conversation, DanglingToolCallReason::UserCancelled);
        if repaired > 0 {
            tracing::info!(
                repaired_count = repaired,
                "Repaired dangling tool calls in initial conversation (likely from a previous crash)"
            );
        }

        let timeline = Timeline::from_seed(conversation)
            .expect("an in-memory seed conversation must form a valid timeline");

        Self::from_timeline(timeline, sampling_config)
    }

    /// Restore state from an already validated durable timeline.
    pub fn from_timeline(timeline: Timeline, sampling_config: SamplingConfig) -> Self {
        let initial_tokens = estimate_conversation_tokens(timeline.surface());

        Self {
            timeline,
            sampling_config,
            prompt_index: 0,
            prompt_texts: Vec::new(),
            total_tokens: initial_tokens,
            stream_start_ms: None,
            turn_start_ms: None,
            agent_edited_paths: BTreeSet::new(),
            last_compaction_prompt_index: None,
            credentials: Credentials::default(),
            estimated_tokens_since_model: 0,
            estimate_at_last_response: initial_tokens,
            last_turn_usage: None,
            prompt_usage: None,
            session_usage: UsageLedger::default(),
            turn_capture: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sampling_config() -> SamplingConfig {
        SamplingConfig {
            base_url: "https://api.example.com".to_string(),
            model: "test-model".to_string(),
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: Default::default(),
            extra_headers: Default::default(),
            query_params: Default::default(),
            env_http_headers: Default::default(),
            context_window: std::num::NonZeroU64::new(128_000).unwrap(),
            reasoning_effort: None,
            stream_tool_calls: None,
        }
    }

    #[test]
    fn estimated_item_token_counter_matches_estimate_item_tokens() {
        use compaction::ItemTokenCounter;

        let counter = EstimatedItemTokenCounter;
        let items = vec![
            ConversationItem::system("you are a helpful assistant"),
            ConversationItem::user("fix the login bug in auth.rs"),
            ConversationItem::assistant("let me look at the file"),
            ConversationItem::tool_result("tc1", "fn login() {}"),
        ];
        for item in &items {
            assert_eq!(
                u64::from(counter.count_item_tokens(item)),
                estimate_item_tokens(item),
                "counter must report the same trusted count as estimate_item_tokens"
            );
        }
    }

    #[test]
    fn new_state_has_correct_defaults() {
        let state = ChatState::new(vec![], test_sampling_config());
        assert_eq!(state.prompt_index, 0);
        assert_eq!(state.total_tokens, 0); // empty conversation → 0
        assert!(state.timeline.surface().is_empty());
        assert!(state.agent_edited_paths.is_empty());
        assert!(state.prompt_texts.is_empty());
        assert!(state.stream_start_ms.is_none());
        assert!(state.turn_start_ms.is_none());
        assert!(state.last_compaction_prompt_index.is_none());
    }

    #[test]
    fn new_state_preserves_initial_conversation() {
        let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("hello"),
        ];
        let state = ChatState::new(items, test_sampling_config());
        assert_eq!(state.timeline.surface_len(), 2);
    }

    #[test]
    fn new_state_estimates_tokens_from_conversation() {
        // 4000 bytes of text per item, bytes / 4 = 1000 tokens each
        let items = vec![
            ConversationItem::system("x".repeat(4000).as_str()),
            ConversationItem::user("y".repeat(4000).as_str()),
            ConversationItem::assistant("z".repeat(4000).as_str()),
            ConversationItem::tool_result("call-1", "w".repeat(4000).as_str()),
        ];
        let state = ChatState::new(items, test_sampling_config());
        assert_eq!(state.total_tokens, 4000); // 4 * (4000/4)
    }

    #[test]
    fn estimate_system_message_tokens_only_counts_system_items() {
        let sys = ConversationItem::system("a".repeat(400));
        assert_eq!(estimate_system_message_tokens(&sys), 100);
        let user = ConversationItem::user("hello");
        assert_eq!(estimate_system_message_tokens(&user), 0);
        let asst = ConversationItem::assistant("hi");
        assert_eq!(estimate_system_message_tokens(&asst), 0);
        let tr = ConversationItem::tool_result("call-1", "x".repeat(4000).as_str());
        assert_eq!(estimate_system_message_tokens(&tr), 0);
    }

    #[test]
    fn estimate_tool_definition_tokens_counts_name_desc_params() {
        // Empty parameters serialize to "null" (4 bytes) in the JSON-string len
        let td = sampling_types::ToolDefinition::function(
            "search",
            Some("find a file"),
            serde_json::json!({}),
        );
        // name=6 + desc=11 + params=`{}`.len()=2 = 19, /4 = 4
        assert_eq!(estimate_tool_definition_tokens(&td), 4);
    }

    #[test]
    fn estimate_messages_tokens_excludes_system_and_sums_rest() {
        // 4000 bytes per item -> 1000 tokens each.
        let items = vec![
            ConversationItem::system("x".repeat(4000).as_str()),
            ConversationItem::user("y".repeat(4000).as_str()),
            ConversationItem::assistant("z".repeat(4000).as_str()),
            ConversationItem::tool_result("call-1", "w".repeat(4000).as_str()),
        ];
        // Total = 4000 (4 items * 1000), system = 1000, messages = 3000.
        assert_eq!(estimate_conversation_tokens(&items), 4000);
        assert_eq!(estimate_messages_tokens(&items), 3000);
    }

    #[test]
    fn estimate_messages_tokens_zero_when_only_system() {
        let items = vec![ConversationItem::system("x".repeat(4000).as_str())];
        assert_eq!(estimate_messages_tokens(&items), 0);
    }

    #[test]
    fn estimate_messages_tokens_zero_for_empty() {
        assert_eq!(estimate_messages_tokens(&[]), 0);
    }

    #[test]
    fn estimate_tool_definitions_tokens_sums_across_slice() {
        let a = sampling_types::ToolDefinition::function("a", None::<&str>, serde_json::json!({}));
        let b = sampling_types::ToolDefinition::function("b", None::<&str>, serde_json::json!({}));
        let single = estimate_tool_definition_tokens(&a);
        assert_eq!(estimate_tool_definitions_tokens(&[a, b]), single * 2);
    }
}

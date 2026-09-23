//! Internal state types for the ChatStateActor.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use sampling_types::{
    ConversationItem, ConversationRequest, DanglingToolCallReason, JsonOutputFormat,
    NativeContinuationFragment, NativeContinuationProjection, NativeContinuationSpan,
    SamplingConfig, TokenUsage, dedup_duplicate_tool_results,
    project_portable_history_with_reasoning, repair_dangling_tool_calls,
};

use crate::types::Credentials;
use crate::usage::UsageLedger;
use crate::{EventSeq, SurfaceId, Timeline};

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
    estimate_tool_tokens(
        &td.function.name,
        td.function.description.as_deref(),
        &td.function.parameters,
    )
}

/// Sum [`estimate_tool_definition_tokens`] across a slice.
pub fn estimate_tool_definitions_tokens(tds: &[sampling_types::ToolDefinition]) -> u64 {
    tds.iter().map(estimate_tool_definition_tokens).sum()
}

/// Sum the same estimate across provider-facing tool specs.
pub fn estimate_tool_specs_tokens(tools: &[sampling_types::ToolSpec]) -> u64 {
    tools
        .iter()
        .map(|tool| estimate_tool_tokens(&tool.name, tool.description.as_deref(), &tool.parameters))
        .sum()
}

/// Estimate the provider-visible input envelope. Sampling controls and model
/// routing do not consume context tokens; messages, tool schemas, tool choice,
/// and native output schemas do.
pub fn estimate_request_input_tokens(request: &ConversationRequest) -> u64 {
    let tool_choice_tokens = request
        .tool_choice
        .as_ref()
        .and_then(|choice| serde_json::to_string(choice).ok())
        .map_or(0, |choice| token_estimation::estimate_tokens(&choice));
    let output_schema_tokens = match request.json_output.as_ref() {
        None => 0,
        Some(JsonOutputFormat::JsonObject) => token_estimation::estimate_tokens("json_object"),
        Some(JsonOutputFormat::JsonSchema(schema)) => serde_json::to_string(schema)
            .map_or(0, |schema| token_estimation::estimate_tokens(&schema)),
    };
    estimate_effective_conversation_tokens(request)
        .saturating_add(estimate_tool_specs_tokens(&request.tools))
        .saturating_add(tool_choice_tokens)
        .saturating_add(output_schema_tokens)
}

fn estimate_wire_items(items: &[ConversationItem], include_reasoning: bool) -> u64 {
    items
        .iter()
        .filter(|item| include_reasoning || !matches!(item, ConversationItem::Reasoning(_)))
        .map(estimate_item_tokens)
        .sum()
}

fn estimate_effective_conversation_tokens(request: &ConversationRequest) -> u64 {
    let Some(native) = &request.native_continuation else {
        return estimate_wire_items(&request.items, false);
    };
    let reasoning_backend = native.portable_reasoning_backend.clone();
    let replay_reasoning = reasoning_backend.is_some();
    let Some(portable_end) = native.portable_prefix_end(&request.items) else {
        return estimate_wire_items(
            &project_portable_history_with_reasoning(&request.items, reasoning_backend),
            replay_reasoning,
        );
    };
    let mut total = estimate_wire_items(
        &project_portable_history_with_reasoning(&request.items[..portable_end], reasoning_backend),
        replay_reasoning,
    );
    let mut cursor = portable_end;
    for span in &native.spans {
        total = total
            .saturating_add(estimate_wire_items(
                &request.items[cursor..span.start],
                false,
            ))
            .saturating_add(span.fragment.estimated_tokens());
        cursor = span.end;
    }
    total.saturating_add(estimate_wire_items(&request.items[cursor..], false))
}

fn estimate_tool_tokens(
    name: &str,
    description: Option<&str>,
    parameters: &serde_json::Value,
) -> u64 {
    let bytes = name.len()
        + description.map_or(0, str::len)
        + serde_json::to_string(parameters).map_or(0, |value| value.len());
    (bytes as u64) / token_estimation::BYTES_PER_TOKEN
}

/// Estimate the provider-visible receive call/result pair for one runtime
/// message. The canonical item occupies one Surface coordinate, but its
/// request projection carries the stable receipt identity in both halves and
/// the complete source/body/reply metadata in the result.
fn estimate_agent_message_tokens(batch: &sampling_types::AgentMessageItem) -> u64 {
    batch
        .messages
        .iter()
        .map(|message| {
            let call_id = sampling_types::agent_message_call_id(&message.receipt_id);
            let arguments = serde_json::json!({
                "receipt_id": message.receipt_id,
                "source_session_id": message.source_session_id,
                "message_id": message.message_id,
            });
            let call = serde_json::json!({
                "role": "assistant",
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": sampling_types::RECEIVE_AGENT_MESSAGE_TOOL_NAME,
                        "arguments": arguments.to_string(),
                    },
                }],
            });
            let result = serde_json::json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": sampling_types::agent_message_result_content(message),
            });
            token_estimation::estimate_tokens(&format!("{call}{result}"))
        })
        .sum()
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
        ConversationItem::ToolResult(tr) => {
            let mut bytes = tr.content.len();
            let mut images = 0;
            for part in &tr.images {
                match part {
                    ContentPart::Text { text } => bytes += text.len(),
                    ContentPart::Image { .. } => images += 1,
                }
            }
            bytes as u64 / token_estimation::BYTES_PER_TOKEN
                + token_estimation::estimate_image_tokens(images)
        }
        ConversationItem::AgentMessage(batch) => estimate_agent_message_tokens(batch),
        ConversationItem::BackendToolCall(b) => {
            token_estimation::estimate_tokens(&b.text_summary())
        }
        ConversationItem::Reasoning(r) => token_estimation::estimate_tokens(&r.text),
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
/// per-variant arithmetic (images, reasoning blobs, tool-call args and runtime
/// receive pairs) stays in one place.
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
    /// Non-durable provider continuation for the current sampling epoch.
    pub continuation: ContinuationLane,
    /// Current sampling configuration (model, context window, etc.).
    pub sampling_config: SamplingConfig,
    /// Provider-anchored projection of the current model-visible context.
    /// Lifetime and per-prompt billing live exclusively in `UsageLedger`.
    pub projected_tokens: u64,
    /// Canonical Surface estimate at the last final request projection.
    pub projected_request_surface_tokens: u64,
    /// Provider-visible input estimate at the last final request projection.
    /// The difference from `projected_request_surface_tokens` is the sole
    /// request-envelope adjustment carried across provider anchors.
    pub projected_request_input_tokens: u64,
    /// Timestamp when the current stream started (epoch ms).
    pub stream_start_ms: Option<i64>,
    /// Timestamp when the current turn started (epoch ms).
    pub turn_start_ms: Option<i64>,
    /// File paths the agent has edited.
    pub agent_edited_paths: BTreeSet<String>,
    /// Opaque credential secrets (api key, optional extra auth, client version).
    /// Stored opaquely — the actor never interprets them.
    pub credentials: Credentials,
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
    /// Lifetime session usage rebuilt from durable Timeline settlement facts.
    pub session_usage: UsageLedger,
    /// Durable attempt facts already applied in this actor epoch. This is
    /// reconstructed from Timeline observations so a replay cannot charge a
    /// settled attempt a second time.
    pub(crate) settled_model_attempts: BTreeMap<String, AttemptUsageSettlement>,
    /// Durable child bills already folded into the parent lifetime ledger.
    pub(crate) settled_subagent_usage: BTreeMap<String, SubagentUsageSettlement>,
    /// Event-sequence turn capture state. `Some` = capture active, `None` = inactive.
    /// Cleared on `TakeTurnMessages` (consumed), `BeginTurnCapture` (new turn),
    /// and the durable rewind transaction (which abandons the turn capture).
    pub(super) turn_capture: Option<TurnCaptureState>,
}

/// The immutable payload recorded for one model-attempt settlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttemptUsageSettlement {
    pub(crate) attempt_key: String,
    pub(crate) model_id: String,
    pub(crate) captured_prompt_index: usize,
    pub(crate) usage: Option<TokenUsage>,
    pub(crate) cost_usd_ticks: Option<i64>,
    pub(crate) api_duration_ms: Option<u64>,
}

/// Immutable terminal bill folded from one child into its parent session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubagentUsageSettlement {
    pub(crate) subagent_id: String,
    pub(crate) by_model: Vec<(String, crate::usage::UsageTotals)>,
    pub(crate) incomplete: bool,
}

impl AttemptUsageSettlement {
    pub(crate) fn check_duplicate(
        &self,
        existing: Option<&Self>,
    ) -> Result<bool, crate::TimelineWriteError> {
        let Some(existing) = existing else {
            return Ok(false);
        };
        if existing.matches(
            &self.model_id,
            self.captured_prompt_index,
            self.usage.as_ref(),
            self.cost_usd_ticks,
            self.api_duration_ms,
        ) {
            Ok(true)
        } else {
            Err(crate::TimelineWriteError::AttemptUsageConflict)
        }
    }

    pub(crate) fn matches(
        &self,
        model_id: &str,
        captured_prompt_index: usize,
        usage: Option<&TokenUsage>,
        cost_usd_ticks: Option<i64>,
        api_duration_ms: Option<u64>,
    ) -> bool {
        self.model_id == model_id
            && self.captured_prompt_index == captured_prompt_index
            && token_usage_matches(self.usage.as_ref(), usage)
            && self.cost_usd_ticks == cost_usd_ticks
            && self.api_duration_ms == api_duration_ms
    }
}

impl SubagentUsageSettlement {
    pub(crate) fn check_duplicate(
        &self,
        existing: Option<&Self>,
    ) -> Result<bool, crate::TimelineWriteError> {
        match existing {
            None => Ok(false),
            Some(existing) if existing == self => Ok(true),
            Some(_) => Err(crate::TimelineWriteError::SubagentUsageConflict),
        }
    }
}

fn token_usage_matches(left: Option<&TokenUsage>, right: Option<&TokenUsage>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.prompt_tokens == right.prompt_tokens
                && left.completion_tokens == right.completion_tokens
                && left.total_tokens == right.total_tokens
                && left.reasoning_tokens == right.reasoning_tokens
                && left.cached_prompt_tokens == right.cached_prompt_tokens
                && left.cache_creation_prompt_tokens == right.cache_creation_prompt_tokens
        }
        _ => false,
    }
}

fn usage_from_timeline(
    timeline: &Timeline,
) -> Result<
    (
        BTreeMap<String, AttemptUsageSettlement>,
        BTreeMap<String, SubagentUsageSettlement>,
        UsageLedger,
    ),
    crate::TimelineWriteError,
> {
    let mut attempts = BTreeMap::new();
    let mut subagents = BTreeMap::new();
    let mut ledger = UsageLedger::default();
    ledger.initialize_segment(timeline.events().first().map(|event| event.at_ms));

    for event in timeline.events() {
        let crate::TimelineEventKind::Observation(observation) = &event.kind else {
            continue;
        };
        let invalid = |reason: String| crate::TimelineWriteError::InvalidUsageObservation {
            seq: event.seq.get(),
            scope: observation.scope.clone(),
            name: observation.name.clone(),
            reason,
        };
        match (observation.scope.as_str(), observation.name.as_str()) {
            ("session_usage", "resume_started" | "incomplete") => {
                if observation.data.is_some() {
                    return Err(invalid("marker must not contain data".into()));
                }
                if observation.name == "resume_started" {
                    ledger.begin_resume_segment(event.seq, event.at_ms);
                } else {
                    ledger.mark_incomplete();
                }
            }
            ("sampling_usage", "attempt_settled") => {
                let data = observation
                    .data
                    .as_ref()
                    .ok_or_else(|| invalid("missing settlement data".into()))?;
                let payload = AttemptUsageSettlement::deserialize(data)
                    .map_err(|error| invalid(error.to_string()))?;
                if payload.check_duplicate(attempts.get(&payload.attempt_key))? {
                    continue;
                }
                if let Some(usage) = payload.usage.as_ref() {
                    ledger.record_main_loop_call(
                        &payload.model_id,
                        usage,
                        payload.api_duration_ms,
                        payload.cost_usd_ticks,
                    );
                } else {
                    ledger.mark_incomplete();
                }
                attempts.insert(payload.attempt_key.clone(), payload);
            }
            ("session_usage", "subagent_settled") => {
                let data = observation
                    .data
                    .as_ref()
                    .ok_or_else(|| invalid("missing settlement data".into()))?;
                let payload = SubagentUsageSettlement::deserialize(data)
                    .map_err(|error| invalid(error.to_string()))?;
                if payload.check_duplicate(subagents.get(&payload.subagent_id))? {
                    continue;
                }
                ledger.record_subagent(&payload.subagent_id, &payload.by_model, payload.incomplete);
                subagents.insert(payload.subagent_id.clone(), payload);
            }
            _ => {}
        }
    }

    Ok((attempts, subagents, ledger))
}

#[derive(Debug, Clone)]
struct NativeSpanRecord {
    surface_ids: Vec<SurfaceId>,
    fragment: NativeContinuationFragment,
}

#[derive(Debug, Clone)]
pub(crate) struct ContinuationLane {
    backend: sampling_types::ApiBackend,
    epoch_nonce: String,
    portable_prefix_len: usize,
    observed_projection: Vec<(SurfaceId, blake3::Hash)>,
    spans: Vec<NativeSpanRecord>,
    portable_reasoning_backend: Option<sampling_types::ApiBackend>,
}

impl ContinuationLane {
    fn new(backend: sampling_types::ApiBackend, portable_prefix_len: usize) -> Self {
        Self {
            backend,
            epoch_nonce: uuid::Uuid::now_v7().to_string(),
            portable_prefix_len,
            observed_projection: Vec::new(),
            spans: Vec::new(),
            portable_reasoning_backend: None,
        }
    }

    pub(super) fn reset(
        &mut self,
        backend: sampling_types::ApiBackend,
        portable_prefix_len: usize,
        reason: &'static str,
    ) {
        let portable_reasoning_backend = self
            .portable_reasoning_backend
            .clone()
            .filter(|learned| *learned == backend);
        tracing::info!(
            reason,
            ?backend,
            native_spans = self.spans.len(),
            "reset native continuation epoch"
        );
        *self = Self::new(backend, portable_prefix_len);
        self.portable_reasoning_backend = portable_reasoning_backend;
    }

    pub(super) fn replace_route(
        &mut self,
        backend: sampling_types::ApiBackend,
        portable_prefix_len: usize,
    ) {
        tracing::info!(
            ?backend,
            native_spans = self.spans.len(),
            "replace native continuation route"
        );
        *self = Self::new(backend, portable_prefix_len);
    }

    pub(super) fn enable_portable_reasoning(
        &mut self,
        backend: sampling_types::ApiBackend,
        changes_history: bool,
    ) -> bool {
        if self.backend != backend
            || backend == sampling_types::ApiBackend::Messages
            || !changes_history
            || self.portable_reasoning_backend.is_some()
        {
            return false;
        }
        self.portable_reasoning_backend = Some(backend);
        true
    }

    pub(super) fn portable_reasoning_backend(&self) -> Option<sampling_types::ApiBackend> {
        self.portable_reasoning_backend.clone()
    }

    pub(super) fn epoch_nonce(&self) -> &str {
        &self.epoch_nonce
    }

    pub(super) fn reconcile_projection(
        &mut self,
        surface_ids: &[SurfaceId],
        items: &[ConversationItem],
    ) -> bool {
        if surface_ids.len() != items.len() {
            self.reset(
                self.backend.clone(),
                items.len(),
                "request_projection_identity_mismatch",
            );
            return false;
        }
        let current: Vec<_> = surface_ids
            .iter()
            .copied()
            .zip(items.iter().map(|item| {
                let bytes = serde_json::to_vec(item).expect("conversation item must serialize");
                blake3::hash(&bytes)
            }))
            .collect();
        let pure_append = current.len() >= self.observed_projection.len()
            && current[..self.observed_projection.len()] == self.observed_projection;
        if !self.observed_projection.is_empty() && !pure_append {
            self.reset(
                self.backend.clone(),
                current.len(),
                "request_projection_changed",
            );
        }
        self.observed_projection = current;
        true
    }

    pub(super) fn install(
        &mut self,
        surface_ids: Vec<SurfaceId>,
        fragment: NativeContinuationFragment,
    ) -> bool {
        if fragment.backend() != self.backend || fragment.is_empty() || surface_ids.is_empty() {
            return false;
        }
        self.spans.push(NativeSpanRecord {
            surface_ids,
            fragment,
        });
        true
    }

    pub(super) fn request_projection(
        &self,
        current_ids: &[SurfaceId],
    ) -> NativeContinuationProjection {
        let mut spans = Vec::with_capacity(self.spans.len());
        for record in &self.spans {
            let Some(start) = record
                .surface_ids
                .first()
                .and_then(|first| current_ids.iter().position(|id| id == first))
            else {
                continue;
            };
            let end = start + record.surface_ids.len();
            if current_ids.get(start..end) == Some(record.surface_ids.as_slice()) {
                spans.push(NativeContinuationSpan {
                    start,
                    end,
                    fragment: record.fragment.clone(),
                });
            }
        }
        NativeContinuationProjection {
            portable_prefix_len: self.portable_prefix_len.min(current_ids.len()),
            spans,
            portable_reasoning_backend: self.portable_reasoning_backend.clone(),
        }
    }
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
    /// the restored Timeline Surface has an assistant message with tool call IDs that
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
            .expect("an in-memory seed has no persisted usage observations")
    }

    /// Restore state from an already validated durable timeline.
    pub fn from_timeline(
        timeline: Timeline,
        sampling_config: SamplingConfig,
    ) -> Result<Self, crate::TimelineWriteError> {
        let initial_tokens = estimate_conversation_tokens(timeline.surface());
        let (settled_model_attempts, settled_subagent_usage, session_usage) =
            usage_from_timeline(&timeline)?;

        Ok(Self {
            continuation: ContinuationLane::new(
                sampling_config.api_backend.clone(),
                timeline.surface_len(),
            ),
            timeline,
            sampling_config,
            projected_tokens: initial_tokens,
            projected_request_surface_tokens: initial_tokens,
            projected_request_input_tokens: initial_tokens,
            stream_start_ms: None,
            turn_start_ms: None,
            agent_edited_paths: BTreeSet::new(),
            credentials: Credentials::default(),
            last_turn_usage: None,
            prompt_usage: None,
            session_usage,
            settled_model_attempts,
            settled_subagent_usage,
            turn_capture: None,
        })
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
        assert_eq!(state.timeline.next_prompt_index(), 0);
        assert_eq!(state.projected_tokens, 0); // empty conversation → 0
        assert!(state.timeline.surface().is_empty());
        assert!(state.agent_edited_paths.is_empty());
        assert!(state.timeline.prompt_records().is_empty());
        assert!(state.stream_start_ms.is_none());
        assert!(state.turn_start_ms.is_none());
        assert!(
            state
                .timeline
                .last_completed_compaction_prompt_index()
                .is_none()
        );
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
        assert_eq!(state.projected_tokens, 4000); // 4 * (4000/4)
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
    fn estimate_agent_message_tokens_includes_projected_identity_and_result() {
        let item = ConversationItem::received_agent_message(sampling_types::AgentMessage {
            receipt_id: "receipt-1".into(),
            source_session_id: "source-session".into(),
            target_session_id: "target-session".into(),
            message_id: "message-1".into(),
            reply_to: Some(sampling_types::AgentMessageRef {
                source_session_id: "target-session".into(),
                message_id: "parent-1".into(),
            }),
            message: "body\twith\r\nsource identity".into(),
        });

        let tokens = estimate_item_tokens(&item);
        assert!(tokens > 0);
        assert!(tokens > token_estimation::estimate_tokens("body\twith\r\nsource identity"));
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

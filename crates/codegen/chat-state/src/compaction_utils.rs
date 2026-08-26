//! Pure utility functions and types for compaction support.
//!
//! These are stateless functions that operate on conversation data only —
//! no I/O, no actor state. They live in `chat-state` so that both
//! this crate and `shell` can share them without duplication.
use sampling_types::{ContentPart, ConversationItem};
use std::collections::BTreeSet;

/// Return the exclusive end index of every causally complete user turn.
///
/// A turn begins with one or more consecutive User items and is complete only
/// after at least one Assistant item and every tool call emitted by each
/// Assistant has a matching ToolResult before the next Assistant or User.
/// Reasoning, backend-tool projections, and unmatched historical ToolResults
/// are transparent. Once a malformed or incomplete turn is reached, later
/// items are not considered independently complete.
///
/// This is the single turn-boundary definition used by fork truncation and
/// child-context summarization.
pub fn complete_turn_ends<'a>(items: impl IntoIterator<Item = &'a ConversationItem>) -> Vec<usize> {
    let items = items.into_iter().collect::<Vec<_>>();
    let mut turn_ends = Vec::new();
    let mut index = 0;

    while index < items.len() {
        while index < items.len() && !matches!(items[index], ConversationItem::User(_)) {
            index += 1;
        }
        if index == items.len() {
            break;
        }
        while index < items.len() && matches!(items[index], ConversationItem::User(_)) {
            index += 1;
        }

        let mut saw_assistant = false;
        let mut pending_tool_calls = std::collections::HashSet::<&str>::new();
        let mut malformed = false;
        while index < items.len()
            && !matches!(
                items[index],
                ConversationItem::User(_) | ConversationItem::System(_)
            )
        {
            match items[index] {
                ConversationItem::Assistant(assistant) => {
                    if !pending_tool_calls.is_empty() {
                        malformed = true;
                        break;
                    }
                    saw_assistant = true;
                    pending_tool_calls.extend(
                        assistant
                            .tool_calls
                            .iter()
                            .map(|tool_call| tool_call.id.as_ref()),
                    );
                }
                ConversationItem::ToolResult(result) => {
                    pending_tool_calls.remove(result.tool_call_id.as_str());
                }
                ConversationItem::Reasoning(_) | ConversationItem::BackendToolCall(_) => {}
                ConversationItem::User(_) | ConversationItem::System(_) => unreachable!(),
            }
            index += 1;
        }

        if malformed || !saw_assistant || !pending_tool_calls.is_empty() {
            break;
        }
        turn_ends.push(index);
    }

    turn_ends
}

/// Drops tool results and flattens assistant `tool_calls` into
/// `[Called tools: ...]` text annotations.
///
/// Mutates assistant text in place; do NOT use this directly when sending
/// to a provider that validates signed `reasoning` blocks against the
/// surrounding content. Use [`prepare_conversation_for_summarization`]
/// instead, which also strips `reasoning` so the mutation is safe.
pub(crate) fn strip_tool_messages_for_conversation_item(
    conversation: Vec<ConversationItem>,
) -> Vec<ConversationItem> {
    conversation
        .into_iter()
        .filter_map(|item| match item {
            ConversationItem::ToolResult(_) => None,
            ConversationItem::Assistant(mut a) => {
                if !a.tool_calls.is_empty() {
                    let tool_names: Vec<String> =
                        a.tool_calls.iter().map(|tc| tc.name.clone()).collect();
                    let tool_info = format!("\n[Called tools: {}]", tool_names.join(", "));
                    a.content = if a.content.is_empty() {
                        std::sync::Arc::<str>::from(tool_info)
                    } else {
                        let mut s = String::with_capacity(a.content.len() + tool_info.len());
                        s.push_str(&a.content);
                        s.push_str(&tool_info);
                        std::sync::Arc::<str>::from(s)
                    };
                    a.tool_calls.clear();
                }
                Some(ConversationItem::Assistant(a))
            }
            other => Some(other),
        })
        .collect()
}
/// Drops every `ConversationItem::Reasoning(_)` sibling.
///
/// Required before sending to backends that reject the structured reasoning
/// shape (signed `Thinking` blocks after text mutation; some Chat Completions
/// providers entirely) and before summarization.
pub fn strip_reasoning_blocks(conversation: Vec<ConversationItem>) -> Vec<ConversationItem> {
    conversation
        .into_iter()
        .filter(|item| !matches!(item, ConversationItem::Reasoning(_)))
        .collect()
}
/// Prepare a conversation for a summarization call (compaction or memory flush).
///
/// Combines `strip_tool_messages_for_conversation_item` (drops tool
/// results, flattens `tool_calls` into text annotations) and
/// `strip_reasoning_blocks`.
///
/// The reasoning strip is required because the text mutation in the
/// tool-message step would invalidate signed `thinking` blocks, which
/// strict providers reject with a 400.
///
/// Images are never erased here. A known text-only runtime must first commit
/// the canonical ImageDescription Sideband + irreversible ImageProjection;
/// an unknown route receives the real image or fails without mutating history.
pub fn prepare_conversation_for_summarization(
    conversation: Vec<ConversationItem>,
) -> Vec<ConversationItem> {
    strip_reasoning_blocks(strip_tool_messages_for_conversation_item(conversation))
}
/// Drop a trailing assistant turn whose `tool_calls` lack a `ToolResult` (else strict backends reject the dangling `tool_use`).
pub fn truncate_trailing_incomplete_tool_call(
    mut conversation: Vec<ConversationItem>,
) -> Vec<ConversationItem> {
    while matches!(
        conversation.last(),
        Some(ConversationItem::Assistant(a)) if !a.tool_calls.is_empty()
    ) {
        conversation.pop();
    }
    conversation
}
/// Cache-aligned summarizer prep: keep tool I/O + images so the prefix matches the engine cache; set `strip_reasoning` when the provider rejects mutated thinking blocks.
pub fn prepare_conversation_for_verbatim_summarization(
    conversation: Vec<ConversationItem>,
    strip_reasoning: bool,
) -> Vec<ConversationItem> {
    let conversation = if strip_reasoning {
        strip_reasoning_blocks(conversation)
    } else {
        conversation
    };
    truncate_trailing_incomplete_tool_call(conversation)
}
/// Per-item token estimate via the trigger-side estimator, so `fit`'s budget matches what fired the compaction (counts images + encrypted reasoning).
fn estimate_item_tokens(item: &ConversationItem) -> u64 {
    crate::actor::state::estimate_item_tokens(item)
}
/// Shrink a verbatim conversation to `max_tokens`: drop oldest whole turns (System kept, tool runs unsplit; the last turn is truncated in place rather than dropped).
pub fn fit_conversation_to_budget(
    conversation: Vec<ConversationItem>,
    max_tokens: u64,
) -> Vec<ConversationItem> {
    let total: u64 = conversation.iter().map(estimate_item_tokens).sum();
    if total <= max_tokens {
        return conversation;
    }
    let mut head: Vec<ConversationItem> = Vec::new();
    let mut body: Vec<ConversationItem> = conversation;
    if matches!(body.first(), Some(ConversationItem::System(_))) {
        head.push(body.remove(0));
    }
    let budget = max_tokens.saturating_sub(head.iter().map(estimate_item_tokens).sum::<u64>());
    let mut remaining = budget;
    let mut start = body.len();
    for i in (0..body.len()).rev() {
        let cost = estimate_item_tokens(&body[i]);
        if cost > remaining {
            break;
        }
        remaining -= cost;
        start = i;
    }
    while start < body.len() && matches!(body[start], ConversationItem::ToolResult(_)) {
        start += 1;
    }
    if start < body.len() {
        head.extend(body.into_iter().skip(start));
    } else {
        head.extend(recover_truncated_tail_unit(body, budget));
    }
    head
}
/// Keep the most-recent turn but truncate its content to `budget` (with its owning `tool_use`) instead of dropping it.
fn recover_truncated_tail_unit(
    mut body: Vec<ConversationItem>,
    budget: u64,
) -> Vec<ConversationItem> {
    let mut results: Vec<ConversationItem> = Vec::new();
    while matches!(body.last(), Some(ConversationItem::ToolResult(_))) {
        results.push(body.pop().expect("last() was Some"));
    }
    results.reverse();
    if results.is_empty() {
        return match body.pop() {
            Some(item) => vec![truncate_item_to_tokens(item, budget)],
            None => Vec::new(),
        };
    }
    let owner = if matches!(
        body.last(),
        Some(ConversationItem::Assistant(a)) if !a.tool_calls.is_empty()
    ) {
        body.pop()
    } else {
        None
    };
    let owner_cost = owner.as_ref().map(estimate_item_tokens).unwrap_or(0);
    let result_budget = budget.saturating_sub(owner_cost);
    let per = (result_budget / results.len() as u64).max(1);
    let mut unit: Vec<ConversationItem> = Vec::new();
    if let Some(o) = owner {
        unit.push(o);
    }
    unit.extend(results.into_iter().map(|r| truncate_item_to_tokens(r, per)));
    unit
}
/// Truncate one item's content text to at most `max_tokens`, appending a `[... truncated N bytes ...]` marker (structural fields kept).
fn truncate_item_to_tokens(item: ConversationItem, max_tokens: u64) -> ConversationItem {
    let max_bytes = (max_tokens as usize).saturating_mul(4);
    match item {
        ConversationItem::ToolResult(mut t) => {
            if let Some(s) = truncate_text_to_bytes(&t.content, max_bytes) {
                t.content = s;
            }
            ConversationItem::ToolResult(t)
        }
        ConversationItem::Assistant(mut a) => {
            if let Some(s) = truncate_text_to_bytes(&a.content, max_bytes) {
                a.content = s;
            }
            ConversationItem::Assistant(a)
        }
        ConversationItem::User(mut u) => {
            for part in &mut u.content {
                if let ContentPart::Text { text } = part
                    && let Some(s) = truncate_text_to_bytes(text, max_bytes)
                {
                    *text = s;
                }
            }
            ConversationItem::User(u)
        }
        other => other,
    }
}
/// Char-boundary-safe prefix of `s` (incl. truncation marker) within `max_bytes`; `None` if `s` already fits.
fn truncate_text_to_bytes(s: &str, max_bytes: usize) -> Option<std::sync::Arc<str>> {
    if s.len() <= max_bytes {
        return None;
    }
    const MARKER_RESERVE: usize = 64;
    let keep = max_bytes.saturating_sub(MARKER_RESERVE);
    let mut end = keep.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let dropped = s.len() - end;
    Some(std::sync::Arc::<str>::from(format!(
        "{}\n[... truncated {dropped} bytes to fit the compaction window ...]",
        &s[..end]
    )))
}
/// Tags injected by the runtime that should be stripped from user queries.
const SYSTEM_TAGS: &[&str] = &[
    "runtime_context",
    "user_info",
    "project_layout",
    "git_status",
    "jj_status",
    "fork-context",
    "system-reminder",
    "agent-memory",
    "system_reminder",
    "background_context",
    "command-name",
    "command-message",
    "command-args",
];
/// Strip all known system/metadata tag blocks from `text`.
///
/// For each tag in [`SYSTEM_TAGS`], removes every `<tag>…</tag>` occurrence
/// (including content). Unclosed tags are left untouched.
fn strip_system_tags(text: &str) -> String {
    let mut result = text.to_string();
    for tag in SYSTEM_TAGS {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        while let Some(start) = result.find(&open) {
            if let Some(rel_end) = result[start..].find(&close) {
                let end_pos = start + rel_end + close.len();
                result.replace_range(start..end_pos, "");
            } else {
                break;
            }
        }
    }
    result.trim().to_string()
}
/// Extracts the user query from a message that may contain metadata tags.
///
/// Looks for content within `<user_query>...</user_query>` tags.
/// If not found, strips known metadata tags (see [`SYSTEM_TAGS`]) and
/// returns the remaining content.
pub fn extract_user_query(text: &str) -> String {
    if let Some(start) = text.find("<user_query>") {
        let content_start = start + "<user_query>".len();
        if let Some(end) = text[content_start..].find("</user_query>") {
            let inner = text[content_start..content_start + end].trim();
            return strip_system_tags(inner);
        }
    }
    strip_system_tags(text)
}
/// Extract the last actual user query text (stripping metadata tags).
///
/// Walks backward through the conversation, finds the last `User` item,
/// and extracts the raw query via [`extract_user_query`].
pub fn extract_last_user_query(conversation: &[ConversationItem]) -> Option<String> {
    conversation
        .iter()
        .rev()
        .find(|item| matches!(item, ConversationItem::User(_)))
        .map(|item| extract_user_query(&item.text_content()))
        .filter(|q| !q.is_empty())
}
/// The continuation prompt added to the conversation after auto-compaction.
///
/// Stored here (rather than only in `shell`) so that query-extraction
/// helpers in this crate can recognise and exclude it from "real user prompt"
/// lists without creating a circular dependency or hard-coding the text in two
/// places.
pub const AUTO_CONTINUE_PROMPT: &str = r#"Continue the conversation from where it left off without asking the user any further questions. Resume directly - do not acknowledge the summary, do not recap what was happening, do not preface with "I'll continue" or similar.
Pick up the last task as if the break never happened."#;
/// Prompt injected after a truncated response to continue generation.
///
/// Uses a user message (not assistant message) per Anthropic's Claude 4.6+
/// error recovery guidance. For Claude 4.5 and earlier, the partial assistant
/// response is already in conversation history as the last assistant turn --
/// this prompt simply asks the model to continue.
pub const TRUNCATION_CONTINUE_PROMPT: &str = r#"Your previous response was interrupted. Continue from where you left off. Do not repeat what you already said. Resume directly."#;
/// Return `true` when the *extracted* query text represents a synthetic
/// session-internal turn rather than a real human-authored prompt.
///
/// The cases handled:
/// - Empty string — the User item contained only metadata tags with no
///   `<user_query>` payload (bootstrap prefix on session start).
/// - `"__auto_continue__"` — the request-id sentinel sometimes stored inside
///   a `<user_query>` wrapper for identification purposes.
/// - The full [`AUTO_CONTINUE_PROMPT`] text — the actual message pushed into
///   the conversation after auto-compaction so the agent keeps progressing.
///   `extract_user_query` returns this as-is (no tags to strip), so it must
///   be explicitly excluded to avoid counting it as a real user query.
pub fn is_synthetic_extracted_query(text: &str) -> bool {
    text.is_empty()
        || text == "__auto_continue__"
        || text == AUTO_CONTINUE_PROMPT
        || text == TRUNCATION_CONTINUE_PROMPT
}
/// Classify whether a `ConversationItem` is a **real** user turn for
/// compaction purposes.
///
/// A user item is NOT a real user turn if any of the following hold:
/// 1. It is not a `User` variant at all.
/// 2. `synthetic_reason` is `Some(…)` (e.g. `SystemReminder`).
/// 3. It has no meaningful content: no images AND its extracted query
///    text is synthetic (empty, `__auto_continue__`, or the full
///    [`AUTO_CONTINUE_PROMPT`]).
///
/// Image-only user prompts (multimodal input with no text) ARE real
/// user turns — they must anchor the compaction boundary even though
/// they have no extractable text query.
///
/// This is the single source of truth for human-authored query extraction and
/// statistics. Compaction lifecycle boundaries use `UserItem::prompt_index`
/// instead, because runtime-owned Goal/task/monitor turns are also causal
/// prompt turns.
pub fn is_real_user_turn(item: &ConversationItem) -> bool {
    match item {
        ConversationItem::User(u) => {
            if u.synthetic_reason.is_some() {
                return false;
            }
            let has_images = u
                .content
                .iter()
                .any(|p| matches!(p, ContentPart::Image { .. }));
            if has_images {
                return true;
            }
            let extracted = extract_user_query(&item.text_content());
            !is_synthetic_extracted_query(&extracted)
        }
        _ => false,
    }
}

/// A contiguous, identity-stable Surface range selected for summary
/// compaction. Indices are only used to materialize the frozen input; the
/// durable transaction commits [`crate::SurfaceRange`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionRangePlan {
    pub target: crate::SurfaceRange,
    pub start_index: usize,
    pub end_index: usize,
    pub source_tokens: u64,
}

/// Select one old Surface range while retaining a recent verbatim tail.
///
/// The normal path shadows complete old prompt turns and starts the retained
/// tail at a causally recorded prompt boundary. This includes runtime-owned
/// turns such as Goal continuations: whether input was human-authored is not a
/// valid lifecycle boundary. For a single very long turn, it can instead
/// shadow older completed response groups after that prompt; the retained tail
/// starts only at a complete response-group boundary, never at a
/// `ToolResult` or in the middle of a
/// `[Reasoning, BackendToolCall, ..., Assistant]` group. `SurfaceId` is the
/// sole range identity—no parallel message-ID registry is introduced.
pub fn plan_compaction_range(
    surface: &[ConversationItem],
    surface_ids: &[crate::SurfaceId],
    retain_tokens: u64,
    min_source_tokens: u64,
) -> Option<CompactionRangePlan> {
    if surface.len() != surface_ids.len() || surface.len() < 2 {
        return None;
    }

    let mut suffix_tokens = vec![0u64; surface.len() + 1];
    for index in (0..surface.len()).rev() {
        suffix_tokens[index] =
            suffix_tokens[index + 1].saturating_add(estimate_item_tokens(&surface[index]));
    }
    let range_tokens = |start: usize, end: usize| {
        suffix_tokens[start].saturating_sub(suffix_tokens[end.saturating_add(1)])
    };
    let prompt_turns = surface
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            matches!(item, ConversationItem::User(user) if user.prompt_index.is_some())
                .then_some(index)
        })
        .collect::<Vec<_>>();
    let first_prompt = *prompt_turns.first()?;
    let last_prompt = *prompt_turns.last()?;
    let source_start = surface[..first_prompt]
        .iter()
        .rposition(|item| {
            !matches!(
                item,
                ConversationItem::User(user)
                    if user.synthetic_reason
                        == Some(sampling_types::SyntheticReason::CompactionMeta)
            )
        })
        .map_or(0, |index| index + 1);

    // Prefer complete old turns. Walk backwards until the retained suffix has
    // the desired budget, then keep everything from that prompt onward.
    // A prior summary immediately before the oldest live user turn belongs to
    // the same rolling context layer; absorb it into the next summary instead
    // of accumulating one permanent Surface node per compaction.
    let mut tail_start = last_prompt;
    for &candidate in prompt_turns.iter().rev() {
        tail_start = candidate;
        if suffix_tokens[candidate] >= retain_tokens {
            break;
        }
    }
    if tail_start > first_prompt {
        let end = tail_start - 1;
        let source_tokens = range_tokens(source_start, end);
        if source_tokens >= min_source_tokens {
            return range_plan(surface_ids, source_start, end, source_tokens);
        }
    }

    // A single active turn can itself exceed the window. Keep its real user
    // prompt and newest response groups verbatim, and summarize only older
    // completed response groups. A valid boundary is the first item in a
    // model response group, not merely any assistant-role item.
    let start = last_prompt.saturating_add(1);
    if start >= surface.len() {
        return None;
    }
    let mut boundary = None;
    for index in (start + 1..surface.len()).rev() {
        if suffix_tokens[index] < retain_tokens {
            continue;
        }
        if is_response_group_start(surface, start, index) {
            boundary = Some(index);
            break;
        }
    }
    let end = boundary?.checked_sub(1)?;
    let source_tokens = range_tokens(start, end);
    (source_tokens >= min_source_tokens)
        .then(|| range_plan(surface_ids, start, end, source_tokens))?
}

fn is_response_group_start(surface: &[ConversationItem], body_start: usize, index: usize) -> bool {
    if index < body_start || index >= surface.len() {
        return false;
    }
    if !matches!(
        &surface[index],
        ConversationItem::Assistant(_)
            | ConversationItem::Reasoning(_)
            | ConversationItem::BackendToolCall(_)
    ) {
        return false;
    }
    index == body_start
        || !matches!(
            &surface[index - 1],
            ConversationItem::Reasoning(_) | ConversationItem::BackendToolCall(_)
        )
}

fn range_plan(
    surface_ids: &[crate::SurfaceId],
    start_index: usize,
    end_index: usize,
    source_tokens: u64,
) -> Option<CompactionRangePlan> {
    let start = *surface_ids.get(start_index)?;
    let end = *surface_ids.get(end_index)?;
    Some(CompactionRangePlan {
        target: crate::SurfaceRange {
            start,
            end,
            shadowed: surface_ids.get(start_index..=end_index)?.to_vec(),
        },
        start_index,
        end_index,
        source_tokens,
    })
}
/// Extract all *real* user queries from a conversation, in order.
///
/// "Real" means the item passes [`is_real_user_turn`] — it has no
/// `synthetic_reason` and its extracted query text is not synthetic.
///
/// This is used by the session-end hooks and any logic that needs to
/// count or enumerate actual human-authored prompts without being
/// polluted by synthetic bootstrap messages or compaction artifacts.
pub fn extract_real_user_queries(conversation: &[ConversationItem]) -> Vec<String> {
    conversation
        .iter()
        .filter(|item| is_real_user_turn(item))
        .map(|item| extract_user_query(&item.text_content()))
        .collect()
}
/// Extract the last *real* user query text from a conversation.
///
/// Unlike [`extract_last_user_query`], this function skips synthetic turns
/// (system reminders, metadata-only bootstrap prefixes, auto-continue
/// prompts) so it always returns content the user actually typed.
///
/// Returns `None` when no real user query is found.
pub fn extract_last_real_user_query(conversation: &[ConversationItem]) -> Option<String> {
    conversation
        .iter()
        .rev()
        .find(|item| is_real_user_turn(item))
        .map(|item| extract_user_query(&item.text_content()))
}
/// Summary of a running subagent for compaction context.
///
/// This is the compaction-layer type. The protocol-layer equivalent is
/// `ActiveSubagentSummary` in tools. The mapping between them
/// happens in `run_compact_inner()` (shell).
#[derive(Clone)]
pub struct RunningSubagentSummary {
    /// The subagent's unique ID.
    pub subagent_id: String,
    /// The agent type name (e.g. "Explore", "general-purpose").
    pub subagent_type: String,
    /// Human-readable description of what the subagent is doing.
    pub description: String,
    /// Wall-clock elapsed time since the subagent was spawned, in milliseconds.
    pub elapsed_ms: u64,
}
/// Summary of a running background task for compaction context.
#[derive(Clone)]
pub struct BackgroundTaskSummary {
    pub task_id: String,
    pub command: String,
    pub status: String,
    /// Model-facing name of the tool that created this task (e.g. `monitor`).
    /// `None` omits it from the reminder.
    pub tool_name: Option<String>,
}
/// Summary of a connected MCP server for compaction context.
#[derive(Clone)]
pub struct CompactionServerSummary {
    pub name: String,
    pub tool_count: usize,
    pub description: Option<String>,
}
/// A dependency-free mirror of `TodoStatus` (tools), kept here so
/// this crate avoids that heavy dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoSummaryStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}
impl TodoSummaryStatus {
    pub fn is_actionable(self) -> bool {
        matches!(self, Self::Pending | Self::InProgress)
    }
    /// Mirrors `TodoStatus::tag()` in tools.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Pending => "[pending]",
            Self::InProgress => "[in_progress]",
            Self::Completed => "[completed]",
            Self::Cancelled => "[cancelled]",
        }
    }
}
/// Compaction-layer summary of a todo item. Protocol-layer equivalent is
/// `TodoItem` in tools.
#[derive(Clone)]
pub struct TodoSummary {
    pub id: String,
    pub content: String,
    pub status: TodoSummaryStatus,
}
/// Context captured at compaction time.
///
/// This is a pure data struct — rendering into system-reminder format is
/// handled by the consumer (e.g. `shell`), which has access to
/// memory backends and other shell-specific dependencies.
pub struct CompactionStateContext {
    /// Monotonic cwd generation; zero means the session has not relocated.
    pub cwd_generation: u64,
    /// Project instructions resolved for the latest destination cwd.
    pub destination_project_instructions: Option<String>,
    /// The last real user query text (skips synthetic injections and
    /// auto-continue prompts).
    pub last_user_query: Option<String>,
    /// Files the agent edited this session (from agent_edited_paths).
    pub agent_edited_paths: Vec<String>,
    /// Running background tasks.
    pub running_tasks: Vec<BackgroundTaskSummary>,
    /// Subagents that are still running at compaction time.
    pub running_subagents: Vec<RunningSubagentSummary>,
    /// Connected MCP servers, for post-compaction system-reminder injection.
    pub connected_mcp_servers: Vec<CompactionServerSummary>,
    /// Todo list captured at compaction time, for post-compaction
    /// system-reminder injection.
    pub todos: Vec<TodoSummary>,
}
/// Live session state captured at compaction time, fed to
/// [`CompactionStateContext::build`].
#[derive(Default)]
pub struct CompactionInputs {
    pub cwd_generation: u64,
    pub destination_project_instructions: Option<String>,
    pub running_tasks: Vec<BackgroundTaskSummary>,
    pub running_subagents: Vec<RunningSubagentSummary>,
    pub agent_edited_paths: BTreeSet<String>,
    pub connected_mcp_servers: Vec<CompactionServerSummary>,
    pub todos: Vec<TodoSummary>,
}
impl CompactionStateContext {
    /// Build the state context from current session state.
    ///
    /// Captures only live state which must be appended to the range summary.
    /// Transcript ownership remains with the Timeline and is never copied
    /// into this side context.
    pub async fn build(conversation: &[ConversationItem], inputs: CompactionInputs) -> Self {
        Self {
            cwd_generation: inputs.cwd_generation,
            destination_project_instructions: inputs.destination_project_instructions,
            last_user_query: extract_last_real_user_query(conversation),
            agent_edited_paths: inputs.agent_edited_paths.into_iter().collect(),
            running_tasks: inputs.running_tasks,
            running_subagents: inputs.running_subagents,
            connected_mcp_servers: inputs.connected_mcp_servers,
            todos: inputs.todos,
        }
    }
    /// Create a task summary from individual fields.
    pub fn task_summary(
        task_id: String,
        command: String,
        status: &str,
        tool_name: Option<String>,
    ) -> BackgroundTaskSummary {
        BackgroundTaskSummary {
            task_id,
            command,
            status: status.to_string(),
            tool_name,
        }
    }
}
pub use compaction::{
    format_compact_summary, format_compact_summary_content, is_degenerate_summary, wrap_user_query,
};
/// Result of sanitizing a compacted conversation history.
pub struct SanitizeResult {
    /// The sanitized conversation items.
    pub items: Vec<ConversationItem>,
    /// `tool_call_id`s that were stripped because no preceding assistant
    /// `tool_calls` entry matched them.
    pub stripped_tool_call_ids: Vec<String>,
}
/// Check whether a compacted conversation satisfies the provider invariant:
///
/// > Every `ToolResult` must have a matching **preceding**
/// > `Assistant.tool_calls[].id`.
///
/// Returns the `tool_call_id`s of any `ToolResult` items that violate
/// the invariant (empty when the history is valid).
///
/// This is a read-only check — it does not modify the conversation.
pub fn validate_compacted_history(items: &[ConversationItem]) -> Vec<String> {
    let mut seen_ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut invalid_ids = Vec::new();
    for item in items {
        match item {
            ConversationItem::Assistant(a) => {
                for tc in &a.tool_calls {
                    seen_ids.insert(&tc.id);
                }
            }
            ConversationItem::ToolResult(tr) if !seen_ids.contains(tr.tool_call_id.as_str()) => {
                invalid_ids.push(tr.tool_call_id.clone());
            }
            _ => {}
        }
    }
    invalid_ids
}
/// Sanitize a compacted conversation by removing orphaned `ToolResult` items.
///
/// Enforces the provider-critical invariant via a left-to-right scan:
///
/// > Every `ToolResult` in the history must have a matching **preceding**
/// > `Assistant.tool_calls[].id`.
///
/// As each `Assistant` is encountered, its tool-call IDs are added to a
/// seen set.  Any `ToolResult` whose `tool_call_id` is not yet in the
/// seen set is stripped (this catches both "no matching assistant" and
/// "result appears before its call").
///
/// **Explicit non-goal**: `Assistant` messages with `tool_calls` but no
/// matching `ToolResult` are NOT stripped — that can be a legitimate
/// in-flight or partially-repaired state and is not the invariant that
/// causes provider 400 errors.
pub fn sanitize_compacted_history(items: Vec<ConversationItem>) -> SanitizeResult {
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stripped_tool_call_ids = Vec::new();
    let sanitized = items
        .into_iter()
        .filter(|item| match item {
            ConversationItem::Assistant(a) => {
                for tc in &a.tool_calls {
                    seen_ids.insert(tc.id.as_ref().to_owned());
                }
                true
            }
            ConversationItem::ToolResult(tr) => {
                if seen_ids.contains(&tr.tool_call_id) {
                    true
                } else {
                    stripped_tool_call_ids.push(tr.tool_call_id.clone());
                    false
                }
            }
            _ => true,
        })
        .collect();
    SanitizeResult {
        items: sanitized,
        stripped_tool_call_ids,
    }
}
/// What [`repair_history`] changed; all-zero/empty means nothing was rewritten.
#[derive(Debug, Clone, Default)]
pub struct HistoryRepairReport {
    /// Duplicate `ToolResult` entries removed.
    pub duplicates_removed: usize,
    /// `tool_call_id`s of orphaned/displaced `ToolResult`s stripped — the
    /// shape behind "unexpected `tool_use_id` found in `tool_result` blocks".
    pub stripped_tool_result_ids: Vec<String>,
    /// Synthetic `ToolResult`s inserted for unanswered `tool_calls`.
    pub synthetic_results_inserted: usize,
}
impl HistoryRepairReport {
    /// Whether the repair modified the conversation.
    pub fn changed(&self) -> bool {
        self.duplicates_removed > 0
            || !self.stripped_tool_result_ids.is_empty()
            || self.synthetic_results_inserted > 0
    }
}
/// Repair provider tool-pairing violations in a conversation (e.g. orphaned
/// `ToolResult`s left by a torn JSONL line, which 400 on every request).
/// Three passes: [`dedup_duplicate_tool_results`],
/// [`strip_displaced_tool_results`], then [`repair_dangling_tool_calls`] to
/// backfill synthetic results for calls the stripping left unanswered.
/// Pure and idempotent.
pub fn repair_history(items: &mut Vec<ConversationItem>) -> HistoryRepairReport {
    repair_history_with_reason(
        items,
        sampling_types::DanglingToolCallReason::HarnessHalted {
            class: "history_repair",
        },
    )
}

pub(crate) fn repair_history_with_reason(
    items: &mut Vec<ConversationItem>,
    reason: sampling_types::DanglingToolCallReason,
) -> HistoryRepairReport {
    let duplicates_removed = sampling_types::dedup_duplicate_tool_results(items);
    let stripped_tool_result_ids = strip_displaced_tool_results(items);
    let synthetic_results_inserted = sampling_types::repair_dangling_tool_calls(items, reason);
    HistoryRepairReport {
        duplicates_removed,
        stripped_tool_result_ids,
        synthetic_results_inserted,
    }
}
/// Strip `ToolResult`s that are not in the contiguous run immediately
/// following the `Assistant` declaring their `tool_call_id` — both orphans
/// (owner gone: the bricked-session case) and displaced results. Returns the
/// stripped ids in order.
///
/// Deliberately stricter than [`sanitize_compacted_history`]'s "matching id
/// anywhere before" (providers require adjacency), and deliberately the same
/// contiguous-run rule as [`repair_dangling_tool_calls`] /
/// [`dedup_duplicate_tool_results`] so the [`repair_history`] passes agree on
/// which calls are answered (a leniency mismatch would make the dangling pass
/// insert synthetic duplicates next to kept results).
pub fn strip_displaced_tool_results(items: &mut Vec<ConversationItem>) -> Vec<String> {
    let mut run_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut stripped = Vec::new();
    items.retain(|item| match item {
        ConversationItem::Assistant(a) => {
            run_ids = a
                .tool_calls
                .iter()
                .map(|tc| tc.id.as_ref().to_owned())
                .collect();
            true
        }
        ConversationItem::ToolResult(tr) => {
            if run_ids.contains(&tr.tool_call_id) {
                true
            } else {
                stripped.push(tr.tool_call_id.clone());
                false
            }
        }
        _ => {
            run_ids.clear();
            true
        }
    });
    stripped
}
#[cfg(test)]
mod tests {
    use super::*;

    fn surface_with_ids(
        items: Vec<ConversationItem>,
    ) -> (Vec<ConversationItem>, Vec<crate::SurfaceId>) {
        let ids = items
            .iter()
            .enumerate()
            .map(|(index, _)| crate::SurfaceId {
                event: crate::EventSeq::new(index as u64),
                item: 0,
            })
            .collect();
        (items, ids)
    }

    #[test]
    fn partial_compaction_preserves_prefix_and_recent_user_turn() {
        let mut items = vec![
            ConversationItem::system("system"),
            ConversationItem::project_instructions("rules"),
            ConversationItem::user("old task"),
            ConversationItem::assistant("x".repeat(800)),
            ConversationItem::user("recent task"),
            ConversationItem::assistant("y".repeat(800)),
        ];
        items[2].set_prompt_index(0);
        items[4].set_prompt_index(1);
        let (surface, ids) = surface_with_ids(items);

        let plan = plan_compaction_range(&surface, &ids, 100, 1).unwrap();
        assert_eq!((plan.start_index, plan.end_index), (2, 3));
        assert_eq!(plan.target.shadowed, ids[2..=3]);
        assert_eq!(plan.target.start, ids[2]);
        assert_eq!(plan.target.end, ids[3]);
    }

    #[test]
    fn repeated_partial_compaction_rolls_prior_summary_into_next_range() {
        let mut items = vec![
            ConversationItem::system("system"),
            ConversationItem::project_instructions("rules"),
            ConversationItem::user_meta("prior compacted history"),
            ConversationItem::user("old retained task"),
            ConversationItem::assistant("x".repeat(800)),
            ConversationItem::user("recent task"),
            ConversationItem::assistant("y".repeat(800)),
        ];
        items[3].set_prompt_index(0);
        items[5].set_prompt_index(1);
        let (surface, ids) = surface_with_ids(items);

        let plan = plan_compaction_range(&surface, &ids, 100, 1).unwrap();
        assert_eq!((plan.start_index, plan.end_index), (2, 4));
        assert_eq!(plan.target.shadowed, ids[2..=4]);
    }

    #[test]
    fn partial_compaction_of_long_turn_keeps_tool_pair_whole() {
        let mut items = vec![
            ConversationItem::system("system"),
            ConversationItem::user("one long task"),
            ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
                id: "call-1".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            }]),
            ConversationItem::tool_result("call-1", "x".repeat(800)),
            ConversationItem::assistant("newest response".repeat(80)),
        ];
        items[1].set_prompt_index(0);
        let (surface, ids) = surface_with_ids(items);

        let plan = plan_compaction_range(&surface, &ids, 100, 1).unwrap();
        assert_eq!((plan.start_index, plan.end_index), (2, 3));
        assert!(matches!(
            surface[plan.start_index],
            ConversationItem::Assistant(_)
        ));
        assert!(matches!(
            surface[plan.end_index],
            ConversationItem::ToolResult(_)
        ));
    }

    #[test]
    fn partial_compaction_keeps_reasoning_with_its_assistant_response() {
        let mut items = vec![
            ConversationItem::system("system"),
            ConversationItem::user("one long task"),
            ConversationItem::assistant("older response".repeat(80)),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "newest reasoning",
            )),
            ConversationItem::assistant("newest response"),
        ];
        items[1].set_prompt_index(0);
        let (surface, ids) = surface_with_ids(items);

        let plan = plan_compaction_range(&surface, &ids, 1, 1).unwrap();
        assert_eq!((plan.start_index, plan.end_index), (2, 2));
        assert!(matches!(
            surface[plan.end_index + 1],
            ConversationItem::Reasoning(_)
        ));
        assert!(matches!(
            surface[plan.end_index + 2],
            ConversationItem::Assistant(_)
        ));
    }

    #[test]
    fn partial_compaction_rejects_mismatched_identity_projection() {
        let mut surface = vec![
            ConversationItem::system("system"),
            ConversationItem::user("task"),
        ];
        surface[1].set_prompt_index(0);
        assert!(plan_compaction_range(&surface, &[], 1, 1).is_none());
    }

    #[test]
    fn goal_continuations_are_prompt_turn_boundaries() {
        let mut items = vec![
            ConversationItem::system("system"),
            ConversationItem::user("runtime context"),
            ConversationItem::system_reminder("continue goal revision 1"),
            ConversationItem::assistant("old work".repeat(80)),
            ConversationItem::system_reminder("continue goal revision 2"),
            ConversationItem::assistant("recent work".repeat(80)),
        ];
        items[2].set_prompt_index(0);
        items[4].set_prompt_index(1);
        let (surface, ids) = surface_with_ids(items);

        let plan = plan_compaction_range(&surface, &ids, 1, 1).unwrap();
        assert_eq!((plan.start_index, plan.end_index), (2, 3));
        assert_eq!(plan.target.shadowed, ids[2..=3]);
    }

    #[test]
    fn synthetic_reminder_without_prompt_index_is_not_a_turn_boundary() {
        let mut items = vec![
            ConversationItem::system("system"),
            ConversationItem::user("task"),
            ConversationItem::assistant("older response".repeat(80)),
            ConversationItem::system_reminder("mid-turn reminder"),
            ConversationItem::assistant("newest response".repeat(80)),
        ];
        items[1].set_prompt_index(0);
        let (surface, ids) = surface_with_ids(items);

        let plan = plan_compaction_range(&surface, &ids, 1, 1).unwrap();
        assert_eq!((plan.start_index, plan.end_index), (2, 3));
    }
    #[test]
    fn test_extract_user_query_with_tags() {
        let input = r#"<user_info>
OS Version: macos
Shell: /bin/bash
</user_info>

<user_query>
create a hello world file
</user_query>"#;
        assert_eq!(extract_user_query(input), "create a hello world file");
    }
    #[test]
    fn test_extract_user_query_multiline() {
        let input = r#"<user_query>
fix the bug in
the login page
</user_query>"#;
        assert_eq!(extract_user_query(input), "fix the bug in\nthe login page");
    }
    #[test]
    fn test_extract_user_query_fallback() {
        let input = r#"<user_info>
OS Version: macos
</user_info>

some plain text"#;
        assert_eq!(extract_user_query(input), "some plain text");
    }
    #[test]
    fn test_extract_user_query_runtime_snapshot_is_empty() {
        let input = r#"<runtime_context>
<user_info>
OS Version: macos
Shell: /bin/zsh
Workspace Path: /workspace
Today's date: 2026-08-18
</user_info>

<jj_status>
Working copy changes:
</jj_status>
</runtime_context>"#;
        assert_eq!(extract_user_query(input), "");
    }
    #[test]
    fn test_extract_user_query_plain_text() {
        let input = "just a simple query";
        assert_eq!(extract_user_query(input), "just a simple query");
    }
    #[test]
    fn test_extract_user_query_strips_system_reminder_inside_user_query() {
        let input = "<user_query>\n\
             <system-reminder>\n\
             This is a scheduled task execution (task t-1, every 5m, recurring).\n\
             </system-reminder>\n\
             \n\
             print free memory\n\
             </user_query>";
        assert_eq!(extract_user_query(input), "print free memory");
    }
    #[test]
    fn test_strip_fork_context_tag() {
        let input = "<fork-context>\nYou inherited context.\n</fork-context>\n\nreal content";
        assert_eq!(extract_user_query(input), "real content");
    }
    #[test]
    fn test_strip_system_reminder_tag() {
        let input =
            "<system-reminder>\nFollow these instructions.\n</system-reminder>\n\nreal content";
        assert_eq!(extract_user_query(input), "real content");
    }
    #[test]
    fn test_strip_agent_memory_tag() {
        let input = "<agent-memory>\nPrevious context.\n</agent-memory>\n\nreal content";
        assert_eq!(extract_user_query(input), "real content");
    }
    #[test]
    fn test_strip_system_underscore_reminder_tag() {
        let input = "<system_reminder>\nReminder text.\n</system_reminder>\n\nreal content";
        assert_eq!(extract_user_query(input), "real content");
    }
    #[test]
    fn test_strip_background_context_tag() {
        let input = "<background_context>\nBackground info.\n</background_context>\n\nreal content";
        assert_eq!(extract_user_query(input), "real content");
    }
    #[test]
    fn test_strip_command_name_tag() {
        let input = "<command-name>execute-plan</command-name>\n\nreal content";
        assert_eq!(extract_user_query(input), "real content");
    }
    #[test]
    fn test_strip_command_message_tag() {
        let input = "<command-message>/execute-plan</command-message>\n\nreal content";
        assert_eq!(extract_user_query(input), "real content");
    }
    #[test]
    fn test_strip_command_args_tag() {
        let input = "<command-args>--dry-run</command-args>\n\nreal content";
        assert_eq!(extract_user_query(input), "real content");
    }
    #[test]
    fn test_strip_multiple_system_tags_at_once() {
        let input = "\
<user_info>OS: linux</user_info>
<fork-context>Inherited.</fork-context>
<system-reminder>Instructions here.</system-reminder>
<agent-memory>Memory data.</agent-memory>

actual user question";
        assert_eq!(extract_user_query(input), "actual user question");
    }
    #[test]
    fn test_strip_unclosed_tag_left_intact() {
        let input = "<fork-context>\nUnclosed tag with no end\n\nreal content";
        assert_eq!(extract_user_query(input), input.trim());
    }
    #[test]
    fn test_strip_system_tags_preserves_existing_behavior() {
        let input = "<user_info>\nOS Version: macos\n</user_info>\n\
                     <project_layout>\nfiles\n</project_layout>\n\
                     <git_status>\nclean\n</git_status>\n\nplain text remains";
        assert_eq!(extract_user_query(input), "plain text remains");
    }
    #[test]
    fn test_strip_duplicate_tags() {
        let input = "<fork-context>A</fork-context><fork-context>B</fork-context> leftover";
        assert_eq!(extract_user_query(input), "leftover");
    }
    #[test]
    fn test_strip_tags_empty_content() {
        let input = "<fork-context></fork-context>";
        assert_eq!(extract_user_query(input), "");
    }
    #[test]
    fn test_strip_close_tag_before_open_tag() {
        let input = "</fork-context>text<fork-context>content</fork-context>more";
        assert_eq!(extract_user_query(input), "</fork-context>textmore");
    }
    #[test]
    fn test_strip_nested_different_tags() {
        let input =
            "<fork-context>outer<system-reminder>inner</system-reminder></fork-context>rest";
        assert_eq!(extract_user_query(input), "rest");
    }
    #[test]
    fn test_extract_last_user_query() {
        let history = vec![ConversationItem::user(
            "<user_info>OS: macos</user_info>\n\n<user_query>\nfix the bug\n</user_query>",
        )];
        let result = extract_last_user_query(&history);
        assert_eq!(result, Some("fix the bug".to_string()));
    }
    #[test]
    fn test_extract_last_user_query_no_user_message() {
        let history = vec![
            ConversationItem::system("system prompt"),
            ConversationItem::assistant("hello"),
        ];
        assert!(extract_last_user_query(&history).is_none());
    }
    #[test]
    fn test_extract_last_user_query_finds_latest() {
        let history = vec![
            ConversationItem::user(
                "<user_info>OS: macos</user_info>\n\n<user_query>\nfirst task\n</user_query>",
            ),
            ConversationItem::assistant("done"),
            ConversationItem::user(
                "<user_info>OS: macos</user_info>\n\n<user_query>\nsecond task\n</user_query>",
            ),
        ];
        let result = extract_last_user_query(&history);
        assert_eq!(result, Some("second task".to_string()));
    }
    #[test]
    fn test_extract_real_user_queries_plain_text() {
        let conv = vec![
            ConversationItem::user("fix the auth bug"),
            ConversationItem::assistant("done"),
            ConversationItem::user("add a test"),
        ];
        let queries = extract_real_user_queries(&conv);
        assert_eq!(queries, vec!["fix the auth bug", "add a test"]);
    }
    #[test]
    fn test_extract_real_user_queries_strips_prefix_returns_query() {
        let first_turn = "<user_info>\nOS Version: macos\n</user_info>\n\
                          <project_layout>\nfiles\n</project_layout>\n\
                          <user_query>\nimplement feature X\n</user_query>";
        let conv = vec![
            ConversationItem::user(first_turn),
            ConversationItem::assistant("done"),
            ConversationItem::user("also add tests"),
        ];
        let queries = extract_real_user_queries(&conv);
        assert_eq!(queries, vec!["implement feature X", "also add tests"]);
    }
    #[test]
    fn test_extract_real_user_queries_excludes_metadata_only() {
        let metadata_only = "<user_info>\nOS Version: macos\n</user_info>\n<project_layout>\nfiles\n</project_layout>";
        let conv = vec![
            ConversationItem::user(metadata_only),
            ConversationItem::assistant("hello"),
            ConversationItem::user("real question"),
        ];
        let queries = extract_real_user_queries(&conv);
        assert_eq!(
            queries,
            vec!["real question"],
            "metadata-only prefix must be excluded"
        );
    }
    #[test]
    fn test_extract_real_user_queries_excludes_auto_continue() {
        let conv = vec![
            ConversationItem::user("<user_query>\n__auto_continue__\n</user_query>"),
            ConversationItem::assistant("continuing"),
            ConversationItem::user("real prompt"),
            ConversationItem::user("<user_query>\n__auto_continue__\n</user_query>"),
        ];
        let queries = extract_real_user_queries(&conv);
        assert_eq!(
            queries,
            vec!["real prompt"],
            "auto-continue sentinels must be excluded"
        );
    }
    #[test]
    fn test_extract_real_user_queries_empty_conversation() {
        let queries = extract_real_user_queries(&[]);
        assert!(queries.is_empty());
    }
    #[test]
    fn test_extract_real_user_queries_no_user_items() {
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::assistant("hello"),
        ];
        let queries = extract_real_user_queries(&conv);
        assert!(queries.is_empty());
    }
    /// The actual AUTO_CONTINUE_PROMPT text stored in the conversation after
    /// auto-compaction must NOT be counted as a real user query.
    #[test]
    fn test_extract_real_user_queries_excludes_actual_auto_continue_prompt() {
        let conv = vec![
            ConversationItem::user(
                "<user_info>OS: macos</user_info>\n<user_query>\nreal task\n</user_query>",
            ),
            ConversationItem::assistant("done"),
            // This is what run_inline_auto_continue() pushes after compaction:
            ConversationItem::user(AUTO_CONTINUE_PROMPT),
            ConversationItem::assistant("continuing..."),
        ];
        let queries = extract_real_user_queries(&conv);
        assert_eq!(
            queries,
            vec!["real task"],
            "AUTO_CONTINUE_PROMPT stored in conversation must be excluded"
        );
    }
    #[test]
    fn test_is_synthetic_empty() {
        assert!(is_synthetic_extracted_query(""));
    }
    #[test]
    fn test_is_synthetic_sentinel() {
        assert!(is_synthetic_extracted_query("__auto_continue__"));
    }
    #[test]
    fn test_is_synthetic_auto_continue_prompt() {
        assert!(
            is_synthetic_extracted_query(AUTO_CONTINUE_PROMPT),
            "the full AUTO_CONTINUE_PROMPT text must be synthetic"
        );
    }
    #[test]
    fn test_is_synthetic_truncation_continue_prompt() {
        assert!(
            is_synthetic_extracted_query(TRUNCATION_CONTINUE_PROMPT),
            "the full TRUNCATION_CONTINUE_PROMPT text must be synthetic"
        );
    }
    #[test]
    fn test_is_synthetic_real_query_is_false() {
        assert!(!is_synthetic_extracted_query("fix the auth bug"));
        assert!(!is_synthetic_extracted_query("add tests"));
    }
    #[test]
    fn test_extract_last_real_user_query_skips_auto_continue_prompt() {
        let conv = vec![
            ConversationItem::user(
                "<user_info>OS: macos</user_info>\n<user_query>\nimplement feature Y\n</user_query>",
            ),
            ConversationItem::assistant("done"),
            ConversationItem::user(AUTO_CONTINUE_PROMPT),
            ConversationItem::assistant("continuing..."),
        ];
        let result = extract_last_real_user_query(&conv);
        assert_eq!(
            result,
            Some("implement feature Y".to_string()),
            "must skip AUTO_CONTINUE_PROMPT and return previous real query"
        );
    }
    #[test]
    fn test_extract_last_real_user_query_no_real_query() {
        let conv = vec![
            ConversationItem::user(AUTO_CONTINUE_PROMPT),
            ConversationItem::assistant("done"),
        ];
        assert!(extract_last_real_user_query(&conv).is_none());
    }
    #[test]
    fn test_extract_last_real_user_query_normal_session() {
        let conv = vec![
            ConversationItem::user(
                "<user_info>OS: macos</user_info>\n<user_query>\nfirst task\n</user_query>",
            ),
            ConversationItem::assistant("done"),
            ConversationItem::user("<user_query>\nsecond task\n</user_query>"),
        ];
        assert_eq!(
            extract_last_real_user_query(&conv),
            Some("second task".to_string())
        );
    }
    #[test]
    fn is_real_user_turn_true_for_real_user() {
        let item = ConversationItem::user("<user_query>\nfix the auth bug\n</user_query>");
        assert!(is_real_user_turn(&item));
    }
    #[test]
    fn is_real_user_turn_false_for_system_reminder() {
        let item = ConversationItem::system_reminder("⚠️ SYSTEM REMINDER");
        assert!(!is_real_user_turn(&item));
    }
    #[test]
    fn is_real_user_turn_false_for_auto_continue() {
        let item = ConversationItem::user(AUTO_CONTINUE_PROMPT);
        assert!(!is_real_user_turn(&item));
        let item = ConversationItem::auto_continue(AUTO_CONTINUE_PROMPT);
        assert!(!is_real_user_turn(&item));
    }
    #[test]
    fn is_real_user_turn_false_for_auto_recovery() {
        let item = ConversationItem::auto_recovery("Try the tool again");
        assert!(!is_real_user_turn(&item));
    }
    #[test]
    fn is_real_user_turn_false_for_empty_bootstrap() {
        let item = ConversationItem::user("<user_info>OS: macos</user_info>");
        assert!(!is_real_user_turn(&item));
        let item = ConversationItem::user(
            "<runtime_context><user_info>OS: macos</user_info></runtime_context>",
        );
        assert!(!is_real_user_turn(&item));
    }
    #[test]
    fn is_real_user_turn_false_for_non_user_items() {
        assert!(!is_real_user_turn(&ConversationItem::system("sys")));
        assert!(!is_real_user_turn(&ConversationItem::assistant("hi")));
    }
    #[test]
    fn is_real_user_turn_true_for_image_only_user() {
        let item = ConversationItem::user_with_parts(vec![ContentPart::Image {
            url: "data:image/png;base64,abc".into(),
        }]);
        assert!(
            is_real_user_turn(&item),
            "image-only user prompt must be a real user turn"
        );
    }
    #[test]
    fn is_real_user_turn_true_for_image_plus_text_user() {
        let item = ConversationItem::user_with_parts(vec![
            ContentPart::Text {
                text: "<user_query>\nwhat is this?\n</user_query>".into(),
            },
            ContentPart::Image {
                url: "data:image/png;base64,abc".into(),
            },
        ]);
        assert!(is_real_user_turn(&item));
    }
    #[test]
    fn is_real_user_turn_false_for_compaction_meta() {
        let item = ConversationItem::user_meta("Called the read_file tool...");
        assert!(
            !is_real_user_turn(&item),
            "user_meta (CompactionMeta) messages must not be real user turns"
        );
    }
    #[test]
    fn extract_last_real_user_query_skips_system_reminder_by_metadata() {
        let conv = vec![
            ConversationItem::user("<user_query>\nimplement feature X\n</user_query>"),
            ConversationItem::assistant("working on it..."),
            ConversationItem::system_reminder("⚠️ SYSTEM REMINDER — stop repeating"),
            ConversationItem::assistant("ok, changing approach"),
        ];
        assert_eq!(
            extract_last_real_user_query(&conv),
            Some("implement feature X".to_string()),
        );
    }
    #[tokio::test]
    async fn test_compaction_state_context_build() {
        let conversation = vec![
            ConversationItem::system("sys"),
            ConversationItem::user(
                "<user_info>OS: macos</user_info>\n\n<user_query>\nfix the bug\n</user_query>",
            ),
            ConversationItem::assistant("Looking at it..."),
            ConversationItem::tool_result("tc1", "file contents"),
        ];
        let mut edited = BTreeSet::new();
        edited.insert("src/main.rs".to_string());
        let running = vec![CompactionStateContext::task_summary(
            "abc".to_string(),
            "cargo test".to_string(),
            "running",
            Some("run_terminal_command".to_string()),
        )];
        let ctx = CompactionStateContext::build(
            &conversation,
            CompactionInputs {
                running_tasks: running,
                agent_edited_paths: edited,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(ctx.last_user_query, Some("fix the bug".to_string()));
        assert_eq!(ctx.agent_edited_paths, vec!["src/main.rs".to_string()]);
        assert_eq!(ctx.running_tasks.len(), 1);
        assert_eq!(ctx.running_tasks[0].command, "cargo test");
    }
    #[tokio::test]
    async fn build_stores_running_subagents() {
        let conversation = vec![
            ConversationItem::user("<user_query>\ntask\n</user_query>"),
            ConversationItem::assistant("working"),
        ];
        let subagents = vec![RunningSubagentSummary {
            subagent_id: "sub-x".into(),
            subagent_type: "Explore".into(),
            description: "searching".into(),
            elapsed_ms: 10_000,
        }];
        let ctx = CompactionStateContext::build(
            &conversation,
            CompactionInputs {
                running_subagents: subagents,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(ctx.running_subagents.len(), 1);
        assert_eq!(ctx.running_subagents[0].subagent_id, "sub-x");
        assert_eq!(ctx.running_subagents[0].subagent_type, "Explore");
        assert_eq!(ctx.running_subagents[0].description, "searching");
        assert_eq!(ctx.running_subagents[0].elapsed_ms, 10_000);
    }
    #[tokio::test]
    async fn build_stores_todos() {
        let conversation = vec![
            ConversationItem::user("<user_query>\ntask\n</user_query>"),
            ConversationItem::assistant("working"),
        ];
        let todos = vec![
            TodoSummary {
                id: "1".into(),
                content: "do the thing".into(),
                status: TodoSummaryStatus::InProgress,
            },
            TodoSummary {
                id: "2".into(),
                content: "do the other thing".into(),
                status: TodoSummaryStatus::Pending,
            },
        ];
        let ctx = CompactionStateContext::build(
            &conversation,
            CompactionInputs {
                todos,
                ..Default::default()
            },
        )
        .await;
        assert_eq!(ctx.todos.len(), 2);
        assert_eq!(ctx.todos[0].id, "1");
        assert_eq!(ctx.todos[0].status, TodoSummaryStatus::InProgress);
        assert_eq!(ctx.todos[1].content, "do the other thing");
    }
    #[test]
    fn degenerate_one_liner_rejected() {
        let raw = "[Called tools: read_file, grep] Explored the compaction code and ran checks.";
        assert!(is_degenerate_summary(raw));
    }
    #[test]
    fn degenerate_band_upper_bound_rejected() {
        let raw = "x".repeat(264);
        assert!(is_degenerate_summary(&raw));
    }
    #[test]
    fn healthy_summary_accepted() {
        let raw = format!(
            "<summary>\n{}\n</summary>",
            "1. Primary Request: fix the bug. ".repeat(40)
        );
        assert!(!is_degenerate_summary(&raw));
    }
    #[test]
    fn floor_boundary_at_500_chars() {
        assert!(is_degenerate_summary(&"y".repeat(499)));
        assert!(!is_degenerate_summary(&"y".repeat(500)));
    }
    #[test]
    fn analysis_wrapping_empty_summary_rejected() {
        let raw = format!(
            "<analysis>\n{}\n</analysis>\n\n<summary>\n</summary>",
            "Walking through the conversation chronologically. ".repeat(100)
        );
        assert!(is_degenerate_summary(&raw));
    }
    #[test]
    fn empty_cleaned_summary_rejected() {
        assert!(is_degenerate_summary(
            "<analysis>\nonly scratchpad, unclosed"
        ));
    }
    #[test]
    fn format_compact_summary_strips_analysis_keeps_summary() {
        let input = "<analysis>\nThinking about the problem...\n</analysis>\n\n<summary>\n1. Primary Request: Fix the bug\n</summary>";
        let result = format_compact_summary(input);
        assert!(!result.contains("Analysis:"));
        assert!(!result.contains("Thinking about the problem"));
        assert!(result.contains("Summary:\n1. Primary Request: Fix the bug"));
        assert!(!result.contains("<analysis>"));
        assert!(!result.contains("</analysis>"));
        assert!(!result.contains("<summary>"));
        assert!(!result.contains("</summary>"));
    }
    #[test]
    fn format_compact_summary_no_tags_passthrough() {
        let input = "Just plain text summary.";
        assert_eq!(format_compact_summary(input), "Just plain text summary.");
    }
    #[test]
    fn format_compact_summary_only_summary() {
        let input = "<summary>\n1. Request: Do something\n</summary>";
        let result = format_compact_summary(input);
        assert_eq!(result, "Summary:\n1. Request: Do something");
    }
    #[test]
    fn format_compact_summary_collapses_blank_lines() {
        let input = "<analysis>\nThought\n</analysis>\n\n\n\n<summary>\nResult\n</summary>";
        let result = format_compact_summary(input);
        assert!(!result.contains("\n\n\n"));
    }
    #[test]
    fn format_compact_summary_analysis_with_summary_references_stripped() {
        let input = "<analysis>\nI need to wrap my output in <summary> tags as instructed.\nLet me organize the sections.\n</analysis>\n\n<summary>\n1. Primary Request: Fix bug\n</summary>";
        let result = format_compact_summary(input);
        assert!(!result.contains("wrap my output in <summary> tags"));
        assert!(!result.contains("<analysis>"));
        assert!(result.contains("Summary:\n1. Primary Request: Fix bug"));
    }
    #[test]
    fn format_compact_summary_unclosed_analysis_strips_remainder() {
        let input = "<analysis>\nPartial reasoning about the task...";
        let result = format_compact_summary(input);
        assert_eq!(result, "");
    }
    #[test]
    fn format_compact_summary_only_analysis_stripped() {
        let input = "<analysis>\nJust reasoning, no summary.\n</analysis>";
        let result = format_compact_summary(input);
        assert_eq!(result, "");
    }
    fn assert_clean_summary(result: &str) {
        assert!(
            result.starts_with("Summary:\n1. Primary Request"),
            "lost real section 1: {result:?}"
        );
        assert!(
            result.contains("9. Optional Next Step"),
            "lost trailing section: {result:?}"
        );
        for needle in [
            "<analysis>",
            "</analysis>",
            "<summary>",
            "</summary>",
            "**Analysis",
            "SCRATCHPAD",
        ] {
            assert!(!result.contains(needle), "leaked {needle:?}: {result:?}");
        }
    }
    #[test]
    fn format_compact_summary_analysis_mentions_tags() {
        let raw = "<analysis>\nSCRATCHPAD: I'll wrap reasoning in <analysis> tags and the result in a <summary> block.\n</analysis>\n\n<summary>\n1. Primary Request and Intent\n- real content\n9. Optional Next Step\n- real next\n</summary>";
        assert_clean_summary(&format_compact_summary(raw));
    }
    #[test]
    fn format_compact_summary_analysis_nested_in_summary() {
        let raw = "<summary>\n<analysis>\nSCRATCHPAD chronological reasoning.\n</analysis>\n\n1. Primary Request and Intent\n- real content\n9. Optional Next Step\n- real next\n</summary>";
        assert_clean_summary(&format_compact_summary(raw));
    }
    #[test]
    fn format_compact_summary_markdown_header_nested_summary() {
        let raw = "<summary>\n**Analysis (internal reasoning before final output):**\nSCRATCHPAD chronological reasoning.\n</analysis>\n\n<summary>\n1. Primary Request and Intent\n- real content\n9. Optional Next Step\n- real next\n</summary>";
        assert_clean_summary(&format_compact_summary(raw));
    }
    #[test]
    fn format_compact_summary_markdown_header_single_summary() {
        let raw = "<summary>\n**Analysis:**\nSCRATCHPAD reasoning.\n</analysis>\n\n1. Primary Request and Intent\n- real content\n9. Optional Next Step\n- real next\n</summary>";
        assert_clean_summary(&format_compact_summary(raw));
    }
    #[test]
    fn format_compact_summary_keeps_sections_on_unbalanced_open_echo() {
        let raw = "<summary>\n1. Primary Request and Intent: build app\n2. Key Technical Concepts: webgl\n3. Files: index.html\n6. All user messages: 'respond with ONLY the <summary> block.'\n9. Optional Next Step: rerun\n</summary>";
        let result = format_compact_summary(raw);
        for needle in [
            "1. Primary Request",
            "2. Key Technical Concepts",
            "3. Files",
            "9. Optional Next Step",
        ] {
            assert!(result.contains(needle), "dropped {needle:?}: {result:?}");
        }
        assert!(!result.contains("<summary>"), "live <summary>: {result:?}");
        assert!(
            !result.contains("</summary>"),
            "live </summary>: {result:?}"
        );
    }
    #[test]
    fn format_compact_summary_keeps_sections_on_section6_orphan_analysis_close() {
        let raw = "<summary>\n1. Primary Request and Intent: build app\n2. Key Technical Concepts: webgl\n6. All user messages: 'wrap analysis in tags</analysis> and respond with ONLY the <summary> block.'\n9. Optional Next Step: rerun\n</summary>";
        let result = format_compact_summary(raw);
        for needle in [
            "1. Primary Request",
            "2. Key Technical Concepts",
            "9. Optional Next Step",
        ] {
            assert!(result.contains(needle), "dropped {needle:?}: {result:?}");
        }
        assert!(
            !result.contains("<analysis>"),
            "live <analysis>: {result:?}"
        );
        assert!(
            !result.contains("</analysis>"),
            "live </analysis>: {result:?}"
        );
        assert!(!result.contains("<summary>"), "live <summary>: {result:?}");
    }
    #[test]
    fn format_compact_summary_strips_scratchpad_with_internal_analysis_mention() {
        let raw = "<summary>\n\
            **Analysis:** I first wrote </analysis> by mistake, then reasoned more.\n\
            </analysis>\n\n\
            1. Primary Request: build app\n\
            9. Optional Next Step: rerun\n\
            </summary>";
        let result = format_compact_summary(raw);
        assert!(result.starts_with("Summary:\n1. Primary Request: build app"));
        assert!(result.contains("9. Optional Next Step: rerun"));
        assert!(
            !result.contains("Analysis"),
            "scratchpad leaked: {result:?}"
        );
        assert!(!result.contains("</analysis>"), "leaked close: {result:?}");
    }
    #[test]
    fn format_compact_summary_unclosed_summary_open_preserves_body() {
        let input = "<summary>\n1. Primary Request: do the thing\n9. Optional Next Step: continue";
        let result = format_compact_summary(input);
        assert!(result.contains("1. Primary Request: do the thing"));
        assert!(result.contains("9. Optional Next Step: continue"));
        assert!(
            !result.contains("<summary>"),
            "tag not neutralized: {result:?}"
        );
    }
    #[test]
    fn format_compact_summary_body_analysis_open_echo_keeps_sections() {
        let raw = "<summary>\n\
            1. Primary Request and Intent: build app\n\
            2. Key Technical Concepts: webgl\n\
            6. All user messages: 'wrap your analysis in <analysis> tags and respond with ONLY the <summary> block.'\n\
            9. Optional Next Step: rerun\n\
            </summary>";
        let result = format_compact_summary(raw);
        assert!(
            result.starts_with("Summary:\n1. Primary Request and Intent: build app"),
            "section 1 / heading lost: {result:?}"
        );
        for needle in ["2. Key Technical Concepts", "9. Optional Next Step"] {
            assert!(result.contains(needle), "dropped {needle:?}: {result:?}");
        }
        assert!(
            !result.contains("<analysis>"),
            "live <analysis>: {result:?}"
        );
        assert!(!result.contains("<summary>"), "live <summary>: {result:?}");
    }
    #[test]
    fn format_compact_summary_nested_scratchpad_with_later_close_echo_keeps_sections() {
        let raw = "<summary>\n\
            <analysis>\nSCRATCHPAD reasoning.\n</analysis>\n\n\
            1. Primary Request: build app\n\
            6. All user messages: 'wrap analysis in tags</analysis> and respond'\n\
            9. Optional Next Step: rerun\n\
            </summary>";
        let result = format_compact_summary(raw);
        assert!(
            result.starts_with("Summary:\n1. Primary Request: build app"),
            "section 1 lost: {result:?}"
        );
        assert!(
            result.contains("9. Optional Next Step: rerun"),
            "section 9 lost: {result:?}"
        );
        assert!(
            !result.contains("SCRATCHPAD"),
            "scratchpad leaked: {result:?}"
        );
        assert!(
            !result.contains("</analysis>"),
            "live </analysis>: {result:?}"
        );
    }
    #[test]
    fn format_compact_summary_body_analysis_pair_spanning_sections_keeps_them() {
        let raw = "<summary>\n\
            1. Primary Request: build app\n\
            6. All user messages: 'wrap your analysis in <analysis> tags'\n\
            7. Pending Tasks: fix the bug\n\
            8. Key files: foo.rs\n\
            9. Optional Next Step: 'end the block with </analysis> when done'\n\
            </summary>";
        let result = format_compact_summary(raw);
        for needle in [
            "1. Primary Request: build app",
            "7. Pending Tasks: fix the bug",
            "8. Key files: foo.rs",
            "9. Optional Next Step",
        ] {
            assert!(result.contains(needle), "dropped {needle:?}: {result:?}");
        }
        assert!(
            !result.contains("<analysis>"),
            "live <analysis>: {result:?}"
        );
        assert!(
            !result.contains("</analysis>"),
            "live </analysis>: {result:?}"
        );
    }
    #[test]
    fn format_compact_summary_multiple_leading_analysis_blocks_all_stripped() {
        let raw = "<analysis>A reasoning</analysis>\n\
            <analysis>B reasoning</analysis>\n\
            <summary>\n1. Primary Request: build app\n9. Optional Next Step: rerun\n</summary>";
        let result = format_compact_summary(raw);
        assert!(
            result.starts_with("Summary:\n1. Primary Request: build app"),
            "scratchpad leaked ahead of heading: {result:?}"
        );
        assert!(result.contains("9. Optional Next Step: rerun"));
        assert!(
            !result.contains("reasoning"),
            "scratchpad prose leaked: {result:?}"
        );
        assert!(
            !result.contains("<analysis>"),
            "live <analysis>: {result:?}"
        );
    }
    #[test]
    fn format_compact_summary_neutralizes_summary_request_tokens() {
        let raw = "1. Primary Request: build app\n\
            6. msgs: '<summary_request>do X</summary_request>'\n\
            9. Optional Next Step: rerun";
        let result = format_compact_summary(raw);
        assert!(
            !result.contains("<summary_request>"),
            "live <summary_request>: {result:?}"
        );
        assert!(
            !result.contains("</summary_request>"),
            "live </summary_request>: {result:?}"
        );
        assert!(result.contains("1. Primary Request: build app"));
        assert!(result.contains("9. Optional Next Step: rerun"));
    }
    #[test]
    fn format_compact_summary_body_reversed_analysis_echo_not_garbled() {
        let raw = "<summary>\n\
            1. Primary Request: build app\n\
            6. msgs: 'output </analysis> then wrap in <analysis> tags'\n\
            9. Optional Next Step: rerun\n\
            </summary>";
        let result = format_compact_summary(raw);
        assert!(result.starts_with("Summary:\n1. Primary Request: build app"));
        assert!(result.contains("9. Optional Next Step: rerun"));
        assert_eq!(
            result.matches("then wrap in").count(),
            1,
            "spanned text duplicated: {result:?}"
        );
    }
    #[test]
    fn format_compact_summary_markdown_numbered_lead_keeps_sections() {
        let raw = "<summary>\n\
            ## 1. Primary Request: build app\n\
            ## 6. All user messages: 'wrap analysis in tags</analysis> and respond.'\n\
            ## 9. Optional Next Step: rerun\n\
            </summary>";
        let result = format_compact_summary(raw);
        for needle in [
            "1. Primary Request: build app",
            "9. Optional Next Step: rerun",
        ] {
            assert!(result.contains(needle), "dropped {needle:?}: {result:?}");
        }
        assert!(!result.contains("</analysis>"), "leaked close: {result:?}");
    }
    #[test]
    fn format_compact_summary_multibyte_adjacent_to_tags() {
        let raw =
            "<summary>1. Primary Request: ship 🚀 to 北京\n9. Optional Next Step: 完成</summary>";
        let result = format_compact_summary(raw);
        assert!(result.starts_with("Summary:\n1. Primary Request: ship 🚀 to 北京"));
        assert!(result.contains("9. Optional Next Step: 完成"));
    }
    #[test]
    fn format_compact_summary_content_adds_preamble() {
        let result = format_compact_summary_content("Some summary text.");
        assert!(result.starts_with("This session is being continued"));
        assert!(result.contains("Some summary text."));
    }
    #[test]
    fn format_compact_summary_content_cleans_tags() {
        let raw = "<analysis>\nThinking\n</analysis>\n\n<summary>\n1. Fix bug\n</summary>";
        let result = format_compact_summary_content(raw);
        assert!(result.starts_with("This session is being continued"));
        assert!(!result.contains("Analysis:"));
        assert!(!result.contains("Thinking"));
        assert!(result.contains("Summary:\n1. Fix bug"));
        assert!(!result.contains("<analysis>"));
        assert!(!result.contains("<summary>"));
    }
    #[test]
    fn format_compact_summary_neutralizes_section6_instruction_echo() {
        let input = "<summary>\n\
            <analysis>\nChronological analysis of the conversation...\n</analysis>\n\n\
            1. Primary Request and Intent: Build a Mario clone.\n\
            6. All user messages: ...</system-reminder> Your task is to create a \
            detailed summary of the conversation so far ... Before providing your \
            final summary, wrap your analysis in <analysis> tags ... 'Do NOT use \
            any tools. You MUST respond with ONLY the <summary>...</summary> block \
            as your text output.'\n\
            7. Pending Tasks: Fix the importmap mismatch.\n\
            9. Optional Next Step: Re-run the verification plan.\n\
            </summary>";
        let result = format_compact_summary(input);
        assert!(!result.contains("<summary>"), "live <summary>: {result}");
        assert!(!result.contains("</summary>"), "live </summary>: {result}");
        assert!(!result.contains("<analysis>"), "live <analysis>: {result}");
        assert!(
            result.contains("7. Pending Tasks: Fix the importmap mismatch."),
            "post-echo section dropped: {result}"
        );
        assert!(result.contains("9. Optional Next Step: Re-run the verification plan."));
        assert!(
            result.contains("<\u{200b}summary>"),
            "tag not neutralized: {result}"
        );
        assert!(result.contains("Summary:\n1. Primary Request and Intent: Build a Mario clone."));
    }
    #[test]
    fn format_compact_summary_content_neutralizes_instruction_echo() {
        let raw = "<summary>\n1. Primary Request: build app.\n\
            6. All user messages: 'You MUST respond with ONLY the \
            <summary>...</summary> block.'\n\
            9. Optional Next Step: continue.\n</summary>";
        let seed = format_compact_summary_content(raw);
        assert!(seed.starts_with("This session is being continued"));
        assert!(
            !seed.contains("<summary>"),
            "live <summary> in seed: {seed}"
        );
        assert!(
            !seed.contains("</summary>"),
            "live </summary> in seed: {seed}"
        );
        assert!(seed.contains("9. Optional Next Step: continue."));
    }
    #[test]
    fn format_compact_summary_malformed_tag_order_does_not_panic() {
        let input = "intro </summary> middle <summary> tail";
        let result = format_compact_summary(input);
        assert!(!result.contains("<summary>"));
        assert!(!result.contains("</summary>"));
        assert!(result.contains("intro"));
        assert!(result.contains("tail"));
    }
    #[test]
    fn sanitize_strips_orphaned_tool_result() {
        use sampling_types::ToolCall;
        let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("prompt"),
            // Orphaned tool result — no assistant with matching tool_calls
            ConversationItem::tool_result("call_ORPHAN", "result"),
            // Valid pair
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "call_VALID".into(),
                name: "read_file".to_string(),
                arguments: "{}".into(),
            }]),
            ConversationItem::tool_result("call_VALID", "ok"),
        ];
        let result = sanitize_compacted_history(items);
        assert_eq!(result.stripped_tool_call_ids, vec!["call_ORPHAN"]);
        assert_eq!(result.items.len(), 4);
        for item in &result.items {
            if let ConversationItem::ToolResult(tr) = item {
                assert_eq!(tr.tool_call_id, "call_VALID");
            }
        }
    }
    #[test]
    fn sanitize_keeps_assistant_with_unanswered_tool_calls() {
        use sampling_types::ToolCall;
        let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("prompt"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "call_UNANSWERED".into(),
                name: "run_cmd".to_string(),
                arguments: "{}".into(),
            }]),
        ];
        let result = sanitize_compacted_history(items);
        assert!(result.stripped_tool_call_ids.is_empty());
        assert_eq!(result.items.len(), 3);
    }
    #[test]
    fn sanitize_strips_result_before_call() {
        use sampling_types::ToolCall;
        let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::tool_result("call_X", "premature result"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "call_X".into(),
                name: "read_file".to_string(),
                arguments: "{}".into(),
            }]),
        ];
        let result = sanitize_compacted_history(items);
        assert_eq!(
            result.stripped_tool_call_ids,
            vec!["call_X"],
            "result-before-call must be stripped"
        );
        assert_eq!(result.items.len(), 2);
    }
    #[test]
    fn validate_detects_result_before_call() {
        use sampling_types::ToolCall;
        let items = vec![
            ConversationItem::tool_result("call_X", "premature"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "call_X".into(),
                name: "edit".to_string(),
                arguments: "{}".into(),
            }]),
        ];
        let invalid = validate_compacted_history(&items);
        assert_eq!(invalid, vec!["call_X"]);
    }
    #[test]
    fn validate_passes_valid_history() {
        use sampling_types::ToolCall;
        let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "call_A".into(),
                name: "edit".to_string(),
                arguments: "{}".into(),
            }]),
            ConversationItem::tool_result("call_A", "done"),
        ];
        assert!(validate_compacted_history(&items).is_empty());
    }
    #[test]
    fn sanitize_noop_on_valid_conversation() {
        use sampling_types::ToolCall;
        let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("prompt"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "call_A".into(),
                name: "edit".to_string(),
                arguments: "{}".into(),
            }]),
            ConversationItem::tool_result("call_A", "done"),
            ConversationItem::assistant("All done."),
        ];
        let result = sanitize_compacted_history(items);
        assert!(result.stripped_tool_call_ids.is_empty());
        assert_eq!(result.items.len(), 5);
    }
    fn call(id: &str) -> sampling_types::ToolCall {
        sampling_types::ToolCall {
            id: id.into(),
            name: "read_file".to_string(),
            arguments: "{}".into(),
        }
    }
    /// The bricked-session shape: the assistant line owning a batch of tool
    /// calls was lost (torn/merged JSONL line skipped on load), so its
    /// results are orphans. Repair must strip them and change nothing else.
    #[test]
    fn repair_history_strips_orphaned_tool_results() {
        let mut items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("prompt"),
            // ← the assistant declaring call_LOST is missing here
            ConversationItem::tool_result("call_LOST", "orphaned result"),
            ConversationItem::assistant_tool_calls(vec![call("call_OK")]),
            ConversationItem::tool_result("call_OK", "fine"),
        ];
        let report = repair_history(&mut items);
        assert!(report.changed());
        assert_eq!(report.stripped_tool_result_ids, vec!["call_LOST"]);
        assert_eq!(report.duplicates_removed, 0);
        assert_eq!(report.synthetic_results_inserted, 0);
        assert_eq!(items.len(), 4);
    }
    /// A result displaced past a user turn has a matching id *somewhere
    /// before*, so the compaction sanitizer would keep it — but providers
    /// require adjacency, so repair must strip it and synthesize a result
    /// for the now-unanswered call.
    #[test]
    fn repair_history_strips_displaced_result_and_backfills_call() {
        let mut items = vec![
            ConversationItem::system("sys"),
            ConversationItem::assistant_tool_calls(vec![call("call_D")]),
            ConversationItem::user("interjection splits the pair"),
            ConversationItem::tool_result("call_D", "arrived too late"),
        ];
        let report = repair_history(&mut items);
        assert_eq!(report.stripped_tool_result_ids, vec!["call_D"]);
        assert_eq!(report.synthetic_results_inserted, 1);
        match (&items[1], &items[2]) {
            (ConversationItem::Assistant(a), ConversationItem::ToolResult(tr)) => {
                assert_eq!(a.tool_calls[0].id.as_ref(), "call_D");
                assert_eq!(tr.tool_call_id, "call_D");
                assert!(
                    tr.content
                        .contains("halted by the harness (history_repair)"),
                    "expected synthetic wording, got: {}",
                    tr.content
                );
            }
            other => panic!("expected assistant+synthetic result, got {other:?}"),
        }
    }
    /// A result split from its owner by another assistant item is stripped
    /// and the call backfilled — keeping it would make the dangling pass
    /// insert a synthetic duplicate beside it (two results for one id).
    #[test]
    fn repair_history_strips_result_split_by_assistant_item() {
        let mut items = vec![
            ConversationItem::system("sys"),
            ConversationItem::assistant_tool_calls(vec![call("call_A")]),
            ConversationItem::assistant("interleaved text"),
            ConversationItem::tool_result("call_A", "no longer contiguous"),
        ];
        let report = repair_history(&mut items);
        assert_eq!(report.stripped_tool_result_ids, vec!["call_A"]);
        assert_eq!(report.synthetic_results_inserted, 1);
        let results: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                ConversationItem::ToolResult(tr) => Some(tr),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("halted by the harness"));
    }
    /// A result whose owner lives before an *earlier, separate* result run
    /// must be stripped: the intervening run flushed the assistant message.
    #[test]
    fn repair_history_strips_result_in_later_run() {
        let mut items = vec![
            ConversationItem::assistant_tool_calls(vec![call("call_A"), call("call_B")]),
            ConversationItem::tool_result("call_A", "ok"),
            ConversationItem::assistant_tool_calls(vec![call("call_C")]),
            ConversationItem::tool_result("call_C", "ok"),
            // call_B's owner was flushed two messages ago.
            ConversationItem::tool_result("call_B", "displaced"),
        ];
        let report = repair_history(&mut items);
        assert_eq!(report.stripped_tool_result_ids, vec!["call_B"]);
        assert_eq!(report.synthetic_results_inserted, 1);
    }
    #[test]
    fn repair_history_dedups_duplicate_results() {
        let mut items = vec![
            ConversationItem::assistant_tool_calls(vec![call("call_A")]),
            ConversationItem::tool_result("call_A", "stale duplicate"),
            ConversationItem::tool_result("call_A", "real result"),
        ];
        let report = repair_history(&mut items);
        assert_eq!(report.duplicates_removed, 1);
        assert!(report.stripped_tool_result_ids.is_empty());
        match &items[1] {
            ConversationItem::ToolResult(tr) => {
                assert_eq!(tr.content.as_ref(), "real result")
            }
            other => panic!("expected tool result, got {other:?}"),
        }
    }
    #[test]
    fn repair_history_is_noop_and_idempotent_on_valid_history() {
        let valid = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("prompt"),
            ConversationItem::assistant_tool_calls(vec![call("call_A")]),
            ConversationItem::tool_result("call_A", "done"),
            ConversationItem::assistant("All done."),
        ];
        let mut items = valid.clone();
        let report = repair_history(&mut items);
        assert!(!report.changed());
        assert_eq!(items.len(), valid.len());
        let mut corrupted = vec![
            ConversationItem::user("prompt"),
            ConversationItem::tool_result("call_ORPHAN", "orphan"),
        ];
        assert!(repair_history(&mut corrupted).changed());
        assert!(!repair_history(&mut corrupted).changed());
    }
    #[test]
    fn wrap_user_query_wraps_text() {
        let result = wrap_user_query("hello world");
        assert_eq!(result, "<user_query>\nhello world\n</user_query>");
    }
    #[test]
    fn wrap_user_query_preserves_multiline() {
        let result = wrap_user_query("line 1\nline 2");
        assert_eq!(result, "<user_query>\nline 1\nline 2\n</user_query>");
    }

    #[test]
    fn conversation_item_drops_tool_results() {
        let result = strip_tool_messages_for_conversation_item(vec![
            ConversationItem::system("system"),
            ConversationItem::user("hello"),
            ConversationItem::assistant("response"),
            ConversationItem::tool_result("call_1", "result"),
        ]);
        assert_eq!(result.len(), 3);
        assert!(
            !result
                .iter()
                .any(|m| matches!(m, ConversationItem::ToolResult(_)))
        );
    }
    /// Load-bearing: documents the intentional contract that
    /// `strip_tool_messages_for_conversation_item` does NOT touch sibling
    /// `Reasoning` items. `prepare_conversation_for_summarization` composes
    /// against this guarantee by chaining `strip_reasoning_blocks` after.
    #[test]
    fn conversation_item_preserves_reasoning_siblings() {
        use sampling_types::{AssistantItem, rs};
        let result = strip_tool_messages_for_conversation_item(vec![
            ConversationItem::system("system"),
            ConversationItem::Reasoning(rs::ReasoningItem {
                id: Some("r_123".to_string()),
                summary: vec![],
                content: None,
                encrypted_content: Some("encrypted_sig".to_string()),
                status: None,
            }),
            ConversationItem::Assistant(AssistantItem {
                content: "response".into(),
                tool_calls: vec![],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
        ]);
        assert_eq!(result.len(), 3);
        assert!(matches!(result[1], ConversationItem::Reasoning(_)));
    }
    #[test]
    fn strip_reasoning_blocks_drops_reasoning_siblings() {
        use sampling_types::{AssistantItem, rs};
        let result = strip_reasoning_blocks(vec![
            ConversationItem::Reasoning(rs::ReasoningItem {
                id: Some("r_123".to_string()),
                summary: vec![rs::SummaryPart::SummaryText(rs::SummaryTextContent {
                    text: "thinking".to_string(),
                })],
                content: None,
                encrypted_content: Some("encrypted_sig".to_string()),
                status: None,
            }),
            ConversationItem::Assistant(AssistantItem {
                content: "response".into(),
                tool_calls: vec![],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
        ]);
        assert_eq!(result.len(), 1, "reasoning sibling must be dropped");
        assert!(matches!(result[0], ConversationItem::Assistant(_)));
    }
    #[test]
    fn strip_reasoning_blocks_passes_other_items_through() {
        let result = strip_reasoning_blocks(vec![
            ConversationItem::system("system"),
            ConversationItem::user("hello"),
            ConversationItem::tool_result("call_1", "result"),
        ]);
        assert_eq!(result.len(), 3);
        assert!(matches!(result[0], ConversationItem::System(_)));
        assert!(matches!(result[1], ConversationItem::User(_)));
        assert!(matches!(result[2], ConversationItem::ToolResult(_)));
    }
    /// Reproduces the production failure that prompted this helper: an
    /// assistant turn with both signed `reasoning` and `tool_calls` triggers a
    /// provider "thinking blocks cannot be modified" 400 because the strip
    /// mutates the surrounding text. After `prepare_conversation_for_summarization`
    /// the message must have no `reasoning` left for the provider to validate.
    #[test]
    fn prepare_for_summarization_drops_reasoning_sibling_on_mutated_assistant() {
        use sampling_types::{AssistantItem, ToolCall, rs};
        let mk_reasoning = || {
            ConversationItem::Reasoning(rs::ReasoningItem {
                id: Some("r_123".to_string()),
                summary: vec![rs::SummaryPart::SummaryText(rs::SummaryTextContent {
                    text: "plan".to_string(),
                })],
                content: None,
                encrypted_content: Some("encrypted_sig".to_string()),
                status: None,
            })
        };
        let result = prepare_conversation_for_summarization(vec![
            ConversationItem::system("system"),
            ConversationItem::user("do stuff"),
            mk_reasoning(),
            ConversationItem::Assistant(AssistantItem {
                content: "I'll search.".into(),
                tool_calls: vec![ToolCall {
                    id: "tc1".into(),
                    name: "grep".into(),
                    arguments: "{}".into(),
                }],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
            ConversationItem::tool_result("tc1", "match found"),
        ]);
        assert_eq!(
            result.len(),
            3,
            "tool_result and reasoning sibling must be dropped"
        );
        assert!(
            !result
                .iter()
                .any(|m| matches!(m, ConversationItem::Reasoning(_))),
            "reasoning sibling must be dropped"
        );
        let ConversationItem::Assistant(a) = &result[2] else {
            panic!("expected assistant at index 2");
        };
        assert!(a.tool_calls.is_empty(), "tool_calls must be cleared");
        assert!(
            a.content.contains("[Called tools: grep]"),
            "tool annotation must be appended; got {:?}",
            a.content,
        );
    }
    #[test]
    fn prepare_for_summarization_drops_standalone_reasoning_sibling() {
        use sampling_types::{AssistantItem, rs};
        let result = prepare_conversation_for_summarization(vec![
            ConversationItem::Reasoning(rs::ReasoningItem {
                id: Some("r_123".to_string()),
                summary: vec![rs::SummaryPart::SummaryText(rs::SummaryTextContent {
                    text: "thinking".to_string(),
                })],
                content: None,
                encrypted_content: None,
                status: None,
            }),
            ConversationItem::Assistant(AssistantItem {
                content: "plain text response".into(),
                tool_calls: vec![],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
        ]);
        assert_eq!(result.len(), 1);
        let ConversationItem::Assistant(a) = &result[0] else {
            panic!("expected assistant");
        };
        assert_eq!(a.content.as_ref(), "plain text response");
    }
    /// Multi-assistant conversation with mixed reasoning/tool_calls states.
    #[test]
    fn prepare_for_summarization_handles_multi_assistant_mixed_conversation() {
        use sampling_types::{AssistantItem, ToolCall, rs};
        let mk_reasoning = || {
            ConversationItem::Reasoning(rs::ReasoningItem {
                id: Some("r".to_string()),
                summary: vec![rs::SummaryPart::SummaryText(rs::SummaryTextContent {
                    text: "thinking".to_string(),
                })],
                content: None,
                encrypted_content: Some("sig".to_string()),
                status: None,
            })
        };
        let result = prepare_conversation_for_summarization(vec![
            ConversationItem::user("first turn"),
            mk_reasoning(),
            ConversationItem::Assistant(AssistantItem {
                content: "calling grep".into(),
                tool_calls: vec![ToolCall {
                    id: "tc1".into(),
                    name: "grep".into(),
                    arguments: "{}".into(),
                }],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
            ConversationItem::tool_result("tc1", "match"),
            ConversationItem::user("second turn"),
            mk_reasoning(),
            ConversationItem::Assistant(AssistantItem {
                content: "thinking only".into(),
                tool_calls: vec![],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
            ConversationItem::tool_result("tc2", "stray"),
            ConversationItem::user("third turn"),
            ConversationItem::Assistant(AssistantItem {
                content: "plain reply".into(),
                tool_calls: vec![],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
        ]);
        assert_eq!(result.len(), 6);
        assert!(
            !result
                .iter()
                .any(|m| matches!(m, ConversationItem::ToolResult(_)))
        );
        assert!(
            !result
                .iter()
                .any(|m| matches!(m, ConversationItem::Reasoning(_)))
        );
        let assistants: Vec<&AssistantItem> = result
            .iter()
            .filter_map(|m| match m {
                ConversationItem::Assistant(a) => Some(a),
                _ => None,
            })
            .collect();
        assert_eq!(assistants.len(), 3);
        for a in &assistants {
            assert!(a.tool_calls.is_empty(), "tool_calls must be cleared");
        }
        assert!(
            assistants[0].content.contains("[Called tools: grep]"),
            "tool-calling assistant must get annotation; got {:?}",
            assistants[0].content
        );
        assert!(
            !assistants[1].content.contains("[Called tools:"),
            "no-tool-call assistant must not get annotation; got {:?}",
            assistants[1].content
        );
        assert!(
            !assistants[2].content.contains("[Called tools:"),
            "plain assistant must not get annotation; got {:?}",
            assistants[2].content
        );
    }
    /// Calling `prepare_conversation_for_summarization` twice must produce
    /// the same result as calling it once. Guarantees the transformation
    /// has no hidden state and is safe to apply defensively at multiple
    /// layers (e.g. memory flush + compaction both routing through it).
    #[test]
    fn prepare_for_summarization_is_idempotent() {
        use sampling_types::{AssistantItem, ToolCall, rs};
        let input = vec![
            ConversationItem::system("system prompt"),
            ConversationItem::user("hello"),
            ConversationItem::Reasoning(rs::ReasoningItem {
                id: Some("r1".to_string()),
                summary: vec![rs::SummaryPart::SummaryText(rs::SummaryTextContent {
                    text: "thought".to_string(),
                })],
                content: None,
                encrypted_content: Some("sig".to_string()),
                status: None,
            }),
            ConversationItem::Assistant(AssistantItem {
                content: "hi".into(),
                tool_calls: vec![ToolCall {
                    id: "tc1".into(),
                    name: "ls".into(),
                    arguments: "{}".into(),
                }],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
            ConversationItem::tool_result("tc1", "files"),
        ];
        let once = prepare_conversation_for_summarization(input.clone());
        let twice = prepare_conversation_for_summarization(once.clone());
        let once_json = serde_json::to_value(&once).unwrap();
        let twice_json = serde_json::to_value(&twice).unwrap();
        assert_eq!(once_json, twice_json, "second pass must be a no-op");
    }
    #[test]
    fn prepare_for_summarization_never_erases_images_without_projection() {
        let mut user = ConversationItem::user("look at this");
        user.add_image("data:image/jpeg;base64,/9j/4AAQ");
        let input = vec![
            ConversationItem::system("sys"),
            user,
            ConversationItem::assistant("ok"),
        ];
        let result = prepare_conversation_for_summarization(input);
        match &result[1] {
            ConversationItem::User(u) => {
                assert!(u.content.iter().any(
                    |part| matches!(part, ContentPart::Image { url } if url.as_ref() == "data:image/jpeg;base64,/9j/4AAQ")
                ));
            }
            _ => panic!("expected User item"),
        }
    }
    /// Verbatim view keeps tool calls (with arguments) and results — no flattening, no dropped results.
    #[test]
    fn verbatim_keeps_tool_calls_args_and_results() {
        use sampling_types::ToolCall;
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("read a.rs"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "c1".into(),
                name: "read_file".to_string(),
                arguments: r#"{"target_file":"a.rs"}"#.into(),
            }]),
            ConversationItem::tool_result("c1", "fn main() {}"),
        ];
        let result = prepare_conversation_for_verbatim_summarization(conv, false);
        match &result[2] {
            ConversationItem::Assistant(a) => {
                assert_eq!(a.tool_calls.len(), 1, "tool call must survive verbatim");
                assert_eq!(a.tool_calls[0].name, "read_file");
                assert!(
                    a.tool_calls[0].arguments.contains("a.rs"),
                    "arguments (the path) must be preserved, not dropped"
                );
                assert!(
                    !a.content.contains("[Called tools:"),
                    "verbatim view must NOT flatten tool calls into text"
                );
            }
            _ => panic!("expected Assistant with tool_calls"),
        }
        match &result[3] {
            ConversationItem::ToolResult(t) => {
                assert_eq!(t.content.as_ref(), "fn main() {}")
            }
            _ => panic!("expected ToolResult to survive"),
        }
    }
    /// Reasoning kept on non-Messages backends, stripped on Messages — tool I/O survives either way.
    #[test]
    fn verbatim_reasoning_kept_unless_messages_backend() {
        use sampling_types::{ToolCall, rs};
        let mk = || {
            vec![
                ConversationItem::system("sys"),
                ConversationItem::Reasoning(rs::ReasoningItem {
                    id: Some("r1".to_string()),
                    summary: vec![],
                    content: None,
                    encrypted_content: Some("sig".to_string()),
                    status: None,
                }),
                ConversationItem::assistant_tool_calls(vec![ToolCall {
                    id: "c1".into(),
                    name: "grep".to_string(),
                    arguments: "{}".into(),
                }]),
                ConversationItem::tool_result("c1", "match"),
            ]
        };
        let kept = prepare_conversation_for_verbatim_summarization(mk(), false);
        assert!(
            kept.iter()
                .any(|i| matches!(i, ConversationItem::Reasoning(_))),
            "reasoning must be kept when strip_reasoning = false (Grow backends)"
        );
        let stripped = prepare_conversation_for_verbatim_summarization(mk(), true);
        assert!(
            !stripped
                .iter()
                .any(|i| matches!(i, ConversationItem::Reasoning(_))),
            "reasoning must be stripped when strip_reasoning = true (Messages backend)"
        );
        assert!(
            stripped
                .iter()
                .any(|i| matches!(i, ConversationItem::ToolResult(_))),
            "tool results must survive even when reasoning is stripped"
        );
    }
    /// A trailing incomplete `tool_calls` turn is dropped; an earlier complete run is preserved.
    #[test]
    fn verbatim_truncates_trailing_incomplete_tool_call() {
        use sampling_types::ToolCall;
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("go"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "c1".into(),
                name: "read_file".to_string(),
                arguments: r#"{"target_file":"a.rs"}"#.into(),
            }]),
            ConversationItem::tool_result("c1", "fn main() {}"),
            // Trailing, no matching ToolResult — results never arrived.
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "c2".into(),
                name: "grep".to_string(),
                arguments: "{}".into(),
            }]),
        ];
        let result = prepare_conversation_for_verbatim_summarization(conv, false);
        assert_eq!(
            result.len(),
            4,
            "trailing incomplete tool call must be dropped"
        );
        assert!(matches!(
            result.last(),
            Some(ConversationItem::ToolResult(_))
        ));
    }
    /// A conversation ending in a complete tool run (tail = `ToolResult`) is left untouched.
    #[test]
    fn verbatim_keeps_trailing_complete_tool_run() {
        use sampling_types::ToolCall;
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "c1".into(),
                name: "read_file".to_string(),
                arguments: "{}".into(),
            }]),
            ConversationItem::tool_result("c1", "ok"),
        ];
        let result = prepare_conversation_for_verbatim_summarization(conv, false);
        assert_eq!(result.len(), 3, "complete trailing run must be preserved");
    }
    /// A conversation already within budget is returned unchanged.
    #[test]
    fn fit_returns_unchanged_when_within_budget() {
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("hi"),
            ConversationItem::assistant("hello"),
        ];
        let out = fit_conversation_to_budget(conv, 1_000_000);
        assert_eq!(out.len(), 3);
    }
    /// Over budget: oldest whole turns dropped; System and most-recent turns survive.
    #[test]
    fn fit_drops_oldest_turns_keeps_system_and_recent() {
        let big = "x".repeat(800);
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user(&big),      // old + large -> dropped
            ConversationItem::assistant(&big), // old + large -> dropped
            ConversationItem::user("recent question"),
            ConversationItem::assistant("recent answer"),
        ];
        let out = fit_conversation_to_budget(conv, 60);
        assert!(
            matches!(out.first(), Some(ConversationItem::System(_))),
            "system must be kept"
        );
        assert!(
            out.iter().any(|i| i.text_content() == "recent answer"),
            "most-recent turn must be kept"
        );
        assert!(
            !out.iter().any(|i| i.text_content().len() > 100),
            "the large old turns must be dropped"
        );
    }
    /// Trimming must not leave a leading orphan `ToolResult` whose assistant turn was dropped.
    #[test]
    fn fit_drops_leading_orphan_tool_result() {
        use sampling_types::ToolCall;
        let big = "y".repeat(2000);
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "c1".into(),
                name: "read_file".to_string(),
                arguments: big.into(),
            }]),
            ConversationItem::tool_result("c1", "result-old"),
            ConversationItem::user("recent"),
        ];
        let out = fit_conversation_to_budget(conv, 5);
        assert!(
            !out.iter()
                .any(|i| matches!(i, ConversationItem::ToolResult(_))),
            "orphaned tool result (its assistant turn was trimmed) must be dropped"
        );
        assert!(matches!(out.first(), Some(ConversationItem::System(_))));
    }
    /// An oversized most-recent tool result is kept but truncated in place (with its `tool_use`), not dropped.
    #[test]
    fn fit_truncates_oversized_tail_result_in_place() {
        use sampling_types::ToolCall;
        let huge = "z".repeat(40_000);
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("old"),
            ConversationItem::assistant("old answer"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "c1".into(),
                name: "read_file".to_string(),
                arguments: "{}".into(),
            }]),
            ConversationItem::tool_result("c1", huge.as_str()), // triggering result
        ];
        let out = fit_conversation_to_budget(conv, 100);
        let tr = out
            .iter()
            .find_map(|i| match i {
                ConversationItem::ToolResult(t) => Some(t),
                _ => None,
            })
            .expect("triggering tool result must be kept (truncated), not dropped");
        assert!(
            tr.content.contains("truncated"),
            "kept result must carry a truncation marker"
        );
        assert!(
            tr.content.len() < huge.len(),
            "kept result content must be shortened"
        );
        assert!(
            out.iter()
                .any(|i| matches!(i, ConversationItem::Assistant(a) if !a.tool_calls.is_empty())),
            "owning assistant tool_use must be kept so the result is not orphaned"
        );
        let est: u64 = out.iter().map(estimate_item_tokens).sum();
        assert!(
            est <= 100 + 64,
            "truncated unit should fit budget (+ marker slack)"
        );
    }
    /// A single oversized trailing text turn is also truncated in place, not dropped.
    #[test]
    fn fit_truncates_oversized_tail_text_item() {
        let huge = "q".repeat(40_000);
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("old"),
            ConversationItem::assistant(huge.as_str()),
        ];
        let out = fit_conversation_to_budget(conv, 100);
        match out.last().expect("tail kept") {
            ConversationItem::Assistant(a) => {
                assert!(a.content.contains("truncated"));
                assert!(a.content.len() < huge.len());
            }
            other => panic!("expected truncated trailing assistant, got {other:?}"),
        }
    }
    /// Incompactable-state regression: `fit` must charge images (765 each), so an image-heavy old turn is trimmed.
    #[test]
    fn fit_counts_user_images_against_budget() {
        use sampling_types::ContentPart;
        let mut img_user = ConversationItem::user("");
        for _ in 0..50 {
            img_user.add_image("data:image/png;base64,AAAA");
        }
        let conv = vec![
            ConversationItem::system("sys"),
            img_user, // old turn, huge by image charges, ~0 by text bytes
            ConversationItem::user("recent question"),
            ConversationItem::assistant("recent answer"),
        ];
        let out = fit_conversation_to_budget(conv, 1_000);
        assert!(
            !out.iter().any(|i| matches!(
                i,
                ConversationItem::User(u)
                    if u.content.iter().any(|p| matches!(p, ContentPart::Image { .. }))
            )),
            "image-heavy old turn must be counted (765/image) and trimmed, not kept"
        );
        assert!(
            out.iter().any(|i| i.text_content() == "recent answer"),
            "recent turn must survive"
        );
    }
    /// Incompactable-state regression: `fit` must charge encrypted-reasoning bytes (enc/4), so the old turn is trimmed.
    #[test]
    fn fit_counts_encrypted_reasoning_against_budget() {
        use sampling_types::rs;
        let big_enc = "Z".repeat(40_000);
        let reasoning = ConversationItem::Reasoning(rs::ReasoningItem {
            id: Some("r1".to_string()),
            summary: vec![],
            content: None,
            encrypted_content: Some(big_enc),
            status: None,
        });
        let conv = vec![
            ConversationItem::system("sys"),
            reasoning, // old turn, huge by encrypted bytes, 0 by visible text
            ConversationItem::user("recent question"),
            ConversationItem::assistant("recent answer"),
        ];
        let out = fit_conversation_to_budget(conv, 1_000);
        assert!(
            !out.iter()
                .any(|i| matches!(i, ConversationItem::Reasoning(_))),
            "encrypted-reasoning bytes must be counted and the old turn trimmed"
        );
        assert!(
            out.iter().any(|i| i.text_content() == "recent answer"),
            "recent turn must survive"
        );
    }
}

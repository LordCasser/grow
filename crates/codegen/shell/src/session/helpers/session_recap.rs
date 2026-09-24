//! Session recap generation helpers.
//!
//! A *recap* is a short "where was I" summary of the session so far, modelled
//! on common coding-agent `/recap` + automatic session-recap features. Unlike
//! compaction, a recap never mutates the conversation: it is generated from a
//! read-only snapshot and surfaced to the client for display only.
//!
//! Generation snapshots the parent session's conversation, projects it into
//! portable history, and appends a single instruction turn that asks for the
//! recap. The pure helpers here build that request and tidy the model's output;
//! the actual model call lives on the `SessionActor` (`handle_recap`).

use crate::sampling::ConversationItem;
use crate::session::helpers::text::floor_char_boundary;
use chat_state::{compaction_utils, estimate_conversation_tokens, estimate_item_tokens};

/// Hard cap on the recap text length (characters). Generous headroom: the recap
/// instruction targets ~25–40 words (≈240 chars at the top end), so this only
/// guards against runaway model output and never cuts a normal recap.
const RECAP_MAX_CHARS: usize = 1200;

/// Build the instruction turn appended to the conversation snapshot.
///
/// All recap directions live in this single user message (wrapped in a
/// `<system-reminder>`) rather than a separate system prompt, so the
/// conversation prefix — including the agent's real system prompt at
/// `conversation[0]` — is reused verbatim and the prompt cache stays warm.
///
/// `tag` is the reminder tag for the active harness (`"system-reminder"`, or
/// template-specific tags).
///
/// Body text only — the pager adds `Recap —` on render (manual and auto).
///
/// Keep in sync with the recap prompt eval harness (hillclimb there first).
/// Few-shots must stay synthetic — never embed real eval/session content.
pub(crate) fn recap_instruction(tag: &str) -> String {
    format!(
        "<{tag}>Write ONE sentence recap body for a user returning from idle. \
         Output ONLY the body (the UI adds the \"Recap —\" label). \
         Do NOT call any tools — respond with plain text only.\n\n\
         Lead with agency:\n\
         - \"You asked …\" if the session was mainly questions, walkthroughs, or review with no landed change.\n\
         - \"We <past-tense verb> …\" if the agent implemented, fixed, merged, or changed code/config/docs \
         (e.g. \"We fixed …\", \"We merged …\", \"We wired …\" — not \"We did fix\" / \"We did merge\").\n\
         - If almost nothing happened: \"You had just begun this session.\"\n\n\
         Shape: <lead>: <concrete specifics — crate/file/flag/behavior/endpoint>. ~25–40 words.\n\n\
         Synthetic examples (style only — adapt to THIS session, do not copy):\n\n\
         You asked how retries work in the payment client: exponential backoff in `payments/retry.rs`, max 5 attempts, 429s only.\n\n\
         You asked for a walkthrough of the auth middleware change: warn-only mode in the API layer, no hard fail on missing claims.\n\n\
         We fixed the flaky integration test: race in `queue_worker` shutdown by awaiting the drain channel before exit.\n\n\
         We merged the feature branch: kept the new diagnostics hooks, dropped the obsolete feature flag in `config/flags.toml`.\n\n\
         Bad (never):\n\
         - Start with Recap / Session recap / extra labels\n\
         - Quote or restate this reminder or any system prompt\n\
         - Bullets, markdown, code fences, extra sentences\n\
         - Call tools or emit tool/function calls\n\
         - Invent work not reflected in the session</{tag}>"
    )
}

/// Prepare the conversation snapshot for a recap request.
///
/// 1. Projects the snapshot into portable history. The projector retains
///    complete local tool exchanges (including result attachments), drops
///    incomplete or ambiguous calls, and optionally keeps visible reasoning.
/// 2. Appends the recap instruction as a final user turn.
pub(crate) fn build_recap_items(
    conversation: Vec<ConversationItem>,
    tag: &str,
    strip_reasoning: bool,
) -> Vec<ConversationItem> {
    // Messages cannot accept durable reasoning without a top-level thinking
    // configuration. Chat filters this display-only item at wire conversion;
    // Responses can retain visible reasoning when its route supports it.
    let mut items = sampling_types::project_portable_history_with_reasoning(
        &conversation,
        (!strip_reasoning).then_some(sampling_types::ApiBackend::Responses),
    );

    items.push(ConversationItem::user(recap_instruction(tag)));
    items
}

/// Cap on the effective context window for recap budgeting: the verified
/// `max_prompt_length` for current `grow-build` / `grow-4.5` product backends
/// (`500000`). Applied via `min(window, CAP)`, so a smaller real window still
/// wins (e.g. a 256k legacy model or a debug override).
const RECAP_CONTEXT_WINDOW_CAP: u64 = 500_000;

/// Fraction of the conservative window a recap may occupy. It follows the
/// client default rather than a remotely raised session threshold, so recap
/// remains conservative while sharing one authoritative default.
const RECAP_BUDGET_THRESHOLD_PERCENT: u64 =
    crate::util::config::DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT as u64;

/// Estimator/serialization slack (mirrors memory-flush's soft-threshold pad). The
/// appended instruction is reserved SEPARATELY via `snapshot_budget`, so it is not
/// double-counted here. (`max_prompt_length` is input-length, so output doesn't count.)
const RECAP_BUDGET_HEADROOM_TOKENS: u64 = 4_000;

/// Budget-aware variant of [`build_recap_items`]. Best-effort: returns a
/// structurally-valid, non-empty request trimmed to the estimated prompt budget
/// (the same bytes/4 estimator compaction triggers on) to prevent
/// `ic_400_prompt_too_long` on long sessions. Not an absolute guarantee — a
/// degenerate tiny window, an oversized retained `System` prefix, or estimator
/// optimism can still exceed the real limit (the default threshold + headroom + 500k cap make
/// that unlikely for normal grow-build sessions).
///
/// * Fast path — if the whole snapshot already fits, returns
///   `build_recap_items(...)` (honors the caller's `strip_reasoning`).
/// * Over budget — project the snapshot into portable history, front-trim to fit via
///   `fit_conversation_to_budget` (System kept, most-recent turn truncated in
///   place, never emptied), then project once more to close any tool boundary
///   the trim may have split before appending the instruction.
///
/// `context_window` MUST be the window of the model the recap is actually sent to
/// (today the session model).
pub(crate) fn budget_recap_items(
    conversation: Vec<ConversationItem>,
    tag: &str,
    strip_reasoning: bool,
    context_window: u64,
) -> Vec<ConversationItem> {
    let effective_window = context_window.min(RECAP_CONTEXT_WINDOW_CAP);
    let prompt_budget = (effective_window.saturating_mul(RECAP_BUDGET_THRESHOLD_PERCENT) / 100)
        .saturating_sub(RECAP_BUDGET_HEADROOM_TOKENS);

    let instruction = ConversationItem::user(recap_instruction(tag));
    let snapshot_budget = prompt_budget.saturating_sub(estimate_item_tokens(&instruction));

    // Use the original snapshot for the fast/trimmed branch decision. Portable
    // projection can normalize an item into different text, so this estimate
    // is not treated as a strict post-projection bound.
    let pre_tokens = estimate_conversation_tokens(&conversation);
    if pre_tokens <= snapshot_budget {
        return build_recap_items(conversation, tag, strip_reasoning);
    }

    // Project before trimming so completed calls/results (and their images) are
    // eligible evidence. The second projection below is required because a
    // front trim can otherwise retain one half of an otherwise valid exchange.
    // Once trimming is required, preserve the previous budget-path behavior:
    // remove durable reasoning while projecting tool evidence. The fast path
    // above still honors the backend-specific `strip_reasoning` setting.
    let snapshot = sampling_types::project_portable_history(&conversation);
    let trimmed = compaction_utils::fit_conversation_to_budget(snapshot, snapshot_budget);
    let mut items = sampling_types::project_portable_history(&trimmed);
    let post_tokens = estimate_conversation_tokens(&items);
    tracing::debug!(
        context_window,
        effective_window,
        prompt_budget,
        snapshot_budget,
        pre_tokens,
        post_tokens,
        "recap over budget: trimmed conversation to fit"
    );
    items.push(instruction);
    items
}

/// Minimum main turns before an automatic return-from-away recap (manual exempt).
pub(crate) const MIN_TURNS_FOR_AUTO_RECAP: usize = 3;

/// Real user prompts (`synthetic_reason.is_none()`), not assistant/tool items.
pub(crate) fn main_turn_count(conversation: &[ConversationItem]) -> usize {
    conversation
        .iter()
        .filter(|item| {
            matches!(
                item,
                ConversationItem::User(u) if u.synthetic_reason.is_none()
            )
        })
        .count()
}

/// Manual: any `main_turns > 0`. Auto: new turn since `last`, min turns, idle.
pub(crate) fn recap_gate(
    main_turns: usize,
    last: usize,
    auto: bool,
    idle_ok: bool,
) -> Result<(), &'static str> {
    if main_turns == 0 {
        return Err("no main turns yet");
    }
    if auto {
        if main_turns <= last {
            return Err("no new main turn since last recap");
        }
        if main_turns < MIN_TURNS_FOR_AUTO_RECAP {
            return Err("fewer than min turns for auto recap");
        }
        if !idle_ok {
            return Err("idle threshold not met");
        }
    }
    Ok(())
}

/// Auto recaps longer than this (raw bytes) are saved but not shown.
pub(crate) const RECAP_AUTO_RAW_DISPLAY_MAX: usize = 500;

/// Auto only: long-tail output — persist artifact, do not display.
pub(crate) fn should_suppress_auto_recap_display(raw: &str, summary: &str) -> bool {
    if raw.len() > RECAP_AUTO_RAW_DISPLAY_MAX {
        return true;
    }
    summary.ends_with('\u{2026}') && summary.len() >= RECAP_MAX_CHARS
}

/// Clean the model's raw recap output into a readable one-liner body.
///
/// Normalizes whitespace, strips a stray leading label/quotes if the model
/// added one anyway, and caps length at [`RECAP_MAX_CHARS`] as a safety net
/// against runaway output (the cap is generous, so a normal recap is never
/// cut). Does not prepend `Recap —` — the pager always prefixes with that
/// label on render.
pub(crate) fn clean_recap_text(raw: &str) -> String {
    // Collapse runs of whitespace/newlines into single spaces (one scrollback line).
    let mut out: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");

    // Strip a stray leading label if the model added one anyway.
    for label in [
        "Recap —",
        "Recap—",
        "Recap -",
        "Recap:",
        "recap:",
        "Session recap:",
        "Summary:",
    ] {
        if let Some(rest) = out.strip_prefix(label) {
            out = rest.trim_start().to_string();
            break;
        }
    }

    // Strip symmetric wrapping quotes around the whole string.
    if out.len() >= 2 {
        let bytes = out.as_bytes();
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            out = out[1..out.len() - 1].trim().to_string();
        }
    }

    if out.len() > RECAP_MAX_CHARS {
        let cut = floor_char_boundary(&out, RECAP_MAX_CHARS);
        out.truncate(cut);
        out = out.trim_end().to_string();
        out.push('\u{2026}'); // …
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampling::ConversationItem;

    #[test]
    fn clean_collapses_whitespace_and_newlines() {
        let raw = "Refactored   the\n\nparser\tand added   tests.";
        assert_eq!(
            clean_recap_text(raw),
            "Refactored the parser and added tests."
        );
    }

    #[test]
    fn clean_strips_leading_label() {
        assert_eq!(
            clean_recap_text("Recap: fixed the auth bug"),
            "fixed the auth bug"
        );
        assert_eq!(
            clean_recap_text("Session recap: wired up the API"),
            "wired up the API"
        );
    }

    #[test]
    fn clean_strips_wrapping_quotes() {
        assert_eq!(clean_recap_text("\"did the thing\""), "did the thing");
        assert_eq!(clean_recap_text("'did the thing'"), "did the thing");
    }

    #[test]
    fn clean_caps_length_on_char_boundary() {
        // Far past the cap → truncated on a char boundary with an ellipsis.
        let long = "word ".repeat(RECAP_MAX_CHARS);
        let out = clean_recap_text(&long);
        assert!(out.len() <= RECAP_MAX_CHARS + 4, "len was {}", out.len());
        assert!(out.ends_with('\u{2026}'));
    }

    #[test]
    fn clean_cap_is_utf8_safe() {
        // 3-byte chars straddling the byte cap must not panic.
        let big = "あ ".repeat(RECAP_MAX_CHARS);
        let out = clean_recap_text(&big);
        assert!(out.ends_with('\u{2026}'));
    }

    #[test]
    fn clean_keeps_normal_recap_in_full() {
        // A normal multi-sentence recap is well under the generous cap, so it is
        // returned verbatim — never cut mid-sentence.
        let recap = "We fixed the flaky integration test by awaiting the drain \
                     channel before exit, added a regression test for the shutdown \
                     path, and updated the runbook with the new sequence.";
        let out = clean_recap_text(recap);
        assert!(out.len() < RECAP_MAX_CHARS);
        assert!(!out.ends_with('\u{2026}'));
        assert!(out.ends_with("with the new sequence."));
    }

    #[test]
    fn build_appends_instruction_user_turn() {
        let conv = vec![
            ConversationItem::system("sys".to_string()),
            ConversationItem::user("hello".to_string()),
            ConversationItem::assistant("hi".to_string()),
        ];
        let items = build_recap_items(conv, "system-reminder", true);
        assert!(matches!(items.last(), Some(ConversationItem::User(_))));
        // System prompt prefix is preserved verbatim for cache reuse.
        assert!(matches!(items.first(), Some(ConversationItem::System(_))));
    }

    #[test]
    fn build_truncates_trailing_tool_result() {
        let conv = vec![
            ConversationItem::system("sys".to_string()),
            ConversationItem::user("hello".to_string()),
            ConversationItem::tool_result("call-1".to_string(), "output".to_string()),
        ];
        let items = build_recap_items(conv, "system-reminder", false);
        // The dangling ToolResult is dropped; only system + user + instruction remain.
        assert_eq!(items.len(), 3);
        assert!(matches!(items.last(), Some(ConversationItem::User(_))));
        assert!(
            !items
                .iter()
                .any(|i| matches!(i, ConversationItem::ToolResult(_)))
        );
    }

    #[test]
    fn build_preserves_complete_tool_evidence_and_drops_partial_calls() {
        use sampling_types::ContentPart;

        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("inspect the deployment"),
            ConversationItem::assistant("I checked the deployment state."),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "deciding which evidence to inspect",
            )),
            ConversationItem::Assistant(sampling_types::AssistantItem {
                content: "The verification is in progress.".into(),
                tool_calls: vec![
                    mk_tool_call("complete", "{}"),
                    mk_tool_call("partial", "{}"),
                ],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
                response_messages: Vec::new(),
            }),
            ConversationItem::tool_result_with_images(
                "complete",
                "verified",
                vec![
                    ContentPart::Text {
                        text: "attachment text".into(),
                    },
                    ContentPart::Image {
                        url: "data:image/png;base64,fixture".into(),
                        description: None,
                    },
                ],
            ),
        ];

        let items = build_recap_items(conv, "system-reminder", false);
        let tool_assistant = items.iter().find_map(|item| match item {
            ConversationItem::Assistant(assistant) if !assistant.tool_calls.is_empty() => {
                Some(assistant)
            }
            _ => None,
        });
        let tool_assistant = tool_assistant.expect("complete tool exchange must be retained");
        assert_eq!(
            tool_assistant.content.as_ref(),
            "The verification is in progress."
        );
        assert_eq!(tool_assistant.tool_calls.len(), 1);
        assert_eq!(tool_assistant.tool_calls[0].id.as_ref(), "complete");

        let result = items.iter().find_map(|item| match item {
            ConversationItem::ToolResult(result) if result.tool_call_id == "complete" => {
                Some(result)
            }
            _ => None,
        });
        let result = result.expect("complete tool result must be retained");
        assert_eq!(result.content.as_ref(), "verified");
        assert_eq!(
            result.images.len(),
            2,
            "result attachments must survive projection"
        );
        assert!(!items.iter().any(|item| {
            matches!(item, ConversationItem::Assistant(assistant)
                if assistant.tool_calls.iter().any(|call| call.id.as_ref() == "partial"))
        }));
    }

    #[test]
    fn main_turn_count_counts_real_users_only() {
        use sampling_types::{ContentPart, SyntheticReason, ToolCall, UserItem};
        use std::sync::Arc;

        let conv = vec![
            ConversationItem::system("sys".to_string()),
            ConversationItem::user("hi".to_string()),
            ConversationItem::assistant("hello".to_string()),
            ConversationItem::user("again".to_string()),
            ConversationItem::User(UserItem {
                content: vec![ContentPart::Text {
                    text: Arc::from("injected"),
                }],
                synthetic_reason: Some(SyntheticReason::SystemReminder),
                permission_evidence: None,
                ..Default::default()
            }),
        ];
        assert_eq!(main_turn_count(&conv), 2);
        let empty: Vec<ConversationItem> = vec![ConversationItem::system("sys".to_string())];
        assert_eq!(main_turn_count(&empty), 0);

        let tool_loop = vec![
            ConversationItem::user("fix it".to_string()),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: Arc::from("c1"),
                name: "read_file".into(),
                arguments: Arc::from("{}"),
            }]),
            ConversationItem::tool_result("c1".to_string(), "ok".to_string()),
            ConversationItem::assistant("done".to_string()),
        ];
        assert_eq!(main_turn_count(&tool_loop), 1);
    }

    #[test]
    fn gate_allows_first_recap_when_last_is_zero() {
        assert!(recap_gate(1, 0, false, false).is_ok());
    }

    #[test]
    fn gate_manual_allows_re_recap_on_same_main_turn() {
        assert!(recap_gate(2, 2, false, true).is_ok());
        assert!(recap_gate(3, 3, false, false).is_ok());
    }

    #[test]
    fn gate_auto_denies_second_recap_on_same_main_turn() {
        assert_eq!(
            recap_gate(3, 3, true, true),
            Err("no new main turn since last recap")
        );
    }

    #[test]
    fn gate_allows_after_new_main_turn() {
        assert!(recap_gate(3, 2, false, false).is_ok());
        assert!(recap_gate(3, 2, true, true).is_ok());
    }

    #[test]
    fn gate_auto_requires_min_turns_and_idle() {
        assert_eq!(
            recap_gate(2, 0, true, true),
            Err("fewer than min turns for auto recap")
        );
        assert_eq!(recap_gate(3, 0, true, false), Err("idle threshold not met"));
        assert!(recap_gate(3, 0, true, true).is_ok());
    }

    #[test]
    fn gate_denies_zero_main_turns() {
        assert_eq!(recap_gate(0, 0, false, true), Err("no main turns yet"));
    }

    #[test]
    fn gate_allows_after_compaction_heal_watermark() {
        assert!(recap_gate(3, 2, false, false).is_ok());
        assert!(recap_gate(3, 3, false, false).is_ok());
        assert_eq!(
            recap_gate(3, 3, true, false),
            Err("no new main turn since last recap")
        );
    }

    #[test]
    fn suppress_auto_long_tail_raw_over_display_max() {
        let normal = "We fixed gitignored project Claude commands loading as slash skills.";
        assert!(!should_suppress_auto_recap_display(
            normal,
            &clean_recap_text(normal)
        ));
        let long_raw = "Creating the PR from the worktree. ".repeat(20);
        assert!(long_raw.len() > RECAP_AUTO_RAW_DISPLAY_MAX);
        assert!(should_suppress_auto_recap_display(
            &long_raw,
            &clean_recap_text(&long_raw)
        ));
    }

    #[test]
    fn suppress_auto_when_clean_hits_hard_cap_ellipsis() {
        let huge = "word ".repeat(RECAP_MAX_CHARS);
        let summary = clean_recap_text(&huge);
        assert!(summary.ends_with('\u{2026}'));
        assert!(should_suppress_auto_recap_display(&huge, &summary));
    }

    #[test]
    fn normal_auto_recap_not_suppressed() {
        let raw = "We fixed the flaky integration test: race in queue_worker shutdown.";
        assert!(raw.len() < RECAP_AUTO_RAW_DISPLAY_MAX);
        assert!(!should_suppress_auto_recap_display(
            raw,
            &clean_recap_text(raw)
        ));
    }

    #[test]
    fn instruction_uses_provided_tag() {
        assert!(recap_instruction("system_reminder").contains("<system_reminder>"));
        assert!(recap_instruction("system-reminder").contains("</system-reminder>"));
    }

    #[test]
    fn instruction_asks_for_one_sentence_body() {
        let text = recap_instruction("system-reminder");
        assert!(text.contains("Output ONLY the body"));
        assert!(text.contains("You asked"));
        assert!(text.contains("We fixed"));
        assert!(text.contains("We merged"));
        assert!(text.contains("payments/retry.rs"));
        assert!(text.contains("queue_worker"));
        assert!(text.contains("We fixed the flaky"));
        assert!(text.contains("We merged the feature"));
        assert!(!text.contains("217584"));
        assert!(!text.contains("lead with \"Recap"));
    }

    #[test]
    fn clean_returns_body_without_recap_prefix() {
        assert_eq!(
            clean_recap_text("Recap: You fixed auth in foo.rs."),
            "You fixed auth in foo.rs."
        );
        assert_eq!(
            clean_recap_text("You fixed auth in foo.rs."),
            "You fixed auth in foo.rs."
        );
    }

    // ---- budget_recap_items ------------------------------------------------

    /// Recompute the prompt budget the helper uses, for end-state assertions.
    fn recap_prompt_budget(context_window: u64) -> u64 {
        (context_window.min(RECAP_CONTEXT_WINDOW_CAP) * RECAP_BUDGET_THRESHOLD_PERCENT / 100)
            .saturating_sub(RECAP_BUDGET_HEADROOM_TOKENS)
    }

    fn mk_reasoning(id: &str) -> ConversationItem {
        ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(format!(
            "secret thinking {id}"
        )))
    }

    fn mk_tool_call(id: &str, args: &str) -> sampling_types::ToolCall {
        use std::sync::Arc;
        sampling_types::ToolCall {
            id: Arc::from(id),
            name: "read_file".into(),
            arguments: Arc::from(args),
        }
    }

    #[test]
    fn budget_fast_path_matches_build_recap_items() {
        // Include a reasoning block so `strip_reasoning=true` actually exercises
        // stripping on the fits path (not just a no-op).
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("hello"),
            mk_reasoning("r1"),
            ConversationItem::assistant("hi"),
        ];
        let budgeted = budget_recap_items(conv.clone(), "system-reminder", true, 256_000);
        let built = build_recap_items(conv, "system-reminder", true);
        assert_eq!(
            serde_json::to_string(&budgeted).unwrap(),
            serde_json::to_string(&built).unwrap(),
            "under-budget snapshot must match build_recap_items verbatim"
        );
        assert!(
            !budgeted
                .iter()
                .any(|i| matches!(i, ConversationItem::Reasoning(_))),
            "strip_reasoning=true must strip reasoning on the fast path too"
        );
        assert!(matches!(
            budgeted.first(),
            Some(ConversationItem::System(_))
        ));
        assert!(matches!(budgeted.last(), Some(ConversationItem::User(_))));
    }

    #[test]
    fn budget_over_budget_trims_within_budget() {
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("q1"),
            ConversationItem::assistant("a1"),
            ConversationItem::user("d".repeat(40_000)), // ~10k est tokens
            ConversationItem::assistant("a2"),
            ConversationItem::user("recent q"),
        ];
        let snapshot_plus_instruction = conv.len() + 1;
        let out = budget_recap_items(conv, "system-reminder", false, 8_000);
        assert!(
            estimate_conversation_tokens(&out) <= recap_prompt_budget(8_000),
            "trimmed recap must fit the prompt budget"
        );
        assert!(
            out.len() < snapshot_plus_instruction,
            "over-budget output must be strictly smaller than the untrimmed snapshot + instruction"
        );
        assert!(matches!(out.first(), Some(ConversationItem::System(_))));
        assert!(matches!(out.last(), Some(ConversationItem::User(_))));
    }

    #[test]
    fn budget_over_budget_drops_orphan_tool_result_at_front() {
        // The Assistant(tool_use) is heavy and gets excluded by the front-trim;
        // its ToolResult would then be a leading orphan — which must be dropped.
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::assistant_tool_calls(vec![mk_tool_call("c1", &"b".repeat(40_000))]),
            ConversationItem::tool_result("c1", "small result"),
            ConversationItem::user("recent"),
        ];
        let out = budget_recap_items(conv, "system-reminder", false, 8_000);
        let first_non_system = out
            .iter()
            .find(|i| !matches!(i, ConversationItem::System(_)));
        assert!(
            !matches!(first_non_system, Some(ConversationItem::ToolResult(_))),
            "retained tail must not begin with an orphan ToolResult"
        );
    }

    #[test]
    fn budget_over_budget_retains_bounded_complete_tool_evidence() {
        // The result is larger than the budget on purpose. The budget fitter
        // truncates the result in place while retaining its owning call; the
        // second portable projection then verifies that the pair remains valid.
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("c".repeat(40_000)), // oldest history, dropped
            ConversationItem::assistant_tool_calls(vec![mk_tool_call("c9", "{}")]),
            ConversationItem::tool_result("c9", "t".repeat(40_000)), // result, > budget
        ];
        let out = budget_recap_items(conv, "system-reminder", false, 8_000);

        assert!(matches!(out.last(), Some(ConversationItem::User(_))));
        let call = out.iter().find_map(|item| match item {
            ConversationItem::Assistant(assistant)
                if assistant
                    .tool_calls
                    .iter()
                    .any(|call| call.id.as_ref() == "c9") =>
            {
                Some(assistant)
            }
            _ => None,
        });
        assert!(
            call.is_some(),
            "the complete call must survive budget trimming"
        );
        let result = out.iter().find_map(|item| match item {
            ConversationItem::ToolResult(result) if result.tool_call_id == "c9" => Some(result),
            _ => None,
        });
        let result = result.expect("the paired result must survive budget trimming");
        assert!(
            result.content.contains("truncated"),
            "oversized result must be bounded in place"
        );
    }

    #[test]
    fn budget_giant_single_turn_truncated_in_place() {
        // A single turn larger than the whole budget must be kept, truncated in
        // place — never dropped to an empty request.
        let conv = vec![ConversationItem::user("y".repeat(200_000))];
        let out = budget_recap_items(conv, "system-reminder", false, 8_000);
        assert!(
            out.len() >= 2,
            "giant turn must be truncated in place, not dropped"
        );
        assert!(matches!(out.last(), Some(ConversationItem::User(_))));
        assert!(estimate_conversation_tokens(&out) <= recap_prompt_budget(8_000));
        let serialized = serde_json::to_string(&out).unwrap();
        assert!(
            serialized.contains("truncated"),
            "retained turn must carry the in-place truncation marker"
        );
    }

    #[test]
    fn budget_over_budget_drops_unpaired_reasoning_even_on_grow() {
        let conv = vec![
            mk_reasoning("r1"),
            ConversationItem::assistant("did stuff"),
            ConversationItem::user("z".repeat(40_000)),
        ];
        // Reasoning attached only to plain assistant prose is not portable
        // evidence, even though grow/Responses may retain supported reasoning
        // attached to a complete tool exchange.
        let out = budget_recap_items(conv, "system-reminder", false, 8_000);
        assert!(
            !out.iter()
                .any(|i| matches!(i, ConversationItem::Reasoning(_))),
            "unpaired reasoning must not enter the portable recap"
        );
    }

    #[test]
    fn budget_fast_path_keeps_supported_reasoning_on_grow() {
        let conv = vec![
            mk_reasoning("r1"),
            ConversationItem::assistant_tool_calls(vec![mk_tool_call("c1", "{}")]),
            ConversationItem::tool_result("c1", "done"),
            ConversationItem::user("small"),
        ];
        // Fits under a large window on grow (strip_reasoning=false): the
        // projector retains visible reasoning needed by supported Responses
        // routes when it belongs to a complete tool exchange.
        let out = budget_recap_items(conv, "system-reminder", false, 256_000);
        assert!(
            out.iter()
                .any(|i| matches!(i, ConversationItem::Reasoning(_))),
            "fits path on grow must keep supported reasoning"
        );
    }

    #[test]
    fn budget_1m_clamps_to_floor_and_256k_shrinks() {
        // One giant user turn larger than any window's budget: fit truncates it in
        // place to exactly snapshot_budget, so the output size is a direct readout
        // of the budget the helper used.
        let giant = || {
            vec![
                ConversationItem::system("sys"),
                ConversationItem::user("x".repeat(2_400_000)),
            ]
        };
        let out_500 = budget_recap_items(giant(), "system-reminder", false, 500_000);
        let out_1m = budget_recap_items(giant(), "system-reminder", false, 1_000_000);
        let out_256 = budget_recap_items(giant(), "system-reminder", false, 256_000);

        // 1M advertises a larger window but clamps to the 500k floor => identical.
        assert_eq!(
            estimate_conversation_tokens(&out_1m),
            estimate_conversation_tokens(&out_500),
            "1M must clamp to the 500k floor (identical budget)"
        );
        assert!(estimate_conversation_tokens(&out_500) <= recap_prompt_budget(500_000));
        // 256k is below the floor => strictly smaller budget and output.
        assert!(
            estimate_conversation_tokens(&out_256) < estimate_conversation_tokens(&out_500),
            "256k must produce a smaller budget than the 500k floor"
        );
        assert!(estimate_conversation_tokens(&out_256) <= recap_prompt_budget(256_000));
    }

    #[test]
    fn budget_empty_conversation_returns_only_instruction() {
        // The helper is directly reachable (the handler gates `vec![]` upstream);
        // an empty snapshot must return just the appended instruction, no panic.
        let out = budget_recap_items(Vec::new(), "system-reminder", false, 256_000);
        assert_eq!(out.len(), 1, "empty input yields only the instruction turn");
        assert!(matches!(out.last(), Some(ConversationItem::User(_))));
    }

    #[test]
    fn budget_threshold_boundary_selects_fast_vs_over_budget() {
        // Lock the `<=` fits-vs-over-budget comparison. At exactly snapshot_budget
        // the fast path is taken (supported reasoning kept); one token over
        // takes the over-budget path and trims the oldest reasoning boundary.
        let tag = "system-reminder";
        let instruction_tokens =
            estimate_item_tokens(&ConversationItem::user(recap_instruction(tag)));
        let snapshot_budget = recap_prompt_budget(8_000).saturating_sub(instruction_tokens);
        assert!(snapshot_budget > 8, "window must leave room for the probe");

        let call = ConversationItem::assistant_tool_calls(vec![mk_tool_call("boundary", "{}")]);
        let result = ConversationItem::tool_result("boundary", "done");
        let reasoning_tokens = estimate_item_tokens(&mk_reasoning("r"));
        let call_tokens = estimate_item_tokens(&call);
        let result_tokens = estimate_item_tokens(&result);
        let filler_tokens = snapshot_budget - reasoning_tokens - call_tokens - result_tokens;

        // Exactly at budget => fast path (`<=`) keeps supported reasoning.
        let at = vec![
            ConversationItem::user("a".repeat((filler_tokens * 4) as usize)),
            mk_reasoning("r"),
            call.clone(),
            result.clone(),
        ];
        assert_eq!(estimate_conversation_tokens(&at), snapshot_budget);
        let out_at = budget_recap_items(at, tag, false, 8_000);
        assert!(
            out_at
                .iter()
                .any(|i| matches!(i, ConversationItem::Reasoning(_))),
            "exactly-at-budget must take the fast path (reasoning kept), locking `<=`"
        );

        // One token over => over-budget path trims away the oldest reasoning.
        let over = vec![
            ConversationItem::user("a".repeat((filler_tokens * 4 + 4) as usize)),
            mk_reasoning("r"),
            call,
            result,
        ];
        assert!(estimate_conversation_tokens(&over) > snapshot_budget);
        let out_over = budget_recap_items(over, tag, false, 8_000);
        assert!(
            !out_over
                .iter()
                .any(|i| matches!(i, ConversationItem::Reasoning(_))),
            "one token over budget must take the over-budget path (reasoning trimmed)"
        );
    }

    #[test]
    fn budget_degenerate_tiny_window_stays_valid_and_nonempty() {
        // Window below the headroom => prompt_budget saturates to 0. The
        // instruction is still appended, so the output necessarily exceeds the
        // computed 0 budget but stays tiny and structurally valid (cannot cause a
        // 400). Asserts graceful degradation — NOT `est <= budget` (documents the
        // informational degenerate-window behavior).
        let conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("w".repeat(40_000)),
        ];
        let out = budget_recap_items(conv, "system-reminder", false, 1_000);
        assert!(
            !out.is_empty(),
            "a degenerate tiny window must still return a valid, non-empty request"
        );
        assert!(matches!(out.last(), Some(ConversationItem::User(_))));
    }
}

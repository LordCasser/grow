//! User-message construction concern for `SessionActor`: runtime-context
//! snapshots, large-prompt offload/truncation, and image payload preparation.
#![allow(clippy::items_after_test_module)]
use super::*;
/// Normalize a free-form name (e.g. an MCP server identifier) into a
/// single safe filesystem segment.
///
/// Replaces anything outside `[A-Za-z0-9._-]` with `_` so the result is a
/// portable directory name on macOS/Linux.
/// Pick the URL value sent to the upstream API for a user-attached image.
///
/// Attachments are always content-addressable inline bytes by the time they
/// reach this boundary. A mutable HTTP locator is never a valid image identity:
/// it could change between the primary request and an irreversible Sideband
/// transcription. `normalize_images` drops inputs without decodable bytes.
pub(super) fn pick_user_image_url(image: &agent_client_protocol::ImageContent) -> String {
    format!("data:{};base64,{}", image.mime_type, image.data)
}
/// True iff `conversation` already contains a tagged project-instructions reminder.
/// Read-only; used by `spawn_session_actor` for idempotent AGENTS.md injection
/// so resumed sessions and forks don't duplicate the message.
pub(super) fn conversation_has_project_instructions(conversation: &[ConversationItem]) -> bool {
    conversation.iter().any(is_project_instructions)
}
/// A project-instructions (AGENTS.md) reminder is a `User` item explicitly tagged
/// [`SyntheticReason::ProjectInstructions`]. Single source of truth for both
/// spawn-time idempotent injection and compaction de-duplication.
pub(super) fn is_project_instructions(item: &ConversationItem) -> bool {
    matches!(
        item,
        ConversationItem::User(user)
            if user.synthetic_reason == Some(SyntheticReason::ProjectInstructions)
    )
}
pub(super) const LARGE_PROMPT_THRESHOLD: usize = 25_000;
pub(super) const TRUNCATED_PROMPT_PREFIX_SIZE: usize = 25_000;
/// Percent of the bounded-prompt budget given to the query (capped; rest is context head).
const LARGE_QUERY_BUDGET_PERCENT: usize = 80;
/// Bytes kept at the TAIL when bounding head+tail, so a trailing question survives.
const BOUNDED_TAIL_BUDGET: usize = 4_000;
/// Bytes reserved for skill instructions (own budget, not crowded out by the query).
pub(super) const SKILL_INLINE_BUDGET: usize = 4_000;
/// Marker between the head and tail of an elided block. Single source of truth.
pub(super) const ELISION_MARKER: &str =
    "\n\n…[middle truncated — full text in the offloaded file]…\n\n";
/// Stable marker opening the offload notice. Single source of truth (for a future strip-on-re-read).
pub(super) const OFFLOAD_NOTICE_MARKER: &str = "[Full request offloaded to file]";
/// In-band notice that REPLACES the offload notice when the full request could
/// not be persisted to the session file (write error or task-join failure).
/// References no path — there is no file to read — so the model is never told to
/// `read_file` a file that does not exist. The bounded head+tail excerpt remains.
const OFFLOAD_FAILED_NOTICE: &str = "\n\n[Full request could not be saved to a file — the excerpt above is truncated. Answer from it, and ask the user to resend the full content if anything essential is missing.]";
/// UTF-8-safe suffix: the last `<= max_bytes` bytes of `s`, on a char boundary.
pub(super) fn truncate_bytes_suffix(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len() - max_bytes;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}
/// Bound `s` to `budget` as HEAD + [`ELISION_MARKER`] + TAIL (trailing question survives). UTF-8-safe.
pub(super) fn bound_head_tail(s: &str, budget: usize) -> String {
    if s.len() <= budget {
        return s.to_string();
    }
    if budget <= ELISION_MARKER.len() {
        return truncate_bytes(s, budget).to_string();
    }
    let content_budget = budget - ELISION_MARKER.len();
    let tail_len = BOUNDED_TAIL_BUDGET.min(content_budget / 2);
    let head_len = content_budget - tail_len;
    let head = truncate_bytes(s, head_len);
    let tail = truncate_bytes_suffix(s, tail_len);
    format!("{head}{ELISION_MARKER}{tail}")
}
/// Build the persisted offload notice with a host-independent blob identity.
/// The provider request projection resolves this identity to a local path.
pub(super) fn build_offload_notice(full_message_len: usize, blob_ref: &str) -> String {
    format!(
        "\n\n{OFFLOAD_NOTICE_MARKER} The text above was truncated ({full_message_len} bytes total). \
The user's FULL request — which may include their actual question and any skill instructions not shown above — is in this file:\n{}\n\
Read this file with read_file before responding; the question you must answer may only be there.",
        blob_ref,
    )
}
/// Build the bounded in-band message for an oversized prompt identified by an
/// immutable logical ref. Pure; preserves message ordering and stays bounded.
pub(super) fn build_truncated_prompt_message(
    context: &str,
    query: &str,
    skill_information: &str,
    blob_ref: &str,
    full_message_len: usize,
) -> String {
    let notice = build_offload_notice(full_message_len, blob_ref);
    debug_assert!(
        notice.len() < TRUNCATED_PROMPT_PREFIX_SIZE,
        "offload notice must be far smaller than the budget"
    );
    let available = TRUNCATED_PROMPT_PREFIX_SIZE.saturating_sub(notice.len());
    let skill_inline = bound_head_tail(skill_information, SKILL_INLINE_BUDGET.min(available));
    let skill_overhead = if skill_inline.is_empty() {
        0
    } else {
        1 + skill_inline.len()
    };
    let rest = available.saturating_sub(skill_overhead);
    let query_budget = rest.saturating_mul(LARGE_QUERY_BUDGET_PERCENT) / 100;
    let query_inline = bound_head_tail(query, query_budget);
    let context_budget = rest.saturating_sub(query_inline.len()).saturating_sub(2);
    let context_inline = truncate_bytes(context, context_budget);
    let query_block = if skill_inline.is_empty() {
        query_inline
    } else {
        format!("{query_inline}\n{skill_inline}")
    };
    if context_inline.is_empty() {
        format!("{query_block}{notice}")
    } else {
        format!("{query_block}\n\n{context_inline}{notice}")
    }
}
/// Replace the file-referencing offload `notice` embedded in `message` with the
/// no-file [`OFFLOAD_FAILED_NOTICE`]. Position-independent (the notice sits at the
/// end for grow ordering), so a failed offload never
/// leaves the model chasing a "read this file" pointer to a file that does not
/// exist. Returns `message` unchanged if the notice is absent (defensive).
pub(super) fn strip_offload_notice(message: &str, notice: &str) -> String {
    message.replacen(notice, OFFLOAD_FAILED_NOTICE, 1)
}
/// Write `full_message` via `writer` and return the bounded in-band message.
/// On write failure the bounded message is still returned (never the oversized original, so a failed offload can't
/// reintroduce the context-window overflow) but with the file-referencing notice
/// swapped for [`OFFLOAD_FAILED_NOTICE`], so the model isn't told to read a file
/// that was never written. The injected `writer` makes this hermetically testable.
pub(super) fn write_offload_and_build(
    full_message: &str,
    message: String,
    blob_ref: &str,
    writer: impl FnOnce(&[u8]) -> std::io::Result<()>,
) -> String {
    match writer(full_message.as_bytes()) {
        Ok(()) => message,
        Err(e) => {
            tracing::warn!(
                ?e,
                full_bytes = full_message.len(),
                "failed to write large-prompt offload file; sending bounded preview with no file reference"
            );
            let notice = build_offload_notice(full_message.len(), blob_ref);
            strip_offload_notice(&message, &notice)
        }
    }
}
impl SessionActor {
    /// Rewrite the user-message prefix at conversation index 1.
    /// Caller must guarantee zero turns.
    pub(super) fn rewrite_zero_turn_prefix(
        conversation: &mut Vec<ConversationItem>,
        new_prefix: String,
    ) {
        let is_prefix_slot = matches!(
            conversation.get(1),
            Some(ConversationItem::User(u)) if u.synthetic_reason.is_none()
        );
        if is_prefix_slot {
            conversation[1] = ConversationItem::user(new_prefix);
        } else {
            let insert_at = conversation.len().min(1);
            conversation.insert(insert_at, ConversationItem::user(new_prefix));
        }
    }
    pub(super) async fn build_user_message_prefix(&self) -> String {
        use agent::prompt::user_message::RuntimeContextSnapshot;

        let display_path = self
            .display_cwd
            .get()
            .map(|path| std::path::PathBuf::from(path.as_str()))
            .unwrap_or_else(|| std::path::PathBuf::from(&self.session_info.cwd));
        let execution_cwd = std::path::Path::new(&self.session_info.cwd);
        let today_local = chrono::Local::now().date_naive();
        let vcs = if self.startup_hints.skip_git_status {
            None
        } else {
            self.gather_vcs_snapshot(execution_cwd).await
        };
        let snapshot = RuntimeContextSnapshot {
            workspace_path: display_path,
            os_version: crate::util::uname::os_kernel_and_release(),
            shell: resolve_session_shell(),
            today_local,
            vcs,
        };
        self.last_announced_local_date.set(today_local);
        snapshot.render()
    }

    async fn gather_vcs_snapshot(
        &self,
        cwd: &std::path::Path,
    ) -> Option<agent::prompt::user_message::VcsSnapshot> {
        use agent::prompt::user_message::{VcsSnapshot, VcsSnapshotKind};
        use workspace::file_system::{git_status_short, jj_status};
        use workspace::session::git::VcsKind;

        let kind = match self.hooks.vcs_kind {
            VcsKind::None => return None,
            VcsKind::Git => VcsSnapshotKind::Git,
            VcsKind::JujutsuColocated => VcsSnapshotKind::Jujutsu,
        };
        let timeout = std::time::Duration::from_secs(5);
        let result = if self.hooks.vcs_kind.is_jj() {
            tokio::time::timeout(timeout, jj_status(cwd)).await
        } else {
            tokio::time::timeout(timeout, git_status_short(cwd)).await
        };
        match result {
            Ok(Ok(status)) => Some(VcsSnapshot { kind, status }),
            Ok(Err(error)) => {
                tracing::warn!(?error, ?kind, "runtime context VCS status failed");
                None
            }
            Err(_) => {
                tracing::warn!(?kind, "runtime context VCS status timed out after 5s");
                None
            }
        }
    }

    /// Build a `PathRewriter` for sanitizing overlay paths in model-facing text.
    ///
    /// Returns `None` when `display_cwd` is unset (no rewriting needed). Used
    /// by tool-result handlers to rewrite prompt_text, error messages, and any
    /// other model-visible content that may embed the real worktree cwd.
    pub(super) fn path_rewriter(&self) -> Option<crate::session::acp_conversion::PathRewriter> {
        crate::session::acp_conversion::PathRewriter::new(
            &self.session_info.cwd,
            self.display_cwd.get().map(|s| s.as_str()),
        )
    }
    /// If the prompt exceeds LARGE_PROMPT_THRESHOLD, write the full content to an
    /// immutable blob and persist a truncated version with its logical identity.
    ///
    /// Takes context and query separately to prioritise the query: kept intact
    /// when it fits, else bounded head+tail (trailing question survives).
    ///
    /// The sampling projection resolves the identity to a local path without
    /// mutating Timeline.
    /// Includes skill information in the assembled prompt.
    pub(super) async fn maybe_truncate_large_prompt_with_skills(
        &self,
        context: String,
        query: String,
        skill_information: String,
    ) -> String {
        let full_message = crate::session::prompt_parser::ParsedPrompt::assemble_parts_with_skills(
            &context,
            &query,
            &skill_information,
        );
        if full_message.len() <= LARGE_PROMPT_THRESHOLD {
            return full_message;
        }
        let blob_ref = get_prompt_blob_ref(&full_message);
        let hash = blob_ref
            .strip_prefix(crate::session::persistence::PROMPT_BLOB_REF_PREFIX)
            .expect("prompt blob reference uses the canonical prefix")
            .to_string();
        let full_len = full_message.len();
        let bounded = build_truncated_prompt_message(
            &context,
            &query,
            &skill_information,
            &blob_ref,
            full_len,
        );
        let join_fallback =
            strip_offload_notice(&bounded, &build_offload_notice(full_len, &blob_ref));
        let session = self.session_directory.clone();
        let offload = tokio::task::spawn_blocking(move || {
            write_offload_and_build(&full_message, bounded, &blob_ref, |bytes| {
                crate::session::persistence::write_immutable_blob_to_directory(
                    &session,
                    &std::path::Path::new("prompts").join(format!("{hash}.txt")),
                    bytes,
                )
            })
        })
        .await;
        match offload {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!(
                    ?e,
                    full_bytes = full_len,
                    "spawn_blocking join failed for large-prompt offload"
                );
                join_fallback
            }
        }
    }
    /// Add a followup message from the permission panel as a user turn in the conversation.
    /// This sends the message to the scrollback and adds it to the conversation context.
    pub(super) async fn add_followup_message_as_user_turn(&self, message: &str) {
        self.inject_synthetic_user_message(
            message,
            ConversationItem::user(message.to_string()),
            true,
            &[],
        )
        .await;
    }
}

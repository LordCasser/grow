//! Compaction methods for `SessionActor`.
//!
//! This module contains all compaction-related methods: manual `/compact`,
//! auto-compact threshold checks, inline auto-compact with auto-continue,
//! error-recovery compaction and preflight overflow detection. These methods form
//! a second `impl SessionActor` block that
//! lives alongside the primary actor module.
use super::SessionActor;
use crate::remote::DEFAULT_CONTEXT_WINDOW;
use crate::session::compaction_config::{
    SUPPRESS_AUTH, SUPPRESS_NONE, SUPPRESS_STICKY, SUPPRESS_TURN, SUPPRESS_UNTIL_SUCCESS,
};
use crate::session::helpers::CompactionStateContext;
use crate::session::helpers::compaction_context::CompactionInputs;
use crate::session::helpers::compaction_context::to_system_reminder;
use crate::session::helpers::session_compact::{
    CompactionOutcome, build_compaction_request_surface, is_context_length_error,
};
use acp_transport::protocol as acp;
use chat_state::compaction_utils::{
    is_degenerate_summary, plan_compaction_range, prepare_conversation_for_verbatim_summarization,
};
use sampling_types::{ApiBackend, ConversationItem};
use std::sync::Arc;

const COMPACTION_RETAIN_PERCENT: u64 = 16;
const MIN_COMPACTION_SOURCE_TOKENS: u64 = 5_000;

/// Resolve the context window to use for provider-error recovery.
///
/// An explicit provider overflow is authoritative even when the local request
/// estimate is lower: tokenizers and provider-side envelopes are not exactly
/// reproducible locally. Metadata-only recovery remains as a fallback for
/// providers whose error text is opaque but which report a smaller window.
fn sampling_error_compaction_window(
    err: &sampler::SamplingErrorInfo,
    projected_tokens: u64,
    configured_context_window: u64,
) -> Option<u64> {
    let metadata_context_window = err
        .model_metadata
        .as_ref()
        .and_then(|metadata| metadata.context_window)
        .filter(|window| *window > 0);
    if matches!(err.kind, sampler::SamplingErrorKind::Api) && is_context_length_error(&err.message)
    {
        return metadata_context_window
            .or_else(|| (configured_context_window > 0).then_some(configured_context_window));
    }
    metadata_context_window.filter(|window| projected_tokens > *window)
}

/// The terminal backend is shared by a primary session and its descendants.
/// Compaction context is session-local, so never leak another session's
/// background commands into this session's model-visible state.
fn background_tasks_owned_by(
    tasks: impl IntoIterator<Item = tools::computer::types::TaskSnapshot>,
    owner_session_id: &str,
) -> Vec<tools::computer::types::TaskSnapshot> {
    tasks
        .into_iter()
        .filter(|task| task.owner_session_id.as_deref() == Some(owner_session_id))
        .collect()
}

fn append_system_reminder_section(
    reminder: Option<String>,
    wrapper_tag: &str,
    section: &str,
) -> Option<String> {
    if section.trim().is_empty() {
        return reminder;
    }
    match reminder {
        Some(mut existing) => {
            let closing = format!("</{wrapper_tag}>");
            if let Some(position) = existing.rfind(&closing) {
                let separator = if existing[..position].ends_with("\n\n") {
                    ""
                } else if existing[..position].ends_with('\n') {
                    "\n"
                } else {
                    "\n\n"
                };
                existing.insert_str(position, &format!("{separator}{}\n", section.trim()));
            } else {
                existing.push_str(&format!(
                    "\n\n<{wrapper_tag}>\n{}\n</{wrapper_tag}>",
                    section.trim()
                ));
            }
            Some(existing)
        }
        None => Some(format!(
            "<{wrapper_tag}>\n{}\n</{wrapper_tag}>",
            section.trim()
        )),
    }
}

fn format_scheduled_tasks_compaction_reminder(
    tasks: &[tools::implementations::grow_build::scheduler::types::ScheduledTask],
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    use std::fmt::Write as _;

    let mut active = tasks
        .iter()
        .filter(|task| !task.is_expired(now))
        .collect::<Vec<_>>();
    active.sort_by(|left, right| {
        left.next_fire_at()
            .cmp(&right.next_fire_at())
            .then_with(|| left.id.cmp(&right.id))
    });
    if active.is_empty() {
        return None;
    }

    let mut body = String::from(
        "## Scheduled Tasks\n\
         These tasks are already registered with this session and continue independently; do not recreate them after compaction.",
    );
    for task in active {
        let prompt = task.prompt.split_whitespace().collect::<Vec<_>>().join(" ");
        let prompt = tools::util::truncate_str(&prompt, 256);
        let cadence = if task.recurring {
            format!("every {}s", task.interval_secs)
        } else {
            "one-shot".to_owned()
        };
        let _ = write!(
            body,
            "\n- \"{}\" ({cadence}, next {}): {}",
            task.id,
            task.next_fire_at().to_rfc3339(),
            prompt
        );
    }
    Some(body)
}

fn context_recall_hint(tool_name: &str) -> String {
    format!(
        "<context-recall>\n\
         Earlier context was unloaded by compaction, not deleted. If a missing detail is relevant, \
         call `{tool_name}` with a specific description of what you need to remember. A read-only \
         sideband will search this session's immutable branch history and return a concise recollection. \
         It does not restore or expand old messages into the active conversation.\n\
         </context-recall>"
    )
}
/// Trigger info for auto-compact decisions.
pub(crate) struct AutoCompactTriggerInfo {
    pub tokens_used: u64,
    pub context_window: u64,
    pub percentage: u8,
    /// Invocation site, recorded verbatim in `AutoCompactPruned` diagnostics
    /// (`pre_sampling` / `preflight_overflow` / `model_switch` /
    /// `context_window_exceeded` / `sampler_error_recovery`).
    pub source: &'static str,
}

/// Stable `data.compact_error` marker on the `acp::Error` returned when
/// compaction replaces the conversation but it still exceeds the context
/// window. The overflow branch matches it and fails the turn instead of
/// resampling in a loop (fail-safe).
pub(crate) const COMPACT_CONVERGED_OVER_WINDOW: &str = "compact_converged_over_window";
const COMPACT_IMAGE_INPUT_UNSUPPORTED: &str = "compact_image_input_unsupported";

/// Whether `err` carries the [`COMPACT_CONVERGED_OVER_WINDOW`] marker.
pub(crate) fn is_compact_converged_over_window(err: &acp::Error) -> bool {
    err.data
        .as_ref()
        .and_then(|d| d.get("compact_error"))
        .and_then(|v| v.as_str())
        == Some(COMPACT_CONVERGED_OVER_WINDOW)
}

fn is_compact_image_input_unsupported(err: &acp::Error) -> bool {
    err.data
        .as_ref()
        .and_then(|data| data.get("compact_error"))
        .and_then(|value| value.as_str())
        == Some(COMPACT_IMAGE_INPUT_UNSUPPORTED)
}

fn compact_image_input_unsupported_error(message: String) -> acp::Error {
    acp::Error::internal_error().data(serde_json::json!({
        "message": message,
        "compact_error": COMPACT_IMAGE_INPUT_UNSUPPORTED,
    }))
}

/// The convergence failure error: compaction succeeded (the history was
/// replaced) but the conversation still exceeds the window, so resampling
/// would overflow again. Carries the `error_kind` marker so turn-end
/// classifies it as explicit input-context exhaustion.
fn compact_converged_over_window_error(context_window: u64) -> acp::Error {
    acp::Error::internal_error().data(serde_json::json!({
        "message": format!(
            "compaction converged but the conversation still exceeds the \
             {context_window}-token context window; rewind to an earlier \
             point, switch to a model with a larger window, or start a new \
             session"
        ),
        "error_kind": ::hooks::event::StopFailureKind::ContextWindowExceeded.as_str(),
        "compact_error": COMPACT_CONVERGED_OVER_WINDOW,
    }))
}

/// A durable replacement is the compaction transaction's commit point. The
/// Timeline terminal therefore remains `Completed`, even when a required
/// post-commit projection prevents the current turn from continuing.
fn compaction_completed(result: &Result<(), acp::Error>, replacement_committed: bool) -> bool {
    replacement_committed || result.is_ok()
}
/// Why auto-compaction was suppressed after a deterministic failure.
/// [`SuppressReason::as_str`] is a stable local diagnostics value; do not rename
/// the strings without updating readers and tests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SuppressReason {
    ProviderLimit,
    Size,
    Auth,
    Schema,
    Other,
}
impl SuppressReason {
    fn as_str(self) -> &'static str {
        match self {
            SuppressReason::ProviderLimit => "provider_limit",
            SuppressReason::Size => "size",
            SuppressReason::Auth => "auth",
            SuppressReason::Schema => "schema",
            SuppressReason::Other => "other",
        }
    }
    /// Suppression scope for this reason:
    /// - `size | schema` → [`SUPPRESS_STICKY`]: cleared only on a context-budget change.
    /// - `provider_limit` → [`SUPPRESS_UNTIL_SUCCESS`]: wait for a model `200`.
    /// - `auth` → [`SUPPRESS_AUTH`]: clear on login/token refresh (not 200 — over-window deadlock).
    /// - `other` → [`SUPPRESS_TURN`]: optimistic per-turn retry.
    fn suppress_state(self) -> u8 {
        match self {
            SuppressReason::Size | SuppressReason::Schema => SUPPRESS_STICKY,
            SuppressReason::ProviderLimit => SUPPRESS_UNTIL_SUCCESS,
            SuppressReason::Auth => SUPPRESS_AUTH,
            SuppressReason::Other => SUPPRESS_TURN,
        }
    }
}
impl SessionActor {
    /// Path to the canonical `timeline.jsonl` ledger if it exists, else `None`.
    /// Hook envelopes use this to expose the durable audit source.
    ///
    /// Missing or non-regular paths are omitted rather than exposing a
    /// dangling or redirected audit pointer.
    pub(crate) fn get_transcript_path(&self) -> Option<String> {
        let path = self
            .session_dir
            .join(crate::session::storage::TIMELINE_FILE);
        if std::fs::symlink_metadata(&path)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        {
            Some(path.to_string_lossy().into_owned())
        } else {
            None
        }
    }
    /// Increment the compaction counter and launch a pre-compaction memory flush.
    ///
    /// The counter is incremented before the flush check so the once-per-cycle
    /// guard does not suppress the first eligible flush.
    async fn maybe_pre_compaction_flush(
        self: &Arc<Self>,
        total_tokens: u64,
        context_window: u64,
        trigger: &'static str,
    ) {
        let compaction_count = self
            .compaction
            .count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        if !self.compaction.memory_flush_enabled {
            return;
        }
        let last_flush = self
            .memory
            .last_flush_compaction
            .load(std::sync::atomic::Ordering::Relaxed);
        if crate::session::helpers::memory_flush::should_flush(
            total_tokens,
            context_window,
            self.compaction.threshold_percent.get(),
            &self.memory.flush_config,
            last_flush,
            compaction_count,
        ) {
            let Some(snapshot) = self.snapshot_memory_flush_state().await else {
                tracing::warn!("pre-compaction memory flush could not freeze Timeline input");
                return;
            };
            if let Some(activity) = self
                .session_activities
                .try_start("pre_compaction_memory_flush")
            {
                tokio::task::spawn_local({
                    let session = self.clone();
                    async move {
                        let _activity = activity;
                        if session.run_memory_flush(trigger, Some(snapshot)).await {
                            session
                                .memory
                                .last_flush_compaction
                                .store(compaction_count, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                });
            }
        }
    }
    /// Runs the compact operation over here which compresses the current conversation
    /// and helps with saving the context for the model
    #[tracing::instrument(
        name = "session.compact",
        skip_all,
        fields(
            session_id = %self.session_info.id.0,
            trigger = "manual",
            mode = tracing::field::Empty,
            detail = tracing::field::Empty,
            pre_tokens = tracing::field::Empty,
            post_tokens = tracing::field::Empty,
            success = tracing::field::Empty,
            error = tracing::field::Empty,
        )
    )]
    pub(crate) async fn run_compact(
        self: &Arc<Self>,
        user_context: Option<String>,
    ) -> Result<(), acp::Error> {
        let Some(_exclusive) = self
            .compaction
            .lease
            .try_enter(crate::session::compaction_config::CompactionOwner::Manual)
        else {
            return Err(acp::Error::internal_error().data("compaction already in progress"));
        };
        let (_cancel, _cancel_scope) = self.compaction.cancel.enter();
        let projected_images = self.project_images_for_known_text_model().await?;
        if projected_images.total_images() > 0 {
            tracing::info!(
                described_images = projected_images.described_images,
                "installed irreversible ImageShadows before manual compaction"
            );
        }
        let total_tokens = self.chat_state_handle.get_projected_tokens().await;
        tracing::Span::current().record("pre_tokens", total_tokens as i64);
        let sampling_config = self.chat_state_handle.get_sampling_config().await;
        let context_window = sampling_config
            .as_ref()
            .map(|c| c.context_window.get())
            .unwrap_or(DEFAULT_CONTEXT_WINDOW);
        self.maybe_pre_compaction_flush(total_tokens, context_window, "pre_compaction")
            .await;
        if let Err(e) = self
            .run_compact_inner(
                user_context,
                ::diagnostics::events::CompactionTrigger::Manual,
            )
            .await
        {
            let span = tracing::Span::current();
            span.record("success", false);
            span.record("error", e.to_string().as_str());
            return Err(e);
        }
        use crate::extensions::notification::SessionUpdate as GrowSessionUpdate;
        let tokens_after = self.chat_state_handle.get_projected_tokens().await;
        let span = tracing::Span::current();
        span.record("post_tokens", tokens_after as i64);
        span.record("success", true);
        self.send_grow_notification(GrowSessionUpdate::AutoCompactCompleted {
            tokens_before: total_tokens,
            tokens_after,
            elapsed_ms: None,
            summary_preview: None,
        })
        .await;
        Ok(())
    }
    async fn emit_compact_cancelled(&self, auto_trigger: bool) -> Result<(), acp::Error> {
        if auto_trigger {
            use crate::extensions::notification::SessionUpdate as GrowSessionUpdate;
            self.send_grow_notification(GrowSessionUpdate::AutoCompactCancelled {
                reason: crate::extensions::notification::AutoCompactCancelReason::UserCancelled,
            })
            .await;
        }
        Err(crate::session::helpers::session_compact::CompactFailure::cancelled_error())
    }
    /// Suppress AUTO compaction after a deterministic failure. Scope depends on
    /// the reason (see [`SuppressReason::suppress_state`]): size/schema sticky,
    /// provider limits until 200, auth until credentials recover, other clears next turn.
    /// Diagnostic + one notification per transition; manual `/compact` exempt.
    async fn suppress_auto_compaction(
        &self,
        reason: SuppressReason,
        estimated_tokens: u64,
        context_window: u64,
    ) {
        let new_state = reason.suppress_state();
        if self
            .compaction
            .auto_compact_suppressed
            .compare_exchange(
                SUPPRESS_NONE,
                new_state,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
        {
            tracing::warn!(
                suppress_reason = reason.as_str(),
                estimated_tokens,
                context_window,
                "auto-compaction suppressed after deterministic compaction failure"
            );
            ::diagnostics::session_ctx::log_event(::diagnostics::events::AutoCompactSuppressed {
                reason: reason.as_str(),
                estimated_tokens,
                context_window,
            });
            let message = match reason {
                SuppressReason::ProviderLimit => {
                    "the model provider rejected compaction because of an account or quota limit. Review the provider response and retry."
                }
                SuppressReason::Auth => {
                    "the model provider rejected its credentials. Check the provider authentication and retry."
                }
                SuppressReason::Size => "this conversation is too large to compact.",
                SuppressReason::Schema => "this conversation can't be summarized.",
                SuppressReason::Other => {
                    "it'll retry on the next turn, or start a new session using /new."
                }
            };
            self.send_grow_notification(
                crate::extensions::notification::SessionUpdate::AutoCompactFailed {
                    error: message.to_string(),
                },
            )
            .await;
        }
    }
    /// Map a deterministic failure's error text to a fixed, content-free
    /// [`SuppressReason`] (drives diagnostics + sticky-vs-per-turn scope).
    fn classify_suppress_reason(error_msg: &str) -> SuppressReason {
        let m = error_msg.to_ascii_lowercase();
        if m.contains("status 402")
            || m.contains("payment required")
            || m.contains("quota exhausted")
            || m.contains("quota limit reached")
            || m.contains("account limit reached")
        {
            SuppressReason::ProviderLimit
        } else if is_context_length_error(&m) {
            SuppressReason::Size
        } else if m.contains("status 401") || m.contains("unauthorized") {
            SuppressReason::Auth
        } else if m.contains("invalid_request_error") {
            SuppressReason::Schema
        } else {
            SuppressReason::Other
        }
    }
    /// ACP error payload string (plain string or `{message, ...}`).
    fn acp_error_message(err: &acp::Error) -> String {
        match err.data.as_ref() {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(obj) => obj
                .get("message")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| obj.to_string()),
            None => err.message.clone(),
        }
    }
    /// Credential/401 compact failure — abort so the caller can reread BYOK; don't sample oversized.
    pub(crate) fn is_auth_compact_error(err: &acp::Error) -> bool {
        matches!(
            Self::classify_suppress_reason(&Self::acp_error_message(err)),
            SuppressReason::Auth
        )
    }
    /// Terminal auth compact failure. Grow is BYOK-only, so every credential
    /// failure belongs to the selected model or its credential helper.
    pub(crate) async fn surface_compact_auth_failure(&self, err: acp::Error) -> acp::Error {
        use crate::extensions::notification::SessionUpdate as GrowSessionUpdate;
        let detailed = Self::acp_error_message(&err);
        let model_id = self.current_catalog_model_id();
        let auth_provider = self.model_auth_provider(&model_id);
        let message = if let Some(provider) = auth_provider.as_ref() {
            format!(
                "{detailed}\n\nThe credential for model `{model_id}` was rejected. Check [auth_provider.{}] and the debug log, then retry.",
                provider.name
            )
        } else {
            format!(
                "{detailed}\n\nThe configured BYOK credential for model `{model_id}` was rejected. Check its provider api_key/env_key and endpoint in ~/.grow/config.toml, then retry."
            )
        };
        let error_type = "provider_credentials";
        tracing::warn!(
            session_id = %self.session_info.id.0,
            error = %message,
            "auto-compact auth failure: aborting turn"
        );
        ::diagnostics::unified_log::warn(
            "auto-compact auth failure: aborting turn",
            Some(self.session_info.id.0.as_ref()),
            Some(serde_json::json!({
                "error_type": error_type,
                "model": model_id,
                "message": crate::util::truncate(&message, 300),
            })),
        );
        self.send_grow_notification(GrowSessionUpdate::RetryState(
            crate::extensions::notification::RetryState::Failed {
                error_type: error_type.to_string(),
                message: message.clone(),
            },
        ))
        .await;
        let data = crate::sampling::error::terminal_error_data(
            message,
            Some(401),
            sampler::SamplingErrorKind::Auth.as_str(),
        );
        acp::Error::internal_error().data(data)
    }
    /// Clear [`SUPPRESS_AUTH`] after a credential update (provider-limit
    /// suppression waits for a successful response).
    pub(crate) fn clear_auth_compact_suppression(&self) {
        let _ = self.compaction.auto_compact_suppressed.compare_exchange(
            SUPPRESS_AUTH,
            SUPPRESS_NONE,
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    /// Provider-limit or auth suppress — a model switch cannot clear these.
    fn is_account_state_suppressed(&self) -> bool {
        matches!(
            self.compaction
                .auto_compact_suppressed
                .load(std::sync::atomic::Ordering::Relaxed),
            SUPPRESS_UNTIL_SUCCESS | SUPPRESS_AUTH
        )
    }
    /// Inner implementation of one causally recorded compaction transaction.
    #[tracing::instrument(
        name = "session.compact_inner",
        skip_all,
        fields(
            session_id = %self.session_info.id.0,
            compaction_tokens_before = tracing::field::Empty,
            compaction_tokens_after = tracing::field::Empty,
            compaction_summary_chars = tracing::field::Empty,
            compaction_degenerate_rejections = tracing::field::Empty,
            compaction_input_overflow_rejections = tracing::field::Empty,
            compaction_deterministic_rejections = tracing::field::Empty,
            compaction_transient_rejections = tracing::field::Empty,
            compaction_attempts = tracing::field::Empty,
            compaction_trigger = tracing::field::Empty,
            compaction_trigger_pct = tracing::field::Empty,
            compaction_threshold_pct = tracing::field::Empty,
            compaction_outcome = tracing::field::Empty,
            compaction_stop_reason = tracing::field::Empty,
            compaction_ttft_ms = tracing::field::Empty,
            compaction_stream_ms = tracing::field::Empty,
            compaction_delta_count = tracing::field::Empty,
            compaction_itl_max_ms = tracing::field::Empty,
        )
    )]
    async fn run_compact_inner(
        &self,
        user_context: Option<String>,
        trigger: ::diagnostics::events::CompactionTrigger,
    ) -> Result<(), acp::Error> {
        let mut image_recovery_attempted = false;
        loop {
            let result = self
                .run_compact_transaction(user_context.clone(), trigger)
                .await;
            if !image_recovery_attempted
                && result
                    .as_ref()
                    .is_err_and(is_compact_image_input_unsupported)
            {
                let key = self.current_model_image_input_key().await.ok_or_else(|| {
                    acp::Error::internal_error()
                        .data("compaction image rejection had no model capability identity")
                })?;
                self.record_unsupported_model_image_input(key.clone())
                    .await
                    .map_err(|error| {
                        acp::Error::internal_error().data(format!(
                            "failed to persist text-only model capability during compaction: {error}"
                        ))
                    })?;
                let projected = self
                    .project_conversation_images_for_text_model(&key)
                    .await
                    .map_err(|error| {
                        acp::Error::internal_error().data(format!(
                            "failed to persist text-only image projection during compaction: {error}"
                        ))
                    })?;
                if projected.total_images() == 0 {
                    return result;
                }
                tracing::warn!(
                    model = key.model(),
                    described_images = projected.described_images,
                    "compaction model rejected images; installed irreversible ImageShadows and retrying from the new Surface"
                );
                image_recovery_attempted = true;
                continue;
            }
            return result;
        }
    }

    /// One durable compaction transaction. Capability recovery starts a fresh
    /// transaction because ImageShadow installation changes the authoritative
    /// Surface and therefore invalidates the first transaction's frozen input.
    async fn run_compact_transaction(
        &self,
        user_context: Option<String>,
        trigger: ::diagnostics::events::CompactionTrigger,
    ) -> Result<(), acp::Error> {
        let id = uuid::Uuid::now_v7().to_string();
        let source_items = self.chat_state_handle.get_conversation_len().await;
        let prompt_index = self.chat_state_handle.get_prompt_index().await;
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
                chat_state::CompactionEvent::Started {
                    id: id.clone(),
                    source_items,
                    prompt_index,
                },
            ))
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "compaction start was not durably recorded: {error}"
                ))
            })?;
        let started = std::time::Instant::now();
        let mut replacement_committed = false;
        let result = self
            .run_compact_attempt(&id, user_context, trigger, &mut replacement_committed)
            .await;
        let terminal = if compaction_completed(&result, replacement_committed) {
            chat_state::CompactionEvent::Completed {
                id,
                source_items,
                result_items: self.chat_state_handle.get_conversation_len().await,
                duration_ms: started.elapsed().as_millis() as u64,
            }
        } else {
            let error = result
                .as_ref()
                .expect_err("non-committed compaction must fail");
            chat_state::CompactionEvent::Failed {
                id,
                duration_ms: started.elapsed().as_millis() as u64,
                error: crate::util::truncate(&error.to_string(), 500).to_string(),
            }
        };
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(terminal))
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "compaction terminal was not durably recorded: {error}"
                ))
            })?;
        // Completed describes the committed replacement transaction. A
        // required post-commit repair may still fail closed for this turn;
        // swallowing that error would allow the next Step to sample without
        // its active Agent/Goal/Behavior authority.
        result
    }

    async fn run_compact_attempt(
        &self,
        transaction_id: &str,
        user_context: Option<String>,
        trigger: ::diagnostics::events::CompactionTrigger,
        replacement_committed: &mut bool,
    ) -> Result<(), acp::Error> {
        let (cancel, _cancel_scope) = self.compaction.cancel.enter();
        let tokens_before = self.chat_state_handle.get_projected_tokens().await;
        tracing::Span::current().record("compaction_tokens_before", tokens_before as i64);
        self.signals_handle().record_compaction(tokens_before);
        let trigger_str = match trigger {
            ::diagnostics::events::CompactionTrigger::Manual => "manual",
            ::diagnostics::events::CompactionTrigger::Auto => "auto",
        };
        let sampling_config = self.chat_state_handle.get_sampling_config().await;
        let context_window = sampling_config
            .as_ref()
            .map(|c| c.context_window.get())
            .unwrap_or(DEFAULT_CONTEXT_WINDOW);
        {
            let span = tracing::Span::current();
            let trigger_pct = if context_window == 0 {
                0
            } else {
                ((tokens_before as f64 / context_window as f64) * 100.0).round() as i64
            };
            span.record("compaction_trigger_pct", trigger_pct);
            span.record(
                "compaction_threshold_pct",
                self.compaction.threshold_percent.get() as i64,
            );
            span.record("compaction_trigger", trigger_str);
        }
        let summary_strips_reasoning = sampling_config
            .as_ref()
            .map(|c| c.api_backend == ApiBackend::Messages)
            .unwrap_or(false);
        let model_id = self.current_catalog_model_id();
        let compaction = ::diagnostics::events::CompactionScope::begin(
            trigger,
            tokens_before,
            context_window,
            model_id.clone(),
            user_context.is_some(),
        );
        let compact_source = trigger_str;
        self.dispatch_hook(
            ::hooks::event::HookEventName::PreCompact,
            ::hooks::event::HookPayload::PreCompact {
                source: compact_source.into(),
            },
            None,
            None,
        )
        .await;
        let max_retries = 3u32;
        let retry_delay_secs = 3u64;
        let Some(materialized) = self
            .chat_state_handle
            .materialize_timeline(self.session_info.id.to_string())
            .await
        else {
            return Err(acp::Error::internal_error()
                .data("Compaction failed: chat-state actor is unavailable"));
        };
        let source_surface_revision = materialized.surface_revision;
        let compaction_input_ref = materialized.input_ref;
        let active_goal = self.active_goal_directive_tag();
        let full_conversation = sampling_types::project_conversation_for_goal_scope(
            materialized.surface,
            active_goal.as_ref(),
        );
        let system_message = full_conversation
            .iter()
            .find(|item| matches!(item, ConversationItem::System(_)))
            .cloned();
        let source_surface = full_conversation.clone();
        let conv_len = source_surface.len();
        let retain_tokens = context_window.saturating_mul(COMPACTION_RETAIN_PERCENT) / 100;
        let range_plan = plan_compaction_range(
            &source_surface,
            &materialized.surface_ids,
            retain_tokens,
            MIN_COMPACTION_SOURCE_TOKENS,
        )
        .ok_or_else(|| {
            acp::Error::internal_error().data(
                "Compaction skipped: no closed Surface range is large enough to summarize while preserving the recent verbatim tail",
            )
        })?;
        let shadowed_control_contexts = materialized
            .active_control_contexts
            .iter()
            .filter_map(|(layer, context)| {
                range_plan
                    .target
                    .shadowed
                    .contains(&context.surface_id)
                    .then_some((*layer, context.item.clone()))
            })
            .collect::<Vec<_>>();
        let context_recall_tool_name = {
            let agent_ref = self.agent.borrow();
            agent_ref
                .tool_bridge()
                .render_prompt(
                    "${{ tools.by_kind.context_recall }}",
                    &serde_json::json!({}),
                )
                .await
                .filter(|name| !name.is_empty() && !name.contains("by_kind"))
        };
        let target_source = crate::session::actor::context_recall::strip_context_recall_derivatives(
            source_surface[range_plan.start_index..=range_plan.end_index].to_vec(),
            None,
            context_recall_tool_name.as_deref(),
        );
        const SUMMARY_BUDGET_RESERVE_TOKENS: u64 = 32_768;
        let verbatim_input_enabled = self.compaction.verbatim_input;
        let mut summary_source = vec![system_message.clone().ok_or_else(|| {
            acp::Error::internal_error()
                .data("Compaction failed: no system message in conversation history")
        })?];
        summary_source.extend(target_source);
        let simplified_messages = if verbatim_input_enabled {
            chat_state::compaction_utils::prepare_conversation_for_verbatim_summarization(
                summary_source.clone(),
                summary_strips_reasoning,
            )
        } else {
            chat_state::compaction_utils::prepare_conversation_for_summarization(
                summary_source.clone(),
            )
        };
        if conv_len == 0 {
            tracing::error!(
                session_id = %self.session_info.id.0,
                "Compaction failed: conversation is empty (ChatStateActor may have died)"
            );
            return Err(
                acp::Error::internal_error().data("Compaction failed: conversation is empty")
            );
        }
        if simplified_messages.is_empty() {
            tracing::error!(
                session_id = %self.session_info.id.0,
                conversation_len = conv_len,
                "Compaction failed: simplified conversation is empty"
            );
            return Err(acp::Error::internal_error()
                .data("Compaction failed: simplified conversation is empty"));
        }
        if !simplified_messages
            .iter()
            .any(|msg| matches!(msg, ConversationItem::System(_)))
        {
            tracing::error!(
                session_id = %self.session_info.id.0,
                conversation_len = conv_len,
                simplified_len = simplified_messages.len(),
                "Compaction failed: no system message in simplified conversation"
            );
            return Err(acp::Error::internal_error()
                .data("Compaction failed: no system message in simplified conversation"));
        }
        let sampling_config = self.reconstruct_full_config().await;
        let sampling_client = self.prepare_chat_completion(false).await?;
        tracing::info!(
            "Running compact with model '{}' (user model: '{}')",
            &sampling_config.model,
            &sampling_config.model
        );
        let sideband_prompt =
            build_compaction_request_surface(simplified_messages.clone(), user_context.as_deref())
                .last()
                .map(|item| serde_json::to_string(item).unwrap_or_else(|_| item.text_content()))
                .unwrap_or_else(|| "summarize the referenced Timeline range".into());
        let compaction_sideband = std::sync::Arc::new(tokio::sync::Mutex::new(
            self.begin_sideband(
                chat_state::SidebandPurpose::CompactionSummary,
                sideband_prompt,
                crate::session::actor::sideband::SidebandSource::Frozen(vec![
                    compaction_input_ref.clone(),
                ]),
                chat_state::SidebandBudgetPolicy {
                    max_attempts: max_retries.saturating_mul(3),
                    max_input_tokens_per_attempt: context_window,
                    max_output_tokens_per_attempt: None,
                },
                chat_state::SidebandRoute {
                    model: sampling_config.model.clone(),
                    backend: sampling_client.api_backend(),
                },
                None,
            )
            .await
            .map_err(|error| {
                acp::Error::internal_error()
                    .data(format!("compaction sideband could not start: {error}"))
            })?,
        ));
        let sideband_feedback = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let mut last_error: Option<acp::Error> = None;
        let mut last_failure_outcome = CompactionOutcome::Failed;
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum InputStage {
            Verbatim,
            VerbatimFitted,
            Simplified,
        }
        impl InputStage {
            fn as_str(self) -> &'static str {
                match self {
                    Self::Verbatim => "verbatim",
                    Self::VerbatimFitted => "verbatim_fitted",
                    Self::Simplified => "simplified",
                }
            }
        }
        let mut input_stage = if verbatim_input_enabled {
            InputStage::Verbatim
        } else {
            InputStage::Simplified
        };
        let estimated_input_tokens = chat_state::estimate_conversation_tokens(&simplified_messages);
        let auto_trigger = matches!(trigger, ::diagnostics::events::CompactionTrigger::Auto);
        let wall_clock_budget_secs = self.compaction.wall_clock_budget_secs;
        let sampler = crate::session::helpers::summary_compaction::ShellCompactionSampler::new(
            user_context.clone(),
            sampling_client,
            sampling_config.clone(),
            self.inference_idle_timeout.get(),
            wall_clock_budget_secs,
            cancel.clone(),
            compaction_sideband.clone(),
            sideband_feedback.clone(),
        );
        let observer = crate::session::helpers::summary_compaction::ShellSummaryObserver::new(
            trigger,
            context_window,
            compaction.compaction_id.clone(),
            self.session_info.id.0.to_string(),
            estimated_input_tokens,
            retry_delay_secs,
            sideband_feedback,
        );
        let summary_config = compaction::SummaryConfig {
            max_attempts: max_retries,
            retry_delay_secs,
            sampling_timeout_secs: 0,
        };
        let mut request_turns = simplified_messages.clone();
        let mut input_overflow_rejections: u32 = 0;
        let mut compact_summary: Option<String> = None;
        while compact_summary.is_none() {
            let summary_result = compaction::generate_summary(
                &sampler,
                &request_turns,
                user_context.as_deref(),
                &summary_config,
                &observer,
            )
            .await;
            match summary_result {
                Ok(summary) => {
                    compact_summary = Some(summary.summary);
                    break;
                }
                Err(compaction::SummaryError::NothingToCompact) => {
                    last_error = Some(
                        acp::Error::internal_error().data("compact failed: nothing to compact"),
                    );
                    break;
                }
                Err(compaction::SummaryError::EmptyResponse) => {
                    last_failure_outcome = if observer.degenerate_seen() {
                        CompactionOutcome::Degenerate
                    } else {
                        CompactionOutcome::Transient
                    };
                    last_error = Some(acp::Error::internal_error().data(
                        observer.last_error_message().unwrap_or_else(|| {
                            "compact failed: model returned empty response".to_string()
                        }),
                    ));
                    break;
                }
                Err(compaction::SummaryError::Sampler {
                    message,
                    deterministic,
                    context_overflow,
                }) => {
                    if cancel.is_cancelled()
                        || message.contains(
                            crate::session::helpers::session_compact::COMPACT_CANCELLED_MSG,
                        )
                    {
                        compaction_sideband
                            .lock()
                            .await
                            .fail(
                                chat_state::SidebandOutcome::Cancelled,
                                "compaction was cancelled",
                            )
                            .await
                            .map_err(|error| {
                                acp::Error::internal_error().data(format!(
                                    "compaction cancellation sideband terminal could not commit: {error}"
                                ))
                            })?;
                        return self.emit_compact_cancelled(auto_trigger).await;
                    }
                    if sampler.take_image_input_unsupported() {
                        last_failure_outcome = CompactionOutcome::Deterministic;
                        last_error = Some(compact_image_input_unsupported_error(message));
                        break;
                    }
                    if context_overflow {
                        let next_stage = match input_stage {
                            InputStage::Verbatim => Some(InputStage::VerbatimFitted),
                            InputStage::VerbatimFitted => Some(InputStage::Simplified),
                            InputStage::Simplified => None,
                        };
                        if let Some(stage) = next_stage {
                            input_overflow_rejections += 1;
                            ::diagnostics::session_ctx::log_event(
                                ::diagnostics::events::CompactionRetryDegraded {
                                    trigger,
                                    reason: "input_overflow",
                                    from_stage: Some(input_stage.as_str()),
                                    to_stage: Some(stage.as_str()),
                                    summary_chars: None,
                                    attempt: observer.attempt_count(),
                                    context_window,
                                    compaction_id: compaction.compaction_id.clone(),
                                },
                            );
                            tracing::warn!(
                                session_id = %self.session_info.id.0,
                                ?stage,
                                error = %message,
                                "Compaction input overflowed deterministically; stepping down the input ladder to avoid an incompactable state"
                            );
                            request_turns = match stage {
                                InputStage::VerbatimFitted => {
                                    let budget = context_window
                                        .saturating_sub(SUMMARY_BUDGET_RESERVE_TOKENS);
                                    let verbatim = chat_state::compaction_utils::prepare_conversation_for_verbatim_summarization(
                                        summary_source.clone(),
                                        summary_strips_reasoning,
                                    );
                                    chat_state::compaction_utils::fit_conversation_to_budget(
                                        verbatim, budget,
                                    )
                                }
                                InputStage::Simplified => {
                                    let simplified_budget = context_window.saturating_mul(7) / 10;
                                    chat_state::compaction_utils::fit_conversation_to_budget(
                                        chat_state::compaction_utils::prepare_conversation_for_summarization(
                                            summary_source.clone(),
                                        ),
                                        simplified_budget,
                                    )
                                }
                                InputStage::Verbatim => {
                                    unreachable!("ladder only steps forward")
                                }
                            };
                            input_stage = stage;
                            continue;
                        }
                        last_failure_outcome = CompactionOutcome::Deterministic;
                        if auto_trigger {
                            self.suppress_auto_compaction(
                                SuppressReason::Size,
                                estimated_input_tokens,
                                context_window,
                            )
                            .await;
                        }
                        last_error = Some(acp::Error::internal_error().data(message));
                        break;
                    }
                    if deterministic {
                        last_failure_outcome = CompactionOutcome::Deterministic;
                        if auto_trigger {
                            let reason = Self::classify_suppress_reason(&message);
                            self.suppress_auto_compaction(
                                reason,
                                estimated_input_tokens,
                                context_window,
                            )
                            .await;
                        }
                        last_error = Some(acp::Error::internal_error().data(message));
                        break;
                    }
                    last_failure_outcome = CompactionOutcome::Transient;
                    last_error = Some(acp::Error::internal_error().data(message));
                    break;
                }
            }
        }
        let diagnostics = observer.into_diagnostics();
        let compact_output = match compact_summary {
            Some(_) => sampler
                .take_last_success()
                .expect("a successful range-summary sample stashes its CompactOutput"),
            None => {
                let span = tracing::Span::current();
                span.record("compaction_attempts", diagnostics.attempts as i64);
                span.record(
                    "compaction_degenerate_rejections",
                    diagnostics.degenerate_rejections as i64,
                );
                span.record(
                    "compaction_input_overflow_rejections",
                    input_overflow_rejections as i64,
                );
                span.record(
                    "compaction_deterministic_rejections",
                    diagnostics.deterministic_rejections as i64,
                );
                span.record(
                    "compaction_transient_rejections",
                    diagnostics.transient_rejections as i64,
                );
                span.record("compaction_outcome", last_failure_outcome.as_str());
                let error = last_error.unwrap_or_else(|| {
                    acp::Error::internal_error().data("compaction failed: unknown error")
                });
                compaction_sideband
                    .lock()
                    .await
                    .fail(
                        chat_state::SidebandOutcome::Failed,
                        error
                            .data
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "compaction failed".into()),
                    )
                    .await
                    .map_err(|record_error| {
                        acp::Error::internal_error().data(format!(
                            "compaction failed and its sideband terminal could not commit: {record_error}"
                        ))
                    })?;
                return Err(error);
            }
        };
        let sideband_result_ref = compaction_sideband
            .lock()
            .await
            .complete(
                compact_output.content.clone(),
                None,
                compact_output.usage.clone(),
                compact_output
                    .stop_reason
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
                Vec::new(),
            )
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "compaction sideband result could not commit: {error}"
                ))
            })?;
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Compaction(
                chat_state::CompactionEvent::Summary {
                    id: transaction_id.to_owned(),
                    input_ref: compaction_input_ref,
                    result_ref: sideband_result_ref,
                    target: range_plan.target.clone(),
                    source_tokens: range_plan.source_tokens,
                    summary_chars: compact_output.content.chars().count(),
                },
            ))
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "compaction summary link was not durably recorded: {error}"
                ))
            })?;
        let generate_session_compact = compact_output.content.clone();
        let (discovered_agents_md, all_skills_for_compaction, _agent_edited_paths, state_context) = {
            let agents_md: Vec<std::path::PathBuf> = self
                .agent
                .borrow()
                .tool_bridge()
                .agents_md_reminded_paths()
                .await
                .into_iter()
                .collect();
            let skills = self.slash_skills_for_resolve().await;
            let edited_paths = self.chat_state_handle.get_agent_edited_paths().await;
            let ctx = {
                let bridge_tasks = self
                    .agent
                    .borrow()
                    .tool_bridge()
                    .list_background_tasks()
                    .await;
                let pending_tasks: Vec<_> =
                    background_tasks_owned_by(bridge_tasks, self.session_id_string().as_str())
                        .into_iter()
                        .filter(|t| !t.completed)
                        .collect();
                let (execute_tool_name, monitor_tool_name) = if pending_tasks.is_empty() {
                    (None, None)
                } else {
                    let agent_ref = self.agent.borrow();
                    let bridge = agent_ref.tool_bridge();
                    let empty = serde_json::json!({});
                    let execute = bridge
                        .render_prompt("${{ tools.by_kind.execute }}", &empty)
                        .await
                        .filter(|s| !s.is_empty() && !s.contains("by_kind"));
                    let monitor = bridge
                        .render_prompt("${{ tools.by_kind.monitor }}", &empty)
                        .await
                        .filter(|s| !s.is_empty() && !s.contains("by_kind"));
                    (execute, monitor)
                };
                let running_tasks: Vec<_> = pending_tasks
                    .into_iter()
                    .map(|t| {
                        let tool_name = match t.kind {
                            tools::computer::types::TaskKind::Monitor => monitor_tool_name.clone(),
                            tools::computer::types::TaskKind::Bash => execute_tool_name.clone(),
                        };
                        CompactionStateContext::task_summary(
                            t.task_id, t.command, "running", tool_name,
                        )
                    })
                    .collect();
                let running_subagents = if let Some(ref event_tx) =
                    self.tool_context.subagent_event_tx
                {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    use tools::implementations::grow_build::task::types::{
                        SubagentEvent, SubagentListActiveRequest,
                    };
                    let _ = event_tx.send(SubagentEvent::ListActive(SubagentListActiveRequest {
                        parent_session_id: self.session_id_string(),
                        respond_to: tx,
                    }));
                    rx.await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| {
                            crate::session::helpers::compaction_context::RunningSubagentSummary {
                                subagent_id: s.subagent_id,
                                subagent_type: s.subagent_type,
                                description: s.description,
                                elapsed_ms: s.elapsed_ms,
                            }
                        })
                        .collect()
                } else {
                    vec![]
                };
                let connected_mcp_servers = {
                    use crate::session::helpers::compaction_context::CompactionServerSummary;
                    use tools::implementations::search_tool::{
                        sanitize_description, truncate_description,
                    };
                    self.connected_server_summaries()
                        .into_iter()
                        .map(|s| {
                            let desc = s
                                .description
                                .map(|d| truncate_description(&sanitize_description(&d)))
                                .filter(|d| !d.is_empty());
                            CompactionServerSummary {
                                name: s.name,
                                tool_count: s.tool_count,
                                description: desc,
                            }
                        })
                        .collect()
                };
                let todos = {
                    use crate::session::helpers::compaction_context::{
                        TodoSummary, TodoSummaryStatus,
                    };
                    use crate::tools::todo::{TodoState, TodoStatus};
                    use tools::types::resources::State;
                    let bridge = self.agent.borrow().tool_bridge().clone();
                    bridge
                        .read_resource::<State<TodoState>>()
                        .await
                        .map(|s| {
                            s.0.todo_items_with_ids()
                                .map(|(id, item)| TodoSummary {
                                    id: id.clone(),
                                    content: item.content.clone(),
                                    status: match item.status {
                                        TodoStatus::Pending => TodoSummaryStatus::Pending,
                                        TodoStatus::InProgress => TodoSummaryStatus::InProgress,
                                        TodoStatus::Completed => TodoSummaryStatus::Completed,
                                        TodoStatus::Cancelled => TodoSummaryStatus::Cancelled,
                                    },
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                };
                CompactionStateContext::build(
                    &source_surface,
                    CompactionInputs {
                        running_tasks,
                        running_subagents,
                        agent_edited_paths: edited_paths.clone(),
                        connected_mcp_servers,
                        todos,
                        ..Default::default()
                    },
                )
                .await
            };
            (agents_md, skills, edited_paths, ctx)
        };
        use crate::session::helpers::compaction_context::SubagentToolNames;
        let subagent_tool_names: Option<SubagentToolNames> =
            if state_context.running_subagents.is_empty() {
                None
            } else {
                let agent_ref = self.agent.borrow();
                let bridge = agent_ref.tool_bridge();
                let empty = serde_json::json!({});
                let poll_name = bridge
                    .render_prompt("${{ tools.by_kind.background_task_action }}", &empty)
                    .await
                    .filter(|s| !s.is_empty() && !s.contains("by_kind"));
                let cancel_name = bridge
                    .render_prompt("${{ tools.by_kind.kill_task_action }}", &empty)
                    .await
                    .filter(|s| !s.is_empty() && !s.contains("by_kind"));
                match (poll_name, cancel_name) {
                    (Some(poll), Some(cancel)) => Some(SubagentToolNames { poll, cancel }),
                    (poll, cancel) => {
                        tracing::warn!(
                            session_id = %self.session_info.id.0,
                            poll_resolved = poll.is_some(),
                            cancel_resolved = cancel.is_some(),
                            "could not resolve subagent tool names, \
                             omitting subagent reminder from compacted conversation"
                        );
                        None
                    }
                }
            };
        use crate::session::helpers::compaction_context::McpToolNames;
        let mcp_tool_names: Option<McpToolNames> = if state_context.connected_mcp_servers.is_empty()
        {
            None
        } else {
            let agent_ref = self.agent.borrow();
            let bridge = agent_ref.tool_bridge();
            let empty = serde_json::json!({});
            let search_name = bridge
                .render_prompt("${{ tools.by_kind.search_tool }}", &empty)
                .await
                .filter(|s| !s.is_empty() && !s.contains("by_kind"));
            let call_name = bridge
                .render_prompt("${{ tools.by_kind.use_tool }}", &empty)
                .await
                .filter(|s| !s.is_empty() && !s.contains("by_kind"));
            match (search_name, call_name) {
                (Some(search), Some(call)) => Some(McpToolNames { search, call }),
                _ => None,
            }
        };
        let memory_backend_impl = {
            let g = self.memory.storage.borrow();
            g.as_ref()
                .zip(self.memory.backend_params.as_ref())
                .map(|(storage, params)| {
                    memory::MemoryBackendImpl::from_session_params(
                        storage.clone(),
                        &memory::MemoryBackendParams {
                            search_source: "compaction_recovery",
                            ..params.clone()
                        },
                    )
                })
        };
        let memory_ref: Option<&dyn tools::types::memory_backend::MemoryBackend> =
            memory_backend_impl
                .as_ref()
                .map(|b| b as &dyn tools::types::memory_backend::MemoryBackend);
        let mut system_reminder = to_system_reminder(
            &state_context,
            &discovered_agents_md,
            &all_skills_for_compaction,
            memory_ref,
            subagent_tool_names.as_ref(),
            mcp_tool_names.as_ref(),
        )
        .await;
        let reminder_wrapper = self.reminder_wrapper_tag();

        // Workflow guidance is turn-scoped rather than a Control projection.
        // The current turn's copy sits immediately before its causal prompt,
        // inside the prefix that compaction replaces. Reinstall the same
        // authoritative workspace/authoring contract in the replacement so a
        // later step in this turn cannot sample without Workflow semantics.
        if *self.turn_behavior.lock() == tool_types::BehaviorId::Workflow {
            let workflow_context = self.workflow_behavior_context()?;
            system_reminder = append_system_reminder_section(
                system_reminder,
                reminder_wrapper,
                &workflow_context,
            );
        }

        let workflow_runs = {
            let tracker = self.workflow_tracker().await;
            let snapshot = tracker.lock().live_status_snapshot();
            snapshot
        };
        if !workflow_runs.is_empty() {
            let section = format!(
                "## Active Workflow Runs\n{}",
                super::reminders::format_workflow_status_reminder(&workflow_runs)
            );
            system_reminder =
                append_system_reminder_section(system_reminder, reminder_wrapper, &section);
        }

        if !self.startup_hints.is_subagent {
            let scheduled_tasks = self
                .agent
                .borrow()
                .tool_bridge()
                .clone()
                .list_scheduled_tasks()
                .await;
            if let Some(section) =
                format_scheduled_tasks_compaction_reminder(&scheduled_tasks, chrono::Utc::now())
            {
                system_reminder =
                    append_system_reminder_section(system_reminder, reminder_wrapper, &section);
            }
        }

        if let Some(ref recovery_backend) = memory_backend_impl {
            let n = recovery_backend
                .search_counter
                .load(std::sync::atomic::Ordering::Relaxed);
            if n > 0 {
                self.memory
                    .compaction_recovery_count
                    .fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                tracing::debug!(
                    target: ::diagnostics::memory_log::TARGET,
                    count = n,
                    "MEMORY_COMPACTION_RECOVERY: {} search(es) performed",
                    n,
                );
            }
        }
        let mut replacement_content =
            compaction::format_compact_summary_content(&generate_session_compact);
        if let Some(tool_name) = context_recall_tool_name {
            replacement_content.push_str("\n\n");
            replacement_content.push_str(&context_recall_hint(&tool_name));
        }
        if let Some(reminder) = system_reminder {
            replacement_content.push_str("\n\n");
            replacement_content.push_str(&reminder);
        }
        let replacement = vec![ConversationItem::user_meta(replacement_content)];
        if cancel.is_cancelled() {
            return self.emit_compact_cancelled(auto_trigger).await;
        }
        self.chat_state_handle
            .replace_compaction_range(range_plan.target, replacement, source_surface_revision)
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "compaction replacement was not durably recorded: {error}"
                ))
            })?;
        *replacement_committed = true;
        let control_reprojection_error = if let Err(error) = self
            .reproject_control_contexts_durably(shadowed_control_contexts)
            .await
        {
            // Replacement is already durable. Keep running the post-commit
            // reset/hook pipeline; startup and the next turn both repair
            // missing Control projections from the Timeline fact source.
            tracing::warn!(
                %error,
                "compaction Control context was not durably re-projected; scheduling an in-transaction repair"
            );
            Some(error)
        } else {
            None
        };
        let new_len = self.chat_state_handle.get_conversation_len().await;
        let post_replace_tokens = self.chat_state_handle.get_projected_tokens().await;
        self.compaction
            .auto_compact_suppressed
            .store(SUPPRESS_NONE, std::sync::atomic::Ordering::Relaxed);
        // Unified convergence check (all paths): if the compacted history
        // still exceeds the context window itself, the next sample would
        // overflow again. Fail-safe: sticky-suppress AUTO and report
        // `CompactConvergedOverWindow` so the overflow branch fails the turn
        // instead of resampling in a loop. The fork threshold check above is
        // untouched (it gates the *trigger* dimension, this gates the
        // *window* dimension).
        let converged_over_window = post_replace_tokens > context_window;
        if converged_over_window {
            self.compaction
                .auto_compact_suppressed
                .store(SUPPRESS_STICKY, std::sync::atomic::Ordering::Relaxed);
            tracing::warn!(
                session_id = %self.session_info.id.0,
                post_replace_tokens,
                context_window,
                "compaction converged but the conversation still exceeds the context window; suppressing AUTO and failing the turn (no re-loop)"
            );
            tracing::Span::current().record("detail", "converged_over_window");
        }
        self.last_idle_flush_conversation_len
            .store(new_len, std::sync::atomic::Ordering::Relaxed);
        self.memory
            .context_injected
            .store(false, std::sync::atomic::Ordering::Relaxed);
        if self.memory.is_enabled() {
            tracing::info!(target: ::diagnostics::memory_log::TARGET, "MEMORY_COMPACT: post-compaction reset, next turn re-checks injection (search only if no block persisted)");
        }
        self.agent
            .borrow()
            .tool_bridge()
            .on_agents_md_compaction()
            .await;
        self.agent
            .borrow()
            .tool_bridge()
            .on_skill_discovery_compaction()
            .await;
        self.persist_announcement_state().await;
        self.dispatch_hook(
            ::hooks::event::HookEventName::PostCompact,
            ::hooks::event::HookPayload::PostCompact {
                source: compact_source.into(),
            },
            None,
            None,
        )
        .await;
        let tokens_after = self.chat_state_handle.get_projected_tokens().await;
        {
            let span = tracing::Span::current();
            span.record("compaction_tokens_after", tokens_after as i64);
            span.record(
                "compaction_summary_chars",
                compact_output.content.chars().count() as i64,
            );
            span.record("compaction_attempts", diagnostics.attempts as i64);
            span.record(
                "compaction_degenerate_rejections",
                diagnostics.degenerate_rejections as i64,
            );
            span.record(
                "compaction_input_overflow_rejections",
                input_overflow_rejections as i64,
            );
            span.record(
                "compaction_deterministic_rejections",
                diagnostics.deterministic_rejections as i64,
            );
            span.record(
                "compaction_transient_rejections",
                diagnostics.transient_rejections as i64,
            );
            let stop_reason = compact_output.stop_reason.as_deref().unwrap_or("stop");
            span.record("compaction_stop_reason", stop_reason);
            let outcome = if compact_output.truncated {
                CompactionOutcome::Truncated
            } else {
                CompactionOutcome::Success
            };
            span.record("compaction_outcome", outcome.as_str());
            span.record("compaction_delta_count", compact_output.delta_count as i64);
            if let Some(ms) = compact_output.ttft_ms {
                span.record("compaction_ttft_ms", ms as i64);
            }
            if let Some(ms) = compact_output.stream_ms {
                span.record("compaction_stream_ms", ms as i64);
            }
            if let Some(ms) = compact_output.itl_max_ms {
                span.record("compaction_itl_max_ms", ms as i64);
            }
        }
        compaction.complete(tokens_after);
        if let Some(initial_error) = control_reprojection_error
            && let Err(repair_error) = self.repair_missing_control_contexts_durably().await
        {
            return Err(acp::Error::internal_error().data(format!(
                "compaction replacement committed, but active Control context could not be restored before the next Step: initial reprojection failed: {initial_error}; repair failed: {repair_error}"
            )));
        }
        if converged_over_window {
            // The conversation was replaced and every post-compaction side
            // effect above has run; only the *outcome* is reported as a
            // convergence failure so the caller fails the turn instead of
            // resampling (which would overflow again).
            return Err(compact_converged_over_window_error(context_window));
        }
        Ok(())
    }
    /// Check if auto-compact should be triggered based on context window usage.
    /// Returns Some(AutoCompactTriggerInfo) if threshold is reached, None otherwise.
    pub(crate) fn should_auto_compact(
        &self,
        projected_tokens: u64,
        context_window: std::num::NonZeroU64,
        source: &'static str,
    ) -> Option<AutoCompactTriggerInfo> {
        let cw = context_window.get();
        if token_estimation::exceeds_threshold(
            projected_tokens,
            cw,
            self.compaction.threshold_percent.get(),
        ) {
            let percentage = token_estimation::usage_percentage_u8(projected_tokens, cw);
            Some(AutoCompactTriggerInfo {
                tokens_used: projected_tokens,
                context_window: cw,
                percentage,
                source,
            })
        } else {
            None
        }
    }
    /// Resolve a provider-error recovery window. Explicit provider overflow
    /// evidence outranks the local estimate; otherwise a smaller provider
    /// metadata window may still prove the request is too large.
    pub(crate) async fn compaction_window_on_error(
        &self,
        err: &sampler::SamplingErrorInfo,
    ) -> Option<u64> {
        if self
            .compaction
            .auto_compact_suppressed
            .load(std::sync::atomic::Ordering::Relaxed)
            != SUPPRESS_NONE
        {
            return None;
        }
        let projected_tokens = self.chat_state_handle.get_projected_tokens().await;
        let configured_context_window = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .map(|config| config.context_window.get())
            .unwrap_or(0);
        sampling_error_compaction_window(err, projected_tokens, configured_context_window)
    }
    /// Pre-sampling compaction check against the canonical projected context
    /// pressure. Returns `None` when `is_flushing`.
    pub(crate) async fn check_auto_compact_needed(&self) -> Option<AutoCompactTriggerInfo> {
        if self
            .memory
            .is_flushing
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }
        let sampling_cfg = self.chat_state_handle.get_sampling_config().await;
        let context_window = sampling_cfg.as_ref().map(|c| c.context_window)?;
        let cw = context_window.get();
        let model = sampling_cfg
            .as_ref()
            .map(|c| c.model.clone())
            .unwrap_or_default();
        let projected_tokens = self.chat_state_handle.get_projected_tokens().await;
        self.signals_handle()
            .update_context_usage(projected_tokens, cw);
        if self
            .compaction
            .auto_compact_suppressed
            .load(std::sync::atomic::Ordering::Relaxed)
            != SUPPRESS_NONE
        {
            return None;
        }
        if self
            .compaction
            .force_compact
            .compare_exchange(
                true,
                false,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
        {
            let percentage = token_estimation::usage_percentage_u8(projected_tokens, cw);
            tracing::info!(
                "Forced auto-compact trigger (debug): model={model}, \
                 {percentage}% full ({projected_tokens}/{cw} tokens)",
            );
            return Some(AutoCompactTriggerInfo {
                tokens_used: projected_tokens,
                context_window: cw,
                percentage,
                source: "pre_sampling",
            });
        }
        if let Some(trigger_info) =
            self.should_auto_compact(projected_tokens, context_window, "pre_sampling")
        {
            tracing::info!(
                "Pre-sampling auto-compact trigger: model={model}, \
                 {}% full ({}/{} tokens)",
                trigger_info.percentage,
                trigger_info.tokens_used,
                trigger_info.context_window,
            );
            return Some(trigger_info);
        }
        None
    }
    /// Returns `Some` when tool call outputs have pushed projected context
    /// count past the context window, indicating pre-emptive compaction is needed.
    pub(crate) async fn check_preflight_overflow(&self) -> Option<AutoCompactTriggerInfo> {
        if self
            .compaction
            .auto_compact_suppressed
            .load(std::sync::atomic::Ordering::Relaxed)
            != SUPPRESS_NONE
        {
            return None;
        }
        let projected_tokens = self.chat_state_handle.get_projected_tokens().await;
        let cfg = self.chat_state_handle.get_sampling_config().await?;
        let cw = cfg.context_window.get();
        if projected_tokens <= cw {
            return None;
        }
        let overflow = projected_tokens.saturating_sub(cw);
        let percentage = token_estimation::usage_percentage_u8(projected_tokens, cw);
        tracing::warn!(
            projected_tokens,
            context_window = cw,
            overflow,
            model = %cfg.model,
            "CONTEXT_OVERFLOW_PREFLIGHT: estimated tokens exceed context window \
             after tool call outputs"
        );
        Some(AutoCompactTriggerInfo {
            tokens_used: projected_tokens,
            context_window: cw,
            percentage,
            source: "preflight_overflow",
        })
    }
    /// On model change: clear sticky/other suppress and compact if the window shrank.
    /// Leaves provider-limit/auth suppression (a switch can't fix those) and short-circuits.
    /// Auth compact failures abort the turn (same as pre-sampling/preflight).
    pub(crate) async fn maybe_compact_on_model_switch(self: &Arc<Self>) -> Result<(), acp::Error> {
        self.refresh_byok_credential().await;
        let Some(prev) = self.compaction.previous_model.take() else {
            return Ok(());
        };
        let Some(cfg) = self.chat_state_handle.get_sampling_config().await else {
            return Ok(());
        };
        if cfg.model == prev.model_slug {
            return Ok(());
        }
        if self.is_account_state_suppressed() {
            return Ok(());
        }
        self.compaction
            .auto_compact_suppressed
            .store(SUPPRESS_NONE, std::sync::atomic::Ordering::Relaxed);
        if prev.context_window <= cfg.context_window.get() {
            return Ok(());
        }
        let projected_tokens = self.chat_state_handle.get_projected_tokens().await;
        let Some(trigger_info) =
            self.should_auto_compact(projected_tokens, cfg.context_window, "model_switch")
        else {
            return Ok(());
        };
        tracing::info!(
            "Proactive model-switch compact: {} ({}) -> {} ({}), {}% full",
            prev.model_slug,
            prev.context_window,
            cfg.model,
            cfg.context_window.get(),
            trigger_info.percentage,
        );
        if let Err(e) = self.run_compact_only(trigger_info).await {
            tracing::error!(error = %e, "Model-switch compaction failed");
            if Self::is_auth_compact_error(&e) {
                return Err(self.surface_compact_auth_failure(e).await);
            }
        }
        Ok(())
    }
    /// Record the model governing the current step. A route change compares
    /// against this snapshot before the next sample.
    pub(crate) async fn record_turn_model(&self) {
        if let Some(cfg) = self.chat_state_handle.get_sampling_config().await {
            self.compaction.previous_model.set(Some(
                crate::session::compaction_config::PreviousModelInfo {
                    model_slug: cfg.model.clone(),
                    context_window: cfg.context_window.get(),
                },
            ));
        }
    }
    /// Pre-prune ladder: model-free tool-result pruning that runs inside
    /// `run_compact_only` before the summary LLM call. Returns `true` when
    /// pruning alone brought projected pressure back under the trigger
    /// threshold (the caller then skips `run_compact_inner`).
    ///
    /// Suppress gate: account-state suppression ([`SUPPRESS_UNTIL_SUCCESS`],
    /// [`SUPPRESS_AUTH`]) and per-turn suppression ([`SUPPRESS_TURN`]) block
    /// the ladder — account state is unrelated to model-free pruning, and
    /// per-turn failures self-heal at the next turn start. [`SUPPRESS_STICKY`]
    /// (deterministic size failures) is allowed through: pruning is exactly
    /// the model-free remedy, and a prune whose strict gate passes clears the
    /// sticky bit (the existing "context-budget change" clear condition).
    ///
    /// Pruning is an **optimization**, not a correctness requirement: every
    /// failure mode here fails open to the existing summary path and never
    /// touches the suppress state. The skip-summary gate is strict — it uses
    /// the same `exceeds_threshold` helper as `should_auto_compact`, so the
    /// summary is skipped only when the post-prune estimate is genuinely below
    /// the trigger threshold.
    ///
    /// Pruning and summary replacement share the actor's signed Surface-delta
    /// transaction, so this gate reads the exact post-transaction projection.
    pub(crate) async fn maybe_pre_prune(
        &self,
        trigger_info: &AutoCompactTriggerInfo,
    ) -> Result<bool, acp::Error> {
        if !self.compaction.pre_prune.get() {
            return Ok(false);
        }
        // Suppress gate: account-state suppression (provider quota / auth) is
        // unrelated to model-free pruning, and per-turn suppression self-heals
        // at the next turn start — both keep blocking. STICKY marks
        // deterministic size failures; pruning is exactly the model-free
        // remedy for those, so it is allowed through, and a prune whose strict
        // gate passes clears the sticky bit below (a context-budget change is
        // the existing STICKY clear condition). Unknown future suppression
        // classes fail closed.
        match self
            .compaction
            .auto_compact_suppressed
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            SUPPRESS_UNTIL_SUCCESS | SUPPRESS_AUTH | SUPPRESS_TURN => return Ok(false),
            SUPPRESS_NONE | SUPPRESS_STICKY => {}
            _ => return Ok(false),
        }
        let context_window = trigger_info.context_window;
        let threshold_percent = self.compaction.threshold_percent.get();
        // Default per-item budget: 5% of the context window, lower bound 1.
        let item_budget: u64 = self
            .compaction
            .pre_prune_token_budget
            .get()
            .unwrap_or_else(|| context_window.saturating_mul(5) / 100)
            .max(1);
        // The plan's target: the same absolute token count the trigger
        // threshold represents (`used * 100 >= cw * pct` ⇒ skip gating uses
        // `exceeds_threshold`, so `cw * pct / 100` is the matching absolute).
        let target = context_window.saturating_mul(threshold_percent as u64) / 100;
        let conversation = self.chat_state_handle.get_conversation().await;
        let plan = compaction::plan_tool_result_pruning(
            &conversation,
            &chat_state::actor::state::EstimatedItemTokenCounter,
            item_budget.min(u32::MAX as u64) as u32,
            target.min(u32::MAX as u64) as u32,
        );
        if plan.is_empty() {
            return Ok(false);
        }
        let prune_start = std::time::Instant::now();
        let report = match self.chat_state_handle.prune_tool_results(plan).await {
            Ok(report) => report,
            Err(e) => {
                // Fail-open: pruning is an optimization; continue with the
                // summary path (the conversation is simply not pruned).
                tracing::warn!(
                    target: super::SESSION_LOG,
                    session_id = %self.session_info.id.0,
                    error = %e,
                    "pre-prune failed; continuing with summary compaction"
                );
                return Ok(false);
            }
        };
        if report.pruned_count == 0 {
            return Ok(false);
        }
        let tokens_after = self.chat_state_handle.get_projected_tokens().await;
        if token_estimation::exceeds_threshold(tokens_after, context_window, threshold_percent) {
            // Still at/over the trigger threshold: continue with the summary
            // path. Its input is smaller now (the pruned content is persisted),
            // so the prune still helped even though the gate did not pass.
            tracing::info!(
                session_id = %self.session_info.id.0,
                pruned_count = report.pruned_count,
                tokens_before = report.tokens_before,
                tokens_after,
                context_window,
                threshold_percent,
                "pre-prune reduced tool-result size but the estimate is still over threshold; continuing with summary compaction"
            );
            return Ok(false);
        }
        let elapsed_ms = prune_start.elapsed().as_millis() as i64;
        tracing::info!(
            session_id = %self.session_info.id.0,
            pruned_count = report.pruned_count,
            tokens_before = report.tokens_before,
            tokens_after,
            context_window,
            threshold_percent,
            budget_tokens = item_budget,
            source = trigger_info.source,
            "pre-prune resolved auto-compact pressure; skipping summary compaction"
        );
        // Pruning resolved the pressure without a model call: the effective
        // context budget changed, which is the existing clear condition for
        // STICKY suppression (same rule as a successful compaction / rewind /
        // model switch). Only this success path touches the suppress state;
        // every gate-failure return above leaves it unchanged.
        self.compaction
            .auto_compact_suppressed
            .store(SUPPRESS_NONE, std::sync::atomic::Ordering::Relaxed);
        ::diagnostics::session_ctx::log_event(::diagnostics::events::AutoCompactPruned {
            tokens_before: report.tokens_before,
            tokens_after,
            pruned_count: report.pruned_count,
            threshold_percent,
            budget_tokens: item_budget,
            source: trigger_info.source,
        });
        // The auto-compact attempt completes here (Started was already sent by
        // `run_compact_only`); `summary_preview` carries a short explanation
        // instead of a summary snippet.
        self.send_grow_notification(
            crate::extensions::notification::SessionUpdate::AutoCompactCompleted {
                tokens_before: trigger_info.tokens_used,
                tokens_after,
                elapsed_ms: Some(elapsed_ms),
                summary_preview: Some(format!(
                    "pruned {} tool result{} ({} → {} tokens)",
                    report.pruned_count,
                    if report.pruned_count == 1 { "" } else { "s" },
                    report.tokens_before,
                    tokens_after,
                )),
            },
        )
        .await;
        Ok(true)
    }
    /// Compact without auto-continue. The outer turn loop rebuilds and retries.
    /// Emits diagnostics (`auto_compact_fired`) and UI notifications automatically.
    #[tracing::instrument(
        name = "session.compact",
        skip_all,
        fields(
            session_id = %self.session_info.id.0,
            trigger = "auto",
            mode = tracing::field::Empty,
            detail = tracing::field::Empty,
            pre_tokens = tracing::field::Empty,
            post_tokens = tracing::field::Empty,
            success = tracing::field::Empty,
            error = tracing::field::Empty,
        )
    )]
    pub(crate) async fn run_compact_only(
        self: &Arc<Self>,
        mut trigger_info: AutoCompactTriggerInfo,
    ) -> Result<(), acp::Error> {
        use crate::extensions::notification::SessionUpdate as GrowSessionUpdate;
        {
            let state = self.state.lock().await;
            if state.pending_manual_compact.is_some()
                || matches!(
                    state.foreground,
                    crate::session::actor::ForegroundState::Compaction
                )
            {
                tracing::debug!("auto compact skipped: manual compaction has priority");
                return Ok(());
            }
        }
        let Some(_exclusive) = self
            .compaction
            .lease
            .try_enter(crate::session::compaction_config::CompactionOwner::Auto)
        else {
            tracing::debug!("auto compact skipped: another compaction owns the lease");
            return Ok(());
        };
        let (_cancel, _cancel_scope) = self.compaction.cancel.enter();
        let projected_images = self.project_images_for_known_text_model().await?;
        if projected_images.total_images() > 0 {
            tracing::info!(
                described_images = projected_images.described_images,
                "installed irreversible ImageShadows before auto compaction"
            );
            let projected_tokens = self.chat_state_handle.get_projected_tokens().await;
            let Some(config) = self.chat_state_handle.get_sampling_config().await else {
                return Err(acp::Error::internal_error()
                    .data("compaction sampling configuration is unavailable"));
            };
            let Some(updated) = self.should_auto_compact(
                projected_tokens,
                config.context_window,
                trigger_info.source,
            ) else {
                tracing::info!(
                    projected_tokens,
                    context_window = config.context_window.get(),
                    "image projection resolved context pressure; skipping summary compaction"
                );
                return Ok(());
            };
            trigger_info = updated;
        }
        let tokens_before = self.chat_state_handle.get_projected_tokens().await;
        tracing::Span::current().record("pre_tokens", tokens_before as i64);
        ::diagnostics::session_ctx::log_event(::diagnostics::events::AutoCompactFired {
            tokens_before: trigger_info.tokens_used,
            percentage: trigger_info.percentage,
        });
        self.signals_handle()
            .record_compaction(trigger_info.tokens_used);
        self.send_grow_notification(GrowSessionUpdate::AutoCompactStarted {
            tokens_used: trigger_info.tokens_used,
            context_window: trigger_info.context_window,
            percentage: trigger_info.percentage,
            reason: format!("Context window {}% full", trigger_info.percentage),
        })
        .await;
        self.maybe_pre_compaction_flush(
            trigger_info.tokens_used,
            trigger_info.context_window,
            "pre_compact_on_error",
        )
        .await;
        match self.maybe_pre_prune(&trigger_info).await {
            Ok(true) => {
                // Pruning alone resolved the pressure: the summary call was
                // skipped. The completed notification was already sent by
                // `maybe_pre_prune`.
                let span = tracing::Span::current();
                span.record("mode", "prune");
                span.record("success", true);
                return Ok(());
            }
            Ok(false) => {}
            Err(e) => {
                // Fail-open: pre-prune is an optimization; never block the
                // summary path.
                tracing::warn!(
                    error = %e,
                    "pre-prune failed; continuing with summary compaction"
                );
            }
        }
        let compact_start = std::time::Instant::now();
        let result = self
            .run_compact_inner(None, ::diagnostics::events::CompactionTrigger::Auto)
            .await;
        let elapsed_ms = compact_start.elapsed().as_millis() as i64;
        match result {
            Ok(()) => {
                let tokens_after = self.chat_state_handle.get_projected_tokens().await;
                let span = tracing::Span::current();
                span.record("post_tokens", tokens_after as i64);
                span.record("success", true);
                self.send_grow_notification(GrowSessionUpdate::AutoCompactCompleted {
                    tokens_before: trigger_info.tokens_used,
                    tokens_after,
                    elapsed_ms: Some(elapsed_ms),
                    summary_preview: None,
                })
                .await;
                Ok(())
            }
            Err(e) => {
                let span = tracing::Span::current();
                span.record("success", false);
                span.record("error", e.to_string().as_str());
                let cancelled = self.compaction.cancel.is_cancelled()
                    || e.data.as_ref().and_then(|d| d.as_str()).is_some_and(|s| {
                        s.contains(crate::session::helpers::session_compact::COMPACT_CANCELLED_MSG)
                    });
                // Some failures happen before the summarizer/provider path
                // classifies them (for example, no eligible closed Surface
                // range). Route every auto-compaction failure through the
                // same policy so one impossible plan cannot retry at every
                // step of a long Goal turn.
                if !cancelled
                    && self
                        .compaction
                        .auto_compact_suppressed
                        .load(std::sync::atomic::Ordering::Relaxed)
                        == SUPPRESS_NONE
                {
                    let reason = Self::classify_suppress_reason(&Self::acp_error_message(&e));
                    self.suppress_auto_compaction(
                        reason,
                        trigger_info.tokens_used,
                        trigger_info.context_window,
                    )
                    .await;
                }
                if !cancelled
                    && self
                        .compaction
                        .auto_compact_suppressed
                        .load(std::sync::atomic::Ordering::Relaxed)
                        == SUPPRESS_NONE
                {
                    self.send_grow_notification(GrowSessionUpdate::AutoCompactFailed {
                        error: String::new(),
                    })
                    .await;
                }
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod context_recall_tests {
    use super::*;

    fn api_error(message: &str, context_window: Option<u64>) -> sampler::SamplingErrorInfo {
        sampler::SamplingErrorInfo {
            kind: sampler::SamplingErrorKind::Api,
            status_code: Some(400),
            message: message.to_owned(),
            is_retryable: false,
            retry_after_secs: None,
            model_metadata: context_window.map(|context_window| {
                sampling_types::ResponseModelMetadata {
                    context_window: Some(context_window),
                    ..Default::default()
                }
            }),
            empty_response_context: None,
            doom_loop_triggers: None,
            doom_loop_aborted_at_chunk: None,
            credential: sampling_types::SentCredential::Unknown,
        }
    }

    #[test]
    fn explicit_provider_overflow_outranks_a_lower_local_estimate() {
        let error = api_error("context window exceeded", Some(128_000));

        assert_eq!(
            sampling_error_compaction_window(&error, 96_000, 1_000_000),
            Some(128_000)
        );
    }

    #[test]
    fn explicit_provider_overflow_falls_back_to_the_configured_window() {
        let error = api_error("request exceeds the context window", None);

        assert_eq!(
            sampling_error_compaction_window(&error, 96_000, 128_000),
            Some(128_000)
        );
    }

    #[test]
    fn metadata_only_recovery_still_requires_the_local_estimate_to_cross_it() {
        let opaque = api_error("invalid request", Some(128_000));

        assert_eq!(
            sampling_error_compaction_window(&opaque, 128_001, 1_000_000),
            Some(128_000)
        );
        assert_eq!(
            sampling_error_compaction_window(&opaque, 96_000, 1_000_000),
            None
        );
    }

    fn task(task_id: &str, owner_session_id: Option<&str>) -> tools::computer::types::TaskSnapshot {
        tools::computer::types::TaskSnapshot {
            goal_definition_revision: None,
            task_id: task_id.into(),
            command: format!("command-{task_id}"),
            display_command: None,
            cwd: "/tmp".into(),
            start_time: std::time::SystemTime::UNIX_EPOCH,
            end_time: None,
            output: String::new(),
            output_file: "/tmp/output".into(),
            truncated: false,
            exit_code: None,
            signal: None,
            completed: false,
            kind: tools::computer::types::TaskKind::Bash,
            block_waited: false,
            explicitly_killed: false,
            owner_session_id: owner_session_id.map(str::to_owned),
            goal_id: None,
            description: None,
            is_backgrounded: true,
        }
    }

    #[test]
    fn background_task_snapshot_is_scoped_to_the_current_session_owner() {
        let tasks = vec![
            task("current", Some("session-current")),
            task("parent", Some("session-parent")),
            task("sibling", Some("session-sibling")),
            task("unowned", None),
        ];

        let owned = background_tasks_owned_by(tasks, "session-current");
        assert_eq!(owned.len(), 1);
        assert_eq!(owned[0].task_id, "current");
    }

    #[test]
    fn live_runtime_sections_share_one_system_reminder_envelope() {
        let reminder = append_system_reminder_section(
            Some("<system-reminder>\n## Existing\nstate\n</system-reminder>".into()),
            "system-reminder",
            "## Workflow\nactive",
        )
        .unwrap();

        assert_eq!(reminder.matches("<system-reminder>").count(), 1);
        assert_eq!(reminder.matches("</system-reminder>").count(), 1);
        assert!(reminder.contains("## Existing\nstate\n\n## Workflow\nactive"));
    }

    #[test]
    fn scheduled_task_recovery_is_bounded_and_excludes_expired_tasks() {
        use tools::implementations::grow_build::scheduler::types::ScheduledTask;

        let now = chrono::Utc::now();
        let mut active =
            ScheduledTask::new(60, "  inspect   the build output  ".into(), true, true);
        active.id = "loop-1".into();
        let mut expired = ScheduledTask::new(60, "stale task".into(), true, true);
        expired.id = "loop-old".into();
        expired.expires_at = Some(now - chrono::Duration::seconds(1));

        let reminder = format_scheduled_tasks_compaction_reminder(&[expired, active], now).unwrap();
        assert!(reminder.contains("## Scheduled Tasks"));
        assert!(reminder.contains("\"loop-1\""));
        assert!(reminder.contains("inspect the build output"));
        assert!(reminder.contains("do not recreate"));
        assert!(!reminder.contains("loop-old"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn impossible_auto_compaction_is_suppressed_for_the_rest_of_the_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                use std::sync::atomic::Ordering;

                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                actor
                    .chat_state_handle
                    .record_provider_context_anchor(90_000);
                let trigger = AutoCompactTriggerInfo {
                    tokens_used: 90_000,
                    context_window: 100_000,
                    percentage: 90,
                    source: "test",
                };

                let error = actor
                    .run_compact_only(trigger)
                    .await
                    .expect_err("a Surface without a closed prompt range cannot compact");
                assert!(
                    SessionActor::acp_error_message(&error).contains("no closed Surface range"),
                    "the fixture must exercise the pre-provider planner failure"
                );
                assert_eq!(
                    actor
                        .compaction
                        .auto_compact_suppressed
                        .load(Ordering::Relaxed),
                    SUPPRESS_TURN
                );
                assert!(
                    actor.check_auto_compact_needed().await.is_none(),
                    "later steps in the same turn must not retry the impossible plan"
                );
                // Recovery/continuation wrappers reuse this durable turn. The
                // per-turn suppression must survive another compaction check;
                // only the next TurnStarted admission clears it.
                let retry = actor.check_auto_compact_needed().await;
                assert!(retry.is_none());
                assert_eq!(
                    actor
                        .compaction
                        .auto_compact_suppressed
                        .load(Ordering::Relaxed),
                    SUPPRESS_TURN,
                    "same-turn recovery must not reset per-turn suppression"
                );

                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Compaction(
                                chat_state::CompactionEvent::Started { .. }
                            )
                        ))
                        .count(),
                    1
                );
                assert!(
                    !events.iter().any(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Sideband(_)
                    )),
                    "range planning must fail before spawning a summarizer Sideband"
                );
            })
            .await;
    }

    #[test]
    fn hint_explains_model_assisted_recall_without_expanding_surface() {
        let hint = context_recall_hint("renamed_context_recall");

        assert!(hint.contains("unloaded by compaction, not deleted"));
        assert!(hint.contains("`renamed_context_recall`"));
        assert!(hint.contains("sideband"));
        assert!(hint.contains("concise recollection"));
        assert!(hint.contains("read-only"));
        assert!(hint.contains("does not restore or expand old messages"));
    }

    #[test]
    fn control_reprojection_failure_closes_transaction_but_fails_the_turn() {
        let control_failure = Err(acp::Error::internal_error()
            .data("compaction Control context was not durably re-projected"));

        assert!(compaction_completed(&control_failure, true));
        assert!(!compaction_completed(&control_failure, false));
        assert!(control_failure.is_err());
    }
}

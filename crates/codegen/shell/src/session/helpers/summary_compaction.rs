//! grow-build's L5 wiring onto the shared range-summary engine
//! (`compaction::code_compaction`).
//!
//! The shared engine drives the sample → retry → degenerate/failure
//! classification loop via [`generate_summary`](compaction::generate_summary);
//! this module adapts grow-build's transport and diagnostics to its two seams:
//!
//! - [`ShellCompactionSampler`] wraps
//!   [`generate_session_compact`](crate::session::helpers::session_compact::generate_session_compact)
//!   as the shared [`CompactionSampler`]. It also stashes the full
//!   [`CompactOutput`] of the last successful call so the L5 loop can still
//!   record the streaming diagnostics (TTFT / stream span / stop reason) that
//!   the shared [`LlmCompactionOutput`] doesn't model.
//! - [`ShellSummaryObserver`] collects rejection counters, feeds each
//!   programmatic rejection into the next Sideband attempt, and emits the
//!   `CompactionRetryDegraded` event.
//!
//! The verbatim → fitted → simplified **input ladder** and auto-compaction
//! suppression stay in L5 (`compaction.rs`), driven by the
//! `context_overflow` / `deterministic` flags on
//! [`SummaryError`](compaction::SummaryError).

use std::sync::Mutex;
use std::time::Duration;

use ::diagnostics::events::{CompactionRetryDegraded, CompactionTrigger};
use acp_transport::protocol as acp;
use async_trait::async_trait;
use compaction::{
    CompactionPrompt, CompactionSampleError, CompactionSampler, LlmCompactionOutput,
    SummaryAttemptOutcome, SummaryObserver,
};
use sampler::SamplerConfig as SamplingConfig;
use sampling_types::{ConversationItem, ConversationRequest};

use crate::sampling::SamplingClient;
use crate::session::actor::sideband::{SidebandRun, SidebandRunError};
use crate::session::helpers::session_compact::{
    CompactFailure, CompactOutput, build_compaction_request_surface, generate_session_compact,
};

/// Wraps `generate_session_compact` as the shared engine's
/// [`CompactionSampler`] for grow-build's range-summary pass.
///
/// Holds the per-call request context the seam does not carry (client and
/// config) and stashes the last successful [`CompactOutput`] so the
/// caller can recover the streaming diagnostics not modeled by
/// [`LlmCompactionOutput`].
///
/// The shared `CompactionPrompt` is ignored because the shell owns the one
/// canonical structured prompt. The parent Timeline spawn and Sideband ledger
/// retain the exact frozen input reference, prompt, attempts, and outcome.
pub(crate) struct ShellCompactionSampler {
    user_context: Option<String>,
    client: SamplingClient,
    sampling_config: SamplingConfig,
    /// Per-chunk idle timeout forwarded to `generate_session_compact`: a stalled
    /// summarizer stream (no model-output chunk for this long) fails instead of
    /// hanging.
    idle_timeout: Duration,
    /// Wall-clock budget (secs) forwarded to `generate_session_compact` as the
    /// reasoning-runaway backstop; `0` disables it.
    wall_clock_budget_secs: u64,
    cancel: tokio_util::sync::CancellationToken,
    sideband: std::sync::Arc<tokio::sync::Mutex<SidebandRun>>,
    sideband_feedback: std::sync::Arc<Mutex<Option<String>>>,
    /// Full output of the most recent successful sample (for L5 telemetry).
    last_success: Mutex<Option<CompactOutput>>,
    image_input_unsupported: std::sync::atomic::AtomicBool,
}

impl ShellCompactionSampler {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        user_context: Option<String>,
        client: SamplingClient,
        sampling_config: SamplingConfig,
        idle_timeout: Duration,
        wall_clock_budget_secs: u64,
        cancel: tokio_util::sync::CancellationToken,
        sideband: std::sync::Arc<tokio::sync::Mutex<SidebandRun>>,
        sideband_feedback: std::sync::Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            user_context,
            client,
            sampling_config,
            idle_timeout,
            wall_clock_budget_secs,
            cancel,
            sideband,
            sideband_feedback,
            last_success: Mutex::new(None),
            image_input_unsupported: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Take the [`CompactOutput`] of the most recent successful sample, if any.
    pub(crate) fn take_last_success(&self) -> Option<CompactOutput> {
        self.last_success.lock().unwrap().take()
    }

    pub(crate) fn take_image_input_unsupported(&self) -> bool {
        self.image_input_unsupported
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    }
}

#[async_trait]
impl CompactionSampler for ShellCompactionSampler {
    type Item = ConversationItem;

    async fn sample_compaction(
        &self,
        turns: &[ConversationItem],
        _prompt: &CompactionPrompt,
        _timeout: Duration,
    ) -> Result<LlmCompactionOutput, CompactionSampleError> {
        let feedback = self.sideband_feedback.lock().unwrap().take();
        // Append the canonical summarization prompt as the final user message.
        let request_surface =
            build_compaction_request_surface(turns.to_vec(), self.user_context.as_deref());
        let audit_request = ConversationRequest {
            items: request_surface.clone(),
            model: Some(self.sampling_config.model.clone()),
            ..ConversationRequest::default()
        };
        self.sideband
            .lock()
            .await
            .attempt_all_sources(&audit_request, self.client.api_backend(), feedback)
            .await
            .map_err(sideband_error_to_sample_error)?;
        let observed_usage = std::sync::Arc::new(Mutex::new(None));
        let usage_slot = std::sync::Arc::clone(&observed_usage);
        let usage_observer: crate::session::helpers::session_compact::CompactUsageObserver =
            std::sync::Arc::new(move |usage| {
                let mut slot = usage_slot.lock().unwrap();
                debug_assert!(slot.is_none(), "compaction usage settled more than once");
                *slot = Some(usage);
            });

        let provider = generate_session_compact(
            request_surface,
            self.client.clone(),
            &self.sampling_config,
            self.idle_timeout,
            self.wall_clock_budget_secs,
            &self.cancel,
            usage_observer,
        );
        let sample = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(CompactFailure::Cancelled),
            result = async {
                self.sideband.lock().await.run_provider(provider).await
            } => result.map_err(sideband_error_to_sample_error)?,
        };
        // `generate_session_compact` drops its exactly-once meter before it
        // returns. Settle that exact provider attempt through the lifecycle
        // root before the shared summary engine can issue a retry. Otherwise
        // a transient/degenerate retry can overtake the durable Goal budget
        // transition produced by the preceding response.
        let usage = observed_usage.lock().unwrap().take().unwrap_or(None);
        let tokens = usage.map(|usage| {
            let uncached = usage.input_tokens.saturating_sub(usage.cache_read_tokens);
            i64::try_from(uncached.saturating_add(usage.output_tokens)).unwrap_or(i64::MAX)
        });
        self.sideband
            .lock()
            .await
            .settle_goal_attempt(tokens)
            .await
            .map_err(sideband_error_to_sample_error)?;

        match sample {
            Ok(output) => {
                let response = output.content.clone();
                *self.last_success.lock().unwrap() = Some(output);
                Ok(LlmCompactionOutput { response })
            }
            Err(failure) => {
                if matches!(&failure, CompactFailure::ImageInputUnsupported(_)) {
                    self.image_input_unsupported
                        .store(true, std::sync::atomic::Ordering::Release);
                }
                let error = compact_failure_to_sample_error(failure);
                *self.sideband_feedback.lock().unwrap() = Some(error.to_string());
                Err(error)
            }
        }
    }
}

/// Map grow-build's [`CompactFailure`] onto the shared engine's
/// [`CompactionSampleError`] so the shared retry loop receives an explicit
/// classification:
///
/// - `Deterministic` → [`CompactionSampleError::Deterministic`]; a
///   context-length overflow keeps its
///   message text so the engine's `is_context_length_error` check fires and
///   sets `context_overflow`.
/// - `Transient` → [`CompactionSampleError::Transient`], so the engine
///   retries it.
fn compact_failure_to_sample_error(failure: CompactFailure) -> CompactionSampleError {
    let (deterministic, err) = match failure {
        CompactFailure::Deterministic(err) => (true, err),
        CompactFailure::ImageInputUnsupported(err) => (true, err),
        CompactFailure::Transient(err) => (false, err),
        CompactFailure::Cancelled => (true, CompactFailure::cancelled_error()),
    };
    let message = acp_error_message(&err);
    if deterministic {
        CompactionSampleError::Deterministic(message)
    } else {
        CompactionSampleError::Transient(message)
    }
}

fn sideband_error_to_sample_error(error: SidebandRunError) -> CompactionSampleError {
    match error {
        SidebandRunError::Invalid(chat_state::SidebandError::AttemptBudgetExceeded) => {
            CompactionSampleError::Deterministic(
                "compaction current message exceeds budget: sideband admission rejected it".into(),
            )
        }
        SidebandRunError::Invalid(error) => CompactionSampleError::Deterministic(error.to_string()),
        SidebandRunError::Cancelled => {
            CompactionSampleError::Deterministic("sideband persistence was cancelled".into())
        }
        SidebandRunError::Admission(error) => CompactionSampleError::Deterministic(error),
        error @ (SidebandRunError::Parent(_)
        | SidebandRunError::Persistence(_)
        | SidebandRunError::InterruptedAppendRecovered) => {
            CompactionSampleError::Transient(error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sideband_budget_rejection_drives_the_compaction_input_ladder() {
        let error = sideband_error_to_sample_error(SidebandRunError::Invalid(
            chat_state::SidebandError::AttemptBudgetExceeded,
        ));
        let CompactionSampleError::Deterministic(message) = error else {
            panic!("budget admission must be a deterministic compaction failure")
        };
        assert!(compaction::is_context_length_error(&message));
    }
}

/// Render the human-readable detail an `acp::Error` carries in its `data`
/// field (where `classify_*` stash `"compact failed: <upstream>"`).
fn acp_error_message(err: &acp::Error) -> String {
    err.data
        .as_ref()
        .and_then(|d| d.as_str())
        .unwrap_or("<no data>")
        .to_string()
}

/// Collected diagnostics from a range-summary pass, drained by the L5 loop after
/// the shared engine returns.
pub(crate) struct SummaryDiagnostic {
    pub attempts: u32,
    pub degenerate_rejections: u32,
    pub transient_rejections: u32,
    pub deterministic_rejections: u32,
}

#[derive(Default)]
struct ObserverState {
    attempts: u32,
    degenerate_rejections: u32,
    transient_rejections: u32,
    deterministic_rejections: u32,
    last_error_msg: Option<String>,
}

/// [`SummaryObserver`] that owns shell-level counters, Sideband retry
/// feedback, `CompactionRetryDegraded`, and warn/error tracing without making
/// the shared engine depend on either persistence or diagnostics backends.
pub(crate) struct ShellSummaryObserver {
    trigger: CompactionTrigger,
    context_window: u64,
    compaction_id: String,
    session_id: String,
    estimated_input_tokens: u64,
    retry_delay_secs: u64,
    state: Mutex<ObserverState>,
    sideband_feedback: std::sync::Arc<Mutex<Option<String>>>,
}

impl ShellSummaryObserver {
    pub(crate) fn new(
        trigger: CompactionTrigger,
        context_window: u64,
        compaction_id: String,
        session_id: String,
        estimated_input_tokens: u64,
        retry_delay_secs: u64,
        sideband_feedback: std::sync::Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            trigger,
            context_window,
            compaction_id,
            session_id,
            estimated_input_tokens,
            retry_delay_secs,
            state: Mutex::new(ObserverState::default()),
            sideband_feedback,
        }
    }

    /// Cumulative number of attempts so far (across all input-ladder stages).
    /// Read mid-loop to label the `input_overflow` retry event.
    pub(crate) fn attempt_count(&self) -> u32 {
        self.state.lock().unwrap().attempts
    }

    /// Whether any attempt so far produced a degenerate summary — lets the L5
    /// loop distinguish degenerate-exhausted from empty-exhausted.
    pub(crate) fn degenerate_seen(&self) -> bool {
        self.state.lock().unwrap().degenerate_rejections > 0
    }

    /// The most recent rendered error/diagnostic detail, for `last_error`.
    pub(crate) fn last_error_message(&self) -> Option<String> {
        self.state.lock().unwrap().last_error_msg.clone()
    }

    /// Drain the collected diagnostics. The cumulative attempt count spans all
    /// input-ladder stages because the same observer instance is shared across
    /// every per-stage call.
    pub(crate) fn into_diagnostics(self) -> SummaryDiagnostic {
        let s = self.state.into_inner().unwrap();
        SummaryDiagnostic {
            attempts: s.attempts,
            degenerate_rejections: s.degenerate_rejections,
            transient_rejections: s.transient_rejections,
            deterministic_rejections: s.deterministic_rejections,
        }
    }
}

impl SummaryObserver for ShellSummaryObserver {
    fn on_attempt(&self, _attempt: u32, outcome: &SummaryAttemptOutcome<'_>) {
        let mut s = self.state.lock().unwrap();
        // The shared `attempt` resets per ladder stage; keep a cumulative count
        // for diagnostics spanning the entire input ladder.
        s.attempts += 1;
        let attempt = s.attempts;

        match outcome {
            SummaryAttemptOutcome::Success { .. } => {}
            SummaryAttemptOutcome::Degenerate {
                summary,
                will_retry,
            } => {
                s.degenerate_rejections += 1;
                let summary_chars = summary.chars().count();
                s.last_error_msg = Some(format!(
                    "compact failed: degenerate summary \
                     ({summary_chars} chars for ~{} input tokens)",
                    self.estimated_input_tokens
                ));
                *self.sideband_feedback.lock().unwrap() = s.last_error_msg.clone();
                if *will_retry {
                    ::diagnostics::session_ctx::log_event(CompactionRetryDegraded {
                        trigger: self.trigger,
                        reason: "degenerate_summary",
                        from_stage: None,
                        to_stage: None,
                        summary_chars: Some(summary_chars as u64),
                        attempt,
                        context_window: self.context_window,
                        compaction_id: self.compaction_id.clone(),
                    });
                    tracing::warn!(
                        session_id = %self.session_id,
                        attempt,
                        summary_chars,
                        estimated_input_tokens = self.estimated_input_tokens,
                        retry_delay_secs = self.retry_delay_secs,
                        "Compaction produced a degenerate summary, retrying in {} seconds...",
                        self.retry_delay_secs
                    );
                } else {
                    tracing::error!(
                        session_id = %self.session_id,
                        attempt,
                        summary_chars,
                        estimated_input_tokens = self.estimated_input_tokens,
                        "Compaction produced only degenerate summaries after max retries"
                    );
                }
            }
            SummaryAttemptOutcome::EmptyResponse { .. } => {
                // The shell surfaces an empty response as a transient error
                // (`generate_session_compact` returns `Transient`), so it never
                // reaches the shared `Ok("")` branch; handle defensively.
                s.transient_rejections += 1;
                let msg = "compact failed: model returned empty response".to_string();
                s.last_error_msg = Some(msg);
                *self.sideband_feedback.lock().unwrap() = s.last_error_msg.clone();
            }
            SummaryAttemptOutcome::Failure {
                message,
                deterministic,
                context_overflow,
                will_retry,
            } => {
                *self.sideband_feedback.lock().unwrap() = Some((*message).to_string());
                // A context overflow does not count toward
                // `deterministic_rejections`: the L5 ladder steps down on it
                // and tracks its own `input_overflow_rejections`.
                if *deterministic {
                    if !*context_overflow {
                        s.deterministic_rejections += 1;
                        tracing::error!(
                            session_id = %self.session_id,
                            attempt,
                            error = %message,
                            "Compaction failed (deterministic error class, no further retries)"
                        );
                    }
                } else {
                    s.transient_rejections += 1;
                    if *will_retry {
                        tracing::warn!(
                            session_id = %self.session_id,
                            attempt,
                            retry_delay_secs = self.retry_delay_secs,
                            error = %message,
                            "Compaction attempt {} failed, retrying in {} seconds...",
                            attempt,
                            self.retry_delay_secs
                        );
                    } else {
                        tracing::error!(
                            session_id = %self.session_id,
                            attempt,
                            error = %message,
                            "Compaction failed after max retries"
                        );
                    }
                }
                s.last_error_msg = Some((*message).to_string());
            }
        }
    }
}

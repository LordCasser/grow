//! Observability seam for a range-summary sampling pass.
//!
//! The shared orchestrator reports per-attempt and terminal outcomes through
//! this trait so each harness can emit its own diagnostics and durable retry
//! feedback without the shared crate depending on a diagnostics or persistence
//! backend.
//!
//! Emission points are part of the behavior contract.

use std::time::Duration;

/// Classified outcome of a single range-summary sample attempt.
///
/// `summary` is borrowed for the duration of the callback so harnesses can
/// classify it or feed rejection detail into a retry without a hot-path clone.
#[derive(Debug)]
pub enum SummaryAttemptOutcome<'a> {
    /// A usable, non-degenerate summary was produced; the pass will succeed.
    Success {
        /// Raw model summary text.
        summary: &'a str,
    },
    /// The model returned an empty / whitespace-only response.
    EmptyResponse {
        /// Whether the orchestrator will retry after this attempt.
        will_retry: bool,
    },
    /// The cleaned summary seed was too short to carry the conversation's task
    /// state; retried like a transient failure.
    Degenerate {
        /// Raw model summary text.
        summary: &'a str,
        /// Whether the orchestrator will retry after this attempt.
        will_retry: bool,
    },
    /// The sampler returned an error.
    Failure {
        /// Rendered error message.
        message: &'a str,
        /// Whether re-sending the *same* input cannot help (auth / schema /
        /// size). Transient failures (timeout / stream blip / 5xx) are `false`.
        deterministic: bool,
        /// Whether the failure was a context-length overflow — the signal the
        /// harness uses to step its input ladder rather than suppress.
        context_overflow: bool,
        /// Whether the orchestrator will retry after this attempt (always
        /// `false` for deterministic failures and context overflows).
        will_retry: bool,
    },
}

/// Receives range-summary outcomes. All methods default to no-ops so
/// harnesses without diagnostics (and tests) can use `()`.
pub trait SummaryObserver: Send + Sync {
    /// One sample attempt finished with the given classified outcome.
    /// `attempt` is 1-based and cumulative across the pass.
    fn on_attempt(&self, _attempt: u32, _outcome: &SummaryAttemptOutcome<'_>) {}

    /// The pass succeeded after `attempts` total attempts.
    fn on_success(&self, _attempts: u32, _summary_chars: usize, _elapsed: Duration) {}

    /// The pass failed terminally after `attempts` total attempts.
    fn on_error(&self, _attempts: u32) {}
}

/// No-op observer for tests and harnesses without diagnostics.
impl SummaryObserver for () {}

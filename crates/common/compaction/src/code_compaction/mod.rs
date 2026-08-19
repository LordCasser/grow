//! Canonical range-summary subsystem.
//!
//! - **Policy & content**: [`prompt`] (summarization prompt), [`summary`]
//!   (summary cleaning + carrier), [`failure`] (deterministic-vs-transient
//!   classification), [`config`] (tunables + trigger/seed defaults).
//! - **Orchestration**: [`compact`] (`build prompt → sample → validate`).
//!
//! Host-specific concerns (triggers, transport, persistence/replay, state
//! commit, metrics observer) stay in the product host (for example
//! `shell`).

pub mod compact;
pub mod config;
pub mod failure;
pub mod observer;
pub mod prompt;
pub mod sample;
pub mod summary;

pub use compact::{GeneratedSummary, SummaryError, generate_summary};
pub use config::{DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT, MIN_SUMMARY_SEED_CHARS, SummaryConfig};
pub use failure::{
    FailureKind, classify_http_status, classify_stream_event_error, is_context_length_error,
};
pub use observer::{SummaryAttemptOutcome, SummaryObserver};
pub use prompt::build_summary_prompt;
pub use sample::{SampleRetryError, SampledSummary, sample_summary_with_retries};
pub use summary::{
    format_compact_summary, format_compact_summary_content, is_degenerate_summary, wrap_user_query,
};

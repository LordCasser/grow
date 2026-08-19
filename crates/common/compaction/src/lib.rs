//! Shared, transport-agnostic compaction engine.
//!
//! This crate owns the single canonical range-summary pipeline: prompt
//! construction, bounded sampling, failure classification, and summary
//! cleaning. Host-specific range selection, trigger wiring,
//! transport, persistence, replay, state commit, and metrics backends stay in
//! each product host (for example `shell`).
//!
//! The crate depends on **neither** a conversation-type crate nor
//! `sampling-types`. It is decoupled from both Grow chat and
//! hosts through a small set of trait seams:
//!
//! - [`ItemTokenCounter`] — trusted token counting per host.
//! - [`CompactionSampler`] — the LLM call.
//! - [`SummaryObserver`] — host metrics and retry diagnostics.
//!
//! [`code_compaction`] contains the pipeline. Shared seams and primitives live
//! in [`token`], [`sampler`], [`prompt`], and [`reminder`].

pub mod code_compaction;
pub mod prompt;
pub mod prune;
pub mod reminder;
pub mod sampler;
pub mod token;

pub use code_compaction::{
    DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT, FailureKind, GeneratedSummary, MIN_SUMMARY_SEED_CHARS,
    SummaryAttemptOutcome, SummaryConfig, SummaryError, SummaryObserver, build_summary_prompt,
    classify_http_status, classify_stream_event_error, format_compact_summary,
    format_compact_summary_content, generate_summary, is_context_length_error,
    is_degenerate_summary, wrap_user_query,
};
pub use prompt::CompactionPrompt;
pub use prune::{
    PruneItem, PrunePlan, ToolResultItem, plan_tool_result_pruning, prune_tool_result_content,
};
// Reminder types/formatters: import from `reminder::` (borrowed views).
pub use reminder::append_reminder_block;
pub use sampler::{CompactionSampleError, CompactionSampler, LlmCompactionOutput};
pub use token::ItemTokenCounter;

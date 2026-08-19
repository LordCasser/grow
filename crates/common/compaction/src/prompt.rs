//! Prompt value passed across the host/sampler boundary.
//!
//! Concrete range-summary prompt content is built by
//! [`crate::code_compaction::build_summary_prompt`].

/// System + user prompt pair for the compaction LLM call.
#[derive(Debug, Clone)]
pub struct CompactionPrompt {
    pub system: String,
    pub user: String,
}

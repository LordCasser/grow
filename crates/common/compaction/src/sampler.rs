//! The `CompactionSampler` seam — the LLM call that produces summaries —
//! plus its output and error types (shared failure classification).

use std::time::Duration;

use async_trait::async_trait;

use crate::prompt::CompactionPrompt;

// ---------------------------------------------------------------------------
// Sampler output + error types
// ---------------------------------------------------------------------------

/// Raw response captured from a compaction LLM call.
#[derive(Debug, Default, Clone)]
pub struct LlmCompactionOutput {
    /// Text from the response channel — the actual compaction summary.
    pub response: String,
}

/// Classified failures at the sampler boundary.
///
/// A sampler must decide whether repeating the same request can succeed. The
/// engine never guesses from error text.
#[derive(Debug)]
pub enum CompactionSampleError {
    /// The sampler hit its end-to-end timeout. Transient.
    Timeout {
        timeout_secs: u64,
        collected_bytes: usize,
    },
    /// Repeating the same request cannot help (invalid config/request,
    /// cancellation, unsupported model).
    Deterministic(String),
    /// The same request may succeed later (transport/service/persistence
    /// failure).
    Transient(String),
    /// The model produced no response-channel content. Transient.
    EmptyResponse,
}

impl std::fmt::Display for CompactionSampleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout {
                timeout_secs,
                collected_bytes,
            } => write!(
                f,
                "Compaction sampling timed out after {}s (collected {} bytes so far)",
                timeout_secs, collected_bytes
            ),
            Self::Deterministic(msg) | Self::Transient(msg) => write!(f, "{msg}"),
            Self::EmptyResponse => {
                write!(f, "Compaction sampler returned no response channel content")
            }
        }
    }
}

impl CompactionSampleError {
    /// Whether this error is deterministic — retrying with the same input
    /// will produce the same failure.
    pub fn is_deterministic(&self) -> bool {
        match self {
            Self::Timeout { .. } | Self::EmptyResponse => false,
            Self::Deterministic(_) => true,
            Self::Transient(_) => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Sampler trait
// ---------------------------------------------------------------------------

/// Interface for the LLM call that produces compaction summaries.
///
/// Implemented by the shell's provider adapter.
#[async_trait]
pub trait CompactionSampler: Send + Sync {
    /// The harness's conversation item type.
    type Item;

    /// Run an LLM compaction call on the given items.
    ///
    /// Implementations should:
    /// - Build a synthetic conversation from the items + prompt.
    /// - Honor the `timeout`.
    /// - Collect both response and thinking channel text.
    async fn sample_compaction(
        &self,
        turns: &[Self::Item],
        prompt: &CompactionPrompt,
        timeout: Duration,
    ) -> Result<LlmCompactionOutput, CompactionSampleError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_deterministic_classification() {
        assert!(
            !CompactionSampleError::Timeout {
                timeout_secs: 1,
                collected_bytes: 0
            }
            .is_deterministic()
        );
        assert!(!CompactionSampleError::EmptyResponse.is_deterministic());
        assert!(CompactionSampleError::Deterministic("bad config".into()).is_deterministic());
        assert!(!CompactionSampleError::Transient("stream error".into()).is_deterministic());
    }
}

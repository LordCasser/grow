//! Transport-agnostic summary generation for one selected context range.
//!
//! Range selection and the durable Surface replacement belong to the host.
//! This module owns only prompt construction, bounded sampling, failure
//! classification, and diagnostics callbacks.

use std::time::{Duration, Instant};

use crate::prompt::CompactionPrompt;
use crate::sampler::CompactionSampler;

use super::config::SummaryConfig;
use super::observer::SummaryObserver;
use super::prompt::build_summary_prompt;
use super::sample::{SampleRetryError, SampledSummary, sample_summary_with_retries};

#[derive(Debug)]
pub enum SummaryError {
    NothingToCompact,
    EmptyResponse,
    Sampler {
        message: String,
        deterministic: bool,
        context_overflow: bool,
    },
}

impl std::fmt::Display for SummaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingToCompact => write!(f, "nothing to compact"),
            Self::EmptyResponse => write!(f, "compaction model returned an empty summary"),
            Self::Sampler { message, .. } => write!(f, "compaction sampling failed: {message}"),
        }
    }
}

impl std::error::Error for SummaryError {}

pub struct GeneratedSummary {
    pub summary: String,
    pub attempts: u32,
}

/// Generate a summary for the host-selected range.
///
/// The host owns the verbatim → fitted → simplified input ladder. A context
/// overflow is returned as a deterministic, explicitly classified error so
/// the host can rebuild a smaller request from the same frozen range.
pub async fn generate_summary<T, S, O>(
    sampler: &S,
    turns: &[T],
    user_context: Option<&str>,
    config: &SummaryConfig,
    observer: &O,
) -> Result<GeneratedSummary, SummaryError>
where
    T: Send + Sync,
    S: CompactionSampler<Item = T> + ?Sized,
    O: SummaryObserver + ?Sized,
{
    if turns.is_empty() {
        return Err(SummaryError::NothingToCompact);
    }

    let prompt = CompactionPrompt {
        system: String::new(),
        user: build_summary_prompt(user_context),
    };
    let timeout = Duration::from_secs(config.sampling_timeout_secs);
    let started = Instant::now();
    match sample_summary_with_retries(
        sampler,
        turns,
        &prompt,
        config.max_attempts,
        Duration::from_secs(config.retry_delay_secs),
        timeout,
        observer,
    )
    .await
    {
        Ok(SampledSummary { summary, attempts }) => {
            observer.on_success(attempts, summary.chars().count(), started.elapsed());
            Ok(GeneratedSummary { summary, attempts })
        }
        Err(SampleRetryError::Empty { attempts }) => {
            observer.on_error(attempts);
            Err(SummaryError::EmptyResponse)
        }
        Err(SampleRetryError::Failure {
            message,
            deterministic,
            context_overflow,
            attempts,
        }) => {
            observer.on_error(attempts);
            Err(SummaryError::Sampler {
                message,
                deterministic,
                context_overflow,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::sampler::{CompactionSampleError, LlmCompactionOutput};

    struct MockSampler {
        responses: Mutex<Vec<Result<String, CompactionSampleError>>>,
        calls: Mutex<usize>,
    }

    impl MockSampler {
        fn scripted(responses: Vec<Result<String, CompactionSampleError>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                calls: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl CompactionSampler for MockSampler {
        type Item = String;

        async fn sample_compaction(
            &self,
            _turns: &[String],
            _prompt: &CompactionPrompt,
            _timeout: Duration,
        ) -> Result<LlmCompactionOutput, CompactionSampleError> {
            *self.calls.lock().unwrap() += 1;
            self.responses
                .lock()
                .unwrap()
                .remove(0)
                .map(|response| LlmCompactionOutput { response })
        }
    }

    fn config() -> SummaryConfig {
        SummaryConfig {
            max_attempts: 2,
            retry_delay_secs: 0,
            sampling_timeout_secs: 5,
        }
    }

    fn healthy_summary() -> String {
        format!(
            "<summary>\n1. Primary Request: test\n{}\n</summary>",
            "x".repeat(super::super::config::MIN_SUMMARY_SEED_CHARS)
        )
    }

    #[tokio::test]
    async fn retries_transient_then_returns_raw_summary() {
        let expected = healthy_summary();
        let sampler = MockSampler::scripted(vec![
            Err(CompactionSampleError::Timeout {
                timeout_secs: 5,
                collected_bytes: 0,
            }),
            Ok(expected.clone()),
        ]);
        let result = generate_summary(&sampler, &["range".into()], None, &config(), &())
            .await
            .unwrap();
        assert_eq!(result.summary, expected);
        assert_eq!(result.attempts, 2);
        assert_eq!(*sampler.calls.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn deterministic_failure_is_not_retried() {
        let sampler = MockSampler::scripted(vec![Err(CompactionSampleError::Deterministic(
            "bad request".into(),
        ))]);
        let result = generate_summary(&sampler, &["range".into()], None, &config(), &()).await;
        assert!(matches!(
            result,
            Err(SummaryError::Sampler {
                deterministic: true,
                ..
            })
        ));
        assert_eq!(*sampler.calls.lock().unwrap(), 1);
    }
}

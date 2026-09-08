//! Sampling log — emits `tracing` events with `target: "sampling_log"`.
//! A dedicated layer in `diagnostics` routes these to
//! `~/.grow/logs/sampling.jsonl`. Enable with `--log-sampling`.

use crate::types::RequestId;

pub const TARGET: &str = "sampling_log";

#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub auth_type: &'static str,
}

pub fn request_span(
    request_id: &RequestId,
    model: &str,
    api_backend: &str,
    base_url: &str,
    auth: &AuthInfo,
) -> tracing::Span {
    tracing::info_span!(
        target: TARGET,
        "sampling_request",
        request_id = %request_id,
        model = model,
        api_backend = api_backend,
        base_url = base_url,
        auth_type = auth.auth_type,
        // Recorded from `SamplerConfig` / response usage as the request
        // progresses; `field::Empty` lets callers `record()` them later.
        reasoning_effort = tracing::field::Empty,
        output_tokens = tracing::field::Empty,
        reasoning_tokens = tracing::field::Empty,
    )
}

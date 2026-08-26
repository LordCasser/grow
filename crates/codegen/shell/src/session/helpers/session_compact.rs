//! Compacts the current conversation and generates a summary of the conversation which
//! gets passed to the next turn of the model
use crate::sampling::{
    ApiBackend, ChatCompletionRequest, ChatRequestMessage, ConversationItem, ConversationRequest,
    SamplingClient as OaiCompatClient, SamplingError, conversation_to_chat_messages,
};
use agent_client_protocol as acp;
use async_openai::types::responses::ResponseStreamEvent;
pub use chat_state::compaction_utils::{
    AUTO_CONTINUE_PROMPT, extract_last_real_user_query, extract_last_user_query,
    extract_real_user_queries, is_synthetic_extracted_query,
};
use futures_util::StreamExt;
use reqwest::StatusCode;
use sampler::SamplerConfig as SamplingConfig;
/// Outcome of a failed `generate_session_compact` call, classified at the
/// point of the typed upstream error so the caller can short-circuit
/// retries without re-parsing free-form error strings.
#[derive(Debug)]
pub(crate) enum CompactFailure {
    /// Retrying the same payload will hit the same failure. The retry loop
    /// in `run_compact_inner` should bail without sleeping or re-issuing.
    Deterministic(acp::Error),
    /// The selected model unconditionally rejected image input. The session
    /// must durably install ImageShadows before retrying a fresh compaction
    /// transaction; retrying this exact request is still deterministic.
    ImageInputUnsupported(acp::Error),
    /// Failure may resolve on retry. The caller follows its existing
    /// N-attempt + backoff loop.
    Transient(acp::Error),
    /// User/stop cancelled the in-flight compact. Do not retry or suppress AUTO.
    Cancelled,
}
/// Stable error payload for a user-cancelled compact (pager + retry loop).
pub(crate) const COMPACT_CANCELLED_MSG: &str = "compact cancelled";
impl CompactFailure {
    pub(crate) fn cancelled_error() -> acp::Error {
        acp::Error::internal_error().data(COMPACT_CANCELLED_MSG)
    }
}

pub(crate) type CompactUsageObserver =
    std::sync::Arc<dyn Fn(Option<chat_state::SidebandUsage>) + Send + Sync>;

/// Exactly-once attempt settlement guard. Once a provider request has been
/// emitted, every exit reports the latest usage if the stream supplied it, or
/// `None` when billing is unknowable. This includes timeout, cancellation,
/// provider failure, empty output, and successful completion.
struct CompactUsageMeter {
    observer: CompactUsageObserver,
    usage: Option<chat_state::SidebandUsage>,
}

impl CompactUsageMeter {
    fn new(observer: CompactUsageObserver) -> Self {
        Self {
            observer,
            usage: None,
        }
    }

    fn observe(&mut self, usage: &chat_state::SidebandUsage) {
        self.usage = Some(usage.clone());
    }
}

impl Drop for CompactUsageMeter {
    fn drop(&mut self) {
        (self.observer)(self.usage.take());
    }
}
pub(crate) use sampling_types::is_context_length_error;
/// Classify an upstream `SamplingError` for the compaction retry loop.
///
/// `Auth`, `InvalidConfiguration`, `Serialization` and
/// `IdleTimeout` are all deterministic by construction (re-issuing the same
/// request cannot change the outcome — auth state, config, payload shape,
/// and stuck-model conditions all persist). 4xx API responses other than
/// 408 (timeout) and 429 (rate limit) are likewise deterministic. Network
/// transport errors, stream-level blips, and 5xx responses are transient.
fn classify_sampling_error(err: SamplingError, image_count: usize) -> CompactFailure {
    let acp_err = acp::Error::internal_error().data(format!("compact failed: {err}"));
    if let SamplingError::Api {
        status, message, ..
    } = &err
        && sampling_types::is_unconditional_image_input_unsupported(
            Some(status.as_u16()),
            message,
            image_count,
        )
    {
        return CompactFailure::ImageInputUnsupported(acp_err);
    }
    let deterministic = match &err {
        SamplingError::Auth { .. }
        | SamplingError::InvalidConfiguration(_)
        | SamplingError::Serialization(_)
        | SamplingError::IdleTimeout { .. } => true,
        SamplingError::Api {
            status, message, ..
        } => {
            is_context_length_error(message)
                || (status.is_client_error()
                    && *status != StatusCode::REQUEST_TIMEOUT
                    && *status != StatusCode::TOO_MANY_REQUESTS)
        }
        SamplingError::Http(_)
        | SamplingError::EventStreamError(_)
        | SamplingError::EmptyResponse { .. }
        | SamplingError::DoomLoopDetected { .. } => false,
    };
    if deterministic {
        CompactFailure::Deterministic(acp_err)
    } else {
        CompactFailure::Transient(acp_err)
    }
}
/// Classify a Anthropic-style stream error event (`ResponseError` /
/// `ResponseFailed.error`) for the compaction retry loop.
///
/// `code` is the structured `code` field on the event (typically a numeric
/// HTTP status as a string, but Anthropic also uses error-type strings like
/// `"invalid_request_error"`). `message` is the human-readable detail.
///
/// Numeric codes are classified by HTTP-status range. The Anthropic
/// `invalid_request_error` marker, which can appear in either field, always
/// maps to `Deterministic` (schema violations cannot be fixed by re-sending
/// the same payload).
fn classify_response_event_error(
    code: Option<&str>,
    message: &str,
    image_count: usize,
) -> CompactFailure {
    let acp_err = acp::Error::internal_error().data(match code {
        Some(c) => format!("compact failed: {c}: {message}"),
        None => format!("compact failed: {message}"),
    });
    let status_code = code
        .and_then(|code| code.parse::<u16>().ok())
        .or_else(|| matches!(code, Some("invalid_request_error")).then_some(400));
    if sampling_types::is_unconditional_image_input_unsupported(status_code, message, image_count) {
        return CompactFailure::ImageInputUnsupported(acp_err);
    }
    if matches!(code, Some("invalid_request_error")) || message.contains("invalid_request_error") {
        return CompactFailure::Deterministic(acp_err);
    }
    if let Some(status_code) = code.and_then(|c| c.parse::<u16>().ok())
        && (400..500).contains(&status_code)
        && status_code != 408
        && status_code != 429
    {
        return CompactFailure::Deterministic(acp_err);
    }
    if is_context_length_error(message) {
        return CompactFailure::Deterministic(acp_err);
    }
    CompactFailure::Transient(acp_err)
}
/// Build the request Surface that will be sent to the compaction model.
///
/// Appends the canonical summarization prompt, optionally splicing in the
/// user-provided `/compact <text>` context, as the final `User` item.
///
/// Pure function — no I/O. Extracted so callers can persist the exact payload
/// that will be sent (for offline prompt iteration) without duplicating the
/// prompt-building logic that lives in `generate_session_compact`.
pub(crate) fn build_compaction_request_surface(
    mut input_surface: Vec<ConversationItem>,
    user_context: Option<&str>,
) -> Vec<ConversationItem> {
    let prompt = build_compaction_prompt(user_context);
    input_surface.push(ConversationItem::user(prompt));
    input_surface
}
/// Build the bare summarization prompt text (without a request Surface). See
/// [`build_compaction_request_surface`] for the wrapper that appends this to a
/// conversation as a user message.
pub(crate) fn build_compaction_prompt(user_context: Option<&str>) -> String {
    compaction::build_summary_prompt(user_context)
}
/// Output of a successful `generate_session_compact`: the summary plus the
/// streaming signals the caller records onto the compaction span. `truncated`
/// is derived from the backend's typed stop reason; `stop_reason` is kept as
/// the raw provider string for drill-down. Latency is captured online (no
/// per-token buffer) — fleet percentiles are computed at query time.
pub(crate) struct CompactOutput {
    pub content: String,
    pub usage: chat_state::SidebandUsage,
    pub stop_reason: Option<String>,
    pub truncated: bool,
    pub ttft_ms: Option<u64>,
    pub stream_ms: Option<u64>,
    pub delta_count: u64,
    pub itl_max_ms: Option<u64>,
}
/// Structured compaction outcome. Converted to a stable string only at the
/// tracing boundary (tracing can't record a custom type directly).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompactionOutcome {
    Success,
    Truncated,
    Deterministic,
    Transient,
    Degenerate,
    Failed,
}
impl CompactionOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Truncated => "truncated",
            Self::Deterministic => "deterministic",
            Self::Transient => "transient",
            Self::Degenerate => "degenerate",
            Self::Failed => "failed",
        }
    }
}
/// O(1) streaming-latency accumulator: time-to-first-token, total stream span,
/// delta count, and worst inter-token gap, computed online so we never buffer
/// per-token timestamps. Fleet percentiles are computed at query time in log analytics.
struct StreamTiming {
    start: std::time::Instant,
    first: Option<std::time::Instant>,
    last: Option<std::time::Instant>,
    count: u64,
    max_gap_ms: u64,
}
impl StreamTiming {
    fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
            first: None,
            last: None,
            count: 0,
            max_gap_ms: 0,
        }
    }
    fn record_delta(&mut self) {
        let now = std::time::Instant::now();
        if self.first.is_none() {
            self.first = Some(now);
        }
        if let Some(prev) = self.last {
            self.max_gap_ms = self
                .max_gap_ms
                .max(now.duration_since(prev).as_millis() as u64);
        }
        self.last = Some(now);
        self.count += 1;
    }
    fn ttft_ms(&self) -> Option<u64> {
        self.first
            .map(|f| f.duration_since(self.start).as_millis() as u64)
    }
    fn stream_ms(&self) -> Option<u64> {
        match (self.first, self.last) {
            (Some(f), Some(l)) => Some(l.duration_since(f).as_millis() as u64),
            _ => None,
        }
    }
    /// Worst inter-token gap; `None` until there are at least two deltas.
    fn itl_max_ms(&self) -> Option<u64> {
        if self.count >= 2 {
            Some(self.max_gap_ms)
        } else {
            None
        }
    }
    /// Wall-clock seconds since the stream started — drives the compaction
    /// wall-clock budget (the reasoning-runaway backstop).
    fn elapsed_secs(&self) -> u64 {
        self.start.elapsed().as_secs()
    }
}
enum StreamStep<T> {
    Item(T),
    Ended,
    IdleTimeout,
}
async fn next_stream_step<S, T>(
    stream: &mut S,
    idle_timeout: std::time::Duration,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<StreamStep<T>, CompactFailure>
where
    S: futures_util::Stream<Item = T> + Unpin,
{
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(CompactFailure::Cancelled),
        step = tokio::time::timeout(idle_timeout, stream.next()) => Ok(match step {
            Ok(Some(item)) => StreamStep::Item(item),
            Ok(None) => StreamStep::Ended,
            Err(_) => StreamStep::IdleTimeout,
        }),
    }
}
/// Abort `fut` if stop wins while the compact HTTP stream is still opening.
async fn await_unless_cancelled<F, T>(
    cancel: &tokio_util::sync::CancellationToken,
    fut: F,
) -> Result<T, CompactFailure>
where
    F: std::future::Future<Output = T>,
{
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(CompactFailure::Cancelled),
        result = fut => Ok(result),
    }
}
#[cfg(test)]
mod compact_cancel_await_tests {
    use super::*;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    #[tokio::test]
    async fn pre_cancelled_token_skips_fut() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = await_unless_cancelled(&cancel, async {
            panic!("fut must not run when already cancelled");
        })
        .await
        .unwrap_err();
        assert!(matches!(err, CompactFailure::Cancelled));
    }
    #[tokio::test]
    async fn cancel_aborts_pending_open() {
        let cancel = CancellationToken::new();
        let cancel2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            cancel2.cancel();
        });
        let started = std::time::Instant::now();
        let err = await_unless_cancelled(&cancel, async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            0u8
        })
        .await
        .unwrap_err();
        assert!(matches!(err, CompactFailure::Cancelled));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "stop must abort stream-open wait, elapsed {:?}",
            started.elapsed()
        );
    }
}
/// Generates a summary of the conversation for compaction.
/// Accepts `Vec<ConversationItem>` so the Responses path can preserve
/// encrypted reasoning. ChatCompletions converts at point of use.
///
/// `input_surface` must already include the summarization prompt as its final
/// user message — use [`build_compaction_request_surface`] to construct it. The
/// split lets callers persist the exact request payload before issuing it.
///
/// The provider request is deliberately tool-free: compaction is a Sideband derivation, not an
/// Agent turn. The conversation prefix may still reuse cached message tokens,
/// but the primary Agent's mutable capability catalog is outside this request.
///
/// Errors carry a [`CompactFailure`] classification so the caller can
/// short-circuit retries on deterministic failures (4xx schema violations,
/// auth errors) while still retrying transient ones (5xx,
/// network blips, rate limits).
pub(crate) async fn generate_session_compact(
    input_surface: Vec<ConversationItem>,
    client: OaiCompatClient,
    sampling_config: &SamplingConfig,
    idle_timeout: std::time::Duration,
    wall_clock_budget_secs: u64,
    cancel: &tokio_util::sync::CancellationToken,
    usage_observer: CompactUsageObserver,
) -> Result<CompactOutput, CompactFailure> {
    // The Sideband Attempt and provider admission are already established
    // before this helper is entered. The exactly-once meter therefore covers
    // cancellation races and every provider/preflight exit after emission;
    // a cancellation observed before admission never reaches this helper.
    let mut usage_meter = CompactUsageMeter::new(usage_observer);
    if cancel.is_cancelled() {
        return Err(CompactFailure::Cancelled);
    }
    let num_messages = input_surface.len();
    let image_count = sampling_types::conversation_image_groups(&input_surface)
        .iter()
        .map(sampling_types::ConversationImageGroup::image_count)
        .sum();
    let output = match sampling_config.api_backend {
        ApiBackend::ChatCompletions => {
            let chat_messages: Vec<ChatRequestMessage> =
                conversation_to_chat_messages(input_surface);
            let message =
                ChatCompletionRequest::new(sampling_config.model.to_owned(), chat_messages);
            tracing::info!(
                compact_model = %sampling_config.model,
                num_messages = num_messages,
                "Sending compact request (streaming)"
            );
            let stream_result =
                await_unless_cancelled(cancel, client.chat_completion_stream(message)).await?;
            let mut stream = match stream_result {
                Ok((s, _metadata)) => s,
                Err(e) => return Err(classify_sampling_error(e, image_count)),
            };
            let mut timing = StreamTiming::new();
            let mut truncated = false;
            let mut stop_reason: Option<String> = None;
            let mut content = String::new();
            let mut usage = chat_state::SidebandUsage::default();
            let mut last_progress_at = std::time::Instant::now();
            loop {
                let idle_remaining = idle_timeout.saturating_sub(last_progress_at.elapsed());
                let chunk_result = match next_stream_step(&mut stream, idle_remaining, cancel)
                    .await?
                {
                    StreamStep::Item(item) => item,
                    StreamStep::Ended => break,
                    StreamStep::IdleTimeout => {
                        return Err(
                            CompactFailure::Transient(
                                acp::Error::internal_error()
                                    .data(
                                        format!(
                                "compact failed: stream idle timeout after {idle_timeout:?} ({} chars received)",
                                content.chars().count()
                            ),
                                    ),
                            ),
                        );
                    }
                };
                if wall_clock_budget_secs > 0 && timing.elapsed_secs() >= wall_clock_budget_secs {
                    return Err(
                        CompactFailure::Transient(
                            acp::Error::internal_error()
                                .data(
                                    format!(
                            "compact failed: exceeded wall-clock budget {wall_clock_budget_secs}s (runaway generation)"
                        ),
                                ),
                        ),
                    );
                }
                match chunk_result {
                    Ok(chunk) => {
                        if let Some(reported) = chunk.usage.as_ref() {
                            let normalized: sampling_types::TokenUsage = reported.clone().into();
                            usage = crate::session::actor::sideband::sideband_usage_from_tokens(
                                &normalized,
                            );
                            usage_meter.observe(&usage);
                        }
                        if let Some(choice) = chunk.choices.first() {
                            let delta = &choice.delta;
                            if choice.finish_reason.is_some()
                                || delta.content.as_deref().is_some_and(|s| !s.is_empty())
                                || delta
                                    .reasoning_content
                                    .as_deref()
                                    .is_some_and(|s| !s.is_empty())
                                || !delta.tool_calls.is_empty()
                            {
                                last_progress_at = std::time::Instant::now();
                            }
                            if let Some(delta_content) = &choice.delta.content {
                                timing.record_delta();
                                content.push_str(delta_content);
                            }
                            if let Some(fr) = choice.finish_reason {
                                let sr = sampling_types::StopReason::from(fr);
                                truncated = matches!(sr, sampling_types::StopReason::Length);
                                stop_reason = Some(sr.as_str().to_string());
                            }
                        }
                    }
                    Err(e) => return Err(classify_sampling_error(e, image_count)),
                }
            }
            CompactOutput {
                content,
                usage,
                stop_reason,
                truncated,
                ttft_ms: timing.ttft_ms(),
                stream_ms: timing.stream_ms(),
                delta_count: timing.count,
                itl_max_ms: timing.itl_max_ms(),
            }
        }
        ApiBackend::Responses => {
            let request = ConversationRequest {
                items: input_surface,
                model: Some(sampling_config.model.to_owned()),
                ..Default::default()
            };
            let stream_result =
                await_unless_cancelled(cancel, client.conversation_stream_responses(request))
                    .await?;
            let mut stream = match stream_result {
                Ok((s, _metadata, _doom_loop)) => s,
                Err(e) => return Err(classify_sampling_error(e, image_count)),
            };
            let mut timing = StreamTiming::new();
            let mut truncated = false;
            let mut stop_reason: Option<String> = None;
            let mut content = String::new();
            let mut usage = chat_state::SidebandUsage::default();
            let mut last_progress_at = std::time::Instant::now();
            loop {
                let idle_remaining = idle_timeout.saturating_sub(last_progress_at.elapsed());
                let chunk_result = match next_stream_step(&mut stream, idle_remaining, cancel)
                    .await?
                {
                    StreamStep::Item(item) => item,
                    StreamStep::Ended => break,
                    StreamStep::IdleTimeout => {
                        return Err(
                            CompactFailure::Transient(
                                acp::Error::internal_error()
                                    .data(
                                        format!(
                                "compact failed: stream idle timeout after {idle_timeout:?} ({} chars received)",
                                content.chars().count()
                            ),
                                    ),
                            ),
                        );
                    }
                };
                if wall_clock_budget_secs > 0 && timing.elapsed_secs() >= wall_clock_budget_secs {
                    return Err(
                        CompactFailure::Transient(
                            acp::Error::internal_error()
                                .data(
                                    format!(
                            "compact failed: exceeded wall-clock budget {wall_clock_budget_secs}s (runaway generation)"
                        ),
                                ),
                        ),
                    );
                }
                match chunk_result {
                    Ok(chunk) => {
                        if !matches!(
                            &chunk,
                            ResponseStreamEvent::ResponseCreated(_)
                                | ResponseStreamEvent::ResponseInProgress(_)
                                | ResponseStreamEvent::ResponseQueued(_)
                        ) {
                            last_progress_at = std::time::Instant::now();
                        }
                        match &chunk {
                            ResponseStreamEvent::ResponseOutputTextDelta(text_delta_event) => {
                                timing.record_delta();
                                content.push_str(&text_delta_event.delta);
                            }
                            ResponseStreamEvent::ResponseCompleted(completed_event) => {
                                if let Some(reported) = completed_event.response.usage.as_ref() {
                                    usage = chat_state::SidebandUsage {
                                        input_tokens: reported.input_tokens.into(),
                                        output_tokens: reported.output_tokens.into(),
                                        cache_read_tokens: reported
                                            .input_tokens_details
                                            .cached_tokens
                                            .into(),
                                        cache_write_tokens: 0,
                                    };
                                    usage_meter.observe(&usage);
                                }
                            }
                            ResponseStreamEvent::ResponseFailed(failed_event) => {
                                if let Some(reported) = failed_event.response.usage.as_ref() {
                                    usage = chat_state::SidebandUsage {
                                        input_tokens: reported.input_tokens.into(),
                                        output_tokens: reported.output_tokens.into(),
                                        cache_read_tokens: reported
                                            .input_tokens_details
                                            .cached_tokens
                                            .into(),
                                        cache_write_tokens: 0,
                                    };
                                    usage_meter.observe(&usage);
                                }
                                let event_error = failed_event.response.error.as_ref();
                                let code = event_error.map(|e| e.code.as_str());
                                let message = event_error
                                    .map(|e| e.message.as_str())
                                    .unwrap_or("unknown error");
                                tracing::warn!(
                                    code = code.unwrap_or("none"),
                                    message = %message,
                                    status = ?failed_event.response.status,
                                    "compact: response.failed event"
                                );
                                return Err(classify_response_event_error(
                                    code,
                                    message,
                                    image_count,
                                ));
                            }
                            ResponseStreamEvent::ResponseError(error_event) => {
                                let code = error_event.code.as_deref();
                                tracing::warn!(
                                    code = code.unwrap_or("none"),
                                    message = %error_event.message,
                                    "compact: stream error event"
                                );
                                return Err(classify_response_event_error(
                                    code,
                                    &error_event.message,
                                    image_count,
                                ));
                            }
                            ResponseStreamEvent::ResponseIncomplete(incomplete_event) => {
                                if let Some(reported) = incomplete_event.response.usage.as_ref() {
                                    usage = chat_state::SidebandUsage {
                                        input_tokens: reported.input_tokens.into(),
                                        output_tokens: reported.output_tokens.into(),
                                        cache_read_tokens: reported
                                            .input_tokens_details
                                            .cached_tokens
                                            .into(),
                                        cache_write_tokens: 0,
                                    };
                                    usage_meter.observe(&usage);
                                }
                                let reason = incomplete_event
                                    .response
                                    .incomplete_details
                                    .as_ref()
                                    .map(|d| d.reason.clone())
                                    .unwrap_or_else(|| "unknown".to_string());
                                tracing::warn!(
                                    reason = %reason,
                                    "compact: response.incomplete event"
                                );
                                stop_reason = Some(reason);
                                truncated = true;
                            }
                            _ => {}
                        }
                    }
                    Err(e) => return Err(classify_sampling_error(e, image_count)),
                }
            }
            CompactOutput {
                content,
                usage,
                stop_reason: stop_reason.or_else(|| Some("stop".to_string())),
                truncated,
                ttft_ms: timing.ttft_ms(),
                stream_ms: timing.stream_ms(),
                delta_count: timing.count,
                itl_max_ms: timing.itl_max_ms(),
            }
        }
        ApiBackend::Messages => {
            let request = ConversationRequest {
                items: input_surface,
                model: Some(sampling_config.model.to_owned()),
                ..Default::default()
            };
            let stream_result =
                await_unless_cancelled(cancel, client.conversation_stream_messages(request))
                    .await?;
            let mut stream = match stream_result {
                Ok((s, _metadata)) => s,
                Err(e) => return Err(classify_sampling_error(e, image_count)),
            };
            let mut timing = StreamTiming::new();
            let mut truncated = false;
            let mut stop_reason: Option<String> = None;
            let mut content = String::new();
            let mut usage = chat_state::SidebandUsage::default();
            let mut last_progress_at = std::time::Instant::now();
            loop {
                let idle_remaining = idle_timeout.saturating_sub(last_progress_at.elapsed());
                let chunk_result = match next_stream_step(&mut stream, idle_remaining, cancel)
                    .await?
                {
                    StreamStep::Item(item) => item,
                    StreamStep::Ended => break,
                    StreamStep::IdleTimeout => {
                        return Err(
                            CompactFailure::Transient(
                                acp::Error::internal_error()
                                    .data(
                                        format!(
                                "compact failed: stream idle timeout after {idle_timeout:?} ({} chars received)",
                                content.chars().count()
                            ),
                                    ),
                            ),
                        );
                    }
                };
                if wall_clock_budget_secs > 0 && timing.elapsed_secs() >= wall_clock_budget_secs {
                    return Err(
                        CompactFailure::Transient(
                            acp::Error::internal_error()
                                .data(
                                    format!(
                            "compact failed: exceeded wall-clock budget {wall_clock_budget_secs}s (runaway generation)"
                        ),
                                ),
                        ),
                    );
                }
                match chunk_result {
                    Ok(event) => {
                        if !matches!(&event, sampling_types::messages::MessageStreamEvent::Ping) {
                            last_progress_at = std::time::Instant::now();
                        }
                        match event {
                            sampling_types::messages::MessageStreamEvent::MessageStart {
                                message,
                            } => {
                                usage = chat_state::SidebandUsage {
                                    input_tokens: message.usage.input_tokens.into(),
                                    output_tokens: message.usage.output_tokens.into(),
                                    cache_read_tokens: message.usage.cache_read_input_tokens.into(),
                                    cache_write_tokens: message
                                        .usage
                                        .cache_creation_input_tokens
                                        .into(),
                                };
                                usage_meter.observe(&usage);
                            }
                            sampling_types::messages::MessageStreamEvent::ContentBlockDelta {
                                delta: sampling_types::messages::StreamDelta::TextDelta { text },
                                ..
                            } => {
                                timing.record_delta();
                                content.push_str(&text);
                            }
                            sampling_types::messages::MessageStreamEvent::MessageDelta {
                                delta,
                                usage: reported,
                            } => {
                                usage.output_tokens = reported.output_tokens.into();
                                if let Some(input_tokens) = reported.input_tokens {
                                    usage.input_tokens = input_tokens.into();
                                }
                                if let Some(cache_read_tokens) = reported.cache_read_input_tokens {
                                    usage.cache_read_tokens = cache_read_tokens.into();
                                }
                                if let Some(cache_write_tokens) =
                                    reported.cache_creation_input_tokens
                                {
                                    usage.cache_write_tokens = cache_write_tokens.into();
                                }
                                usage_meter.observe(&usage);
                                if let Some(sr) = delta.stop_reason {
                                    truncated = matches!(
                                    sr,
                                    sampling_types::messages::StopReason::MaxTokens
                                        | sampling_types::messages::StopReason::ModelContextWindowExceeded
                                );
                                    stop_reason = Some(
                                        match sr {
                                            sampling_types::messages::StopReason::EndTurn => {
                                                "end_turn".to_string()
                                            }
                                            sampling_types::messages::StopReason::MaxTokens => {
                                                "max_tokens".to_string()
                                            }
                                            sampling_types::messages::StopReason::ToolUse => {
                                                "tool_use".to_string()
                                            }
                                            sampling_types::messages::StopReason::StopSequence => {
                                                "stop_sequence".to_string()
                                            }
                                            sampling_types::messages::StopReason::Refusal => {
                                                "refusal".to_string()
                                            }
                                            sampling_types::messages::StopReason::PauseTurn => {
                                                "pause_turn".to_string()
                                            }
                                            sampling_types::messages::StopReason::ModelContextWindowExceeded => {
                                                "model_context_window_exceeded".to_string()
                                            }
                                            sampling_types::messages::StopReason::Unknown(
                                                s,
                                            ) => s,
                                        },
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(e) => return Err(classify_sampling_error(e, image_count)),
                }
            }
            CompactOutput {
                content,
                usage,
                stop_reason,
                truncated,
                ttft_ms: timing.ttft_ms(),
                stream_ms: timing.stream_ms(),
                delta_count: timing.count,
                itl_max_ms: timing.itl_max_ms(),
            }
        }
    };
    if output.content.is_empty() {
        Err(CompactFailure::Transient(
            acp::Error::internal_error().data("compact failed: model returned empty response"),
        ))
    } else {
        Ok(output)
    }
}
/// Tests for `classify_sampling_error` and `classify_response_event_error`.
/// Pin the deterministic-vs-transient mapping for every `SamplingError`
/// variant and for the meaningful branches of the response-event classifier
/// (numeric code, `invalid_request_error` marker in code or message, and
/// the default-to-transient fallback for unknown / missing codes).
/// Also covers `StreamTiming` boundaries and `CompactionOutcome::as_str`.
#[cfg(test)]
mod classify_tests {
    use super::*;
    fn is_det(failure: &CompactFailure) -> bool {
        matches!(
            failure,
            CompactFailure::Deterministic(_) | CompactFailure::ImageInputUnsupported(_)
        )
    }
    #[test]
    fn sampling_api_4xx_is_deterministic_except_408_and_429() {
        let det = |s: StatusCode| {
            is_det(&classify_sampling_error(
                SamplingError::Api {
                    status: s,
                    message: "test".into(),
                    model_metadata: None,
                    retry_after_secs: None,
                    should_retry: None,
                },
                0,
            ))
        };
        assert!(det(StatusCode::BAD_REQUEST));
        assert!(det(StatusCode::UNAUTHORIZED));
        assert!(det(StatusCode::FORBIDDEN));
        assert!(det(StatusCode::NOT_FOUND));
        assert!(det(StatusCode::PAYLOAD_TOO_LARGE));
        assert!(!det(StatusCode::REQUEST_TIMEOUT));
        assert!(!det(StatusCode::TOO_MANY_REQUESTS));
        assert!(!det(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(!det(StatusCode::BAD_GATEWAY));
        assert!(!det(StatusCode::SERVICE_UNAVAILABLE));
    }
    #[test]
    fn sampling_non_api_variants_classify_correctly() {
        assert!(is_det(&classify_sampling_error(
            SamplingError::auth_unknown("expired"),
            0,
        )));
        assert!(is_det(&classify_sampling_error(
            SamplingError::InvalidConfiguration("missing key"),
            0,
        )));
        assert!(is_det(&classify_sampling_error(
            SamplingError::IdleTimeout { elapsed_secs: 60 },
            0,
        )));
        assert!(!is_det(&classify_sampling_error(
            SamplingError::EventStreamError("conn reset".into()),
            0,
        )));
        assert!(!is_det(&classify_sampling_error(
            SamplingError::from_stream_error("overloaded_error", "try again"),
            0,
        )));
    }
    #[test]
    fn response_event_invalid_request_error_marker_is_deterministic() {
        assert!(is_det(&classify_response_event_error(
            Some("invalid_request_error"),
            "messages.27.content.1: ...",
            0,
        )));
        assert!(is_det(&classify_response_event_error(
            Some("400"),
            "Provider returned invalid_request_error: messages.X...",
            0,
        )));
    }
    #[test]
    fn response_event_numeric_codes_match_http_classification() {
        let det = |c: &str| is_det(&classify_response_event_error(Some(c), "msg", 0));
        assert!(det("400"));
        assert!(det("401"));
        assert!(det("403"));
        assert!(det("404"));
        assert!(!det("408"));
        assert!(!det("429"));
        assert!(!det("500"));
        assert!(!det("503"));
    }
    #[test]
    fn response_event_unknown_code_defaults_to_transient() {
        assert!(!is_det(&classify_response_event_error(None, "msg", 0)));
        assert!(!is_det(&classify_response_event_error(
            Some("error"),
            "msg",
            0,
        )));
        assert!(!is_det(&classify_response_event_error(
            Some("overloaded_error"),
            "msg",
            0,
        )));
    }
    #[test]
    fn response_event_marker_in_message_with_no_code_is_deterministic() {
        assert!(is_det(&classify_response_event_error(
            None,
            "messages.X.content.Y: invalid_request_error: ...",
            0,
        )));
    }
    #[test]
    fn response_event_context_length_message_is_deterministic() {
        assert!(is_det(&classify_response_event_error(
            None,
            "The prompt is too long for this model's context window.",
            0,
        )));
    }
    #[test]
    fn sampling_api_500_with_context_length_message_is_deterministic() {
        assert!(is_det(&classify_sampling_error(
            SamplingError::Api {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "API error (status 500 Internal Server Error): \
                      The prompt is too long for this model's context window."
                    .into(),
                model_metadata: None,
                retry_after_secs: None,
                should_retry: None,
            },
            0
        )));
    }
    #[test]
    fn sampling_http_is_transient() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let http_err = rt
            .block_on(reqwest::get("http://127.0.0.1:0"))
            .expect_err("connecting to port 0 must fail");
        assert!(!is_det(&classify_sampling_error(
            SamplingError::Http(http_err),
            0
        )));
    }
    #[test]
    fn sampling_serialization_is_deterministic() {
        let serde_err = serde_json::from_str::<u32>("not a number").unwrap_err();
        assert!(is_det(&classify_sampling_error(
            SamplingError::Serialization(serde_err),
            0,
        )));
    }
    #[test]
    fn classifier_preserves_acp_error_data() {
        let CompactFailure::Deterministic(err) = classify_sampling_error(
            SamplingError::Api {
                status: StatusCode::BAD_REQUEST,
                message: "bad payload".into(),
                model_metadata: None,
                retry_after_secs: None,
                should_retry: None,
            },
            0,
        ) else {
            panic!("expected Deterministic for 400");
        };
        let data = err.data.as_ref().and_then(|d| d.as_str()).unwrap();
        assert!(data.contains("compact failed"));
        assert!(data.contains("bad payload"));
        let CompactFailure::Transient(err) = classify_sampling_error(
            SamplingError::Api {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "upstream blip".into(),
                model_metadata: None,
                retry_after_secs: None,
                should_retry: None,
            },
            0,
        ) else {
            panic!("expected Transient for 500");
        };
        let data = err.data.as_ref().and_then(|d| d.as_str()).unwrap();
        assert!(data.contains("upstream blip"));
    }
    #[test]
    fn image_capability_rejection_is_typed_only_when_the_request_has_images() {
        let reject = |image_count| {
            classify_sampling_error(
                SamplingError::Api {
                    status: StatusCode::BAD_REQUEST,
                    message: "input_image is not supported by this model".into(),
                    model_metadata: None,
                    retry_after_secs: None,
                    should_retry: None,
                },
                image_count,
            )
        };
        assert!(matches!(
            reject(1),
            CompactFailure::ImageInputUnsupported(_)
        ));
        assert!(matches!(reject(0), CompactFailure::Deterministic(_)));
        assert!(matches!(
            classify_sampling_error(
                SamplingError::Api {
                    status: StatusCode::BAD_REQUEST,
                    message: "unsupported image format: animated webp".into(),
                    model_metadata: None,
                    retry_after_secs: None,
                    should_retry: None,
                },
                1,
            ),
            CompactFailure::Deterministic(_)
        ));
        assert!(matches!(
            classify_response_event_error(
                Some("invalid_request_error"),
                "this model only supports text",
                1,
            ),
            CompactFailure::ImageInputUnsupported(_)
        ));
    }
    #[test]
    fn stream_timing_boundaries() {
        let mut t = StreamTiming::new();
        assert_eq!(t.count, 0);
        assert_eq!(t.ttft_ms(), None);
        assert_eq!(t.stream_ms(), None);
        assert_eq!(t.itl_max_ms(), None);
        t.record_delta();
        assert_eq!(t.count, 1);
        assert!(t.ttft_ms().is_some());
        assert!(t.stream_ms().is_some());
        assert_eq!(t.itl_max_ms(), None);
        t.record_delta();
        assert_eq!(t.count, 2);
        assert!(t.itl_max_ms().is_some());
    }
    #[test]
    fn compaction_outcome_as_str_is_stable() {
        assert_eq!(CompactionOutcome::Success.as_str(), "success");
        assert_eq!(CompactionOutcome::Truncated.as_str(), "truncated");
        assert_eq!(CompactionOutcome::Deterministic.as_str(), "deterministic");
        assert_eq!(CompactionOutcome::Transient.as_str(), "transient");
        assert_eq!(CompactionOutcome::Degenerate.as_str(), "degenerate");
        assert_eq!(CompactionOutcome::Failed.as_str(), "failed");
    }
}

/// Regression: ChatCompletions compaction must not panic on a standalone `Reasoning` sibling.
#[cfg(test)]
mod reasoning_compaction_regression_tests {
    use super::*;
    use crate::sampling::{SamplerConfig, SamplingClient, rs};
    use axum::Router;
    use axum::response::sse::{Event, KeepAlive, Sse};
    use axum::routing::post;
    use futures_util::stream;
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    /// Minimal ChatCompletions SSE stream: one content token, `stop`, then `[DONE]`.
    fn summary_stream() -> Vec<Event> {
        vec![
            Event::default().data(
                json!({
                    "id": "chatcmpl-test",
                    "object": "chat.completion.chunk",
                    "created": 1234567890,
                    "model": "test-model",
                    "choices": [{
                        "index": 0,
                        "delta": { "role": "assistant", "content": "<summary>ok</summary>" },
                        "finish_reason": "stop"
                    }]
                })
                .to_string(),
            ),
            Event::default().data("[DONE]"),
        ]
    }
    /// SSE stream: a reasoning delta (no content), then a content delta + `stop`.
    fn reasoning_then_summary_stream() -> Vec<Event> {
        vec![
            Event::default().data(
                json!({
                    "id": "chatcmpl-test",
                    "object": "chat.completion.chunk",
                    "created": 1234567890,
                    "model": "test-model",
                    "choices": [{
                        "index": 0,
                        "delta": { "role": "assistant", "reasoning_content": "let me think about the summary" },
                        "finish_reason": null
                    }]
                })
                .to_string(),
            ),
            Event::default().data(
                json!({
                    "id": "chatcmpl-test",
                    "object": "chat.completion.chunk",
                    "created": 1234567890,
                    "model": "test-model",
                    "choices": [{
                        "index": 0,
                        "delta": { "content": "<summary>ok</summary>" },
                        "finish_reason": "stop"
                    }]
                })
                .to_string(),
            ),
            Event::default().data("[DONE]"),
        ]
    }
    /// A reasoning delta that precedes the content delta must not break
    /// summary extraction.
    #[tokio::test]
    async fn chat_completions_compaction_extracts_summary_after_reasoning_delta() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                let stream = stream::iter(
                    reasoning_then_summary_stream()
                        .into_iter()
                        .map(Ok::<_, std::convert::Infallible>),
                );
                Sse::new(stream).keep_alive(KeepAlive::default())
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        let base_url = format!("http://{addr}/v1");
        let config = test_config(&base_url);
        let client = SamplingClient::new(config.clone()).unwrap();
        let input_surface = vec![
            ConversationItem::system("You are a helpful assistant."),
            ConversationItem::user("<user_query>\nfix the bug\n</user_query>"),
            ConversationItem::assistant("I fixed it."),
            ConversationItem::user("Summarize the conversation so far."),
        ];
        let output = generate_session_compact(
            input_surface,
            client,
            &config,
            std::time::Duration::from_secs(30),
            0,
            &tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(|_| {}),
        )
        .await
        .unwrap_or_else(|_| panic!("compaction must succeed"));
        assert_eq!(output.content, "<summary>ok</summary>");
        let _ = shutdown_tx.send(());
    }
    fn test_config(base_url: &str) -> SamplerConfig {
        SamplerConfig {
            api_key: Some("test-api-key".to_string()),
            base_url: base_url.to_string(),
            model: "test-model".to_string(),
            output_limit: Some(1000),
            temperature: Some(0.7),
            top_p: None,
            api_backend: ApiBackend::ChatCompletions,
            auth_scheme: Default::default(),
            extra_headers: Default::default(),
            query_params: Default::default(),
            env_http_headers: Default::default(),
            context_window: 256_000,
            force_http1: false,
            max_retries: None,
            stream_tool_calls: false,
            idle_timeout_secs: None,
            reasoning_effort: None,
            origin_client: None,
            attribution_callback: None,
            bearer_resolver: None,
            compactions_remaining: None,
            compaction_at_tokens: None,
            doom_loop_recovery: None,
        }
    }

    #[tokio::test]
    async fn already_cancelled_compaction_still_settles_the_admitted_attempt() {
        let config = test_config("http://127.0.0.1:1/v1");
        let client = SamplingClient::new(config.clone()).unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let observer = {
            let observed = std::sync::Arc::clone(&observed);
            std::sync::Arc::new(move |usage| observed.lock().unwrap().push(usage))
                as CompactUsageObserver
        };

        assert!(matches!(
            generate_session_compact(
                vec![ConversationItem::user("summarize")],
                client,
                &config,
                std::time::Duration::from_secs(1),
                0,
                &cancel,
                observer,
            )
            .await,
            Err(CompactFailure::Cancelled)
        ));
        assert_eq!(observed.lock().unwrap().as_slice(), &[None]);
    }
    #[tokio::test]
    async fn chat_completions_compaction_does_not_panic_on_reasoning_sibling() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                let stream = stream::iter(
                    summary_stream()
                        .into_iter()
                        .map(Ok::<_, std::convert::Infallible>),
                );
                Sse::new(stream).keep_alive(KeepAlive::default())
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        let base_url = format!("http://{addr}/v1");
        let config = test_config(&base_url);
        let client = SamplingClient::new(config.clone()).unwrap();
        let input_surface = vec![
            ConversationItem::system("You are a helpful assistant."),
            ConversationItem::user("<user_query>\nfix the bug\n</user_query>"),
            ConversationItem::Reasoning(rs::ReasoningItem {
                id: Some("r1".to_string()),
                summary: vec![rs::SummaryPart::SummaryText(rs::SummaryTextContent {
                    text: "thinking about the bug".to_string(),
                })],
                content: None,
                encrypted_content: None,
                status: None,
            }),
            ConversationItem::assistant("I fixed it."),
            ConversationItem::user("Summarize the conversation so far."),
        ];
        let result = generate_session_compact(
            input_surface,
            client,
            &config,
            std::time::Duration::from_secs(30),
            0,
            &tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(|_| {}),
        )
        .await;
        let output = result
            .unwrap_or_else(|_| panic!("compaction must succeed for a Reasoning-bearing history"));
        assert_eq!(output.content, "<summary>ok</summary>");
        let _ = shutdown_tx.send(());
    }
    #[tokio::test]
    async fn chat_completions_compaction_is_tool_free() {
        use std::sync::{Arc, Mutex};
        let captured: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let cap = captured.clone();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |body: axum::Json<serde_json::Value>| {
                let cap = cap.clone();
                async move {
                    cap.lock().unwrap().push(body.0);
                    let stream = stream::iter(
                        summary_stream()
                            .into_iter()
                            .map(Ok::<_, std::convert::Infallible>),
                    );
                    Sse::new(stream).keep_alive(KeepAlive::default())
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        let base_url = format!("http://{addr}/v1");
        let config = test_config(&base_url);
        let input_surface = vec![
            ConversationItem::system("You are a helpful assistant."),
            ConversationItem::user("<user_query>\nfix the bug\n</user_query>"),
            ConversationItem::assistant("I fixed it."),
            ConversationItem::user("Summarize the conversation so far."),
        ];
        let client = SamplingClient::new(config.clone()).unwrap();
        generate_session_compact(
            input_surface,
            client,
            &config,
            std::time::Duration::from_secs(30),
            0,
            &tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(|_| {}),
        )
        .await
        .unwrap_or_else(|_| panic!("tool-free compaction must succeed"));
        let bodies = captured.lock().unwrap();
        assert_eq!(bodies.len(), 1, "mock must receive one request");
        let request = &bodies[0];
        assert!(
            request.get("tools").is_none(),
            "Sideband compaction must not advertise tools"
        );
        assert!(
            request.get("tool_choice").is_none(),
            "Sideband compaction must not advertise tool_choice"
        );
        let _ = shutdown_tx.send(());
    }
    fn responses_summary_stream() -> Vec<Event> {
        vec![
            Event::default().data(
                json!({
                    "type": "response.created",
                    "sequence_number": 0,
                    "response": {
                        "id": "resp_test",
                        "object": "response",
                        "created_at": 1234567890,
                        "model": "test-model",
                        "status": "in_progress",
                        "output": []
                    }
                })
                .to_string(),
            ),
            Event::default().data(
                json!({
                    "type": "response.output_text.delta",
                    "sequence_number": 1,
                    "item_id": "msg_test",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": "<summary>ok</summary>"
                })
                .to_string(),
            ),
            Event::default().data(
                json!({
                    "type": "response.completed",
                    "sequence_number": 2,
                    "response": {
                        "id": "resp_test",
                        "object": "response",
                        "created_at": 1234567890,
                        "model": "test-model",
                        "status": "completed",
                        "output": []
                    }
                })
                .to_string(),
            ),
        ]
    }
    fn test_config_responses(base_url: &str) -> SamplerConfig {
        let mut config = test_config(base_url);
        config.api_backend = ApiBackend::Responses;
        config
    }
    #[tokio::test]
    async fn responses_compaction_is_tool_free() {
        use std::sync::{Arc, Mutex};
        let captured: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let cap = captured.clone();
        let app = Router::new().route(
            "/v1/responses",
            post(move |body: axum::Json<serde_json::Value>| {
                let cap = cap.clone();
                async move {
                    cap.lock().unwrap().push(body.0);
                    let stream = stream::iter(
                        responses_summary_stream()
                            .into_iter()
                            .map(Ok::<_, std::convert::Infallible>),
                    );
                    Sse::new(stream).keep_alive(KeepAlive::default())
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        let base_url = format!("http://{addr}/v1");
        let config = test_config_responses(&base_url);
        let input_surface = vec![
            ConversationItem::system("You are a helpful assistant."),
            ConversationItem::user("<user_query>\nfix the bug\n</user_query>"),
            ConversationItem::assistant("I fixed it."),
            ConversationItem::user("Summarize the conversation so far."),
        ];
        let client = SamplingClient::new(config.clone()).unwrap();
        generate_session_compact(
            input_surface,
            client,
            &config,
            std::time::Duration::from_secs(30),
            0,
            &tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(|_| {}),
        )
        .await
        .unwrap_or_else(|_| panic!("tool-free Responses compaction must succeed"));
        let bodies = captured.lock().unwrap();
        assert_eq!(bodies.len(), 1, "mock must receive one request");
        let request = &bodies[0];
        assert!(
            request
                .get("tools")
                .map(|t| t.as_array().is_none_or(|a| a.is_empty()))
                .unwrap_or(true),
            "Sideband compaction must not advertise tools"
        );
        assert!(
            request.get("tool_choice").is_none(),
            "Sideband compaction must not advertise tool_choice"
        );
        let _ = shutdown_tx.send(());
    }
    #[tokio::test]
    async fn stalled_compaction_stream_times_out_as_transient() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                let stream = stream::pending::<Result<Event, std::convert::Infallible>>();
                Sse::new(stream)
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        let base_url = format!("http://{addr}/v1");
        let config = test_config(&base_url);
        let client = SamplingClient::new(config.clone()).unwrap();
        let input_surface = vec![
            ConversationItem::system("You are a helpful assistant."),
            ConversationItem::user("Summarize the conversation so far."),
        ];
        let result = generate_session_compact(
            input_surface,
            client,
            &config,
            std::time::Duration::from_millis(150),
            0,
            &tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(|_| {}),
        )
        .await;
        match result {
            Err(CompactFailure::Transient(err)) => {
                let data = err
                    .data
                    .as_ref()
                    .and_then(|d| d.as_str())
                    .unwrap_or_default();
                assert!(
                    data.contains("idle timeout"),
                    "expected an idle-timeout transient failure, got: {data}"
                );
            }
            Err(
                CompactFailure::Deterministic(_)
                | CompactFailure::ImageInputUnsupported(_)
                | CompactFailure::Cancelled,
            ) => {
                panic!(
                    "a stalled stream must be retryable (Transient), not Deterministic/Cancelled"
                )
            }
            Ok(_) => panic!("a stalled stream must not produce a summary"),
        }
        let _ = shutdown_tx.send(());
    }
    #[tokio::test]
    async fn completed_then_stalled_stream_errors_no_salvage() {
        let app = Router::new()
            .route(
                "/v1/chat/completions",
                post(|| async {
                    let events = stream::iter(
                            vec![Ok::<_, std::convert::Infallible>(
                    Event::default().data(
                        json!({
                            "id": "chatcmpl-test",
                            "object": "chat.completion.chunk",
                            "created": 1234567890,
                            "model": "test-model",
                            "choices": [{
                                "index": 0,
                                "delta": { "role": "assistant", "content": "<summary>ok</summary>" },
                                "finish_reason": "stop"
                            }]
                        })
                        .to_string(),
                    ),
                )],
                        )
                        .chain(
                            stream::pending::<Result<Event, std::convert::Infallible>>(),
                        );
                    Sse::new(events)
                }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        let base_url = format!("http://{addr}/v1");
        let config = test_config(&base_url);
        let client = SamplingClient::new(config.clone()).unwrap();
        let input_surface = vec![
            ConversationItem::system("You are a helpful assistant."),
            ConversationItem::user("Summarize the conversation so far."),
        ];
        let result = generate_session_compact(
            input_surface,
            client,
            &config,
            std::time::Duration::from_millis(150),
            0,
            &tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(|_| {}),
        )
        .await;
        match result {
            Err(CompactFailure::Transient(err)) => {
                let data = err
                    .data
                    .as_ref()
                    .and_then(|d| d.as_str())
                    .unwrap_or_default();
                assert!(
                    data.contains("idle timeout"),
                    "expected an idle-timeout transient failure, got: {data}"
                );
            }
            Err(
                CompactFailure::Deterministic(_)
                | CompactFailure::ImageInputUnsupported(_)
                | CompactFailure::Cancelled,
            ) => {
                panic!("a stalled stream must be retryable (Transient), not Deterministic")
            }
            Ok(_) => {
                panic!(
                    "salvage removed: a completed-but-unterminated stream must error, not return a summary"
                )
            }
        }
        let _ = shutdown_tx.send(());
    }
    #[tokio::test]
    async fn substantial_partial_errors_no_salvage() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                let body = "x".repeat(2500);
                let events = stream::iter(vec![Ok::<_, std::convert::Infallible>(
                    Event::default().data(
                        json!({
                            "id": "chatcmpl-test",
                            "object": "chat.completion.chunk",
                            "created": 1234567890,
                            "model": "test-model",
                            "choices": [{
                                "index": 0,
                                "delta": { "role": "assistant", "content": body }
                            }]
                        })
                        .to_string(),
                    ),
                )])
                .chain(stream::pending::<Result<Event, std::convert::Infallible>>());
                Sse::new(events)
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        let base_url = format!("http://{addr}/v1");
        let config = test_config(&base_url);
        let client = SamplingClient::new(config.clone()).unwrap();
        let input_surface = vec![
            ConversationItem::system("You are a helpful assistant."),
            ConversationItem::user("Summarize the conversation so far."),
        ];
        let result = generate_session_compact(
            input_surface,
            client,
            &config,
            std::time::Duration::from_millis(150),
            0,
            &tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(|_| {}),
        )
        .await;
        match result {
            Err(CompactFailure::Transient(err)) => {
                let data = err
                    .data
                    .as_ref()
                    .and_then(|d| d.as_str())
                    .unwrap_or_default();
                assert!(
                    data.contains("idle timeout"),
                    "expected an idle-timeout transient failure, got: {data}"
                );
            }
            Err(
                CompactFailure::Deterministic(_)
                | CompactFailure::ImageInputUnsupported(_)
                | CompactFailure::Cancelled,
            ) => {
                panic!("a stalled stream must be retryable (Transient), not Deterministic")
            }
            Ok(_) => {
                panic!("salvage removed: a substantial partial must error, not be returned")
            }
        }
        let _ = shutdown_tx.send(());
    }
    #[tokio::test]
    async fn thin_partial_retries_on_stall() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                let events = stream::iter(vec![Ok::<_, std::convert::Infallible>(
                    Event::default().data(
                        json!({
                            "id": "chatcmpl-test",
                            "object": "chat.completion.chunk",
                            "created": 1234567890,
                            "model": "test-model",
                            "choices": [{
                                "index": 0,
                                "delta": { "role": "assistant", "content": "partial" }
                            }]
                        })
                        .to_string(),
                    ),
                )])
                .chain(stream::pending::<Result<Event, std::convert::Infallible>>());
                Sse::new(events)
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        let base_url = format!("http://{addr}/v1");
        let config = test_config(&base_url);
        let client = SamplingClient::new(config.clone()).unwrap();
        let input_surface = vec![
            ConversationItem::system("You are a helpful assistant."),
            ConversationItem::user("Summarize the conversation so far."),
        ];
        let result = generate_session_compact(
            input_surface,
            client,
            &config,
            std::time::Duration::from_millis(150),
            0,
            &tokio_util::sync::CancellationToken::new(),
            std::sync::Arc::new(|_| {}),
        )
        .await;
        match result {
            Err(CompactFailure::Transient(_)) => {}
            _ => panic!("a thin stalled body must retry (Transient), not salvage"),
        }
        let _ = shutdown_tx.send(());
    }
}

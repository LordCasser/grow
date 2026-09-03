//! Per-request streaming task.
//!
//! Spawned by the actor's `Submit` handler. Owns the retry loop and
//! consumes a Layer 2 stream from the matching backend transform.
//! Cancellation is cooperative via `CancellationToken`.

use std::pin::pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use sampling_types::{
    ConversationRequest, ConversationResponse, EmptyResponseContext, SamplingError, SentCredential,
    TokenUsage, error::Result as SamplingResult,
};

use crate::client::{ApiBackend, SamplingClient};
use crate::config::{RetryPolicy, SamplerConfig};
use crate::events::{SamplingErrorInfo, SamplingErrorKind, SamplingEvent};
use crate::handle::{AttemptScopeCapture, AttemptUsage, AttemptUsageSink};
use crate::metrics::InferenceLatencyStats;
use crate::retry::{
    self as retry_mod, RetryDecision, classify_error, clone_error, resolve_max_retries,
};
use crate::stream::responses::stream_responses_tracked;
use crate::stream::{stream_chat_completions, stream_messages};
use crate::types::RequestId;

/// Default per-chunk idle timeout when neither config nor caller
/// supplies one. Matches the shell's session-level default
/// (5 minutes -- long enough for cold-start reasoning, short enough
/// to detect dead streams before the user gives up).
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300;

pub(crate) type CompletionResult =
    Result<(ConversationResponse, InferenceLatencyStats), SamplingError>;

/// Result type for the `submit_and_collect` oneshot. Carries the rich
/// `SamplingError` so callers can inspect retryability, status code,
/// etc., without losing information through the
/// `SamplingErrorInfo` round trip.
/// Outcome of a single attempt within the retry loop.
enum AttemptOutcome {
    /// Stream emitted [`SamplingEvent::Completed`] with a non-empty
    /// response.
    Completed {
        response: Box<ConversationResponse>,
        metrics: InferenceLatencyStats,
    },
    /// Stream emitted [`SamplingEvent::Completed`] but the response
    /// was empty (no text, no tool calls). The retry loop treats this
    /// as a transient failure (the model returned reasoning-only or
    /// the stream was truncated). Metrics from the empty attempt are
    /// discarded; a successful retry produces fresh ones.
    Empty {
        context: EmptyResponseContext,
        usage: Option<TokenUsage>,
    },
    /// Stream emitted [`SamplingEvent::Failed`]. The captured raw
    /// error is what the retry loop classifies; if no rich error was
    /// captured (e.g. the failure was synthesised inside the L2
    /// transform), `error` was reconstructed from the
    /// [`SamplingErrorInfo`].
    Failed {
        error: SamplingError,
        usage: Option<TokenUsage>,
    },
    /// `cancel_token` fired mid-attempt. The retry loop bails out
    /// without further attempts.
    Cancelled,
    /// Failed to construct the underlying raw stream (e.g., HTTP
    /// connect error before any chunks arrive).
    InitFailed { error: SamplingError },
    /// Output was truncated by the configured max_tokens limit. Carries the
    /// partial response (with completed text/tool_calls; incomplete thinking
    /// and tool_use blocks discarded by the stream layer) for the session
    /// layer to persist and continue from.
    Truncated {
        partial_response: Box<ConversationResponse>,
        metrics: InferenceLatencyStats,
    },
    /// Generation hit the model's context window mid-turn (Anthropic
    /// model_context_window_exceeded). Session layer must compact, not continue.
    ContextWindowExceeded {
        partial_response: Box<ConversationResponse>,
        metrics: InferenceLatencyStats,
    },
    /// Anthropic pause_turn: assistant content is complete but the server-tool
    /// loop hit its iteration limit. Session layer resends this content to continue.
    PauseTurn {
        response: Box<ConversationResponse>,
        metrics: InferenceLatencyStats,
    },
}

struct AttemptRun {
    outcome: AttemptOutcome,
    scope: Option<String>,
    provider_started: bool,
}

/// Run a single sampling request to completion (or final failure).
///
/// Returns the request id so the actor can clean it up from
/// `active_requests` via [`tokio::task::JoinSet::join_next`].
pub(crate) async fn run_request_task(
    request_id: RequestId,
    request: ConversationRequest,
    config: SamplerConfig,
    retry_policy: RetryPolicy,
    event_tx: mpsc::UnboundedSender<SamplingEvent>,
    cancel_token: CancellationToken,
    completion_tx: Option<oneshot::Sender<CompletionResult>>,
    scope_capture: Option<AttemptScopeCapture>,
    usage_sink: Option<AttemptUsageSink>,
) -> RequestId {
    let mut completion_tx = completion_tx;
    let idle_timeout = Duration::from_secs(
        config
            .idle_timeout_secs
            .unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS),
    );
    let configured_max_retries = config.max_retries.or(Some(retry_policy.max_retries));
    let max_retries = if configured_max_retries == Some(0) {
        0
    } else {
        resolve_max_retries(configured_max_retries)
    };

    // Build the initial client. Configuration errors here are fatal
    // (no point retrying with the same broken config).
    let mut client = match SamplingClient::new(config.clone()) {
        Ok(c) => c,
        Err(err) => {
            emit_failed(&event_tx, &request_id, &err, None);
            send_completion(&mut completion_tx, Err(err));
            return request_id;
        }
    };

    let sampling_span = crate::sampling_log::request_span(
        &request_id,
        &config.model,
        &format!("{:?}", client.api_backend()),
        &config.base_url,
        &client.auth_info(),
    );
    if let Some(eff) = config.reasoning_effort {
        sampling_span.record("reasoning_effort", eff.as_str());
    }

    let request = request;
    let mut retry_count: u32 = 0;
    // Doom-loop recovery keeps its own resample budget, independent of the
    // transport/empty budget above.
    let doom_policy = (max_retries > 0)
        .then_some(config.doom_loop_recovery)
        .flatten();
    let doom_max_retries = doom_policy.map_or(0, |p| p.max_retries);
    let mut doom_retry_count: u32 = 0;
    let output_observed = Arc::new(AtomicBool::new(false));
    loop {
        if cancel_token.is_cancelled() {
            handle_cancellation(&event_tx, &request_id, &mut completion_tx);
            return request_id;
        }

        // Once the resample budget is spent, the attempt runs with the abort
        // disarmed so it can complete and be accepted as-is.
        let doom_check = doom_policy.filter(|_| doom_retry_count < doom_max_retries);
        let attempt = run_one_attempt(
            &client,
            request.clone(),
            request_id.clone(),
            idle_timeout,
            &event_tx,
            &cancel_token,
            doom_check,
            Arc::clone(&output_observed),
            scope_capture.as_ref(),
        )
        .instrument(sampling_span.clone())
        .await;
        let AttemptRun {
            outcome,
            scope: attempt_scope,
            provider_started,
        } = match attempt {
            Ok(attempt) => attempt,
            Err(error) => {
                let error = SamplingError::EventStreamError(format!(
                    "provider attempt admission failed: {error}"
                ));
                emit_failed(&event_tx, &request_id, &error, None);
                send_completion(&mut completion_tx, Err(error));
                return request_id;
            }
        };

        let outcome_usage = match &outcome {
            AttemptOutcome::Completed { response, .. }
            | AttemptOutcome::Truncated {
                partial_response: response,
                ..
            }
            | AttemptOutcome::ContextWindowExceeded {
                partial_response: response,
                ..
            }
            | AttemptOutcome::PauseTurn { response, .. } => response.usage.clone(),
            AttemptOutcome::Empty { usage, .. } | AttemptOutcome::Failed { usage, .. } => {
                usage.clone()
            }
            AttemptOutcome::Cancelled | AttemptOutcome::InitFailed { .. } => None,
        }
        .or_else(|| {
            // An unconditional image-capability rejection is a provider-side
            // request validation failure: inference never started, so the
            // attempt has exact zero usage. Keeping it out of the generic
            // unknown-usage path is what lets the session durably install an
            // ImageShadow and resubmit to the text-only model. The matcher is
            // intentionally strict; malformed-image, policy, transport, and
            // every other usage-less failure remain incomplete.
            let error = match &outcome {
                AttemptOutcome::Failed { error, .. } | AttemptOutcome::InitFailed { error } => {
                    Some(error)
                }
                _ => None,
            }?;
            let info = crate::events::SamplingErrorInfo::from(error);
            sampling_types::is_unconditional_image_input_unsupported(
                info.status_code,
                &info.message,
                request.image_count(),
            )
            .then(sampling_types::TokenUsage::default)
        });
        if provider_started
            && let Some(usage) = &outcome_usage
            && let Some(sink) = &usage_sink
            && let Err(error) = sink(AttemptUsage::Known {
                scope: attempt_scope.clone(),
                usage: usage.clone(),
            })
            .await
        {
            finish_usage_settlement_failure(&event_tx, &request_id, &mut completion_tx, error);
            return request_id;
        } else if provider_started
            && outcome_usage.is_none()
            && let Some(sink) = &usage_sink
            && let Err(error) = sink(AttemptUsage::Incomplete {
                scope: attempt_scope,
            })
            .await
        {
            finish_usage_settlement_failure(&event_tx, &request_id, &mut completion_tx, error);
            return request_id;
        }

        // Transport and empty-response retries are safe only while the
        // attempt has produced no model output. Doom-loop recovery has its
        // own discard-and-resample semantics and budget below.
        let effective_max_retries =
            if retry_policy.retry_only_before_output && output_observed.load(Ordering::Relaxed) {
                0
            } else {
                max_retries
            };

        match outcome {
            AttemptOutcome::Completed {
                response,
                mut metrics,
            } => {
                metrics.attempts = retry_count + doom_retry_count + 1;
                if let Some(policy) = doom_policy {
                    let confident = policy.confident_triggers(&response.doom_loop_signals);
                    if !confident.is_empty() {
                        tracing::warn!(
                            target: crate::sampling_log::TARGET,
                            triggers = ?confident,
                            attempt = doom_retry_count + 1,
                            outcome = "accepted_after_budget",
                            "doom-loop recovery: resample budget spent; accepting as-is"
                        );
                    }
                }
                // Surface token usage on the sampling span alongside effort.
                if let Some(usage) = response.usage.as_ref() {
                    sampling_span.record("output_tokens", usage.completion_tokens);
                    sampling_span.record("reasoning_tokens", usage.reasoning_tokens);
                }
                // Emit Completed only after the loop succeeds; the L2
                // stream's terminal event was suppressed by
                // `run_one_attempt`.
                let _ = event_tx.send(SamplingEvent::Completed {
                    request_id: request_id.clone(),
                    response: response.clone(),
                    metrics: metrics.clone(),
                });
                send_completion(&mut completion_tx, Ok((*response, metrics)));
                return request_id;
            }
            AttemptOutcome::Empty { context, .. } => {
                tracing::warn!(
                    target: crate::sampling_log::TARGET,
                    empty_response = true,
                    empty_reason = context.reason.as_str(),
                    had_reasoning = context.had_reasoning,
                    content_len = context.content_len,
                    tool_call_count = context.tool_call_count,
                    completion_tokens = context.completion_tokens.unwrap_or(0),
                    reasoning_tokens = context.reasoning_tokens.unwrap_or(0),
                    finish_reason = context.finish_reason_str(),
                    first_choice_seen = context.first_choice_seen,
                    model = %context.model,
                    "empty response from model: {reason} (retrying)",
                    reason = context.reason,
                );
                let err = SamplingError::EmptyResponse { context };
                if !apply_retry_decision(
                    &err,
                    &mut retry_count,
                    effective_max_retries,
                    &retry_policy,
                    &event_tx,
                    &request_id,
                    &mut client,
                    &config,
                    &cancel_token,
                    &mut completion_tx,
                    None,
                )
                .await
                {
                    return request_id;
                }
            }
            AttemptOutcome::Failed { error, usage } => {
                // Doom-loop resamples run on their own budget and never
                // consult the transport classifier, so no classifier change
                // can silently debit the transport budget for a doom failure.
                if let SamplingError::DoomLoopDetected { .. } = &error {
                    let backoff = retry_mod::doom_loop_backoff(doom_retry_count + 1);
                    doom_retry_count += 1;
                    tracing::warn!(
                        target: crate::sampling_log::TARGET,
                        reason = %error,
                        attempt = doom_retry_count,
                        max_retries = doom_max_retries,
                        outcome = "resampled",
                        "doom-loop recovery: discarding the poisoned attempt and resampling"
                    );
                    emit_retrying(
                        &event_tx,
                        &request_id,
                        doom_retry_count,
                        doom_max_retries,
                        &error,
                    );
                    if sleep_or_cancel(backoff, &cancel_token).await {
                        continue;
                    }
                    handle_cancellation(&event_tx, &request_id, &mut completion_tx);
                    return request_id;
                }
                if !apply_retry_decision(
                    &error,
                    &mut retry_count,
                    effective_max_retries,
                    &retry_policy,
                    &event_tx,
                    &request_id,
                    &mut client,
                    &config,
                    &cancel_token,
                    &mut completion_tx,
                    usage,
                )
                .await
                {
                    return request_id;
                }
            }
            AttemptOutcome::Cancelled => {
                handle_cancellation(&event_tx, &request_id, &mut completion_tx);
                return request_id;
            }
            AttemptOutcome::InitFailed { error } => {
                if !apply_retry_decision(
                    &error,
                    &mut retry_count,
                    effective_max_retries,
                    &retry_policy,
                    &event_tx,
                    &request_id,
                    &mut client,
                    &config,
                    &cancel_token,
                    &mut completion_tx,
                    None,
                )
                .await
                {
                    return request_id;
                }
            }
            // Truncation-class outcomes are facts the sampler reports, not
            // errors it retries: the same parameters would truncate again, so
            // skip the retry decision and surface the partial response. The
            // session layer tells the three kinds apart via
            // `response.stop_reason` (continue vs compact vs resend-to-continue).
            AttemptOutcome::Truncated {
                partial_response,
                metrics,
            }
            | AttemptOutcome::ContextWindowExceeded {
                partial_response,
                metrics,
            }
            | AttemptOutcome::PauseTurn {
                response: partial_response,
                metrics,
            } => {
                let mut metrics = metrics;
                metrics.attempts = retry_count + doom_retry_count + 1;
                // Surface token usage on the sampling span alongside effort.
                if let Some(usage) = partial_response.usage.as_ref() {
                    sampling_span.record("output_tokens", usage.completion_tokens);
                    sampling_span.record("reasoning_tokens", usage.reasoning_tokens);
                }
                tracing::info!(
                    target: crate::sampling_log::TARGET,
                    stop_reason = ?partial_response.stop_reason,
                    "turn ended by truncation-class stop_reason; partial response surfaced (no retry)"
                );
                // Emit Completed: the L2 stream's terminal event was
                // suppressed by `run_one_attempt`, so this is the only
                // terminal event the session sees.
                let _ = event_tx.send(SamplingEvent::Completed {
                    request_id: request_id.clone(),
                    response: partial_response.clone(),
                    metrics: metrics.clone(),
                });
                send_completion(&mut completion_tx, Ok((*partial_response, metrics)));
                return request_id;
            }
        }
    }
}

/// Apply a [`RetryDecision`]. Returns `true` if the loop should
/// continue, `false` if the request is finished (either fatal or
/// emit-to-session). Performs the side-effects of the decision:
/// sleeping, rebuilding the client, or emitting the `Retrying` event.
#[allow(clippy::too_many_arguments)]
async fn apply_retry_decision(
    err: &SamplingError,
    retry_count: &mut u32,
    max_retries: u32,
    retry_policy: &RetryPolicy,
    event_tx: &mpsc::UnboundedSender<SamplingEvent>,
    request_id: &RequestId,
    client: &mut SamplingClient,
    config: &SamplerConfig,
    cancel_token: &CancellationToken,
    completion_tx: &mut Option<oneshot::Sender<CompletionResult>>,
    terminal_usage: Option<TokenUsage>,
) -> bool {
    let rate_limit_threshold = if retry_policy.rate_limit_retry_threshold == 0 {
        retry_mod::RATE_LIMIT_RETRY_THRESHOLD
    } else {
        retry_policy.rate_limit_retry_threshold
    };
    let decision = classify_error(err, *retry_count, max_retries, rate_limit_threshold);

    match decision {
        RetryDecision::Retry { backoff } => {
            *retry_count += 1;
            emit_retrying(event_tx, request_id, *retry_count, max_retries, err);
            if sleep_or_cancel(backoff, cancel_token).await {
                true
            } else {
                handle_cancellation(event_tx, request_id, completion_tx);
                false
            }
        }
        RetryDecision::RetryWithBackoff { backoff, .. } => {
            *retry_count += 1;
            emit_retrying(event_tx, request_id, *retry_count, max_retries, err);
            if sleep_or_cancel(backoff, cancel_token).await {
                true
            } else {
                handle_cancellation(event_tx, request_id, completion_tx);
                false
            }
        }
        RetryDecision::RetryWithClientRebuild { backoff } => {
            *retry_count += 1;
            emit_retrying(event_tx, request_id, *retry_count, max_retries, err);
            if !sleep_or_cancel(backoff, cancel_token).await {
                handle_cancellation(event_tx, request_id, completion_tx);
                return false;
            }

            // Rebuild client with HTTP/1.1 fallback to escape poisoned
            // HTTP/2 connection pools.
            let mut http1_config = config.clone();
            http1_config.force_http1 = true;
            match SamplingClient::new(http1_config) {
                Ok(fresh) => {
                    *client = fresh;
                    tracing::info!("rebuilt sampling client with HTTP/1.1 fallback for retry");
                }
                Err(rebuild_err) => {
                    tracing::warn!(
                        error = %rebuild_err,
                        "failed to rebuild HTTP/1.1 client for retry; reusing existing client"
                    );
                }
            }
            true
        }
        RetryDecision::EmitToSession(emitted_err) => {
            emit_failed(event_tx, request_id, &emitted_err, terminal_usage);
            send_completion(completion_tx, Err(emitted_err));
            false
        }
        RetryDecision::Fatal(fatal_err) => {
            // Emit only on true budget exhaustion (hit the retry / rate-limit
            // cap), mirroring `classify_error`'s Fatal conditions — NOT on a
            // server `x-should-retry: false` or a non-retryable error, which
            // are also Fatal but are not "exhausted".
            let next_attempt = *retry_count + 1;
            let server_said_stop = matches!(err.should_retry_header(), Some(false));
            let budget_exhausted = !server_said_stop
                && if err.is_rate_limited() {
                    next_attempt >= max_retries.min(rate_limit_threshold)
                } else {
                    err.is_retryable() && next_attempt >= max_retries
                };
            if budget_exhausted {
                let exhausted_span = tracing::info_span!(
                    "http.retries_exhausted",
                    total_attempts = next_attempt as i64,
                    model = %config.model,
                    error = %err,
                    status_code = tracing::field::Empty,
                );
                let status_code = match err {
                    SamplingError::Api { status, .. } => Some(status.as_u16()),
                    SamplingError::Http(e) => e.status().map(|s| s.as_u16()),
                    _ => None,
                };
                if let Some(status) = status_code {
                    exhausted_span.record("status_code", status as i64);
                }
                exhausted_span.in_scope(|| {});
            }
            emit_failed(event_tx, request_id, &fatal_err, terminal_usage);
            send_completion(completion_tx, Err(fatal_err));
            false
        }
    }
}

async fn sleep_or_cancel(duration: Duration, cancel_token: &CancellationToken) -> bool {
    tokio::select! {
        biased;
        _ = cancel_token.cancelled() => false,
        _ = tokio::time::sleep(duration) => true,
    }
}

async fn capture_attempt_scope(
    capture: Option<&AttemptScopeCapture>,
) -> Result<Option<String>, String> {
    match capture {
        Some(capture) => capture().await,
        None => Ok(None),
    }
}

fn take_started_attempt_scope(slot: &Mutex<Option<Option<String>>>) -> Option<String> {
    slot.lock()
        .expect("attempt scope slot poisoned")
        .take()
        .expect("provider result requires a first-poll admission")
}

fn cancelled_attempt_from_scope_slot(slot: &Mutex<Option<Option<String>>>) -> AttemptRun {
    match slot.lock().expect("attempt scope slot poisoned").take() {
        Some(scope) => AttemptRun {
            outcome: AttemptOutcome::Cancelled,
            scope,
            provider_started: true,
        },
        None => AttemptRun {
            outcome: AttemptOutcome::Cancelled,
            scope: None,
            provider_started: false,
        },
    }
}

/// Run a single attempt: build the raw stream, drive it through the
/// matching L2 transform, and forward all non-terminal events to
/// `event_tx`. Captures the rich `SamplingError` from the underlying
/// raw stream so the retry loop can classify it accurately.
///
/// `doom_check` is the doom-loop policy while the resample budget lasts;
/// `None` disarms the mid-stream abort and the terminal confidence check so
/// the attempt completes and its response can be accepted.
#[allow(clippy::too_many_arguments)]
async fn run_one_attempt(
    client: &SamplingClient,
    request: ConversationRequest,
    request_id: RequestId,
    idle_timeout: Duration,
    event_tx: &mpsc::UnboundedSender<SamplingEvent>,
    cancel_token: &CancellationToken,
    doom_check: Option<sampling_types::DoomLoopRecoveryPolicy>,
    output_observed: Arc<AtomicBool>,
    scope_capture: Option<&AttemptScopeCapture>,
) -> Result<AttemptRun, String> {
    // The provider-open future can be dropped by the cancellation branch
    // after an earlier poll admitted Goal usage but before it returns a raw
    // stream. Keep that lease outside the selected future so cancellation can
    // still settle the exact scope as incomplete. `None` means never polled;
    // `Some(None)` means polled with no active Goal.
    let scope_slot: Arc<Mutex<Option<Option<String>>>> = Arc::new(Mutex::new(None));
    match client.api_backend() {
        ApiBackend::ChatCompletions => {
            let branch_scope = Arc::clone(&scope_slot);
            let opened = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    return Ok(cancelled_attempt_from_scope_slot(&scope_slot));
                },
                result = async {
                    let scope = capture_attempt_scope(scope_capture).await?;
                    *branch_scope.lock().expect("attempt scope slot poisoned") = Some(scope);
                    let opened = client.conversation_stream(request).await;
                    Ok::<_, String>(opened)
                } => result?,
            };
            let scope = take_started_attempt_scope(&scope_slot);
            let (raw, metadata) = match opened {
                Ok(pair) => pair,
                Err(error) => {
                    return Ok(AttemptRun {
                        outcome: AttemptOutcome::InitFailed { error },
                        scope,
                        provider_started: true,
                    });
                }
            };
            let (teed, captured) = tee_errors(raw);
            let l2 = stream_chat_completions(teed, metadata, request_id.clone(), idle_timeout);
            Ok(AttemptRun {
                outcome: drive_l2(
                    l2,
                    request_id,
                    event_tx,
                    cancel_token,
                    captured,
                    None,
                    output_observed,
                )
                .await,
                scope,
                provider_started: true,
            })
        }
        ApiBackend::Responses => {
            let branch_scope = Arc::clone(&scope_slot);
            let opened = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    return Ok(cancelled_attempt_from_scope_slot(&scope_slot));
                },
                result = async {
                    let scope = capture_attempt_scope(scope_capture).await?;
                    *branch_scope.lock().expect("attempt scope slot poisoned") = Some(scope);
                    let opened = client.conversation_stream_responses(request).await;
                    Ok::<_, String>(opened)
                } => result?,
            };
            let scope = take_started_attempt_scope(&scope_slot);
            let (raw, metadata, doom_loop) = match opened {
                Ok(parts) => parts,
                Err(error) => {
                    return Ok(AttemptRun {
                        outcome: AttemptOutcome::InitFailed { error },
                        scope,
                        provider_started: true,
                    });
                }
            };
            if doom_check.is_none()
                && let Some(collector) = &doom_loop
            {
                collector.disarm_abort();
            }
            let (teed, captured) = tee_errors(raw);
            let l2 = stream_responses_tracked(
                teed,
                metadata,
                request_id.clone(),
                idle_timeout,
                doom_loop,
                Arc::clone(&output_observed),
            );
            Ok(AttemptRun {
                outcome: drive_l2(
                    l2,
                    request_id,
                    event_tx,
                    cancel_token,
                    captured,
                    doom_check,
                    output_observed,
                )
                .await,
                scope,
                provider_started: true,
            })
        }
        ApiBackend::Messages => {
            let branch_scope = Arc::clone(&scope_slot);
            let opened = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    return Ok(cancelled_attempt_from_scope_slot(&scope_slot));
                },
                result = async {
                    let scope = capture_attempt_scope(scope_capture).await?;
                    *branch_scope.lock().expect("attempt scope slot poisoned") = Some(scope);
                    let opened = client.conversation_stream_messages(request).await;
                    Ok::<_, String>(opened)
                } => result?,
            };
            let scope = take_started_attempt_scope(&scope_slot);
            let (raw, metadata) = match opened {
                Ok(pair) => pair,
                Err(error) => {
                    return Ok(AttemptRun {
                        outcome: AttemptOutcome::InitFailed { error },
                        scope,
                        provider_started: true,
                    });
                }
            };
            let (teed, captured) = tee_errors(raw);
            let l2 = stream_messages(teed, metadata, request_id.clone(), idle_timeout);
            Ok(AttemptRun {
                outcome: drive_l2(
                    l2,
                    request_id,
                    event_tx,
                    cancel_token,
                    captured,
                    None,
                    output_observed,
                )
                .await,
                scope,
                provider_started: true,
            })
        }
    }
}

/// Captured-error cell shared between the tee adapter and the
/// per-request task.
type ErrorCell = Arc<Mutex<Option<SamplingError>>>;

/// Wrap a raw chunk stream so its first error is captured into a
/// shared cell. The wrapped stream still yields the original
/// `Result<T, SamplingError>` items unchanged so the L2 transform sees
/// them and converts them to `SamplingErrorInfo` for events.
fn tee_errors<'a, T: Send + 'a>(
    raw: BoxStream<'a, SamplingResult<T>>,
) -> (BoxStream<'a, SamplingResult<T>>, ErrorCell) {
    let cell: ErrorCell = Arc::new(Mutex::new(None));
    let cell_clone = Arc::clone(&cell);
    let teed = raw
        .map(move |item| {
            if let Err(ref e) = item
                && let Ok(mut guard) = cell_clone.lock()
                && guard.is_none()
            {
                // Capture only the first error -- subsequent errors
                // on a torn-down stream are usually secondary effects
                // of the same disconnect.
                *guard = Some(clone_error(e));
            }
            item
        })
        .boxed();
    (teed, cell)
}

/// Drive an L2 event stream: forward non-terminal events to
/// `event_tx`, watch `cancel_token`, return `AttemptOutcome` based on
/// the terminal event (or cancellation). `doom_check`, when set, turns a
/// completed response carrying confident doom-loop signals into a
/// retryable failure (belt-and-braces behind the mid-stream abort).
#[allow(clippy::too_many_arguments)]
async fn drive_l2(
    l2: impl futures_util::Stream<Item = SamplingEvent>,
    request_id: RequestId,
    event_tx: &mpsc::UnboundedSender<SamplingEvent>,
    cancel_token: &CancellationToken,
    captured: ErrorCell,
    doom_check: Option<sampling_types::DoomLoopRecoveryPolicy>,
    output_observed: Arc<AtomicBool>,
) -> AttemptOutcome {
    let mut l2 = pin!(l2);
    loop {
        tokio::select! {
            biased;
            next = l2.next() => match next {
                Some(SamplingEvent::Completed { response, metrics, .. }) => {
                    if response_has_observed_output(&response) {
                        output_observed.store(true, Ordering::Relaxed);
                    }
                    // Doom outranks the truncation/empty classes: a confident
                    // loop poisons the attempt whatever else it looks like.
                    if let Some(policy) = doom_check {
                        let triggers = policy.confident_triggers(&response.doom_loop_signals);
                        if !triggers.is_empty() {
                            return AttemptOutcome::Failed {
                                error: SamplingError::DoomLoopDetected {
                                    triggers,
                                    aborted_at_chunk: None,
                                },
                                usage: response.usage.clone(),
                            };
                        }
                    }
                    // Truncation-class stop reasons are facts, not errors:
                    // classify them for the retry loop so the partial
                    // response survives to the session layer. `Some(..)`
                    // branches move `response`; the `_` arm keeps it for the
                    // empty/Completed checks below.
                    match response.stop_reason {
                        Some(sampling_types::StopReason::Length) => {
                            return AttemptOutcome::Truncated {
                                partial_response: response,
                                metrics,
                            };
                        }
                        Some(sampling_types::StopReason::ModelContextWindowExceeded) => {
                            return AttemptOutcome::ContextWindowExceeded {
                                partial_response: response,
                                metrics,
                            };
                        }
                        Some(sampling_types::StopReason::PauseTurn) => {
                            return AttemptOutcome::PauseTurn { response, metrics };
                        }
                        _ => {}
                    }
                    // A content-filtered turn (Anthropic refusal, OpenAI
                    // content_filter stop reason) is legitimately content-less and
                    // deterministic — resampling it would retry-storm.
                    let content_filtered = response.stop_reason
                        == Some(sampling_types::StopReason::ContentFilter);
                    if !content_filtered && let Some(reason) = response.empty_reason() {
                        let context = build_empty_context(reason, &response);
                        let usage = response.usage.clone();
                        return AttemptOutcome::Empty { context, usage };
                    }
                    return AttemptOutcome::Completed { response, metrics };
                }
                Some(SamplingEvent::Failed { error: info, .. }) => {
                    let raw = captured
                        .lock()
                        .ok()
                        .and_then(|mut g| g.take());
                    let error = raw.unwrap_or_else(|| synthesize_from_info(&info));
                    return AttemptOutcome::Failed { error, usage: info.usage };
                }
                Some(other) => {
                    if matches!(
                        other,
                        SamplingEvent::FirstToken { .. }
                            | SamplingEvent::ChannelToken { .. }
                            | SamplingEvent::ToolCallDelta { .. }
                    ) {
                        output_observed.store(true, Ordering::Relaxed);
                    }
                    let _ = event_tx.send(retag(other, &request_id));
                    // Give a buffered terminal priority over cancellation so
                    // its provider-reported usage is never discarded.  For a
                    // non-terminal backlog, however, observe cancellation
                    // immediately after one item to keep Stop latency bounded.
                    if cancel_token.is_cancelled() {
                        return AttemptOutcome::Cancelled;
                    }
                }
                None => {
                    // L2 streams always terminate with Completed or
                    // Failed; reaching None means the producer was
                    // dropped without termination -- treat as a
                    // synthetic transport error.
                    return AttemptOutcome::Failed {
                        error: SamplingError::EventStreamError(
                            "stream dropped without terminal event".to_string(),
                        ),
                        usage: None,
                    };
                }
            },
            _ = cancel_token.cancelled() => {
                return AttemptOutcome::Cancelled;
            }
        }
    }
}

/// A terminal frame can carry the first complete output without any preceding
/// delta. Treat response content, tool activity, reasoning, or billed output
/// tokens as observed so a later transient classification cannot replay it.
fn response_has_observed_output(response: &ConversationResponse) -> bool {
    response
        .assistant()
        .is_some_and(|assistant| !assistant.content.is_empty() || !assistant.tool_calls.is_empty())
        || response.reasoning_items().any(|reasoning| {
            !reasoning.summary.is_empty()
                || reasoning.content.is_some()
                || reasoning.encrypted_content.is_some()
        })
        || response.backend_tool_items().next().is_some()
        || response
            .usage
            .as_ref()
            .is_some_and(|usage| usage.completion_tokens > 0 || usage.reasoning_tokens > 0)
}

/// Re-tag a forwarded event with the canonical request_id. The L2
/// transform tags events with the id we passed in, so this is
/// usually a no-op; keeping the helper makes the data-flow explicit.
fn retag(event: SamplingEvent, _request_id: &RequestId) -> SamplingEvent {
    event
}

/// Reconstruct a [`SamplingError`] from a [`SamplingErrorInfo`] when
/// the L2 transform fired a synthesised Failed event (idle timeout,
/// `ResponseFailed`, server error event) and there is no captured raw
/// error in the cell.
fn synthesize_from_info(info: &SamplingErrorInfo) -> SamplingError {
    crate::events::sampling_error_from_info(info)
}

/// Build an [`EmptyResponseContext`] from a completed-but-empty response.
fn build_empty_context(
    reason: sampling_types::EmptyReason,
    response: &ConversationResponse,
) -> EmptyResponseContext {
    let had_reasoning = response
        .reasoning_items()
        .any(|r| !r.summary.is_empty() || r.content.is_some() || r.encrypted_content.is_some());
    let (content_len, tool_call_count, model, first_choice_seen) = match response.assistant() {
        Some(a) => (
            a.content.len(),
            a.tool_calls.len(),
            a.model_id.clone().unwrap_or_default(),
            // If model_id is set, the L2 saw at least one choice.
            a.model_id.is_some(),
        ),
        None => (0, 0, String::new(), false),
    };

    let finish_reason = response.raw_stop_reason.clone().or_else(|| {
        response
            .stop_reason
            .map(|stop_reason| stop_reason.as_str().to_owned())
    });
    let (completion_tokens, reasoning_tokens, prompt_tokens) = response
        .usage
        .as_ref()
        .map(|u| {
            (
                Some(u.completion_tokens),
                Some(u.reasoning_tokens),
                Some(u.prompt_tokens),
            )
        })
        .unwrap_or((None, None, None));

    EmptyResponseContext {
        reason,
        had_reasoning,
        content_len,
        tool_call_count,
        finish_reason,
        completion_tokens,
        reasoning_tokens,
        prompt_tokens,
        model,
        first_choice_seen,
    }
}

fn emit_failed(
    event_tx: &mpsc::UnboundedSender<SamplingEvent>,
    request_id: &RequestId,
    err: &SamplingError,
    usage: Option<TokenUsage>,
) {
    let mut info = SamplingErrorInfo::from(err);
    info.usage = usage;
    let _ = event_tx.send(SamplingEvent::Failed {
        request_id: request_id.clone(),
        error: info,
    });
}

fn emit_retrying(
    event_tx: &mpsc::UnboundedSender<SamplingEvent>,
    request_id: &RequestId,
    attempt: u32,
    max_retries: u32,
    err: &SamplingError,
) {
    let info = SamplingErrorInfo::from(err);
    let _ = event_tx.send(SamplingEvent::Retrying {
        request_id: request_id.clone(),
        attempt,
        max_retries,
        kind: info.kind,
        reason: err.to_string(),
        doom_loop_triggers: info.doom_loop_triggers,
        doom_loop_aborted_at_chunk: info.doom_loop_aborted_at_chunk,
    });
}

fn handle_cancellation(
    event_tx: &mpsc::UnboundedSender<SamplingEvent>,
    request_id: &RequestId,
    completion_tx: &mut Option<oneshot::Sender<CompletionResult>>,
) {
    // No status code, no upstream API error -- this is a client-side
    // termination. Use kind=Api so consumers that switch on kind have
    // a sensible default; the message clearly identifies it.
    let info = SamplingErrorInfo {
        kind: SamplingErrorKind::Api,
        status_code: None,
        message: "request cancelled".to_string(),
        is_retryable: false,
        retry_after_secs: None,
        model_metadata: None,
        empty_response_context: None,
        doom_loop_triggers: None,
        doom_loop_aborted_at_chunk: None,
        credential: SentCredential::Unknown,
        usage: None,
    };
    let _ = event_tx.send(SamplingEvent::Failed {
        request_id: request_id.clone(),
        error: info,
    });
    send_completion(
        completion_tx,
        Err(SamplingError::auth_unknown("request cancelled")),
    );
}

fn finish_usage_settlement_failure(
    event_tx: &mpsc::UnboundedSender<SamplingEvent>,
    request_id: &RequestId,
    completion_tx: &mut Option<oneshot::Sender<CompletionResult>>,
    error: String,
) {
    let error =
        SamplingError::EventStreamError(format!("attempt usage settlement failed: {error}"));
    // RequestStarted must always have a sampler terminal. Shell's stream
    // drain barrier consumes this event; completing only the oneshot would
    // otherwise manufacture a fixed five-second timeout during persistence
    // failure or teardown.
    emit_failed(event_tx, request_id, &error, None);
    send_completion(completion_tx, Err(error));
}

fn send_completion(
    completion_tx: &mut Option<oneshot::Sender<CompletionResult>>,
    result: CompletionResult,
) {
    if let Some(tx) = completion_tx.take() {
        let _ = tx.send(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    #[tokio::test]
    async fn attempt_scope_capture_is_evaluated_once_per_attempt() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls_for_capture = Arc::clone(&calls);
        let capture: AttemptScopeCapture = Arc::new(move || {
            let n = calls_for_capture.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move { Ok(Some(format!("scope-{n}"))) })
        });

        assert_eq!(
            capture_attempt_scope(Some(&capture)).await,
            Ok(Some("scope-0".into()))
        );
        assert_eq!(
            capture_attempt_scope(Some(&capture)).await,
            Ok(Some("scope-1".into()))
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn pre_cancelled_attempt_never_enters_provider_usage_scope() {
        let client = SamplingClient::new(SamplerConfig {
            base_url: "http://127.0.0.1:9".into(),
            model: "test-model".into(),
            api_backend: ApiBackend::Responses,
            ..SamplerConfig::default()
        })
        .unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls_for_capture = Arc::clone(&calls);
        let capture: AttemptScopeCapture = Arc::new(move || {
            calls_for_capture.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { Ok(Some("must-not-be-created".into())) })
        });
        let cancel = CancellationToken::new();
        cancel.cancel();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();

        let attempt = run_one_attempt(
            &client,
            ConversationRequest::default(),
            RequestId::from("pre-cancelled"),
            Duration::from_secs(1),
            &event_tx,
            &cancel,
            None,
            Arc::new(AtomicBool::new(false)),
            Some(&capture),
        )
        .await
        .unwrap();

        assert!(matches!(attempt.outcome, AttemptOutcome::Cancelled));
        assert!(!attempt.provider_started);
        assert!(attempt.scope.is_none());
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn cancel_during_admission_never_starts_provider_for_every_backend() {
        for backend in [
            ApiBackend::ChatCompletions,
            ApiBackend::Responses,
            ApiBackend::Messages,
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = SamplingClient::new(SamplerConfig {
                base_url: format!("http://{}", listener.local_addr().unwrap()),
                model: "test-model".into(),
                api_backend: backend,
                ..SamplerConfig::default()
            })
            .unwrap();
            let (entered_tx, mut entered_rx) = mpsc::unbounded_channel();
            let capture: AttemptScopeCapture = Arc::new(move || {
                let entered_tx = entered_tx.clone();
                Box::pin(async move {
                    entered_tx.send(()).unwrap();
                    std::future::pending().await
                })
            });
            let cancel = CancellationToken::new();
            let (event_tx, _event_rx) = mpsc::unbounded_channel();
            let mut attempt = Box::pin(run_one_attempt(
                &client,
                ConversationRequest::default(),
                RequestId::from("cancel-admission"),
                Duration::from_secs(1),
                &event_tx,
                &cancel,
                None,
                Arc::new(AtomicBool::new(false)),
                Some(&capture),
            ));
            tokio::select! {
                biased;
                _ = &mut attempt => panic!("admission must wait"),
                _ = entered_rx.recv() => {}
            }
            assert!(
                tokio::time::timeout(Duration::from_millis(10), listener.accept())
                    .await
                    .is_err()
            );
            cancel.cancel();
            let attempt = tokio::time::timeout(Duration::from_secs(1), attempt)
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(attempt.outcome, AttemptOutcome::Cancelled));
            assert!(!attempt.provider_started);
            assert!(attempt.scope.is_none());
        }
    }

    #[tokio::test]
    async fn cancel_during_stream_open_settles_the_first_poll_scope_for_every_backend() {
        for backend in [
            ApiBackend::ChatCompletions,
            ApiBackend::Responses,
            ApiBackend::Messages,
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (_socket, _) = listener.accept().await.unwrap();
                std::future::pending::<()>().await;
            });
            let config = SamplerConfig {
                api_key: Some("test-key".into()),
                base_url: format!("http://{address}"),
                model: "test-model".into(),
                api_backend: backend.clone(),
                max_retries: Some(0),
                ..SamplerConfig::default()
            };
            let (scope_tx, mut scope_rx) = mpsc::unbounded_channel();
            let capture: AttemptScopeCapture = Arc::new(move || {
                let _ = scope_tx.send(());
                Box::pin(async { Ok(Some("scope-on-wire".into())) })
            });
            let (usage_tx, mut usage_rx) = mpsc::unbounded_channel();
            let sink: AttemptUsageSink = Arc::new(move |usage| {
                let usage_tx = usage_tx.clone();
                Box::pin(async move { usage_tx.send(usage).map_err(|error| error.to_string()) })
            });
            let (event_tx, _event_rx) = mpsc::unbounded_channel();
            let cancel = CancellationToken::new();
            let task_cancel = cancel.clone();
            let task = tokio::spawn(run_request_task(
                RequestId::from(format!("cancel-open-{backend:?}")),
                ConversationRequest {
                    model: Some("test-model".into()),
                    ..ConversationRequest::default()
                },
                config,
                RetryPolicy::default(),
                event_tx,
                task_cancel,
                None,
                Some(capture),
                Some(sink),
            ));

            tokio::time::timeout(Duration::from_secs(2), scope_rx.recv())
                .await
                .expect("provider future was first-polled")
                .expect("scope notification channel remains open");
            cancel.cancel();
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .expect("cancel must stop stream-open")
                .expect("request task must not panic");

            assert!(matches!(
                usage_rx.recv().await,
                Some(AttemptUsage::Incomplete { scope: Some(scope) })
                    if scope == "scope-on-wire"
            ));
            server.abort();
        }
    }

    #[tokio::test]
    async fn usage_settlement_failure_emits_terminal_before_completion() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (completion_tx, completion_rx) = oneshot::channel();
        let mut completion_tx = Some(completion_tx);
        let request_id = RequestId::from("usage-settlement-failure");

        finish_usage_settlement_failure(
            &event_tx,
            &request_id,
            &mut completion_tx,
            "timeline unavailable".into(),
        );

        assert!(matches!(
            event_rx.recv().await,
            Some(SamplingEvent::Failed { request_id: id, .. }) if id == request_id
        ));
        assert!(completion_rx.await.expect("completion sent").is_err());
    }

    #[test]
    fn synthesize_idle_timeout_extracts_elapsed_secs() {
        let info = SamplingErrorInfo {
            kind: SamplingErrorKind::IdleTimeout,
            status_code: None,
            message: "inference idle timeout after 240s with no chunks".to_string(),
            is_retryable: false,
            retry_after_secs: None,
            model_metadata: None,
            empty_response_context: None,
            doom_loop_triggers: None,
            doom_loop_aborted_at_chunk: None,
            credential: SentCredential::Unknown,
            usage: None,
        };
        let err = synthesize_from_info(&info);
        match err {
            SamplingError::IdleTimeout { elapsed_secs } => assert_eq!(elapsed_secs, 240),
            other => panic!("expected IdleTimeout, got {other:?}"),
        }
    }

    #[test]
    fn synthesize_api_500_round_trips() {
        let info = SamplingErrorInfo {
            kind: SamplingErrorKind::Api,
            status_code: Some(500),
            message: "boom".to_string(),
            is_retryable: true,
            retry_after_secs: None,
            model_metadata: None,
            empty_response_context: None,
            doom_loop_triggers: None,
            doom_loop_aborted_at_chunk: None,
            credential: SentCredential::Unknown,
            usage: None,
        };
        let err = synthesize_from_info(&info);
        match err {
            SamplingError::Api {
                status, message, ..
            } => {
                assert_eq!(status.as_u16(), 500);
                assert_eq!(message, "boom");
            }
            other => panic!("expected Api, got {other:?}"),
        }
    }

    #[test]
    fn synthesize_rate_limited_preserves_retry_after() {
        let info = SamplingErrorInfo {
            kind: SamplingErrorKind::RateLimited,
            status_code: Some(429),
            message: "slow down".to_string(),
            is_retryable: true,
            retry_after_secs: Some(7),
            model_metadata: None,
            empty_response_context: None,
            doom_loop_triggers: None,
            doom_loop_aborted_at_chunk: None,
            credential: SentCredential::Unknown,
            usage: None,
        };
        let err = synthesize_from_info(&info);
        match err {
            SamplingError::Api {
                status,
                retry_after_secs,
                ..
            } => {
                assert_eq!(status.as_u16(), 429);
                assert_eq!(retry_after_secs, Some(7));
            }
            other => panic!("expected Api(429), got {other:?}"),
        }
    }

    #[test]
    fn synthesize_serialization_stays_serialization() {
        // Round-trip a REAL error's Display so a Display-template rewording
        // cannot silently reintroduce double-prefixing.
        let original = SamplingError::Serialization(
            serde_json::from_str::<i32>("missing field `delta`").unwrap_err(),
        );
        let info = SamplingErrorInfo::from(&original);
        let err = synthesize_from_info(&info);
        assert!(
            matches!(err, SamplingError::Serialization(_)),
            "expected Serialization, got {err:?}"
        );
        assert!(!err.is_retryable());
        assert_eq!(
            err.to_string(),
            info.message,
            "rebuilt Display must round-trip without double-prefixing"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn retry_sleep_returns_immediately_on_cancellation() {
        let cancel_token = CancellationToken::new();
        let sleeper = sleep_or_cancel(Duration::from_secs(120), &cancel_token);
        tokio::pin!(sleeper);

        cancel_token.cancel();
        assert!(!sleeper.await);
    }

    #[tokio::test(start_paused = true)]
    async fn retry_decision_cancellation_emits_terminal_cancel() {
        let cancel_token = CancellationToken::new();
        cancel_token.cancel();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let (completion_tx, completion_rx) = oneshot::channel();
        let mut completion_tx = Some(completion_tx);
        let mut retry_count = 0;
        let config = SamplerConfig {
            base_url: "http://localhost".into(),
            model: "test-model".into(),
            ..Default::default()
        };
        let mut client = SamplingClient::new(config.clone()).expect("test client");
        let error = SamplingError::EventStreamError("retry me".into());

        let should_continue = apply_retry_decision(
            &error,
            &mut retry_count,
            2,
            &RetryPolicy::default(),
            &event_tx,
            &RequestId::from("cancel-backoff"),
            &mut client,
            &config,
            &cancel_token,
            &mut completion_tx,
            None,
        )
        .await;

        assert!(!should_continue);
        assert!(matches!(
            event_rx.recv().await,
            Some(SamplingEvent::Retrying { .. })
        ));
        assert!(matches!(
            event_rx.recv().await,
            Some(SamplingEvent::Failed { .. })
        ));
        assert!(completion_rx.await.expect("completion sent").is_err());
    }

    #[tokio::test]
    async fn tee_captures_first_error_only() {
        let items: Vec<SamplingResult<u32>> = vec![
            Ok(1),
            Err(SamplingError::EventStreamError("first".into())),
            Err(SamplingError::EventStreamError("second".into())),
        ];
        let raw = stream::iter(items).boxed();
        let (mut teed, cell) = tee_errors(raw);
        while teed.next().await.is_some() {}
        let captured = cell.lock().unwrap().take().expect("error captured");
        match captured {
            SamplingError::EventStreamError(msg) => assert_eq!(msg, "first"),
            other => panic!("expected EventStreamError, got {other:?}"),
        }
    }

    #[test]
    fn empty_response_diagnostics_preserve_unknown_wire_finish_reason() {
        let response = ConversationResponse {
            items: vec![
                sampling_types::ConversationItem::Reasoning(
                    sampling_types::synthesized_reasoning_item("internal reasoning"),
                ),
                sampling_types::ConversationItem::assistant(""),
            ],
            stop_reason: Some(sampling_types::StopReason::Stop),
            usage: None,
            cost_usd_ticks: None,
            message_chunks_emitted: 0,
            doom_loop_signals: Vec::new(),
            stop_message: None,
            message_id: None,
            raw_stop_reason: Some("unexpected_state".into()),
            stop_sequence: None,
        };

        let context = build_empty_context(sampling_types::EmptyReason::ReasoningOnly, &response);
        assert!(context.had_reasoning);
        assert_eq!(context.finish_reason.as_deref(), Some("unexpected_state"));
    }

    #[test]
    fn terminal_response_output_classifier_covers_content_reasoning_and_tools() {
        let response = |items| ConversationResponse {
            items,
            stop_reason: Some(sampling_types::StopReason::Stop),
            usage: None,
            cost_usd_ticks: None,
            message_chunks_emitted: 0,
            doom_loop_signals: Vec::new(),
            stop_message: None,
            message_id: None,
            raw_stop_reason: None,
            stop_sequence: None,
        };

        assert!(!response_has_observed_output(&response(vec![
            sampling_types::ConversationItem::assistant("")
        ])));
        assert!(response_has_observed_output(&response(vec![
            sampling_types::ConversationItem::assistant("terminal-only output")
        ])));
        assert!(response_has_observed_output(&response(vec![
            sampling_types::ConversationItem::Reasoning(
                sampling_types::synthesized_reasoning_item("terminal-only reasoning"),
            )
        ])));
        assert!(response_has_observed_output(&response(vec![
            sampling_types::ConversationItem::assistant_tool_calls(vec![
                sampling_types::ToolCall {
                    id: "call-1".into(),
                    name: "read_file".into(),
                    arguments: "{}".into(),
                },
            ])
        ])));
    }

    // ── drive_l2 stop_reason classification ──────────────────────────

    use sampling_types::{
        AssistantItem, ConversationItem, DoomLoopRecoveryPolicy, DoomLoopSignal, StopReason,
    };
    use std::time::Instant;

    /// Drive `drive_l2` over a single `Completed` event carrying the given
    /// stop_reason; returns the outcome.
    async fn drive_l2_with_stop_reason(
        stop_reason: Option<StopReason>,
        doom_signals: Vec<DoomLoopSignal>,
        doom_check: Option<DoomLoopRecoveryPolicy>,
    ) -> (AttemptOutcome, bool) {
        let request_id = RequestId::from("drive-l2-test");
        let response = ConversationResponse {
            items: vec![ConversationItem::Assistant(AssistantItem {
                content: "partial answer".into(),
                tool_calls: vec![],
                model_id: Some("test-model".into()),
                model_fingerprint: None,
                reasoning_effort: None,
            })],
            stop_reason,
            usage: None,
            cost_usd_ticks: None,
            message_chunks_emitted: 0,
            doom_loop_signals: doom_signals,
            stop_message: None,
            message_id: None,
            raw_stop_reason: None,
            stop_sequence: None,
        };
        let metrics = InferenceLatencyStats::from_timestamps(Instant::now(), &[], Instant::now());
        let l2 = stream::iter(vec![SamplingEvent::Completed {
            request_id: request_id.clone(),
            response: Box::new(response),
            metrics,
        }]);
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();
        let captured: ErrorCell = Arc::new(Mutex::new(None));
        let output_observed = Arc::new(AtomicBool::new(false));
        let outcome = drive_l2(
            l2,
            request_id,
            &event_tx,
            &cancel_token,
            captured,
            doom_check,
            Arc::clone(&output_observed),
        )
        .await;
        (outcome, output_observed.load(Ordering::Relaxed))
    }

    #[tokio::test]
    async fn drive_l2_length_maps_to_truncated_outcome() {
        let (outcome, _) = drive_l2_with_stop_reason(Some(StopReason::Length), vec![], None).await;
        match outcome {
            AttemptOutcome::Truncated {
                partial_response, ..
            } => {
                // The partial response must survive intact for the session
                // layer to persist and continue from.
                assert_eq!(partial_response.stop_reason, Some(StopReason::Length));
                let a = partial_response
                    .assistant()
                    .expect("assistant item present");
                assert_eq!(a.content.as_ref(), "partial answer");
            }
            _ => panic!("expected Truncated outcome"),
        }
    }

    #[tokio::test]
    async fn drive_l2_context_window_exceeded_outcome() {
        let (outcome, _) =
            drive_l2_with_stop_reason(Some(StopReason::ModelContextWindowExceeded), vec![], None)
                .await;
        match outcome {
            AttemptOutcome::ContextWindowExceeded {
                partial_response, ..
            } => {
                assert_eq!(
                    partial_response.stop_reason,
                    Some(StopReason::ModelContextWindowExceeded)
                );
            }
            _ => panic!("expected ContextWindowExceeded outcome"),
        }
    }

    #[tokio::test]
    async fn drive_l2_pause_turn_outcome() {
        let (outcome, _) =
            drive_l2_with_stop_reason(Some(StopReason::PauseTurn), vec![], None).await;
        match outcome {
            AttemptOutcome::PauseTurn { response, .. } => {
                assert_eq!(response.stop_reason, Some(StopReason::PauseTurn));
            }
            _ => panic!("expected PauseTurn outcome"),
        }
    }

    #[tokio::test]
    async fn drive_l2_untouched_stop_reasons_complete() {
        for stop in [
            Some(StopReason::Stop),
            Some(StopReason::ToolCalls),
            Some(StopReason::ContentFilter),
            None,
        ] {
            let label = format!("{stop:?}");
            let (outcome, _) = drive_l2_with_stop_reason(stop, vec![], None).await;
            assert!(
                matches!(outcome, AttemptOutcome::Completed { .. }),
                "{label}: expected Completed outcome"
            );
        }
    }

    #[tokio::test]
    async fn drive_l2_doom_check_outranks_truncation() {
        let signals = vec![DoomLoopSignal::parse("tail_repetition:4@thinking")];
        let (outcome, _) = drive_l2_with_stop_reason(
            Some(StopReason::Length),
            signals,
            Some(DoomLoopRecoveryPolicy::default()),
        )
        .await;
        match outcome {
            AttemptOutcome::Failed {
                error: SamplingError::DoomLoopDetected { .. },
                ..
            } => {}
            _ => panic!("expected Failed(DoomLoopDetected) outcome"),
        }
    }

    #[tokio::test]
    async fn drive_l2_terminal_frame_without_deltas_marks_response_output() {
        let (_, output_observed) =
            drive_l2_with_stop_reason(Some(StopReason::Stop), vec![], None).await;

        assert!(output_observed);
    }

    #[tokio::test]
    async fn drive_l2_buffered_terminal_outranks_simultaneous_cancel_and_preserves_usage() {
        let request_id = RequestId::from("drive-l2-terminal-cancel-race");
        let response = ConversationResponse {
            items: vec![ConversationItem::assistant("finished")],
            stop_reason: Some(StopReason::Stop),
            usage: Some(sampling_types::TokenUsage {
                prompt_tokens: 11,
                completion_tokens: 7,
                total_tokens: 18,
                ..Default::default()
            }),
            cost_usd_ticks: None,
            message_chunks_emitted: 0,
            doom_loop_signals: Vec::new(),
            stop_message: None,
            message_id: None,
            raw_stop_reason: Some("stop".into()),
            stop_sequence: None,
        };
        let metrics = InferenceLatencyStats::from_timestamps(Instant::now(), &[], Instant::now());
        let l2 = stream::iter(vec![SamplingEvent::Completed {
            request_id: request_id.clone(),
            response: Box::new(response),
            metrics,
        }]);
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();
        cancel_token.cancel();

        let outcome = drive_l2(
            l2,
            request_id,
            &event_tx,
            &cancel_token,
            Arc::new(Mutex::new(None)),
            None,
            Arc::new(AtomicBool::new(false)),
        )
        .await;

        match outcome {
            AttemptOutcome::Completed { response, .. } => {
                assert_eq!(response.assistant_text(), "finished");
                assert_eq!(
                    response.usage.as_ref().map(|usage| usage.total_tokens),
                    Some(18)
                );
            }
            _ => panic!("buffered terminal must win over simultaneous cancellation"),
        }
    }

    #[tokio::test]
    async fn drive_l2_protocol_rejection_preserves_terminal_usage() {
        let request_id = RequestId::from("invalid-terminal-usage");
        let mut error = SamplingErrorInfo::from(&SamplingError::Serialization(
            serde::de::Error::custom("Responses protocol: incomplete tool call"),
        ));
        error.usage = Some(TokenUsage {
            prompt_tokens: 11,
            completion_tokens: 7,
            total_tokens: 18,
            ..Default::default()
        });
        let l2 = stream::iter(vec![SamplingEvent::Failed {
            request_id: request_id.clone(),
            error,
        }]);
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let outcome = drive_l2(
            l2,
            request_id,
            &event_tx,
            &CancellationToken::new(),
            Arc::new(Mutex::new(None)),
            None,
            Arc::new(AtomicBool::new(false)),
        )
        .await;
        match outcome {
            AttemptOutcome::Failed {
                error,
                usage: Some(usage),
            } => {
                assert!(!error.is_retryable());
                assert_eq!(usage.total_tokens, 18);
            }
            _ => panic!("validation failure must retain usage for settlement"),
        }
    }
}

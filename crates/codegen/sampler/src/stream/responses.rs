//! Layer-2 stream transform for the OpenAI Responses API.
//!
//! Consumes a raw `rs::ResponseStreamEvent` stream and produces
//! [`SamplingEvent`]s. Pure: no I/O, no shell coupling.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::{BoxStream, Stream};

use sampling_types::{
    ConversationItem, ConversationResponse, ResponseModelMetadata, SamplingError, StopReason,
    TokenUsage, rs,
};

use super::protocol_failure;
use crate::events::{SamplingChannel, SamplingErrorInfo, SamplingEvent};
use crate::metrics::InferenceLatencyStats;
use crate::types::RequestId;

fn response_usage(response: &rs::Response) -> Option<TokenUsage> {
    response.usage.as_ref().map(|u| TokenUsage {
        prompt_tokens: u.input_tokens,
        completion_tokens: u.output_tokens,
        total_tokens: u.total_tokens,
        reasoning_tokens: u.output_tokens_details.reasoning_tokens,
        cached_prompt_tokens: u.input_tokens_details.cached_tokens,
        cache_creation_prompt_tokens: 0,
    })
}

fn same_tool_identity(left: &rs::FunctionToolCall, right: &rs::FunctionToolCall) -> bool {
    left.id == right.id
        && left.call_id == right.call_id
        && left.name == right.name
        && left.namespace == right.namespace
}

/// Returns whether a Responses API event reflects real model progress
/// rather than a liveness-only heartbeat / status transition.
pub(crate) fn responses_event_has_meaningful_content(event: &rs::ResponseStreamEvent) -> bool {
    use rs::ResponseStreamEvent;

    match event {
        ResponseStreamEvent::ResponseCreated(_)
        | ResponseStreamEvent::ResponseInProgress(_)
        | ResponseStreamEvent::ResponseQueued(_) => false,
        ResponseStreamEvent::ResponseOutputTextDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseOutputTextDone(event) => !event.text.is_empty(),
        ResponseStreamEvent::ResponseRefusalDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseRefusalDone(event) => !event.refusal.is_empty(),
        ResponseStreamEvent::ResponseFunctionCallArgumentsDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseFunctionCallArgumentsDone(event) => {
            !event.arguments.is_empty() || event.name.as_ref().is_some_and(|name| !name.is_empty())
        }
        ResponseStreamEvent::ResponseReasoningSummaryTextDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseReasoningSummaryTextDone(event) => !event.text.is_empty(),
        ResponseStreamEvent::ResponseReasoningTextDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseReasoningTextDone(event) => !event.text.is_empty(),
        ResponseStreamEvent::ResponseMCPCallArgumentsDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseMCPCallArgumentsDone(event) => !event.arguments.is_empty(),
        ResponseStreamEvent::ResponseCodeInterpreterCallCodeDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseCodeInterpreterCallCodeDone(event) => !event.code.is_empty(),
        ResponseStreamEvent::ResponseCustomToolCallInputDelta(event) => !event.delta.is_empty(),
        ResponseStreamEvent::ResponseCustomToolCallInputDone(event) => !event.input.is_empty(),
        ResponseStreamEvent::ResponseCompleted(event) => {
            !event.response.output.is_empty()
                || event
                    .response
                    .usage
                    .as_ref()
                    .is_some_and(|usage| usage.output_tokens > 0)
        }
        ResponseStreamEvent::ResponseIncomplete(event) => {
            !event.response.output.is_empty()
                || event
                    .response
                    .usage
                    .as_ref()
                    .is_some_and(|usage| usage.output_tokens > 0)
        }
        ResponseStreamEvent::ResponseFailed(event) => {
            !event.response.output.is_empty()
                || event
                    .response
                    .usage
                    .as_ref()
                    .is_some_and(|usage| usage.output_tokens > 0)
        }
        ResponseStreamEvent::ResponseOutputItemAdded(_)
        | ResponseStreamEvent::ResponseOutputItemDone(_)
        | ResponseStreamEvent::ResponseContentPartAdded(_)
        | ResponseStreamEvent::ResponseContentPartDone(_)
        | ResponseStreamEvent::ResponseFileSearchCallInProgress(_)
        | ResponseStreamEvent::ResponseFileSearchCallSearching(_)
        | ResponseStreamEvent::ResponseFileSearchCallCompleted(_)
        | ResponseStreamEvent::ResponseReasoningSummaryPartAdded(_)
        | ResponseStreamEvent::ResponseReasoningSummaryPartDone(_)
        | ResponseStreamEvent::ResponseImageGenerationCallCompleted(_)
        | ResponseStreamEvent::ResponseImageGenerationCallGenerating(_)
        | ResponseStreamEvent::ResponseImageGenerationCallInProgress(_)
        | ResponseStreamEvent::ResponseImageGenerationCallPartialImage(_)
        | ResponseStreamEvent::ResponseMCPCallCompleted(_)
        | ResponseStreamEvent::ResponseMCPCallFailed(_)
        | ResponseStreamEvent::ResponseMCPCallInProgress(_)
        | ResponseStreamEvent::ResponseMCPListToolsCompleted(_)
        | ResponseStreamEvent::ResponseMCPListToolsFailed(_)
        | ResponseStreamEvent::ResponseMCPListToolsInProgress(_)
        | ResponseStreamEvent::ResponseCodeInterpreterCallInProgress(_)
        | ResponseStreamEvent::ResponseCodeInterpreterCallInterpreting(_)
        | ResponseStreamEvent::ResponseCodeInterpreterCallCompleted(_)
        | ResponseStreamEvent::ResponseOutputTextAnnotationAdded(_)
        | ResponseStreamEvent::ResponseError(_) => true,
        _ => true,
    }
}

pub(crate) fn responses_event_may_have_output(event: &rs::ResponseStreamEvent) -> bool {
    !matches!(event, rs::ResponseStreamEvent::ResponseError(_))
        && responses_event_has_meaningful_content(event)
}

/// Preserve Responses API termination diagnostics without letting wire values
/// become control semantics. The typed mapping remains the sole authority for
/// turn behavior; this string only retains the exact status and, when present,
/// the exact incomplete-detail reason carried alongside it.
fn responses_raw_stop_reason(
    status: &rs::Status,
    incomplete_details: Option<&rs::IncompleteDetails>,
) -> String {
    let status = match status {
        rs::Status::Completed => "completed",
        rs::Status::Failed => "failed",
        rs::Status::InProgress => "in_progress",
        rs::Status::Cancelled => "cancelled",
        rs::Status::Queued => "queued",
        rs::Status::Incomplete => "incomplete",
    };
    incomplete_details.map_or_else(
        || status.to_owned(),
        |details| format!("{status}:{}", details.reason),
    )
}

/// Transform a raw Responses API event stream into a stream of
/// [`SamplingEvent`]s.
///
/// Yields exactly one terminal event ([`SamplingEvent::Completed`] or
/// [`SamplingEvent::Failed`]) per request. Server-side `ResponseFailed`
/// and `ResponseError` events preserve the provider's error classification.
/// Local protocol failures never enter the transient HTTP retry path.
///
/// `doom_loop` is the collector returned alongside `raw_stream` by
/// `SamplingClient::conversation_stream_responses`; any signals the SSE
/// decoder recorded are drained onto the final `ConversationResponse`.
/// `None` (check disabled) leaves the response untouched.
pub fn stream_responses<'a>(
    raw_stream: BoxStream<'a, Result<rs::ResponseStreamEvent, SamplingError>>,
    model_metadata: Option<ResponseModelMetadata>,
    request_id: RequestId,
    idle_timeout: Duration,
    doom_loop: Option<crate::doom_loop::DoomLoopSignalCollector>,
) -> impl Stream<Item = SamplingEvent> + Send + 'a {
    stream_responses_tracked(
        raw_stream,
        model_metadata,
        request_id,
        idle_timeout,
        doom_loop,
        Arc::new(AtomicBool::new(false)),
    )
}

pub(crate) fn stream_responses_tracked<'a>(
    raw_stream: BoxStream<'a, Result<rs::ResponseStreamEvent, SamplingError>>,
    model_metadata: Option<ResponseModelMetadata>,
    request_id: RequestId,
    idle_timeout: Duration,
    doom_loop: Option<crate::doom_loop::DoomLoopSignalCollector>,
    output_observed: Arc<AtomicBool>,
) -> impl Stream<Item = SamplingEvent> + Send + 'a {
    async_stream::stream! {
        use rs::{ResponseStreamEvent, Status};

        let stream_start = Instant::now();
        let mut chunk_timestamps: Vec<Instant> = Vec::new();

        yield SamplingEvent::StreamStarted {
            request_id: request_id.clone(),
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
        };

        if let Some(metadata) = model_metadata {
            yield SamplingEvent::ModelMetadata {
                request_id: request_id.clone(),
                metadata,
            };
        }

        let mut final_response: Option<rs::Response> = None;
        let mut response_id: Option<String> = None;
        let mut chunk_index: u64 = 0;
        let mut message_chunk_count: u64 = 0;
        let mut first_token_emitted = false;
        let mut reasoning_acc = String::new();
        let mut last_content_chunk_at = Instant::now();

        // Maps Responses API `output_index` to our tool-only `tool_index`.
        // Populated when `ResponseOutputItemAdded` carries a `FunctionCall`;
        // later `ResponseFunctionCallArgumentsDelta` events
        // look up `output_index` here to find the matching `tool_index`.
        // Tool-only UI index, observed identity/argument prefix, args-done flag.
        let mut streamed_tools: BTreeMap<u32, (u32, rs::FunctionToolCall, bool)> = BTreeMap::new();
        let mut added_indices = BTreeSet::new();
        let mut done_indices = BTreeSet::new();
        let mut next_tool_index: u32 = 0;
        let mut completed_tool_items: BTreeMap<u32, rs::FunctionToolCall> = BTreeMap::new();

        let mut stream = raw_stream;
        loop {
            let event_result = match tokio::time::timeout(idle_timeout, stream.next()).await {
                Ok(Some(event_result)) => event_result,
                Ok(None) => break,
                Err(_elapsed) => {
                    let err = SamplingError::IdleTimeout {
                        elapsed_secs: idle_timeout.as_secs(),
                    };
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }
            };

            let event = match event_result {
                Ok(event) => event,
                Err(err) => {
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }
            };

            if responses_event_may_have_output(&event) {
                output_observed.store(true, Ordering::Relaxed);
            }

            // A confident server-detected loop aborts the attempt (dropping
            // the SSE connection) so the retry loop can resample instead of
            // streaming the burning tail. Checked before the event is
            // processed so a terminal frame carrying the signal never
            // becomes the accepted response while the abort is armed.
            if let Some(triggers) = doom_loop.as_ref().and_then(|c| c.abort_triggers()) {
                let err = SamplingError::DoomLoopDetected {
                    triggers,
                    aborted_at_chunk: Some(chunk_index),
                };
                yield SamplingEvent::Failed {
                    request_id: request_id.clone(),
                    error: SamplingErrorInfo::from(&err),
                };
                return;
            }

            let event_has_content = responses_event_has_meaningful_content(&event);

            // Track whether ResponseIncomplete should break the loop
            // after the content-aware idle check below.
            let mut should_break = false;

            if let Some(response) = match &event {
                ResponseStreamEvent::ResponseCreated(event) => Some(&event.response),
                ResponseStreamEvent::ResponseInProgress(event) => Some(&event.response),
                ResponseStreamEvent::ResponseQueued(event) => Some(&event.response),
                _ => None,
            } {
                if response_id.as_ref().is_some_and(|id| id != &response.id) {
                    yield protocol_failure(&request_id, "Responses protocol: conflicting response id", None);
                    return;
                }
                response_id = Some(response.id.clone());
            }
            match event {
                ResponseStreamEvent::ResponseOutputTextDelta(text_delta_event) => {
                    let delta = text_delta_event.delta;
                    if !delta.is_empty() {
                        if !first_token_emitted {
                            first_token_emitted = true;
                            yield SamplingEvent::FirstToken {
                                request_id: request_id.clone(),
                            };
                        }
                        chunk_timestamps.push(Instant::now());
                        chunk_index += 1;
                        message_chunk_count += 1;
                        yield SamplingEvent::ChannelToken {
                            request_id: request_id.clone(),
                            channel: SamplingChannel::Text,
                            text: delta,
                            chunk_index,
                        };
                    }
                }

                ResponseStreamEvent::ResponseReasoningSummaryTextDelta(summary_event) => {
                    let delta = summary_event.delta;
                    if !delta.is_empty() {
                        if !first_token_emitted {
                            first_token_emitted = true;
                            yield SamplingEvent::FirstToken {
                                request_id: request_id.clone(),
                            };
                        }
                        chunk_index += 1;
                        yield SamplingEvent::ChannelToken {
                            request_id: request_id.clone(),
                            channel: SamplingChannel::Reasoning,
                            text: delta,
                            chunk_index,
                        };
                    }
                }

                ResponseStreamEvent::ResponseReasoningTextDelta(reasoning_event) => {
                    let delta = reasoning_event.delta;
                    if !delta.is_empty() {
                        if !first_token_emitted {
                            first_token_emitted = true;
                            yield SamplingEvent::FirstToken {
                                request_id: request_id.clone(),
                            };
                        }
                        chunk_index += 1;
                        reasoning_acc.push_str(&delta);
                        yield SamplingEvent::ChannelToken {
                            request_id: request_id.clone(),
                            channel: SamplingChannel::Reasoning,
                            text: delta,
                            chunk_index,
                        };
                    }
                }

                // Start of a Responses FunctionCall — emit initial id+name
                // and remember the output_index → tool_index mapping.
                ResponseStreamEvent::ResponseOutputItemAdded(added_event) => {
                    if !added_indices.insert(added_event.output_index) || done_indices.contains(&added_event.output_index) {
                        yield protocol_failure(&request_id, "Responses protocol: duplicate or already closed output index", None);
                        return;
                    }
                    if let rs::OutputItem::FunctionCall(fc) = added_event.item {
                        let tool_index = next_tool_index;
                        next_tool_index += 1;
                        streamed_tools.insert(added_event.output_index, (tool_index, fc.clone(), false));

                        yield SamplingEvent::ToolCallDelta {
                            request_id: request_id.clone(),
                            tool_index,
                            id: Some(fc.call_id),
                            name: Some(fc.name),
                            arguments_delta: None,
                        };
                    }
                }

                // Continuation chunk for a streaming FunctionCall's args.
                // An orphan or mismatched delta cannot be silently discarded:
                // otherwise the UI and eventual executable call can disagree.
                ResponseStreamEvent::ResponseFunctionCallArgumentsDelta(args_event) => {
                    let Some((tool_index, call, arguments_done)) = streamed_tools.get_mut(&args_event.output_index) else {
                        yield protocol_failure(&request_id, "Responses protocol: arguments delta without a function item", None);
                        return;
                    };
                    if *arguments_done || done_indices.contains(&args_event.output_index)
                        || call.id.as_deref().is_some_and(|id| id != args_event.item_id)
                    {
                        yield protocol_failure(&request_id, "Responses protocol: arguments delta for a closed or mismatched item", None);
                        return;
                    }
                    let delta = args_event.delta;
                    call.arguments.push_str(&delta);
                    if !delta.is_empty() {
                        yield SamplingEvent::ToolCallDelta {
                            request_id: request_id.clone(),
                            tool_index: *tool_index,
                            id: None,
                            name: None,
                            arguments_delta: Some(delta),
                        };
                    }
                }

                ResponseStreamEvent::ResponseFunctionCallArgumentsDone(args_event) => {
                    let Some((_, call, arguments_done)) = streamed_tools.get_mut(&args_event.output_index) else {
                        yield protocol_failure(&request_id, "Responses protocol: arguments done without a function item", None);
                        return;
                    };
                    if *arguments_done || done_indices.contains(&args_event.output_index)
                        || call.id.as_deref().is_some_and(|id| id != args_event.item_id)
                        || args_event.name.as_ref().is_some_and(|name| name != &call.name)
                        || !args_event.arguments.starts_with(&call.arguments)
                    {
                        yield protocol_failure(&request_id, "Responses protocol: conflicting function arguments done", None);
                        return;
                    }
                    call.arguments = args_event.arguments;
                    *arguments_done = true;
                }

                ResponseStreamEvent::ResponseCompleted(completed_event) => {
                    if completed_event.response.status != Status::Completed {
                        yield protocol_failure(&request_id, "Responses protocol: response.completed has a non-completed status", response_usage(&completed_event.response));
                        return;
                    }
                    final_response = Some(completed_event.response);
                    should_break = true;
                }

                ResponseStreamEvent::ResponseIncomplete(incomplete_event) => {
                    if incomplete_event.response.status != Status::Incomplete {
                        yield protocol_failure(&request_id, "Responses protocol: response.incomplete has a non-incomplete status", response_usage(&incomplete_event.response));
                        return;
                    }
                    final_response = Some(incomplete_event.response);
                    should_break = true;
                }

                ResponseStreamEvent::ResponseOutputItemDone(done_event) => {
                    let index = done_event.output_index;
                    if !done_indices.insert(index) {
                        yield protocol_failure(&request_id, "Responses protocol: duplicate output item done", None);
                        return;
                    }
                    match done_event.item {
                        rs::OutputItem::FunctionCall(call) => {
                            if added_indices.contains(&index) && !streamed_tools.contains_key(&index) {
                                yield protocol_failure(&request_id, "Responses protocol: output item changed to a function call", None);
                                return;
                            }
                            completed_tool_items.insert(index, call);
                        }
                        _ if streamed_tools.contains_key(&index) => {
                            yield protocol_failure(&request_id, "Responses protocol: function item changed type", None);
                            return;
                        }
                        _ => {}
                    }
                }

                ResponseStreamEvent::ResponseFailed(failed_event) => {
                    let response = failed_event.response;
                    if response.status != Status::Failed {
                        yield protocol_failure(&request_id, "Responses protocol: response.failed has a non-failed status", response_usage(&response));
                        return;
                    }
                    let (error_code, error_message) = response
                        .error
                        .as_ref()
                        .map(|e| (e.code.clone(), e.message.clone()))
                        .unwrap_or_else(|| {
                            ("response_failed".to_string(), "unknown error".to_string())
                        });
                    let err = SamplingError::from_stream_error(error_code, error_message);
                    let mut error = SamplingErrorInfo::from(&err);
                    error.usage = response_usage(&response);
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error,
                    };
                    return;
                }

                ResponseStreamEvent::ResponseError(error_event) => {
                    let code = error_event.code.unwrap_or_else(|| "error".to_string());
                    let err = SamplingError::from_stream_error(code, error_event.message);
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }

                // All other events (intermediate progress, annotations,
                // hosted tools, image gen, file search, etc.) — no action needed.
                _ => {}
            }

            if event_has_content {
                last_content_chunk_at = Instant::now();
            } else if last_content_chunk_at.elapsed() > idle_timeout {
                let err = SamplingError::IdleTimeout {
                    elapsed_secs: idle_timeout.as_secs(),
                };
                yield SamplingEvent::Failed {
                    request_id: request_id.clone(),
                    error: SamplingErrorInfo::from(&err),
                };
                return;
            }

            if should_break {
                break;
            }
        }

        // ── Build the final response ─────────────────────────────────
        let mut response = match final_response {
            Some(r) => r,
            None => {
                yield protocol_failure(&request_id, "Responses protocol: stream ended without a terminal response", None);
                return;
            }
        };

        // Usage accounting fields (`prompt_tokens`, `completion_tokens`,
        // `cached_prompt_tokens`, `reasoning_tokens`) are the cumulative
        // wire values — they sum across every server-side turn of the
        // agent loop and are what we report in local usage diagnostics.
        //
        // `total_tokens` is the live context length used to drive the
        // CLI `/context` bar, the auto-compact threshold, and
        // `meta.totalTokens` on persisted sessions. The SSE decoder
        // (`deserialize_response_event`) has already rewritten
        // `u.total_tokens` to `context_details.input + output` when
        // the backend emits it; on older deployments the wire
        // value passes through unchanged.
        let usage = response_usage(&response);
        if let Some(provider_error) = &response.error {
            let err = SamplingError::from_stream_error(provider_error.code.clone(), provider_error.message.clone());
            let mut error = SamplingErrorInfo::from(&err);
            error.usage = usage;
            yield SamplingEvent::Failed { request_id: request_id.clone(), error };
            return;
        }
        if response_id.as_ref().is_some_and(|id| id != &response.id) {
            yield protocol_failure(&request_id, "Responses protocol: conflicting terminal response id", usage);
            return;
        }
        // The terminal snapshot is authoritative, including when optional
        // intermediate events were omitted, but cannot contradict evidence we
        // already observed or silently lose an announced executable item.
        for (index, (_, observed, arguments_done)) in &streamed_tools {
            let matches = matches!(response.output.get(*index as usize), Some(rs::OutputItem::FunctionCall(call))
                if same_tool_identity(observed, call)
                    && if *arguments_done { call.arguments == observed.arguments }
                       else { call.arguments.starts_with(&observed.arguments) });
            if !matches {
                yield protocol_failure(&request_id, format!("Responses protocol: terminal snapshot conflicts with function item {index}"), usage);
                return;
            }
        }
        for (index, done) in &completed_tool_items {
            let matches = matches!(response.output.get(*index as usize), Some(rs::OutputItem::FunctionCall(call))
                if same_tool_identity(done, call) && done.arguments == call.arguments
                    && done.status.is_none_or(|status| status == rs::OutputStatus::Completed));
            if !matches {
                yield protocol_failure(&request_id, format!("Responses protocol: terminal snapshot conflicts with done function item {index}"), usage);
                return;
            }
        }

        let cost_usd_ticks = response
            .metadata
            .as_mut()
            .and_then(|m| m.remove(crate::client::COST_USD_TICKS_METADATA_KEY))
            .and_then(|s| s.parse::<i64>().ok());

        let status = response.status.clone();
        let raw_stop_reason =
            responses_raw_stop_reason(&status, response.incomplete_details.as_ref());
        let incomplete_stop = match response.incomplete_details.as_ref().map(|details| details.reason.as_str()) {
            Some("max_output_tokens") => StopReason::Length,
            Some("content_filter") => StopReason::ContentFilter,
            // A supplier extension is not evidence of token exhaustion and
            // must not trigger Grow's automatic truncation-continuation loop.
            _ => StopReason::Stop,
        };

        // Convert to ConversationItem(s); patch in accumulated reasoning
        // text as a fallback when the final response lacks `content` /
        // `summary` (the streaming deltas may have arrived out of band).
        // Splice policy lives in `inject_streaming_reasoning_fallback`.
        for (index, item) in response.output.iter_mut().enumerate() {
            let rs::OutputItem::FunctionCall(call) = item else { continue };
            // An omitted optional status on the terminal snapshot can use an
            // earlier matching item.done as evidence. Explicitly incomplete
            // status, conflicting identities or changed arguments never can.
            if call.status.is_none()
                && let Some(done) = u32::try_from(index).ok().and_then(|index| completed_tool_items.get(&index))
                && done.status.is_none_or(|status| status == rs::OutputStatus::Completed)
                && done.id == call.id && done.call_id == call.call_id
                && done.name == call.name && done.namespace == call.namespace
                && done.arguments == call.arguments
            {
                call.status = Some(rs::OutputStatus::Completed);
            }
        }
        let native_continuation = Some(sampling_types::responses_native_fragment(&response));
        let mut items = match sampling_types::response_to_conversation_items(response) {
            Ok(items) => items,
            Err(err) => {
                let mut error = SamplingErrorInfo::from(&err);
                error.usage = usage;
                yield SamplingEvent::Failed { request_id: request_id.clone(), error };
                return;
            }
        };
        sampling_types::inject_streaming_reasoning_fallback(&mut items, reasoning_acc);

        let has_tool_calls = items.iter().any(|i| match i {
            ConversationItem::Assistant(a) => !a.tool_calls.is_empty(),
            _ => false,
        });

        let stop_reason = if has_tool_calls {
            Some(StopReason::ToolCalls)
        } else {
            match status {
                Status::Completed => Some(StopReason::Stop),
                Status::Incomplete => Some(incomplete_stop),
                _ => None,
            }
        };

        let stream_end = Instant::now();
        let metrics =
            InferenceLatencyStats::from_timestamps(stream_start, &chunk_timestamps, stream_end);

        // Warn-only for now: surface the server-reported triggers once per
        // request (raw labels only — ZDR-safe) and attach them for callers.
        let doom_loop_signals = doom_loop
            .as_ref()
            .map(|collector| collector.take())
            .unwrap_or_default();
        if !doom_loop_signals.is_empty() {
            tracing::warn!(
                request_id = %request_id,
                triggers = ?doom_loop_signals.iter().map(|s| s.raw.as_str()).collect::<Vec<_>>(),
                "server reported doom-loop triggers for this response"
            );
        }

        let conversation_response = ConversationResponse {
            items,
            stop_reason,
            usage,
            cost_usd_ticks,
            message_chunks_emitted: message_chunk_count,
            doom_loop_signals,
            stop_message: None, // not reported on the Responses API
            message_id: None,   // no provider message id on the Responses API
            raw_stop_reason: Some(raw_stop_reason),
            stop_sequence: None,
            native_continuation,
        };

        yield SamplingEvent::Completed {
            request_id: request_id.clone(),
            response: Box::new(conversation_response),
            metrics,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_openai::types::responses as rs_types;
    use futures_util::stream;
    use std::pin::pin;

    fn rid() -> RequestId {
        RequestId::from("resp-test")
    }

    /// Build a minimal `rs_types::Response` for use in `ResponseCompleted`
    fn build_response(status: rs_types::Status) -> rs_types::Response {
        rs_types::Response {
            background: None,
            billing: None,
            conversation: None,
            created_at: 0,
            completed_at: None,
            error: None,
            id: "resp_1".into(),
            incomplete_details: None,
            instructions: None,
            max_output_tokens: None,
            metadata: None,
            model: "test-model".into(),
            object: "response".into(),
            output: vec![],
            parallel_tool_calls: None,
            previous_response_id: None,
            prompt: None,
            prompt_cache_key: None,
            prompt_cache_retention: None,
            reasoning: None,
            safety_identifier: None,
            service_tier: None,
            status,
            temperature: None,
            text: None,
            tool_choice: None,
            tools: None,
            top_logprobs: None,
            top_p: None,
            truncation: None,
            usage: None,
        }
    }

    fn empty_completed_response() -> rs_types::Response {
        build_response(rs_types::Status::Completed)
    }

    fn failed_response_with_error(message: &str) -> rs_types::Response {
        let mut r = build_response(rs_types::Status::Failed);
        r.error = Some(rs_types::ErrorObject {
            code: "server_error".into(),
            message: message.into(),
        });
        r
    }

    fn text_delta_event(delta: &str) -> rs::ResponseStreamEvent {
        rs::ResponseStreamEvent::ResponseOutputTextDelta(rs_types::ResponseTextDeltaEvent {
            sequence_number: 0,
            item_id: "item-1".into(),
            output_index: 0,
            content_index: 0,
            delta: delta.into(),
            logprobs: None,
        })
    }

    fn completed_event() -> rs::ResponseStreamEvent {
        rs::ResponseStreamEvent::ResponseCompleted(rs_types::ResponseCompletedEvent {
            response: empty_completed_response(),
            sequence_number: 0,
        })
    }

    fn incomplete_event(reason: &str) -> rs::ResponseStreamEvent {
        let mut response = build_response(rs_types::Status::Incomplete);
        response.incomplete_details = Some(rs_types::IncompleteDetails {
            reason: reason.to_owned(),
        });
        rs::ResponseStreamEvent::ResponseIncomplete(rs_types::ResponseIncompleteEvent {
            response,
            sequence_number: 0,
        })
    }

    async fn collect(s: impl Stream<Item = SamplingEvent>) -> Vec<SamplingEvent> {
        let mut out = Vec::new();
        let mut s = pin!(s);
        while let Some(ev) = s.next().await {
            out.push(ev);
        }
        out
    }

    #[tokio::test]
    async fn missing_completed_event_yields_failed() {
        let raw =
            stream::iter(Vec::<Result<rs::ResponseStreamEvent, SamplingError>>::new()).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(error.kind, crate::events::SamplingErrorKind::Serialization);
                assert_eq!(error.status_code, None);
                assert!(!error.is_retryable);
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn text_delta_then_completed_yields_completed_with_stop() {
        let raw = stream::iter(vec![Ok(text_delta_event("hello")), Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        let text_tokens: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                SamplingEvent::ChannelToken {
                    channel: SamplingChannel::Text,
                    text,
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text_tokens, vec!["hello"]);

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.stop_reason, Some(StopReason::Stop));
                assert_eq!(response.raw_stop_reason.as_deref(), Some("completed"));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn incomplete_status_and_detail_are_preserved_beside_typed_length() {
        let raw = stream::iter(vec![Ok(incomplete_event("max_output_tokens"))]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.stop_reason, Some(StopReason::Length));
                assert_eq!(
                    response.raw_stop_reason.as_deref(),
                    Some("incomplete:max_output_tokens")
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn completed_does_not_consume_tail_errors_or_wait_for_eof() {
        for pending_tail in [false, true] {
            let tail: BoxStream<'_, Result<rs::ResponseStreamEvent, SamplingError>> =
                if pending_tail {
                    stream::pending().boxed()
                } else {
                    stream::once(async {
                        Err(SamplingError::EventStreamError("trailing error".into()))
                    })
                    .boxed()
                };
            let raw = stream::iter(vec![Ok(completed_event())])
                .chain(tail)
                .boxed();
            let events = tokio::time::timeout(
                Duration::from_secs(1),
                collect(stream_responses(
                    raw,
                    None,
                    rid(),
                    Duration::from_secs(60),
                    None,
                )),
            )
            .await
            .expect("completed must not depend on EOF");
            assert!(matches!(
                events.last(),
                Some(SamplingEvent::Completed { .. })
            ));
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event,
                        SamplingEvent::Completed { .. } | SamplingEvent::Failed { .. }
                    ))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn empty_failed_response_is_not_treated_as_output() {
        let event = rs::ResponseStreamEvent::ResponseFailed(rs_types::ResponseFailedEvent {
            response: failed_response_with_error("boom"),
            sequence_number: 0,
        });
        assert!(!responses_event_may_have_output(&event));
    }

    #[tokio::test]
    async fn response_failed_yields_failed_500() {
        let failed = rs::ResponseStreamEvent::ResponseFailed(rs_types::ResponseFailedEvent {
            response: failed_response_with_error("boom"),
            sequence_number: 0,
        });
        let raw = stream::iter(vec![Ok(failed)]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(error.kind, crate::events::SamplingErrorKind::Api);
                assert_eq!(error.status_code, Some(500));
                assert!(error.message.contains("boom"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mid_stream_transport_error_yields_failed() {
        let raw = stream::iter(vec![
            Ok(text_delta_event("hi")),
            Err(SamplingError::EventStreamError("conn reset".into())),
        ])
        .boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        assert!(
            events
                .iter()
                .any(|e| matches!(e, SamplingEvent::Failed { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SamplingEvent::Completed { .. }))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn idle_timeout_when_stream_stalls() {
        let raw = stream::iter(vec![Ok(text_delta_event("hi"))])
            .chain(stream::pending())
            .boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_millis(100),
            None,
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(error.kind, crate::events::SamplingErrorKind::IdleTimeout);
            }
            other => panic!("expected Failed(IdleTimeout), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn model_metadata_yielded_after_stream_started() {
        let raw = stream::iter(vec![Ok(completed_event())]).boxed();
        let metadata = ResponseModelMetadata {
            context_window: Some(8192),
            ..Default::default()
        };
        let events = collect(stream_responses(
            raw,
            Some(metadata),
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;

        assert!(matches!(events[0], SamplingEvent::StreamStarted { .. }));
        assert!(matches!(events[1], SamplingEvent::ModelMetadata { .. }));
    }

    #[test]
    fn meaningful_content_classifier_basics() {
        // Text delta with content is meaningful.
        let event = text_delta_event("foo");
        assert!(responses_event_has_meaningful_content(&event));
        // Empty text delta is not.
        let empty = text_delta_event("");
        assert!(!responses_event_has_meaningful_content(&empty));
        // A terminal frame with no output is not irreversible model output.
        assert!(!responses_event_has_meaningful_content(&completed_event()));
    }

    #[test]
    fn output_classifier_covers_non_forwarded_backend_events() {
        let queued = rs::ResponseStreamEvent::ResponseQueued(rs_types::ResponseQueuedEvent {
            sequence_number: 0,
            response: empty_completed_response(),
        });
        assert!(!responses_event_may_have_output(&queued));

        let response_error = rs::ResponseStreamEvent::ResponseError(rs_types::ResponseErrorEvent {
            sequence_number: 1,
            code: Some("server_error".into()),
            message: "failed before output".into(),
            param: None,
        });
        assert!(!responses_event_may_have_output(&response_error));

        let refusal =
            rs::ResponseStreamEvent::ResponseRefusalDelta(rs_types::ResponseRefusalDeltaEvent {
                sequence_number: 1,
                item_id: "item-1".into(),
                output_index: 0,
                content_index: 0,
                delta: "no".into(),
            });
        assert!(responses_event_may_have_output(&refusal));
    }

    #[tokio::test]
    async fn tracked_stream_marks_non_forwarded_refusal_as_output() {
        let output_observed = Arc::new(AtomicBool::new(false));
        let refusal =
            rs::ResponseStreamEvent::ResponseRefusalDelta(rs_types::ResponseRefusalDeltaEvent {
                sequence_number: 0,
                item_id: "item-1".into(),
                output_index: 0,
                content_index: 0,
                delta: "no".into(),
            });
        let raw = stream::iter(vec![Ok(refusal), Ok(completed_event())]).boxed();
        let _ = collect(stream_responses_tracked(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
            Arc::clone(&output_observed),
        ))
        .await;

        assert!(output_observed.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn tracked_stream_leaves_empty_completion_retryable() {
        let output_observed = Arc::new(AtomicBool::new(false));
        let raw = stream::iter(vec![Ok(completed_event())]).boxed();
        let _ = collect(stream_responses_tracked(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
            Arc::clone(&output_observed),
        ))
        .await;

        assert!(!output_observed.load(Ordering::Relaxed));
    }

    fn function_call_added_event(
        output_index: u32,
        call_id: &str,
        name: &str,
    ) -> rs::ResponseStreamEvent {
        rs::ResponseStreamEvent::ResponseOutputItemAdded(rs_types::ResponseOutputItemAddedEvent {
            sequence_number: 0,
            output_index,
            item: rs_types::OutputItem::FunctionCall(rs_types::FunctionToolCall {
                arguments: String::new(),
                call_id: call_id.into(),
                namespace: None,
                name: name.into(),
                id: Some(format!("item-{output_index}")),
                status: None,
            }),
        })
    }

    fn completed_tools(calls: &[(&str, &str, &str)]) -> rs::ResponseStreamEvent {
        let mut response = empty_completed_response();
        response.output = calls
            .iter()
            .enumerate()
            .map(|(index, (id, name, arguments))| {
                rs::OutputItem::FunctionCall(rs::FunctionToolCall {
                    arguments: (*arguments).into(),
                    call_id: (*id).into(),
                    name: (*name).into(),
                    id: Some(format!("item-{index}")),
                    namespace: None,
                    status: Some(rs::OutputStatus::Completed),
                })
            })
            .collect();
        rs::ResponseStreamEvent::ResponseCompleted(rs::ResponseCompletedEvent {
            sequence_number: 10,
            response,
        })
    }

    #[tokio::test]
    async fn conflicting_tool_stream_evidence_never_completes() {
        let added = || function_call_added_event(0, "call_0", "test_tool");
        let good = || completed_tools(&[("call_0", "test_tool", "{}")]);
        let done = || {
            serde_json::from_value::<rs::ResponseStreamEvent>(serde_json::json!({
            "type":"response.output_item.done", "sequence_number":2, "output_index":0,
            "item":{"type":"function_call","id":"item-0","call_id":"call_0","name":"test_tool","arguments":"{}","status":"completed"}
        })).unwrap()
        };
        let wrong_item = serde_json::from_value(serde_json::json!({
            "type":"response.function_call_arguments.delta", "sequence_number":1,
            "output_index":0, "item_id":"other-item", "delta":"{}"
        }))
        .unwrap();
        let args_done = || {
            serde_json::from_value(serde_json::json!({
                "type":"response.function_call_arguments.done", "sequence_number":2,
                "output_index":0, "item_id":"item-0", "arguments":"{}", "name":"test_tool"
            }))
            .unwrap()
        };
        for wire in [
            vec![added(), added(), good()],
            vec![added(), wrong_item, good()],
            vec![
                added(),
                function_call_args_delta_event(0, "{\"x\":1}"),
                good(),
            ],
            vec![added(), completed_event()],
            vec![
                added(),
                completed_tools(&[("call_other", "test_tool", "{}")]),
            ],
            vec![done(), done(), good()],
            vec![
                done(),
                completed_tools(&[("call_0", "test_tool", "{\"changed\":true}")]),
            ],
            vec![
                added(),
                args_done(),
                function_call_args_delta_event(0, " "),
                good(),
            ],
            vec![added(), args_done(), args_done(), good()],
            vec![
                added(),
                done(),
                function_call_args_delta_event(0, "{}"),
                good(),
            ],
        ] {
            let events = collect(stream_responses(
                stream::iter(wire.into_iter().map(Ok)).boxed(),
                None,
                rid(),
                Duration::from_secs(1),
                None,
            ))
            .await;
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event,
                        SamplingEvent::Completed { .. } | SamplingEvent::Failed { .. }
                    ))
                    .count(),
                1
            );
            assert!(
                matches!(events.last(), Some(SamplingEvent::Failed { error, .. })
                if error.kind == crate::events::SamplingErrorKind::Serialization && !error.is_retryable),
                "{events:?}"
            );
        }
    }

    #[tokio::test]
    async fn incomplete_reason_does_not_invent_token_exhaustion() {
        for (reason, expected) in [
            ("max_output_tokens", StopReason::Length),
            ("content_filter", StopReason::ContentFilter),
            ("vendor_stop", StopReason::Stop),
        ] {
            let events = collect(stream_responses(
                stream::iter([Ok(incomplete_event(reason))]).boxed(),
                None,
                rid(),
                Duration::from_secs(1),
                None,
            ))
            .await;
            let Some(SamplingEvent::Completed { response, .. }) = events.last() else {
                panic!("{events:?}");
            };
            assert_eq!(response.stop_reason, Some(expected));
            assert_eq!(
                response.raw_stop_reason.as_deref(),
                Some(format!("incomplete:{reason}").as_str())
            );
        }
    }

    #[tokio::test]
    async fn tool_completion_requires_status_or_matching_done_evidence() {
        use rs::Status;
        for (overall, item_status, arguments, done, accepted) in [
            (Status::Completed, None, "{}", false, true),
            (
                Status::Completed,
                Some(rs::OutputStatus::Incomplete),
                "{}",
                true,
                false,
            ),
            (
                Status::Incomplete,
                Some(rs::OutputStatus::Completed),
                "{}",
                false,
                true,
            ),
            (Status::Incomplete, None, "{}", false, false),
            (Status::Incomplete, None, "{}", true, true),
            (
                Status::Incomplete,
                Some(rs::OutputStatus::InProgress),
                "{}",
                true,
                false,
            ),
            (
                Status::Completed,
                Some(rs::OutputStatus::Completed),
                "{",
                true,
                false,
            ),
        ] {
            let call = rs::FunctionToolCall {
                call_id: "call_a".into(),
                id: Some("fc_a".into()),
                name: "lookup".into(),
                namespace: None,
                arguments: arguments.into(),
                status: item_status,
            };
            let mut response = build_response(overall.clone());
            response
                .output
                .push(rs::OutputItem::FunctionCall(call.clone()));
            let mut wire = vec![];
            if done {
                let mut done_call = call;
                done_call.status = Some(rs::OutputStatus::Completed);
                wire.push(Ok(rs::ResponseStreamEvent::ResponseOutputItemDone(
                    rs::ResponseOutputItemDoneEvent {
                        sequence_number: 1,
                        output_index: 0,
                        item: rs::OutputItem::FunctionCall(done_call),
                    },
                )));
            }
            wire.push(Ok(if overall == Status::Completed {
                rs::ResponseStreamEvent::ResponseCompleted(rs::ResponseCompletedEvent {
                    sequence_number: 2,
                    response,
                })
            } else {
                rs::ResponseStreamEvent::ResponseIncomplete(rs::ResponseIncompleteEvent {
                    sequence_number: 2,
                    response,
                })
            }));
            let events = collect(stream_responses(
                stream::iter(wire).boxed(),
                None,
                rid(),
                Duration::from_secs(60),
                None,
            ))
            .await;
            assert_eq!(
                matches!(events.last(), Some(SamplingEvent::Completed { .. })),
                accepted,
                "overall={overall:?} item={item_status:?} args={arguments} done={done}: {events:?}"
            );
            if let Some(SamplingEvent::Completed { response, .. }) = events.last() {
                assert_eq!(response.tool_calls()[0].id.as_ref(), "call_a");
                assert_eq!(response.stop_reason, Some(StopReason::ToolCalls));
            }
        }
    }

    fn function_call_args_delta_event(output_index: u32, delta: &str) -> rs::ResponseStreamEvent {
        rs::ResponseStreamEvent::ResponseFunctionCallArgumentsDelta(
            rs_types::ResponseFunctionCallArgumentsDeltaEvent {
                sequence_number: 0,
                item_id: format!("item-{output_index}"),
                output_index,
                delta: delta.into(),
            },
        )
    }

    type Delta = (u32, Option<String>, Option<String>, Option<String>);

    /// Extract all ToolCallDelta events as (tool_index, id, name, arguments_delta).
    fn tool_call_deltas(evs: &[SamplingEvent]) -> Vec<Delta> {
        evs.iter()
            .filter_map(|e| match e {
                SamplingEvent::ToolCallDelta {
                    tool_index,
                    id,
                    name,
                    arguments_delta,
                    ..
                } => Some((
                    *tool_index,
                    id.clone(),
                    name.clone(),
                    arguments_delta.clone(),
                )),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn function_call_emits_initial_id_name_then_arg_deltas() {
        let events: Vec<Result<rs::ResponseStreamEvent, SamplingError>> = vec![
            Ok(function_call_added_event(0, "call_xyz", "do_thing")),
            Ok(function_call_args_delta_event(0, "{\"x\":")),
            Ok(function_call_args_delta_event(0, "1}")),
            Ok(completed_tools(&[("call_xyz", "do_thing", "{\"x\":1}")])),
        ];
        let raw = stream::iter(events).boxed();
        let evs = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        let deltas = tool_call_deltas(&evs);

        assert_eq!(deltas.len(), 3);
        assert_eq!(deltas[0].0, 0);
        assert_eq!(deltas[0].1.as_deref(), Some("call_xyz"));
        assert_eq!(deltas[0].2.as_deref(), Some("do_thing"));
        assert_eq!(deltas[0].3, None);
        assert_eq!(deltas[1].0, 0);
        assert_eq!(deltas[1].1, None);
        assert_eq!(deltas[1].2, None);
        assert_eq!(deltas[1].3.as_deref(), Some("{\"x\":"));
        assert_eq!(deltas[2].3.as_deref(), Some("1}"));
        assert!(matches!(evs.last(), Some(SamplingEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn function_call_args_delta_without_added_event_fails() {
        let events: Vec<Result<rs::ResponseStreamEvent, SamplingError>> = vec![
            Ok(function_call_args_delta_event(7, "{\"oops\":1}")),
            Ok(completed_event()),
        ];
        let raw = stream::iter(events).boxed();
        let evs = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        assert_eq!(tool_call_deltas(&evs).len(), 0);
        assert!(
            matches!(evs.last(), Some(SamplingEvent::Failed { error, .. })
            if error.kind == crate::events::SamplingErrorKind::Serialization)
        );
    }

    #[tokio::test]
    async fn multiple_function_calls_get_distinct_tool_indices() {
        let events: Vec<Result<rs::ResponseStreamEvent, SamplingError>> = vec![
            Ok(function_call_added_event(0, "call_a", "tool_a")),
            Ok(function_call_added_event(1, "call_b", "tool_b")),
            Ok(function_call_args_delta_event(0, "{\"a\":1}")),
            Ok(function_call_args_delta_event(1, "{\"b\":2}")),
            Ok(completed_tools(&[
                ("call_a", "tool_a", "{\"a\":1}"),
                ("call_b", "tool_b", "{\"b\":2}"),
            ])),
        ];
        let raw = stream::iter(events).boxed();
        let evs = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        let deltas = tool_call_deltas(&evs);

        assert_eq!(deltas.len(), 4);
        assert_eq!(deltas[0].0, 0);
        assert_eq!(deltas[0].1.as_deref(), Some("call_a"));
        assert_eq!(deltas[1].0, 1);
        assert_eq!(deltas[1].1.as_deref(), Some("call_b"));
        assert_eq!(deltas[2].0, 0);
        assert_eq!(deltas[2].3.as_deref(), Some("{\"a\":1}"));
        assert_eq!(deltas[3].0, 1);
        assert_eq!(deltas[3].3.as_deref(), Some("{\"b\":2}"));
        assert!(matches!(evs.last(), Some(SamplingEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn doom_loop_collector_signals_land_on_completed_response() {
        use sampling_types::doom_loop::{DOOM_LOOP_CHECK_EVENT_TYPE, SAMPLE_CHECK_EVENT_DATA};
        let collector = crate::doom_loop::DoomLoopSignalCollector::default();
        assert!(collector.absorb(DOOM_LOOP_CHECK_EVENT_TYPE, SAMPLE_CHECK_EVENT_DATA));
        let raw = stream::iter(vec![Ok(text_delta_event("hello")), Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            Some(collector),
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.doom_loop_signals.len(), 1);
                assert_eq!(
                    response.doom_loop_signals[0].raw,
                    "tail_repetition:4@response"
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    /// An armed collector holding a confident signal aborts the attempt with
    /// a retryable doom-loop failure; disarmed, the same stream completes and
    /// the signals ride the response instead.
    #[tokio::test]
    async fn confident_signal_aborts_stream_unless_disarmed() {
        let confident = r#"{"type":"response.doom_loop_check","doom_loop_check":{"triggers":["tail_repetition:8@thinking"]}}"#;

        let collector = crate::doom_loop::DoomLoopSignalCollector::default();
        assert!(collector.absorb("response.doom_loop_check", confident));
        let raw = stream::iter(vec![Ok(text_delta_event("hi")), Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            Some(collector),
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(
                    error.kind,
                    crate::events::SamplingErrorKind::DoomLoopDetected
                );
                assert!(error.is_retryable);
                assert_eq!(
                    error.doom_loop_triggers.as_deref(),
                    Some(&["tail_repetition:8@thinking".to_string()][..])
                );
            }
            other => panic!("expected Failed(DoomLoopDetected), got {other:?}"),
        }
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, SamplingEvent::Completed { .. }))
        );

        let collector = crate::doom_loop::DoomLoopSignalCollector::default();
        assert!(collector.absorb("response.doom_loop_check", confident));
        collector.disarm_abort();
        let raw = stream::iter(vec![Ok(text_delta_event("hi")), Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            Some(collector),
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.doom_loop_signals.len(), 1);
            }
            other => panic!("expected Completed after disarm, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn doom_loop_signals_empty_without_collector_or_triggers() {
        let raw = stream::iter(vec![Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            None,
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert!(response.doom_loop_signals.is_empty());
            }
            other => panic!("expected Completed, got {other:?}"),
        }

        // A collector that never saw a trigger also leaves the field empty.
        let raw = stream::iter(vec![Ok(completed_event())]).boxed();
        let events = collect(stream_responses(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
            Some(crate::doom_loop::DoomLoopSignalCollector::default()),
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert!(response.doom_loop_signals.is_empty());
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }
}

//! Layer-2 stream transform for the Anthropic Messages API.
//!
//! Consumes a raw `MessageStreamEvent` stream and produces
//! [`SamplingEvent`]s. Pure: no I/O, no shell coupling.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::{BoxStream, Stream};

use sampling_types::messages::{self, MessageStreamEvent};
use sampling_types::{
    AssistantItem, ConversationItem, ConversationResponse, ResponseModelMetadata, SamplingError,
    StopReason, TokenUsage, ToolCall, rs,
};

use super::protocol_failure;
use crate::events::{SamplingChannel, SamplingErrorInfo, SamplingEvent};
use crate::metrics::InferenceLatencyStats;
use crate::types::RequestId;

/// The verbatim wire string for a Messages API stop reason, before it collapses
/// into the internal [`StopReason`]. Uses the enum's serde `snake_case`
/// renaming so it cannot drift from the wire contract.
fn messages_stop_reason_wire(sr: &messages::StopReason) -> String {
    match serde_json::to_value(sr) {
        Ok(serde_json::Value::String(s)) => s,
        other => {
            debug_assert!(
                false,
                "StopReason must serialize to a string, got {other:?}"
            );
            "end_turn".to_string()
        }
    }
}

/// Returns whether a Messages API event reflects real model progress
/// rather than a liveness-only heartbeat (Ping).
pub(crate) fn messages_event_has_meaningful_content(event: &MessageStreamEvent) -> bool {
    match event {
        MessageStreamEvent::Ping => false,
        MessageStreamEvent::ContentBlockDelta { delta, .. } => match delta {
            messages::StreamDelta::TextDelta { text } => !text.is_empty(),
            messages::StreamDelta::ThinkingDelta { thinking } => !thinking.is_empty(),
            messages::StreamDelta::SignatureDelta { signature } => !signature.is_empty(),
            messages::StreamDelta::InputJsonDelta { partial_json } => !partial_json.is_empty(),
        },
        MessageStreamEvent::MessageStart { .. }
        | MessageStreamEvent::MessageDelta { .. }
        | MessageStreamEvent::MessageStop
        | MessageStreamEvent::ContentBlockStart { .. }
        | MessageStreamEvent::ContentBlockStop { .. }
        | MessageStreamEvent::Error { .. } => true,
    }
}

/// Per-block streaming accumulator. The Anthropic Messages API reports
/// content as a sequence of indexed blocks (text / thinking /
/// tool_use), each with start / delta / stop events. We accumulate
/// per-index and finalize each block on `ContentBlockStop`.
struct BlockState {
    block_type: BlockType,
    text_acc: String,
    tool_name: String,
    tool_id: String,
    args_acc: String,
    initial_input: Option<serde_json::Value>,
    args_started: bool,
    thinking_acc: String,
    signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockType {
    Text,
    ToolUse,
    Thinking,
    RedactedThinking,
}

/// Transform a raw Anthropic Messages API stream into a stream of
/// [`SamplingEvent`]s.
///
/// Yields exactly one terminal event ([`SamplingEvent::Completed`] or
/// [`SamplingEvent::Failed`]) per request. Server-side `Error` events
/// preserve the provider's error classification. Protocol violations are
/// non-retryable and never produce executable conversation items.
pub fn stream_messages<'a>(
    raw_stream: BoxStream<'a, Result<MessageStreamEvent, SamplingError>>,
    model_metadata: Option<ResponseModelMetadata>,
    request_id: RequestId,
    idle_timeout: Duration,
) -> impl Stream<Item = SamplingEvent> + Send + 'a {
    async_stream::stream! {
        use messages::{ContentBlock, StreamDelta};

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

        // Per-block accumulators keyed by content block index.
        let mut blocks: BTreeMap<u32, BlockState> = BTreeMap::new();
        let mut seen_blocks = BTreeSet::new();
        let mut message_delta_seen = false;
        let mut message_stop_seen = false;
        // Validate closed tools immediately, but drain to a legitimate terminal
        // usage snapshot before failing. No calls are dispatched by this layer.
        let mut invalid_response: Option<String> = None;

        // Final-message-level accumulators
        let mut final_model: Option<String> = None;
        // Anthropic Messages API `input_tokens` is the uncached portion; cache hits and writes are reported
        // in separate buckets and must be summed for the true total prompt size.
        let mut final_input_tokens: u32 = 0;
        let mut final_cache_read_input_tokens: u32 = 0;
        let mut final_cache_creation_input_tokens: u32 = 0;
        let mut final_output_tokens: u32 = 0;
        let mut final_stop_reason: Option<StopReason> = None;
        let mut final_stop_message: Option<String> = None;
        let mut final_message_id: Option<String> = None;
        let mut final_raw_stop_reason: Option<String> = None;
        // The provider's matched stop sequence (Messages `message_delta.stop_sequence`),
        // set only on a `stop_sequence`-terminated turn; carried through so the
        // headless `streaming-messages-json` consumer can echo it.
        let mut final_stop_sequence: Option<String> = None;

        // Assistant-response accumulators (built up as ContentBlockStop
        // events fire). Reasoning is collected into a synthesized
        // `rs::ReasoningItem` and emitted as a sibling
        // `ConversationItem::Reasoning` before the trailing Assistant.
        let mut assistant_text = String::new();
        let mut assistant_tool_calls: Vec<ToolCall> = Vec::new();
        let mut assistant_reasoning: Option<rs::ReasoningItem> = None;

        // Index counters
        let mut chunk_index: u64 = 0;
        let mut message_chunk_count: u64 = 0;
        let mut first_token_emitted = false;
        let mut last_content_chunk_at = Instant::now();

        // Tool-call index counter for per-tool deltas (separate from
        // the block index, which can be interleaved with text/thinking
        // blocks).
        let mut next_tool_index: u32 = 0;
        let mut block_to_tool_index: BTreeMap<u32, u32> = BTreeMap::new();

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

            let mut event_has_content = messages_event_has_meaningful_content(&event);

            let protocol_error = match &event {
                MessageStreamEvent::Ping | MessageStreamEvent::Error { .. } => None,
                MessageStreamEvent::MessageStart { message } => {
                    if final_message_id.is_some() {
                        Some("duplicate message_start")
                    } else if message.id.trim().is_empty() || message.model.trim().is_empty()
                        || message.r#type != "message" || message.role != "assistant"
                        || !message.content.is_empty() || message.stop_reason.is_some()
                    {
                        Some("invalid message_start")
                    } else { None }
                }
                _ if final_message_id.is_none() => Some("event received before message_start"),
                MessageStreamEvent::ContentBlockStart { index, content_block } => {
                    if message_delta_seen { Some("content received after message_delta") }
                    else if seen_blocks.contains(index) { Some("duplicate content block index") }
                    else if matches!(content_block, ContentBlock::Image { .. } | ContentBlock::ToolResult { .. }) {
                        Some("unsupported assistant content block")
                    } else { None }
                }
                MessageStreamEvent::ContentBlockDelta { index, delta } => {
                    if message_delta_seen { Some("content received after message_delta") }
                    else { match blocks.get(index) {
                        None => Some("delta for an unopened or closed content block"),
                        Some(state) => if matches!((state.block_type, delta),
                            (BlockType::Text, StreamDelta::TextDelta { .. })
                            | (BlockType::Thinking, StreamDelta::ThinkingDelta { .. } | StreamDelta::SignatureDelta { .. })
                            | (BlockType::ToolUse, StreamDelta::InputJsonDelta { .. })
                        ) { None } else { Some("delta type does not match content block") },
                    }}
                }
                MessageStreamEvent::ContentBlockStop { index } => {
                    if message_delta_seen { Some("content received after message_delta") }
                    else if !blocks.contains_key(index) { Some("stop for an unopened or closed content block") }
                    else { None }
                }
                MessageStreamEvent::MessageDelta { delta, .. } => {
                    let reason = delta.stop_reason.as_ref().map(messages_stop_reason_wire);
                    if reason.as_ref().is_some_and(|reason| reason.trim().is_empty()
                        || final_raw_stop_reason.as_ref().is_some_and(|previous| previous != reason)) {
                        Some("conflicting or empty stop_reason")
                    } else if delta.stop_sequence.as_ref().is_some_and(|sequence|
                        final_stop_sequence.as_ref().is_some_and(|previous| previous != sequence)) {
                        Some("conflicting stop_sequence")
                    } else { None }
                }
                MessageStreamEvent::MessageStop => None,
            };
            if let Some(message) = protocol_error {
                yield protocol_failure(&request_id, format!("Messages stream protocol: {message}"), None);
                return;
            }

            match event {
                MessageStreamEvent::MessageStart { message } => {
                    final_message_id = Some(message.id.clone());
                    final_model = Some(message.model.clone());
                    final_input_tokens = message.usage.input_tokens;
                    final_output_tokens = message.usage.output_tokens;
                    final_cache_read_input_tokens = message.usage.cache_read_input_tokens;
                    final_cache_creation_input_tokens = message.usage.cache_creation_input_tokens;
                    // Surface the real id/model/input-usage in order, before any
                    // content, so partial-mode framing emits them on the real
                    // `message_start` instead of a synthesized placeholder.
                    yield SamplingEvent::ResponseStarted {
                        request_id: request_id.clone(),
                        message_id: message.id,
                        model: message.model,
                        input_tokens: u64::from(message.usage.input_tokens),
                        cache_read_input_tokens: u64::from(
                            message.usage.cache_read_input_tokens,
                        ),
                        cache_creation_input_tokens: u64::from(
                            message.usage.cache_creation_input_tokens,
                        ),
                    };
                }

                MessageStreamEvent::ContentBlockStart {
                    index,
                    content_block,
                } => {
                    seen_blocks.insert(index);
                    match content_block {
                    ContentBlock::Thinking {
                        thinking,
                        signature,
                    } => {
                        blocks.insert(
                            index,
                            BlockState {
                                block_type: BlockType::Thinking,
                                text_acc: String::new(),
                                tool_name: String::new(),
                                tool_id: String::new(),
                                args_acc: String::new(),
                                initial_input: None,
                                args_started: false,
                                thinking_acc: thinking.clone(),
                                signature: signature.clone(),
                            },
                        );
                        if !first_token_emitted {
                            first_token_emitted = true;
                            yield SamplingEvent::FirstToken {
                                request_id: request_id.clone(),
                            };
                        }
                    }
                    ContentBlock::Text { text, .. } => {
                        blocks.insert(
                            index,
                            BlockState {
                                block_type: BlockType::Text,
                                text_acc: text.clone(),
                                tool_name: String::new(),
                                tool_id: String::new(),
                                args_acc: String::new(),
                                initial_input: None,
                                args_started: false,
                                thinking_acc: String::new(),
                                signature: String::new(),
                            },
                        );
                        if !first_token_emitted {
                            first_token_emitted = true;
                            yield SamplingEvent::FirstToken {
                                request_id: request_id.clone(),
                            };
                        }
                    }
                    ContentBlock::ToolUse { id, name, input, .. } => {
                        let tool_index = next_tool_index;
                        next_tool_index += 1;
                        block_to_tool_index.insert(index, tool_index);

                        blocks.insert(
                            index,
                            BlockState {
                                block_type: BlockType::ToolUse,
                                text_acc: String::new(),
                                tool_name: name.clone(),
                                tool_id: id.clone(),
                                // Anthropic Messages API streams arguments via
                                // InputJsonDelta events; starting from
                                // "{}" then appending fragments would
                                // produce invalid JSON.
                                args_acc: String::new(),
                                initial_input: Some(input),
                                args_started: false,
                                thinking_acc: String::new(),
                                signature: String::new(),
                            },
                        );

                        // Emit initial id+name so subscribers can pre-allocate
                        // UI for the tool call before arguments stream in.
                        yield SamplingEvent::ToolCallDelta {
                            request_id: request_id.clone(),
                            tool_index,
                            id: Some(id),
                            name: Some(name),
                            arguments_delta: None,
                        };
                    }
                    // Encrypted reasoning the model chose to redact. Deliberately
                    // parse-only: the `RedactedThinking` wire variant exists so a
                    // stream that includes one deserializes instead of failing the
                    // whole event parse and discarding an already-streamed
                    // response, but its opaque `data` blob is not surfaced as a
                    // `SamplingEvent` — forwarding it to the headless reducer's
                    // `redacted_thinking` block would need a new event threaded
                    // through the deferred sampler→shell→reducer hop and handled by
                    // every `SamplingEvent` consumer (TUI included), so it is not
                    // wired. No consumer claims redacted_thinking support.
                    ContentBlock::RedactedThinking { .. } => {
                        blocks.insert(index, BlockState {
                            block_type: BlockType::RedactedThinking,
                            text_acc: String::new(), tool_name: String::new(), tool_id: String::new(),
                            args_acc: String::new(), initial_input: None, args_started: false,
                            thinking_acc: String::new(), signature: String::new(),
                        });
                    }
                    // Image / ToolResult are not expected in assistant streams.
                    _ => {}
                }},

                MessageStreamEvent::ContentBlockDelta { index, delta } => {
                    if let Some(state) = blocks.get_mut(&index) {
                        match delta {
                            StreamDelta::ThinkingDelta { thinking } => {
                                if !thinking.is_empty() {
                                    state.thinking_acc.push_str(&thinking);
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
                                        text: thinking,
                                        chunk_index,
                                    };
                                }
                            }
                            StreamDelta::SignatureDelta { signature } => {
                                state.signature = signature;
                            }
                            StreamDelta::TextDelta { text } => {
                                if !text.is_empty() {
                                    state.text_acc.push_str(&text);
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
                                        text,
                                        chunk_index,
                                    };
                                }
                            }
                            StreamDelta::InputJsonDelta { partial_json } => {
                                state.args_started = true;
                                state.args_acc.push_str(&partial_json);
                                if !partial_json.is_empty() && let Some(&tool_index) = block_to_tool_index.get(&index) {
                                    yield SamplingEvent::ToolCallDelta {
                                        request_id: request_id.clone(),
                                        tool_index,
                                        id: None,
                                        name: None,
                                        arguments_delta: Some(partial_json),
                                    };
                                }
                            }
                        }
                    }
                }

                MessageStreamEvent::ContentBlockStop { index } => {
                    if let Some(mut state) = blocks.remove(&index) {
                        match state.block_type {
                            BlockType::Text => {
                                if !state.text_acc.is_empty() {
                                    if !assistant_text.is_empty() {
                                        assistant_text.push('\n');
                                    }
                                    assistant_text.push_str(&state.text_acc);
                                }
                            }
                            BlockType::Thinking => {
                                // Surface the encrypted signature in order (at the
                                // thinking block's stop) so partial-mode framing can
                                // emit `signature_delta` before its `content_block_stop`.
                                if !state.signature.is_empty() {
                                    yield SamplingEvent::ReasoningCompleted {
                                        request_id: request_id.clone(),
                                        signature: state.signature.clone(),
                                    };
                                }
                                if !state.thinking_acc.is_empty() || !state.signature.is_empty() {
                                    // Anthropic Messages API `Thinking` blocks uniquely
                                    // carry an encrypted `signature` distinct
                                    // from the text; either field may be
                                    // empty. Build directly rather than via
                                    // `synthesized_reasoning_item` since the
                                    // helper assumes a non-empty summary.
                                    let summary = if state.thinking_acc.is_empty() {
                                        vec![]
                                    } else {
                                        vec![rs::SummaryPart::SummaryText(
                                            rs::SummaryTextContent {
                                                text: state.thinking_acc,
                                            },
                                        )]
                                    };
                                    let encrypted_content = if state.signature.is_empty() {
                                        None
                                    } else {
                                        Some(state.signature)
                                    };
                                    assistant_reasoning = Some(rs::ReasoningItem {
                                        // async-openai >= 0.41: `id` is `Option<String>`.
                                        id: Some(String::new()),
                                        summary,
                                        content: None,
                                        encrypted_content,
                                        status: None,
                                    });
                                }
                            }
                            BlockType::ToolUse => {
                                let input = state.initial_input.take().expect("tool block has initial input");
                                let initial_valid = input.is_object()
                                    && (!state.args_started || input.as_object().is_some_and(|object| object.is_empty()));
                                if !state.args_started {
                                    state.args_acc = input.to_string();
                                }
                                if !initial_valid || !state.args_acc.trim_start().starts_with('{')
                                    || serde_json::from_str::<serde::de::IgnoredAny>(&state.args_acc).is_err()
                                {
                                    invalid_response.get_or_insert_with(|| format!(
                                        "invalid or incomplete JSON object at tool block {index}"
                                    ));
                                } else {
                                    assistant_tool_calls.push(ToolCall {
                                        id: std::sync::Arc::<str>::from(state.tool_id),
                                        name: state.tool_name,
                                        arguments: std::sync::Arc::<str>::from(state.args_acc),
                                    });
                                }
                            }
                            BlockType::RedactedThinking => {}
                        }
                    }
                }

                MessageStreamEvent::MessageDelta { delta, usage } => {
                    event_has_content = !message_delta_seen
                        || (final_raw_stop_reason.is_none() && delta.stop_reason.is_some())
                        || final_output_tokens != usage.output_tokens
                        || usage.input_tokens.is_some_and(|n| n != final_input_tokens)
                        || usage.cache_read_input_tokens.is_some_and(|n| n != final_cache_read_input_tokens)
                        || usage.cache_creation_input_tokens.is_some_and(|n| n != final_cache_creation_input_tokens);
                    message_delta_seen = true;
                    if !blocks.is_empty() {
                        invalid_response.get_or_insert_with(|| "message_delta with unclosed content blocks".into());
                    }
                    // Normalize the provider's stop detail to a plain message;
                    // the shell logs it when it surfaces a refusal.
                    if let Some(details) = delta.stop_details {
                        final_stop_message = details.explanation;
                    }
                    // Keep the exact wire string so consumers can echo it.
                    if let Some(reason) = &delta.stop_reason {
                        final_raw_stop_reason = Some(messages_stop_reason_wire(reason));
                    }
                    // The matched stop sequence rides the same terminal delta
                    // (present only on a `stop_sequence` stop); carry it verbatim.
                    if delta.stop_sequence.is_some() {
                        final_stop_sequence = delta.stop_sequence.clone();
                    }
                    final_stop_reason = delta.stop_reason.map(|sr| match sr {
                        messages::StopReason::EndTurn => StopReason::Stop,
                        messages::StopReason::MaxTokens => StopReason::Length,
                        messages::StopReason::StopSequence => StopReason::Stop,
                        messages::StopReason::ToolUse => StopReason::ToolCalls,
                        // The model declined to continue; whatever streamed is
                        // the complete response, so end the turn cleanly.
                        messages::StopReason::Refusal => StopReason::ContentFilter,
                        messages::StopReason::PauseTurn => {
                            // The server-tool loop hit its iteration limit; the
                            // session layer will resend this content to continue.
                            StopReason::PauseTurn
                        }
                        messages::StopReason::ModelContextWindowExceeded => {
                            // Output-side generation hit the context window
                            // limit; the session layer must compact, not
                            // continue — distinct from the user-configured
                            // max_tokens limit.
                            StopReason::ModelContextWindowExceeded
                        }
                        messages::StopReason::Unknown(wire) => {
                            tracing::warn!(
                                wire_stop_reason = %wire,
                                "unrecognized stop_reason in messages stream; treating as stop"
                            );
                            StopReason::Stop
                        }
                    }).or(final_stop_reason);
                    final_output_tokens = usage.output_tokens;
                    // Optional on the delta; preserve message_start values when omitted.
                    if let Some(input) = usage.input_tokens {
                        final_input_tokens = input;
                    }
                    if let Some(cache_read) = usage.cache_read_input_tokens {
                        final_cache_read_input_tokens = cache_read;
                    }
                    if let Some(cache_creation) = usage.cache_creation_input_tokens {
                        final_cache_creation_input_tokens = cache_creation;
                    }
                }

                MessageStreamEvent::MessageStop => {
                    message_stop_seen = true;
                    break;
                }

                MessageStreamEvent::Ping => {
                    // Liveness only, no action; the inner timeout was
                    // already reset above by the successful `next()`.
                }

                MessageStreamEvent::Error { error } => {
                    let err = SamplingError::from_stream_error(error.r#type, error.message);
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }
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
        }

        // ── Build the final response ─────────────────────────────────
        let model_id = final_model.unwrap_or_default();
        // Match the OAI Responses convention: prompt_tokens = full prompt, cached_prompt_tokens = cache hits only.
        let total_prompt_tokens = final_input_tokens
            .saturating_add(final_cache_read_input_tokens)
            .saturating_add(final_cache_creation_input_tokens);
        let usage = if message_stop_seen && final_stop_reason.is_some() {
            Some(TokenUsage {
                prompt_tokens: total_prompt_tokens,
                completion_tokens: final_output_tokens,
                total_tokens: total_prompt_tokens.saturating_add(final_output_tokens),
                reasoning_tokens: 0,
                cached_prompt_tokens: final_cache_read_input_tokens,
                cache_creation_prompt_tokens: final_cache_creation_input_tokens,
            })
        } else {
            None
        };

        if !message_stop_seen || !message_delta_seen || final_stop_reason.is_none() || !blocks.is_empty() {
            invalid_response.get_or_insert_with(|| "stream ended without message_start, closed blocks, stop_reason and message_stop".into());
        }
        if let Some(message) = invalid_response {
            yield protocol_failure(&request_id, format!("Messages stream protocol: {message}"), usage);
            return;
        }

        let stop_reason = if !assistant_tool_calls.is_empty() {
            // Completed tool_use blocks win even over Refusal: the calls are
            // real model output the agent loop must resolve.
            Some(StopReason::ToolCalls)
        } else {
            final_stop_reason
        };

        let assistant_item = ConversationItem::Assistant(AssistantItem {
            content: std::sync::Arc::<str>::from(assistant_text),
            tool_calls: assistant_tool_calls,
            model_id: Some(model_id),
            model_fingerprint: None,
            // The Messages API does not echo the applied reasoning effort.
            reasoning_effort: None,
        });

        let mut items: Vec<ConversationItem> = Vec::new();
        if let Some(r) = assistant_reasoning {
            items.push(ConversationItem::Reasoning(r));
        }
        items.push(assistant_item);

        let stream_end = Instant::now();
        let metrics =
            InferenceLatencyStats::from_timestamps(stream_start, &chunk_timestamps, stream_end);

        let response = ConversationResponse {
            items,
            stop_reason,
            usage,
            // Anthropic Messages API carries no cost on the wire.
            cost_usd_ticks: None,
            message_chunks_emitted: message_chunk_count,
            doom_loop_signals: Vec::new(),
            stop_message: final_stop_message,
            message_id: final_message_id,
            raw_stop_reason: final_raw_stop_reason,
            stop_sequence: final_stop_sequence,
        };

        yield SamplingEvent::Completed {
            request_id: request_id.clone(),
            response: Box::new(response),
            metrics,
        };
    }
}

#[cfg(test)]
#[path = "messages_tests.rs"]
mod tests;

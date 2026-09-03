//! Layer-2 stream transform for the Chat Completions API.
//!
//! Consumes a raw `ChatCompletionChunk` stream and produces
//! [`SamplingEvent`]s. Pure: no I/O, no shell coupling.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use futures_util::stream::{BoxStream, Stream};

use sampling_types::{
    AssistantItem, ChatCompletionChunk, ConversationItem, ConversationResponse,
    ResponseModelMetadata, SamplingError, StopReason, TokenUsage, ToolCall,
};

use super::protocol_failure;
use crate::events::{SamplingChannel, SamplingErrorInfo, SamplingEvent};
use crate::metrics::InferenceLatencyStats;
use crate::types::RequestId;

// Usage is optional and often arrives after finish_reason. Bound the entire
// tail (not each frame) so keepalives cannot hold a completed request open.
const USAGE_TAIL_TIMEOUT: Duration = Duration::from_secs(2);

fn merge_tool_identity(
    current: &mut String,
    incoming: Option<String>,
    field: &str,
    index: u32,
) -> Result<Option<String>, SamplingError> {
    let Some(value) = incoming.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    if current.is_empty() {
        *current = value.clone();
        Ok(Some(value))
    } else if *current == value {
        Ok(None)
    } else {
        Err(SamplingError::Serialization(serde::de::Error::custom(
            format!("Chat stream protocol: conflicting tool {field} at index {index}"),
        )))
    }
}

/// Transform a raw Chat Completions chunk stream into a stream of
/// [`SamplingEvent`]s.
///
/// The output stream emits exactly one terminal event per request:
/// [`SamplingEvent::Completed`] on normal stream end, or
/// [`SamplingEvent::Failed`] on error / idle timeout. Callers must not
/// consume past the terminal event (the implementation `return`s after
/// yielding it).
///
/// `idle_timeout` covers two cases:
/// 1. The transport stops yielding chunks at all (`tokio::time::timeout`).
/// 2. The transport keeps yielding empty / keepalive chunks but no
///    meaningful content (separate `last_content_chunk_at` timer).
///
/// Both produce `SamplingEvent::Failed { kind: IdleTimeout }`.
pub fn stream_chat_completions<'a>(
    raw_stream: BoxStream<'a, Result<ChatCompletionChunk, SamplingError>>,
    model_metadata: Option<ResponseModelMetadata>,
    request_id: RequestId,
    idle_timeout: Duration,
) -> impl Stream<Item = SamplingEvent> + Send + 'a {
    async_stream::stream! {
        let stream_start = Instant::now();
        let mut chunk_timestamps: Vec<Instant> = Vec::new();

        // Emit StreamStarted before reading any chunks so subscribers
        // can record TTFB / TTLB baselines.
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

        // Per-response accumulators
        let mut first_chunk_seen = false;
        let mut first_choice_seen = false;
        let mut response_id = String::new();
        let mut first_token_emitted = false;
        let mut model: String = String::new();
        let mut model_fingerprint: Option<String> = None;
        let mut usage: Option<TokenUsage> = None;
        let mut cost_usd_ticks: Option<i64> = None;
        let mut finish_reason: Option<StopReason> = None;
        let mut raw_finish_reason: Option<String> = None;
        let mut tail_deadline: Option<tokio::time::Instant> = None;

        let mut content_acc = String::new();
        let mut reasoning_acc = String::new();
        // Tool call deltas keyed by positional index. Each entry is
        // (id, name, arguments_buffer); the first chunk for an index
        // carries id+name and starts the arguments buffer, subsequent
        // chunks append to arguments only.
        let mut tool_call_acc: BTreeMap<u32, (String, String, String)> = BTreeMap::new();

        // Index counter spanning text + reasoning chunks (matches the
        // shell's chunk_index used for notification correlation).
        let mut chunk_index: u64 = 0;
        // Separate counter for AgentMessageChunk (text-only) emissions;
        // mirrored onto ConversationResponse.message_chunks_emitted so
        // downstream can detect lost-streaming-events scenarios.
        let mut message_chunk_count: u64 = 0;

        // Content-aware idle timer: the outer
        // `tokio::time::timeout(idle_timeout, stream.next())` already
        // catches "transport stops yielding chunks". This second timer
        // catches the more subtle case where the model keeps emitting
        // keepalive / empty-delta SSE events that satisfy the outer
        // timer but make no real progress -- some inference engines
        // do exactly that.
        let mut last_content_chunk_at = Instant::now();

        let mut stream = raw_stream;
        loop {
            let timeout = tail_deadline.map_or(idle_timeout, |deadline| {
                deadline.saturating_duration_since(tokio::time::Instant::now()).min(idle_timeout)
            });
            if timeout.is_zero() && finish_reason.is_some() {
                break;
            }
            let next = match tokio::time::timeout(timeout, stream.next()).await {
                Ok(Some(next)) => next,
                Ok(None) => break, // stream ended normally
                Err(_elapsed) => {
                    if finish_reason.is_some() {
                        break;
                    }
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
            let chunk = match next {
                Ok(chunk) => chunk,
                Err(err) => {
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }
            };

            if !first_chunk_seen {
                response_id = chunk.id.clone();
                model = chunk.model.clone();
                model_fingerprint = chunk
                    .system_fingerprint
                    .clone()
                    .filter(|s| !s.is_empty());
                first_chunk_seen = true;
            } else if chunk.id != response_id {
                yield protocol_failure(&request_id, "Chat stream protocol: conflicting response id", None);
                return;
            }

            if chunk.choices.len() > 1 {
                yield protocol_failure(&request_id, "Chat stream protocol: multiple choices in a single-candidate response", None);
                return;
            }

            if let Some(u) = chunk.usage.clone() {
                // Wire cost is cumulative for the response, so last-write-wins.
                // Never clobber a known cost with missing/unreported.
                let chunk_cost = sampling_types::reported_cost_ticks(u.cost_in_usd_ticks);
                cost_usd_ticks = match (cost_usd_ticks, chunk_cost) {
                    (_, Some(n)) => Some(n),
                    (prev, None) => prev,
                };
                usage = Some(u.into());
            }

            // Track whether this chunk carried meaningful content.
            // Set inside the choices loop and checked at the end.
            let mut chunk_has_content = false;

            for choice in chunk.choices.into_iter() {
                // Grow requests one candidate. Never combine different
                // candidates' text or executable calls into one response.
                if choice.index != 0 {
                    let err = SamplingError::Serialization(serde::de::Error::custom(
                        "Chat stream protocol: unexpected nonzero choice index",
                    ));
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }
                first_choice_seen = true;
                let already_finished = finish_reason.is_some();
                if let Some(fr) = choice.finish_reason {
                    if fr.as_str().trim().is_empty()
                        || raw_finish_reason.as_deref().is_some_and(|previous| previous != fr.as_str()) {
                        yield protocol_failure(&request_id, "Chat stream protocol: conflicting finish_reason", usage);
                        return;
                    }
                    raw_finish_reason = Some(fr.as_str().to_owned());
                    finish_reason = Some(fr.into());
                    tail_deadline.get_or_insert_with(|| tokio::time::Instant::now() + USAGE_TAIL_TIMEOUT);
                    chunk_has_content |= !already_finished;
                }

                let delta = choice.delta;
                if already_finished
                    && (delta.content.as_ref().is_some_and(|text| !text.is_empty())
                        || delta.reasoning_content.as_ref().is_some_and(|text| !text.is_empty())
                        || !delta.tool_calls.is_empty())
                {
                    let err = SamplingError::Serialization(serde::de::Error::custom(
                        "Chat stream protocol: output received after finish_reason",
                    ));
                    yield SamplingEvent::Failed {
                        request_id: request_id.clone(),
                        error: SamplingErrorInfo::from(&err),
                    };
                    return;
                }

                if let Some(text) = delta.content
                    && !text.is_empty()
                {
                    if !first_token_emitted {
                        first_token_emitted = true;
                        yield SamplingEvent::FirstToken {
                            request_id: request_id.clone(),
                        };
                    }
                    chunk_has_content = true;
                    chunk_timestamps.push(Instant::now());
                    chunk_index += 1;
                    message_chunk_count += 1;
                    content_acc.push_str(&text);
                    yield SamplingEvent::ChannelToken {
                        request_id: request_id.clone(),
                        channel: SamplingChannel::Text,
                        text,
                        chunk_index,
                    };
                }

                if let Some(thought) = delta.reasoning_content
                    && !thought.is_empty()
                {
                    if !first_token_emitted {
                        first_token_emitted = true;
                        yield SamplingEvent::FirstToken {
                            request_id: request_id.clone(),
                        };
                    }
                    chunk_has_content = true;
                    chunk_index += 1;
                    reasoning_acc.push_str(&thought);
                    yield SamplingEvent::ChannelToken {
                        request_id: request_id.clone(),
                        channel: SamplingChannel::Reasoning,
                        text: thought,
                        chunk_index,
                    };
                }

                for tc_delta in delta.tool_calls.into_iter() {
                    if tc_delta.kind.as_deref().is_some_and(|kind| kind != "function") {
                        yield protocol_failure(&request_id, "Chat stream protocol: unsupported tool-call type", None);
                        return;
                    }
                    let func = tc_delta.function.unwrap_or_default();
                    if tc_delta.id.as_ref().is_none_or(|id| id.trim().is_empty())
                        && func.name.as_ref().is_none_or(|name| name.trim().is_empty())
                        && func.arguments.as_ref().is_none_or(String::is_empty)
                    {
                        continue;
                    }
                    let entry = tool_call_acc
                        .entry(tc_delta.index)
                        .or_insert_with(|| (String::new(), String::new(), String::new()));

                    let identity = merge_tool_identity(&mut entry.0, tc_delta.id, "id", tc_delta.index)
                        .and_then(|id| {
                            merge_tool_identity(&mut entry.1, func.name, "name", tc_delta.index)
                                .map(|name| (id, name))
                        });
                    let (id_for_event, name_for_event) = match identity {
                        Ok(identity) => identity,
                        Err(err) => {
                            yield SamplingEvent::Failed {
                                request_id: request_id.clone(),
                                error: SamplingErrorInfo::from(&err),
                            };
                            return;
                        }
                    };
                    let args_for_event = func.arguments.filter(|args| !args.is_empty());
                    if let Some(args) = &args_for_event {
                        entry.2.push_str(args);
                    }
                    if id_for_event.is_none() && name_for_event.is_none() && args_for_event.is_none() {
                        continue;
                    }
                    chunk_has_content = true;

                    yield SamplingEvent::ToolCallDelta {
                        request_id: request_id.clone(),
                        tool_index: tc_delta.index,
                        id: id_for_event,
                        name: name_for_event,
                        arguments_delta: args_for_event,
                    };
                }
            }

            if chunk_has_content {
                last_content_chunk_at = Instant::now();
            } else if last_content_chunk_at.elapsed() > idle_timeout {
                if finish_reason.is_some() {
                    break;
                }
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

        // EOF is not completion evidence, even for text-only or empty output.
        if !first_choice_seen || finish_reason.is_none() {
            yield protocol_failure(&request_id, "Chat stream protocol: stream ended without a choice finish_reason", None);
            return;
        }
        for (index, (_, _, arguments)) in &tool_call_acc {
            if serde_json::from_str::<serde::de::IgnoredAny>(arguments).is_err() {
                let err = SamplingError::Serialization(serde::de::Error::custom(format!(
                    "Chat stream protocol: incomplete or invalid JSON arguments at tool index {index}"
                )));
                let mut error = SamplingErrorInfo::from(&err);
                error.usage = usage;
                yield SamplingEvent::Failed {
                    request_id: request_id.clone(),
                    error,
                };
                return;
            }
        }

        // ── Build the final response ─────────────────────────────────
        let tool_calls: Vec<ToolCall> = tool_call_acc
            .into_values()
            .map(|(id, name, arguments)| ToolCall {
                id: std::sync::Arc::<str>::from(id),
                name,
                arguments: std::sync::Arc::<str>::from(arguments),
            })
            .collect();

        // A complete call can accompany length or another terminal reason.
        if !tool_calls.is_empty() {
            finish_reason = Some(StopReason::ToolCalls);
        }

        // Build the trailing Assistant + any reasoning sibling.
        let mut items: Vec<ConversationItem> = Vec::new();
        if first_choice_seen {
            if !reasoning_acc.is_empty() {
                items.push(ConversationItem::Reasoning(
                    sampling_types::synthesized_reasoning_item(reasoning_acc),
                ));
            }
            items.push(ConversationItem::Assistant(AssistantItem {
                content: std::sync::Arc::<str>::from(content_acc),
                tool_calls,
                model_id: Some(model),
                model_fingerprint,
                // Chat Completions does not echo the applied reasoning effort.
                reasoning_effort: None,
            }));
        } else {
            items.push(ConversationItem::assistant(""));
        }

        let stream_end = Instant::now();
        let metrics =
            InferenceLatencyStats::from_timestamps(stream_start, &chunk_timestamps, stream_end);

        let response = ConversationResponse {
            items,
            stop_reason: finish_reason,
            usage,
            cost_usd_ticks,
            message_chunks_emitted: message_chunk_count,
            doom_loop_signals: Vec::new(),
            stop_message: None,
            message_id: None,
            raw_stop_reason: raw_finish_reason,
            stop_sequence: None,
        };

        yield SamplingEvent::Completed {
            request_id: request_id.clone(),
            response: Box::new(response),
            metrics,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use sampling_types::{
        ChatChunkChoice, ChatChunkDelta, FinishReason, Role, ToolCallDelta as ChunkToolCallDelta,
        ToolCallFunctionDelta, Usage, rs,
    };
    use std::pin::pin;

    fn rid() -> RequestId {
        RequestId::from("test-req")
    }

    fn make_chunk(deltas: Vec<ChatChunkDelta>) -> ChatCompletionChunk {
        ChatCompletionChunk {
            id: "chunk-1".into(),
            object: "chat.completion.chunk".into(),
            created: 0,
            model: "test-model".into(),
            choices: deltas
                .into_iter()
                .enumerate()
                .map(|(i, delta)| ChatChunkChoice {
                    index: i as u32,
                    delta,
                    finish_reason: None,
                })
                .collect(),
            usage: None,
            system_fingerprint: None,
        }
    }

    fn text_chunk(text: &str) -> ChatCompletionChunk {
        make_chunk(vec![ChatChunkDelta {
            role: Some(Role::Assistant),
            content: Some(text.to_string()),
            reasoning_content: None,
            tool_calls: vec![],
            tool_call_id: None,
        }])
    }

    fn final_chunk(reason: FinishReason) -> ChatCompletionChunk {
        let mut chunk = make_chunk(vec![ChatChunkDelta::default()]);
        chunk.choices[0].finish_reason = Some(reason);
        chunk
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
    async fn empty_stream_yields_started_then_protocol_failure() {
        let raw = stream::iter(Vec::<Result<ChatCompletionChunk, SamplingError>>::new()).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;

        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], SamplingEvent::StreamStarted { .. }));
        match &events[1] {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(error.kind, crate::events::SamplingErrorKind::Serialization);
                assert!(!error.is_retryable);
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn text_eof_and_conflicting_terminals_are_protocol_failures() {
        let mut wrong_id = text_chunk("wrong");
        wrong_id.id = "another-response".into();
        let mut unknown_tool = make_chunk(vec![ChatChunkDelta {
            tool_calls: vec![ChunkToolCallDelta {
                index: 0,
                id: Some("call_bad".into()),
                kind: Some("vendor_tool".into()),
                function: Some(ToolCallFunctionDelta {
                    name: Some("test_tool".into()),
                    arguments: Some("{}".into()),
                }),
            }],
            ..Default::default()
        }]);
        unknown_tool.choices[0].finish_reason = Some(FinishReason::ToolCalls);
        for chunks in [
            vec![text_chunk("partial")],
            vec![make_chunk(vec![])],
            vec![text_chunk("hello"), wrong_id],
            vec![
                final_chunk(FinishReason::Stop),
                final_chunk(FinishReason::Length),
            ],
            vec![final_chunk(FinishReason::Stop), text_chunk("late")],
            vec![unknown_tool],
        ] {
            let events = collect(stream_chat_completions(
                stream::iter(chunks.into_iter().map(Ok)).boxed(),
                None,
                rid(),
                Duration::from_secs(1),
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
    async fn completed_chat_tail_is_bounded_without_inventing_usage() {
        let prefix = stream::iter([Ok(text_chunk("done")), Ok(final_chunk(FinishReason::Stop))]);
        let tail = stream::unfold((), |_| async {
            tokio::time::sleep(Duration::from_millis(3)).await;
            Some((Ok(final_chunk(FinishReason::Stop)), ()))
        });
        let events = tokio::time::timeout(
            Duration::from_secs(1),
            collect(stream_chat_completions(
                prefix.chain(tail).boxed(),
                None,
                rid(),
                Duration::from_millis(20),
            )),
        )
        .await
        .unwrap();
        let Some(SamplingEvent::Completed { response, .. }) = events.last() else {
            panic!("{events:?}");
        };
        assert_eq!(response.assistant().unwrap().content.as_ref(), "done");
        assert!(response.usage.is_none());
    }

    #[tokio::test]
    async fn text_only_stream_emits_first_token_then_channel_tokens_then_completed() {
        let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
            Ok(text_chunk("Hello, ")),
            Ok(text_chunk("world!")),
            Ok(final_chunk(FinishReason::Stop)),
        ];
        let raw = stream::iter(chunks).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;

        // Expected sequence: StreamStarted, FirstToken, ChannelToken(Text)
        // x 2, Completed.
        assert!(matches!(events[0], SamplingEvent::StreamStarted { .. }));
        assert!(matches!(events[1], SamplingEvent::FirstToken { .. }));

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
        assert_eq!(text_tokens, vec!["Hello, ", "world!"]);

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let a = response.assistant().expect("assistant item present");
                assert_eq!(a.content.as_ref(), "Hello, world!");
                assert_eq!(response.stop_reason, Some(StopReason::Stop));
                assert_eq!(response.raw_stop_reason.as_deref(), Some("stop"));
                assert_eq!(response.message_chunks_emitted, 2);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn known_finish_reasons_preserve_raw_wire_value_beside_typed_reason() {
        for (wire, typed, raw_reason) in [
            (FinishReason::Stop, StopReason::Stop, "stop"),
            (FinishReason::Length, StopReason::Length, "length"),
            (FinishReason::ToolCalls, StopReason::ToolCalls, "tool_calls"),
            (
                FinishReason::FunctionCall,
                StopReason::ToolCalls,
                "function_call",
            ),
            (
                FinishReason::ContentFilter,
                StopReason::ContentFilter,
                "content_filter",
            ),
        ] {
            let raw = stream::iter(vec![Ok(text_chunk("done")), Ok(final_chunk(wire))]).boxed();
            let events = collect(stream_chat_completions(
                raw,
                None,
                rid(),
                Duration::from_secs(60),
            ))
            .await;

            match events.last().unwrap() {
                SamplingEvent::Completed { response, .. } => {
                    assert_eq!(response.stop_reason, Some(typed));
                    assert_eq!(response.raw_stop_reason.as_deref(), Some(raw_reason));
                }
                other => panic!("expected Completed, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn unknown_finish_reason_preserves_content_usage_and_raw_reason() {
        let mut terminal = final_chunk(FinishReason::Unknown("unexpected_state".into()));
        terminal.usage = Some(Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            prompt_tokens_details: None,
            completion_tokens_details: None,
            cost_in_usd_ticks: None,
        });
        let raw = stream::iter(vec![Ok(text_chunk("done")), Ok(terminal)]).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.assistant_text(), "done");
                assert_eq!(response.stop_reason, Some(StopReason::Stop));
                assert_eq!(
                    response.raw_stop_reason.as_deref(),
                    Some("unexpected_state")
                );
                let usage = response.usage.as_ref().expect("terminal usage preserved");
                assert_eq!(usage.prompt_tokens, 100);
                assert_eq!(usage.completion_tokens, 50);
                assert_eq!(usage.total_tokens, 150);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unknown_finish_reason_with_tool_call_normalizes_to_tool_calls() {
        let tool_chunk = make_chunk(vec![ChatChunkDelta {
            role: Some(Role::Assistant),
            content: None,
            reasoning_content: None,
            tool_calls: vec![ChunkToolCallDelta {
                index: 0,
                id: Some("call_abc".into()),
                kind: Some("function".into()),
                function: Some(ToolCallFunctionDelta {
                    name: Some("do_thing".into()),
                    arguments: Some("{}".into()),
                }),
            }],
            tool_call_id: None,
        }]);
        let raw = stream::iter(vec![
            Ok(tool_chunk),
            Ok(final_chunk(FinishReason::Unknown(
                "unexpected_state".into(),
            ))),
        ])
        .boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.stop_reason, Some(StopReason::ToolCalls));
                assert_eq!(response.tool_calls().len(), 1);
                assert_eq!(
                    response.raw_stop_reason.as_deref(),
                    Some("unexpected_state")
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn complete_tool_call_overrides_typed_length_but_preserves_raw_length() {
        let tool_chunk = make_chunk(vec![ChatChunkDelta {
            role: Some(Role::Assistant),
            content: None,
            reasoning_content: None,
            tool_calls: vec![ChunkToolCallDelta {
                index: 0,
                id: Some("call_length".into()),
                kind: Some("function".into()),
                function: Some(ToolCallFunctionDelta {
                    name: Some("do_thing".into()),
                    arguments: Some("{}".into()),
                }),
            }],
            tool_call_id: None,
        }]);
        let raw = stream::iter(vec![Ok(tool_chunk), Ok(final_chunk(FinishReason::Length))]).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.stop_reason, Some(StopReason::ToolCalls));
                assert_eq!(response.raw_stop_reason.as_deref(), Some("length"));
                assert_eq!(response.tool_calls().len(), 1);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reasoning_only_unknown_finish_reason_remains_empty_for_resampling() {
        let reasoning = make_chunk(vec![ChatChunkDelta {
            role: Some(Role::Assistant),
            content: None,
            reasoning_content: Some("unfinished reasoning".into()),
            tool_calls: vec![],
            tool_call_id: None,
        }]);
        let raw = stream::iter(vec![
            Ok(reasoning),
            Ok(final_chunk(FinishReason::Unknown(
                "unexpected_state".into(),
            ))),
        ])
        .boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert!(response.is_empty());
                assert_eq!(response.stop_reason, Some(StopReason::Stop));
                assert_eq!(
                    response.raw_stop_reason.as_deref(),
                    Some("unexpected_state")
                );
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reasoning_chunk_emits_reasoning_channel_and_first_token_once() {
        let mut reasoning_chunk = make_chunk(vec![ChatChunkDelta {
            role: Some(Role::Assistant),
            content: None,
            reasoning_content: Some("thinking...".into()),
            tool_calls: vec![],
            tool_call_id: None,
        }]);
        reasoning_chunk.choices[0].finish_reason = None;

        let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
            Ok(reasoning_chunk),
            Ok(text_chunk("done")),
            Ok(final_chunk(FinishReason::Stop)),
        ];
        let raw = stream::iter(chunks).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;

        // FirstToken should appear exactly once.
        let first_token_count = events
            .iter()
            .filter(|e| matches!(e, SamplingEvent::FirstToken { .. }))
            .count();
        assert_eq!(first_token_count, 1);

        let mut saw_reasoning = false;
        let mut saw_text = false;
        for e in &events {
            if let SamplingEvent::ChannelToken { channel, text, .. } = e {
                match channel {
                    SamplingChannel::Reasoning => {
                        assert_eq!(text, "thinking...");
                        saw_reasoning = true;
                    }
                    SamplingChannel::Text => {
                        assert_eq!(text, "done");
                        saw_text = true;
                    }
                }
            }
        }
        assert!(saw_reasoning && saw_text);

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let r = response
                    .reasoning_items()
                    .next()
                    .expect("reasoning sibling preserved");
                let rs::SummaryPart::SummaryText(t) = &r.summary[0];
                assert_eq!(t.text, "thinking...");
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn tool_call_stream_emits_deltas_and_assembles_final_call() {
        // First chunk has id + name + part of arguments.
        let chunk1 = make_chunk(vec![ChatChunkDelta {
            role: None,
            content: None,
            reasoning_content: None,
            tool_calls: vec![ChunkToolCallDelta {
                index: 0,
                id: Some("call_abc".into()),
                kind: Some("function".into()),
                function: Some(ToolCallFunctionDelta {
                    name: Some("do_thing".into()),
                    arguments: Some("{\"x\":".into()),
                }),
            }],
            tool_call_id: None,
        }]);
        // Second chunk has only argument fragment.
        let chunk2 = make_chunk(vec![ChatChunkDelta {
            role: None,
            content: None,
            reasoning_content: None,
            tool_calls: vec![ChunkToolCallDelta {
                index: 0,
                id: None,
                kind: None,
                function: Some(ToolCallFunctionDelta {
                    name: None,
                    arguments: Some("1}".into()),
                }),
            }],
            tool_call_id: None,
        }]);

        let raw = stream::iter::<Vec<Result<ChatCompletionChunk, SamplingError>>>(vec![
            Ok(chunk1),
            Ok(chunk2),
            Ok(final_chunk(FinishReason::ToolCalls)),
        ])
        .boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;

        let deltas: Vec<_> = events
            .iter()
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
            .collect();

        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0].0, 0);
        assert_eq!(deltas[0].1.as_deref(), Some("call_abc"));
        assert_eq!(deltas[0].2.as_deref(), Some("do_thing"));
        assert_eq!(deltas[0].3.as_deref(), Some("{\"x\":"));
        assert_eq!(deltas[1].1, None);
        assert_eq!(deltas[1].2, None);
        assert_eq!(deltas[1].3.as_deref(), Some("1}"));

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let calls = response.tool_calls();
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id.as_ref(), "call_abc");
                assert_eq!(calls[0].name, "do_thing");
                assert_eq!(calls[0].arguments.as_ref(), "{\"x\":1}");
                // Tool calls force ToolCalls stop reason.
                assert_eq!(response.stop_reason, Some(StopReason::ToolCalls));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    fn tool_delta(
        index: u32,
        id: Option<&str>,
        name: Option<&str>,
        args: &str,
    ) -> ChatCompletionChunk {
        make_chunk(vec![ChatChunkDelta {
            tool_calls: vec![ChunkToolCallDelta {
                index,
                id: id.map(str::to_owned),
                function: Some(ToolCallFunctionDelta {
                    name: name.map(str::to_owned),
                    arguments: Some(args.to_owned()),
                }),
                ..Default::default()
            }],
            ..Default::default()
        }])
    }

    #[tokio::test]
    async fn tool_identity_continuations_preserve_final_and_preview_identity() {
        for continuation in [None, Some(""), Some(" "), Some("call_1")] {
            let repeated_name = if continuation == Some("call_1") {
                Some("lookup")
            } else {
                continuation
            };
            let chunks = vec![
                Ok(tool_delta(0, Some("call_1"), Some("lookup"), "{\"x\":")),
                Ok(tool_delta(1, Some("call_2"), Some("other"), "{}")),
                Ok(tool_delta(0, continuation, repeated_name, "1}")),
                Ok(final_chunk(FinishReason::ToolCalls)),
            ];
            let events = collect(stream_chat_completions(
                stream::iter(chunks).boxed(),
                None,
                rid(),
                Duration::from_secs(60),
            ))
            .await;
            let Some(SamplingEvent::Completed { response, .. }) = events.last() else {
                panic!("{events:?}")
            };
            let calls = response.tool_calls();
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0].id.as_ref(), "call_1");
            assert_eq!(calls[0].name, "lookup");
            assert_eq!(calls[0].arguments.as_ref(), "{\"x\":1}");
            assert_eq!(calls[1].id.as_ref(), "call_2");
            let ids: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    SamplingEvent::ToolCallDelta { id: Some(id), .. } => Some(id.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(ids, ["call_1", "call_2"]);
        }
    }

    #[tokio::test]
    async fn conflicting_identity_and_bare_tool_eof_fail_closed() {
        for chunks in [
            vec![Ok(tool_delta(0, Some("a"), Some("f"), "{}"))],
            vec![
                Ok(tool_delta(0, Some("a"), Some("f"), "{")),
                Ok(tool_delta(0, Some("b"), None, "}")),
            ],
            vec![
                Ok(tool_delta(0, Some("a"), Some("f"), "{")),
                Ok(tool_delta(0, None, Some("g"), "}")),
            ],
        ] {
            let events = collect(stream_chat_completions(
                stream::iter(chunks).boxed(),
                None,
                rid(),
                Duration::from_secs(60),
            ))
            .await;
            let Some(SamplingEvent::Failed { error, .. }) = events.last() else {
                panic!("{events:?}")
            };
            assert_eq!(error.kind, crate::events::SamplingErrorKind::Serialization);
            assert!(!error.is_retryable);
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, SamplingEvent::Completed { .. }))
            );
        }
    }

    #[tokio::test]
    async fn arguments_can_precede_identity_and_usage_can_follow_finish() {
        let mut usage = make_chunk(vec![]);
        usage.usage = Some(Usage {
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
            prompt_tokens_details: None,
            completion_tokens_details: None,
            cost_in_usd_ticks: None,
        });
        let chunks = vec![
            Ok(tool_delta(0, None, None, "{}")),
            Ok(tool_delta(0, Some("a"), Some("f"), "")),
            Ok(final_chunk(FinishReason::ToolCalls)),
            Ok(usage),
        ];
        let events = collect(stream_chat_completions(
            stream::iter(chunks).boxed(),
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;
        let Some(SamplingEvent::Completed { response, .. }) = events.last() else {
            panic!("{events:?}")
        };
        assert_eq!(response.tool_calls()[0].id.as_ref(), "a");
        assert_eq!(response.usage.as_ref().unwrap().total_tokens, 3);
    }

    #[test]
    fn tool_delta_requires_an_explicit_index() {
        assert!(
            serde_json::from_str::<ChunkToolCallDelta>(
                r#"{"id":"a","function":{"name":"f","arguments":"{}"}}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ChunkToolCallDelta>(
                r#"{"index":0,"id":null,"function":{"name":null,"arguments":"{}"}}"#
            )
            .is_ok()
        );
    }

    #[tokio::test]
    async fn truncated_json_and_multiple_choices_never_complete_tools() {
        let mut other_choice = tool_delta(0, Some("a"), Some("f"), "{}");
        other_choice.choices[0].index = 1;
        for first in [tool_delta(0, Some("a"), Some("f"), "{\"x\":"), other_choice] {
            let events = collect(stream_chat_completions(
                stream::iter(vec![Ok(first), Ok(final_chunk(FinishReason::Length))]).boxed(),
                None,
                rid(),
                Duration::from_secs(60),
            ))
            .await;
            assert!(matches!(events.last(), Some(SamplingEvent::Failed { .. })));
        }
    }

    #[tokio::test]
    async fn arbitrary_utf8_argument_splits_and_empty_identity_are_equivalent() {
        let args = r#"{"city":"杭州","nested":{"ok":true}}"#;
        for split in args
            .char_indices()
            .map(|(index, _)| index)
            .chain([args.len()])
        {
            let events = collect(stream_chat_completions(
                stream::iter(vec![
                    Ok(tool_delta(0, Some("a"), Some("f"), &args[..split])),
                    Ok(tool_delta(0, Some(""), None, &args[split..])),
                    Ok(final_chunk(FinishReason::ToolCalls)),
                ])
                .boxed(),
                None,
                rid(),
                Duration::from_secs(60),
            ))
            .await;
            let Some(SamplingEvent::Completed { response, .. }) = events.last() else {
                panic!("split {split}: {events:?}")
            };
            assert_eq!(response.tool_calls()[0].arguments.as_ref(), args);
        }
    }

    #[tokio::test]
    async fn no_op_tool_deltas_do_not_create_calls_or_keep_the_stream_alive() {
        let chunks = vec![
            Ok(tool_delta(0, Some(""), Some(" "), "")),
            Ok(text_chunk("answer")),
            Ok(final_chunk(FinishReason::Stop)),
        ];
        let events = collect(stream_chat_completions(
            stream::iter(chunks).boxed(),
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;
        let Some(SamplingEvent::Completed { response, .. }) = events.last() else {
            panic!("{events:?}")
        };
        assert!(response.tool_calls().is_empty());
        assert_eq!(response.assistant_text(), "answer");

        let wire = async_stream::stream! {
            yield Ok(tool_delta(0, Some("a"), Some("f"), "{"));
            loop {
                tokio::time::sleep(Duration::from_millis(1)).await;
                yield Ok(tool_delta(0, Some("a"), Some("f"), ""));
            }
        };
        let events = tokio::time::timeout(
            Duration::from_secs(2),
            collect(stream_chat_completions(
                wire.boxed(),
                None,
                rid(),
                Duration::from_millis(30),
            )),
        )
        .await
        .unwrap();
        assert!(
            matches!(events.last(), Some(SamplingEvent::Failed { error, .. }) if error.kind == crate::events::SamplingErrorKind::IdleTimeout)
        );
    }

    #[tokio::test]
    async fn mid_stream_error_yields_failed_no_completed() {
        let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
            Ok(text_chunk("hi")),
            Err(SamplingError::EventStreamError("conn reset".into())),
        ];
        let raw = stream::iter(chunks).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
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
        // A stream that yields one chunk then hangs forever.
        let raw = stream::iter(vec![Ok(text_chunk("hello"))])
            .chain(stream::pending())
            .boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_millis(100),
        ))
        .await;

        // Stream should emit StreamStarted, FirstToken, ChannelToken
        // then Failed(IdleTimeout) when the stall hits the deadline.
        match events.last().unwrap() {
            SamplingEvent::Failed { error, .. } => {
                assert_eq!(error.kind, crate::events::SamplingErrorKind::IdleTimeout);
            }
            other => panic!("expected Failed(IdleTimeout), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn model_metadata_yielded_after_stream_started() {
        let raw = stream::iter(Vec::<Result<ChatCompletionChunk, SamplingError>>::new()).boxed();
        let metadata = ResponseModelMetadata {
            context_window: Some(8192),
            output_limit: Some(4096),
            models_etag: None,
        };
        let events = collect(stream_chat_completions(
            raw,
            Some(metadata.clone()),
            rid(),
            Duration::from_secs(60),
        ))
        .await;

        assert!(matches!(events[0], SamplingEvent::StreamStarted { .. }));
        match &events[1] {
            SamplingEvent::ModelMetadata { metadata: m, .. } => {
                assert_eq!(m.context_window, Some(8192));
                assert_eq!(m.output_limit, Some(4096));
            }
            other => panic!("expected ModelMetadata second, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn usage_is_extracted_from_chunk() {
        let mut chunk_with_usage = make_chunk(vec![ChatChunkDelta::default()]);
        chunk_with_usage.usage = Some(Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            prompt_tokens_details: None,
            completion_tokens_details: None,
            cost_in_usd_ticks: None,
        });

        let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
            Ok(text_chunk("ok")),
            Ok(chunk_with_usage),
            Ok(final_chunk(FinishReason::Stop)),
        ];
        let raw = stream::iter(chunks).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;

        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                let u = response.usage.as_ref().expect("usage extracted");
                assert_eq!(u.prompt_tokens, 100);
                assert_eq!(u.completion_tokens, 50);
                assert_eq!(u.total_tokens, 150);
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    /// Server-reported cost lands on the response; the REST mapper's `0`
    /// backfill means "unreported" and must yield `None`.
    #[tokio::test]
    async fn cost_is_extracted_and_zero_is_unreported() {
        for (wire, expected) in [(Some(78), Some(78)), (Some(0), None), (None, None)] {
            let mut chunk_with_usage = make_chunk(vec![ChatChunkDelta::default()]);
            chunk_with_usage.usage = Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                prompt_tokens_details: None,
                completion_tokens_details: None,
                cost_in_usd_ticks: wire,
            });
            let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
                Ok(text_chunk("ok")),
                Ok(chunk_with_usage),
                Ok(final_chunk(FinishReason::Stop)),
            ];
            let raw = stream::iter(chunks).boxed();
            let events = collect(stream_chat_completions(
                raw,
                None,
                rid(),
                Duration::from_secs(60),
            ))
            .await;
            match events.last().unwrap() {
                SamplingEvent::Completed { response, .. } => {
                    assert_eq!(response.cost_usd_ticks, expected, "wire {wire:?}");
                }
                other => panic!("expected Completed, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn later_missing_cost_does_not_clobber_earlier_ticks() {
        let mut first = make_chunk(vec![ChatChunkDelta::default()]);
        first.usage = Some(Usage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            prompt_tokens_details: None,
            completion_tokens_details: None,
            cost_in_usd_ticks: Some(99),
        });
        let mut second = make_chunk(vec![ChatChunkDelta::default()]);
        second.usage = Some(Usage {
            prompt_tokens: 12,
            completion_tokens: 6,
            total_tokens: 18,
            prompt_tokens_details: None,
            completion_tokens_details: None,
            cost_in_usd_ticks: Some(0),
        });
        let chunks: Vec<Result<ChatCompletionChunk, SamplingError>> = vec![
            Ok(text_chunk("ok")),
            Ok(first),
            Ok(second),
            Ok(final_chunk(FinishReason::Stop)),
        ];
        let raw = stream::iter(chunks).boxed();
        let events = collect(stream_chat_completions(
            raw,
            None,
            rid(),
            Duration::from_secs(60),
        ))
        .await;
        match events.last().unwrap() {
            SamplingEvent::Completed { response, .. } => {
                assert_eq!(response.cost_usd_ticks, Some(99));
            }
            other => panic!("expected Completed, got {other:?}"),
        }
    }
}

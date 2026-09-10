//! Read-only audit probe linked against the existing workspace libraries.
//! No provider requests, session files, or production behavior are changed.
use futures_util::{StreamExt, stream};
use sampler::{RequestId, SamplingEvent};
use sampling_types::{ConversationItem, ConversationRequest, StopReason};
use serde_json::json;
use std::time::Duration;

fn inspect(label: &str, events: Vec<SamplingEvent>, raw_reason: &str) {
    let Some(SamplingEvent::Completed { response, .. }) = events.last() else {
        panic!("{label}: {events:?}");
    };
    assert_eq!(response.stop_reason, Some(StopReason::ToolCalls));
    assert_eq!(response.raw_stop_reason.as_deref(), Some(raw_reason));
    assert_eq!(response.tool_calls()[0].name, "FinishTurn");
    assert!(!response.assistant_text().trim().is_empty());
    println!(
        "{label}: raw={raw_reason}, normalized=tool_calls, visible_text=true, FinishTurn retained"
    );
}

fn main() {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let args = json!({"status":"completed", "reason":"done"}).to_string();
        for reason in ["content_filter", "length"] {
            let chunk: sampling_types::ChatCompletionChunk = serde_json::from_value(json!({
                "id":"chat-probe", "object":"chat.completion.chunk", "created":0, "model":"probe",
                "choices":[{"index":0,"delta":{"role":"assistant","content":"先检查：",
                    "tool_calls":[{"index":0,"id":"finish","type":"function",
                        "function":{"name":"FinishTurn","arguments":args}}]},"finish_reason":reason}],
                "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}
            })).unwrap();
            inspect("Chat", sampler::stream_chat_completions(stream::iter([Ok(chunk)]).boxed(),
                None, RequestId::from("probe-chat"), Duration::from_secs(1)).collect().await, reason);
        }
        for reason in ["refusal", "max_tokens"] {
            let wire = vec![
                json!({"type":"message_start","message":{"id":"msg-probe","type":"message",
                    "role":"assistant","model":"probe","content":[],"stop_reason":null,
                    "usage":{"input_tokens":1,"output_tokens":0}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"先检查："}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"finish","name":"FinishTurn","input":{}}}),
                json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":args}}),
                json!({"type":"content_block_stop","index":1}),
                json!({"type":"message_delta","delta":{"stop_reason":reason},"usage":{"output_tokens":1}}),
                json!({"type":"message_stop"}),
            ].into_iter().map(|value| Ok(serde_json::from_value::<sampling_types::messages::MessageStreamEvent>(value).unwrap()));
            inspect("Messages", sampler::stream_messages(stream::iter(wire).boxed(),
                None, RequestId::from("probe-messages"), Duration::from_secs(1)).collect().await, reason);
        }
        for reason in ["content_filter", "max_output_tokens"] {
            let event: sampling_types::rs::ResponseStreamEvent = serde_json::from_value(json!({
                "type":"response.incomplete","sequence_number":0,"response":{
                    "id":"resp-probe","object":"response","created_at":0,"model":"probe",
                    "status":"incomplete","incomplete_details":{"reason":reason},
                    "output":[{"type":"message","id":"text","role":"assistant","status":"completed",
                        "content":[{"type":"output_text","text":"先检查：","annotations":[]}]},
                        {"type":"function_call","id":"fc","call_id":"finish","name":"FinishTurn",
                        "arguments":args,"status":"completed"}]}})).unwrap();
            inspect("Responses", sampler::stream_responses(stream::iter([Ok(event)]).boxed(),
                None, RequestId::from("probe-responses"), Duration::from_secs(1), None).collect().await,
                &format!("incomplete:{reason}"));
        }
    });
    let request = ConversationRequest {
        items: vec![
            ConversationItem::Assistant(sampling_types::AssistantItem {
                content: "finished".into(),
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
                tool_calls: vec!["a.b", "a/b"]
                    .into_iter()
                    .map(|id| sampling_types::ToolCall {
                        id: id.into(),
                        name: "read_file".into(),
                        arguments: "{}".into(),
                    })
                    .collect(),
            }),
            ConversationItem::tool_result("a.b", "result A"),
            ConversationItem::tool_result("a/b", "result B"),
        ],
        ..Default::default()
    };
    let wire = serde_json::to_value(sampling_types::build_messages_request(&request)).unwrap();
    let mut call_ids = Vec::new();
    let mut result_ids = Vec::new();
    for message in wire["messages"].as_array().unwrap() {
        for block in message["content"].as_array().unwrap() {
            match block["type"].as_str() {
                Some("tool_use") => call_ids.push(block["id"].as_str().unwrap()),
                Some("tool_result") => result_ids.push(block["tool_use_id"].as_str().unwrap()),
                _ => {}
            }
        }
    }
    assert_eq!(call_ids, ["a_b", "a_b"]);
    assert_eq!(result_ids, ["a_b", "a_b"]);
    println!(
        "Messages target encoding: distinct a.b and a/b become duplicate call/result IDs {call_ids:?}"
    );
}

//! Audit probes: these assert the CURRENT observed wire shapes, including defects.
//! Keep outside product regression tests; run as a temporary integration target.
use sampling_types::*;
use serde_json::{Value, json};

fn skill(source: SkillLoadSource) -> ConversationItem {
    ConversationItem::SkillLoad(SkillLoadItem {
        input_id: "audit-input".into(), reference_index: 0, call_id: "a".into(),
        skill_name: "audit".into(), source, args: "".into(),
        prepared_body: "audit skill body".into(), prompt_index: Some(1),
    })
}
fn agent() -> ConversationItem {
    ConversationItem::received_agent_message(AgentMessage {
        receipt_id: "audit-receipt".into(), source_session_id: "source".into(),
        target_session_id: "target".into(), message_id: "message".into(),
        reply_to: None, message: "audit agent body".into(),
    })
}
fn calls() -> ConversationItem {
    ConversationItem::assistant_tool_calls(vec![
        ToolCall { id: "a".into(), name: "load_skill".into(), arguments: r#"{"name":"audit","args":""}"#.into() },
        ToolCall { id: "b".into(), name: "read_file".into(), arguments: "{}".into() },
    ])
}
fn projection(prefix: usize, learned: Option<ApiBackend>) -> NativeContinuationProjection {
    NativeContinuationProjection { portable_prefix_len: prefix, spans: vec![], portable_reasoning_backend: learned }
}
fn wire(request: &ConversationRequest, backend: ApiBackend) -> Value {
    match backend {
        ApiBackend::Responses => serde_json::to_value(rs::CreateResponse::from(request)).unwrap(),
        ApiBackend::ChatCompletions => serde_json::to_value(ChatCompletionRequest::from(request.clone())).unwrap(),
        ApiBackend::Messages => serde_json::to_value(build_messages_request(request)).unwrap(),
    }
}
fn interleaved(fact: ConversationItem) -> ConversationRequest {
    let a_result = match &fact {
        ConversationItem::SkillLoad(s) if s.source == SkillLoadSource::Model => skill_load_result_content(s),
        _ => "first".into(),
    };
    ConversationRequest::from_items(vec![calls(), ConversationItem::tool_result("a", a_result), fact, ConversationItem::tool_result("b", "second")])
}
fn native(backend: ApiBackend) -> NativeContinuationFragment {
    match backend {
        ApiBackend::ChatCompletions => NativeContinuationFragment::ChatCompletions(
            ChatCompletionRequest::from(ConversationRequest::from_items(vec![calls()])).messages.remove(0)
        ),
        ApiBackend::Responses => NativeContinuationFragment::Responses(serde_json::from_value(json!([
            {"type":"function_call", "call_id":"a", "name":"load_skill", "arguments":"{}"},
            {"type":"function_call", "call_id":"b", "name":"read_file", "arguments":"{}"}
        ])).unwrap()),
        ApiBackend::Messages => NativeContinuationFragment::Messages(serde_json::from_value(json!([
            {"type":"tool_use", "id":"a", "name":"load_skill", "input":{}},
            {"type":"tool_use", "id":"b", "name":"read_file", "input":{}}
        ])).unwrap()),
    }
}
#[test]
fn responses_agent_receipt_still_has_synthetic_call_without_reasoning() {
    for learned in [false, true] {
        let mut req = ConversationRequest::from_items(vec![ConversationItem::user("continue"), agent()]);
        req.native_continuation = Some(projection(req.items.len(), learned.then_some(ApiBackend::Responses)));
        let w = wire(&req, ApiBackend::Responses);
        let input = w["input"].as_array().unwrap();
        assert_eq!(input.len(), 3);
        assert_eq!(input[1]["name"], RECEIVE_AGENT_MESSAGE_TOOL_NAME);
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[2]["type"], "function_call_output");
        assert!(!input.iter().any(|v| v["type"] == "reasoning"));
    }
}
#[test]
fn learned_chat_skips_reasoning_field_on_both_runtime_facts() {
    for fact in [agent(), skill(SkillLoadSource::User)] {
        let mut req = ConversationRequest::from_items(vec![ConversationItem::assistant("earlier answer"), fact]);
        req.native_continuation = Some(projection(req.items.len(), Some(ApiBackend::ChatCompletions)));
        let w = wire(&req, ApiBackend::ChatCompletions);
        let messages = w["messages"].as_array().unwrap();
        assert_eq!(messages[0]["reasoning_content"], "");
        assert_eq!(messages[1]["role"], "assistant");
        assert!(messages[1]["tool_calls"].is_array());
        assert!(messages[1].get("reasoning_content").is_none());
    }
}
#[test]
fn runtime_only_portable_projection_has_no_assistant_for_chat_recovery_gate() {
    for fact in [agent(), skill(SkillLoadSource::User)] {
        let projected = project_portable_history_with_reasoning(&[fact], Some(ApiBackend::ChatCompletions));
        assert!(!projected.iter().any(|v| matches!(v, ConversationItem::Assistant(_))));
        // ChatState::EnablePortableReasoning tests exactly this plus native spans.
    }
}
#[test]
fn portable_rebuild_loses_second_complete_tool_exchange_across_runtime_fact() {
    for fact in [agent(), skill(SkillLoadSource::User), skill(SkillLoadSource::Model)] {
        let mut req = interleaved(fact);
        req.native_continuation = Some(projection(req.items.len(), None));
        for backend in [ApiBackend::Responses, ApiBackend::ChatCompletions, ApiBackend::Messages] {
            let w = wire(&req, backend.clone());
            assert!(!w.to_string().contains("second"), "{backend:?}: {w}");
            assert!(w.to_string().contains("first") || w.to_string().contains("audit skill body"));
        }
    }
}
#[test]
fn reset_prefix_stops_at_model_skill_fact_and_leaves_orphan_result() {
    let mut req = interleaved(skill(SkillLoadSource::Model));
    // A reset after the assistant sets the prefix before the later tool results.
    req.native_continuation = Some(projection(1, None));
    assert_eq!(req.native_continuation.as_ref().unwrap().portable_prefix_end(&req.items), Some(2));
    let w = wire(&req, ApiBackend::Responses);
    let input = w["input"].as_array().unwrap();
    assert!(!input.iter().any(|v| v["type"] == "function_call" && v["call_id"] == "b"));
    assert!(input.iter().any(|v| v["type"] == "function_call_output" && v["call_id"] == "b"));
}
#[test]
fn native_split_does_not_defer_runtime_receipt_past_all_results() {
    for backend in [ApiBackend::Responses, ApiBackend::ChatCompletions, ApiBackend::Messages] {
        let mut req = interleaved(agent());
        req.native_continuation = Some(NativeContinuationProjection {
            portable_prefix_len: 0,
            spans: vec![NativeContinuationSpan { start: 0, end: 1, fragment: native(backend.clone()) }],
            portable_reasoning_backend: None,
        });
        let w = wire(&req, backend.clone());
        let encoded = w.to_string();
        assert!(encoded.find(RECEIVE_AGENT_MESSAGE_TOOL_NAME).unwrap() < encoded.find("second").unwrap(), "{backend:?}: {w}");
    }
}
#[test]
fn previous_responses_user_skill_fix_is_still_present() {
    let req = ConversationRequest::from_items(vec![skill(SkillLoadSource::User)]);
    let w = wire(&req, ApiBackend::Responses);
    assert_eq!(w["input"].as_array().unwrap().len(), 1);
    assert_eq!(w["input"][0]["role"], "user");
    assert!(w["input"][0]["content"].as_str().unwrap().contains("audit skill body"));
}

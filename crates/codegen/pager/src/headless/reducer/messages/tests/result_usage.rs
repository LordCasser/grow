//! Terminal result line: usage/modelUsage, error subtypes, cost, num_turns.

use super::*;
use pretty_assertions::assert_eq;
use shell::extensions::notification::{PromptUsage, PromptUsageModel};

const USAGE_KEYS: &[&str] = &[
    "input_tokens",
    "full_input_tokens",
    "output_tokens",
    "cache_read_input_tokens",
    "cache_creation_input_tokens",
    "cache_read_known_input_tokens",
    "cache_read_unknown_calls",
    "cache_write_unknown_calls",
];

const MODEL_KEYS: &[&str] = &[
    "inputTokens",
    "fullInputTokens",
    "outputTokens",
    "cacheReadInputTokens",
    "cacheCreationInputTokens",
    "cacheReadKnownInputTokens",
    "cacheReadUnknownCalls",
    "cacheWriteUnknownCalls",
];

fn messages_result(raw: Option<&Value>) -> Value {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("hi".into()));
    let out = r.finish(&TurnEnd {
        stop_reason: "end_turn",
        session_id: "sess-1",
        request_id: "req-1",
        usage: raw,
        structured_output: None,
        result_text: "",
        duration_ms: 0,
    });
    out.last().expect("result line").clone()
}

fn attached(raw: &Value) -> Value {
    let mut scratch = json!({});
    crate::headless::attach_result_usage(&mut scratch, raw);
    scratch
}

fn billed(
    input: u64,
    cached_read: u64,
    cache_creation: u64,
    output: u64,
    cost_ticks: Option<i64>,
    read_unknown: u64,
    write_unknown: u64,
) -> PromptUsageModel {
    PromptUsageModel {
        input_tokens: input,
        output_tokens: output,
        total_tokens: input + output,
        cached_read_tokens: cached_read,
        cache_creation_tokens: cache_creation,
        cache_read_known_input_tokens: input,
        cache_read_unknown_calls: read_unknown,
        cache_write_unknown_calls: write_unknown,
        model_calls: 1,
        cost_usd_ticks: cost_ticks,
        ..Default::default()
    }
}

fn prompt_usage(totals: PromptUsageModel, model: PromptUsageModel, incomplete: bool) -> Value {
    let mut model_usage = indexmap::IndexMap::new();
    model_usage.insert("grow-4".to_string(), model);
    serde_json::to_value(PromptUsage {
        totals,
        model_usage,
        num_turns: 1,
        usage_is_incomplete: incomplete,
    })
    .expect("prompt usage")
}

/// Known cost matches the number from `attach_result_usage`, including zero.
/// A missing source cost is null here, not an exact zero.
fn assert_cost_synonym(got: &Value, source: Option<&Value>) {
    match source {
        Some(cost) => assert_eq!(got, cost),
        None => assert!(got.is_null(), "unknown cost must be null, got {got}"),
    }
}

fn assert_matches_attach(result: &Value, source: &Value) {
    let incomplete = source
        .get("usage_is_incomplete")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert_eq!(result["usage_is_incomplete"], incomplete);
    assert_cost_synonym(&result["total_cost_usd"], source.get("total_cost_usd"));
    if let Some(usage) = source.get("usage") {
        for key in USAGE_KEYS {
            assert_eq!(result["usage"].get(*key), usage.get(*key), "usage.{key}");
        }
    }
    match source.get("modelUsage").and_then(Value::as_object) {
        Some(models) => {
            for (name, row) in models {
                let got = &result["modelUsage"][name];
                assert_cost_synonym(&got["costUSD"], row.get("costUSD"));
                for key in MODEL_KEYS {
                    assert_eq!(got.get(*key), row.get(*key), "{name}.{key}");
                }
            }
        }
        None => assert_eq!(result["modelUsage"], json!({})),
    }
}

#[test]
fn messages_usage_drops_reasoning_tokens() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("hi".into()));
    r.reduce(StreamEvent::ResponseCompleted {
        message_id: None,
        stop_reason: Some("end_turn".into()),
        usage: Some(ResponseUsage {
            input_tokens: Some(4),
            full_input_tokens: 5,
            output_tokens: 2,
            cache_read_input_tokens: Some(1),
            cache_creation_input_tokens: Some(0),
            reasoning_tokens: 9,
        }),
        signature: None,
        stop_sequence: None,
    });
    let msg = r.flush_assistant(Some("end_turn")).expect("assistant");
    let usage = &msg["message"]["usage"];
    assert_eq!(usage["input_tokens"], 4);
    assert_eq!(usage["output_tokens"], 2);
    assert!(usage.get("reasoning_tokens").is_none(), "{usage:?}");
}

#[test]
fn messages_refusal_marks_result_error() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("declined".into()));
    let out = r.finish(&turn_end("refusal", "declined"));
    let result = out.last().expect("result line");
    assert_eq!(result["type"], "result");
    assert_eq!(result["is_error"], true, "{result:?}");
    assert_eq!(result["subtype"], "error_during_execution", "{result:?}");
    assert!(result["errors"].is_array(), "{result:?}");
    assert!(
        result.get("result").is_none(),
        "error result omits result text"
    );
}

#[test]
fn messages_result_carries_required_fields() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("hi".into()));
    let out = r.finish(&turn_end("end_turn", "done"));
    let result = out.last().expect("result line");
    assert_eq!(result["subtype"], "success");
    assert_eq!(result["result"], "hi");
    assert_eq!(result["stop_reason"], "end_turn");
    for key in [
        "duration_ms",
        "duration_api_ms",
        "num_turns",
        "total_cost_usd",
        "usage_is_incomplete",
        "modelUsage",
    ] {
        assert!(result.get(key).is_some(), "missing {key}: {result:?}");
    }
    assert_eq!(result["usage_is_incomplete"], true);
    assert!(
        result["total_cost_usd"].is_null(),
        "no ledger is not an exact zero fee: {result:?}"
    );
    assert!(result["permission_denials"].is_null());
    for key in [
        "input_tokens",
        "output_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ] {
        assert!(result["usage"].get(key).is_some(), "usage missing {key}");
    }
}

#[test]
fn messages_result_usage_splits_disjoint_buckets() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("hi".into()));
    let aggregate = json!({
        "inputTokens": 100,
        "outputTokens": 7,
        "totalTokens": 107,
        "cachedReadTokens": 10,
        "cacheCreationTokens": 5,
        "numTurns": 1,
    });
    let out = r.finish(&TurnEnd {
        stop_reason: "end_turn",
        session_id: "sess-1",
        request_id: "req-1",
        usage: Some(&aggregate),
        structured_output: None,
        result_text: "",
        duration_ms: 0,
    });
    let usage = &out.last().unwrap()["usage"];
    assert_eq!(usage["input_tokens"], 85);
    assert_eq!(usage["cache_read_input_tokens"], 10);
    assert_eq!(usage["cache_creation_input_tokens"], 5);
    assert_eq!(usage["output_tokens"], 7);
}

#[test]
fn messages_result_usage_incomplete_aggregate_keeps_input_unknown() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("hi".into()));
    r.reduce(StreamEvent::ResponseCompleted {
        message_id: None,
        stop_reason: Some("end_turn".into()),
        usage: Some(ResponseUsage {
            cache_creation_input_tokens: Some(5),
            ..Default::default()
        }),
        signature: None,
        stop_sequence: None,
    });
    let out = r.finish(&end_turn());
    let usage = &out.last().unwrap()["usage"];
    assert!(usage["input_tokens"].is_null());
    assert!(usage["cache_creation_input_tokens"].is_null());
}

#[test]
fn messages_model_usage_maps_and_zero_fills() {
    let rows = json!({
        "grow-4": {"inputTokens": 90, "outputTokens": 7, "cacheReadInputTokens": 10, "cacheCreationInputTokens": 25, "costUSD": 0.02},
    });
    let out = messages_model_usage(Some(&rows), Some("grow-4"), Some(131_072));
    let mu = &out["grow-4"];
    assert_eq!(mu["inputTokens"], 90);
    assert_eq!(mu["outputTokens"], 7);
    assert_eq!(mu["cacheReadInputTokens"], 10);
    assert_eq!(mu["cacheCreationInputTokens"], 25);
    assert_eq!(mu["contextWindow"], 131_072);
    assert!(mu["maxOutputTokens"].is_null());
    assert!((mu["costUSD"].as_f64().unwrap() - 0.02).abs() < 1e-9);
    assert_eq!(messages_model_usage(None, None, None), json!({}));
}

#[test]
fn messages_result_carries_durations() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("hi".into()));
    let aggregate = json!({"inputTokens": 10, "outputTokens": 2, "apiDurationMs": 1234});
    let out = r.finish(&TurnEnd {
        stop_reason: "end_turn",
        session_id: "sess-1",
        request_id: "req-1",
        usage: Some(&aggregate),
        structured_output: None,
        result_text: "",
        duration_ms: 4242,
    });
    let result = out.last().unwrap();
    assert_eq!(result["duration_ms"], 4242);
    assert_eq!(result["duration_api_ms"], 1234);
}

#[test]
fn messages_error_flushes_then_marks_error_result() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("partial answer".into()));
    let out = r.error("boom", None, 0, None);
    assert!(out.iter().any(|m| m["type"] == "assistant"));
    let result = out.last().unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["subtype"], "error_during_execution");
    assert_eq!(result["is_error"], true);
    assert_eq!(result["errors"][0], "boom");
    assert!(result.get("result").is_none());
    let assistant = out.iter().find(|m| m["type"] == "assistant").unwrap();
    assert!(
        assistant["message"]["stop_reason"].is_null(),
        "generic error frame reports null stop_reason, not end_turn: {assistant:?}"
    );
    assert!(result["stop_reason"].is_null());
}

#[test]
fn messages_error_max_tokens_stamps_stop_reason() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage(
        "partial before truncation".into(),
    ));
    let out = r.error("output truncated", None, 0, Some("max_tokens"));
    let assistant = out
        .iter()
        .find(|m| m["type"] == "assistant")
        .expect("partial content flushed as an assistant frame");
    assert_eq!(assistant["message"]["stop_reason"], "max_tokens");
    assert_eq!(
        assistant["message"]["content"][0]["text"],
        "partial before truncation"
    );
    let result = out.last().unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["subtype"], "error_during_execution");
    assert_eq!(result["is_error"], true);
    assert_eq!(result["stop_reason"], "max_tokens");
}

#[test]
fn messages_partial_error_max_tokens_recovers_real_id_and_usage() {
    let mut r = messages(true);
    let mut out = Vec::new();
    out.extend(r.reduce(StreamEvent::ResponseStarted {
        message_id: Some("msg_real".into()),
        model: Some("grow-4".into()),
        input_tokens: 42,
        cache_read_input_tokens: Some(100),
        cache_creation_input_tokens: Some(20),
    }));
    out.extend(r.reduce(StreamEvent::AgentMessage(
        "partial before truncation".into(),
    )));
    out.extend(r.error("output truncated", None, 0, Some("max_tokens")));
    let start = out
        .iter()
        .find(|m| m["event"]["type"] == "message_start")
        .expect("message_start");
    assert_eq!(start["event"]["message"]["id"], "msg_real");
    assert_eq!(start["event"]["message"]["usage"]["input_tokens"], 42);
    assert_eq!(
        start["event"]["message"]["usage"]["cache_read_input_tokens"],
        100
    );
    let assistant = out
        .iter()
        .find(|m| m["type"] == "assistant")
        .expect("partial content flushed as an assistant frame");
    assert_eq!(assistant["message"]["id"], "msg_real");
    assert_eq!(assistant["message"]["stop_reason"], "max_tokens");
    assert_eq!(assistant["message"]["usage"]["input_tokens"], 42);
    assert_eq!(
        assistant["message"]["usage"]["cache_read_input_tokens"],
        100
    );
    assert_eq!(
        assistant["message"]["usage"]["cache_creation_input_tokens"],
        20
    );
    assert_eq!(assistant["message"]["usage"]["output_tokens"], 0);
}

#[test]
fn messages_partial_error_max_tokens_delta_carries_input_usage() {
    let mut r = messages(true);
    let mut out = Vec::new();
    out.extend(r.reduce(StreamEvent::ResponseStarted {
        message_id: Some("msg_real".into()),
        model: Some("grow-4".into()),
        input_tokens: 42,
        cache_read_input_tokens: Some(100),
        cache_creation_input_tokens: Some(20),
    }));
    out.extend(r.reduce(StreamEvent::AgentMessage(
        "partial before truncation".into(),
    )));
    out.extend(r.error("output truncated", None, 0, Some("max_tokens")));
    let delta = out
        .iter()
        .find(|m| m["event"]["type"] == "message_delta")
        .expect("message_delta");
    assert_eq!(delta["event"]["delta"]["stop_reason"], "max_tokens");
    assert_eq!(delta["event"]["usage"]["input_tokens"], 42);
    assert_eq!(delta["event"]["usage"]["cache_read_input_tokens"], 100);
    assert_eq!(delta["event"]["usage"]["cache_creation_input_tokens"], 20);
    let assistant = out
        .iter()
        .find(|m| m["type"] == "assistant")
        .expect("frame");
    assert_eq!(assistant["message"]["usage"]["input_tokens"], 42);
    assert_eq!(
        assistant["message"]["usage"]["cache_read_input_tokens"],
        100
    );
}

#[test]
fn messages_partial_generic_error_delta_stop_reason_null() {
    let mut r = messages(true);
    let mut out = Vec::new();
    out.extend(r.reduce(StreamEvent::AgentMessage("partial answer".into())));
    out.extend(r.error("boom", None, 0, None));
    let delta = out
        .iter()
        .find(|m| m["event"]["type"] == "message_delta")
        .expect("message_delta");
    assert!(
        delta["event"]["delta"]["stop_reason"].is_null(),
        "generic error partial delta reports null stop_reason: {delta:?}"
    );
    let assistant = out
        .iter()
        .find(|m| m["type"] == "assistant")
        .expect("frame");
    assert!(assistant["message"]["stop_reason"].is_null());
}

#[test]
fn messages_structured_output_error_marks_retry_subtype() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("bad output".into()));
    let out = r.finish(&TurnEnd {
        stop_reason: "end_turn",
        session_id: "sess-1",
        request_id: "req-1",
        usage: None,
        structured_output: Some(Err("output does not match schema".into())),
        result_text: "",
        duration_ms: 0,
    });
    let result = out.last().unwrap();
    assert_eq!(result["subtype"], "error_max_structured_output_retries");
    assert_eq!(result["is_error"], true);
    assert_eq!(result["errors"][0], "output does not match schema");
    assert!(result.get("result").is_none());
    assert!(result.get("structured_output").is_none());
}

#[test]
fn messages_max_turns_marks_error_subtype() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("working".into()));
    assert!(r.max_turns().is_empty());
    let out = r.finish(&turn_end("cancelled", ""));
    let result = out.last().expect("result line");
    assert_eq!(result["subtype"], "error_max_turns", "{result:?}");
    assert_eq!(result["is_error"], true, "{result:?}");
}

#[test]
fn to_line_degrades_failing_serialize_to_error_line() {
    struct AlwaysFails;
    impl Serialize for AlwaysFails {
        fn serialize<S: serde::Serializer>(&self, _s: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("boom"))
        }
    }
    let line = to_line(&AlwaysFails);
    assert_eq!(line["type"], "error");
    let message = line["message"].as_str().expect("message string");
    assert!(message.contains("serialize failed"), "{message}");
    assert!(message.contains("boom"), "{message}");
}

#[test]
fn non_finite_cost_serializes_as_unknown_not_zero() {
    let line = to_line(&MessagesLine::Result(Box::new(ResultLine {
        subtype: "success",
        is_error: false,
        duration_ms: 0,
        duration_api_ms: 0,
        num_turns: 1,
        result: None,
        stop_reason: None,
        total_cost_usd: Some(f64::INFINITY),
        usage_is_incomplete: false,
        usage: MessageUsage::default(),
        model_usage: json!({}),
        structured_output: None,
        errors: None,
        session_id: "s".into(),
        uuid: "u".into(),
    })));
    assert_eq!(line["type"], "result", "not the error fallback: {line}");
    assert!(line["total_cost_usd"].is_null(), "{line}");

    let mu = to_line(&ModelUsage {
        input_tokens: Some(0),
        full_input_tokens: 0,
        output_tokens: 0,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        cache_read_known_input_tokens: 0,
        cache_read_unknown_calls: 0,
        cache_write_unknown_calls: 0,
        cost_usd: Some(f64::NAN),
        context_window: None,
    });
    assert_ne!(mu["type"], "error", "not the error fallback: {mu}");
    assert!(mu["costUSD"].is_null(), "{mu}");
}

fn assert_result_matches_prompt(
    totals: PromptUsageModel,
    model: PromptUsageModel,
    incomplete: bool,
    check: impl FnOnce(&Value),
) {
    let raw = prompt_usage(totals, model, incomplete);
    let source = attached(&raw);
    check(&source);
    assert_matches_attach(&messages_result(Some(&raw)), &source);
}

#[test]
fn messages_result_without_usage_is_incomplete_and_cost_unknown() {
    let result = messages_result(None);
    assert_eq!(result["usage_is_incomplete"], true);
    assert!(result["total_cost_usd"].is_null(), "{result}");
    assert_eq!(result["modelUsage"], json!({}));
}

#[test]
fn messages_result_cost_and_completeness_match_attach() {
    let known = billed(100, 40, 5, 10, Some(2_000_000_000), 0, 0);
    assert_result_matches_prompt(known.clone(), known, false, |source| {
        assert_eq!(source["total_cost_usd"], 0.2);
        assert_eq!(source["modelUsage"]["grow-4"]["costUSD"], 0.2);
        assert_eq!(source["usage"]["input_tokens"], 55);
        assert!(source.get("usage_is_incomplete").is_none());
    });

    let zero = billed(8, 0, 0, 0, Some(0), 0, 0);
    assert_result_matches_prompt(zero.clone(), zero, false, |source| {
        assert_eq!(source["total_cost_usd"], 0.0);
        assert_eq!(source["modelUsage"]["grow-4"]["costUSD"], 0.0);
    });

    let incomplete = billed(50, 20, 0, 5, Some(5_000_000_000), 0, 0);
    assert_result_matches_prompt(incomplete.clone(), incomplete, true, |source| {
        assert_eq!(source["usage_is_incomplete"], true);
        assert!(source.get("total_cost_usd").is_none());
        assert!(source["modelUsage"]["grow-4"].get("costUSD").is_none());
        assert_eq!(source["usage"]["input_tokens"], 30);
        assert_eq!(source["usage"]["cache_read_input_tokens"], 20);
    });

    let unknown = billed(30, 0, 0, 3, None, 0, 0);
    assert_result_matches_prompt(unknown.clone(), unknown, false, |source| {
        assert!(source.get("usage_is_incomplete").is_none());
        assert!(source.get("total_cost_usd").is_none());
        assert_eq!(source["usage"]["input_tokens"], 30);
        assert_eq!(source["usage"]["output_tokens"], 3);
    });

    let mut partial = billed(100, 40, 0, 10, Some(1_000_000_000), 0, 0);
    partial.cost_is_partial = true;
    partial.cost_missing_calls = 1;
    assert_result_matches_prompt(partial.clone(), partial, false, |source| {
        assert_eq!(source["cost_is_partial"], true);
        assert!(source.get("usage_is_incomplete").is_none());
        assert!(source.get("total_cost_usd").is_none());
        assert!(source["modelUsage"]["grow-4"].get("costUSD").is_none());
        assert_eq!(source["usage"]["input_tokens"], 60);
        assert_eq!(source["usage"]["cache_read_input_tokens"], 40);
    });

    let totals = billed(80, 10, 5, 4, Some(1_000_000_000), 0, 0);
    let model = billed(80, 0, 0, 4, None, 1, 2);
    assert_result_matches_prompt(totals, model, false, |source| {
        assert_eq!(source["total_cost_usd"], 0.1);
        assert!(source["usage"]["input_tokens"].as_u64().is_some());
        assert!(source["modelUsage"]["grow-4"]["inputTokens"].is_null());
        assert!(source["modelUsage"]["grow-4"].get("costUSD").is_none());
        assert_eq!(source["modelUsage"]["grow-4"]["fullInputTokens"], 80);
        assert_eq!(source["modelUsage"]["grow-4"]["cacheReadUnknownCalls"], 1);
        assert_eq!(source["modelUsage"]["grow-4"]["cacheWriteUnknownCalls"], 2);
    });
}

#[test]
fn messages_finish_abnormal_outcomes_stamp_null_stop_reason() {
    let frame_stop = |stop_reason: &str, prime: fn(&mut MessagesReducer)| -> Value {
        let mut r = messages(false);
        r.reduce(StreamEvent::AgentMessage("streamed content".into()));
        prime(&mut r);
        let out = r.finish(&TurnEnd {
            stop_reason,
            session_id: "sess-1",
            request_id: "req-1",
            usage: None,
            structured_output: None,
            result_text: "",
            duration_ms: 0,
        });
        out.iter()
            .find(|m| m["type"] == "assistant")
            .expect("assistant frame")["message"]["stop_reason"]
            .clone()
    };
    assert!(frame_stop("refusal", |_| {}).is_null(), "refusal");
    assert!(frame_stop("cancelled", |_| {}).is_null(), "cancelled");
    assert!(
        frame_stop("end_turn", |r| {
            r.max_turns();
        })
        .is_null(),
        "max_turns"
    );
    assert_eq!(frame_stop("end_turn", |_| {}), "end_turn");
}

#[test]
fn messages_cancelled_turn_marks_error_result() {
    let mut r = messages(false);
    r.reduce(StreamEvent::AgentMessage("partial before cancel".into()));
    let out = r.finish(&turn_end("cancelled", "partial before cancel"));
    let result = out.last().expect("result line");
    assert_eq!(result["type"], "result");
    assert_eq!(result["is_error"], true, "{result:?}");
    assert_ne!(
        result["subtype"], "success",
        "cancelled is not success: {result:?}"
    );
    assert_eq!(result["subtype"], "error_during_execution", "{result:?}");
    assert_eq!(result["errors"][0], "cancelled", "{result:?}");
    assert!(
        result.get("result").is_none(),
        "error result omits result text"
    );
    let assistant = out
        .iter()
        .find(|m| m["type"] == "assistant")
        .expect("assistant frame");
    assert!(
        assistant["message"]["stop_reason"].is_null(),
        "cancelled frame reports null stop_reason: {assistant:?}"
    );
}

#[test]
fn messages_num_turns_counts_contentless_response() {
    let mut r = messages(false);
    let mut out = Vec::new();
    out.extend(r.reduce(response_started("msg_a", None, 0)));
    out.extend(r.reduce(StreamEvent::AgentMessage("hi".into())));
    out.extend(r.reduce(response_completed("msg_a", "end_turn")));
    out.extend(r.reduce(response_started("msg_b", None, 0)));
    out.extend(r.reduce(response_completed("msg_b", "end_turn")));
    out.extend(r.finish(&end_turn()));
    let frames = out.iter().filter(|m| m["type"] == "assistant").count();
    assert_eq!(frames, 1, "contentless B emits no frame: {out:?}");
    let result = out.last().expect("result line");
    assert_eq!(
        result["num_turns"], 2,
        "both the content-bearing and the contentless response count: {result:?}"
    );
}

#[test]
fn messages_retry_exhausted_null_stop_reason_overrides_retained_end_turn() {
    for partials in [false, true] {
        let mut r = messages(partials);
        r.reduce(StreamEvent::AgentMessage("streamed content".into()));
        r.reduce(response_completed("msg_a", "end_turn"));
        let out = r.finish(&TurnEnd {
            stop_reason: "end_turn",
            session_id: "sess-1",
            request_id: "req-1",
            usage: None,
            structured_output: Some(Err("output does not match schema".into())),
            result_text: "",
            duration_ms: 0,
        });
        let assistant = out
            .iter()
            .find(|m| m["type"] == "assistant")
            .expect("assistant frame");
        assert!(
            assistant["message"]["stop_reason"].is_null(),
            "retained end_turn must not win on failure (partials={partials}): {assistant:?}"
        );
        let result = out.last().expect("result line");
        assert_eq!(result["subtype"], "error_max_structured_output_retries");
        if partials {
            let delta = out
                .iter()
                .find(|m| m["event"]["type"] == "message_delta")
                .expect("message_delta");
            assert!(
                delta["event"]["delta"]["stop_reason"].is_null(),
                "partial delta null too: {delta:?}"
            );
        }
    }
}

#[test]
fn messages_late_orphaned_completion_does_not_inflate_num_turns() {
    let mut r = messages(false);
    let mut out = Vec::new();
    out.extend(r.reduce(StreamEvent::ToolCall(tool_call_ev())));
    out.extend(r.reduce(StreamEvent::ToolCallUpdate(tool_update(
        "completed",
        json!("done"),
    ))));
    out.extend(r.reduce(response_completed("msg_late", "end_turn")));
    out.extend(r.finish(&end_turn()));
    let result = out.last().expect("result line");
    assert_eq!(
        result["num_turns"], 1,
        "orphaned late completion must not add a turn: {result:?}"
    );
}

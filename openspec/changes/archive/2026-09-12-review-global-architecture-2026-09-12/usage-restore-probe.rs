use chat_state::{ChatStateActor, NullTimelinePersistence, ObservationEvent, Timeline, TimelineEventKind, UsageTotals};
use sampling_types::SamplingConfig;
use serde_json::{Value, json};

fn config() -> SamplingConfig {
    SamplingConfig { base_url: "https://example.invalid".into(), model: "test-model".into(), output_limit: None, temperature: None, top_p: None, api_backend: Default::default(), extra_headers: Default::default(), query_params: Default::default(), env_http_headers: Default::default(), context_window: std::num::NonZeroU64::new(128_000).unwrap(), reasoning_effort: None, stream_tool_calls: None }
}
fn bill(n: u64) -> Vec<(String, UsageTotals)> {
    vec![("model-a".into(), UsageTotals { input_tokens: n, output_tokens: 2, model_calls: 1, ..Default::default() })]
}
fn payload(n: u64) -> Value { json!({"subagent_id":"child-a", "by_model":bill(n), "incomplete":false}) }
async fn restore(label: &str, data: Vec<Value>) {
    let mut timeline = Timeline::default();
    for data in data {
        timeline.record(TimelineEventKind::Observation(ObservationEvent {
            scope: "session_usage".into(), name: "subagent_settled".into(), turn: None, step: None, data: Some(data),
        })).unwrap();
    }
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = ChatStateActor::spawn_from_timeline(timeline.events().to_vec(), config(), Box::new(NullTimelinePersistence), tx, Default::default()).await;
    match result {
        Ok(handle) => {
            let usage = handle.try_get_session_usage().await.unwrap();
            println!("{label}: restore=accepted input_tokens={} output_tokens={} incomplete={}", usage.totals.input_tokens, usage.totals.output_tokens, usage.incomplete);
        }
        Err(e) => println!("{label}: restore=rejected error={e:?}"),
    }
}
async fn probe() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let live = ChatStateActor::spawn(vec![], config(), Box::new(NullTimelinePersistence), tx, Default::default());
    println!("live-first: {:?}", live.record_subagent_usage("child-a".into(), bill(3), false, false).await);
    println!("live-conflict: {:?}", live.record_subagent_usage("child-a".into(), bill(30), false, false).await);
    restore("exact-duplicate", vec![payload(3), payload(3)]).await;
    restore("conflicting-duplicate", vec![payload(3), payload(30)]).await;
    restore("malformed-settlement", vec![json!({"subagent_id":17})]).await;
}
fn main() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    tokio::task::LocalSet::new().block_on(&rt, probe());
}

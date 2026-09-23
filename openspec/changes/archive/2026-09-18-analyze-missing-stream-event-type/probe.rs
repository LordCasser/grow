//! Offline incident probe; see verification.md for its temporary Cargo entry.
use axum::{Router, routing::post};
use futures_util::StreamExt;
use sampler::{SamplerConfig, SamplingClient};
use sampling_types::{ApiBackend, ConversationItem, ConversationRequest, SamplingError};

#[tokio::main]
async fn main() {
    diagnostics::tls::install_ring_provider_once();
    let incident = r#"{"request_id":"00000000-0000-0000-0000-000000000000","code":"InvalidParameter","message":"Output data may contain inappropriate content."}"#;
    assert_eq!(incident.len(), 138);
    assert!(sampling_types::error::try_parse_stream_error(incident).is_none());
    for (label, data) in [
        ("incident_flat_error", incident.to_owned()),
        ("recognized_rejection", serde_json::json!({
            "type": "error", "error": {"type": "InvalidParameter", "message": "Output data may contain inappropriate content."}
        }).to_string()),
        ("recognized_overload", serde_json::json!({
            "type": "error", "error": {"type": "overloaded_error", "message": "temporary overload"}
        }).to_string()),
    ] {
        let sse = format!("event: error\ndata: {data}\n\n");
        let app = Router::new().route("/v1/messages", post(move || {
            let body = sse.clone();
            async move { ([("content-type", "text/event-stream")], body) }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let client = SamplingClient::new(SamplerConfig {
            base_url: format!("http://{address}/v1"),
            model: "incident-fixture".into(),
            api_backend: ApiBackend::Messages,
            ..Default::default()
        }).unwrap();
        let request = ConversationRequest::from_items(vec![ConversationItem::user("fixture")]);
        let (mut stream, _) = client.conversation_stream_messages(request).await.unwrap();
        let error = stream.next().await.unwrap().unwrap_err();
        println!("{label}: {error}");
        let decision = sampler::retry::classify_error(&error, 0, 15, 2);
        println!("  classification: {decision:?}");
        match label {
            "incident_flat_error" => {
                assert_eq!(error.to_string(), "serialization error: missing field `type` at line 1 column 138");
                assert!(matches!(decision, sampler::retry::RetryDecision::Fatal(SamplingError::Serialization(_))));
            }
            "recognized_rejection" => {
                assert!(matches!(decision, sampler::retry::RetryDecision::Fatal(SamplingError::Api { status, .. }) if status.as_u16() == 400));
            }
            _ => assert!(matches!(decision, sampler::retry::RetryDecision::RetryWithClientRebuild { .. })),
        }
        server.abort();
    }
    // A prospective flat-envelope fix must also classify provider codes:
    // accepting this shape alone would currently turn these into HTTP 500.
    for code in ["DataInspectionFailed", "data_inspection_failed", "Throttling", "ServiceUnavailable"] {
        let error = SamplingError::from_stream_error(code, "fixture");
        println!("code mapping {code}: {error}; retryable={}", error.is_retryable());
    }
}

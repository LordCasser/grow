//! M0 probes of the pinned official SDK, not a Grow protocol driver.
//! The scripted peer is test-only. Production must use the SDK MRTR helper.
//! `sdk_limitation_*` tests document missing host extension points; passing
//! them does not mean Grow's corresponding acceptance requirement is met.

use std::{sync::Arc, time::Duration};

use mcp::rmcp::{
    ClientHandler, ClientServiceExt,
    model::{
        CallToolRequestParams, ClientCapabilities, ClientConfig, DiscoverResult,
        ElicitRequestParams, ElicitResult, ElicitationAction, ErrorData, ProtocolVersion,
        RequestId, RequestMetaObject, ServerCapabilities,
    },
    service::{ClientLifecycleMode, RequestContext, RoleClient, RunningService, ServiceError},
    transport::async_rw::AsyncRwTransport,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

tokio::task_local! {
    static OWNER: &'static str;
}

#[derive(Debug)]
struct Callback {
    owner: Option<&'static str>,
    key: RequestId,
    ct: CancellationToken,
}

struct Handler {
    observations: mpsc::UnboundedSender<Callback>,
    wait_for_cancel: bool,
}

impl ClientHandler for Handler {
    async fn create_elicitation(
        &self,
        params: ElicitRequestParams,
        context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, ErrorData> {
        let owner = OWNER.try_with(|owner| *owner).ok();
        self.observations
            .send(Callback {
                owner,
                key: context.id,
                ct: context.ct.clone(),
            })
            .unwrap();
        if self.wait_for_cancel {
            context.ct.cancelled().await;
            return Err(ErrorData::internal_error("handler cancelled", None));
        }
        let message = match params {
            ElicitRequestParams::FormElicitationParams { message, .. }
            | ElicitRequestParams::UrlElicitationParams { message, .. } => message,
            _ => {
                return Err(ErrorData::invalid_params(
                    "unsupported elicitation mode",
                    None,
                ));
            }
        };
        Ok(ElicitResult::new(ElicitationAction::Accept)
            .with_content(json!({"value": owner.unwrap_or(&message)})))
    }

    #[expect(deprecated)] // Intentionally test a mixed, unsupported roots round.
    async fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> Result<mcp::rmcp::model::ListRootsResult, ErrorData> {
        Err(ErrorData::invalid_params("roots are not supported", None))
    }

    fn get_info(&self) -> ClientConfig {
        serde_json::from_value(json!({
            "protocolVersion": "2026-07-28",
            "clientInfo": {"name": "grow-sdk-probe", "version": "0"},
            "capabilities": {"elicitation": {"form": {}, "url": {}}}
        }))
        .unwrap()
    }
}

struct Fixture {
    service: Arc<RunningService<RoleClient, Handler>>,
    wire: mpsc::UnboundedReceiver<Value>,
    callbacks: mpsc::UnboundedReceiver<Callback>,
    peer: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.peer.abort();
    }
}

impl Fixture {
    async fn start(reply: impl FnMut(&Value) -> Option<Value> + Send + 'static) -> Self {
        Self::with_lifecycle(
            reply,
            false,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
    }

    async fn with_lifecycle(
        mut reply: impl FnMut(&Value) -> Option<Value> + Send + 'static,
        wait_for_cancel: bool,
        lifecycle: ClientLifecycleMode,
    ) -> Self {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        let (server_read, mut server_write) = tokio::io::split(server);
        let (wire_tx, wire) = mpsc::unbounded_channel();
        let (observations, callbacks) = mpsc::unbounded_channel();
        let peer = tokio::spawn(async move {
            let mut reader = BufReader::new(server_read);
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                    break;
                }
                let request: Value = serde_json::from_str(&line).unwrap();
                wire_tx.send(request.clone()).unwrap();
                let Some(response) = reply(&request) else {
                    continue;
                };
                let mut bytes = serde_json::to_vec(&response).unwrap();
                bytes.push(b'\n');
                if server_write.write_all(&bytes).await.is_err() {
                    break;
                }
            }
        });
        let handler = Handler {
            observations,
            wait_for_cancel,
        };
        let service = tokio::time::timeout(
            Duration::from_secs(2),
            handler.serve_with_lifecycle(
                AsyncRwTransport::<RoleClient, _, _>::new(client_read, client_write),
                lifecycle,
            ),
        )
        .await
        .expect("SDK startup deadline")
        .unwrap();
        Self {
            service: Arc::new(service),
            wire,
            callbacks,
            peer,
        }
    }

    async fn next_wire(&mut self, method: &str) -> Value {
        loop {
            let message = tokio::time::timeout(Duration::from_secs(2), self.wire.recv())
                .await
                .expect("wire deadline")
                .expect("wire closed");
            if message["method"] == method {
                return message;
            }
        }
    }
}

fn response(request: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": request["id"], "result": result})
}

fn discovery(request: &Value) -> Value {
    response(
        request,
        serde_json::to_value(DiscoverResult::new(
            vec![ProtocolVersion::V_2026_07_28],
            ServerCapabilities::default(),
        ))
        .unwrap(),
    )
}

fn form(message: &str) -> Value {
    json!({"method": "elicitation/create", "params": {
        "mode": "form", "message": message,
        "requestedSchema": {"type": "object", "properties": {"value": {"type": "string"}}}
    }})
}

fn complete() -> Value {
    json!({"resultType": "complete", "content": [
        {"type": "image", "data": "aGVsbG8=", "mimeType": "image/png"}
    ], "structuredContent": {"done": true}, "isError": true})
}

fn input_required(inputs: Value, state: Option<&str>) -> Value {
    let mut result = json!({"resultType": "input_required", "inputRequests": inputs});
    if let Some(state) = state {
        result["requestState"] = json!(state);
    }
    result
}

#[tokio::test]
async fn sdk_drives_rounds_clears_old_state_and_preserves_typed_final_result() {
    let mut round = 0;
    let mut fixture = Fixture::start(move |request| match request["method"].as_str() {
        Some("server/discover") => Some(discovery(request)),
        Some("tools/call") => {
            let result = match round {
                0 => input_required(json!({"same-key": form("first")}), Some("opaque")),
                1 => input_required(json!({"same-key": form("second")}), None),
                _ => complete(),
            };
            round += 1;
            Some(response(request, result))
        }
        _ => None,
    })
    .await;
    let result = fixture
        .service
        .call_tool(CallToolRequestParams::new("echo"))
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(result).unwrap(), complete());
    let initial = fixture.next_wire("tools/call").await;
    let first = fixture.next_wire("tools/call").await;
    let second = fixture.next_wire("tools/call").await;
    assert_ne!(initial["id"], first["id"]);
    assert_ne!(first["id"], second["id"]);
    assert_eq!(first["params"]["requestState"], "opaque");
    assert!(second["params"].get("requestState").is_none());
    assert_eq!(
        first["params"]["inputResponses"]["same-key"]["content"]["value"],
        "first"
    );
    assert_eq!(
        second["params"]["inputResponses"]["same-key"]["content"]["value"],
        "second"
    );
    for request in [initial, first, second] {
        let meta: RequestMetaObject =
            serde_json::from_value(request["params"]["_meta"].clone()).unwrap();
        assert_eq!(meta.protocol_version(), Some(ProtocolVersion::V_2026_07_28));
        assert_eq!(meta.client_info().unwrap().name, "grow-sdk-probe");
    }
}

#[tokio::test]
async fn sdk_preserves_task_local_owner_for_parallel_calls_with_the_same_input_key() {
    let mut fixture = Fixture::start(|request| match request["method"].as_str() {
        Some("server/discover") => Some(discovery(request)),
        Some("tools/call") => Some(response(
            request,
            if request["params"].get("inputResponses").is_none() {
                input_required(json!({"same-key": form("answer")}), Some("opaque"))
            } else {
                let name = request["params"]["name"].as_str().unwrap();
                assert_eq!(
                    request["params"]["inputResponses"]["same-key"]["content"]["value"],
                    name
                );
                complete()
            },
        )),
        _ => None,
    })
    .await;
    let (a, b) = tokio::join!(
        OWNER.scope(
            "call-a",
            fixture
                .service
                .call_tool(CallToolRequestParams::new("call-a"))
        ),
        OWNER.scope(
            "call-b",
            fixture
                .service
                .call_tool(CallToolRequestParams::new("call-b"))
        ),
    );
    a.unwrap();
    b.unwrap();
    let a = fixture.callbacks.recv().await.unwrap();
    let b = fixture.callbacks.recv().await.unwrap();
    assert_ne!(a.owner, b.owner);
    assert_eq!(a.key, b.key); // The SDK key alone is not a logical call identity.
}

#[tokio::test]
async fn sdk_limitation_connection_capabilities_override_call_metadata() {
    let mut fixture = Fixture::start(|request| match request["method"].as_str() {
        Some("server/discover") => Some(discovery(request)),
        Some("tools/call") => Some(response(request, complete())),
        _ => None,
    })
    .await;
    let mut meta = RequestMetaObject::new();
    meta.set_client_capabilities(ClientCapabilities::default());
    let mut params = CallToolRequestParams::new("headless");
    params.meta = Some(meta);
    fixture.service.call_tool(params).await.unwrap();
    let request = fixture.next_wire("tools/call").await;
    let meta: RequestMetaObject =
        serde_json::from_value(request["params"]["_meta"].clone()).unwrap();
    assert!(
        meta.client_capabilities().unwrap().elicitation.is_some(),
        "SDK behavior changed: re-evaluate the per-call capability extension gap"
    );
}

#[tokio::test]
async fn sdk_limitation_mixed_round_calls_handler_before_rejecting_unsupported_roots() {
    let mut fixture = Fixture::start(|request| match request["method"].as_str() {
        Some("server/discover") => Some(discovery(request)),
        Some("tools/call") => Some(response(request, input_required(json!({
            "a-url": {"method": "elicitation/create", "params": {
                "mode": "url", "message": "consent", "url": "https://example.com/consent", "elicitationId": "test"
            }},
            "z-roots": {"method": "roots/list"}
        }), Some("opaque")))),
        _ => None,
    }).await;
    let error = fixture
        .service
        .call_tool(CallToolRequestParams::new("mixed"))
        .await
        .unwrap_err();
    assert!(matches!(error, ServiceError::McpError(_)));
    assert!(
        fixture.callbacks.try_recv().is_ok(),
        "SDK behavior changed: re-evaluate the whole-round preflight extension gap"
    );
}

#[tokio::test]
async fn sdk_limitation_nested_context_token_is_not_the_original_owner_token() {
    let mut fixture = Fixture::with_lifecycle(
        |request| match request["method"].as_str() {
            Some("server/discover") => Some(discovery(request)),
            Some("tools/call") => Some(response(
                request,
                input_required(json!({"key": form("wait")}), None),
            )),
            _ => None,
        },
        true,
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
    )
    .await;
    let owner_ct = CancellationToken::new();
    let call_ct = owner_ct.clone();
    let service = fixture.service.clone();
    let call = tokio::spawn(async move {
        tokio::select! {
            result = service.call_tool(CallToolRequestParams::new("wait")) => Some(result),
            _ = call_ct.cancelled() => None,
        }
    });
    let callback = tokio::time::timeout(Duration::from_secs(2), fixture.callbacks.recv())
        .await
        .unwrap()
        .unwrap();
    owner_ct.cancel();
    assert!(call.await.unwrap().is_none());
    assert!(
        !callback.ct.is_cancelled(),
        "SDK behavior changed: re-evaluate cancellation propagation"
    );
}

#[tokio::test]
async fn sdk_limitation_dropping_automatic_call_does_not_cancel_the_active_wire_request() {
    let mut fixture = Fixture::start(|request| match request["method"].as_str() {
        Some("server/discover") => Some(discovery(request)),
        _ => None,
    })
    .await;
    let service = fixture.service.clone();
    let call =
        tokio::spawn(async move { service.call_tool(CallToolRequestParams::new("slow")).await });
    fixture.next_wire("tools/call").await;
    call.abort();
    assert!(call.await.unwrap_err().is_cancelled());
    assert!(
        tokio::time::timeout(Duration::from_millis(80), fixture.wire.recv())
            .await
            .is_err(),
        "SDK behavior changed: re-evaluate the active-wire cancellation extension gap"
    );
}

#[tokio::test(start_paused = true)]
async fn sdk_bounds_state_only_rounds_and_applies_its_own_backoff() {
    let mut fixture = Fixture::start(|request| match request["method"].as_str() {
        Some("server/discover") => Some(discovery(request)),
        Some("tools/call") => Some(response(
            request,
            input_required(json!({}), Some("pending")),
        )),
        _ => None,
    })
    .await;
    let started = tokio::time::Instant::now();
    let error = fixture
        .service
        .call_tool_with_mrtr_max_rounds(CallToolRequestParams::new("poll"), 3)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ServiceError::InputRequiredRoundsExceeded { max_rounds: 3 }
    ));
    assert!(started.elapsed() >= Duration::from_millis(350));
    for _ in 0..3 {
        fixture.next_wire("tools/call").await;
    }
    assert!(fixture.wire.try_recv().is_err());
}

#[tokio::test]
async fn sdk_auto_lifecycle_falls_back_to_legacy_without_custom_negotiation() {
    let mut fixture = Fixture::with_lifecycle(
        |request| match request["method"].as_str() {
            Some("server/discover") => {
                Some(json!({"jsonrpc": "2.0", "id": request["id"], "error": {
                    "code": -32601, "message": "legacy server"
                }}))
            }
            Some("initialize") => Some(response(
                request,
                json!({
                    "protocolVersion": request["params"]["protocolVersion"], "capabilities": {},
                    "serverInfo": {"name": "legacy", "version": "0"}
                }),
            )),
            Some("tools/call") => Some(response(request, complete())),
            _ => None,
        },
        false,
        ClientLifecycleMode::Auto {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            legacy_version: Some(ProtocolVersion::V_2025_06_18),
        },
    )
    .await;
    fixture
        .service
        .call_tool(CallToolRequestParams::new("echo"))
        .await
        .unwrap();
    fixture.next_wire("server/discover").await;
    let initialize = fixture.next_wire("initialize").await;
    assert_eq!(initialize["params"]["protocolVersion"], "2025-06-18");
}

#[tokio::test]
async fn wrapped_http_uses_sdk_discovery_metadata_headers_and_automatic_mrtr() {
    use axum::{Json, http::HeaderMap, routing::post};
    use mcp::{
        mcp_http_client::{McpHttpClient, WarnBudget},
        rmcp::transport::{
            StreamableHttpClientTransport,
            streamable_http_client::StreamableHttpClientTransportConfig,
        },
    };

    diagnostics::tls::install_ring_provider_once();
    let wire = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let captured = wire.clone();
    let app = axum::Router::new().route(
        "/mcp",
        post(move |headers: HeaderMap, Json(request): Json<Value>| {
            let captured = captured.clone();
            async move {
                captured.lock().push((request.clone(), headers));
                Json(match request["method"].as_str() {
                    Some("server/discover") => discovery(&request),
                    Some("tools/call") => response(
                        &request,
                        if request["params"].get("inputResponses").is_none() {
                            input_required(json!({"key": form("http")}), Some("opaque"))
                        } else {
                            complete()
                        },
                    ),
                    method => panic!("unexpected SDK method: {method:?}"),
                })
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://{}/mcp", listener.local_addr().unwrap());
    let peer = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (observations, _callbacks) = mpsc::unbounded_channel();
    let handler = Handler {
        observations,
        wait_for_cancel: false,
    };
    let service = handler
        .serve_with_lifecycle(
            StreamableHttpClientTransport::with_client(
                McpHttpClient::new(
                    reqwest::Client::default(),
                    "sdk-probe",
                    WarnBudget::default(),
                ),
                StreamableHttpClientTransportConfig::with_uri(uri),
            ),
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        service.call_tool(CallToolRequestParams::new("echo")),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(serde_json::to_value(result).unwrap(), complete());
    service.cancel().await.unwrap();
    peer.abort();
    let requests = wire.lock();
    assert_eq!(requests.len(), 3); // discovery, initial call, SDK continuation
    assert_eq!(requests[0].0["method"], "server/discover");
    for (request, headers) in requests.iter().skip(1) {
        assert_eq!(headers["mcp-method"], "tools/call");
        assert_eq!(headers["mcp-name"], "echo");
        assert!(!headers.contains_key("mcp-session-id"));
        let meta: RequestMetaObject =
            serde_json::from_value(request["params"]["_meta"].clone()).unwrap();
        assert_eq!(meta.protocol_version(), Some(ProtocolVersion::V_2026_07_28));
    }
}

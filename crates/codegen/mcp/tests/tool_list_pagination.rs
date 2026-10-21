use std::collections::VecDeque;
use std::sync::Arc;

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use mcp::rmcp::ServiceExt;
use mcp::rmcp::transport::StreamableHttpClientTransport;
use mcp::servers::list_all_tools;

#[ctor::ctor]
fn install_rustls_provider() {
    diagnostics::tls::install_ring_provider_once();
}

#[derive(Clone)]
struct Page {
    tools: Vec<Value>,
    next_cursor: Option<String>,
    fail: bool,
}

#[derive(Clone)]
struct MockState {
    pages: Arc<Mutex<VecDeque<Page>>>,
    cursors: Arc<Mutex<Vec<Option<String>>>>,
}

async fn handle_post(State(state): State<MockState>, Json(req): Json<Value>) -> Response {
    match req["method"].as_str() {
        Some("initialize") => {
            let result = json!({
                "jsonrpc": "2.0",
                "id": req["id"],
                "result": {
                    "protocolVersion": req["params"]["protocolVersion"],
                    "capabilities": {},
                    "serverInfo": {"name": "pagination-test", "version": "0.0.0"},
                },
            });
            ([("mcp-session-id", "pagination-test")], Json(result)).into_response()
        }
        Some("tools/list") => {
            let cursor = req["params"]["cursor"].as_str().map(str::to_owned);
            state.cursors.lock().await.push(cursor);
            let Some(page) = state.pages.lock().await.pop_front() else {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            };
            if page.fail {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            Json(json!({
                "jsonrpc": "2.0",
                "id": req["id"],
                "result": {
                    "tools": page.tools,
                    "nextCursor": page.next_cursor,
                },
            }))
            .into_response()
        }
        _ => StatusCode::ACCEPTED.into_response(),
    }
}

async fn handle_get() -> Response {
    (
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(futures::stream::pending::<Result<String, std::io::Error>>()),
    )
        .into_response()
}

async fn connect(
    pages: Vec<Page>,
) -> (
    mcp::rmcp::service::RunningService<mcp::rmcp::RoleClient, ()>,
    MockState,
) {
    let state = MockState {
        pages: Arc::new(Mutex::new(pages.into())),
        cursors: Arc::new(Mutex::new(Vec::new())),
    };
    let app = axum::Router::new()
        .route("/mcp", get(handle_get).post(handle_post))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let uri = format!("http://{addr}/mcp");
    let transport = StreamableHttpClientTransport::from_uri(uri.as_str());
    let service = ().serve(transport).await.unwrap();
    (service, state)
}

fn page(start: usize, count: usize, next_cursor: Option<&str>) -> Page {
    Page {
        tools: (start..start + count)
            .map(|index| {
                json!({
                    "name": format!("tool_{index}"),
                    "inputSchema": {"type": "object"},
                })
            })
            .collect(),
        next_cursor: next_cursor.map(str::to_owned),
        fail: false,
    }
}

#[tokio::test]
async fn collects_all_112_tools_across_pages() {
    let pages = [32, 32, 32, 16]
        .into_iter()
        .enumerate()
        .map(|(index, count)| {
            let cursor = (index < 3).then(|| format!("page-{}", index + 1));
            page(index * 32, count, cursor.as_deref())
        })
        .collect();
    let (service, state) = connect(pages).await;

    let tools = list_all_tools(service.peer()).await.unwrap();

    assert_eq!(tools.len(), 112);
    assert_eq!(
        *state.cursors.lock().await,
        vec![
            None,
            Some("page-1".into()),
            Some("page-2".into()),
            Some("page-3".into())
        ]
    );
    let _ = service.cancel().await;
}

#[tokio::test]
async fn empty_page_with_next_cursor_continues() {
    let (service, state) = connect(vec![
        page(0, 1, Some("empty")),
        page(1, 0, Some("last")),
        page(1, 1, None),
    ])
    .await;

    let tools = list_all_tools(service.peer()).await.unwrap();

    assert_eq!(tools.len(), 2);
    assert_eq!(
        *state.cursors.lock().await,
        vec![None, Some("empty".into()), Some("last".into())]
    );
    let _ = service.cancel().await;
}

#[tokio::test]
async fn later_page_error_fails_discovery() {
    let (service, state) = connect(vec![
        page(0, 1, Some("second")),
        Page {
            fail: true,
            ..page(1, 0, None)
        },
    ])
    .await;

    let result = list_all_tools(service.peer()).await;

    assert!(result.is_err());
    assert_eq!(
        *state.cursors.lock().await,
        vec![None, Some("second".into())]
    );
    let _ = service.cancel().await;
}

#[tokio::test]
async fn repeated_cursor_fails_discovery() {
    let (service, state) =
        connect(vec![page(0, 1, Some("again")), page(1, 1, Some("again"))]).await;

    let result = list_all_tools(service.peer()).await;

    assert!(result.unwrap_err().to_string().contains("repeated cursor"));
    assert_eq!(
        *state.cursors.lock().await,
        vec![None, Some("again".into())]
    );
    let _ = service.cancel().await;
}

#[tokio::test]
async fn cursor_cycle_fails_discovery() {
    let (service, state) = connect(vec![
        page(0, 1, Some("a")),
        page(1, 1, Some("b")),
        page(2, 1, Some("a")),
    ])
    .await;

    let result = list_all_tools(service.peer()).await;

    assert!(result.unwrap_err().to_string().contains("repeated cursor"));
    assert_eq!(
        *state.cursors.lock().await,
        vec![None, Some("a".into()), Some("b".into())]
    );
    let _ = service.cancel().await;
}

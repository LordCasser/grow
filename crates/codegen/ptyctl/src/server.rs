//! HTTP + WebSocket server exposing PTY session control.

use std::future::IntoFuture;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::{Extension, Router};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Mutex, watch};
use tower_http::cors::CorsLayer;

use crate::session::{PtySession, SessionControl, WaitCondition, WaitOutcome};
use crate::term::ScreenOpts;

/// Shared application state.
type AppState = Arc<Mutex<PtySession>>;

/// Query parameters for screen endpoint.
#[derive(Debug, serde::Deserialize, Default)]
pub struct ScreenParams {
    /// Row range, 1-indexed (e.g. "1:5", "5:", ":10", "5").
    pub rows: Option<String>,
    /// Column range, 1-indexed.
    pub cols: Option<String>,
    /// Character to show at cursor position.
    pub cursor: Option<char>,
    /// Output format: "text" (default), "styled", "html".
    pub format: Option<String>,
    /// Include trailing empty lines.
    pub full: Option<bool>,
}

/// Request body for send endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct SendRequest {
    pub keys: String,
}

/// Request body for resize endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct ResizeRequest {
    pub cols: u16,
    pub rows: u16,
}

/// Query parameters for scrollback endpoint.
#[derive(Debug, serde::Deserialize, Default)]
pub struct ScrollbackParams {
    /// Number of scrollback lines to return (default: 100).
    pub lines: Option<usize>,
}

/// Default wait timeout in milliseconds.
const WAIT_DEFAULT_TIMEOUT_MS: u64 = 10_000;
/// Maximum wait timeout in milliseconds.
const WAIT_MAX_TIMEOUT_MS: u64 = 120_000;

/// Query parameters for wait endpoint — exactly one condition must be set.
#[derive(Debug, serde::Deserialize, Default)]
pub struct WaitParams {
    /// Wait until this text appears on screen.
    pub text: Option<String>,
    /// Wait until this regex matches the screen text.
    pub regex: Option<String>,
    /// Wait until this text is absent from the screen.
    pub gone: Option<String>,
    /// Wait until the grid has been unchanged for this many milliseconds.
    pub stable_ms: Option<u64>,
    /// Timeout in milliseconds (default 10000, capped at 120000).
    pub timeout_ms: Option<u64>,
}

/// Incoming WebSocket message from client.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type")]
enum WsClientMessage {
    /// Send raw input text to the PTY.
    #[serde(rename = "input")]
    Input { data: String },
    /// Send vim-notation keys to the PTY.
    #[serde(rename = "keys")]
    Keys { keys: String },
    /// Resize the terminal.
    #[serde(rename = "resize")]
    Resize { cols: u16, rows: u16 },
}

/// The router and its accepted WS connections share one bounded retirement.
pub struct SessionServer {
    router: Router,
    connections: watch::Receiver<usize>,
}

const SERVICE_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const WS_SEND_TIMEOUT: Duration = Duration::from_millis(500);
const WS_TERMINAL_TIMEOUT: Duration = Duration::from_secs(1);

impl SessionServer {
    pub async fn serve(
        self,
        listener: tokio::net::TcpListener,
        control: &SessionControl,
    ) -> Result<()> {
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let mut connections = self.connections;
        let serving = axum::serve(listener, self.router)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .into_future();
        tokio::pin!(serving);
        let terminal = tokio::select! {
            result = &mut serving => return result.context("HTTP server error"),
            result = control.wait_for_shutdown() => result,
        };
        let _ = shutdown_tx.send(());
        let drained = tokio::time::timeout(SERVICE_DRAIN_TIMEOUT, async {
            serving.await.context("HTTP server error")?;
            // axum's upgraded connections outlive the HTTP serve future.
            // Count upgrades at acceptance, including ones not yet scheduled.
            while *connections.borrow_and_update() != 0 {
                connections
                    .changed()
                    .await
                    .context("WS connection tracking closed")?;
            }
            Ok(())
        })
        .await
        .context("HTTP/WS service did not drain within 2 seconds")
        .and_then(|r| r);
        match (terminal, drained) {
            (Ok(()), result) => result,
            (Err(primary), Ok(())) => Err(primary),
            (Err(primary), Err(cleanup)) => {
                Err(primary.context(format!("service cleanup also failed: {cleanup:#}")))
            }
        }
    }
}

struct WsConnection(watch::Sender<usize>);

impl WsConnection {
    fn accept(connections: watch::Sender<usize>) -> Self {
        connections.send_modify(|count| *count += 1);
        Self(connections)
    }
}

impl Drop for WsConnection {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}

/// Build the HTTP router and retain its upgraded-connection retirement handle.
pub fn build_router(session: PtySession) -> SessionServer {
    let state: AppState = Arc::new(Mutex::new(session));
    let (connections, connection_rx) = watch::channel(0);

    let router = Router::new()
        .route("/query/screen", get(handle_screen))
        .route("/query/cursor", get(handle_cursor))
        .route("/query/status", get(handle_status))
        .route("/query/scrollback", get(handle_scrollback))
        // Top-level on purpose: wait is a synchronization primitive, neither a /query read nor a /control mutation.
        .route("/wait", get(handle_wait))
        .route("/control/send", post(handle_send))
        .route("/control/resize", post(handle_resize))
        .route("/control/stop", post(handle_stop))
        .route("/ws", get(handle_ws_upgrade))
        .layer(CorsLayer::very_permissive())
        .layer(Extension(connections))
        .with_state(state);
    SessionServer {
        router,
        connections: connection_rx,
    }
}

/// Parse a range string like "1:5", "5:", ":10", "5" into a Range<usize>.
fn parse_range(s: &str) -> Option<std::ops::Range<usize>> {
    if let Some((a, b)) = s.split_once(':') {
        let start = if a.is_empty() {
            1
        } else {
            a.parse::<usize>().ok()?
        };
        let end = if b.is_empty() {
            usize::MAX
        } else {
            b.parse::<usize>().ok()?
        };
        Some(start..end)
    } else {
        let n = s.parse::<usize>().ok()?;
        Some(n..n + 1)
    }
}

fn build_screen_opts(params: &ScreenParams) -> ScreenOpts {
    ScreenOpts {
        rows: params.rows.as_deref().and_then(parse_range),
        cols: params.cols.as_deref().and_then(parse_range),
        cursor_char: params.cursor,
        include_empty: params.full.unwrap_or(false),
    }
}

async fn handle_screen(
    State(state): State<AppState>,
    Query(params): Query<ScreenParams>,
) -> Response {
    let session = state.lock().await;
    let opts = build_screen_opts(&params);

    let format = params.format.as_deref().unwrap_or("text");
    match format {
        "styled" => {
            let styled = session.screen_styled(&opts).await;
            Json(styled).into_response()
        }
        "html" => {
            let html = session.screen_html(&opts).await;
            ([(axum::http::header::CONTENT_TYPE, "text/html")], html).into_response()
        }
        _ => {
            let output = session.screen(&opts).await;
            Json(output).into_response()
        }
    }
}

async fn handle_cursor(State(state): State<AppState>) -> Json<serde_json::Value> {
    let session = state.lock().await;
    let cursor = session.cursor().await;
    Json(serde_json::json!({
        "row": cursor.row,
        "col": cursor.col,
    }))
}

async fn handle_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let session = state.lock().await;
    let status = session.status().await;
    Json(serde_json::json!({
        "alive": status.alive,
        "pid": status.pid,
        "exit_code": status.exit_code,
        "size": [status.size.0, status.size.1],
        "modes": status.modes,
        "scrollback_lines": status.scrollback_lines,
    }))
}

async fn handle_send(
    State(state): State<AppState>,
    Json(req): Json<SendRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session = state.lock().await;
    match session.send_keys(&req.keys).await {
        Ok(()) => Ok(Json(serde_json::json!({"ok": true}))),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )),
    }
}

async fn handle_resize(
    State(state): State<AppState>,
    Json(req): Json<ResizeRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let session = state.lock().await;
    match session.resize(req.cols, req.rows).await {
        Ok(()) => Ok(Json(serde_json::json!({"ok": true}))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )),
    }
}

/// Long-poll until a screen condition is met or the timeout elapses.
///
/// Timeout is a normal outcome (200 with `matched: false` + diagnostics),
/// not an error — `--gone`/`--stable_ms` waits time out routinely.
async fn handle_wait(
    State(state): State<AppState>,
    Query(params): Query<WaitParams>,
) -> Result<Json<WaitOutcome>, (StatusCode, Json<serde_json::Value>)> {
    let mut conditions = Vec::new();
    if let Some(text) = params.text {
        conditions.push(WaitCondition::Text(text));
    }
    if let Some(pattern) = params.regex {
        conditions.push(WaitCondition::Regex(pattern));
    }
    if let Some(text) = params.gone {
        conditions.push(WaitCondition::Gone(text));
    }
    if let Some(ms) = params.stable_ms {
        conditions.push(WaitCondition::StableMs(ms));
    }
    let (Some(condition), true) = (conditions.pop(), conditions.is_empty()) else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "exactly one of text, regex, gone, stable_ms is required"
            })),
        ));
    };
    let timeout_ms = params
        .timeout_ms
        .unwrap_or(WAIT_DEFAULT_TIMEOUT_MS)
        .min(WAIT_MAX_TIMEOUT_MS);

    // Clone the wait handles and drop the session guard, or send/screen would block for the whole wait.
    let handle = state.lock().await.wait_handle();
    match handle
        .wait_for(condition, std::time::Duration::from_millis(timeout_ms))
        .await
    {
        Ok(outcome) => Ok(Json(outcome)),
        // Alternate format keeps the regex parse detail from the error chain.
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": format!("{e:#}")})),
        )),
    }
}

async fn handle_stop(State(state): State<AppState>) -> Response {
    let control = state.lock().await.control_handle();
    stop_response(control.stop().await)
}

fn stop_response(result: Result<()>) -> Response {
    match result {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{error:#}")})),
        )
            .into_response(),
    }
}

// -- Scrollback --

async fn handle_scrollback(
    State(state): State<AppState>,
    Query(params): Query<ScrollbackParams>,
) -> Json<serde_json::Value> {
    let session = state.lock().await;
    let count = params.lines.unwrap_or(100);
    let lines = session.scrollback(count).await;
    Json(serde_json::json!({
        "count": lines.len(),
        "lines": lines,
    }))
}

// -- WebSocket streaming --

/// Upgrade an HTTP request to a WebSocket connection.
async fn handle_ws_upgrade(
    State(state): State<AppState>,
    Extension(connections): Extension<watch::Sender<usize>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let connection = WsConnection::accept(connections);
    // Subscribe before accepting the upgrade so the first post-handshake output
    // already belongs to this connection's FIFO, even before its task is polled.
    let (output_rx, control) = {
        let session = state.lock().await;
        (session.subscribe(), session.control_handle())
    };
    ws.on_upgrade(move |socket| async move {
        let _connection = connection;
        handle_ws_connection(socket, state, output_rx, control).await;
    })
}

/// Handle a single WebSocket connection.
///
/// Protocol:
///
/// **Server -> Client:**
/// - Binary frames: raw PTY output bytes (high-throughput streaming)
/// - Text frames: JSON `{"type":"closed","exit_code":N}` on process exit
///
/// **Client -> Server:**
/// - Binary frames: raw bytes written to PTY stdin
/// - Text frames: JSON with `type` field:
///   - `{"type":"input","data":"text"}` — send text to PTY
///   - `{"type":"keys","keys":"<C-c>"}` — vim-notation keystrokes
///   - `{"type":"resize","cols":120,"rows":40}` — resize terminal
async fn handle_ws_connection(
    socket: WebSocket,
    state: AppState,
    mut output_rx: tokio::sync::broadcast::Receiver<Vec<u8>>,
    control: SessionControl,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let sending = async {
        let terminal = control.wait_for_exit();
        tokio::pin!(terminal);
        let result = loop {
            tokio::select! {
                biased;
                result = &mut terminal => break result,
                output = output_rx.recv() => match output {
                    Ok(bytes) => {
                        if !ws_send(&mut ws_tx, Message::Binary(bytes.into())).await { return; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        if !ws_send(&mut ws_tx, lag_warning(n)).await { return; }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break terminal.await,
                }
            }
        };
        // Complete terminal observation fences the producer. A retained Session
        // sender need not close: drain the accepted FIFO before the real result.
        let _ = tokio::time::timeout(WS_TERMINAL_TIMEOUT, async {
            loop {
                match output_rx.try_recv() {
                    Ok(bytes) => {
                        if !ws_send(&mut ws_tx, Message::Binary(bytes.into())).await {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                        if !ws_send(&mut ws_tx, lag_warning(n)).await {
                            return;
                        }
                    }
                    Err(
                        tokio::sync::broadcast::error::TryRecvError::Empty
                        | tokio::sync::broadcast::error::TryRecvError::Closed,
                    ) => break,
                }
            }
            let event = match result {
                Ok(code) => serde_json::json!({"type": "closed", "exit_code": code}),
                Err(error) => serde_json::json!({
                    "type": "error", "exit_code": null,
                    "message": format!("PTY terminal state is unknown: {error:#}"),
                }),
            };
            if ws_send(&mut ws_tx, Message::Text(event.to_string().into())).await {
                let _ = tokio::time::timeout(WS_SEND_TIMEOUT, ws_tx.close()).await;
            }
        })
        .await;
    };
    let receiving = async {
        while let Some(Ok(message)) = ws_rx.next().await {
            match message {
                Message::Binary(bytes) => {
                    let session = state.lock().await;
                    let _ = session.send_bytes(&bytes).await;
                }
                Message::Text(text) => {
                    if let Ok(command) = serde_json::from_str::<WsClientMessage>(&text) {
                        let session = state.lock().await;
                        match command {
                            WsClientMessage::Input { data } => {
                                let _ = session.send_bytes(data.as_bytes()).await;
                            }
                            WsClientMessage::Keys { keys } => {
                                let _ = session.send_keys(&keys).await;
                            }
                            WsClientMessage::Resize { cols, rows } => {
                                let _ = session.resize(cols, rows).await;
                            }
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    };
    tokio::pin!(sending, receiving);
    tokio::select! {
        _ = &mut sending => {},
        _ = &mut receiving => {},
    }
}

async fn ws_send(
    socket: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: Message,
) -> bool {
    matches!(
        tokio::time::timeout(WS_SEND_TIMEOUT, socket.send(message)).await,
        Ok(Ok(()))
    )
}

fn lag_warning(chunks: u64) -> Message {
    Message::Text(
        serde_json::json!({
            "type": "warning",
            "message": format!("dropped {chunks} output chunks (slow consumer)"),
        })
        .to_string()
        .into(),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::{Duration, Instant};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::session::tests::start_session;

    #[tokio::test]
    async fn failed_settlement_is_an_explicit_http_error() {
        let response = super::stop_response(Err(anyhow::anyhow!("owned child settlement unknown")));
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("settlement unknown")
        );
        assert!(body.get("ok").is_none());
    }

    /// Send one raw HTTP/1.1 request and read the full response (Connection: close).
    async fn http(port: u16, request: &str) -> String {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect failed");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("write failed");
        let mut response = Vec::new();
        stream
            .read_to_end(&mut response)
            .await
            .expect("read failed");
        String::from_utf8_lossy(&response).into_owned()
    }

    fn get(path: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
    }

    fn post_json(path: &str, body: &str) -> String {
        format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// A long-poll /wait must not hold the session mutex: the send and screen
    /// requests served mid-wait are exactly what make the wait complete.
    #[tokio::test(flavor = "multi_thread")]
    async fn wait_does_not_block_other_endpoints() {
        let session = start_session(vec!["/bin/sh".into()]).await;
        let router = super::build_router(session).router;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let start = Instant::now();
        // Long-poll for text that only appears if /control/send gets through mid-wait.
        let wait_task = tokio::spawn(async move {
            http(port, &get("/wait?text=WAIT_DONE_77&timeout_ms=30000")).await
        });

        // /query/screen must answer while the wait is in flight (deadline-polled).
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let resp = http(port, &get("/query/screen")).await;
            if resp.starts_with("HTTP/1.1 200") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "screen not served during wait: {resp}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let resp = http(
            port,
            &post_json("/control/send", r#"{"keys":"echo WAIT_DONE_77<CR>"}"#),
        )
        .await;
        assert!(
            resp.starts_with("HTTP/1.1 200"),
            "send not served during wait: {resp}"
        );

        let wait_resp = wait_task.await.unwrap();
        assert!(wait_resp.starts_with("HTTP/1.1 200"), "{wait_resp}");
        assert!(wait_resp.contains(r#""matched":true"#), "{wait_resp}");
        // A deadlocked handler could only return at its 30s timeout.
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "wait took {:?}",
            start.elapsed()
        );

        // Zero or multiple conditions are usage errors.
        let resp = http(port, &get("/wait")).await;
        assert!(resp.starts_with("HTTP/1.1 400"), "{resp}");
        let resp = http(port, &get("/wait?text=a&gone=b")).await;
        assert!(resp.starts_with("HTTP/1.1 400"), "{resp}");

        // Bounded shutdown so the shell doesn't outlive the test.
        let resp = http(port, &post_json("/control/send", r#"{"keys":"exit<CR>"}"#)).await;
        assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let resp = http(port, &get("/query/status")).await;
            if resp.contains(r#""alive":false"#) {
                break;
            }
            assert!(Instant::now() < deadline, "shell did not exit: {resp}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

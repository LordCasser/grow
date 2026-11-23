use std::future::pending;
use std::process::{Output, Stdio};
use std::time::Duration;

use axum::extract::{Path as RoutePath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::post};
use serde_json::{Value, json};
use test_support::TestSandbox;
use tokio::process::Command;
use tokio::sync::mpsc;

const CREATED: &str = "HTTP client created";
const HANDSHAKE_OK: &str = "MCP handshake OK";
const HANDSHAKE_TIMEOUT: &str = "MCP handshake timed out";
const DISCOVERY_OK: &str = "MCP tool discovery OK";
const DISCOVERY_TIMEOUT: &str = "MCP tool discovery timed out";

async fn mcp_fixture(
    RoutePath(server): RoutePath<String>,
    State(stalled_pages): State<mpsc::UnboundedSender<()>>,
    Json(request): Json<Value>,
) -> Response {
    let result = match request["method"].as_str() {
        Some("initialize") if server == "handshake-stall" => return pending().await,
        Some("initialize") => json!({
            "protocolVersion": request["params"]["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "doctor-fixture", "version": "1"},
        }),
        Some("notifications/initialized") => return StatusCode::ACCEPTED.into_response(),
        Some("tools/list") => {
            let cursor = request["params"]["cursor"].as_str();
            match cursor {
                None => json!({
                    "tools": [{"name": "first", "inputSchema": {"type": "object"}}],
                    "nextCursor": "second",
                }),
                Some("second") if server == "discovery-stall" => {
                    stalled_pages.send(()).unwrap();
                    return pending().await;
                }
                Some("second") => json!({
                    "tools": [{"name": "second", "inputSchema": {"type": "object"}}],
                }),
                _ => return StatusCode::BAD_REQUEST.into_response(),
            }
        }
        _ => return StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };
    Json(json!({"jsonrpc": "2.0", "id": request["id"], "result": result})).into_response()
}

async fn doctor(sandbox: &TestSandbox, args: &[&str], exit_code: i32) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_grow"));
    sandbox.apply_to_tokio_command(&mut command);
    command
        .current_dir(sandbox.workspace())
        .args(["mcp", "doctor"])
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(25), command.output())
        .await
        .unwrap_or_else(|_| panic!("grow mcp doctor {args:?} did not exit"))
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit_code),
        "args: {args:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

fn checks<'a>(report: &'a Value, name: &str, expected: &[(&str, bool)]) -> &'a [Value] {
    let server = report["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|server| server["name"] == name)
        .unwrap();
    assert_eq!(
        server["healthy"],
        expected.iter().all(|(_, passed)| *passed)
    );
    let actual = server["checks"].as_array().unwrap();
    assert_eq!(actual.len(), expected.len(), "{server}");
    for (check, (label, passed)) in actual.iter().zip(expected) {
        assert_eq!(check["label"], *label, "{server}");
        assert_eq!(check["passed"], *passed, "{server}");
    }
    actual
}

#[tokio::test]
async fn http_doctor_reports_probe_stages_and_real_exit_status() {
    let sandbox = TestSandbox::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let names = ["ready", "handshake-stall", "discovery-stall"];
    let mut config = String::new();
    for name in names {
        config.push_str(&format!(
            "\n[mcp_servers.{name}]\nurl = \"http://{address}/{name}\"\nstartup_timeout_sec = 1\n"
        ));
    }
    std::fs::write(sandbox.grow_home().join("config.toml"), config).unwrap();
    let (stalled_pages, observed) = mpsc::unbounded_channel();
    let app = Router::new()
        .route("/{server}", post(mcp_fixture))
        .with_state(stalled_pages);
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let (ok_json, bad_json, ok_text, handshake_text, discovery_text) = tokio::join!(
        doctor(&sandbox, &["--json", "ready"], 0),
        doctor(&sandbox, &["--json"], 1),
        doctor(&sandbox, &["ready"], 0),
        doctor(&sandbox, &["handshake-stall"], 1),
        doctor(&sandbox, &["discovery-stall"], 1),
    );
    server.abort();

    let successful = [(CREATED, true), (HANDSHAKE_OK, true), (DISCOVERY_OK, true)];
    let ok: Value = serde_json::from_slice(&ok_json.stdout).unwrap();
    assert_eq!(ok["healthy_count"], 1);
    assert_eq!(ok["failing_count"], 0);
    let ready = checks(&ok, "ready", &successful);
    assert!(
        ready[1]["detail"]
            .as_str()
            .unwrap()
            .starts_with("protocol 20")
    );
    assert!(ready[2]["detail"].as_str().unwrap().contains("2 tools"));

    let bad: Value = serde_json::from_slice(&bad_json.stdout).unwrap();
    assert_eq!(bad["healthy_count"], 1);
    assert_eq!(bad["failing_count"], 2);
    checks(&bad, "ready", &successful);
    let handshake = checks(
        &bad,
        "handshake-stall",
        &[(CREATED, true), (HANDSHAKE_TIMEOUT, false)],
    );
    assert!(
        handshake[1]["detail"]
            .as_str()
            .unwrap()
            .contains("within 1s")
    );
    let discovery = checks(
        &bad,
        "discovery-stall",
        &[
            (CREATED, true),
            (HANDSHAKE_OK, true),
            (DISCOVERY_TIMEOUT, false),
        ],
    );
    assert!(
        discovery[2]["detail"]
            .as_str()
            .unwrap()
            .contains("7s initialization budget")
    );

    let ok_text = String::from_utf8(ok_text.stdout).unwrap();
    for label in [
        CREATED,
        HANDSHAKE_OK,
        "protocol 20",
        DISCOVERY_OK,
        "2 tools",
        "Found 1 healthy, 0 failing.",
    ] {
        assert!(ok_text.contains(label), "missing {label}: {ok_text}");
    }
    let handshake_text = String::from_utf8(handshake_text.stdout).unwrap();
    assert!(handshake_text.contains("MCP handshake timed out (did not complete within 1s)"));
    assert!(!handshake_text.contains("MCP tool discovery"));
    let discovery_text = String::from_utf8(discovery_text.stdout).unwrap();
    assert!(discovery_text.contains("MCP tool discovery timed out (7s initialization budget"));

    assert_eq!(observed.len(), 2, "both failed probes reached page two");
}

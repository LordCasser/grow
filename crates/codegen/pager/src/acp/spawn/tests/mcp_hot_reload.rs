use super::*;
use acp_transport::{AcpAgentTx, acp_send, protocol as acp};
use serde_json::{Value, json};
use shell::agent::config::Config as AgentConfig;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

#[path = "mcp_hot_reload/lifecycle.rs"]
mod lifecycle;

#[derive(Default)]
struct McpCounts {
    initialize: AtomicUsize,
    initialize_completed: AtomicUsize,
    list: AtomicUsize,
    call: AtomicUsize,
    wire_call: AtomicUsize,
}

struct ToolCatalog {
    names: Vec<String>,
    argument: String,
}

struct McpBehavior {
    identity: Option<String>,
    catalog: Mutex<ToolCatalog>,
    pending_catalog: Mutex<Option<ToolCatalog>>,
    initialize_gate: Mutex<Option<Arc<tokio::sync::Semaphore>>>,
    call_delay_ms: std::sync::atomic::AtomicU64,
}

struct MockMcpServer {
    url: String,
    counts: Arc<McpCounts>,
    behavior: Arc<McpBehavior>,
    task: tokio::task::JoinHandle<()>,
}

impl MockMcpServer {
    async fn start() -> Self {
        Self::start_with_behavior(None, &["echo"], "value").await
    }

    async fn start_with_identity(identity: &str) -> Self {
        Self::start_with_behavior(Some(identity), &["echo"], "value").await
    }

    async fn start_with_tools(identity: &str, names: &[&str], argument: &str) -> Self {
        Self::start_with_behavior(Some(identity), names, argument).await
    }

    async fn start_blocked_with_identity(identity: &str) -> Self {
        Self::start_with_behavior(Some(identity), &["echo"], "value").await
    }

    async fn start_with_behavior(identity: Option<&str>, names: &[&str], argument: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let counts = Arc::new(McpCounts::default());
        let behavior = Arc::new(McpBehavior {
            identity: identity.map(str::to_owned),
            catalog: Mutex::new(ToolCatalog {
                names: names.iter().map(|name| (*name).to_owned()).collect(),
                argument: argument.to_owned(),
            }),
            pending_catalog: Mutex::new(None),
            initialize_gate: Mutex::new(None),
            call_delay_ms: std::sync::atomic::AtomicU64::new(0),
        });
        let counts_for_task = counts.clone();
        let behavior_for_task = behavior.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let counts = counts_for_task.clone();
                let behavior = behavior_for_task.clone();
                tokio::spawn(async move {
                    serve_mcp_connection(stream, counts, behavior).await;
                });
            }
        });
        Self {
            url: format!("http://{address}/mcp"),
            counts,
            behavior,
            task,
        }
    }

    fn queue_catalog_change(&self, names: &[&str], argument: &str) {
        *self.behavior.pending_catalog.lock().unwrap() = Some(ToolCatalog {
            names: names.iter().map(|name| (*name).to_owned()).collect(),
            argument: argument.to_owned(),
        });
    }

    fn block_initialization(&self) {
        *self.behavior.initialize_gate.lock().unwrap() =
            Some(Arc::new(tokio::sync::Semaphore::new(0)));
    }

    fn release_initialization(&self) {
        if let Some(gate) = self.behavior.initialize_gate.lock().unwrap().as_ref() {
            gate.add_permits(16);
        }
    }

    fn initialize_count(&self) -> usize {
        self.counts.initialize.load(Ordering::SeqCst)
    }

    fn initialize_completed_count(&self) -> usize {
        self.counts.initialize_completed.load(Ordering::SeqCst)
    }

    fn list_count(&self) -> usize {
        self.counts.list.load(Ordering::SeqCst)
    }

    fn call_count(&self) -> usize {
        self.counts.call.load(Ordering::SeqCst)
    }

    fn wire_call_count(&self) -> usize {
        self.counts.wire_call.load(Ordering::SeqCst)
    }
}

impl Drop for MockMcpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve_mcp_connection(
    mut stream: TcpStream,
    counts: Arc<McpCounts>,
    behavior: Arc<McpBehavior>,
) {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let (header_end, content_length) = loop {
        let Ok(read) = stream.read(&mut chunk).await else {
            return;
        };
        if read == 0 {
            return;
        }
        bytes.extend_from_slice(&chunk[..read]);
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if bytes.len() >= header_end + 4 + content_length {
            break (header_end, content_length);
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let request_line = headers.lines().next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default();
    let path = request_parts.next().unwrap_or_default();
    if method == "GET" {
        write_http_response(&mut stream, 405, "Method Not Allowed", &[]).await;
        return;
    }
    if method != "POST" || path != "/mcp" {
        write_http_response(&mut stream, 404, "Not Found", &[]).await;
        return;
    }

    let body_start = header_end + 4;
    let request: Value =
        match serde_json::from_slice(&bytes[body_start..body_start + content_length]) {
            Ok(request) => request,
            Err(_) => {
                write_http_response(&mut stream, 400, "Bad Request", &[]).await;
                return;
            }
        };
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        write_http_response(&mut stream, 400, "Bad Request", &[]).await;
        return;
    };
    let Some(id) = request.get("id") else {
        // MCP notifications such as notifications/initialized have no id.
        write_http_response(&mut stream, 202, "Accepted", &[]).await;
        return;
    };
    let mut send_tools_changed = false;
    let result = match method {
        "initialize" => {
            counts.initialize.fetch_add(1, Ordering::SeqCst);
            let initialize_gate = behavior.initialize_gate.lock().unwrap().clone();
            if let Some(gate) = initialize_gate {
                let permit = tokio::time::timeout(Duration::from_secs(30), gate.acquire())
                    .await
                    .expect("blocked initialize fixture timed out")
                    .expect("blocked initialize gate was closed");
                permit.forget();
            }
            counts.initialize_completed.fetch_add(1, Ordering::SeqCst);
            json!({
                "protocolVersion": request["params"]["protocolVersion"],
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "grow-test-mcp", "version": "1.0.0"}
            })
        }
        "tools/list" => {
            counts.list.fetch_add(1, Ordering::SeqCst);
            let catalog = behavior.catalog.lock().unwrap();
            let tools = catalog
                .names
                .iter()
                .map(|name| {
                    let mut input_schema = json!({
                        "type": "object",
                        "properties": {},
                        "required": []
                    });
                    input_schema["properties"][catalog.argument.clone()] =
                        json!({"type": "string"});
                    input_schema["required"] = json!([catalog.argument.clone()]);
                    json!({
                        "name": name,
                        "description": "Return the provided value",
                        "inputSchema": input_schema
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "tools": tools
            })
        }
        "tools/call" => {
            counts.wire_call.fetch_add(1, Ordering::SeqCst);
            let tool_name = request["params"]["name"].as_str().unwrap_or_default();
            let (tool_exists, value) = {
                let catalog = behavior.catalog.lock().unwrap();
                let value = request["params"]["arguments"][&catalog.argument]
                    .as_str()
                    .map(str::to_owned);
                (catalog.names.iter().any(|name| name == tool_name), value)
            };
            let Some(value) = value.filter(|_| tool_exists) else {
                let body = json!({
                    "jsonrpc":"2.0",
                    "id":id,
                    "error":{"code":-32602,"message":format!("unknown tool or invalid argument: {tool_name}")}
                })
                .to_string();
                write_http_response(&mut stream, 200, "OK", body.as_bytes()).await;
                return;
            };
            counts.call.fetch_add(1, Ordering::SeqCst);
            let delay = behavior.call_delay_ms.load(Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(delay)).await;
            if let Some(catalog) = behavior.pending_catalog.lock().unwrap().take() {
                *behavior.catalog.lock().unwrap() = catalog;
                send_tools_changed = true;
            }
            let text = behavior
                .identity
                .as_deref()
                .map_or_else(|| value.to_string(), |prefix| format!("{prefix}:{value}"));
            json!({
                "content": [{
                    "type": "text",
                    "text": text
                }],
                "isError": false
            })
        }
        _ => {
            let body = json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32601,"message":"method not found"}}).to_string();
            write_http_response(&mut stream, 200, "OK", body.as_bytes()).await;
            return;
        }
    };
    let response = json!({"jsonrpc":"2.0", "id":id, "result":result});
    let body = response.to_string();
    if send_tools_changed {
        let notification = json!({
            "jsonrpc":"2.0",
            "method":"notifications/tools/list_changed"
        });
        let sse = format!(
            "event: message\ndata: {}\n\nevent: message\ndata: {}\n\n",
            notification, response
        );
        write_http_response_with_content_type(
            &mut stream,
            200,
            "OK",
            "text/event-stream; charset=utf-8",
            sse.as_bytes(),
        )
        .await;
    } else {
        write_http_response(&mut stream, 200, "OK", body.as_bytes()).await;
    }
}

async fn write_http_response(stream: &mut TcpStream, status: u16, reason: &str, body: &[u8]) {
    write_http_response_with_content_type(stream, status, reason, "application/json", body).await;
}

async fn write_http_response_with_content_type(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) {
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    );
    let _ = stream.write_all(header.as_bytes()).await;
    let _ = stream.write_all(body).await;
    let _ = stream.shutdown().await;
}

fn mcp_config(name: &str, url: &str, enabled: bool) -> String {
    format!("[mcp_servers.{name}]\nurl = \"{url}\"\nenabled = {enabled}\n")
}

fn config_update(project_watch_enabled: bool, mcp_servers: &str) -> String {
    format!("[features]\nmcp_recursive_config_watch = {project_watch_enabled}\n\n{mcp_servers}")
}

async fn mcp_list(tx: &AcpAgentTx, session_id: &str) -> Value {
    let params = json!({"sessionId": session_id});
    let request = acp::ExtRequest::new(
        "grow/mcp/list",
        serde_json::value::to_raw_value(&params).unwrap().into(),
    );
    let response: acp::ExtResponse =
        tokio::time::timeout(Duration::from_secs(5), acp_send(request, tx))
            .await
            .expect("grow/mcp/list response timed out")
            .unwrap();
    extension_result(response)
}

fn extension_result(response: acp::ExtResponse) -> Value {
    let envelope: shell::session::ExtMethodResult<Value> =
        serde_json::from_str(response.0.get()).unwrap();
    assert!(envelope.error.is_none(), "{:?}", envelope.error);
    envelope.result.expect("successful extension result")
}

fn ready_server<'a>(list: &'a Value, name: &str) -> Option<&'a Value> {
    list["servers"].as_array()?.iter().find(|server| {
        server["name"] == name
            && server["session"]["enabled"] == true
            && server["session"]["status"] == "ready"
            && server["session"]["tools"]
                .as_array()
                .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == "echo"))
    })
}

async fn wait_for_ready_server(tx: &AcpAgentTx, session_id: &str, server: &str) -> Value {
    let mut last_list = Value::Null;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let list = mcp_list(tx, session_id).await;
            if ready_server(&list, server).is_some() {
                return list;
            }
            last_list = list;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("MCP server {server} did not become ready for {session_id}: {last_list}")
    })
}

async fn call_echo(tx: &AcpAgentTx, session_id: &str, server: &str, value: &str) -> Value {
    let params = json!({
        "sessionId": session_id,
        "server": server,
        "tool": "echo",
        "arguments": {"value": value}
    });
    let request = acp::ExtRequest::new(
        "grow/mcp/call",
        serde_json::value::to_raw_value(&params).unwrap().into(),
    );
    let response: acp::ExtResponse =
        tokio::time::timeout(Duration::from_secs(5), acp_send(request, tx))
            .await
            .expect("grow/mcp/call response timed out")
            .unwrap();
    extension_result(response)
}

async fn assert_echo(tx: &AcpAgentTx, session_id: &str, server: &str, value: &str) {
    let result = call_echo(tx, session_id, server, value).await;
    assert_eq!(result["content"][0]["text"], value);
    assert_eq!(result["isError"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn in_process_session_reloads_global_and_project_mcp_catalog() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::in_process_session_reloads_global_and_project_mcp_catalog"
    )) {
        return;
    }
    run_mcp_hot_reload(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn in_process_session_reloads_global_without_project_watch_when_disabled() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::in_process_session_reloads_global_without_project_watch_when_disabled"
    )) {
        return;
    }
    run_mcp_hot_reload(false).await;
}

async fn run_mcp_hot_reload(project_watch_enabled: bool) {
    diagnostics::tls::install_ring_provider_once();
    let grow_home = shell::util::grow_home::grow_home();
    std::fs::write(
        grow_home.join("config.toml"),
        format!("[features]\nmcp_recursive_config_watch = {project_watch_enabled}\n"),
    )
    .unwrap();
    let _project_watch = test_support::EnvGuard::set(
        "GROW_MCP_RECURSIVE_CONFIG_WATCH",
        if project_watch_enabled { "1" } else { "0" },
    );
    let _folder_trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");

    let workspace_a = tempfile::tempdir().unwrap();
    let workspace_b_actual = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let workspace_b = {
        let alias = workspace_a.path().join("workspace-b");
        std::os::unix::fs::symlink(workspace_b_actual.path(), &alias).unwrap();
        alias
    };
    #[cfg(not(unix))]
    let workspace_b = workspace_b_actual.path().to_path_buf();
    shell::agent::folder_trust::grant_folder_trust(workspace_a.path());
    shell::agent::folder_trust::grant_folder_trust(&workspace_b);

    let client_server = MockMcpServer::start().await;
    let global_server = MockMcpServer::start().await;
    let project_server = MockMcpServer::start().await;
    let disabled_server = MockMcpServer::start().await;

    let config_text = r#"
[models]
default = "test/default"

[provider.test]
api_backend = "chat_completions"

[provider.test.options]
base_url = "http://127.0.0.1:1/v1"
api_key = "test"

[provider.test.models.default]
context_window = 128000
"#;
    let raw_config: toml::Value = toml::from_str(config_text).unwrap();
    let mut agent_config = AgentConfig::new_from_toml_cfg(&raw_config).unwrap();
    agent_config.default_model_override = None;
    agent_config.models.default = Some("test/default".into());
    agent_config.remote_settings = Some(Default::default());

    let cancel = CancellationToken::new();
    let spawned = spawn_shell(agent_config, &cancel, None).await.unwrap();
    let mut guard = AgentShutdownGuard::new(cancel.clone(), Some(spawned.thread_handle));
    let channel = spawned.channel;
    let tx = channel.tx;
    let mut rx = channel.rx;
    let drain_cancel = cancel.clone();
    let drain_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = drain_cancel.cancelled() => break,
                message = rx.recv() => if message.is_none() { break },
            }
        }
    });

    let initialized: acp::InitializeResponse = tokio::time::timeout(
        Duration::from_secs(5),
        acp_send(acp::InitializeRequest::new(acp::ProtocolVersion::V1), &tx),
    )
    .await
    .expect("ACP initialize response timed out")
    .unwrap();
    assert!(initialized.protocol_version == acp::ProtocolVersion::V1);
    let session: acp::NewSessionResponse = tokio::time::timeout(
        Duration::from_secs(8),
        acp_send(
            acp::NewSessionRequest::new(workspace_a.path().to_path_buf()).mcp_servers(vec![
                acp::McpServer::Http(acp::McpServerHttp::new(
                    "client-server",
                    client_server.url.clone(),
                )),
            ]),
            &tx,
        ),
    )
    .await
    .expect("session/new response timed out")
    .unwrap();
    let session_id = session.session_id.0.to_string();
    wait_for_ready_server(&tx, &session_id, "client-server").await;
    assert_eq!(client_server.initialize_count(), 1);
    assert!(client_server.list_count() > 0);
    assert_echo(&tx, &session_id, "client-server", "before-refresh").await;

    let global_config = grow_home.join("config.toml");
    let global_mcp = format!(
        "{}\n{}\n{}",
        mcp_config("global-server", &global_server.url, true),
        mcp_config("disabled-server", &disabled_server.url, false),
        // Distinct names at one URL have independent identities. Adding the
        // second name must keep the existing client's connection alive.
        mcp_config("shadow-client", &client_server.url, true)
    );
    std::fs::write(
        &global_config,
        config_update(project_watch_enabled, &global_mcp),
    )
    .unwrap();
    wait_for_ready_server(&tx, &session_id, "global-server").await;
    assert_echo(&tx, &session_id, "global-server", "global-added").await;
    assert_echo(&tx, &session_id, "client-server", "client-preserved").await;
    wait_for_ready_server(&tx, &session_id, "shadow-client").await;
    assert_echo(&tx, &session_id, "shadow-client", "separate-name").await;
    assert_eq!(client_server.initialize_count(), 2);
    assert!(client_server.call_count() > 0);
    let refreshed_catalog = mcp_list(&tx, &session_id).await;
    assert!(ready_server(&refreshed_catalog, "client-server").is_some());
    assert!(ready_server(&refreshed_catalog, "shadow-client").is_some());
    assert!(global_server.list_count() > 0);
    assert_eq!(disabled_server.initialize_count(), 0);

    let global_initializations = global_server.initialize_count();
    std::fs::write(
        &global_config,
        config_update(project_watch_enabled, &global_mcp),
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert_eq!(global_server.initialize_count(), global_initializations);
    assert_eq!(client_server.initialize_count(), 2);

    let session_b: acp::NewSessionResponse = tokio::time::timeout(
        Duration::from_secs(8),
        acp_send(acp::NewSessionRequest::new(workspace_b.clone()), &tx),
    )
    .await
    .expect("second session/new response timed out")
    .unwrap();
    let session_b_id = session_b.session_id.0.to_string();
    wait_for_ready_server(&tx, &session_b_id, "global-server").await;
    wait_for_ready_server(&tx, &session_b_id, "shadow-client").await;
    let client_initializations_before_project = client_server.initialize_count();

    let project_config_dir = workspace_b.join(".grow");
    std::fs::create_dir(&project_config_dir).unwrap();
    std::fs::write(
        project_config_dir.join("config.toml"),
        mcp_config("project-server", &project_server.url, true),
    )
    .unwrap();
    if project_watch_enabled {
        wait_for_ready_server(&tx, &session_b_id, "project-server").await;
        assert_echo(&tx, &session_b_id, "project-server", "project-added").await;
    } else {
        // The watcher debounce is about one second; observe beyond it so a late
        // project catalog refresh cannot pass this assertion accidentally.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let session_b_catalog = mcp_list(&tx, &session_b_id).await;
        assert!(ready_server(&session_b_catalog, "project-server").is_none());
        assert_eq!(project_server.initialize_count(), 0);
    }

    let session_a_catalog = mcp_list(&tx, &session_id).await;
    assert!(
        ready_server(&session_a_catalog, "project-server").is_none(),
        "a project server must remain scoped to its owning session cwd"
    );
    assert_echo(
        &tx,
        &session_id,
        "client-server",
        "client-survives-project-refresh",
    )
    .await;
    assert_eq!(
        client_server.initialize_count(),
        client_initializations_before_project
    );
    if project_watch_enabled {
        assert_eq!(project_server.initialize_count(), 1);
        assert!(project_server.list_count() > 0);
    } else {
        assert_eq!(project_server.initialize_count(), 0);
    }

    cancel.cancel();
    let worker = guard.thread.take().expect("in-process worker handle");
    assert!(matches!(
        join_agent_thread(worker, Duration::from_secs(30)),
        JoinOutcome::Joined
    ));
    drop(guard);
    drain_task.abort();
    let _ = drain_task.await;

    // A global catalog change after shutdown must not leave a live watcher behind.
    let post_shutdown_mcp = format!(
        "{}\n{}",
        global_mcp,
        mcp_config("disabled-server", &disabled_server.url, true)
    );
    std::fs::write(
        &global_config,
        config_update(project_watch_enabled, &post_shutdown_mcp),
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(disabled_server.initialize_count(), 0);
}

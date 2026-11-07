//! AC-03a: a new session's LSP backend must be built from that session's own
//! plugin snapshot (cwd + trust + `_meta.pluginDirs`), not from the process
//! shared "latest" registry.
//!
//! The fixture drives the real public ACP entry (`MvpAgent::new` +
//! `connect_agent_v1`/`connect_client_v1` duplex pipes, `initialize`,
//! `authenticate`, `session/new` with per-session `pluginDirs`) and observes the
//! language servers that actually start: each plugin's LSP command is a stdio
//! JSON-RPC Python stub that appends one JSON line on spawn and again on
//! `initialize`, recording its label, pid, process cwd and the handshake's
//! `rootUri`.
//!
//! `rootUri` is the session-attribution evidence: the LSP manager is built with
//! the session cwd as its workspace root, so a stub that answers a session's
//! handshake names that session's cwd. The child's own process cwd is recorded
//! for diagnosis only — `LspClient::start_stdio` does not set `current_dir`, so
//! it is inherited from the test process and is never session identity.
//! `tool_ctx.lsp_server_names` is deliberately not asserted on: same-named
//! servers from different snapshots are indistinguishable there.
//!
//! Completion barrier: [`LspManager::ensure_initialized`] starts one session's
//! servers sequentially in `BTreeMap` key order and awaits each handshake before
//! starting the next. Every server key used here sorts before
//! [`USER_SERVER_NAME`] (`"project-lsp" < "user-lsp"`, `"snapshot-lsp" <
//! "user-lsp"`, both asserted in `assert_plugin_sorts_before_controls`), so
//! awaiting the user server's `initialize` for a cwd proves that backend already
//! finished everything it was given. Two sessions sharing one cwd each start
//! their own `user-lsp`, so that phase waits for two such handshakes. The
//! negative "this plugin must not start here" is therefore decided at a real
//! synchronization point, not after a sleep.
//!
//! The inherited-backend branch (`tool_ctx.lsp.is_some()`) is not reachable from
//! this primary entry: the context is freshly built with no LSP and the only
//! assignment in `spawn_and_register_session` is this branch. Its existing
//! pass-through (parent `ToolContext.lsp` Arc cloned into subagent contexts and
//! session actors) is covered by the existing directed unit tests, not
//! re-simulated here.
//!
//! Single `#[test]`: HOME/GROW_HOME/cwd mutation, the process-global cached
//! `grow_home()`, and plugin discovery are process-wide.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use acp_transport::{
    AcpAgentGatewayReceiver as GatewayReceiver, AcpAgentGatewaySender as GatewaySender,
    LineBufferedRead,
};
use acp_transport::{AcpAgentHandler as _, AcpClientHandler, protocol as acp};
use serde_json::{Value, json};
use shell::agent::config::Config as AgentConfig;
use shell::agent::mvp_agent::MvpAgent;
use tempfile::TempDir;
use test_support::MockInferenceServer;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

const DUPLEX_BUFFER_BYTES: usize = 8 * 1024 * 1024;
const RPC_TIMEOUT: Duration = Duration::from_secs(60);
/// Language-server handshake plus process spawn is asynchronous; this only
/// bounds the wait for evidence, it is not an expected latency.
const LSP_OBSERVE_TIMEOUT: Duration = Duration::from_secs(30);
/// Server key shared by every session-scope plugin below: the same key with
/// different commands is what makes a snapshot mix-up observable.
const PLUGIN_SERVER_NAME: &str = "snapshot-lsp";
const PROJECT_SERVER_NAME: &str = "project-lsp";
const USER_SERVER_NAME: &str = "user-lsp";
/// Launch-dir project plugin: manifest name and stub label.
const LAUNCH_PLUGIN_NAME: &str = "leak-lsp";

fn test_model_config(base_url: &str) -> String {
    format!(
        "[features]\nlsp_tools = true\n\n\
         [models]\ndefault = \"test/test-model\"\n\n\
         [provider.test]\napi_backend = \"chat_completions\"\n\n\
         [provider.test.options]\nbase_url = \"{base_url}\"\n\n\
         [provider.test.models.test-model]\nname = \"Test Model\"\ncontext_window = 128000\n"
    )
}

/// Same config plus the launch-dir project plugin enabled for the agent config
/// (the process shared registry is built from it). The session-level registry
/// reads disk config, so the user tier carries the same enable list for a
/// session whose own cwd is the launch dir.
fn launch_plugin_config(base_url: &str) -> String {
    format!(
        "{}\n[plugins]\nenabled = [\"{LAUNCH_PLUGIN_NAME}\"]\n",
        test_model_config(base_url)
    )
}

// ── Environment isolation ────────────────────────────────────────────────

/// RAII: set/remove an env var, restoring the prior value on drop.
struct EnvVarGuard {
    key: &'static str,
    prev: Option<std::ffi::OsString>,
}
impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let prev = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, prev }
    }
    fn remove(key: &'static str) -> Self {
        let prev = std::env::var_os(key);
        unsafe { std::env::remove_var(key) };
        Self { key, prev }
    }
}
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.prev.take() {
            Some(v) => unsafe { std::env::set_var(self.key, v) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

/// RAII process cwd for the launch-dir plugin discovery `MvpAgent::new` reads.
struct CwdGuard(PathBuf);
impl CwdGuard {
    fn set(dir: &Path) -> Self {
        let prev = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(dir).expect("set current dir");
        Self(prev)
    }
}
impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

// ── Stdio LSP stub ───────────────────────────────────────────────────────

/// Minimal LSP-over-stdio server. Honours `initialize` so the client's startup
/// barrier is satisfied, records the handshake's workspace identity, and exits
/// on `exit`/EOF. Every record is one `O_APPEND` write of a complete line.
const LSP_STUB: &str = r#"
import json, os, sys

LABEL = sys.argv[1]
MARKER = sys.argv[2]

def record(event, extra=None):
    entry = {"label": LABEL, "event": event, "pid": os.getpid(), "cwd": os.getcwd()}
    if extra:
        entry.update(extra)
    with open(MARKER, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry) + "\n")

def read_message():
    headers = {}
    while True:
        line = sys.stdin.readline()
        if not line:
            return None
        if line.strip() == "":
            break
        if ":" in line:
            key, value = line.split(":", 1)
            headers[key.strip()] = value.strip()
    length = int(headers.get("Content-Length", 0))
    if length == 0:
        return None
    return json.loads(sys.stdin.read(length))

def send_message(msg):
    body = json.dumps(msg)
    sys.stdout.write("Content-Length: %d\r\n\r\n%s" % (len(body), body))
    sys.stdout.flush()

record("spawn")

while True:
    msg = read_message()
    if msg is None:
        record("eof")
        break
    method = msg.get("method")
    msg_id = msg.get("id")
    if method == "initialize":
        params = msg.get("params") or {}
        folders = params.get("workspaceFolders") or []
        record("initialize", {
            "rootUri": params.get("rootUri"),
            "workspaceFolder": folders[0].get("uri") if folders else None,
        })
        send_message({
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {"capabilities": {"textDocumentSync": 1, "definitionProvider": True}},
        })
    elif method == "initialized":
        pass
    elif method == "shutdown":
        send_message({"jsonrpc": "2.0", "id": msg_id, "result": None})
    elif method == "exit":
        record("exit")
        break
    elif msg_id is not None:
        send_message({"jsonrpc": "2.0", "id": msg_id, "result": None})
"#;

fn write_lsp_stub(dir: &Path) -> PathBuf {
    let path = dir.join("lsp_stub.py");
    std::fs::write(&path, LSP_STUB).expect("write LSP stub");
    path
}

fn stub_server_config(stub: &Path, label: &str, marker: &Path) -> Value {
    json!({
        "command": "python3",
        "args": [
            stub.to_string_lossy(),
            label,
            marker.to_string_lossy(),
        ],
        "extensions": { ".txt": "plaintext" },
        "startup_timeout": 20000_u64,
    })
}

// ── Plugin / config fixtures ─────────────────────────────────────────────

/// Plugin declaring its LSP servers in a file (`.lsp.json` at the plugin root).
fn write_file_lsp_plugin(plugins_dir: &Path, name: &str, stub: &Path, label: &str, marker: &Path) {
    let dir = plugins_dir.join(name);
    std::fs::create_dir_all(&dir).expect("plugin dir");
    std::fs::write(dir.join("plugin.json"), format!(r#"{{"name":"{name}"}}"#)).expect("manifest");
    let servers = json!({ PLUGIN_SERVER_NAME: stub_server_config(stub, label, marker) });
    std::fs::write(dir.join(".lsp.json"), servers.to_string()).expect("lsp config");
}

/// Plugin declaring the same server key inline in `plugin.json`.
fn write_inline_lsp_plugin(
    plugins_dir: &Path,
    name: &str,
    stub: &Path,
    label: &str,
    marker: &Path,
) {
    let dir = plugins_dir.join(name);
    std::fs::create_dir_all(&dir).expect("plugin dir");
    let manifest = json!({
        "name": name,
        "lspServers": { PLUGIN_SERVER_NAME: stub_server_config(stub, label, marker) },
    });
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).expect("manifest");
}

/// User-tier `$GROW_HOME/lsp.json`: always eligible regardless of trust.
/// Distinct server key from every plugin server, so merge precedence can never
/// mask a wrongly-included plugin by name.
fn write_user_lsp(grow_home: &Path, stub: &Path, marker: &Path) {
    std::fs::create_dir_all(grow_home).expect("grow home");
    let servers = json!({ USER_SERVER_NAME: stub_server_config(stub, "user", marker) });
    std::fs::write(grow_home.join("lsp.json"), servers.to_string()).expect("user lsp.json");
}

/// Project-tier `<cwd>/.grow/lsp.json`: eligible when the folder is trusted.
fn write_project_lsp(cwd: &Path, stub: &Path, label: &str, marker: &Path) {
    let dir = cwd.join(".grow");
    std::fs::create_dir_all(&dir).expect("project .grow");
    let servers = json!({ PROJECT_SERVER_NAME: stub_server_config(stub, label, marker) });
    std::fs::write(dir.join("lsp.json"), servers.to_string()).expect("project lsp.json");
}

/// Project-scope plugin at the agent's launch cwd, enabled through
/// `[plugins].enabled` (agent config for the shared registry, user config for a
/// session whose own cwd is that directory).
fn write_launch_dir_plugin(launch_dir: &Path, stub: &Path, marker: &Path) {
    write_file_lsp_plugin(
        &launch_dir.join(".grow").join("plugins"),
        LAUNCH_PLUGIN_NAME,
        stub,
        LAUNCH_PLUGIN_NAME,
        marker,
    );
}

/// Project config that disables the plugin passed in the same session's
/// `_meta.pluginDirs`: discovered, but contributing nothing.
fn write_disabled_plugin_config(cwd: &Path, name: &str) {
    let dir = cwd.join(".grow");
    std::fs::create_dir_all(&dir).expect("project .grow");
    std::fs::write(
        dir.join("config.toml"),
        format!("[plugins]\ndisabled = [\"{name}\"]\n"),
    )
    .expect("project config");
}

// ── Marker evidence ──────────────────────────────────────────────────────

/// Parses every complete marker line strictly. A trailing unterminated fragment
/// is a concurrent append still in flight and is retried by the next poll; a
/// complete line that is not valid JSON is a fixture/protocol defect and fails
/// loudly — silently dropping it could erase exactly the leaked row a negative
/// assertion looks for.
fn read_events(marker: &Path) -> Vec<Value> {
    let Ok(contents) = std::fs::read_to_string(marker) else {
        return Vec::new();
    };
    let complete = match contents.rfind('\n') {
        Some(idx) => &contents[..=idx],
        None => return Vec::new(),
    };
    complete
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .unwrap_or_else(|e| panic!("malformed LSP marker line {line:?}: {e}"))
        })
        .collect()
}

/// Labels of servers whose `initialize` handshake reported `root_uri`.
fn initialized_labels(events: &[Value], root_uri: &str) -> BTreeSet<String> {
    events
        .iter()
        .filter(|e| e["event"] == json!("initialize"))
        .filter(|e| e["rootUri"].as_str() == Some(root_uri))
        .filter_map(|e| e["label"].as_str().map(str::to_owned))
        .collect()
}

fn initialize_count(events: &[Value], root_uri: &str, label: &str) -> usize {
    events
        .iter()
        .filter(|e| e["event"] == json!("initialize"))
        .filter(|e| e["rootUri"].as_str() == Some(root_uri))
        .filter(|e| e["label"] == json!(label))
        .count()
}

async fn wait_until(marker: &Path, mut done: impl FnMut(&[Value]) -> bool) -> Vec<Value> {
    let start = tokio::time::Instant::now();
    loop {
        let events = read_events(marker);
        if done(&events) {
            return events;
        }
        if start.elapsed() >= LSP_OBSERVE_TIMEOUT {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn spawn_pids(events: &[Value]) -> Vec<u64> {
    events
        .iter()
        .filter(|e| e["event"] == json!("spawn"))
        .filter_map(|e| e["pid"].as_u64())
        .collect()
}

/// Pids whose `initialize` handshake named one of `roots`.
///
/// Only an LSP backend built with that workspace root can answer with that
/// `rootUri`, so this is exactly the set of servers the sessions in question
/// started. The agent also owns a launch-dir workspace backend
/// (`workspace::handle`) that loads user/project LSP for the launch cwd with no
/// session `ProcessScope`; it can never match a session root here.
fn session_owned_pids(events: &[Value], roots: &[&str]) -> Vec<u64> {
    events
        .iter()
        .filter(|e| e["event"] == json!("initialize"))
        .filter(|e| roots.iter().any(|root| e["rootUri"] == json!(root)))
        .filter_map(|e| e["pid"].as_u64())
        .collect()
}

/// `ps` state of a pid: `None` when gone, `Some("Z…")` when a killed child has
/// not been reaped yet. Only pids this fixture recorded are ever queried.
fn process_state(pid: u64) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let state = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!state.is_empty()).then_some(state)
}

async fn wait_until_reclaimed(pids: &[u64]) -> Vec<(u64, String)> {
    let start = tokio::time::Instant::now();
    loop {
        let alive: Vec<(u64, String)> = pids
            .iter()
            .filter_map(|&pid| process_state(pid).map(|state| (pid, state)))
            .filter(|(_, state)| !state.starts_with('Z'))
            .collect();
        if alive.is_empty() || start.elapsed() >= Duration::from_secs(15) {
            return alive;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Synchronous, bounded post-kill check for the fixture-cleanup tail (the agent
/// runtime is no longer driving async work here).
fn wait_after_kill(pids: &[u64]) -> bool {
    let start = std::time::Instant::now();
    loop {
        let alive = pids
            .iter()
            .filter_map(|&pid| process_state(pid))
            .any(|state| !state.starts_with('Z'));
        if !alive || start.elapsed() >= Duration::from_secs(10) {
            return !alive;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ── ACP harness ──────────────────────────────────────────────────────────

struct AutoApproveClient;
#[async_trait::async_trait(?Send)]
impl AcpClientHandler for AutoApproveClient {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        let outcome = args
            .options
            .iter()
            .find(|o| o.kind == acp::PermissionOptionKind::AllowOnce)
            .or(args.options.first())
            .map(|o| {
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                    o.option_id.clone(),
                ))
            })
            .unwrap_or(acp::RequestPermissionOutcome::Cancelled);
        Ok(acp::RequestPermissionResponse::new(outcome))
    }
    async fn session_notification(&self, _args: acp::SessionNotification) -> acp::Result<()> {
        Ok(())
    }
}

async fn ext_method(
    conn: &acp_transport::ClientSideConnection,
    method: &str,
    params: Value,
) -> Value {
    let params_json =
        serde_json::value::RawValue::from_string(params.to_string()).expect("serialize ext params");
    let resp = tokio::time::timeout(
        RPC_TIMEOUT,
        conn.ext_method(acp::ExtRequest::new(
            method,
            std::sync::Arc::from(params_json),
        )),
    )
    .await
    .unwrap_or_else(|_| panic!("{method} timed out"))
    .unwrap_or_else(|e| panic!("{method} failed: {e}"));
    serde_json::from_str(resp.0.get()).unwrap_or_else(|e| panic!("{method}: bad response: {e}"))
}

/// Builds an in-process agent from `config_toml` and returns an initialized,
/// authenticated client connection over duplex pipes. IO tasks spawn on the
/// current `LocalSet`.
async fn connect_and_auth(config_toml: &str) -> acp_transport::ClientSideConnection {
    let raw: toml::Value = toml::from_str(config_toml).expect("valid explicit test model TOML");
    let mut agent_config =
        AgentConfig::new_from_toml_cfg(&raw).expect("valid explicit test config");
    agent_config.models.default = Some("test/test-model".to_owned());
    agent_config.default_model_override = None;
    agent_config.remote_settings = Some(shell::util::config::RemoteSettings::default());
    let (gw_tx, gw_rx) = tokio::sync::mpsc::unbounded_channel();
    let gateway = GatewaySender::new(gw_tx);
    let agent = MvpAgent::new(gateway, &agent_config).expect("valid config");
    let (c2a_a, c2a_b) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
    let (a2c_a, a2c_b) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
    let agent_incoming = LineBufferedRead::spawn_local(c2a_b.compat());
    let (agent_conn, agent_io) =
        acp_transport::connect_agent_v1(agent, a2c_a.compat_write(), agent_incoming, |fut| {
            tokio::task::spawn_local(fut);
        });
    tokio::task::spawn_local(GatewayReceiver::new(gw_rx, agent_conn).run());
    tokio::task::spawn_local(agent_io);
    let client_incoming = LineBufferedRead::spawn_local(a2c_b.compat());
    let (client_conn, client_io) = acp_transport::connect_client_v1(
        AutoApproveClient,
        c2a_a.compat_write(),
        client_incoming,
        |fut| {
            tokio::task::spawn_local(fut);
        },
    );
    tokio::task::spawn_local(client_io);
    let init = tokio::time::timeout(
        RPC_TIMEOUT,
        client_conn.initialize(
            acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                .client_capabilities(
                    acp::ClientCapabilities::new()
                        .fs(acp::FileSystemCapabilities::new())
                        .terminal(false),
                )
                .meta(
                    json!({
                        "startupHints": {
                            "nonInteractive": true,
                            "skipGitStatus": true,
                            "skipProjectLayout": true,
                        },
                        "clientType": "session-lsp-snapshot-test",
                        "clientVersion": "0.0-test",
                    })
                    .as_object()
                    .cloned(),
                ),
        ),
    )
    .await
    .expect("initialize timed out")
    .expect("initialize failed");
    let method = init
        .auth_methods
        .iter()
        .find(|m| &*m.id().0 == "provider.api_key")
        .expect("provider.api_key auth method not advertised");
    tokio::time::timeout(
        RPC_TIMEOUT,
        client_conn.authenticate(
            acp::AuthenticateRequest::new(method.id().clone())
                .meta(json!({ "headless": true }).as_object().cloned()),
        ),
    )
    .await
    .expect("authenticate timed out")
    .expect("authenticate failed");
    client_conn
}

async fn new_session(
    conn: &acp_transport::ClientSideConnection,
    cwd: &Path,
    plugin_dirs: &[PathBuf],
) -> acp::SessionId {
    tokio::time::timeout(
        RPC_TIMEOUT,
        conn.new_session(
            acp::NewSessionRequest::new(cwd.to_path_buf()).meta(
                json!({
                    "modelId": "test-model",
                    "pluginDirs": plugin_dirs
                        .iter()
                        .map(|p| p.to_string_lossy())
                        .collect::<Vec<_>>(),
                })
                .as_object()
                .cloned(),
            ),
        ),
    )
    .await
    .expect("session/new timed out")
    .expect("session/new failed")
    .session_id
}

/// Closes the sessions and proves their `ProcessScope` reaped exactly the
/// session-owned stub pids (never a global process-name kill).
async fn close_and_reap(
    conn: &acp_transport::ClientSideConnection,
    sessions: &[acp::SessionId],
    pids: &[u64],
) {
    for session in sessions {
        let resp = ext_method(
            conn,
            "grow/session/close",
            json!({ "sessionId": session.0.as_ref() }),
        )
        .await;
        assert_eq!(
            resp["result"]["success"],
            json!(true),
            "grow/session/close on {} failed: {resp}",
            session.0
        );
    }
    let alive = wait_until_reclaimed(&pids).await;
    // Fixture ownership: never leave a recorded stub running.
    for (pid, _) in &alive {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }
    assert!(
        alive.is_empty(),
        "session close must reap the LSP processes this fixture started: {alive:?}"
    );
}

fn file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .expect("absolute path has a file URL")
        .to_string()
}

fn labels(labels: &[&str]) -> BTreeSet<String> {
    labels.iter().map(|l| (*l).to_owned()).collect()
}

// ── Sandbox ──────────────────────────────────────────────────────────────

/// Isolated HOME/GROW_HOME plus fixture paths. Guards drop before the temp home
/// is removed.
struct Sandbox {
    home: PathBuf,
    grow_home: PathBuf,
    marker: PathBuf,
    stub: PathBuf,
    session_plugins: PathBuf,
    _proxy: EnvVarGuard,
    _inference: EnvVarGuard,
    _lsp_env: EnvVarGuard,
    _grow_home: EnvVarGuard,
    _home: EnvVarGuard,
    _home_tmp: TempDir,
}

impl Sandbox {
    fn new(server: &MockInferenceServer) -> Self {
        let home_tmp = TempDir::new().expect("home");
        let home = dunce::canonicalize(home_tmp.path()).expect("canonical home");
        let grow_home = home.join(".grow");
        let proxy = EnvVarGuard::set("GROW_CLI_CHAT_PROXY_BASE_URL", server.url());
        let inference = EnvVarGuard::set("GROW_INFERENCE_BASE_URL", server.url());
        // The feature flag comes from the config; make sure no ambient env
        // override decides for this test (env still outranks config in
        // production).
        let lsp_env = EnvVarGuard::remove("GROW_LSP_TOOLS");
        let grow_home_guard = EnvVarGuard::set("GROW_HOME", &grow_home);
        let home_guard = EnvVarGuard::set("HOME", &home);

        let fixture_root = home.join("fixtures");
        std::fs::create_dir_all(&fixture_root).expect("fixtures");
        let stub = write_lsp_stub(&fixture_root);
        let marker = fixture_root.join("lsp-events.jsonl");
        write_user_lsp(&grow_home, &stub, &marker);
        let session_plugins = fixture_root.join("session-plugins");
        Self {
            home,
            grow_home,
            marker,
            stub,
            session_plugins,
            _proxy: proxy,
            _inference: inference,
            _lsp_env: lsp_env,
            _grow_home: grow_home_guard,
            _home: home_guard,
            _home_tmp: home_tmp,
        }
    }

    fn dir(&self, name: &str) -> PathBuf {
        let dir = self.home.join(name);
        std::fs::create_dir_all(&dir).expect("project dir");
        dunce::canonicalize(&dir).expect("canonical project dir")
    }
}

/// The completion barrier below requires every non-user server key to sort
/// before `user-lsp`; assert it so a rename cannot quietly weaken the tests.
fn assert_plugin_sorts_before_controls() {
    assert!(
        PLUGIN_SERVER_NAME < USER_SERVER_NAME,
        "{PLUGIN_SERVER_NAME} must start before the user control"
    );
    assert!(
        PROJECT_SERVER_NAME < USER_SERVER_NAME,
        "{PROJECT_SERVER_NAME} must start before the user control"
    );
}

// ── Test ─────────────────────────────────────────────────────────────────

#[test]
fn session_lsp_uses_own_plugin_snapshot() {
    assert_plugin_sorts_before_controls();
    diagnostics::tls::install_ring_provider_once();
    let mock_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .expect("mock runtime");
    let server = mock_rt
        .block_on(MockInferenceServer::start())
        .expect("mock server");
    let sandbox = Sandbox::new(&server);
    // Owned handles for the async phase: `Sandbox` itself stays outside the
    // coroutine (its guards must outlive every session), so only owned paths are
    // captured.
    let stub = sandbox.stub.clone();
    let marker = sandbox.marker.clone();
    let plugins = sandbox.session_plugins.clone();
    let plugin = move |n: &str| dunce::canonicalize(plugins.join(n)).expect("canonical plugin dir");
    let (stub, marker) = (&stub, &marker);

    // Phase 1: one cwd, different session pluginDirs (regression entry point).
    let shared_cwd = sandbox.dir("shared-cwd");
    // Phase 2: different cwds, same plugin server key.
    let dir_c = sandbox.dir("project-c");
    let dir_d = sandbox.dir("project-d");
    write_project_lsp(&dir_d, stub, "project-d", marker);
    // Phase 3: a process latest really published by the launch dir.
    let neutral = sandbox.dir("neutral");
    let launcher = sandbox.dir("launcher");
    write_launch_dir_plugin(&launcher, stub, marker);
    let launch_plugin_dir = dunce::canonicalize(
        launcher
            .join(".grow")
            .join("plugins")
            .join(LAUNCH_PLUGIN_NAME),
    )
    .expect("launch plugin dir");
    let dir_g = sandbox.dir("project-g");
    let dir_e = sandbox.dir("project-e");
    write_project_lsp(&dir_e, stub, "project-e", marker);
    let dir_f = sandbox.dir("project-f");
    write_disabled_plugin_config(&dir_f, "snap-disabled");

    write_file_lsp_plugin(
        &sandbox.session_plugins,
        "snap-file",
        stub,
        "session-a",
        marker,
    );
    write_inline_lsp_plugin(
        &sandbox.session_plugins,
        "snap-inline",
        stub,
        "session-b",
        marker,
    );
    write_file_lsp_plugin(
        &sandbox.session_plugins,
        "snap-file-c",
        stub,
        "session-c",
        marker,
    );
    write_inline_lsp_plugin(
        &sandbox.session_plugins,
        "snap-inline-d",
        stub,
        "session-d",
        marker,
    );
    write_file_lsp_plugin(
        &sandbox.session_plugins,
        "snap-disabled",
        stub,
        "session-disabled",
        marker,
    );

    let agent_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("agent runtime");
    let local = tokio::task::LocalSet::new();
    agent_rt.block_on(local.run_until(async move {
        // ── Phase 1: same cwd, different `_meta.pluginDirs` ───────────────
        // Shared latest stays empty for this agent: it launches from `neutral`
        // (no project plugins) and session dirs are never published.
        let conn = {
            let _cwd = CwdGuard::set(&neutral);
            connect_and_auth(&test_model_config(&server.url())).await
        };
        let session_a = new_session(&conn, &shared_cwd, &[plugin("snap-file")]).await;

        let root_shared = file_uri(&shared_cwd);
        // A alone first: `user-lsp` is the last key a backend starts, so its
        // handshake proves this session finished starting and the label set is
        // final *for this one session*. Aggregating two sessions at one cwd
        // afterwards could not attribute a server to its owner.
        let events = wait_until(marker, |events| {
            initialize_count(events, &root_shared, "user") >= 1
        })
        .await;
        let evidence = serde_json::to_string_pretty(&events).expect("serialize evidence");
        assert_eq!(
            initialized_labels(&events, &root_shared),
            labels(&["session-a", "user"]),
            "session A alone must start only its own file-based plugin LSP plus user config\n{evidence}"
        );
        for expected in ["session-a", "user"] {
            assert_eq!(
                initialize_count(&events, &root_shared, expected),
                1,
                "session A must start `{expected}` exactly once\n{evidence}"
            );
        }
        assert!(
            events.iter().all(|e| e["label"] != json!("session-b")),
            "session B's plugin must not run before session B exists\n{evidence}"
        );
        eprintln!("phase1a marker evidence (session A only):\n{evidence}");

        // B joins the same cwd while A stays open. The second `user-lsp`
        // handshake is B's own completion point, so the only permitted change
        // is B's server appearing once — A's must not restart.
        let session_b = new_session(&conn, &shared_cwd, &[plugin("snap-inline")]).await;
        let events = wait_until(marker, |events| {
            initialize_count(events, &root_shared, "user") >= 2
        })
        .await;
        let evidence = serde_json::to_string_pretty(&events).expect("serialize evidence");
        assert_eq!(
            initialized_labels(&events, &root_shared),
            labels(&["session-a", "session-b", "user"]),
            "two sessions sharing a cwd must each start only their own plugin LSP\n{evidence}"
        );
        assert_eq!(
            initialize_count(&events, &root_shared, "session-a"),
            1,
            "session A's file plugin server must not start again for session B\n{evidence}"
        );
        assert_eq!(
            initialize_count(&events, &root_shared, "session-b"),
            1,
            "session B must add exactly one inline plugin server\n{evidence}"
        );
        let phase1_pids = session_owned_pids(&events, &[&root_shared]);
        eprintln!("phase1b marker evidence (sessions A and B):\n{evidence}");
        eprintln!("phase1 session-owned stub pids: {phase1_pids:?}");

        close_and_reap(&conn, &[session_a, session_b], &phase1_pids).await;

        // ── Phase 2: different cwds, same plugin server key ───────────────
        let session_c = new_session(&conn, &dir_c, &[plugin("snap-file-c")]).await;
        let session_d = new_session(&conn, &dir_d, &[plugin("snap-inline-d")]).await;
        let (root_c, root_d) = (file_uri(&dir_c), file_uri(&dir_d));
        let events = wait_until(marker, |events| {
            initialized_labels(events, &root_c).contains("user")
                && initialized_labels(events, &root_d).contains("user")
        })
        .await;
        let evidence = serde_json::to_string_pretty(&events).expect("serialize evidence");
        assert_eq!(
            initialized_labels(&events, &root_c),
            labels(&["session-c", "user"]),
            "session C must start its own file-based plugin LSP in its own cwd only\n{evidence}"
        );
        assert_eq!(
            initialized_labels(&events, &root_d),
            labels(&["project-d", "session-d", "user"]),
            "session D must start its own inline plugin LSP plus its allowed project LSP\n{evidence}"
        );
        eprintln!("phase2 marker evidence:\n{evidence}");

        close_and_reap(
            &conn,
            &[session_c, session_d],
            &session_owned_pids(&events, &[&root_c, &root_d]),
        )
        .await;

        // ── Phase 3: populated process latest vs empty/disabled snapshot ──
        let conn2 = {
            let _cwd = CwdGuard::set(&launcher);
            connect_and_auth(&launch_plugin_config(&server.url())).await
        };
        // Control: a session that legitimately includes the same plugin dir via
        // `_meta.pluginDirs` starts it, so the plugin really is active here and
        // the negative cases below are not vacuous.
        let session_g = new_session(&conn2, &dir_g, &[launch_plugin_dir.clone()]).await;
        // Session creation above runs the real shared-registry prime
        // (`ensure_plugin_registry` → `reload` at the launch cwd). An unknown
        // session id makes the public `grow/plugins/list` read that shared
        // snapshot, so the negative cases below rest on the registry production
        // actually published, not on config reasoning.
        let shared = ext_method(
            &conn2,
            "grow/plugins/list",
            json!({ "sessionId": "fixture-unknown-shared-latest" }),
        )
        .await;
        let shared_plugins = shared["result"]["plugins"]
            .as_array()
            .unwrap_or_else(|| panic!("grow/plugins/list returned no plugins array: {shared}"));
        let shared_names: BTreeSet<&str> = shared_plugins
            .iter()
            .filter_map(|p| p["name"].as_str())
            .collect();
        let shared_evidence =
            serde_json::to_string_pretty(&shared).expect("serialize shared registry");
        let launch_plugin = shared_plugins
            .iter()
            .find(|p| p["name"] == json!(LAUNCH_PLUGIN_NAME))
            .unwrap_or_else(|| {
                panic!("process shared latest must contain the launch plugin\n{shared_evidence}")
            });
        assert_eq!(
            launch_plugin["enabled"],
            json!(true),
            "the launch plugin in the shared latest must be active\n{shared_evidence}"
        );
        assert_eq!(
            launch_plugin["scope"],
            json!("project"),
            "the launch plugin is the launch cwd's project-scope plugin\n{shared_evidence}"
        );
        assert_eq!(
            launch_plugin["root"],
            json!(launch_plugin_dir.to_string_lossy()),
            "shared latest entry must be the launch dir plugin\n{shared_evidence}"
        );
        // `_meta.pluginDirs` plugins are session-only: they must never reach the
        // process registry, so other sessions cannot borrow them either.
        for session_only in ["snap-file", "snap-inline", "snap-disabled"] {
            assert!(
                !shared_names.contains(session_only),
                "session-only plugin `{session_only}` leaked into the process registry\n{shared_evidence}"
            );
        }
        eprintln!("phase3 shared registry evidence:\n{shared_evidence}");
        // No `pluginDirs`: this session's snapshot has no plugin at all.
        let session_e = new_session(&conn2, &dir_e, &[]).await;
        // Discovered but disabled by the project config: zero active plugins.
        let session_f = new_session(&conn2, &dir_f, &[plugin("snap-disabled")]).await;

        let (root_g, root_e, root_f) = (file_uri(&dir_g), file_uri(&dir_e), file_uri(&dir_f));
        let events = wait_until(marker, |events| {
            initialized_labels(events, &root_g).contains("user")
                && initialized_labels(events, &root_e).contains("user")
                && initialized_labels(events, &root_f).contains("user")
        })
        .await;
        let evidence = serde_json::to_string_pretty(&events).expect("serialize evidence");
        assert_eq!(
            initialized_labels(&events, &root_g),
            labels(&[LAUNCH_PLUGIN_NAME, "user"]),
            "control session that includes the launch plugin dir must start it (proves the plugin \
             is active); the process latest is primed from this same launch dir\n{evidence}"
        );
        assert_eq!(
            initialized_labels(&events, &root_e),
            labels(&["project-e", "user"]),
            "a session with no plugin contribution keeps only its own user/project LSP, never \
             the launch dir's plugin from the process latest\n{evidence}"
        );
        assert_eq!(
            initialized_labels(&events, &root_f),
            labels(&["user"]),
            "a session whose only plugin is disabled must contribute no plugin LSP\n{evidence}"
        );
        assert!(
            events.iter().all(|e| e["label"] != json!("session-disabled")),
            "a disabled session plugin must never start\n{evidence}"
        );
        // Session-only pluginDirs must never reach the process registry: no
        // other session may start them either.
        for leaked in ["session-a", "session-b", "session-c", "session-d"] {
            assert!(
                events
                    .iter()
                    .filter(|e| e["label"] == json!(leaked))
                    .all(|e| e["rootUri"] != json!(root_e) && e["rootUri"] != json!(root_f)),
                "session-only plugin `{leaked}` must not start in a session without it\n{evidence}"
            );
        }
        eprintln!("phase3 marker evidence:\n{evidence}");

        close_and_reap(
            &conn2,
            &[session_g, session_e, session_f],
            &session_owned_pids(&events, &[&root_g, &root_e, &root_f]),
        )
        .await;
    }));

    // Both agents are dropped here. Each also owns a launch-dir workspace
    // backend (`workspace::handle`, user/project LSP only, no session
    // `ProcessScope`) that this fix neither creates nor manages; it must still
    // not outlive the fixture.
    let leftover = spawn_pids(&read_events(&sandbox.marker));
    eprintln!("all fixture stub pids (recorded spawn events): {leftover:?}");
    let alive = agent_rt.block_on(wait_until_reclaimed(&leftover));
    for (pid, state) in &alive {
        eprintln!(
            "launch-dir workspace LSP pid {pid} still {state}; killing fixture-owned process"
        );
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }
    assert!(
        wait_after_kill(&leftover),
        "fixture-owned LSP processes must not outlive the test: {leftover:?}"
    );
}

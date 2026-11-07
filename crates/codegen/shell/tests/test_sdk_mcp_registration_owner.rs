//! Real Mvp new/resident/cold SDK registration, plus SharedMcpPool inheritance.
//! All phases share one re-executed process and one GROW_HOME. The fixture retains
//! the Mvp handle beside its real ACP connection only to inspect existing public
//! session/pool APIs; SDK replies always come from actual leader clients.
#![cfg(unix)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use acp_transport::{
    AcpAgentGatewayReceiver as GatewayReceiver, AcpAgentGatewaySender as GatewaySender,
    LineBufferedRead, protocol as acp,
};
use serde_json::{Value, json};
use shell::agent::config::Config as AgentConfig;
use shell::agent::mvp_agent::MvpAgent;
use shell::leader::{
    ClientCapabilities, ClientMode, LeaderClient, LeaderServerControlState, LeaderServerMetadata,
    run_leader_server,
};
use shell::session::mcp_servers::{McpState, SharedMcpPool};
use shell::session::{SessionCommand, SessionHandle};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use tokio_util::sync::CancellationToken;

const TEST: &str = "sdk_registration_survives_resident_load_and_rebinds_after_cold_load";
const CHILD: &str = "GROW_SDK_REGISTRATION_TEST_CHILD";
const TIMEOUT: Duration = Duration::from_secs(60);
const SERVER: &str = "sdk-owner";
const SERVER_ID: &str = "same-sdk-server";

struct ChildProcess(std::process::Child);
impl Drop for ChildProcess {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

#[test]
fn sdk_registration_survives_resident_load_and_rebinds_after_cold_load() {
    if std::env::var_os(CHILD).is_none() {
        let sandbox = tempfile::tempdir().unwrap();
        let grow_home = sandbox.path().join(".grow");
        let cwd = sandbox.path().join("project");
        std::fs::create_dir_all(&grow_home).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        // The child receives its environment before any runtime or library can
        // start threads/cache HOME. A current-thread runtime alone cannot do that.
        #[expect(
            clippy::disallowed_methods,
            reason = "owned fixture subprocess has bounded wait and kill/wait Drop"
        )]
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD, sandbox.path())
            .env("HOME", sandbox.path())
            .env("GROW_HOME", &grow_home)
            .env("GROW_API_KEY", "test-key-for-ci")
            .current_dir(&cwd)
            .spawn()
            .unwrap();
        let mut child = ChildProcess(child);
        let deadline = std::time::Instant::now() + Duration::from_secs(180);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success(), "isolated SDK lifecycle fixture: {status}");
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "SDK lifecycle child timed out"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    diagnostics::tls::install_ring_provider_once();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(tokio::task::LocalSet::new().run_until(lifecycle()));
}

type Rpc = (String, Value, oneshot::Sender<Value>);

struct Peer {
    owner: u64,
    requests: Rc<RefCell<Vec<Value>>>,
    rpc: mpsc::UnboundedSender<Rpc>,
    task: tokio::task::JoinHandle<()>,
}
impl Peer {
    async fn connect(socket: &Path, label: &'static str) -> Self {
        let mut client = LeaderClient::connect(
            socket.to_path_buf(),
            label,
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await
        .unwrap();
        let owner = client.registration().client_id;
        let requests = Rc::new(RefCell::new(Vec::new()));
        let observed = requests.clone();
        let (rpc, mut commands) = mpsc::unbounded_channel::<Rpc>();
        let task = tokio::task::spawn_local(async move {
            let mut id = 10_000_u64;
            let mut pending = HashMap::new();
            loop {
                tokio::select! {
                    command = commands.recv() => {
                        let Some((method, params, reply)) = command else { break };
                        id += 1;
                        pending.insert(id, reply);
                        client.send(json!({"jsonrpc":"2.0", "id":id,
                            "method":method, "params":params}).to_string()).unwrap();
                    }
                    message = client.recv() => {
                        let Some(message) = message else { break };
                        let message: Value = serde_json::from_str(&message).unwrap();
                        if let Some(method) = message["method"].as_str() {
                            if method == "_grow/mcp/sdk_call" {
                                let params = &message["params"];
                                assert_eq!(params["serverId"], SERVER_ID);
                                assert_eq!(params["_meta"]["grow/leaderClientId"], owner);
                                let inner = &params["message"];
                                let result = match inner["method"].as_str().unwrap() {
                                    "initialize" => json!({
                                        "protocolVersion":inner["params"]["protocolVersion"],
                                        "capabilities":{"tools":{}},
                                        "serverInfo":{"name":label,"version":"1.0"}
                                    }),
                                    "tools/list" => json!({"tools":[{
                                        "name":"ping", "description":"registration probe",
                                        "inputSchema":{"type":"object","properties":{}}
                                    }]}),
                                    "tools/call" => {
                                        assert_eq!(inner["params"]["name"], "ping");
                                        json!({"content":[{"type":"text","text":format!(
                                            "{label}:{}", inner["params"]["arguments"]["phase"].as_str().unwrap()
                                        )}]})
                                    }
                                    other => panic!("unexpected SDK method: {other}"),
                                };
                                observed.borrow_mut().push(message.clone());
                                client.send(json!({"jsonrpc":"2.0", "id":message["id"],
                                    "result":{"jsonrpc":"2.0", "id":inner["id"],
                                        "result":result}}).to_string()).unwrap();
                            } else {
                                assert!(message.get("id").is_none(), "unexpected reverse: {message}");
                            }
                        } else if let Some(reply) = message["id"].as_u64().and_then(|id| pending.remove(&id)) {
                            let _ = reply.send(message);
                        }
                    }
                }
            }
            client.cancel();
        });
        Self {
            owner,
            requests,
            rpc,
            task,
        }
    }

    async fn call(&self, method: &str, params: Value) -> Value {
        let (reply, response) = oneshot::channel();
        self.rpc.send((method.into(), params, reply)).unwrap();
        let response = tokio::time::timeout(TIMEOUT, response)
            .await
            .unwrap_or_else(|_| panic!("{method} timed out"))
            .unwrap();
        assert!(response.get("error").is_none(), "{method}: {response}");
        response["result"].clone()
    }

    fn assert_exchange(&self, session: &str, methods: &[&str], phase: &str) {
        let requests = self.requests.borrow();
        let actual: Vec<_> = requests
            .iter()
            .map(|request| request["params"]["message"]["method"].as_str().unwrap())
            .collect();
        assert_eq!(actual, methods, "SDK exchanges for {phase}");
        for request in requests.iter() {
            assert_eq!(request["params"]["sessionId"], session);
            assert_eq!(
                request["params"]["_meta"]["grow/leaderClientId"],
                self.owner
            );
            assert!(request["id"].is_string() || request["id"].is_number());
            assert!(!request["params"]["message"]["id"].is_null());
        }
        assert_eq!(
            requests.last().unwrap()["params"]["message"]["params"]["arguments"]["phase"],
            phase
        );
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// Same transport as leader::in_process::spawn_agent, retaining its MvpAgent Rc
// so the test can use get_session_handle and the existing pool command. No
// production hook or alternate SDK registration path is introduced.
fn spawn_agent(
    mut incoming: mpsc::UnboundedReceiver<String>,
    outgoing: mpsc::UnboundedSender<String>,
    config: &AgentConfig,
) -> (Rc<MvpAgent>, Vec<tokio::task::JoinHandle<()>>) {
    let (agent_read, mut agent_write) = tokio::io::simplex(8 * 1024 * 1024);
    let (output_read, output_write) = tokio::io::simplex(8 * 1024 * 1024);
    let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
    let agent = Rc::new(MvpAgent::new(GatewaySender::new(gateway_tx), config).unwrap());
    let (connection, io) = acp_transport::connect_agent_v1(
        agent.clone(),
        output_write.compat_write(),
        LineBufferedRead::spawn_local(agent_read.compat()),
        |future| {
            tokio::task::spawn_local(future);
        },
    );
    let tasks = vec![
        tokio::task::spawn_local(async move {
            let _ = io.await;
        }),
        tokio::task::spawn_local(GatewayReceiver::new(gateway_rx, connection).run()),
        tokio::task::spawn_local(async move {
            while let Some(message) = incoming.recv().await {
                agent_write.write_all(message.as_bytes()).await.unwrap();
                agent_write.write_all(b"\n").await.unwrap();
            }
        }),
        tokio::task::spawn_local(async move {
            let mut lines = BufReader::new(output_read).lines();
            while let Some(line) = lines.next_line().await.unwrap() {
                if outgoing.send(line).is_err() {
                    break;
                }
            }
        }),
    ];
    (agent, tasks)
}

async fn initialized_pool(handle: &SessionHandle) -> SharedMcpPool {
    let (respond_to, response) = oneshot::channel();
    handle
        .cmd_tx
        .send(SessionCommand::SnapshotInitializedMcpPool { respond_to })
        .unwrap();
    tokio::time::timeout(TIMEOUT, response)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}

fn declaration() -> Value {
    json!({"modelId":"test-model", "grow/mcp/servers":[{"name":SERVER,"serverId":SERVER_ID}]})
}
fn assert_result(result: Value, expected: &str) {
    assert_eq!(result["content"][0]["text"], expected, "{result}");
}
async fn call(peer: &Peer, session: &str, phase: &str) {
    let result = peer
        .call(
            "_grow/mcp/call",
            json!({
                "sessionId":session,"server":SERVER,"tool":"ping","arguments":{"phase":phase}
            }),
        )
        .await;
    assert_result(
        result["result"].clone(),
        &format!("{}:{phase}", if phase == "cold" { "B" } else { "A" }),
    );
}

async fn lifecycle() {
    let server = test_support::MockInferenceServer::start().await.unwrap();
    let config_text = format!(
        "[models]\ndefault = 'test/test-model'\n\
         [provider.test]\napi_backend = 'chat_completions'\n\
         [provider.test.options]\nbase_url = '{}'\n\
         [provider.test.models.test-model]\nname = 'Test Model'\ncontext_window = 128000\n",
        server.url(),
    );
    std::fs::write(::config::grow_home().join("config.toml"), &config_text).unwrap();
    let raw: toml::Value = toml::from_str(&config_text).unwrap();
    let mut config = AgentConfig::new_from_toml_cfg(&raw).unwrap();
    config.remote_settings = Some(shell::util::config::RemoteSettings::default());
    let socket = ::config::grow_home().join("sdk-owner.sock");
    let (to_agent, agent_requests) = mpsc::unbounded_channel();
    let (agent_responses, from_agent) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let control = LeaderServerControlState::new(LeaderServerMetadata {
        pid: std::process::id(),
        socket_path: socket.clone(),
        lock_path: socket.with_extension("lock"),
        leader_binary_version: env!("CARGO_PKG_VERSION").into(),
    });
    let leader = tokio::task::spawn_local(run_leader_server(
        socket.clone(),
        to_agent,
        from_agent,
        cancel.clone(),
        true,
        Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        shell::agent::activity::AgentActivity::default(),
        tokio::sync::watch::channel(true).1,
        tokio::sync::watch::channel(shell::leader::ShutdownReason::Manual).0,
        None,
        control,
    ));
    let (agent, tasks) = spawn_agent(agent_requests, agent_responses, &config);
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !socket.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "leader socket did not bind"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let a = Peer::connect(&socket, "A").await;
    let b = Peer::connect(&socket, "B").await;
    assert_ne!(a.owner, b.owner);
    a.call(
        "initialize",
        json!({"protocolVersion":1,"clientCapabilities":{},"_meta":{
            "startupHints":{"nonInteractive":true,"skipGitStatus":true,"skipProjectLayout":true},
            "clientType":"sdk-owner-test","clientVersion":"0.0.0-test"
        }}),
    )
    .await;
    a.call(
        "authenticate",
        json!({"methodId":"provider.api_key","_meta":{"headless":true}}),
    )
    .await;
    let cwd = std::env::current_dir().unwrap();
    let created = a
        .call(
            "session/new",
            json!({"cwd":cwd,"mcpServers":[],"_meta":declaration()}),
        )
        .await;
    let session = created["sessionId"].as_str().unwrap();
    let sid = acp::SessionId::new(session.to_owned());
    let original = agent.get_session_handle(&sid).unwrap();
    let pool = initialized_pool(&original).await;
    assert_eq!(pool.len(), 1);
    call(&b, session, "new").await;
    // New phase: one initialize, then two catalog listings — the background
    // init worker's `get_tool_registrations` discovery and the Ready-event
    // refresh (`SessionRestartActions::refresh_tools`) — both idempotent
    // listings over the same registered invoker, then the single tool call.
    a.assert_exchange(
        session,
        &["initialize", "tools/list", "tools/list", "tools/call"],
        "new",
    );
    assert!(b.requests.borrow().is_empty());
    a.requests.borrow_mut().clear();

    // B's valid fresh SDK declaration cannot replace a resident registration.
    b.call(
        "session/load",
        json!({"sessionId":session,"cwd":cwd,"mcpServers":[],"_meta":declaration()}),
    )
    .await;
    let resident = agent.get_session_handle(&sid).unwrap();
    assert!(original.cmd_tx.same_channel(&resident.cmd_tx));
    let resident_pool = initialized_pool(&resident).await;
    assert!(Arc::ptr_eq(
        pool.get_client(SERVER).unwrap(),
        resident_pool.get_client(SERVER).unwrap()
    ));
    call(&b, session, "resident").await;
    a.assert_exchange(session, &["tools/call"], "resident");
    assert!(b.requests.borrow().is_empty());
    a.requests.borrow_mut().clear();

    // The actual parent SDK Arc is imported through the production inheritance
    // API. No new registration or invoker is constructed for the inheritor.
    let mut grant = resident_pool.clone();
    grant.freeze_current_clients();
    let mut inherited = McpState::new(Vec::new());
    inherited.import_shared_clients(&grant);
    assert!(inherited.owned_clients.is_empty());
    assert!(Arc::ptr_eq(
        pool.get_client(SERVER).unwrap(),
        inherited.get_client(SERVER).unwrap()
    ));
    let inherited = Arc::new(tokio::sync::Mutex::new(inherited));
    let result = shell::extensions::mcp::call_mcp_tool(
        &inherited,
        SERVER,
        None,
        "ping",
        json!({"phase":"inherited"}),
    )
    .await
    .unwrap();
    assert_result(serde_json::to_value(result).unwrap(), "A:inherited");
    a.assert_exchange(session, &["tools/call"], "inherited");
    assert!(b.requests.borrow().is_empty());
    a.requests.borrow_mut().clear();
    drop(inherited);
    drop(grant);
    drop(resident_pool);
    drop(pool);
    drop(resident);

    // grow/session/close awaits the actor thread/writer retirement. A successful
    // immediate cold load is also a real check that its storage lease was freed.
    let closed = b
        .call("_grow/session/close", json!({"sessionId":session}))
        .await;
    assert_eq!(closed["result"]["success"], true, "{closed}");
    assert!(agent.get_session_handle(&sid).is_none());
    assert!(original.cmd_tx.is_closed());
    b.call(
        "session/load",
        json!({"sessionId":session,"cwd":cwd,"mcpServers":[],"_meta":declaration()}),
    )
    .await;
    let cold = agent.get_session_handle(&sid).unwrap();
    assert!(!original.cmd_tx.same_channel(&cold.cmd_tx));
    drop(original);
    let cold_pool = initialized_pool(&cold).await;
    assert_eq!(cold_pool.len(), 1);
    call(&a, session, "cold").await;
    // Cold load re-runs the full initialization, so the two-listing sequence
    // repeats on the new registration bound to B.
    b.assert_exchange(
        session,
        &["initialize", "tools/list", "tools/list", "tools/call"],
        "cold",
    );
    assert!(
        a.requests.borrow().is_empty(),
        "cold registration must not reuse A's invoker"
    );
    drop(cold_pool);
    drop(cold);
    b.call("_grow/session/close", json!({"sessionId":session}))
        .await;
    drop(a);
    drop(b);
    cancel.cancel();
    tokio::time::timeout(TIMEOUT, leader)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    for task in tasks {
        task.abort();
    }
}

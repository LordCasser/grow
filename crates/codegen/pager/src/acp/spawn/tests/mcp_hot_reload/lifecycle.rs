use super::*;
use std::path::Path;
use tokio::task::JoinHandle;

struct WorkerHarness {
    cancel: CancellationToken,
    guard: AgentShutdownGuard,
    tx: AcpAgentTx,
    drain_task: JoinHandle<()>,
}

fn explicit_agent_config() -> AgentConfig {
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
    let mut config = AgentConfig::new_from_toml_cfg(&raw_config).unwrap();
    config.default_model_override = None;
    config.models.default = Some("test/default".into());
    config.remote_settings = Some(Default::default());
    config
}

async fn start_worker() -> WorkerHarness {
    diagnostics::tls::install_ring_provider_once();
    let cancel = CancellationToken::new();
    let spawned = spawn_shell(explicit_agent_config(), &cancel, None)
        .await
        .unwrap();
    let guard = AgentShutdownGuard::new(cancel.clone(), Some(spawned.thread_handle));
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
    WorkerHarness {
        cancel,
        guard,
        tx,
        drain_task,
    }
}

async fn initialize(tx: &AcpAgentTx) {
    let _: acp::InitializeResponse = tokio::time::timeout(
        Duration::from_secs(5),
        acp_send(acp::InitializeRequest::new(acp::ProtocolVersion::V1), tx),
    )
    .await
    .expect("ACP initialize response timed out")
    .unwrap();
}

async fn stop_worker(mut worker: WorkerHarness) {
    worker.cancel.cancel();
    let handle = worker
        .guard
        .thread
        .take()
        .expect("in-process worker handle");
    assert!(matches!(
        join_agent_thread(handle, Duration::from_secs(30)),
        JoinOutcome::Joined
    ));
    drop(worker.guard);
    worker.drain_task.abort();
    let _ = worker.drain_task.await;
}

async fn new_session(tx: &AcpAgentTx, cwd: &Path, mcp_servers: Vec<acp::McpServer>) -> String {
    let response: acp::NewSessionResponse = tokio::time::timeout(
        Duration::from_secs(8),
        acp_send(
            acp::NewSessionRequest::new(cwd.to_path_buf()).mcp_servers(mcp_servers),
            tx,
        ),
    )
    .await
    .expect("session/new response timed out")
    .unwrap();
    response.session_id.0.to_string()
}

async fn load_session(
    tx: &AcpAgentTx,
    session_id: &str,
    cwd: &Path,
    mcp_servers: Vec<acp::McpServer>,
) {
    let request = acp::LoadSessionRequest::new(
        acp::SessionId::new(session_id.to_string()),
        cwd.to_path_buf(),
    )
    .mcp_servers(mcp_servers);
    let _: acp::LoadSessionResponse =
        tokio::time::timeout(Duration::from_secs(15), acp_send(request, tx))
            .await
            .expect("session/load response timed out")
            .unwrap();
}

fn client_http_server(name: &str, url: &str) -> acp::McpServer {
    acp::McpServer::Http(acp::McpServerHttp::new(name, url.to_string()))
}

async fn call_echo_result(
    tx: &AcpAgentTx,
    session_id: &str,
    server: &str,
    value: &str,
) -> Result<Value, String> {
    call_tool_result(tx, session_id, server, "echo", "value", value).await
}

async fn call_tool_result(
    tx: &AcpAgentTx,
    session_id: &str,
    server: &str,
    tool: &str,
    argument: &str,
    value: &str,
) -> Result<Value, String> {
    call_tool_result_with_server_url(tx, session_id, server, None, tool, argument, value).await
}

async fn call_tool_result_with_server_url(
    tx: &AcpAgentTx,
    session_id: &str,
    server: &str,
    server_url: Option<&str>,
    tool: &str,
    argument: &str,
    value: &str,
) -> Result<Value, String> {
    let mut arguments = json!({});
    arguments[argument] = json!(value);
    let params = json!({
        "sessionId": session_id,
        "server": server,
        "serverUrl": server_url,
        "tool": tool,
        "arguments": arguments
    });
    let request = acp::ExtRequest::new(
        "grow/mcp/call",
        serde_json::value::to_raw_value(&params).unwrap().into(),
    );
    let response = tokio::time::timeout(Duration::from_secs(5), acp_send(request, tx))
        .await
        .map_err(|_| "grow/mcp/call response timed out".to_string())?
        .map_err(|error| format!("grow/mcp/call RPC error: {error:?}"))?;
    let value: Value = serde_json::from_str(response.0.get())
        .map_err(|error| format!("invalid grow/mcp/call response: {error}"))?;
    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
        return Err(error.to_string());
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| format!("grow/mcp/call response has no result: {value}"))
}

async fn assert_identity_call(tx: &AcpAgentTx, session_id: &str, server: &str, identity: &str) {
    let result = call_echo_result(tx, session_id, server, "route-check")
        .await
        .unwrap();
    assert_eq!(
        result["content"][0]["text"],
        format!("{identity}:route-check")
    );
    assert_eq!(result["isError"], false);
}

async fn wait_for_identity_call(tx: &AcpAgentTx, session_id: &str, server: &str, identity: &str) {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Ok(result) = call_echo_result(tx, session_id, server, "route-check").await
                && result["content"][0]["text"] == format!("{identity}:route-check")
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("server {server} did not route to identity {identity}"));
}

async fn assert_call_fails_without_dispatch(tx: &AcpAgentTx, session_id: &str, server: &str) {
    assert!(
        call_echo_result(tx, session_id, server, "must-not-route")
            .await
            .is_err(),
        "removed server {server} unexpectedly accepted a call"
    );
}

async fn wait_for_absent_server(tx: &AcpAgentTx, session_id: &str, server: &str) -> bool {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let list = mcp_list(tx, session_id).await;
            if ready_server(&list, server).is_none() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or(false)
}

async fn wait_for_tool_names(
    tx: &AcpAgentTx,
    session_id: &str,
    server_name: &str,
    expected: &[&str],
) -> Value {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let list = mcp_list(tx, session_id).await;
            let tools = list["servers"]
                .as_array()
                .and_then(|servers| {
                    servers.iter().find(|server| {
                        server["name"] == server_name
                            && server["session"]["enabled"] == true
                            && server["session"]["status"] == "ready"
                    })
                })
                .map(|server| {
                    // McpSessionView omits an empty tools vector on the wire.
                    let mut names = server["session"]["tools"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
                        .collect::<Vec<_>>();
                    names.sort();
                    names
                });
            let mut expected = expected
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>();
            expected.sort();
            if tools.as_ref() == Some(&expected) {
                return list;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("tool catalog for {server_name} did not become {expected:?}"))
}

async fn wait_for_server_initializations(server: &MockMcpServer, expected: usize) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while server.initialize_count() < expected {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "MCP endpoint {} initialized {} times, expected at least {expected}",
            server.url,
            server.initialize_count()
        )
    });
}

async fn wait_for_completed_initializations(server: &MockMcpServer, expected: usize) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while server.initialize_completed_count() < expected {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "MCP endpoint {} completed {} initializations, expected at least {expected}",
            server.url,
            server.initialize_completed_count()
        )
    });
}

fn write_global_mcp(servers: &str) {
    let path = shell::util::grow_home::grow_home().join("config.toml");
    std::fs::write(&path, config_update(true, servers)).unwrap();
}

fn write_global_mcp_with_auto_restart(servers: &str, enabled: bool) {
    let path = shell::util::grow_home::grow_home().join("config.toml");
    std::fs::write(
        &path,
        format!(
            "[features]\nmcp_recursive_config_watch = true\nmcp_auto_restart = {enabled}\n\n{servers}"
        ),
    )
    .unwrap();
}

fn write_project_mcp(cwd: &Path, servers: &str) {
    let dir = cwd.join(".grow");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), servers).unwrap();
}

async fn isolated(test_name: &str) -> bool {
    crate::test_util::run_with_isolated_grow_home(test_name)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn global_and_project_catalog_lifecycle_tracks_current_endpoints() {
    let test_name = concat!(
        module_path!(),
        "::global_and_project_catalog_lifecycle_tracks_current_endpoints"
    );
    if isolated(test_name).await {
        return;
    }
    let _project_watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let _folder_trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");
    write_global_mcp("");

    let cwd_a = tempfile::tempdir().unwrap();
    let cwd_b = tempfile::tempdir().unwrap();
    shell::agent::folder_trust::grant_folder_trust(cwd_a.path());
    shell::agent::folder_trust::grant_folder_trust(cwd_b.path());
    let global_old = MockMcpServer::start_with_identity("global-old").await;
    let global_new = MockMcpServer::start_with_identity("global-new").await;
    let project_a_old = MockMcpServer::start_with_identity("project-a-old").await;
    let project_a_new = MockMcpServer::start_with_identity("project-a-new").await;
    let project_b = MockMcpServer::start_with_identity("project-b").await;

    let worker = start_worker().await;
    initialize(&worker.tx).await;
    let session_a = new_session(&worker.tx, cwd_a.path(), vec![]).await;
    let session_b = new_session(&worker.tx, cwd_b.path(), vec![]).await;

    write_global_mcp(&mcp_config("global-changing", &global_old.url, true));
    wait_for_ready_server(&worker.tx, &session_a, "global-changing").await;
    wait_for_ready_server(&worker.tx, &session_b, "global-changing").await;
    assert_identity_call(&worker.tx, &session_a, "global-changing", "global-old").await;
    assert_identity_call(&worker.tx, &session_b, "global-changing", "global-old").await;
    let old_global_calls = global_old.call_count();
    let old_global_initializations = global_old.initialize_count();

    write_project_mcp(
        cwd_a.path(),
        &mcp_config("project-changing", &project_a_old.url, true),
    );
    write_project_mcp(
        cwd_b.path(),
        &mcp_config("project-changing", &project_b.url, true),
    );
    wait_for_ready_server(&worker.tx, &session_a, "project-changing").await;
    wait_for_ready_server(&worker.tx, &session_b, "project-changing").await;
    assert_identity_call(&worker.tx, &session_a, "project-changing", "project-a-old").await;
    assert_identity_call(&worker.tx, &session_b, "project-changing", "project-b").await;
    assert_eq!(project_b.initialize_count(), 1);

    write_global_mcp(&mcp_config("global-changing", &global_new.url, true));
    wait_for_server_initializations(&global_new, 2).await;
    wait_for_ready_server(&worker.tx, &session_a, "global-changing").await;
    wait_for_ready_server(&worker.tx, &session_b, "global-changing").await;
    assert_identity_call(&worker.tx, &session_a, "global-changing", "global-new").await;
    assert_identity_call(&worker.tx, &session_b, "global-changing", "global-new").await;
    let old_global_wire_calls = global_old.wire_call_count();
    let new_global_wire_calls = global_new.wire_call_count();
    assert!(
        call_tool_result_with_server_url(
            &worker.tx,
            &session_a,
            "global-changing",
            Some(&global_old.url),
            "echo",
            "value",
            "retired-endpoint",
        )
        .await
        .is_err(),
        "an explicit retired URL must not fall back to the same-name replacement"
    );
    assert_eq!(global_old.wire_call_count(), old_global_wire_calls);
    assert_eq!(global_new.wire_call_count(), new_global_wire_calls);
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert_eq!(global_old.initialize_count(), old_global_initializations);
    assert_eq!(global_old.call_count(), old_global_calls);

    let project_b_initializations = project_b.initialize_count();
    let project_a_old_initializations_before_replace = project_a_old.initialize_count();
    write_project_mcp(
        cwd_a.path(),
        &mcp_config("project-changing", &project_a_new.url, true),
    );
    wait_for_server_initializations(&project_a_new, 1).await;
    wait_for_ready_server(&worker.tx, &session_a, "project-changing").await;
    assert_identity_call(&worker.tx, &session_a, "project-changing", "project-a-new").await;
    assert_identity_call(&worker.tx, &session_b, "project-changing", "project-b").await;
    assert_eq!(project_b.initialize_count(), project_b_initializations);
    assert_eq!(project_a_old.call_count(), 1);
    assert_eq!(
        project_a_old.initialize_count(),
        project_a_old_initializations_before_replace
    );

    write_global_mcp("");
    assert!(wait_for_absent_server(&worker.tx, &session_a, "global-changing").await);
    assert!(wait_for_absent_server(&worker.tx, &session_b, "global-changing").await);
    let global_old_calls = global_old.call_count();
    let global_new_calls = global_new.call_count();
    let global_old_wire_calls = global_old.wire_call_count();
    let global_new_wire_calls = global_new.wire_call_count();
    let global_new_initializations = global_new.initialize_count();
    assert_call_fails_without_dispatch(&worker.tx, &session_a, "global-changing").await;
    assert_call_fails_without_dispatch(&worker.tx, &session_b, "global-changing").await;
    assert_eq!(global_old.call_count(), global_old_calls);
    assert_eq!(global_new.call_count(), global_new_calls);
    assert_eq!(global_old.wire_call_count(), global_old_wire_calls);
    assert_eq!(global_new.wire_call_count(), global_new_wire_calls);
    assert_eq!(
        global_new.initialize_count(),
        global_new_initializations,
        "a removed global server must not be initialized again"
    );

    std::fs::remove_file(cwd_a.path().join(".grow/config.toml")).unwrap();
    assert!(wait_for_absent_server(&worker.tx, &session_a, "project-changing").await);
    let project_a_old_calls = project_a_old.call_count();
    let project_a_new_calls = project_a_new.call_count();
    let project_a_old_wire_calls = project_a_old.wire_call_count();
    let project_a_new_wire_calls = project_a_new.wire_call_count();
    let project_a_new_initializations = project_a_new.initialize_count();
    let project_b_calls_before_a_failure = project_b.call_count();
    let project_b_wire_calls_before_a_failure = project_b.wire_call_count();
    assert_call_fails_without_dispatch(&worker.tx, &session_a, "project-changing").await;
    assert_eq!(
        project_b.call_count(),
        project_b_calls_before_a_failure,
        "session A's removed project server call must not route into session B"
    );
    assert_eq!(
        project_b.wire_call_count(),
        project_b_wire_calls_before_a_failure
    );
    assert_identity_call(&worker.tx, &session_b, "project-changing", "project-b").await;
    assert_eq!(project_a_old.call_count(), project_a_old_calls);
    assert_eq!(project_a_new.call_count(), project_a_new_calls);
    assert_eq!(project_a_old.wire_call_count(), project_a_old_wire_calls);
    assert_eq!(project_a_new.wire_call_count(), project_a_new_wire_calls);
    assert_eq!(
        project_a_new.initialize_count(),
        project_a_new_initializations,
        "a removed project server must not be initialized again"
    );
    assert_eq!(
        project_a_old.initialize_count(),
        project_a_old_initializations_before_replace
    );
    assert_eq!(project_b.initialize_count(), project_b_initializations);

    std::fs::remove_file(cwd_b.path().join(".grow/config.toml")).unwrap();
    assert!(wait_for_absent_server(&worker.tx, &session_b, "project-changing").await);
    let project_b_calls = project_b.call_count();
    let project_b_wire_calls = project_b.wire_call_count();
    assert_call_fails_without_dispatch(&worker.tx, &session_b, "project-changing").await;
    assert_eq!(project_b.call_count(), project_b_calls);
    assert_eq!(project_b.wire_call_count(), project_b_wire_calls);
    stop_worker(worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn repository_ancestor_mcp_changes_refresh_nested_sessions() {
    let test_name = concat!(
        module_path!(),
        "::repository_ancestor_mcp_changes_refresh_nested_sessions"
    );
    if isolated(test_name).await {
        return;
    }
    let _project_watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let _folder_trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");
    write_global_mcp("");

    let repo = tempfile::tempdir().unwrap();
    git2::Repository::init(repo.path()).unwrap();
    let cwd_a = repo.path().join("a");
    let cwd_b = repo.path().join("b");
    std::fs::create_dir_all(&cwd_a).unwrap();
    std::fs::create_dir_all(&cwd_b).unwrap();
    shell::agent::folder_trust::grant_folder_trust(&cwd_a);
    shell::agent::folder_trust::grant_folder_trust(&cwd_b);

    let shared_old = MockMcpServer::start_with_identity("repo-shared-old").await;
    let shared_new = MockMcpServer::start_with_identity("repo-shared-new").await;
    let local_a = MockMcpServer::start_with_identity("a-local").await;

    let worker = start_worker().await;
    initialize(&worker.tx).await;
    let session_a = new_session(&worker.tx, &cwd_a, vec![]).await;
    let session_b = new_session(&worker.tx, &cwd_b, vec![]).await;

    write_project_mcp(
        repo.path(),
        &mcp_config("repo-shared", &shared_old.url, true),
    );
    wait_for_ready_server(&worker.tx, &session_a, "repo-shared").await;
    wait_for_ready_server(&worker.tx, &session_b, "repo-shared").await;
    assert_identity_call(&worker.tx, &session_a, "repo-shared", "repo-shared-old").await;
    assert_identity_call(&worker.tx, &session_b, "repo-shared", "repo-shared-old").await;

    write_project_mcp(
        repo.path(),
        &mcp_config("repo-shared", &shared_new.url, true),
    );
    wait_for_server_initializations(&shared_new, 2).await;
    wait_for_identity_call(&worker.tx, &session_a, "repo-shared", "repo-shared-new").await;
    wait_for_identity_call(&worker.tx, &session_b, "repo-shared", "repo-shared-new").await;
    let shared_old_wire_calls = shared_old.wire_call_count();

    std::fs::remove_file(repo.path().join(".grow/config.toml")).unwrap();
    assert!(wait_for_absent_server(&worker.tx, &session_a, "repo-shared").await);
    assert!(wait_for_absent_server(&worker.tx, &session_b, "repo-shared").await);
    let shared_new_wire_calls = shared_new.wire_call_count();
    assert_call_fails_without_dispatch(&worker.tx, &session_a, "repo-shared").await;
    assert_call_fails_without_dispatch(&worker.tx, &session_b, "repo-shared").await;
    assert_eq!(shared_old.wire_call_count(), shared_old_wire_calls);
    assert_eq!(shared_new.wire_call_count(), shared_new_wire_calls);

    write_project_mcp(&cwd_a, &mcp_config("a-local", &local_a.url, true));
    wait_for_ready_server(&worker.tx, &session_a, "a-local").await;
    assert_identity_call(&worker.tx, &session_a, "a-local", "a-local").await;
    let session_b_catalog = mcp_list(&worker.tx, &session_b).await;
    assert!(ready_server(&session_b_catalog, "a-local").is_none());
    let local_a_wire_calls = local_a.wire_call_count();
    assert_call_fails_without_dispatch(&worker.tx, &session_b, "a-local").await;
    assert_eq!(local_a.wire_call_count(), local_a_wire_calls);
    stop_worker(worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn resident_session_load_with_empty_client_catalog_removes_client_server() {
    let test_name = concat!(
        module_path!(),
        "::resident_session_load_with_empty_client_catalog_removes_client_server"
    );
    if isolated(test_name).await {
        return;
    }
    let _project_watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let client_server = MockMcpServer::start_with_identity("client-only").await;
    let global_server = MockMcpServer::start_with_identity("global-later").await;
    let cwd = tempfile::tempdir().unwrap();
    let _folder_trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");
    shell::agent::folder_trust::grant_folder_trust(cwd.path());
    write_global_mcp("");

    let worker = start_worker().await;
    initialize(&worker.tx).await;
    let session_id = new_session(
        &worker.tx,
        cwd.path(),
        vec![client_http_server("client-only", &client_server.url)],
    )
    .await;
    wait_for_ready_server(&worker.tx, &session_id, "client-only").await;
    assert_identity_call(&worker.tx, &session_id, "client-only", "client-only").await;

    load_session(&worker.tx, &session_id, cwd.path(), vec![]).await;
    let absent_after_empty_load =
        wait_for_absent_server(&worker.tx, &session_id, "client-only").await;
    write_global_mcp(&mcp_config("global-later", &global_server.url, true));
    wait_for_ready_server(&worker.tx, &session_id, "global-later").await;
    let absent_after_global_refresh =
        wait_for_absent_server(&worker.tx, &session_id, "client-only").await;
    assert!(
        absent_after_empty_load,
        "empty resident session/load must remove the previous client-only server"
    );
    assert!(
        absent_after_global_refresh,
        "a later global refresh must not resurrect the removed client-only server"
    );
    let calls_before = client_server.call_count();
    assert_call_fails_without_dispatch(&worker.tx, &session_id, "client-only").await;
    assert_eq!(client_server.call_count(), calls_before);
    assert_identity_call(&worker.tx, &session_id, "global-later", "global-later").await;
    stop_worker(worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn cold_session_load_uses_current_catalog_and_restores_project_watching() {
    let test_name = concat!(
        module_path!(),
        "::cold_session_load_uses_current_catalog_and_restores_project_watching"
    );
    if isolated(test_name).await {
        return;
    }
    let _project_watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let _folder_trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");
    let cwd = tempfile::tempdir().unwrap();
    shell::agent::folder_trust::grant_folder_trust(cwd.path());
    let client_server = MockMcpServer::start_with_identity("client-cold").await;
    let global_before = MockMcpServer::start_with_identity("global-before").await;
    let global_current = MockMcpServer::start_with_identity("global-current").await;
    let global_live = MockMcpServer::start_with_identity("global-live").await;
    let project_cold = MockMcpServer::start_with_identity("project-cold").await;
    let project_live = MockMcpServer::start_with_identity("project-live").await;
    let project_added_live = MockMcpServer::start_with_identity("project-added-live").await;

    write_global_mcp(&mcp_config("global-cold", &global_before.url, true));
    write_project_mcp(
        cwd.path(),
        &mcp_config("project-cold", &project_cold.url, true),
    );
    let first = start_worker().await;
    initialize(&first.tx).await;
    let client_config = client_http_server("client-cold", &client_server.url);
    let session_id = new_session(&first.tx, cwd.path(), vec![client_config.clone()]).await;
    wait_for_ready_server(&first.tx, &session_id, "client-cold").await;
    wait_for_ready_server(&first.tx, &session_id, "global-cold").await;
    wait_for_ready_server(&first.tx, &session_id, "project-cold").await;
    assert_identity_call(&first.tx, &session_id, "global-cold", "global-before").await;
    stop_worker(first).await;

    write_global_mcp(&mcp_config("global-cold", &global_current.url, true));
    write_project_mcp(
        cwd.path(),
        &mcp_config("project-cold", &project_live.url, true),
    );
    let old_global_calls_before_recovery = global_before.call_count();
    let old_project_initializations_before_recovery = project_cold.initialize_count();

    let second = start_worker().await;
    initialize(&second.tx).await;
    load_session(&second.tx, &session_id, cwd.path(), vec![client_config]).await;
    wait_for_server_initializations(&global_current, 1).await;
    wait_for_server_initializations(&project_live, 1).await;
    wait_for_ready_server(&second.tx, &session_id, "client-cold").await;
    wait_for_ready_server(&second.tx, &session_id, "global-cold").await;
    wait_for_ready_server(&second.tx, &session_id, "project-cold").await;
    assert_identity_call(&second.tx, &session_id, "client-cold", "client-cold").await;
    assert_identity_call(&second.tx, &session_id, "global-cold", "global-current").await;
    assert_identity_call(&second.tx, &session_id, "project-cold", "project-live").await;
    let catalog = mcp_list(&second.tx, &session_id).await;
    let current_global_entry = catalog["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == "global-cold")
        .expect("cold load must list current global configuration");
    assert_eq!(current_global_entry["url"], global_current.url);
    assert_eq!(global_before.call_count(), old_global_calls_before_recovery);
    assert_eq!(global_before.initialize_count(), 1);
    assert_eq!(
        project_cold.initialize_count(),
        old_project_initializations_before_recovery,
        "cold load must not reconnect the replaced project endpoint"
    );

    write_global_mcp(&format!(
        "{}\n{}",
        mcp_config("global-cold", &global_current.url, true),
        mcp_config("global-live", &global_live.url, true)
    ));
    wait_for_server_initializations(&global_live, 1).await;
    wait_for_ready_server(&second.tx, &session_id, "global-live").await;
    assert_identity_call(&second.tx, &session_id, "global-live", "global-live").await;
    write_project_mcp(
        cwd.path(),
        &format!(
            "{}\n{}",
            mcp_config("project-cold", &project_live.url, true),
            mcp_config("project-live", &project_added_live.url, true)
        ),
    );
    wait_for_server_initializations(&project_added_live, 1).await;
    wait_for_ready_server(&second.tx, &session_id, "project-live").await;
    assert_identity_call(
        &second.tx,
        &session_id,
        "project-live",
        "project-added-live",
    )
    .await;
    stop_worker(second).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn sse_tools_changed_refreshes_schema_with_auto_restart_disabled() {
    let test_name = concat!(
        module_path!(),
        "::sse_tools_changed_refreshes_schema_with_auto_restart_disabled"
    );
    if isolated(test_name).await {
        return;
    }
    let _project_watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let _liveness = test_support::EnvGuard::set("GROW_MCP_LIVENESS_WATCHERS", "1");
    let _auto_restart = test_support::EnvGuard::set("GROW_MCP_AUTO_RESTART", "false");
    let _folder_trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");
    write_global_mcp_with_auto_restart("", false);

    let cwd = tempfile::tempdir().unwrap();
    shell::agent::folder_trust::grant_folder_trust(cwd.path());
    let server = MockMcpServer::start_with_tools("mutable-catalog", &["echo"], "value").await;
    let worker = start_worker().await;
    initialize(&worker.tx).await;
    let session_id = new_session(&worker.tx, cwd.path(), vec![]).await;

    write_global_mcp_with_auto_restart(&mcp_config("mutable-catalog", &server.url, true), false);
    wait_for_ready_server(&worker.tx, &session_id, "mutable-catalog").await;
    wait_for_tool_names(&worker.tx, &session_id, "mutable-catalog", &["echo"]).await;

    server.queue_catalog_change(&["replacement"], "replacement_value");
    let changed_response = call_tool_result(
        &worker.tx,
        &session_id,
        "mutable-catalog",
        "echo",
        "value",
        "change-to-replacement",
    )
    .await
    .unwrap();
    assert_eq!(
        changed_response["content"][0]["text"],
        "mutable-catalog:change-to-replacement"
    );
    wait_for_tool_names(&worker.tx, &session_id, "mutable-catalog", &["replacement"]).await;
    let replacement_response = call_tool_result(
        &worker.tx,
        &session_id,
        "mutable-catalog",
        "replacement",
        "replacement_value",
        "new-schema",
    )
    .await
    .unwrap();
    assert_eq!(
        replacement_response["content"][0]["text"],
        "mutable-catalog:new-schema"
    );
    let calls_before_old_tool_attempt = server.call_count();
    assert!(
        call_tool_result(
            &worker.tx,
            &session_id,
            "mutable-catalog",
            "echo",
            "value",
            "stale-tool",
        )
        .await
        .is_err(),
        "the current MCP server must reject the retired tool name"
    );
    assert_eq!(server.call_count(), calls_before_old_tool_attempt);

    server.queue_catalog_change(&[], "unused");
    let empty_response = call_tool_result(
        &worker.tx,
        &session_id,
        "mutable-catalog",
        "replacement",
        "replacement_value",
        "change-to-empty",
    )
    .await
    .unwrap();
    assert_eq!(
        empty_response["content"][0]["text"],
        "mutable-catalog:change-to-empty"
    );
    wait_for_tool_names(&worker.tx, &session_id, "mutable-catalog", &[]).await;
    let calls_before_empty_tool_attempt = server.call_count();
    assert!(
        call_tool_result(
            &worker.tx,
            &session_id,
            "mutable-catalog",
            "replacement",
            "replacement_value",
            "must-not-route",
        )
        .await
        .is_err(),
        "the current MCP server must reject the retired tool after an empty tools/list"
    );
    assert_eq!(server.call_count(), calls_before_empty_tool_attempt);
    stop_worker(worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn stale_blocked_initialize_cannot_restore_replaced_or_deleted_project_servers() {
    let test_name = concat!(
        module_path!(),
        "::stale_blocked_initialize_cannot_restore_replaced_or_deleted_project_servers"
    );
    if isolated(test_name).await {
        return;
    }
    let _project_watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let _liveness = test_support::EnvGuard::set("GROW_MCP_LIVENESS_WATCHERS", "1");
    let _auto_restart = test_support::EnvGuard::set("GROW_MCP_AUTO_RESTART", "false");
    let _folder_trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");
    write_global_mcp_with_auto_restart("", false);

    let cwd_a = tempfile::tempdir().unwrap();
    let cwd_b = tempfile::tempdir().unwrap();
    shell::agent::folder_trust::grant_folder_trust(cwd_a.path());
    shell::agent::folder_trust::grant_folder_trust(cwd_b.path());
    let blocked_replace = MockMcpServer::start_blocked_with_identity("blocked-replace").await;
    let blocked_delete = MockMcpServer::start_blocked_with_identity("blocked-delete").await;
    blocked_replace.block_initialization();
    blocked_delete.block_initialization();
    let replacement = MockMcpServer::start_with_identity("replacement-endpoint").await;
    let other_session_server = MockMcpServer::start_with_identity("other-session").await;

    let worker = start_worker().await;
    initialize(&worker.tx).await;
    let session_a = new_session(&worker.tx, cwd_a.path(), vec![]).await;
    let session_b = new_session(&worker.tx, cwd_b.path(), vec![]).await;
    write_project_mcp(
        cwd_b.path(),
        &mcp_config("other-project-server", &other_session_server.url, true),
    );
    wait_for_ready_server(&worker.tx, &session_b, "other-project-server").await;
    assert_identity_call(
        &worker.tx,
        &session_b,
        "other-project-server",
        "other-session",
    )
    .await;

    write_project_mcp(
        cwd_a.path(),
        &format!(
            "{}\n{}",
            mcp_config("replace-race", &blocked_replace.url, true),
            mcp_config("delete-race", &blocked_delete.url, true)
        ),
    );
    wait_for_server_initializations(&blocked_replace, 1).await;
    wait_for_server_initializations(&blocked_delete, 1).await;

    write_project_mcp(
        cwd_a.path(),
        &mcp_config("replace-race", &replacement.url, true),
    );
    wait_for_server_initializations(&replacement, 1).await;
    wait_for_ready_server(&worker.tx, &session_a, "replace-race").await;
    blocked_replace.release_initialization();
    blocked_delete.release_initialization();
    wait_for_completed_initializations(&blocked_replace, 1).await;
    wait_for_completed_initializations(&blocked_delete, 1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_identity_call(
        &worker.tx,
        &session_a,
        "replace-race",
        "replacement-endpoint",
    )
    .await;
    assert!(wait_for_absent_server(&worker.tx, &session_a, "delete-race").await);
    let blocked_replace_calls = blocked_replace.call_count();
    let blocked_delete_calls = blocked_delete.call_count();
    assert_call_fails_without_dispatch(&worker.tx, &session_a, "delete-race").await;
    assert_eq!(blocked_replace.call_count(), blocked_replace_calls);
    assert_eq!(blocked_delete.call_count(), blocked_delete_calls);
    assert_eq!(blocked_replace.list_count(), 0);
    assert_eq!(blocked_delete.list_count(), 0);
    assert_eq!(blocked_replace.initialize_count(), 1);
    assert_eq!(blocked_delete.initialize_count(), 1);

    let other_initializations = other_session_server.initialize_count();
    let other_calls = other_session_server.call_count();
    assert_identity_call(
        &worker.tx,
        &session_b,
        "other-project-server",
        "other-session",
    )
    .await;
    assert_eq!(
        other_session_server.initialize_count(),
        other_initializations
    );
    assert_eq!(other_session_server.call_count(), other_calls + 1);
    stop_worker(worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn watcher_ready_reconciles_sessions_created_during_registration() {
    let test_name = concat!(
        module_path!(),
        "::watcher_ready_reconciles_sessions_created_during_registration"
    );
    if isolated(test_name).await {
        return;
    }
    let _project_watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let _folder_trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");
    write_global_mcp("");

    let home = shell::util::grow_home::grow_home();
    let (registration_started, registration_release, _registration_gate) =
        arm_config_watcher_registration_gate(home, WatcherRegistrationKind::Config);
    let global_server = MockMcpServer::start_with_identity("gap-global").await;
    let project_server = MockMcpServer::start_with_identity("gap-project").await;
    let worker = start_worker().await;
    registration_started
        .await
        .expect("config watcher registration should reach its test gate");

    initialize(&worker.tx).await;
    let cwd_a = tempfile::tempdir().unwrap();
    let cwd_b = tempfile::tempdir().unwrap();
    shell::agent::folder_trust::grant_folder_trust(cwd_a.path());
    shell::agent::folder_trust::grant_folder_trust(cwd_b.path());
    let session_a = new_session(&worker.tx, cwd_a.path(), vec![]).await;
    let session_b = new_session(&worker.tx, cwd_b.path(), vec![]).await;

    let latest_global = format!(
        "{}\n[disabled_mcp_tools]\ngap-global = [\"echo\"]\n",
        mcp_config("gap-global", &global_server.url, true)
    );
    write_global_mcp(&latest_global);
    write_project_mcp(
        cwd_a.path(),
        &mcp_config("gap-project", &project_server.url, true),
    );
    registration_release
        .send(())
        .expect("config watcher test gate should still be waiting");

    wait_for_ready_server(&worker.tx, &session_a, "gap-global").await;
    wait_for_ready_server(&worker.tx, &session_b, "gap-global").await;
    policy_identity::wait_enabled(&worker.tx, &session_a, "gap-global", false).await;
    policy_identity::wait_enabled(&worker.tx, &session_b, "gap-global", false).await;
    wait_for_ready_server(&worker.tx, &session_a, "gap-project").await;
    let session_b_catalog = mcp_list(&worker.tx, &session_b).await;
    assert!(ready_server(&session_b_catalog, "gap-project").is_none());

    let before = global_server.wire_call_count();
    assert_call_fails_without_dispatch(&worker.tx, &session_a, "gap-global").await;
    assert_eq!(global_server.wire_call_count(), before);
    assert_eq!(global_server.initialize_count(), 2);
    assert_eq!(project_server.initialize_count(), 1);
    stop_worker(worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn config_policy_hot_reload_continues_while_skills_registration_is_blocked() {
    let test_name = concat!(
        module_path!(),
        "::config_policy_hot_reload_continues_while_skills_registration_is_blocked"
    );
    if isolated(test_name).await {
        return;
    }
    let _project_watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let _folder_trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");

    let global_server = MockMcpServer::start_with_identity("skills-gate").await;
    write_global_mcp(&mcp_config("skills-gate", &global_server.url, true));
    let home = shell::util::grow_home::grow_home();
    let (registration_started, registration_release, _registration_gate) =
        arm_config_watcher_registration_gate(home, WatcherRegistrationKind::Skills);

    let worker = start_worker().await;
    registration_started
        .await
        .expect("skills watcher registration should reach its test gate");

    initialize(&worker.tx).await;
    let cwd = tempfile::tempdir().unwrap();
    shell::agent::folder_trust::grant_folder_trust(cwd.path());
    let session = new_session(&worker.tx, cwd.path(), vec![]).await;
    wait_for_ready_server(&worker.tx, &session, "skills-gate").await;
    policy_identity::wait_enabled(&worker.tx, &session, "skills-gate", true).await;

    write_global_mcp(&format!(
        "{}\n[disabled_mcp_tools]\nskills-gate = [\"echo\"]\n",
        mcp_config("skills-gate", &global_server.url, true)
    ));
    policy_identity::wait_enabled(&worker.tx, &session, "skills-gate", false).await;

    registration_release
        .send(())
        .expect("skills watcher test gate should still be waiting");
    stop_worker(worker).await;
}

#[path = "policy_identity.rs"]
mod policy_identity;

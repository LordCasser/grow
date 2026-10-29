use super::*;

pub(super) async fn wait_enabled(tx: &AcpAgentTx, session: &str, name: &str, enabled: bool) {
    let mut last_catalog = Value::Null;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let list = mcp_list(tx, session).await;
            if list["servers"].as_array().unwrap().iter().any(|server| {
                server["name"] == name
                    && server["session"]["tools"].as_array().is_some_and(|tools| {
                        tools
                            .iter()
                            .any(|tool| tool["name"] == "echo" && tool["enabled"] == enabled)
                    })
            }) {
                return;
            }
            last_catalog = list;
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("tool enable policy did not converge; last catalog: {last_catalog:#}")
    });
}

async fn wait_slow_call(tx: &AcpAgentTx, session: &str, server: &str, success: bool) {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let result = call_echo_result(tx, session, server, "policy-probe").await;
            if result.is_ok() == success {
                if let Err(error) = result {
                    assert!(error.contains("timed out"), "{error}");
                }
                return;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await
    .expect("call timeout policy did not converge");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn auxiliary_policy_hot_refresh_disables_and_retimes_existing_connections() {
    if isolated(concat!(
        module_path!(),
        "::auxiliary_policy_hot_refresh_disables_and_retimes_existing_connections"
    ))
    .await
    {
        return;
    }
    let _watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let _trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");
    let cwd = tempfile::tempdir().unwrap();
    shell::agent::folder_trust::grant_folder_trust(cwd.path());
    let server = MockMcpServer::start_with_identity("policy").await;
    let config = mcp_config("policy", &server.url, true);
    write_global_mcp(&config);
    let worker = start_worker().await;
    initialize(&worker.tx).await;
    let sid = new_session(&worker.tx, cwd.path(), vec![]).await;
    wait_for_ready_server(&worker.tx, &sid, "policy").await;
    wait_enabled(&worker.tx, &sid, "policy", true).await;
    let inits = server.initialize_count();

    // Only the disable mask changes: no endpoint diff or forced discovery.
    write_global_mcp(&format!(
        "{config}\n[disabled_mcp_tools]\npolicy = [\"echo\"]\n"
    ));
    wait_enabled(&worker.tx, &sid, "policy", false).await;
    let before = server.wire_call_count();
    assert_call_fails_without_dispatch(&worker.tx, &sid, "policy").await;
    assert_eq!(server.wire_call_count(), before);
    write_global_mcp(&config);
    wait_enabled(&worker.tx, &sid, "policy", true).await;
    assert_identity_call(&worker.tx, &sid, "policy", "policy").await;

    server.behavior.call_delay_ms.store(1250, Ordering::SeqCst);
    write_global_mcp(&format!(
        "{config}\ntool_timeout_sec = 3\n[mcp_servers.policy.tool_timeouts]\necho = 1\n"
    ));
    wait_slow_call(&worker.tx, &sid, "policy", false).await;
    // Removing just the tool override reveals the server-level timeout.
    write_global_mcp(&format!("{config}\ntool_timeout_sec = 3\n"));
    wait_slow_call(&worker.tx, &sid, "policy", true).await;
    // A lower server default applies to the existing Arc/client too.
    write_global_mcp(&format!("{config}\ntool_timeout_sec = 1\n"));
    wait_slow_call(&worker.tx, &sid, "policy", false).await;
    write_global_mcp(&config);
    wait_slow_call(&worker.tx, &sid, "policy", true).await;
    assert_eq!(
        server.initialize_count(),
        inits,
        "policy edits must retain this connection"
    );
    stop_worker(worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn server_name_identity_survives_layer_conflicts_reload_and_cold_load() {
    if isolated(concat!(
        module_path!(),
        "::server_name_identity_survives_layer_conflicts_reload_and_cold_load"
    ))
    .await
    {
        return;
    }
    let _watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let _trust = test_support::EnvGuard::unset("GROW_FOLDER_TRUST");
    let cwd = tempfile::tempdir().unwrap();
    shell::agent::folder_trust::grant_folder_trust(cwd.path());
    let disk = MockMcpServer::start_with_identity("disk").await;
    let client = MockMcpServer::start_with_identity("client").await;
    let other = MockMcpServer::start_with_identity("other").await;
    let disk_new = MockMcpServer::start_with_identity("disk-new").await;
    write_global_mcp(&mcp_config("slot", &disk.url, true));
    let worker = start_worker().await;
    initialize(&worker.tx).await;
    let client_catalog = vec![
        client_http_server("slot", &client.url),
        client_http_server("other", &other.url),
    ];
    let a = new_session(&worker.tx, cwd.path(), client_catalog.clone()).await;
    let b = new_session(&worker.tx, cwd.path(), vec![]).await;
    wait_for_ready_server(&worker.tx, &a, "slot").await;
    wait_for_ready_server(&worker.tx, &a, "other").await;
    wait_for_ready_server(&worker.tx, &b, "slot").await;
    assert_identity_call(&worker.tx, &a, "slot", "client").await;
    assert_identity_call(&worker.tx, &b, "slot", "disk").await;
    let list = mcp_list(&worker.tx, &a).await;
    let entries = list["servers"].as_array().unwrap();
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry["name"] == "slot")
            .unwrap()["url"],
        client.url
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry["name"] == "other")
            .unwrap()["url"],
        other.url
    );
    assert_eq!(
        list["servers"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["name"] == "slot")
            .count(),
        1
    );
    let before = [
        disk.wire_call_count(),
        client.wire_call_count(),
        other.wire_call_count(),
    ];
    for wrong_url in [&disk.url, &other.url] {
        assert!(
            call_tool_result_with_server_url(
                &worker.tx,
                &a,
                "slot",
                Some(wrong_url),
                "echo",
                "value",
                "no-partial-match"
            )
            .await
            .is_err()
        );
    }
    assert_eq!(
        before,
        [
            disk.wire_call_count(),
            client.wire_call_count(),
            other.wire_call_count()
        ]
    );
    let exact = call_tool_result_with_server_url(
        &worker.tx,
        &a,
        "slot",
        Some(&client.url),
        "echo",
        "value",
        "exact",
    )
    .await
    .unwrap();
    assert_eq!(exact["content"][0]["text"], "client:exact");
    async fn mutate(tx: &AcpAgentTx, method: &str, params: Value) {
        let request = acp::ExtRequest::new(
            method,
            serde_json::value::to_raw_value(&params).unwrap().into(),
        );
        let _: acp::ExtResponse =
            tokio::time::timeout(Duration::from_secs(10), acp_send(request, tx))
                .await
                .unwrap()
                .unwrap();
    }
    // Configuration UI mutations must resolve the same slot as disk reloads.
    mutate(
        &worker.tx,
        "grow/mcp/upsert",
        json!({"session_id": a, "server_name":"slot", "url":disk_new.url}),
    )
    .await;
    assert_identity_call(&worker.tx, &a, "slot", "client").await;
    wait_for_identity_call(&worker.tx, &b, "slot", "disk-new").await;
    mutate(
        &worker.tx,
        "grow/mcp/toggle",
        json!({"session_id": a, "server_name":"slot", "enabled":false}),
    )
    .await;
    assert_call_fails_without_dispatch(&worker.tx, &a, "slot").await;
    mutate(
        &worker.tx,
        "grow/mcp/toggle",
        json!({"session_id": a, "server_name":"slot", "enabled":true}),
    )
    .await;
    wait_for_ready_server(&worker.tx, &a, "slot").await;
    assert_identity_call(&worker.tx, &a, "slot", "client").await;
    mutate(
        &worker.tx,
        "grow/mcp/delete",
        json!({"session_id": a, "server_name":"slot"}),
    )
    .await;
    assert_identity_call(&worker.tx, &a, "slot", "client").await;
    let after_mutation = mcp_list(&worker.tx, &a).await;
    assert_eq!(
        after_mutation["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "slot")
            .unwrap()["url"],
        client.url
    );
    write_global_mcp(&mcp_config("slot", &disk_new.url, true));
    wait_for_identity_call(&worker.tx, &b, "slot", "disk-new").await;
    assert_identity_call(&worker.tx, &a, "slot", "client").await;
    stop_worker(worker).await;

    let worker = start_worker().await;
    initialize(&worker.tx).await;
    load_session(&worker.tx, &a, cwd.path(), client_catalog).await;
    wait_for_ready_server(&worker.tx, &a, "slot").await;
    assert_identity_call(&worker.tx, &a, "slot", "client").await;
    assert!(
        call_tool_result_with_server_url(
            &worker.tx,
            &a,
            "slot",
            Some(&disk_new.url),
            "echo",
            "value",
            "cold-mismatch"
        )
        .await
        .is_err()
    );
    load_session(&worker.tx, &a, cwd.path(), vec![]).await;
    wait_for_identity_call(&worker.tx, &a, "slot", "disk-new").await;
    stop_worker(worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn duplicate_client_names_are_rejected_before_new_load_or_update_mutates_session() {
    if isolated(concat!(
        module_path!(),
        "::duplicate_client_names_are_rejected_before_new_load_or_update_mutates_session"
    ))
    .await
    {
        return;
    }
    let cwd = tempfile::tempdir().unwrap();
    write_global_mcp("");
    let first = MockMcpServer::start_with_identity("first").await;
    let second = MockMcpServer::start_with_identity("second").await;
    let worker = start_worker().await;
    initialize(&worker.tx).await;
    let dup = vec![
        client_http_server("same", &first.url),
        client_http_server("same", &second.url),
    ];
    let result: Result<acp::NewSessionResponse, _> = acp_send(
        acp::NewSessionRequest::new(cwd.path().to_path_buf()).mcp_servers(dup.clone()),
        &worker.tx,
    )
    .await;
    assert!(format!("{:?}", result.unwrap_err()).contains("duplicate MCP server name"));
    assert_eq!(first.initialize_count() + second.initialize_count(), 0);
    let sid = new_session(&worker.tx, cwd.path(), vec![dup[0].clone()]).await;
    wait_for_ready_server(&worker.tx, &sid, "same").await;
    let result: Result<acp::LoadSessionResponse, _> = acp_send(
        acp::LoadSessionRequest::new(acp::SessionId::new(sid.clone()), cwd.path().to_path_buf())
            .mcp_servers(dup.clone()),
        &worker.tx,
    )
    .await;
    assert!(format!("{:?}", result.unwrap_err()).contains("duplicate MCP server name"));
    let req = acp::ExtRequest::new(
        "grow/session/update_mcp_servers",
        serde_json::value::to_raw_value(&json!({"sessionId": sid, "mcpServers": dup}))
            .unwrap()
            .into(),
    );
    assert!(
        format!("{:?}", acp_send(req, &worker.tx).await.unwrap_err())
            .contains("duplicate MCP server name")
    );
    assert_identity_call(&worker.tx, &sid, "same", "first").await;
    assert_eq!(second.initialize_count(), 0);
    assert_eq!(second.wire_call_count(), 0);
    stop_worker(worker).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn pending_initialize_and_cold_load_use_latest_auxiliary_policy() {
    if isolated(concat!(
        module_path!(),
        "::pending_initialize_and_cold_load_use_latest_auxiliary_policy"
    ))
    .await
    {
        return;
    }
    let _watch = test_support::EnvGuard::set("GROW_MCP_RECURSIVE_CONFIG_WATCH", "1");
    let cwd = tempfile::tempdir().unwrap();
    let server = MockMcpServer::start_with_identity("pending-policy").await;
    server.block_initialization();
    let config = mcp_config("pending-policy", &server.url, true);
    write_global_mcp(&format!(
        "{config}\nstartup_timeout_sec = 10\ntool_timeout_sec = 1\n"
    ));
    let worker = start_worker().await;
    initialize(&worker.tx).await;
    let sid = new_session(&worker.tx, cwd.path(), vec![]).await;
    wait_for_server_initializations(&server, 1).await;
    let latest = format!("{config}\nstartup_timeout_sec = 2\ntool_timeout_sec = 3\n");
    write_global_mcp(&format!(
        "{latest}\n[disabled_mcp_tools]\npending-policy = [\"echo\"]\n"
    ));
    // Explicitly admit a policy-only reload while the old handshake is blocked.
    let reload = acp::ExtRequest::new(
        "grow/internal/reload_mcp_catalog",
        serde_json::value::to_raw_value(&json!({"projectRoot": null}))
            .unwrap()
            .into(),
    );
    let _: acp::ExtResponse = acp_send(reload, &worker.tx).await.unwrap();
    server.release_initialization();
    wait_for_ready_server(&worker.tx, &sid, "pending-policy").await;
    wait_enabled(&worker.tx, &sid, "pending-policy", false).await;
    let before = server.wire_call_count();
    assert_call_fails_without_dispatch(&worker.tx, &sid, "pending-policy").await;
    assert_eq!(server.wire_call_count(), before);
    write_global_mcp(&latest);
    wait_enabled(&worker.tx, &sid, "pending-policy", true).await;
    server.behavior.call_delay_ms.store(1250, Ordering::SeqCst);
    assert!(
        call_echo_result(&worker.tx, &sid, "pending-policy", "updated-after-init")
            .await
            .is_ok()
    );
    assert_eq!(server.initialize_count(), 1);
    stop_worker(worker).await;

    write_global_mcp(&format!(
        "{latest}\n[disabled_mcp_tools]\npending-policy = [\"echo\"]\n"
    ));
    let worker = start_worker().await;
    initialize(&worker.tx).await;
    load_session(&worker.tx, &sid, cwd.path(), vec![]).await;
    wait_for_ready_server(&worker.tx, &sid, "pending-policy").await;
    wait_enabled(&worker.tx, &sid, "pending-policy", false).await;
    let before = server.wire_call_count();
    assert_call_fails_without_dispatch(&worker.tx, &sid, "pending-policy").await;
    assert_eq!(server.wire_call_count(), before);
    write_global_mcp(&latest);
    wait_enabled(&worker.tx, &sid, "pending-policy", true).await;
    assert!(
        call_echo_result(&worker.tx, &sid, "pending-policy", "cold-policy")
            .await
            .is_ok()
    );
    stop_worker(worker).await;
}

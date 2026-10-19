use super::{
    build_minimal_agent_for_tests, make_test_handle_with_receiver, run_local_for_bridge_test,
};
use crate::session::SessionCommand;
use acp_transport::protocol as acp;

fn test_http_server(name: String, host: &str) -> acp::McpServer {
    acp::McpServer::Http(
        acp::McpServerHttp::new(name, format!("http://{host}.invalid/mcp")).headers(vec![]),
    )
}

#[path = "mcp_server_validation.rs"]
mod mcp_server_validation;

fn server_name(server: &acp::McpServer) -> &str {
    match server {
        acp::McpServer::Http(acp::McpServerHttp { name, .. })
        | acp::McpServer::Sse(acp::McpServerSse { name, .. })
        | acp::McpServer::Stdio(acp::McpServerStdio { name, .. }) => name,
        _ => "",
    }
}

#[test]
fn accepted_client_mcp_intent_is_visible_to_concurrent_catalog_reload() {
    run_local_for_bridge_test(|| async {
        for replacement in [None, Some("new-client")] {
            let cwd = tempfile::tempdir().unwrap();
            let agent = build_minimal_agent_for_tests();
            let session_id = acp::SessionId::new(format!("mcp-catalog-{}", uuid::Uuid::new_v4()));
            let old = test_http_server(
                format!("old-{}", uuid::Uuid::new_v4()),
                &format!("old-{}", uuid::Uuid::new_v4()),
            );
            let new = replacement.map(|host| {
                test_http_server(
                    format!("new-{}", uuid::Uuid::new_v4()),
                    &format!("{host}-{}", uuid::Uuid::new_v4()),
                )
            });
            let (mut handle, mut commands) = make_test_handle_with_receiver("test/default", None);
            handle.info.id = session_id.clone();
            handle.info.cwd = cwd.path().to_string_lossy().into_owned();
            handle.initial_client_mcp_servers = vec![old.clone()];
            handle.mcp_servers = vec![old.clone()];
            agent
                .sessions
                .borrow_mut()
                .insert(session_id.clone(), handle);

            let client_intent = new.clone().into_iter().collect::<Vec<_>>();
            let update_request = acp::ExtRequest::new(
                "grow/session/update_mcp_servers",
                serde_json::value::to_raw_value(&serde_json::json!({
                    "sessionId": session_id.clone(),
                    "mcpServers": client_intent,
                }))
                .unwrap()
                .into(),
            );
            let update = crate::extensions::session_admin::handle(&agent, &update_request);
            tokio::pin!(update);

            let first = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                tokio::select! {
                    result = &mut update => panic!("update returned before actor command: {result:?}"),
                    command = commands.recv() => command.expect("first MCP update command"),
                }
            })
            .await
            .expect("first update command must be enqueued");
            let first_respond_to = match first {
                SessionCommand::UpdateMcpServers {
                    mcp_servers,
                    respond_to,
                } => {
                    assert!(
                        mcp_servers
                            .iter()
                            .all(|server| server_name(server) != server_name(&old)),
                        "the first command must apply the new client intent"
                    );
                    if let Some(new) = &new {
                        assert!(
                            mcp_servers
                                .iter()
                                .any(|server| server_name(server) == server_name(new)),
                            "the first command must include the replacement server"
                        );
                    }
                    respond_to
                }
                _ => panic!("unexpected first command"),
            };

            let reload_request = acp::ExtRequest::new(
                "grow/internal/reload_mcp_catalog",
                serde_json::value::to_raw_value(&serde_json::json!({ "projectRoot": null }))
                    .unwrap()
                    .into(),
            );
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                crate::extensions::session_admin::handle(&agent, &reload_request),
            )
            .await
            .expect("catalog reload must enqueue promptly")
            .expect("catalog reload should succeed");

            let second = tokio::time::timeout(std::time::Duration::from_secs(5), commands.recv())
                .await
                .expect("reload command must be enqueued")
                .expect("second MCP update command");
            let second_respond_to = match second {
                SessionCommand::UpdateMcpServers {
                    mcp_servers,
                    respond_to,
                } => {
                    assert!(
                        mcp_servers
                            .iter()
                            .all(|server| server_name(server) != server_name(&old)),
                        "disk reload must not revive the superseded client server"
                    );
                    if let Some(new) = &new {
                        assert!(
                            mcp_servers
                                .iter()
                                .any(|server| server_name(server) == server_name(new)),
                            "disk reload must include the latest client server"
                        );
                    }
                    respond_to
                }
                _ => panic!("unexpected reload command"),
            };

            {
                let sessions = agent.sessions.borrow();
                let handle = sessions
                    .get(&session_id)
                    .expect("test session remains live");
                assert_eq!(
                    handle
                        .initial_client_mcp_servers
                        .iter()
                        .map(server_name)
                        .collect::<Vec<_>>(),
                    new.iter().map(server_name).collect::<Vec<_>>(),
                    "client source must be published before update initialization is acknowledged"
                );
            }

            first_respond_to
                .send(Ok(()))
                .expect("first update response receiver");
            tokio::time::timeout(std::time::Duration::from_secs(5), &mut update)
                .await
                .expect("update handler must finish after acknowledgement")
                .expect("update handler should succeed");
            // Disk reload is fire-and-forget; its response receiver is dropped.
            let _ = second_respond_to.send(Ok(()));

            drop(commands);
        }
    });
}

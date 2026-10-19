use super::super::{
    build_minimal_agent_for_tests, make_test_handle_with_receiver, run_local_for_bridge_test,
};
use acp_transport::protocol as acp;

#[test]
fn duplicate_update_mcp_servers_is_rejected_without_mutating_session() {
    run_local_for_bridge_test(|| async {
        let cwd = tempfile::tempdir().unwrap();
        let agent = build_minimal_agent_for_tests();
        let session_id = acp::SessionId::new(format!("mcp-duplicate-{}", uuid::Uuid::new_v4()));
        let old = acp::McpServer::Http(
            acp::McpServerHttp::new("old".to_owned(), "https://old.example/mcp".to_owned())
                .headers(vec![]),
        );
        let duplicate_servers = vec![
            acp::McpServer::Http(
                acp::McpServerHttp::new(
                    "shared".to_owned(),
                    "https://first.example/mcp".to_owned(),
                )
                .headers(vec![]),
            ),
            acp::McpServer::Http(
                acp::McpServerHttp::new(
                    "shared".to_owned(),
                    "https://second.example/mcp".to_owned(),
                )
                .headers(vec![]),
            ),
        ];
        let (mut handle, mut commands) = make_test_handle_with_receiver("test/default", None);
        handle.info.id = session_id.clone();
        handle.info.cwd = cwd.path().to_string_lossy().into_owned();
        handle.initial_client_mcp_servers = vec![old.clone()];
        handle.mcp_servers = vec![old.clone()];
        agent
            .sessions
            .borrow_mut()
            .insert(session_id.clone(), handle);

        let request = acp::ExtRequest::new(
            "grow/session/update_mcp_servers",
            serde_json::value::to_raw_value(&serde_json::json!({
                "sessionId": session_id.clone(),
                "mcpServers": duplicate_servers,
            }))
            .unwrap()
            .into(),
        );
        let error = crate::extensions::session_admin::handle(&agent, &request)
            .await
            .expect_err("duplicate client names must be invalid params");
        assert_eq!(error.code, acp::Error::invalid_params().code);

        {
            let sessions = agent.sessions.borrow();
            let handle = sessions.get(&session_id).expect("session remains present");
            assert_eq!(handle.initial_client_mcp_servers.len(), 1);
            assert_eq!(handle.mcp_servers.len(), 1);
            assert!(matches!(
                &handle.initial_client_mcp_servers[0],
                acp::McpServer::Http(acp::McpServerHttp { name, url, .. })
                    if name == "old" && url == "https://old.example/mcp"
            ));
        }
        assert!(matches!(
            commands.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    });
}

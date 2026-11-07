use super::super::{
    build_minimal_agent_for_tests, make_test_handle_with_receiver, run_local_for_bridge_test,
};
use acp_transport::AcpAgentHandler;
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

/// The SDK duplicate guard sits deliberately before `initialize` lifecycle tasks in
/// `new`/`load`, so these tests exercise the real trait ingress on an agent that was
/// never initialized: an uninitialized real `new`/`load` must still return the
/// duplicate-specific invalid_params. If the guard regresses, the calls fail with an
/// initialize error instead and the duplicate reason assertion below catches it.
/// No initialize/auth is needed here — initializing would actually spawn worktree/
/// session-cleanup tasks, bootstrap the search index, and start the coordinator.
/// Real `session/new` ingress: one client SDK array declaring the same parsed name
/// twice is rejected as invalid_params with a duplicate reason, before any session,
/// storage or catalog effect.
#[test]
fn new_session_with_duplicate_sdk_mcp_declarations_is_rejected_before_effects() {
    run_local_for_bridge_test(|| async {
        let cwd = tempfile::tempdir().unwrap();
        let agent = build_minimal_agent_for_tests();
        let session_id = acp::SessionId::new(format!("sdk-dup-new-{}", uuid::Uuid::new_v4()));
        let info = crate::session::info::Info {
            id: session_id.clone(),
            cwd: cwd.path().to_string_lossy().into_owned(),
        };
        let session_dir = crate::session::persistence::session_dir(&info);

        let request = acp::NewSessionRequest::new(cwd.path().to_owned()).meta(
            serde_json::json!({
                "sessionId": session_id.0.as_ref(),
                "grow/mcp/servers": [
                    { "name": "harness", "serverId": "srv_0" },
                    { "name": "harness", "serverId": "srv_1" },
                ],
            })
            .as_object()
            .cloned(),
        );
        let error = agent
            .new_session(request)
            .await
            .expect_err("duplicate SDK declarations must be invalid params");
        assert_eq!(error.code, acp::Error::invalid_params().code);
        assert!(
            format!("{error:?}").contains("duplicate"),
            "unexpected error: {error:?}"
        );

        assert!(
            agent.sessions.borrow().get(&session_id).is_none(),
            "no session handle may be registered for a rejected duplicate array"
        );
        assert!(
            !session_dir.exists(),
            "no storage may be created for a rejected duplicate array"
        );
    });
}

/// Real `session/load` ingress: duplicate SDK declarations are rejected before load
/// admission, leaving the retained resident handle and its catalog untouched and its
/// command channel undrained.
#[test]
fn load_session_with_duplicate_sdk_mcp_declarations_preserves_resident_handle() {
    run_local_for_bridge_test(|| async {
        let cwd = tempfile::tempdir().unwrap();
        let agent = build_minimal_agent_for_tests();
        let session_id = acp::SessionId::new(format!("sdk-dup-load-{}", uuid::Uuid::new_v4()));
        let old = acp::McpServer::Http(
            acp::McpServerHttp::new("old".to_owned(), "https://old.example/mcp".to_owned())
                .headers(vec![]),
        );
        let (mut handle, mut commands) = make_test_handle_with_receiver("test/default", None);
        handle.info.id = session_id.clone();
        handle.info.cwd = cwd.path().to_string_lossy().into_owned();
        handle.initial_client_mcp_servers = vec![old.clone()];
        handle.mcp_servers = vec![old.clone()];
        agent
            .sessions
            .borrow_mut()
            .insert(session_id.clone(), handle);

        let request = acp::LoadSessionRequest::new(session_id.clone(), cwd.path().to_owned()).meta(
            serde_json::json!({
                "grow/mcp/servers": [
                    { "name": "harness", "serverId": "srv_0" },
                    { "name": "harness", "serverId": "srv_1" },
                ],
            })
            .as_object()
            .cloned(),
        );
        let error = agent
            .load_session(request)
            .await
            .expect_err("duplicate SDK declarations must be invalid params");
        assert_eq!(error.code, acp::Error::invalid_params().code);
        assert!(
            format!("{error:?}").contains("duplicate"),
            "unexpected error: {error:?}"
        );

        {
            let sessions = agent.sessions.borrow();
            let handle = sessions
                .get(&session_id)
                .expect("resident handle must be preserved on load rejection");
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

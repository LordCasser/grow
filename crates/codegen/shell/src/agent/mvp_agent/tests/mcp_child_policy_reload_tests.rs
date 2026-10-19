use super::*;

#[tokio::test(flavor = "current_thread")]
async fn internal_reload_fans_out_policy_only_command_to_matching_child() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let agent = build_minimal_agent_for_tests();
            let root = tempfile::tempdir().unwrap();
            let project = root.path().join("project");
            let outside = root.path().join("outside");
            std::fs::create_dir_all(&project).unwrap();
            std::fs::create_dir_all(&outside).unwrap();

            let matching_id = acp::SessionId::new("matching-workflow-child");
            let unrelated_id = acp::SessionId::new("unrelated-child");
            let (mut matching, mut matching_commands) =
                make_test_handle_with_receiver("model", None);
            matching.info.id = matching_id.clone();
            matching.info.cwd = project.to_string_lossy().into_owned();
            matching.initial_client_mcp_servers = vec![];
            let (mut unrelated, mut unrelated_commands) =
                make_test_handle_with_receiver("model", None);
            unrelated.info.id = unrelated_id.clone();
            unrelated.info.cwd = outside.to_string_lossy().into_owned();
            agent
                .active_child_sessions
                .borrow_mut()
                .insert(matching_id, matching);
            agent
                .active_child_sessions
                .borrow_mut()
                .insert(unrelated_id, unrelated);

            let params = serde_json::json!({ "projectRoot": project });
            let raw = serde_json::value::to_raw_value(&params).unwrap();
            let request = acp::ExtRequest::new(
                "grow/internal/reload_mcp_catalog",
                std::sync::Arc::from(raw),
            );
            let response =
                crate::extensions::session_admin::handle_reload_mcp_catalog(&agent, &request).await;
            assert!(response.is_ok(), "reload failed: {response:?}");

            let command = matching_commands
                .recv()
                .await
                .expect("child refresh command");
            let crate::session::SessionCommand::UpdateMcpServers { mcp_servers, .. } = command
            else {
                panic!("expected child policy refresh command");
            };
            assert!(
                mcp_servers.is_empty(),
                "child must not receive a replacement catalog"
            );
            assert!(
                unrelated_commands.try_recv().is_err(),
                "out-of-scope child was refreshed"
            );
        })
        .await;
}

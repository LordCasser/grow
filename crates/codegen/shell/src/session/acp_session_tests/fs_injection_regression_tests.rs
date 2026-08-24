use tools::computer::local::{LocalTerminalBackend, MockFs};
use tools::computer::types::{AsyncFileSystem, TerminalBackend};
use tools::notification::ToolNotificationHandle;
use tools::registry::types::{SessionContext, ToolConfig, ToolServerConfig};

/// A ToolBridge built with a custom FileSystem must route writes through it.
#[tokio::test]
async fn tool_bridge_routes_writes_through_injected_fs() {
    let cwd = std::path::PathBuf::from("/tmp/fs-injection-test-nonexistent");
    let file_path = cwd.join("new.txt");

    let mock_fs = std::sync::Arc::new(MockFs::new());
    let fs: std::sync::Arc<dyn AsyncFileSystem> = mock_fs.clone();
    let terminal: std::sync::Arc<dyn TerminalBackend> =
        std::sync::Arc::new(LocalTerminalBackend::new());

    let builder = tools::bridge::ToolBridge::get_builder();
    let config = ToolServerConfig {
        tools: vec![
            ToolConfig {
                id: "Grow:read_file".into(),
                params: None,
                name_override: None,
                params_name_overrides: None,
                description_override: None,
                kind: None,
            },
            ToolConfig {
                id: "Grow:search_replace".into(),
                params: None,
                name_override: None,
                params_name_overrides: None,
                description_override: None,
                kind: None,
            },
        ],
    };
    let ctx = SessionContext {
        backend: terminal,
        fs,
        cwd: cwd.clone(),
        session_folder: std::env::temp_dir().join("grow-test-fs"),
        session_env: std::sync::Arc::new(std::collections::HashMap::new()),
        notification_handle: ToolNotificationHandle::noop(),
        owner_session_id: None,
        subagent: None,
        parent_scheduler_handle: None,
        skills: vec![],
        resources_persistence: std::sync::Arc::new(
            tools::persistence::ResourcesPersistence::noop(),
        ),
        memory_backend: None,
        web_fetch_config: Default::default(),
        lsp: None,
        app_builder_deployer_config: Default::default(),
        system_reminder_tag: tools::reminders::DEFAULT_REMINDER_TAG,
    };
    let bridge = tools::bridge::ToolBridge::finalize_builder(builder, config, ctx)
        .await
        .expect("finalize_builder should succeed");

    // Create a new file via search_replace (old_string="" = new file).
    let result = bridge
        .call(
            "search_replace",
            serde_json::json!({
                "file_path": file_path.to_string_lossy(),
                "old_string": "",
                "new_string": "hello from ACP\n",
            }),
            "test-call-1",
        )
        .await;
    assert!(
        result.is_ok(),
        "search_replace should succeed: {:?}",
        result.err()
    );

    // The write must have landed in MockFs, not on real disk.
    let written = mock_fs
        .get_file(&file_path)
        .await
        .expect("Write went to disk instead of injected FileSystem");
    assert_eq!(String::from_utf8(written).unwrap(), "hello from ACP\n");
}

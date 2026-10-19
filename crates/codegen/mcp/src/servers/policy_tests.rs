use super::*;

#[test]
fn refreshed_policy_replaces_and_clears_overrides_with_meta_precedence() {
    let initial = McpClientTimeoutOverrides {
        startup_timeout_sec: Some(8),
        tool_timeout_sec: Some(9),
        tool_timeouts: Some(HashMap::from([("one".to_owned(), 10)])),
        expose_image_base64: Some(true),
    };
    let client = McpClient::new_acp(
        "policy".into(),
        "server".into(),
        Arc::new(PolicyTestInvoker::default()),
        Some(&initial),
        None,
    );
    assert_eq!(client.startup_timeout_sec(), 8);
    assert_eq!(client.tool_timeout_for("one"), 10);
    assert!(client.expose_image_base64());

    let updated = McpClientTimeoutOverrides {
        startup_timeout_sec: Some(4),
        tool_timeout_sec: Some(5),
        tool_timeouts: Some(HashMap::from([("one".to_owned(), 6)])),
        expose_image_base64: Some(false),
    };
    let meta = McpServerMetaConfig {
        startup_timeout_ms: Some(1_001),
        tool_timeout_ms: Some(0),
        tool_timeouts_ms: Some(HashMap::from([
            ("one".to_owned(), 1_001),
            ("two".to_owned(), 1),
        ])),
        expose_image_base64: Some(true),
    };
    client.refresh_policy(Some(&updated), Some(&meta));
    assert_eq!(client.startup_timeout_sec(), 2);
    assert_eq!(client.tool_timeout_sec(), 0);
    assert_eq!(client.tool_timeout_for("one"), 2);
    assert_eq!(client.tool_timeout_for("two"), 1);
    assert!(client.expose_image_base64());

    let replacement = McpClientTimeoutOverrides {
        startup_timeout_sec: Some(3),
        tool_timeout_sec: Some(7),
        tool_timeouts: Some(HashMap::from([("one".to_owned(), 11)])),
        expose_image_base64: Some(false),
    };
    client.refresh_policy(Some(&replacement), None);
    assert_eq!(client.startup_timeout_sec(), 3);
    assert_eq!(client.tool_timeout_sec(), 7);
    assert_eq!(client.tool_timeout_for("one"), 11);
    assert_eq!(client.tool_timeout_for("two"), 7);
    assert!(!client.expose_image_base64());

    client.refresh_policy(None, None);
    assert_eq!(client.startup_timeout_sec(), DEFAULT_STARTUP_TIMEOUT_SECS);
    assert_eq!(client.tool_timeout_sec(), DEFAULT_TOOL_TIMEOUT_SECS);
    assert_eq!(client.tool_timeout_for("one"), DEFAULT_TOOL_TIMEOUT_SECS);
    assert!(!client.expose_image_base64());
}

#[derive(Default)]
struct PolicyTestInvoker {
    call_timeouts: std::sync::Mutex<Vec<std::time::Duration>>,
    call_started: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl crate::acp_transport::AcpReverseInvoker for PolicyTestInvoker {
    async fn invoke(
        &self,
        _server_id: &str,
        message: serde_json::Value,
        timeout: std::time::Duration,
    ) -> Result<serde_json::Value, String> {
        let id = message
            .get("id")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let result = match message.get("method").and_then(|value| value.as_str()) {
            Some("initialize") => serde_json::json!({
                "protocolVersion": message["params"]["protocolVersion"],
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "policy-test", "version": "1"}
            }),
            Some("tools/list") => serde_json::json!({
                "tools": [{
                    "name": "work",
                    "description": "delayed test tool",
                    "inputSchema": {"type": "object"}
                }]
            }),
            Some("tools/call") => {
                self.call_timeouts.lock().unwrap().push(timeout);
                self.call_started.notify_one();
                tokio::time::sleep(std::time::Duration::from_millis(1_300)).await;
                serde_json::json!({"content": [{"type": "text", "text": "done"}]})
            }
            other => return Err(format!("unexpected MCP method: {other:?}")),
        };
        Ok(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }
}

fn policy_test_tool(client: &Arc<McpClient>) -> McpErasedTool {
    let mut state = McpState::new(vec![]);
    state
        .owned_clients
        .insert(client.server_name().to_owned(), Arc::clone(client));
    McpErasedTool {
        tool: McpTool::new(
            "work".to_owned(),
            "delayed test tool".to_owned(),
            client.server_name().to_owned(),
            Arc::new(Mutex::new(state)),
            serde_json::json!({"type": "object"}),
            None,
        ),
    }
}

#[tokio::test]
async fn admitted_call_keeps_deadline_and_reverse_backstop_never_shrinks() {
    let initial = McpClientTimeoutOverrides {
        startup_timeout_sec: Some(1),
        tool_timeout_sec: Some(3),
        ..Default::default()
    };
    let invoker = Arc::new(PolicyTestInvoker::default());
    let client = Arc::new(McpClient::new_acp(
        "policy".into(),
        "server".into(),
        invoker.clone(),
        Some(&initial),
        None,
    ));
    let tool = policy_test_tool(&client);

    let first_tool = tool;
    let first_client = Arc::clone(&client);
    let first = tokio::spawn(async move {
        first_tool
            .try_call_tool(
                &first_client,
                &serde_json::json!({}),
                &mut false,
                &mut false,
            )
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        invoker.call_started.notified(),
    )
    .await
    .expect("first tool call did not start");
    client.refresh_policy(
        Some(&McpClientTimeoutOverrides {
            startup_timeout_sec: Some(1),
            tool_timeout_sec: Some(1),
            ..Default::default()
        }),
        None,
    );
    assert!(
        first.await.unwrap().is_ok(),
        "admitted 3s call should survive policy reduction"
    );

    let second_tool = policy_test_tool(&client);
    let second = second_tool
        .try_call_tool(&client, &serde_json::json!({}), &mut false, &mut false)
        .await;
    assert!(
        second.is_err(),
        "a new call should use the updated 1s deadline"
    );

    client.refresh_policy(
        Some(&McpClientTimeoutOverrides {
            startup_timeout_sec: Some(1),
            tool_timeout_sec: Some(4),
            ..Default::default()
        }),
        None,
    );
    let third_tool = policy_test_tool(&client);
    assert!(
        third_tool
            .try_call_tool(&client, &serde_json::json!({}), &mut false, &mut false,)
            .await
            .is_ok(),
        "a call after extending policy should complete"
    );

    let backstops = invoker.call_timeouts.lock().unwrap();
    assert_eq!(backstops.len(), 3);
    assert!(
        backstops
            .iter()
            .all(|timeout| *timeout >= std::time::Duration::from_secs(3))
    );
    assert!(backstops[2] >= std::time::Duration::from_secs(4));
}

#[derive(Default)]
struct BlockedInitializePolicyInvoker {
    initialize_started: tokio::sync::Notify,
    release_initialize: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl crate::acp_transport::AcpReverseInvoker for BlockedInitializePolicyInvoker {
    async fn invoke(
        &self,
        _server_id: &str,
        message: serde_json::Value,
        _timeout: std::time::Duration,
    ) -> Result<serde_json::Value, String> {
        let id = message
            .get("id")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let result = match message.get("method").and_then(|value| value.as_str()) {
            Some("initialize") => {
                self.initialize_started.notify_one();
                self.release_initialize.notified().await;
                serde_json::json!({
                    "protocolVersion": message["params"]["protocolVersion"],
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "blocked-policy-test", "version": "1"}
                })
            }
            Some("tools/list") => serde_json::json!({
                "tools": [{"name": "work", "description": "work", "inputSchema": {"type": "object"}}]
            }),
            Some("tools/call") => {
                tokio::time::sleep(std::time::Duration::from_millis(1_200)).await;
                serde_json::json!({"content": [{"type": "text", "text": "done"}]})
            }
            other => return Err(format!("unexpected MCP method: {other:?}")),
        };
        Ok(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }
}

#[tokio::test]
async fn admitted_tool_deadline_is_captured_before_initialization_wait() {
    let initial = McpClientTimeoutOverrides {
        startup_timeout_sec: Some(8),
        tool_timeout_sec: Some(3),
        ..Default::default()
    };
    let invoker = Arc::new(BlockedInitializePolicyInvoker::default());
    let client = Arc::new(McpClient::new_acp(
        "policy".into(),
        "server".into(),
        invoker.clone(),
        Some(&initial),
        None,
    ));
    let tool = policy_test_tool(&client);
    let call_client = Arc::clone(&client);
    let call = tokio::spawn(async move {
        tool.try_call_tool(&call_client, &serde_json::json!({}), &mut false, &mut false)
            .await
    });

    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        invoker.initialize_started.notified(),
    )
    .await
    .expect("initialization did not block as expected");
    client.refresh_policy(
        Some(&McpClientTimeoutOverrides {
            startup_timeout_sec: Some(8),
            tool_timeout_sec: Some(1),
            ..Default::default()
        }),
        None,
    );
    invoker.release_initialize.notify_one();

    let result = tokio::time::timeout(std::time::Duration::from_secs(4), call)
        .await
        .expect("admitted call did not settle")
        .unwrap();
    assert!(
        result.is_ok(),
        "the call admitted with a 3s timeout should survive a policy update to 1s during initialization: {result:?}"
    );
}

#[tokio::test]
async fn refreshed_startup_timeout_bounds_the_next_handshake() {
    let initial = McpClientTimeoutOverrides {
        startup_timeout_sec: Some(20),
        ..Default::default()
    };
    let client = McpClient::new_acp(
        "startup-policy".into(),
        "server".into(),
        Arc::new(BlockedInitializePolicyInvoker::default()),
        Some(&initial),
        None,
    );
    client.refresh_policy(
        Some(&McpClientTimeoutOverrides {
            startup_timeout_sec: Some(0),
            ..Default::default()
        }),
        None,
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        client.ensure_initialized(),
    )
    .await
    .expect("refreshed startup bound was ignored");
    assert!(
        result.is_err(),
        "blocked handshake must honor the refreshed zero bound"
    );
}

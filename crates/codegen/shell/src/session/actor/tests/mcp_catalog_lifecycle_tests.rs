use super::support::*;
use super::*;
use crate::session::mcp_servers::McpClient;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
struct CatalogServer {
    tools: Mutex<Vec<String>>,
    calls: AtomicUsize,
    block_list: AtomicBool,
    fail_list: AtomicBool,
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl ::mcp::acp_transport::AcpReverseInvoker for CatalogServer {
    async fn invoke(
        &self,
        _: &str,
        message: Value,
        _: std::time::Duration,
    ) -> Result<Value, String> {
        let result = match message["method"].as_str().unwrap_or_default() {
            "initialize" => json!({
                "protocolVersion": message["params"]["protocolVersion"],
                "capabilities": {"tools": {}},
                "serverInfo": {"name":"catalog-fixture", "version":"1"}
            }),
            "tools/list" => {
                if self.fail_list.swap(false, Ordering::SeqCst) {
                    return Err("catalog temporarily unavailable".into());
                }
                let tools = self.tools.lock().unwrap().clone();
                if self.block_list.swap(false, Ordering::SeqCst) {
                    self.started.notify_one();
                    self.release.notified().await;
                }
                json!({"tools": tools.iter().map(|name| {
                    let mut input_schema = json!({
                        "type":"object",
                        "properties":{"value":{"type":"string"}},
                        "required":["value"]
                    });
                    input_schema["properties"][name] = json!({"type":"string"});
                    json!({
                        "name":name,
                        "description":format!("catalog {name}"),
                        "inputSchema":input_schema
                    })
                }).collect::<Vec<_>>()})
            }
            "tools/call" => {
                let name = message["params"]["name"].as_str().unwrap_or_default();
                if !self.tools.lock().unwrap().iter().any(|tool| tool == name) {
                    return Err(format!("retired tool {name}"));
                }
                self.calls.fetch_add(1, Ordering::SeqCst);
                json!({"content":[{"type":"text", "text":format!("{name}:{}",message["params"]["arguments"]["value"].as_str().unwrap())}], "isError":false})
            }
            _ => return Err("unexpected request".into()),
        };
        Ok(json!({"jsonrpc":"2.0", "id":message["id"], "result":result}))
    }
}

async fn fixture() -> SessionActor {
    let (gateway, _notifications) = tokio::sync::mpsc::unbounded_channel();
    let (persistence, _records) = tokio::sync::mpsc::unbounded_channel();
    create_test_actor(0, 128000, 80, gateway, persistence).await
}

async fn install(
    actor: &SessionActor,
    server: &Arc<CatalogServer>,
) -> (Arc<McpClient>, ::mcp::servers::McpClientEpisode) {
    let client = Arc::new(McpClient::new_acp(
        "demo".into(),
        "fixture".into(),
        server.clone(),
        None,
        None,
    ));
    client.ensure_initialized().await.unwrap();
    let mut state = actor.mcp_state.lock().await;
    let episode = state.bind_client_events(&client);
    state.owned_clients.insert("demo".into(), client.clone());
    (client, episode)
}

async fn listed(actor: &SessionActor) -> Vec<String> {
    let bridge_schemas: std::collections::BTreeMap<_, _> = actor
        .agent
        .borrow()
        .tool_bridge()
        .tool_definitions()
        .await
        .into_iter()
        .filter_map(|entry| {
            let name = entry.function.name.strip_prefix("demo__")?.to_owned();
            let schema = entry.function.parameters;
            Some((name, schema))
        })
        .collect();
    let snapshot_schemas: std::collections::BTreeMap<_, _> = actor
        .mcp
        .tool_metadata_snapshot
        .lock()
        .unwrap()
        .tools
        .iter()
        .filter(|entry| entry.server_name == "demo")
        .map(|entry| (entry.tool_name.clone(), entry.input_schema.clone()))
        .collect();
    assert_eq!(
        bridge_schemas, snapshot_schemas,
        "ToolBridge and model search snapshot must agree on full input schemas"
    );
    bridge_schemas.into_keys().collect()
}

fn catalog_schema(name: &str) -> Value {
    let mut schema = json!({
        "type":"object",
        "properties":{"value":{"type":"string"}},
        "required":["value"]
    });
    schema["properties"][name] = json!({"type":"string"});
    schema
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_catalog_recovery_replaces_tools_and_empty_catalog_removes_them() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = fixture().await;
            let server = Arc::new(CatalogServer::default());
            *server.tools.lock().unwrap() = vec!["old".into()];
            let (client, episode) = install(&actor, &server).await;
            actor.refresh_mcp_server_tools("demo", episode).await;
            assert_eq!(listed(&actor).await, ["old"]);
            let old_schema = catalog_schema("old");
            let old_bridge_schema = actor
                .agent
                .borrow()
                .tool_bridge()
                .tool_definitions()
                .await
                .into_iter()
                .find(|entry| entry.function.name == "demo__old")
                .unwrap()
                .function
                .parameters;
            assert_eq!(old_bridge_schema, old_schema);
            *server.tools.lock().unwrap() = vec!["replacement".into()];
            client.recover().await.unwrap();
            let episode = actor.mcp_state.lock().await.bind_client_events(&client);
            actor.refresh_mcp_server_tools("demo", episode).await;
            assert_eq!(listed(&actor).await, ["replacement"]);
            let replacement_bridge_schema = actor
                .agent
                .borrow()
                .tool_bridge()
                .tool_definitions()
                .await
                .into_iter()
                .find(|entry| entry.function.name == "demo__replacement")
                .unwrap()
                .function
                .parameters;
            assert_eq!(replacement_bridge_schema, catalog_schema("replacement"));
            assert_ne!(replacement_bridge_schema, old_schema);
            let snapshot = actor.mcp.tool_metadata_snapshot.lock().unwrap();
            let replacement_snapshot = snapshot
                .tools
                .iter()
                .find(|entry| entry.server_name == "demo" && entry.tool_name == "replacement")
                .unwrap();
            assert_eq!(
                replacement_snapshot.input_schema,
                catalog_schema("replacement")
            );
            assert!(!snapshot.tools.iter().any(|entry| {
                entry.server_name == "demo"
                    && entry.tool_name == "old"
                    && entry.input_schema == old_schema
            }));
            drop(snapshot);
            let bridge = actor.agent.borrow().tool_bridge().clone();
            assert!(
                bridge
                    .call("demo__old", json!({"value":"old"}), "old-call")
                    .await
                    .is_err()
            );
            let result = bridge
                .call("demo__replacement", json!({"value":"new"}), "new-call")
                .await
                .unwrap();
            assert!(result.prompt_text.contains("replacement:new"));
            assert_eq!(server.calls.load(Ordering::SeqCst), 1);
            server.tools.lock().unwrap().clear();
            actor.refresh_mcp_server_tools("demo", episode).await;
            assert!(listed(&actor).await.is_empty());
            assert!(
                bridge
                    .call(
                        "demo__replacement",
                        json!({"value":"retired"}),
                        "empty-call"
                    )
                    .await
                    .is_err()
            );
            assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_catalog_late_discovery_cannot_replace_new_client_tools() {
    tokio::task::LocalSet::new().run_until(async {
        let actor = fixture().await;
        let old = Arc::new(CatalogServer::default());
        *old.tools.lock().unwrap() = vec!["old".into()];
        let (_, old_episode) = install(&actor, &old).await;
        old.block_list.store(true, Ordering::SeqCst);
        let discovery = actor.refresh_mcp_server_tools("demo", old_episode);
        tokio::pin!(discovery);
        tokio::select! {
            _ = old.started.notified() => {},
            _ = &mut discovery => panic!("discovery must wait for release"),
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => panic!("discovery did not start"),
        }
        let new = Arc::new(CatalogServer::default());
        *new.tools.lock().unwrap() = vec!["current".into()];
        let (_, new_episode) = install(&actor, &new).await;
        actor.refresh_mcp_server_tools("demo", new_episode).await;
        old.release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(5), discovery).await.unwrap();
        actor.refresh_mcp_server_tools("demo", old_episode).await;
        assert_eq!(listed(&actor).await, ["current"]);
        assert_eq!(old.calls.load(Ordering::SeqCst), 0);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_catalog_retirement_prunes_snapshot_before_bridge_restore() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = fixture().await;
            let server = Arc::new(CatalogServer::default());
            *server.tools.lock().unwrap() = vec!["retired".into()];
            let (_, episode) = install(&actor, &server).await;
            actor.refresh_mcp_server_tools("demo", episode).await;
            actor.mcp_state.lock().await.owned_clients.remove("demo");
            actor.unregister_server_tools("demo");
            assert!(listed(&actor).await.is_empty());
            let publication = actor.mcp.publication_bridge.lock().await;
            actor.restore_mcp_tools_from_snapshot(&publication).await;
            drop(publication);
            assert!(listed(&actor).await.is_empty());
            assert_eq!(server.calls.load(Ordering::SeqCst), 0);
        })
        .await;
}

// grow_home() is process-cached; configure the home before the child starts.
fn run_isolated_policy_test(name: &str) -> bool {
    const MARKER: &str = "GROW_MCP_POLICY_TEST_CHILD";
    if std::env::var_os(MARKER).is_some() {
        return false;
    }
    let name = name.strip_prefix("shell::").unwrap_or(name);
    let home = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("GROW_HOME", home.path())
        .env(MARKER, "1")
        .output()
        .expect("run isolated MCP policy test");
    assert!(
        output.status.success(),
        "isolated {name} failed\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn live_auxiliary_policy_refresh_obeys_trust_and_reconciles_disabled_tools() {
    if run_isolated_policy_test(concat!(
        module_path!(),
        "::live_auxiliary_policy_refresh_obeys_trust_and_reconciles_disabled_tools"
    )) {
        return;
    }
    tokio::task::LocalSet::new().run_until(async {
        let home = crate::util::grow_home::grow_home();
        let global_config = home.join("config.toml");
        std::fs::write(
            &global_config,
            r#"
[mcp_servers.demo]
url = "http://127.0.0.1:1/unused"
startup_timeout_sec = 8
tool_timeout_sec = 40
tool_timeouts = { work = 30 }

[disabled_mcp_tools]
demo = ["work"]
"#,
        )
        .unwrap();

        fn project() -> tempfile::TempDir {
            let directory = tempfile::tempdir().unwrap();
            git2::Repository::init(directory.path()).unwrap();
            std::fs::create_dir_all(directory.path().join(".grow")).unwrap();
            directory
        }
        fn write_project_policy(directory: &std::path::Path, disabled: &str) {
            std::fs::write(
                directory.join(".grow/config.toml"),
                format!(
                    "[mcp_servers.demo]\nurl = \"http://127.0.0.1:1/unused\"\nstartup_timeout_sec = 2\ntool_timeout_sec = 5\ntool_timeouts = {{ work = 3 }}\n\n[disabled_mcp_tools]\ndemo = {disabled}\n"
                ),
            )
            .unwrap();
        }

        let trusted = project();
        let untrusted = project();
        write_project_policy(trusted.path(), "[]");
        write_project_policy(untrusted.path(), "[]");
        crate::agent::folder_trust::record_for_test(trusted.path(), true);
        crate::agent::folder_trust::record_for_test(untrusted.path(), false);

        let mut trusted_actor = fixture().await;
        trusted_actor.session_info.cwd = trusted.path().display().to_string();
        let mut untrusted_actor = fixture().await;
        untrusted_actor.session_info.cwd = untrusted.path().display().to_string();

        let trusted_server = Arc::new(CatalogServer::default());
        *trusted_server.tools.lock().unwrap() = vec!["work".into()];
        let (trusted_client, trusted_episode) = install(&trusted_actor, &trusted_server).await;
        trusted_actor
            .refresh_mcp_server_tools("demo", trusted_episode)
            .await;

        let untrusted_server = Arc::new(CatalogServer::default());
        *untrusted_server.tools.lock().unwrap() = vec!["work".into()];
        let (untrusted_client, untrusted_episode) =
            install(&untrusted_actor, &untrusted_server).await;
        untrusted_actor
            .refresh_mcp_server_tools("demo", untrusted_episode)
            .await;

        trusted_actor.refresh_mcp_auxiliary_policy().await;
        untrusted_actor.refresh_mcp_auxiliary_policy().await;
        assert_eq!(trusted_client.startup_timeout_sec(), 2);
        assert_eq!(trusted_client.tool_timeout_sec(), 5);
        assert_eq!(trusted_client.tool_timeout_for("work"), 3);
        assert_eq!(untrusted_client.startup_timeout_sec(), 8);
        assert_eq!(untrusted_client.tool_timeout_sec(), 40);
        assert_eq!(untrusted_client.tool_timeout_for("work"), 30);
        assert_eq!(listed(&trusted_actor).await, ["work"]);
        assert!(listed(&untrusted_actor).await.is_empty());

        let untrusted_bridge = untrusted_actor.agent.borrow().tool_bridge().clone();
        assert!(untrusted_bridge
            .call("demo__work", json!({"value":"blocked"}), "blocked-bridge")
            .await
            .is_err());
        assert!(crate::extensions::mcp::call_mcp_tool(
            &untrusted_actor.mcp_state,
            "demo",
            None,
            "work",
            json!({"value":"blocked"}),
        )
        .await
        .is_err());
        assert_eq!(untrusted_server.calls.load(Ordering::SeqCst), 0);

        // A remote list refresh must not restore a registration that the live
        // policy has disabled; it must keep bridge and search snapshot aligned.
        untrusted_actor
            .refresh_mcp_server_tools("demo", untrusted_episode)
            .await;
        assert!(listed(&untrusted_actor).await.is_empty());
        assert_eq!(untrusted_server.calls.load(Ordering::SeqCst), 0);

        let trusted_bridge = trusted_actor.agent.borrow().tool_bridge().clone();
        trusted_bridge
            .call("demo__work", json!({"value":"enabled"}), "enabled-bridge")
            .await
            .unwrap();
        crate::extensions::mcp::call_mcp_tool(
            &trusted_actor.mcp_state,
            "demo",
            None,
            "work",
            json!({"value":"enabled"}),
        )
        .await
        .unwrap();
        assert_eq!(trusted_server.calls.load(Ordering::SeqCst), 2);

        // Removing the trusted project's empty override exposes the global
        // disabled mask and global timeout values again.
        std::fs::remove_file(trusted.path().join(".grow/config.toml")).unwrap();
        trusted_actor.refresh_mcp_auxiliary_policy().await;
        assert_eq!(trusted_client.startup_timeout_sec(), 8);
        assert_eq!(trusted_client.tool_timeout_sec(), 40);
        assert_eq!(trusted_client.tool_timeout_for("work"), 30);
        assert!(listed(&trusted_actor).await.is_empty());
        assert!(trusted_bridge
            .call("demo__work", json!({"value":"blocked"}), "global-mask")
            .await
            .is_err());
        assert!(crate::extensions::mcp::call_mcp_tool(
            &trusted_actor.mcp_state,
            "demo",
            None,
            "work",
            json!({"value":"blocked"}),
        )
        .await
        .is_err());
        assert_eq!(trusted_server.calls.load(Ordering::SeqCst), 2);

        // Reintroducing the trusted empty array re-enables the existing tool.
        write_project_policy(trusted.path(), "[]");
        trusted_actor.refresh_mcp_auxiliary_policy().await;
        assert_eq!(listed(&trusted_actor).await, ["work"]);
        trusted_bridge
            .call("demo__work", json!({"value":"reenabled"}), "reenabled")
            .await
            .unwrap();
        assert_eq!(trusted_server.calls.load(Ordering::SeqCst), 3);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn subagent_auxiliary_refresh_updates_local_gate_without_mutating_shared_client() {
    if run_isolated_policy_test(concat!(
        module_path!(),
        "::subagent_auxiliary_refresh_updates_local_gate_without_mutating_shared_client"
    )) {
        return;
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            let home = crate::util::grow_home::grow_home();
            std::fs::write(
                home.join("config.toml"),
                r#"
[mcp_servers.demo]
url = "http://127.0.0.1:1/unused"
startup_timeout_sec = 2
tool_timeout_sec = 7

[disabled_mcp_tools]
demo = ["work"]
"#,
            )
            .unwrap();

            let mut actor = fixture().await;
            actor.session_info.cwd = home.display().to_string();
            actor.startup_hints.is_subagent = true;
            let server = Arc::new(CatalogServer::default());
            *server.tools.lock().unwrap() = vec!["work".into()];
            let (client, episode) = install(&actor, &server).await;
            let shared_startup_timeout = client.startup_timeout_sec();
            let shared_tool_timeout = client.tool_timeout_sec();
            {
                let mut state = actor.mcp_state.lock().await;
                state.owned_clients.remove("demo");
                state.shared_clients.insert("demo".into(), client.clone());
            }
            actor.refresh_mcp_server_tools("demo", episode).await;
            assert_eq!(listed(&actor).await, ["work"]);
            let existing_configs =
                serde_json::to_value(&actor.mcp_state.lock().await.configs).unwrap();

            crate::session::actor::run_loop::refresh_subagent_mcp_auxiliary_policy(
                &actor,
                vec![acp::McpServer::Http(
                    acp::McpServerHttp::new(
                        "replacement".to_owned(),
                        "http://127.0.0.1:2/replacement".to_owned(),
                    )
                    .headers(vec![]),
                )],
            )
            .await;

            assert!(listed(&actor).await.is_empty());
            assert_eq!(
                serde_json::to_value(&actor.mcp_state.lock().await.configs).unwrap(),
                existing_configs
            );
            assert_eq!(
                client.startup_timeout_sec(),
                shared_startup_timeout,
                "a subagent must not rewrite policy on its parent's shared client"
            );
            assert_eq!(
                client.tool_timeout_sec(),
                shared_tool_timeout,
                "a subagent must not rewrite policy on its parent's shared client"
            );
            assert!(
                crate::extensions::mcp::call_mcp_tool(
                    &actor.mcp_state,
                    "demo",
                    None,
                    "work",
                    json!({"value":"blocked"}),
                )
                .await
                .is_err()
            );
            assert_eq!(server.calls.load(Ordering::SeqCst), 0);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_initial_discovery_follows_agent_switch() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway, _notifications) = tokio::sync::mpsc::unbounded_channel();
            let (persistence, _records) = tokio::sync::mpsc::unbounded_channel();
            let actor = Arc::new(create_test_actor(0, 128000, 80, gateway, persistence).await);
            let server = Arc::new(CatalogServer::default());
            *server.tools.lock().unwrap() = vec!["echo".into()];
            server.block_list.store(true, Ordering::SeqCst);
            actor.mcp_state.lock().await.set_acp_servers(
                vec![::mcp::servers::AcpServerEntry {
                    name: "demo".into(),
                    server_id: "fixture".into(),
                }],
                server.clone(),
            );
            let old_bridge = actor.agent.borrow().tool_bridge().clone();
            actor.ensure_mcp_tools_initialized().await;
            tokio::time::timeout(std::time::Duration::from_secs(5), server.started.notified())
                .await
                .unwrap();
            let mut definition = agent::AgentDefinition::default_grow_build();
            definition.name = "mcp-reviewer".into();
            let (tx, rx) = tokio::sync::oneshot::channel();
            // A blocked remote list must not block a local Agent replacement.
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                actor.admit_agent_selection(definition, None, tx).await;
                rx.await.unwrap().unwrap();
            })
            .await
            .unwrap();
            let bridge = actor.agent.borrow().tool_bridge().clone();
            assert!(!Arc::ptr_eq(&old_bridge, &bridge));
            server.release.notify_one();
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                actor.mcp_initialization_worker.take().unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(listed(&actor).await, ["echo"]);
            assert!(
                old_bridge
                    .tool_definitions()
                    .await
                    .iter()
                    .all(|t| t.function.name != "demo__echo")
            );
            let result = bridge
                .call("demo__echo", json!({"value":"after switch"}), "switch-call")
                .await
                .unwrap();
            assert!(result.prompt_text.contains("echo:after switch"));
            assert_eq!(server.calls.load(Ordering::SeqCst), 1);
            let mut definition = agent::AgentDefinition::default_grow_build();
            definition.name = "mcp-reviewer-again".into();
            let (tx, rx) = tokio::sync::oneshot::channel();
            actor.admit_agent_selection(definition, None, tx).await;
            rx.await.unwrap().unwrap();
            assert_eq!(listed(&actor).await, ["echo"]);
            let bridge = actor.agent.borrow().tool_bridge().clone();
            bridge
                .call(
                    "demo__echo",
                    json!({"value":"second switch"}),
                    "second-switch-call",
                )
                .await
                .unwrap();
            assert_eq!(server.calls.load(Ordering::SeqCst), 2);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_initial_discovery_respects_retirement_and_disabled_tools() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for retire in [true, false] {
                let actor = fixture().await;
                let server = Arc::new(CatalogServer::default());
                *server.tools.lock().unwrap() = vec!["echo".into()];
                server.block_list.store(true, Ordering::SeqCst);
                actor.mcp_state.lock().await.set_acp_servers(
                    vec![::mcp::servers::AcpServerEntry {
                        name: "demo".into(),
                        server_id: "fixture".into(),
                    }],
                    server.clone(),
                );
                actor.ensure_mcp_tools_initialized().await;
                tokio::time::timeout(std::time::Duration::from_secs(5), server.started.notified())
                    .await
                    .unwrap();
                {
                    let mut state = actor.mcp_state.lock().await;
                    if retire {
                        // Advance catalog generation while discovery is outstanding.
                        assert!(state.update_configs(vec![acp::McpServer::Http(
                            acp::McpServerHttp::new("replacement", "http://127.0.0.1:1")
                        )]));
                    } else {
                        state
                            .disabled_tools
                            .insert("demo".into(), ["echo".into()].into_iter().collect());
                    }
                }
                server.release.notify_one();
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    actor.mcp_initialization_worker.take().unwrap(),
                )
                .await
                .unwrap()
                .unwrap();
                assert!(listed(&actor).await.is_empty());
                let bridge = actor.agent.borrow().tool_bridge().clone();
                assert!(
                    bridge
                        .call(
                            "demo__echo",
                            json!({"value":"must not execute"}),
                            "blocked-call"
                        )
                        .await
                        .is_err()
                );
                assert_eq!(server.calls.load(Ordering::SeqCst), 0);
                if retire {
                    assert!(
                        !actor
                            .mcp_state
                            .lock()
                            .await
                            .owned_clients
                            .contains_key("demo")
                    );
                } else {
                    assert!(
                        actor
                            .mcp_state
                            .lock()
                            .await
                            .disabled_tool_registrations
                            .contains_key("demo__echo")
                    );
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_shared_discovery_follows_agent_switch() {
    tokio::task::LocalSet::new().run_until(async {
        let parent = fixture().await;
        let server = Arc::new(CatalogServer::default());
        *server.tools.lock().unwrap() = vec!["echo".into()];
        let (_, episode) = install(&parent, &server).await;
        parent.refresh_mcp_server_tools("demo", episode).await;
        let pool = ::mcp::servers::SharedMcpPool::from_state(&*parent.mcp_state.lock().await);
        let (gateway, _notifications) = tokio::sync::mpsc::unbounded_channel();
        let (persistence, _records) = tokio::sync::mpsc::unbounded_channel();
        let actor = Arc::new(create_test_actor(0, 128000, 80, gateway, persistence).await);
        actor.mcp_state.lock().await.import_shared_clients(&pool);
        let old_bridge = actor.agent.borrow().tool_bridge().clone();
        server.block_list.store(true, Ordering::SeqCst);
        let discovery = actor.register_shared_client_tools();
        tokio::pin!(discovery);
        tokio::select! {
            _ = server.started.notified() => {},
            _ = &mut discovery => panic!("shared discovery must wait"),
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => panic!("shared discovery did not start"),
        }
        let mut definition = agent::AgentDefinition::default_grow_build();
        definition.name = "shared-mcp-reviewer".into();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            actor.admit_agent_selection(definition, None, tx).await;
            rx.await.unwrap().unwrap();
        }).await.unwrap();
        server.release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(5), discovery).await.unwrap();
        assert_eq!(listed(&actor).await, ["echo"]);
        assert!(old_bridge.tool_definitions().await.iter().all(|t| t.function.name != "demo__echo"));
        let bridge = actor.agent.borrow().tool_bridge().clone();
        bridge.call("demo__echo", json!({"value":"shared"}), "shared-switch-call").await.unwrap();
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_shared_discovery_filters_current_named_inheritance_after_agent_switch() {
    tokio::task::LocalSet::new().run_until(async {
        let parent = fixture().await;
        let server = Arc::new(CatalogServer::default());
        *server.tools.lock().unwrap() = vec!["echo".into()];
        let (_, episode) = install(&parent, &server).await;
        parent.refresh_mcp_server_tools("demo", episode).await;
        let pool = ::mcp::servers::SharedMcpPool::from_state(&*parent.mcp_state.lock().await);
        let (gateway, _notifications) = tokio::sync::mpsc::unbounded_channel();
        let (persistence, _records) = tokio::sync::mpsc::unbounded_channel();
        let mut actor = create_test_actor(0, 128000, 80, gateway, persistence).await;
        actor.mcp_state.lock().await.import_shared_clients(&pool);
        let old_bridge = actor.agent.borrow().tool_bridge().clone();
        let shared_client_ids = actor.mcp_state.lock().await.shared_client_ids();
        let capabilities = crate::session::subagent_capability::SubagentCapabilityState::from_bridge(
            &old_bridge,
            &tools::registry::types::ToolServerConfig { tools: vec![] },
            tool_types::SubagentCapabilityMode::ReadOnly,
            Some(pool.eligibility()),
            shared_client_ids,
        )
        .await;
        assert!(capabilities.mcp_server_eligible("demo"));
        actor.subagent_capabilities = Some(capabilities);
        let actor = Arc::new(actor);
        server.block_list.store(true, Ordering::SeqCst);
        let discovery = actor.register_shared_client_tools();
        tokio::pin!(discovery);
        tokio::select! {
            _ = server.started.notified() => {},
            _ = &mut discovery => panic!("shared discovery must wait"),
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => panic!("shared discovery did not start"),
        }
        let mut definition = agent::AgentDefinition::default_grow_build();
        definition.name = "shared-mcp-reviewer-named".into();
        definition.mcp_inheritance = agent::config::McpInheritance::Named(vec!["other".into()]);
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            actor.admit_agent_selection(definition, None, tx).await;
            rx.await.unwrap().unwrap();
        })
        .await
        .unwrap();
        server.release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(5), discovery)
            .await
            .unwrap();
        assert!(
            !actor.subagent_capabilities.as_ref().unwrap().mcp_server_eligible("demo"),
            "the current Agent's named inheritance must filter the inherited authority"
        );
        assert!(listed(&actor).await.is_empty());
        assert!(old_bridge.tool_definitions().await.iter().all(|t| t.function.name != "demo__echo"));
        let bridge = actor.agent.borrow().tool_bridge().clone();
        assert!(bridge.call("demo__echo", json!({"value":"shared"}), "shared-switch-call").await.is_err());
        assert_eq!(server.calls.load(Ordering::SeqCst), 0);
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_shared_discovery_failure_preserves_catalog_but_empty_catalog_removes_it() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let parent = fixture().await;
            let server = Arc::new(CatalogServer::default());
            *server.tools.lock().unwrap() = vec!["echo".into()];
            let (_, episode) = install(&parent, &server).await;
            parent.refresh_mcp_server_tools("demo", episode).await;
            let pool = ::mcp::servers::SharedMcpPool::from_state(&*parent.mcp_state.lock().await);
            let (gateway, _notifications) = tokio::sync::mpsc::unbounded_channel();
            let (persistence, _records) = tokio::sync::mpsc::unbounded_channel();
            let actor = Arc::new(create_test_actor(0, 128000, 80, gateway, persistence).await);
            actor.mcp_state.lock().await.import_shared_clients(&pool);
            actor.register_shared_client_tools().await;
            assert_eq!(listed(&actor).await, ["echo"]);

            server.fail_list.store(true, Ordering::SeqCst);
            actor.register_shared_client_tools().await;
            assert_eq!(listed(&actor).await, ["echo"]);
            let bridge = actor.agent.borrow().tool_bridge().clone();
            let result = bridge
                .call(
                    "demo__echo",
                    json!({"value":"after failed list"}),
                    "failed-list-call",
                )
                .await
                .unwrap();
            assert!(result.prompt_text.contains("echo:after failed list"));
            assert_eq!(server.calls.load(Ordering::SeqCst), 1);

            server.tools.lock().unwrap().clear();
            actor.register_shared_client_tools().await;
            assert!(listed(&actor).await.is_empty());
            assert!(
                bridge
                    .call(
                        "demo__echo",
                        json!({"value":"after empty list"}),
                        "empty-list-call"
                    )
                    .await
                    .is_err()
            );
            assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_snapshot_rejects_a_superseded_publication_generation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = fixture().await;
            let server = Arc::new(CatalogServer::default());
            *server.tools.lock().unwrap() = vec!["echo".into()];
            let (_, episode) = install(&actor, &server).await;
            actor.refresh_mcp_server_tools("demo", episode).await;
            let old_generation = actor.mcp_state.lock().await.generation();
            actor
                .mcp_state
                .lock()
                .await
                .update_configs(vec![acp::McpServer::Http(acp::McpServerHttp::new(
                    "replacement",
                    "http://127.0.0.1:1",
                ))]);
            actor
                .mcp
                .tool_metadata_snapshot
                .lock()
                .unwrap()
                .tools
                .clear();
            actor.mcp.reminder_dirty.store(false, Ordering::Relaxed);
            let published = refresh_mcp_snapshot_and_schedule_reminder_with(
                actor.agent.borrow().tool_bridge().clone(),
                Arc::clone(&actor.mcp_state),
                actor.mcp.tool_metadata_snapshot.clone(),
                Arc::clone(&actor.mcp.reminder_dirty),
                true,
                old_generation,
            )
            .await;
            assert!(!published);
            assert!(
                actor
                    .mcp
                    .tool_metadata_snapshot
                    .lock()
                    .unwrap()
                    .tools
                    .is_empty()
            );
            assert!(!actor.mcp.reminder_dirty.load(Ordering::Relaxed));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_snapshot_refresh_waits_for_publication_boundary() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = fixture().await;
            let server = Arc::new(CatalogServer::default());
            *server.tools.lock().unwrap() = vec!["echo".into()];
            let (_, episode) = install(&actor, &server).await;
            actor.refresh_mcp_server_tools("demo", episode).await;
            let publication = actor.mcp.publication_bridge.lock().await;
            let refresh = actor.refresh_mcp_snapshot_and_schedule_reminder();
            tokio::pin!(refresh);
            assert!(futures::poll!(&mut refresh).is_pending());
            // A publisher may retire the tool before the waiting refresh runs.
            actor.unregister_server_tools("demo");
            drop(publication);
            tokio::time::timeout(std::time::Duration::from_secs(5), refresh)
                .await
                .unwrap();
            assert!(listed(&actor).await.is_empty());
        })
        .await;
}

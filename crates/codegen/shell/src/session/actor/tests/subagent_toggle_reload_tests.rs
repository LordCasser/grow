use super::*;
use crate::session::actor::tests::support;
use tools::types::tool::ToolKind;

async fn toggle_actor(child: bool) -> std::sync::Arc<SessionActor> {
    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move { while persistence_rx.recv().await.is_some() {} });
    let mut actor = support::create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
    let spec = std::sync::Arc::get_mut(&mut actor.rebuild_spec).unwrap();
    spec.subagents_enabled = true;
    if child {
        spec.prompt_audience = agent::prompt::context::PromptAudience::Subagent;
        spec.subagent_depth = 1;
        spec.subagents_max_depth = 2;
        spec.initial_subagent_capability_mode = Some(tool_types::SubagentCapabilityMode::All);
        actor.startup_hints.is_subagent = true;
    }
    let mut definition = agent::AgentDefinition::default_grow_build();
    definition.subagents.allow = vec!["explore".into()];
    definition.agents_md = false;
    definition.discover_skills = false;
    actor.apply_agent_definition(definition).await.unwrap();
    std::sync::Arc::new(actor)
}

async fn has_task(actor: &SessionActor) -> bool {
    let bridge = actor.agent.borrow().tool_bridge().clone();
    bridge.tool_for_kind(ToolKind::Task).await.is_some()
}

async fn task_description(actor: &SessionActor) -> Option<String> {
    let bridge = actor.agent.borrow().tool_bridge().clone();
    let tool_name = bridge.tool_for_kind(ToolKind::Task).await?;
    bridge
        .tool_definitions()
        .await
        .into_iter()
        .find(|tool| tool.function.name == tool_name)
        .and_then(|tool| tool.function.description)
}

fn plugin_registry_with_agent(root: &std::path::Path) -> agent::plugins::PluginRegistry {
    use agent::plugins::{DiscoveredPlugin, PluginOrigin, PluginScope, discovery::PluginId};

    let manifest: agent::plugins::PluginManifest = serde_json::from_value(serde_json::json!({
        "name": "catalog-plugin",
        "agents": ["agents"]
    }))
    .unwrap();
    let discovered = DiscoveredPlugin {
        id: PluginId::new(PluginScope::User, root, "catalog-plugin"),
        root: root.to_path_buf(),
        canonical_root: root.to_path_buf(),
        scope: PluginScope::User,
        origin: PluginOrigin::UserGrow,
        trusted: true,
        agent_dirs: manifest.agent_dirs(root),
        manifest,
        skill_dirs: Vec::new(),
        command_dirs: Vec::new(),
        hooks_path: None,
        mcp_config_path: None,
        lsp_config_path: None,
        conflict: None,
    };
    agent::plugins::PluginRegistry::from_discovered(
        vec![discovered],
        &[],
        &["catalog-plugin".into()],
    )
}

async fn catalog_actor(
    child: bool,
    cwd: &std::path::Path,
    selector: &str,
    cli_agent: Option<agent::AgentDefinition>,
    plugin_registry: Option<std::sync::Arc<agent::plugins::PluginRegistry>>,
) -> std::sync::Arc<SessionActor> {
    let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move { while persistence_rx.recv().await.is_some() {} });
    let mut actor = support::create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
    let spec = std::sync::Arc::get_mut(&mut actor.rebuild_spec).unwrap();
    spec.working_directory = cwd.to_path_buf();
    spec.subagents_enabled = true;
    spec.subagent_toggle
        .write()
        .insert(selector.to_string(), false);
    spec.cli_subagents = cli_agent.into_iter().collect();
    *spec.plugin_registry.write() = plugin_registry;
    if child {
        spec.prompt_audience = agent::prompt::context::PromptAudience::Subagent;
        spec.subagent_depth = 1;
        spec.subagents_max_depth = 2;
        spec.initial_subagent_capability_mode = Some(tool_types::SubagentCapabilityMode::All);
        actor.startup_hints.is_subagent = true;
    }
    let mut definition = agent::AgentDefinition::default_grow_build();
    definition.subagents.allow = vec![selector.to_string()];
    definition.agents_md = false;
    definition.discover_skills = false;
    actor.apply_agent_definition(definition).await.unwrap();
    std::sync::Arc::new(actor)
}

async fn apply_catalog_reload(actor: &std::sync::Arc<SessionActor>, first_boundary: bool) {
    actor.admit_subagent_catalog_reload().await;
    let boundary = if first_boundary {
        actor
            .end_step_control_boundary("continued")
            .await
            .expect("test turn has an active step boundary")
    } else {
        actor.state.lock().await.pending_step_controls.boundary()
    };
    assert_eq!(
        actor
            .apply_pending_controls_at_step_boundary(boundary)
            .await,
        (false, true, false)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cli_and_plugin_catalogs_rebuild_across_toggle_delete_for_primary_and_child() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let temp = tempfile::tempdir().unwrap();
            let plugin_root = temp.path().join("catalog-plugin");
            let plugin_agents = plugin_root.join("agents");
            std::fs::create_dir_all(&plugin_agents).unwrap();
            std::fs::write(
                plugin_agents.join("reviewer.md"),
                "---\nname: reviewer\ndescription: Plugin reviewer\n---\n",
            )
            .unwrap();
            let registry = std::sync::Arc::new(plugin_registry_with_agent(&plugin_root));

            for (selector, cli_agent, plugins) in [
                (
                    "cli-reviewer",
                    Some({
                        let mut definition = agent::AgentDefinition::default_grow_build();
                        definition.name = "cli-reviewer".into();
                        definition.description = "CLI reviewer".into();
                        definition
                    }),
                    None,
                ),
                ("catalog-plugin:reviewer", None, Some(registry.clone())),
            ] {
                for child in [false, true] {
                    let actor = catalog_actor(
                        child,
                        temp.path(),
                        selector,
                        cli_agent.clone(),
                        plugins.clone(),
                    )
                    .await;
                    assert!(!has_task(&actor).await, "starts with source disabled");

                    support::begin_test_active_causal_turn(&actor).await;
                    actor
                        .rebuild_spec
                        .subagent_toggle
                        .write()
                        .insert(selector.to_string(), true);
                    apply_catalog_reload(&actor, true).await;
                    let description = task_description(&actor).await.unwrap();
                    assert!(description.contains(selector), "{description}");

                    actor
                        .rebuild_spec
                        .subagent_toggle
                        .write()
                        .insert(selector.to_string(), false);
                    apply_catalog_reload(&actor, false).await;
                    assert!(!has_task(&actor).await, "disabled catalog removes Task");

                    actor.rebuild_spec.subagent_toggle.write().remove(selector);
                    apply_catalog_reload(&actor, false).await;
                    let description = task_description(&actor).await.unwrap();
                    assert!(
                        description.contains(selector),
                        "deletion re-enables: {description}"
                    );
                }
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn subagent_toggle_reload_removes_and_restores_empty_catalog_at_step_boundary() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for child in [false, true] {
                let actor = toggle_actor(child).await;
                assert!(has_task(&actor).await);
                support::begin_test_active_causal_turn(&actor).await;
                actor
                    .rebuild_spec
                    .subagent_toggle
                    .write()
                    .insert("explore".into(), false);
                actor.admit_subagent_catalog_reload().await;
                assert!(
                    has_task(&actor).await,
                    "active Step retains its tool catalog"
                );
                let boundary = actor.end_step_control_boundary("continued").await.unwrap();
                assert_eq!(
                    actor
                        .apply_pending_controls_at_step_boundary(boundary)
                        .await,
                    (false, true, false)
                );
                assert!(
                    !has_task(&actor).await,
                    "all permitted types disabled removes Task"
                );
                actor.rebuild_spec.subagent_toggle.write().clear();
                actor.admit_subagent_catalog_reload().await;
                // No provider request is needed: apply the next captured safe boundary.
                let boundary = actor.state.lock().await.pending_step_controls.boundary();
                assert_eq!(
                    actor
                        .apply_pending_controls_at_step_boundary(boundary)
                        .await,
                    (false, true, false)
                );
                assert!(
                    has_task(&actor).await,
                    "deletion restores default-enabled Task"
                );
                let bridge = actor.agent.borrow().tool_bridge().clone();
                let task_name = bridge.tool_for_kind(ToolKind::Task).await.unwrap();
                let tools = bridge.tool_definitions().await;
                let task = tools
                    .iter()
                    .find(|tool| tool.function.name == task_name)
                    .unwrap();
                let description = task.function.description.as_deref().unwrap();
                assert!(description.contains("explore"));
                assert!(!description.contains("general-purpose"));
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn subagent_toggle_reload_invalidates_prepared_agent_catalog() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = toggle_actor(false).await;
            support::begin_test_active_causal_turn(&actor).await;
            let mut definition = actor.agent.borrow().definition().clone();
            definition.name = "selected-after-reload".into();
            let (responds_to, response) = tokio::sync::oneshot::channel();
            actor
                .admit_agent_selection(definition, None, responds_to)
                .await;
            let prepared = actor
                .state
                .lock()
                .await
                .pending_step_controls
                .agent
                .as_ref()
                .unwrap()
                .value
                .preparation
                .clone();
            assert!(prepared.wait_ready().await);
            assert!(prepared.has_agent());
            actor
                .rebuild_spec
                .subagent_toggle
                .write()
                .insert("explore".into(), false);
            let boundary = actor.end_step_control_boundary("continued").await.unwrap();
            assert_eq!(
                actor
                    .apply_pending_controls_at_step_boundary(boundary)
                    .await,
                (false, true, false)
            );
            assert_eq!(
                response.await.unwrap().unwrap(),
                crate::session::DesiredStateOutcome::Applied(())
            );
            assert_eq!(actor.agent.borrow().name(), "selected-after-reload");
            assert!(
                !has_task(&actor).await,
                "prepared selection must not restore stale enabled Task"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn subagent_toggle_reload_idle_drains_and_preserves_frozen_workflow() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = toggle_actor(false).await;
            actor
                .rebuild_spec
                .subagent_toggle
                .write()
                .insert("explore".into(), false);
            actor.admit_subagent_catalog_reload().await;
            // Await the existing idle-control owner; no polling or timer determines correctness.
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    if actor.state.lock().await.foreground.is_idle() {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(!has_task(&actor).await);
            let mut frozen = toggle_actor(false).await;
            let owner = std::sync::Arc::get_mut(&mut frozen).unwrap();
            std::sync::Arc::get_mut(&mut owner.rebuild_spec)
                .unwrap()
                .workflow_owned = true;
            frozen
                .rebuild_spec
                .subagent_toggle
                .write()
                .insert("explore".into(), false);
            frozen.admit_subagent_catalog_reload().await;
            assert!(
                frozen
                    .state
                    .lock()
                    .await
                    .pending_step_controls
                    .next_key()
                    .is_none()
            );
            assert!(
                has_task(&frozen).await,
                "frozen Workflow catalog is not rebuilt by live toggle"
            );
        })
        .await;
}

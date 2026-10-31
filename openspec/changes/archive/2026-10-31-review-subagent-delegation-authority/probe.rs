//! Temporary audit probes: assertions describe the observed defect, not the desired contract.
use super::*;
use agent::plugins::discovery::PluginId;
use agent::plugins::{DiscoveredPlugin, PluginOrigin, PluginRegistry, PluginScope};
use tools::implementations::grow_build::task::types::SubagentExecutionRecipe;

fn plugin_registry(root: &std::path::Path) -> PluginRegistry {
    let agents = root.join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("audit-worker.md"),
        "---\nname: audit-worker\ndescription: audit worker\n---\n",
    )
    .unwrap();
    PluginRegistry::from_discovered(
        vec![DiscoveredPlugin {
            manifest: serde_json::from_value(serde_json::json!({"name":"audit-plugin"})).unwrap(),
            id: PluginId::new(PluginScope::User, root, "audit-plugin"),
            root: root.to_path_buf(),
            canonical_root: root.to_path_buf(),
            scope: PluginScope::User,
            origin: PluginOrigin::UserGrow,
            trusted: true,
            skill_dirs: vec![],
            command_dirs: vec![],
            agent_dirs: vec![agents],
            hooks_path: None,
            mcp_config_path: None,
            lsp_config_path: None,
            conflict: None,
        }],
        &[],
        &["audit-plugin".into()],
    )
}

#[test]
fn audit_probe_plugin_alias_bypasses_toggle_and_parent_deny() {
    let root = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(plugin_registry(root.path()));
    let qualified = "audit-plugin:audit-worker";
    let bare = "audit-worker";
    let mut ctx = ctx_with_toggle(HashMap::from([(qualified.into(), false)]));
    ctx.parent_cwd = root.path().to_path_buf();
    ctx.plugin_registry = Some(registry.clone());
    assert_eq!(
        resolve_agent_definition(bare, &ctx)
            .unwrap()
            .selector_identity(),
        qualified
    );
    assert!(matches!(
        gate_subagent_type(qualified, &ctx),
        SubagentValidateTypeOutcome::Disabled
    ));
    assert!(matches!(
        gate_subagent_type(bare, &ctx),
        SubagentValidateTypeOutcome::Ok
    ));
    let entries = agent::discovery::all_subagents_with_plugins(
        root.path(),
        &ctx.subagent_toggle.read(),
        Some(&registry),
    );
    assert!(
        entries.iter().any(|entry| entry.name == qualified),
        "disabled plugin remains advertised"
    );
    ctx.subagent_toggle.write().clear();
    let mut parent = agent::AgentDefinition::default_grow_build();
    parent.subagents.deny = vec![qualified.into()];
    ctx.subagent_filter = parent.subagent_filter();
    assert!(matches!(
        gate_subagent_type(qualified, &ctx),
        SubagentValidateTypeOutcome::ParentRestricted { .. }
    ));
    assert!(matches!(
        gate_subagent_type(bare, &ctx),
        SubagentValidateTypeOutcome::Ok
    ));
}

#[test]
fn audit_probe_native_path_alias_bypasses_toggle_and_parent_deny() {
    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    let agents = root.path().join(".grow/agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("audit-worker.md"),
        "---\nname: audit-worker\ndescription: audit worker\n---\n",
    )
    .unwrap();
    let mut ctx = ctx_with_toggle(HashMap::from([("audit-worker".into(), false)]));
    ctx.parent_cwd = root.path().to_path_buf();
    assert!(matches!(
        gate_subagent_type("audit-worker", &ctx),
        SubagentValidateTypeOutcome::Disabled
    ));
    assert!(matches!(
        gate_subagent_type("./audit-worker", &ctx),
        SubagentValidateTypeOutcome::Ok
    ));
    let named = resolve_agent_definition("audit-worker", &ctx).unwrap();
    let alias = resolve_agent_definition("./audit-worker", &ctx).unwrap();
    assert_eq!(
        std::fs::canonicalize(named.source_path.unwrap()).unwrap(),
        std::fs::canonicalize(alias.source_path.unwrap()).unwrap()
    );
    ctx.subagent_toggle.write().clear();
    let mut parent = agent::AgentDefinition::default_grow_build();
    parent.subagents.deny = vec!["audit-worker".into()];
    ctx.subagent_filter = parent.subagent_filter();
    assert!(matches!(
        gate_subagent_type("audit-worker", &ctx),
        SubagentValidateTypeOutcome::ParentRestricted { .. }
    ));
    assert!(matches!(
        gate_subagent_type("./audit-worker", &ctx),
        SubagentValidateTypeOutcome::Ok
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn audit_probe_cli_agent_gate_and_tool_catalog_disagree() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let mut cli_agent = agent::AgentDefinition::general_purpose();
            cli_agent.name = "audit-cli-only".into();
            let mut parent = agent::AgentDefinition::default_grow_build();
            parent.subagents.allow = vec![cli_agent.name.clone()];
            parent.agents_md = false;
            parent.discover_skills = false;
            let mut ctx = ctx_with_toggle(HashMap::new());
            ctx.parent_cwd = root.path().to_path_buf();
            ctx.subagent_filter = parent.subagent_filter();
            let mut cfg = crate::agent::config::Config::default();
            cfg.cli_agents = vec![cli_agent];
            ctx.agent_config = Some(cfg);
            assert!(matches!(
                gate_subagent_type("audit-cli-only", &ctx),
                SubagentValidateTypeOutcome::Ok
            ));
            let mut spec = crate::session::agent_rebuild::test_rebuild_spec_default();
            let mutable = std::sync::Arc::get_mut(&mut spec).unwrap();
            mutable.working_directory = root.path().to_path_buf();
            mutable.subagents_enabled = true;
            let built = spec
                .build_agent_with_initial_overrides(parent, None, None)
                .await
                .unwrap();
            assert!(
                built
                    .tool_bridge()
                    .tool_for_kind(tools::types::tool::ToolKind::Task)
                    .await
                    .is_none(),
                "an allowed CLI-only target is absent from the actual runtime catalog"
            );
        })
        .await;
}

#[test]
fn audit_probe_restore_recipe_recomputes_default_rwx() {
    let request = auto_wake_test_request("audit-restore");
    assert!(request.runtime_overrides.capability_mode.is_none());
    let mut original = agent::AgentDefinition::general_purpose();
    original.capability_mode = Some(tool_types::SubagentCapabilityMode::ReadOnly);
    let birth =
        resolution::resolve_runtime_config(&request.runtime_overrides, &original).capability_mode;
    assert_eq!(birth, tool_types::SubagentCapabilityMode::ReadOnly);
    let recipe = SubagentExecutionRecipe::from_value(&request.execution_recipe()).unwrap();
    assert!(recipe.runtime_overrides.capability_mode.is_none());
    // The production restore loader chooses the current selected definition;
    // it does not feed Spawn.capability_mode back into this resolver.
    let mut selected = original.clone();
    selected.capability_mode = Some(tool_types::SubagentCapabilityMode::All);
    let restored =
        resolution::resolve_runtime_config(&recipe.runtime_overrides, &selected).capability_mode;
    let old_ceiling = crate::session::subagent_capability::DelegableCapabilityCeiling::new(
        birth,
        Default::default(),
    );
    let recreated_ceiling = crate::session::subagent_capability::DelegableCapabilityCeiling::new(
        restored,
        Default::default(),
    );
    assert_eq!(
        old_ceiling.constrain_mode(tool_types::SubagentCapabilityMode::All),
        birth
    );
    assert_eq!(
        recreated_ceiling.constrain_mode(tool_types::SubagentCapabilityMode::All),
        tool_types::SubagentCapabilityMode::All
    );
}

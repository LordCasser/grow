use super::*;
use agent::plugins::discovery::PluginId;
use agent::plugins::{DiscoveredPlugin, PluginOrigin, PluginRegistry, PluginScope};

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
fn plugin_alias_uses_canonical_toggle_and_parent_filter() {
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(plugin_registry(root.path()));
    let qualified = "audit-plugin:audit-worker";
    let mut ctx = ctx_with_toggle(HashMap::from([(qualified.into(), false)]));
    ctx.parent_cwd = root.path().to_path_buf();
    ctx.plugin_registry = Some(registry);
    for invalid in [
        "audit-plugin:./audit-worker",
        "audit-plugin:dir/audit-worker",
        "audit-plugin:AUDIT-WORKER",
        "AUDIT-WORKER",
        "audit-plugin:audit-worker/..",
    ] {
        assert!(resolve_agent_definition(invalid, &ctx).is_none());
        assert!(matches!(
            gate_subagent_type(invalid, &ctx),
            SubagentValidateTypeOutcome::Unknown { .. }
        ));
    }
    for selector in [qualified, "audit-worker"] {
        assert!(matches!(
            gate_subagent_type(selector, &ctx),
            SubagentValidateTypeOutcome::Disabled
        ));
    }
    // CLI fallback must not resurrect the bare alias of a disabled plugin
    // in either the model catalog or diagnostic candidates.
    let mut cli = agent::AgentDefinition::general_purpose();
    cli.name = "audit-worker".into();
    let cli_agents = vec![cli];
    let toggles = ctx.subagent_toggle.read().clone();
    let resolution = super::super::resolution::DefinitionResolutionContext {
        cwd: root.path(),
        plugins: ctx.plugin_registry.as_deref(),
        cli_agents: &cli_agents,
        toggles: &toggles,
    };
    assert!(
        !super::super::resolution::available_agent_names(&resolution)
            .iter()
            .any(|name| name == qualified || name == "audit-worker")
    );
    let cli_names = vec!["audit-worker".to_owned()];
    let validation = super::super::resolution::DefinitionValidationContext {
        cwd: root.path(),
        plugins: ctx.plugin_registry.as_deref(),
        cli_agent_names: &cli_names,
        toggles: &toggles,
    };
    assert!(
        !super::super::resolution::available_validation_agent_names(&validation)
            .iter()
            .any(|name| name == qualified || name == "audit-worker")
    );
    assert!(
        super::super::resolution::capture_agent_definitions(
            &resolution,
            &Default::default(),
            &Default::default()
        )
        .values()
        .all(|definition| definition.selector_identity() != qualified)
    );
    ctx.subagent_toggle.write().clear();
    let mut parent = agent::AgentDefinition::default_grow_build();
    parent.subagents.deny = vec![qualified.into()];
    ctx.subagent_filter = parent.subagent_filter();
    for selector in [qualified, "audit-worker"] {
        assert!(matches!(
            gate_subagent_type(selector, &ctx),
            SubagentValidateTypeOutcome::ParentRestricted { .. }
        ));
    }
    parent.subagents.deny.clear();
    parent.subagents.allow = vec![qualified.into()];
    ctx.subagent_filter = parent.subagent_filter();
    for selector in [qualified, "audit-worker"] {
        assert!(matches!(
            gate_subagent_type(selector, &ctx),
            SubagentValidateTypeOutcome::Ok
        ));
    }
    // A native file with the same bare ID retains native precedence. Neither
    // the plugin's toggle nor its allow entry grants this other definition.
    git2::Repository::init(root.path()).unwrap();
    let agents = root.path().join(".grow/agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("audit-worker.md"),
        "---\nname: ignored\ndescription: native\n---\n",
    )
    .unwrap();
    assert!(matches!(
        gate_subagent_type("audit-worker", &ctx),
        SubagentValidateTypeOutcome::ParentRestricted { .. }
    ));
    assert!(matches!(
        gate_subagent_type(qualified, &ctx),
        SubagentValidateTypeOutcome::Ok
    ));
}

#[test]
fn native_path_aliases_are_not_registered_identities() {
    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    let agents = root.path().join(".grow/agents/team");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("worker.md"),
        "---\nname: ignored\ndescription: worker\n---\n",
    )
    .unwrap();
    let mut ctx = ctx_with_toggle(HashMap::from([("team/worker".into(), false)]));
    ctx.parent_cwd = root.path().to_path_buf();
    assert!(matches!(
        gate_subagent_type("team/worker", &ctx),
        SubagentValidateTypeOutcome::Disabled
    ));
    for selector in [
        "./team/worker",
        "TEAM/worker",
        "team/WORKER",
        "team/./worker",
        "team//worker",
        "team/../team/worker",
        "/team/worker",
        "team/worker/",
        "team\\worker",
    ] {
        assert!(
            resolve_agent_definition(selector, &ctx).is_none(),
            "{selector}"
        );
        assert!(
            matches!(
                gate_subagent_type(selector, &ctx),
                SubagentValidateTypeOutcome::Unknown { .. }
            ),
            "{selector}"
        );
    }
    ctx.subagent_toggle.write().clear();
    let mut parent = agent::AgentDefinition::default_grow_build();
    parent.subagents.deny = vec!["team/worker".into()];
    ctx.subagent_filter = parent.subagent_filter();
    assert!(matches!(
        gate_subagent_type("team/worker", &ctx),
        SubagentValidateTypeOutcome::ParentRestricted { .. }
    ));
    parent.subagents.deny.clear();
    parent.subagents.allow = vec!["team/worker".into()];
    ctx.subagent_filter = parent.subagent_filter();
    assert!(matches!(
        gate_subagent_type("team/worker", &ctx),
        SubagentValidateTypeOutcome::Ok
    ));
}

#[test]
fn restored_birth_capability_requires_the_persisted_effective_fact() {
    use super::super::resolution::restored_birth_capability;
    use tool_types::SubagentCapabilityMode as Mode;
    assert_eq!(
        restored_birth_capability(Some("read-only")).unwrap(),
        Mode::ReadOnly
    );
    assert_eq!(restored_birth_capability(Some("all")).unwrap(), Mode::All);
    assert!(
        restored_birth_capability(None)
            .unwrap_err()
            .contains("missing")
    );
    assert!(
        restored_birth_capability(Some("ReadOnly"))
            .unwrap_err()
            .contains("invalid")
    );
    assert!(restored_birth_capability(Some("")).is_err());
}

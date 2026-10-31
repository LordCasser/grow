use super::*;
use crate::session::SessionCommand;
use tools::implementations::grow_build::task::types::SubagentValidateTypeOutcome;

#[tokio::test(flavor = "current_thread")]
async fn subagent_toggle_reload_updates_admission_and_primary_child_catalogs() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let agent = build_minimal_agent_for_tests();
            let (mut parent, mut parent_commands) =
                make_test_handle_with_receiver("test/default", None);
            let parent_id = acp::SessionId::new("toggle-parent");
            parent.info.id = parent_id.clone();
            let (mut child, mut child_commands) =
                make_test_handle_with_receiver("test/default", None);
            let child_id = acp::SessionId::new("toggle-child");
            child.info.id = child_id.clone();
            let (mut frozen, mut frozen_commands) =
                make_test_handle_with_receiver("test/default", None);
            frozen.workflow_run_id = Some("frozen-run".into());
            agent
                .sessions
                .borrow_mut()
                .insert(parent_id.clone(), parent);
            agent
                .active_child_sessions
                .borrow_mut()
                .insert(child_id.clone(), child);
            agent
                .active_child_sessions
                .borrow_mut()
                .insert(acp::SessionId::new("toggle-frozen"), frozen);
            let shared_before_reload = agent.cfg.borrow().subagent_toggle.clone();
            let permission_mode = agent.cfg.borrow().subagent_permission_mode;
            for (source, disabled) in [
                ("[subagents.toggle]\ngeneral-purpose = false", true),
                ("[subagents.toggle]\ngeneral-purpose = true", false),
                ("[subagents.toggle]\ngeneral-purpose = false", true),
                ("", false),
            ] {
                let config = toml::from_str(source).unwrap();
                crate::extensions::session_admin::apply_subagent_toggle_config(&agent, &config)
                    .unwrap();
                assert_eq!(
                    shared_before_reload
                        .read()
                        .get("general-purpose")
                        .copied()
                        .unwrap_or(true),
                    !disabled
                );
                for id in [&parent_id, &child_id] {
                    let context = agent.build_subagent_validation_context(id.0.as_ref());
                    let result =
                        crate::agent::subagent::validate_subagent_type("general-purpose", &context);
                    assert_eq!(
                        matches!(result, SubagentValidateTypeOutcome::Disabled),
                        disabled
                    );
                    if !disabled {
                        assert!(matches!(result, SubagentValidateTypeOutcome::Ok));
                    }
                }
                assert!(matches!(
                    parent_commands.try_recv().unwrap(),
                    SessionCommand::ReloadSubagentCatalog
                ));
                assert!(matches!(
                    child_commands.try_recv().unwrap(),
                    SessionCommand::ReloadSubagentCatalog
                ));
                assert!(frozen_commands.try_recv().is_err());
            }
            let before = shared_before_reload.read().clone();
            let invalid = toml::from_str("[subagents.toggle]\ngeneral-purpose = 'false'").unwrap();
            assert!(
                crate::extensions::session_admin::apply_subagent_toggle_config(&agent, &invalid)
                    .is_err()
            );
            assert_eq!(*shared_before_reload.read(), before);
            assert_eq!(agent.cfg.borrow().subagent_permission_mode, permission_mode);
            assert!(parent_commands.try_recv().is_err());
            assert!(child_commands.try_recv().is_err());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn subagent_validation_reports_the_immediate_child_profile() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let agent = build_minimal_agent_for_tests();
            let (mut child, _commands) = make_test_handle_with_receiver("test/default", None);
            let child_id = acp::SessionId::new("restricted-child");
            child.info.id = child_id.clone();
            let mut definition = agent::AgentDefinition::default_grow_build();
            definition.subagents.allow = vec!["explore".into()];
            child
                .agent_profile
                .replace("team/researcher".into(), definition.subagent_filter());
            agent
                .active_child_sessions
                .borrow_mut()
                .insert(child_id.clone(), child);
            agent
                .cfg
                .borrow()
                .subagent_toggle
                .write()
                .insert("general-purpose".into(), true);
            let context = agent.build_subagent_validation_context(child_id.0.as_ref());
            match crate::agent::subagent::validate_subagent_type("general-purpose", &context) {
                SubagentValidateTypeOutcome::ParentRestricted {
                    parent_agent,
                    available,
                } => {
                    assert_eq!(parent_agent, "team/researcher");
                    assert_eq!(available, vec!["explore"]);
                }
                other => panic!("expected immediate parent restriction: {other:?}"),
            }
        })
        .await;
}

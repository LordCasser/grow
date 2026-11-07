use super::*;
use crate::views::extensions_modal::{
    ExtensionsModalState, ExtensionsTab, ModalMessage, TabDataState,
};
use tools::implementations::skills::types::SkillInfo;

fn catalog(name: &str) -> Vec<SkillInfo> {
    vec![SkillInfo {
        name: name.into(),
        path: format!("/{name}/SKILL.md"),
        ..Default::default()
    }]
}

fn completion(
    toggle: bool,
    session_id: acp::SessionId,
    binding_epoch: u32,
    success: bool,
) -> TaskResult {
    let result = if success {
        Ok(catalog("returned"))
    } else {
        Err("old failure".into())
    };
    if toggle {
        TaskResult::SkillsToggleDone {
            agent_id: AgentId(0),
            session_id,
            binding_epoch,
            result,
        }
    } else {
        TaskResult::SkillsListLoaded {
            agent_id: AgentId(0),
            session_id,
            binding_epoch,
            result,
        }
    }
}

#[test]
fn skills_binding_stale_success_and_error_leave_catalog_and_pending_untouched() {
    for return_to_a in [false, true] {
        for toggle in [false, true] {
            for success in [false, true] {
                let mut app = test_app_with_agent();
                let id = AgentId(0);
                let agent = app.agents.get_mut(&id).unwrap();
                agent.bind_session_id(acp::SessionId::new("A"));
                agent.session_binding_epoch = 10;
                // Capture through the actual action producer before rebinding.
                let action = if toggle {
                    Action::ToggleSkill {
                        skill_name: "deploy".into(),
                        enabled: false,
                    }
                } else {
                    Action::ReloadSkills
                };
                let effects = dispatch(action, &mut app);
                let (sid, epoch) = effects
                    .iter()
                    .find_map(|effect| match effect {
                        Effect::FetchSkillsList {
                            session_id,
                            binding_epoch,
                            ..
                        }
                        | Effect::ToggleSkill {
                            session_id,
                            binding_epoch,
                            ..
                        } => Some((session_id.clone(), *binding_epoch)),
                        _ => None,
                    })
                    .unwrap();
                let agent = app.agents.get_mut(&id).unwrap();
                agent.bind_session_id(acp::SessionId::new("B"));
                if return_to_a {
                    agent.bind_session_id(acp::SessionId::new("A"));
                }
                let mut modal = ExtensionsModalState::new(ExtensionsTab::Skills);
                modal.skills_data = TabDataState::Loaded(catalog("current-directory"));
                modal.modal_message = Some(ModalMessage::Error("current error".into()));
                modal.pending_action = Some("current toggle".into());
                modal.pending_entry_index = Some(3);
                modal.picker_state.selected = 3;
                agent.extensions_modal = Some(modal);
                assert!(
                    dispatch(
                        Action::TaskComplete(completion(toggle, sid, epoch, success)),
                        &mut app
                    )
                    .is_empty()
                );
                let modal = app.agents[&id].extensions_modal.as_ref().unwrap();
                assert!(
                    matches!(&modal.skills_data, TabDataState::Loaded(skills) if skills == &catalog("current-directory"))
                );
                assert_eq!(
                    modal.modal_message,
                    Some(ModalMessage::Error("current error".into()))
                );
                assert_eq!(modal.pending_action.as_deref(), Some("current toggle"));
                assert_eq!(modal.pending_entry_index, Some(3));
                assert_eq!(modal.picker_state.selected, 3);
            }
        }
    }
}

#[test]
fn skills_binding_current_success_and_error_settle_normally() {
    for toggle in [false, true] {
        for success in [false, true] {
            let mut app = test_app_with_agent();
            let id = AgentId(0);
            let agent = app.agents.get_mut(&id).unwrap();
            let sid = agent.session.session_id.clone().unwrap();
            let epoch = agent.session_binding_epoch;
            let mut modal = ExtensionsModalState::new(ExtensionsTab::Skills);
            modal.skills_data = TabDataState::Loaded(catalog("previous"));
            modal.pending_action = Some("pending".into());
            modal.pending_entry_index = Some(3);
            modal.picker_state.selected = 3;
            agent.extensions_modal = Some(modal);
            dispatch(
                Action::TaskComplete(completion(toggle, sid, epoch, success)),
                &mut app,
            );
            let modal = app.agents[&id].extensions_modal.as_ref().unwrap();
            assert!(modal.pending_action.is_none());
            assert!(modal.pending_entry_index.is_none());
            if success {
                assert!(
                    matches!(&modal.skills_data, TabDataState::Loaded(skills) if skills == &catalog("returned"))
                );
                if toggle {
                    assert_eq!(modal.picker_state.selected, 0);
                }
            } else if toggle {
                assert_eq!(
                    modal.modal_message,
                    Some(ModalMessage::Error("old failure".into()))
                );
                assert!(
                    matches!(&modal.skills_data, TabDataState::Loaded(skills) if skills == &catalog("previous"))
                );
            } else {
                assert!(
                    matches!(&modal.skills_data, TabDataState::Error(error) if error == "old failure")
                );
            }
        }
    }
}

#[test]
fn skills_binding_wrong_session_at_current_epoch_cannot_settle_pending() {
    for toggle in [false, true] {
        for success in [false, true] {
            let mut app = test_app_with_agent();
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            let epoch = agent.session_binding_epoch;
            let mut modal = ExtensionsModalState::new(ExtensionsTab::Skills);
            modal.pending_action = Some("current request".into());
            modal.pending_entry_index = Some(2);
            agent.extensions_modal = Some(modal);
            dispatch(
                Action::TaskComplete(completion(
                    toggle,
                    acp::SessionId::new("another-session"),
                    epoch,
                    success,
                )),
                &mut app,
            );
            let modal = app.agents[&AgentId(0)].extensions_modal.as_ref().unwrap();
            assert!(matches!(&modal.skills_data, TabDataState::Loading));
            assert!(modal.modal_message.is_none());
            assert_eq!(modal.pending_action.as_deref(), Some("current request"));
            assert_eq!(modal.pending_entry_index, Some(2));
        }
    }
}

#[test]
fn skills_binding_no_session_modal_creates_before_fetching() {
    let mut app = test_app_with_agent();
    app.agents.get_mut(&AgentId(0)).unwrap().unbind_session_id();
    let effects = dispatch(
        Action::OpenExtensionsModal {
            tab: ExtensionsTab::Skills,
            trigger: diagnostics::events::ExtensionsModalTrigger::SlashCommand,
        },
        &mut app,
    );
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::CreateSession {
            agent_id: AgentId(0),
            ..
        }
    )));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::FetchSkillsList { .. }))
    );
    assert!(app.agents[&AgentId(0)].session.pending_extensions_fetch());
    let effects = dispatch(
        Action::TaskComplete(TaskResult::SessionCreated {
            agent_id: AgentId(0),
            session_id: acp::SessionId::new("created-owner"),
            models: None,
        }),
        &mut app,
    );
    assert_fetch_binding(&effects, &app);
    assert!(!app.agents[&AgentId(0)].session.pending_extensions_fetch());
}

fn assert_fetch_binding(effects: &[Effect], app: &AppView) {
    let agent = &app.agents[&AgentId(0)];
    assert!(effects.iter().any(|effect| matches!(effect,
        Effect::FetchSkillsList { session_id, binding_epoch, .. }
        if Some(session_id) == agent.session.session_id.as_ref() && *binding_epoch == agent.session_binding_epoch
    )), "Skills fetch must capture current binding: {effects:?}");
}

#[test]
fn skills_binding_open_reload_toggle_and_session_ready_producers_capture_epoch() {
    let mut app = test_app_with_agent();
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session_binding_epoch = 23;
    let effects = dispatch(
        Action::OpenExtensionsModal {
            tab: ExtensionsTab::Skills,
            trigger: diagnostics::events::ExtensionsModalTrigger::SlashCommand,
        },
        &mut app,
    );
    assert_fetch_binding(&effects, &app);
    let effects = dispatch(Action::ReloadSkills, &mut app);
    assert_fetch_binding(&effects, &app);
    let effects = dispatch(
        Action::ToggleSkill {
            skill_name: "deploy".into(),
            enabled: false,
        },
        &mut app,
    );
    let agent = &app.agents[&AgentId(0)];
    assert!(
        matches!(&effects[..], [Effect::ToggleSkill { session_id, binding_epoch: 23, .. }]
        if Some(session_id) == agent.session.session_id.as_ref())
    );
    for load in [false, true] {
        let mut app = test_app_with_agent();
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.session.session_id = None;
        agent.session_binding_epoch = 31;
        agent.session.set_pending_extensions_fetch();
        agent.extensions_modal = Some(ExtensionsModalState::new(ExtensionsTab::Skills));
        let result = if load {
            TaskResult::SessionLoaded {
                agent_id: AgentId(0),
                session_id: acp::SessionId::new("ready"),
                models: None,
                code_restored: false,
                restore_summary: None,
                foreground: None,
                descendant_ownership: None,
            }
        } else {
            TaskResult::SessionCreated {
                agent_id: AgentId(0),
                session_id: acp::SessionId::new("ready"),
                models: None,
            }
        };
        let effects = dispatch(Action::TaskComplete(result), &mut app);
        assert_fetch_binding(&effects, &app);
    }
}

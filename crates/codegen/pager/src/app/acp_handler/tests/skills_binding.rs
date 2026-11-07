use super::*;
use crate::app::actions::Effect;
use crate::views::extensions_modal::{ExtensionsModalState, ExtensionsTab, TabDataState};

#[test]
fn skills_binding_plugins_changed_refetch_captures_current_session_epoch() {
    let mut app = make_app_with_agent("skills-owner");
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.session_binding_epoch = 47;
    let mut modal = ExtensionsModalState::new(ExtensionsTab::Skills);
    modal.skills_data = TabDataState::Loaded(vec![]);
    agent.extensions_modal = Some(modal);
    assert!(handle(
        make_ext_session_notification(
            "skills-owner",
            GrowSessionUpdate::PluginsChanged { plugins: vec![] },
        ),
        &mut app
    ));
    assert!(
        app.pending_effects.iter().any(|effect| matches!(effect,
            Effect::FetchSkillsList { agent_id: AgentId(0), session_id, binding_epoch: 47 }
            if session_id.0.as_ref() == "skills-owner"
        )),
        "notification must schedule a fetch with the current binding"
    );
    assert!(matches!(
        &app.agents[&AgentId(0)]
            .extensions_modal
            .as_ref()
            .unwrap()
            .skills_data,
        TabDataState::Loading
    ));
}

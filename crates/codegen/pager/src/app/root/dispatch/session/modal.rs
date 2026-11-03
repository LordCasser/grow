//! Session rename and local-view retirement (dashboard and deletion).
//!
//! The `/sessions` picker modal was removed; rename-via-slash and
//! dashboard close still use these dispatchers.
use crate::app::actions::Effect;
use crate::app::root::dispatch::ctx::{SwitchCause, show_welcome, switch_to_agent};
use crate::app::root::dispatch::task_result::unregister_session_effect;
use crate::app::root::{ActiveView, AppView};
use crate::app::session::AgentId;
use acp_transport::protocol as acp;
/// Scope async deletion outcomes to the views that initiated this request.
pub(in crate::app::root::dispatch) fn session_delete_bindings(
    app: &AppView,
    session_id: &str,
) -> Vec<(AgentId, u32)> {
    app.agents
        .iter()
        .filter(|(_, agent)| {
            agent
                .session
                .session_id
                .as_ref()
                .is_some_and(|sid| sid.0.as_ref() == session_id)
        })
        .map(|(id, agent)| (*id, agent.session_binding_epoch))
        .collect()
}

/// Remove an agent and clean up all references to it:
/// `forked_from` pointers on surviving agents.
pub(in crate::app::root::dispatch) fn remove_agent_and_cleanup(
    app: &mut AppView,
    agent_id: AgentId,
) {
    let mut removed = app.agents.shift_remove(&agent_id);
    if let Some(agent) = removed.as_mut() {
        if let Some(effect) = agent.pending_queue_release.take() {
            app.pending_effects.push(effect);
        }
        if let Some(effect) = agent.release_server_queue_edit_on_close() {
            app.pending_effects.push(effect);
        }
    }
    for agent in app.agents.values_mut() {
        if agent.session.forked_from == Some(agent_id) {
            agent.session.forked_from = None;
        }
    }
    if removed.is_some() {
        drop(removed);
        crate::memory_release::release_retained_memory_with("agent-close");
    }
}
/// Ask the runtime to close; retain the bound view until its acknowledgement.
pub(in crate::app::root::dispatch) fn dispatch_sessions_confirm_close(
    app: &mut AppView,
    closed_id: AgentId,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&closed_id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        if agent.session.state.is_busy()
            || agent.session.loading_replay
            || agent.session.prompt_history_loading
        {
            app.show_toast("Session is still starting; close after startup completes");
            return vec![];
        }
        return finish_session_close(app, closed_id);
    };
    agent
        .session
        .start_command(crate::app::session::AgentCommand::CloseSession);
    vec![Effect::CloseSession {
        agent_id: closed_id,
        session_id,
        binding_epoch: agent.session_binding_epoch,
    }]
}

pub(in crate::app::root::dispatch) fn handle_session_close_resolved(
    app: &mut AppView,
    agent_id: AgentId,
    session_id: acp::SessionId,
    binding_epoch: u32,
    result: Result<(), String>,
) -> Vec<Effect> {
    let Some(_) = app.agents.get_mut(&agent_id).filter(|agent| {
        agent.session.session_id.as_ref() == Some(&session_id)
            && agent.session_binding_epoch == binding_epoch
    }) else {
        return vec![];
    };
    if let Err(error) = result {
        app.show_toast(&format!("Couldn't close session: {error}. Retry close"));
        return vec![];
    }
    finish_session_close(app, agent_id)
}

fn finish_session_close(app: &mut AppView, closed_id: AgentId) -> Vec<Effect> {
    let session_id = app
        .agents
        .get(&closed_id)
        .and_then(|agent| agent.session.session_id.clone());
    retire_agent_view(app, closed_id);
    if let Some(d) = app.dashboard.as_mut() {
        d.set_success("Session closed");
    }
    unregister_session_effect(session_id)
}

/// Retire local ownership (drafts, children, interactions and routing) after a
/// runtime outcome. The caller owns the server outcome and registry effect.
pub(in crate::app::root::dispatch) fn retire_agent_view(app: &mut AppView, closed_id: AgentId) {
    let row = crate::views::dashboard::DashboardRowId::TopLevel(closed_id);
    let neighbor = super::super::dashboard::dashboard_neighbor_row(app, &row);
    let attached = app.dashboard.as_ref().and_then(|d| d.attached_agent) == Some(closed_id);
    let foreground = matches!(app.active_view, ActiveView::Agent(id) if id == closed_id);
    app.sync_local_drafts(std::time::Instant::now());
    if attached {
        if let Some(d) = app.dashboard.as_mut() {
            d.close_popup();
        }
        if foreground {
            app.active_view = ActiveView::AgentDashboard;
        }
    } else if foreground {
        let parent = app
            .agents
            .get(&closed_id)
            .and_then(|a| a.session.forked_from)
            .filter(|p| app.agents.contains_key(p));
        let fallback = parent.or_else(|| app.agents.keys().copied().find(|id| *id != closed_id));
        if let Some(target) = fallback {
            switch_to_agent(app, target, SwitchCause::Picker);
        } else {
            show_welcome(app);
        }
    }
    remove_agent_and_cleanup(app, closed_id);
    // Retiring the owner triggers the existing draft key boundary/flush.
    app.sync_local_drafts(std::time::Instant::now());
    if let Some(d) = app.dashboard.as_mut() {
        if d.selected.as_ref() == Some(&row) {
            match neighbor {
                Some(n) => d.focus_row(n),
                None => d.focus_new_agent_button(),
            }
        }
    }
}
/// Rename the current session via grow/session/rename.
///
/// Produces Effect::RenameSession which spawns an async ACP ext request.
/// On completion, TaskResult::RenameSessionComplete shows the result.
pub(in crate::app::root::dispatch) fn dispatch_rename_session(
    app: &mut AppView,
    title: String,
) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };
    agent.display_name = Some(title.clone());
    vec![Effect::RenameSession {
        agent_id: id,
        session_id,
        title,
        cwd: agent.session.cwd.clone(),
    }]
}

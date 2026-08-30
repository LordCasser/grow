//! Session loading, session pickers, and deep-search dispatchers.
use super::fork::build_child_fork_marker;
use super::lifecycle::dispatch_new_worktree_session;
use super::list::dispatch_fetch_session_list;
use crate::app::actions::Effect;
use crate::app::agent_view::AgentView;
use crate::app::root::AppView;
use crate::app::root::dispatch::ctx::{
    SwitchCause, get_active_agent, get_active_agent_mut, switch_to_agent, with_active_agent,
};
use crate::app::root::dispatch::modes::inherit_permission_mode;
use crate::app::root::dispatch::prompt::defer_to_open_reload_window;
use crate::app::root::dispatch::queue::{
    enqueue_behavior_control, enqueue_model_control, maybe_drain_queue, note_peek_page_flip,
    pending_control_effects,
};
use crate::app::root::dispatch::status::notify_session_ready;
use crate::app::root::dispatch::transcript::extensions_modal_tab_fetches;
use crate::app::session::{AgentCommand, AgentId, AgentSession};
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::SessionEvent;
use crate::scrollback::state::ScrollbackState;
use acp_transport::protocol as acp;

/// Reconcile and reissue each exact-session control domain after a replacement
/// ACP transport has restored authoritative state. Descendants retain their
/// own tokens and session ids; `agent_id` is the owning root used for result
/// routing and page-flip effects.
pub(crate) fn reconcile_controls_after_reconnect(
    agent_id: AgentId,
    agent: &mut AgentView,
    cli_effort_token: Option<&str>,
) -> Vec<Effect> {
    fn visit(
        agent_id: AgentId,
        view: &mut AgentView,
        cli_effort_token: Option<&str>,
        effects: &mut Vec<Effect>,
    ) {
        let Some(session_id) = view.session.session_id.clone() else {
            return;
        };
        if let Some((model_id, effort)) =
            super::lifecycle::apply_deferred_model_switch(view, cli_effort_token)
            && !view.session.has_pending_model_control(&model_id, effort)
        {
            effects.extend(enqueue_model_control(
                agent_id,
                session_id.clone(),
                &mut view.session,
                model_id,
                effort,
                true,
            ));
        }
        if let Some(mode) = view.session.deferred_session_mode {
            if !view.session.has_pending_behavior_control(mode) {
                // A matching loaded mode does not prove that the Shell's
                // interrupt-confirmation latch is clear. Keep the prompt
                // parked and reissue the exact Behavior selection; only its
                // authoritative applied update may release admission.
                effects.extend(enqueue_behavior_control(
                    agent_id,
                    session_id.clone(),
                    &mut view.session,
                    mode,
                    true,
                ));
            }
        }
        effects.extend(pending_control_effects(
            agent_id,
            session_id,
            &mut view.session,
        ));
        for child in view.subagent_views.values_mut() {
            visit(agent_id, child, cli_effort_token, effects);
        }
    }

    let mut effects = Vec::new();
    visit(agent_id, agent, cli_effort_token, &mut effects);
    effects
}
/// Create a placeholder agent and load an existing session by ID.
///
/// `session_cwd` overrides the CWD in the `LoadSessionRequest`. This is needed
/// when resuming a session that was created in a different CWD (e.g., a worktree).
pub(in crate::app::root::dispatch) fn dispatch_load_session(
    app: &mut AppView,
    session_id: String,
    session_cwd: Option<std::path::PathBuf>,
) -> Vec<Effect> {
    if !app.session_startup_allowed() {
        app.deferred_startup.session =
            Some(crate::app::session_startup::DeferredSessionStartup::Load {
                session_id,
                session_cwd,
            });
        return vec![];
    }
    dispatch_load_session_ungated(app, session_id, session_cwd)
}
/// Clear `session_id` from any existing agent that already owns the given
/// session, then return a freshly constructed [`acp::SessionId`].
///
/// Without this, `find_session_match` finds the stale agent first (IndexMap
/// insertion order) and routes all ACP notifications to it instead of the
/// new agent.
pub(in crate::app::root::dispatch) fn clear_stale_session_id(
    app: &mut AppView,
    session_id: &str,
) -> acp::SessionId {
    let sid = acp::SessionId::new(session_id);
    for agent in app.agents.values_mut() {
        if agent.session.session_id.as_ref() == Some(&sid) {
            agent.unbind_session_id();
        }
    }
    sid
}
/// If a local agent already owns this id, focus it.
///
/// - Overlay: retarget when on the dashboard list, already in overlay (attached
///   matches visible), or attached already points at the agent we will show
///   (so switch activates overlay with the correct `focus_row`).
pub(in crate::app::root::dispatch) fn focus_if_session_already_open(
    app: &mut AppView,
    session_id: &str,
) -> Option<AgentId> {
    use crate::app::root::ActiveView;
    use crate::views::dashboard::DashboardRowId;
    let existing_id = app.agents.iter().find_map(|(id, a)| {
        let sid_ok = a
            .session
            .session_id
            .as_ref()
            .is_some_and(|sid| &*sid.0 == session_id);
        if !sid_ok {
            return None;
        }
        Some(*id)
    })?;
    if let Some(agent) = app.agents.get_mut(&existing_id) {
        agent.active_subagent = None;
    }
    let retarget_overlay = match app.active_view {
        ActiveView::AgentDashboard => true,
        ActiveView::Agent(visible) => app.dashboard.as_ref().is_some_and(|d| {
            d.attached_agent == Some(visible) || d.attached_agent == Some(existing_id)
        }),
        _ => false,
    };
    if retarget_overlay && let Some(d) = app.dashboard.as_mut() {
        d.focus_row(DashboardRowId::TopLevel(existing_id));
        d.attached_agent = Some(existing_id);
    }
    switch_to_agent(app, existing_id, SwitchCause::Load);
    Some(existing_id)
}
fn dispatch_load_session_ungated(
    app: &mut AppView,
    session_id: String,
    session_cwd: Option<std::path::PathBuf>,
) -> Vec<Effect> {
    invalidate_picker_fetch_on_dismiss(app);
    if focus_if_session_already_open(app, &session_id).is_some() {
        return vec![];
    }
    let acp_session_id = clear_stale_session_id(app, &session_id);
    let control_handoff = app.screen_mode_control_handoffs.remove(&session_id);
    let agent_id = AgentId(app.next_agent_id);
    app.next_agent_id += 1;
    let mut scrollback = ScrollbackState::new();
    scrollback.set_appearance(app.appearance.clone());
    let loading_msg = if matches!(app.restore_code, Some(true)) {
        format!("Restoring code for session {}\u{2026}", &session_id)
    } else {
        format!("Loading session {}\u{2026}", &session_id)
    };
    let agent = AgentView::new(
        {
            let mut session = AgentSession::new(
                agent_id,
                app.acp_tx.clone(),
                Some(acp_session_id),
                app.models.clone(),
                session_cwd.clone().unwrap_or_else(|| app.cwd.clone()),
                inherit_permission_mode(app),
            );
            if let Some(handoff) = control_handoff {
                session.restore_screen_mode_control_handoff(handoff);
            }
            session.begin_replay();
            session.available_commands = app.bootstrap_acp_commands.clone();
            session.available_commands_generation = 1;
            session.deferred_model_switch = app.deferred_model_switch_from_cli();
            session
        },
        scrollback,
    );
    app.agents.insert(agent_id, agent);
    let agent_mut = app.agents.get_mut(&agent_id).unwrap();
    agent_mut.session.attached_as_viewer = true;
    agent_mut.begin_replay_window();
    agent_mut.session.set_live_feedback(
        "session-load",
        crate::scrollback::blocks::NoticeTone::Progress,
        loading_msg,
    );
    agent_mut.prompt.set_compact(app.appearance.prompt.compact);
    agent_mut.prompt.adopt_slash_mru(app.slash_mru.clone());
    agent_mut
        .prompt
        .adopt_command_tags(app.command_tags.clone());
    agent_mut
        .prompt
        .set_contextual_hints(app.contextual_hints.undo, app.contextual_hints.plan_mode);
    agent_mut.set_session_recap_available(app.session_recap_available);
    agent_mut.scrollback.begin_batch();
    if matches!(app.restore_code, Some(true)) {
        agent_mut.session.start_command(AgentCommand::RestoreCode);
        agent_mut.session.turn_started_at = Some(std::time::Instant::now());
    }
    agent_mut.apply_app_scoped_gates(app.screen_mode, &app.active_announcements);

    agent_mut
        .prompt
        .slash_controller
        .registry_mut()
        .set_plugins_visible(!app.appearance.disable_plugins);
    app.mark_project_picker_done();
    switch_to_agent(app, agent_id, SwitchCause::Load);
    vec![Effect::LoadSession {
        agent_id,
        session_id,
        session_cwd,
    }]
}
/// Load the session selected in the session picker.
pub(in crate::app::root::dispatch) fn dispatch_pick_session(
    app: &mut AppView,
    index: usize,
) -> Vec<Effect> {
    use crate::views::modal::ActiveModal;
    let mut picker_dismissed = false;
    let entry_data = if let Some(agent) = get_active_agent_mut(app) {
        if let Some(ActiveModal::SessionPicker { entries, .. }) = agent.active_modal.as_mut() {
            let data = entries
                .as_ref()
                .and_then(|s| s.get(index))
                .map(|e| e.id.clone());
            agent.active_modal = None;
            picker_dismissed = true;
            data
        } else {
            None
        }
    } else {
        None
    };
    if picker_dismissed {
        invalidate_picker_fetch_on_dismiss(app);
    }
    let session_id = match entry_data {
        Some(d) => d,
        None => {
            let sessions = match app.session_picker_entries.take() {
                Some(s) => s,
                None => return vec![],
            };
            if !picker_dismissed {
                invalidate_picker_fetch_on_dismiss(app);
            }
            let entry = match sessions.get(index) {
                Some(e) => e,
                None => return vec![],
            };
            let id = entry.id.clone();
            app.session_picker_loading = false;
            app.session_picker_state.set_query("");
            app.session_picker_state.search_active = false;
            app.session_picker_state.expanded.clear();
            app.session_picker_content_results = None;
            app.session_picker_content_loading = false;
            id
        }
    };
    let local_cwd = app.cwd.to_string_lossy().to_string();
    if shell::session::resolve_local_session(&session_id, &local_cwd).is_some() {
        return dispatch_load_session(app, session_id, None);
    }
    if let Some(original_cwd) = shell::session::resolve_local_session_any_cwd(&session_id) {
        return dispatch_load_session(
            app,
            session_id,
            Some(std::path::PathBuf::from(original_cwd)),
        );
    }
    app.show_toast("Session not found locally");
    vec![]
}
/// Pick a session from the picker and resume it in a new git worktree.
pub(in crate::app::root::dispatch) fn dispatch_pick_session_in_worktree(
    app: &mut AppView,
    index: usize,
) -> Vec<Effect> {
    use crate::views::modal::ActiveModal;
    let mut picker_dismissed = false;
    let entry_data = if let Some(agent) = get_active_agent_mut(app) {
        if let Some(ActiveModal::SessionPicker { entries, .. }) = agent.active_modal.as_mut() {
            let data = entries
                .as_ref()
                .and_then(|s| s.get(index))
                .map(|e| e.id.clone());
            agent.active_modal = None;
            picker_dismissed = true;
            data
        } else {
            None
        }
    } else {
        None
    };
    if picker_dismissed {
        invalidate_picker_fetch_on_dismiss(app);
    }
    let session_id = match entry_data {
        Some(d) => d,
        None => {
            let sessions = match app.session_picker_entries.take() {
                Some(s) => s,
                None => return vec![],
            };
            if !picker_dismissed {
                invalidate_picker_fetch_on_dismiss(app);
            }
            let entry = match sessions.get(index) {
                Some(e) => e,
                None => return vec![],
            };
            let id = entry.id.clone();
            app.session_picker_loading = false;
            app.session_picker_state.set_query("");
            app.session_picker_state.search_active = false;
            app.session_picker_state.expanded.clear();
            id
        }
    };
    dispatch_new_worktree_session(app, Some(session_id), None, None, None, None, None)
}
fn keep_picker_entry(entry: &crate::app::root::SessionPickerEntry, session_id: &str) -> bool {
    entry.id != session_id
}
/// Remove a deleted session identity from the modal session picker and the
/// welcome-screen picker, then re-anchor the selection on a real row.
///
/// Called after [`crate::app::actions::TaskResult::DeleteSessionComplete`] so
/// the just-deleted entry vanishes from the open list without a full refetch.
pub(in crate::app::root::dispatch) fn remove_session_from_pickers(
    app: &mut AppView,
    session_id: &str,
) {
    use crate::views::modal::ActiveModal;
    use crate::views::session_picker::build_entry_map;
    app.session_picker_detail_generation += 1;
    if let Some(agent) = get_active_agent_mut(app)
        && let Some(ActiveModal::SessionPicker {
            entries,
            content_results,
            state,
            content_loading,
            entries_query,
            pending_delete,
            ..
        }) = agent.active_modal.as_mut()
    {
        if pending_delete
            .as_ref()
            .is_some_and(|pd| pd.session_id == session_id)
        {
            *pending_delete = None;
        }
        if let Some(list) = entries.as_mut() {
            list.retain(|entry| keep_picker_entry(entry, session_id));
        }
        if let Some(hits) = content_results.as_mut() {
            hits.retain(|h| h.session_id != session_id);
        }
        let current_repo =
            crate::views::session_picker::repo_name_from_cwd(&agent.session.cwd.to_string_lossy());
        let map = build_entry_map(
            entries.as_deref(),
            content_results.as_deref(),
            crate::views::session_picker::effective_filter_query(
                state.query(),
                entries_query.as_deref(),
            ),
            true,
            *content_loading,
            Some(current_repo.as_str()),
        );
        reanchor_grouped_selection(state, &map);
    }
    if let Some(list) = app.session_picker_entries.as_mut() {
        list.retain(|entry| keep_picker_entry(entry, session_id));
    }
    if let Some(hits) = app.session_picker_content_results.as_mut() {
        hits.retain(|h| h.session_id != session_id);
    }
    let welcome_current_repo =
        crate::views::session_picker::repo_name_from_cwd(&app.cwd.to_string_lossy());
    let welcome_map = build_entry_map(
        app.session_picker_entries.as_deref(),
        app.session_picker_content_results.as_deref(),
        crate::views::session_picker::effective_filter_query(
            app.session_picker_state.query(),
            app.session_picker_entries_query.as_deref(),
        ),
        app.session_picker_grouped,
        app.session_picker_content_loading,
        Some(welcome_current_repo.as_str()),
    );
    reanchor_grouped_selection(&mut app.session_picker_state, &welcome_map);
}
/// Clamp `state.selected` to a selectable slot in a grouped picker `map`
/// (`Some` = selectable row, `None` = non-selectable header).
pub(in crate::app::root::dispatch) fn reanchor_grouped_selection<T>(
    state: &mut crate::views::picker::PickerState,
    map: &[Option<T>],
) {
    state.scroll_offset = None;
    if map.is_empty() {
        state.selected = 0;
        return;
    }
    let mut sel = state.selected.min(map.len() - 1);
    while sel > 0 && map[sel].is_none() {
        sel -= 1;
    }
    if map[sel].is_none() {
        sel = map.iter().position(|e| e.is_some()).unwrap_or(0);
    }
    state.selected = sel;
}
/// Trigger a deep content search when the session picker query changes.
///
/// Any query of 2+ chars searches content — title matches never suppress
/// it. Forced (Ctrl+/) searches fire immediately; keystrokes otherwise
/// coalesce through [`Effect::DebounceSessionSearch`], whose expiry runs
/// the search only if its seq is still current. Shorter queries clear the
/// content results.
///
/// Checks the active agent's modal first; if no modal session picker
/// exists, falls back to the welcome-screen picker state.
pub(in crate::app::root::dispatch) fn dispatch_trigger_deep_search(
    app: &mut AppView,
    force: bool,
) -> Vec<Effect> {
    use crate::views::modal::ActiveModal;
    if let Some(agent) = get_active_agent_mut(app)
        && let Some(ActiveModal::SessionPicker {
            state,
            content_results,
            content_loading,
            deep_search_seq,
            ..
        }) = agent.active_modal.as_mut()
    {
        let query = state.query().trim().to_string();
        *deep_search_seq += 1;
        let seq = *deep_search_seq;
        if query.len() < 2 {
            *content_results = None;
            *content_loading = false;
            return vec![];
        }
        *content_loading = true;
        if force {
            return vec![Effect::DeepSearchSessions { query, seq }];
        }
        return vec![Effect::DebounceSessionSearch { query, seq }];
    }
    let query = app.session_picker_state.query().trim().to_string();
    app.session_picker_deep_search_seq += 1;
    let seq = app.session_picker_deep_search_seq;
    if query.len() < 2 {
        app.session_picker_content_results = None;
        app.session_picker_content_loading = false;
        return vec![];
    }
    app.session_picker_content_loading = true;
    if force {
        vec![Effect::DeepSearchSessions { query, seq }]
    } else {
        vec![Effect::DebounceSessionSearch { query, seq }]
    }
}
pub(in crate::app::root::dispatch) fn session_picker_entry_matches(
    app: &AppView,
    session_id: &str,
) -> bool {
    use crate::views::modal::ActiveModal;
    if let Some(agent) = get_active_agent(app)
        && let Some(ActiveModal::SessionPicker {
            entries,
            content_results,
            ..
        }) = agent.active_modal.as_ref()
    {
        return entries
            .as_ref()
            .is_some_and(|entries| entries.iter().any(|entry| entry.id == session_id))
            || content_results
                .as_ref()
                .is_some_and(|results| results.iter().any(|hit| hit.session_id == session_id));
    }
    app.session_picker_entries
        .as_ref()
        .is_some_and(|entries| entries.iter().any(|entry| entry.id == session_id))
        || app
            .session_picker_content_results
            .as_ref()
            .is_some_and(|results| results.iter().any(|hit| hit.session_id == session_id))
}
/// Pick a session from deep content search results.
pub(in crate::app::root::dispatch) fn dispatch_pick_content_session(
    app: &mut AppView,
    session_id: String,
    _cwd: String,
) -> Vec<Effect> {
    app.session_picker_entries = None;
    app.session_picker_loading = false;
    app.session_picker_state.reset();
    app.session_picker_content_results = None;
    app.session_picker_content_loading = false;
    invalidate_picker_fetch_on_dismiss(app);
    let local_cwd = app.cwd.to_string_lossy().to_string();
    if shell::session::resolve_local_session(&session_id, &local_cwd).is_some() {
        return dispatch_load_session(app, session_id, None);
    }
    if let Some(original_cwd) = shell::session::resolve_local_session_any_cwd(&session_id) {
        return dispatch_load_session(
            app,
            session_id,
            Some(std::path::PathBuf::from(original_cwd)),
        );
    }
    if focus_if_session_already_open(app, &session_id).is_some() {
        return vec![];
    }
    app.show_toast("Session not found locally");
    vec![]
}
pub(in crate::app::root::dispatch) fn handle_session_loaded(
    app: &mut AppView,
    agent_id: AgentId,
    session_id: acp::SessionId,
    new_models: Option<shell::agent::models::SessionModelState>,
    code_restored: bool,
    restore_summary: Option<String>,
    restore_degree: Option<workspace::session::git::RestoreDegree>,
    foreground: Option<crate::app::prompt_queue::ForegroundSnapshot>,
) -> Vec<Effect> {
    tracing::info!(
        "Session loaded for agent {:?} session {:?}",
        agent_id,
        session_id,
    );
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        if defer_to_open_reload_window(agent, agent_id, "SessionLoaded") {
            return vec![];
        }
        let hydrate_sid = session_id.clone();
        agent.bind_session_id(session_id);
        agent.session.clear_live_feedback("session-load");
        agent.scrollback.end_batch();
        agent.session.loading_replay = false;
        agent.session.replay_live_cursor_seen = false;
        agent.session.restore_degree = restore_degree;
        agent.session.finish_turn(&mut agent.scrollback);
        agent.mark_turn_finished();
        if let Some(m) = new_models {
            agent.session.models = Some(m).into();
        }
        match (code_restored, restore_summary.as_deref()) {
            (true, Some(s)) => {
                agent
                    .scrollback
                    .push_block(RenderBlock::notice(format!("\u{2713} Code restored: {s}")));
            }
            (false, Some(s)) => {
                agent.scrollback.push_block(RenderBlock::notice(format!(
                    "\u{26A0} Code restore failed: {s}"
                )));
            }
            _ => {}
        }
        if let Some(info) = agent.pending_fork_banner.take() {
            let sid = agent
                .session
                .session_id
                .as_ref()
                .map(|s| s.0.as_ref())
                .unwrap_or("???");
            let banner = build_child_fork_marker(
                sid,
                &info.parent_sid,
                info.worktree,
                crate::views::dashboard::session_switch_hint_command(app.screen_mode.is_minimal()),
            );
            agent.scrollback.push_block(RenderBlock::notice(banner));
        }
        let running_prompt_id = foreground.map(|snapshot| snapshot.prompt_id);
        let adopting = running_prompt_id
            .as_deref()
            .is_some_and(|pid| agent.should_adopt_running_prompt(pid));
        let preserve = running_prompt_id.as_deref().filter(|_| adopting);
        agent.reset_follow_ups_for_reload_preserving(preserve);
        if adopting && let Some(running_pid) = running_prompt_id {
            agent.adopt_running_prompt(running_pid);
        } else {
            agent.scrollback.finish_all_running();
            for child in agent.subagent_views.values_mut() {
                child.scrollback.finish_all_running();
            }
        }
        let control_effects =
            reconcile_controls_after_reconnect(agent_id, agent, app.cli_effort_token.as_deref());
        let mut effects = Vec::new();
        if let Some(directive) = agent.pending_first_prompt.take() {
            agent.session.enqueue_prompt_front(directive);
        }
        let drain = maybe_drain_queue(agent);
        let page_flip_entry = drain.page_flip_entry;
        effects.extend(drain.effects);
        let cwd = agent.session.cwd.clone();
        effects.push(Effect::HydrateSessionTitleFromDisk {
            agent_id,
            session_id: hydrate_sid.clone(),
            cwd: cwd.clone(),
        });
        agent.session.prompt_history_loading = true;
        effects.push(Effect::FetchPromptHistory {
            agent_id,
            cwd,
            session_id: hydrate_sid.to_string(),
        });
        let revision = agent.session.begin_agent_metadata_read();
        effects.push(Effect::FetchSessionAgentName {
            agent_id,
            session_id: hydrate_sid.clone(),
            revision,
        });
        if app.plugin_cta_enabled {
            effects.push(Effect::FetchPluginCtaCatalog {
                agent_id,
                session_id: hydrate_sid.clone(),
            });
        }
        effects.extend(control_effects);
        if agent.session.take_pending_extensions_fetch()
            && let Some(modal) = agent.extensions_modal.as_mut()
        {
            effects.extend(extensions_modal_tab_fetches(
                modal,
                agent_id,
                hydrate_sid.clone(),
            ));
        }
        effects.push(Effect::RegisterActiveSession {
            session_id: hydrate_sid,
            cwd: agent.session.cwd.display().to_string(),
        });
        notify_session_ready(&app.notification_service, agent);
        crate::memory_release::release_retained_memory_with("session-load-replay");
        note_peek_page_flip(app, agent_id, page_flip_entry);
        crate::app::subagent::restore_descendant_state(app, agent_id);
        return effects;
    }
    vec![]
}
pub(in crate::app::root::dispatch) fn handle_session_load_failed(
    app: &mut AppView,
    agent_id: AgentId,
    session_id: acp::SessionId,
    error: String,
) -> Vec<Effect> {
    tracing::error!(agent = ?agent_id, session = ?session_id, error = %error, "Session load failed");
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        if defer_to_open_reload_window(agent, agent_id, "SessionLoadFailed") {
            return vec![];
        }
        agent.session.clear_pending_extensions_fetch();
        agent.session.clear_live_feedback("session-load");
        agent.session.prompt_history_loading = false;
        agent.session.finish_command();
        agent.mark_turn_finished();
        agent.scrollback.end_batch();
        agent.session.loading_replay = false;
        agent.session.replay_live_cursor_seen = false;
        agent.pending_first_prompt = None;
        agent.pending_fork_banner = None;
        // A failed load is not an open session. Release the eager identity so
        // selecting the same session retries instead of focusing this error
        // surface; the failure Notice remains attached to this view.
        agent.unbind_session_id();
        agent
            .scrollback
            .push_block(RenderBlock::session_event(SessionEvent::TurnFailed {
                error: format!("Couldn't load session: {error}"),
                elapsed: None,
            }));
    }
    vec![]
}
pub(in crate::app::root::dispatch) fn handle_session_search_debounce_expired(
    app: &mut AppView,
    query: String,
    seq: u64,
) -> Vec<Effect> {
    if live_deep_search_seq(app) != Some(seq) {
        return vec![];
    }
    vec![Effect::DeepSearchSessions { query, seq }]
}
/// The deep-search seq of the surface that can still consume results: an
/// open modal SessionPicker (its own counter), else the welcome-screen
/// picker only while the welcome view is showing. `None` when neither
/// surface is live — dismissing a modal bumps the WELCOME counter, which
/// can collide with (not invalidate) a modal-armed seq, so those expiries
/// are dropped by liveness rather than counter arithmetic.
fn live_deep_search_seq(app: &AppView) -> Option<u64> {
    use crate::views::modal::ActiveModal;
    if let Some(agent) = get_active_agent(app)
        && let Some(ActiveModal::SessionPicker {
            deep_search_seq, ..
        }) = agent.active_modal.as_ref()
    {
        return Some(*deep_search_seq);
    }
    matches!(app.active_view, crate::app::root::ActiveView::Welcome)
        .then_some(app.session_picker_deep_search_seq)
}
pub(in crate::app::root::dispatch) fn handle_card_detail_loaded(
    app: &mut AppView,
    session_id: String,
    generation: u64,
    detail: crate::app::root::CardDetail,
) -> Vec<Effect> {
    use crate::views::modal::ActiveModal;
    if generation != app.session_picker_detail_generation {
        return vec![];
    }
    if let Some(agent) = get_active_agent_mut(app)
        && let Some(ActiveModal::SessionPicker { entries, .. }) = agent.active_modal.as_mut()
    {
        if let Some(entry) = entries
            .as_mut()
            .and_then(|sessions| sessions.iter_mut().find(|entry| entry.id == session_id))
        {
            entry.card_detail = Some(detail);
        }
        return vec![];
    }
    if let Some(ref mut sessions) = app.session_picker_entries
        && let Some(entry) = sessions.iter_mut().find(|entry| entry.id == session_id)
    {
        entry.card_detail = Some(detail);
    }
    vec![]
}
pub(in crate::app::root::dispatch) fn handle_deep_search_results(
    app: &mut AppView,
    results: Vec<shell::extensions::session_search::SearchSessionHit>,
    seq: u64,
) -> Vec<Effect> {
    use crate::views::modal::ActiveModal;
    if let Some(agent) = get_active_agent_mut(app)
        && let Some(ActiveModal::SessionPicker {
            content_results,
            content_loading,
            deep_search_seq,
            ..
        }) = agent.active_modal.as_mut()
    {
        if seq == *deep_search_seq {
            *content_results = Some(results);
            *content_loading = false;
        }
        return vec![];
    }
    if seq == app.session_picker_deep_search_seq {
        app.session_picker_content_results = Some(results);
        app.session_picker_content_loading = false;
    }
    vec![]
}
pub(in crate::app::root::dispatch) fn dispatch_show_session_picker(
    app: &mut AppView,
) -> Vec<Effect> {
    use crate::views::modal::ActiveModal;
    with_active_agent(app, |agent| {
        agent.active_modal = Some(ActiveModal::SessionPicker {
            state: crate::views::picker::PickerState::default(),
            entries: None,
            loading: true,
            previous_palette: None,
            window: crate::views::modal_window::ModalWindowState::new(),
            content_results: None,
            content_loading: false,
            deep_search_seq: 0,
            entries_query: None,
            pending_delete: None,
        });
    });
    dispatch_fetch_session_list(app)
}
/// The picker (modal `/resume` or welcome screen) was dismissed without a
/// pick. Its own fields die with it, but a still-current in-flight
/// list/search fetch would fall through to the welcome picker fields in
/// `handle_session_list_loaded`, stamping them with a query the welcome
/// search box never had — or repopulating a picker the user just closed.
/// Invalidate it (same seq idiom as `dispatch_fetch_session_list`).
pub(in crate::app::root::dispatch) fn dispatch_session_picker_closed(
    app: &mut AppView,
) -> Vec<Effect> {
    invalidate_picker_fetch_on_dismiss(app);
    vec![]
}
/// Fetch invalidation shared by EVERY picker-dismissal path:
/// modal Esc/mouse close, modal and welcome picks (all variants), and the
/// welcome-screen Esc. A modal close must not invalidate the welcome screen's
/// plain list fetch. A welcome dismissal must bump
/// and drop the loading flag: the welcome view survives the close, so a
/// still-loading flag holds `show_picker` in a spinner limbo that ignores
/// input until the late response lands and resurrects the picker.
fn invalidate_picker_fetch_on_dismiss(app: &mut AppView) {
    let welcome_dismissal = matches!(app.active_view, crate::app::root::ActiveView::Welcome);
    if welcome_dismissal {
        app.session_picker_list_seq += 1;
    }
    if welcome_dismissal {
        app.session_picker_loading = false;
    }
    app.session_picker_deep_search_seq += 1;
    app.session_picker_content_loading = false;
}
pub(in crate::app::root::dispatch) fn dispatch_pick_content_session_in_worktree(
    app: &mut AppView,
    session_id: String,
    _: String,
) -> Vec<Effect> {
    app.session_picker_entries = None;
    app.session_picker_loading = false;
    app.session_picker_state.reset();
    app.session_picker_content_results = None;
    app.session_picker_content_loading = false;
    if let Some(agent) = get_active_agent_mut(app) {
        agent.active_modal = None;
    }
    invalidate_picker_fetch_on_dismiss(app);
    dispatch_new_worktree_session(app, Some(session_id), None, None, None, None, None)
}

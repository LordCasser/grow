//! Dashboard dispatchers: attach, overlays, rows, renames, and permissions.

use super::ctx::{show_welcome, surface_screen_mode_switch_hint};
use super::dashboard_diagnostics::{
    log_dashboard_attached, log_dashboard_closed, log_dashboard_launched, log_dashboard_opened,
};
use super::router::dispatch;
use super::session::fork::open_project_question_with_context;
use super::session::lifecycle::{
    dispatch_new_session_inner_with_id, dispatch_new_worktree_session,
};
use super::session::load::dispatch_load_session;
use super::session::load::focus_if_session_already_open;
use super::session::modal::dispatch_sessions_confirm_close;
use crate::app::actions::{Action, Effect};
use crate::app::agent_view::AgentView;
use crate::app::root::{ActiveView, AppView, DashboardReturn, TrustState};
use crate::app::session::AgentId;

// ---------------------------------------------------------------------------
// Agent Dashboard dispatchers
// ---------------------------------------------------------------------------

/// Build a `DashboardState` from persisted layout (pins, reorder, and
/// grouping), loading and caching `app.dashboard_persisted` on first use.
/// Used to materialize the overview before it has been opened.
fn dashboard_state_from_persisted(app: &mut AppView) -> crate::views::dashboard::DashboardState {
    use crate::views::dashboard::{DashboardState, load_persisted};
    if app.dashboard_persisted.is_none() {
        app.dashboard_persisted = load_persisted();
    }
    let persisted = app
        .dashboard_persisted
        .clone()
        .unwrap_or_else(crate::views::dashboard::PersistedDashboard::defaults);
    let resolver = crate::views::dashboard::SessionIdResolver::from_agents(&app.agents);
    DashboardState::from_persisted(&persisted, &resolver)
}

pub(super) fn ensure_dashboard_state(app: &mut AppView) {
    if app.dashboard.is_some() {
        return;
    }
    let mut state = dashboard_state_from_persisted(app);
    state.gc_stale_refs(&dashboard_alive_fn(&app.agents));
    state.adopt_slash_mru(app.slash_mru.clone());
    state.adopt_command_tags(app.command_tags.clone());
    state.set_screen_mode(app.screen_mode);
    state.set_recap_visible(app.session_recap_available);
    app.dashboard = Some(state);
}

/// Configure the dashboard for display: snapshot app-wide state (cwd, models,
/// plugins, and persistent defaults) and reset per-session staging. Shared by
/// `dispatch_open_dashboard` and the overlay-cycle path; no-op if unallocated.
fn configure_dashboard_state(app: &mut AppView) {
    let bootstrap_commands = app.bootstrap_acp_commands.clone();
    let models = app.models.clone();
    let disable_plugins = app.appearance.disable_plugins;
    let pending_permission = crate::app::actions::PermissionModeKind::from_runtime(
        super::modes::inherit_permission_mode(app),
    );
    let cwd = app.cwd.clone();
    let cwd_has_git_ancestor = app.cwd_has_git_ancestor;
    let has_agents = !app.agents.is_empty();
    if let Some(d) = app.dashboard.as_mut() {
        d.close_popup();
        d.location_picker = None;
        d.cwd = cwd.clone();
        d.cwd_has_git_ancestor = cwd_has_git_ancestor;
        d.dispatch_worktree = false;
        d.worktree_dialog = None;
        d.pending_worktree_prompt = None;
        d.pending_worktree_attach = false;
        d.focus_new_agent_button();
        d.list_focused = has_agents;
        d.dispatch.file_search.retarget(&cwd);
        // Tool gating disabled (None): the dashboard has no agent toolset.
        d.dispatch
            .slash_controller
            .registry_mut()
            .set_plugins_visible(!disable_plugins);
        d.dispatch
            .sync_acp_commands(&bootstrap_commands, None, &models);
        // There is no session tool snapshot on the dashboard. Keep Workflow
        // visible during this unknown interval; the session is the authority
        // that validates the runtime gate once it is created.
        d.dispatch.slash_controller.set_workflows_available(true);
        d.models = models;
        d.pending_model = None;
        d.pending_behavior = tools::types::BehaviorId::Normal;
        d.pending_permission = pending_permission;
        d.dispatch.slash_controller.set_selection_context(
            None,
            d.pending_behavior,
            false,
            None,
            app.auto_mode_gate,
            d.pending_permission.as_canonical(),
        );
    }
}

/// Open the dashboard view. Respects the
/// [`crate::views::dashboard::dashboard_enabled`] feature flag (env var
/// override + persisted setting). The dashboard is independent of leader
/// mode: it renders local sessions from `app.agents` and, when connected
/// via a leader, additionally polls the leader roster (see the roster-poll
/// gate in the event loop).
pub(super) fn dispatch_open_dashboard(app: &mut AppView) -> Vec<Effect> {
    use crate::views::dashboard::dashboard_enabled;

    if !dashboard_enabled() {
        app.show_toast("Agent dashboard is disabled in this configuration");
        return vec![];
    }
    // Same rationale for folder trust: opening the dashboard would visually
    // dismiss the trust question with the folder still unanswered. Toast and
    // stay put (mirrors the auth gate above) so the question is resolved first.
    if matches!(app.trust_state, TrustState::Pending { .. }) {
        app.show_toast("Answer the folder-trust question to open the dashboard");
        return vec![];
    }
    // Edge case 24: idempotent toggle — opening from the dashboard view
    // itself just closes.
    //
    // Ctrl+\ opens the overview from session detail and exits it back
    // to the prior view when the overview is already active.
    if matches!(app.active_view, ActiveView::AgentDashboard) {
        return dispatch_exit_dashboard(app);
    }
    clear_session_commands(app);
    // Stamp return target for this visit (clears any prior leftover).
    app.dashboard_return = match app.active_view {
        ActiveView::Agent(id) => Some(DashboardReturn::Agent(id)),
        _ => None,
    };
    // Preserve in-memory state across reopen.
    // `app.dashboard.is_some()` means we've previously initialised
    // it; preserve the user's filter / dispatch text / hover /
    // selection. Otherwise seed from persisted state.
    if app.dashboard.is_none() {
        ensure_dashboard_state(app);
    } else if let Some(d) = app.dashboard.as_mut() {
        // Subsequent reopen — just gc dead ids; in-memory state stays.
        d.gc_stale_refs(&dashboard_alive_fn(&app.agents));
        d.set_recap_visible(app.session_recap_available);
    }
    // Refresh each local agent's git context (branch / worktree / label)
    // from disk so the row subtitles show the LATEST branch and worktree
    // name, not a value that may have gone stale since the last
    // `git_head_changed` notification. Runs on open (a deliberate user
    // action), never per frame; `compute_cwd_git_info` returns `None` for
    // non-repo cwds so existing values are left intact rather than cleared.
    let agent_cwds: Vec<(AgentId, std::path::PathBuf)> = app
        .agents
        .iter()
        .map(|(id, a)| (*id, a.session.cwd.clone()))
        .collect();
    for (id, cwd) in agent_cwds {
        if let Some(info) = crate::git_info::compute_cwd_git_info(&cwd)
            && let Some(agent) = app.agents.get_mut(&id)
        {
            agent.current_branch = info.branch;
            agent.is_worktree = info.is_worktree;
            agent.main_repo = info.main_repo;
            agent.worktree_label = info.worktree_label;
        }
    }
    // The overview owns the new-session composer. Row selection is its
    // navigation cursor and has strong focus only while the list is focused;
    // Enter or click opens the selected session's ordinary detail view.
    configure_dashboard_state(app);
    app.active_view = ActiveView::AgentDashboard;
    log_dashboard_opened(app);
    app.dashboard_sessions_loading = true;
    if app.leader_mode {
        return vec![Effect::FetchRoster];
    }
    vec![Effect::FetchDashboardSessions]
}

/// Helper: produce a closure that answers "does this DashboardRowId
/// still exist in `agents`?". Static lifetime not possible (closures
/// borrow), so callers pass `&app.agents`.
fn dashboard_alive_fn(
    agents: &indexmap::IndexMap<AgentId, AgentView>,
) -> impl Fn(&crate::views::dashboard::DashboardRowId) -> bool + '_ {
    move |id| match id {
        crate::views::dashboard::DashboardRowId::TopLevel(a) => agents.contains_key(a),
        crate::views::dashboard::DashboardRowId::Subagent {
            parent,
            child_session_id,
        } => agents
            .get(parent)
            .is_some_and(|a| a.session.subagent_sessions.contains_key(child_session_id)),
        // Roster-only rows are not locally hosted; they aren't tracked by
        // `agents` and are never persisted, so treat them as not alive for
        // pinned/reorder GC purposes.
        crate::views::dashboard::DashboardRowId::Roster { .. } => false,
    }
}

pub(super) fn dispatch_exit_dashboard(app: &mut AppView) -> Vec<Effect> {
    // Clear the attached-session marker and restore its viewport before
    // leaving the overview.
    if let Some(d) = app.dashboard.as_mut() {
        d.restore_peek_viewport(&mut app.agents);
        d.close_popup();
    }
    log_dashboard_closed(app);
    let preferred = app
        .dashboard_return
        .take()
        .filter(|t| app.agents.contains_key(&t.agent_id()));
    // Overlay chrome only when the preferred target is still alive — never
    // on the insertion-order fallback after the return agent was closed.
    let (return_id, rearm_overlay) = match preferred {
        Some(t) => (Some(t.agent_id()), t.is_overlay()),
        None => (app.agents.keys().next().copied(), false),
    };
    if let Some(id) = return_id {
        app.active_view = ActiveView::Agent(id);
        if rearm_overlay {
            rearm_session_overlay(app, id);
        }
        surface_screen_mode_switch_hint(app, id);
    } else {
        show_welcome(app);
    }
    vec![]
}

/// Restore session-overlay chrome (`attached_agent` + row cursor).
/// Keeps a live subagent takeover; otherwise clears it and selects TopLevel.
fn rearm_session_overlay(app: &mut AppView, id: AgentId) {
    use crate::views::dashboard::DashboardRowId;
    let live_child = app.agents.get(&id).and_then(|a| {
        a.active_subagent
            .as_ref()
            .filter(|c| a.session.subagent_sessions.contains_key(*c))
            .cloned()
    });
    let row = match live_child {
        Some(child_session_id) => DashboardRowId::Subagent {
            parent: id,
            child_session_id,
        },
        None => {
            if let Some(agent) = app.agents.get_mut(&id) {
                agent.clear_session_commands();
                agent.active_subagent = None;
            }
            DashboardRowId::TopLevel(id)
        }
    };
    if let Some(d) = app.dashboard.as_mut() {
        d.focus_row(row);
        d.attached_agent = Some(id);
    }
}

pub(super) fn dispatch_dashboard_attach(
    app: &mut AppView,
    id: crate::views::dashboard::DashboardRowId,
) -> Vec<Effect> {
    use crate::views::dashboard::DashboardRowId;
    // Attach is a fullscreen view switch AND signals
    // "session-overlay mode" via `attached_agent`. The agent view
    // takes the full screen and the renderer wraps it in a bordered
    // frame with `[‹]` / `[›]` cycle and `[Dashboard]` return controls
    // (mirrors the subagent fullscreen takeover). Input goes
    // straight to the agent because `active_view = Agent(id)`, so
    // Enter/Ctrl+R/etc. all work as in any regular agent view.
    //
    // Keep stop-action state scoped to the previously attached session.
    clear_session_commands(app);
    if let Some(d) = app.dashboard.as_mut() {
        d.restore_peek_viewport(&mut app.agents);
    }
    match id {
        DashboardRowId::TopLevel(agent_id) => {
            if !app.agents.contains_key(&agent_id) {
                if let Some(d) = app.dashboard.as_mut() {
                    d.set_error("Session no longer exists");
                }
                return vec![];
            }
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                // Drop any prior subagent takeover so the agent
                // view paints the parent, not whatever subagent
                // the user last opened fullscreen.
                agent.clear_session_commands();
                agent.active_subagent = None;
            }
            if let Some(d) = app.dashboard.as_mut() {
                // `focus_row` (not a bare `selected` assignment) so the
                // section cursor / button focus are cleared — exactly one
                // cursor target stays active.
                d.focus_row(DashboardRowId::TopLevel(agent_id));
                // Signal session-overlay mode: render the agent
                // wrapped with `[‹]` / `[›]` cycle and `[Dashboard]`
                // return controls in the title bar.
                d.attached_agent = Some(agent_id);
            }
            app.active_view = ActiveView::Agent(agent_id);
            log_dashboard_attached(&DashboardRowId::TopLevel(agent_id));
            surface_screen_mode_switch_hint(app, agent_id);
        }
        DashboardRowId::Subagent {
            parent,
            child_session_id,
        } => {
            // If the subagent has finished/disappeared
            // between row build and attach, toast and stay put rather
            // than silently switching to the parent.
            let alive = app
                .agents
                .get(&parent)
                .is_some_and(|a| a.session.subagent_sessions.contains_key(&child_session_id));
            if !alive {
                if let Some(d) = app.dashboard.as_mut() {
                    d.set_warning("Subagent no longer running");
                }
                return vec![];
            }
            if let Some(agent) = app.agents.get_mut(&parent) {
                agent.open_subagent_fullscreen(child_session_id.clone());
            }
            let row_id = DashboardRowId::Subagent {
                parent,
                child_session_id,
            };
            if let Some(d) = app.dashboard.as_mut() {
                d.focus_row(row_id.clone());
                d.attached_agent = Some(parent);
            }
            app.active_view = ActiveView::Agent(parent);
            log_dashboard_attached(&row_id);
            surface_screen_mode_switch_hint(app, parent);
        }
        DashboardRowId::Roster { session_id } => {
            // A roster row is a leader-hosted session this client is not
            // locally attached to. Attaching issues an ACP `session/load`,
            // which the leader join-not-steal-subscribes: it adds this
            // client to the session's subscriber set (replay + live
            // broadcast) instead of stealing ownership. Mirror the session
            // picker's strict resume path — inserting a
            // leader-hosted, possibly cross-cwd session into THIS cwd's
            // recent list would be wrong.

            let session_cwd = app
                .leader_roster
                .iter()
                .chain(app.dashboard_local_sessions.iter())
                .find(|e| e.session_id == session_id)
                .map(|e| std::path::PathBuf::from(&e.cwd));

            // Already local (e.g. double-click after the row converted): focus only.
            if let Some(existing_id) = focus_if_session_already_open(app, session_id.as_str()) {
                log_dashboard_attached(&DashboardRowId::TopLevel(existing_id));
                return vec![];
            }

            // Mirror the picker resume path: allocate a new local agent and
            // emit `Effect::LoadSession` (strict load). `dispatch_load_session`
            // already sets `app.active_view` to the new agent.
            let effects = dispatch_load_session(app, session_id, session_cwd);
            if let Some(new_id) = effects.iter().find_map(|e| match e {
                Effect::LoadSession { agent_id, .. } => Some(*agent_id),
                _ => None,
            }) {
                if let Some(d) = app.dashboard.as_mut() {
                    d.focus_row(DashboardRowId::TopLevel(new_id));
                    d.attached_agent = Some(new_id);
                }
                log_dashboard_attached(&DashboardRowId::TopLevel(new_id));
            }
            return effects;
        }
    }
    vec![]
}

/// Return from the attached session detail to the dashboard overview.
pub(super) fn dispatch_dashboard_overlay_exit(app: &mut AppView) -> Vec<Effect> {
    // Capture the return target before clearing the attached-session marker.
    if let ActiveView::Agent(id) = app.active_view {
        app.dashboard_return = Some(DashboardReturn::Overlay(id));
    }
    clear_session_commands(app);
    if let Some(d) = app.dashboard.as_mut() {
        d.restore_peek_viewport(&mut app.agents);
        d.close_popup();
    }
    // Clear any stop action scoped to this attached session before leaving.
    app.active_view = ActiveView::AgentDashboard;
    vec![]
}

/// Clear stop-action state when navigation changes the attached session.
/// Mouse navigation calls this too because it bypasses key-input routing.
fn clear_session_commands(app: &mut AppView) {
    let current = match app.active_view {
        ActiveView::Agent(id) => Some(id),
        _ => app
            .dashboard
            .as_ref()
            .and_then(|dashboard| dashboard.attached_agent),
    };
    if let Some(agent) = current.and_then(|id| app.agents.get_mut(&id)) {
        agent.clear_session_commands();
    }
}

/// Stop work in the attached session. Recheck execution state so a newly
/// started turn is cancelled; this does not close the session or its view.
pub(super) fn dispatch_stop_session_work(app: &mut AppView) -> Vec<Effect> {
    use crate::views::dashboard::DashboardRowId;
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let row = match app
        .agents
        .get(&id)
        .and_then(|agent| agent.active_subagent.clone())
    {
        Some(child_session_id) => DashboardRowId::Subagent {
            parent: id,
            child_session_id,
        },
        None => DashboardRowId::TopLevel(id),
    };
    dispatch_stop_row(app, row)
}

pub(super) fn dispatch_close_current_session(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get(&id) else {
        return vec![];
    };
    if agent.active_subagent.is_some() {
        app.show_toast("Return to the parent session to close it");
        return vec![];
    }
    let closing = agent.session.state.command_in_flight()
        == Some(&crate::app::session::AgentCommand::CloseSession);
    if !closing && !crate::views::dashboard::classify_top_level(agent).allows_delete() {
        app.show_toast("Stop session work before closing");
        return vec![];
    }
    dispatch_sessions_confirm_close(app, id)
}

/// Toggle worktree-dispatch mode for the dashboard (bound to Ctrl+W).
///
/// When armed, the next agent dispatched from the dashboard spawns in a fresh
/// git worktree and the `[+ New session]` button reads `[+ New Worktree]`.
/// Worktrees require a git repo, so outside one the toggle is a no-op and we
/// surface an explanatory toast instead of silently doing nothing (and the
/// dashboard is never left in worktree mode in a non-git directory).
pub(super) fn dispatch_dashboard_toggle_worktree(app: &mut AppView) -> Vec<Effect> {
    let has_git = app.cwd_has_git_ancestor;
    if let Some(d) = app.dashboard.as_mut() {
        if has_git {
            d.dispatch_worktree = !d.dispatch_worktree;
        } else {
            d.dispatch_worktree = false;
            d.set_error("Not a git repository — worktrees need one");
        }
    }
    vec![]
}

fn snapshot_prompt_widget(
    prompt: &mut crate::views::prompt_widget::PromptWidget,
    text: String,
) -> crate::views::prompt_widget::StashedPrompt {
    if prompt.text() == text
        || !prompt.images.is_empty()
        || !prompt.textarea().elements().is_empty()
    {
        prompt.stash().with_transformed_text(text)
    } else {
        crate::views::prompt_widget::StashedPrompt::from_submission(text, Vec::new(), Vec::new())
    }
}

/// Open the worktree-label dialog and stash the dispatch prompt until confirm.
fn open_dashboard_worktree_dialog(
    app: &mut AppView,
    prompt: Option<String>,
    attach: bool,
) -> Vec<Effect> {
    if let Some(d) = app.dashboard.as_mut() {
        d.pending_worktree_prompt =
            prompt.map(|text| snapshot_prompt_widget(&mut d.dispatch, text));
        d.pending_worktree_attach = attach;
        d.worktree_dialog = Some(crate::app::root::NewWorktreeDialogState::new());
        d.dispatch.set_text("");
        d.clear_feedback();
    }
    vec![]
}

/// Create a new session AND switch into its detail view. Routed from the
/// `[+ New session]` button and Enter-on-empty-prompt while it's focused.
/// Mirrors `dispatch_dashboard_dispatch`'s new-session arm with `attach=true`,
/// minus the prompt enqueue.
pub(super) fn dispatch_dashboard_create_new_agent_with_detail(app: &mut AppView) -> Vec<Effect> {
    // Worktree mode armed + git repo: open the label dialog (which spawns the
    // agent in a fresh worktree on confirm) instead of a plain session. The
    // button opens the detail view, so confirm attaches (`attach = true`).
    if app.cwd_has_git_ancestor && app.dashboard.as_ref().is_some_and(|d| d.dispatch_worktree) {
        return open_dashboard_worktree_dialog(app, None, /* attach */ true);
    }
    let pending_model = app.dashboard.as_ref().and_then(|d| d.pending_model.clone());
    let (pending_behavior, pending_permission) = app
        .dashboard
        .as_ref()
        .map(|d| (d.pending_behavior, d.pending_permission))
        .unwrap_or((
            tools::types::BehaviorId::Normal,
            crate::app::actions::PermissionModeKind::Ask,
        ));
    let model_id = pending_model.as_ref().map(|m| m.id.clone());
    log_dashboard_launched("new_agent_button");
    let picker_needed = app.needs_project_picker();
    let (new_id, mut effects) = dispatch_new_session_inner_with_id(app, model_id.clone());
    if let Some(agent) = app.agents.get_mut(&new_id) {
        apply_pending_dispatch_config(
            agent,
            pending_model.as_ref(),
            pending_behavior,
            pending_permission,
        );
    }
    if let Some(d) = app.dashboard.as_mut() {
        d.restore_peek_viewport(&mut app.agents);
        // Clear the dispatch input even though we don't enqueue
        // anything — a stray paste while the button is focused
        // (no typed Enter) shouldn't survive the view switch.
        d.dispatch.set_text("");
        d.clear_feedback();
        d.filter = crate::views::dashboard::Filter::None;
        // Snap the cursor onto the new row so the overlay's `i/n`
        // indicator matches the active view, AND so the chrome's
        // `[‹]` / `[›]` cycle anchors on the right starting point.
        d.focus_row(crate::views::dashboard::DashboardRowId::TopLevel(new_id));
        d.attached_agent = Some(new_id);
    }
    if picker_needed {
        effects.extend(open_project_question_with_context(
            app,
            String::new(),
            model_id,
            false,
            true,
            true,
        ));
        return effects;
    }
    app.active_view = ActiveView::Agent(new_id);
    surface_screen_mode_switch_hint(app, new_id);
    effects
}

/// Open the dashboard's shortcuts cheatsheet modal.
///
/// Builds the entry list from the registry, scoped to the `DashboardFocused`
/// + `Always` contexts. Reached only through the `/shortcuts` dispatch path.
pub(super) fn dispatch_dashboard_open_shortcuts_help(app: &mut AppView) {
    let Some(d) = app.dashboard.as_mut() else {
        return;
    };
    if d.shortcuts_modal.is_some() {
        // Idempotent — reopening while the modal is already open is a no-op
        // (rather than blowing away the
        // user's search query / scroll position with a fresh
        // build).
        return;
    }
    use crate::actions::When;
    let contexts = [When::DashboardFocused, When::Always];
    let entries = crate::views::shortcuts_help::build_entries(
        &contexts,
        &app.registry,
        /* vim_mode */ false,
    );
    let state = crate::views::shortcuts_help::build_initial_picker_state(&entries);
    d.shortcuts_modal = Some(Box::new(crate::views::dashboard::ShortcutsModalState {
        entries,
        state,
        window: Default::default(),
        filter_active: false,
        collapsed_sections: crate::views::shortcuts_help::default_collapsed(),
        expanded_ids: std::collections::HashSet::new(),
        mode: crate::views::shortcuts_help::ShortcutsHelpMode::Browse,
    }));
}

/// Short display label for a directory in the location picker — the
/// basename (truncated), or `~` for the home directory itself.
fn location_picker_label(path: &std::path::Path) -> String {
    if dirs::home_dir().is_some_and(|h| h == path) {
        return "~".to_string();
    }
    let raw = path.file_name().and_then(|n| n.to_str()).unwrap_or("/");
    crate::render::line_utils::truncate_str(raw, 30)
}

/// Resolve a raw location-picker / `/cd` path string to an absolute path,
/// expanding a leading `~` and joining relative paths against `cwd`.
/// Returns `None` for empty input or when `~` can't be expanded. The
/// caller is responsible for validating the result is a directory.
pub(super) fn resolve_location_input(
    input: &str,
    cwd: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let expanded: std::path::PathBuf = if trimmed == "~" {
        dirs::home_dir()?
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        dirs::home_dir()?.join(rest)
    } else {
        std::path::PathBuf::from(trimmed)
    };
    if expanded.is_absolute() {
        Some(expanded)
    } else {
        Some(cwd.join(expanded))
    }
}

/// Open the dashboard's location picker. Seeds the candidate list with
/// the current cwd (marked `(current)`) followed by recent project
/// directories from session history. Idempotent — a no-op if the picker
/// is already open or the dashboard isn't active.
pub(super) fn dispatch_dashboard_open_location_picker(app: &mut AppView) -> Vec<Effect> {
    use crate::views::dashboard::{LocationCandidate, LocationPickerState};

    if !matches!(app.active_view, ActiveView::AgentDashboard) {
        // `/cd` reached from a non-dashboard surface — the location
        // picker is a dashboard affordance, so guide the user there.
        // Gate on the dashboard being the *foreground* view (not merely
        // `app.dashboard.is_some()`, which stays true for the rest of the
        // session once the dashboard has been opened even once).
        app.show_toast("Open the Agent Dashboard (/agents) to change location");
        return vec![];
    }
    // Idempotent — re-triggering while open keeps the current query.
    if app
        .dashboard
        .as_ref()
        .is_some_and(|d| d.location_picker.is_some())
    {
        return vec![];
    }

    let cwd = app.cwd.clone();
    let mut candidates: Vec<LocationCandidate> = Vec::new();
    candidates.push(LocationCandidate {
        label: location_picker_label(&cwd),
        detail: format!(
            "{}  (current)",
            crate::project_picker::sources::display_path(&cwd)
        ),
        worktree: None,
        path: cwd.clone(),
    });
    let picker_id = if let Some(d) = app.dashboard.as_mut() {
        let mut lp =
            LocationPickerState::new(candidates, cwd.clone(), std::collections::HashMap::new());
        // Reflect the dashboard's current worktree arming so reopening the
        // picker shows the toggle in its existing state.
        lp.worktree_mode = d.dispatch_worktree;
        let picker_id = lp.picker_id().to_owned();
        d.location_picker = Some(lp);
        Some(picker_id)
    } else {
        None
    };
    crate::unified_log::info("dashboard.location_picker.opened", None, None);
    picker_id
        .map(|picker_id| Effect::FetchDashboardLocationCandidates {
            base_cwd: cwd,
            picker_id,
        })
        .into_iter()
        .collect()
}

/// Apply recent directories to the still-open dashboard location picker.
/// Closing/reopening the picker or changing its base cwd makes a completion
/// stale.
pub(super) fn handle_dashboard_location_candidates_loaded(
    app: &mut AppView,
    base_cwd: std::path::PathBuf,
    picker_id: String,
    recent_dirs: Vec<(std::path::PathBuf, chrono::DateTime<chrono::Utc>)>,
    worktrees: std::collections::HashMap<std::path::PathBuf, String>,
) -> Vec<Effect> {
    let Some(dashboard) = app.dashboard.as_mut() else {
        return vec![];
    };
    let Some(picker) = dashboard.location_picker.as_mut() else {
        return vec![];
    };
    if picker.base_cwd != base_cwd || picker.picker_id() != picker_id {
        return vec![];
    }
    picker.replace_recent_candidates(&recent_dirs, worktrees);
    vec![]
}

/// Apply a location-picker / `/cd` selection. Resolves + validates the
/// path, and on success updates `app.cwd` + the process cwd (so newly
/// dispatched dashboard sessions spawn there) and closes the modal. On
/// failure the modal stays open with an inline error and the cwd is
/// unchanged.
pub(super) fn dispatch_dashboard_change_location(app: &mut AppView, input: String) -> Vec<Effect> {
    // Gate on the dashboard being the *foreground* view. `/cd <path>` typed
    // from another surface (welcome / an agent session / the dashboard
    // overlay) would otherwise silently change the process cwd, since
    // `app.dashboard` stays `Some` for the rest of the session once opened.
    if !matches!(app.active_view, ActiveView::AgentDashboard) {
        app.show_toast("Open the Agent Dashboard (/agents) to change location");
        return vec![];
    }
    let path = match resolve_location_input(&input, &app.cwd).filter(|p| p.is_dir()) {
        Some(p) => p,
        None => {
            if let Some(lp) = app
                .dashboard
                .as_mut()
                .and_then(|d| d.location_picker.as_mut())
            {
                lp.error = Some(format!("Not a directory: {}", input.trim()));
            } else if let Some(d) = app.dashboard.as_mut() {
                // `/cd <bad path>` typed into the dispatch box (no picker
                // open) — surface the error as a dashboard toast.
                d.set_error(format!("Not a directory: {}", input.trim()));
            }
            return vec![];
        }
    };

    crate::unified_log::info(
        "dashboard.location_picker.changed",
        None,
        Some(serde_json::json!({ "path": path.display().to_string() })),
    );

    let changed = commit_cwd_snapshot(app, &path);
    let display = crate::project_picker::sources::display_path(&path);

    let has_git = app.cwd_has_git_ancestor;
    if let Some(d) = app.dashboard.as_mut() {
        // Carry the picker's worktree toggle onto the dashboard so the next
        // dispatched agent honors it after the modal closes — but only when
        // the destination is a git repo. Worktrees require one, so navigating
        // to a non-git directory forces worktree mode off (the dashboard is
        // never in worktree mode outside a repo).
        if let Some(wt) = d.location_picker.as_ref().map(|lp| lp.worktree_mode) {
            d.dispatch_worktree = wt && has_git;
        } else if !has_git {
            d.dispatch_worktree = false;
        }
        d.location_picker = None;
        if changed {
            d.set_info(format!("Working directory: {display}"));
        }
    }

    // Change the process cwd via an effect, not inline, so the reducer
    // stays side-effect free and parallel tests don't leak the process
    // cwd into each other.
    vec![Effect::SetWorkingDir { path }]
}

/// Commit the application/dashboard view of a working-directory change.
///
/// Session creation and dashboard location selection must observe the same
/// cwd/git snapshot. Keep this reducer-side update together; the process cwd
/// is still changed later by the returned `SetWorkingDir` effect.
pub(super) fn commit_cwd_snapshot(app: &mut AppView, path: &std::path::Path) -> bool {
    let changed = app.cwd != path;
    app.cwd = path.to_path_buf();
    app.cwd_has_git_ancestor = path.ancestors().any(|p| p.join(".git").exists());
    crate::git_info::populate_from_cwd_async(path.to_path_buf());

    let has_git = app.cwd_has_git_ancestor;
    if let Some(d) = app.dashboard.as_mut() {
        d.cwd = path.to_path_buf();
        d.cwd_has_git_ancestor = has_git;
        if !has_git {
            d.dispatch_worktree = false;
        }
        // Keep dashboard `@` completion rooted at the same directory as a
        // session selected through the project picker.
        d.dispatch.file_search.retarget(path);
    }
    changed
}

/// Confirm the dashboard worktree-label dialog: create the next dashboard
/// agent in a fresh worktree at `app.cwd`, replaying any prompt stashed when
/// the dialog opened. The dialog itself was already cleared by the input
/// handler. Surfaces a dashboard toast (instead of creating) when the cwd
/// isn't a git repository.
pub(super) fn dispatch_dashboard_confirm_worktree(
    app: &mut AppView,
    label: Option<String>,
) -> Vec<Effect> {
    // Apply the prompt, attach choice, and staged model/mode together.
    // The worktree path must honor all of these exactly like the normal
    // dispatch path (`dispatch_dashboard_dispatch`).
    let (mut prompt, attach) = match app.dashboard.as_mut() {
        Some(d) => (
            d.pending_worktree_prompt.take(),
            std::mem::replace(&mut d.pending_worktree_attach, false),
        ),
        None => (None, false),
    };
    let pending_model = app.dashboard.as_ref().and_then(|d| d.pending_model.clone());
    let (pending_behavior, pending_permission) = app
        .dashboard
        .as_ref()
        .map(|d| (d.pending_behavior, d.pending_permission))
        .unwrap_or((
            tools::types::BehaviorId::Normal,
            crate::app::actions::PermissionModeKind::Ask,
        ));
    if !app.cwd_has_git_ancestor {
        if let Some(d) = app.dashboard.as_mut() {
            // Restore the typed prompt if the cwd stopped being a repo.
            if let Some(p) = prompt {
                d.dispatch.restore(p);
            }
            d.set_error("Not a git repository — can't create a worktree here");
        }
        return vec![];
    }
    let (prompt_text, mut images, chip_elements) = if let Some(stashed) = prompt.take() {
        let (text, images, chip_elements) = stashed.into_submission();
        (Some(text), images, chip_elements)
    } else {
        (None, Vec::new(), Vec::new())
    };
    let model_id = pending_model.as_ref().map(|m| m.id.clone());
    let effects =
        dispatch_new_worktree_session(app, None, label, prompt_text, model_id, None, None);
    if let Some(new_id) = effects.iter().find_map(|e| match e {
        Effect::CreateWorktreeSession { agent_id, .. } => Some(*agent_id),
        _ => None,
    }) {
        // Apply the staged effort / plan / always-approve mode to the new
        // worktree agent (base model is seeded via the effect's `model_id`),
        // then carry any pasted images onto the replayed prompt — both mirror
        // `dispatch_dashboard_dispatch`.
        if let Some(agent) = app.agents.get_mut(&new_id) {
            apply_pending_dispatch_config(
                agent,
                pending_model.as_ref(),
                pending_behavior,
                pending_permission,
            );
            if let Some(entry) = agent.session.pending_prompts.back_mut() {
                entry.images = std::mem::take(&mut images);
                entry.chip_elements = chip_elements;
            }
        }
        if attach {
            // Send+open: attach the dashboard's detail-view overlay onto the
            // agent (`dispatch_new_worktree_session` already set `active_view`).
            if let Some(d) = app.dashboard.as_mut() {
                d.restore_peek_viewport(&mut app.agents);
                d.focus_row(crate::views::dashboard::DashboardRowId::TopLevel(new_id));
                d.attached_agent = Some(new_id);
            }
        } else {
            // Plain Enter: undo the view switch `dispatch_new_worktree_session`
            // made and stay on the dashboard.
            app.active_view = ActiveView::AgentDashboard;
        }
    }
    crate::prompt_images::drain_and_cleanup(&mut images);
    effects
}

/// Cycle attached detail through locally open sessions in `app.agents`
/// insertion order, independent of overview filters, grouping, and pins.
/// Attaches dashboard chrome on the first cycle.
pub(super) fn dispatch_dashboard_overlay_cycle(app: &mut AppView, delta: i32) -> Vec<Effect> {
    use crate::views::dashboard::DashboardRowId;

    // Anchor on the agent on screen, not a possibly-stale `attached_agent`:
    // keeps cycling correct after a session switch the view without
    // re-attaching. (Cycling is only ever invoked from an agent view.)
    let ActiveView::Agent(current) = app.active_view else {
        return vec![];
    };
    if app.agents.len() <= 1 {
        return vec![];
    }
    if !crate::views::dashboard::dashboard_enabled() {
        return vec![];
    }
    let order = crate::views::dashboard::overlay_cycle_order(&app.agents);
    if order.len() <= 1 {
        return vec![];
    }
    let Some(idx) = order.iter().position(|id| *id == current) else {
        return vec![];
    };
    let n = order.len() as i32;
    let next_idx = (((idx as i32) + delta).rem_euclid(n)) as usize;
    let next_id = order[next_idx];
    if next_id == current {
        return vec![];
    }
    // Materialize + configure only on a real switch — otherwise a
    // cycle-created dashboard renders bare on back-out (default cwd, empty
    // `/model`, wrong auto-approve).
    if app.dashboard.is_none() {
        ensure_dashboard_state(app);
        configure_dashboard_state(app);
    }
    // Drop any prior subagent takeover on the next agent so the
    // overlay paints the parent view, mirroring the attach path.
    if let Some(agent) = app.agents.get_mut(&next_id) {
        agent.clear_session_commands();
        agent.active_subagent = None;
    }
    // Clear prefix/stop state for the current session before changing the
    // attached session; mouse navigation bypasses key-input routing.
    clear_session_commands(app);
    if let Some(d) = app.dashboard.as_mut() {
        d.restore_peek_viewport(&mut app.agents);
        d.attached_agent = Some(next_id);
        d.focus_row(DashboardRowId::TopLevel(next_id));
    }
    app.active_view = ActiveView::Agent(next_id);
    surface_screen_mode_switch_hint(app, next_id);
    vec![]
}

pub(super) fn dispatch_dashboard_dispatch(
    app: &mut AppView,
    text: String,
    attach: bool,
) -> Vec<Effect> {
    // Paste-then-immediate-send: a Cmd+V image probe is still off-thread. Stash
    // this send and re-issue it once the probe completes so the image is never
    // dropped from the dispatched prompt's content blocks.
    if let Some(d) = app.dashboard.as_mut()
        && d.paste_probe_in_flight > 0
    {
        d.deferred_dispatch_send =
            Some(crate::views::dashboard::state::DeferredDispatchSend { attach });
        return vec![];
    }
    let trimmed = text.trim().to_string();
    // Reject only an empty / whitespace-only prompt; any non-empty
    // input (even a single character) dispatches a new session. The
    // keyboard path already filters empty input upstream (see
    // `DashboardState::handle_key`), so this guard mainly protects the
    // slash-fallback callers.
    if trimmed.is_empty() {
        if let Some(d) = app.dashboard.as_mut() {
            d.set_warning("Type a prompt to dispatch a session");
        }
        return vec![];
    }
    // Cap dispatch text at 64 KiB so a runaway paste
    // doesn't queue a multi-megabyte prompt.
    //
    // Toast shows BOTH the character count (what the
    // user thinks in) AND the byte budget (the actual limit) so the
    // char/byte unit mismatch isn't surprising.
    const MAX_DISPATCH_BYTES: usize = 64 * 1024;
    if text.len() > MAX_DISPATCH_BYTES {
        let chars = text.chars().count();
        if let Some(d) = app.dashboard.as_mut() {
            d.set_error(format!(
                "Prompt too long ({chars} chars / {} bytes; max ~64 KiB)",
                text.len()
            ));
        }
        return vec![];
    }

    // Worktree mode armed (via the location picker) AND the cwd is a git
    // repo: stash the prompt and open the worktree-label dialog. Confirming
    // it spawns the agent in a fresh worktree and replays this prompt (see
    // `Action::DashboardConfirmWorktree`). The `attach` flag rides along so
    // confirm honors the same detail-vs-dashboard choice as a normal dispatch
    // (bare Enter stays on the dashboard; Ctrl+S opens the detail view).
    // Outside a repo we fall through to a normal session dispatch (the toggle
    // is hidden there anyway).
    if app.cwd_has_git_ancestor && app.dashboard.as_ref().is_some_and(|d| d.dispatch_worktree) {
        return open_dashboard_worktree_dialog(app, Some(text), attach);
    }

    // The composer always creates a new session. Row focus controls only
    // overview navigation; existing sessions own their prompts in detail.
    // Return the new AgentId from the inner constructor
    // so we don't have to rely on `app.agents.last()`.
    //
    // Carry the dashboard's staged model / plan-mode (set via `/model` and
    // `/plan`) onto the new session: the model id seeds `CreateSession`, and
    // effort / plan are applied post-creation by `apply_pending_dispatch_config`.
    let pending_model = app.dashboard.as_ref().and_then(|d| d.pending_model.clone());
    let (pending_behavior, pending_permission) = app
        .dashboard
        .as_ref()
        .map(|d| (d.pending_behavior, d.pending_permission))
        .unwrap_or((
            tools::types::BehaviorId::Normal,
            crate::app::actions::PermissionModeKind::Ask,
        ));
    let model_id = pending_model.as_ref().map(|m| m.id.clone());
    let prompt_state = app
        .dashboard
        .as_mut()
        .map(|dashboard| snapshot_prompt_widget(&mut dashboard.dispatch, text.clone()))
        .unwrap_or_else(|| {
            crate::views::prompt_widget::StashedPrompt::from_submission(
                text,
                Vec::new(),
                Vec::new(),
            )
        });
    let picker_needed = app.needs_project_picker();
    let (prompt_text, mut pasted_images, chip_elements, pending_stash) = if picker_needed {
        // The complete stash is restored into the placeholder below and is
        // immediately re-stashed by the picker. This preserves image/chip
        // ownership while the question view owns input.
        (
            prompt_state.text.clone(),
            Vec::new(),
            Vec::new(),
            Some(prompt_state),
        )
    } else {
        let (text, images, chips) = prompt_state.into_submission();
        (text, images, chips, None)
    };
    log_dashboard_launched("prompt");
    let (new_id, mut effects) = dispatch_new_session_inner_with_id(app, model_id.clone());
    if let Some(agent) = app.agents.get_mut(&new_id) {
        if picker_needed {
            agent
                .prompt
                .restore(pending_stash.expect("picker keeps the prompt stash"));
        } else {
            agent.session.enqueue_prompt(prompt_text.clone());
            if let Some(entry) = agent.session.pending_prompts.back_mut() {
                entry.images = std::mem::take(&mut pasted_images);
                entry.chip_elements = chip_elements;
            }
        }
        apply_pending_dispatch_config(
            agent,
            pending_model.as_ref(),
            pending_behavior,
            pending_permission,
        );
    }
    crate::prompt_images::drain_and_cleanup(&mut pasted_images);
    // Only clear the input AFTER successful session
    // creation. If the agent failed to register the prompt above we
    // would have already returned without effects.
    if let Some(d) = app.dashboard.as_mut() {
        d.dispatch.set_text("");
        d.clear_feedback();
        d.filter = crate::views::dashboard::Filter::None;
    }
    if picker_needed {
        effects.extend(open_project_question_with_context(
            app,
            prompt_text,
            model_id,
            true,
            true,
            attach,
        ));
        return effects;
    }
    if attach {
        // Ctrl+S (Send+Open) — walk into the new agent's
        // detail view AND paint the session-overlay chrome.
        // Mirrors `dispatch_dashboard_attach` and
        // `dispatch_dashboard_create_new_agent_with_detail`:
        // both `attached_agent` and `selected` follow the new
        // row so the detail title bar's cycle controls have an anchor;
        // Ctrl+\ returns to the dashboard.
        if let Some(d) = app.dashboard.as_mut() {
            d.restore_peek_viewport(&mut app.agents);
            d.focus_row(crate::views::dashboard::DashboardRowId::TopLevel(new_id));
            d.attached_agent = Some(new_id);
        }
        app.active_view = ActiveView::Agent(new_id);
        surface_screen_mode_switch_hint(app, new_id);
    } else {
        // Plain Enter (Send) stays on the dashboard, no auto-select.
        // The freshly-created row is left unselected so the overview
        // navigation cursor stays where the user had it — the input
        // always dispatches a NEW session regardless of selection, so
        // prompt ownership stays with the new-session composer.
        app.active_view = ActiveView::AgentDashboard;
    }
    effects
}

/// Resolve a slash command typed into the dashboard's dispatch input.
///
/// The dashboard has no session context, so the execution path is more
/// limited than the agent view's:
///
///   - Builtin commands that return `CommandResult::Action(...)` (e.g.
///     `/agents`, `/quit`, `/theme`, `/settings`, `/help`, `/model`,
///     `/mcps`, `/plugin`, …) are dispatched identically to the agent path.
///   - `CommandResult::Message` / `Error` surface through the dashboard's
///     typed feedback slot (no scrollback to push into).
///   - `CommandResult::Handled` / `HandledNoOp` clear the input.
///   - ACP-advertised `CommandResult::HostCommand` values,
///     `CommandResult::QueueCommand`, and `InjectSkill` fall back to
///     dispatching a new session with the
///     text as its first prompt — the dashboard treats them like a
///     bare free-text dispatch. This keeps the user's typing from
///     being silently dropped on the floor when they invoke a
///     plugin/skill from the dashboard surface. Unknown slash commands are
///     errors and never take this path.
///
/// Offer / execute tri-state (matches completion's [`command_offered`]):
///   - **Unknown** token → local error; never a new-session prompt.
///   - **Registered, session-scoped** (hidden on this surface) → clear
///     dispatch + error toast; do **not** spawn with the slash as a prompt.
///   - **Registered, not visible** (auth/feature gate, e.g. `/usage` on
///     external auth) → still `command.run` so the command owns the error.
///   - **Registered, offered** → MRU + `command.run` (e.g. `/model` /
///     `/plan` stage the next spawn).
pub(super) fn dispatch_dashboard_dispatch_slash(app: &mut AppView, text: String) -> Vec<Effect> {
    use crate::slash::command::{CommandExecCtx, CommandResult};
    use crate::slash::parse_invocation;

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() || !trimmed.starts_with('/') {
        return vec![];
    }

    let show_tips_from_app = app.show_tips;
    let auto_update_from_app = app.auto_update;
    let respect_manual_folds_from_app = app.appearance.scrollback.scroll.respect_manual_folds;
    let auto_mode_gate_from_app = app.auto_mode_gate;
    let ask_user_question_timeout_enabled_from_app = app.ask_user_question_timeout_enabled;
    // Build the execution context from app-wide state. The dashboard
    // is session-less, so `session_id` is `None`. Offered session-less
    // opt-ins (`/model`, `/effort`, `/behavior`, `/permission`) and
    // pager-global commands still run
    // and may toast if a dispatcher needs an agent.
    let result = {
        let Some(invocation) = parse_invocation(trimmed.as_str()) else {
            return vec![];
        };

        // Get the slash registry from the dashboard's prompt widget.
        // The dashboard owns its own registry (populated at open time
        // with builtins + ACP commands from `bootstrap_acp_commands`).
        let Some(dashboard) = app.dashboard.as_ref() else {
            return vec![];
        };
        let reg = dashboard.dispatch.slash_controller.registry();

        {
            use diagnostics::events::{PagerCommandSource, PagerSlashCommand};
            use diagnostics::session_ctx::log_event;
            let source = if reg.is_builtin(invocation.token) {
                PagerCommandSource::Builtin
            } else {
                PagerCommandSource::NonBuiltin
            };
            log_event(PagerSlashCommand {
                command_name: invocation.token.to_string(),
                source,
            });
        }

        let Some(command) = reg.get(invocation.token).cloned() else {
            if let Some(dashboard) = app.dashboard.as_mut() {
                dashboard.dispatch.set_text("");
                dashboard.set_error(format!("Unknown command: /{}", invocation.token));
            }
            return vec![];
        };
        // Registered but not offered on this surface:
        //   - session-scoped → toast; never spawn with the slash as a prompt
        //   - `visible() == false` (e.g. `/usage` on external auth) → still
        //     `run()` so the command owns the refusal message
        if !dashboard
            .dispatch
            .slash_controller
            .is_command_offered(command.as_ref(), &app.models)
            && command.session_scoped()
            && !command.offered_when_session_less()
        {
            let name = command.name();
            if let Some(d) = app.dashboard.as_mut() {
                d.dispatch.set_text("");
                d.set_error(format!("/{name} only works in a session"));
            }
            return vec![];
        }
        if let Some(dashboard) = app.dashboard.as_mut() {
            // Records MRU and queues an off-thread persist internally.
            dashboard
                .dispatch
                .slash_controller
                .record_command_use(invocation.token, invocation.token);
        }

        let dashboard_multiline = app.dashboard.as_ref().is_some_and(|d| d.multiline_mode);
        let (pending_behavior, pending_permission) = app
            .dashboard
            .as_ref()
            .map(|d| (d.pending_behavior, d.pending_permission))
            .unwrap_or((
                tools::types::BehaviorId::Normal,
                crate::app::actions::PermissionModeKind::Ask,
            ));
        let mut ctx = CommandExecCtx {
            models: &app.models,
            session_id: None,
            bundle_state: &app.bundle_state,
            screen_mode: app.screen_mode,
            pager_state: crate::settings::PagerLocalSnapshot {
                multiline_mode: dashboard_multiline,
                permission_mode: pending_permission.as_runtime(),
                subagent_permission_mode: app.subagent_permission_mode,
                current_model_id: app.models.current_model_id_str().map(str::to_owned),
                available_models: app
                    .models
                    .available
                    .iter()
                    .map(|(id, _info)| (id.0.to_string(), id.clone()))
                    .collect(),
                behavior_mode: pending_behavior,
                // Sessionless dashboard state has no authoritative tool
                // snapshot yet; keep Workflow optimistically visible.
                workflows_available: true,
                goal_available: false,
                show_tips: show_tips_from_app,
                auto_update: auto_update_from_app,
                vim_mode: crate::appearance::cache::load_vim_mode(),
                scroll_speed: crate::appearance::cache::load_scroll_speed(),
                respect_manual_folds: respect_manual_folds_from_app,
                auto_mode_gate: auto_mode_gate_from_app,
                ask_user_question_timeout_enabled: ask_user_question_timeout_enabled_from_app,
            },
        };
        command.run(&mut ctx, invocation.args)
    };

    match result {
        CommandResult::Handled | CommandResult::HandledNoOp => {
            if let Some(d) = app.dashboard.as_mut() {
                d.dispatch.set_text("");
                d.clear_feedback();
            }
            vec![]
        }
        CommandResult::Error(msg) => {
            if let Some(d) = app.dashboard.as_mut() {
                d.dispatch.set_text("");
                d.set_error(msg);
            }
            vec![]
        }
        CommandResult::Message(msg) => {
            if let Some(d) = app.dashboard.as_mut() {
                d.dispatch.set_text("");
                d.set_info(msg);
            }
            vec![]
        }
        CommandResult::Action(Action::ExitSession) => {
            // ExitSession from a session-less surface is meaningless
            // — collapse it to `/agents`' exit semantics.
            if let Some(d) = app.dashboard.as_mut() {
                d.dispatch.set_text("");
            }
            dispatch(Action::ExitDashboard, app)
        }
        // `/model` on the session-less dashboard stages the model for the
        // NEXT spawned agent instead of switching a (nonexistent) session.
        // Effort is part of the same per-spawn staging; no persistent default
        // is modified here.
        CommandResult::Action(Action::SwitchModel { model_id, effort }) => {
            stage_dashboard_model(app, model_id, effort);
            vec![]
        }
        CommandResult::Action(Action::PatchEffort { model_id, effort }) => {
            stage_dashboard_model(app, model_id, Some(effort));
            vec![]
        }
        // Bare selector commands reuse their normal suggestions in the
        // dashboard input. This surface has no AgentView-owned modal stack,
        // so opening the selector means entering the same Slash args phase.
        CommandResult::Action(Action::OpenCommandPicker {
            command,
            args_query,
        }) => {
            if let Some(d) = app.dashboard.as_mut() {
                let suffix = if args_query.is_empty() {
                    String::new()
                } else {
                    args_query
                };
                let picker_input = format!("/{command} {suffix}");
                d.dispatch.set_text(&picker_input);
                d.dispatch.set_cursor(picker_input.len());
                d.dispatch.refresh_slash(&d.models);
                d.clear_feedback();
            }
            vec![]
        }
        // Behavior selectors stage the next spawned Agent. Selection is
        // idempotent and independent from its Permission policy.
        CommandResult::Action(Action::SetBehaviorMode(mode)) => {
            if let Some(d) = app.dashboard.as_mut() {
                d.dispatch.set_text("");
                d.clear_feedback();
                d.pending_behavior = mode;
                d.dispatch.slash_controller.set_selection_context(
                    None,
                    mode,
                    false,
                    None,
                    app.auto_mode_gate,
                    d.pending_permission.as_canonical(),
                );
            }
            vec![]
        }
        // A Behavior command with a prompt stages the Behavior before spawn.
        CommandResult::Action(Action::SetBehaviorThenPrompt { mode, prompt }) => {
            if let Some(d) = app.dashboard.as_mut() {
                d.pending_behavior = mode;
            }
            match prompt {
                Some(prompt) => dispatch_dashboard_dispatch(app, prompt, /* attach */ false),
                None => {
                    if let Some(d) = app.dashboard.as_mut() {
                        d.dispatch.set_text("");
                        d.clear_feedback();
                    }
                    vec![]
                }
            }
        }
        // `/plan` with an existing plan has nothing to show on the
        // session-less dashboard.
        CommandResult::Action(Action::ShowPlan) => {
            if let Some(d) = app.dashboard.as_mut() {
                d.dispatch.set_text("");
                d.set_warning("No plan to show on the dashboard");
            }
            vec![]
        }
        CommandResult::Action(Action::SetPermissionMode(permission)) => {
            let permission = if permission.is_auto() && !app.auto_mode_gate {
                crate::app::actions::PermissionModeKind::Ask
            } else {
                permission
            };
            if let Some(d) = app.dashboard.as_mut() {
                d.dispatch.set_text("");
                d.clear_feedback();
                d.pending_permission = permission;
                d.dispatch.slash_controller.set_selection_context(
                    None,
                    d.pending_behavior,
                    false,
                    None,
                    app.auto_mode_gate,
                    permission.as_canonical(),
                );
            }
            vec![]
        }
        CommandResult::Action(action) => {
            if let Some(d) = app.dashboard.as_mut() {
                d.dispatch.set_text("");
                d.clear_feedback();
            }
            dispatch(action, app)
        }
        CommandResult::Doctor(_) => {
            if let Some(d) = app.dashboard.as_mut() {
                d.dispatch.set_text("");
                d.set_warning("Open a session to run /doctor.");
            }
            vec![]
        }
        CommandResult::QueueCommand(_)
        | CommandResult::InjectSkill { .. }
        | CommandResult::HostCommand(_) => {
            // These results all expect an agent session to consume
            // them. The dashboard has none, so route the original
            // text through the normal "new session with this text as
            // the first prompt" path so the user's intent isn't lost.
            dispatch_dashboard_dispatch(app, text, /* attach */ false)
        }
    }
}

/// Stage a model (+ optional reasoning effort) for the next agent the
/// dashboard spawns. Label is the catalog id (`provider/model`). Clears the
/// dispatch input + any error toast.
fn stage_dashboard_model(
    app: &mut AppView,
    model_id: shell::agent::models::ModelId,
    effort: Option<shell::sampling::types::ReasoningEffort>,
) {
    let display = model_id.0.to_string();
    if let Some(d) = app.dashboard.as_mut() {
        d.dispatch.set_text("");
        // Mirror the staged choice into the dashboard's catalog snapshot so a
        // subsequent `/model` dropdown marks THIS model as `(current)` (and
        // its effort as `(active)`) rather than the app default the snapshot
        // was seeded with at open. Without this the dropdown lags one step
        // behind `pending_model`.
        d.models.set_current(model_id.clone(), effort);
        d.pending_model = Some(crate::views::dashboard::PendingDispatchModel {
            id: model_id,
            effort,
            display,
        });
        let pending = d
            .pending_model
            .as_ref()
            .expect("pending model was just staged");
        let selection = match pending.effort {
            Some(effort) => format!("{} ({effort} effort)", pending.display),
            None => pending.display.clone(),
        };
        d.set_success(format!("{selection} set for next session"));
    }
}

/// Apply the dashboard's staged model, Behavior, and Permission to a freshly
/// spawned agent. The base model is already seeded via `CreateSession`'s
/// `model_id`; here we stash the reasoning effort (pushed to the shell once
/// the session exists, mirroring the agent-view flow) and the deferred
/// `BehaviorId` (consumed in the `SessionCreated` handlers).
pub(super) fn apply_pending_dispatch_config(
    agent: &mut AgentView,
    pending_model: Option<&crate::views::dashboard::PendingDispatchModel>,
    pending_behavior: tools::types::BehaviorId,
    pending_permission: crate::app::actions::PermissionModeKind,
) {
    if let Some(m) = pending_model {
        // The base model is seeded via `CreateSession.model_id`; only stash a
        // deferred switch when an explicit effort must be pushed. Setting it
        // (or clearing to `None`) also overrides any CLI `-m` default so the
        // dashboard's `/model` choice wins.
        agent.session.deferred_model_switch = m.effort.map(|e| (m.id.clone(), Some(e)));
    }
    agent.session.deferred_session_mode =
        (pending_behavior != tools::types::BehaviorId::Normal).then_some(pending_behavior);

    agent.session.permission_mode = match pending_permission {
        crate::app::actions::PermissionModeKind::Auto => shell::util::config::PermissionMode::Auto,
        crate::app::actions::PermissionModeKind::AlwaysApprove => {
            shell::util::config::PermissionMode::AlwaysApprove
        }
        crate::app::actions::PermissionModeKind::Ask => shell::util::config::PermissionMode::Ask,
    };
}

pub(super) fn dispatch_dashboard_toggle_pin(app: &mut AppView) -> Vec<Effect> {
    if let Some(d) = app.dashboard.as_mut() {
        let _ = d.toggle_pin_selected();
    }
    dispatch_dashboard_persist(app)
}

pub(super) fn dispatch_dashboard_begin_rename(app: &mut AppView) {
    let Some(d) = app.dashboard.as_mut() else {
        return;
    };
    let Some(sel) = d.selected.clone() else {
        return;
    };
    // Only top-level rows are renameable (subagents are tool-spawned
    // and have no user-visible name to rename).
    if sel.is_subagent() {
        d.set_warning("Subagent rows can't be renamed");
        return;
    }
    // The draft starts EMPTY (not prefilled with the current title):
    // renames are almost always full rewrites, so prefilling only costs
    // the user a hold-Backspace. Esc / empty-draft Enter cancel without
    // touching the existing name.
    if let Some(d) = app.dashboard.as_mut() {
        d.rename = Some(crate::views::dashboard::state::RenameDraft::new(sel, ""));
    }
}

pub(super) fn dispatch_dashboard_commit_rename(app: &mut AppView) -> Vec<Effect> {
    let Some(d) = app.dashboard.as_mut() else {
        return vec![];
    };
    let Some(rn) = d.rename.take() else {
        return vec![];
    };
    // Edge case 5: empty/whitespace draft cancels without committing.
    let trimmed = rn.text().trim();
    if trimmed.is_empty() {
        return vec![];
    }
    let title: String = crate::views::session_title::sanitize_display_text(trimmed).into_owned();
    if title.is_empty() {
        return vec![];
    }
    let crate::views::dashboard::DashboardRowId::TopLevel(agent_id) = rn.row else {
        return vec![];
    };
    let mut effects = Vec::new();
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        if let Some(session_id) = agent.session.session_id.clone() {
            let cwd = agent.session.cwd.clone();
            agent.display_name = Some(title.clone());
            effects.push(Effect::RenameSession {
                agent_id,
                session_id,
                title,
                cwd,
            });
        } else {
            agent.display_name = Some(title);
        }
    }
    effects
}

/// Pick the dashboard row the cursor should land on after `closed` is
/// removed. Computed against the CURRENT (pre-removal) display order so
/// it mirrors what the user sees: the next visible row below `closed`
/// (so the cursor stays put while the rows below shift up — i.e. it
/// "moves down 1" relative to the list) or, when `closed` is the last
/// row, the previous visible row. Section headers are skipped so the
/// cursor always lands on an agent row. Returns `None` when `closed`
/// is the only row — the caller then clears the selection and lets
/// `reanchor_selection` fall back to the `[+ New session]` button.
///
/// Without this, closing the selected agent leaves a stale cursor that
/// `reanchor_selection` drops to `None`, and the next ↑/↓ restarts from
/// the top of the list — a jarring jump.
pub(super) fn dashboard_neighbor_row(
    app: &AppView,
    closed: &crate::views::dashboard::DashboardRowId,
) -> Option<crate::views::dashboard::DashboardRowId> {
    use crate::views::dashboard::Focusable;
    let d = app.dashboard.as_ref()?;
    let home = crate::views::dashboard::render::cached_home();
    let roster: &[crate::app::roster::RosterEntry] = if app.leader_mode {
        &app.leader_roster
    } else {
        &app.dashboard_local_sessions
    };
    let rows = crate::views::dashboard::build_rows_with_roster(
        &app.agents,
        &d.pinned,
        &d.reorder,
        None,
        d.grouping,
        &d.filter,
        home,
        roster,
    );
    let focusables = crate::views::dashboard::render::focusables(
        &rows,
        d.grouping,
        &d.filter,
        &d.collapsed_sections,
        d.idle_show_all,
        d.search_mode,
    );
    let cur = focusables
        .iter()
        .position(|f| matches!(f, Focusable::Row(id) if id == closed))?;
    // Next row below (down 1); else the closed row was last → previous row.
    let next = focusables[cur + 1..].iter().find_map(|f| match f {
        Focusable::Row(id) => Some(id.clone()),
        Focusable::Section(_) | Focusable::IdleOverflow => None,
    });
    next.or_else(|| {
        focusables[..cur].iter().rev().find_map(|f| match f {
            Focusable::Row(id) => Some(id.clone()),
            Focusable::Section(_) | Focusable::IdleOverflow => None,
        })
    })
}

/// Explicit stop of selected session work; never close or delete history.
pub(super) fn dispatch_dashboard_stop(app: &mut AppView) -> Vec<Effect> {
    let Some(row) = app.dashboard.as_ref().and_then(|d| d.selected.clone()) else {
        return vec![];
    };
    dispatch_stop_row(app, row)
}

fn dispatch_stop_row(
    app: &mut AppView,
    sel: crate::views::dashboard::DashboardRowId,
) -> Vec<Effect> {
    use crate::views::dashboard::DashboardRowId;
    use std::time::Instant;
    match &sel {
        DashboardRowId::TopLevel(id) => {
            let id = *id;
            let Some(agent) = app.agents.get_mut(&id) else {
                return vec![];
            };
            match stop_top_level_activity(agent) {
                Some(effects) => effects,
                None => {
                    let message =
                        if crate::views::dashboard::classify_top_level(agent).allows_delete() {
                            "No session work to stop"
                        } else {
                            "Stop controls are unavailable; open session details"
                        };
                    app.show_toast(message);
                    vec![]
                }
            }
        }
        DashboardRowId::Subagent {
            parent,
            child_session_id,
        } => {
            let Some(agent) = app.agents.get_mut(parent) else {
                return vec![];
            };
            let Some(info) = agent.session.subagent_sessions.get_mut(child_session_id) else {
                return vec![];
            };
            if !info.is_running() {
                app.show_toast("No subagent work to stop");
                return vec![];
            }
            let subagent_id = info.subagent_id.to_string();
            info.pending_kill = true;
            info.kill_requested_at = Some(Instant::now());
            let session_id = agent.session.session_id.clone();
            session_id
                .map(|sid| Effect::KillSubagent {
                    session_id: sid,
                    subagent_id,
                })
                .into_iter()
                .collect()
        }
        DashboardRowId::Roster { .. } => {
            app.show_toast("Open the session before stopping its work");
            vec![]
        }
    }
}

/// Stop everything keeping a busy top-level row out of Idle: a running
/// turn, ordinary children, Goal, manageable Workflows, background commands,
/// scheduled `/loop`s and queued prompts. Keep the selected root's plan and
/// existing local task markers together; runtime activity still awaits ACK.
/// Returns `Some(effects)` when control was requested — empty if only the
/// local prompt queue — or `None` when there was nothing stoppable (so the
/// caller can explain why).
fn stop_top_level_activity(agent: &mut crate::app::agent_view::AgentView) -> Option<Vec<Effect>> {
    let activity = crate::app::session::activity::AgentActivityProjection::from_sessions(
        &agent.session,
        agent.subagent_views.values().map(|child| &child.session),
    );
    let session_id = agent.session.session_id.clone();
    let mut effects = Vec::new();

    // Turn / background work need a session id to reach the backend.
    if let Some(session_id) = session_id {
        let turn_busy = agent.session.state.is_turn_running()
            || agent.session.state.is_compact_running()
            || agent.session.state.is_cancelling()
            || agent.session.turn_activity().is_some();
        if turn_busy || activity.subagents {
            if agent.session.state.is_compact_running() {
                agent.session.cancel_compact_command();
            } else if agent.session.state.is_turn_running() {
                agent.session.cancel_turn(&mut agent.scrollback);
            }
            effects.push(Effect::CancelTurn {
                session_id: session_id.clone(),
                cancel_subagents: true,
                pause_goal: activity.goal_active,
                trigger: None,
                rewind_if_pristine: false,
            });
        } else if activity.goal_active {
            effects.push(Effect::ExecuteSlashCommand {
                agent_id: agent.session.id,
                session_id: session_id.clone(),
                request: crate::slash::HostCommandRequest::new(
                    "/goal pause",
                    "Pause the active Goal",
                ),
            });
        }
        for run in agent
            .session
            .workflow_runs
            .iter()
            .filter(|run| run.is_active() && run.can_stop())
        {
            effects.push(Effect::ExecuteSlashCommand {
                agent_id: agent.session.id,
                session_id: session_id.clone(),
                request: crate::slash::HostCommandRequest::new(
                    format!("/workflow-run stop {}", run.run_id),
                    "Stop the active Workflow",
                ),
            });
        }
        let running: Vec<String> = agent
            .session
            .bg_tasks
            .values()
            .filter(|t| t.status == crate::app::session::BgTaskStatus::Running)
            .map(|t| t.task_id.clone())
            .collect();
        for task_id in running {
            if let Some(task) = agent.session.bg_tasks.get_mut(&task_id) {
                task.pending_kill = true;
                task.kill_requested_at = Some(std::time::Instant::now());
            }
            effects.push(Effect::KillBgTask {
                session_id: session_id.clone(),
                task_id,
            });
        }
        let scheduled: Vec<String> = agent.session.scheduled_tasks.keys().cloned().collect();
        for task_id in scheduled {
            agent.session.scheduled_tasks.remove(&task_id);
            effects.push(Effect::DeleteScheduledTask {
                session_id: session_id.clone(),
                task_id,
            });
        }
    }

    // Queued prompts are local (unsent), so dropping them needs no effect
    // and works even before the session exists (a just-dispatched row).
    let dropped_queue = !agent.session.pending_prompts.is_empty();
    if dropped_queue {
        agent.session.pending_prompts.clear();
        agent.sync_queue_pane();
    }

    (!effects.is_empty() || dropped_queue).then_some(effects)
}

pub(super) fn dispatch_dashboard_delete(app: &mut AppView) -> Vec<Effect> {
    let Some(row) = app.dashboard.as_ref().and_then(|d| d.selected.clone()) else {
        return vec![];
    };
    super::session::deletion::request(app, row)
}

pub(super) fn dispatch_dashboard_toggle_grouping(app: &mut AppView) -> Vec<Effect> {
    if let Some(d) = app.dashboard.as_mut() {
        d.toggle_grouping();
    }
    dispatch_dashboard_persist(app)
}

pub(super) fn dispatch_dashboard_select(app: &mut AppView, next: bool) {
    let Some(d) = app.dashboard.as_mut() else {
        return;
    };
    // We don't have rows cached here; reconstruct from agents.
    // Use the shared `cached_home()` instead of
    // re-reading the env var on every keystroke.
    let home = crate::views::dashboard::render::cached_home();
    // Same roster source the renderer uses, so navigation matches the visible
    // rows: leader roster in leader mode, local idle sessions otherwise.
    // Disjoint field borrows (`app.dashboard` is held mutably via `d`).
    let roster: &[crate::app::roster::RosterEntry] = if app.leader_mode {
        &app.leader_roster
    } else {
        &app.dashboard_local_sessions
    };
    let rows = crate::views::dashboard::build_rows_with_roster(
        &app.agents,
        &d.pinned,
        &d.reorder,
        None,
        d.grouping,
        &d.filter,
        home,
        roster,
    );
    // Unified, display-order cursor targets: section headers AND visible
    // rows (collapsed sections contribute only their header, so their
    // hidden rows are skipped). Placeholders are excluded.
    let focusables = crate::views::dashboard::render::focusables(
        &rows,
        d.grouping,
        &d.filter,
        &d.collapsed_sections,
        d.idle_show_all,
        d.search_mode,
    );
    let set_cursor = |d: &mut crate::views::dashboard::DashboardState,
                      f: &crate::views::dashboard::Focusable| {
        match f {
            crate::views::dashboard::Focusable::Section(key) => d.focus_section(*key),
            crate::views::dashboard::Focusable::Row(id) => d.focus_row(id.clone()),
            crate::views::dashboard::Focusable::IdleOverflow => d.focus_idle_overflow(),
        }
    };
    // Button-focused navigation contract:
    //   - Down on the button → first focusable (header or row).
    //   - Up on the button   → stay on the button (no wrap).
    if d.new_agent_button_focused {
        if next && !focusables.is_empty() {
            set_cursor(d, &focusables[0]);
            d.clear_manual_scroll();
        }
        return;
    }
    if focusables.is_empty() {
        // Nothing to land on AND button isn't focused → fall back to the
        // button so the cursor lives somewhere.
        d.focus_new_agent_button();
        return;
    }
    // Current index from the active cursor (section header or row).
    let cur = focusables
        .iter()
        .position(|f| match f {
            crate::views::dashboard::Focusable::Section(key) => d.selected_section == Some(*key),
            crate::views::dashboard::Focusable::Row(id) => d.selected.as_ref() == Some(id),
            crate::views::dashboard::Focusable::IdleOverflow => d.selected_idle_overflow,
        })
        .unwrap_or(0);
    // Up on the first focusable → focus the `[+ New session]` button.
    // The button acts as a sentinel above index 0, exactly like the
    // agent's tabs in the agents modal.
    if !next && cur == 0 {
        d.focus_new_agent_button();
        d.clear_manual_scroll();
        return;
    }
    let new = if next {
        (cur + 1).min(focusables.len() - 1)
    } else {
        cur.saturating_sub(1)
    };
    set_cursor(d, &focusables[new]);
    // Arrow-key nav is selection-driven: re-engage the clamp's
    // snap-to-selection so the viewport tracks the cursor. Without
    // this, ↑/↓ after a wheel scroll would leave the cursor visibly
    // selected on a row outside the viewport.
    d.clear_manual_scroll();
}

pub(super) fn dispatch_dashboard_reorder(app: &mut AppView, up: bool) -> Vec<Effect> {
    let Some(d) = app.dashboard.as_mut() else {
        return vec![];
    };
    let Some(sel) = d.selected.clone() else {
        return vec![];
    };
    // Maintain a stable explicit ordering list. The renderer applies
    // this list as a "float to position" rule inside the row's group.
    let pos = d.reorder.iter().position(|r| *r == sel);
    if up {
        match pos {
            Some(0) => {
                d.reorder.remove(0);
            }
            Some(i) => {
                d.reorder.swap(i, i - 1);
            }
            None => {
                d.reorder.insert(0, sel);
            }
        }
    } else {
        match pos {
            Some(i) if i + 1 < d.reorder.len() => {
                d.reorder.swap(i, i + 1);
            }
            Some(_) => {
                // Already at the bottom — append to end.
            }
            None => {
                d.reorder.push(sel);
            }
        }
    }
    dispatch_dashboard_persist(app)
}

fn dispatch_dashboard_persist(app: &mut AppView) -> Vec<Effect> {
    let Some(d) = app.dashboard.as_ref() else {
        return vec![];
    };
    // Don't hardcode `enabled = true`. Thread through
    // the on-disk value so a user who deliberately set `enabled =
    // false` doesn't get it silently overwritten on the next pin or
    // grouping toggle.
    let enabled = app
        .dashboard_persisted
        .as_ref()
        .map(|p| p.enabled)
        .unwrap_or(true);
    let resolver = crate::views::dashboard::SessionIdResolver::from_agents(&app.agents);
    let persisted = d.to_persisted(enabled, &resolver);
    app.dashboard_persisted = Some(persisted.clone());
    vec![Effect::PersistDashboard(persisted)]
}

use crate::app::actions::Effect;
use crate::app::root::dispatch::ctx::{get_active_agent, get_active_agent_mut};
use crate::app::root::{ActiveView, AppView, SessionPickerBinding, SessionPickerEntry};
use crate::views::modal::ActiveModal;
use crate::views::picker::PickerState;
use crate::views::session_picker::{
    PickerSelectionAnchor, capture_picker_selection, effective_filter_query, repo_name_from_cwd,
    restore_picker_selection,
};

use shell::session::unified_list::ListScope;

type SearchHit = shell::extensions::session_search::SearchSessionHit;

struct PickerSurface<'a> {
    entries: &'a mut Option<Vec<SessionPickerEntry>>,
    loading: &'a mut bool,
    state: &'a mut PickerState,
    content_results: &'a mut Option<Vec<SearchHit>>,
    content_loading: &'a mut bool,
    entries_query: &'a mut Option<String>,
    grouped: bool,
    current_repo: String,
}

impl PickerSurface<'_> {
    fn capture_selection(&self) -> PickerSelectionAnchor {
        capture_picker_selection(
            self.entries.as_deref(),
            self.content_results.as_deref(),
            self.state,
            effective_filter_query(self.state.query(), self.entries_query.as_deref()),
            self.grouped,
            *self.content_loading,
            Some(&self.current_repo),
        )
    }

    fn restore_selection(&mut self, anchor: PickerSelectionAnchor) {
        let filter_query =
            effective_filter_query(self.state.query(), self.entries_query.as_deref()).to_owned();
        restore_picker_selection(
            anchor,
            self.entries.as_deref(),
            self.content_results.as_deref(),
            self.state,
            &filter_query,
            self.grouped,
            *self.content_loading,
            Some(&self.current_repo),
        );
        self.state.expanded.clear();
    }

    fn native_loaded(
        &mut self,
        sessions: Vec<SessionPickerEntry>,
        query: Option<String>,
        empty_notice: String,
    ) -> Option<String> {
        let anchor = self.capture_selection();
        let is_search = query.is_some();
        *self.loading = false;
        if is_search {
            *self.content_loading = false;
        }
        *self.entries_query = query;
        *self.entries = Some(sessions);
        let notice = (!is_search && self.entries.as_ref().is_some_and(Vec::is_empty))
            .then_some(empty_notice);
        self.restore_selection(anchor);
        notice
    }

    fn native_failed(&mut self, error_notice: String, is_search: bool) -> Option<String> {
        let anchor = self.capture_selection();
        *self.loading = false;
        if is_search {
            *self.content_loading = false;
        }
        *self.entries = Some(Vec::new());
        *self.entries_query = None;
        self.restore_selection(anchor);
        Some(error_notice)
    }
}

fn current_picker_binding(app: &AppView) -> Option<SessionPickerBinding> {
    match app.active_view {
        ActiveView::Welcome => Some(SessionPickerBinding {
            root_agent_id: None,
            child_session_key: None,
            session_id: None,
            session_binding_epoch: None,
            cwd: app.cwd.clone(),
        }),
        ActiveView::Agent(root_agent_id) => {
            let root = app.agents.get(&root_agent_id)?;
            let child_session_key = if root.permission_queue.is_empty() {
                root.active_subagent
                    .as_ref()
                    .filter(|key| root.subagent_views.contains_key(*key))
                    .cloned()
            } else {
                None
            };
            let agent = get_active_agent(app)?;
            Some(SessionPickerBinding {
                root_agent_id: Some(root_agent_id),
                child_session_key,
                session_id: agent.session.session_id.clone(),
                session_binding_epoch: Some(agent.session_binding_epoch),
                cwd: agent.session.cwd.clone(),
            })
        }
        _ => None,
    }
}

fn picker_visible(app: &AppView) -> bool {
    if app.session_picker_list_binding.as_ref() != current_picker_binding(app).as_ref() {
        return false;
    }
    (matches!(app.active_view, ActiveView::Welcome)
        && (app.session_picker_loading || app.session_picker_entries.is_some()))
        || get_active_agent(app).is_some_and(|agent| {
            matches!(
                agent.active_modal.as_ref(),
                Some(ActiveModal::SessionPicker { .. })
            )
        })
}

pub(in crate::app::root::dispatch) fn refresh_pending_picker_after_owner_switch(
    app: &mut AppView,
) -> Vec<Effect> {
    let loading = match app.active_view {
        ActiveView::Welcome => app.session_picker_loading,
        ActiveView::Agent(_) => get_active_agent(app)
            .and_then(|agent| agent.active_modal.as_ref())
            .is_some_and(|modal| matches!(modal, ActiveModal::SessionPicker { loading: true, .. })),
        _ => false,
    };
    if !loading || app.session_picker_list_binding.as_ref() == current_picker_binding(app).as_ref()
    {
        return vec![];
    }
    dispatch_fetch_session_list(app)
}

pub(in crate::app::root::dispatch) fn dispatch_fetch_session_list(
    app: &mut AppView,
) -> Vec<Effect> {
    let Some(binding) = current_picker_binding(app) else {
        return vec![];
    };
    app.session_picker_list_seq += 1;
    app.session_picker_list_binding = Some(binding.clone());
    app.session_picker_detail_generation += 1;
    app.session_picker_loading = true;
    app.session_picker_entries = None;
    app.session_picker_state.selected = 0;
    app.session_picker_state.set_query("");
    app.session_picker_state.search_active = false;
    app.session_picker_state.expanded.clear();
    app.session_picker_content_results = None;
    app.session_picker_content_loading = false;
    app.session_picker_entries_query = None;
    vec![Effect::FetchSessionList {
        cwd: binding.cwd,
        query: None,
        seq: app.session_picker_list_seq,
        kind_filter: None,
    }]
}

pub(in crate::app::root::dispatch) fn handle_session_list_loaded(
    app: &mut AppView,
    sessions: Vec<SessionPickerEntry>,
    scope: ListScope,
    seq: u64,
    query: Option<String>,
) -> Vec<Effect> {
    if seq != app.session_picker_list_seq {
        return vec![];
    }
    if !picker_visible(app) {
        return vec![];
    }
    let Some(request_cwd) = app
        .session_picker_list_binding
        .as_ref()
        .map(|binding| binding.cwd.clone())
    else {
        return vec![];
    };
    app.session_picker_detail_generation += 1;
    let empty_notice = "No sessions found for this directory".to_owned();
    let is_browse = query.is_none();
    let mut sessions = Some(sessions);
    let mut notice = None;
    if let Some(agent) = get_active_agent_mut(app) {
        let current_repo = repo_name_from_cwd(&agent.session.cwd.to_string_lossy());
        if let Some(ActiveModal::SessionPicker {
            entries,
            loading,
            state,
            content_results,
            content_loading,
            entries_query,
            ..
        }) = agent.active_modal.as_mut()
        {
            notice = PickerSurface {
                entries,
                loading,
                state,
                content_results,
                content_loading,
                entries_query,
                grouped: true,
                current_repo,
            }
            .native_loaded(
                sessions.take().unwrap_or_default(),
                query.clone(),
                empty_notice.clone(),
            );
        }
    }
    if let Some(sessions) = sessions
        && matches!(app.active_view, crate::app::root::ActiveView::Welcome)
    {
        let current_repo = repo_name_from_cwd(&app.cwd.to_string_lossy());
        notice = PickerSurface {
            entries: &mut app.session_picker_entries,
            loading: &mut app.session_picker_loading,
            state: &mut app.session_picker_state,
            content_results: &mut app.session_picker_content_results,
            content_loading: &mut app.session_picker_content_loading,
            entries_query: &mut app.session_picker_entries_query,
            grouped: app.session_picker_grouped,
            current_repo,
        }
        .native_loaded(sessions, query, empty_notice);
    }
    if let Some(notice) = notice {
        app.show_toast(&notice);
    } else if scope.is_relaxed()
        && app.session_picker_relaxed_notified_for.as_deref() != Some(request_cwd.as_path())
    {
        // Welcome view drops toasts; don't consume the one-shot notice unless
        // it can render.
        if !matches!(app.active_view, crate::app::root::ActiveView::Welcome) {
            // Notify once per directory using the same cwd as the list request.
            app.session_picker_relaxed_notified_for = Some(request_cwd);
            let message = match scope {
                ListScope::Repo => {
                    "No sessions in this directory. Showing other sessions from this repository."
                }
                _ => "No sessions in this directory. Showing sessions from other directories.",
            };
            app.show_toast(message);
        }
    }
    // A cwd-scoped browse clears the latch so a later relax re-notifies; search
    // responses leave it alone.
    if !scope.is_relaxed() && is_browse {
        app.session_picker_relaxed_notified_for = None;
    }
    vec![]
}

pub(in crate::app::root::dispatch) fn handle_session_list_failed(
    app: &mut AppView,
    error: String,
    seq: u64,
    query: Option<String>,
) -> Vec<Effect> {
    if seq != app.session_picker_list_seq {
        return vec![];
    }
    if !picker_visible(app) {
        return vec![];
    }
    app.session_picker_detail_generation += 1;
    tracing::warn!(error = %error, "session list fetch failed");
    let error_notice = format!("Couldn't load sessions: {error}");
    let is_search = query.is_some();
    let mut handled = false;
    let mut notice = None;
    if let Some(agent) = get_active_agent_mut(app) {
        let current_repo = repo_name_from_cwd(&agent.session.cwd.to_string_lossy());
        if let Some(ActiveModal::SessionPicker {
            entries,
            loading,
            state,
            content_results,
            content_loading,
            entries_query,
            ..
        }) = agent.active_modal.as_mut()
        {
            notice = PickerSurface {
                entries,
                loading,
                state,
                content_results,
                content_loading,
                entries_query,
                grouped: true,
                current_repo,
            }
            .native_failed(error_notice.clone(), is_search);
            handled = true;
        }
    }
    if !handled && matches!(app.active_view, crate::app::root::ActiveView::Welcome) {
        let current_repo = repo_name_from_cwd(&app.cwd.to_string_lossy());
        notice = PickerSurface {
            entries: &mut app.session_picker_entries,
            loading: &mut app.session_picker_loading,
            state: &mut app.session_picker_state,
            content_results: &mut app.session_picker_content_results,
            content_loading: &mut app.session_picker_content_loading,
            entries_query: &mut app.session_picker_entries_query,
            grouped: app.session_picker_grouped,
            current_repo,
        }
        .native_failed(error_notice, is_search);
    }
    if let Some(notice) = notice {
        app.show_toast(&notice);
    }
    vec![]
}

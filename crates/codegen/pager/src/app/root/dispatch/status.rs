//! Session status, usage, and info dispatchers.

use agent_client_protocol as acp;

use super::ctx::get_active_agent;
use crate::app::actions::Effect;
use crate::app::agent_view::AgentView;
use crate::app::root::{ActiveView, AppView};
use crate::app::session::AgentId;
use crate::notifications::{NotificationEvent, NotificationEventKind};
use crate::scrollback::block::RenderBlock;
use crate::views::modal::ActiveModal;
use crate::views::usage_modal::{UsageModalState, UsageModalTab, UsageTabData};

/// Show session info: fetch via x.ai/session/info.
///
/// Fullscreen/inline opens the tabbed usage modal (Session Info tab) and fills
/// it when `TaskResult::SessionInfoComplete` arrives; minimal mode keeps the
/// legacy scrollback block (fetch nonce `0` = scrollback intent).
pub(super) fn dispatch_show_session_info(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        // No active session — error should have been caught by slash command,
        // but guard here just in case.
        return vec![];
    };

    if app.screen_mode.is_minimal() {
        vec![Effect::ShowSessionInfo {
            agent_id: id,
            session_id,
            show_resolved_model: app.show_resolved_model,
            nonce: 0,
        }]
    } else {
        open_usage_modal(app, id, UsageModalTab::SessionInfo, &session_id)
    }
}

pub(super) fn scrub_error_for_toast(error: &str) -> String {
    const MAX_TOAST_ERROR_LEN: usize = 120;
    if error.len() > MAX_TOAST_ERROR_LEN
        || error
            .chars()
            .any(crate::render::line_utils::is_unsafe_display_char)
    {
        "server error (see logs for details)".to_string()
    } else {
        error.to_string()
    }
}

/// Show context info: fetch via grow/session/info and display rich breakdown.
///
/// Fullscreen/inline opens the tabbed usage modal (Context tab) and fills it
/// when `TaskResult::ContextInfoComplete` arrives; minimal mode keeps the
/// legacy scrollback block (fetch nonce `0` = scrollback intent). Also the
/// entry point for a context-bar click.
pub(super) fn dispatch_show_context_info(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };

    if app.screen_mode.is_minimal() {
        vec![Effect::ShowContextInfo {
            agent_id: id,
            session_id,
            nonce: 0,
        }]
    } else {
        open_usage_modal(app, id, UsageModalTab::Context, &session_id)
    }
}

/// `/usage` — local session token and context usage.
///
/// Fullscreen/inline opens the tabbed usage modal (Usage tab); minimal mode
/// keeps the legacy scrollback block.
pub(super) fn dispatch_show_usage(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let session_id = {
        let Some(agent) = app.agents.get_mut(&id) else {
            return vec![];
        };
        agent.session.session_id.clone()
    };
    match session_id {
        Some(session_id) => {
            if app.screen_mode.is_minimal() {
                vec![Effect::FetchSessionUsage {
                    agent_id: id,
                    session_id,
                    nonce: 0,
                }]
            } else {
                open_usage_modal(app, id, UsageModalTab::Usage, &session_id)
            }
        }
        None => {
            if let Some(agent) = app.agents.get_mut(&id) {
                agent.scrollback.push_block(RenderBlock::system(
                    "Session usage is unavailable until the session starts.".to_string(),
                ));
            }
            vec![]
        }
    }
}

/// Open the tabbed usage modal on `tab` and fire all three tab fetches with a
/// fresh epoch. The modal state keeps the epoch; TaskResults carry it back and
/// are applied only while the same modal open is still present — a result from
/// before a close/reopen can never overwrite the newer request.
fn open_usage_modal(
    app: &mut AppView,
    agent_id: AgentId,
    tab: UsageModalTab,
    session_id: &acp::SessionId,
) -> Vec<Effect> {
    let nonce = crate::views::usage_modal::next_fetch_nonce();
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        agent.active_modal = Some(ActiveModal::Usage {
            state: UsageModalState::open(tab, nonce),
        });
    }
    vec![
        Effect::FetchSessionUsage {
            agent_id,
            session_id: session_id.clone(),
            nonce,
        },
        Effect::ShowContextInfo {
            agent_id,
            session_id: session_id.clone(),
            nonce,
        },
        Effect::ShowSessionInfo {
            agent_id,
            session_id: session_id.clone(),
            show_resolved_model: app.show_resolved_model,
            nonce,
        },
    ]
}

/// True when the agent's open usage modal carries the given fetch epoch.
fn usage_modal_matches(agent: &AgentView, nonce: u64) -> bool {
    nonce > 0
        && matches!(&agent.active_modal, Some(ActiveModal::Usage { state }) if state.fetch_nonce == nonce)
}

/// Commit a session-usage result either into the open usage modal (epoch
/// match) or, for scrollback-intent fetches (`nonce == 0`, minimal mode), as a
/// system block — still only if `session_id` matches. Stale epochs are dropped.
pub(super) fn apply_session_usage_result(
    app: &mut AppView,
    agent_id: AgentId,
    session_id: &acp::SessionId,
    result: Result<Box<shell::extensions::notification::PromptUsage>, String>,
    nonce: u64,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if agent.session.session_id.as_ref() != Some(session_id) {
        return vec![];
    }
    if nonce == 0 {
        let text = match &result {
            Ok(usage) => crate::app::status_blocks::session_usage_block_text(usage),
            Err(error) => format!("Couldn't load session usage: {error}"),
        };
        agent.scrollback.push_block(RenderBlock::system(text));
        return vec![];
    }
    if usage_modal_matches(agent, nonce) {
        let data = match result {
            Ok(usage) => UsageTabData::Loaded(usage),
            Err(error) => UsageTabData::Failed(error),
        };
        if let Some(ActiveModal::Usage { state }) = agent.active_modal.as_mut() {
            state.usage = data;
        }
    }
    vec![]
}

/// Commit a one-line "update available" notice into the active agent's
/// scrollback. Minimal mode has no welcome screen (the full TUI's update
/// surface), so the background update check's result is shown here instead
/// No-op when there is no active agent.
pub(crate) fn commit_minimal_update_notice(app: &mut AppView, latest_version: &str) {
    if let ActiveView::Agent(id) = app.active_view
        && let Some(agent) = app.agents.get_mut(&id)
    {
        agent.scrollback.push_block(RenderBlock::system(format!(
            "Update available: v{latest_version} — restart to apply."
        )));
    }
}

/// `/queue` — commit a read-only list of the queued prompts as a system block.
/// The text is built by [`crate::app::status_blocks::queue_block_text`]; this
/// just resolves the active agent and pushes it. Works in every render mode; the
/// primary inspection surface in minimal, which has no interactive `QueuePane`.
pub(super) fn dispatch_show_queue(app: &mut AppView) -> Vec<Effect> {
    if let ActiveView::Agent(id) = app.active_view
        && let Some(agent) = app.agents.get_mut(&id)
    {
        let text = crate::app::status_blocks::queue_block_text(agent);
        agent.scrollback.push_block(RenderBlock::system(text));
    }
    vec![]
}

/// `/tasks` — commit a read-only list of background tasks, subagents, and
/// scheduled (`/loop`) tasks as a system block. The text is built by
/// [`crate::app::status_blocks::tasks_block_text`]; this just resolves the
/// active agent and pushes it. Works in every render mode; the primary snapshot
/// surface in minimal, which has no interactive `TasksPane`.
pub(super) fn dispatch_show_tasks(app: &mut AppView) -> Vec<Effect> {
    if let ActiveView::Agent(id) = app.active_view
        && let Some(agent) = app.agents.get_mut(&id)
    {
        let text = crate::app::status_blocks::tasks_block_text(agent);
        agent.scrollback.push_block(RenderBlock::system(text));
    }
    vec![]
}

/// Open the hidden `/gboom` easter egg as a modal over the active agent
/// view. Requires a graphics-capable terminal (kitty protocol or iTerm2);
/// otherwise a toast explains why nothing happened. On session-less
/// surfaces (dashboard, welcome) this is a silent no-op.
///
/// Targets the top-level agent view (where the prompt lives), not a
/// focused subagent view: the modal's tick/draw plumbing runs on the
/// top-level view, like the other full-screen overlays.
pub(super) fn dispatch_open_gboom(app: &mut AppView) -> Vec<Effect> {
    use crate::terminal::image::{GraphicsProtocol, detect_graphics_protocol};
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    if detect_graphics_protocol() == GraphicsProtocol::None {
        agent.show_toast(
            "No demons here \u{2014} GBOOM needs a graphics-capable terminal \
             (kitty, Ghostty, WezTerm, iTerm2)",
        );
        return vec![];
    }
    // Close other media modals: they share the kitty placement id. Drop the
    // image viewer's in-flight loader too (its close path clears both —
    // a stale completion is rejected by the viewer owner id).
    agent.image_viewer = None;
    agent.gboom = Some(crate::gboom::GboomState::new());
    vec![]
}

/// Emit a `SessionReady` notification for the given agent.
///
/// Takes `&NotificationService` separately from `&AgentView` to avoid
/// borrow-checker conflicts when `agent` is borrowed from `app.agents`.
pub(super) fn notify_session_ready(
    notification_service: &crate::notifications::NotificationService,
    agent: &AgentView,
) {
    notification_service.notify(NotificationEvent {
        kind: NotificationEventKind::SessionReady,
        title: "Grow".into(),
        body: NotificationEventKind::SessionReady.as_str().into(),
        session_id: agent.session.session_id.as_ref().map(|s| s.0.to_string()),
    });
}

// TaskResult handlers.

/// `SessionInfoComplete` — refresh the agent's live context/agent-name state,
/// then route the payload: scrollback block for scrollback-intent fetches
/// (`nonce == 0`, minimal), usage-modal rows for a matching open modal epoch,
/// or drop when the modal was closed/reopened since the request.
pub(super) fn handle_session_info_complete(
    app: &mut AppView,
    agent_id: AgentId,
    info: Box<shell::session::SessionInfoResponse>,
    text: String,
    title: Option<String>,
    show_resolved_model: bool,
    nonce: u64,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    agent.session.apply_agent_name(info.data.agent_name.clone());
    if let Some(modal) = agent.agents_modal.as_mut() {
        modal.active_agent = info.data.agent_name.clone();
    }
    agent.apply_full_context_info(info.data.context.clone());
    if nonce == 0 {
        agent.scrollback.push_block(RenderBlock::system(text));
    } else if usage_modal_matches(agent, nonce) {
        let rows = crate::views::usage_modal::session_info_rows(
            &info,
            title.as_deref(),
            show_resolved_model,
        );
        if let Some(ActiveModal::Usage { state }) = agent.active_modal.as_mut() {
            state.session_info = UsageTabData::Loaded(rows);
        }
    }
    vec![]
}

/// `SessionInfoFailed` — same routing as [`handle_session_info_complete`].
pub(super) fn handle_session_info_failed(
    app: &mut AppView,
    agent_id: AgentId,
    error: String,
    nonce: u64,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if nonce == 0 {
        agent.scrollback.push_block(RenderBlock::system(format!(
            "Couldn't load session info: {error}"
        )));
    } else if usage_modal_matches(agent, nonce) {
        if let Some(ActiveModal::Usage { state }) = agent.active_modal.as_mut() {
            state.session_info = UsageTabData::Failed(error);
        }
    }
    vec![]
}

/// `ContextInfoComplete` — refresh the agent's live context state, then route
/// the payload: scrollback block (`nonce == 0`) or usage-modal Context tab.
pub(super) fn handle_context_info_complete(
    app: &mut AppView,
    agent_id: AgentId,
    info: Box<shell::session::SessionInfoResponse>,
    nonce: u64,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    let model = info.data.model.as_deref().unwrap_or("unknown").to_string();
    // Take ownership of the snapshot once, hand a clone to the agent's
    // running counters, then move the original into the display payload
    // (scrollback block or modal tab).
    let snapshot = info.data.context;
    agent.apply_full_context_info(snapshot.clone());
    if nonce == 0 {
        agent
            .scrollback
            .push_block(RenderBlock::context_info(snapshot, model));
    } else if usage_modal_matches(agent, nonce) {
        if let Some(ActiveModal::Usage { state }) = agent.active_modal.as_mut() {
            state.context = UsageTabData::Loaded(crate::views::usage_modal::ContextSnapshot {
                snapshot,
                model,
            });
        }
    }
    vec![]
}

/// `ContextInfoFailed` — same routing as [`handle_context_info_complete`].
pub(super) fn handle_context_info_failed(
    app: &mut AppView,
    agent_id: AgentId,
    error: String,
    nonce: u64,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if nonce == 0 {
        agent.scrollback.push_block(RenderBlock::system(format!(
            "Couldn't load context info: {error}"
        )));
    } else if usage_modal_matches(agent, nonce) {
        if let Some(ActiveModal::Usage { state }) = agent.active_modal.as_mut() {
            state.context = UsageTabData::Failed(error);
        }
    }
    vec![]
}

// Action handlers.

/// Copy a Session Info row value from the usage modal (mouse click / Enter).
/// Reuses the shared clipboard channel; the index addresses the rows rendered
/// for the open modal's Session Info tab.
pub(super) fn dispatch_copy_usage_modal_value(app: &mut AppView, index: usize) -> Vec<Effect> {
    let value = app
        .agents
        .values()
        .find_map(|agent| match &agent.active_modal {
            Some(ActiveModal::Usage { state }) => state.copy_value(index),
            _ => None,
        });
    if let Some(value) = value {
        let delivery = crate::clipboard::copy_text_or_file(&value);
        app.show_toast(delivery.toast_message().as_ref());
    }
    vec![]
}

pub(super) fn dispatch_copy_session_id(app: &mut AppView, index: usize) -> Vec<Effect> {
    use crate::views::modal::ActiveModal;
    // Try agent modal first, then fall back to app fields (welcome screen).
    let id = get_active_agent(app)
        .and_then(|agent| {
            if let Some(ActiveModal::SessionPicker {
                entries: Some(ref e),
                ..
            }) = agent.active_modal
            {
                e.get(index).map(|entry| entry.id.clone())
            } else {
                None
            }
        })
        .or_else(|| {
            app.session_picker_entries
                .as_ref()
                .and_then(|s| s.get(index))
                .map(|e| e.id.clone())
        });
    if let Some(id) = id {
        let delivery = crate::clipboard::copy_text_or_file(&id);
        app.show_toast(delivery.toast_message().as_ref());
    }
    vec![]
}

/// Open the onboarding tutorial overlay (top-level modal — works over both
/// the welcome screen and an agent session). Toggles: dispatching while
/// open closes instead of stacking.
pub(super) fn dispatch_open_tutorial(app: &mut AppView) -> Vec<Effect> {
    // Minimal mode has no modal host: the overlay would render nothing
    // while the app-level intercept swallowed all input.
    if app.screen_mode.is_minimal() {
        return vec![];
    }
    if app.tutorial.is_some() {
        app.tutorial = None;
        return vec![];
    }
    app.tutorial = Some(crate::views::tutorial::TutorialState::new());
    vec![]
}

pub(super) fn dispatch_show_release_notes(
    app: &mut AppView,
    title: String,
    content: String,
) -> Vec<Effect> {
    match app.active_view {
        ActiveView::Agent(id) => {
            if let Some(agent) = app.agents.get_mut(&id) {
                agent.active_modal = Some(crate::views::modal::ActiveModal::DocViewer {
                    title,
                    content,
                    scroll: 0,
                    window: crate::views::modal_window::ModalWindowState::new(),
                    cached_lines: None,
                    previous_palette: None,
                    standalone: true,
                });
            }
        }
        // The welcome screen no longer hosts a doc viewer (release notes open
        // inside a session via `/release-notes`).
        _ => {}
    }
    vec![]
}

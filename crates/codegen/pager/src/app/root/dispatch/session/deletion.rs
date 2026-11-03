//! One persistent, target-bound confirmation for session history deletion.
use crate::app::actions::{Action, AfterSessionDelete, Effect};
use crate::app::root::{ActiveView, AppView, InputOutcome};
use crate::app::session::AgentId;
use crate::views::dashboard::DashboardRowId;
use crate::views::modal_window::{self as mw, ModalWindowState};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

#[derive(Debug)]
pub(crate) struct SessionDeletion {
    session_id: String,
    cwd: String,
    title: String,
    bindings: Vec<(AgentId, u32)>,
    window: ModalWindowState,
}

pub(in crate::app::root::dispatch) fn request_current(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    if app
        .agents
        .get(&id)
        .is_some_and(|agent| agent.active_subagent.is_some())
    {
        app.show_toast("Return to the parent session to delete its history");
        return vec![];
    }
    request(app, DashboardRowId::TopLevel(id))
}

pub(in crate::app::root::dispatch) fn request(
    app: &mut AppView,
    row: DashboardRowId,
) -> Vec<Effect> {
    let target = match row {
        DashboardRowId::TopLevel(id) => app.agents.get(&id).and_then(|agent| {
            if !crate::views::dashboard::classify_top_level(agent).allows_delete() {
                return None;
            }
            Some((
                agent.session.session_id.as_ref()?.to_string(),
                agent.session.cwd.display().to_string(),
                crate::views::session_title::entry_title(agent),
            ))
        }),
        DashboardRowId::Roster { session_id } => app
            .leader_roster
            .iter()
            .chain(app.dashboard_local_sessions.iter())
            .find(|entry| {
                entry.session_id == session_id
                    && crate::views::dashboard::roster_activity_to_state(entry.activity)
                        .allows_delete()
            })
            .map(|entry| {
                (
                    session_id,
                    entry.cwd.clone(),
                    entry
                        .title
                        .clone()
                        .unwrap_or_else(|| "Session history".into()),
                )
            }),
        DashboardRowId::Subagent { .. } => None,
    };
    let Some((session_id, cwd, title)) = target else {
        app.show_toast(
            "Only saved, idle sessions have history to delete; use Close session for drafts",
        );
        return vec![];
    };
    app.session_deletion = Some(SessionDeletion {
        bindings: super::modal::session_delete_bindings(app, &session_id),
        session_id,
        cwd,
        title,
        window: ModalWindowState::new(),
    });
    vec![]
}

pub(in crate::app::root::dispatch) fn confirm(app: &mut AppView) -> Vec<Effect> {
    let Some(target) = app.session_deletion.take() else {
        return vec![];
    };
    // A rebound/replaced view must require a fresh confirmation. Also check
    // newly opened local writers; the server remains canonical for disk state.
    if super::modal::session_delete_bindings(app, &target.session_id) != target.bindings
        || target.bindings.iter().any(|(id, epoch)| {
            app.agents.get(id).is_none_or(|agent| {
                agent.session_binding_epoch != *epoch
                    || !crate::views::dashboard::classify_top_level(agent).allows_delete()
            })
        })
        || app
            .leader_roster
            .iter()
            .chain(app.dashboard_local_sessions.iter())
            .any(|entry| {
                entry.session_id == target.session_id
                    && !crate::views::dashboard::roster_activity_to_state(entry.activity)
                        .allows_delete()
            })
    {
        app.show_toast("Session changed or became busy; review it before deleting");
        return vec![];
    }
    app.show_toast("Deleting session…");
    vec![Effect::DeleteSession {
        session_id: target.session_id,
        cwd: target.cwd,
        bindings: target.bindings,
        after: AfterSessionDelete::Stay,
    }]
}

impl SessionDeletion {
    pub(crate) fn handle_input(&mut self, event: &Event) -> InputOutcome {
        let action = match event {
            Event::Key(key)
                if key.kind == KeyEventKind::Press && key.modifiers == KeyModifiers::NONE =>
            {
                match key.code {
                    KeyCode::Char('y') => Some(Action::ConfirmSessionDelete),
                    KeyCode::Char('n') | KeyCode::Esc => Some(Action::CancelSessionDelete),
                    _ => None,
                }
            }
            Event::Mouse(mouse) => {
                match mw::handle_modal_mouse(&mut self.window, mouse.kind, mouse.column, mouse.row)
                {
                    mw::ModalWindowOutcome::ShortcutActivated(1) => {
                        Some(Action::ConfirmSessionDelete)
                    }
                    mw::ModalWindowOutcome::CloseRequested
                    | mw::ModalWindowOutcome::ShortcutActivated(0) => {
                        Some(Action::CancelSessionDelete)
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        action
            .map(InputOutcome::Action)
            .unwrap_or(InputOutcome::Changed)
    }
    pub(crate) fn render(
        &mut self,
        buf: &mut ratatui::buffer::Buffer,
        area: ratatui::layout::Rect,
    ) {
        use ratatui::widgets::{Paragraph, Widget, Wrap};
        let shortcuts = [
            mw::Shortcut {
                label: "Y Delete permanently",
                clickable: true,
                id: 1,
            },
            mw::Shortcut {
                label: "N / Esc Cancel",
                clickable: true,
                id: 0,
            },
        ];
        let config = mw::ModalWindowConfig {
            title: "Delete session history?",
            tabs: None,
            shortcuts: &shortcuts,
            sizing: mw::ModalSizing {
                width_pct: 0.85,
                max_width: 90,
                min_width: 30,
                v_margin: 2,
                h_pad: 2,
                v_pad: 1,
                footer_lines: 2,
            },
            fold_info: None,
        };
        if let Some(content) = mw::render_modal_window(
            buf,
            area,
            &mut self.window,
            &config,
            &crate::theme::Theme::current(),
        ) {
            let title = crate::views::session_title::sanitize_display_text(&self.title);
            Paragraph::new(format!("{title}\n{}\n\nPermanently remove this session's history.\nCannot be undone.\nForked sessions keep their own history.", self.session_id))
                .wrap(Wrap { trim: false }).render(content.content, buf);
        }
    }
}

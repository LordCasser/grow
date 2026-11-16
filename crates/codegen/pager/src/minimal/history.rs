//! Minimal history browser.
//!
//! PageUp opens the active agent's existing scrollback on a temporary
//! alternate screen. The screen holds no second transcript: paging calls
//! [`AgentView::request_history_at_viewport_boundary`] and native commit
//! state is left untouched. Esc, a new permission/question/plan surface,
//! or the shared quit confirmation leaves that screen before any live paint.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::QueueableCommand as _;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Widget as _};

use crate::app::PagerTerminal;
use crate::app::agent_view::AgentView;
use crate::app::root::{ActiveView, AppView, InputOutcome};
use crate::minimal_api::{self, TranscriptOwner};
use crate::motion::FrameStamp;
use crate::scrollback::ScrollbackPane;
use crate::views::agent::{ActivePane, PaneAreas};

/// Alternate-screen host bound to one visible transcript owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryBrowser {
    owner: TranscriptOwner,
    /// Root and visible view bindings; independent of the native printed owner.
    binding_epochs: (u32, u32),
    phase: HistoryPhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HistoryPhase {
    /// PageUp requested the screen; the enter write has not succeeded.
    Entering,
    /// Alternate screen is showing `saved` inline viewport underneath.
    Active { saved: Rect },
    /// Leave was requested; the leave write has not succeeded.
    Leaving { saved: Rect },
}

enum Nav {
    PageUp,
    PageDown,
    Up,
    Down,
}

/// Terminal resource responsibility survives the AppView and its draw phase.
/// Arm before submitting Enter: a failed write or flush may already have
/// switched the terminal. Only a successfully flushed Leave releases it.
static HISTORY_ALT_SCREEN: AtomicBool = AtomicBool::new(false);

/// Exit and panic teardown must leave before restoring the native cursor,
/// including when a confirmed Quit skips the next browser frame.
pub(crate) fn leave_history_alt_screen_for_teardown(writer: &mut impl io::Write) -> io::Result<()> {
    if HISTORY_ALT_SCREEN.load(Ordering::Acquire) {
        write_alternate(writer, false)?;
    }
    Ok(())
}

/// Drop or arm a leave when the browser must yield. Entering has not adopted
/// a terminal viewport, so it can be discarded; any partially submitted Enter
/// remains covered by the resource latch. Active waits for a successful Leave.
pub(crate) fn yield_history_browser(app: &mut AppView) {
    reconcile_owner(app);
    let phase = app
        .minimal_state
        .history_browser
        .as_ref()
        .map(|browser| browser.phase.clone());
    match phase {
        Some(HistoryPhase::Entering) => app.minimal_state.history_browser = None,
        Some(HistoryPhase::Active { saved }) => {
            if let Some(browser) = app.minimal_state.history_browser.as_mut() {
                browser.phase = HistoryPhase::Leaving { saved };
            }
        }
        Some(HistoryPhase::Leaving { .. }) | None => {}
    }
}

/// Minimal PageUp / history-screen keys. `None` lets the ordinary Minimal
/// path handle the event.
pub(crate) fn handle_history_key(app: &mut AppView, ev: &Event) -> Option<InputOutcome> {
    let Event::Key(key) = ev else {
        return None;
    };
    if key.kind == KeyEventKind::Release || key.modifiers != KeyModifiers::NONE {
        return None;
    }
    reconcile_owner(app);
    if priority_surface(app) {
        yield_history_browser(app);
        return None;
    }
    let browsing = app.minimal_state.history_browser.is_some();
    if key.code == KeyCode::Esc && browsing {
        yield_history_browser(app);
        return Some(InputOutcome::Changed);
    }
    let Some(nav) = nav_key(key.code, browsing) else {
        // The history screen owns unmodified keys. Modified chords stay on the
        // normal path so quit confirmation and Minimal intercepts still run.
        return browsing.then_some(InputOutcome::Changed);
    };
    if !browsing {
        let owner = minimal_api::minimal_visible_owner(app)?;
        let binding_epochs = binding_epochs(app, &owner)?;
        app.minimal_state.history_browser = Some(HistoryBrowser {
            owner,
            binding_epochs,
            phase: HistoryPhase::Entering,
        });
    } else if matches!(
        app.minimal_state
            .history_browser
            .as_ref()
            .map(|browser| &browser.phase),
        Some(HistoryPhase::Leaving { .. })
    ) {
        return Some(InputOutcome::Changed);
    }
    scroll_history(app, nav);
    Some(InputOutcome::Changed)
}

/// Apply a pending screen switch and, once it is up, paint the shared
/// scrollback. A failed write leaves the phase unchanged.
pub(crate) fn present(
    app: &mut AppView,
    terminal: &mut PagerTerminal,
    frame: FrameStamp,
) -> io::Result<bool> {
    reconcile_owner(app);
    if priority_surface(app) {
        yield_history_browser(app);
    }
    let Some(browser) = app.minimal_state.history_browser.clone() else {
        // An Enter write can reach the terminal before reporting a failure.
        // If input retired that still-Entering browser, leave before live paint.
        leave_history_alt_screen_for_teardown(terminal.backend_mut())?;
        return Ok(false);
    };
    match browser.phase {
        HistoryPhase::Entering => {
            let saved = terminal.viewport_area();
            switch_alternate(terminal, true)?;
            adopt_fullscreen(terminal);
            if let Some(slot) = app.minimal_state.history_browser.as_mut()
                && slot.owner == browser.owner
            {
                slot.phase = HistoryPhase::Active { saved };
            }
            paint(app, terminal, frame);
            Ok(true)
        }
        HistoryPhase::Active { .. } => {
            adopt_fullscreen(terminal);
            paint(app, terminal, frame);
            Ok(true)
        }
        HistoryPhase::Leaving { saved } => {
            switch_alternate(terminal, false)?;
            terminal.set_viewport_area(saved);
            if app
                .minimal_state
                .history_browser
                .as_ref()
                .is_some_and(|slot| slot.owner == browser.owner)
            {
                app.minimal_state.history_browser = None;
            }
            Ok(false)
        }
    }
}

fn nav_key(code: KeyCode, browsing: bool) -> Option<Nav> {
    match code {
        KeyCode::PageUp => Some(Nav::PageUp),
        KeyCode::PageDown if browsing => Some(Nav::PageDown),
        KeyCode::Up if browsing => Some(Nav::Up),
        KeyCode::Down if browsing => Some(Nav::Down),
        _ => None,
    }
}

fn scroll_history(app: &mut AppView, nav: Nav) {
    let ActiveView::Agent(id) = app.active_view else {
        return;
    };
    let mut effects = Vec::new();
    if let Some(root) = app.agents.get_mut(&id) {
        let agent = minimal_api::visible_agent_mut(root);
        prepare_browser_viewport(agent);
        let forward = match nav {
            Nav::PageUp => {
                agent.scrollback.page_up();
                false
            }
            Nav::PageDown => {
                agent.scrollback.page_down();
                true
            }
            Nav::Up => {
                agent.scrollback.scroll_up(1);
                false
            }
            Nav::Down => {
                agent.scrollback.scroll_down(1);
                true
            }
        };
        agent.request_history_at_viewport_boundary(forward, &mut effects);
    }
    app.pending_effects.extend(effects);
}

fn prepare_browser_viewport(agent: &mut AgentView) {
    let (width, height) = agent.last_terminal_size;
    if width == 0 || height <= 1 {
        return;
    }
    let pane = Rect {
        x: 0,
        y: 0,
        width,
        height: height - 1,
    };
    // Wheel routing hit-tests the last painted pane. Point it at the history
    // screen before the next frame so the shared scroll path reaches this view.
    agent.pane_areas = PaneAreas {
        scrollback: pane,
        ..PaneAreas::default()
    };
    agent.active_pane = ActivePane::Scrollback;
    agent.scrollback.prepare_layout(pane.width, pane.height);
}

fn binding_epochs(app: &AppView, owner: &TranscriptOwner) -> Option<(u32, u32)> {
    let root = app.agents.get(&owner.root_agent)?;
    let visible = minimal_api::app_visible_agent(app, owner.root_agent)?;
    Some((root.session_binding_epoch, visible.session_binding_epoch))
}

fn reconcile_owner(app: &mut AppView) {
    let current = minimal_api::minimal_visible_owner(app);
    let Some(browser) = app.minimal_state.history_browser.as_ref() else {
        return;
    };
    if current.as_ref() == Some(&browser.owner)
        && binding_epochs(app, &browser.owner) == Some(browser.binding_epochs)
    {
        return;
    }
    match browser.phase {
        HistoryPhase::Entering => app.minimal_state.history_browser = None,
        HistoryPhase::Active { saved } => {
            if let Some(browser) = app.minimal_state.history_browser.as_mut() {
                browser.phase = HistoryPhase::Leaving { saved };
            }
        }
        HistoryPhase::Leaving { .. } => {}
    }
}

fn priority_surface(app: &AppView) -> bool {
    if app.exit_suspension.is_some()
        || app.session_deletion.is_some()
        || app.new_worktree_dialog.is_some()
        || app.tutorial.is_some()
        || app.pending_action.as_ref().is_some_and(|pending| {
            !pending.expired()
                && matches!(
                    pending.action,
                    crate::app::actions::Action::Quit
                        | crate::app::actions::Action::QuitConfirmed
                        | crate::app::actions::Action::ForceQuit
                )
        })
    {
        return true;
    }
    let ActiveView::Agent(id) = app.active_view else {
        return false;
    };
    let Some(root) = app.agents.get(&id) else {
        return false;
    };
    if root.extensions_modal.is_some() || root.active_modal.is_some() {
        return true;
    }
    let visible = root
        .active_subagent
        .as_ref()
        .filter(|_| root.permission_queue.is_empty())
        .and_then(|key| root.subagent_views.get(key))
        .map_or(root, |child| child.as_ref());
    !visible.permission_queue.is_empty()
        || visible.question_view.is_some()
        || visible.plan_approval_view.is_some()
}

fn switch_alternate(terminal: &mut PagerTerminal, enter: bool) -> io::Result<()> {
    write_alternate(terminal.backend_mut(), enter)
}

fn write_alternate(writer: &mut impl io::Write, enter: bool) -> io::Result<()> {
    if enter {
        HISTORY_ALT_SCREEN.store(true, Ordering::Release);
        writer.queue(EnterAlternateScreen)?;
    } else {
        writer.queue(LeaveAlternateScreen)?;
    }
    writer.flush()?;
    if !enter {
        HISTORY_ALT_SCREEN.store(false, Ordering::Release);
    }
    Ok(())
}

fn adopt_fullscreen(terminal: &mut PagerTerminal) {
    let screen = terminal.last_known_area();
    if screen.width == 0 || screen.height == 0 {
        return;
    }
    terminal.set_viewport_area(Rect {
        x: 0,
        y: 0,
        width: screen.width,
        height: screen.height,
    });
}

fn paint(app: &mut AppView, terminal: &mut PagerTerminal, frame: FrameStamp) {
    let active_id = match app.active_view {
        ActiveView::Agent(id) => Some(id),
        _ => None,
    };
    let AppView {
        cursor,
        agents,
        scratch,
        ..
    } = app;
    crate::render::draw::draw_frame(terminal, cursor, |ui, _| {
        let area = ui.area();
        let buf = ui.buffer_mut();
        if area.width == 0 || area.height == 0 {
            return (None, None);
        }
        Clear.render(area, buf);
        let hint_h = u16::from(area.height > 0);
        let pane = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height.saturating_sub(hint_h),
        };
        if let Some(id) = active_id
            && let Some(root) = agents.get_mut(&id)
        {
            let agent = minimal_api::visible_agent_mut(root);
            agent.pane_areas = PaneAreas {
                scrollback: pane,
                ..PaneAreas::default()
            };
            agent.active_pane = ActivePane::Scrollback;
            if pane.width > 0 && pane.height > 0 {
                agent.scrollback.prepare_layout(pane.width, pane.height);
                let _ = ScrollbackPane::new()
                    .active(true)
                    .with_frame(frame)
                    .render_with_scratch(pane, buf, &agent.scrollback, scratch);
            }
        }
        if hint_h > 0 {
            let theme = crate::theme::Theme::current();
            let row = Rect {
                x: area.x,
                y: area.y + pane.height,
                width: area.width,
                height: 1,
            };
            buf.set_style(row, theme.dim());
            buf.set_string(row.x, row.y, "History  PgUp/PgDn  Esc live", theme.dim());
        }
        (None, None)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ScreenMode;
    use crate::app::actions::{Action, Effect, TaskResult};
    use crate::app::root::dispatch;
    use crate::app::session::AgentId;
    use acp_transport::protocol as acp;
    use crossterm::event::{KeyEvent, KeyModifiers};
    use shell::session::storage::{
        HistoryEntity, HistoryPageQuery, HistoryPageResponse, HistoryRow, HistorySourceKey,
        SessionUpdate,
    };

    const WIDTH: u16 = 40;
    const VIEWPORT: u16 = 4;

    fn page_key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn text_row(session: &acp::SessionId, record: u64, text: &str) -> HistoryRow {
        HistoryRow {
            source: HistorySourceKey { record, part: 0 },
            entity: Some(HistoryEntity::Message {
                event_seq: record,
                item_ordinal: 0,
            }),
            fragment: None,
            update: SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
                session.clone(),
                acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                    acp::ContentBlock::Text(acp::TextContent::new(text.to_string())),
                )),
            ))),
        }
    }

    fn page_response(
        session: &acp::SessionId,
        rows: Vec<HistoryRow>,
        older_cursor: Option<&str>,
        newer_cursor: Option<&str>,
    ) -> HistoryPageResponse {
        HistoryPageResponse {
            session_id: session.clone(),
            cut: "cut".to_string(),
            rows,
            range: None,
            older_cursor: older_cursor.map(str::to_string),
            newer_cursor: newer_cursor.map(str::to_string),
            target_source: None,
        }
    }

    fn minimal_app(session_text: &str) -> (AppView, AgentId, acp::SessionId) {
        let mut app = AppView::test_app_with_agent();
        let id = AgentId(0);
        let session_id = acp::SessionId::new(session_text);
        app.screen_mode = ScreenMode::Minimal;
        {
            let agent = app.agents.get_mut(&id).unwrap();
            agent.session.session_id = Some(session_id.clone());
            agent.history_cut = Some("cut".into());
            agent.history_page_request = Some(1);
            agent.history_page_request_seq = 1;
            agent
                .scrollback
                .push_block(crate::scrollback::block::RenderBlock::notice("RECENT-LIVE"));
            agent.scrollback.prepare_layout(WIDTH, VIEWPORT);
            agent.scrollback.mark_committed(0);
        }
        let effects = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: 0,
                request_id: 1,
                generation: 0,
                query: HistoryPageQuery::Latest,
                result: Ok(page_response(
                    &session_id,
                    vec![
                        text_row(&session_id, 32, "reply three"),
                        text_row(&session_id, 31, "reply two"),
                        text_row(&session_id, 30, "reply one"),
                    ],
                    Some("older-a"),
                    None,
                )),
            }),
            &mut app,
        );
        assert!(effects.is_empty(), "latest adoption must not emit effects");
        app.agents
            .get_mut(&id)
            .unwrap()
            .scrollback
            .prepare_layout(WIDTH, VIEWPORT);
        let recent = recent_id(&app, id);
        assert!(
            app.agents[&id].scrollback.is_committed(recent),
            "adopting the latest page must keep the already printed tail"
        );
        (app, id, session_id)
    }

    fn recent_id(app: &AppView, id: AgentId) -> crate::scrollback::entry::EntryId {
        app.agents[&id]
            .scrollback
            .iter_entries()
            .find_map(|(entry_id, entry)| match &entry.block {
                crate::scrollback::block::RenderBlock::Notice(notice)
                    if notice.text.contains("RECENT-LIVE") =>
                {
                    Some(entry_id)
                }
                _ => None,
            })
            .expect("recent live tail")
    }

    fn committed_ids(app: &AppView, id: AgentId) -> Vec<crate::scrollback::entry::EntryId> {
        app.agents[&id]
            .scrollback
            .iter_entries()
            .filter(|(entry_id, _)| app.agents[&id].scrollback.is_committed(*entry_id))
            .map(|(entry_id, _)| entry_id)
            .collect()
    }

    fn history_request(effects: &[Effect]) -> Option<&Effect> {
        effects
            .iter()
            .find(|effect| matches!(effect, Effect::ReadHistoryPage { .. }))
    }

    #[test]
    fn page_up_enters_browser_and_adopts_before_without_resetting_native_frontier() {
        let (mut app, id, session_id) = minimal_app("minimal-before");
        let printed = minimal_api::minimal_printed_owner(&app).cloned();
        let recent = recent_id(&app, id);
        let committed_before = committed_ids(&app, id);

        app.handle_input(&page_key(KeyCode::PageUp));
        let request = history_request(&app.pending_effects)
            .expect("PageUp at the loaded window must emit the original page request");
        let Effect::ReadHistoryPage {
            request_id,
            generation,
            binding_epoch,
            query: HistoryPageQuery::Before { cursor },
            ..
        } = request
        else {
            panic!("PageUp must request Before, got {request:?}");
        };
        assert_eq!(cursor, "older-a");
        let (request_id, generation, binding_epoch) = (*request_id, *generation, *binding_epoch);
        app.pending_effects.clear();

        let effects = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Before {
                    cursor: "older-a".into(),
                },
                result: Ok(page_response(
                    &session_id,
                    vec![text_row(&session_id, 20, "OLDER-PAGE")],
                    Some("older-b"),
                    None,
                )),
            }),
            &mut app,
        );
        assert!(
            matches!(
                effects.as_slice(),
                [Effect::ReadHistoryPage {
                    query: HistoryPageQuery::Before { cursor },
                    request_id: next_id,
                    ..
                }] if cursor == "older-b"
                    && Some(*next_id) == app.agents[&id].history_page_request
            ),
            "the visible older boundary prefetches one page: {effects:?}"
        );
        let agent = &app.agents[&id];
        let text = agent
            .scrollback
            .iter_entries()
            .filter_map(|(_, entry)| entry.block.searchable_text())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            text.matches("OLDER-PAGE").count(),
            1,
            "older content is resident exactly once"
        );
        assert!(agent.scrollback.is_committed(recent));
        let committed_after = committed_ids(&app, id);
        assert!(
            committed_after.len() >= committed_before.len(),
            "extending an existing response need not create another render block"
        );
        assert!(
            agent.scrollback.iter_entries().any(|(id, entry)| {
                entry
                    .block
                    .searchable_text()
                    .is_some_and(|text| text.contains("OLDER-PAGE"))
                    && agent.scrollback.is_committed(id)
            }),
            "the adopted older content is already committed, so it cannot print natively"
        );
        assert_eq!(minimal_api::minimal_printed_owner(&app).cloned(), printed);
        assert!(app.minimal_state.history_browser.is_some());
        assert!(
            matches!(
                app.minimal_state
                    .history_browser
                    .as_ref()
                    .map(|browser| &browser.phase),
                Some(HistoryPhase::Entering)
            ),
            "browsing must not begin a new native epoch"
        );

        let (request_id, generation, binding_epoch) = {
            let agent = app.agents.get_mut(&id).unwrap();
            let request_id = agent.history_page_request_seq + 1;
            agent.history_page_request = Some(request_id);
            agent.history_page_request_seq = request_id;
            agent.scrollback.prepare_layout(WIDTH, VIEWPORT);
            (
                request_id,
                agent.history_generation,
                agent.session_binding_epoch,
            )
        };
        let effects = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Around {
                    target: "opaque".into(),
                },
                result: Ok(page_response(
                    &session_id,
                    vec![text_row(&session_id, 40, "AROUND-PAGE")],
                    Some("older-j"),
                    Some("newer-j"),
                )),
            }),
            &mut app,
        );
        assert!(effects.is_empty());
        app.agents
            .get_mut(&id)
            .unwrap()
            .scrollback
            .prepare_layout(WIDTH, VIEWPORT);
        app.handle_input(&page_key(KeyCode::PageDown));
        let after = history_request(&app.pending_effects)
            .expect("PageDown at the newer end must emit After");
        assert!(
            matches!(
                after,
                Effect::ReadHistoryPage { query: HistoryPageQuery::After { cursor }, .. }
                    if cursor == "newer-j"
            ),
            "got {after:?}"
        );
        assert!(app.agents[&id].scrollback.is_committed(recent));
    }

    #[test]
    fn esc_permission_and_owner_switch_retire_the_browser_without_resetting_commit() {
        let (mut app, id, _session) = minimal_app("minimal-retire");
        let recent = recent_id(&app, id);
        let printed_owner = minimal_api::minimal_visible_owner(&app);
        app.minimal_state.visible_owner = printed_owner.clone();
        app.handle_input(&page_key(KeyCode::PageUp));
        assert!(app.minimal_state.history_browser.is_some());
        app.handle_input(&page_key(KeyCode::Esc));
        assert!(app.minimal_state.history_browser.is_none());
        assert_eq!(
            minimal_api::minimal_printed_owner(&app),
            printed_owner.as_ref()
        );
        assert!(app.agents[&id].scrollback.is_committed(recent));

        app.handle_input(&page_key(KeyCode::PageUp));
        app.agents.get_mut(&id).unwrap().session_binding_epoch += 1;
        reconcile_owner(&mut app);
        assert!(app.minimal_state.history_browser.is_none());
        assert_eq!(
            minimal_api::minimal_printed_owner(&app),
            printed_owner.as_ref()
        );
        assert!(app.agents[&id].scrollback.is_committed(recent));

        app.handle_input(&page_key(KeyCode::PageUp));
        app.agents.get_mut(&id).unwrap().session.session_id =
            Some(acp::SessionId::new("other-session"));
        app.handle_input(&page_key(KeyCode::PageDown));
        assert!(
            app.minimal_state.history_browser.is_none(),
            "a new session binding retires an unentered browser"
        );

        app.agents.get_mut(&id).unwrap().session.session_id =
            Some(acp::SessionId::new("minimal-retire"));
        minimal_api::set_plan_approval_for_test(
            app.agents.get_mut(&id).unwrap(),
            minimal_api::test_plan_approval(
                crate::views::plan_approval_view::PlanApprovalFocus::Preview,
            ),
        );
        let pending_before = app.pending_effects.len();
        app.handle_input(&page_key(KeyCode::PageUp));
        assert!(app.minimal_state.history_browser.is_none());
        assert_eq!(app.pending_effects.len(), pending_before);
        assert!(app.agents[&id].scrollback.is_committed(recent));
    }

    #[test]
    fn root_or_child_binding_epoch_retires_the_selected_child_browser() {
        for rebind_root in [true, false] {
            let (mut app, id, _session) = minimal_app("minimal-child-binding");
            let recent = recent_id(&app, id);
            let mut child = minimal_api::test_agent_view(Some("child"), "/tmp/child".into());
            child.session.id = AgentId(1);
            child
                .scrollback
                .push_block(crate::scrollback::block::RenderBlock::notice(
                    "printed child",
                ));
            child.scrollback.mark_committed(0);
            let child_recent = child.scrollback.get(0).unwrap().id;
            let root = app.agents.get_mut(&id).unwrap();
            root.subagent_views.insert("child".into(), Box::new(child));
            root.active_subagent = Some("child".into());
            let printed_owner = minimal_api::minimal_visible_owner(&app);
            app.minimal_state.visible_owner = printed_owner.clone();

            app.handle_input(&page_key(KeyCode::PageUp));
            assert!(app.minimal_state.history_browser.is_some());
            let root = app.agents.get_mut(&id).unwrap();
            if rebind_root {
                root.session_binding_epoch += 1;
            } else {
                root.subagent_views
                    .get_mut("child")
                    .unwrap()
                    .session_binding_epoch += 1;
            }
            reconcile_owner(&mut app);
            assert!(app.minimal_state.history_browser.is_none());
            assert_eq!(
                minimal_api::minimal_printed_owner(&app),
                printed_owner.as_ref()
            );
            assert!(app.agents[&id].scrollback.is_committed(recent));
            assert!(
                app.agents[&id].subagent_views["child"]
                    .scrollback
                    .is_committed(child_recent)
            );
        }
    }

    #[test]
    fn double_quit_keeps_the_alternate_screen_for_teardown_without_a_draw() {
        use crate::render::draw::{TermWriter, WriterSync};

        let (mut app, _id, _session) = minimal_app("minimal-quit-batch");
        let owner = minimal_api::minimal_visible_owner(&app).unwrap();
        let printed_owner = minimal_api::minimal_printed_owner(&app).cloned();
        let saved = Rect::new(0, 20, WIDTH, VIEWPORT);
        let (tx, _rx) = std::sync::mpsc::channel();
        let writer = TermWriter::new(tx, WriterSync::new()).unwrap();
        let mut terminal = ratatui_inline::Terminal::with_options(
            ratatui::backend::CrosstermBackend::new(writer),
            ratatui::TerminalOptions {
                viewport: ratatui::Viewport::Fixed(Rect::new(0, 0, WIDTH, 24)),
            },
        )
        .unwrap();
        terminal.set_viewport_area(saved);
        let mut wire = Vec::new();
        leave_history_alt_screen_for_teardown(&mut wire).unwrap();
        assert!(wire.is_empty(), "no browser means no gratuitous Leave");

        app.handle_input(&page_key(KeyCode::PageUp));
        assert!(present(&mut app, &mut terminal, FrameStamp::default()).unwrap());
        assert_eq!(terminal.viewport_area(), Rect::new(0, 0, WIDTH, 24));
        assert_eq!(
            minimal_api::minimal_printed_owner(&app).cloned(),
            printed_owner
        );
        app.handle_input(&page_key(KeyCode::Esc));
        assert!(!present(&mut app, &mut terminal, FrameStamp::default()).unwrap());
        assert_eq!(terminal.viewport_area(), saved);
        leave_history_alt_screen_for_teardown(&mut wire).unwrap();
        assert!(
            wire.is_empty(),
            "normal Leave releases teardown responsibility"
        );

        app.handle_input(&page_key(KeyCode::PageUp));
        assert!(present(&mut app, &mut terminal, FrameStamp::default()).unwrap());
        let committed_before = committed_ids(&app, AgentId(0));
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session_binding_epoch += 1;
        reconcile_owner(&mut app);
        assert!(matches!(
            app.minimal_state
                .history_browser
                .as_ref()
                .map(|browser| &browser.phase),
            Some(HistoryPhase::Leaving { .. })
        ));
        assert!(!present(&mut app, &mut terminal, FrameStamp::default()).unwrap());
        assert_eq!(terminal.viewport_area(), saved);
        assert_eq!(committed_ids(&app, AgentId(0)), committed_before);
        assert_eq!(
            minimal_api::minimal_printed_owner(&app).cloned(),
            printed_owner
        );

        let binding_epochs = binding_epochs(&app, &owner).unwrap();
        app.handle_input(&page_key(KeyCode::PageUp));
        assert!(present(&mut app, &mut terminal, FrameStamp::default()).unwrap());
        assert_eq!(
            app.minimal_state.history_browser,
            Some(HistoryBrowser {
                owner,
                binding_epochs,
                phase: HistoryPhase::Active { saved },
            }),
        );

        let first = app.handle_input(&Event::Key(KeyEvent::new(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
        )));
        assert!(
            matches!(first, InputOutcome::Changed | InputOutcome::Unchanged),
            "the first Quit only arms confirmation, got {first:?}"
        );
        assert!(
            matches!(
                app.minimal_state
                    .history_browser
                    .as_ref()
                    .map(|browser| &browser.phase),
                Some(HistoryPhase::Leaving { .. })
            ),
            "confirmation must arm a leave without writing it"
        );

        let second = app.handle_input(&Event::Key(KeyEvent::new(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
        )));
        assert!(
            matches!(second, InputOutcome::Action(Action::Quit)),
            "the second Quit in the same batch exits without a draw, got {second:?}"
        );
        // The terminal resource outlives the view; no final present is needed.
        drop(app);
        drop(terminal);
        wire.clear();
        leave_history_alt_screen_for_teardown(&mut wire).unwrap();
        assert_eq!(wire, b"\x1b[?1049l");
        wire.clear();
        leave_history_alt_screen_for_teardown(&mut wire).unwrap();
        assert!(
            wire.is_empty(),
            "successful cleanup consumes responsibility"
        );

        struct FailWrite;
        impl io::Write for FailWrite {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("write failed"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert!(write_alternate(&mut FailWrite, true).is_err());
        leave_history_alt_screen_for_teardown(&mut wire).unwrap();
        assert_eq!(
            wire, b"\x1b[?1049l",
            "Enter owns cleanup before the first write"
        );
        wire.clear();

        struct FailFlush;
        impl io::Write for FailFlush {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("flush failed"))
            }
        }
        assert!(write_alternate(&mut FailFlush, true).is_err());
        assert!(leave_history_alt_screen_for_teardown(&mut FailFlush).is_err());
        leave_history_alt_screen_for_teardown(&mut wire).unwrap();
        assert_eq!(
            wire, b"\x1b[?1049l",
            "failed switches keep cleanup responsibility"
        );
        wire.clear();
        write_alternate(&mut wire, true).unwrap();
        write_alternate(&mut wire, false).unwrap();
        wire.clear();
        leave_history_alt_screen_for_teardown(&mut wire).unwrap();
        assert!(wire.is_empty(), "normal Leave also releases the resource");
    }
}

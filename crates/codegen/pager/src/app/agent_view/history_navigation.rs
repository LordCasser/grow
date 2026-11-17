//! Unified history-boundary paging for the main transcript.
//!
//! Every scroll/turn navigation over the main view funnels into the same
//! check: when the visible window approaches an unloaded end of the active
//! history interval, request the continuation page. The window owner
//! ([`super::history_window`]) merges the response and keeps the viewport
//! anchor; this module only decides *when* to ask.

use super::AgentView;
use crate::app::actions::Effect;

/// Rows of look-ahead before an unloaded interval end counts as reached.
/// One viewport: the page request is armed while the boundary is still
/// approaching, and any position that can see the boundary row qualifies.
fn near_threshold(viewport_height: usize) -> usize {
    viewport_height.max(1)
}

impl AgentView {
    /// Request the next history page when the visible window has reached the
    /// unloaded end of the active history interval in the direction the user
    /// navigated.
    ///
    /// `forward` is the navigation direction: `true` toward the newer end
    /// (scroll-down / next-turn), `false` toward the older end (scroll-up /
    /// prev-turn). Only that end is examined, so a jump cannot page the wrong
    /// side of a small window.
    ///
    /// Called after every user navigation that mutates this view's scrollback
    /// (keyboard actions through the dispatch router, wheel through
    /// [`crate::app::root::AppView::dispatch_scroll`], scrollbar). The window
    /// protocol keeps at most one page request in flight per view, so repeated
    /// navigation at the same boundary is idempotent; a failed request keeps
    /// the window and cursor and is retried by the next navigation.
    /// Non-visible agents and surfaces owned by a modal/viewer never page in
    /// the background.
    pub(crate) fn request_history_at_viewport_boundary(
        &mut self,
        forward: bool,
        effects: &mut Vec<Effect>,
    ) {
        if self.history_cut().is_none()
            || self.history_latest_pending
            || self.history_page_request.is_some()
            || !self.history_browsing_surface()
        {
            return;
        }
        let Some(bounds) = self.history_viewport_bounds() else {
            return;
        };
        if forward {
            if bounds.near_newer
                && self.history_newer_cursor().is_some()
                && self.request_history_after(effects)
            {
                self.show_toast("Loading newer history…");
            }
        } else if bounds.near_older
            && self.history_older_cursor().is_some()
            && self.request_history_before(effects)
        {
            self.show_toast("Loading older history…");
        }
    }

    /// The main transcript is the browsing surface: overlays, viewers, and
    /// modal pickers that own the screen must not trigger background paging.
    /// Needs-input overlays (permission/question/plan) keep scrollback-pane
    /// navigation available, so they do not block paging by themselves.
    fn history_browsing_surface(&self) -> bool {
        self.block_viewer.is_none()
            && self.line_viewer.is_none()
            && self.active_modal.is_none()
            && self.image_viewer.is_none()
            && self.extensions_modal.is_none()
            && self.agents_modal.is_none()
            && self.scrollback_search.is_none()
            && self.btw_state.is_none()
            && !self.show_goal_detail
            && !self.show_workflows
            && self.rewind_state.is_none()
            && self.inline_edit.is_none()
            && self.jump_state.is_none()
            && self.cancel_turn_view.is_none()
            && self.goal_interrupt_view.is_none()
    }

    /// Distance test between the visible window and the two ends of the
    /// installed history interval. The live tail below the interval is never
    /// an interval end: the newer end is the interval's last row, and the
    /// fullscreen bottom after a cut does not qualify as continuation.
    fn history_viewport_bounds(&self) -> Option<HistoryViewportBounds> {
        let (first_id, last_id) = self.history_interval_entry_ids()?;
        let first_index = self.scrollback.index_of_id(first_id)?;
        let last_index = self.scrollback.index_of_id(last_id)?;
        let range = self.scrollback.visible_entry_range();
        if last_index < first_index || !range.contains(&first_index) || !range.contains(&last_index)
        {
            return None;
        }
        let virtual_y = self.scrollback.get_cached_virtual_y()?;
        let base = *virtual_y.get(range.start)?;
        let (offset, viewport_height, total_height) = self.scrollback.scroll_info();
        if viewport_height == 0 {
            return None;
        }
        let viewport_top = base + offset;
        let viewport_bottom = viewport_top + viewport_height as usize;
        let threshold = near_threshold(viewport_height as usize);
        let interval_start = virtual_y.get(first_index).copied()?;
        // The row one past the interval's last entry is the interval end;
        // when that entry is the final content row, the end is the content
        // bottom.
        let interval_end = match virtual_y.get(last_index + 1) {
            Some(&y) => y,
            None => base + total_height,
        };
        Some(HistoryViewportBounds {
            near_older: viewport_top.saturating_sub(interval_start) < threshold
                && interval_start.saturating_sub(viewport_bottom) < threshold,
            near_newer: interval_end.saturating_sub(viewport_bottom) < threshold
                && viewport_top.saturating_sub(interval_end) < threshold,
        })
    }
}

struct HistoryViewportBounds {
    near_older: bool,
    near_newer: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions::{Action, TaskResult};
    use crate::app::root::AppView;
    use crate::app::root::dispatch;
    use crate::app::session::AgentId;
    use acp_transport::protocol as acp;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
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
            descendant_ownership: None,
        }
    }

    /// An agent whose latest history page is installed: each row's rendered
    /// height exceeds the viewport, so any scroll reaches an interval end.
    /// The live tail below the interval stays distinct from it.
    fn agent_with_interval(session_text: &str) -> (AppView, AgentId, acp::SessionId) {
        let mut app = AppView::test_app_with_agent();
        let id = AgentId(0);
        let session_id = acp::SessionId::new(session_text);
        {
            let agent = app.agents.get_mut(&id).unwrap();
            agent.session.session_id = Some(session_id.clone());
            agent.history_cut = Some("cut".into());
            agent
                .scrollback
                .push_block(crate::scrollback::block::RenderBlock::notice("live tail"));
            agent.scrollback.prepare_layout(WIDTH, VIEWPORT);
        }
        adopt_latest(
            &mut app,
            id,
            &session_id,
            vec![
                text_row(&session_id, 32, "reply three"),
                text_row(&session_id, 31, "reply two"),
                text_row(&session_id, 30, "reply one"),
            ],
            Some("older-a"),
            None,
        );
        (app, id, session_id)
    }

    /// Drive the real adoption path (TaskComplete dispatch) for a Latest page.
    fn adopt_latest(
        app: &mut AppView,
        id: AgentId,
        session_id: &acp::SessionId,
        rows: Vec<HistoryRow>,
        older_cursor: Option<&str>,
        newer_cursor: Option<&str>,
    ) {
        let (request_id, generation, binding_epoch) = {
            let agent = app.agents.get_mut(&id).unwrap();
            agent.arm_history_load(Some("cut".into()));
            assert!(agent.request_latest_history_page(&mut Vec::new()));
            assert!(agent.history_latest_pending);
            (
                agent.history_page_request.unwrap(),
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
                query: HistoryPageQuery::Latest,
                result: Ok(page_response(session_id, rows, older_cursor, newer_cursor)),
            }),
            app,
        );
        assert!(
            effects.is_empty(),
            "page adoption must not spawn side effects"
        );
        assert!(!app.agents[&id].history_latest_pending);
        app.agents
            .get_mut(&id)
            .unwrap()
            .scrollback
            .prepare_layout(WIDTH, VIEWPORT);
    }

    fn history_effects(effects: &[Effect]) -> Vec<&crate::app::actions::Effect> {
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::ReadHistoryPage { .. }))
            .collect()
    }

    fn arm_and_adopt_before(
        app: &mut AppView,
        id: AgentId,
        session_id: &acp::SessionId,
        request_id: u64,
        armed_cursor: &str,
        rows: Vec<HistoryRow>,
        older_cursor: Option<&str>,
    ) {
        let (generation, binding_epoch) = {
            let agent = app.agents.get(&id).unwrap();
            assert_eq!(agent.history_page_request, Some(request_id));
            (agent.history_generation, agent.session_binding_epoch)
        };
        let effects = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Before {
                    cursor: armed_cursor.into(),
                },
                result: Ok(page_response(session_id, rows, older_cursor, None)),
            }),
            app,
        );
        match older_cursor {
            Some(cursor) => assert!(
                matches!(
                    effects.as_slice(),
                    [Effect::ReadHistoryPage {
                        query: HistoryPageQuery::Before { cursor: next_cursor },
                        request_id: next_id,
                        ..
                    }] if next_cursor == cursor
                        && Some(*next_id) == app.agents[&id].history_page_request
                ),
                "a still-visible boundary prefetches exactly the next page: {effects:?}"
            ),
            None => assert!(effects.is_empty(), "exhausted boundary: {effects:?}"),
        }
        app.agents
            .get_mut(&id)
            .unwrap()
            .scrollback
            .prepare_layout(WIDTH, VIEWPORT);
    }

    #[test]
    fn page_up_at_the_older_boundary_dispatches_one_before_request() {
        let (mut app, id, _session_id) = agent_with_interval("boundary-1");
        let effects = dispatch::dispatch(Action::PageUp, &mut app);
        assert!(
            matches!(
                history_effects(&effects).as_slice(),
                [Effect::ReadHistoryPage { query: HistoryPageQuery::Before { cursor }, .. }]
                    if cursor == "older-a"
            ),
            "PageUp at the interval top must arm a Before request via the real dispatch path"
        );
        assert_eq!(app.agents[&id].history_page_request, Some(2));

        // Same boundary, navigation repeated: still one in-flight request.
        let repeat = dispatch::dispatch(Action::HalfPageUp, &mut app);
        assert!(
            history_effects(&repeat).is_empty(),
            "an in-flight boundary request must suppress further arming"
        );
    }

    #[test]
    fn three_consecutive_older_pages_read_through_the_dispatch_path() {
        let (mut app, id, session_id) = agent_with_interval("boundary-3");
        assert!(
            dispatch::dispatch(Action::GotoTop, &mut app)
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryPage { .. }))
        );
        arm_and_adopt_before(
            &mut app,
            id,
            &session_id,
            2,
            "older-a",
            vec![
                text_row(&session_id, 21, "reply zero"),
                text_row(&session_id, 20, "reply minus one"),
            ],
            Some("older-b"),
        );
        assert_eq!(app.agents[&id].history_older_cursor(), Some("older-b"));
        let text = app.agents[&id]
            .scrollback
            .iter_entries()
            .filter_map(|(_, entry)| entry.block.searchable_text())
            .collect::<Vec<_>>()
            .join("\n");
        for chunk in [
            "reply minus one",
            "reply zero",
            "reply one",
            "reply two",
            "reply three",
        ] {
            assert_eq!(
                text.matches(chunk).count(),
                1,
                "one copy of {chunk}: {text}"
            );
        }
        assert!(text.find("reply minus one") < text.find("reply zero"));
        assert!(text.find("reply zero") < text.find("reply one"));

        // Adoption already prefetched the next page at the visible boundary.
        // A second user action must not duplicate the in-flight request.
        let second = dispatch::dispatch(Action::PageUp, &mut app);
        assert!(history_effects(&second).is_empty());
        assert_eq!(app.agents[&id].history_page_request, Some(3));
        arm_and_adopt_before(
            &mut app,
            id,
            &session_id,
            3,
            "older-b",
            vec![text_row(&session_id, 10, "deep history")],
            None,
        );
        // Third navigation at an exhausted older end arms nothing.
        let third = dispatch::dispatch(Action::PageUp, &mut app);
        assert!(
            history_effects(&third).is_empty(),
            "an exhausted older cursor must stop paging"
        );
    }

    #[test]
    fn return_trip_does_not_treat_the_live_tail_as_a_newer_interval_end() {
        let (mut app, id, session_id) = agent_with_interval("boundary-return");
        dispatch::dispatch(Action::GotoTop, &mut app);
        arm_and_adopt_before(
            &mut app,
            id,
            &session_id,
            2,
            "older-a",
            vec![text_row(&session_id, 20, "reply zero")],
            Some("older-b"),
        );
        // Walk back down over loaded content to the fullscreen bottom.
        let effects = dispatch::dispatch(Action::GotoBottom, &mut app);
        assert!(
            history_effects(&effects).is_empty(),
            "the live-tail bottom is not an interval end when no continuation exists"
        );
        let down = dispatch::dispatch(Action::PageDown, &mut app);
        assert!(history_effects(&down).is_empty());
    }

    #[test]
    fn newer_continuation_pages_when_the_interval_end_is_reached() {
        let (mut app, id, session_id) = agent_with_interval("boundary-newer");
        // Simulate a jumped interval whose newer end is unloaded: adopt an
        // Around window with a continuation cursor, through the real
        // adoption path.
        let (request_id, generation, binding_epoch) = {
            let agent = app.agents.get_mut(&id).unwrap();
            let request_id = agent.history_page_request_seq + 1;
            agent.history_page_request = Some(request_id);
            agent.history_page_request_seq = request_id;
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
                    vec![
                        text_row(&session_id, 41, "mid two"),
                        text_row(&session_id, 40, "mid one"),
                    ],
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
        assert_eq!(app.agents[&id].history_newer_cursor(), Some("newer-j"));

        // Scrolling down to the interval end (not the content bottom) arms
        // the After request.
        let down = dispatch::dispatch(Action::GotoBottom, &mut app);
        assert!(
            matches!(
                history_effects(&down).as_slice(),
                [Effect::ReadHistoryPage { query: HistoryPageQuery::After { cursor }, .. }]
                    if cursor == "newer-j"
            ),
            "reaching the interval end with a continuation must arm an After request"
        );
    }

    #[test]
    fn wrong_binding_or_generation_results_do_not_disturb_browsing() {
        let (mut app, id, session_id) = agent_with_interval("boundary-stale");
        dispatch::dispatch(Action::GotoTop, &mut app);
        let (generation, before_len) = {
            let agent = app.agents.get_mut(&id).unwrap();
            agent.history_page_request = Some(9);
            agent.history_page_request_seq = 9;
            (agent.history_generation, agent.scrollback.len())
        };
        let effects = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: 99,
                request_id: 9,
                generation,
                query: HistoryPageQuery::Before {
                    cursor: "gone".into(),
                },
                result: Ok(page_response(
                    &session_id,
                    vec![text_row(&session_id, 77, "stale")],
                    None,
                    None,
                )),
            }),
            &mut app,
        );
        assert!(effects.is_empty());
        let agent = &app.agents[&id];
        assert_eq!(
            agent.scrollback.len(),
            before_len,
            "a wrong-epoch response must not merge"
        );
        // The in-flight slot stays armed for the real response.
        assert_eq!(agent.history_page_request, Some(9));
    }

    #[test]
    fn failed_page_keeps_window_and_is_retryable_by_navigation() {
        let (mut app, id, session_id) = agent_with_interval("boundary-fail");
        dispatch::dispatch(Action::GotoTop, &mut app);
        assert_eq!(app.agents[&id].history_page_request, Some(2));
        let generation = app.agents[&id].history_generation;
        let binding_epoch = app.agents[&id].session_binding_epoch;
        let effects = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id: 2,
                generation,
                query: HistoryPageQuery::Before {
                    cursor: "older-a".into(),
                },
                result: Err("network down".into()),
            }),
            &mut app,
        );
        assert!(effects.is_empty());
        assert_eq!(app.agents[&id].history_page_request, None);
        assert_eq!(app.agents[&id].history_older_cursor(), Some("older-a"));

        let retry = dispatch::dispatch(Action::PageUp, &mut app);
        assert!(
            matches!(
                history_effects(&retry).as_slice(),
                [Effect::ReadHistoryPage { .. }]
            ),
            "the next user navigation retries the failed boundary"
        );
    }

    #[test]
    fn modals_do_not_page_in_the_background() {
        let (mut app, id, _session_id) = agent_with_interval("boundary-modal");
        app.agents.get_mut(&id).unwrap().active_modal =
            Some(crate::views::modal::ActiveModal::CommandPalette {
                entries: Vec::new(),
                state: crate::views::picker::PickerState::input_active(),
                window: crate::views::modal_window::ModalWindowState::new(),
            });
        let effects = dispatch::dispatch(Action::GotoTop, &mut app);
        assert!(
            history_effects(&effects).is_empty(),
            "a modal-owned surface must not arm history requests"
        );
    }

    #[test]
    fn fullscreen_child_pages_from_the_child_owner() {
        let (mut app, id, _session_id) = agent_with_interval("boundary-parent");
        let child_session = acp::SessionId::new("boundary-child");
        let mut child = crate::app::agent_view::test_fixtures::make_agent();
        child.session.session_id = Some(child_session.clone());
        child.history_cut = Some("cut".into());
        child
            .scrollback
            .push_block(crate::scrollback::block::RenderBlock::notice("child tail"));
        child.scrollback.prepare_layout(WIDTH, VIEWPORT);
        let agent = app.agents.get_mut(&id).unwrap();
        agent
            .subagent_views
            .insert("child-sid".into(), Box::new(child));
        agent.open_subagent_fullscreen("child-sid".into());
        // Install the child's interval through the window owner API; the
        // TaskComplete routing for child sessions is tracked separately.
        {
            let agent = app.agents.get_mut(&id).unwrap();
            let child = agent.subagent_views.get_mut("child-sid").unwrap();
            child.arm_history_load(Some("cut".into()));
            let generation = child.history_generation;
            let outcome = child.apply_history_page(
                generation,
                &HistoryPageQuery::Latest,
                page_response(
                    &child_session,
                    vec![
                        text_row(&child_session, 34, "child four"),
                        text_row(&child_session, 33, "child three"),
                    ],
                    Some("child-older"),
                    None,
                ),
            );
            assert!(matches!(
                outcome,
                crate::app::agent_view::HistoryPageOutcome::Adopted
            ));
            child.scrollback.prepare_layout(WIDTH, VIEWPORT);
        }

        let effects = dispatch::dispatch(Action::GotoTop, &mut app);
        let child = &app.agents[&id].subagent_views["child-sid"];
        assert!(
            matches!(
                history_effects(&effects).as_slice(),
                [Effect::ReadHistoryPage { session_id, query: HistoryPageQuery::Before { cursor }, .. }]
                    if session_id.0.as_ref() == "boundary-child" && cursor == "child-older"
            ),
            "fullscreen child navigation must page from the child owner"
        );
        assert_eq!(child.history_page_request, Some(1));
        assert!(
            app.agents[&id].history_page_request.is_none(),
            "the parent window must stay untouched while the child browses"
        );
    }

    #[test]
    fn wheel_input_pages_through_the_dispatch_scroll_funnel() {
        let (mut app, id, _session_id) = agent_with_interval("boundary-wheel");
        // Park the viewport one scroll from the interval top.
        app.agents
            .get_mut(&id)
            .unwrap()
            .scrollback
            .set_scroll_offset(1);
        let wheel = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        // The event-loop input path: handle_input flushes wheel lines through
        // dispatch_scroll, which runs the boundary check in the wheel's
        // direction and publishes the request into pending_effects.
        // The wheel normalizer flushes on a 16ms cadence. Give this event an
        // explicit arrival beyond that deadline so suite speed cannot suppress it.
        let outcome = app.handle_input_at_with_paste_provenance(
            &wheel,
            std::time::Instant::now() + std::time::Duration::from_millis(20),
            crate::app::root::PasteProvenance::Terminal,
        );
        assert!(matches!(outcome, crate::app::root::InputOutcome::Changed));
        assert_eq!(
            app.agents[&id].history_page_request,
            Some(2),
            "a wheel that reaches the interval top must arm the next page"
        );
        assert!(
            matches!(
                app.pending_effects.as_slice(),
                [Effect::ReadHistoryPage { query: HistoryPageQuery::Before { cursor }, .. }]
                    if cursor == "older-a"
            ),
            "the wheel seam must publish the request for the event loop to execute"
        );

        // Wheel under a modal: the surface guard blocks paging.
        app.pending_effects.clear();
        app.agents.get_mut(&id).unwrap().history_page_request = None;
        app.agents.get_mut(&id).unwrap().active_modal =
            Some(crate::views::modal::ActiveModal::CommandPalette {
                entries: Vec::new(),
                state: crate::views::picker::PickerState::input_active(),
                window: crate::views::modal_window::ModalWindowState::new(),
            });
        let _ = app.handle_input(&wheel);
        assert_eq!(
            app.agents[&id].history_page_request, None,
            "wheel under a modal must not page"
        );
        assert!(app.pending_effects.is_empty());
    }

    #[test]
    fn wheel_at_the_live_tail_bottom_never_arms_an_after_request() {
        let (mut app, id, _session_id) = agent_with_interval("boundary-wheel-tail");
        dispatch::dispatch(Action::GotoBottom, &mut app);
        app.pending_effects.clear();
        let wheel = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        let _ = app.handle_input(&wheel);
        assert!(
            app.agents[&id].history_page_request.is_none(),
            "the live tail has no newer continuation after a Latest page"
        );
    }

    #[test]
    fn scrollbar_directions_use_the_same_helper_contract() {
        // mouse.rs (scrollbar owner) settles the offset, then calls the same
        // directed helper; this pins the direction contract it relies on.
        let (mut app, id, _session_id) = agent_with_interval("boundary-scrollbar");
        let agent = app.agents.get_mut(&id).unwrap();
        agent.scrollback.set_scroll_offset(0);
        let mut effects = Vec::new();
        agent.request_history_at_viewport_boundary(false, &mut effects);
        assert!(
            matches!(
                effects.as_slice(),
                [Effect::ReadHistoryPage { query: HistoryPageQuery::Before { cursor }, .. }]
                    if cursor == "older-a"
            ),
            "a scrollbar jump to the content top must arm the older page"
        );
    }

    #[test]
    fn key_navigation_uses_the_same_boundary_as_page_actions() {
        let (mut app, id, _session_id) = agent_with_interval("boundary-keys");
        // Turn navigation at the top boundary.
        let effects = dispatch::dispatch(Action::PrevTurn, &mut app);
        assert!(
            matches!(
                history_effects(&effects).as_slice(),
                [Effect::ReadHistoryPage {
                    query: HistoryPageQuery::Before { .. },
                    ..
                }]
            ),
            "PrevTurn at the interval top must page"
        );
        app.agents.get_mut(&id).unwrap().history_page_request = None;
        let scroll = dispatch::dispatch(Action::ScrollUp(3), &mut app);
        assert!(
            matches!(
                history_effects(&scroll).as_slice(),
                [Effect::ReadHistoryPage { .. }]
            ),
            "line scrolls share the boundary check"
        );
    }

    #[test]
    fn prompt_focused_page_up_pages_the_shared_scrollback() {
        let (mut app, id, _session_id) = agent_with_interval("boundary-prompt");
        let agent = app.agents.get_mut(&id).unwrap();
        agent.active_pane = crate::views::agent::ActivePane::Prompt;
        let outcome = agent.handle_input_with_prompt_paging(
            &page_key(KeyCode::PageUp),
            &crate::actions::ActionRegistry::defaults(),
            &mut Vec::new(),
        );
        assert!(
            matches!(
                outcome,
                crate::app::root::InputOutcome::Action(Action::PageUp)
            ),
            "prompt paging PageUp resolves to the shared scroll action"
        );
    }
}

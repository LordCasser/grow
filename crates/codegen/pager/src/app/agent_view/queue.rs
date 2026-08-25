//! Prompt-queue pane: visibility toggles, key handling, row removal, and
//! server-order reconciliation.

#[cfg(test)]
use super::test_fixtures;
use super::{AgentPane, AgentView, PromptMode, overlay_action_to_outcome};
use crate::actions::ActionRegistry;
use crate::app::actions::Action;
use crate::app::root::InputOutcome;
use crossterm::event::KeyEvent;

impl AgentView {
    /// Remove a local queue row: fix selection, drop the entry, hide the
    /// pane if the merged view emptied. Returns the removed prompt, if any.
    pub(in crate::app) fn remove_local_queue_row(
        &mut self,
        id: u64,
        effects: &mut Vec<super::actions::Effect>,
    ) -> Option<crate::app::session::QueuedPrompt> {
        let pos = self
            .session
            .pending_prompts
            .iter()
            .position(|p| p.id == id)?;
        // Deleting the row being edited discards the edit — and must exit
        // BEFORE the removal so a potential auto-hide pane switch can't hit
        // the editing lock (see queue_edit.rs ordering invariant).
        if matches!(
            self.prompt_mode,
            PromptMode::EditingQueued { id: editing_id, server_id: None, .. } if editing_id == id
        ) {
            let release_effect = self.exit_editing_mode();
            debug_assert!(release_effect.is_none());
        }
        self.queue.select_after_delete(id);
        let prompt = self.session.pending_prompts.remove(pos);
        if self.visible_queue_is_empty() {
            self.hide_queue_pane(effects);
        }
        prompt
    }

    /// Force-send a queued follow-up mid-turn from the prompt (empty composer).
    ///
    /// Always the **top** visible row (first under the server-then-local merge
    /// order — the next item that would drain). Bare Enter and the send-now
    /// chord share this path; queue-pane selection / mouse "Send now" keep
    /// intentional selection. Returns `None` when there is nothing to send.
    pub(super) fn try_send_now_queued_from_prompt(
        &mut self,
        effects: &mut Vec<super::actions::Effect>,
    ) -> Option<InputOutcome> {
        if !self.session.state.is_turn_running() {
            return None;
        }
        self.sync_queue_pane();
        if !self.held_queue_top_sendable() {
            return None;
        }
        let ids = self.queue.entry_ids();
        let id = *ids.first()?;
        let outcome = self.force_interject_queue_row(id, effects);
        // Acting on the prompt-path send-now while its tip is up is the user
        // accepting the hint — mirrors the undo / image-input funnels so the
        // send_now `shown → accepted` conversion is measurable.
        if matches!(outcome, InputOutcome::Action(_))
            && self.ephemeral_tip.current_key() == Some(crate::tips::send_now::SEND_NOW_TIP_KEY)
        {
            diagnostics::session_ctx::log_event(diagnostics::events::ContextualTip {
                tip: diagnostics::events::ContextualTipKind::SendNow,
                action: diagnostics::events::ContextualTipAction::Accepted,
            });
            self.ephemeral_tip
                .clear(crate::tips::send_now::SEND_NOW_TIP_KEY);
        }
        Some(outcome)
    }

    /// Whether the foreground is in a wait that can use the compact parked
    /// presentation. This is display-only: Enter still queues, steering still
    /// targets the same foreground turn, and Goal lifecycle is unaffected.
    pub(crate) fn is_parked_wait(&self) -> bool {
        crate::views::turn_status::is_parkable_wait(&self.resolve_turn_activity())
    }

    /// The current wait is a foreground subagent await — steerable, but excluded
    /// from the parked look (the parent is blocked, not completed; the
    /// subagent reports its own progress).
    pub(crate) fn is_waiting_on_subagent(&self) -> bool {
        use crate::acp::tracker::{TurnActivity, WaitingReason};
        matches!(
            self.resolve_turn_activity(),
            Some(TurnActivity::Waiting(WaitingReason::Subagent))
        )
    }

    /// Visible held rows for the "N queued" hint. 0 outside parked waits.
    pub(crate) fn held_queue_count(&self) -> usize {
        if !self.is_parked_wait() {
            return 0;
        }
        self.visible_held_queue_len()
    }

    /// Pane-visible held rows (excludes running + send-now echo).
    pub(crate) fn visible_held_queue_len(&self) -> usize {
        let running = self.session.current_prompt_id.as_deref();
        let server = self
            .session
            .shared_queue
            .iter()
            .filter(|e| crate::views::queue_pane::visible_held_server_row(&e.id, running))
            .count();
        server + self.session.pending_prompts.len()
    }

    /// Whether bare Enter on the empty composer would actually send the TOP
    /// visible held row — the "Enter to send now" half of the inline hint.
    /// Only prompt-like rows can steer, regardless of whether they are already
    /// server-authoritative or still local. Bash/client-expanded rows remain
    /// FIFO work, so advertising Enter for them would over-promise.
    pub(crate) fn held_queue_top_sendable(&self) -> bool {
        let running = self.session.current_prompt_id.as_deref();
        // Merge order: server rows render (and send) first.
        if let Some(entry) = self
            .session
            .shared_queue
            .iter()
            .find(|e| crate::views::queue_pane::visible_held_server_row(&e.id, running))
        {
            return crate::views::queue_pane::kind_from_wire(&entry.kind)
                == crate::app::session::QueueEntryKind::Prompt;
        }
        self.session.pending_prompts.front().is_some_and(|p| {
            p.kind == crate::app::session::QueueEntryKind::Prompt && p.wire_matches_display()
        })
    }

    /// Rebuild the queue pane via [`visible_held_server_row`] excludes.
    pub(crate) fn sync_queue_pane(&mut self) {
        self.queue.sync_from_merged(
            &self.session.pending_prompts,
            &self.session.shared_queue,
            self.session.current_prompt_id.as_deref(),
        );
    }

    /// Whether the stopped-session look is active: the turn is parked in a
    /// parkable wait that is not a foreground subagent await. Purely
    /// view-derived — no transcript row is written for a park. Drives the
    /// idle keybar and the parked turn-status cue; flips back off (the
    /// running chrome returns) the moment the wait ends and the turn resumes.
    pub(crate) fn renders_parked(&self) -> bool {
        self.is_parked_wait() && !self.is_waiting_on_subagent()
    }

    /// Live counts for the turn-status watching cue; see
    /// [`crate::views::turn_status::Watchers`].
    pub(crate) fn watchers(&self) -> crate::views::turn_status::Watchers {
        let mut watchers = crate::views::turn_status::Watchers::default();
        for task in self
            .session
            .bg_tasks
            .values()
            .filter(|t| t.status == crate::app::session::BgTaskStatus::Running)
        {
            if task.is_monitor {
                watchers.monitors += 1;
            } else {
                watchers.commands += 1;
            }
        }
        watchers.loops = self.session.scheduled_tasks.len();
        watchers.subagents = self
            .session
            .subagent_sessions
            .values()
            .filter(|s| s.is_running() && s.workflow_run_id.is_none())
            .count();
        watchers.workflows = self
            .session
            .workflow_runs
            .iter()
            .filter(|run| run.is_active())
            .count();
        watchers
    }

    /// Shared tail of every turn-end marker push
    /// (`push_turn_terminal_marker`).
    pub(crate) fn push_end_marker_block(
        &mut self,
        event: crate::scrollback::blocks::SessionEvent,
        stop_hooks: Vec<(String, Vec<crate::scrollback::blocks::tool::HookRunEntry>)>,
        prompt_id: Option<String>,
    ) {
        // The marker keeps its turn's pid for the tail-merge attribution check.
        let block = crate::scrollback::blocks::SessionEventBlock::with_stop_hooks(
            event, stop_hooks, prompt_id,
        );
        self.scrollback
            .push_block(crate::scrollback::block::RenderBlock::SessionEvent(block));
    }

    /// `Some(is_prompt_like)` for a resolvable merged-queue row; `None` when it
    /// can't be resolved. Prompt-like rows may interject: plain prompts, plus
    /// raw skill slash rows (`/find-session args`) whose wire payload IS the
    /// display text — the shell expands those at the interjection drain. Rows
    /// with a client-expanded payload (for example `/loop`) and non-prompt
    /// kinds stay queued: interjecting them would send the display text, not
    /// the payload.
    pub(in crate::app) fn queue_row_prompt_like(&self, id: u64) -> Option<bool> {
        use crate::app::session::QueueEntryKind;
        use crate::views::queue_pane::{QueueRowOrigin, kind_from_wire};

        if let Some(local) = self.session.pending_prompts.iter().find(|p| p.id == id) {
            return Some(local.kind == QueueEntryKind::Prompt && local.wire_matches_display());
        }
        let row = self.queue.row_ref(id)?;
        if row.origin != QueueRowOrigin::Server {
            return None;
        }
        let server_id = row.server_id?;
        let wire = self
            .session
            .shared_queue
            .iter()
            .find(|e| e.id == server_id)?;
        Some(kind_from_wire(&wire.kind) == QueueEntryKind::Prompt)
    }

    /// Atomically steer one merged-queue row into the active turn.
    pub(in crate::app) fn force_interject_queue_row(
        &mut self,
        id: u64,
        effects: &mut Vec<super::actions::Effect>,
    ) -> InputOutcome {
        if !self.session.state.is_turn_running() {
            self.show_toast("No turn running — prompt will send when ready");
            return InputOutcome::Changed;
        }
        let row = self.queue.row_ref(id);
        let is_server = matches!(
            row.as_ref().map(|r| r.origin),
            Some(crate::views::queue_pane::QueueRowOrigin::Server)
        );
        // Steering is model input. Bash and client-expanded control rows keep
        // their FIFO identity and execute only after the foreground owner
        // completes; trying to "send now" must never degrade into cancellation.
        if self.queue_row_prompt_like(id) != Some(true) {
            self.show_toast("Can't send this now — it runs when the current turn ends");
            return InputOutcome::Changed;
        }
        if is_server {
            // Server row: ask the agent to atomically steer it via
            // `grow/queue/interject`. Only prompt-like rows are consumed;
            // non-prompt work is authoritatively left in FIFO.
            if let Some(row) = row.as_ref()
                && let Some(server_id) = row.server_id.clone()
            {
                // Still an optimistic echo: its `session/prompt` RPC is in
                // flight, so an interject fired now would overtake the row
                // shell-side and silently no-op (dropping the send-now and
                // hiding the row behind the armed cancel expectation). Park
                // the intent; the confirming `grow/queue/changed` broadcast
                // fires it with the row's authoritative version (see
                // `resolve_send_now_awaiting_confirm`).
                if self.session.has_optimistic_queue_echo(&server_id) {
                    self.session
                        .park_send_now_until_queue_confirmation(server_id);
                    return InputOutcome::Changed;
                }
                return InputOutcome::Action(Action::QueueInterjectShared {
                    id: server_id,
                    expected_version: row.version,
                    new_text: None,
                });
            }
            return InputOutcome::Changed;
        }
        if let Some(prompt) = self.remove_local_queue_row(id, effects) {
            return InputOutcome::Action(Action::SteerPrompt {
                text: prompt.text,
                images: prompt.images,
            });
        }
        InputOutcome::Changed
    }

    /// Toggle queue pane visibility (shared by Ctrl-; shortcut and badge click).
    pub(in crate::app) fn toggle_queue_pane(&mut self, effects: &mut Vec<super::actions::Effect>) {
        self.queue.overlay.toggle();
        self.queue.on_state_change();
        if self.queue.overlay.focused {
            self.set_active_pane(AgentPane::Queue, effects);
        } else if self.active_pane == AgentPane::Queue {
            self.set_active_pane(AgentPane::Scrollback, effects);
        }
    }

    /// Queue-pane-focused key handling.
    ///
    /// Routes through: overlay structural keys → queue actions → navigation.
    pub(in crate::app) fn handle_queue_key(
        &mut self,
        key: &KeyEvent,
        registry: &ActionRegistry,
        effects: &mut Vec<super::actions::Effect>,
    ) -> InputOutcome {
        use crate::views::overlay::{handle_overlay_key, handle_overlay_nav_key};
        use crate::views::queue_pane::{QueueEvent, QueueRowOrigin};

        // Structural keys through shared handler (Esc, Ctrl-F, etc.).
        let action = handle_overlay_key(&mut self.queue.overlay, key)
            .or_else(|| handle_overlay_nav_key(&mut self.queue.overlay, key));
        if let Some(action) = action {
            self.queue.on_state_change();
            // Overlay dismiss skips hide_queue_pane; reset edge when queue is empty.
            if !self.queue.overlay.visible && self.visible_queue_is_empty() {
                self.queue.reset_auto_show_edge();
            }
            if !self.queue.overlay.visible || !self.queue.overlay.focused {
                self.set_active_pane(AgentPane::Scrollback, effects);
            }
            return overlay_action_to_outcome(action);
        }

        // Queue-specific actions (delete, edit, reorder). `x`/Delete = row delete.
        if let Some(event) = self.queue.handle_key(key, registry) {
            // Resolve the selected row's origin so edits route correctly:
            // Server-origin rows go to the agent as `grow/queue/*`
            // commands (the rebroadcast is the source of truth); Local rows
            // keep today's in-place mutation.
            let row = self.queue.row_ref(Self::queue_event_id(&event));
            let is_server = matches!(row.as_ref().map(|r| r.origin), Some(QueueRowOrigin::Server));

            match event {
                QueueEvent::DeleteSelected { id } => {
                    if is_server {
                        // Optimistic remove; server rebroadcast is authoritative.
                        if let (Some(_sid), Some(row)) = (self.session.session_id.as_ref(), row)
                            && let Some(server_id) = row.server_id
                        {
                            self.session.shared_queue.retain(|e| e.id != server_id);
                            if self.visible_queue_is_empty() {
                                self.hide_queue_pane(effects);
                            }
                            return InputOutcome::Action(Action::QueueRemoveShared {
                                id: server_id,
                                expected_version: row.version,
                            });
                        }
                        return InputOutcome::Changed;
                    }
                    // No drain kick (cf. mouse [cancel]): queue focus is unreachable mid-edit.
                    self.remove_local_queue_row(id, effects);
                }
                QueueEvent::EditSelected { id } => {
                    // Entry into editing mode lives in `queue_edit.rs`.
                    self.enter_queue_edit(id, is_server, row, effects);
                }
                QueueEvent::SwapUp { id } => {
                    if is_server {
                        if let Some(ordered_ids) = self.server_queue_reordered(id, true) {
                            return InputOutcome::Action(Action::QueueReorderShared {
                                ordered_ids,
                            });
                        }
                        return InputOutcome::Changed;
                    }
                    self.session.swap_prompt_up(id);
                }
                QueueEvent::SwapDown { id } => {
                    if is_server {
                        if let Some(ordered_ids) = self.server_queue_reordered(id, false) {
                            return InputOutcome::Action(Action::QueueReorderShared {
                                ordered_ids,
                            });
                        }
                        return InputOutcome::Changed;
                    }
                    self.session.swap_prompt_down(id);
                }
                QueueEvent::ForceInterject { id } => {
                    return self.force_interject_queue_row(id, effects);
                }
            }
            return InputOutcome::Changed;
        }

        // Navigation keys (j/k, y to copy, etc.).
        if self.queue.handle_navigation_key(key) {
            InputOutcome::Changed
        } else {
            InputOutcome::Unchanged
        }
    }

    /// The selection id carried by a [`QueueEvent`].
    fn queue_event_id(event: &crate::views::queue_pane::QueueEvent) -> u64 {
        use crate::views::queue_pane::QueueEvent;
        match event {
            QueueEvent::DeleteSelected { id }
            | QueueEvent::EditSelected { id }
            | QueueEvent::SwapUp { id }
            | QueueEvent::SwapDown { id }
            | QueueEvent::ForceInterject { id } => *id,
        }
    }

    /// True when the pane would show zero rows.
    pub(in crate::app) fn visible_queue_is_empty(&self) -> bool {
        self.visible_held_queue_len() == 0
    }

    /// Hide the queue pane. Only steals focus when the queue pane was active —
    /// prompt-path send-now of the last local row must not yank the user out
    /// of the composer into scrollback.
    pub(in crate::app) fn hide_queue_pane(&mut self, effects: &mut Vec<super::actions::Effect>) {
        self.queue.overlay.visible = false;
        self.queue.overlay.focused = false;
        // External hide skips sync auto-hide; reset so next enqueue can auto-show.
        self.queue.reset_auto_show_edge();
        if self.active_pane == AgentPane::Queue {
            self.set_active_pane(AgentPane::Scrollback, effects);
        }
    }

    /// Reorder payload for `grow/queue/reorder`. Omit only running; include
    /// send-now in the list but do not swap past it (shell ranks missing ids last).
    fn server_queue_reordered(&self, selection_id: u64, up: bool) -> Option<Vec<String>> {
        let server_id = self.queue.row_ref(selection_id)?.server_id?;
        let running = self.session.current_prompt_id.as_deref();
        let all_ids: Vec<String> = self
            .session
            .shared_queue
            .iter()
            .filter(|e| Some(e.id.as_str()) != running)
            .map(|e| e.id.clone())
            .collect();
        let mut swappable: Vec<String> = all_ids
            .iter()
            .filter(|id| crate::views::queue_pane::visible_held_server_row(id, running))
            .cloned()
            .collect();
        let pos = swappable.iter().position(|x| x == &server_id)?;
        let swap_with = if up {
            pos.checked_sub(1)?
        } else {
            let next = pos + 1;
            if next >= swappable.len() {
                return None;
            }
            next
        };
        swappable.swap(pos, swap_with);
        let mut swap_iter = swappable.into_iter();
        let ordered: Vec<String> = all_ids
            .into_iter()
            .map(|id| {
                if crate::views::queue_pane::visible_held_server_row(&id, running) {
                    swap_iter
                        .next()
                        .expect("swappable count matches visible slots")
                } else {
                    id
                }
            })
            .collect();
        Some(ordered)
    }
}

#[cfg(test)]
mod queue_steering_tests {
    use super::test_fixtures::{make_agent, make_running_agent, running_agent_local_only};
    use super::*;

    #[test]
    fn local_queue_row_becomes_same_turn_steering() {
        let mut agent = running_agent_local_only();
        let running_id = agent.session.current_prompt_id.clone();
        agent.sync_queue_pane();
        let row = agent.queue.entry_ids()[0];
        let mut effects = Vec::new();
        let outcome = agent.force_interject_queue_row(row, &mut effects);
        assert!(matches!(
            outcome,
            InputOutcome::Action(Action::SteerPrompt { .. })
        ));
        assert_eq!(agent.session.current_prompt_id, running_id);
    }

    #[test]
    fn optimistic_row_converts_to_steer_only_after_confirmation() {
        let mut agent = running_agent_local_only();
        agent.session.mark_optimistic_queue_echo("queued-1");
        agent
            .session
            .park_send_now_until_queue_confirmation("queued-1".into());
        assert_eq!(
            agent
                .session
                .resolve_send_now_awaiting_confirm(&[("queued-1".into(), 3)], Some("running")),
            Some(("queued-1".into(), 3))
        );
        assert!(!agent.session.has_optimistic_queue_echo("queued-1"));
    }

    #[test]
    fn server_bash_row_cannot_be_steered_or_advertised_as_sendable() {
        let mut agent = make_running_agent();
        agent.session.shared_queue[0].kind = "bash".into();
        agent.sync_queue_pane();
        let row = agent.queue.entry_ids()[0];

        assert!(!agent.held_queue_top_sendable());
        let mut effects = Vec::new();
        assert!(matches!(
            agent.force_interject_queue_row(row, &mut effects),
            InputOutcome::Changed
        ));
        assert_eq!(agent.session.shared_queue.len(), 1);
        assert!(
            agent
                .toast
                .as_ref()
                .is_some_and(|(message, _)| message.contains("Can't send this now"))
        );
    }

    #[test]
    fn clean_server_edit_pane_switch_releases_hold_through_caller_effects() {
        let mut agent = make_agent();
        agent.session.session_id = Some(agent_client_protocol::SessionId::new("s1"));
        agent.prompt_mode = PromptMode::EditingQueued {
            id: 7,
            original: "queued".into(),
            server_id: Some("q1".into()),
            kind: crate::app::session::QueueEntryKind::Prompt,
        };
        agent.prompt.set_text("queued");
        agent.force_active_pane(AgentPane::Prompt);

        let mut effects = Vec::new();
        assert!(agent.set_active_pane(AgentPane::Scrollback, &mut effects));

        assert!(matches!(agent.prompt_mode, PromptMode::Normal));
        assert!(matches!(
            effects.as_slice(),
            [crate::app::actions::Effect::QueueReleaseEdit { session_id, id }]
                if session_id == &agent_client_protocol::SessionId::new("s1") && id == "q1"
        ));
    }
}

#[cfg(test)]
mod watcher_tests {
    use super::super::{test_agent_view, test_fixtures};
    use crate::app::session::WorkflowRunSnapshot;
    use crate::views::turn_status::Watchers;

    fn active_workflow(run_id: &str) -> WorkflowRunSnapshot {
        WorkflowRunSnapshot {
            run_id: run_id.to_owned(),
            definition_id: None,
            definition_scope: None,
            definition_hash: None,
            name: "workflow".to_owned(),
            objective: "objective".to_owned(),
            status: "active".to_owned(),
            management_available: true,
            builtin: false,
            phases: Vec::new(),
            current_phase: None,
            agents: Vec::new(),
            agent_budget: None,
            agents_used: 0,
            agents_remaining: None,
            agent_usage_incomplete: false,
            active_agents: 0,
            elapsed_ms: 0,
            received_at: std::time::Instant::now(),
            pause_message: None,
            result_summary: None,
        }
    }

    fn insert_bg_task(
        agent: &mut crate::app::agent_view::AgentView,
        task_id: &str,
        is_monitor: bool,
    ) {
        agent.session.bg_tasks.insert(
            task_id.into(),
            crate::app::session::BgTaskState {
                task_id: task_id.into(),
                tool_call_id: format!("call-{task_id}"),
                command: "sleep 5".into(),
                description: None,
                cwd: "/tmp".into(),
                output_file: "/tmp/out".into(),
                status: crate::app::session::BgTaskStatus::Running,
                start_time: std::time::SystemTime::now(),
                end_time: None,
                exit_code: None,
                signal: None,
                stdout: String::new(),
                stdout_line_count: 0,
                truncated: false,
                pending_kill: false,
                kill_requested_at: None,
                scrollback_entry_id: None,
                is_monitor,
                restored_from_replay: false,
            },
        );
    }

    #[test]
    fn watchers_counts_monitors_apart_from_commands() {
        let mut agent = test_agent_view(Some("s1"), std::path::PathBuf::from("/tmp"));
        insert_bg_task(&mut agent, "bg-1", false);
        insert_bg_task(&mut agent, "mon-1", true);
        insert_bg_task(&mut agent, "done-1", false);
        agent.session.bg_tasks.get_mut("done-1").unwrap().status =
            crate::app::session::BgTaskStatus::Done;
        assert_eq!(
            agent.watchers(),
            Watchers {
                commands: 1,
                monitors: 1,
                loops: 0,
                subagents: 0,
                workflows: 0,
            }
        );
    }

    #[test]
    fn workflow_children_coalesce_into_one_workflow_watcher() {
        let mut agent = test_agent_view(Some("s1"), std::path::PathBuf::from("/tmp"));
        agent.session.workflow_runs.push(active_workflow("wf-1"));
        let mut child_a = test_fixtures::running_subagent_info("child-a");
        child_a.workflow_run_id = Some("wf-1".into());
        let mut child_b = test_fixtures::running_subagent_info("child-b");
        child_b.workflow_run_id = Some("wf-1".into());
        agent
            .session
            .subagent_sessions
            .insert("child-a".into(), child_a);
        agent
            .session
            .subagent_sessions
            .insert("child-b".into(), child_b);

        assert_eq!(
            agent.watchers(),
            Watchers {
                commands: 0,
                monitors: 0,
                loops: 0,
                subagents: 0,
                workflows: 1,
            }
        );
    }

    #[test]
    fn standalone_subagent_and_workflow_remain_distinct_watchers() {
        let mut agent = test_agent_view(Some("s1"), std::path::PathBuf::from("/tmp"));
        agent.session.workflow_runs.push(active_workflow("wf-1"));
        let mut workflow_child = test_fixtures::running_subagent_info("workflow-child");
        workflow_child.workflow_run_id = Some("wf-1".into());
        agent
            .session
            .subagent_sessions
            .insert("workflow-child".into(), workflow_child);
        agent.session.subagent_sessions.insert(
            "standalone-child".into(),
            test_fixtures::running_subagent_info("standalone-child"),
        );

        assert_eq!(
            agent.watchers(),
            Watchers {
                commands: 0,
                monitors: 0,
                loops: 0,
                subagents: 1,
                workflows: 1,
            }
        );
    }
}

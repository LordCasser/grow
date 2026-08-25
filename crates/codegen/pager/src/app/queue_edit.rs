//! Queued-prompt editing (`PromptMode::EditingQueued`) state machine.
//!
//! Extracted from `agent_view.rs` as a sibling `impl AgentView` block (same
//! pattern as shell's `compaction.rs`): entry from the queue pane,
//! editing-mode key intercepts, the dirty-edit focus lock, and the
//! exit/cleanup paths.
//!
//! Stash invariant: `stashed_prompt` is set exactly once on entry
//! (`enter_queue_edit`) and `take()`n exactly once on exit —
//! `exit_editing_mode` is the sole restore owner: every exit (including
//! lost-row cancel) restores the draft exactly once.
//!
//! A dirty pane switch is blocked and never arms the undrawn `EditConfirm`
//! modal, which would otherwise capture all input invisibly.

use crossterm::event::{KeyCode, KeyEvent};

use crate::key;
use crate::views::modal::{ActiveModal, EditConfirmResult, ModalConfirmation};
use crate::views::queue_pane::QueueRowRef;

use super::actions::Action;
use super::agent_view::{AgentPane, AgentView, PromptInputMode};
use super::app_view::InputOutcome;

/// State of the prompt widget's editing context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptMode {
    /// Normal mode: typing a new prompt to send.
    Normal,
    /// Editing a queued prompt.
    EditingQueued {
        /// Stable selection ID of the prompt being edited. For local rows
        /// this is the `QueuedPrompt.id` monotonic counter; for server rows
        /// this is the synthesized `QueuedPromptEntry.id` (a hash of
        /// `server_id`) so [`AgentView::queue`] selection works uniformly.
        id: u64,
        /// Snapshot of the original text (for dirty detection).
        original: String,
        /// When `Some`, this is a server-authoritative shared-queue row and
        /// `server_id` is the agent's stable `prompt_id`. On save we route
        /// the change through `Action::QueueEditShared` (and rely on the
        /// `grow/queue/changed` rebroadcast for the visual result) instead
        /// of mutating the local `pending_prompts` mirror. `None` is the
        /// pre-existing local-origin path.
        server_id: Option<String>,
        /// Kind snapshot for the interject guard's vanished-row fallback.
        kind: crate::app::agent::QueueEntryKind,
    },
}

impl AgentView {
    /// Editing-mode key intercepts for the prompt pane.
    ///
    /// Bare Enter saves, Esc (or Ctrl-C on empty) cancels. Shift/Alt+Enter
    /// inserts a newline (same as the normal composer) and must not save.
    /// Apple Terminal Cmd/Shift/Opt+Enter is rescued inside `is_mod_enter`
    /// via CoreGraphics — not a universal Cmd+Enter binding.
    /// Interject is remappable, so it is handled via the
    /// `ActionId::InterjectPrompt` registry arm → `interject_editing_queued_intercept`,
    /// not matched as a raw key here.
    ///
    /// Returns `None` when not editing or unhandled — must fall through to the widget.
    pub(super) fn handle_editing_queued_key(
        &mut self,
        key: &KeyEvent,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) -> Option<InputOutcome> {
        if let PromptMode::EditingQueued { id, server_id, .. } = &self.prompt_mode {
            let (id, server_id) = (*id, server_id.clone());
            let ctrl_c_empty = key!('c', CONTROL).matches(key) && self.prompt.text().is_empty();

            // Before bare-Enter save: Shift/Alt flags, or Apple Terminal bare
            // Enter with Cmd/Shift/Opt held (CoreGraphics rescue in is_mod_enter).
            if crate::input::is_mod_enter(key) {
                self.prompt.textarea.insert_str("\n");
                return Some(InputOutcome::Changed);
            }
            if key!(Enter).matches(key) && !self.prompt.text().trim().is_empty() {
                return Some(self.save_edited_queued_row(id, server_id, true, effects));
            }
            if key.code == KeyCode::Esc || ctrl_c_empty {
                if let Some(effect) = self.exit_editing_mode() {
                    effects.push(effect);
                }
                return Some(InputOutcome::Action(Action::DrainQueue));
            }
        }
        None
    }

    /// Dirty-edit focus lock checked by `set_active_pane`.
    ///
    /// `None` = not editing / no lock — the caller proceeds with the normal
    /// pane switch. `Some(switched)` = the lock handled the switch: `false`
    /// when blocked behind the confirm modal, `true` after a clean exit.
    pub(super) fn editing_lock_on_pane_switch(
        &mut self,
        target: AgentPane,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) -> Option<bool> {
        if let PromptMode::EditingQueued { ref original, .. } = self.prompt_mode
            && target != AgentPane::Prompt
        {
            let dirty = self.prompt.text() != original;
            if dirty {
                // Block the switch; never arm `EditConfirm` here (it has no draw
                // arm, so it would capture all input invisibly). Resolve with Enter/Esc.
                // Clear the overlay-focus flip toggle callers set before switching.
                match target {
                    AgentPane::Queue => self.queue.overlay.focused = false,
                    AgentPane::Todo => self.todo.overlay.focused = false,
                    AgentPane::Tasks => self.tasks.overlay.focused = false,
                    AgentPane::Catalog => self.catalog.overlay.focused = false,
                    _ => {}
                }
                self.show_toast("Editing a queued prompt: press Enter to save, Esc to discard");
                return Some(false); // blocked, no modal armed
            }
            // Clean edit — silently exit editing mode.
            if let Some(effect) = self.exit_editing_mode() {
                effects.push(effect);
            }
            self.active_pane = target;
            return Some(true);
        }
        None
    }

    /// Resolve an `EditConfirm` modal keypress (the modal has already been
    /// `take()`n out of `active_modal` by `handle_modal_key`).
    pub(super) fn handle_edit_confirm_choice(
        &mut self,
        confirm: ModalConfirmation<EditConfirmResult>,
        pending_target: AgentPane,
        ch: char,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) -> InputOutcome {
        if let Some(result) = confirm.resolve(ch) {
            let was_drain_blocked = self.drain_blocked();
            match result {
                EditConfirmResult::Cancel => {
                    // Dismiss dialog, stay in editing mode.
                    // (active_modal already taken)
                }
                EditConfirmResult::Save => {
                    // Empty edit: keep the original row text — a
                    // queued prompt must never be blanked by Save.
                    if self.prompt.text().trim().is_empty() {
                        if let Some(effect) = self.exit_editing_mode() {
                            effects.push(effect);
                        }
                        self.force_active_pane(pending_target);
                        if was_drain_blocked {
                            return InputOutcome::Action(Action::DrainQueue);
                        }
                        return InputOutcome::Changed;
                    }
                    let outcome = match self.prompt_mode.clone() {
                        PromptMode::EditingQueued { id, server_id, .. } => {
                            // Drain only when "save & send" was the
                            // advertised label (see helper doc).
                            self.save_edited_queued_row(id, server_id, was_drain_blocked, effects)
                        }
                        // Unreachable in practice: the modal only
                        // opens from `EditingQueued`.
                        _ => {
                            if let Some(effect) = self.exit_editing_mode() {
                                effects.push(effect);
                            }
                            InputOutcome::Changed
                        }
                    };
                    self.force_active_pane(pending_target);
                    return outcome;
                }
                EditConfirmResult::Discard => {
                    // Discard changes (revert to original), exit editing.
                    if let Some(effect) = self.exit_editing_mode() {
                        effects.push(effect);
                    }
                    self.force_active_pane(pending_target);
                    if was_drain_blocked {
                        return InputOutcome::Action(Action::DrainQueue);
                    }
                    return InputOutcome::Changed;
                }
                EditConfirmResult::Delete => {
                    // Delete the prompt entirely from the queue.
                    // Server-origin rows route through
                    // `Action::QueueRemoveShared`; local rows mutate
                    // the mirror.
                    if let PromptMode::EditingQueued {
                        id: _,
                        server_id: Some(server_id),
                        ..
                    } = self.prompt_mode.clone()
                    {
                        let expected_version = self
                            .session
                            .shared_queue
                            .iter()
                            .find(|e| e.id == server_id)
                            .map(|e| e.version)
                            .unwrap_or(0);
                        if let Some(effect) = self.exit_editing_mode() {
                            effects.push(effect);
                        }
                        self.force_active_pane(pending_target);
                        return InputOutcome::Action(Action::QueueRemoveShared {
                            id: server_id,
                            expected_version,
                        });
                    }
                    if let PromptMode::EditingQueued { id, .. } = self.prompt_mode {
                        self.session.pending_prompts.retain(|p| p.id != id);
                    }
                    if let Some(effect) = self.exit_editing_mode() {
                        effects.push(effect);
                    }
                    self.force_active_pane(pending_target);
                    // If drain was blocked and we deleted the front,
                    // the next prompt (if any) should now drain.
                    if was_drain_blocked {
                        return InputOutcome::Action(Action::DrainQueue);
                    }
                    return InputOutcome::Changed;
                }
            }
        } else {
            // Key didn't match any option — restore modal, keep blocking.
            self.active_modal = Some(ActiveModal::EditConfirm {
                modal: confirm,
                pending_target,
            });
        }
        InputOutcome::Changed
    }

    /// Enter editing mode for the queue row selected via
    /// `QueueEvent::EditSelected` (called from `handle_queue_key`).
    pub(super) fn enter_queue_edit(
        &mut self,
        id: u64,
        is_server: bool,
        row: Option<QueueRowRef>,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) {
        use crate::app::agent::QueueEntryKind;
        // Still an optimistic echo: its `session/prompt` RPC is in flight, so
        // the shell has no row to hold yet — the `hold_edit` would no-op and
        // the later-confirmed row could be absorbed while the composer edits
        // it. Ignore until the confirming `grow/queue/changed` lands (mirrors
        // the send-now park gate in `force_interject_queue_row`).
        if let Some(sid) = row.as_ref().and_then(|r| r.server_id.as_deref())
            && self.session.has_optimistic_queue_echo(sid)
        {
            return;
        }
        type QueueEditEntryData = (
            String,
            QueueEntryKind,
            Option<String>,
            Vec<crate::prompt_images::PastedImage>,
            Vec<crate::app::agent::ChipElement>,
        );

        // Resolve text + display kind from whichever mirror owns
        // the row, plus the server `prompt_id` for server-origin
        // rows. The save path in `save_edited_queued_row` / the
        // modal-confirm `Save` arm branches on `server_id`.
        let entry_data: Option<QueueEditEntryData> = if is_server {
            row.as_ref()
                .and_then(|r| r.server_id.clone())
                .and_then(|server_id| {
                    self.session
                        .shared_queue
                        .iter()
                        .find(|e| e.id == server_id)
                        .map(|w| {
                            (
                                w.text.clone(),
                                crate::views::queue_pane::kind_from_wire(&w.kind),
                                Some(server_id),
                                Vec::new(),
                                Vec::new(),
                            )
                        })
                })
        } else {
            // Only local rows own image and chip state.
            self.session
                .pending_prompts
                .iter()
                .find(|p| p.id == id)
                .map(|p| {
                    (
                        p.text.clone(),
                        p.kind,
                        None,
                        p.images.clone(),
                        p.chip_elements.clone(),
                    )
                })
        };
        if let Some((text, kind, server_id, images, chip_elements)) = entry_data {
            self.stashed_prompt = if self.prompt.text().is_empty() {
                None
            } else {
                Some(self.prompt.stash())
            };
            // Load queued text and enter editing mode.
            // Set prompt_input_mode based on entry kind so the prompt
            // renders with the correct visual (yellow `!` prefix
            // for bash entries, normal for prompts/commands).
            self.prompt
                .restore(crate::views::prompt_widget::StashedPrompt::from_submission(
                    text.clone(),
                    images,
                    chip_elements,
                ));
            // `server_id: Some(_)` routes the save through
            // `Action::QueueEditShared` (server LWW); `None` is
            // the existing local-mirror mutation path.
            self.prompt_mode = PromptMode::EditingQueued {
                id,
                original: text,
                server_id: server_id.clone(),
                kind,
            };
            self.prompt_input_mode = if kind == QueueEntryKind::BashCommand {
                PromptInputMode::Bash
            } else {
                PromptInputMode::Normal
            };
            self.set_active_pane(AgentPane::Prompt, effects);
            if let (Some(sid), Some(session_id)) = (server_id, self.session.session_id.clone()) {
                effects.push(crate::app::actions::Effect::QueueHoldEdit {
                    session_id,
                    id: sid,
                });
            }
        }
    }

    /// Save the edited composer text back to the queued row and exit edit
    /// mode. Single owner of the save invariants for the bare-Enter
    /// intercept, the idle edit-interject, and the modal Save arm.
    ///
    /// `drain`: whether a local-row save requests a queue drain. Enter-save
    /// and idle edit-interject always drain (the user just released the
    /// front edit lock); modal Save drains only when the drain was blocked
    /// on this edit — a plain save of a non-front row must not start the
    /// head prompt's turn.
    fn save_edited_queued_row(
        &mut self,
        id: u64,
        server_id: Option<String>,
        drain: bool,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) -> InputOutcome {
        match server_id {
            Some(server_id) => {
                let new_text = self.prompt.text().to_string();
                // Server-origin row: route the edit through the agent (LWW); the
                // rebroadcast updates every client's mirror, so don't mutate
                // locally. Keep the hold until the edit lands — see
                // `exit_editing_mode_keeping_hold`.
                self.exit_editing_mode_keeping_hold();
                InputOutcome::Action(Action::QueueEditShared {
                    id: server_id,
                    new_text,
                })
            }
            None => {
                let edited = self.prompt.stash();
                let (new_text, mut images, chip_elements) = edited.into_submission();
                // Local row: in-place mutation (existing behavior).
                // Recompute token ranges for the edited text — the stale
                // ranges would point at the pre-edit byte offsets.
                let skill_token_ranges = self
                    .prompt
                    .slash_controller
                    .recognized_token_ranges(&new_text, &self.session.models);
                if let Some(entry) = self.session.pending_prompts.iter_mut().find(|p| p.id == id) {
                    let retained: std::collections::HashSet<u64> = images
                        .iter()
                        .map(|image| image.preview.identity())
                        .collect();
                    for old in entry.images.drain(..) {
                        if !retained.contains(&old.preview.identity()) {
                            crate::prompt_images::cleanup_temp_file(&old);
                        }
                    }
                    entry.text = new_text;
                    entry.images = std::mem::take(&mut images);
                    entry.chip_elements = chip_elements;
                    entry.skill_token_ranges = skill_token_ranges;
                    // Clear stale wire_blocks — edited text may no longer match
                    // the original skill invocation. The prompt will be sent as
                    // plain text via the normal path. If it still starts with `/`,
                    // the shell's resolve() handles it.
                    entry.wire_blocks = None;
                    // display_as_skill rides wire_blocks (see its field doc) — clear both
                    // together, or the drain keeps stale skill styling over the ranges.
                    entry.display_as_skill = false;
                }
                crate::prompt_images::drain_and_cleanup(&mut images);
                if let Some(effect) = self.exit_editing_mode() {
                    effects.push(effect);
                }
                if drain {
                    InputOutcome::Action(Action::DrainQueue)
                } else {
                    InputOutcome::Changed
                }
            }
        }
    }

    /// Interject-key intercept while editing a queued row, delegated from
    /// the `ActionId::InterjectPrompt` registry arm in `handle_prompt_key`.
    /// Falling through would strand `EditingQueued` with the row still
    /// queued (dirty-modal loop + blocked drain). `None` = not editing —
    /// the arm proceeds with its normal interject handling.
    pub(super) fn interject_editing_queued_intercept(
        &mut self,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) -> Option<InputOutcome> {
        if let PromptMode::EditingQueued {
            id,
            server_id,
            kind,
            ..
        } = &self.prompt_mode
        {
            let (id, server_id, kind) = (*id, server_id.clone(), *kind);
            return Some(self.interject_edited_queued(id, server_id, kind, effects));
        }
        None
    }

    /// Interject key pressed while editing a queued row: turn running →
    /// interject the EDITED text and remove the row; idle → bare-Enter
    /// save; empty composer → no-op, stay in edit mode.
    fn interject_edited_queued(
        &mut self,
        id: u64,
        server_id: Option<String>,
        kind: crate::app::agent::QueueEntryKind,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) -> InputOutcome {
        let text = self.prompt.text().trim().to_string();
        if text.is_empty() {
            return InputOutcome::Changed;
        }
        if !self.session.state.is_turn_running() {
            return self.save_edited_queued_row(id, server_id, true, effects);
        }
        // Non-prompt rows stay queued (see `queue_row_prompt_like`): save the edit.
        let row_prompt_like = self.queue_row_prompt_like(id);
        if row_prompt_like == Some(false) {
            self.show_toast("Can't send this mid-turn — it runs when the current turn ends");
            return self.save_edited_queued_row(id, server_id, true, effects);
        }
        if row_prompt_like.is_none() && kind != crate::app::agent::QueueEntryKind::Prompt {
            self.show_toast("Queued prompt is no longer in the queue");
            return self.save_edited_queued_row(id, server_id, true, effects);
        }
        match server_id {
            Some(server_id) => {
                // Server rows: the queue wire (`grow/queue/interject` newText)
                // is text-only, so composer images can't ride along — known
                // limitation, dropped with an accurate toast.
                if !self.prompt.images.is_empty() {
                    self.prompt.images.clear();
                    self.show_toast("Images can't be attached when editing a shared queued prompt");
                }
                // new_text carries the edit — without it the agent would
                // interject the original server-side text. Release is safe
                // here: interject removes the row from the queue (no
                // combine-on-stale-text window for a still-queued hold).
                let expected_version = self.queue.row_ref(id).map(|r| r.version);
                if let Some(effect) = self.exit_editing_mode() {
                    effects.push(effect);
                }
                match expected_version {
                    Some(expected_version) => InputOutcome::Action(Action::QueueInterjectShared {
                        id: server_id,
                        expected_version,
                        new_text: Some(text),
                    }),
                    // Row vanished from the mirror — just interject the text.
                    None => InputOutcome::Action(Action::Interject {
                        text,
                        images: vec![],
                    }),
                }
            }
            None => {
                // Exit before row removal so auto-hide cannot re-enter or strand edit mode.
                let edited = self.prompt.stash();
                let (_, images, _) = edited.into_submission();
                if let Some(effect) = self.exit_editing_mode() {
                    effects.push(effect);
                }
                if let Some(mut row) = self.remove_local_queue_row(id, effects) {
                    let retained: std::collections::HashSet<u64> = images
                        .iter()
                        .map(|image| image.preview.identity())
                        .collect();
                    for old in row.images.drain(..) {
                        if !retained.contains(&old.preview.identity()) {
                            crate::prompt_images::cleanup_temp_file(&old);
                        }
                    }
                }
                InputOutcome::Action(Action::Interject { text, images })
            }
        }
    }

    /// Whether the drain is blocked because the user is editing the front prompt.
    pub(crate) fn drain_blocked(&self) -> bool {
        if let PromptMode::EditingQueued { id, .. } = &self.prompt_mode {
            self.session.state.is_idle()
                && self
                    .session
                    .pending_prompts
                    .front()
                    .is_some_and(|p| p.id == *id)
        } else {
            false
        }
    }

    /// Exit a server-origin queue edit whose row vanished, restoring the pre-edit draft.
    pub(crate) fn cancel_editing_queued_for_lost_row(
        &mut self,
    ) -> Option<crate::app::actions::Effect> {
        if !matches!(
            self.prompt_mode,
            PromptMode::EditingQueued {
                server_id: Some(_),
                ..
            }
        ) {
            return None;
        }
        // Restore the pre-edit draft; keeping the orphaned edit text would look
        // "duplicated" (the row is now the running turn). A concurrent-removal edit is lost.
        let effect = self.exit_editing_mode();
        self.show_toast("Queued prompt is no longer in the queue");
        effect
    }

    /// Exit editing mode: restore stashed text, clear mode, focus queue pane.
    /// No-op unless `EditingQueued`. The default exit; releases the
    /// server-side combine hold (cancel, lost-row, interject, modal paths).
    ///
    /// Always resets `prompt_input_mode` to `Normal` so it doesn't leak
    /// into subsequent normal prompt entry.
    pub(super) fn exit_editing_mode(&mut self) -> Option<crate::app::actions::Effect> {
        self.exit_editing_mode_inner(true)
    }

    /// Exit editing without emitting `QueueReleaseEdit` — the server-row save
    /// path's `QueueEditShared` clears the hold on the shell instead. Releasing
    /// here would flush first (via the caller's root effect mailbox) and let combine merge the
    /// row on stale text before the edit lands.
    fn exit_editing_mode_keeping_hold(&mut self) {
        let release_effect = self.exit_editing_mode_inner(false);
        debug_assert!(release_effect.is_none());
    }

    fn exit_editing_mode_inner(
        &mut self,
        release_hold: bool,
    ) -> Option<crate::app::actions::Effect> {
        // Idempotent: remove_local_queue_row's guard may have exited already;
        // a second take() of the spent stash would wipe the composer.
        if !matches!(self.prompt_mode, PromptMode::EditingQueued { .. }) {
            return None;
        }
        let release_effect = if release_hold
            && let PromptMode::EditingQueued {
                server_id: Some(sid),
                ..
            } = &self.prompt_mode
            && let Some(session_id) = self.session.session_id.clone()
        {
            Some(crate::app::actions::Effect::QueueReleaseEdit {
                session_id,
                id: sid.clone(),
            })
        } else {
            None
        };
        let stash = self.stashed_prompt.take().unwrap_or_default();
        self.prompt.restore(stash);
        self.finish_editing_exit();
        release_effect
    }

    /// Shared tail of every edit exit — stash policy stays with the callers.
    fn finish_editing_exit(&mut self) {
        self.prompt_mode = PromptMode::Normal;
        self.prompt_input_mode = PromptInputMode::Normal;
        // Editing is over, so a pending EditConfirm is meaningless — left
        // behind it would eat all input without ever rendering. Other
        // modal variants are untouched.
        if matches!(self.active_modal, Some(ActiveModal::EditConfirm { .. })) {
            self.active_modal = None;
        }
        // Return focus to queue pane (if still visible).
        // Force=true: we just cleared editing mode, no lock to check.
        if self.queue.is_visible() {
            self.force_active_pane(AgentPane::Queue);
        } else {
            self.force_active_pane(AgentPane::Scrollback);
        }
    }
}

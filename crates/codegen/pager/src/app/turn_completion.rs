//! Finalizing a turn from a terminal turn signal.
//!
//! The durable, persisted and replayed `GrowSessionUpdate::TurnCompleted`
//! and the driver's `PromptResponse` are the two terminal rails. Both enter
//! the same first-wins finalizer ([`finalize_prompt_terminal`]): the first
//! signal whose prompt id exactly matches the running turn wins, finishes
//! the turn, pushes the marker, and runs the full turn-end teardown exactly
//! once. The losing rail contributes only its extra metadata — a late
//! `PromptResponse` merges usage / structured output / error details into
//! [`crate::app::agent::AgentSession::finalized_pr_meta`] instead of
//! re-finishing or re-marking.
//!
//! The durable rail may only finalize a *terminal* turn state
//! (`TurnRunning`/`TurnCancelling`). While a turn is still `TurnSubmitting`
//! (the prompt RPC is in flight and the server has not confirmed the
//! foreground) a durable terminal is **Ignored** — recovery from a lost
//! submission is the prompt-status watchdog's job (Phase C). The PR rail is
//! the driver's own RPC terminal and may finalize from `TurnSubmitting`.

use std::time::Duration;

use crate::notifications::{NotificationEvent, NotificationEventKind};
use crate::scrollback::blocks::SessionEvent;
use agent_client_protocol as acp;

use super::agent::{AgentId, FinalizedPrMeta};
use super::agent_view::AgentView;
use super::app_view::AppView;
use super::dispatch::drain_root_permission_queue;

/// Push a turn-terminal marker ("Turn completed/cancelled/failed"), folding
/// any pending stop/stop_failure hook runs into it so they render inline
/// (right-justified) on the marker line instead of as a standalone block.
///
/// Both marker rails route through here: the driver's `PromptResponse` and
/// durable terminal finalization — the single finalizer pushes the marker
/// only when it wins the first-wins gate. (Wake turns route through
/// `finish_wake_turn` in acp_handler, which maps their stop reason and calls
/// here only when a marker is due.) `event == None`
/// (bash turns, rate-limit / re-auth UX that replaces the marker) flushes the
/// held hooks as the legacy standalone lifecycle block so failures stay
/// visible.
///
/// A stamped stash folds only on an exact ending-id match. On a mismatch it
/// flushes standalone (the ending turn is THE turn — an older stash has no
/// marker coming). An unstamped stash keeps the legacy
/// stashed-during-this-turn heuristic.
pub(super) fn push_turn_terminal_marker(
    agent: &mut AgentView,
    event: Option<SessionEvent>,
    ending_prompt_id: Option<&str>,
) {
    let pending = agent.pending_stop_hooks.take();
    let groups = match pending {
        None => Vec::new(),
        Some(pending) => {
            let stale = match (pending.prompt_id.as_deref(), ending_prompt_id) {
                (Some(stashed), Some(ending)) => stashed != ending,
                (Some(_), None) => true,
                (None, _) => false,
            };
            if stale {
                for (name, runs) in pending.groups {
                    agent.scrollback.push_lifecycle_hooks(name, runs);
                }
                Vec::new()
            } else {
                pending.groups
            }
        }
    };

    match event {
        Some(event) => {
            agent.push_end_marker_block(event, groups, ending_prompt_id.map(str::to_string));
        }
        None => {
            for (name, runs) in groups {
                agent.scrollback.push_lifecycle_hooks(name, runs);
            }
        }
    }
}

/// Metadata a terminal signal carries beyond the prompt id. The durable
/// rail and the `PromptResponse` rail each fill the fields they know; the
/// finalizer's marker / notification decisions are driven by these options
/// so both rails converge on one teardown.
pub(super) struct TerminalMeta {
    /// Whether the signal represents a successful end (PR rail: the RPC
    /// resolved `Ok`; durable rail: the stop reason is a success reason).
    pub pr_ok: bool,
    /// Failure text (PR rail: the `Err` payload; durable rail: `agentResult`
    /// for the `error` stop reason). `None` = the signal is not a failure.
    pub failed_error: Option<String>,
    /// Whether the turn was cancelling when the signal landed (PR rail: the
    /// local state + wire stop reason; durable rail: `cancelled`).
    pub was_cancelling: bool,
    /// Bash turn: no completion marker (the execute block is the visual
    /// entry) and no TurnComplete notification.
    pub bash_turn: bool,
    /// A dedicated UX already rendered the failure (PR rail: rate-limit /
    /// model-incompatible / context-overflow; durable rail: `rate_limit`):
    /// suppress the TurnFailed marker and the error notification.
    pub skip_error_marker: bool,
    /// Whether this signal may finalize from `TurnSubmitting`. The PR rail
    /// may (the driver's own RPC terminal is authoritative); the durable
    /// rail may not — a durable terminal during `TurnSubmitting` is Ignored
    /// and recovery is the prompt-status watchdog's job (Phase C).
    pub accepts_submitting: bool,
}

/// What applying a terminal turn signal did to one agent, plus the
/// app-level outcome the caller must apply after the agent borrow ends.
pub(super) struct TerminalOutcome {
    pub apply: TerminalApply,
    /// Turn-end notification decision (TurnComplete / AgentError), computed
    /// from the signal BEFORE `finish_turn` clears the flags it depends on
    /// (`rate_limited`, `model_incompatible`, `bash_turn`).
    pub notification: Option<(NotificationEventKind, String)>,
}

impl TerminalOutcome {
    fn ignored() -> Self {
        Self {
            apply: TerminalApply::Ignored,
            notification: None,
        }
    }
}

/// What applying a terminal turn signal did to one agent.
pub(super) enum TerminalApply {
    /// No change: a driver turn the signal does not provably match, or a
    /// duplicate/stale terminal for an already-finished viewer turn.
    Ignored,
    /// A prompt-id-authoritative terminal won and finalized the foreground
    /// turn (marker + full teardown). The caller performs the queue/adoption
    /// handoff exactly once.
    ViewerFinalized,
}

/// Apply the durable `TurnCompleted` rail. An exact prompt id may finish
/// either a driver or a viewer immediately. A later `PromptResponse`
/// contributes only its extra metadata (see [`merge_finalized_pr_meta`]).
/// A durable terminal while the turn is still `TurnSubmitting` is Ignored
/// (the prompt-status watchdog owns that recovery; Phase C).
pub(super) fn finalize_turn_from_durable_terminal(
    agent: &mut AgentView,
    _session_id: &str,
    prompt_id: &str,
    stop_reason: Option<&str>,
    agent_result: Option<&str>,
    _cancel_trigger: Option<&str>,
) -> TerminalOutcome {
    let stop_reason = stop_reason.unwrap_or_default();
    let meta = TerminalMeta {
        pr_ok: !matches!(stop_reason, "cancelled" | "error" | "rate_limit"),
        failed_error: (stop_reason == "error").then(|| {
            agent_result
                .map(str::to_string)
                .unwrap_or_else(|| "unknown error".to_string())
        }),
        was_cancelling: stop_reason == "cancelled",
        bash_turn: agent.bash_turn,
        skip_error_marker: stop_reason == "rate_limit",
        accepts_submitting: false,
    };
    finalize_prompt_terminal(agent, Some(prompt_id), meta)
}

/// First-wins prompt transition shared by both terminal rails. State alone
/// is insufficient: an old terminal may arrive after a new turn has started,
/// so an exact prompt identity is required whenever the signal carries one
/// (the durable rail always does). A pid-less `PromptResponse` (older shell
/// without promptId meta) is attributed to the running turn, matching the
/// legacy behavior — but only while a turn is actually running: after any
/// finalize (state Idle) it is ignored, so a stale response can never push a
/// second marker or re-run the teardown.
///
/// The winning signal runs the ENTIRE turn-end teardown (permission queue
/// drain, plan approval dismissal, cancel-panel cleanup, bash wrap-up,
/// prompt-suggestion wipe) exactly once; the losing signal must not repeat
/// any of it.
pub(super) fn finalize_prompt_terminal(
    agent: &mut AgentView,
    prompt_id: Option<&str>,
    meta: TerminalMeta,
) -> TerminalOutcome {
    let current_prompt_id = agent.session.current_prompt_id.clone();
    match (&current_prompt_id, prompt_id) {
        // Exact identity gate: a pid-bearing signal must match the running
        // turn's pid, or it is not this turn's terminal.
        (Some(current), Some(pid)) if current != pid => return TerminalOutcome::ignored(),
        // A pid-bearing signal with no running turn can never match.
        (None, Some(_)) => return TerminalOutcome::ignored(),
        // Pid-less signal with no tracked turn: attribute to a genuinely
        // running turn (legacy old-shell behavior); after any finalize
        // (state Idle, current cleared) it is ignored.
        (None, None) => {
            if !meta.accepts_submitting
                || !(agent.session.state.is_turn_running() || agent.session.state.is_cancelling())
            {
                return TerminalOutcome::ignored();
            }
        }
        // Pid-less signal with a tracked turn: attributed to it below.
        (Some(_), None) => {}
        // Exact identity match: this signal is the running turn's terminal.
        (Some(_), Some(_)) => {}
    }
    if !meta.accepts_submitting && !agent.session.state.is_terminal_turn() {
        // Durable rail contract: while `TurnSubmitting` the server has not
        // confirmed the foreground, so a durable terminal cannot be trusted
        // to end the right turn; the prompt-status watchdog owns recovery.
        return TerminalOutcome::ignored();
    }

    // Capture elapsed BEFORE `mark_turn_finished()` clears `turn_started_at`.
    // The anchor was back-dated from the authoritative `turnStartMs` on
    // adoption, so this reads the same wall-clock duration the driver shows.
    let elapsed_opt = agent.turn_elapsed();
    let elapsed = elapsed_opt.unwrap_or_default();
    // Read before `finish_turn()` clears it; keys the pending stop-hook stash.
    let ending_prompt_id = agent
        .session
        .current_prompt_id
        .clone()
        .or_else(|| prompt_id.map(str::to_string));

    let event = terminal_marker_event(&meta, elapsed_opt, elapsed);
    let notification = terminal_notification(&meta, elapsed_opt);

    agent.session.finish_turn(&mut agent.scrollback);
    push_turn_terminal_marker(agent, event, ending_prompt_id.as_deref());
    agent.scrollback.seal_subagent_permission_group();

    // ── Full turn-end teardown (the PR rail's sequence, now shared) ──
    agent.mark_turn_finished();
    agent.activity_started_at = None;
    agent.last_activity = None;

    // Drain all queued permission requests — the turn is over, so any
    // pending permissions are stale. Send Cancelled to each.
    drain_root_permission_queue(agent);

    // Dismiss any active plan approval or review — the turn that produced
    // it has completed, so the state is stale.
    if let Some(mut pav) = agent.plan_approval_view.take() {
        pav.send_stale_cancel();
        agent.plan_next_comment_id = pav.next_comment_id;
        agent.prompt.restore(pav.stashed_prompt);
        agent.line_viewer = None;
    }

    agent.cancel_turn_view = None;
    agent.cancel_turn_buttons.clear();

    // After a bash-mode turn, scroll to bottom so the user sees the command
    // output, but keep focus on the prompt for consistency with normal
    // prompt behavior.
    if meta.bash_turn {
        agent.bash_turn = false;
        agent.scrollback.goto_bottom();
    }
    // Predicted-next-prompt (tab autocomplete): wipe any stale suggestion at
    // every turn boundary.
    agent.prompt.prompt_suggestion.clear();

    // First-wins winner: a late PromptResponse for this pid merges metadata
    // only. Cleared when the next turn starts (`start_turn_boundary`). A
    // pid-less finalize (legacy shell, no tracked pid) records nothing —
    // there is no id a late response could be matched against.
    if let Some(pid) = &ending_prompt_id {
        agent.session.finalized_prompt = Some(pid.clone());
    }

    TerminalOutcome {
        apply: TerminalApply::ViewerFinalized,
        notification,
    }
}

/// Map a terminal signal to its lifecycle marker. Preserves both rails'
/// rules exactly:
/// - an RPC failure → `TurnFailed`, unless a dedicated UX already rendered
///   it (rate-limit / context-overflow / model-incompatible, or the durable
///   `rate_limit` stop reason);
/// - a cancel → `TurnCancelled`;
/// - a bash turn → no marker (the execute block is the visual entry);
/// - anything else → `TurnCompleted`.
///
/// `elapsed_opt` is the raw turn span (a `TurnFailed` without an anchor
/// renders "Turn failed: …" instead of "in 0.0s"); the other markers use the
/// zero-anchored `elapsed`.
fn terminal_marker_event(
    meta: &TerminalMeta,
    elapsed_opt: Option<Duration>,
    elapsed: Duration,
) -> Option<SessionEvent> {
    if let Some(error) = meta.failed_error.as_deref() {
        if meta.skip_error_marker {
            return None;
        }
        return Some(SessionEvent::TurnFailed {
            error: error.to_string(),
            elapsed: elapsed_opt,
        });
    }
    if meta.was_cancelling {
        return Some(SessionEvent::TurnCancelled { elapsed });
    }
    if meta.bash_turn || (!meta.pr_ok && meta.skip_error_marker) {
        return None;
    }
    Some(SessionEvent::TurnCompleted {
        elapsed: Some(elapsed),
    })
}

/// Turn-end notification decision, computed from the signal before
/// `finish_turn` clears the flags it depends on. Both rails converge: a
/// viewer's turn end resets the terminal title and notifies exactly like the
/// driver's.
fn terminal_notification(
    meta: &TerminalMeta,
    elapsed_opt: Option<Duration>,
) -> Option<(NotificationEventKind, String)> {
    if meta.pr_ok && !meta.was_cancelling && !meta.bash_turn {
        let body = match elapsed_opt {
            Some(d) => format!("Turn complete in {}.", crate::util::format_duration(d)),
            None => String::from("Turn complete."),
        };
        Some((NotificationEventKind::TurnComplete, body))
    } else if let Some(error) = meta.failed_error.as_deref() {
        if meta.skip_error_marker {
            None
        } else {
            Some((NotificationEventKind::AgentError, format!("Error: {error}")))
        }
    } else {
        None
    }
}

/// Merge a late `PromptResponse`'s metadata into the record for the turn the
/// durable rail already finalized. Called only when
/// [`crate::app::agent::AgentSession::finalized_prompt`] matches the response's pid: the response
/// is the same turn's RPC terminal, so only its extra data is applied — no
/// finish, no marker, no drain, no adoption handoff (all ran once at the
/// first-wins finalize).
pub(super) fn merge_finalized_pr_meta(
    agent: &mut AgentView,
    result: &Result<acp::PromptResponse, String>,
) {
    let incoming = match result {
        Ok(pr) => FinalizedPrMeta {
            usage: pr.usage.clone(),
            structured_output: pr
                .meta
                .as_ref()
                .and_then(|m| m.get("structuredOutputError"))
                .and_then(|v| v.as_str())
                .map(|e| Err(e.to_string()))
                .or_else(|| {
                    pr.meta
                        .as_ref()
                        .and_then(|m| m.get("structuredOutput"))
                        .cloned()
                        .map(Ok)
                }),
            error: None,
        },
        Err(err) => FinalizedPrMeta {
            usage: None,
            structured_output: None,
            error: Some(err.clone()),
        },
    };
    // Fill missing fields only, so a second late response cannot clobber
    // data the first one already contributed.
    let slot = agent
        .session
        .finalized_pr_meta
        .get_or_insert_with(FinalizedPrMeta::default);
    if slot.usage.is_none() {
        slot.usage = incoming.usage;
    }
    if slot.structured_output.is_none() {
        slot.structured_output = incoming.structured_output;
    }
    if slot.error.is_none() {
        slot.error = incoming.error;
    }
}

/// Apply the turn-end notification + idle title escapes. Shared by both
/// rails. `queue_empty == false` suppresses the TurnComplete badge (it fires
/// only after the final queued turn) and skips the idle escapes (the next
/// turn starts immediately and would overwrite them).
pub(super) fn apply_terminal_notifications(
    app: &mut AppView,
    agent_id: AgentId,
    notification: Option<(NotificationEventKind, String)>,
    queue_empty: bool,
) {
    let Some((kind, body)) = notification else {
        return;
    };
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return;
    };
    let session_name = agent
        .display_name
        .as_deref()
        .or(agent.generated_session_title.as_deref());

    if queue_empty {
        let cwd_str = app.cwd.to_string_lossy();
        let model = agent.session.models.current_model_name();
        let idle_title = crate::notifications::TitleState {
            session_name,
            model: model.as_deref(),
            activity: None,
            has_pending_permissions: false,
            cwd: Some(&cwd_str),
            turn_elapsed: None,
            is_busy: false,
            focused: true,
        };
        let frame = crate::motion::FrameStamp::capture(app.motion_origin);
        app.pending_notification_escapes = app
            .notification_service
            .build_idle_escapes(&idle_title, frame);
    }

    if kind != NotificationEventKind::TurnComplete || queue_empty {
        // Defer the notification so the terminal has time to apply the idle
        // title. Ghostty debounces setTitle() by 75 ms
        // (SurfaceView_AppKit.swift:576), so we need >75 ms before the
        // notification reads self.title for the subtitle.
        let session_id = agent.session.session_id.as_ref().map(|s| s.0.to_string());
        let notif_title = session_name
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Grow".into());
        app.deferred_notification = Some((
            NotificationEvent {
                kind,
                title: notif_title,
                body,
                session_id,
            },
            std::time::Instant::now() + std::time::Duration::from_millis(100),
        ));
    }
}

/// Map durable terminal finalization to redraw and notification effects.
pub(super) fn apply_terminal_outcome(
    outcome: TerminalOutcome,
    app: &mut AppView,
    agent_id: AgentId,
    is_active: bool,
) -> bool {
    let TerminalOutcome {
        apply,
        notification,
    } = outcome;
    match apply {
        TerminalApply::Ignored => false,
        TerminalApply::ViewerFinalized => {
            let page_flip_entry;
            let mut queue_empty = true;
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                queue_empty = agent.session.pending_prompts.is_empty();
                let drain = super::dispatch::maybe_drain_queue(agent);
                app.pending_effects.extend(drain.effects);
                page_flip_entry = drain.page_flip_entry;
            } else {
                page_flip_entry = None;
            }
            super::dispatch::note_peek_page_flip(app, agent_id, page_flip_entry);
            apply_terminal_notifications(app, agent_id, notification, queue_empty);
            let _ = is_active;
            true
        }
    }
}

#[cfg(test)]
#[path = "turn_completion/tests.rs"]
mod tests;

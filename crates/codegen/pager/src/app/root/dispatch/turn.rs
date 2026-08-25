//! Turn cancellation, task/subagent kills, and prompt-admission recovery.

use super::ctx::find_agent_by_session_id;
use super::permissions::drain_root_permission_queue;
use crate::app::actions::Effect;
use crate::app::agent_view::ActivePane;
use crate::app::agent_view::AgentView;
use crate::app::root::{ActiveView, AppView};
use crate::app::session::InterruptIntent;
use crate::views::modal::GoalInterruptChoice;
use std::time::Instant;

/// Map `[ui].cancel_subagents_on_turn_cancel` / in-memory agent preference to
/// `cancel_subagents` for the cancel wire payload. `None` means prompt.
fn effective_cancel_subagents_preference(
    agent_pref: Option<bool>,
    ui: &shell::agent::config::UiConfig,
) -> Option<bool> {
    agent_pref.or(match ui.cancel_subagents_on_turn_cancel.as_deref() {
        Some("always_stop") => Some(true),
        Some("always_continue") => Some(false),
        _ => None,
    })
}

fn cancel_subagents_pref_canonical(stop: bool) -> &'static str {
    if stop {
        "always_stop"
    } else {
        "always_continue"
    }
}

fn cancel_subagents_pref_canonical_from_ui(ui: &shell::agent::config::UiConfig) -> &'static str {
    match ui.cancel_subagents_on_turn_cancel.as_deref() {
        Some("always_stop") => "always_stop",
        Some("always_continue") => "always_continue",
        _ => "ask",
    }
}

/// Apply a global always-stop / always-continue preference to every agent and
/// `app.current_ui` (in-memory only; caller emits `Effect::PersistSetting`).
pub(super) fn apply_cancel_subagents_preference_global(app: &mut AppView, stop: bool) {
    let canonical = cancel_subagents_pref_canonical(stop);
    app.current_ui.cancel_subagents_on_turn_cancel = Some(canonical.to_string());
    for agent in app.agents.values_mut() {
        agent.cancel_subagents_preference = Some(stop);
    }
}

pub(super) fn dispatch_cancel_turn(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let ui_pref = effective_cancel_subagents_preference(None, &app.current_ui);

    // Scoped agent borrow: extract decisions, then release before `do_cancel_turn`.
    let preferred_cancel_subagents = {
        let Some(agent) = app.agents.get_mut(&id) else {
            return vec![];
        };
        let resolved_pref = agent.cancel_subagents_preference.or(ui_pref);
        // Retry path: a cancel was already sent (`TurnCancelling`) but the turn
        // never resolved — the `session/cancel` notification or the turn-end
        // response may have been lost in transit. Re-send instead of silently
        // no-opping (cancel is idempotent on the agent), so Ctrl+C / palette
        // CancelTurn is never a dead key on a stuck "Cancelling…" spinner.
        // Skips the subagent panel — that
        // choice was already made (or defaulted) on the first cancel.
        if agent.session.state.is_cancelling() {
            let Some(session_id) = agent.session.session_id.clone() else {
                return vec![];
            };
            crate::unified_log::info(
                "cancel.retry",
                Some(&session_id.0),
                Some(serde_json::json!({
                    "current_prompt_id": agent.session.current_prompt_id,
                })),
            );
            return vec![Effect::CancelTurn {
                session_id,
                // Replay the exact original intent (pause_goal AND
                // cancel_subagents); without a stashed intent fall back to the
                // resolved preference (Legacy semantics).
                cancel_subagents: agent
                    .last_interrupt
                    .map_or(resolved_pref.unwrap_or(true), |i| i.cancel_subagents),
                pause_goal: agent.last_interrupt.as_ref().is_some_and(|i| i.pause_goal),
                // A fresh gesture (e.g. a second Ctrl+C on a stuck spinner) re-set
                // the hint; consume it so the re-sent cancel still carries the trigger.
                trigger: agent.cancel_trigger_hint.take(),
                // Retry cancel of a stuck turn — no local prompt rewind here.
                rewind_if_pristine: false,
            }];
        }
        // Goal interrupt: while the Goal is Active, an interrupt gesture ALWAYS
        // opens the Goal panel — never a silent default, and the legacy
        // `cancel_subagents_on_turn_cancel` preference is ignored (product
        // decision: Goal interrupts always ask). The panel covers both "a turn
        // is running" (Full: pause / stop turn / stop turn+subagents) and "no
        // turn" (gap / verifying / planning: pause only) — never pretends to
        // cancel a turn that is not running.
        if agent
            .session
            .goal_state
            .as_ref()
            .is_some_and(|g| matches!(g.status, crate::app::session::GoalDisplayStatus::Active))
        {
            if agent.goal_interrupt_view.is_some() {
                return vec![];
            }
            let has_turn =
                agent.session.state.is_turn_running() || agent.session.state.is_compact_running();
            let running_subagents = agent
                .session
                .subagent_sessions
                .values()
                .filter(|s| s.is_running() && s.workflow_run_id.is_none())
                .count()
                > 0;
            agent.goal_interrupt_view = Some(crate::views::modal::GoalInterruptViewState {
                active_idx: 0,
                choices: if has_turn {
                    crate::views::modal::GoalInterruptChoice::for_active_turn(running_subagents)
                } else {
                    crate::views::modal::GoalInterruptChoice::pause_only()
                },
            });
            // Default focus to the picker (mirrors the Legacy panel).
            if agent.active_pane == ActivePane::Scrollback {
                agent.active_pane = ActivePane::Prompt;
            }
            return vec![];
        }
        if !agent.session.state.is_turn_running() && !agent.session.state.is_compact_running() {
            return vec![];
        }
        if agent.session.state.is_compact_running() {
            // No subagent picker for `/compact` — just stop the generation.
            resolved_pref.or(Some(true))
        } else if let Some(stop) = resolved_pref {
            Some(stop)
        } else {
            // Check all running subagents, not just those from the current turn.
            // This is broader than the old TUI (which filtered by parent_prompt_id),
            // but intentional: subagents kept alive from a previous cancel should
            // still prompt the user on the next cancel.
            let running_count = agent
                .session
                .subagent_sessions
                .values()
                .filter(|s| s.is_running() && s.workflow_run_id.is_none())
                .count();
            if running_count > 0 && agent.cancel_turn_view.is_none() {
                agent.cancel_turn_view = Some(crate::views::modal::CancelTurnViewState {
                    active_idx: 0,
                    running_count,
                });
                // Default focus to the picker so keyboard up/down navigates options
                // immediately. Without this, if the user triggered cancel while the
                // scrollback pane was focused (e.g. browsing history), the modal
                // would open but keystrokes would still go to scrollback — the
                // picker was only reachable via mouse hover/click.
                if agent.active_pane == ActivePane::Scrollback {
                    agent.active_pane = ActivePane::Prompt;
                }
                return vec![];
            }
            None
        }
    };

    do_cancel_turn(app, preferred_cancel_subagents.unwrap_or(true))
}

pub(super) fn dispatch_cancel_turn_choice(
    app: &mut AppView,
    choice: crate::views::modal::CancelTurnChoice,
) -> Vec<Effect> {
    use crate::views::modal::CancelTurnChoice;
    let cancel_subagents = matches!(
        choice,
        CancelTurnChoice::StopRunning | CancelTurnChoice::AlwaysStop
    );

    if let ActiveView::Agent(id) = app.active_view
        && let Some(agent) = app.agents.get_mut(&id)
    {
        agent.cancel_turn_view = None;
        agent.cancel_turn_buttons.clear();
    }

    let mut effects = Vec::new();
    match choice {
        CancelTurnChoice::AlwaysStop | CancelTurnChoice::AlwaysContinue => {
            let stop = matches!(choice, CancelTurnChoice::AlwaysStop);
            let prev_canonical = cancel_subagents_pref_canonical_from_ui(&app.current_ui);
            let new_canonical = cancel_subagents_pref_canonical(stop);
            apply_cancel_subagents_preference_global(app, stop);
            if prev_canonical != new_canonical {
                tracing::info!(
                    target: "settings",
                    key = "cancel_subagents_on_turn_cancel",
                    value = new_canonical,
                    "setting changed",
                );
                effects.push(Effect::PersistSetting {
                    key: "cancel_subagents_on_turn_cancel",
                    value: crate::settings::SettingValue::Enum(new_canonical),
                    rollback_value: crate::settings::SettingValue::Enum(prev_canonical),
                });
            }
        }
        // One-shot choices: apply only to this cancel; global/session pref unchanged.
        CancelTurnChoice::StopRunning | CancelTurnChoice::ContinueToRun => {}
    }

    effects.extend(do_cancel_turn(app, cancel_subagents));
    effects
}

/// Submit an explicit Goal-interrupt choice. Maps the three axes
/// {pause goal, cancel turn, stop subagents} onto the cancel pipeline; with
/// no active turn the "Pause goal" choice routes through the `/goal pause`
/// command plane instead of a fake turn-cancel.
pub(super) fn dispatch_goal_interrupt_choice(
    app: &mut AppView,
    choice: GoalInterruptChoice,
) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let mut effects = Vec::new();
    if let Some(agent) = app.agents.get_mut(&id) {
        agent.goal_interrupt_view = None;
        agent.goal_interrupt_buttons.clear();
    }
    let has_turn = app
        .agents
        .get(&id)
        .is_some_and(|a| a.session.state.is_turn_running() || a.session.state.is_compact_running());
    match choice {
        GoalInterruptChoice::PauseGoal => {
            if has_turn {
                if let Some(agent) = app.agents.get_mut(&id) {
                    agent.last_interrupt = Some(InterruptIntent {
                        pause_goal: true,
                        cancel_subagents: false,
                    });
                }
                effects.extend(do_cancel_turn_with_pause(app, false, true));
            } else if let Some(agent) = app.agents.get_mut(&id)
                && let Some(session_id) = agent.session.session_id.clone()
            {
                // No turn to cancel: `/goal pause` handles the no-turn case
                // (pause the Goal, keep subagents). Same G/T/S semantics as
                // the with-turn path.
                effects.push(Effect::ExecuteSlashCommand {
                    agent_id: id,
                    session_id,
                    command: "/goal pause".into(),
                });
            }
        }
        GoalInterruptChoice::StopTurnOnly => {
            if let Some(agent) = app.agents.get_mut(&id) {
                agent.last_interrupt = Some(InterruptIntent {
                    pause_goal: false,
                    cancel_subagents: false,
                });
            }
            effects.extend(do_cancel_turn_with_pause(app, false, false));
        }
        GoalInterruptChoice::StopTurnAndSubagents => {
            if let Some(agent) = app.agents.get_mut(&id) {
                agent.last_interrupt = Some(InterruptIntent {
                    pause_goal: false,
                    cancel_subagents: true,
                });
            }
            effects.extend(do_cancel_turn_with_pause(app, true, false));
        }
    }
    effects
}

pub(super) fn do_cancel_turn(app: &mut AppView, cancel_subagents: bool) -> Vec<Effect> {
    do_cancel_turn_with_pause(app, cancel_subagents, false)
}

/// [`do_cancel_turn`] plus the explicit "pause the Goal" intent. Only the
/// Goal panel's "Pause goal" choice passes `pause_goal: true`; every other
/// caller keeps the Goal untouched.
pub(super) fn do_cancel_turn_with_pause(
    app: &mut AppView,
    cancel_subagents: bool,
    pause_goal: bool,
) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    if agent.session.state.is_compact_running() {
        agent.session.cancel_compact_command();
        agent.cancel_turn_view = None;
        agent.cancel_turn_buttons.clear();
        drain_root_permission_queue(agent);
        let Some(session_id) = agent.session.session_id.clone() else {
            return vec![];
        };
        return vec![Effect::CancelTurn {
            session_id,
            cancel_subagents,
            pause_goal,
            trigger: agent.cancel_trigger_hint.take(),
            rewind_if_pristine: false,
        }];
    }
    if !agent.session.state.is_turn_running() {
        return vec![];
    }
    // If the server hasn't emitted any activity yet AND there are no other
    // queued prompts, "rewind" the prompt back into the input box and remove
    // its scrollback block. The cancel notification still flies to the
    // server, but the local turn state is reset to Idle immediately so the
    // UI looks like the user never hit Send.
    //
    // Skip rewind when queued prompts exist: restoring the in-flight prompt
    // to the input box while the next queued prompt drains would mix two
    // user intentions in confusing ways. Fall back to the standard cancel
    // flow in that case.
    //
    // Clearing `current_prompt_id` (via `finish_turn`) is what makes orphan
    // chunks/PR for the cancelled turn get dropped by the `promptId` gate
    // in acp_handler / PromptResponse handler.
    // When a prompt is queued on the server-authoritative shared queue, cancel
    // restores the FRONT queued prompt to the input instead (handled after the
    // cleanup below). So skip the in-flight rewind in that case — the user wants
    // the queued prompt back, not the in-flight one.
    //
    // Minimal mode prints each committed block once into the terminal's native
    // scrollback, and that print can't be "un-printed". A user-prompt block
    // commits immediately (it is never `is_running`), so a just-promoted queued
    // prompt's block is already in native scrollback by the time the user can
    // cancel it. Rewinding then `remove_entry`s it from scrollback *state* while
    // the printed copy stays on screen AND restores the text into the input —
    // showing the prompt twice (dogfood bug: double-Esc on a queued prompt). Skip
    // the rewind when the in-flight block has already committed and fall back to
    // the standard cancel. `committed` is always false in alt-screen / inline, so
    // this is a no-op outside minimal.
    let in_flight_committed = match agent.session.in_flight_prompt.as_ref() {
        Some(stashed) => agent.scrollback.is_committed(stashed.scrollback_entry),
        None => false,
    };
    // The rewind REPLACES the composer with the stashed in-flight prompt.
    // Esc (and the mouse stop / palette cancel) fire with the draft intact —
    // unlike keyboard Ctrl+C, which only cancels on an empty prompt — so a
    // non-empty composer holds a NEWER draft the rewind would clobber.
    // Trigger-agnostic on purpose: fall back to the standard cancel.
    let composer_has_draft = !agent.prompt.text().is_empty() || !agent.prompt.images.is_empty();
    let rewinding = agent.session.shared_queue.is_empty()
        && app.cancel_rewind_enabled
        && agent.session.in_flight_prompt.is_some()
        && agent.session.pending_prompts.is_empty()
        && !in_flight_committed
        && !composer_has_draft;
    if rewinding && let Some(stashed) = agent.session.in_flight_prompt.take() {
        if let Some(pid) = agent.session.current_prompt_id.clone() {
            agent.note_rewound_prompt(&pid);
        }
        agent.prompt.set_text(&stashed.text);
        agent.prompt.restore_chip_elements(&stashed.chip_elements);
        agent.prompt.set_images(stashed.images);
        agent.prompt.set_cursor(stashed.text.len());
        for id in stashed.combined_scrollback_entries {
            agent.scrollback.remove_entry(id);
        }
        agent.scrollback.remove_entry(stashed.scrollback_entry);
        // Full state reset: tracker cleanup + state Idle + clear timing
        // fields + clear current_prompt_id.
        agent.session.finish_turn(&mut agent.scrollback);
        agent.mark_turn_finished();
        agent.session.activity_started_at = None;
        agent.session.last_activity = None;
    } else {
        agent.session.cancel_turn(&mut agent.scrollback);
    }
    agent.cancel_turn_view = None;
    agent.cancel_turn_buttons.clear();
    drain_root_permission_queue(agent);
    if let Some(mut pav) = agent.plan_approval_view.take() {
        pav.send_stale_cancel();
        agent.plan_next_comment_id = pav.next_comment_id;
        agent.prompt.restore(pav.stashed_prompt);
        agent.line_viewer = None;
    }

    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };

    // Server-authoritative queue: the agent owns the drain. On an interactive
    // cancel we only tear down the running turn and let the agent promote the
    // FRONT queued prompt as the next turn — its `grow/queue/changed`
    // rebroadcast (carrying `running_prompt_id`) is the source of truth, and the
    // pager adopts it via `handle_queue_changed` / `apply_turn_start_shim`. We
    // do NOT pull any queued prompt back into the input or predict the new queue
    // order client-side; the user's first queued prompt is what runs next.
    vec![Effect::CancelTurn {
        session_id,
        cancel_subagents,
        pause_goal,
        // Consume the gesture hint set by the key/mouse handler (persists
        // through the subagent picker until this final build). `None` for
        // non-gesture callers.
        trigger: agent.cancel_trigger_hint.take(),
        // Mirror the local rewind on the wire: when we restored the prompt to
        // the composer above, ask the shell to trim its pristine copy too so a
        // resend can't pair the kept copy with the new send.
        rewind_if_pristine: rewinding,
    }]
}

/// A submitting prompt that receives no queue or terminal signal asks the
/// shell for authoritative status. Time never fabricates a terminal.
pub(crate) const PROMPT_STATUS_WATCHDOG_DELAY: std::time::Duration =
    std::time::Duration::from_secs(2);

/// A prompt that is already `Running` (admitted) but whose lifecycle
/// signals (TurnCompleted / PromptResponse) went missing asks for
/// authoritative status once this much time passes with no NEW activity.
/// Bounded: one query in flight per prompt, and every non-terminal
/// response simply re-arms the observation window — time never fabricates
/// a terminal; only a status response that reports one ends the turn.
pub(crate) const PROMPT_STATUS_RUNNING_WATCHDOG_DELAY: std::time::Duration =
    std::time::Duration::from_secs(30);

pub(crate) fn poll_stalled_prompt_submissions(
    app: &mut AppView,
    now: std::time::Instant,
) -> Option<Vec<Effect>> {
    let stalled_submissions = app
        .agents
        .iter()
        .filter_map(|(id, agent)| {
            let prompt_id = agent.session.current_prompt_id.as_ref()?;
            // One status query in flight per prompt: the response handler
            // clears the marker, re-arming the watchdog for the next window.
            if agent.session.prompt_status_query_matches(prompt_id) {
                return None;
            }
            let stalled = if agent.session.state.is_turn_submitting() {
                // Submission is silent client-side (no activity can exist),
                // so the elapsed window alone decides.
                agent.session.turn_started_at.is_some_and(|started| {
                    now.saturating_duration_since(started) >= PROMPT_STATUS_WATCHDOG_DELAY
                })
            } else if agent.session.state.is_turn_running() {
                running_turn_stalled(agent, now)
            } else {
                false
            };
            if stalled {
                Some((*id, prompt_id.clone(), agent.session.session_id.clone()?))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if stalled_submissions.is_empty() {
        return None;
    }

    let mut effects = Vec::new();
    for (agent_id, prompt_id, session_id) in stalled_submissions {
        if let Some(agent) = app.agents.get_mut(&agent_id) {
            agent.session.begin_prompt_status_query(prompt_id.clone());
        }
        effects.push(Effect::QueryPromptStatus {
            agent_id,
            session_id,
            prompt_id,
        });
    }
    Some(effects)
}

/// Whether a `TurnRunning` turn looks stalled: the running window elapsed
/// since the turn started, and no NEW activity arrived for the full window.
///
/// "New activity" is reducer-owned: accepted prompt events and authoritative
/// Running status observations re-arm the window. Rendering and visibility
/// never participate in lifecycle recovery.
///
/// The tracker cannot gate this on its own: its in-flight state only clears
/// via `finish_turn`, which never runs when TurnCompleted AND PromptResponse
/// are both lost, so `tracker.activity()` would keep reporting "activity"
/// forever. A stale anchor on a genuinely busy turn costs only one bounded
/// status query per window whose `Running` response refreshes the anchors
/// without re-running the turn-start shim (see the `PromptStatusResolved`
/// `Running` arm), so it is safe by construction.
fn running_turn_stalled(agent: &AgentView, now: std::time::Instant) -> bool {
    let Some(started) = agent.session.turn_started_at else {
        return false;
    };
    if now.saturating_duration_since(started) < PROMPT_STATUS_RUNNING_WATCHDOG_DELAY {
        return false;
    }
    let liveness = [
        agent.session.last_prompt_event_at,
        agent.session.last_status_observed_at,
        Some(started),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(started);
    now.saturating_duration_since(liveness) >= PROMPT_STATUS_RUNNING_WATCHDOG_DELAY
}

/// Earliest lifecycle reconciliation deadline across every local agent.
pub(crate) fn next_prompt_watchdog_deadline(app: &AppView) -> Option<std::time::Instant> {
    app.agents
        .values()
        .filter_map(|agent| {
            let prompt_id = agent.session.current_prompt_id.as_deref()?;
            if agent.session.prompt_status_query_matches(prompt_id) {
                return None;
            }
            if agent.session.state.is_turn_submitting() {
                return agent
                    .session
                    .turn_started_at?
                    .checked_add(PROMPT_STATUS_WATCHDOG_DELAY);
            }
            if !agent.session.state.is_turn_running() {
                return None;
            }
            [
                agent.session.last_prompt_event_at,
                agent.session.last_status_observed_at,
                agent.session.turn_started_at,
            ]
            .into_iter()
            .flatten()
            .max()?
            .checked_add(PROMPT_STATUS_RUNNING_WATCHDOG_DELAY)
        })
        .min()
}

pub(super) fn dispatch_cancel_scheduled_task(app: &mut AppView, task_id: String) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };

    // Remove from local state immediately (optimistic).
    agent.session.scheduled_tasks.remove(&task_id);

    vec![Effect::DeleteScheduledTask {
        session_id,
        task_id,
    }]
}

pub(super) fn dispatch_kill_bg_task(app: &mut AppView, task_id: String) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };

    // Mark as pending_kill for UI feedback
    if let Some(task) = agent.session.bg_tasks.get_mut(&task_id) {
        task.pending_kill = true;
        task.kill_requested_at = Some(Instant::now());
    }

    vec![Effect::KillBgTask {
        session_id,
        task_id,
    }]
}

pub(super) fn dispatch_kill_subagent(app: &mut AppView, subagent_id: String) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };

    // Mark as pending_kill for UI feedback
    for info in agent.session.subagent_sessions.values_mut() {
        if info.subagent_id.as_ref() == subagent_id {
            info.pending_kill = true;
            info.kill_requested_at = Some(Instant::now());
        }
    }

    vec![Effect::KillSubagent {
        session_id,
        subagent_id,
    }]
}

pub(super) fn dispatch_demote_to_background(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    if !agent.session.state.is_turn_running() {
        return vec![];
    }
    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };
    // Get the tool_call_id of the currently running execute tool
    let Some(tool_call_id) = agent
        .session
        .tracker
        .running_execute_tool_call_id()
        .map(|s| s.to_string())
    else {
        return vec![];
    };

    tracing::info!(tool_call_id = %tool_call_id, "Demoting execute tool to background");

    vec![Effect::DemoteToBackground {
        session_id,
        tool_call_id,
    }]
}

// TaskResult handlers.

pub(super) fn handle_bg_task_killed(
    app: &mut AppView,
    session_id: String,
    task_id: String,
    outcome: Option<tools::types::KillOutcome>,
) -> Vec<Effect> {
    use tools::types::KillOutcome;
    if let Some(agent) = find_agent_by_session_id(&mut app.agents, &session_id) {
        match outcome {
            Some(KillOutcome::Killed) => {
                // Stay in pending_kill state — task_completed notification
                // will arrive and clear it.
                tracing::info!(task_id = %task_id, "Kill signal sent");
            }
            Some(KillOutcome::AlreadyExited) => {
                if let Some(task) = agent.session.bg_tasks.get_mut(&task_id) {
                    task.pending_kill = false;
                    task.kill_requested_at = None;
                }
            }
            Some(KillOutcome::NotFound) => {
                // Stale row (e.g. restored from a resume replay but the
                // process belongs to a previous session lifetime): the
                // agent has nothing to kill, so drop the row and finish
                // its "Task started" scrollback entry (stops the
                // running accent that the replay restore turned on).
                tracing::info!(task_id = %task_id, "Task not found, removing");
                if let Some(task) = agent.session.bg_tasks.remove(&task_id)
                    && let Some(entry_id) = task.scrollback_entry_id
                {
                    agent.scrollback.finish_running(entry_id);
                }
            }
            None => {
                // Error envelope or unparseable payload: clear the
                // pending state so the user can retry, keep the row.
                tracing::warn!(task_id = %task_id, "Kill outcome missing or unparseable");
                if let Some(task) = agent.session.bg_tasks.get_mut(&task_id) {
                    task.pending_kill = false;
                    task.kill_requested_at = None;
                }
            }
        }
    }
    vec![]
}

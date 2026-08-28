//! ACP message handling.
//!
//! Routes incoming [`AcpClientMessage`] notifications to the appropriate
//! agent's tracker, queues permission requests for interactive handling,
//! and Grow session extension notifications (`grow/session_notification` and
//! replay-path `grow/session/update`).

use std::collections::hash_map::Entry;
use std::path::PathBuf;
use std::sync::Arc;

use acp_transport::AcpClientMessage;
use agent_client_protocol as acp;

use super::actions::Effect;
use shell::extensions::notification::{SessionNotification, SessionUpdate as GrowSessionUpdate};
use shell::tools::todo::todo_item_from_plan_entry;
use workspace::permission::bash_command_splitting::BashCommandHighlights;

use super::agent_view::{AgentPane, AgentView, InputMode};
use super::root::{ActiveView, AppView};
use crate::acp::meta::NotificationMeta;
use crate::acp::tracker::TurnActivity;
use crate::app::session::{
    AgentId, AgentSession, AgentState, BgTaskState, BgTaskStatus, GoalDisplayState,
    GoalDisplayStatus,
};
use crate::app::subagent::SubagentInfo;
use crate::notifications::{NotificationEvent, NotificationEventKind};
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::SessionEvent;
use crate::views::permission_view::{
    McpScope, McpScopeState, PermissionFocus, PermissionViewState,
};

mod background;
mod follow_ups;
mod interactions;
mod mcp;
mod permissions;
mod prompt_origin;
pub(crate) use prompt_origin::viewer_turn_anchor;
mod queue;
mod routing;
mod session_notification;
mod settings;
mod subagent_activity;

#[cfg(test)]
use permissions::{MCP_ARGS_MAX_LINE_CHARS, MCP_ARGS_MAX_LINES, mcp_args_lines};
use permissions::{apply_recap_block, handle_permission_request, should_drop_late_auto_recap};

// Child modules using `super::*` need these sibling symbols in scope.
use routing::{
    SessionMatch, find_session_match, is_matched_agent_active, is_matched_view_active,
    mcp_target_agent, resolve_notif_agent, resolve_target_agent_view, resolve_target_view,
};

pub(crate) use settings::apply_models_state_update;
pub(crate) use subagent_activity::finalize_killed_subagent;
use subagent_activity::{subagent_activity_label, sync_subagent_activity};

use session_notification::{
    advance_reconnect_cursor, behavior_mode_update_resolution, behavior_mode_update_target,
    confirm_context_used, detect_plan_mode_change, drop_unexpected_replay,
};
pub(crate) use session_notification::{
    apply_deferred_authoritative_controls, handle_descendant_state_replay,
    handle_session_notification, sync_child_control_projection,
};

pub(crate) fn sync_child_control_projection_by_session_id(
    app: &mut AppView,
    child_session_id: &str,
) -> bool {
    fn visit(agent: &mut AgentView, child_session_id: &str) -> bool {
        if agent.subagent_views.contains_key(child_session_id) {
            return sync_child_control_projection(agent, child_session_id);
        }
        agent
            .subagent_views
            .values_mut()
            .any(|child| visit(child, child_session_id))
    }
    app.agents
        .values_mut()
        .any(|agent| visit(agent, child_session_id))
}

pub(crate) fn sync_all_subagent_control_projections(agent: &mut AgentView) {
    let child_ids = agent.subagent_views.keys().cloned().collect::<Vec<_>>();
    for child_id in child_ids {
        if let Some(child) = agent.subagent_views.get_mut(&child_id) {
            sync_all_subagent_control_projections(child);
        }
        sync_child_control_projection(agent, &child_id);
    }
}

use queue::handle_queue_changed;

use background::{
    derive_child_cwd, handle_git_head_changed, handle_monitor_event, handle_scheduled_task_created,
    handle_scheduled_task_deleted, handle_scheduled_task_fired, handle_task_backgrounded,
    handle_task_completed, route_bg_task_stdout,
};
use follow_ups::handle_follow_ups;
pub(crate) use interactions::handle_ask_user_question;
use interactions::handle_plan_approval;
use mcp::{handle_mcp_init_progress, handle_mcp_initialized, handle_mcp_server_status};
use settings::{
    handle_announcements_update, handle_models_update, handle_sessions_changed,
    handle_settings_update,
};

// Test-only bare-name surface for `tests/*` (`use super::*`).
#[cfg(test)]
#[allow(unused_imports)]
use background::*;
#[cfg(test)]
#[allow(unused_imports)]
use follow_ups::*;
#[cfg(test)]
#[allow(unused_imports)]
use interactions::*;
#[cfg(test)]
#[allow(unused_imports)]
use mcp::*;
#[cfg(test)]
#[allow(unused_imports)]
use prompt_origin::*;
#[cfg(test)]
#[allow(unused_imports)]
use queue::*;
#[cfg(test)]
#[allow(unused_imports)]
use routing::*;
#[cfg(test)]
#[allow(unused_imports)]
use session_notification::*;
#[cfg(test)]
#[allow(unused_imports)]
use settings::*;
#[cfg(test)]
#[allow(unused_imports)]
use subagent_activity::*;
/// Handle an ACP notification (session update, permission request, etc.).
///
/// Returns `true` if the active view was visually affected (needs redraw).
/// Notifications are routed to the agent whose `session_id` matches, even when
/// that agent is not the currently active view -- streaming chunks for a
/// background agent must still land in its own scrollback so the user sees
/// the full turn after switching back.
pub(crate) fn handle(msg: AcpClientMessage, app: &mut AppView) -> bool {
    match msg {
        AcpClientMessage::SessionNotification(notif) => {
            let mut meta = NotificationMeta::from_json(notif.request.meta.as_ref());

            let affected = match find_session_match(app, &notif.request.session_id) {
                Some(SessionMatch::Root(id)) => {
                    let is_active = is_matched_agent_active(app, id);
                    let agent = app
                        .agents
                        .get_mut(&id)
                        .expect("find_session_match returned an existing AgentId");
                    let observed_at = std::time::Instant::now();

                    // Live-only dedup: a per-session `eventId` highwater drops
                    // re-delivered live duplicates (leader fan-out, reconnect
                    // re-emit). Replay is EXEMPT — the per-process counter resets
                    // each resume, so persisted history concatenates non-monotonic
                    // 0..N runs; gating it by the highwater would latch a pre-reset
                    // peak and truncate the restored transcript. Replayed
                    // history is authoritative + ordered, so it always renders and
                    // never seeds the highwater.
                    //
                    // Premise: ACP-stream live delivery is in id order —
                    // actor ACP lines (chunks and the plan-mode
                    // `CurrentModeUpdate`s) are stamped at `event_tx` enqueue
                    // time and drained FIFO. The Grow stream is direct-emitted
                    // and keeps a SEPARATE highwater (see the Grow dedup in
                    // `handle_session_notification`). Residual class: ACP
                    // lines that skip `event_tx` — the bridge's bash stdout
                    // (no `event_tx` surface) and the turn-start user echo —
                    // can mint an id after, but deliver before, queued
                    // lower-id lines; with chunk buffering off on pager
                    // sessions that window is one actor drain hop (accepted).
                    let dedup_drop = !meta.is_replay
                        && meta.event_seq.is_some_and(|seq| {
                            agent
                                .session
                                .last_applied_event_seq
                                .is_some_and(|last| seq <= last)
                        });
                    if let Some(seq) = meta.event_seq
                        && !meta.is_replay
                        && !dedup_drop
                    {
                        agent.session.last_applied_event_seq = Some(seq);
                    }

                    if drop_unexpected_replay(
                        agent,
                        &meta,
                        notif.request.session_id.0.as_ref(),
                        "session/update",
                    ) {
                        notif.response_tx.send(Ok(())).ok();
                        return false;
                    }

                    // Re-derive the per-turn viewer flag from prompt-id
                    // ownership BEFORE the adopt/drop gate below.
                    //
                    // `attached_as_viewer` starts true on a `session/load`
                    // attach and is cleared when this client sends its own
                    // prompt — but a client that has driven a turn can later
                    // VIEW a turn ANOTHER client drives (a `/loop` cron, or a
                    // plain prompt typed in a different pane). Left sticky-false,
                    // the gate dropped those deltas and the pane rendered
                    // nothing. A prompt id this client never originated is
                    // another actor-owned turn → view it; one it
                    // originated is its own → drive it (strict gate).
                    //
                    // Only re-derive on a real, non-replay, non-duplicate delta
                    // that does NOT match the active turn.
                    if !dedup_drop
                        && !meta.is_replay
                        && let Some(notif_pid) = meta.prompt_id.as_deref()
                        && agent.session.current_prompt_id.as_deref() != Some(notif_pid)
                    {
                        agent.session.attached_as_viewer =
                            !agent.is_self_originated_prompt(notif_pid);
                    }

                    // Store context usage and turn timing on agent state.
                    //
                    // Gate on `!dedup_drop`: a deduped delta is an
                    // already-applied or stale out-of-order event (its
                    // `eventId` is `<=` the highwater). A fresher event has
                    // already advanced the highwater and set newer `totalTokens`
                    // / `turnStartMs`, so applying the stale values here would
                    // REGRESS them. This is the replay/live-overlap case (leader
                    // fan-out, reconnect, re-emit after the gate): a historical
                    // replay delta carrying a LOWER `totalTokens` arriving after
                    // a live one would otherwise drop the context bar below the
                    // real usage. The dedup already drops the render; the
                    // token/timing state must respect it too.
                    if !dedup_drop {
                        if let Some(tokens) = meta.total_tokens {
                            confirm_context_used(agent, tokens);
                        }
                        if let Some(ts) = meta.turn_start_ms {
                            agent.session.turn_start_ms = Some(ts);
                            agent.session.turn_start_ms_prompt = meta.prompt_id.clone();
                        }
                    }

                    let mut settings_modal_refresh_needed = false;
                    let mut workflows_modal_refresh = false;
                    let mut behavior_drain = None;

                    // Extract Plan updates before passing to tracker (tracker skips them).
                    let mutated = if dedup_drop {
                        tracing::debug!(
                            session_id = notif.request.session_id.0.as_ref(),
                            event_seq = meta.event_seq,
                            last_applied = agent.session.last_applied_event_seq,
                            is_replay = meta.is_replay,
                            "load-race: session/update DROPPED by dedup highwater (event_seq <= last_applied)"
                        );
                        // Already-applied event delivered again — drop it (do not
                        // re-render). Not a mutation, so no redraw.
                        false
                    } else if let acp::SessionUpdate::SessionInfoUpdate(ref update) =
                        notif.request.update
                    {
                        let changed = match &update.title {
                            acp::MaybeUndefined::Value(title) => {
                                let title = crate::util::decode_html_entities(title).into_owned();
                                let is_user = update
                                    .meta
                                    .as_ref()
                                    .and_then(|meta| meta.get("grow/titleSource"))
                                    .and_then(serde_json::Value::as_str)
                                    == Some("user");
                                agent.generated_session_title = Some(title.clone());
                                if is_user {
                                    agent.display_name = Some(title);
                                }
                                true
                            }
                            acp::MaybeUndefined::Null => {
                                agent.generated_session_title = None;
                                agent.display_name = None;
                                true
                            }
                            acp::MaybeUndefined::Undefined => false,
                        };
                        advance_reconnect_cursor(agent, &mut meta);
                        changed
                    } else if let acp::SessionUpdate::Plan(plan) = notif.request.update {
                        // A Plan update may still be useful to the transcript
                        // after its turn stopped being foreground, but only
                        // activity stamped with the active prompt identity may
                        // re-arm the foreground watchdog.
                        if !meta.is_replay
                            && meta.prompt_id.as_ref().is_some_and(|prompt_id| {
                                agent.session.current_prompt_id.as_ref() == Some(prompt_id)
                            })
                        {
                            agent.session.last_prompt_event_at = Some(observed_at);
                        }
                        let items: Vec<_> = plan
                            .entries
                            .into_iter()
                            .map(todo_item_from_plan_entry)
                            .collect();
                        agent.todo.update_todos(items);
                        agent.mark_reload_todo_update();
                        advance_reconnect_cursor(agent, &mut meta);
                        !meta.is_replay && !agent.session.loading_replay
                    } else if let acp::SessionUpdate::ToolCallUpdate(ref tcu) = notif.request.update
                        && route_bg_task_stdout(tcu, &mut agent.session)
                    {
                        // Stdout chunk for a bg task — routed to central store,
                        // not to the scrollback tracker. A background task has
                        // its own lifecycle and must never keep the foreground
                        // prompt's watchdog alive, even when it inherited the
                        // same prompt id before being backgrounded.
                        advance_reconnect_cursor(agent, &mut meta);
                        !meta.is_replay && !agent.session.loading_replay
                    } else if !meta.is_replay
                        && let Some(notif_pid) = meta.prompt_id.as_ref()
                        && agent.session.current_prompt_id.as_ref() != Some(notif_pid)
                        && !agent.session.attached_as_viewer
                    {
                        tracing::debug!(
                            session_id = notif.request.session_id.0.as_ref(),
                            notif_prompt_id = meta.prompt_id.as_deref(),
                            current_prompt_id = agent.session.current_prompt_id.as_deref(),
                            attached_as_viewer = agent.session.attached_as_viewer,
                            loading_replay = agent.session.loading_replay,
                            "load-race: session/update DROPPED by promptId-mismatch gate on a non-viewer (stale/rewound-turn guard)"
                        );
                        // The notification's `promptId` does not match the
                        // currently-active prompt. Drop — belongs to a rewound
                        // turn or stale in-flight work.
                        //
                        // EXCEPTION (multi-client / leader mode): a viewer
                        // (`attached_as_viewer`) is watching a session another
                        // client is driving. It has no turn of its own, so a
                        // mismatching `promptId` is NOT stale — it is the
                        // driver's live (or next) turn. Fall through to the
                        // adoption branch below so the delta renders instead of
                        // freezing the viewer at its load snapshot. This is
                        // scoped to viewers so a locally-created driver's
                        // post-rewind stale-chunk drop is preserved (a driver
                        // always has `attached_as_viewer == false`).
                        !agent.session.loading_replay
                    } else {
                        let behavior_resolution = (!meta.is_replay)
                            .then(|| behavior_mode_update_resolution(&notif.request.update))
                            .flatten();
                        let behavior_target = (!meta.is_replay)
                            .then(|| behavior_mode_update_target(&notif.request.update))
                            .flatten();
                        if !meta.is_replay {
                            agent.session.last_prompt_event_at = Some(observed_at);
                        }
                        // Adopt a mismatching `promptId` so subsequent chunks for
                        // the same turn match and render — but ONLY for a viewer
                        // watching another client's turn.
                        //
                        // Prompt ids are opaque identities. If the shell emits
                        // activity for an actor-owned regular turn, its durable
                        // terminal provides the matching exit path.
                        if let Some(notif_pid) = meta.prompt_id.as_ref()
                            && agent.session.current_prompt_id.as_ref() != Some(notif_pid)
                            && agent.session.attached_as_viewer
                        {
                            agent.session.current_prompt_id = Some(notif_pid.clone());
                            // A viewer adopting another client's new turn: drop
                            // the prior turn's chips but KEEP the seen ring so a
                            // stale prior-turn replay stays rejected. The adopted
                            // turn's own follow_ups (if already applied then
                            // cleared here) still re-render: `apply_follow_ups`
                            // matches their stamped `promptId` to the now-current
                            // `current_prompt_id` set just above.
                            agent.clear_follow_ups();
                            // The adopted turn's follow_ups may have arrived on
                            // the ext channel BEFORE this session/update (separate
                            // channels) and been buffered — render them now that
                            // the turn is current.
                            agent.flush_pending_follow_ups(notif_pid);
                        }
                        // Detect plan mode transitions from tool call completions.
                        settings_modal_refresh_needed |=
                            detect_plan_mode_change(&notif.request.update, agent);
                        settings_modal_refresh_needed |= matches!(
                            &notif.request.update,
                            acp::SessionUpdate::AvailableCommandsUpdate(_)
                        );

                        let had_activity_before = agent.session.tracker.activity().is_some();
                        agent.session.handle_update(
                            notif.request.update,
                            &meta,
                            &mut agent.scrollback,
                        );
                        // Once the server has emitted any activity (chunk, tool,
                        // retry, etc.), the in-flight prompt can no longer be
                        // "rewound" by Ctrl+C. Clear the stash on the transition.
                        if !had_activity_before && agent.session.tracker.activity().is_some() {
                            agent.session.in_flight_prompt = None;

                            // Log initial TTFA once per turn (activity flips None→Some each loop).
                            if let Some(started) = agent.session.turn_started_at
                                && agent.session.first_activity_logged_for != Some(started)
                            {
                                agent.session.first_activity_logged_for = Some(started);
                                let activity_label = agent
                                    .session
                                    .tracker
                                    .activity()
                                    .map(|a| a.as_label())
                                    .unwrap_or("unknown");
                                let ttfa_ms = started.elapsed().as_millis() as u64;
                                let sid = agent.session.session_id.as_ref().map(|s| s.0.as_ref());
                                crate::unified_log::info(
                                    "turn.first_activity",
                                    sid,
                                    Some(serde_json::json!({
                                        "ttfa_ms": ttfa_ms,
                                        "activity": activity_label,
                                    })),
                                );
                            }
                        }

                        // Drain pending ACP commands immediately after handle_update.
                        // This is the SINGLE generation bump site — ensures exactly
                        // one bump per AvailableCommandsUpdate received.
                        if let Some(commands) = agent.session.tracker.take_pending_acp_commands() {
                            let workflows_changed = workflow_commands(&commands)
                                != workflow_commands(&agent.session.available_commands);
                            agent.session.available_commands = commands;
                            agent.session.available_commands_generation += 1;
                            refresh_workflow_run_capabilities(agent);
                            workflows_modal_refresh =
                                workflows_changed && agent.extensions_modal.is_some();
                        }
                        if let Some(definitions) =
                            agent.session.tracker.take_pending_workflow_definitions()
                        {
                            agent.workflows_view.definitions = definitions;
                        }
                        if let Some(diagnostics) =
                            agent.session.tracker.take_pending_workflow_diagnostics()
                        {
                            agent.workflows_view.diagnostics = diagnostics;
                        }
                        // Tools list arrives in the same update's `meta` payload.
                        // Stash it on the session so the per-frame sync in
                        // `app/root/mod.rs` can push it through to the slash registry
                        // alongside the command catalog.
                        if let Some(tools) = agent.session.tracker.take_pending_acp_tools() {
                            agent.session.available_tools = Some(tools.into_iter().collect());
                        }
                        for entry_id in agent.session.tracker.take_pending_edit_hl() {
                            agent.submit_edit_highlight(entry_id);
                        }

                        // Viewer chrome (leader / multi-client). A viewer has no
                        // turn of its own and never calls start_turn(), so it
                        // would stay `Idle` — hiding the "⠿ Responding…" status
                        // line, the elapsed/token counter, and the Ctrl+c:cancel
                        // / Ctrl+Enter:interject footer hints (all gated on
                        // `AgentState::TurnRunning`). Enter TurnRunning whenever a
                        // turn is in flight (a prompt id is adopted) and we are
                        // not already running.
                        //
                        // This is placed AFTER `handle_update` (not in the adopt
                        // block above) on purpose: the adopt block only fires on
                        // a prompt-id MISMATCH and is suppressed during the
                        // `loading_replay` window. A client that reattaches
                        // MID-turn adopts the running id during its replay window
                        // (TurnRunning suppressed there) and then receives
                        // post-load deltas that MATCH `current_prompt_id` — which
                        // skip the adopt block — so it would never flip to
                        // TurnRunning. Checking here on every applied live viewer
                        // delta closes that gap, independent of whether the load
                        // response carried a structured foreground snapshot, of
                        // delta ordering, and of whether a given delta carries a
                        // prompt id.
                        //
                        // Do NOT call start_turn(): it resets the tracker. We
                        // only flip state + stamp the elapsed timer.
                        //
                        // Enter TurnRunning only for an adoptable prompt — see
                        // `should_adopt_running_prompt` (true iff the turn has a
                        // durable terminal exit). This lets a viewer (and the
                        // dashboard's locally-tracked row) show every regular
                        // foreground turn as Working without inventing a second
                        // lifecycle for internal origins.
                        if agent.session.attached_as_viewer
                            && !meta.is_replay
                            && !agent.session.loading_replay
                            && agent
                                .session
                                .current_prompt_id
                                .as_deref()
                                .is_some_and(|pid| {
                                    !agent.session.replayed_terminal_prompts.contains(pid)
                                        && !agent.is_rewound_prompt(pid)
                                })
                            && !matches!(agent.session.state, AgentState::TurnRunning)
                        {
                            agent.session.state = AgentState::TurnRunning;
                            agent.session.last_status_observed_at = Some(observed_at);
                            // Back-date from the authoritative `turnStartMs` so a
                            // viewer's elapsed matches the driver's instead of
                            // starting at the time-to-first-delta.
                            agent.session.turn_started_at =
                                Some(viewer_turn_anchor(agent.session.turn_start_ms));
                        }

                        advance_reconnect_cursor(agent, &mut meta);

                        if let Some(resolution) = behavior_resolution
                            && agent
                                .session
                                .resolve_in_flight_behavior(
                                    agent.session.behavior_mode,
                                    resolution,
                                    behavior_target,
                                )
                                .is_some()
                        {
                            crate::app::acp_handler::apply_deferred_authoritative_controls(
                                agent,
                                notif.request.session_id.0.as_ref(),
                            );
                        }
                        if matches!(
                            behavior_resolution,
                            Some(crate::app::session::BehaviorControlResolution::Applied)
                        ) && agent
                            .session
                            .deferred_session_mode
                            .is_some_and(|target| target == agent.session.behavior_mode)
                        {
                            // The admission latch can predate the live control
                            // FIFO (for example, a session created from the
                            // Dashboard). Any matching authoritative applied
                            // update releases it; an unrelated initial update,
                            // rejection, or confirmation never does.
                            agent.session.deferred_session_mode = None;
                            behavior_drain =
                                Some(crate::app::root::dispatch::maybe_drain_queue(agent));
                        }

                        !meta.is_replay && !agent.session.loading_replay
                    };

                    if let Some(drain) = behavior_drain {
                        crate::app::root::dispatch::note_peek_page_flip(
                            app,
                            id,
                            drain.page_flip_entry,
                        );
                        app.pending_effects.extend(drain.effects);
                    }
                    if settings_modal_refresh_needed {
                        crate::app::root::dispatch::refresh_open_settings_modals(app);
                    }
                    if workflows_modal_refresh {
                        queue_open_workflows_modal_refresh(app, id);
                    }

                    // Mutation always happens; redraw only when the matched
                    // agent is the visible one.
                    mutated && is_active
                }
                Some(SessionMatch::Child(parent_id)) => {
                    let is_active = is_matched_agent_active(app, parent_id);
                    let mut child_behavior_drain = None;
                    let parent = app
                        .agents
                        .get_mut(&parent_id)
                        .expect("find_session_match returned an existing AgentId");
                    // Re-derive the &str key to avoid making SessionMatch::Child
                    // carry an owned String (see find_session_match docs).
                    let child_key: &str = notif.request.session_id.0.as_ref();

                    let activity_label = {
                        let child_view = parent
                            .subagent_views
                            .get_mut(child_key)
                            .expect("find_session_match returned an existing subagent_views key");
                        if let Some(tokens) = meta.total_tokens {
                            confirm_context_used(child_view, tokens);
                        }
                        if let Some(ts) = meta.turn_start_ms {
                            child_view.session.turn_start_ms = Some(ts);
                        }
                        let behavior_resolution = (!meta.is_replay)
                            .then(|| behavior_mode_update_resolution(&notif.request.update))
                            .flatten();
                        let behavior_target = (!meta.is_replay)
                            .then(|| behavior_mode_update_target(&notif.request.update))
                            .flatten();
                        let _ = detect_plan_mode_change(&notif.request.update, child_view);
                        child_view.session.handle_update(
                            notif.request.update,
                            &meta,
                            &mut child_view.scrollback,
                        );
                        for entry_id in child_view.session.tracker.take_pending_edit_hl() {
                            child_view.submit_edit_highlight(entry_id);
                        }
                        if let Some(resolution) = behavior_resolution
                            && child_view
                                .session
                                .resolve_in_flight_behavior(
                                    child_view.session.behavior_mode,
                                    resolution,
                                    behavior_target,
                                )
                                .is_some()
                        {
                            crate::app::acp_handler::apply_deferred_authoritative_controls(
                                child_view,
                                notif.request.session_id.0.as_ref(),
                            );
                        }
                        if matches!(
                            behavior_resolution,
                            Some(crate::app::session::BehaviorControlResolution::Applied)
                        ) && child_view
                            .session
                            .deferred_session_mode
                            .is_some_and(|target| target == child_view.session.behavior_mode)
                        {
                            child_view.session.deferred_session_mode = None;
                            child_behavior_drain =
                                Some(crate::app::root::dispatch::maybe_drain_queue(child_view));
                        }
                        subagent_activity_label(child_view)
                    };

                    sync_subagent_activity(parent, child_key, activity_label);

                    if let Some(drain) = child_behavior_drain {
                        crate::app::root::dispatch::note_peek_page_flip(
                            app,
                            parent_id,
                            drain.page_flip_entry,
                        );
                        app.pending_effects.extend(drain.effects);
                    }
                    is_active
                }
                None => {
                    tracing::debug!(
                        session_id = notif.request.session_id.0.as_ref(),
                        agent_count = app.agents.len(),
                        "load-race: session/update DROPPED — no agent matches session_id (view not loaded yet?)"
                    );
                    false
                }
            };
            notif.response_tx.send(Ok(())).ok();
            affected
        }
        AcpClientMessage::RequestPermission(perm) => handle_permission_request(perm, app),
        AcpClientMessage::ExtNotification(ext) => {
            let affected = handle_ext_notification(&ext.request, app);
            ext.response_tx.send(Ok(())).ok();
            affected
        }
        AcpClientMessage::ExtMethod(ext) => handle_ext_method(ext, app),
        AcpClientMessage::WaitForTerminalExit(args) => {
            args.response_tx
                .send(Err(crate::acp::wait_for_exit_not_supported("pager")))
                .ok();
            false
        }
        _ => false,
    }
}

fn workflow_commands(
    commands: &[acp::AvailableCommand],
) -> Vec<(&str, &str, Option<&str>, Option<&str>)> {
    commands
        .iter()
        .filter_map(|command| {
            let meta = command.meta.as_ref()?;
            let source = meta.get("workflowSource")?.as_str();
            Some((
                command.name.as_str(),
                command.description.as_str(),
                source,
                meta.get("workflowPath").and_then(serde_json::Value::as_str),
            ))
        })
        .collect()
}

pub(crate) fn refresh_workflow_run_capabilities(agent: &mut AgentView) {
    let management_available = agent
        .session
        .available_commands
        .iter()
        .any(|command| command.name == "workflow-run");
    for run in &mut agent.session.workflow_runs {
        run.management_available = management_available;
    }
}

fn queue_open_workflows_modal_refresh(app: &mut AppView, agent_id: AgentId) {
    let Some(session_id) = app
        .agents
        .get(&agent_id)
        .and_then(|agent| agent.session.session_id.clone())
    else {
        return;
    };
    let already_pending = app.pending_effects.iter().any(|effect| {
        matches!(
            effect,
            Effect::FetchWorkflowsList {
                agent_id: pending_id,
                ..
            } if *pending_id == agent_id
        )
    });
    if !already_pending {
        app.pending_effects.push(Effect::FetchWorkflowsList {
            agent_id,
            session_id,
        });
    }
}

/// Handle an Grow extension notification.
fn handle_ext_notification(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    match notif.method.as_ref() {
        "grow/session_notification" | "grow/session/update" => {
            handle_session_notification(notif, app)
        }
        "grow/follow_ups" => handle_follow_ups(notif, app),
        "grow/task_backgrounded" => handle_task_backgrounded(notif, app),
        "grow/task_completed" => handle_task_completed(notif, app),
        "grow/models/update" => handle_models_update(notif, app),
        "grow/settings/update" => handle_settings_update(notif, app),
        "grow/sessions/changed" => handle_sessions_changed(notif, app),
        "grow/queue/changed" => handle_queue_changed(notif, app),
        "grow/session/interjection" => handle_interjection(notif, app),
        "grow/monitor_event" => handle_monitor_event(notif, app),
        "grow/scheduled_task_created" => handle_scheduled_task_created(notif, app),
        "grow/scheduled_task_fired" => handle_scheduled_task_fired(notif, app),
        "grow/scheduled_task_deleted" => handle_scheduled_task_deleted(notif, app),
        "grow/announcements/update" => handle_announcements_update(notif, app),
        "grow/git_head_changed" => handle_git_head_changed(notif, app),
        "grow/mcp/init_progress" => handle_mcp_init_progress(notif, app),
        "grow/mcp_initialized" => handle_mcp_initialized(notif, app),
        "grow/mcp/server_status" => handle_mcp_server_status(notif, app),
        _ => false,
    }
}

/// Handle `grow/session/interjection` — the leader broadcasts this
/// sessionId-bearing notification to every attached client when a mid-turn
/// interjection is queued (emitted from the session actor's `Interject`
/// command handler). Each client renders the interjection as a scrollback
/// block.
///
/// The originating pager renders an optimistic block immediately in
/// `dispatch_interject` and records the interjection id in
/// `self_interjection_ids`; when its own broadcast echoes back here it is
/// deduped (dropped) by that id. Other panes (which never minted the id) render
/// the block — fixing the multi-client bug where an interjection typed in one
/// pane was invisible in the others. A `null`/absent id (older shell) always
/// renders, so legacy shells degrade to "render everywhere" rather than drop.
fn handle_interjection(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(notif.params.get()) else {
        tracing::warn!("Failed to parse grow/session/interjection");
        return false;
    };
    let Some(session_id) = parsed.get("sessionId").and_then(|v| v.as_str()) else {
        return false;
    };
    let Some(text) = parsed.get("text").and_then(|v| v.as_str()) else {
        return false;
    };
    let interjection_id = parsed.get("interjectionId").and_then(|v| v.as_str());

    let sid = acp::SessionId::new(session_id.to_string());
    let Some(SessionMatch::Root(id)) = find_session_match(app, &sid) else {
        return false;
    };
    let is_active = is_matched_agent_active(app, id);
    let Some(agent) = app.agents.get_mut(&id) else {
        return false;
    };

    // Dedup our own optimistic echo: if we minted this id we already rendered
    // the block locally — drop the broadcast copy (and forget the id).
    if let Some(iid) = interjection_id
        && agent.session.consume_self_interjection(iid)
    {
        return false;
    }

    agent
        .scrollback
        .push_block(RenderBlock::interjection_prompt(text));
    is_active
}

/// Handle an ACP `ext_method` request (blocking request that expects a response).
///
/// Dispatches on method string. Unknown methods get `method_not_found` error.
/// The response sender is stashed (for `ask_user_question`) or replied to
/// immediately (for unknown methods).
fn handle_ext_method(ext: acp_transport::AcpArgs<acp::ExtRequest>, app: &mut AppView) -> bool {
    match ext.request.method.as_ref() {
        "grow/ask_user_question" => handle_ask_user_question(ext, app),
        "grow/plan_approval" => handle_plan_approval(ext, app),
        unknown => {
            tracing::warn!("Unknown ext_method: {unknown}");
            ext.response_tx
                .send(Err(acp::Error::new(
                    -32601,
                    format!("Method not found: {unknown}"),
                )))
                .ok();
            false
        }
    }
}

#[cfg(test)]
mod tests;

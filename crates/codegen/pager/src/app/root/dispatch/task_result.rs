//! Async task-result application: routes task results into state.
use super::cta::{
    handle_cta_plugin_install_done, handle_cta_plugin_reload_done,
    handle_plugin_cta_catalog_loaded, handle_plugin_cta_debounce_expired,
    handle_plugin_cta_mcps_loaded,
};
use super::ctx::{find_agent_by_session_id, find_agent_view_by_session_id, get_active_agent_mut};
use super::dashboard::handle_dashboard_location_candidates_loaded;
use super::notes::{handle_btw_response, handle_memory_note_saved};
use super::prompt::{
    handle_compact_complete, handle_prompt_response, handle_suggestion_debounce_expired,
};
use super::queue::maybe_drain_queue;
use super::rewind::{
    dispatch_rewind_success, handle_rewind_execute_failed, handle_rewind_points_loaded,
    handle_rewind_preview_complete, handle_rewind_preview_failed,
};
use super::router::{dispatch, dispatch_action_result};
use super::session::fork::{
    handle_fork_session_failed, handle_fork_session_ready, handle_project_picker_recents_loaded,
    handle_worktree_forked,
};
use super::session::lifecycle::{
    dispatch_exit_session, handle_session_created, handle_session_failed,
    handle_switch_model_complete, handle_worktree_session_created, handle_worktree_session_failed,
};
use super::session::list::{handle_session_list_failed, handle_session_list_loaded};
use super::session::load::{
    handle_card_detail_loaded, handle_deep_search_results, handle_session_load_failed,
    handle_session_loaded, handle_session_search_debounce_expired, remove_session_from_pickers,
};
use super::session::modal::remove_agent_and_cleanup;
use super::settings::ui::apply_setting_rollback;
use super::status::{
    apply_session_usage_result, handle_context_info_complete, handle_context_info_failed,
    handle_session_info_complete, handle_session_info_failed, scrub_error_for_toast,
};
use super::transcript::{
    handle_hooks_list_loaded, handle_marketplace_list_loaded, handle_marketplace_updates_available,
    handle_mcp_toggle_done, handle_plugins_list_loaded, handle_skills_toggle_done,
};
use super::turn::handle_bg_task_killed;
use crate::app::actions::{
    ClipboardPasteCompletion, ClipboardPasteContext, ClipboardPasteFailure, ClipboardPasteTarget,
    DoctorFixTarget, DoctorPlanningOutcome, Effect, ProbedAttachment, PromptStatusWire,
    SubagentKillOutcome, TaskResult,
};
use crate::app::root::{ActiveView, AppView};
use crate::app::session::AgentId;
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::{NoticeCategory, NoticeTone};
use agent_client_protocol as acp;
pub(super) fn unregister_session_effect(session_id: Option<acp::SessionId>) -> Vec<Effect> {
    session_id
        .map(|sid| Effect::UnregisterActiveSession { session_id: sid })
        .into_iter()
        .collect()
}
pub(super) fn unregister_all_active_sessions(app: &AppView) -> Vec<Effect> {
    app.agents
        .values()
        .filter_map(|a| {
            a.session
                .session_id
                .as_ref()
                .map(|sid| Effect::UnregisterActiveSession {
                    session_id: sid.clone(),
                })
        })
        .collect()
}
fn handle_mcp_setup_submit_done(
    app: &mut AppView,
    agent_id: AgentId,
    server_name: String,
    result: Result<(), String>,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if let Some(modal) = agent.extensions_modal.as_mut() {
        modal.pending_action = None;
        modal.pending_entry_index = None;
        if let Err(error) = result {
            modal.modal_message = Some(crate::views::extensions_modal::ModalMessage::Error(
                format!("{server_name} setup failed: {error}"),
            ));
            return vec![];
        }
        modal.mcp_setup = None;
    }
    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };
    vec![Effect::FetchMcpsList {
        agent_id,
        session_id,
    }]
}
pub(super) const X11_PRIMARY_PASTE_HINT: &str = "Try Shift+Insert to paste selected text";
fn show_clipboard_toast(target: &ClipboardPasteTarget, message: &str, app: &mut AppView) {
    match target {
        ClipboardPasteTarget::AgentPrompt { agent_id, .. } => {
            if let Some(agent) = app.agents.get_mut(agent_id) {
                agent.show_toast(message);
            }
        }
        ClipboardPasteTarget::DashboardDispatch | ClipboardPasteTarget::DashboardPeek { .. } => {
            if let Some(dashboard) = app.dashboard.as_mut() {
                dashboard.set_info(message);
            }
        }
    }
}
pub(super) fn maybe_show_x11_primary_paste_hint(
    eligible: bool,
    completion: ClipboardPasteCompletion,
    target: &ClipboardPasteTarget,
    app: &mut AppView,
) {
    if !eligible || completion != ClipboardPasteCompletion::FullMiss {
        return;
    }
    show_clipboard_toast(target, X11_PRIMARY_PASTE_HINT, app);
}
/// Whether a completed clipboard probe should fall through to the `grow wrap`
/// host-image request. A clean `FullMiss` always qualifies; a remote read
/// *error* (`AttachmentRead`) also qualifies because inside `grow wrap` the
/// authoritative pasteboard is the local host's, not the (absent) remote one, so
/// the error is recoverable over the wrap OSC path. Every other failure
/// (`TextRead`, `TargetInsertion`, `AlreadyReported`) is a real dead end and
/// must keep toasting. The request itself still self-gates on
/// `osc52_sink_active()`, so this is inert outside `grow wrap`.
pub(super) fn wrap_host_image_request_eligible(completion: ClipboardPasteCompletion) -> bool {
    matches!(
        completion,
        ClipboardPasteCompletion::FullMiss
            | ClipboardPasteCompletion::Failed(ClipboardPasteFailure::AttachmentRead)
    )
}
pub(super) fn show_clipboard_failure(
    target: &ClipboardPasteTarget,
    failure: ClipboardPasteFailure,
    app: &mut AppView,
) {
    let message = match failure {
        ClipboardPasteFailure::AlreadyReported => return,
        ClipboardPasteFailure::TextRead => "Couldn't read clipboard text",
        ClipboardPasteFailure::AttachmentRead => "Couldn't read clipboard contents",
        ClipboardPasteFailure::TargetInsertion => "Couldn't paste clipboard contents",
    };
    show_clipboard_toast(target, message, app);
}
fn apply_clipboard_paste_result(
    ctx: ClipboardPasteContext,
    image: ProbedAttachment,
    file_urls: Option<String>,
    app: &mut AppView,
) -> (ClipboardPasteCompletion, Vec<Effect>) {
    let mut effects = Vec::new();
    let completion = match ctx.target.clone() {
        ClipboardPasteTarget::AgentPrompt { agent_id, .. } => app
            .agents
            .get_mut(&agent_id)
            .map_or(ClipboardPasteCompletion::Dropped, |agent| {
                agent.complete_clipboard_attachment_paste(ctx, image, file_urls, &mut effects)
            }),
        ClipboardPasteTarget::DashboardDispatch | ClipboardPasteTarget::DashboardPeek { .. } => app
            .dashboard
            .as_mut()
            .map_or(ClipboardPasteCompletion::Dropped, |dashboard| {
                dashboard.complete_clipboard_attachment_paste(ctx, image, file_urls, &mut effects)
            }),
    };
    (completion, effects)
}
fn drain_clipboard_target(
    target: &ClipboardPasteTarget,
    app: &mut AppView,
    mut effects: Vec<Effect>,
) -> Vec<Effect> {
    match target {
        ClipboardPasteTarget::AgentPrompt { agent_id, .. } => {
            let is_active = app.active_view == ActiveView::Agent(*agent_id);
            let Some(agent) = app.agents.get_mut(agent_id) else {
                return effects;
            };
            let resend = agent.take_deferred_send_after_paste();
            let action = if is_active {
                resend.and_then(|kind| agent.build_deferred_send_action(kind))
            } else {
                None
            };
            if let Some(action) = action {
                effects.extend(dispatch(action, app));
            }
            effects
        }
        ClipboardPasteTarget::DashboardDispatch | ClipboardPasteTarget::DashboardPeek { .. } => {
            let Some(dashboard) = app.dashboard.as_mut() else {
                return effects;
            };
            let resends = dashboard.take_deferred_sends_after_paste();
            if matches!(app.active_view, ActiveView::AgentDashboard) {
                for action in resends {
                    effects.extend(dispatch(action, app));
                }
            }
            effects
        }
    }
}
pub(crate) fn current_doctor_target(
    app: &AppView,
    target: &DoctorFixTarget,
) -> Option<DoctorFixTarget> {
    let agent = app.agents.get(&target.agent_id)?;
    if agent.session.cwd != target.cwd {
        return None;
    }
    match (&target.session_id, &agent.session.session_id) {
        (Some(expected), Some(current))
            if expected == current
                && target.session_binding_epoch == agent.session_binding_epoch =>
        {
            Some(target.clone())
        }
        (None, Some(current))
            if agent.session_binding_epoch == target.session_binding_epoch.wrapping_add(1) =>
        {
            Some(DoctorFixTarget {
                session_id: Some(current.clone()),
                session_binding_epoch: agent.session_binding_epoch,
                ..target.clone()
            })
        }
        (None, None) if target.session_binding_epoch == agent.session_binding_epoch => {
            Some(target.clone())
        }
        _ => None,
    }
}
pub(crate) fn deliver_doctor_message(
    app: &mut AppView,
    preferred: AgentId,
    tone: NoticeTone,
    message: String,
) {
    let destination = app
        .agents
        .contains_key(&preferred)
        .then_some(preferred)
        .or_else(|| match app.active_view {
            ActiveView::Agent(id) if app.agents.contains_key(&id) => Some(id),
            _ => app.agents.keys().next().copied(),
        });
    if let Some(destination) = destination
        && let Some(agent) = app.agents.get_mut(&destination)
    {
        agent.scrollback.push_block(RenderBlock::typed_notice(
            tone,
            NoticeCategory::Command,
            message,
            None,
        ));
        return;
    }
    app.startup_warnings.push(crate::startup::StartupWarning {
        severity: if matches!(tone, NoticeTone::Warning | NoticeTone::Error) {
            crate::startup::WarningSeverity::Warning
        } else {
            crate::startup::WarningSeverity::Info
        },
        message,
        action: None,
    });
}
/// Handle a completed async task result.
pub(super) fn dispatch_task_result(result: TaskResult, app: &mut AppView) -> Vec<Effect> {
    match result {
        TaskResult::SessionCreated {
            agent_id,
            session_id,
            models: new_models,
        } => handle_session_created(app, agent_id, session_id, new_models),
        TaskResult::SessionFailed { agent_id, error } => {
            handle_session_failed(app, agent_id, error)
        }
        TaskResult::WorktreeSessionCreated {
            agent_id,
            session_id,
            worktree_path,
            session_cwd,
            models: new_models,
        } => handle_worktree_session_created(
            app,
            agent_id,
            session_id,
            worktree_path,
            session_cwd,
            new_models,
        ),
        TaskResult::WorktreeForked {
            agent_id,
            session_id,
            worktree_path,
            session_cwd,
            code_restored,
            restore_summary,
            restore_degree,
        } => handle_worktree_forked(
            app,
            agent_id,
            session_id,
            worktree_path,
            session_cwd,
            code_restored,
            restore_summary,
            restore_degree,
        ),
        TaskResult::WorktreeSessionFailed { agent_id, error } => {
            handle_worktree_session_failed(app, agent_id, error)
        }
        TaskResult::ForkSessionReady {
            agent_id,
            new_session_id,
            cwd,
        } => handle_fork_session_ready(app, agent_id, new_session_id, cwd),
        TaskResult::ForkSessionFailed { agent_id, error } => {
            handle_fork_session_failed(app, agent_id, error)
        }
        TaskResult::SessionLoaded {
            agent_id,
            session_id,
            models: new_models,
            code_restored,
            restore_summary,
            restore_degree,
            foreground,
        } => handle_session_loaded(
            app,
            agent_id,
            session_id,
            new_models,
            code_restored,
            restore_summary,
            restore_degree,
            foreground,
        ),
        TaskResult::SessionTitleFromDisk { agent_id, title } => {
            if let Some(agent) = app.agents.get_mut(&agent_id)
                && let Some((t, is_manual)) = title.filter(|(s, _)| !s.trim().is_empty())
            {
                if is_manual && agent.display_name.is_none() {
                    agent.display_name = Some(t.clone());
                }
                agent.generated_session_title = Some(t);
            }
            vec![]
        }
        TaskResult::SessionLoadFailed {
            agent_id,
            session_id,
            error,
        } => handle_session_load_failed(app, agent_id, session_id, error),
        TaskResult::SessionListLoaded {
            sessions,
            scope,
            seq,
            query,
        } => handle_session_list_loaded(app, sessions, scope, seq, query),
        TaskResult::SessionListFailed { error, seq, query } => {
            handle_session_list_failed(app, error, seq, query)
        }
        TaskResult::ProjectPickerRecentsLoaded {
            agent_id,
            picker_id,
            dirs,
        } => handle_project_picker_recents_loaded(app, agent_id, picker_id, dirs),
        TaskResult::DashboardLocationCandidatesLoaded {
            base_cwd,
            picker_id,
            dirs,
            worktrees,
        } => handle_dashboard_location_candidates_loaded(app, base_cwd, picker_id, dirs, worktrees),
        TaskResult::SessionSearchDebounceExpired { query, seq } => {
            handle_session_search_debounce_expired(app, query, seq)
        }
        TaskResult::RosterLoaded { sessions } => {
            app.leader_roster = sessions;
            app.dashboard_sessions_loading = false;
            vec![]
        }
        TaskResult::RosterFailed { error } => {
            tracing::debug!(error = %error, "leader roster fetch failed");
            app.dashboard_sessions_loading = false;
            vec![]
        }
        TaskResult::DashboardSessionsLoaded { sessions } => {
            app.dashboard_local_sessions = sessions;
            app.dashboard_sessions_loading = false;
            vec![]
        }
        TaskResult::CardDetailLoaded {
            session_id,
            generation,
            detail,
        } => handle_card_detail_loaded(app, session_id, generation, detail),
        TaskResult::PromptResponse {
            agent_id,
            result,
            http_status,
            prompt_id,
        } => handle_prompt_response(app, agent_id, result, http_status, prompt_id),
        TaskResult::PromptStatusResolved {
            agent_id,
            prompt_id,
            status,
        } => {
            let is_active = app.active_view == ActiveView::Agent(agent_id);
            let Some(agent) = app.agents.get_mut(&agent_id) else {
                return vec![];
            };
            if agent.session.current_prompt_id.as_deref() != Some(prompt_id.as_str()) {
                return vec![];
            }
            agent.session.clear_prompt_status_query();
            match status {
                Ok(PromptStatusWire::Running { turn_start_ms }) => {
                    let observed_at = std::time::Instant::now();
                    // The turn-start shim adopts a server-confirmed running
                    // turn and belongs to the still-Submitting window only
                    // (queue/changed's own shim is gated the same way). When
                    // the turn is already Running the shim already ran — via
                    // queue/changed or a first Running response — and running
                    // it again would start a second turn boundary, dropping
                    // the current turn's follow-up chips (W4). The anchors
                    // below are shared by both arms.
                    let page_flip = if agent.session.state.is_turn_submitting() {
                        super::queue::apply_turn_start_shim(
                            agent,
                            prompt_id.clone(),
                            None,
                            "prompt",
                            None,
                        )
                    } else {
                        None
                    };
                    let turn_start_ms = i64::try_from(turn_start_ms).unwrap_or(i64::MAX);
                    agent.session.turn_start_ms = Some(turn_start_ms);
                    agent.session.turn_start_ms_prompt = Some(prompt_id);
                    if agent.session.turn_started_at.is_none() {
                        agent.session.turn_started_at = Some(
                            crate::app::acp_handler::viewer_turn_anchor(Some(turn_start_ms)),
                        );
                    }
                    agent.session.last_status_observed_at = Some(observed_at);
                    super::queue::note_peek_page_flip(app, agent_id, page_flip);
                    vec![]
                }
                Ok(PromptStatusWire::Queued { .. }) => {
                    // The server admitted the prompt but has NOT promoted it
                    // — another turn is running and the promoting broadcast
                    // has not come yet (a busy foreground turn, e.g. a Goal
                    // round, or any turn the pager has not yet adopted). A
                    // LOCALLY-DRAINED prompt must not stay on "Sending…" for
                    // the turn's whole duration: resolve the submitting state
                    // and let the queue/changed re-merged row represent the
                    // message; the future promoting broadcast starts the turn
                    // via the turn-start shim (which reuses the painted user
                    // block by text). The pre-existing behaviour re-armed the
                    // 2s watchdog forever, which is what made busy foreground
                    // turns look hung.
                    if agent.session.state.is_turn_submitting() {
                        agent.session.state = crate::app::session::AgentState::Idle;
                        agent.session.current_prompt_id = None;
                        agent.session.in_flight_prompt = None;
                        agent.mark_turn_finished();
                    } else {
                        // A non-terminal observation must never rewrite the
                        // display anchor or fabricate completion. Re-arm the
                        // reducer-owned liveness window only.
                        agent.session.last_status_observed_at = Some(std::time::Instant::now());
                    }
                    vec![]
                }
                Ok(PromptStatusWire::Terminal {
                    stop_reason,
                    agent_result,
                }) => {
                    // The turn ran while lifecycle signals were lost. Establish
                    // the exact prompt boundary unless the turn already owns
                    // the foreground: an already-Running/Cancelling turn's pid
                    // matches the first-wins finalizer directly, so the
                    // boundary reset (tracker re-seed, follow-up chips) must
                    // not run a second time. Submitting and idle-with-stale-pid
                    // recoveries still need the boundary to seed the finalize.
                    if !agent.session.state.is_terminal_turn() {
                        agent.start_turn_boundary(Some(&prompt_id));
                    }
                    agent.session.current_prompt_id = Some(prompt_id.clone());
                    let outcome = agent.finalize_turn_from_durable_terminal(
                        &prompt_id,
                        Some(&stop_reason),
                        agent_result.as_deref(),
                    );
                    app.apply_terminal_outcome(outcome, agent_id, is_active);
                    vec![]
                }
                Ok(PromptStatusWire::Unknown) => {
                    if agent.session.state.is_turn_running() {
                        agent.session.last_status_observed_at = Some(std::time::Instant::now());
                        return vec![];
                    }
                    agent.session.state = crate::app::session::AgentState::Idle;
                    agent.session.current_prompt_id = None;
                    agent.session.in_flight_prompt = None;
                    agent.mark_turn_finished();
                    agent.show_toast("Prompt was not admitted by the session.");
                    let drain = super::queue::maybe_drain_queue(agent);
                    super::queue::note_peek_page_flip(app, agent_id, drain.page_flip_entry);
                    drain.effects
                }
                Err(error) => {
                    if agent.session.state.is_turn_running() {
                        agent.session.last_status_observed_at = Some(std::time::Instant::now());
                        tracing::warn!(%error, %prompt_id, "prompt watchdog status query failed");
                        return vec![];
                    }
                    agent.session.state = crate::app::session::AgentState::Idle;
                    agent.session.current_prompt_id = None;
                    agent.session.in_flight_prompt = None;
                    agent.mark_turn_finished();
                    agent.show_toast(&error);
                    let drain = super::queue::maybe_drain_queue(agent);
                    super::queue::note_peek_page_flip(app, agent_id, drain.page_flip_entry);
                    drain.effects
                }
            }
        }
        TaskResult::PreferredModelPersisted { result } => {
            if let Err(err) = result
                && let Some(agent) = get_active_agent_mut(app)
            {
                agent.scrollback.push_block(RenderBlock::notice(format!(
                    "Couldn't save preferred model: {err} (still active for this session)"
                )));
            }
            vec![]
        }
        TaskResult::CancelComplete => {
            tracing::trace!("Cancel notification sent successfully");
            vec![]
        }
        TaskResult::KillSubagentComplete {
            session_id,
            subagent_id,
            outcome,
        } => {
            if let SubagentKillOutcome::NothingLive { status } = outcome {
                let status = status.as_deref().unwrap_or("cancelled");
                crate::app::acp_handler::finalize_killed_subagent(
                    app,
                    &session_id,
                    &subagent_id,
                    status,
                );
            }
            vec![]
        }
        TaskResult::CompactComplete {
            agent_id,
            track_foreground,
            result,
        } => handle_compact_complete(app, agent_id, track_foreground, result),
        TaskResult::SwitchModelComplete {
            agent_id,
            session_id,
            control_token,
            model_id,
            effort,
            result,
        } => handle_switch_model_complete(
            app,
            agent_id,
            session_id,
            control_token,
            model_id,
            effort,
            result,
        ),
        TaskResult::SwitchAgentComplete {
            agent_id,
            session_id,
            control_token,
            agent_name,
            result,
        } => {
            if result == Ok(crate::app::actions::ControlRpcOutcome::AuthoritativeUpdatePending) {
                return vec![];
            }
            let mut page_flip_entry = None;
            let mut effects = vec![];
            if let Some(agent) =
                find_agent_view_by_session_id(&mut app.agents, session_id.0.as_ref())
            {
                let completion = agent.session.complete_control(control_token);
                if completion == crate::app::session::SessionControlCompletion::Stale {
                    return vec![];
                }
                match result {
                    // AgentChanged + durable ControlStateUpdate own the
                    // committed projection and terminal Notice.
                    Ok(crate::app::actions::ControlRpcOutcome::Superseded) => {}
                    Ok(crate::app::actions::ControlRpcOutcome::AuthoritativeUpdatePending) => {
                        unreachable!("handled above")
                    }
                    Err(error) if !error.terminal_published => {
                        agent.scrollback.push_block(RenderBlock::terminal_notice(
                            format!(
                                "control:agent:{}:{}:{}",
                                session_id.0, control_token.generation, control_token.sequence
                            ),
                            NoticeTone::Error,
                            NoticeCategory::Control,
                            format!("Agent switch to {agent_name} failed"),
                            Some(format!(
                                "Reason: {}. Retry /agent {agent_name} or choose another Agent.",
                                error.message
                            )),
                        ));
                    }
                    Err(_) => {}
                }
                crate::app::acp_handler::apply_deferred_authoritative_controls(
                    agent,
                    session_id.0.as_ref(),
                );
                let drain = maybe_drain_queue(agent);
                page_flip_entry = drain.page_flip_entry;
                effects.extend(drain.effects);
            }
            crate::app::acp_handler::sync_child_control_projection_by_session_id(
                app,
                session_id.0.as_ref(),
            );
            super::queue::note_peek_page_flip(app, agent_id, page_flip_entry);
            effects
        }
        TaskResult::SwitchBehaviorComplete {
            agent_id,
            session_id,
            control_token,
            mode,
            result,
        } => {
            // Success is intentionally not a completion: only the shell's
            // CurrentModeUpdate commits a Behavior transition. This prevents a
            // successful transport response from releasing a first prompt when
            // the shell subsequently reports rejected/confirmation_required.
            if result == Ok(crate::app::actions::ControlRpcOutcome::AuthoritativeUpdatePending) {
                return vec![];
            }
            let mut effects = vec![];
            let mut page_flip_entry = None;
            if let Some(agent) =
                find_agent_view_by_session_id(&mut app.agents, session_id.0.as_ref())
            {
                let completion = agent.session.complete_control(control_token);
                if completion == crate::app::session::SessionControlCompletion::Stale {
                    return vec![];
                }
                if let Err(error) = result
                    && !error.terminal_published
                {
                    agent.scrollback.push_block(RenderBlock::terminal_notice(
                        format!(
                            "control:behavior:{}:{}:{}",
                            session_id.0, control_token.generation, control_token.sequence
                        ),
                        NoticeTone::Error,
                        NoticeCategory::Control,
                        format!("{} Behavior switch failed", mode.display_label()),
                        Some(format!(
                            "Reason: {}. Retry the Behavior command after the current foreground boundary.",
                            error.message
                        )),
                    ));
                }
                if completion == crate::app::session::SessionControlCompletion::Drained {
                    crate::app::acp_handler::apply_deferred_authoritative_controls(
                        agent,
                        session_id.0.as_ref(),
                    );
                    let drain = maybe_drain_queue(agent);
                    page_flip_entry = drain.page_flip_entry;
                    effects.extend(drain.effects);
                }
                // On a terminal transport failure the deferred admission latch
                // deliberately remains: its prompt stays in the local FIFO and
                // cannot be accidentally sent under the old server mode.
            }
            crate::app::acp_handler::sync_child_control_projection_by_session_id(
                app,
                session_id.0.as_ref(),
            );
            super::queue::note_peek_page_flip(app, agent_id, page_flip_entry);
            effects
        }
        TaskResult::BgTaskKilled {
            session_id,
            task_id,
            outcome,
        } => handle_bg_task_killed(app, session_id, task_id, outcome),
        TaskResult::BgTaskKillFailed {
            session_id,
            task_id,
            error,
        } => {
            tracing::warn!(task_id = %task_id, error = %error, "Failed to kill bg task");
            if let Some(agent) = find_agent_by_session_id(&mut app.agents, &session_id)
                && let Some(task) = agent.session.bg_tasks.get_mut(&task_id)
            {
                task.pending_kill = false;
                task.kill_requested_at = None;
            }
            vec![]
        }
        TaskResult::ChangelogFetched { markdown, entries } => {
            app.changelog_markdown = markdown;
            app.changelog_bullets = shell::util::changelog::bullets_from_entries(&entries, 3);
            vec![]
        }
        TaskResult::ClipboardAttachmentProbed {
            ctx,
            image,
            file_urls,
        } => {
            let is_clipboard_key = ctx.source.is_clipboard_key();
            let primary_hint_eligible = is_clipboard_key
                && !app.screen_mode.is_minimal()
                && crate::clipboard::x11_primary_guidance_available();
            let target = ctx.target.clone();
            let wrap_text = if is_clipboard_key {
                ctx.source.text().map(str::to_owned)
            } else {
                None
            };
            let (completion, completion_effects) =
                apply_clipboard_paste_result(ctx, image, file_urls, app);
            let wrap_request_emitted = wrap_host_image_request_eligible(completion)
                && is_clipboard_key
                && crate::wrap_clipboard_image::maybe_request_wrap_host_image(
                    None,
                    wrap_text.as_deref(),
                    None,
                );
            let effects = drain_clipboard_target(&target, app, completion_effects);
            maybe_show_x11_primary_paste_hint(
                primary_hint_eligible && !wrap_request_emitted,
                completion,
                &target,
                app,
            );
            if let ClipboardPasteCompletion::Failed(failure) = completion
                && !wrap_request_emitted
            {
                show_clipboard_failure(&target, failure, app);
            }
            effects
        }
        TaskResult::ImageViewerLoaded {
            agent_id,
            child_session_id,
            owner_id,
            result,
        } => {
            let Some(root) = app.agents.get_mut(&agent_id) else {
                return vec![];
            };
            let target = match child_session_id.as_deref() {
                Some(child_id) => {
                    let Some(child) = root.subagent_views.get_mut(child_id) else {
                        return vec![];
                    };
                    child
                }
                None => root,
            };
            let current_owner = target
                .image_viewer
                .as_ref()
                .map(|viewer| viewer.overlay_owner_id);
            if current_owner != Some(owner_id) {
                return vec![];
            }
            match result {
                crate::prompt_images::ImageLoadResult::Loaded(data) => {
                    if let Some(viewer) = target.image_viewer.as_mut() {
                        viewer.apply_loaded(data);
                    }
                }
                crate::prompt_images::ImageLoadResult::Failed => {
                    target.image_viewer = None;
                    target.show_toast("Couldn't load image preview");
                }
            }
            vec![]
        }
        TaskResult::PromptImagePreviewPrepared => vec![],
        TaskResult::DoctorFixPlanned { target, result } => {
            let Some(target) = current_doctor_target(app, &target) else {
                deliver_doctor_message(
                    app,
                    target.agent_id,
                    NoticeTone::Warning,
                    "This fix was cancelled because the session changed. Run `/doctor fix` again."
                        .to_owned(),
                );
                return vec![];
            };
            match result {
                Ok(DoctorPlanningOutcome::Listing(listing)) => {
                    deliver_doctor_message(app, target.agent_id, NoticeTone::Info, listing);
                }
                Ok(DoctorPlanningOutcome::Plan(plan)) => {
                    super::prompt::open_doctor_fix_question(app, target, plan);
                }
                Ok(DoctorPlanningOutcome::RunLocally(command)) => {
                    deliver_doctor_message(
                        app,
                        target.agent_id,
                        NoticeTone::Info,
                        format!(
                            "This fix configures your local computer, not this SSH session.\nOn your local computer, run: {command}"
                        ),
                    );
                }
                Err(error) => deliver_doctor_message(
                    app,
                    target.agent_id,
                    NoticeTone::Error,
                    if error.starts_with("Could not prepare the fix:") {
                        error
                    } else {
                        format!("Could not prepare the fix: {error}")
                    },
                ),
            }
            vec![]
        }
        TaskResult::DoctorFixApplied { target, result } => {
            if let Some(agent) = app.agents.get_mut(&target.agent_id) {
                agent.session.clear_live_feedback("doctor-fix");
            }
            let (tone, message) = match result {
                Ok(outcome) => (
                    NoticeTone::Success,
                    crate::diagnostics::format_fix_success(&outcome),
                ),
                Err(error) if error.starts_with("Could not apply the fix:") => {
                    (NoticeTone::Error, error)
                }
                Err(error) => (
                    NoticeTone::Error,
                    format!("Could not apply the fix: {error}"),
                ),
            };
            deliver_doctor_message(app, target.agent_id, tone, message);
            vec![]
        }
        TaskResult::AnnouncementsHiddenPersisted { result } => {
            if let Err(e) = result {
                tracing::warn!("Failed to persist announcements hidden state: {}", e);
            }
            vec![]
        }
        TaskResult::PromptHistoryLoaded { agent_id, prompts } => {
            use tools::implementations::skills::skill::extract_skill_display_text;
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                agent.session.prompt_history_loading = false;
                agent.session.prompt_history = prompts
                    .into_iter()
                    .map(|p| extract_skill_display_text(&p).unwrap_or(p))
                    .collect();
                if agent.prompt.history_search.is_active() {
                    let history = agent.combined_prompt_history();
                    agent.prompt.history_search.refresh_items(&history);
                    if !agent.prompt.history_search.is_browse() {
                        let query = agent.prompt.text().to_owned();
                        agent.prompt.history_search.update_query(&query);
                    }
                }
            }
            vec![]
        }
        TaskResult::McpsListLoaded { agent_id, result } => {
            use crate::views::extensions_modal::TabDataState;
            if let Some(agent) = app.agents.get_mut(&agent_id)
                && let Some(ref mut modal) = agent.extensions_modal
            {
                modal.pending_action = None;
                modal.pending_entry_index = None;
                modal.mcps_data = match result {
                    Ok(response) => TabDataState::Loaded(response),
                    Err(e) => TabDataState::Error(e),
                };
            }
            vec![]
        }
        TaskResult::McpSetupSubmitDone {
            agent_id,
            server_name,
            result,
        } => handle_mcp_setup_submit_done(app, agent_id, server_name, result),
        TaskResult::HooksListLoaded { agent_id, result } => {
            handle_hooks_list_loaded(app, agent_id, result)
        }
        TaskResult::PluginsListLoaded { agent_id, result } => {
            handle_plugins_list_loaded(app, agent_id, result)
        }
        TaskResult::HooksActionResult { agent_id, result }
        | TaskResult::PluginsActionResult { agent_id, result }
        | TaskResult::MarketplaceActionResult { agent_id, result } => {
            dispatch_action_result(app, agent_id, result)
        }
        TaskResult::CtaPluginInstallDone {
            agent_id,
            plugin_name,
            result,
        } => handle_cta_plugin_install_done(app, agent_id, plugin_name, result),
        TaskResult::CtaPluginReloadDone {
            agent_id,
            plugin_name,
            result,
        } => handle_cta_plugin_reload_done(app, agent_id, plugin_name, result),
        TaskResult::PluginCtaMcpsLoaded {
            agent_id,
            plugin_name,
            result,
        } => handle_plugin_cta_mcps_loaded(app, agent_id, plugin_name, result),
        TaskResult::CtaInstalledDismissTimeout {
            agent_id,
            plugin_name,
        } => {
            use crate::app::agent_view::CtaPhase;
            if let Some(agent) = app.agents.get_mut(&agent_id)
                && let CtaPhase::Installed { name } = &agent.plugin_cta.phase
                && *name == plugin_name
            {
                agent.plugin_cta.phase = CtaPhase::Hidden;
            }
            vec![]
        }
        TaskResult::McpToggleDone { agent_id, result } => {
            handle_mcp_toggle_done(app, agent_id, result)
        }
        TaskResult::MarketplaceUpdatesAvailable { agent_id, updates } => {
            handle_marketplace_updates_available(app, agent_id, updates)
        }
        TaskResult::MarketplaceListLoaded { agent_id, result } => {
            handle_marketplace_list_loaded(app, agent_id, result)
        }
        TaskResult::PluginCtaCatalogLoaded { agent_id, result } => {
            handle_plugin_cta_catalog_loaded(app, agent_id, result)
        }
        TaskResult::SkillsListLoaded { agent_id, result } => {
            use crate::views::extensions_modal::TabDataState;
            if let Some(agent) = app.agents.get_mut(&agent_id)
                && let Some(ref mut modal) = agent.extensions_modal
            {
                modal.skills_data = match result {
                    Ok(skills) => TabDataState::Loaded(skills),
                    Err(e) => TabDataState::Error(e),
                };
                modal.pending_action = None;
                modal.pending_entry_index = None;
            }
            vec![]
        }
        TaskResult::WorkflowsListLoaded {
            agent_id,
            session_id,
            result,
        } => {
            use crate::views::extensions_modal::TabDataState;
            if let Some(agent) = app.agents.get_mut(&agent_id)
                && agent.session.session_id.as_ref() == Some(&session_id)
                && let Some(ref mut modal) = agent.extensions_modal
            {
                modal.workflows_data = match result {
                    Ok(workflows) => TabDataState::Loaded(workflows),
                    Err(e) => TabDataState::Error(e),
                };
            }
            vec![]
        }
        TaskResult::SkillsToggleDone { agent_id, result } => {
            handle_skills_toggle_done(app, agent_id, result)
        }
        TaskResult::SessionAgentNameResolved {
            agent_id,
            session_id,
            revision,
            agent_name,
        } => {
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                if agent.session.session_id.as_ref() != Some(&session_id)
                    || !agent.session.agent_metadata_read_is_current(revision)
                {
                    return vec![];
                }
                agent.session.apply_agent_name(agent_name.clone());
                if let Some(modal) = agent.agents_modal.as_mut() {
                    modal.active_agent = agent_name;
                }
            }
            vec![]
        }
        TaskResult::SessionInfoComplete {
            agent_id,
            session_id,
            revision,
            info,
            text,
            title,
            show_resolved_model,
            nonce,
        } => handle_session_info_complete(
            app,
            agent_id,
            session_id,
            revision,
            info,
            text,
            title,
            show_resolved_model,
            nonce,
        ),
        TaskResult::SessionInfoFailed {
            agent_id,
            error,
            nonce,
        } => handle_session_info_failed(app, agent_id, error, nonce),
        TaskResult::RenameSessionComplete { agent_id, title } => {
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                let safe = crate::views::session_title::sanitize_display_text(&title);
                agent
                    .scrollback
                    .push_block(crate::scrollback::block::RenderBlock::notice(format!(
                        "Session renamed to \"{safe}\""
                    )));
            }
            vec![]
        }
        TaskResult::RenameSessionFailed { agent_id, error } => {
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                agent
                    .scrollback
                    .push_block(crate::scrollback::block::RenderBlock::notice(format!(
                        "Couldn't rename session: {error}"
                    )));
            }
            vec![]
        }
        TaskResult::DeleteSessionComplete { session_id, after } => {
            use crate::app::actions::AfterSessionDelete;
            remove_session_from_pickers(app, &session_id);
            if after == AfterSessionDelete::Stay {
                app.dashboard_local_sessions
                    .retain(|entry| entry.session_id != session_id);
                app.leader_roster
                    .retain(|entry| entry.session_id != session_id);
                app.show_toast("Session deleted");
                return vec![];
            }
            let sid = acp::SessionId::new(session_id.clone());
            let to_remove: Vec<_> = app
                .agents
                .iter()
                .filter(|(_, agent)| agent.session.session_id.as_ref() == Some(&sid))
                .map(|(id, _)| *id)
                .collect();
            let foreground =
                matches!(app.active_view, ActiveView::Agent(id) if to_remove.contains(&id));
            let roster_row = crate::views::dashboard::DashboardRowId::Roster {
                session_id: session_id.clone(),
            };
            let closed_rows: Vec<_> = to_remove
                .iter()
                .copied()
                .map(crate::views::dashboard::DashboardRowId::TopLevel)
                .chain(std::iter::once(roster_row))
                .collect();
            let selected = app.dashboard.as_ref().and_then(|d| d.selected.clone());
            let neighbor = if after == AfterSessionDelete::Dashboard
                && let Some(sel) = selected.as_ref().filter(|sel| closed_rows.contains(sel))
            {
                super::dashboard::dashboard_neighbor_row(app, sel)
            } else {
                None
            };
            app.dashboard_local_sessions
                .retain(|entry| entry.session_id != session_id);
            app.leader_roster
                .retain(|entry| entry.session_id != session_id);
            for id in to_remove {
                remove_agent_and_cleanup(app, id);
            }
            let mut effects = unregister_session_effect(Some(sid));
            if after == AfterSessionDelete::Dashboard {
                if let Some(d) = app.dashboard.as_mut() {
                    d.delete_confirm = None;
                    let selected_closed = d
                        .selected
                        .as_ref()
                        .is_some_and(|sel| closed_rows.contains(sel));
                    match (selected_closed, neighbor) {
                        (true, Some(n)) => d.focus_row(n),
                        (true, None) => d.focus_new_agent_button(),
                        _ => {}
                    }
                }
                if foreground {
                    super::dashboard::ensure_dashboard_state(app);
                    app.active_view = ActiveView::AgentDashboard;
                }
            } else if foreground && after == AfterSessionDelete::Welcome {
                effects.extend(dispatch_exit_session(app));
            }
            app.show_toast("Session deleted");
            effects
        }
        TaskResult::DeleteSessionFailed { session_id, error } => {
            tracing::warn!(session_id = %session_id, error = %error, "session delete failed");
            app.show_toast(&format!("Couldn't delete session: {error}"));
            vec![]
        }
        TaskResult::ContextInfoComplete {
            agent_id,
            info,
            nonce,
        } => handle_context_info_complete(app, agent_id, info, nonce),
        TaskResult::ContextInfoFailed {
            agent_id,
            error,
            nonce,
        } => handle_context_info_failed(app, agent_id, error, nonce),
        TaskResult::SessionUsageComplete {
            agent_id,
            session_id,
            usage,
            nonce,
        } => apply_session_usage_result(app, agent_id, &session_id, Ok(usage), nonce),
        TaskResult::SessionUsageFailed {
            agent_id,
            session_id,
            error,
            nonce,
        } => apply_session_usage_result(app, agent_id, &session_id, Err(error), nonce),
        TaskResult::MemoryNoteSaved { agent_id, result } => {
            handle_memory_note_saved(app, agent_id, result)
        }
        TaskResult::MemoryNoteRewritten {
            agent_id,
            result,
            nonce,
        } => {
            if let Some(agent) = app.agents.get_mut(&agent_id)
                && let Ok(markdown) = result
                && let Some(crate::views::modal::ActiveModal::RememberNoteReview {
                    ref mut enhanced_content,
                    ref mut cached_lines,
                    rewrite_nonce,
                    ..
                }) = agent.active_modal
                && rewrite_nonce == nonce
            {
                *enhanced_content = Some(markdown);
                *cached_lines = None;
            }
            vec![]
        }
        TaskResult::BundleStatusReady {
            has_cache,
            version,
            agents,
            skills,
        } => {
            app.bundle_state.has_cache = has_cache;
            app.bundle_state.version = version.unwrap_or_default();
            app.bundle_state.agents = agents;
            app.bundle_state.skills = skills;
            vec![]
        }
        TaskResult::BundleStatusFailed { error } => {
            tracing::warn!(error = %error, "bundle status fetch failed");
            vec![]
        }
        TaskResult::CatalogEntryReady {
            kind,
            name,
            content,
        } => {
            if let ActiveView::Agent(id) = app.active_view
                && let Some(agent) = app.agents.get_mut(&id)
            {
                let title = format!("{kind}: {name}");
                agent.block_viewer = Some(
                    crate::views::block_viewer::BlockViewerPane::for_plain_text(&title, &content),
                );
            }
            vec![]
        }
        TaskResult::CatalogEntryFailed { error } => {
            tracing::warn!(error = %error, "catalog entry fetch failed");
            if let ActiveView::Agent(id) = app.active_view
                && let Some(agent) = app.agents.get_mut(&id)
            {
                agent
                    .scrollback
                    .push_block(RenderBlock::notice(format!("Couldn't load entry: {error}")));
            }
            vec![]
        }
        TaskResult::BtwResponse {
            agent_id,
            result,
            minimal_request_id,
        } => handle_btw_response(app, agent_id, result, minimal_request_id),
        TaskResult::InterjectQueued { .. } => vec![],
        TaskResult::RecapRequested {
            session_id,
            auto,
            error,
        } => {
            if let Some(error) = error {
                tracing::debug!(%error, "recap request failed");
                if !auto
                    && let Some(agent) = find_agent_by_session_id(&mut app.agents, &session_id.0)
                {
                    agent.session.clear_live_feedback("recap");
                    agent.show_toast(super::recap_unavailable_toast(
                        super::scrollback_has_user_messages(&agent.scrollback),
                    ));
                }
            }
            vec![]
        }
        TaskResult::InterjectFailed {
            agent_id,
            error,
            text,
            blocks,
        } => {
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                let id = agent.session.next_queue_id;
                agent.session.next_queue_id += 1;
                agent
                    .session
                    .pending_prompts
                    .push_front(crate::app::session::QueuedPrompt {
                        id,
                        text,
                        kind: crate::app::session::QueueEntryKind::Prompt,
                        wire_blocks: blocks,
                        images: Vec::new(),
                        display_as_skill: false,
                        chip_elements: Vec::new(),
                        skill_token_ranges: Vec::new(),
                        combined_texts: Vec::new(),
                    });
                agent.show_toast(&format!("Interjection failed — requeued: {error}"));
            }
            vec![]
        }
        TaskResult::SlashCommandExecuted {
            agent_id: _,
            session_id,
            request,
            error,
        } => {
            if let Some(error) = error
                && let Some(agent) =
                    find_agent_view_by_session_id(&mut app.agents, session_id.0.as_ref())
            {
                agent.scrollback.push_block(RenderBlock::terminal_notice(
                    format!("command:{}:rejected", request.invocation_id),
                    NoticeTone::Error,
                    NoticeCategory::Command,
                    format!("{} failed", request.command),
                    Some(format!(
                        "Reason: {error}\nRecovery: verify the command arguments and retry."
                    )),
                ));
            }
            vec![]
        }
        TaskResult::TrajectoryLaunched { agent_id, result } => {
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                match result {
                    Ok(url) => agent.open_url_or_show(&url),
                    Err(error) => agent.show_toast(&error),
                }
            }
            vec![]
        }
        TaskResult::TrajectoryRuntimeEnded { agent_id, message } => {
            tracing::warn!(agent = ?agent_id, %message, "Trajectory debugger stopped after launch");
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                agent.show_toast(&message);
            } else {
                app.show_toast(&message);
            }
            vec![]
        }
        TaskResult::AvailableCommandsRefreshed { agent_id, commands } => {
            if !commands.is_empty()
                && let Some(agent) = app.agents.get_mut(&agent_id)
            {
                agent.session.available_commands = commands;
                agent.session.available_commands_generation += 1;
                super::super::super::acp_handler::refresh_workflow_run_capabilities(agent);
            }
            vec![]
        }
        TaskResult::DeepSearchResults { results, seq } => {
            handle_deep_search_results(app, results, seq)
        }
        TaskResult::RewindPointsLoaded { agent_id, points } => {
            handle_rewind_points_loaded(app, agent_id, points)
        }
        TaskResult::RewindPointsFailed { agent_id, error } => {
            let Some(agent) = app.agents.get_mut(&agent_id) else {
                return vec![];
            };
            agent.rewind_state = None;
            app.show_toast(&format!("Rewind failed: {error}"));
            vec![]
        }
        TaskResult::RewindPreviewComplete {
            agent_id,
            response,
            target_prompt_index,
            mode,
        } => handle_rewind_preview_complete(app, agent_id, response, target_prompt_index, mode),
        TaskResult::RewindPreviewFailed { agent_id, error } => {
            handle_rewind_preview_failed(app, agent_id, error)
        }
        TaskResult::RewindExecuteComplete { agent_id, response } => {
            dispatch_rewind_success(app, agent_id, response)
        }
        TaskResult::RewindExecuteFailed { agent_id, error } => {
            handle_rewind_execute_failed(app, agent_id, error)
        }
        TaskResult::SuggestionDebounceExpired {
            agent_id,
            generation,
        } => handle_suggestion_debounce_expired(app, agent_id, generation),
        TaskResult::PluginCtaDebounceExpired {
            agent_id,
            generation,
        } => handle_plugin_cta_debounce_expired(app, agent_id, generation),
        TaskResult::ShellSuggestionsLoaded {
            agent_id,
            response,
            request_text,
            request_cursor,
        } => {
            let Some(agent) = app.agents.get_mut(&agent_id) else {
                return vec![];
            };
            if agent.prompt_input_mode != crate::app::agent_view::PromptInputMode::Bash {
                return vec![];
            }
            let generation = response.generation;
            agent
                .prompt
                .suggestions
                .on_suggestions_loaded(response, &request_text, request_cursor);
            let text = agent.prompt.text().to_owned();
            agent.prompt.suggestions.set_last_request_text(&text);
            let mut effects = Vec::new();
            if agent.prompt.suggestions.take_pending_tab(generation) {
                agent.shell_completion_tab(&mut effects);
            }
            effects
        }
        TaskResult::PromptSuggestionLoaded {
            agent_id,
            suggestion,
            generation,
        } => {
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                agent
                    .prompt
                    .prompt_suggestion
                    .on_loaded(suggestion, generation);
                agent.refresh_prompt_suggestion_gate();
                agent.log_prompt_suggestion_shown_if_visible();
            }
            vec![]
        }
        TaskResult::SettingPersisted { key, value } => {
            tracing::trace!(target: "settings", ?key, ?value, "setting persisted");
            vec![]
        }
        TaskResult::SettingPersistFailed {
            key,
            rollback_value,
            error,
        } => {
            let rollback_effects = apply_setting_rollback(app, key, &rollback_value);
            tracing::warn!(target: "settings", ?key, ?rollback_value, %error, "setting persist failed; rolled back");
            let scrubbed = scrub_error_for_toast(&error);
            app.show_toast(&format!("\u{2717} Could not save {key}: {scrubbed}"));
            rollback_effects
        }
        TaskResult::SettingPersistFailedBestEffort { key, error } => {
            tracing::warn!(
                target: "settings",
                ?key, %error,
                "setting persist failed (best-effort); in-memory state stays at optimistic value",
            );
            let scrubbed = scrub_error_for_toast(&error);
            app.show_toast(&format!("\u{2717} Could not save {key}: {scrubbed}"));
            vec![]
        }
    }
}

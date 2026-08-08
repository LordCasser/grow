//! Prompt and bash-command submission dispatchers and reload-window helpers.

use super::ctx::with_active_agent;
use super::interject;
use super::queue::{
    drain_prompt_state_to_last_queued, immediate_server_send_eligible, maybe_drain_queue,
    note_peek_page_flip, push_server_queue_echo, retire_optimistic_echo,
};
use super::router::dispatch;
use super::session::fork::open_project_question;
use super::session::lifecycle::skip_picker_and_create_session;
use crate::app::actions::{Action, DoctorFixTarget, Effect};
use crate::app::agent::{AgentCommand, AgentId, AgentState};
use crate::app::agent_view::AgentView;
use crate::app::app_view::{ActiveView, AppView};
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::SessionEvent;
use crate::slash::command::DoctorRequest;
use agent_client_protocol as acp;
use diagnostics::session_ctx::log_event;

fn scrollback_has_recent_context_too_large(
    scrollback: &crate::scrollback::state::ScrollbackState,
) -> bool {
    for idx in (0..scrollback.len()).rev() {
        match scrollback.entry(idx).map(|entry| &entry.block) {
            Some(RenderBlock::SessionEvent(event))
                if matches!(
                    event.event,
                    SessionEvent::ContextTooLarge | SessionEvent::CompactionFailed { .. }
                ) =>
            {
                return true;
            }
            Some(RenderBlock::SessionEvent(_)) | Some(RenderBlock::System(_)) => {}
            _ => break,
        }
    }
    false
}

/// Enqueue a prompt and try to drain immediately.
///
/// The prompt is always pushed to the queue first. If the agent is idle
/// (and has a session), `maybe_drain_queue` pops the front prompt and
/// sends it in the same dispatch call — no deferred ticks.
/// Start (if needed) and submit the initial prompt from `grow "<prompt>"`.
///
/// Shared by the TUI startup path and deferred startup via
/// `deferred_startup.prompt`. It reuses the exact `NewSession` /
/// `SendPrompt` actions the welcome screen dispatches, so the normal
/// session-creation + enqueue/drain machinery carries the prompt (the
/// prompt waits in the queue until `SessionCreated`/`SessionLoaded` drains
/// it). `NewSession` is only dispatched when no session is active yet — a
/// `--resume`/`-c`/`-w` session started earlier in startup is reused.
pub(crate) fn dispatch_initial_prompt(app: &mut AppView, prompt: String) -> Vec<Effect> {
    let mut effects = Vec::new();
    if !matches!(app.active_view, ActiveView::Agent(_)) {
        effects.extend(dispatch(Action::NewSession, app));
    }
    effects.extend(dispatch(Action::SendPrompt(prompt), app));
    effects
}

pub(super) fn collect_live_doctor_report_for_terminal(
    app: &AppView,
    agent_id: AgentId,
    terminal: &crate::terminal::TerminalContext,
) -> Option<crate::diagnostics::DiagnosticReport> {
    let agent = app.agents.get(&agent_id)?;
    let report = crate::slash::commands::doctor::DoctorCommand::report_for_terminal(
        terminal,
        app.screen_mode,
        crate::diagnostics::TuiRuntimeRequest {
            workspace: &agent.session.cwd,
            notification_method: app.notification_service.config().method,
            notification_protocol: app.notification_service.protocol(),
            notification_condition: app.notification_service.config().condition,
        },
    );
    Some(report)
}

fn doctor_fix_target(agent: &AgentView) -> DoctorFixTarget {
    DoctorFixTarget {
        agent_id: agent.session.id,
        session_id: agent.session.session_id.clone(),
        session_binding_epoch: agent.session_binding_epoch,
        cwd: agent.session.cwd.clone(),
    }
}

pub(super) fn dispatch_doctor(request: DoctorRequest, app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(agent_id) = app.active_view else {
        return vec![];
    };
    let terminal = crate::terminal::terminal_context().clone();
    let Some(report) = collect_live_doctor_report_for_terminal(app, agent_id, &terminal) else {
        return vec![];
    };

    match request {
        DoctorRequest::Report => {
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                agent.scrollback.push_block(RenderBlock::system(
                    crate::diagnostics::format_doctor(&report),
                ));
            }
        }
        DoctorRequest::ListFixes | DoctorRequest::Fix(_) => {
            let Some(agent) = app.agents.get(&agent_id) else {
                return vec![];
            };
            let target = doctor_fix_target(agent);
            return vec![Effect::PlanDoctorFix {
                target,
                report: Box::new(report),
                terminal,
                request,
            }];
        }
    }
    vec![]
}

pub(super) fn open_doctor_fix_question(
    app: &mut AppView,
    target: DoctorFixTarget,
    plan: Box<crate::diagnostics::FixPlan>,
) {
    use crate::views::question_view::{LocalQuestionKind, QuestionViewState};
    use tools::implementations::grow_build::ask_user_question::{Question, QuestionOption};

    let Some(agent) = app.agents.get_mut(&target.agent_id) else {
        return;
    };
    if agent.question_view.is_some() {
        agent.scrollback.push_block(RenderBlock::system(
            "Close the current question before applying this fix.",
        ));
        return;
    }
    let preview = crate::diagnostics::format_fix_preview(&plan);
    let question = Question {
        question: "Apply this fix?".to_owned(),
        options: vec![
            QuestionOption {
                label: "Apply".to_owned(),
                description: "Make the changes shown above.".to_owned(),
                preview: Some(preview),
                id: None,
            },
            QuestionOption {
                label: "Cancel".to_owned(),
                description: "Do not change the configuration.".to_owned(),
                preview: None,
                id: None,
            },
        ],
        multi_select: Some(false),
        id: None,
    };
    let stashed = agent.prompt.stash();
    agent.question_view = Some(
        QuestionViewState::new("doctor-fix".to_owned(), vec![question], stashed)
            .with_local_kind(LocalQuestionKind::DoctorFix { target, plan })
            .with_no_freeform(),
    );
    agent.prompt.set_text("");
}

pub(super) fn dispatch_send_prompt(app: &mut AppView, text: String) -> Vec<Effect> {
    crate::unified_log::info(
        "prompt.enqueue",
        None,
        Some(serde_json::json!({"len": text.len()})),
    );
    dispatch_send_prompt_inner(
        app, text, /* consume_input */ true, /* literal */ false,
        /* is_follow_up */ false,
    )
}

/// Clear the active prompt and record non-empty text in prompt history (Esc Esc).
pub(super) fn dispatch_clear_prompt(app: &mut AppView) -> Vec<Effect> {
    with_active_agent(app, |agent| {
        let text = agent.prompt.text().to_string();
        // Same move-to-front / cap as send / interject.
        interject::record_interject_prompt_history(agent, &text);
        // Clears chips/images via PromptWidget::set_text empty path.
        agent.prompt.set_text("");
    });
    vec![]
}

/// Open the prompt-history search panel on the active agent (composer as
/// filter query). Dispatched by `/history`; the slash pipeline has already
/// cleared the composer, so the panel opens with an empty query.
pub(super) fn dispatch_open_history_search(app: &mut AppView) -> Vec<Effect> {
    with_active_agent(app, |agent| {
        let history = agent.combined_prompt_history();
        let current_text = agent.prompt.text().to_string();
        agent
            .prompt
            .history_search
            .activate(&history, &current_text);
    });
    vec![]
}

/// Show the "ctrl+z to undo" hint after the user wiped a substantial draft.
/// Gated by the per-tip `contextual_hints.undo` gate (default ON). The
/// ephemeral-tip seen gate caps it at `UNDO_TIP_SEEN_CAP` shows per session
/// (in-memory `app.tip_seen_counts`); nothing is persisted to disk.
pub(super) fn dispatch_show_undo_tip(app: &mut AppView) -> Vec<Effect> {
    if !app.contextual_hints.undo {
        return vec![];
    }
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    // Shows and increments the per-session count in place (no disk write).
    // Emit the impression only when the tip actually took the slot (mirrors
    // the `tip.shown` gate), so gated no-ops and TTL refreshes don't count.
    if agent.show_ephemeral_tip(
        crate::tips::clear_detector::undo_tip(),
        &mut app.tip_seen_counts,
    ) {
        log_event(diagnostics::events::ContextualTip {
            tip: diagnostics::events::ContextualTipKind::Undo,
            action: diagnostics::events::ContextualTipAction::Shown,
        });
    }
    vec![]
}

/// Show the one-shot "Tight on space? Try /compact-mode" hint after the first
/// stable agent-view draw landed in the small-screen band (the trigger gates
/// on band + user compact OFF; see `AppView::maybe_trigger_small_screen_tip`).
/// Gated by the per-tip `contextual_hints.small_screen` gate (default ON).
/// Seen-gated in-memory via `app.tip_seen_counts`; nothing persists to disk.
///
/// Called directly from the draw-path trigger — not routed as an `Action`,
/// so it returns `()` and "no effects from draw" holds structurally.
pub(in crate::app) fn show_small_screen_tip(app: &mut AppView) {
    if !app.contextual_hints.small_screen {
        return;
    }
    let ActiveView::Agent(id) = app.active_view else {
        return;
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return;
    };
    // Impression only when the tip actually takes the slot (mirrors undo/plan).
    if agent.show_ephemeral_tip(
        crate::tips::small_screen::small_screen_tip(),
        &mut app.tip_seen_counts,
    ) {
        log_event(diagnostics::events::ContextualTip {
            tip: diagnostics::events::ContextualTipKind::SmallScreen,
            action: diagnostics::events::ContextualTipAction::Shown,
        });
    }
}

/// Show the existing one-shot SSH discovery tip, redirected to `/doctor`.
pub(in crate::app) fn show_ssh_wrap_tip(app: &mut AppView) {
    if !app.contextual_hints.ssh_wrap {
        return;
    }
    let ActiveView::Agent(id) = app.active_view else {
        return;
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return;
    };
    if agent.show_ephemeral_tip(
        crate::tips::ssh_wrap::ssh_wrap_tip(),
        &mut app.tip_seen_counts,
    ) {
        log_event(diagnostics::events::ContextualTip {
            tip: diagnostics::events::ContextualTipKind::SshWrap,
            action: diagnostics::events::ContextualTipAction::Shown,
        });
    }
}

pub(super) fn dispatch_show_plan_nudge(app: &mut AppView) -> Vec<Effect> {
    if !app.contextual_hints.plan_mode {
        return vec![];
    }
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    // Shows and increments the per-session count in place (no disk write).
    // Impression counts only on a real show (see `dispatch_show_undo_tip`).
    if agent.show_ephemeral_tip(
        crate::tips::plan_nudge::plan_nudge_tip(),
        &mut app.tip_seen_counts,
    ) {
        log_event(diagnostics::events::ContextualTip {
            tip: diagnostics::events::ContextualTipKind::PlanMode,
            action: diagnostics::events::ContextualTipAction::Shown,
        });
    }
    vec![]
}

/// After a fold/nav double-click on scrollback, tip that Word select lives in
/// `/settings`. Gated by `contextual_hints.word_select` (default ON).
pub(super) fn dispatch_show_word_select_tip(app: &mut AppView) -> Vec<Effect> {
    if !app.contextual_hints.word_select {
        return vec![];
    }
    // Already on word_select — tip would be wrong / redundant.
    if crate::appearance::cache::load_keep_text_selection().selects_word() {
        return vec![];
    }
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    if agent.show_ephemeral_tip(
        crate::tips::word_select::word_select_tip(),
        &mut app.tip_seen_counts,
    ) {
        log_event(diagnostics::events::ContextualTip {
            tip: diagnostics::events::ContextualTipKind::WordSelect,
            action: diagnostics::events::ContextualTipAction::Shown,
        });
    }
    // Snapshot the prompt as of this double-click (also on a same-key TTL
    // refresh — a new double-click is a new moment). Any later divergence
    // (typed, pasted, dropped) refuses the chord and retires the tip; a
    // seen-cap-gated no-show leaves the slot to another tip and skips this.
    if agent.ephemeral_tip.current_key() == Some(crate::tips::word_select::WORD_SELECT_TIP_KEY) {
        agent.word_select_tip_prompt_snapshot = Some(agent.prompt.text().to_string());
    }
    vec![]
}

/// Accept the word-select tip via its advertised chord: flip
/// `keep_text_selection` to `word_select` (cache + persist + toast, the same
/// path as the settings modal) and retire the tip so one impression maps to
/// at most one acceptance. No-op unless the tip is on screen — the chord is
/// tip-scoped and must not become a global setting toggle.
pub(super) fn dispatch_accept_word_select_tip(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    if agent.ephemeral_tip.current_key() != Some(crate::tips::word_select::WORD_SELECT_TIP_KEY) {
        return vec![];
    }
    agent
        .ephemeral_tip
        .clear(crate::tips::word_select::WORD_SELECT_TIP_KEY);
    agent.word_select_tip_prompt_snapshot = None;
    log_event(diagnostics::events::ContextualTip {
        tip: diagnostics::events::ContextualTipKind::WordSelect,
        action: diagnostics::events::ContextualTipAction::Accepted,
    });
    super::settings::setters::set_keep_text_selection(
        app,
        crate::appearance::TextSelection::WordSelect,
    )
}

/// After queuing a follow-up mid-turn, tip that empty Enter force-sends the top
/// queued item. Gated by the per-tip `contextual_hints.send_now` gate (default
/// ON). Seen-gated in-memory via `app.tip_seen_counts`.
fn maybe_show_send_now_tip(app: &mut AppView) {
    if !app.contextual_hints.send_now {
        return;
    }
    let ActiveView::Agent(id) = app.active_view else {
        return;
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return;
    };
    // Impression only when the tip actually takes the slot (mirrors undo/plan).
    if agent.show_ephemeral_tip(
        crate::tips::send_now::send_now_tip(),
        &mut app.tip_seen_counts,
    ) {
        log_event(diagnostics::events::ContextualTip {
            tip: diagnostics::events::ContextualTipKind::SendNow,
            action: diagnostics::events::ContextualTipAction::Shown,
        });
    }
}

/// Whether submitting `text` may open the project picker. Slash commands,
/// `exit`/`quit` aliases, and empty input never send a prompt to the agent,
/// so they must pass through untouched.
pub(super) fn input_can_trigger_project_picker(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty()
        && !t.starts_with('/')
        && !t.starts_with('!')
        && !matches!(t, "exit" | "quit" | ":q" | ":q!" | ":wq" | ":wq!")
}

/// Body of [`dispatch_send_prompt`], parameterized over whether to consume
/// the prompt textarea after the command is processed.
///
/// `consume_input = true` (Enter from the prompt) wipes the textarea, drains
/// pending prompt images into the queue, and inserts the text into the local
/// up-arrow history. `consume_input = false` (modal-driven dispatch from the
/// command palette or ArgPicker) preserves the user's draft, leaves prompt
/// images attached, and skips the history insert. The slash-registry
/// resolution and the downstream `Effect`s are identical in both cases.
///
/// `literal = true` (follow-up chip click) submits `text` straight to the
/// model: the slash-command, exit-alias, and project-picker branches are all
/// skipped so server/model-controlled chip text can never execute a command
/// nor be diverted into the project question.
pub(super) fn dispatch_send_prompt_inner(
    app: &mut AppView,
    text: String,
    consume_input: bool,
    literal: bool,
    is_follow_up: bool,
) -> Vec<Effect> {
    // Submitting is a fresh intent that retires any armed double-press. The
    // AppView pending-action check only resets on KEY events, so a submit with
    // no intervening key (mouse send, follow-up chip click `SubmitFollowUp`,
    // `SendSlashCommandPreservingDraft`) would otherwise leave a stale arm
    // (e.g. an idle-Esc `ClearPrompt`) that shadows the next Esc — firing stale
    // ClearPrompt|Rewind instead of the mid-turn Esc policy until TTL. Cleared in
    // the common funnel so every submit path is covered, before any early-return
    // guard below.
    app.pending_action = None;
    if app.reconnect_pending {
        app.show_toast("Reconnecting, please wait...");
        return vec![];
    }

    // The picker intercepts only real, user-authored prompts; slash commands,
    // exit aliases, empty input, and literal chip submissions pass through so
    // they never spawn it (a chip is a model suggestion for an already-running
    // session, not the first-prompt project choice).
    if !literal && input_can_trigger_project_picker(&text) && app.needs_project_picker() {
        return open_project_question(app, text);
    }

    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    // Capture app-level fields before the mut-borrow on `agent`.
    let show_tips_from_app = app.show_tips;
    let auto_update_from_app = app.auto_update;
    let respect_manual_folds_from_app = app.appearance.scrollback.scroll.respect_manual_folds;
    let auto_mode_gate_from_app = app.auto_mode_gate;
    let ask_user_question_timeout_enabled_from_app = app.ask_user_question_timeout_enabled;
    // Set when a plain prompt is queued while a turn is running (local path);
    // shown after the agent borrow ends so we can re-enter via the tip helper.
    let mut tip_send_now_after_queue = false;
    let scheduler_background_loops_seed = app.scheduler_background_loops_seed;
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };

    // Paste-then-immediate-send: an image probe from a just-pasted Cmd+V is
    // still off-thread. Stash this send and re-issue it once the probe completes
    // so the image is never dropped from the built content blocks. Scoped to
    // `consume_input` sends: only those clear the draft, so only they can drop a
    // not-yet-attached image — draft-preserving sends (follow-up chip,
    // slash-preserving) keep it in the draft for the next real send.
    if consume_input && agent.paste_probe_in_flight > 0 {
        agent.deferred_send = Some(crate::app::agent_view::AgentDeferredSend::SendPrompt);
        return vec![];
    }

    // Submitting the prompt retires any edit-contextual ephemeral tip
    // (ambient tips live out their TTL across the submit).
    agent.ephemeral_tip.clear_on_submit();

    let trimmed = text.trim();

    let mut effects = Vec::new();

    // ── Registry-based slash command execution ─────────────────────
    // If the text starts with `/`, run it through the slash registry.
    // The registry resolves builtins, ACP-advertised commands, and
    // unknown commands uniformly. Dispatch is the SOLE execution owner.
    // `literal` (chip click) skips this so chip text is never a command.
    if !literal && trimmed.starts_with('/') {
        use crate::slash::command::{CommandExecCtx, CommandResult};
        use crate::slash::parse_invocation;

        // Build execution context.
        let exec_result = {
            let mut ctx = CommandExecCtx {
                models: &agent.session.models,
                session_id: agent.session.session_id.as_ref(),
                bundle_state: &app.bundle_state,
                screen_mode: app.screen_mode,
                // PAGER-owned snapshot for slash commands.
                pager_state: crate::settings::PagerLocalSnapshot {
                    multiline_mode: agent.multiline_mode,
                    yolo_mode: agent.session.is_yolo(),
                    auto_mode: agent.session.is_auto(),
                    current_model_id: agent
                        .session
                        .models
                        .current_model_id_str()
                        .map(str::to_owned),
                    available_models: agent
                        .session
                        .models
                        .available
                        .iter()
                        .map(|(id, _info)| (id.0.to_string(), id.clone()))
                        .collect(),
                    behavior_mode: agent.behavior_mode_pending.unwrap_or(agent.behavior_mode),
                    workflows_available: agent.prompt.slash_controller.workflows_available(),
                    deep_research_available: agent
                        .prompt
                        .slash_controller
                        .registry()
                        .get("deep-research")
                        .is_some(),
                    goal_available: agent
                        .prompt
                        .slash_controller
                        .registry()
                        .get("goal")
                        .is_some(),
                    show_tips: show_tips_from_app,
                    auto_update: auto_update_from_app,
                    vim_mode: crate::appearance::cache::load_vim_mode(),
                    scroll_speed: crate::appearance::cache::load_scroll_speed(),
                    respect_manual_folds: respect_manual_folds_from_app,
                    auto_mode_gate: auto_mode_gate_from_app,
                    ask_user_question_timeout_enabled: ask_user_question_timeout_enabled_from_app,
                    // This session's own value (what its fires will actually
                    // do), seed only until the session response lands.
                    scheduler_background_loops: agent
                        .scheduler_background_loops
                        .unwrap_or(scheduler_background_loops_seed),
                },
            };

            if let Some(invocation) = parse_invocation(trimmed) {
                let (is_builtin, command) = {
                    let reg = agent.prompt.slash_controller.registry();
                    let is_builtin = reg.is_builtin(invocation.token);
                    // Bypasses only the menu-only hide (hard gates still
                    // return `None`); see `CommandRegistry::get_for_dispatch`.
                    let command = reg.get_for_dispatch(invocation.token).cloned();
                    (is_builtin, command)
                };
                {
                    use diagnostics::events::{PagerCommandSource, PagerSlashCommand};
                    use diagnostics::session_ctx::log_event;
                    let source = if is_builtin {
                        PagerCommandSource::Builtin
                    } else {
                        PagerCommandSource::NonBuiltin
                    };
                    log_event(PagerSlashCommand {
                        command_name: invocation.token.to_string(),
                        source,
                    });
                }
                if let Some(command) = command {
                    // Central screen-mode gate. Such a command is already
                    // filtered out of every completion surface, but it stays
                    // resolvable so a fully-typed invocation earns a hint that
                    // names the way out instead of leaking to the model.
                    if let Some(refusal) = command
                        .mode_support()
                        .refusal(invocation.token, ctx.screen_mode)
                    {
                        CommandResult::Message(refusal)
                    } else {
                        agent
                            .prompt
                            .slash_controller
                            .record_command_use(invocation.token, invocation.token);
                        command.run(&mut ctx, invocation.args)
                    }
                } else {
                    CommandResult::Error(format!("Unknown command: /{}", invocation.token))
                }
            } else {
                CommandResult::Error("Unknown command: /".to_string())
            }
        };

        // Map CommandResult to pager behavior. (MRU persistence is queued
        // off-thread inside `record_command_use` above.)
        match exec_result {
            CommandResult::Handled | CommandResult::HandledNoOp => {
                if consume_input {
                    agent.prompt.set_text("");
                }
                return vec![];
            }
            CommandResult::Error(msg) => {
                if consume_input {
                    agent.prompt.set_text("");
                }
                agent.scrollback.push_block(RenderBlock::system(msg));
                return vec![];
            }
            CommandResult::Message(msg) => {
                if consume_input {
                    agent.prompt.set_text("");
                }
                agent.scrollback.push_block(RenderBlock::system(msg));
                return vec![];
            }
            CommandResult::Doctor(request) => {
                if consume_input {
                    agent.prompt.set_text("");
                }
                return dispatch_doctor(request, app);
            }
            CommandResult::Action(Action::ExitSession) => {
                if consume_input {
                    agent.prompt.set_text("");
                }
                return dispatch(Action::ExitSession, app);
            }
            CommandResult::Action(Action::EditPromptExternal) => {
                // Typed slash input occupies the composer; the palette route preserves an existing draft.
                if consume_input {
                    agent.prompt.set_text("");
                }
                return dispatch(Action::EditPromptExternal, app);
            }
            CommandResult::Action(action) => {
                if consume_input {
                    agent.prompt.set_text("");
                }
                return dispatch(action, app);
            }
            CommandResult::QueueCommand(cmd_text) => {
                let Some(session_id) = agent.session.session_id.clone() else {
                    if consume_input {
                        agent.prompt.set_text("");
                    }
                    agent.scrollback.push_block(RenderBlock::system(
                        "/compact requires an active session.".to_string(),
                    ));
                    return vec![];
                };
                let track_foreground = agent.session.state.is_idle();
                if track_foreground {
                    agent.session.start_command(AgentCommand::Compact);
                    agent.turn_started_at = Some(std::time::Instant::now());
                }
                let user_context = cmd_text
                    .strip_prefix("/compact")
                    .map(str::trim)
                    .filter(|context| !context.is_empty())
                    .map(str::to_string);
                if consume_input {
                    agent.prompt.set_text("");
                }
                return vec![Effect::Compact {
                    agent_id: id,
                    session_id,
                    user_context,
                    track_foreground,
                }];
            }
            CommandResult::InjectSkill {
                display_text,
                prompt_blocks,
                display_as_skill,
                scheduled_task_preview,
            } => {
                // Enqueue with display text for scrollback but wire_blocks
                // for the actual prompt sent to the model. Leading skill
                // invocation: display_as_skill owns styling (no ranges).
                let id = agent.session.next_queue_id;
                agent.session.next_queue_id += 1;
                agent
                    .session
                    .pending_prompts
                    .push_back(crate::app::agent::QueuedPrompt {
                        wire_blocks: Some(prompt_blocks),
                        display_as_skill,
                        ..crate::app::agent::QueuedPrompt::plain(
                            id,
                            display_text,
                            crate::app::agent::QueueEntryKind::Prompt,
                        )
                    });

                // Insert a provisional scheduled task so the tasks pane shows
                // it immediately, before the LLM round-trips through
                // scheduler_create. Keyed by a provisional ID; replaced when
                // the real ScheduledTaskCreated notification arrives.
                if let Some(preview) = scheduled_task_preview {
                    use crate::app::agent::ScheduledTaskInfo;
                    let provisional_id = format!("provisional-{}", id);
                    agent.session.scheduled_tasks.insert(
                        provisional_id.clone(),
                        ScheduledTaskInfo {
                            task_id: provisional_id,
                            prompt: preview.prompt,
                            human_schedule: preview.human_schedule,
                            created_at: std::time::Instant::now(),
                            next_fire_at: preview.next_fire_at,
                            tag: preview.tag,
                            last_subagent_id: None,
                        },
                    );
                }
            }
            CommandResult::HostCommand(command_text) => {
                if let Some(session_id) = agent.session.session_id.clone() {
                    if consume_input {
                        agent.prompt.set_text("");
                    }
                    return vec![Effect::ExecuteSlashCommand {
                        agent_id: id,
                        session_id,
                        command: command_text,
                    }];
                }
                agent.scrollback.push_block(RenderBlock::system(
                    "This command requires an active session.".to_string(),
                ));
                return vec![];
            }
        }
        if consume_input {
            // Drain prompt images before clearing prompt state.
            drain_prompt_state_to_last_queued(agent);
            agent.prompt.set_text("");
        }
        // Every non-enqueue arm returned above. Queued slash work bypasses
        // the picker, so create the deferred session or it never drains.
        effects = skip_picker_and_create_session(app, id);
    } else if !literal && matches!(trimmed, "exit" | "quit" | ":q" | ":q!" | ":wq" | ":wq!") {
        if consume_input {
            agent.prompt.set_text("");
        }
        return dispatch(Action::Quit, app);
    } else if agent.behavior_mode_pending.unwrap_or(agent.behavior_mode)
        == tools::types::SessionMode::Goal
        && agent.goal_state.is_none()
        && agent.prompt.images.is_empty()
        && let Some(session_id) = agent.session.session_id.clone()
    {
        // Goal Behavior has no objective yet: its first ordinary text is the
        // definition command, not an implementer message. Route it through
        // the same hidden control turn as an explicit `/goal set` so it gets
        // one response log, never a user bubble, and cannot race planning in
        // the actor mailbox.
        if consume_input {
            agent.prompt.set_text("");
            let history_key = text.trim().to_string();
            agent
                .session
                .prompt_history
                .retain(|item| item.trim() != history_key);
            agent.session.prompt_history.insert(0, text.clone());
            if agent.session.prompt_history.len() > 200 {
                agent.session.prompt_history.truncate(200);
            }
        }
        return vec![Effect::ExecuteSlashCommand {
            agent_id: id,
            session_id,
            command: format!("/goal set {}", text.trim()),
        }];
    } else {
        // ── Server-authoritative immediate send (plain prompt only) ──
        // A plain prompt typed while a turn is RUNNING is sent to the agent
        // immediately instead of being held in the local drip-feed queue. The
        // agent appends it to its authoritative `pending_inputs` (no concurrent
        // turn starts — validated keystone) and drives the drain via
        // `grow/queue/changed`. We render an optimistic echo into the shared
        // queue keyed by `prompt_id`; the broadcast reconciles it by id.
        //
        // The IDLE case is unchanged (falls through to the local path below,
        // which drains instantly and renders the user block) — preserving the
        // byte-for-byte idle experience. Image/skill/editing/non-running cases
        // also stay local; they're out of immediate-send scope.
        // Plain prompts also require "no images" (image prompts stay local).
        //
        // A follow-up chip submission supersedes the current response's
        // suggestions: clear the visible chips here — INSIDE the send/enqueue
        // path, after the `reconnect_pending` and active-agent early-return
        // guards — so the chips are cleared ONLY when the suggestion actually
        // sends/enqueues. Placing it before those guards (the prior fix) cleared
        // the chips even when `reconnect_pending` aborted with a toast and no
        // send, losing both the chips and the submit. This single clear covers
        // BOTH the immediate-send and enqueue subpaths below; `clear_follow_ups`
        // is idempotent (so the immediate-send branch's own clear is a no-op)
        // and keeps `follow_up_seen` (a stale re-delivery stays rejected).
        //
        // Gate on a BOUND session: with no `session_id`, the enqueue subpath
        // below queues the text but `maybe_drain_queue` returns WITHOUT emitting
        // `SendPrompt` (nothing can drain to an unbound session), so clearing the
        // chips here would lose the click with nothing submitted. Leaving them
        // shown preserves the suggestion for a retry once the session binds.
        if is_follow_up && agent.session.session_id.is_some() {
            agent.clear_follow_ups();
        }

        // If the user queues a follow-up while a turn is already running, surface
        // a short tip advertising send-now — plain Enter queues; Enter again on
        // the emptied composer sends the queued message now (cancel-and-send).
        let queued_while_running = agent.session.state.is_turn_running();

        // Composer-recognized slash tokens at submit time: styles the
        // scrollback echo and rides the wire meta so replay restyles it.
        let skill_token_ranges = agent
            .prompt
            .slash_controller
            .recognized_token_ranges(&text, &agent.session.models);

        let immediate_server_send =
            immediate_server_send_eligible(agent) && agent.prompt.images.is_empty();
        tracing::debug!(
            target: "qtrace",
            pid = std::process::id(),
            event = "send_route_plain",
            immediate = immediate_server_send,
            is_turn_running = agent.session.state.is_turn_running(),
            shared_queue_len = agent.shared_queue.len(),
            pending_len = agent.session.pending_prompts.len(),
            current_prompt_id = agent.session.current_prompt_id.as_deref().unwrap_or(""),
            session = agent.session.session_id.as_ref().map(|s| s.0.as_ref()).unwrap_or(""),
            images = agent.prompt.images.len(),
            text = %text.chars().take(48).collect::<String>(),
            "plain prompt send routing decision",
        );

        // Parked + held occupancy → append; empty held → cancel-and-send.
        let parked_sendable_wait = agent.is_parked_on_sendable_wait();
        if immediate_server_send {
            let session_id = agent
                .session
                .session_id
                .clone()
                .expect("session_id is_some checked");
            let agent_id = agent.session.id;
            let prompt_id = uuid::Uuid::new_v4().to_string();
            // Self-originated: when this prompt becomes the running turn (via the
            // `running_prompt_id` adoption + turn-start shim), the ACP gate must
            // treat its deltas as ours, not adopt them as another client's turn.
            agent.note_self_originated_prompt(&prompt_id);

            if consume_input {
                // Plain prompt: no images to drain. Clear textarea + record
                // up-arrow history (same as the local path's history insert).
                agent.prompt.set_text("");
                let trimmed_key = text.trim().to_string();
                if !trimmed_key.is_empty() {
                    agent
                        .session
                        .prompt_history
                        .retain(|p| p.trim() != trimmed_key);
                    agent.session.prompt_history.insert(0, text.clone());
                    if agent.session.prompt_history.len() > 200 {
                        agent.session.prompt_history.truncate(200);
                    }
                }
            }

            // A new prompt is taking the wheel: the previous response's
            // follow-up chips must not linger into it. The local drain
            // (`maybe_drain_queue`) and the turn-start shim clear them on
            // their paths; this immediate-send path returns early, so it must
            // clear them here too (notably a chip click, which submits while
            // a turn is running). `clear_follow_ups` keeps `follow_up_seen`
            // (turn-boundary semantics) so a stale re-delivery stays rejected.
            agent.clear_follow_ups();

            // `agent` borrow ends here; push the optimistic echo via `app`.
            let sid_str = session_id.0.to_string();
            push_server_queue_echo(app, agent_id, &sid_str, &prompt_id, &text, "prompt");
            crate::unified_log::info(
                "prompt.send_server_authoritative",
                Some(&sid_str),
                Some(serde_json::json!({ "kind": "prompt", "len": text.len() })),
            );
            if queued_while_running && !parked_sendable_wait {
                maybe_show_send_now_tip(app);
            }
            return vec![Effect::SendPrompt {
                agent_id,
                session_id,
                text,
                prompt_id,
                skill_token_ranges,
            }];
        }

        agent
            .session
            .enqueue_prompt_with_skill_tokens(text.clone(), skill_token_ranges);
        if consume_input {
            // Drain prompt images before clearing prompt state.
            drain_prompt_state_to_last_queued(agent);
            agent.prompt.set_text("");
        }
        // Local queue while a turn is running (e.g. images attached): tip after
        // this branch so the agent mut-borrow is released first.
        tip_send_now_after_queue = queued_while_running;
    }

    // Mid-turn local queue: advertise send-now via the ephemeral tip (skip during
    // a sendable wait — the inline hint already says it).
    if tip_send_now_after_queue {
        let inline_hint_shown = app
            .agents
            .get(&id)
            .is_some_and(|agent| agent.held_queue_count() > 0);
        if !inline_hint_shown {
            maybe_show_send_now_tip(app);
        }
    }

    let drain = {
        let Some(agent) = app.agents.get_mut(&id) else {
            return effects;
        };

        // Insert into local prompt history (move-to-front dedup, cap at 200).
        // Skipped for modal-driven dispatch: the user didn't type these
        // commands and shouldn't see them in up-arrow history.
        if consume_input {
            let trimmed_key = text.trim().to_string();
            if !trimmed_key.is_empty() {
                agent
                    .session
                    .prompt_history
                    .retain(|p| p.trim() != trimmed_key);
                agent.session.prompt_history.insert(0, text.clone());
                if agent.session.prompt_history.len() > 200 {
                    agent.session.prompt_history.truncate(200);
                }
            }
        }
        maybe_drain_queue(agent)
    };
    effects.extend(drain.effects);
    note_peek_page_flip(app, id, drain.page_flip_entry);
    effects
}

/// Enqueue a bash command and try to drain immediately.
///
/// Bash commands go through the same enqueue/drain pipeline as normal prompts,
/// just with `QueueEntryKind::BashCommand`. No scrollback block is pushed here;
/// the execute block from the shell IS the visual entry.
pub(super) fn dispatch_send_bash_command(app: &mut AppView, command: String) -> Vec<Effect> {
    if app.reconnect_pending {
        app.show_toast("Reconnecting, please wait...");
        return vec![];
    }

    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    // Submitting a bash command retires any edit-contextual ephemeral tip.
    agent.ephemeral_tip.clear_on_submit();
    if agent.session.session_id.is_none() {
        let effects = skip_picker_and_create_session(app, id);
        if let Some(agent) = app.agents.get_mut(&id) {
            agent.session.enqueue_bash_command(command);
            agent.prompt.set_text("");
        }
        return effects;
    }

    // Store in prompt history with `! ` prefix for restore semantics.
    let history_key = format!("! {}", command.trim());
    agent
        .session
        .prompt_history
        .retain(|p| p.trim() != history_key);
    agent.session.prompt_history.insert(0, history_key);
    if agent.session.prompt_history.len() > 200 {
        agent.session.prompt_history.truncate(200);
    }

    // ── Server-authoritative immediate send for bash while running ──
    // A bash command typed while a turn is RUNNING is sent to the agent
    // immediately (it's already a `session/prompt` with bash meta) and echoed
    // into the shared queue with `kind="bash"`. On `running_prompt_id`
    // adoption the turn-start shim sets `bash_turn` (no user block). The IDLE
    // case is unchanged: enqueue locally + drain instantly.
    let bash_immediate = immediate_server_send_eligible(agent);
    tracing::debug!(
        target: "qtrace",
        pid = std::process::id(),
        event = "send_route_bash",
        immediate = bash_immediate,
        is_turn_running = agent.session.state.is_turn_running(),
        shared_queue_len = agent.shared_queue.len(),
        pending_len = agent.session.pending_prompts.len(),
        current_prompt_id = agent.session.current_prompt_id.as_deref().unwrap_or(""),
        session = agent.session.session_id.as_ref().map(|s| s.0.as_ref()).unwrap_or(""),
        text = %command.chars().take(48).collect::<String>(),
        "bash command send routing decision",
    );
    if bash_immediate {
        let session_id = agent
            .session
            .session_id
            .clone()
            .expect("session_id is_some checked");
        let agent_id = agent.session.id;
        let prompt_id = uuid::Uuid::new_v4().to_string();
        // Self-originated (see the plain immediate-send path): keep this turn's
        // deltas ours in the ACP gate once it becomes the running turn.
        agent.note_self_originated_prompt(&prompt_id);
        agent.prompt.set_text("");

        let sid_str = session_id.0.to_string();
        push_server_queue_echo(app, agent_id, &sid_str, &prompt_id, &command, "bash");
        crate::unified_log::info(
            "prompt.send_server_authoritative",
            Some(&sid_str),
            Some(serde_json::json!({ "kind": "bash", "len": command.len() })),
        );
        return vec![Effect::SendBashCommand {
            agent_id,
            session_id,
            command,
            prompt_id,
        }];
    }

    agent.session.enqueue_bash_command(command.clone());
    agent.prompt.set_text("");

    let drain = maybe_drain_queue(agent);
    note_peek_page_flip(app, id, drain.page_flip_entry);
    drain.effects
}

/// Whether a load-result handler must stand down because a reconnect reload
/// window is open on the agent.
///
/// The window owns the agent's batch / `loading_replay` / turn state; a load
/// result resolving mid-window (a stale fresh-view load, or `/resume` racing a
/// reconnect) must not close it — flipping `loading_replay` would make the
/// replay gate drop the rest of the reconnect replay, and a failure block
/// would be pushed into staging state. The window finalize supersedes the
/// result.
pub(super) fn defer_to_open_reload_window(
    agent: &AgentView,
    agent_id: AgentId,
    result: &str,
) -> bool {
    if agent.session_reload.is_none() {
        return false;
    }
    tracing::warn!(
        agent = ?agent_id,
        result,
        "load result during an open reload window — deferring to the window finalize"
    );
    true
}

/// The initiation-side counterpart of [`defer_to_open_reload_window`]: a load
/// INITIATION that takes over the agent (fork/worktree-fork/remote-restore
/// binding a session) finalizes any open reload window as failed first, so
/// the new load owns the agent's batch/replay state and its results are not
/// deferred. Unreachable through today's flows (these arms target freshly
/// created `session_id: None` agents, which can never host a window) —
/// defense in depth against future initiation paths on live agents.
pub(super) fn supersede_open_reload_window(
    agent: &mut AgentView,
    agent_id: AgentId,
    initiation: &str,
) {
    if agent.session_reload.is_none() {
        return;
    }
    tracing::warn!(
        agent = ?agent_id,
        initiation,
        "load initiation supersedes an open reload window (finalizing as failed)"
    );
    agent.abort_session_reload();
}

// TaskResult handlers.

pub(super) fn handle_prompt_response(
    app: &mut AppView,
    agent_id: AgentId,
    result: Result<acp::PromptResponse, String>,
    _http_status: Option<u16>,
    prompt_id: Option<String>,
) -> Vec<Effect> {
    // The `agent` borrow lasts only through the pid gate and the shared
    // finalizer; the app-level tail (notifications, adoption handoff, queue
    // drain, suggestion fetch) re-borrows per step.
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    // Discard PromptResponses that don't belong to the currently
    // active prompt -- they belong to a turn the user rewound, or to
    // a queued prompt that never became the running turn.
    //
    // The prompt id comes from two places depending on the arm:
    //   - `Ok`:  the agent echoes `promptId` in PR meta.
    //   - `Err`: an `acp::Error` carries NO meta, so we fall back to
    //            the `prompt_id` the pager minted when it sent this
    //            RPC (threaded through `TaskResult::PromptResponse`).
    //
    // Without the `Err` fallback, a queued prompt's RPC error has no
    // id to gate on and is misattributed to the running turn — e.g.
    // when a queued prompt is removed in leader mode and its
    // `respond_to` is dropped on the leader, the resulting
    // "session failed to respond" error would detonate an unrelated
    // in-flight turn with a spurious "Turn failed" (on the
    // submitter's screen, even when another client did the edit).
    let response_pid = match &result {
        Ok(pr) => pr
            .meta
            .as_ref()
            .and_then(|m| m.get("promptId"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        Err(_) => prompt_id.clone(),
    };
    if let Some(response_pid) = response_pid.as_deref()
        && agent.session.current_prompt_id.as_deref() != Some(response_pid)
    {
        // A late PromptResponse for a turn the durable rail already
        // finalized (the first-wins winner is recorded on the agent):
        // merge the RPC's extra metadata only. No finish, no marker, no
        // drain, no adoption handoff — all of that ran exactly once
        // when the durable terminal won. The merge window is bounded to
        // "the finalized turn is still the current foreground, or the
        // session is idle": once a NEWER turn owns the slot (even as a
        // server-unconfirmed submission), the late response is discarded
        // like any other stale terminal.
        if agent.finalized_prompt.as_deref() == Some(response_pid)
            && (agent.session.current_prompt_id.is_none()
                || agent.session.current_prompt_id.as_deref() == Some(response_pid))
        {
            crate::app::turn_completion::merge_finalized_pr_meta(agent, &result);
            return vec![];
        }
        {
            // Not the running turn: this response must not touch the active turn.
            // Server-authoritative queue lifecycle: this prompt's RPC
            // resolved without becoming the running turn (removed,
            // cancelled, rewound). Retire its optimistic echo so a
            // later `grow/queue/changed` broadcast can't re-pin a
            // stale placeholder and reorder the queue.
            if let Some(sid) = agent.session.session_id.as_ref().map(|s| s.0.to_string()) {
                retire_optimistic_echo(
                    &mut app.optimistic_prompt_echoes,
                    &mut app.shared_prompt_queues,
                    &sid,
                    response_pid,
                );
                agent.shared_queue.retain(|e| e.id != response_pid);
                agent.note_queue_echo_retired(response_pid);
            }
            // Resolved-without-running never adopts; explicit for the
            // session-less arm (no note_queue_echo_retired above).
            return vec![];
        }
    }
    let was_cancelling = agent.session.state.is_cancelling()
        || matches!(
            &result,
            Ok(pr) if pr.stop_reason == acp::StopReason::Cancelled
        );
    // Send-now cancel: suppress the "Turn cancelled by user" marker (the new
    // prompt follows right under the partial). Wire `cancelTrigger` wins, else
    let rate_limited = agent.session.rate_limited;
    let model_incompatible = agent.session.model_incompatible;
    // The RetryState handler already pushed the actionable context-overflow
    // block, so the generic TurnFailed + error toast are redundant.
    let context_overflow = scrollback_has_recent_context_too_large(&agent.scrollback);
    let elapsed = agent.turn_elapsed();

    {
        let sid = agent.session.session_id.as_ref().map(|s| s.0.as_ref());
        let elapsed_ms = elapsed.map(|d| d.as_millis() as u64).unwrap_or(0);
        let ok = result.is_ok();
        crate::unified_log::info(
            "turn.complete",
            sid,
            Some(serde_json::json!({
                "elapsed_ms": elapsed_ms,
                "ok": ok,
                "was_cancelling": was_cancelling,
            })),
        );
    }

    // qtrace: turn end on this client. This clears current_prompt_id
    // and (briefly) returns the client to Idle — the start of the
    // leader-mode turn-end window where a freshly-sent prompt can be
    // wrongly local-drained before the next running-prompt broadcast
    // is adopted.
    tracing::debug!(
        target: "qtrace",
        pid = std::process::id(),
        event = "turn_end",
        prompt_id = prompt_id.as_deref().unwrap_or(""),
        was_cancelling,
        shared_queue_len = agent.shared_queue.len(),
        pending_len = agent.session.pending_prompts.len(),
        session = agent.session.session_id.as_ref().map(|s| s.0.as_ref()).unwrap_or(""),
        "turn ended; client returning to idle",
    );

    // The terminal part converges on the shared first-wins finalizer
    // (turn_completion): it finishes the turn, pushes the marker, runs the
    // full teardown, records the winner, and consumes
    // promptId metadata attributes the response to its exact foreground owner.
    let was_bash_turn = agent.bash_turn;
    let outcome = crate::app::turn_completion::finalize_prompt_terminal(
        agent,
        response_pid.as_deref(),
        crate::app::turn_completion::TerminalMeta {
            pr_ok: result.is_ok(),
            failed_error: result.as_ref().err().cloned(),
            was_cancelling,
            bash_turn: was_bash_turn,
            skip_error_marker: rate_limited || model_incompatible || context_overflow,
            accepts_submitting: true,
        },
    );
    let crate::app::turn_completion::TerminalOutcome {
        apply,
        notification,
    } = outcome;
    // The pid gate above guarantees the finalizer wins on this rail; an
    // Ignored outcome is defensive and must not fall through to the handoff.
    if !matches!(
        apply,
        crate::app::turn_completion::TerminalApply::ViewerFinalized
    ) {
        return vec![];
    }

    if let Err(ref err) = result {
        tracing::error!(agent = ?agent_id, error = %err, "Prompt failed");
    }

    let queue_empty = app
        .agents
        .get(&agent_id)
        .is_some_and(|agent| agent.session.pending_prompts.is_empty());
    crate::app::turn_completion::apply_terminal_notifications(
        app,
        agent_id,
        notification,
        queue_empty,
    );

    // Cancelled turns resume queue processing one item at a time
    // through the same drain path as normal completions.
    // `maybe_drain_queue` keeps the idle-only and editing-front
    // guards so we do not send from under the user.
    if app.reconnect_pending {
        return vec![];
    }

    let drain = {
        let Some(agent) = app.agents.get_mut(&agent_id) else {
            return vec![];
        };
        maybe_drain_queue(agent)
    };
    let page_flip_entry = drain.page_flip_entry;
    let mut effects = drain.effects;

    // Predicted-next-prompt (tab autocomplete): fetch a fresh suggestion
    // (the stale one was wiped by the finalizer) — but only after a clean,
    // non-bash agent turn that leaves the session idle with an empty prompt
    // and no queued work, local or server-side (a draft in progress or a
    // draining queue means the user is already mid-thought). Placed after
    // `maybe_drain_queue` so `is_idle` reflects a locally-drained next
    // turn.
    if crate::views::prompt_suggestion::resolve_enabled()
        && result.is_ok()
        && !was_cancelling
        && !was_bash_turn
        && let Some(agent) = app.agents.get_mut(&agent_id)
        && agent.prompt.text().is_empty()
        && agent.session.pending_prompts.is_empty()
        && agent.shared_queue.is_empty()
        && agent.session.state.is_idle()
        && let Some(session_id) = agent.session.session_id.as_ref().map(|s| s.0.to_string())
    {
        let generation = agent.prompt.prompt_suggestion.begin_fetch();
        let model = crate::views::prompt_suggestion::resolve_model(&agent.session.models);
        effects.push(Effect::FetchPromptSuggestion {
            agent_id,
            generation,
            model,
            session_id: Some(session_id),
        });
    }

    note_peek_page_flip(app, agent_id, page_flip_entry);
    effects
}

pub(super) fn handle_compact_complete(
    app: &mut AppView,
    agent_id: AgentId,
    track_foreground: bool,
    result: Result<crate::app::actions::CompactRequestStatus, String>,
) -> Vec<Effect> {
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        if !track_foreground {
            let message = match result {
                Ok(crate::app::actions::CompactRequestStatus::Scheduled) => {
                    "Compaction scheduled after the current turn.".to_string()
                }
                Ok(crate::app::actions::CompactRequestStatus::AlreadyRunning) => {
                    "Compaction is already pending or running.".to_string()
                }
                Ok(crate::app::actions::CompactRequestStatus::Completed) => {
                    "Conversation compacted.".to_string()
                }
                Err(error) => format!("Compaction request failed: {error}"),
            };
            agent.scrollback.push_block(RenderBlock::system(message));
            return vec![];
        }
        // Defensive: only process if we're still in a compact command state.
        let was_cancelling = matches!(
            agent.session.state,
            AgentState::CommandCancelling {
                command: AgentCommand::Compact,
            }
        );
        if !matches!(
            agent.session.state,
            AgentState::CommandRunning {
                command: AgentCommand::Compact,
                ..
            } | AgentState::CommandCancelling {
                command: AgentCommand::Compact,
            }
        ) {
            tracing::debug!("Ignoring CompactComplete (not in compact command state)");
            return vec![];
        }

        let elapsed = agent.turn_elapsed();
        agent.session.finish_command();

        match &result {
            Ok(crate::app::actions::CompactRequestStatus::Completed) => {
                agent.scrollback.push_block(RenderBlock::session_event(
                    SessionEvent::CompactCompleted {
                        elapsed: elapsed.unwrap_or_default(),
                    },
                ));
            }
            Ok(crate::app::actions::CompactRequestStatus::Scheduled) => {
                agent.scrollback.push_block(RenderBlock::system(
                    "Compaction scheduled after the current turn.".to_string(),
                ));
            }
            Ok(crate::app::actions::CompactRequestStatus::AlreadyRunning) => {
                agent.scrollback.push_block(RenderBlock::system(
                    "Compaction is already pending or running.".to_string(),
                ));
            }
            Err(err) if was_cancelling || err.contains("compact cancelled") => {
                agent.scrollback.push_block(RenderBlock::session_event(
                    SessionEvent::CompactionCancelled,
                ));
            }
            Err(err) => {
                tracing::error!(agent = ?agent_id, error = %err, "Compaction failed");
                agent.scrollback.push_block(RenderBlock::session_event(
                    SessionEvent::CompactionFailed {
                        error: String::new(),
                    },
                ));
            }
        }

        agent.mark_turn_finished();
        agent.activity_started_at = None;
        agent.last_activity = None;

        if app.reconnect_pending {
            return vec![];
        }
        let drain = maybe_drain_queue(agent);
        note_peek_page_flip(app, agent_id, drain.page_flip_entry);
        return drain.effects;
    }
    vec![]
}

pub(super) fn handle_suggestion_debounce_expired(
    app: &mut AppView,
    agent_id: AgentId,
    generation: u64,
) -> Vec<Effect> {
    // Route by the arming agent (the timer carries it), not the active
    // view: a view switch inside the debounce window must neither fire a
    // spurious fetch on another agent nor drop this one's.
    let Some(agent) = app.agents.get(&agent_id) else {
        return vec![];
    };
    // Bash-mode feature: a debounce that outlives the mode fetches nothing.
    if agent.prompt_input_mode != crate::app::agent_view::PromptInputMode::Bash {
        return vec![];
    }
    if !agent.prompt.suggestions.on_debounce_expired(generation) {
        return vec![];
    }
    let text = agent.prompt.text().to_owned();
    let cursor = agent.prompt.cursor();
    let cwd = agent.session.cwd.to_string_lossy().into_owned();
    let include_ai = agent.prompt.suggestions.ai_enabled;
    let ai_model = agent.prompt.suggestions.ai_model.clone();
    let session_id = agent.session.session_id.as_ref().map(|s| s.0.to_string());
    vec![Effect::FetchShellSuggestions {
        agent_id,
        text,
        cursor,
        cwd,
        generation,
        limit: crate::views::suggestion_controller::SHELL_SUGGEST_WIRE_LIMIT,
        include_ai,
        ai_model,
        session_id,
        // The as-you-type (ghost) surface keeps history/AI providers.
        token_only: false,
    }]
}

//! Fork and project-selection dispatchers and fork placeholder builders.
use super::lifecycle::dispatch_new_session_inner_with_id;
use crate::app::actions::Effect;
use crate::app::agent_view::AgentView;
use crate::app::root::dispatch::ctx::{SwitchCause, switch_to_agent};
use crate::app::root::dispatch::modes::inherit_permission_mode;
use crate::app::root::dispatch::prompt::supersede_open_reload_window;
use crate::app::root::{ActiveView, AppView};
use crate::app::session::{AgentCommand, AgentId, AgentSession};
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::SessionEvent;
use crate::scrollback::state::ScrollbackState;
use agent_client_protocol as acp;
use std::time::Instant;
/// Top-level `/fork` dispatcher. Resolves the worktree decision: an
/// explicit `--worktree` / `--no-worktree` flag short-circuits to
/// [`dispatch_fork_resolved`]. When no flag is given and a persisted
/// `fork_worktree_mode` preference is set (`Always` / `Never`), the
/// popup is skipped and the corresponding path is taken directly. The
/// `Ask` default opens the [`open_fork_question`] modal so the user is
/// asked.
///
/// When the parent session's working directory is **not** inside a git
/// repository (indicated by the absence of a `git_head_changed`
/// notification — `current_branch` is `None`):
/// - `--worktree` is rejected with a toast (nothing to create a worktree from).
/// - No flag (regardless of `fork_worktree_mode`): the worktree question
///   is skipped and the fork proceeds with `worktree = false`.
///
/// Note: if the notification has not arrived yet (rare — user forks
/// before the shell sends `git_head_changed`), the fallback to
/// `worktree = false` is safe and the worktree can be created manually
/// afterwards.
///
/// Two failure surfaces:
/// - Active view is not an agent: toast and return.
/// - Active agent has no `session_id` (still being created): toast and
///   return. Both rejections are deliberate -- queueing the fork until
///   `SessionLoaded` would require persisting `ForkArgs` across the
///   `TaskResult` and is deferred to v2.
pub(in crate::app::root::dispatch) fn dispatch_fork(
    app: &mut AppView,
    args: crate::slash::commands::fork::ForkArgs,
) -> Vec<Effect> {
    let ActiveView::Agent(parent_id) = app.active_view else {
        app.show_toast("/fork only works inside a session");
        return vec![];
    };
    let (has_session, in_git_repo) = app
        .agents
        .get(&parent_id)
        .map(|a| (a.session.session_id.is_some(), a.current_branch.is_some()))
        .unwrap_or((false, false));
    if !has_session {
        app.show_toast("Cannot fork: session is still being created");
        return vec![];
    }
    match args.worktree_override {
        Some(true) if !in_git_repo => {
            app.show_toast("Cannot create worktree: not in a git repository");
            vec![]
        }
        Some(worktree) => dispatch_fork_resolved(app, worktree, args.directive),
        None => {
            if in_git_repo {
                use crate::app::root::WorktreeMode;
                match app.fork_worktree_mode {
                    WorktreeMode::Always => dispatch_fork_resolved(app, true, args.directive),
                    WorktreeMode::Never => dispatch_fork_resolved(app, false, args.directive),
                    WorktreeMode::Ask => open_fork_question(app, args.directive),
                }
            } else {
                dispatch_fork_resolved(app, false, args.directive)
            }
        }
    }
}
/// If `persist_mode` is `Some`, write `mode` into `*field` and append
/// a [`Effect::PersistWorktreeMode`] to `effects` with the given
/// `config_key`.
pub(in crate::app::root::dispatch) fn apply_persist_worktree_mode(
    field: &mut crate::app::root::WorktreeMode,
    effects: &mut Vec<Effect>,
    persist_mode: Option<crate::app::root::WorktreeMode>,
    config_key: &'static str,
) {
    if let Some(mode) = persist_mode {
        *field = mode;
        effects.push(Effect::PersistWorktreeMode { mode, config_key });
    }
}
/// Build the two persistence options shared by the fork and new-session
/// worktree question modals ("Always worktree" / "Never worktree").
pub(super) fn worktree_persist_options()
-> [tools::implementations::grow_build::ask_user_question::QuestionOption; 2] {
    use tools::implementations::grow_build::ask_user_question::QuestionOption;
    [
        QuestionOption {
            label: "Always worktree".into(),
            description: "Use worktree and stop asking (reset in config.toml)".into(),
            preview: None,
            id: None,
        },
        QuestionOption {
            label: "Never worktree".into(),
            description: "Skip worktree and stop asking (reset in config.toml)".into(),
            preview: None,
            id: None,
        },
    ]
}
/// Open the local worktree question modal on the active agent. Refuses
/// if a question (ACP or local) is already on screen, surfacing a toast
/// instead -- the modal-collision protocol.
fn open_fork_question(app: &mut AppView, directive: Option<String>) -> Vec<Effect> {
    use crate::views::question_view::{LocalQuestionKind, QuestionViewState};
    use tools::implementations::grow_build::ask_user_question::{Question, QuestionOption};
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    if agent.question_view.is_some() {
        app.show_toast("Finish answering the current question first");
        return vec![];
    }
    let mut options = vec![
        QuestionOption {
            label: "Yes".into(),
            description: "Fork in a new isolated git worktree".into(),
            preview: None,
            id: None,
        },
        QuestionOption {
            label: "No".into(),
            description: "Fork in the current cwd".into(),
            preview: None,
            id: None,
        },
    ];
    options.extend(worktree_persist_options());
    let question = Question {
        question: "Run this fork in an isolated git worktree?".into(),
        id: None,
        options,
        multi_select: Some(false),
    };
    let agent = app.agents.get_mut(&id).expect("agent present (re-borrow)");
    let stashed = agent.prompt.stash();
    let state = QuestionViewState::new(
        format!("fork-{}", uuid::Uuid::new_v4()),
        vec![question],
        stashed,
    )
    .with_local_kind(LocalQuestionKind::Fork { directive });
    agent.replace_question_view(Some(state));
    agent.prompt.set_text("");
    vec![]
}
/// Construct the placeholder agent, push discoverability markers, flip
/// the discovery gate, switch to the new agent, and emit the appropriate
/// fork effect (worktree or no-worktree path).
///
/// `worktree == true` reuses the existing
/// [`Effect::CreateWorktreeSession`] pipeline (with `load_session_id`
/// set to the parent session id). `worktree == false` emits the new
/// [`Effect::ForkSession`] which calls `grow/session/fork` directly.
pub(in crate::app::root::dispatch) fn dispatch_fork_resolved(
    app: &mut AppView,
    worktree: bool,
    directive: Option<String>,
) -> Vec<Effect> {
    let ActiveView::Agent(parent_id) = app.active_view else {
        return vec![];
    };
    let Some(parent) = app.agents.get(&parent_id) else {
        return vec![];
    };
    let Some(parent_session_id) = parent.session.session_id.clone() else {
        app.show_toast("Cannot fork: session not yet created");
        return vec![];
    };
    let parent_cwd = parent.session.cwd.clone();
    let parent_is_worktree = parent.session.is_worktree;
    let new_id = AgentId(app.next_agent_id);
    app.next_agent_id += 1;
    let new_agent = build_fork_placeholder(app, new_id, parent_id, &parent_cwd, worktree);
    let parent_marker = match directive.as_deref() {
        Some(d) => format!("Forked: {d}"),
        None => "Forked".to_string(),
    };
    app.agents.insert(new_id, new_agent);
    {
        let agent = app
            .agents
            .get_mut(&new_id)
            .expect("just-inserted agent missing");
        agent.prompt.set_compact(app.appearance.prompt.compact);
        agent.prompt.adopt_slash_mru(app.slash_mru.clone());
        agent.prompt.adopt_command_tags(app.command_tags.clone());
        agent
            .prompt
            .set_contextual_hints(app.contextual_hints.undo, app.contextual_hints.plan_mode);
        agent.set_session_recap_available(app.session_recap_available);

        agent.apply_app_scoped_gates(app.screen_mode, &app.active_announcements);

        agent
            .prompt
            .slash_controller
            .registry_mut()
            .set_plugins_visible(!app.appearance.disable_plugins);
        agent.pending_fork_banner = Some(crate::app::agent_view::PendingForkBanner {
            parent_sid: parent_session_id.0.to_string(),
            worktree,
        });
        if worktree {
            agent.session.set_live_feedback(
                "worktree",
                crate::scrollback::blocks::NoticeTone::Progress,
                "Creating worktree\u{2026}",
            );
        }
        agent.pending_first_prompt = directive;
    }
    if let Some(parent_mut) = app.agents.get_mut(&parent_id) {
        parent_mut
            .scrollback
            .push_block(RenderBlock::notice(parent_marker));
    }
    switch_to_agent(app, new_id, SwitchCause::Fork);
    if let Some(d) = app.dashboard.as_mut()
        && d.attached_agent == Some(parent_id)
    {
        d.attached_agent = Some(new_id);
        d.focus_row(crate::views::dashboard::DashboardRowId::TopLevel(new_id));
    }
    if worktree {
        vec![Effect::CreateWorktreeSession {
            agent_id: new_id,
            load_session_id: Some(parent_session_id.0.to_string()),
            label: None,
            git_ref: None,
            // Fork resumes the parent session, which carries its own model.
            model_id: None,
            preferred_session_id: None,
        }]
    } else {
        vec![Effect::ForkSession {
            agent_id: new_id,
            parent_session_id,
            parent_cwd,
            parent_is_worktree,
            new_session_id: None,
        }]
    }
}
pub(in crate::app::root::dispatch) fn open_project_question(
    app: &mut AppView,
    prompt_text: String,
) -> Vec<Effect> {
    open_project_question_with_context(app, prompt_text, None, true, false, false)
}

/// Open the project picker while parking all create-time context on the
/// placeholder AgentView. The plain wrapper above is used by normal prompt
/// submission; dashboard dispatches pass model/routing explicitly.
pub(in crate::app::root::dispatch) fn open_project_question_with_context(
    app: &mut AppView,
    prompt_text: String,
    model_id: Option<acp::ModelId>,
    submit_prompt: bool,
    return_to_dashboard: bool,
    attach_to_dashboard: bool,
) -> Vec<Effect> {
    use crate::views::question_view::{LocalQuestionKind, QuestionViewState};
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    // A bound session (or a session whose MCP init has already started) owns
    // its lifecycle. A stale picker flag/event must not create a second
    // pending placeholder or session.
    if agent.session.session_id.is_some()
        || agent.session.mcp_init_progress().is_some()
        || (agent.pending_project_create.is_some() && agent.question_view.is_none())
    {
        return vec![];
    }
    if let Some(question) = agent.question_view.as_mut() {
        if let Some(LocalQuestionKind::ProjectSelect { stashed_prompt, .. }) =
            question.local_kind.as_mut()
        {
            *stashed_prompt = prompt_text.clone();
            let mut stash = std::mem::take(&mut question.stashed_prompt);
            if stash.text != prompt_text {
                stash = if stash.text.is_empty() {
                    crate::views::prompt_widget::StashedPrompt::from_submission(
                        prompt_text,
                        Vec::new(),
                        Vec::new(),
                    )
                } else {
                    stash.with_transformed_text(prompt_text)
                };
            }
            question.stashed_prompt = stash;
        }
        if submit_prompt && let Some(pending) = agent.pending_project_create.as_mut() {
            pending.submit_prompt = true;
        }
        return vec![];
    }
    // Show the current-directory choice immediately. Recent directories are
    // only an enhancement and are loaded by an async effect below; the
    // pending create capability is installed before that effect can return.
    let pq = crate::project_picker::build_project_question(&[], &app.cwd);
    let prompt_fallback = prompt_text.clone();
    let mut stashed = agent.prompt.stash();
    if stashed.text.is_empty() && !prompt_text.is_empty() {
        stashed = crate::views::prompt_widget::StashedPrompt::from_submission(
            prompt_text,
            Vec::new(),
            Vec::new(),
        );
    }
    let state = QuestionViewState::new(
        format!("project-select-{}", uuid::Uuid::new_v4()),
        vec![pq.question],
        stashed,
    )
    .with_local_kind(LocalQuestionKind::ProjectSelect {
        resolved_paths: pq.resolved_paths,
        original_cwd: app.cwd.clone(),
        stashed_prompt: prompt_fallback,
        dont_ask_index: pq.dont_ask_index,
    });
    let picker_id = state.tool_call_id.clone();
    agent.pending_project_create = Some(crate::app::agent_view::PendingProjectCreate {
        model_id,
        prompt: None,
        submit_prompt,
        return_to_dashboard,
        attach_to_dashboard,
    });
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    agent.replace_question_view(Some(state));
    agent.prompt.set_text("");
    crate::unified_log::info("project_picker.opened", None, None);
    vec![Effect::FetchProjectPickerRecents {
        agent_id: id,
        picker_id,
    }]
}

/// Apply asynchronously loaded recent directories only while the same
/// untouched project picker still owns the pending create capability.
pub(in crate::app::root::dispatch) fn handle_project_picker_recents_loaded(
    app: &mut AppView,
    agent_id: AgentId,
    picker_id: String,
    recent_dirs: Vec<(std::path::PathBuf, chrono::DateTime<chrono::Utc>)>,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if agent.session.session_id.is_some()
        || agent.session.mcp_init_progress().is_some()
        || agent.pending_project_create.is_none()
    {
        return vec![];
    }
    let Some(question) = agent.question_view.as_mut() else {
        return vec![];
    };
    let untouched = question.tool_call_id == picker_id
        && question.active_tab == 0
        && matches!(
            question.focus,
            crate::views::question_view::QuestionFocus::Navigation
        )
        && question.cursor() == 0
        && question.per_question_freeform.iter().all(String::is_empty)
        && question
            .per_question_freeform_selected
            .iter()
            .all(|selected| !selected)
        && question.selections.first().is_some_and(|selection| {
            matches!(
                selection,
                crate::views::question_view::QuestionSelection::Single(None)
            )
        });
    if !untouched {
        return vec![];
    }
    let Some(crate::views::question_view::LocalQuestionKind::ProjectSelect {
        resolved_paths,
        original_cwd,
        dont_ask_index,
        ..
    }) = question.local_kind.as_mut()
    else {
        return vec![];
    };
    let pq = crate::project_picker::build_project_question(&recent_dirs, original_cwd);
    if let Some(current_question) = question.questions.first_mut() {
        *current_question = pq.question;
    }
    *resolved_paths = pq.resolved_paths;
    *dont_ask_index = pq.dont_ask_index;
    vec![]
}
pub(in crate::app::root::dispatch) fn dispatch_project_selected(
    app: &mut AppView,
    path: std::path::PathBuf,
    stashed_prompt: String,
    disable_picker: bool,
) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    // `pending_project_create` is the one-shot create capability. Validate
    // and consume it before touching cwd, picker preferences, or effects so
    // duplicate/late selections are pure no-ops.
    let pending = {
        let Some(agent) = app.agents.get_mut(&id) else {
            return vec![];
        };
        if agent.session.session_id.is_some() || agent.session.mcp_init_progress().is_some() {
            return vec![];
        }
        agent.pending_project_create.take()
    };
    let Some(pending) = pending else {
        return vec![];
    };
    crate::unified_log::info(
        "project_picker.selected",
        None,
        Some(
            serde_json::json!({"path": path.display().to_string(), "prompt_len": stashed_prompt.len(), "disable_picker": disable_picker}),
        ),
    );
    app.mark_project_picker_done();
    let mut effects = Vec::new();
    if disable_picker {
        app.project_picker_disabled = true;
        app.show_toast("Won't ask about project directory again (reset in config.toml)");
        effects.push(Effect::PersistProjectPickerDisabled { disabled: true });
    }
    let path = if path.is_dir() {
        path
    } else {
        app.show_toast("Directory not found, continuing in current directory");
        app.cwd.clone()
    };
    super::super::dashboard::commit_cwd_snapshot(app, &path);
    effects.push(Effect::SetWorkingDir { path: path.clone() });
    let (model_id, prompt_stash, submit_prompt, return_to_dashboard, attach_to_dashboard) = (
        pending.model_id,
        pending.prompt,
        pending.submit_prompt,
        pending.return_to_dashboard,
        pending.attach_to_dashboard,
    );
    if let Some(agent) = app.agents.get_mut(&id) {
        let changed = agent.session.cwd != path;
        agent.session.cwd = path.clone();
        if changed {
            let display = crate::project_picker::sources::display_path(&path);
            agent.show_toast(&format!("Updated working directory to {display}"));
        }
    }
    if let Some(agent) = app.agents.get_mut(&id) {
        agent.session.update_mcp_init_progress(0, 0);
        agent.session.prompt_history_loading = true;
    }
    let mut prompt_effect = None;
    if let Some(agent) = app.agents.get_mut(&id) {
        let fallback_is_submission = prompt_stash.is_none() && !stashed_prompt.trim().is_empty();
        let prompt_stash = prompt_stash.unwrap_or_else(|| {
            crate::views::prompt_widget::StashedPrompt::from_submission(
                stashed_prompt,
                Vec::new(),
                Vec::new(),
            )
        });
        if submit_prompt || fallback_is_submission {
            let (prompt_text, mut images, chip_elements) = prompt_stash.into_submission();
            if !prompt_text.trim().is_empty() || !images.is_empty() {
                agent.session.enqueue_prompt(prompt_text);
                if let Some(entry) = agent.session.pending_prompts.back_mut() {
                    entry.images = std::mem::take(&mut images);
                    entry.chip_elements = chip_elements;
                }
            }
            crate::prompt_images::drain_and_cleanup(&mut images);
        } else {
            agent.prompt.restore(prompt_stash);
        }
        prompt_effect = Some((return_to_dashboard, attach_to_dashboard));
    }
    let preferred_session_id = app.deferred_startup.preferred_session_id.take();
    effects.push(Effect::CreateSession {
        agent_id: id,
        cwd: path,
        model_id,
        preferred_session_id,
    });
    if let Some((return_to_dashboard, attach_to_dashboard)) = prompt_effect
        && return_to_dashboard
    {
        if let Some(d) = app.dashboard.as_mut() {
            d.restore_peek_viewport(&mut app.agents);
            d.dispatch.set_text("");
            d.clear_feedback();
            d.filter = crate::views::dashboard::Filter::None;
            d.focus_row(crate::views::dashboard::DashboardRowId::TopLevel(id));
            d.attached_agent = attach_to_dashboard.then_some(id);
        }
        if !attach_to_dashboard {
            app.active_view = ActiveView::AgentDashboard;
        }
    }
    effects
}
/// Build the placeholder [`AgentView`] for a fork. Centralises the
/// `AgentSession`/spinner construction shared by both worktree and
/// no-worktree branches so the parallel struct literal does not drift.
fn build_fork_placeholder(
    app: &AppView,
    new_id: AgentId,
    parent_id: AgentId,
    parent_cwd: &std::path::Path,
    worktree: bool,
) -> AgentView {
    let mut scrollback = ScrollbackState::new();
    scrollback.set_appearance(app.appearance.clone());
    let mut agent = AgentView::new(
        {
            let mut session = AgentSession::new(
                new_id,
                app.acp_tx.clone(),
                None,
                app.models.clone(),
                parent_cwd.to_path_buf(),
                inherit_permission_mode(app),
            );
            session.mark_forked_from(parent_id);
            session.available_commands = app.bootstrap_acp_commands.clone();
            session.available_commands_generation = 1;
            session.deferred_model_switch = app.deferred_model_switch_from_cli();
            session
        },
        scrollback,
    );
    let cmd = if worktree {
        AgentCommand::CreateWorktree
    } else {
        AgentCommand::ForkSession
    };
    agent.session.start_command(cmd);
    agent.session.turn_started_at = Some(Instant::now());
    agent
}
/// Build the discoverability banner for the child agent. Includes the
/// child's session id, the full parent session id, and — when
/// `switch_hint` names a command (the caller's
/// [`crate::views::dashboard::session_switch_hint_command`]: `/agents`
/// normally, `/resume` in minimal mode where the dashboard is refused) —
/// a session-switch tip so the user knows how to switch back. No-worktree
/// case appends the dim continuation `(both agents share cwd)`.
///
/// Called in `TaskResult::SessionLoaded` (not at dispatch time) because
/// the child's session id is not known until the backend responds.
pub(in crate::app::root::dispatch) fn build_child_fork_marker(
    session_id: &str,
    parent_sid: &str,
    worktree: bool,
    switch_hint: Option<&str>,
) -> String {
    let header = if let Some(cmd) = switch_hint {
        format!(
            "Session {session_id} (forked from {parent_sid}) \u{2014} use {cmd} to switch between sessions",
        )
    } else {
        format!("Session {session_id} (forked from {parent_sid})")
    };
    if worktree {
        header
    } else {
        format!("{header}\n  (both agents share cwd)")
    }
}
pub(in crate::app::root::dispatch) fn dispatch_startup_fork_session(
    app: &mut AppView,
    parent_session_id: String,
    parent_cwd: Option<std::path::PathBuf>,
    new_session_id: Option<String>,
) -> Vec<Effect> {
    if !app.session_startup_allowed() {
        app.deferred_startup.session =
            Some(crate::app::session_startup::DeferredSessionStartup::Fork {
                parent_session_id,
                parent_cwd,
                new_session_id,
            });
        return vec![];
    }
    let (_agent_id, mut effects) = dispatch_new_session_inner_with_id(app, None);
    let agent_id = app
        .agents
        .keys()
        .next_back()
        .copied()
        .expect("fork placeholder agent");
    effects.retain(|e| !matches!(e, Effect::CreateSession { .. }));
    let cwd = parent_cwd.unwrap_or_else(|| app.cwd.clone());
    let parent_is_worktree =
        crate::app::session_startup::parent_session_is_worktree(&parent_session_id, &cwd);
    effects.push(Effect::ForkSession {
        agent_id,
        parent_session_id: acp::SessionId::new(parent_session_id),
        parent_cwd: cwd,
        parent_is_worktree,
        new_session_id,
    });
    effects
}
#[allow(clippy::too_many_arguments)]
pub(in crate::app::root::dispatch) fn handle_worktree_forked(
    app: &mut AppView,
    agent_id: AgentId,
    session_id: acp::SessionId,
    worktree_path: std::path::PathBuf,
    session_cwd: std::path::PathBuf,
    code_restored: bool,
    restore_summary: Option<String>,
    restore_degree: Option<workspace::session::git::RestoreDegree>,
) -> Vec<Effect> {
    let session_id_str = session_id.0.to_string();
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        agent.session.clear_live_feedback("worktree");
        supersede_open_reload_window(agent, agent_id, "WorktreeForked");
        agent.session.finish_command();
        agent.mark_turn_finished();
        agent.bind_session_id(session_id);
        agent.scrollback.begin_batch();
        agent.begin_replay_window();
        agent.session.restore_degree = restore_degree;
        agent.session.cwd = session_cwd.clone();
        agent.session.is_worktree = true;
        app.restore_code = None;
        agent.prompt.file_search.retarget(&session_cwd);
        agent.scrollback.push_block(RenderBlock::notice(format!(
            "Worktree ready: {}",
            worktree_path.display()
        )));
        match (code_restored, restore_summary.as_deref()) {
            (true, Some(s)) => {
                agent
                    .scrollback
                    .push_block(RenderBlock::notice(format!("\u{2713} Code restored: {s}")));
            }
            (false, Some(s)) => {
                agent.scrollback.push_block(RenderBlock::notice(format!(
                    "\u{26A0} Code restore failed: {s}"
                )));
            }
            _ => {}
        }
        return vec![Effect::LoadSession {
            agent_id,
            session_id: session_id_str,
            session_cwd: Some(session_cwd),
        }];
    }
    vec![]
}
pub(in crate::app::root::dispatch) fn handle_fork_session_ready(
    app: &mut AppView,
    agent_id: AgentId,
    new_session_id: acp::SessionId,
    cwd: std::path::PathBuf,
) -> Vec<Effect> {
    let session_id_str = new_session_id.0.to_string();
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        agent.session.clear_live_feedback("worktree");
        supersede_open_reload_window(agent, agent_id, "ForkSessionReady");
        agent.session.finish_command();
        agent.mark_turn_finished();
        agent.bind_session_id(new_session_id);
        agent.scrollback.begin_batch();
        agent.begin_replay_window();
        agent.session.cwd = cwd.clone();
        agent.prompt.file_search.retarget(&cwd);
        return vec![Effect::LoadSession {
            agent_id,
            session_id: session_id_str,
            session_cwd: Some(cwd),
        }];
    }
    vec![]
}
pub(in crate::app::root::dispatch) fn handle_fork_session_failed(
    app: &mut AppView,
    agent_id: AgentId,
    error: String,
) -> Vec<Effect> {
    tracing::error!(agent = ?agent_id, error = %error, "Fork session failed");
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        agent.session.clear_live_feedback("worktree");
        agent.session.clear_pending_extensions_fetch();
        agent.session.finish_command();
        let elapsed = agent.turn_elapsed();
        agent.mark_turn_finished();
        agent.pending_first_prompt = None;
        agent.pending_fork_banner = None;
        agent
            .scrollback
            .push_block(RenderBlock::session_event(SessionEvent::TurnFailed {
                error,
                elapsed,
            }));
    }
    vec![]
}

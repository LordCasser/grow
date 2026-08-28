//! Tests for the dispatch module tree: shared fixtures and per-domain test modules.
mod cta_e2e;
mod dashboard;
mod jump;
mod modes;
mod notes;
mod permissions;
mod rewind;
mod router;
mod session;
mod settings;
mod status;
mod task_result;
mod transcript;
mod turn;
mod turn_pipeline;
use super::cta::{
    CTA_MCP_ABSENT_MAX_ATTEMPTS, CTA_MCP_POLL_MAX_ATTEMPTS, cta_impression_plugin_name,
    cta_install_error_category, cta_install_relative_path, plugin_cta_phase_for,
};
use super::ctx::{find_agent_by_session_id, get_active_agent, get_active_agent_mut};
use super::dashboard::{
    apply_pending_dispatch_config, dispatch_dashboard_attach, dispatch_dashboard_begin_rename,
    dispatch_dashboard_commit_rename, dispatch_dashboard_confirm_worktree,
    dispatch_dashboard_create_new_agent_with_detail, dispatch_dashboard_delete,
    dispatch_dashboard_dispatch, dispatch_dashboard_dispatch_slash,
    dispatch_dashboard_overlay_cycle, dispatch_dashboard_overlay_exit,
    dispatch_dashboard_overlay_stop, dispatch_dashboard_peek_reply,
    dispatch_dashboard_permission_followup, dispatch_dashboard_permission_select,
    dispatch_dashboard_question_answer, dispatch_dashboard_stop, dispatch_exit_dashboard,
    dispatch_open_dashboard, ensure_dashboard_state, resolve_location_input,
};
use super::modes::{ALWAYS_APPROVE_ON_UNDER_PLAN_TOAST, permission_mode_toast};
use super::permissions::drain_root_permission_queue;
use super::session::fork::build_child_fork_marker;
use super::session::lifecycle::{dispatch_new_session_inner, drain_startup_actions, finish_trust};
use super::session::load::reanchor_grouped_selection;
use super::session::modal::{dispatch_rename_session, dispatch_sessions_confirm_close};
use super::settings::setters::set_default_model_inner;
use super::settings::ui::{action_for_reset, apply_setting_rollback};
use super::task_result::dispatch_task_result;
use super::*;
use crate::acp::model_state::ModelState;
use crate::app::actions::{
    Action, ControlRequestFailure, ControlRpcOutcome, Effect, PermissionModeKind,
    SubagentKillOutcome, TaskResult,
};
use crate::app::agent_view::{ActivePane, AgentView, PromptMode};
use crate::app::root::{
    ActiveView, AppView, PasteProvenance, TrustState, WelcomeAnnouncementState,
};
use crate::app::session::{AgentId, AgentSession, AgentState};
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::{SessionEvent, ToolCallBlock};
use crate::scrollback::state::ScrollbackState;
use agent_client_protocol as acp;
use indexmap::IndexMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

fn control_rpc_accepted() -> Result<ControlRpcOutcome, ControlRequestFailure> {
    Ok(ControlRpcOutcome::AuthoritativeUpdatePending)
}

fn local_control_failure(message: impl Into<String>) -> ControlRequestFailure {
    ControlRequestFailure {
        message: message.into(),
        terminal_published: false,
    }
}
fn test_app() -> AppView {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    AppView {
        motion_origin: Instant::now(),
        active_view: ActiveView::Welcome,
        agents: IndexMap::new(),
        next_agent_id: 0,
        models: ModelState::default(),
        registry: crate::actions::ActionRegistry::defaults(),
        settings_registry: std::sync::Arc::new(crate::settings::SettingsRegistry::defaults()),
        current_ui: shell::agent::config::UiConfig::default(),
        cwd: PathBuf::from("/tmp"),
        project_picker_shown: true,
        project_picker_disabled: false,
        cwd_has_git_ancestor: false,
        acp_tx: tx,
        scratch: crate::scrollback::render::ScratchBuffer::new(),
        cursor: crate::render::draw::CursorState::new(),
        pending_action: None,
        exit_session_pending: None,
        scroll_state: crate::input::mouse::MouseScrollState::default(),
        scroll_config: crate::input::mouse::ScrollConfig::default(),
        appearance: crate::appearance::AppearanceConfig::default(),
        notification_service: crate::notifications::NotificationService::new(Default::default()),
        pending_notification_escapes: None,
        deferred_notification: None,
        active_announcements: vec![],
        hidden_announcement_ids: Default::default(),
        announcement: None,
        changelog_markdown: None,
        changelog_bullets: Vec::new(),
        tips: Vec::new(),
        tip: None,
        cli_model_override: None,
        cli_effort_token: None,
        default_permission_mode: shell::util::config::PermissionMode::Ask,
        permission_mode_from_soft_default: true,
        auto_mode_gate: true,
        always_approve_policy_block: None,
        always_approve_launch_block_notice: None,
        screen_mode_switch_hint: None,
        require_plan_approval: false,
        plan_mode: false,
        subagents: false,
        ask_user: false,
        mouse_captured: true,
        new_worktree_dialog: None,
        contextual_hints: Default::default(),
        remote_contextual_hints: None,
        tip_seen_counts: Default::default(),
        last_known_terminal_rows: 0,
        small_screen_tip_evaluated: false,
        ssh_wrap_tip_evaluated: false,
        clipboard_focus_tip: Default::default(),
        new_session_worktree_mode: crate::app::root::WorktreeMode::Never,
        fork_worktree_mode: crate::app::root::WorktreeMode::Ask,
        restore_code: None,
        resume_local_miss: None,
        agent_override: None,
        bootstrap_acp_commands: Vec::new(),
        trust_state: TrustState::Done,
        deferred_startup: Default::default(),
        show_tips: None,
        auto_update: None,
        ask_user_question_timeout_enabled: None,
        bundle_state: crate::app::bundle::BundleState::default(),
        scroll_debug_hud: crate::views::scroll_debug_hud::ScrollDebugHud::new(),
        fps_hud: crate::views::fps_hud::FpsHud::new(),
        slash_mru: std::rc::Rc::new(std::cell::RefCell::new(
            crate::slash::mru::SlashMru::new_in_memory(),
        )),
        command_tags: std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new())),
        welcome_menu_index: None,
        welcome_menu_rects: Vec::new(),
        last_mouse_pos: None,
        last_scroll_pos: None,
        last_cache_evict_at: None,
        welcome_announcement: WelcomeAnnouncementState::default(),
        welcome_toast: None,
        welcome_on_promo_cta: false,
        welcome_promo_cta_rect: None,
        session_picker_entries: None,
        session_picker_loading: false,
        session_picker_state: crate::views::picker::PickerState::with_mode(
            crate::views::picker::PickerMode::FullScreen,
        ),
        session_picker_relaxed_notified_for: None,
        session_picker_content_results: None,
        session_picker_content_loading: false,
        session_picker_deep_search_seq: 0,
        session_picker_list_seq: 0,
        session_picker_detail_generation: 0,
        session_picker_entries_query: None,
        startup_warnings: Vec::new(),
        pending_update_version: None,
        restart_for_update: false,
        relaunch: None,
        screen_mode_control_handoffs: Default::default(),
        screen_mode: crate::app::ScreenMode::Inline,
        pending_effects: Vec::new(),
        pending_editor: None,
        pending_pager_path: None,
        pending_pager_ansi: false,
        minimal_state: crate::minimal_api::MinimalState::default(),
        reconnect_pending: false,
        show_resolved_model: true,
        plugin_cta_enabled: false,
        leader_mode: true,
        leader_roster: Vec::new(),
        dashboard_local_sessions: Vec::new(),
        dashboard_sessions_loading: false,
        shared_prompt_queues: std::collections::HashMap::new(),
        optimistic_prompt_echoes: std::collections::HashMap::new(),
        session_picker_grouped: false,
        cancel_rewind_enabled: true,
        session_recap_available: false,
        tutorial: None,
        dashboard: None,
        dashboard_return: None,
        dashboard_persisted: None,
        keyboard_normalizer: crate::input::KeyboardNormalizer::from_terminal_context(),
    }
}
/// Build a default `AgentSession` for
/// tests. Centralises the fixture so new fields on `AgentSession`
/// don't break every test that constructs one by hand. The
/// `acp_tx` is cloned from the test `AppView`; the
/// `deferred_model_switch` is pulled from the `AppView`'s CLI
/// overrides for parity with `dispatch_new_session_inner`.
fn make_test_agent_session(app: &AppView, id: AgentId, sid: &str) -> AgentSession {
    {
        let mut session = AgentSession::new(
            id,
            app.acp_tx.clone(),
            Some(sid.to_string().into()),
            ModelState::default(),
            PathBuf::from("/tmp"),
            shell::util::config::PermissionMode::Ask,
        );
        session.deferred_model_switch = app.deferred_model_switch_from_cli();
        session
    }
}
pub(super) fn test_app_with_agent() -> AppView {
    let mut app = test_app();
    let id = AgentId(0);
    let session = make_test_agent_session(&app, id, "test-session");
    let mut agent = AgentView::new(session, ScrollbackState::new());
    agent.active_pane = ActivePane::Scrollback;
    app.agents.insert(id, agent);
    app.next_agent_id = 1;
    switch_to_agent(&mut app, id, SwitchCause::New);
    app
}
/// Give a test agent a generated title so the dashboard renders it.
///
/// The dashboard hides empty (no-real-turn) sessions
/// (`views::dashboard::row::is_empty_top_level`); nav/render tests that
/// rely on their placeholder agents being visible call this to opt in.
fn mark_agent_nonempty(app: &mut AppView, id: AgentId) {
    if let Some(a) = app.agents.get_mut(&id) {
        a.generated_session_title = Some(format!("Session {}", id.0));
    }
}
fn make_test_subagent(child_sid: &str, sa_id: &str) -> crate::app::subagent::SubagentInfo {
    crate::app::subagent::SubagentInfo {
        subagent_id: Arc::from(sa_id),
        child_session_id: Arc::from(child_sid),
        description: Arc::from("test subagent"),
        subagent_type: Arc::from("general-purpose"),
        model: None,
        context_source: None,
        resumed_from: None,
        capability_mode: None,
        permission_mode: None,
        effective_permission_mode: None,
        workflow_run_id: None,
        context_normalized: false,
        parent_prompt_id: None,
        started_at: std::time::Instant::now(),
        last_progress_at: std::time::Instant::now(),
        finished: false,
        status: None,
        error: None,
        duration_ms: None,
        tool_calls: None,
        turns: None,
        turn_count: None,
        tool_call_count: None,
        tokens_used: None,
        context_window_tokens: None,
        context_usage_pct: None,
        tools_used: Vec::new(),
        error_count: None,
        activity_label: None,
        is_background: false,
        pending_kill: false,
        kill_requested_at: None,
        scrollback_entry_id: None,
        prompt: None,
        child_cwd: None,
        worktree_path: None,
        child_updates_replayed: false,
    }
}
fn cta_entry(name: &str, status: &str) -> extension_types::MarketplacePluginEntry {
    extension_types::MarketplacePluginEntry {
        name: name.into(),
        version: None,
        description: None,
        category: None,
        author: None,
        tags: Vec::new(),
        keywords: Vec::new(),
        domains: Vec::new(),
        homepage: None,
        relative_path: format!("plugins/{name}"),
        install_status: status.into(),
        installed_version: None,
        components: None,
        remote_url: None,
        remote_ref: None,
        remote_sha: None,
        remote_subdir: None,
    }
}
fn cta_outcome(
    status: extension_types::OutcomeStatus,
    message: &str,
) -> extension_types::ActionOutcome {
    extension_types::ActionOutcome {
        status,
        message: message.into(),
        requires_reload: false,
        requires_restart: false,
    }
}
fn cta_mcp_server(
    name: &str,
    plugin: Option<&str>,
    status: crate::views::mcps_modal::McpServerDisplayStatus,
) -> crate::views::mcps_modal::McpServerInfo {
    crate::views::mcps_modal::McpServerInfo {
        name: name.into(),
        display_name: None,
        status,
        tool_count: 0,
        setup_required: false,
        setup: None,
        setup_values: std::collections::HashMap::new(),
        tools: vec![],
        enabled: true,
        source: plugin
            .map(|p| format!("plugin: {p}"))
            .unwrap_or_else(|| "local".into()),
        plugin_name: plugin.map(str::to_string),
    }
}
/// Extract text from the last system message in an agent's scrollback.
pub(super) fn last_system_text(app: &AppView, id: AgentId) -> String {
    system_text_from_end(app, id, 0)
}
/// Like [`last_system_text`] but takes an offset from the end.
/// `offset = 0` is the last entry, `offset = 1` is second-to-last, etc.
fn system_text_from_end(app: &AppView, id: AgentId, offset: usize) -> String {
    let sb = &app.agents[&id].scrollback;
    let idx = sb.len() - 1 - offset;
    let entry = sb.get(idx).expect("scrollback index out of bounds");
    match &entry.block {
        RenderBlock::Notice(sys) => sys.text.clone(),
        other => panic!("expected System block at index {idx}, got {other:?}"),
    }
}
/// Insert a placeholder agent at `id` so `switch_to_agent` recognises
/// it (the helper's defensive check uses `app.agents.contains_key`).
/// `session_id` and `active_pane` are populated to mirror the
/// existing `test_app_with_agent` setup; these tests do not read
/// either field.
fn insert_placeholder_agent(app: &mut AppView, id: AgentId) {
    let mut agent = AgentView::new(
        {
            let session = AgentSession::new(
                id,
                app.acp_tx.clone(),
                Some("placeholder".into()),
                ModelState::default(),
                PathBuf::from("/tmp"),
                shell::util::config::PermissionMode::Ask,
            );
            session
        },
        ScrollbackState::new(),
    );
    agent.active_pane = ActivePane::Scrollback;
    app.agents.insert(id, agent);
}
/// Build an app with three agents (ids 0, 1, 2) and `active_view` set
/// to agent 0.
pub(super) fn three_agent_app() -> AppView {
    let mut app = test_app_with_agent();
    insert_placeholder_agent(&mut app, AgentId(1));
    insert_placeholder_agent(&mut app, AgentId(2));
    app
}
use crate::slash::commands::fork::ForkArgs;
fn fork_args(worktree_override: Option<bool>, directive: Option<&str>) -> ForkArgs {
    ForkArgs {
        worktree_override,
        directive: directive.map(String::from),
    }
}
/// Build a single-agent app for the `/fork` dispatcher tests.
///
/// Sets `current_branch` to `Some("main")` so the agent appears to be
/// inside a git repo. This is required because `dispatch_fork` skips
/// the worktree question when `current_branch` is `None` (non-git cwd).
fn fork_test_app() -> AppView {
    let mut app = test_app_with_agent();
    app.agents.get_mut(&AgentId(0)).unwrap().current_branch = Some("main".into());
    app
}
/// Build a minimal `AcpArgs<acp::ExtRequest>` for an
/// `grow/ask_user_question` ext-method request. Returns the args
/// plus the receiver half of the response oneshot so the test can
/// assert the handler completes the ACP roundtrip.
fn make_ask_user_question_args(
    tool_call_id: &str,
) -> (
    acp_transport::AcpArgs<acp::ExtRequest>,
    tokio::sync::oneshot::Receiver<acp_transport::AcpResult<acp::ExtResponse>>,
) {
    use tools::implementations::grow_build::ask_user_question::{
        AskUserQuestionExtRequest, Question, QuestionOption,
    };
    let req = AskUserQuestionExtRequest {
        session_id: "test-session".into(),
        tool_call_id: tool_call_id.into(),
        mode: tools::implementations::grow_build::ask_user_question::AskUserQuestionMode::Default,
        questions: vec![Question {
            question: "ACP-driven question".into(),
            options: vec![QuestionOption {
                label: "ok".into(),
                description: "ok".into(),
                preview: None,
                id: None,
            }],
            multi_select: Some(false),
            id: None,
        }],
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let ext = acp::ExtRequest::new(
        "grow/ask_user_question",
        serde_json::value::to_raw_value(&req)
            .expect("serialize AskUserQuestionExtRequest")
            .into(),
    );
    (
        acp_transport::AcpArgs {
            request: ext,
            response_tx: tx,
        },
        rx,
    )
}
fn set_forked_from(app: &mut AppView, child: AgentId, parent: AgentId) {
    if let Some(agent) = app.agents.get_mut(&child) {
        agent.session.forked_from = Some(parent);
    }
}
fn make_bg_task(task_id: &str) -> crate::app::session::BgTaskState {
    crate::app::session::BgTaskState {
        task_id: task_id.into(),
        tool_call_id: String::new(),
        command: "sleep 99".into(),
        description: None,
        cwd: String::new(),
        output_file: String::new(),
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
        is_monitor: false,
        restored_from_replay: false,
    }
}
/// Set up a two-agent app: agent 0 is active with "sess-A",
/// agent 1 is inactive with "sess-B" and a bg task.
fn two_agent_app_with_bg_task() -> AppView {
    let mut app = test_app_with_agent();
    app.agents[&AgentId(0)].session.session_id = Some(acp::SessionId::new("sess-A"));
    let id1 = AgentId(1);
    let mut agent1 = AgentView::new(
        {
            let session = AgentSession::new(
                id1,
                app.acp_tx.clone(),
                Some(acp::SessionId::new("sess-B")),
                ModelState::default(),
                PathBuf::from("/tmp"),
                shell::util::config::PermissionMode::Ask,
            );
            session
        },
        ScrollbackState::new(),
    );
    let mut task = make_bg_task("task-B-1");
    task.pending_kill = true;
    task.kill_requested_at = Some(std::time::Instant::now());
    agent1.session.bg_tasks.insert("task-B-1".into(), task);
    app.agents.insert(id1, agent1);
    app.next_agent_id = 2;
    assert!(matches!(app.active_view, ActiveView::Agent(AgentId(0))));
    app
}
fn project_picker_app() -> AppView {
    let mut app = test_app();
    app.cwd = PathBuf::from("/tmp");
    app.project_picker_shown = false;
    app
}
/// Test helper: open Settings then OpenResetConfirm for `key`.
/// Extracted so individual tests don't have to repeat the
/// open-then-open ritual.
fn setup_reset_confirm_open(app: &mut AppView, key: crate::settings::SettingKey) {
    use crate::views::modal::ActiveModal;
    let _ = dispatch(Action::OpenSettings, app);
    let _ = dispatch(Action::OpenResetConfirm { key }, app);
    let agent = app.agents.get(&AgentId(0)).expect("agent must exist");
    assert!(
        matches!(
            agent.active_modal,
            Some(ActiveModal::ResetSettingsConfirm { .. })
        ),
        "setup_reset_confirm_open: ResetSettingsConfirm must be active",
    );
}
fn make_picker_entry(id: &str, cwd: &str) -> crate::app::root::SessionPickerEntry {
    crate::app::root::SessionPickerEntry {
        id: id.into(),
        summary: id.into(),
        updated_at: chrono::Utc::now(),
        created_at: chrono::Utc::now(),
        cwd: cwd.into(),
        hostname: None,
        model_id: None,
        num_messages: 0,
        last_active_at: None,
        branch: None,
        repo_name: "repo".into(),
        worktree_label: None,
        card_detail: None,
    }
}
/// Open a SessionPicker modal on the active agent seeded with `entries`.
fn open_session_picker_with(app: &mut AppView, entries: Vec<crate::app::root::SessionPickerEntry>) {
    use crate::views::modal::ActiveModal;
    let agent = get_active_agent_mut(app).expect("active agent");
    agent.active_modal = Some(ActiveModal::SessionPicker {
        state: crate::views::picker::PickerState::default(),
        entries: Some(entries),
        loading: false,
        previous_palette: None,
        window: crate::views::modal_window::ModalWindowState::new(),
        content_results: None,
        content_loading: false,
        deep_search_seq: 0,
        entries_query: None,
        pending_delete: None,
    });
}
/// Toast strings match the expected format and contain on/off
/// status.
fn read_toast(app: &AppView) -> String {
    let agent = app.agents.get(&AgentId(0)).expect("agent must exist");
    agent
        .toast
        .as_ref()
        .map(|(s, _)| s.message.clone())
        .expect("toast should be set")
}
/// Helper: enqueue a single permission containing the new
/// "enable-always-approve" option (AllowOnce kind, position 0 —
/// default-selected by the real `enqueue_permission` helper),
/// a regular "opt-allow-once" (AllowOnce kind, position 1), and
/// a "opt-reject-once" (RejectOnce, position 2). Mirrors the
/// option list the shell builds for TUI/Pager.
/// Returns the response receiver for the injected permission.
fn enqueue_permission_with_enable_always_approve(
    app: &mut AppView,
) -> tokio::sync::oneshot::Receiver<acp::Result<acp::RequestPermissionResponse>> {
    use crate::views::permission_view::{PermissionFocus, PermissionViewState};
    use std::sync::Arc;
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let request = acp::RequestPermissionRequest::new(
        acp::SessionId::new(Arc::from("test-session")),
        acp::ToolCallUpdate::new(
            acp::ToolCallId::new(Arc::from("tc-enable-aa-1")),
            acp::ToolCallUpdateFields::default(),
        ),
        vec![
            acp::PermissionOption::new(
                acp::PermissionOptionId::new(Arc::from(
                    workspace::permission::ENABLE_ALWAYS_APPROVE_OPTION_ID,
                )),
                "Yes, and don't ask again for anything",
                acp::PermissionOptionKind::AllowOnce,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::new(Arc::from("opt-allow-once")),
                "Yes, proceed",
                acp::PermissionOptionKind::AllowOnce,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::new(Arc::from("opt-reject-once")),
                "No",
                acp::PermissionOptionKind::RejectOnce,
            ),
        ],
    );
    let options = request.options.clone();
    agent.push_permission(PermissionViewState {
        request: acp_transport::AcpArgs {
            request,
            response_tx,
        },
        id: 1,
        focus: PermissionFocus::Options,
        options,
        active_idx: 0,
        bash_highlights: None,
        bash_selection_count: 0,
        bash_command_raw: None,
        mcp_scope: None,
        title: "test-enable-always-approve".to_string(),
        description: vec![],
        args_expanded: false,
        desc_scroll: 0,
        subagent_label: None,
        options_area_height: 0,
        options_scroll_offset: 0,
    });
    response_rx
}
const POLICY_WARNING: &str =
    workspace::permission::resolution::ALWAYS_APPROVE_PIN_REASON_REQUIREMENTS;
fn agent_toast(app: &AppView) -> Option<String> {
    app.agents[&AgentId(0)]
        .toast
        .as_ref()
        .map(|(s, _)| s.message.clone())
}
/// Use the `theme_cache::test_lock` to serialize tests that touch
/// the in-memory theme state (single mutable global). Mirrors the
/// pattern used by `theme::cache::tests`.
fn with_theme_test_env(f: impl FnOnce()) {
    let _guard = crate::theme::cache::test_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::theme::cache::reset_for_test();
    crate::theme::cache::seed_auto_theme_defaults_for_test();
    crate::theme::cache::set(crate::theme::ThemeKind::GrowNight);
    crate::theme::system_appearance::clear_mock();
    f();
    crate::theme::system_appearance::clear_mock();
    crate::theme::cache::reset_for_test();
}
fn agent_scrollback_len(app: &AppView) -> usize {
    app.agents.get(&AgentId(0)).unwrap().scrollback.len()
}
use crate::scrollback::blocks::UserPromptBlock;
/// Helper: open the dashboard against an existing `app`.
fn open_dashboard(app: &mut AppView) {
    let _ = dispatch_open_dashboard(app);
}
/// Display-order list of selectable row ids — the same order
/// `dashboard_neighbor_row` and the renderer walk. Test-only mirror
/// of the row build in `dispatch_dashboard_select`.
fn dashboard_row_order(app: &AppView) -> Vec<crate::views::dashboard::DashboardRowId> {
    let d = app.dashboard.as_ref().unwrap();
    let home = crate::views::dashboard::render::cached_home();
    let roster: &[crate::app::roster::RosterEntry] = if app.leader_mode {
        &app.leader_roster
    } else {
        &app.dashboard_local_sessions
    };
    let rows = crate::views::dashboard::build_rows_with_roster(
        &app.agents,
        &d.pinned,
        &d.reorder,
        None,
        d.grouping,
        &d.filter,
        home,
        roster,
    );
    crate::views::dashboard::render::focusables(
        &rows,
        d.grouping,
        &d.filter,
        &d.collapsed_sections,
        d.idle_show_all,
        d.search_mode,
    )
    .into_iter()
    .filter_map(|f| match f {
        crate::views::dashboard::Focusable::Row(id) => Some(id),
        crate::views::dashboard::Focusable::Section(_)
        | crate::views::dashboard::Focusable::IdleOverflow => None,
    })
    .collect()
}
/// Build a synthetic `PermissionViewState` with the given id and
/// options. Pushes it to the agent's permission_queue.
///
/// Returns the response receiver so tests can verify
/// the response was actually `send`'d through the oneshot. The
/// previous version dropped the receiver (`_rx`), which let
/// "happy-path" tests assert the queue was popped but masked
/// regressions where the pop happened without the corresponding
/// send.
fn push_synthetic_permission(
    agent: &mut crate::app::agent_view::AgentView,
    id: usize,
    options: Vec<(&str, &str)>,
) -> tokio::sync::oneshot::Receiver<Result<acp::RequestPermissionResponse, acp::Error>> {
    use crate::views::permission_view::{PermissionFocus, PermissionViewState};
    let (tx, rx) =
        tokio::sync::oneshot::channel::<Result<acp::RequestPermissionResponse, acp::Error>>();
    let session_id = agent
        .session
        .session_id
        .clone()
        .expect("synthetic permission requires a session id");
    let request = acp_transport::AcpArgs {
        request: acp::RequestPermissionRequest::new(
            session_id,
            acp::ToolCallUpdate::new(
                acp::ToolCallId::new(std::sync::Arc::from("tc-1")),
                acp::ToolCallUpdateFields::default(),
            ),
            options
                .iter()
                .map(|(oid, name)| {
                    acp::PermissionOption::new(
                        acp::PermissionOptionId::new(std::sync::Arc::from(*oid)),
                        name.to_string(),
                        if *oid == "reject" {
                            acp::PermissionOptionKind::RejectOnce
                        } else {
                            acp::PermissionOptionKind::AllowOnce
                        },
                    )
                })
                .collect(),
        ),
        response_tx: tx,
    };
    let opts = request.request.options.clone();
    let state = PermissionViewState {
        request,
        id,
        focus: PermissionFocus::Options,
        options: opts,
        active_idx: 0,
        bash_highlights: None,
        bash_selection_count: 0,
        bash_command_raw: None,
        mcp_scope: None,
        title: "Test permission".to_string(),
        description: Vec::new(),
        args_expanded: false,
        desc_scroll: 0,
        subagent_label: None,
        options_area_height: 0,
        options_scroll_offset: 0,
    };
    agent.push_permission(state);
    rx
}
const MOUSE_OFF_STICKY: &str = crate::app::MOUSE_OFF_HINT;
fn reset_mouse_capture_enabled(on: bool) {
    crate::app::MOUSE_CAPTURE_ENABLED.store(on, std::sync::atomic::Ordering::Release);
}
fn mouse_capture_is_enabled() -> bool {
    crate::app::MOUSE_CAPTURE_ENABLED.load(std::sync::atomic::Ordering::Acquire)
}

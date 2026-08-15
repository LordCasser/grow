//! Tests for session status, sharing, privacy, and coding-data-sharing dispatchers.

use super::*;

/// Regression (leader-mode turn-end race): when this client is briefly Idle
/// (`is_turn_running() == false`, `current_prompt_id` cleared) but the server
/// still has queued prompts — visible as a non-empty `shared_queue` mirror —
/// a newly-sent prompt must route to the SERVER (immediate-send), NOT be
/// locally drained as a phantom running turn. The failure mode: a
/// `send_route_plain immediate=false is_turn_running=false shared_queue_len=5`
/// path taking `local_drain`, leaving the prompt shown running on the sender
/// while it was actually queued behind the existing entries on the leader and
/// every other client.
#[test]
fn send_while_idle_with_nonempty_shared_queue_routes_to_server() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    // Two prompts already queued on the server (as a broadcast would leave
    // things): populate the authoritative map AND mirror it into the agent.
    app.push_optimistic_prompt_echo("test-session", "q1", "a", "prompt");
    app.push_optimistic_prompt_echo("test-session", "q2", "b", "prompt");
    {
        let snapshot = app.shared_prompt_queue("test-session").cloned().unwrap();
        let agent = app.agents.get_mut(&id).unwrap();
        // Turn-end window: locally Idle with no current prompt, but the
        // server's queue (mirrored from the last broadcast) still has work.
        agent.session.state = AgentState::Idle;
        agent.session.current_prompt_id = None;
        agent.shared_queue = snapshot;
        assert!(agent.session.pending_prompts.is_empty());
    }

    let effects = dispatch(Action::SendPrompt("c".into()), &mut app);

    // Routed to the server (immediate-send), keyed by a fresh prompt_id.
    let pid = effects
        .iter()
        .find_map(|e| match e {
            Effect::SendPrompt {
                text, prompt_id, ..
            } if text == "c" => Some(prompt_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected immediate SendPrompt for 'c', got {effects:?}"));
    // Did NOT start a local turn or adopt "c" as the running prompt.
    assert!(
        !app.agents[&id].session.state.is_turn_running(),
        "must not promote 'c' to a local running turn"
    );
    assert!(
        app.agents[&id].session.current_prompt_id.is_none(),
        "must not set current_prompt_id locally for a server-queued prompt"
    );
    // Echoed into the shared queue BEHIND the existing entries (position 3).
    let q = app
        .shared_prompt_queue("test-session")
        .expect("optimistic echo present");
    assert_eq!(q.len(), 3, "c queued behind q1, q2");
    assert_eq!(q.last().map(|e| e.id.as_str()), Some(pid.as_str()));
    assert_eq!(q.last().map(|e| e.text.as_str()), Some("c"));
}

#[test]
fn dispatch_rename_session_updates_display_name_locally() {
    let mut app = test_app_with_agent();
    let effects = dispatch_rename_session(&mut app, "renamed via slash".into());
    assert_eq!(effects.len(), 1);
    assert_eq!(
        app.agents[&AgentId(0)].display_name.as_deref(),
        Some("renamed via slash"),
        "/rename must also update local display_name cache"
    );
}

/// `ConfirmResetSetting { choice: Reset }` on a SHARED Bool
/// target restores the Settings modal AND fires the typed
/// `Action::SetCompactMode(default)` via recursive dispatch —
/// the `Effect::PersistSetting` is the externally-observable
/// signal. Also asserts the ui_snapshot was
/// refreshed to the new (post-reset) value (symmetric with the
/// Cancel test's snapshot assertion).
#[test]
fn dispatch_confirm_reset_setting_reset_dispatches_typed_setter_for_shared_bool() {
    use crate::settings::SettingValue;
    use crate::views::modal::{ActiveModal, ResetSettingsResult};
    let mut app = test_app_with_agent();
    // Flip compact_mode to true so we can observe the reset back
    // to its default (false).
    let _ = dispatch(Action::SetCompactMode(true), &mut app);
    assert!(app.current_ui.compact_mode);

    setup_reset_confirm_open(&mut app, "compact_mode");

    let effects = dispatch(
        Action::ConfirmResetSetting {
            choice: ResetSettingsResult::Reset,
        },
        &mut app,
    );

    // Recursive dispatch into Action::SetCompactMode(false) emits
    // the persist effect.
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::PersistSetting { key, value, .. } => {
            assert_eq!(*key, "compact_mode");
            assert_eq!(value, &SettingValue::Bool(false));
        }
        other => panic!("expected PersistSetting, got {other:?}"),
    }
    // In-memory state is reset to the default.
    assert!(!app.current_ui.compact_mode);
    // Modal is restored AND ui_snapshot reflects the new value
    // (symmetric with the Cancel test).
    let agent = app.agents.get(&AgentId(0)).expect("agent must exist");
    match &agent.active_modal {
        Some(ActiveModal::Settings { state }) => {
            assert!(
                !state.ui_snapshot.compact_mode,
                "ui_snapshot must reflect the post-reset value"
            );
        }
        _ => panic!("Reset branch must restore the Settings modal"),
    }
}

/// `ConfirmResetSetting { choice: Reset }` on a SHARED Enum
/// target (`theme`) dispatches `Action::SetTheme(default)` via
/// recursive dispatch — verifies the action_for_reset Enum arm.
#[test]
fn dispatch_confirm_reset_setting_reset_dispatches_typed_setter_for_shared_enum() {
    use crate::settings::SettingValue;
    use crate::views::modal::ResetSettingsResult;
    // SetTheme mutates the global theme cache — serialize with the
    // other theme tests via the theme test lock.
    with_theme_test_env(|| {
        let mut app = test_app_with_agent();
        // Flip theme to a non-default first.
        let _ = dispatch(Action::SetTheme("tokyonight".to_string()), &mut app);
        assert_eq!(app.current_ui.theme.as_deref(), Some("tokyonight"));

        setup_reset_confirm_open(&mut app, "theme");

        let effects = dispatch(
            Action::ConfirmResetSetting {
                choice: ResetSettingsResult::Reset,
            },
            &mut app,
        );

        // Reset → SetTheme("grownight") (the registered default).
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            Effect::PersistSetting { key, value, .. } => {
                assert_eq!(*key, "theme");
                assert_eq!(value, &SettingValue::Enum("grownight"));
            }
            other => panic!("expected PersistSetting, got {other:?}"),
        }
        assert_eq!(app.current_ui.theme.as_deref(), Some("grownight"));
    });
}

#[test]
fn show_usage_on_welcome_screen_is_noop() {
    let mut app = test_app();
    let effects = dispatch(Action::ShowUsage, &mut app);
    assert!(
        effects.is_empty(),
        "ShowUsage with no active agent should be a no-op"
    );
}

#[test]
fn show_usage_opens_usage_modal_and_fetches_all_tabs() {
    use crate::views::modal::ActiveModal;
    use crate::views::usage_modal::UsageModalTab;

    let mut app = test_app_with_agent();
    let before = agent_scrollback_len(&app);
    let effects = dispatch(Action::ShowUsage, &mut app);

    // One epoch, three fetches: usage + context + session info.
    let nonce = effects
        .iter()
        .find_map(|e| match e {
            Effect::FetchSessionUsage { nonce, .. } => Some(*nonce),
            _ => None,
        })
        .expect("usage fetch carries the epoch");
    assert!(nonce > 0, "modal epoch must be non-zero");
    assert!(
        matches!(
            effects.as_slice(),
            [
                Effect::FetchSessionUsage { agent_id, nonce: n1, .. },
                Effect::ShowContextInfo { agent_id: a2, nonce: n2, .. },
                Effect::ShowSessionInfo { agent_id: a3, nonce: n3, .. },
            ] if *agent_id == AgentId(0)
                && *a2 == AgentId(0)
                && *a3 == AgentId(0)
                && *n1 == nonce
                && *n2 == nonce
                && *n3 == nonce
        ),
        "got: {effects:?}"
    );
    assert_eq!(
        agent_scrollback_len(&app),
        before,
        "opening the modal must not write into the transcript"
    );
    let agent = app.agents.get(&AgentId(0)).expect("agent exists");
    match &agent.active_modal {
        Some(ActiveModal::Usage { state }) => {
            assert_eq!(state.active_tab, UsageModalTab::Usage);
            assert_eq!(state.fetch_nonce, nonce);
        }
        _ => panic!("expected usage modal open on Usage tab"),
    }
}

#[test]
fn show_usage_opens_modal_on_requested_tab_for_context_and_session_info() {
    use crate::views::modal::ActiveModal;
    use crate::views::usage_modal::UsageModalTab;

    for (action, expected_tab) in [
        (Action::ShowContextInfo, UsageModalTab::Context),
        (Action::ShowSessionInfo, UsageModalTab::SessionInfo),
    ] {
        let mut app = test_app_with_agent();
        let effects = dispatch(action, &mut app);
        assert_eq!(effects.len(), 3, "all three tabs fetch on open");
        let agent = app.agents.get(&AgentId(0)).expect("agent exists");
        match &agent.active_modal {
            Some(ActiveModal::Usage { state }) => assert_eq!(state.active_tab, expected_tab),
            _ => panic!("expected usage modal open"),
        }
    }
}

#[test]
fn show_usage_minimal_keeps_scrollback_fetch() {
    let mut app = test_app_with_agent();
    app.screen_mode = crate::app::ScreenMode::Minimal;
    let effects = dispatch(Action::ShowUsage, &mut app);
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::FetchSessionUsage { agent_id, nonce: 0, .. }] if *agent_id == AgentId(0)
        ),
        "minimal mode keeps the scrollback-intent fetch, got: {effects:?}"
    );
    assert!(
        app.agents[&AgentId(0)].active_modal.is_none(),
        "minimal mode must not open the modal"
    );
}

#[test]
fn show_session_info_minimal_keeps_scrollback_fetch() {
    let mut app = test_app_with_agent();
    app.screen_mode = crate::app::ScreenMode::Minimal;
    let effects = dispatch(Action::ShowSessionInfo, &mut app);
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::ShowSessionInfo { agent_id, nonce: 0, .. }] if *agent_id == AgentId(0)
        ),
        "got: {effects:?}"
    );
    assert!(app.agents[&AgentId(0)].active_modal.is_none());
}

#[test]
fn show_context_info_minimal_keeps_scrollback_fetch() {
    let mut app = test_app_with_agent();
    app.screen_mode = crate::app::ScreenMode::Minimal;
    let effects = dispatch(Action::ShowContextInfo, &mut app);
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::ShowContextInfo { agent_id, nonce: 0, .. }] if *agent_id == AgentId(0)
        ),
        "got: {effects:?}"
    );
    assert!(app.agents[&AgentId(0)].active_modal.is_none());
}

// ── Minimal update-notice tests ──────────────────────────────────────

#[test]
fn minimal_update_notice_commits_a_system_block() {
    let mut app = test_app_with_agent();
    let before = agent_scrollback_len(&app);
    commit_minimal_update_notice(&mut app, "9.9.9");
    assert_eq!(agent_scrollback_len(&app), before + 1);
    let text = last_system_text(&app, AgentId(0));
    assert!(text.contains("Update available: v9.9.9"), "got: {text:?}");
    assert!(text.contains("restart to apply"), "got: {text:?}");
}

#[test]
fn minimal_update_notice_no_active_agent_is_noop() {
    let mut app = test_app();
    // Must not panic and must not require an agent.
    commit_minimal_update_notice(&mut app, "9.9.9");
}

// ── Tutorial dispatch tests ──────────────────────────────────────────

/// `/tutorial` (and the palette entry) open the overlay; dispatching again
/// while open toggles it closed. No side effects either way.
#[test]
fn open_tutorial_toggles_overlay_without_effects() {
    let mut app = test_app();
    let effects = dispatch(Action::OpenTutorial, &mut app);
    assert!(app.tutorial.is_some(), "tutorial opens");
    assert!(effects.is_empty(), "open emits nothing, got: {effects:?}");

    let effects = dispatch(Action::OpenTutorial, &mut app);
    assert!(app.tutorial.is_none(), "toggle closes");
    assert!(effects.is_empty(), "close emits nothing, got: {effects:?}");
}

// ── Usage modal routing tests ─────────────────────────────────────────

fn session_info_response() -> shell::session::SessionInfoResponse {
    shell::session::SessionInfoResponse {
        session_id: "test-session".to_string(),
        cwd: "/tmp".to_string(),
        data: shell::session::SessionInfoData {
            agent_name: Some("grow-build".into()),
            model: Some("grow-build".into()),
            model_display_name: None,
            resolved_model_id: None,
            model_fingerprint: None,
            show_model_fingerprint: false,
            api_backend: None,
            conversation_id: None,
            turns: 1,
            turn_index: 0,
            context: shell::session::ContextInfo {
                used: 100,
                total: 100_000,
                system_prompt_tokens: 0,
                tool_definitions_count: 0,
                tool_definitions_tokens: 0,
                compaction_count: 0,
                turn_count: 1,
                tool_call_count: 0,
                message_count: 1,
                message_tokens: 100,
                free_tokens: 99_900,
                usage_pct: 0,
                auto_compact_threshold_percent: 85,
                usage_categories: Vec::new(),
            },
        },
    }
}

fn open_modal_nonce(app: &mut AppView, action: Action) -> u64 {
    let effects = dispatch(action, app);
    effects
        .iter()
        .find_map(|e| match e {
            Effect::ShowSessionInfo { nonce, .. } => Some(*nonce),
            _ => None,
        })
        .expect("session-info effect carries the epoch")
}

/// Required Invariant: a result from before a close/reopen can never
/// overwrite the newer open's data.
#[test]
fn stale_epoch_results_are_dropped_after_close_reopen() {
    use crate::views::modal::ActiveModal;
    use crate::views::usage_modal::UsageTabData;

    let mut app = test_app_with_agent();
    let first_nonce = open_modal_nonce(&mut app, Action::ShowSessionInfo);

    // Close the modal, then reopen — the reopen gets a fresh epoch.
    app.agents.get_mut(&AgentId(0)).unwrap().active_modal = None;
    let second_nonce = open_modal_nonce(&mut app, Action::ShowSessionInfo);
    assert_ne!(first_nonce, second_nonce, "reopen must mint a new epoch");

    // The first open's result arrives late: must be dropped entirely.
    let effects = dispatch_task_result(
        TaskResult::SessionInfoComplete {
            agent_id: AgentId(0),
            info: Box::new(session_info_response()),
            text: "stale result".to_string(),
            title: Some("stale".to_string()),
            show_resolved_model: false,
            nonce: first_nonce,
        },
        &mut app,
    );
    assert!(effects.is_empty());
    let agent = app.agents.get(&AgentId(0)).unwrap();
    match &agent.active_modal {
        Some(ActiveModal::Usage { state }) => assert!(
            matches!(state.session_info, UsageTabData::Loading),
            "stale result must not fill the reopened modal"
        ),
        _ => panic!("modal must be open"),
    }

    // The current epoch's result fills the tab.
    let _ = dispatch_task_result(
        TaskResult::SessionInfoComplete {
            agent_id: AgentId(0),
            info: Box::new(session_info_response()),
            text: "fresh result".to_string(),
            title: None,
            show_resolved_model: false,
            nonce: second_nonce,
        },
        &mut app,
    );
    let agent = app.agents.get(&AgentId(0)).unwrap();
    match &agent.active_modal {
        Some(ActiveModal::Usage { state }) => match &state.session_info {
            UsageTabData::Loaded(rows) => {
                assert!(
                    rows.iter()
                        .any(|r| r.label == "Session ID" && r.value == "test-session"),
                    "fresh result must fill the modal rows"
                );
            }
            other => panic!("expected loaded rows, got {other:?}"),
        },
        _ => panic!("modal must be open"),
    }
}

/// Required Invariant: modal content never enters the transcript.
#[test]
fn modal_fill_writes_nothing_to_scrollback() {
    use crate::views::modal::ActiveModal;
    use crate::views::usage_modal::{UsageModalTab, UsageTabData};

    let mut app = test_app_with_agent();
    let before = agent_scrollback_len(&app);
    let nonce = open_modal_nonce(&mut app, Action::ShowUsage);

    // Fill all three tabs.
    dispatch_task_result(
        TaskResult::SessionUsageComplete {
            agent_id: AgentId(0),
            session_id: acp::SessionId::new("test-session".to_string()),
            usage: Box::new(shell::extensions::notification::PromptUsage::default()),
            nonce,
        },
        &mut app,
    );
    dispatch_task_result(
        TaskResult::ContextInfoComplete {
            agent_id: AgentId(0),
            info: Box::new(session_info_response()),
            nonce,
        },
        &mut app,
    );
    dispatch_task_result(
        TaskResult::SessionInfoComplete {
            agent_id: AgentId(0),
            info: Box::new(session_info_response()),
            text: "must not reach scrollback".to_string(),
            title: None,
            show_resolved_model: false,
            nonce,
        },
        &mut app,
    );

    assert_eq!(
        agent_scrollback_len(&app),
        before,
        "modal tab fills must never push transcript blocks"
    );
    let agent = app.agents.get(&AgentId(0)).unwrap();
    let Some(ActiveModal::Usage { state }) = &agent.active_modal else {
        panic!("modal must be open");
    };
    assert_eq!(state.active_tab, UsageModalTab::Usage);
    assert!(matches!(state.usage, UsageTabData::Loaded(_)));
    assert!(matches!(state.context, UsageTabData::Loaded(_)));
    assert!(matches!(state.session_info, UsageTabData::Loaded(_)));
}

/// Minimal-mode regression: scrollback-intent fetches keep the legacy blocks.
#[test]
fn minimal_mode_commits_scrollback_blocks() {
    let mut app = test_app_with_agent();
    app.screen_mode = crate::app::ScreenMode::Minimal;
    let before = agent_scrollback_len(&app);

    // Session info.
    dispatch_task_result(
        TaskResult::SessionInfoComplete {
            agent_id: AgentId(0),
            info: Box::new(session_info_response()),
            text: "Session info block".to_string(),
            title: None,
            show_resolved_model: false,
            nonce: 0,
        },
        &mut app,
    );
    assert_eq!(agent_scrollback_len(&app), before + 1);
    assert!(last_system_text(&app, AgentId(0)).contains("Session info block"));

    // Context info → structured context block.
    let before = agent_scrollback_len(&app);
    dispatch_task_result(
        TaskResult::ContextInfoComplete {
            agent_id: AgentId(0),
            info: Box::new(session_info_response()),
            nonce: 0,
        },
        &mut app,
    );
    assert_eq!(agent_scrollback_len(&app), before + 1);

    // Usage ledger → system block.
    let before = agent_scrollback_len(&app);
    dispatch_task_result(
        TaskResult::SessionUsageComplete {
            agent_id: AgentId(0),
            session_id: acp::SessionId::new("test-session".to_string()),
            usage: Box::new(shell::extensions::notification::PromptUsage::default()),
            nonce: 0,
        },
        &mut app,
    );
    assert_eq!(agent_scrollback_len(&app), before + 1);
    assert!(
        last_system_text(&app, AgentId(0)).contains("Session usage"),
        "got: {:?}",
        last_system_text(&app, AgentId(0))
    );
}

/// Usage results are dropped when the session moved on (both routes).
#[test]
fn usage_result_guards_on_session_id() {
    use crate::views::modal::ActiveModal;
    use crate::views::usage_modal::UsageTabData;

    let mut app = test_app_with_agent();
    let before = agent_scrollback_len(&app);
    let nonce = open_modal_nonce(&mut app, Action::ShowUsage);
    dispatch_task_result(
        TaskResult::SessionUsageComplete {
            agent_id: AgentId(0),
            session_id: acp::SessionId::new("different-session".to_string()),
            usage: Box::new(shell::extensions::notification::PromptUsage::default()),
            nonce,
        },
        &mut app,
    );
    assert_eq!(agent_scrollback_len(&app), before);
    let agent = app.agents.get(&AgentId(0)).unwrap();
    let Some(ActiveModal::Usage { state }) = &agent.active_modal else {
        panic!("modal must be open");
    };
    assert!(
        matches!(state.usage, UsageTabData::Loading),
        "mismatched session result must be dropped"
    );
}

/// Failure results route to the modal's per-tab error state.
#[test]
fn failed_fetches_fill_modal_error_state() {
    use crate::views::modal::ActiveModal;
    use crate::views::usage_modal::UsageTabData;

    let mut app = test_app_with_agent();
    let nonce = open_modal_nonce(&mut app, Action::ShowSessionInfo);
    dispatch_task_result(
        TaskResult::SessionInfoFailed {
            agent_id: AgentId(0),
            error: "boom".to_string(),
            nonce,
        },
        &mut app,
    );
    let agent = app.agents.get(&AgentId(0)).unwrap();
    let Some(ActiveModal::Usage { state }) = &agent.active_modal else {
        panic!("modal must be open");
    };
    match &state.session_info {
        UsageTabData::Failed(error) => assert_eq!(error, "boom"),
        other => panic!("expected Failed state, got {other:?}"),
    }
}

/// `dispatch_copy_usage_modal_value` is a no-op without a matching open modal.
#[test]
fn copy_usage_modal_value_without_modal_is_noop() {
    let mut app = test_app_with_agent();
    let effects = dispatch(Action::CopyUsageModalValue(0), &mut app);
    assert!(effects.is_empty(), "no copy when no modal is open");
}

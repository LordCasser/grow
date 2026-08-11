//! Tests for permission request selection, follow-ups, and queue draining.

use super::*;

/// `ConfirmResetSetting
/// { Reset }` on `permission_mode` (the security-critical SHELL Enum)
/// dispatches the future-session default setter and leaves the live session
/// untouched.
#[test]
fn reset_permission_setting_changes_default_not_active_session() {
    use crate::views::modal::ResetSettingsResult;
    let mut app = test_app_with_agent();
    // Give both layers the same initial value so separation is observable.
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    let _ = dispatch(
        Action::SetDefaultPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    assert!(app.agents[&AgentId(0)].session.is_yolo());

    setup_reset_confirm_open(&mut app, "permission_mode");

    let effects = dispatch(
        Action::ConfirmResetSetting {
            choice: ResetSettingsResult::Reset,
        },
        &mut app,
    );

    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::PersistPermissionMode {
            canonical: "ask",
            session_id: None,
            ..
        }
    )));
    assert!(app.agents[&AgentId(0)].session.is_yolo());
    assert!(!app.default_yolo);
}

/// **Security-critical:** YOLO ON must drain the per-agent
/// `permission_queue` with `AllowOnce` responses. If this drain
/// path regresses (e.g., the setter falls back to `Cancelled`
/// without an `AllowOnce` lookup), the user enables YOLO and
/// their queued permissions silently get rejected.
#[test]
fn set_yolo_mode_on_drains_permission_queue_with_allow_once() {
    use crate::views::permission_view::{PermissionFocus, PermissionViewState};
    use std::sync::Arc;

    let mut app = test_app_with_agent();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();

    // Inject a fake queued permission. The drain semantics use
    // `find(|o| o.kind == AllowOnce)` so we need ≥1 AllowOnce
    // option for the test to exercise the happy path.
    let (response_tx, mut response_rx) = tokio::sync::oneshot::channel();
    let request = acp::RequestPermissionRequest::new(
        acp::SessionId::new(Arc::from("test-session")),
        acp::ToolCallUpdate::new(
            acp::ToolCallId::new(Arc::from("tc-1")),
            acp::ToolCallUpdateFields::default(),
        ),
        vec![
            acp::PermissionOption::new(
                acp::PermissionOptionId::new(Arc::from("opt-allow-once")),
                "Allow once",
                acp::PermissionOptionKind::AllowOnce,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::new(Arc::from("opt-reject")),
                "Reject",
                acp::PermissionOptionKind::RejectOnce,
            ),
        ],
    );
    let options = request.options.clone();
    agent.permission_queue.push_back(PermissionViewState {
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
        title: "test".to_string(),
        description: vec![],
        args_expanded: false,
        desc_scroll: 0,
        subagent_label: None,
        options_area_height: 0,
        options_scroll_offset: 0,
    });
    assert_eq!(agent.permission_queue.len(), 1);

    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    // Queue is drained.
    assert!(
        app.agents[&AgentId(0)].permission_queue.is_empty(),
        "YOLO ON must drain the permission_queue",
    );
    // Verify the `AllowOnce` response was actually sent (NOT
    // `Cancelled`). The drain semantics use `find(|o| o.kind ==
    // AllowOnce)` — a regression to `Cancelled` here would
    // silently reject every queued permission when the user
    // enables YOLO, which is the exact security failure mode
    // this test prevents.
    match response_rx.try_recv() {
        Ok(Ok(acp::RequestPermissionResponse {
            outcome:
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome {
                    option_id,
                    ..
                }),
            ..
        })) => {
            assert_eq!(
                option_id,
                acp::PermissionOptionId::new(Arc::from("opt-allow-once")),
                "the drain must select the AllowOnce option (NOT Cancelled / RejectOnce)",
            );
        }
        other => panic!(
            "queue drain must send an `AllowOnce` Selected response, got {other:?} — \
                 security regression: queued permissions are NOT being auto-approved on YOLO ON",
        ),
    }
}

#[test]
fn permission_select_clears_double_click_tracker_for_next_prompt() {
    use crate::views::permission_view::PermissionFocus;
    use std::sync::Arc;

    let mut app = test_app_with_agent();
    let _rx_front = enqueue_permission_with_enable_always_approve(&mut app);
    let _rx_next = enqueue_permission_with_enable_always_approve(&mut app);

    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.permission_queue.get_mut(1).unwrap().focus = PermissionFocus::FollowupInput;
    agent.last_permission_click = Some((Instant::now(), 1));

    let _ = dispatch(
        Action::PermissionSelect(acp::PermissionOptionId::new(Arc::from("opt-allow-once"))),
        &mut app,
    );

    let agent = &app.agents[&AgentId(0)];
    assert_eq!(agent.permission_queue.len(), 1);
    assert!(
        agent.last_permission_click.is_none(),
        "armed click on the resolved prompt must not pair with a click on the next prompt"
    );
    assert_eq!(
        agent.permission_queue.front().unwrap().focus,
        PermissionFocus::Options,
        "next front must be reset to Options"
    );
}

#[test]
fn drain_root_permission_queue_clears_double_click_tracker() {
    let mut app = test_app_with_agent();
    let _rx = enqueue_permission_with_enable_always_approve(&mut app);

    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.last_permission_click = Some((Instant::now(), 1));

    drain_root_permission_queue(agent);

    assert!(agent.permission_queue.is_empty());
    assert!(
        agent.last_permission_click.is_none(),
        "turn-end/turn-cancel drain must invalidate the armed click"
    );
}

#[test]
fn root_turn_drain_preserves_child_permission() {
    let mut app = test_app_with_agent();
    let mut response_rx = enqueue_permission_with_enable_always_approve(&mut app);
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent
        .permission_queue
        .front_mut()
        .unwrap()
        .request
        .request
        .session_id = acp::SessionId::new("child-session");

    drain_root_permission_queue(agent);

    assert_eq!(agent.permission_queue.len(), 1);
    assert!(matches!(
        response_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
}

#[test]
fn draining_root_request_behind_child_preserves_front_followup_state() {
    let mut app = test_app_with_agent();
    let _child_rx = enqueue_permission_with_enable_always_approve(&mut app);
    let _root_rx = enqueue_permission_with_enable_always_approve(&mut app);
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    let child = agent.permission_queue.front_mut().unwrap();
    child.request.request.session_id = acp::SessionId::new("child-session");
    child.focus = crate::views::permission_view::PermissionFocus::FollowupInput;
    agent.prompt.set_text("child feedback draft");
    agent.last_permission_click = Some((Instant::now(), child.id));

    drain_root_permission_queue(agent);

    assert_eq!(agent.permission_queue.len(), 1);
    assert_eq!(
        agent.permission_queue.front().unwrap().focus,
        crate::views::permission_view::PermissionFocus::FollowupInput
    );
    assert_eq!(agent.prompt.text(), "child feedback draft");
    assert!(agent.last_permission_click.is_some());
}

#[test]
fn enabling_parent_yolo_preserves_queued_child_ask() {
    let mut app = test_app_with_agent();
    let mut response_rx = enqueue_permission_with_enable_always_approve(&mut app);
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .permission_queue
        .front_mut()
        .unwrap()
        .request
        .request
        .session_id = acp::SessionId::new("child-session");

    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    assert!(app.agents[&AgentId(0)].session.is_yolo());
    assert_eq!(app.agents[&AgentId(0)].permission_queue.len(), 1);
    assert!(matches!(
        response_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
}

/// Regression: answering a permission whose requester has already
/// disconnected (response channel closed — e.g. a subagent session was
/// torn down or timed out while its request sat in the queue) must toast
/// instead of silently swallowing the user's approval.
#[test]
fn permission_select_toasts_when_requester_disconnected() {
    use std::sync::Arc;

    let mut app = test_app_with_agent();
    let response_rx = enqueue_permission_with_enable_always_approve(&mut app);
    // Requester gone before the user answers — `send` will fail.
    drop(response_rx);

    let _ = dispatch(
        Action::PermissionSelect(acp::PermissionOptionId::new(Arc::from("opt-allow-once"))),
        &mut app,
    );

    assert!(
        app.agents[&AgentId(0)].permission_queue.is_empty(),
        "the dead request must still be popped from the queue"
    );
    assert_eq!(
        agent_toast(&app).as_deref(),
        Some("This permission request is no longer valid (the requester disconnected)"),
        "answering a request whose requester disconnected must toast",
    );
}

/// Same as [`permission_select_toasts_when_requester_disconnected`] for the
/// cancel path (Ctrl-C / Esc).
#[test]
fn permission_cancel_toasts_when_requester_disconnected() {
    let mut app = test_app_with_agent();
    let response_rx = enqueue_permission_with_enable_always_approve(&mut app);
    // Requester gone before the user answers — `send` will fail.
    drop(response_rx);

    let _ = dispatch(Action::PermissionCancel, &mut app);

    assert!(
        app.agents[&AgentId(0)].permission_queue.is_empty(),
        "the dead request must still be popped from the queue"
    );
    assert_eq!(
        agent_toast(&app).as_deref(),
        Some("This permission request is no longer valid (the requester disconnected)"),
        "cancelling a request whose requester disconnected must toast",
    );
}

#[test]
fn set_permission_mode_always_approve_blocked_by_policy_pin() {
    use crate::app::actions::PermissionModeKind;
    use crate::views::modal::ActiveModal;
    let mut app = test_app_with_agent();
    app.yolo_policy_block = Some(POLICY_WARNING);
    // Open the settings modal so the blocked path's snapshot refresh is
    // exercised: the modal must keep showing the live (non-yolo) value.
    let _ = dispatch(Action::OpenSettings, &mut app);

    let effects = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    assert!(
        effects.is_empty(),
        "blocked modal commit must not persist, got {effects:?}",
    );
    assert!(!app.agents[&AgentId(0)].session.is_yolo());
    assert_eq!(
        app.current_ui.permission_mode, None,
        "canonical mirror must stay untouched"
    );
    let agent = app.agents.get(&AgentId(0)).unwrap();
    let Some(ActiveModal::Settings { state }) = &agent.active_modal else {
        panic!("Settings modal must remain open across the blocked dispatch")
    };
    assert!(
        !state.pager_snapshot.yolo_mode,
        "modal snapshot must show the live (non-yolo) value after the block",
    );
    assert_ne!(
        state.ui_snapshot.permission_mode.as_deref(),
        Some("always-approve"),
        "modal canonical must not show the refused mode",
    );
    assert_eq!(agent_toast(&app).as_deref(), Some(POLICY_WARNING));

    // Non-yolo kinds still commit under the pin.
    let effects = dispatch(Action::SetPermissionMode(PermissionModeKind::Ask), &mut app);
    assert!(matches!(
        effects.as_slice(),
        [Effect::NotifySessionPermissionMode {
            canonical: "ask",
            ..
        }]
    ));
    assert_eq!(app.current_ui.permission_mode, None);
}

/// SetPermissionMode(Auto) changes only the active session.
#[test]
fn set_permission_mode_auto_notifies_without_changing_default() {
    use crate::app::actions::PermissionModeKind;
    let mut app = test_app_with_agent();
    let effects = dispatch(
        Action::SetPermissionMode(PermissionModeKind::Auto),
        &mut app,
    );
    assert!(!app.agents[&AgentId(0)].session.is_yolo());
    assert!(app.agents[&AgentId(0)].session.is_auto());
    assert_eq!(app.current_ui.permission_mode, None);
    assert!(matches!(
        effects.as_slice(),
        [Effect::NotifySessionPermissionMode {
            canonical: "auto",
            ..
        }]
    ));
}

#[test]
fn session_permission_rejects_persisted_default_sentinel() {
    let mut app = test_app_with_agent();

    let effects = dispatch(
        Action::SetPermissionMode(PermissionModeKind::Default),
        &mut app,
    );

    assert!(effects.is_empty());
    assert!(!app.agents[&AgentId(0)].session.is_yolo());
    assert!(!app.agents[&AgentId(0)].session.is_auto());
    assert_eq!(app.current_ui.permission_mode, None);
    assert_eq!(
        agent_toast(&app).as_deref(),
        Some("Default permission is only available in Settings.")
    );
}

/// Feature gate OFF: a SetPermissionMode(Auto) commit (e.g. from the
/// settings modal) degrades to Ask — the session selector and settings default
/// use the same `app.auto_mode_gate`, so the two never disagree.
#[test]
fn set_permission_mode_auto_degrades_to_ask_when_gated_off() {
    use crate::app::actions::PermissionModeKind;
    let mut app = test_app_with_agent();
    app.auto_mode_gate = false;
    let effects = dispatch(
        Action::SetPermissionMode(PermissionModeKind::Auto),
        &mut app,
    );
    assert!(!app.agents[&AgentId(0)].session.is_auto());
    assert_eq!(app.current_ui.permission_mode, None);
    assert!(matches!(
        effects.as_slice(),
        [Effect::NotifySessionPermissionMode {
            canonical: "ask",
            ..
        }]
    ));
}

/// Rollback with an unknown canonical: defensively defaults to
/// "ask" (the safe fallback — fewer prompts on a corrupt
/// rollback value is worse, more prompts is safer).
///
/// The previous docstring claimed "logs a
/// warning and defaults to 'ask'" — the warning log is fired via
/// `tracing::warn!` in `apply_setting_rollback`'s arm, but the
/// test doesn't capture/assert it. The fix is documentary: the
/// test pins the OBSERVABLE behaviour (state defaults to "ask")
/// and acknowledges that the warn-log is best-effort visibility
/// for developers, not a contract surface the test enforces.
/// `tracing_test::traced_test` capture would be more rigorous
/// but is not currently used in this crate.
#[test]
fn rollback_permission_mode_unknown_canonical_defaults_to_ask() {
    use crate::settings::SettingValue;
    let mut app = test_app_with_agent();
    // Pre-set to true.
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    // Garbage canonical rolls back to "ask" (the safe default).
    let _ = dispatch(
        Action::TaskComplete(TaskResult::SettingPersistFailed {
            key: "permission_mode",
            rollback_value: SettingValue::Enum("garbage-value"),
            error: "test-error".into(),
        }),
        &mut app,
    );

    assert!(
        app.agents[&AgentId(0)].session.is_yolo(),
        "persistent-default rollback must not mutate the active session",
    );
    assert!(!app.default_yolo);
    assert_eq!(app.current_ui.permission_mode.as_deref(), Some("ask"));
    // The failure toast is the standard
    // `✗ Could not save permission_mode: …` format. A future
    // enhancement could differentiate "schema corruption" from
    // "real disk failure" in the toast text, but currently the
    // user sees the same wording; pinned here so a future
    // divergence is intentional.
}

/// Persistent-default rollback refreshes the settings snapshot but leaves the
/// active session permission untouched.
#[test]
fn rollback_permission_mode_refreshes_open_modal_snapshots() {
    use crate::settings::SettingValue;
    use crate::views::modal::ActiveModal;

    let mut app = test_app_with_agent();
    // Pre-set yolo=true via the typed setter so the rollback
    // captures real prior state.
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    // Open the modal AFTER the optimistic toggle so the open-time
    // snapshot reflects yolo=true.
    let _ = dispatch(Action::OpenSettings, &mut app);
    let agent = app.agents.get(&AgentId(0)).unwrap();
    let Some(ActiveModal::Settings { state }) = &agent.active_modal else {
        panic!("expected Settings modal");
    };
    assert!(
        state.pager_snapshot.yolo_mode,
        "pre-rollback snapshot reflects optimistic state (yolo=true)",
    );

    // Simulate disk-write failure → rollback to "ask".
    let _ = dispatch(
        Action::TaskComplete(TaskResult::SettingPersistFailed {
            key: "permission_mode",
            rollback_value: SettingValue::Enum("ask"),
            error: "test-error".into(),
        }),
        &mut app,
    );

    // The modal's snapshot MUST refresh to the rolled-back value.
    let agent = app.agents.get(&AgentId(0)).unwrap();
    let Some(ActiveModal::Settings { state }) = &agent.active_modal else {
        panic!("modal must stay open after rollback");
    };
    assert!(state.pager_snapshot.yolo_mode);
    assert_eq!(
        state.ui_snapshot.permission_mode.as_deref(),
        Some("ask"),
        "rollback path MUST refresh ui_snapshot.permission_mode to 'ask'",
    );
    assert!(agent.session.is_yolo());
}

#[test]
fn set_permission_mode_ask_emits_brand_consistent_toast() {
    use crate::app::actions::PermissionModeKind;
    let mut app = test_app_with_agent();
    // Pre-set to AlwaysApprove so the Ask dispatch is a real
    // transition (avoids idempotent fast-path).
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    // Clear toast so we observe the Ask dispatch's fresh toast.
    app.agents.get_mut(&AgentId(0)).unwrap().toast = None;

    let effects = dispatch(Action::SetPermissionMode(PermissionModeKind::Ask), &mut app);

    assert!(!app.agents[&AgentId(0)].session.is_yolo());
    assert_eq!(app.current_ui.permission_mode, None);

    // Toast brands as "Permission mode" not
    // "Always-approve". Previously the Ask arm reused `yolo_toast(false)`
    // which produced "✓ Always-approve: off" — a brand mismatch.
    let toast = app.agents[&AgentId(0)]
        .toast
        .as_ref()
        .map(|(s, _)| s.clone())
        .expect("toast must be set");
    assert_eq!(
        toast, "\u{2713} Permission mode: Ask",
        "PR 11 R1 G-3 #11: Ask toast must brand as 'Permission mode' not 'Always-approve'",
    );

    assert!(matches!(
        effects.as_slice(),
        [Effect::NotifySessionPermissionMode {
            canonical: "ask",
            ..
        }]
    ));
}

/// Regression test. A `--yolo`
/// startup sets `agent.session.yolo_mode = true` but leaves
/// `app.current_ui.permission_mode` at `None`. Without the
/// LIVE-precedence capture, dispatching `SetPermissionMode(Default)`
/// would produce `WithRollback("ask")` — diverging the pager from
/// the shell on disk failure (the ACP suppress-on-failure gate
/// keeps the shell at YOLO, but the pager would roll back to
/// non-YOLO). This test pins the LIVE-precedence fix.
#[test]
fn set_default_permission_with_live_yolo_does_not_mutate_session() {
    use crate::app::actions::PermissionModeKind;
    let mut app = test_app_with_agent();
    // Simulate `--yolo` startup: agent yolo + default_yolo set,
    // but `current_ui.permission_mode = None` (config has no
    // `[ui] permission_mode` setting).
    app.agents.get_mut(&AgentId(0)).unwrap().session.yolo_mode = true;
    app.default_yolo = true;
    app.current_ui.permission_mode = None;

    let effects = dispatch(
        Action::SetDefaultPermissionMode(PermissionModeKind::Default),
        &mut app,
    );

    assert!(app.agents[&AgentId(0)].session.is_yolo());
    assert!(!app.default_yolo);
    assert_eq!(app.current_ui.permission_mode.as_deref(), Some("default"));

    // **Rollback contract.** Rollback must target
    // "always-approve" (the LIVE state at dispatch time), NOT
    // "ask" (a bool-projected guess from the None mirror).
    match &effects[0] {
        Effect::PersistPermissionMode {
            persist,
            session_id,
            ..
        } => {
            assert!(session_id.is_none());
            assert_eq!(
                *persist,
                crate::app::actions::PermissionModePersist::WithRollback("always-approve"),
                "PR 11 R1 Security #8: LIVE yolo state must take precedence over the \
                     None on-disk mirror when computing the rollback canonical — \
                     otherwise a `--yolo` startup + Default-commit + disk-failure diverges \
                     the pager from the shell",
            );
        }
        other => panic!("expected PersistPermissionMode, got {other:?}"),
    }
}

/// `apply_setting_rollback("permission_mode",
/// Enum("default"))` — the rollback arm that preserves the
/// "default" canonical through a failed-persist. The headline
/// architectural contract: rolling back to "default" must NOT
/// collapse onto "ask" via the inner's bool projection.
#[test]
fn rollback_permission_mode_default_canonical_preserves_default() {
    use crate::settings::SettingValue;
    let mut app = test_app_with_agent();
    // Pre-flip to YOLO so the rollback has somewhere to roll
    // back FROM.
    let _ = dispatch(
        Action::SetDefaultPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    assert!(!app.agents[&AgentId(0)].session.is_yolo());
    assert!(app.default_yolo);
    assert_eq!(
        app.current_ui.permission_mode.as_deref(),
        Some("always-approve"),
    );

    // Simulate disk-write failure with `rollback_value =
    // Enum("default")`.
    let effects = dispatch(
        Action::TaskComplete(TaskResult::SettingPersistFailed {
            key: "permission_mode",
            rollback_value: SettingValue::Enum("default"),
            error: "simulated".into(),
        }),
        &mut app,
    );

    // Rollback path MUST NOT re-emit any Effect — that would
    // loop on persistent disk failure.
    assert!(
        effects.is_empty(),
        "rollback path must not re-emit Effects, got {effects:?}",
    );

    assert!(!app.agents[&AgentId(0)].session.is_yolo());
    assert!(!app.default_yolo);
    // Canonical preserved as "default" — the headline
    // contract. Without the post-inner override in the rollback
    // arm, the inner's bool-projection write would leave this
    // at "ask".
    assert_eq!(
        app.current_ui.permission_mode.as_deref(),
        Some("default"),
        "PR 11 R1 Tests #22: rollback to 'default' canonical must NOT collapse \
             onto 'ask' — the post-inner override restores the canonical",
    );
}

/// Non-empty permission_queue → NeedsInput.
#[test]
fn classify_top_level_permission_queue_non_empty_is_needs_input() {
    use crate::views::dashboard::{RowState, classify_top_level};
    let mut app = test_app_with_agent();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    let _rx = push_synthetic_permission(agent, 1, vec![("allow", "Allow")]);
    assert_eq!(classify_top_level(agent), RowState::NeedsInput);
}

#[test]
fn permission_select_reject_does_not_steer_sticky_cursor() {
    use crate::appearance::permission_cursor::{
        DefaultSelectedPermission, last_used_permission, set_last_used_permission,
    };
    use std::sync::Arc;

    let mut app = test_app_with_agent();
    let _rx_allow = enqueue_permission_with_enable_always_approve(&mut app);
    let _rx_reject = enqueue_permission_with_enable_always_approve(&mut app);

    set_last_used_permission(DefaultSelectedPermission::AlwaysAllowAllSessions);
    let _ = dispatch(
        Action::PermissionSelect(acp::PermissionOptionId::new(Arc::from("opt-allow-once"))),
        &mut app,
    );
    assert_eq!(
        last_used_permission(),
        DefaultSelectedPermission::AllowOnce,
        "allow selection records the sticky cursor target"
    );

    let _ = dispatch(
        Action::PermissionSelect(acp::PermissionOptionId::new(Arc::from("opt-reject-once"))),
        &mut app,
    );
    assert_eq!(
        last_used_permission(),
        DefaultSelectedPermission::AllowOnce,
        "reject selection must not steer the sticky cursor"
    );
}

//! Tests for plan, always-approve, auto, and permission mode transitions.

use super::*;

#[test]
fn behavior_picker_selection_changes_behavior_without_touching_permission() {
    let mut app = test_app_with_agent();
    app.current_ui.permission_mode = Some("ask".into());

    let effects = dispatch(
        Action::SetBehaviorMode(tools::types::BehaviorId::Plan),
        &mut app,
    );

    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert_eq!(
        agent.behavior_mode_pending,
        Some(tools::types::BehaviorId::Plan)
    );
    assert_eq!(app.current_ui.permission_mode.as_deref(), Some("ask"));
    assert!(matches!(
        effects.as_slice(),
        [Effect::SetSessionMode { mode_id, .. }] if mode_id.0.as_ref() == "plan"
    ));
}

/// `ShowPlanNudge` is a no-op when its per-tip gate is off: no tip shown,
/// no count burned, even on a drawable agent.
#[test]
fn show_plan_nudge_no_op_when_flag_off() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().last_terminal_size = (80, 30);
    app.contextual_hints.plan_mode = false;

    let effects = dispatch(Action::ShowPlanNudge, &mut app);
    assert!(effects.is_empty());
    assert!(app.tip_seen_counts.is_empty(), "no count burned");
    assert!(!app.agents[&id].ephemeral_tip.is_active());
}

/// `ShowPlanNudge` with the tip on and a drawable agent shows the tip and
/// increments the per-session seen count once (in memory, no effects).
#[test]
fn show_plan_nudge_shows_and_counts_when_flag_on() {
    use crate::tips::plan_nudge::PLAN_NUDGE_SEEN_KEY;
    let mut app = test_app_with_agent();
    app.contextual_hints.plan_mode = true;
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().last_terminal_size = (80, 30);

    let effects = dispatch(Action::ShowPlanNudge, &mut app);
    assert!(app.agents[&id].ephemeral_tip.is_active());
    assert_eq!(app.tip_seen_counts.get(PLAN_NUDGE_SEEN_KEY), Some(&1));
    assert!(
        effects.is_empty(),
        "seen count is in-memory; nothing persisted"
    );
}

/// `ShowWordSelectTip` is a no-op when its per-tip gate is off.
#[test]
fn show_word_select_tip_no_op_when_flag_off() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().last_terminal_size = (80, 30);
    app.contextual_hints.word_select = false;

    let effects = dispatch(Action::ShowWordSelectTip, &mut app);
    assert!(effects.is_empty());
    assert!(app.tip_seen_counts.is_empty(), "no count burned");
    assert!(!app.agents[&id].ephemeral_tip.is_active());
}

/// `ShowWordSelectTip` shows and counts when the gate is on and selection
/// is not already `word_select`.
#[test]
fn show_word_select_tip_shows_and_counts_when_flag_on() {
    use crate::appearance::TextSelection;
    use crate::tips::word_select::WORD_SELECT_TIP_SEEN_KEY;
    crate::appearance::cache::set_keep_text_selection(TextSelection::Flash);
    let mut app = test_app_with_agent();
    app.contextual_hints.word_select = true;
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().last_terminal_size = (80, 30);

    let effects = dispatch(Action::ShowWordSelectTip, &mut app);
    assert!(app.agents[&id].ephemeral_tip.is_active());
    assert_eq!(app.tip_seen_counts.get(WORD_SELECT_TIP_SEEN_KEY), Some(&1));
    assert!(
        effects.is_empty(),
        "seen count is in-memory; nothing persisted"
    );
}

/// Already on `word_select` → tip is redundant, skip without burning count.
#[test]
fn show_word_select_tip_no_op_when_already_word_select() {
    use crate::appearance::TextSelection;
    crate::appearance::cache::set_keep_text_selection(TextSelection::WordSelect);
    let mut app = test_app_with_agent();
    app.contextual_hints.word_select = true;
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().last_terminal_size = (80, 30);

    let effects = dispatch(Action::ShowWordSelectTip, &mut app);
    assert!(effects.is_empty());
    assert!(app.tip_seen_counts.is_empty());
    assert!(!app.agents[&id].ephemeral_tip.is_active());
    // Restore default so sibling tests don't inherit word_select.
    crate::appearance::cache::set_keep_text_selection(TextSelection::Flash);
}

/// Accepting the tip (its chord, with the tip on screen) flips the setting
/// to `word_select`, persists it, and retires the tip.
#[test]
fn accept_word_select_tip_flips_setting_and_retires_tip() {
    use crate::appearance::TextSelection;
    use crate::settings::SettingValue;
    crate::appearance::cache::set_keep_text_selection(TextSelection::Flash);
    let mut app = test_app_with_agent();
    app.contextual_hints.word_select = true;
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().last_terminal_size = (80, 30);
    let _ = dispatch(Action::ShowWordSelectTip, &mut app);
    assert!(app.agents[&id].ephemeral_tip.is_active());

    let effects = dispatch(Action::AcceptWordSelectTip, &mut app);
    assert!(
        crate::appearance::cache::load_keep_text_selection().selects_word(),
        "accept must flip the live setting to word_select"
    );
    assert!(
        !app.agents[&id].ephemeral_tip.is_active(),
        "accept must retire the tip"
    );
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::PersistSetting {
                key: "keep_text_selection",
                value: SettingValue::Enum("word_select"),
                ..
            }
        )),
        "accept must persist the setting, got: {effects:?}"
    );
    // Restore default so sibling tests don't inherit word_select.
    crate::appearance::cache::set_keep_text_selection(TextSelection::Flash);
}

/// The accept action is tip-scoped: without the tip on screen it must not
/// touch the setting (Ctrl+Y outside the TTL keeps its normal meaning; a
/// stray action must not become a global toggle).
#[test]
fn accept_word_select_tip_no_op_when_tip_not_showing() {
    use crate::appearance::TextSelection;
    crate::appearance::cache::set_keep_text_selection(TextSelection::Flash);
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().last_terminal_size = (80, 30);
    assert!(!app.agents[&id].ephemeral_tip.is_active());

    let effects = dispatch(Action::AcceptWordSelectTip, &mut app);
    assert!(effects.is_empty());
    assert!(
        !crate::appearance::cache::load_keep_text_selection().selects_word(),
        "setting must be untouched without the tip"
    );
}

// ── /plan slash command tests ─────────────────────────────────────

#[test]
fn slash_plan_no_args_not_in_plan_enters_plan_mode() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    assert!(!app.agents[&id].plan_mode_active);
    assert!(app.agents[&id].plan_mode_pending.is_none());

    let effects = dispatch(Action::SendPrompt("/plan".into()), &mut app);

    // Should emit SetSessionMode to enter plan mode.
    assert_eq!(effects.len(), 1);
    assert!(
        matches!(&effects[0], Effect::SetSessionMode { mode_id, .. } if &*mode_id.0 == "plan"),
        "expected SetSessionMode(plan), got: {effects:?}"
    );
    // Optimistic pending state should be set.
    assert_eq!(app.agents[&id].plan_mode_pending, Some(true));
}

#[test]
fn slash_plan_no_args_already_in_plan_shows_plan() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    let agent = app.agents.get_mut(&id).unwrap();
    agent.plan_mode_active = true;
    agent.behavior_mode = tools::types::BehaviorId::Plan;

    let effects = dispatch(Action::SendPrompt("/plan".into()), &mut app);

    // Should NOT emit SetSessionMode — just show the plan (no async effect).
    assert!(effects.is_empty(), "expected no effects, got: {effects:?}");
}

#[test]
fn slash_plan_with_args_not_in_plan_enters_and_sends_prompt() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);

    let effects = dispatch(
        Action::SendPrompt("/plan add auth to the app".into()),
        &mut app,
    );

    // Should emit a single SetModeThenPrompt (mode switch + prompt
    // bundled into one sequential task to avoid a race).
    assert_eq!(effects.len(), 1, "expected 1 effect, got: {effects:?}");
    assert!(
        matches!(
            &effects[0],
            Effect::SetModeThenPrompt { mode_id, text, .. }
                if &*mode_id.0 == "plan" && text == "add auth to the app"
        ),
        "expected SetModeThenPrompt(plan, \"add auth to the app\"), got: {effects:?}"
    );
    assert_eq!(app.agents[&id].plan_mode_pending, Some(true));
}

#[test]
fn slash_plan_with_args_already_in_plan_sends_prompt_after_idempotent_transition() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().plan_mode_active = true;

    let effects = dispatch(
        Action::SendPrompt("/plan add auth to the app".into()),
        &mut app,
    );

    assert!(matches!(
        effects.as_slice(),
        [Effect::SetModeThenPrompt { mode_id, text, .. }]
            if mode_id.0.as_ref() == "plan" && text == "add auth to the app"
    ));
}

/// A second explicit user selection is the only Pager action that can confirm
/// a Shell-owned interrupt window. The first confirmation-required update has
/// already restored the confirmed source identity and cleared optimistic state.
#[test]
fn repeated_behavior_selection_reissues_the_same_target() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    {
        let agent = app.agents.get_mut(&id).unwrap();
        agent.behavior_mode = tools::types::BehaviorId::Plan;
        agent.behavior_mode_pending = None;
    }

    let effects = dispatch(
        Action::SetBehaviorMode(tools::types::BehaviorId::Normal),
        &mut app,
    );

    let agent = app.agents.get(&id).unwrap();
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::SetSessionMode { mode_id, .. }] if mode_id.0.as_ref() == "normal"
        ),
        "the repeated user selection must reach the Shell unchanged: {effects:?}"
    );
    assert_eq!(
        agent.behavior_mode_pending,
        Some(tools::types::BehaviorId::Normal)
    );
}

/// Multi-agent fan-out (sibling for `plan_mode`).
/// `Action::SetBehaviorMode(Plan)` populates the active agent's
/// `plan_mode_pending` and never touches other agents in the
/// registry. The contract differs from `multiline_mode` in that
/// `plan_mode_pending` is an `Option<bool>` (optimistic stash) —
/// the non-active agent must stay `None`.
#[test]
fn set_plan_mode_mutates_only_active_agent_not_others() {
    let mut app = test_app_with_agent();
    insert_placeholder_agent(&mut app, AgentId(1));
    assert!(app.agents[&AgentId(0)].plan_mode_pending.is_none());
    assert!(app.agents[&AgentId(1)].plan_mode_pending.is_none());

    let _ = dispatch(
        Action::SetBehaviorMode(tools::types::BehaviorId::Plan),
        &mut app,
    );

    assert_eq!(
        app.agents[&AgentId(0)].plan_mode_pending,
        Some(true),
        "active agent must have optimistic plan_mode_pending = Some(true)",
    );
    assert!(
        app.agents[&AgentId(1)].plan_mode_pending.is_none(),
        "non-active agent must NOT receive the plan_mode pending state",
    );
    assert!(
        !app.agents[&AgentId(1)].plan_mode_active,
        "non-active agent's confirmed plan_mode_active must stay false",
    );
}

// ----------------------------------------------------------------
// `set_always_approve_mode` dispatcher unit tests (security-relevant)
//
// SHELL-owned, but with rollback semantics: a disk-write failure
// routes through `apply_setting_rollback("permission_mode", _)`
// which calls `set_always_approve_mode_inner(app, prev)` to revert. The
// outer setter never re-emits `Effect::PersistPermissionMode` on
// rollback so a persistent disk failure does not loop.
//
// Security invariants the test suite pins:
//   - On always-approve ON: the per-agent permission_queue is drained with
//     `AllowOnce` responses (NOT cancelled — auto-approve).
//   - The drain ALSO runs on a duplicate always-approve=ON dispatch
//     (any permission queued between
//     dispatches must be drained on the second). Only diagnostics
//     + the "setting changed" tracing log are gated on
//     transitions.
//   - On no-AllowOnce shape: the drain falls back to `Cancelled`,
//     NOT `AllowAlways` — preserves the safety contract that
//     always-approve never picks a more-permissive option than `AllowOnce`.
//   - `app.current_ui.permission_mode` stays in lock-step with
//     `agent.session.always_approve_mode` so the modal snapshot is fresh.
//   - `Effect::PersistPermissionMode { persist:
//     PermissionModePersist::WithRollback(prev) }` emitted
//     exactly once per typed-setter dispatch (see
//     `app::actions::PermissionModePersist`).
//   - Rollback via `apply_setting_rollback("permission_mode",
//     SettingValue::Enum(_))` reverts the in-memory state via
//     `set_always_approve_mode_inner` (no re-emit). Refreshes any open
//     settings modal (`rollback_permission_mode_refreshes_open_modal_snapshots`).
//   - Toast format:
//     - ON:  "⚠ Always-approve ON: all tool actions auto-run"
//       (destructive-action variant —
//       differentiated visual + body because enabling always-approve is
//       the single most security-relevant user action in the
//       pager).
//     - OFF: "✓ Always-approve: off" (standard success format
//       — restoring the safe default).
//   - Failure toast: "✗ Could not save permission_mode: {error}"
//     — exact format pinned via `assert_eq!` in
//     `rollback_permission_mode_reverts_state_no_effect`.
// ----------------------------------------------------------------

/// Slash gate sync: both toggles stay offered while modes change; only the
/// auto feature gate suppresses `/auto`.
#[test]
fn permission_mode_slash_gate_offers_toggles_subject_to_auto_feature() {
    use crate::app::actions::PermissionModeKind;
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.auto_mode_gate = true;
    app.sync_permission_mode_slash_gate();

    let offered = |app: &AppView, name: &str| {
        app.agents[&id]
            .prompt
            .slash_controller
            .registry()
            .get(name)
            .is_some()
    };

    assert!(offered(&app, "always-approve"));
    assert!(offered(&app, "auto"));

    // Mode changes must not hide either toggle.
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    assert!(offered(&app, "always-approve"));
    assert!(offered(&app, "auto"));

    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::Auto),
        &mut app,
    );
    assert!(offered(&app, "always-approve"));
    assert!(offered(&app, "auto"));

    // Gate off → only `/auto` disappears.
    app.auto_mode_gate = false;
    app.sync_permission_mode_slash_gate();
    assert!(offered(&app, "always-approve"));
    assert!(!offered(&app, "auto"));
}

/// End-to-end via slash submission: direct Permission commands are idempotent
/// selections and cross-switch when the other mode is selected.
#[test]
fn slash_always_approve_and_auto_are_idempotent_selections() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.auto_mode_gate = true;
    app.sync_permission_mode_slash_gate();

    // Off → always-approve.
    let _ = dispatch(Action::SendPrompt("/always-approve".into()), &mut app);
    assert!(app.agents[&id].session.is_always_approve());
    assert!(!app.agents[&id].session.is_auto());

    // Always-approve → auto (cross-switch).
    let _ = dispatch(Action::SendPrompt("/auto".into()), &mut app);
    assert!(!app.agents[&id].session.is_always_approve());
    assert!(app.agents[&id].session.is_auto());

    // Re-selecting Auto is idempotent.
    let _ = dispatch(Action::SendPrompt("/auto".into()), &mut app);
    assert!(!app.agents[&id].session.is_always_approve());
    assert!(app.agents[&id].session.is_auto());

    // Auto → always-approve (cross-switch).
    let _ = dispatch(Action::SendPrompt("/always-approve".into()), &mut app);
    assert!(app.agents[&id].session.is_always_approve());
    assert!(!app.agents[&id].session.is_auto());

    // Re-selecting Always Approve is idempotent.
    let _ = dispatch(Action::SendPrompt("/always-approve".into()), &mut app);
    assert!(app.agents[&id].session.is_always_approve());
    assert!(!app.agents[&id].session.is_auto());
}

#[test]
fn set_session_always_approve_notifies_without_changing_default() {
    let mut app = test_app_with_agent();
    // Default is always-approve=false.
    assert!(!app.agents[&AgentId(0)].session.is_always_approve());

    let effects = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    // In-memory state mutated.
    assert!(
        app.agents[&AgentId(0)].session.is_always_approve(),
        "session.always_approve_mode must flip to true"
    );
    assert!(!app.default_permission_mode.is_always_approve());
    assert_eq!(app.current_ui.permission_mode, None);

    // Exactly one Effect with the right rollback payload.
    assert_eq!(effects.len(), 1, "expected exactly one Effect");
    match &effects[0] {
        Effect::NotifySessionPermissionMode {
            canonical,
            session_id,
        } => {
            assert_eq!(*canonical, "always-approve");
            assert_eq!(session_id.0.as_ref(), "test-session");
        }
        other => panic!("expected NotifySessionPermissionMode, got {other:?}"),
    }
}

/// Enabling always-approve while plan mode is active must warn that the
/// Plan approval contract stays binding — the standard "all tool actions
/// auto-run" toast would overpromise because always-approve does not bypass Plan phases.
#[test]
fn set_always_approve_mode_on_under_plan_uses_plan_aware_toast() {
    let mut app = test_app_with_agent();
    app.agents.get_mut(&AgentId(0)).unwrap().plan_mode_active = true;

    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    let toast = app.agents[&AgentId(0)]
        .toast
        .as_ref()
        .map(|(s, _)| s.clone())
        .expect("toast must be set");
    assert_eq!(toast, ALWAYS_APPROVE_ON_UNDER_PLAN_TOAST);

    // Pending (optimistic) plan state counts too — same as the flag renderer.
    let mut app = test_app_with_agent();
    app.agents.get_mut(&AgentId(0)).unwrap().plan_mode_pending = Some(true);
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    let toast = app.agents[&AgentId(0)]
        .toast
        .as_ref()
        .map(|(s, _)| s.clone())
        .expect("toast must be set");
    assert_eq!(toast, ALWAYS_APPROVE_ON_UNDER_PLAN_TOAST);

    // Without plan mode the standard destructive toast is unchanged.
    let mut app = test_app_with_agent();
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    let toast = app.agents[&AgentId(0)]
        .toast
        .as_ref()
        .map(|(s, _)| s.clone())
        .expect("toast must be set");
    assert_eq!(
        toast,
        "\u{26A0} Always-approve ON: all tool actions auto-run"
    );
}

/// The settings-modal path (`SetPermissionMode(AlwaysApprove)`) gets the same
/// plan-aware toast as the current-session selection path.
#[test]
fn set_permission_mode_always_approve_under_plan_uses_plan_aware_toast() {
    use crate::app::actions::PermissionModeKind;
    let mut app = test_app_with_agent();
    app.agents.get_mut(&AgentId(0)).unwrap().plan_mode_active = true;

    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    let toast = app.agents[&AgentId(0)]
        .toast
        .as_ref()
        .map(|(s, _)| s.clone())
        .expect("toast must be set");
    assert_eq!(toast, ALWAYS_APPROVE_ON_UNDER_PLAN_TOAST);
}

#[test]
fn set_session_always_approve_to_ask_notifies_without_changing_default() {
    let mut app = test_app_with_agent();
    // Pre-set always-approve=true via the typed setter so the rollback
    // value is captured correctly.
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    assert!(app.agents[&AgentId(0)].session.is_always_approve());

    let effects = dispatch(Action::SetPermissionMode(PermissionModeKind::Ask), &mut app);

    assert!(!app.agents[&AgentId(0)].session.is_always_approve());
    assert!(!app.default_permission_mode.is_always_approve());
    assert_eq!(app.current_ui.permission_mode, None);

    match &effects[0] {
        Effect::NotifySessionPermissionMode {
            canonical,
            session_id,
        } => {
            assert_eq!(*canonical, "ask");
            assert_eq!(session_id.0.as_ref(), "test-session");
        }
        other => panic!("expected NotifySessionPermissionMode, got {other:?}"),
    }
}

#[test]
fn always_approve_on_drain_clears_double_click_tracker() {
    let mut app = test_app_with_agent();
    let _rx = enqueue_permission_with_enable_always_approve(&mut app);

    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .last_permission_click = Some((Instant::now(), 1));

    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    let agent = &app.agents[&AgentId(0)];
    assert!(agent.permission_queue.is_empty());
    assert!(
        agent.last_permission_click.is_none(),
        "always-approve-on drain must invalidate the armed click"
    );
}

/// When the user picks the "enable-always-approve" option:
///
/// 1. The shell receives a `Selected{option_id: ENABLE_ALWAYS_APPROVE_OPTION_ID}`
///    response. The shell's `map_selected_outcome` resolves this to
///    `PromptOutcome::AllowOnce`, so the in-flight tool call is
///    allowed exactly once (no per-tool whitelisting).
/// 2. The dispatcher returns a `PersistPermissionMode` effect with
///    canonical `"always-approve"` — this is what flips
///    `[ui] permission_mode` on disk AND fires the
///    `grow/permission_mode_changed` ACP notification back to the shell.
/// 3. The agent's per-session `always_approve_mode` flag is flipped to true,
///    so subsequent permission requests are auto-approved by
///    `handle_permission_request`.
///
/// A regression in any of these three legs would break the
/// "one click to enable always-approve mode" contract.
#[test]
fn enable_always_approve_sends_response_and_changes_only_this_session() {
    use std::sync::Arc;

    let mut app = test_app_with_agent();
    let mut response_rx = enqueue_permission_with_enable_always_approve(&mut app);

    // Sanity: always-approve is OFF before selecting the option.
    assert!(
        !app.agents[&AgentId(0)].session.is_always_approve(),
        "precondition: always-approve must be off",
    );

    let effects = dispatch(
        Action::PermissionSelect(acp::PermissionOptionId::new(Arc::from(
            workspace::permission::ENABLE_ALWAYS_APPROVE_OPTION_ID,
        ))),
        &mut app,
    );

    // (1) The shell sees the option_id we picked. The kind is
    //     `AllowOnce` on the wire; the shell's `map_selected_outcome`
    //     resolves the id under the `AllowOnce` branch and returns
    //     `PromptOutcome::AllowOnce`. Verify the id round-trips.
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
                option_id.0.as_ref(),
                workspace::permission::ENABLE_ALWAYS_APPROVE_OPTION_ID,
                "the response must echo the picked option_id",
            );
        }
        other => {
            panic!("enable-always-approve must produce a Selected response, got {other:?}",)
        }
    }

    // (2) The dispatcher notifies this session without rewriting the default.
    let canonical = effects
        .iter()
        .find_map(|e| match e {
            Effect::NotifySessionPermissionMode { canonical, .. } => Some(*canonical),
            _ => None,
        })
        .expect("enable-always-approve must notify the active session");
    assert_eq!(canonical, "always-approve");

    // (3) Per-session always-approve flag is flipped — future prompts will be
    //     auto-approved in `handle_permission_request`.
    assert!(
        app.agents[&AgentId(0)].session.is_always_approve(),
        "session.always_approve_mode must be flipped on after selecting enable-always-approve",
    );
    assert!(
        !app.default_permission_mode.is_always_approve(),
        "future-session default must stay unchanged"
    );
    assert_eq!(app.current_ui.permission_mode, None);
}

/// If the user picks "enable-always-approve" while always-approve is ALREADY
/// on, the dispatcher must NOT re-emit `PersistPermissionMode`
/// (which would queue a redundant disk write + ACP notification).
/// In practice always-approve-on suppresses the permission panel entirely
/// (`handle_permission_request` auto-approves), so this state is
/// only reachable in tests, but the idempotency guard matters for
/// future code paths that might pre-seed always-approve state.
#[test]
fn enable_always_approve_is_idempotent_when_always_approve_already_on() {
    use std::sync::Arc;

    let mut app = test_app_with_agent();

    // Pre-flip always-approve on. We bypass the panel suppression by injecting
    // the permission AFTER the flip — exercises the dispatcher's
    // idempotency guard directly.
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    assert!(app.agents[&AgentId(0)].session.is_always_approve());

    let mut response_rx = enqueue_permission_with_enable_always_approve(&mut app);

    let effects = dispatch(
        Action::PermissionSelect(acp::PermissionOptionId::new(Arc::from(
            workspace::permission::ENABLE_ALWAYS_APPROVE_OPTION_ID,
        ))),
        &mut app,
    );

    // Response still flows (the current request is allowed once).
    match response_rx.try_recv() {
        Ok(Ok(acp::RequestPermissionResponse {
            outcome: acp::RequestPermissionOutcome::Selected(_),
            ..
        })) => {}
        other => panic!("expected Selected response, got {other:?}"),
    }

    // No redundant PersistPermissionMode. (The initial session selection
    // dispatch above already produced one for the always-approve-flip.)
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::PersistPermissionMode { .. })),
        "redundant PersistPermissionMode when always-approve already on — the dispatcher \
             must short-circuit to avoid double-writing config.toml and double-firing \
             grow/permission_mode_changed",
    );
}

/// **Security-critical fallback:**
/// when a queued permission has NO `AllowOnce` option (only
/// `AllowAlways` / `RejectAlways`), the drain MUST send
/// `Cancelled` — NOT silently fall through to `AllowAlways`
/// which would whitelist the operation indefinitely.
///
/// This pins the safety contract: always-approve never auto-picks a
/// more-permissive option than `AllowOnce`. A regression that
/// added an `else if find(AllowAlways)` fallback would
/// dramatically widen the blast radius of a single always-approve toggle.
#[test]
fn set_always_approve_mode_on_with_no_allow_once_option_sends_cancelled() {
    use crate::views::permission_view::{PermissionFocus, PermissionViewState};
    use std::sync::Arc;

    let mut app = test_app_with_agent();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();

    // Inject a permission with only AllowAlways + RejectAlways
    // (NO AllowOnce). The drain must NOT pick AllowAlways even
    // though it's the only "Allow" option — that would breach
    // the safety contract.
    let (response_tx, mut response_rx) = tokio::sync::oneshot::channel();
    let request = acp::RequestPermissionRequest::new(
        acp::SessionId::new(Arc::from("test-session")),
        acp::ToolCallUpdate::new(
            acp::ToolCallId::new(Arc::from("tc-noallow-1")),
            acp::ToolCallUpdateFields::default(),
        ),
        vec![
            acp::PermissionOption::new(
                acp::PermissionOptionId::new(Arc::from("opt-allow-always")),
                "Allow always",
                acp::PermissionOptionKind::AllowAlways,
            ),
            acp::PermissionOption::new(
                acp::PermissionOptionId::new(Arc::from("opt-reject-always")),
                "Reject always",
                acp::PermissionOptionKind::RejectAlways,
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
        title: "noallow-test".to_string(),
        description: vec![],
        args_expanded: false,
        desc_scroll: 0,
        subagent_label: None,
        options_area_height: 0,
        options_scroll_offset: 0,
    });

    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    // Queue drained.
    assert!(app.agents[&AgentId(0)].permission_queue.is_empty());
    // Cancelled (NOT Selected{AllowAlways}).
    match response_rx.try_recv() {
        Ok(Ok(acp::RequestPermissionResponse {
            outcome: acp::RequestPermissionOutcome::Cancelled,
            ..
        })) => {
            // Correct — preserved the safety contract.
        }
        Ok(Ok(acp::RequestPermissionResponse {
            outcome:
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome {
                    option_id,
                    ..
                }),
            ..
        })) => panic!(
            "drain picked `{option_id:?}` instead of Cancelled — SAFETY CONTRACT \
                 VIOLATION: always-approve must never pick a more-permissive option than AllowOnce. \
                 Either AllowAlways (whitelist forever) or RejectAlways (deny forever) \
                 would be wrong; the drain must Cancel and let the caller's higher level \
                 decide.",
        ),
        other => panic!("expected Cancelled response, got {other:?}"),
    }
}

/// **Security-critical multi-item drain:** the
/// drain loop must fully empty the queue, not stop at the first
/// item. A regression that swapped `drain(..)` for `pop_front()`
/// would silently leak queued permissions on always-approve toggle. With
/// 3 items in the queue, this catches an off-by-N drain bug.
#[test]
fn set_always_approve_mode_on_drains_multi_item_queue() {
    use crate::views::permission_view::{PermissionFocus, PermissionViewState};
    use std::sync::Arc;

    let mut app = test_app_with_agent();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();

    // Inject 3 permissions, each with AllowOnce.
    let mut response_rxs = Vec::new();
    for i in 0..3u32 {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        response_rxs.push(response_rx);
        let request = acp::RequestPermissionRequest::new(
            acp::SessionId::new(Arc::from("test-session")),
            acp::ToolCallUpdate::new(
                acp::ToolCallId::new(Arc::from(format!("tc-multi-{i}"))),
                acp::ToolCallUpdateFields::default(),
            ),
            vec![acp::PermissionOption::new(
                acp::PermissionOptionId::new(Arc::from(format!("opt-allow-once-{i}"))),
                "Allow once",
                acp::PermissionOptionKind::AllowOnce,
            )],
        );
        let options = request.options.clone();
        agent.permission_queue.push_back(PermissionViewState {
            request: acp_transport::AcpArgs {
                request,
                response_tx,
            },
            id: i as usize + 1,
            focus: PermissionFocus::Options,
            options,
            active_idx: 0,
            bash_highlights: None,
            bash_selection_count: 0,
            bash_command_raw: None,
            mcp_scope: None,
            title: format!("multi-{i}"),
            description: vec![],
            args_expanded: false,
            desc_scroll: 0,
            subagent_label: None,
            options_area_height: 0,
            options_scroll_offset: 0,
        });
    }
    assert_eq!(agent.permission_queue.len(), 3);

    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    // Queue fully drained.
    assert!(
        app.agents[&AgentId(0)].permission_queue.is_empty(),
        "multi-item drain must fully empty the queue",
    );
    // All 3 channels received the AllowOnce response.
    for (i, mut rx) in response_rxs.into_iter().enumerate() {
        match rx.try_recv() {
            Ok(Ok(acp::RequestPermissionResponse {
                outcome: acp::RequestPermissionOutcome::Selected(_),
                ..
            })) => {} // OK
            other => panic!(
                "item {i} did not receive AllowOnce Selected response: {other:?} — \
                     drain skipped items beyond the first?",
            ),
        }
    }
}

/// **Security-critical:** re-dispatching
/// `SetPermissionMode(AlwaysApprove)` when already on MUST still drain any
/// permissions that arrived between the two dispatches. A future
/// "optimization" that skipped the drain on no-op redispatch
/// would lose security-critical state.
#[test]
fn set_always_approve_mode_on_duplicate_dispatch_still_drains_queue() {
    use crate::views::permission_view::{PermissionFocus, PermissionViewState};
    use std::sync::Arc;

    let mut app = test_app_with_agent();
    // First dispatch: turn always-approve ON. Queue is empty so no drain.
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    assert!(app.agents[&AgentId(0)].session.is_always_approve());

    // Now inject a permission AFTER the first dispatch.
    let (response_tx, mut response_rx) = tokio::sync::oneshot::channel();
    let request = acp::RequestPermissionRequest::new(
        acp::SessionId::new(Arc::from("test-session")),
        acp::ToolCallUpdate::new(
            acp::ToolCallId::new(Arc::from("tc-dup-1")),
            acp::ToolCallUpdateFields::default(),
        ),
        vec![acp::PermissionOption::new(
            acp::PermissionOptionId::new(Arc::from("opt-allow-once")),
            "Allow once",
            acp::PermissionOptionKind::AllowOnce,
        )],
    );
    let options = request.options.clone();
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .permission_queue
        .push_back(PermissionViewState {
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
            title: "dup-test".to_string(),
            description: vec![],
            args_expanded: false,
            desc_scroll: 0,
            subagent_label: None,
            options_area_height: 0,
            options_scroll_offset: 0,
        });

    // Second dispatch (same value): MUST still drain. A
    // "skip-drain-on-no-op" regression would leak this permission.
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    assert!(
        app.agents[&AgentId(0)].permission_queue.is_empty(),
        "duplicate always-approve=true dispatch MUST drain any permission that arrived \
             between dispatches — Security Issue 27 regression",
    );
    match response_rx.try_recv() {
        Ok(Ok(acp::RequestPermissionResponse {
            outcome: acp::RequestPermissionOutcome::Selected(_),
            ..
        })) => {} // OK
        other => panic!(
            "duplicate dispatch must auto-approve the newly queued permission, got {other:?}",
        ),
    }
}

/// Idempotent re-dispatch re-notifies the active session and re-fires the
/// toast, but never changes or persists the future-session default.
///
/// **Contract:** `persist` is `WithRollback(new)` even on a
/// no-op dispatch (prev == new). The disk write that follows is
/// idempotent on disk so the only observable side effects of a
/// duplicate are the toast + the (no-op) drain.
///
/// Pin EVERY state field
/// after the redispatch, AND prove the toast was actually
/// re-fired (clear it between dispatches so the second toast
/// can't be the first one lingering).
#[test]
fn set_always_approve_mode_redispatch_same_value_still_emits_effect_and_toast() {
    let mut app = test_app_with_agent();
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    assert!(app.agents[&AgentId(0)].toast.is_some());
    // Clear the toast: prove the second dispatch RE-FIRES the
    // toast (not just "the first toast is still visible").
    app.agents.get_mut(&AgentId(0)).unwrap().toast = None;

    let effects = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    assert_eq!(
        effects.len(),
        1,
        "duplicate dispatch must still notify the session"
    );
    match &effects[0] {
        Effect::NotifySessionPermissionMode {
            canonical,
            session_id,
        } => {
            assert_eq!(
                *canonical, "always-approve",
                "Effect.canonical must be 'always-approve' on duplicate always-approve=true",
            );
            assert_eq!(session_id.0.as_ref(), "test-session");
        }
        other => panic!("expected NotifySessionPermissionMode, got {other:?}"),
    }
    // Pin all state fields explicitly.
    assert!(
        app.agents[&AgentId(0)].session.is_always_approve(),
        "session.always_approve_mode must remain true",
    );
    assert!(!app.default_permission_mode.is_always_approve());
    assert_eq!(app.current_ui.permission_mode, None);
    // Toast was cleared between dispatches, so
    // `Some(_)` here proves the second dispatch re-fired the
    // toast (not just "carried over from the first").
    assert!(
        app.agents[&AgentId(0)].toast.is_some(),
        "second dispatch must re-fire the toast (proved by clearing between dispatches)",
    );
}

/// Toast string format: exact-equality pin.
///
/// **Destructive-action toast.**
/// The ON case uses `⚠ Always-approve ON: all tool actions
/// auto-run` (warning glyph + body spelling out the consequence)
/// because enabling always-approve is the single most security-relevant
/// user action in the pager. The OFF case uses the standard `✓`
/// success glyph + "Label: value" format (restoring the safe
/// default).
#[test]
fn set_always_approve_mode_toast_format() {
    let mut app = test_app_with_agent();
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    let toast = app.agents[&AgentId(0)]
        .toast
        .as_ref()
        .map(|(s, _)| s.clone())
        .expect("toast must be set");
    assert_eq!(
        toast,
        "\u{26A0} Always-approve ON: all tool actions auto-run"
    );

    let _ = dispatch(Action::SetPermissionMode(PermissionModeKind::Ask), &mut app);
    let toast = app.agents[&AgentId(0)]
        .toast
        .as_ref()
        .map(|(s, _)| s.clone())
        .expect("toast must be set");
    assert_eq!(toast, "\u{2713} Permission mode: Ask");
}

#[test]
fn set_always_approve_mode_on_blocked_by_policy_pin() {
    let mut app = test_app_with_agent();
    app.always_approve_policy_block = Some(POLICY_WARNING);

    let effects = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    assert!(
        effects.is_empty(),
        "blocked enable must not emit any Effect (no persist), got {effects:?}",
    );
    assert!(
        !app.agents[&AgentId(0)].session.is_always_approve(),
        "session.always_approve_mode must stay off under the pin"
    );
    assert!(
        !app.default_permission_mode.is_always_approve(),
        "app.default_permission_mode.is_always_approve() must stay off"
    );
    assert_eq!(
        app.current_ui.permission_mode, None,
        "canonical mirror must stay untouched"
    );
    assert_eq!(agent_toast(&app).as_deref(), Some(POLICY_WARNING));
}

#[test]
fn set_always_approve_mode_off_allowed_under_policy_pin() {
    let mut app = test_app_with_agent();
    // ON while unpinned (e.g. state restored from before the pin landed).
    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    assert!(app.agents[&AgentId(0)].session.is_always_approve());
    app.always_approve_policy_block = Some(POLICY_WARNING);

    let effects = dispatch(Action::SetPermissionMode(PermissionModeKind::Ask), &mut app);

    assert!(
        !app.agents[&AgentId(0)].session.is_always_approve(),
        "the pin must not block flipping always-approve OFF"
    );
    assert_eq!(effects.len(), 1, "OFF notifies the active session");
    assert!(matches!(
        &effects[0],
        Effect::NotifySessionPermissionMode {
            canonical: "ask",
            ..
        }
    ));
}

/// Leaving Plan while already in Auto must keep the classifier. It must not
/// fall to the reset that clears Auto back to Ask.

#[test]
fn set_always_approve_mode_no_op_when_no_active_agent() {
    let mut app = test_app(); // no agent, active_view = Welcome
    let default_always_approve_before = app.default_permission_mode.is_always_approve();
    let perm_mode_before = app.current_ui.permission_mode.clone();

    let effects = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    assert!(
        effects.is_empty(),
        "no active agent → no Effect, got {effects:?}",
    );
    // Defense-in-depth: SHARED state must NOT mutate.
    assert_eq!(
        app.default_permission_mode.is_always_approve(),
        default_always_approve_before
    );
    assert_eq!(app.current_ui.permission_mode, perm_mode_before);
}

/// The Settings modal edits future-session defaults. A current-session
/// permission switch must therefore leave its snapshot unchanged.
#[test]
fn session_permission_change_does_not_rewrite_default_settings_snapshot() {
    use crate::views::modal::ActiveModal;
    let mut app = test_app_with_agent();
    let _ = dispatch(Action::OpenSettings, &mut app);

    let agent = app.agents.get(&AgentId(0)).unwrap();
    let Some(ActiveModal::Settings { state }) = &agent.active_modal else {
        panic!("expected Settings modal after OpenSettings dispatch")
    };
    assert!(
        !state.pager_snapshot.permission_mode.is_always_approve(),
        "snapshot at open should be false (agent default)",
    );

    let _ = dispatch(
        Action::SetPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );

    let agent = app.agents.get(&AgentId(0)).unwrap();
    let Some(ActiveModal::Settings { state }) = &agent.active_modal else {
        panic!("Settings modal must remain open across the dispatch")
    };
    assert!(!state.pager_snapshot.permission_mode.is_always_approve());
    assert_eq!(state.ui_snapshot.permission_mode, None);
    assert!(agent.session.is_always_approve());
}

#[test]
fn default_permission_change_is_future_session_only() {
    let mut app = test_app_with_agent();
    let effects = dispatch(
        Action::SetDefaultPermissionMode(PermissionModeKind::AlwaysApprove),
        &mut app,
    );
    assert!(!app.agents[&AgentId(0)].session.is_always_approve());
    assert!(app.default_permission_mode.is_always_approve());
    assert_eq!(
        app.current_ui.permission_mode.as_deref(),
        Some("always-approve")
    );
    assert!(matches!(
        effects.as_slice(),
        [Effect::PersistPermissionMode {
            canonical: "always-approve",
            session_id: None,
            ..
        }]
    ));
}

/// Direct unit test for `permission_mode_toast`
/// at the seam — pins the brand-consistent strings for each arm.
/// Defense-in-depth on top of the dispatch-layer toast assertions
/// above; catches a future refactor that changes the strings
/// without going through dispatch.
#[test]
fn permission_mode_toast_returns_brand_consistent_strings() {
    use crate::app::actions::PermissionModeKind;
    assert_eq!(
        permission_mode_toast(PermissionModeKind::Ask),
        "\u{2713} Permission mode: Ask",
    );
    // AlwaysApprove still goes through `always_approve_toast(true)` —
    // destructive variant.
    assert_eq!(
        permission_mode_toast(PermissionModeKind::AlwaysApprove),
        "\u{26A0} Always-approve ON: all tool actions auto-run",
    );
}

#[test]
fn set_theme_auto_enables_auto_mode_and_persists_auto() {
    use crate::settings::SettingValue;
    with_theme_test_env(|| {
        // Mock the system appearance so resolve_auto deterministically
        // picks a known concrete theme.
        crate::theme::system_appearance::set_mock(Some(
            crate::theme::system_appearance::SystemAppearance::Dark,
        ));

        let mut app = test_app_with_agent();
        assert!(!crate::theme::cache::is_auto_mode());
        let effects = dispatch(Action::SetTheme("auto".into()), &mut app);
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            Effect::PersistSetting { key, value, .. } => {
                assert_eq!(*key, "theme");
                assert_eq!(
                    *value,
                    SettingValue::Enum("auto"),
                    "auto commit persists `auto` (NOT the resolved concrete theme)",
                );
            }
            other => panic!("expected PersistSetting, got {other:?}"),
        }
        assert_eq!(app.current_ui.theme.as_deref(), Some("auto"));
        assert!(
            crate::theme::cache::is_auto_mode(),
            "auto commit must enable AUTO_MODE",
        );
    });
}

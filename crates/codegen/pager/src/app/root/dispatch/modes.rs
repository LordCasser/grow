//! Behavior, permission, and plan-view transitions.

use super::ctx::with_active_agent;
use super::queue::{maybe_drain_queue, note_peek_page_flip};
use super::session::lifecycle::skip_picker_and_create_session;
use super::settings::ui::{refresh_open_settings_modals, save_success_toast};
use crate::app::actions::Effect;
use crate::app::root::{ActiveView, AppView};
use agent_client_protocol as acp;

/// Show the current plan: if a plan file exists, open it in the preview
/// overlay popover. If no plan has been written yet, show a toast.
///
/// Delegates to `AgentView::show_plan_preview()` which reads the session artifact
/// from `~/.grow/sessions/<urlencoded_cwd>/<session_id>/plan.md`.
pub(super) fn dispatch_show_plan(app: &mut AppView) -> Vec<Effect> {
    with_active_agent(app, |agent| {
        if agent.plan_approval_view.is_some() {
            agent.reopen_plan_approval();
        } else {
            agent.show_plan_preview();
        }
    });
    vec![]
}

/// Select a Behavior and optionally send its first prompt after the shell
/// confirms the transition. `SetModeThenPrompt` prevents prompts from being
/// delivered to the old Behavior while confirmation or rejection is pending.
pub(super) fn dispatch_set_behavior_then_prompt(
    app: &mut AppView,
    mode: tools::types::BehaviorId,
    prompt: Option<String>,
) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };

    if let Some(reason) = agent.behavior_unavailable_reason(mode) {
        agent.show_toast(&reason);
        return vec![];
    }
    let Some(session_id) = agent.session.session_id.clone() else {
        agent.show_toast("No active session");
        return vec![];
    };
    agent.session.behavior_mode_pending = Some(mode);
    agent.session.plan_mode_pending = Some(mode.is_plan());
    let mode_id = acp::SessionModeId::new(mode.as_id());
    if let Some(prompt) = prompt {
        let skill_token_ranges = agent
            .prompt
            .slash_controller
            .recognized_token_ranges(&prompt, &agent.session.models);
        agent
            .session
            .enqueue_prompt_with_skill_tokens(prompt, skill_token_ranges);
        let drain = maybe_drain_queue(agent);
        note_peek_page_flip(app, id, drain.page_flip_entry);
        let mut effects = Vec::with_capacity(1);
        for eff in drain.effects {
            match eff {
                Effect::SendPrompt {
                    agent_id,
                    text,
                    prompt_id,
                    skill_token_ranges,
                    ..
                } => {
                    effects.push(Effect::SetModeThenPrompt {
                        session_id: session_id.clone(),
                        mode_id: mode_id.clone(),
                        agent_id,
                        text,
                        prompt_id,
                        skill_token_ranges,
                    });
                }
                other => effects.push(other),
            }
        }
        if effects.is_empty() {
            effects.push(Effect::SetSessionMode {
                session_id,
                mode_id,
            });
        }
        effects
    } else {
        vec![Effect::SetSessionMode {
            session_id,
            mode_id,
        }]
    }
}

/// Select one mutually exclusive primary Agent Behavior. Permission policy is
/// deliberately untouched; the shell remains authoritative for transition
/// guards such as an active Workflow run blocking entry into Plan.
pub(super) fn dispatch_set_behavior_mode(
    app: &mut AppView,
    mode: tools::types::BehaviorId,
) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    if agent.session.deferred_session_mode.is_some() && mode == agent.session.behavior_mode {
        // A failed deferred admission left the first prompt parked. Picking
        // the already-current Behavior is an explicit fallback decision,
        // not an idempotent no-op: consume the admission token and send the
        // queue under the identity the Shell already owns. Compare against
        // the authoritative mode, never the optimistic pending target.
        agent.session.deferred_session_mode = None;
        agent.show_toast(&format!(
            "Queued prompt will use {} Behavior",
            mode.display_label()
        ));
        let drain = maybe_drain_queue(agent);
        note_peek_page_flip(app, id, drain.page_flip_entry);
        return drain.effects;
    }
    if let Some(reason) = agent.behavior_unavailable_reason(mode) {
        agent.show_toast(&reason);
        return vec![];
    }
    let effective = agent
        .session
        .behavior_mode_pending
        .unwrap_or(agent.session.behavior_mode);
    if effective == mode {
        agent.show_toast("Behavior is already selected");
        return vec![];
    }
    if mode == tools::types::BehaviorId::Plan
        && agent.ephemeral_tip.current_key() == Some(crate::tips::plan_nudge::PLAN_NUDGE_KEY)
    {
        diagnostics::session_ctx::log_event(diagnostics::events::ContextualTip {
            tip: diagnostics::events::ContextualTipKind::PlanMode,
            action: diagnostics::events::ContextualTipAction::Accepted,
        });
        agent
            .ephemeral_tip
            .clear(crate::tips::plan_nudge::PLAN_NUDGE_KEY);
    }
    agent.session.behavior_mode_pending = Some(mode);
    if agent.session.deferred_session_mode.is_some() {
        // Retrying or replacing a failed first-prompt admission keeps one
        // authoritative target instead of creating a second queue mechanism.
        agent.session.deferred_session_mode = Some(mode);
    }
    agent.session.plan_mode_pending = Some(mode.is_plan());
    agent.show_mode_switch_banner(mode.display_label());

    let session_id = agent.session.session_id.clone();
    if session_id.is_none() {
        agent.session.deferred_session_mode =
            (mode != tools::types::BehaviorId::Normal).then_some(mode);
    }
    refresh_open_settings_modals(app);
    let Some(session_id) = session_id else {
        return skip_picker_and_create_session(app, id);
    };
    vec![Effect::SetSessionMode {
        session_id,
        mode_id: acp::SessionModeId::new(mode.as_id()),
    }]
}

/// The single gate for client paths that ENABLE always-approve: `Some(reason)`
/// iff `enabling` and the pin (`app.always_approve_policy_block`) is set. Every enabling
/// path routes through here (or [`refuse_if_always_approve_locked`]) so new paths stay
/// gated by default; callers must NOT persist on a refusal.
pub(super) fn always_approve_enable_blocked(app: &AppView, enabling: bool) -> Option<&'static str> {
    if enabling {
        app.always_approve_policy_block
    } else {
        None
    }
}

/// `Vec<Effect>` wrapper for the persisting setters: on a refusal, toast and
/// return `Some(vec![])` (no persist); `None` means proceed.
fn refuse_if_always_approve_locked(app: &mut AppView, enabling: bool) -> Option<Vec<Effect>> {
    let warning = always_approve_enable_blocked(app, enabling)?;
    app.show_toast(warning);
    Some(vec![])
}

/// When the auto gate is off, force the displayed permission mode off Auto and
/// clamp every agent's per-session mode, so the UI, selectors, settings
/// snapshot, and each tab's badge never show Auto while the feature is
/// disabled. Shared by the startup reconcile and the mid-session kill-switch.
/// Clearing every agent (not just when the global mirror still reads "auto")
/// matters because `switch_to_agent` re-anchors the mirror to the active tab.
pub(crate) fn downgrade_displayed_auto_if_gated(app: &mut AppView) {
    if app.auto_mode_gate {
        return;
    }
    for agent in app.agents.values_mut() {
        if agent.session.is_auto() {
            agent.session.permission_mode = shell::util::config::PermissionMode::Ask;
        }
    }
    if app.current_ui.permission_mode.as_deref() == Some("auto") {
        app.current_ui.permission_mode = Some("ask".into());
    }
}

/// Canonical mode inherited by a newly created session. Runtime policy gates
/// clamp stale settings projections at this boundary.
pub(crate) fn inherit_permission_mode(app: &AppView) -> shell::util::config::PermissionMode {
    match app.default_permission_mode {
        shell::util::config::PermissionMode::Auto if !app.auto_mode_gate => {
            shell::util::config::PermissionMode::Ask
        }
        shell::util::config::PermissionMode::AlwaysApprove
            if app.always_approve_policy_block.is_some() =>
        {
            shell::util::config::PermissionMode::Ask
        }
        mode => mode,
    }
}

fn set_permission_mode_inner_scoped(app: &mut AppView, mode: shell::util::config::PermissionMode) {
    if always_approve_enable_blocked(app, mode.is_always_approve()).is_some() {
        tracing::warn!("always-approve enable blocked by managed policy");
        return;
    }
    let ActiveView::Agent(id) = app.active_view else {
        return;
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return;
    };

    let previous_mode = agent.session.permission_mode();

    // Drain ordering invariant: flag flip BEFORE the drain (see fn
    // doc-comment). Do NOT reorder these without re-reading the
    // contract.
    agent.session.permission_mode = mode;

    if mode.is_always_approve() {
        // always-approve ON: auto-approve only permissions owned by this root session.
        // Child asks share the visual queue but follow an independent mode and
        // must survive a later parent mode switch. The root drain runs even on
        // idempotent re-dispatch, prefers `AllowOnce`, and never selects
        // `AllowAlways`.
        let original_front = agent.permission_queue.front().map(|permission| {
            (
                permission.request.request.session_id.clone(),
                permission.request.request.tool_call.tool_call_id.clone(),
            )
        });
        let root_session_id = agent.session.session_id.clone();
        let mut retained = std::collections::VecDeque::new();
        let mut approved_any = false;
        for perm in agent.take_permission_queue() {
            if root_session_id
                .as_ref()
                .is_some_and(|session_id| perm.request.request.session_id == *session_id)
            {
                approved_any = true;
                let response = if let Some(allow) = perm
                    .options
                    .iter()
                    .find(|o| o.kind == acp::PermissionOptionKind::AllowOnce)
                {
                    acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Selected(
                        acp::SelectedPermissionOutcome::new(allow.option_id.clone()),
                    ))
                } else {
                    acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled)
                };
                super::permissions::respond_permission(agent, perm.request, response);
            } else {
                retained.push_back(perm);
            }
        }
        agent.replace_permission_queue(retained);
        let front_removed = approved_any
            && original_front.is_some_and(|(session_id, tool_call_id)| {
                agent.permission_queue.front().is_none_or(|permission| {
                    permission.request.request.session_id != session_id
                        || permission.request.request.tool_call.tool_call_id != tool_call_id
                })
            });
        if front_removed {
            super::permissions::resolve_permission_queue_transition(agent);
        }
    }

    // Diagnostic + tracing guarded on real state change only.
    if previous_mode != mode {
        diagnostics::session_ctx::log_event(diagnostics::events::PermissionModeChanged {
            mode,
            previous_mode,
            trigger: diagnostics::events::PermissionModeTrigger::Pager,
        });
        tracing::info!(target: "settings", key = "permission_mode", value = ?mode, "setting changed");
    }
}

/// Set `permission_mode`. SHELL-owned, emits
/// `Effect::PersistPermissionMode` with rollback. The drain runs
/// unconditionally on always-approve (even duplicate dispatches) because
/// a permission could arrive between dispatches.
/// Set the active session's permission mode without changing the persisted
/// default used by future sessions.
pub(super) fn set_permission_mode(
    app: &mut AppView,
    kind: crate::app::actions::PermissionModeKind,
) -> Vec<Effect> {
    // Feature gate: a commit to Auto is inert when the auto permission-mode
    // feature is disabled. Reading `app.auto_mode_gate` here keeps the settings
    // default and session selector in lockstep — both degrade Auto → Ask when
    // the gate is off.
    let kind =
        if matches!(kind, crate::app::actions::PermissionModeKind::Auto) && !app.auto_mode_gate {
            crate::app::actions::PermissionModeKind::Ask
        } else {
            kind
        };
    // Managed policy pins always-approve off — keep the modal on live state.
    if let Some(blocked) = refuse_if_always_approve_locked(app, kind.is_always_approve()) {
        refresh_open_settings_modals(app);
        return blocked;
    }
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let (session_id, effective_plan) = app
        .agents
        .get(&id)
        .map(|a| {
            (
                a.session.session_id.clone(),
                a.session
                    .plan_mode_pending
                    .unwrap_or(a.session.plan_mode_active),
            )
        })
        .unwrap_or((None, false));
    let Some(session_id) = session_id else {
        app.show_toast("Permission can be changed after the session connects.");
        return vec![];
    };

    // State mutation via shared inner. Inner clears the soft-default latch.
    set_permission_mode_inner_scoped(app, kind.as_runtime());

    // Toast on every save (plan-aware for AlwaysApprove, mirroring
    // `set_always_approve_mode` — the plan edit gate stays binding under always-approve).
    if kind.is_always_approve() && effective_plan {
        app.show_toast(ALWAYS_APPROVE_ON_UNDER_PLAN_TOAST);
    } else {
        app.show_toast(&permission_mode_toast(kind));
    }

    vec![Effect::NotifySessionPermissionMode {
        canonical: kind.as_canonical(),
        session_id,
    }]
}

/// Persist the default permission for future sessions. This deliberately does
/// not touch the active Agent or drain its permission queue.
pub(super) fn set_default_permission_mode(
    app: &mut AppView,
    kind: crate::app::actions::PermissionModeKind,
) -> Vec<Effect> {
    if matches!(kind, crate::app::actions::PermissionModeKind::Auto) && !app.auto_mode_gate {
        app.show_toast("Auto permission mode is unavailable");
        return vec![];
    }
    if let Some(blocked) = refuse_if_always_approve_locked(app, kind.is_always_approve()) {
        return blocked;
    }
    let previous = shell::util::config::permission_mode_canonical_str(app.default_permission_mode);
    if previous == kind.as_canonical() {
        return vec![];
    }
    app.default_permission_mode = kind.as_runtime();
    app.current_ui.permission_mode = Some(kind.as_canonical().to_string());
    refresh_open_settings_modals(app);
    app.show_toast(&format!(
        "✓ Default permission mode: {}",
        kind.display_name()
    ));
    vec![Effect::PersistPermissionMode {
        canonical: kind.as_canonical(),
        session_id: None,
        persist: crate::app::actions::PermissionModePersist::WithRollback(previous),
    }]
}

/// Build the toast for a `permission_mode` commit. `AlwaysApprove`
/// reuses `always_approve_toast(true)` (destructive). `Ask` gets a dedicated
/// "Permission mode: ..." toast matching the picker brand.
pub(super) fn permission_mode_toast(kind: crate::app::actions::PermissionModeKind) -> String {
    use crate::app::actions::PermissionModeKind;
    match kind {
        PermissionModeKind::AlwaysApprove => always_approve_toast(true),
        PermissionModeKind::Auto => "\u{2713} Permission mode: Auto (classifier)".to_string(),
        PermissionModeKind::Ask => "\u{2713} Permission mode: Ask".to_string(),
    }
}

/// always-approve-ON toast when plan mode is active: always-approve arms the permission
/// fast path, but it does not bypass Plan phases or the approved contract.
pub(super) const ALWAYS_APPROVE_ON_UNDER_PLAN_TOAST: &str =
    "\u{26A0} Always-approve ON: Plan approval phases and contract remain enforced";

/// Build the always-approve toast — ⚠ on ON (destructive), ✓ on OFF (safe default).
fn always_approve_toast(new: bool) -> String {
    if new {
        // Warning glyph + consequence — only post-commit feedback.
        "\u{26A0} Always-approve ON: all tool actions auto-run".to_string()
    } else {
        // OFF restores safe default — uniform ✓ glyph.
        save_success_toast("Always-approve", false)
    }
}

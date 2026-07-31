//! Behavior, permission, and plan-view transitions.

use super::ctx::with_active_agent;
use super::queue::{maybe_drain_queue, note_peek_page_flip};
use super::session::lifecycle::skip_picker_and_create_session;
use super::settings::ui::{refresh_open_settings_modals, save_success_toast};
use crate::app::actions::Effect;
use crate::app::app_view::{ActiveView, AppView};
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
    mode: grow_tools::types::SessionMode,
    prompt: Option<String>,
) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };

    let unavailable = match mode {
        grow_tools::types::SessionMode::Workflow => {
            !agent.prompt.slash_controller.workflows_available()
        }
        grow_tools::types::SessionMode::DeepResearch => agent
            .prompt
            .slash_controller
            .registry()
            .get("deep-research")
            .is_none(),
        grow_tools::types::SessionMode::Goal => agent
            .prompt
            .slash_controller
            .registry()
            .get("goal")
            .is_none(),
        _ => false,
    };
    if unavailable {
        agent.show_toast(&format!("{} behavior is unavailable", mode.as_id()));
        return vec![];
    }
    let Some(session_id) = agent.session.session_id.clone() else {
        agent.show_toast("No active session");
        return vec![];
    };
    agent.behavior_mode_pending = Some(mode);
    agent.plan_mode_pending = Some(mode.is_plan());
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
    mode: grow_tools::types::SessionMode,
) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    let unavailable = match mode {
        grow_tools::types::SessionMode::Workflow => {
            !agent.prompt.slash_controller.workflows_available()
        }
        grow_tools::types::SessionMode::DeepResearch => agent
            .prompt
            .slash_controller
            .registry()
            .get("deep-research")
            .is_none(),
        grow_tools::types::SessionMode::Goal => agent
            .prompt
            .slash_controller
            .registry()
            .get("goal")
            .is_none(),
        _ => false,
    };
    if unavailable {
        agent.show_toast("This behavior is unavailable in this session");
        return vec![];
    }
    let effective = agent.behavior_mode_pending.unwrap_or(agent.behavior_mode);
    if effective == mode {
        agent.show_toast("Behavior is already selected");
        return vec![];
    }
    if mode == grow_tools::types::SessionMode::Plan
        && agent.ephemeral_tip.current_key() == Some(crate::tips::plan_nudge::PLAN_NUDGE_KEY)
    {
        grow_diagnostics::session_ctx::log_event(grow_diagnostics::events::ContextualTip {
            tip: grow_diagnostics::events::ContextualTipKind::PlanMode,
            action: grow_diagnostics::events::ContextualTipAction::Accepted,
        });
        agent
            .ephemeral_tip
            .clear(crate::tips::plan_nudge::PLAN_NUDGE_KEY);
    }
    agent.behavior_mode_pending = Some(mode);
    agent.plan_mode_pending = Some(mode.is_plan());
    agent.show_mode_switch_banner(match mode {
        grow_tools::types::SessionMode::Default => "Default",
        grow_tools::types::SessionMode::Ask => "Clarify",
        grow_tools::types::SessionMode::Plan => "Plan",
        grow_tools::types::SessionMode::Workflow => "Dynamic Workflow",
        grow_tools::types::SessionMode::DeepResearch => "Deep Research",
        grow_tools::types::SessionMode::Goal => "Goal",
    });

    let session_id = agent.session.session_id.clone();
    if session_id.is_none() {
        agent.deferred_session_mode =
            (mode != grow_tools::types::SessionMode::Default).then_some(mode);
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

pub(super) fn dispatch_dismiss_behavior_switch_warning(app: &mut AppView) -> Vec<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };
    agent.behavior_switch_warning_pending = false;
    agent.mode_switch_banner = None;
    agent.behavior_mode_pending = None;
    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };
    vec![Effect::SetSessionMode {
        session_id,
        mode_id: acp::SessionModeId::new(agent.behavior_mode.as_id()),
    }]
}

/// The single gate for client paths that ENABLE always-approve: `Some(reason)`
/// iff `enabling` and the pin (`app.yolo_policy_block`) is set. Every enabling
/// path routes through here (or [`refuse_if_yolo_locked`]) so new paths stay
/// gated by default; callers must NOT persist on a refusal.
pub(super) fn yolo_enable_blocked(app: &AppView, enabling: bool) -> Option<&'static str> {
    if enabling {
        app.yolo_policy_block
    } else {
        None
    }
}

/// `Vec<Effect>` wrapper for the persisting setters: on a refusal, toast and
/// return `Some(vec![])` (no persist); `None` means proceed.
fn refuse_if_yolo_locked(app: &mut AppView, enabling: bool) -> Option<Vec<Effect>> {
    let warning = yolo_enable_blocked(app, enabling)?;
    app.show_toast(warning);
    Some(vec![])
}

/// Canonical "auto wins only when yolo is off" precedence — the single source
/// of truth for the yolo-over-auto rule applied at every reconnect / seed / meta
/// site. Callers pass the already-resolved auto signal (a per-session flag or a
/// `permission_mode == Some("auto")` test).
pub(crate) fn effective_auto(yolo: bool, auto: bool) -> bool {
    !yolo && auto
}

/// When the auto gate is off, force the displayed permission mode off Auto and
/// clear every agent's per-session auto flag, so the UI, selectors, settings
/// snapshot, and each tab's badge never show Auto while the feature is
/// disabled. Shared by the startup reconcile and the mid-session kill-switch.
/// Clearing every agent (not just when the global mirror still reads "auto")
/// matters because `switch_to_agent` re-anchors the mirror to the active tab.
pub(crate) fn downgrade_displayed_auto_if_gated(app: &mut AppView) {
    if app.auto_mode_gate {
        return;
    }
    for agent in app.agents.values_mut() {
        agent.session.auto_mode = false;
    }
    if app.current_ui.permission_mode.as_deref() == Some("auto") {
        app.current_ui.permission_mode = Some("ask".into());
    }
}

/// Whether a newly created session should start with the Auto display flag set:
/// the gate is on, the current UI mode is Auto, and yolo is not winning. Mirrors
/// the canonical `auto && !yolo` precedence used on the wire (`ClientCapabilities`
/// / `SessionFlags`). The `auto_mode_gate` check is defense-in-depth so a stale
/// `current_ui == "auto"` can never seed a new session into Auto when gated off.
pub(super) fn inherit_auto_mode(app: &AppView) -> bool {
    app.auto_mode_gate
        && effective_auto(
            app.default_yolo,
            app.current_ui.permission_mode.as_deref() == Some("auto"),
        )
}

fn set_yolo_mode_inner_scoped(app: &mut AppView, new: bool, update_default: bool) {
    if yolo_enable_blocked(app, new).is_some() {
        tracing::warn!("always-approve enable blocked by managed policy");
        return;
    }
    // Global mirrors update unconditionally (even if the user navigated
    // away from the agent mid-rollback). Per-agent state is gated below.
    if update_default {
        app.default_yolo = new;
        app.permission_mode_from_soft_default = false;
        app.current_ui.permission_mode =
            Some(if new { "always-approve" } else { "ask" }.to_string());
    }

    let ActiveView::Agent(id) = app.active_view else {
        return;
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return;
    };

    let previous_state = agent.session.is_yolo();

    // Drain ordering invariant: flag flip BEFORE the drain (see fn
    // doc-comment). Do NOT reorder these without re-reading the
    // contract.
    agent.session.yolo_mode = new;

    if new {
        // YOLO ON: auto-approve all queued permissions. Drain runs
        // even on idempotent re-dispatch. Prefers `AllowOnce`; falls
        // back to `Cancelled` (never `AllowAlways`).
        agent.last_permission_click = None;
        for perm in agent.permission_queue.drain(..) {
            if let Some(allow) = perm
                .options
                .iter()
                .find(|o| o.kind == acp::PermissionOptionKind::AllowOnce)
            {
                perm.request
                    .response_tx
                    .send(Ok(acp::RequestPermissionResponse::new(
                        acp::RequestPermissionOutcome::Selected(
                            acp::SelectedPermissionOutcome::new(allow.option_id.clone()),
                        ),
                    )))
                    .ok();
            } else {
                perm.request
                    .response_tx
                    .send(Ok(acp::RequestPermissionResponse::new(
                        acp::RequestPermissionOutcome::Cancelled,
                    )))
                    .ok();
            }
        }
        super::permissions::restore_permission_stashes(agent);
    }

    // Diagnostic + tracing guarded on real state change only.
    if previous_state != new {
        grow_diagnostics::session_ctx::log_event(grow_diagnostics::events::YoloToggled {
            enabled: new,
            previous_state,
            trigger: grow_diagnostics::events::YoloTrigger::Pager,
        });
        tracing::info!(target: "settings", key = "permission_mode", value = new, "setting changed");
    }
}

/// Set YOLO (`permission_mode`). SHELL-owned, emits
/// `Effect::PersistPermissionMode` with rollback. The drain runs
/// unconditionally on YOLO=ON (even duplicate dispatches) because
/// a permission could arrive between dispatches.
fn capture_prev_permission_canonical(app: &AppView, prev_yolo: bool) -> &'static str {
    if prev_yolo {
        "always-approve"
    } else {
        match app.current_ui.permission_mode.as_deref() {
            Some("default") => "default",
            Some("auto") => "auto",
            _ => "ask",
        }
    }
}

/// Set the active session's permission mode without changing the persisted
/// default used by future sessions.
pub(super) fn set_permission_mode(
    app: &mut AppView,
    kind: crate::app::actions::PermissionModeKind,
) -> Vec<Effect> {
    if matches!(kind, crate::app::actions::PermissionModeKind::Default) {
        app.show_toast("Default permission is only available in Settings.");
        return vec![];
    }
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
    if let Some(blocked) = refuse_if_yolo_locked(app, kind.is_always_approve()) {
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
                a.plan_mode_pending.unwrap_or(a.plan_mode_active),
            )
        })
        .unwrap_or((None, false));
    let Some(session_id) = session_id else {
        app.show_toast("Permission can be changed after the session connects.");
        return vec![];
    };

    // State mutation via shared inner. We overwrite the canonical
    // below for the Default case. Inner clears the soft-default latch.
    set_yolo_mode_inner_scoped(app, kind.is_always_approve(), false);
    if let Some(agent) = app.agents.get_mut(&id) {
        agent.session.auto_mode = matches!(kind, crate::app::actions::PermissionModeKind::Auto);
    }

    // Toast on every save (plan-aware for AlwaysApprove, mirroring
    // `set_yolo_mode` — the plan edit gate stays binding under yolo).
    if kind.is_always_approve() && effective_plan {
        app.show_toast(YOLO_ON_UNDER_PLAN_TOAST);
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
    if let Some(blocked) = refuse_if_yolo_locked(app, kind.is_always_approve()) {
        return blocked;
    }
    let previous = capture_prev_permission_canonical(app, app.default_yolo);
    if previous == kind.as_canonical() {
        return vec![];
    }
    app.default_yolo = kind.is_always_approve();
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
/// reuses `yolo_toast(true)` (destructive). `Ask` and `Default` get
/// dedicated "Permission mode: ..." toasts matching the picker brand.
pub(super) fn permission_mode_toast(kind: crate::app::actions::PermissionModeKind) -> String {
    use crate::app::actions::PermissionModeKind;
    match kind {
        PermissionModeKind::AlwaysApprove => yolo_toast(true),
        PermissionModeKind::Auto => "\u{2713} Permission mode: Auto (classifier)".to_string(),
        PermissionModeKind::Ask => "\u{2713} Permission mode: Ask".to_string(),
        PermissionModeKind::Default => "\u{2713} Permission mode: Default".to_string(),
    }
}

/// YOLO-ON toast when plan mode is active: always-approve arms the permission
/// fast path, but it does not bypass Plan phases or the approved contract.
pub(super) const YOLO_ON_UNDER_PLAN_TOAST: &str =
    "\u{26A0} Always-approve ON: Plan approval phases and contract remain enforced";

/// Build the YOLO toast — ⚠ on ON (destructive), ✓ on OFF (safe default).
fn yolo_toast(new: bool) -> String {
    if new {
        // Warning glyph + consequence — only post-commit feedback.
        "\u{26A0} Always-approve ON: all tool actions auto-run".to_string()
    } else {
        // OFF restores safe default — uniform ✓ glyph.
        save_success_toast("Always-approve", false)
    }
}

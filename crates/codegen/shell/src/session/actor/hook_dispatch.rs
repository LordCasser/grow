//! Hook dispatch concern for `SessionActor`: run contexts, hook execution
//! notifications and diagnostics, and turn/tool outcome mapping.

use super::*;

use ::hooks::config::{HandlerType, HookOrigin, OnFailure};
use ::hooks::dispatcher::{HookGateEffect, HookPlanAction};
use ::hooks::event::{GateKind, HookEventEnvelope, HookEventName};
use ::hooks::result::{HookDecision, HookRunResult, HookSkipReason};

/// The one authoritative result of a hook occurrence. Callers consume this
/// only after [`SessionActor::dispatch_hook_occurrence`] has durably closed the
/// complete occurrence in the Timeline.
pub(super) enum HookAggregate {
    Observe {
        results: Vec<HookRunResult>,
    },
    Prompt {
        decision: HookDecision,
        results: Vec<HookRunResult>,
    },
    Tool {
        decision: HookDecision,
        results: Vec<HookRunResult>,
    },
    Stop {
        result: ::hooks::dispatcher::StopDispatchResult,
    },
}

#[derive(Clone, Copy)]
pub(super) enum HookDispatchPolicy {
    Execute,
    SkipAllPolicyDisabled,
}

impl HookAggregate {
    pub(super) fn into_tool_decision(self) -> HookDecision {
        let Self::Tool { decision, .. } = self else {
            unreachable!("non-tool hook aggregate used as a tool gate")
        };
        decision
    }

    pub(super) fn into_stop_result(self) -> ::hooks::dispatcher::StopDispatchResult {
        let Self::Stop { result } = self else {
            unreachable!("non-stop hook aggregate used as a stop gate")
        };
        result
    }
}

enum PlannedHandler {
    File(::hooks::dispatcher::HookPlanEntry),
    Client(super::hooks::PlannedClientHook),
}

impl PlannedHandler {
    fn runtime_name(&self) -> String {
        match self {
            Self::File(entry) => entry.identity.name.clone(),
            Self::Client(client) => format!("client:{}", client.callback_id),
        }
    }
}

struct OccurrencePlanEntry {
    timeline: chat_state::HookHandlerPlan,
    handler: PlannedHandler,
}

fn timeline_event(event: HookEventName) -> chat_state::HookEventType {
    match event {
        HookEventName::SessionStart => chat_state::HookEventType::SessionStart,
        HookEventName::UserPromptSubmit => chat_state::HookEventType::UserPromptSubmit,
        HookEventName::PreToolUse => chat_state::HookEventType::PreToolUse,
        HookEventName::PostToolUse => chat_state::HookEventType::PostToolUse,
        HookEventName::PostToolUseFailure => chat_state::HookEventType::PostToolUseFailure,
        HookEventName::PermissionDenied => chat_state::HookEventType::PermissionDenied,
        HookEventName::Stop => chat_state::HookEventType::Stop,
        HookEventName::StopFailure => chat_state::HookEventType::StopFailure,
        HookEventName::StopCancelled => chat_state::HookEventType::StopCancelled,
        HookEventName::Notification => chat_state::HookEventType::Notification,
        HookEventName::SubagentStart => chat_state::HookEventType::SubagentStart,
        HookEventName::SubagentStop => chat_state::HookEventType::SubagentStop,
        HookEventName::PreCompact => chat_state::HookEventType::PreCompact,
        HookEventName::PostCompact => chat_state::HookEventType::PostCompact,
        HookEventName::SessionEnd => chat_state::HookEventType::SessionEnd,
    }
}

fn timeline_gate(gate: GateKind) -> chat_state::HookGateKind {
    match gate {
        GateKind::Observe => chat_state::HookGateKind::Observe,
        GateKind::Prompt => chat_state::HookGateKind::Prompt,
        GateKind::Tool => chat_state::HookGateKind::Tool,
        GateKind::Stop => chat_state::HookGateKind::Stop,
    }
}

fn timeline_skip(reason: HookSkipReason) -> chat_state::HookRunSkipReason {
    match reason {
        HookSkipReason::MatcherMiss => chat_state::HookRunSkipReason::MatcherMiss,
        HookSkipReason::Disabled => chat_state::HookRunSkipReason::Disabled,
        HookSkipReason::PolicyDisabled => chat_state::HookRunSkipReason::PolicyDisabled,
        HookSkipReason::PriorBlock => chat_state::HookRunSkipReason::PriorBlock,
        HookSkipReason::ProcessInterrupted => chat_state::HookRunSkipReason::ProcessInterrupted,
    }
}

fn timeline_kind(kind: HandlerType) -> chat_state::HookHandlerKind {
    match kind {
        HandlerType::Command => chat_state::HookHandlerKind::Command,
        HandlerType::Http => chat_state::HookHandlerKind::Http,
    }
}

fn timeline_provenance(
    entry: &::hooks::dispatcher::HookPlanEntry,
) -> Option<chat_state::HookHandlerProvenance> {
    let origin = match &entry.action {
        HookPlanAction::Execute(spec) => ::hooks::config::hook_origin(spec),
        HookPlanAction::Skip(_) => match entry.identity.provenance {
            ::hooks::config::HookProvenance::User => HookOrigin::UserConfig,
            ::hooks::config::HookProvenance::Plugin => HookOrigin::Plugin,
            ::hooks::config::HookProvenance::File => {
                let name = entry.identity.name.as_str();
                if name.starts_with(::hooks::config::GLOBAL_HOOK_PREFIX) {
                    HookOrigin::UserFile
                } else if name.starts_with(::hooks::config::PROJECT_HOOK_PREFIX) {
                    HookOrigin::ProjectFile
                } else if name.starts_with(::hooks::config::AGENT_HOOK_PREFIX) {
                    HookOrigin::Agent
                } else if name.starts_with(::hooks::config::PLUGIN_HOOK_PREFIX) {
                    HookOrigin::Plugin
                } else {
                    HookOrigin::Unknown
                }
            }
        },
    };
    match origin {
        HookOrigin::UserConfig => Some(chat_state::HookHandlerProvenance::UserConfig),
        HookOrigin::UserFile => Some(chat_state::HookHandlerProvenance::UserFile),
        HookOrigin::ProjectFile => Some(chat_state::HookHandlerProvenance::ProjectFile),
        HookOrigin::Plugin => Some(chat_state::HookHandlerProvenance::Plugin),
        HookOrigin::Agent => Some(chat_state::HookHandlerProvenance::Agent),
        HookOrigin::Unknown => None,
    }
}

fn frozen_handler_source_allows_execution(
    handler: &PlannedHandler,
    frozen: chat_state::HookHandlerProvenance,
    project_scope_allowed: bool,
) -> bool {
    match handler {
        PlannedHandler::Client(_) => frozen == chat_state::HookHandlerProvenance::Client,
        PlannedHandler::File(entry) => {
            let Some(actual) = timeline_provenance(entry) else {
                return false;
            };
            actual == frozen
                && match frozen {
                    chat_state::HookHandlerProvenance::ProjectFile
                    | chat_state::HookHandlerProvenance::Agent => project_scope_allowed,
                    chat_state::HookHandlerProvenance::UserConfig
                    | chat_state::HookHandlerProvenance::UserFile
                    | chat_state::HookHandlerProvenance::Plugin => true,
                    chat_state::HookHandlerProvenance::Client => false,
                }
        }
    }
}

fn result_outcome(result: &HookRunResult) -> chat_state::HookRunOutcome {
    match result {
        HookRunResult::Success { .. } => chat_state::HookRunOutcome::Success,
        HookRunResult::Blocked { .. } => chat_state::HookRunOutcome::Blocked,
        // Do not persist runner error strings: they may contain expanded command,
        // URL or environment data. The in-memory result remains available for UI.
        HookRunResult::Failed { .. } => chat_state::HookRunOutcome::Failed {
            message: "hook execution failed".into(),
        },
        HookRunResult::TimedOut { .. } => chat_state::HookRunOutcome::TimedOut,
        HookRunResult::Cancelled { .. } => chat_state::HookRunOutcome::Cancelled,
        HookRunResult::Skipped { .. } => unreachable!("skipped handlers do not finish"),
    }
}

fn result_elapsed_ms(result: &HookRunResult) -> u64 {
    match result {
        HookRunResult::Success { elapsed, .. }
        | HookRunResult::Blocked { elapsed, .. }
        | HookRunResult::Failed { elapsed, .. }
        | HookRunResult::TimedOut { elapsed, .. }
        | HookRunResult::Cancelled { elapsed, .. } => elapsed.as_millis() as u64,
        HookRunResult::Skipped { .. } => 0,
    }
}

fn typed_failure_reason(outcome: &chat_state::HookRunOutcome) -> Option<&'static str> {
    match outcome {
        chat_state::HookRunOutcome::Failed { .. } => Some("hook execution failed"),
        chat_state::HookRunOutcome::TimedOut => Some("hook timed out"),
        chat_state::HookRunOutcome::Cancelled => Some("hook was cancelled"),
        chat_state::HookRunOutcome::Success
        | chat_state::HookRunOutcome::Blocked
        | chat_state::HookRunOutcome::InterruptedOutcomeUnknown => None,
    }
}

fn admission_failure_block_reason(
    gate: GateKind,
    failure_policy: chat_state::HookFailurePolicy,
    outcome: &chat_state::HookRunOutcome,
) -> Option<&'static str> {
    if !matches!(gate, GateKind::Prompt | GateKind::Tool)
        || failure_policy != chat_state::HookFailurePolicy::Block
    {
        return None;
    }
    typed_failure_reason(outcome)
}

fn failure_gate(gate: GateKind, hook_name: &str, reason: Option<&str>) -> HookGateEffect {
    match (gate, reason) {
        (GateKind::Prompt, Some(reason)) => HookGateEffect::Prompt(HookDecision::Deny {
            reason: reason.into(),
            hook_name: hook_name.into(),
        }),
        (GateKind::Tool, Some(reason)) => HookGateEffect::Tool(HookDecision::Deny {
            reason: reason.into(),
            hook_name: hook_name.into(),
        }),
        (GateKind::Prompt, None) => HookGateEffect::Prompt(HookDecision::Allow),
        (GateKind::Tool, None) => HookGateEffect::Tool(HookDecision::Allow),
        (GateKind::Stop, _) => HookGateEffect::Stop(Default::default()),
        (GateKind::Observe, _) => HookGateEffect::Observe,
    }
}

fn durable_handler_identity(namespace: &str, identity: &str) -> String {
    format!(
        "{namespace}:blake3:{}",
        blake3::hash(identity.as_bytes()).to_hex()
    )
}

fn truncate_durable_hook_text(mut value: String) -> String {
    value = value.replace('\0', "�");
    if value.len() <= chat_state::MAX_HOOK_CONTROL_TEXT_BYTES {
        return value;
    }
    let mut end = chat_state::MAX_HOOK_CONTROL_TEXT_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

fn durable_hook_reason(value: &str, fallback: &'static str) -> String {
    let normalized = truncate_durable_hook_text(value.to_owned());
    if normalized.trim().is_empty() {
        fallback.into()
    } else {
        normalized
    }
}

fn durable_optional_hook_text(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let normalized = truncate_durable_hook_text(value.to_owned());
        (!normalized.trim().is_empty()).then_some(normalized)
    })
}

fn gate_control(effect: &HookGateEffect) -> chat_state::HookRunControl {
    match effect {
        HookGateEffect::Observe
        | HookGateEffect::Prompt(HookDecision::Allow)
        | HookGateEffect::Tool(HookDecision::Allow) => chat_state::HookRunControl::None,
        HookGateEffect::Prompt(HookDecision::Deny { reason, .. })
        | HookGateEffect::Tool(HookDecision::Deny { reason, .. }) => {
            chat_state::HookRunControl::Block {
                reason: durable_hook_reason(reason, "blocked by hook"),
            }
        }
        HookGateEffect::Stop(signals) => {
            if signals.stop_reason.is_some() {
                chat_state::HookRunControl::StopForce {
                    reason: durable_optional_hook_text(signals.stop_reason.as_deref()),
                }
            } else if signals.block_reason.is_some() || signals.additional_context.is_some() {
                let reason = durable_optional_hook_text(signals.block_reason.as_deref());
                let additional_context =
                    durable_optional_hook_text(signals.additional_context.as_deref());
                if reason.is_none() && additional_context.is_none() {
                    chat_state::HookRunControl::None
                } else {
                    chat_state::HookRunControl::StopKeepWorking {
                        reason,
                        additional_context,
                    }
                }
            } else {
                chat_state::HookRunControl::None
            }
        }
    }
}

fn timeline_decision(aggregate: &HookAggregate) -> chat_state::HookAggregateDecision {
    match aggregate {
        HookAggregate::Observe { .. } => chat_state::HookAggregateDecision::Observe,
        HookAggregate::Prompt { decision, .. } => chat_state::HookAggregateDecision::Prompt {
            decision: timeline_gate_decision(decision),
        },
        HookAggregate::Tool { decision, .. } => chat_state::HookAggregateDecision::Tool {
            decision: timeline_gate_decision(decision),
        },
        HookAggregate::Stop { result } => {
            let decision = if let Some(force) = &result.prevent_continuation {
                chat_state::HookStopDecision::ForceStop {
                    reason: durable_optional_hook_text(Some(&force.reason)),
                }
            } else if result.blocks.is_empty() && result.additional_context.is_empty() {
                chat_state::HookStopDecision::AllowStop
            } else {
                chat_state::HookStopDecision::KeepWorking {
                    reasons: result
                        .blocks
                        .iter()
                        .map(|block| durable_hook_reason(&block.reason, "blocked by hook"))
                        .collect(),
                    additional_context: result
                        .additional_context
                        .iter()
                        .filter_map(|context| durable_optional_hook_text(Some(context)))
                        .collect(),
                }
            };
            chat_state::HookAggregateDecision::Stop { decision }
        }
    }
}

fn timeline_gate_decision(decision: &HookDecision) -> chat_state::HookGateDecision {
    match decision {
        HookDecision::Allow => chat_state::HookGateDecision::Allow,
        HookDecision::Deny { reason, .. } => chat_state::HookGateDecision::Block {
            reason: durable_hook_reason(reason, "blocked by hook"),
        },
    }
}

struct ExecutedHandler {
    outcome: chat_state::HookRunOutcome,
    control: chat_state::HookRunControl,
    result: HookRunResult,
    gate: HookGateEffect,
    decisive: bool,
}

fn apply_frozen_failure_policy(
    gate_kind: GateKind,
    failure_policy: chat_state::HookFailurePolicy,
    hook_name: &str,
    mut executed: ExecutedHandler,
) -> ExecutedHandler {
    let Some(failure_reason) = typed_failure_reason(&executed.outcome) else {
        // Rebuild all non-failure control at the durability boundary as well;
        // client and file handlers may both carry untrusted response text.
        executed.control = gate_control(&executed.gate);
        return executed;
    };

    let block_reason = admission_failure_block_reason(gate_kind, failure_policy, &executed.outcome);
    executed.gate = failure_gate(gate_kind, hook_name, block_reason);
    executed.control = block_reason.map_or(chat_state::HookRunControl::None, |reason| {
        chat_state::HookRunControl::Block {
            reason: durable_hook_reason(reason, failure_reason),
        }
    });
    executed.decisive = block_reason.is_some();
    executed
}

fn aggregate_results_mut(aggregate: &mut HookAggregate) -> &mut Vec<HookRunResult> {
    match aggregate {
        HookAggregate::Observe { results }
        | HookAggregate::Prompt { results, .. }
        | HookAggregate::Tool { results, .. } => results,
        HookAggregate::Stop { result } => &mut result.results,
    }
}

fn absorb_execution(aggregate: &mut HookAggregate, hook_name: &str, effect: &HookGateEffect) {
    match (aggregate, effect) {
        (HookAggregate::Prompt { decision, .. }, HookGateEffect::Prompt(handler_decision))
        | (HookAggregate::Tool { decision, .. }, HookGateEffect::Tool(handler_decision)) => {
            if matches!(handler_decision, HookDecision::Deny { .. }) {
                *decision = handler_decision.clone();
            }
        }
        (HookAggregate::Stop { result }, HookGateEffect::Stop(signals)) => {
            result.absorb(hook_name, signals.clone());
        }
        (HookAggregate::Observe { .. }, HookGateEffect::Observe) => {}
        _ => debug_assert!(false, "hook aggregate and handler gate differ"),
    }
}

/// Encode a [`CancellationCategory`](crate::session::events::CancellationCategory)
/// as its bare snake_case wire string for the `after_turn` hook payload.
/// Deliberately `serde_json::to_value` + `as_str`, NOT `to_string` — the
/// latter yields the quoted form and fails the workspace decode.
pub(super) fn cancellation_category_to_wire_string(
    category: Option<crate::session::events::CancellationCategory>,
) -> Option<String> {
    let category = category?;
    serde_json::to_value(category)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
}

/// Returns `(notification_type, message, title, level)` when this update should
/// trigger a vendor-compatible `Notification` hook.
///
/// Internal/high-frequency updates (hook scrollback, retry progress, config
/// changes) are excluded so migrated hooks only fire on user-attention
/// events — not on every tool call or session tick.
#[allow(clippy::type_complexity)]
pub(super) fn notification_hook_for_update(
    update: &GrowSessionUpdate,
) -> Option<(String, Option<String>, Option<String>, Option<String>)> {
    match update {
        GrowSessionUpdate::DiffReview { .. } => Some((
            "permission_prompt".into(),
            Some("Diff review requested".into()),
            None,
            Some("info".into()),
        )),
        GrowSessionUpdate::RetryState(RetryState::Exhausted { reason, .. }) => Some((
            "agent_error".into(),
            Some(reason.clone()),
            None,
            Some("error".into()),
        )),
        GrowSessionUpdate::RetryState(RetryState::Failed { message, .. }) => Some((
            "agent_error".into(),
            Some(message.clone()),
            None,
            Some("error".into()),
        )),
        _ => None,
    }
}

impl SessionActor {
    pub(super) fn hook_run_ctx(&self) -> ::hooks::runner::RunContext<'_> {
        ::hooks::runner::RunContext {
            session_id: &self.session_info.id.0,
            workspace_root: &self.hooks.resolved_workspace_root,
            process_scope: self.tool_context.process_scope.clone(),
        }
    }

    /// Send structured hook execution data for rich scrollback rendering.
    ///
    /// `prompt_id` is `None` for session-level dispatches (session_start /
    /// session-end stop).
    pub(super) async fn send_hook_execution(
        &self,
        event_name: &str,
        tool_name: Option<&str>,
        prompt_id: Option<&str>,
        projection: &chat_state::HookLifecycleProjection,
    ) {
        use crate::extensions::notification::{HookRunEntryDto, HookRunStatusDto};
        let runs = projection
            .handlers
            .iter()
            .zip(&projection.runs)
            .filter_map(|(handler, run)| {
                let status = match run {
                    chat_state::HookHandlerLifecycle::Pending
                    | chat_state::HookHandlerLifecycle::Started => return None,
                    chat_state::HookHandlerLifecycle::Skipped { .. } => HookRunStatusDto::Skipped,
                    chat_state::HookHandlerLifecycle::Finished {
                        elapsed_ms,
                        outcome,
                        control,
                    } => match outcome {
                        chat_state::HookRunOutcome::Success => HookRunStatusDto::Success {
                            elapsed_ms: *elapsed_ms,
                        },
                        chat_state::HookRunOutcome::Blocked => {
                            let detail = match control {
                                chat_state::HookRunControl::Block { reason } => reason.clone(),
                                _ => "blocked by hook".into(),
                            };
                            HookRunStatusDto::Blocked {
                                detail,
                                elapsed_ms: *elapsed_ms,
                            }
                        }
                        chat_state::HookRunOutcome::Failed { message } => {
                            HookRunStatusDto::Failed {
                                error: message.clone(),
                                elapsed_ms: *elapsed_ms,
                            }
                        }
                        chat_state::HookRunOutcome::TimedOut => HookRunStatusDto::Failed {
                            error: "hook timed out".into(),
                            elapsed_ms: *elapsed_ms,
                        },
                        chat_state::HookRunOutcome::Cancelled => HookRunStatusDto::Failed {
                            error: "hook cancelled".into(),
                            elapsed_ms: *elapsed_ms,
                        },
                        chat_state::HookRunOutcome::InterruptedOutcomeUnknown => {
                            HookRunStatusDto::Failed {
                                error: "hook outcome unknown after interruption".into(),
                                elapsed_ms: *elapsed_ms,
                            }
                        }
                    },
                };
                Some(HookRunEntryDto {
                    name: handler.name.clone(),
                    status,
                    output: None,
                })
            })
            .collect::<Vec<_>>();
        let annotations = projection
            .handlers
            .iter()
            .zip(&projection.runs)
            .flat_map(|(handler, run)| {
                let chat_state::HookHandlerLifecycle::Finished { control, .. } = run else {
                    return Vec::new();
                };
                match control {
                    chat_state::HookRunControl::Block { reason } => vec![format!(
                        "\u{26a0} `{}` blocked by hook `{}`: {reason}",
                        tool_name.unwrap_or("Input"),
                        handler.name
                    )],
                    chat_state::HookRunControl::StopKeepWorking {
                        reason: Some(reason),
                        ..
                    } => vec![format!(
                        "\u{21a9} Stop blocked by hook `{}`, continuing: {reason}",
                        handler.name
                    )],
                    chat_state::HookRunControl::StopKeepWorking {
                        additional_context: Some(context),
                        ..
                    } => vec![format!(
                        "\u{21a9} Stop hook `{}` feedback, continuing: {context}",
                        handler.name
                    )],
                    chat_state::HookRunControl::StopKeepWorking {
                        reason: None,
                        additional_context: None,
                    } => Vec::new(),
                    chat_state::HookRunControl::StopForce { reason } => vec![match reason {
                        Some(reason) => {
                            format!(
                                "\u{26a0} Hook `{}` stopped the agent: {reason}",
                                handler.name
                            )
                        }
                        None => format!("\u{26a0} Hook `{}` stopped the agent", handler.name),
                    }],
                    chat_state::HookRunControl::None => Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        if runs.is_empty() && annotations.is_empty() {
            return;
        }

        // HookExecution is a live transport projection. The Timeline Hook
        // occurrence is the only durable fact source and is queried again on
        // reconnect/inspection instead of copying this projection to updates.jsonl.
        self.send_transient_hook_notification(GrowSessionUpdate::HookExecution {
            occurrence_id: projection.occurrence_id.clone(),
            event_name: event_name.to_string(),
            tool_name: tool_name.map(|s| s.to_string()),
            prompt_id: prompt_id.map(|s| s.to_string()),
            runs,
            annotations,
        })
        .await;
    }

    /// Returns the resolved workspace root for hook envelopes.
    pub(super) fn hook_workspace_root(&self) -> String {
        self.hooks.resolved_workspace_root.clone()
    }

    /// Subagent type for tool-hook attribution, or `None` for the top-level session. Prefers
    /// the task `subagent_type`, falling back to the agent definition name for older spawns.
    pub(super) fn subagent_type_label(&self) -> Option<String> {
        if !self.startup_hints.is_subagent {
            return None;
        }
        Some(
            self.startup_hints
                .subagent_type
                .clone()
                .unwrap_or_else(|| self.agent.borrow().definition().selector_identity()),
        )
    }

    /// The session's current permission mode. Behavior is intentionally
    /// independent and never masquerades as an approval policy.
    pub(super) fn permission_mode_label(&self) -> &'static str {
        let request_mode = self.startup_hints.permission_request_mode();
        match self.permissions.effective_request_mode(request_mode) {
            workspace::permission::types::EffectivePermissionMode::AlwaysApprove => {
                "always-approve"
            }
            workspace::permission::types::EffectivePermissionMode::Auto => "auto",
            workspace::permission::types::EffectivePermissionMode::Ask => "ask",
        }
    }

    /// Dispatch one occurrence from a frozen file+client plan. The Timeline is
    /// authoritative: no handler runs before Triggered, and no caller-visible
    /// result is returned before Completed is durable.
    pub(super) async fn dispatch_hook_occurrence(
        &self,
        event: HookEventName,
        cause: chat_state::HookCause,
        envelope: HookEventEnvelope,
        gate: GateKind,
        policy: HookDispatchPolicy,
    ) -> Result<HookAggregate, chat_state::TimelineWriteError> {
        debug_assert_eq!(event, envelope.hook_event_name);
        debug_assert!(
            event == HookEventName::UserPromptSubmit || gate == event.traits().gate,
            "only synthetic user_prompt_submit may override its configured gate"
        );
        let generation = self.hook_config_generation();
        let registry = self.hooks.registry.borrow().clone();
        let mut plan = Vec::new();
        if let Some(registry) = registry.as_ref() {
            for mut entry in ::hooks::dispatcher::plan_dispatch(registry, event, &envelope) {
                let index = u32::try_from(plan.len()).map_err(|_| {
                    chat_state::TimelineWriteError::Invalid(chat_state::TimelineError::InvalidHook)
                })?;
                let provenance = timeline_provenance(&entry);
                if provenance.is_none() {
                    // An unclassifiable file source has no authority to execute.
                    // Keep it visible as a policy skip under the conservative
                    // project tier instead of inventing a privileged origin.
                    entry.action = HookPlanAction::Skip(HookSkipReason::PolicyDisabled);
                }
                if matches!(policy, HookDispatchPolicy::SkipAllPolicyDisabled) {
                    entry.action = HookPlanAction::Skip(HookSkipReason::PolicyDisabled);
                }
                let failure_policy = match &entry.action {
                    HookPlanAction::Execute(spec)
                        if spec.on_failure == OnFailure::Block
                            && matches!(
                                event,
                                HookEventName::UserPromptSubmit | HookEventName::PreToolUse
                            ) =>
                    {
                        chat_state::HookFailurePolicy::Block
                    }
                    HookPlanAction::Execute(_) | HookPlanAction::Skip(_) => {
                        chat_state::HookFailurePolicy::Allow
                    }
                };
                let action = match &entry.action {
                    HookPlanAction::Execute(_) => chat_state::HookHandlerPlanAction::Execute,
                    HookPlanAction::Skip(reason) => chat_state::HookHandlerPlanAction::Skip {
                        reason: timeline_skip(*reason),
                    },
                };
                plan.push(OccurrencePlanEntry {
                    timeline: chat_state::HookHandlerPlan {
                        index,
                        run_id: uuid::Uuid::now_v7().to_string(),
                        name: durable_handler_identity("handler", &entry.identity.name),
                        provenance: provenance
                            .unwrap_or(chat_state::HookHandlerProvenance::ProjectFile),
                        kind: timeline_kind(entry.identity.handler_type),
                        failure_policy,
                        action,
                    },
                    handler: PlannedHandler::File(entry),
                });
            }
        }
        for client in self.plan_client_hooks(&envelope) {
            let index = u32::try_from(plan.len()).map_err(|_| {
                chat_state::TimelineWriteError::Invalid(chat_state::TimelineError::InvalidHook)
            })?;
            plan.push(OccurrencePlanEntry {
                timeline: chat_state::HookHandlerPlan {
                    index,
                    run_id: uuid::Uuid::now_v7().to_string(),
                    name: durable_handler_identity("client", &client.callback_id),
                    provenance: chat_state::HookHandlerProvenance::Client,
                    kind: chat_state::HookHandlerKind::Client,
                    failure_policy: chat_state::HookFailurePolicy::Allow,
                    action: match policy {
                        HookDispatchPolicy::Execute => chat_state::HookHandlerPlanAction::Execute,
                        HookDispatchPolicy::SkipAllPolicyDisabled => {
                            chat_state::HookHandlerPlanAction::Skip {
                                reason: chat_state::HookRunSkipReason::PolicyDisabled,
                            }
                        }
                    },
                },
                handler: PlannedHandler::Client(client),
            });
        }

        // A standalone Notification hook is the lifecycle root for that
        // user-attention signal, so its cause is its own occurrence identity.
        // Every other cause must reference an independently durable lifecycle.
        let occurrence_id = match (&event, &cause) {
            (
                HookEventName::Notification,
                chat_state::HookCause::Notification { notification_id },
            ) => notification_id.clone(),
            _ => uuid::Uuid::now_v7().to_string(),
        };
        self.record_hook_event(chat_state::HookEvent::Triggered {
            occurrence_id: occurrence_id.clone(),
            event: timeline_event(event),
            gate: timeline_gate(gate),
            cause,
            config_generation: generation,
            handlers: plan.iter().map(|entry| entry.timeline.clone()).collect(),
        })
        .await?;

        let plan_len = plan.len();
        let (file_plan, client_plan): (Vec<_>, Vec<_>) = plan
            .into_iter()
            .partition(|entry| matches!(&entry.handler, PlannedHandler::File(_)));
        debug_assert!(
            file_plan
                .iter()
                .chain(&client_plan)
                .enumerate()
                .all(|(index, entry)| entry.timeline.index == index as u32),
            "file handlers must precede client handlers in the frozen plan"
        );
        let mut aggregate = match gate {
            GateKind::Observe => HookAggregate::Observe {
                results: Vec::with_capacity(plan_len),
            },
            GateKind::Prompt => HookAggregate::Prompt {
                decision: HookDecision::Allow,
                results: Vec::with_capacity(plan_len),
            },
            GateKind::Tool => HookAggregate::Tool {
                decision: HookDecision::Allow,
                results: Vec::with_capacity(plan_len),
            },
            GateKind::Stop => HookAggregate::Stop {
                result: ::hooks::dispatcher::StopDispatchResult::default(),
            },
        };
        let mut decisive = false;

        // File hooks retain their configured order: one may mutate project
        // state that a later handler observes, and a decisive result prevents
        // later external effects.
        for entry in file_plan {
            let index = entry.timeline.index;
            let run_id = entry.timeline.run_id.clone();
            let hook_name = entry.handler.runtime_name();
            let planned_skip = match &entry.handler {
                PlannedHandler::File(file) => match &file.action {
                    HookPlanAction::Skip(reason) => Some(*reason),
                    HookPlanAction::Execute(_) => None,
                },
                PlannedHandler::Client(_) => None,
            };
            // A previous handler may have run arbitrary code before this one.
            // Re-resolve the workspace entity and trust grant for every
            // project/agent handler at its own execution boundary.
            let project_scope_allowed = !matches!(
                entry.timeline.provenance,
                chat_state::HookHandlerProvenance::ProjectFile
                    | chat_state::HookHandlerProvenance::Agent
            ) || crate::agent::folder_trust::project_scope_allowed(
                self.tool_context.cwd.as_path(),
            );
            let source_revalidation_failed = planned_skip.is_none()
                && !frozen_handler_source_allows_execution(
                    &entry.handler,
                    entry.timeline.provenance,
                    project_scope_allowed,
                );
            if let Some(reason) = planned_skip
                .or_else(|| source_revalidation_failed.then_some(HookSkipReason::PolicyDisabled))
                .or_else(|| decisive.then_some(HookSkipReason::PriorBlock))
            {
                self.record_hook_event(chat_state::HookEvent::RunSkipped {
                    occurrence_id: occurrence_id.clone(),
                    run_id,
                    handler_index: index,
                    reason: timeline_skip(reason),
                })
                .await?;
                aggregate_results_mut(&mut aggregate)
                    .push(HookRunResult::Skipped { hook_name, reason });
                continue;
            }

            self.record_hook_event(chat_state::HookEvent::RunStarted {
                occurrence_id: occurrence_id.clone(),
                run_id: run_id.clone(),
                handler_index: index,
            })
            .await?;

            let executed = match entry.handler {
                PlannedHandler::File(file) => {
                    let HookPlanAction::Execute(spec) = file.action else {
                        unreachable!("planned skips were handled before RunStarted")
                    };
                    let ctx = self.hook_run_ctx();
                    let execution =
                        ::hooks::dispatcher::run_one_hook(&spec, &envelope, &ctx, gate).await;
                    let outcome = if matches!(&execution.gate, HookGateEffect::Stop(_))
                        && !matches!(
                            &execution.result,
                            HookRunResult::Failed { .. }
                                | HookRunResult::TimedOut { .. }
                                | HookRunResult::Cancelled { .. }
                        ) {
                        chat_state::HookRunOutcome::Success
                    } else {
                        result_outcome(&execution.result)
                    };
                    ExecutedHandler {
                        outcome,
                        control: chat_state::HookRunControl::None,
                        result: execution.result,
                        gate: execution.gate,
                        decisive: execution.decisive,
                    }
                }
                PlannedHandler::Client(_) => unreachable!("client handler partitioned as file"),
            };
            decisive |= self
                .finish_hook_handler(
                    &occurrence_id,
                    &entry.timeline,
                    &hook_name,
                    gate,
                    &mut aggregate,
                    executed,
                )
                .await?;
        }

        if decisive {
            // One ordered policy chain spans file and client handlers. A
            // decisive file result prevents every later client callback from
            // producing an external side effect.
            for entry in client_plan {
                let hook_name = entry.handler.runtime_name();
                self.record_hook_event(chat_state::HookEvent::RunSkipped {
                    occurrence_id: occurrence_id.clone(),
                    run_id: entry.timeline.run_id,
                    handler_index: entry.timeline.index,
                    reason: timeline_skip(HookSkipReason::PriorBlock),
                })
                .await?;
                aggregate_results_mut(&mut aggregate).push(HookRunResult::Skipped {
                    hook_name,
                    reason: HookSkipReason::PriorBlock,
                });
            }
        } else {
            // Client callbacks are an ordered policy chain, not a fan-out.
            // Only the callback whose turn has arrived may be started. Once a
            // callback returns a decisive deny/stop decision, later callbacks
            // must have neither an RPC side effect nor a Started lifecycle.
            for entry in client_plan {
                let index = entry.timeline.index;
                let run_id = entry.timeline.run_id.clone();
                let hook_name = entry.handler.runtime_name();
                if let chat_state::HookHandlerPlanAction::Skip { reason } = &entry.timeline.action {
                    self.record_hook_event(chat_state::HookEvent::RunSkipped {
                        occurrence_id: occurrence_id.clone(),
                        run_id,
                        handler_index: index,
                        reason: *reason,
                    })
                    .await?;
                    aggregate_results_mut(&mut aggregate).push(HookRunResult::Skipped {
                        hook_name,
                        reason: HookSkipReason::PolicyDisabled,
                    });
                    continue;
                }
                let PlannedHandler::Client(client) = entry.handler else {
                    unreachable!("file handler partitioned as client")
                };
                if decisive {
                    self.record_hook_event(chat_state::HookEvent::RunSkipped {
                        occurrence_id: occurrence_id.clone(),
                        run_id,
                        handler_index: index,
                        reason: timeline_skip(HookSkipReason::PriorBlock),
                    })
                    .await?;
                    aggregate_results_mut(&mut aggregate).push(HookRunResult::Skipped {
                        hook_name,
                        reason: HookSkipReason::PriorBlock,
                    });
                    continue;
                }
                self.record_hook_event(chat_state::HookEvent::RunStarted {
                    occurrence_id: occurrence_id.clone(),
                    run_id,
                    handler_index: index,
                })
                .await?;
                let executed = self.execute_client_handler(gate, &envelope, client).await;
                decisive |= self
                    .finish_hook_handler(
                        &occurrence_id,
                        &entry.timeline,
                        &hook_name,
                        gate,
                        &mut aggregate,
                        executed,
                    )
                    .await?;
            }
        }

        self.record_hook_event(chat_state::HookEvent::Completed {
            occurrence_id: occurrence_id.clone(),
            decision: timeline_decision(&aggregate),
        })
        .await?;

        let tool_name = envelope.payload.match_value();
        if let Some(projection) = self
            .chat_state_handle
            .hook_projection(occurrence_id.as_str())
            .await
        {
            self.send_hook_execution(
                &event.to_string(),
                tool_name,
                envelope.prompt_id.as_deref(),
                &projection,
            )
            .await;
            self.emit_hook_executed_diagnostics(&event.to_string(), tool_name, &projection)
                .await;
        } else {
            tracing::warn!(
                occurrence_id,
                "durable hook occurrence was unavailable for UI projection"
            );
        }
        Ok(aggregate)
    }

    async fn finish_hook_handler(
        &self,
        occurrence_id: &str,
        handler: &chat_state::HookHandlerPlan,
        hook_name: &str,
        gate: GateKind,
        aggregate: &mut HookAggregate,
        executed: ExecutedHandler,
    ) -> Result<bool, chat_state::TimelineWriteError> {
        // A runner reports only the typed outcome. For execution failures,
        // the frozen Timeline plan is the sole authority for fail-open vs
        // fail-closed behavior and for the one persisted control value.
        let executed =
            apply_frozen_failure_policy(gate, handler.failure_policy, hook_name, executed);
        let decisive = executed.decisive;
        let elapsed_ms = result_elapsed_ms(&executed.result);
        self.record_hook_event(chat_state::HookEvent::RunFinished {
            occurrence_id: occurrence_id.to_owned(),
            run_id: handler.run_id.clone(),
            handler_index: handler.index,
            elapsed_ms,
            outcome: executed.outcome,
            control: executed.control,
        })
        .await?;
        absorb_execution(aggregate, hook_name, &executed.gate);
        aggregate_results_mut(aggregate).push(executed.result);
        Ok(decisive)
    }

    async fn record_hook_event(
        &self,
        event: chat_state::HookEvent,
    ) -> Result<(), chat_state::TimelineWriteError> {
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Hook(event))
            .await
            .map(|_| ())
    }

    pub(super) async fn dispatch_observe_hook(
        &self,
        event: HookEventName,
        cause: chat_state::HookCause,
        payload: ::hooks::event::HookPayload,
        prompt_id: Option<String>,
    ) -> Result<(), chat_state::TimelineWriteError> {
        let envelope = self.make_hook_envelope(event, prompt_id, payload);
        let aggregate = self
            .dispatch_hook_occurrence(
                event,
                cause,
                envelope,
                GateKind::Observe,
                HookDispatchPolicy::Execute,
            )
            .await?;
        debug_assert!(matches!(aggregate, HookAggregate::Observe { .. }));
        Ok(())
    }

    /// Prompt-ingress adapter. Human callers pass the already-durable Input
    /// identity with `Prompt`; synthetic callers pass their durable Turn or
    /// Notification cause with `Observe`.
    pub(super) async fn dispatch_prompt_hook(
        &self,
        cause: chat_state::HookCause,
        envelope: HookEventEnvelope,
        gate: GateKind,
    ) -> Result<HookAggregate, chat_state::TimelineWriteError> {
        self.dispatch_hook_occurrence(
            HookEventName::UserPromptSubmit,
            cause,
            envelope,
            gate,
            HookDispatchPolicy::Execute,
        )
        .await
    }

    async fn execute_client_handler(
        &self,
        gate_kind: GateKind,
        envelope: &HookEventEnvelope,
        client: super::hooks::PlannedClientHook,
    ) -> ExecutedHandler {
        let hook_name = format!("client:{}", client.callback_id);
        if gate_kind == GateKind::Observe {
            let started = std::time::Instant::now();
            let delivered = self.notify_planned_client_hook(&client.callback_id, envelope);
            let elapsed = started.elapsed();
            let result = if delivered {
                HookRunResult::Success {
                    hook_name,
                    elapsed,
                    http_info: None,
                }
            } else {
                HookRunResult::Failed {
                    hook_name,
                    error: "failed to serialize client hook notification".into(),
                    elapsed,
                    http_info: None,
                }
            };
            return ExecutedHandler {
                outcome: result_outcome(&result),
                control: chat_state::HookRunControl::None,
                result,
                gate: HookGateEffect::Observe,
                decisive: false,
            };
        }

        let (response, elapsed, diagnostic) = self
            .run_client_gate_callback(
                &client.callback_id,
                client.timeout,
                envelope.payload.match_value(),
                envelope,
            )
            .await;
        let response_failure = match diagnostic {
            ::diagnostics::events::ClientHookGateOutcome::TimedOut => {
                Some(chat_state::HookRunOutcome::TimedOut)
            }
            ::diagnostics::events::ClientHookGateOutcome::TransportError => {
                Some(chat_state::HookRunOutcome::Failed {
                    message: "client hook transport failed".into(),
                })
            }
            ::diagnostics::events::ClientHookGateOutcome::Malformed => {
                Some(chat_state::HookRunOutcome::Failed {
                    message: "client hook returned an invalid response".into(),
                })
            }
            ::diagnostics::events::ClientHookGateOutcome::Denied
            | ::diagnostics::events::ClientHookGateOutcome::Proceeded => None,
        };
        let (gate, decisive) = match gate_kind {
            GateKind::Prompt | GateKind::Tool => {
                let decision =
                    if response.decision == crate::extensions::hooks::ClientHookDecision::Deny {
                        HookDecision::Deny {
                            reason: response
                                .system_message
                                .filter(|message| !message.trim().is_empty())
                                .unwrap_or_else(|| "blocked by client hook".into()),
                            hook_name: hook_name.clone(),
                        }
                    } else {
                        HookDecision::Allow
                    };
                let decisive = matches!(&decision, HookDecision::Deny { .. });
                let gate = if gate_kind == GateKind::Prompt {
                    HookGateEffect::Prompt(decision)
                } else {
                    HookGateEffect::Tool(decision)
                };
                (gate, decisive)
            }
            GateKind::Stop => {
                let block_reason = (response.decision
                    == crate::extensions::hooks::ClientHookDecision::Deny)
                    .then(|| {
                        response
                            .system_message
                            .filter(|message| !message.trim().is_empty())
                            .unwrap_or_else(|| "blocked by client hook".into())
                    });
                let stop_reason = (response.continue_ == Some(false)).then(|| {
                    response
                        .stop_reason
                        .filter(|message| !message.trim().is_empty())
                        .unwrap_or_else(|| "stopped by client hook".into())
                });
                let signals = ::hooks::dispatcher::StopSignals {
                    block_reason,
                    stop_reason,
                    additional_context: response
                        .additional_context
                        .filter(|message| !message.trim().is_empty()),
                };
                let decisive = signals.block_reason.is_some() || signals.stop_reason.is_some();
                (HookGateEffect::Stop(signals), decisive)
            }
            GateKind::Observe => unreachable!(),
        };
        let outcome = response_failure.unwrap_or_else(|| match &gate {
            HookGateEffect::Prompt(HookDecision::Deny { .. })
            | HookGateEffect::Tool(HookDecision::Deny { .. }) => {
                chat_state::HookRunOutcome::Blocked
            }
            HookGateEffect::Stop(_)
            | HookGateEffect::Prompt(HookDecision::Allow)
            | HookGateEffect::Tool(HookDecision::Allow) => chat_state::HookRunOutcome::Success,
            HookGateEffect::Observe => unreachable!(),
        });
        let result = match (&outcome, &gate) {
            (
                chat_state::HookRunOutcome::Blocked,
                HookGateEffect::Prompt(HookDecision::Deny { reason, .. })
                | HookGateEffect::Tool(HookDecision::Deny { reason, .. }),
            ) => HookRunResult::Blocked {
                hook_name,
                detail: reason.clone(),
                elapsed,
                http_info: None,
            },
            (_, HookGateEffect::Stop(signals))
                if signals.block_reason.is_some() || signals.stop_reason.is_some() =>
            {
                HookRunResult::Blocked {
                    hook_name,
                    detail: ::hooks::dispatcher::stop_detail(
                        signals.stop_reason.is_some(),
                        signals.stop_reason.as_deref(),
                        signals.block_reason.as_deref(),
                    )
                    .unwrap_or_default(),
                    elapsed,
                    http_info: None,
                }
            }
            (chat_state::HookRunOutcome::TimedOut, _) => HookRunResult::TimedOut {
                hook_name,
                timeout_ms: client.timeout.as_millis() as u64,
                elapsed,
                http_info: None,
            },
            (chat_state::HookRunOutcome::Failed { message }, _) => HookRunResult::Failed {
                hook_name,
                error: message.clone(),
                elapsed,
                http_info: None,
            },
            _ => HookRunResult::Success {
                hook_name,
                elapsed,
                http_info: None,
            },
        };
        ExecutedHandler {
            outcome,
            control: chat_state::HookRunControl::None,
            result,
            gate,
            decisive,
        }
    }

    pub(super) async fn emit_hook_executed_diagnostics(
        &self,
        event_name: &str,
        tool_name: Option<&str>,
        projection: &chat_state::HookLifecycleProjection,
    ) {
        let tool = tool_name.map(|s| s.to_string());
        for (handler, run) in projection.handlers.iter().zip(&projection.runs) {
            let chat_state::HookHandlerLifecycle::Finished {
                elapsed_ms,
                outcome,
                control,
            } = run
            else {
                continue;
            };
            let diagnostic_outcome = match outcome {
                chat_state::HookRunOutcome::Success => ::diagnostics::events::HookOutcome::Success,
                chat_state::HookRunOutcome::Blocked => ::diagnostics::events::HookOutcome::Blocked,
                chat_state::HookRunOutcome::Failed { .. }
                | chat_state::HookRunOutcome::TimedOut
                | chat_state::HookRunOutcome::Cancelled
                | chat_state::HookRunOutcome::InterruptedOutcomeUnknown => {
                    ::diagnostics::events::HookOutcome::Error
                }
            };
            ::diagnostics::session_ctx::log_event(::diagnostics::events::HookExecuted {
                hook_name: handler.name.clone(),
                event: event_name.to_string(),
                tool_name: tool.clone(),
                duration_ms: *elapsed_ms,
                outcome: diagnostic_outcome,
            });

            let blocked = matches!(control, chat_state::HookRunControl::Block { .. })
                || matches!(
                    control,
                    chat_state::HookRunControl::StopKeepWorking {
                        reason: Some(_),
                        ..
                    }
                );
            if blocked {
                ::diagnostics::session_ctx::log_event(::diagnostics::events::HookBlocked {
                    occurrence_id: projection.occurrence_id.clone(),
                    hook_name: handler.name.clone(),
                });
            }

            if handler.kind == chat_state::HookHandlerKind::Client {
                let client_outcome = match (outcome, control) {
                    (_, chat_state::HookRunControl::Block { .. })
                    | (chat_state::HookRunOutcome::Blocked, _) => {
                        Some(::diagnostics::events::ClientHookGateOutcome::Denied)
                    }
                    (chat_state::HookRunOutcome::Success, _) => {
                        Some(::diagnostics::events::ClientHookGateOutcome::Proceeded)
                    }
                    (chat_state::HookRunOutcome::TimedOut, _) => {
                        Some(::diagnostics::events::ClientHookGateOutcome::TimedOut)
                    }
                    (chat_state::HookRunOutcome::Failed { message }, _)
                        if message.contains("invalid response") =>
                    {
                        Some(::diagnostics::events::ClientHookGateOutcome::Malformed)
                    }
                    (chat_state::HookRunOutcome::Failed { .. }, _) => {
                        Some(::diagnostics::events::ClientHookGateOutcome::TransportError)
                    }
                    (chat_state::HookRunOutcome::Cancelled, _)
                    | (chat_state::HookRunOutcome::InterruptedOutcomeUnknown, _) => None,
                };
                if let Some(outcome) = client_outcome {
                    ::diagnostics::session_ctx::log_event(::diagnostics::events::ClientHookGate {
                        callback_id: handler.name.clone(),
                        tool_name: tool.clone(),
                        outcome,
                        duration_ms: *elapsed_ms,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod notification_hook_filter_tests {
    use super::*;
    use crate::extensions::notification::{HookRunEntryDto, HookRunStatusDto, RetryState};

    #[test]
    fn hook_updates_do_not_fire_notification_hook() {
        let execution = GrowSessionUpdate::HookExecution {
            occurrence_id: "occurrence-1".into(),
            event_name: "pre_tool_use".into(),
            tool_name: Some("read_file".into()),
            prompt_id: None,
            runs: vec![HookRunEntryDto {
                name: "test".into(),
                status: HookRunStatusDto::Success { elapsed_ms: 1 },
                output: None,
            }],
            annotations: Vec::new(),
        };
        assert!(notification_hook_for_update(&execution).is_none());
    }

    #[test]
    fn retry_in_progress_does_not_fire_notification_hook() {
        let update = GrowSessionUpdate::RetryState(RetryState::Retrying {
            attempt: 1,
            max_retries: 3,
            reason: "timeout".into(),
        });
        assert!(notification_hook_for_update(&update).is_none());
    }

    #[test]
    fn retry_exhaustion_fires_one_agent_error_hook() {
        let update = GrowSessionUpdate::RetryState(RetryState::Exhausted {
            attempts: 3,
            reason: "completion requirements were not met".into(),
            is_rate_limited: false,
        });

        let (ty, message, title, level) =
            notification_hook_for_update(&update).expect("terminal retry failure should fire");
        assert_eq!(ty, "agent_error");
        assert_eq!(
            message.as_deref(),
            Some("completion requirements were not met")
        );
        assert_eq!(title, None);
        assert_eq!(level.as_deref(), Some("error"));
    }

    #[test]
    fn diff_review_fires_permission_prompt() {
        let update = GrowSessionUpdate::DiffReview { content: vec![] };
        let (ty, message, _, level) = notification_hook_for_update(&update).expect("should fire");
        assert_eq!(ty, "permission_prompt");
        assert_eq!(message.as_deref(), Some("Diff review requested"));
        assert_eq!(level.as_deref(), Some("info"));
    }

    #[test]
    fn task_completed_does_not_fire_via_filter() {
        let update = GrowSessionUpdate::TaskCompleted {
            task_snapshot: tools::types::TaskSnapshot {
                goal_definition_revision: None,
                task_id: "task-1".into(),
                command: "echo hi".into(),
                display_command: None,
                cwd: "/tmp".into(),
                start_time: std::time::SystemTime::UNIX_EPOCH,
                end_time: None,
                output: String::new(),
                output_file: std::path::PathBuf::from("/tmp/out"),
                truncated: false,
                exit_code: Some(0),
                signal: None,
                completed: true,
                kind: Default::default(),
                block_waited: false,
                explicitly_killed: false,
                owner_session_id: None,
                goal_id: None,
                description: None,
                is_backgrounded: false,
            },
        };
        assert!(notification_hook_for_update(&update).is_none());
    }

    #[test]
    fn frozen_provenance_is_revalidated_and_unknown_sources_fail_closed() {
        let file = |name: &str, provenance| {
            PlannedHandler::File(::hooks::dispatcher::HookPlanEntry {
                identity: ::hooks::dispatcher::HookHandlerIdentity {
                    name: name.into(),
                    handler_type: HandlerType::Command,
                    provenance,
                },
                action: HookPlanAction::Skip(HookSkipReason::MatcherMiss),
            })
        };

        let unknown = file("unclassified", ::hooks::config::HookProvenance::File);
        let PlannedHandler::File(unknown_entry) = &unknown else {
            unreachable!()
        };
        assert_eq!(timeline_provenance(unknown_entry), None);
        assert!(!frozen_handler_source_allows_execution(
            &unknown,
            chat_state::HookHandlerProvenance::ProjectFile,
            true,
        ));

        let project = file("project/review", ::hooks::config::HookProvenance::File);
        assert!(frozen_handler_source_allows_execution(
            &project,
            chat_state::HookHandlerProvenance::ProjectFile,
            true,
        ));
        assert!(!frozen_handler_source_allows_execution(
            &project,
            chat_state::HookHandlerProvenance::ProjectFile,
            false,
        ));
        assert!(!frozen_handler_source_allows_execution(
            &project,
            chat_state::HookHandlerProvenance::UserFile,
            true,
        ));

        let user = file("global/review", ::hooks::config::HookProvenance::File);
        assert!(frozen_handler_source_allows_execution(
            &user,
            chat_state::HookHandlerProvenance::UserFile,
            false,
        ));
        let client = PlannedHandler::Client(super::super::hooks::PlannedClientHook {
            callback_id: "callback".into(),
            timeout: std::time::Duration::from_secs(1),
        });
        assert!(frozen_handler_source_allows_execution(
            &client,
            chat_state::HookHandlerProvenance::Client,
            false,
        ));
        assert!(!frozen_handler_source_allows_execution(
            &client,
            chat_state::HookHandlerProvenance::UserConfig,
            true,
        ));
    }

    #[test]
    fn frozen_failure_policy_is_only_adapter_block_authority() {
        let failures = [
            HookRunResult::Failed {
                hook_name: "test".into(),
                error: "expanded runner detail must stay in memory".into(),
                elapsed: std::time::Duration::ZERO,
                http_info: None,
            },
            HookRunResult::TimedOut {
                hook_name: "test".into(),
                timeout_ms: 10,
                elapsed: std::time::Duration::ZERO,
                http_info: None,
            },
            HookRunResult::Cancelled {
                hook_name: "test".into(),
                elapsed: std::time::Duration::ZERO,
                http_info: None,
            },
        ];
        for (index, failure) in failures.into_iter().enumerate() {
            let occurrence_id = format!("occurrence-{index}");
            let run_id = format!("run-{index}");
            let outcome = result_outcome(&failure);
            let candidate = || ExecutedHandler {
                outcome: outcome.clone(),
                control: chat_state::HookRunControl::Block {
                    reason: "runner supplied duplicate control".into(),
                },
                result: failure.clone(),
                gate: HookGateEffect::Tool(HookDecision::Deny {
                    reason: "runner supplied duplicate decision".into(),
                    hook_name: "test".into(),
                }),
                decisive: true,
            };
            let allowed = apply_frozen_failure_policy(
                GateKind::Tool,
                chat_state::HookFailurePolicy::Allow,
                "test",
                candidate(),
            );
            assert!(matches!(allowed.control, chat_state::HookRunControl::None));
            assert!(matches!(
                allowed.gate,
                HookGateEffect::Tool(HookDecision::Allow)
            ));
            assert!(!allowed.decisive);

            let prompt_blocked = apply_frozen_failure_policy(
                GateKind::Prompt,
                chat_state::HookFailurePolicy::Block,
                "test",
                candidate(),
            );
            assert!(matches!(
                prompt_blocked.gate,
                HookGateEffect::Prompt(HookDecision::Deny { .. })
            ));
            assert!(matches!(
                prompt_blocked.control,
                chat_state::HookRunControl::Block { .. }
            ));

            let blocked = apply_frozen_failure_policy(
                GateKind::Tool,
                chat_state::HookFailurePolicy::Block,
                "test",
                candidate(),
            );
            let reason = match &blocked.control {
                chat_state::HookRunControl::Block { reason } => reason.clone(),
                other => panic!("typed blocking failure must derive one control, got {other:?}"),
            };
            assert!(!reason.contains("runner supplied"));
            assert!(blocked.decisive);
            let mut aggregate = HookAggregate::Tool {
                decision: HookDecision::Allow,
                results: Vec::new(),
            };
            absorb_execution(&mut aggregate, "test", &blocked.gate);
            let mut timeline = chat_state::Timeline::default();
            let turn = chat_state::TurnId((index + 1) as u64);
            let step = chat_state::StepId { turn, index: 0 };
            timeline
                .record(chat_state::TimelineEventKind::Turn(
                    chat_state::TurnEvent::Started {
                        id: turn,
                        input_ids: Vec::new(),
                        identity: chat_state::TurnIdentity {
                            origin: "test".into(),
                            turn_kind: "internal".into(),
                            goal_id: None,
                            goal_definition_revision: None,
                            stage_id: None,
                        },
                        model_id: "model".into(),
                        input_message_count: 0,
                        prompt_index: index,
                        prompt_text: "test".into(),
                        input_kind: chat_state::TurnInputKind::Prompt,
                        redirect_kind: None,
                    },
                ))
                .unwrap();
            timeline
                .record(chat_state::TimelineEventKind::Step(
                    chat_state::StepEvent::Started { id: step },
                ))
                .unwrap();
            timeline
                .record(chat_state::TimelineEventKind::Tool(
                    chat_state::ToolEvent::Started {
                        call_id: format!("call-{index}"),
                        turn,
                        step,
                        name: "test_tool".into(),
                        input: None,
                    },
                ))
                .unwrap();
            timeline
                .record(chat_state::TimelineEventKind::Hook(
                    chat_state::HookEvent::Triggered {
                        occurrence_id: occurrence_id.clone(),
                        event: chat_state::HookEventType::PreToolUse,
                        gate: chat_state::HookGateKind::Tool,
                        cause: chat_state::HookCause::Tool {
                            call_id: format!("call-{index}"),
                        },
                        config_generation: 1,
                        handlers: vec![chat_state::HookHandlerPlan {
                            index: 0,
                            run_id: run_id.clone(),
                            name: "test".into(),
                            provenance: chat_state::HookHandlerProvenance::UserConfig,
                            kind: chat_state::HookHandlerKind::Command,
                            failure_policy: chat_state::HookFailurePolicy::Block,
                            action: chat_state::HookHandlerPlanAction::Execute,
                        }],
                    },
                ))
                .unwrap();
            timeline
                .record(chat_state::TimelineEventKind::Hook(
                    chat_state::HookEvent::RunStarted {
                        occurrence_id: occurrence_id.clone(),
                        run_id: run_id.clone(),
                        handler_index: 0,
                    },
                ))
                .unwrap();
            timeline
                .record(chat_state::TimelineEventKind::Hook(
                    chat_state::HookEvent::RunFinished {
                        occurrence_id: occurrence_id.clone(),
                        run_id,
                        handler_index: 0,
                        elapsed_ms: 0,
                        outcome: blocked.outcome,
                        control: blocked.control,
                    },
                ))
                .unwrap();
            timeline
                .record(chat_state::TimelineEventKind::Hook(
                    chat_state::HookEvent::Completed {
                        occurrence_id,
                        decision: timeline_decision(&aggregate),
                    },
                ))
                .unwrap();
        }
    }

    #[test]
    fn durable_hook_text_is_utf8_safe_bounded_and_nul_free() {
        let raw = format!("{}\0secret-tail", "界".repeat(2_000));
        let gate = HookGateEffect::Tool(HookDecision::Deny {
            reason: raw.clone(),
            hook_name: "test".into(),
        });
        let normalized = match gate_control(&gate) {
            chat_state::HookRunControl::Block { reason } => reason,
            other => panic!("deny must persist block control, got {other:?}"),
        };

        assert!(normalized.len() <= chat_state::MAX_HOOK_CONTROL_TEXT_BYTES);
        assert!(!normalized.contains('\0'));
        assert!(std::str::from_utf8(normalized.as_bytes()).is_ok());
        assert_eq!(normalized.len() % "界".len(), 0);

        let aggregate = HookAggregate::Tool {
            decision: HookDecision::Deny {
                reason: raw,
                hook_name: "test".into(),
            },
            results: Vec::new(),
        };
        let decision_reason = match timeline_decision(&aggregate) {
            chat_state::HookAggregateDecision::Tool {
                decision: chat_state::HookGateDecision::Block { reason },
            } => reason,
            other => panic!("deny must persist block decision, got {other:?}"),
        };
        assert_eq!(decision_reason, normalized);

        let stop_control = gate_control(&HookGateEffect::Stop(::hooks::dispatcher::StopSignals {
            block_reason: None,
            stop_reason: None,
            additional_context: Some("文".repeat(2_000)),
        }));
        assert!(matches!(
            stop_control,
            chat_state::HookRunControl::StopKeepWorking {
                additional_context: Some(context),
                ..
            } if context.len() <= chat_state::MAX_HOOK_CONTROL_TEXT_BYTES
        ));
    }

    #[test]
    fn durable_handler_identity_is_bounded_stable_and_redacted() {
        let raw = "callback?deployment_key=super-secret";
        let first = durable_handler_identity("client", raw);
        let second = durable_handler_identity("client", raw);

        assert_eq!(first, second);
        assert!(first.starts_with("client:blake3:"));
        assert!(first.len() <= chat_state::MAX_HOOK_HANDLER_NAME_BYTES);
        assert!(!first.contains(raw));
        assert!(!first.contains("super-secret"));

        let handler = durable_handler_identity("handler", raw);
        assert!(handler.starts_with("handler:blake3:"));
        assert!(handler.len() <= chat_state::MAX_HOOK_HANDLER_NAME_BYTES);
        assert!(!handler.contains("super-secret"));
    }
}

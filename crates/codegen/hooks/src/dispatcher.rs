use crate::config::{HandlerType, HookProvenance, HookSpec, OnFailure};
use crate::discovery::HookRegistry;
use crate::event::{HookEventEnvelope, HookEventName};
use crate::result::{HookDecision, HookRunResult, HookSkipReason};
use crate::runner::{self, GateKind, HookRunnerResult, RunContext};

fn dispatch_span(event: HookEventName, hook_count: usize) -> tracing::Span {
    tracing::info_span!(
        "hooks.dispatch",
        hook_event = %event,
        hook_count = hook_count as i64,
        num_success = tracing::field::Empty,
        num_failed = tracing::field::Empty,
        num_blocking = tracing::field::Empty,
        num_skipped = tracing::field::Empty,
        total_duration_ms = tracing::field::Empty,
    )
}

/// Command/URL-free runtime identity for a planned handler. Durable adapters
/// must still bound and opaque the name because source labels are user-owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookHandlerIdentity {
    pub name: String,
    pub handler_type: HandlerType,
    pub provenance: HookProvenance,
}

#[derive(Debug, Clone)]
pub enum HookPlanAction {
    Execute(Box<HookSpec>),
    Skip(HookSkipReason),
}

#[derive(Debug, Clone)]
pub struct HookPlanEntry {
    pub identity: HookHandlerIdentity,
    pub action: HookPlanAction,
}

fn safe_identity(spec: &HookSpec) -> HookHandlerIdentity {
    HookHandlerIdentity {
        name: spec.name.clone(),
        handler_type: spec.handler_type,
        provenance: spec.layer,
    }
}

fn plan_dispatch_with_policy(
    registry: &HookRegistry,
    event: HookEventName,
    envelope: &HookEventEnvelope,
    policy_disabled: impl Fn(&str) -> bool,
) -> Vec<HookPlanEntry> {
    let match_value = envelope.payload.match_value();
    registry
        .hooks_for(event)
        .iter()
        .map(|spec| {
            let reason = if spec.validate().is_err() {
                Some(HookSkipReason::PolicyDisabled)
            } else if !spec.enabled {
                Some(HookSkipReason::Disabled)
            } else if policy_disabled(&spec.name) {
                Some(HookSkipReason::PolicyDisabled)
            } else if !crate::matcher::matcher_allows(spec.matcher.as_ref(), match_value) {
                Some(HookSkipReason::MatcherMiss)
            } else {
                None
            };
            HookPlanEntry {
                identity: safe_identity(spec),
                action: reason
                    .map(HookPlanAction::Skip)
                    .unwrap_or_else(|| HookPlanAction::Execute(Box::new(spec.clone()))),
            }
        })
        .collect()
}

/// Deterministic, owned dispatch plan in registry order. Planning performs no
/// hook execution, allowing a caller to durably record Triggered first.
pub fn plan_dispatch(
    registry: &HookRegistry,
    event: HookEventName,
    envelope: &HookEventEnvelope,
) -> Vec<HookPlanEntry> {
    plan_dispatch_with_policy(registry, event, envelope, crate::trust::is_hook_disabled)
}

fn planned_skip(identity: &HookHandlerIdentity, reason: HookSkipReason) -> HookRunResult {
    HookRunResult::Skipped {
        hook_name: identity.name.clone(),
        reason,
    }
}

#[derive(Debug, Clone)]
pub enum HookGateEffect {
    Observe,
    Prompt(HookDecision),
    Tool(HookDecision),
    Stop(StopSignals),
}

#[derive(Debug, Clone)]
pub struct HookExecution {
    pub result: HookRunResult,
    pub gate: HookGateEffect,
    /// True when an admission chain must not execute later handlers.
    pub decisive: bool,
}

async fn run_one_hook_detailed(
    spec: &HookSpec,
    envelope: &HookEventEnvelope,
    ctx: &RunContext<'_>,
    gate: GateKind,
) -> HookExecution {
    let (raw, elapsed, http_info) = runner::run_hook(spec, envelope, ctx, gate).await;
    let result = match &raw {
        HookRunnerResult::Decision(HookDecision::Deny { reason, .. }) => HookRunResult::Blocked {
            hook_name: spec.name.clone(),
            detail: format!("denied: {reason}"),
            elapsed,
            http_info: http_info.clone(),
        },
        HookRunnerResult::Stop(outcome) => match stop_outcome_detail(outcome) {
            Some(detail) => HookRunResult::Blocked {
                hook_name: spec.name.clone(),
                detail,
                elapsed,
                http_info: http_info.clone(),
            },
            None => HookRunResult::Success {
                hook_name: spec.name.clone(),
                elapsed,
                http_info: http_info.clone(),
            },
        },
        HookRunnerResult::TimedOut => HookRunResult::TimedOut {
            hook_name: spec.name.clone(),
            timeout_ms: spec.timeout_ms,
            elapsed,
            http_info: http_info.clone(),
        },
        HookRunnerResult::Cancelled => HookRunResult::Cancelled {
            hook_name: spec.name.clone(),
            elapsed,
            http_info: http_info.clone(),
        },
        HookRunnerResult::Failed(error) => HookRunResult::Failed {
            hook_name: spec.name.clone(),
            error: error.clone(),
            elapsed,
            http_info: http_info.clone(),
        },
        HookRunnerResult::Success | HookRunnerResult::Decision(HookDecision::Allow) => {
            HookRunResult::Success {
                hook_name: spec.name.clone(),
                elapsed,
                http_info: http_info.clone(),
            }
        }
    };
    let failed = matches!(
        &result,
        HookRunResult::Failed { .. }
            | HookRunResult::TimedOut { .. }
            | HookRunResult::Cancelled { .. }
    );
    let failure_decision = || {
        if spec.on_failure == OnFailure::Block && failed {
            HookDecision::Deny {
                reason: crate::event::clip_text(
                    &format!("hook '{}' failed and is configured to block", spec.name),
                    1000,
                ),
                hook_name: spec.name.clone(),
            }
        } else {
            HookDecision::Allow
        }
    };
    let gate = match (&raw, gate) {
        (HookRunnerResult::Decision(decision), GateKind::Prompt) => {
            HookGateEffect::Prompt(decision.clone())
        }
        (HookRunnerResult::Decision(decision), GateKind::Tool) => {
            HookGateEffect::Tool(decision.clone())
        }
        (HookRunnerResult::Stop(outcome), GateKind::Stop) => HookGateEffect::Stop(StopSignals {
            block_reason: outcome.block_reason.clone(),
            stop_reason: outcome.force_stop.as_ref().map(|force| {
                force
                    .reason
                    .clone()
                    .unwrap_or_else(|| "stopped by hook".to_string())
            }),
            additional_context: outcome.additional_context.clone(),
        }),
        (_, GateKind::Prompt) => HookGateEffect::Prompt(failure_decision()),
        (_, GateKind::Tool) => HookGateEffect::Tool(failure_decision()),
        (_, GateKind::Stop) => HookGateEffect::Stop(StopSignals::default()),
        (_, GateKind::Observe) => HookGateEffect::Observe,
    };
    let decisive = match &gate {
        HookGateEffect::Prompt(HookDecision::Deny { .. })
        | HookGateEffect::Tool(HookDecision::Deny { .. }) => true,
        HookGateEffect::Stop(signals) => {
            signals.stop_reason.is_some() || signals.block_reason.is_some()
        }
        HookGateEffect::Observe
        | HookGateEffect::Prompt(HookDecision::Allow)
        | HookGateEffect::Tool(HookDecision::Allow) => false,
    };
    HookExecution {
        result,
        gate,
        decisive,
    }
}

/// Execute exactly one already-planned spec.
pub async fn run_one_hook(
    spec: &HookSpec,
    envelope: &HookEventEnvelope,
    ctx: &RunContext<'_>,
    gate: GateKind,
) -> HookExecution {
    run_one_hook_detailed(spec, envelope, ctx, gate).await
}

/// Result of a `pre_tool_use` dispatch: the final decision plus per-hook
/// execution details (for scrollback enrichment).
pub struct PreToolUseResult {
    pub decision: HookDecision,
    pub results: Vec<HookRunResult>,
}

/// Dispatch a `pre_tool_use` event against all matching hooks.
///
/// Runs hooks sequentially in config order. Only an explicit `deny`
/// decision from a hook stops the chain and blocks the tool call.
///
/// Hook failures (timeouts, cancellations, crashes, command-not-found,
/// env-var pre-spawn refusals, malformed output) follow the handler's explicit
/// `on_failure` policy. The default is fail-open; `on_failure = block` makes
/// that handler's failure decisive.
///
/// Returns `Allow` if no hooks match, all hooks allow, or all failing
/// hooks are non-blocking by virtue of this fail-open policy.
pub async fn dispatch_pre_tool_use(
    registry: &HookRegistry,
    envelope: &HookEventEnvelope,
    ctx: &RunContext<'_>,
) -> PreToolUseResult {
    let plan = plan_dispatch(registry, HookEventName::PreToolUse, envelope);
    if plan.is_empty() {
        return PreToolUseResult {
            decision: HookDecision::Allow,
            results: Vec::new(),
        };
    }

    let span = dispatch_span(HookEventName::PreToolUse, plan.len());
    let _enter = span.enter();

    let mut run_results = Vec::new();
    let mut decision = HookDecision::Allow;
    let mut prior_block = false;

    for entry in plan {
        if prior_block {
            run_results.push(planned_skip(&entry.identity, HookSkipReason::PriorBlock));
            continue;
        }
        let spec = match entry.action {
            HookPlanAction::Execute(spec) => spec,
            HookPlanAction::Skip(reason) => {
                run_results.push(planned_skip(&entry.identity, reason));
                continue;
            }
        };
        let execution = run_one_hook(&spec, envelope, ctx, GateKind::Tool).await;
        if let HookGateEffect::Tool(handler_decision) = execution.gate
            && execution.decisive
        {
            decision = handler_decision;
            prior_block = true;
        }
        run_results.push(execution.result);
    }

    record_dispatch_counts(&span, &run_results);
    PreToolUseResult {
        decision,
        results: run_results,
    }
}

/// Admission dispatch for `user_prompt_submit`. Explicit deny and execution
/// failure plus `on_failure = block` both deny the prompt.
pub async fn dispatch_user_prompt_submit(
    registry: &HookRegistry,
    envelope: &HookEventEnvelope,
    ctx: &RunContext<'_>,
) -> PreToolUseResult {
    let plan = plan_dispatch(registry, HookEventName::UserPromptSubmit, envelope);
    let mut results = Vec::with_capacity(plan.len());
    let mut decision = HookDecision::Allow;
    let mut prior_block = false;
    for entry in plan {
        if prior_block {
            results.push(planned_skip(&entry.identity, HookSkipReason::PriorBlock));
            continue;
        }
        match entry.action {
            HookPlanAction::Skip(reason) => results.push(planned_skip(&entry.identity, reason)),
            HookPlanAction::Execute(spec) => {
                let execution = run_one_hook(&spec, envelope, ctx, GateKind::Prompt).await;
                if let HookGateEffect::Prompt(handler_decision) = execution.gate
                    && execution.decisive
                {
                    decision = handler_decision;
                    prior_block = true;
                }
                results.push(execution.result);
            }
        }
    }
    PreToolUseResult { decision, results }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopBlock {
    pub hook_name: String,
    pub reason: String,
}

/// Aggregated signals from a `Stop`/`SubagentStop` gate dispatch.
#[derive(Debug, Default)]
pub struct StopDispatchResult {
    pub blocks: Vec<StopBlock>,
    pub additional_context: Vec<String>,
    /// First `continue: false` wins and overrides any blocks.
    pub prevent_continuation: Option<StopBlock>,
    pub results: Vec<HookRunResult>,
}

impl StopDispatchResult {
    pub fn wants_continuation(&self) -> bool {
        self.prevent_continuation.is_none()
            && (!self.blocks.is_empty() || !self.additional_context.is_empty())
    }

    /// The first force-stop wins (later ones are dropped); blocks and context
    /// accumulate in call order.
    pub fn absorb(&mut self, hook_name: &str, signals: StopSignals) {
        if let Some(reason) = signals.stop_reason
            && self.prevent_continuation.is_none()
        {
            self.prevent_continuation = Some(StopBlock {
                hook_name: hook_name.to_string(),
                reason,
            });
        }
        if let Some(reason) = signals.block_reason {
            self.blocks.push(StopBlock {
                hook_name: hook_name.to_string(),
                reason,
            });
        }
        if let Some(context) = signals.additional_context {
            self.additional_context.push(context);
        }
    }
}

/// One hook's stop signals, normalized for [`StopDispatchResult::absorb`].
/// A `Some` in `stop_reason` is what marks the hook as force-stopping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StopSignals {
    pub block_reason: Option<String>,
    pub stop_reason: Option<String>,
    pub additional_context: Option<String>,
}

/// Scrollback detail for a stop signal, shared by the file and client gates so
/// the wording can't drift. A force-stop wins over a block; its reason may be absent.
pub fn stop_detail(
    prevented: bool,
    prevent_reason: Option<&str>,
    block_reason: Option<&str>,
) -> Option<String> {
    if prevented {
        return Some(match prevent_reason {
            Some(reason) => format!("prevented continuation: {reason}"),
            None => "prevented continuation".to_string(),
        });
    }
    block_reason.map(|reason| format!("blocked stop: {reason}"))
}

fn stop_outcome_detail(outcome: &crate::result::StopHookOutcome) -> Option<String> {
    stop_detail(
        outcome.force_stop.is_some(),
        outcome
            .force_stop
            .as_ref()
            .and_then(|f| f.reason.as_deref()),
        outcome.block_reason.as_deref(),
    )
}

/// Dispatch a `Stop` or `SubagentStop` gate against all matching hooks.
///
/// Runs in stable order. The first block or force-stop is decisive; remaining
/// planned entries are recorded as `PriorBlock`. Additional context alone is
/// not decisive.
pub async fn dispatch_stop(
    registry: &HookRegistry,
    event: HookEventName,
    envelope: &HookEventEnvelope,
    ctx: &RunContext<'_>,
) -> StopDispatchResult {
    if event.traits().gate != GateKind::Stop {
        debug_assert!(false, "dispatch_stop called with non-stop event {event:?}");
        tracing::error!(%event, "dispatch_stop called with a non-stop event; ignoring");
        return StopDispatchResult::default();
    }
    let plan = plan_dispatch(registry, event, envelope);
    if plan.is_empty() {
        return StopDispatchResult::default();
    }

    let span = dispatch_span(event, plan.len());
    let _enter = span.enter();

    let mut out = StopDispatchResult::default();
    let mut prior_block = false;
    for entry in plan {
        if prior_block {
            out.results
                .push(planned_skip(&entry.identity, HookSkipReason::PriorBlock));
            continue;
        }
        let spec = match entry.action {
            HookPlanAction::Skip(reason) => {
                out.results.push(planned_skip(&entry.identity, reason));
                continue;
            }
            HookPlanAction::Execute(spec) => spec,
        };
        let execution = run_one_hook(&spec, envelope, ctx, GateKind::Stop).await;
        if let HookGateEffect::Stop(signals) = execution.gate {
            out.absorb(&spec.name, signals);
        }
        prior_block = execution.decisive;
        out.results.push(execution.result);
    }

    record_dispatch_counts(&span, &out.results);
    out
}

/// Dispatch an observe-only event against all matching hooks; never denies.
pub async fn dispatch_non_blocking(
    registry: &HookRegistry,
    event: HookEventName,
    envelope: &HookEventEnvelope,
    ctx: &RunContext<'_>,
) -> Vec<HookRunResult> {
    if event == HookEventName::UserPromptSubmit {
        return dispatch_user_prompt_submit(registry, envelope, ctx)
            .await
            .results;
    }
    debug_assert!(
        event.traits().gate == GateKind::Observe,
        "dispatch_non_blocking called with gate event {event:?}"
    );
    let plan = plan_dispatch(registry, event, envelope);
    if plan.is_empty() {
        return Vec::new();
    }

    let span = dispatch_span(event, plan.len());
    let _enter = span.enter();

    let mut results = Vec::with_capacity(plan.len());

    for entry in plan {
        match entry.action {
            HookPlanAction::Skip(reason) => results.push(planned_skip(&entry.identity, reason)),
            HookPlanAction::Execute(spec) => {
                results.push(
                    run_one_hook(&spec, envelope, ctx, GateKind::Observe)
                        .await
                        .result,
                );
            }
        }
    }

    record_dispatch_counts(&span, &results);

    results
}

fn record_dispatch_counts(span: &tracing::Span, results: &[HookRunResult]) {
    let mut num_success = 0i64;
    let mut num_failed = 0i64;
    let mut num_skipped = 0i64;
    let mut total_duration_ms = 0i64;
    let mut num_blocked = 0i64;
    for r in results {
        match r {
            HookRunResult::Success { elapsed, .. } => {
                num_success += 1;
                total_duration_ms += elapsed.as_millis() as i64;
            }
            HookRunResult::Blocked { elapsed, .. } => {
                num_blocked += 1;
                total_duration_ms += elapsed.as_millis() as i64;
            }
            HookRunResult::Failed { elapsed, .. } => {
                num_failed += 1;
                total_duration_ms += elapsed.as_millis() as i64;
            }
            HookRunResult::TimedOut { elapsed, .. } | HookRunResult::Cancelled { elapsed, .. } => {
                num_failed += 1;
                total_duration_ms += elapsed.as_millis() as i64;
            }
            HookRunResult::Skipped { .. } => num_skipped += 1,
        }
    }
    span.record("num_success", num_success);
    span.record("num_failed", num_failed);
    span.record("num_blocking", num_blocked);
    span.record("num_skipped", num_skipped);
    span.record("total_duration_ms", total_duration_ms);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HookSpec;
    use crate::event::{HookEventEnvelope, HookEventName, HookPayload};
    use crate::matcher::HookMatcher;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn pre_tool_use_envelope(tool_name: &str) -> HookEventEnvelope {
        HookEventEnvelope {
            hook_event_name: HookEventName::PreToolUse,
            session_id: "test-session".into(),
            cwd: "/tmp".into(),
            workspace_root: "/tmp".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            transcript_path: None,
            client_identifier: None,
            prompt_id: None,
            permission_mode: None,
            payload: HookPayload::PreToolUse {
                tool_name: tool_name.into(),
                tool_use_id: "tu-1".into(),
                tool_input: serde_json::json!({"command": "ls"}),
                tool_input_truncated: false,
                subagent_type: None,
            },
        }
    }

    fn session_start_envelope() -> HookEventEnvelope {
        HookEventEnvelope {
            hook_event_name: HookEventName::SessionStart,
            session_id: "test-session".into(),
            cwd: "/tmp".into(),
            workspace_root: "/tmp".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            transcript_path: None,
            client_identifier: None,
            prompt_id: None,
            permission_mode: None,
            payload: HookPayload::SessionStart {
                source: "new".into(),
                model_id: None,
                agent_type: None,
            },
        }
    }

    fn user_prompt_envelope() -> HookEventEnvelope {
        let mut envelope = session_start_envelope();
        envelope.hook_event_name = HookEventName::UserPromptSubmit;
        envelope.payload = HookPayload::UserPromptSubmit {
            prompt: Some("ship it".into()),
        };
        envelope
    }

    fn run_ctx() -> RunContext<'static> {
        RunContext {
            session_id: "test-session",
            workspace_root: "/tmp",
            process_scope: None,
        }
    }

    /// Helper: create a HookSpec pointing at `sh -c '<script>'` that prints
    /// the given JSON and exits with the given code.
    fn make_command_spec(
        name: &str,
        matcher: Option<&str>,
        enabled: bool,
        script: &str,
    ) -> HookSpec {
        HookSpec {
            name: name.into(),
            event: HookEventName::PreToolUse,
            handler_type: crate::config::HandlerType::Command,
            configured_matcher: matcher.map(|s| s.to_string()),
            matcher: matcher.map(|s| HookMatcher::new(s).unwrap()),
            enabled,
            command: Some(PathBuf::from(script)),
            command_raw: Some(script.to_string()),
            url: None,
            url_raw: None,
            timeout_ms: 5000,
            on_failure: crate::config::OnFailure::Allow,
            source_dir: PathBuf::from("/tmp"),
            extra_env: HashMap::new(),
            layer: crate::config::HookProvenance::File,
        }
    }

    fn registry_from_specs(specs: Vec<HookSpec>) -> HookRegistry {
        let (mut registry, _) = crate::discovery::load_hooks(None, None);
        registry.append_specs(specs);
        registry
    }

    #[test]
    fn planning_records_matcher_and_policy_skips_in_stable_order() {
        let execute = make_command_spec("execute", Some("read_file"), true, "echo ok");
        let miss = make_command_spec("miss", Some("write_file"), true, "echo ok");
        let disabled = make_command_spec("disabled", None, false, "echo ok");
        let policy = make_command_spec("policy", None, true, "echo ok");
        let registry = registry_from_specs(vec![execute, miss, disabled, policy]);
        let plan = plan_dispatch_with_policy(
            &registry,
            HookEventName::PreToolUse,
            &pre_tool_use_envelope("read_file"),
            |name| name == "policy",
        );
        assert_eq!(
            plan.iter()
                .map(|entry| entry.identity.name.as_str())
                .collect::<Vec<_>>(),
            ["execute", "miss", "disabled", "policy"]
        );
        assert!(matches!(&plan[0].action, HookPlanAction::Execute(_)));
        assert!(matches!(
            &plan[1].action,
            HookPlanAction::Skip(HookSkipReason::MatcherMiss)
        ));
        assert!(matches!(
            &plan[2].action,
            HookPlanAction::Skip(HookSkipReason::Disabled)
        ));
        assert!(matches!(
            &plan[3].action,
            HookPlanAction::Skip(HookSkipReason::PolicyDisabled)
        ));
    }

    #[tokio::test]
    async fn on_failure_block_is_decisive_and_marks_later_hooks_prior_block() {
        let mut strict = make_command_spec("strict", None, true, "exit 1");
        strict.on_failure = OnFailure::Block;
        let later = make_command_spec("later", None, true, "echo '{\"decision\":\"allow\"}'");
        let result = dispatch_pre_tool_use(
            &registry_from_specs(vec![strict, later]),
            &pre_tool_use_envelope("read_file"),
            &run_ctx(),
        )
        .await;
        assert!(matches!(result.decision, HookDecision::Deny { .. }));
        assert!(matches!(result.results[0], HookRunResult::Failed { .. }));
        assert!(matches!(
            result.results[1],
            HookRunResult::Skipped {
                reason: HookSkipReason::PriorBlock,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn user_prompt_explicit_deny_blocks_and_skips_later_hooks() {
        let mut deny = make_command_spec(
            "prompt-deny",
            None,
            true,
            "echo '{\"decision\":\"block\",\"reason\":\"no prompt\"}'",
        );
        deny.event = HookEventName::UserPromptSubmit;
        let mut later = make_command_spec("later", None, true, "echo ok");
        later.event = HookEventName::UserPromptSubmit;
        let result = dispatch_user_prompt_submit(
            &registry_from_specs(vec![deny, later]),
            &user_prompt_envelope(),
            &run_ctx(),
        )
        .await;
        assert_eq!(
            result.decision,
            HookDecision::Deny {
                reason: "no prompt".into(),
                hook_name: "prompt-deny".into(),
            }
        );
        assert!(matches!(result.results[0], HookRunResult::Blocked { .. }));
        assert!(matches!(
            result.results[1],
            HookRunResult::Skipped {
                reason: HookSkipReason::PriorBlock,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn user_prompt_on_failure_block_denies() {
        let mut strict = make_command_spec("prompt-failure", None, true, "exit 1");
        strict.event = HookEventName::UserPromptSubmit;
        strict.on_failure = OnFailure::Block;
        let result = dispatch_user_prompt_submit(
            &registry_from_specs(vec![strict]),
            &user_prompt_envelope(),
            &run_ctx(),
        )
        .await;
        assert!(matches!(result.decision, HookDecision::Deny { .. }));
        assert!(matches!(result.results[0], HookRunResult::Failed { .. }));
    }

    #[test]
    fn match_value_extracts_per_payload_field() {
        assert_eq!(
            pre_tool_use_envelope("run_terminal_cmd")
                .payload
                .match_value(),
            Some("run_terminal_cmd")
        );
        assert_eq!(session_start_envelope().payload.match_value(), Some("new"));

        let notification = HookPayload::Notification {
            notification_type: "permission_prompt".into(),
            message: None,
            title: None,
            level: None,
        };
        assert_eq!(notification.match_value(), Some("permission_prompt"));
    }

    /// An empty subagent type (parent-side fire with no spawn record) yields
    /// `None` so matchers fire-all instead of silently matching nothing.
    #[test]
    fn subagent_match_value_is_none_when_type_empty() {
        let mut envelope = stop_envelope();
        envelope.hook_event_name = HookEventName::SubagentStop;
        let payload = |subagent_type: &str| HookPayload::SubagentStop {
            phase: crate::event::SubagentStopPhase::Observe,
            subagent_id: "sub-1".into(),
            subagent_type: subagent_type.into(),
            stop_hook_active: None,
            last_assistant_message: None,
        };
        envelope.payload = payload("explore");
        assert_eq!(envelope.payload.match_value(), Some("explore"));
        envelope.payload = payload("");
        assert_eq!(envelope.payload.match_value(), None);
    }

    #[tokio::test]
    async fn empty_registry_allows() {
        let registry = registry_from_specs(vec![]);
        let envelope = pre_tool_use_envelope("run_terminal_cmd");
        let result = dispatch_pre_tool_use(&registry, &envelope, &run_ctx()).await;
        assert_eq!(result.decision, HookDecision::Allow);
    }

    #[tokio::test]
    async fn single_deny_hook() {
        let spec = make_command_spec(
            "deny-hook",
            None,
            true,
            "echo '{\"decision\":\"deny\",\"reason\":\"blocked\"}'; exit 2",
        );
        let registry = registry_from_specs(vec![spec]);
        let envelope = pre_tool_use_envelope("run_terminal_cmd");
        let result = dispatch_pre_tool_use(&registry, &envelope, &run_ctx()).await;
        match result.decision {
            HookDecision::Deny {
                ref reason,
                ref hook_name,
            } => {
                assert_eq!(reason, "blocked");
                assert_eq!(hook_name, "deny-hook");
            }
            ref other => panic!("expected Deny, got {other:?}"),
        }
        assert_eq!(result.results.len(), 1);
    }

    #[tokio::test]
    async fn disabled_hook_is_skipped_allows() {
        let spec = make_command_spec(
            "disabled-deny",
            None,
            false, // disabled!
            "echo '{\"decision\":\"deny\",\"reason\":\"should not run\"}'; exit 2",
        );
        let registry = registry_from_specs(vec![spec]);
        let envelope = pre_tool_use_envelope("run_terminal_cmd");
        let result = dispatch_pre_tool_use(&registry, &envelope, &run_ctx()).await;
        assert_eq!(result.decision, HookDecision::Allow);
    }

    #[tokio::test]
    async fn matcher_filters_by_tool() {
        let spec = make_command_spec(
            "bash-deny",
            Some("run_terminal_cmd"),
            true,
            "echo '{\"decision\":\"deny\",\"reason\":\"bash blocked\"}'; exit 2",
        );
        let registry = registry_from_specs(vec![spec]);

        let fired = dispatch_pre_tool_use(
            &registry,
            &pre_tool_use_envelope("run_terminal_cmd"),
            &run_ctx(),
        )
        .await;
        match fired.decision {
            HookDecision::Deny { ref reason, .. } => assert_eq!(reason, "bash blocked"),
            ref other => panic!("expected Deny, got {other:?}"),
        }

        let skipped =
            dispatch_pre_tool_use(&registry, &pre_tool_use_envelope("read_file"), &run_ctx()).await;
        assert_eq!(skipped.decision, HookDecision::Allow);
        assert!(matches!(
            skipped.results[0],
            HookRunResult::Skipped {
                reason: HookSkipReason::MatcherMiss,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn first_deny_wins_short_circuits() {
        let deny_spec = make_command_spec(
            "first-deny",
            None,
            true,
            "echo '{\"decision\":\"deny\",\"reason\":\"first says no\"}'; exit 2",
        );
        let allow_spec = make_command_spec(
            "second-allow",
            None,
            true,
            "echo '{\"decision\":\"allow\"}'",
        );
        let registry = registry_from_specs(vec![deny_spec, allow_spec]);
        let envelope = pre_tool_use_envelope("run_terminal_cmd");
        let result = dispatch_pre_tool_use(&registry, &envelope, &run_ctx()).await;
        match result.decision {
            HookDecision::Deny {
                ref reason,
                ref hook_name,
                ..
            } => {
                assert_eq!(reason, "first says no");
                assert_eq!(hook_name, "first-deny");
            }
            ref other => panic!("expected Deny, got {other:?}"),
        }
        assert!(matches!(
            result.results[1],
            HookRunResult::Skipped {
                reason: HookSkipReason::PriorBlock,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn allow_then_deny_denies() {
        let allow_spec =
            make_command_spec("broad-allow", None, true, "echo '{\"decision\":\"allow\"}'");
        let deny_spec = make_command_spec(
            "strict-deny",
            None,
            true,
            "echo '{\"decision\":\"deny\",\"reason\":\"strict policy\"}'; exit 2",
        );
        let registry = registry_from_specs(vec![allow_spec, deny_spec]);
        let envelope = pre_tool_use_envelope("run_terminal_cmd");
        let result = dispatch_pre_tool_use(&registry, &envelope, &run_ctx()).await;
        match result.decision {
            HookDecision::Deny {
                ref reason,
                ref hook_name,
                ..
            } => {
                assert_eq!(reason, "strict policy");
                assert_eq!(hook_name, "strict-deny");
            }
            ref other => panic!("expected Deny from strict filter, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn fail_open_on_hook_crash() {
        let spec = make_command_spec("crasher", None, true, "exit 1");
        let registry = registry_from_specs(vec![spec]);
        let envelope = pre_tool_use_envelope("run_terminal_cmd");
        let result = dispatch_pre_tool_use(&registry, &envelope, &run_ctx()).await;
        assert_eq!(
            result.decision,
            HookDecision::Allow,
            "fail-open: a crashing hook must not block the tool call"
        );
        assert_eq!(result.results.len(), 1);
        assert!(
            matches!(&result.results[0], HookRunResult::Failed { hook_name, .. } if hook_name == "crasher"),
            "the failure must still appear in run_results for UI scrollback, got {:?}",
            result.results
        );
    }

    #[tokio::test]
    async fn fail_open_then_deny_lets_deny_win() {
        let crash_spec = make_command_spec("crasher", None, true, "exit 1");
        let deny_spec = make_command_spec(
            "denier",
            None,
            true,
            "echo '{\"decision\":\"deny\",\"reason\":\"nope\"}'; exit 2",
        );
        let registry = registry_from_specs(vec![crash_spec, deny_spec]);
        let envelope = pre_tool_use_envelope("run_terminal_cmd");
        let result = dispatch_pre_tool_use(&registry, &envelope, &run_ctx()).await;
        match result.decision {
            HookDecision::Deny {
                ref hook_name,
                ref reason,
            } => {
                assert_eq!(hook_name, "denier");
                assert_eq!(reason, "nope");
            }
            ref other => panic!("expected Deny from explicit denier, got {other:?}"),
        }
        assert_eq!(result.results.len(), 2);
        assert!(
            matches!(&result.results[1], HookRunResult::Blocked { detail, .. }
                if detail == "denied: nope"),
            "a deny is the hook's decision, not a failure: {:?}",
            result.results[1]
        );
    }

    fn stop_envelope() -> HookEventEnvelope {
        HookEventEnvelope {
            hook_event_name: HookEventName::Stop,
            session_id: "test-session".into(),
            cwd: "/tmp".into(),
            workspace_root: "/tmp".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            transcript_path: None,
            client_identifier: None,
            prompt_id: None,
            permission_mode: None,
            payload: HookPayload::Stop {
                reason: "end_turn".into(),
                stop_hook_active: false,
                last_assistant_message: Some("done".into()),
                background_tasks: None,
                session_crons: None,
            },
        }
    }

    fn stop_spec(name: &str, script: &str) -> HookSpec {
        let mut spec = make_command_spec(name, None, true, script);
        spec.event = HookEventName::Stop;
        spec
    }

    #[test]
    fn absorb_folds_signals_with_first_force_stop_winning() {
        let mut out = StopDispatchResult::default();
        out.absorb(
            "b1",
            StopSignals {
                block_reason: Some("first block".into()),
                ..Default::default()
            },
        );
        out.absorb(
            "s1",
            StopSignals {
                stop_reason: Some("stop now".into()),
                additional_context: Some("ctx".into()),
                ..Default::default()
            },
        );
        out.absorb(
            "s2",
            StopSignals {
                stop_reason: Some("too late".into()),
                block_reason: Some("second block".into()),
                ..Default::default()
            },
        );

        assert!(!out.wants_continuation(), "a force-stop overrides blocks");
        assert_eq!(
            out.blocks
                .iter()
                .map(|b| b.reason.as_str())
                .collect::<Vec<_>>(),
            ["first block", "second block"]
        );
        assert_eq!(out.additional_context, ["ctx"]);
        let prevent = out
            .prevent_continuation
            .as_ref()
            .expect("force-stop captured");
        assert_eq!(prevent.hook_name, "s1");
        assert_eq!(prevent.reason, "stop now");
    }

    #[test]
    fn absorb_empty_wants_no_continuation() {
        let out = StopDispatchResult::default();
        assert!(!out.wants_continuation());
        assert!(out.prevent_continuation.is_none());
    }

    #[tokio::test]
    async fn stop_first_block_skips_remaining_plan() {
        let registry = registry_from_specs(vec![
            stop_spec("b1", "echo '{\"decision\":\"block\",\"reason\":\"first\"}'"),
            stop_spec("allow", "echo ok"),
            stop_spec(
                "b2",
                "echo '{\"decision\":\"block\",\"reason\":\"second\"}'",
            ),
        ]);
        let result =
            dispatch_stop(&registry, HookEventName::Stop, &stop_envelope(), &run_ctx()).await;
        assert!(result.wants_continuation());
        assert_eq!(
            result
                .blocks
                .iter()
                .map(|b| b.reason.as_str())
                .collect::<Vec<_>>(),
            ["first"]
        );
        assert_eq!(result.results.len(), 3);
        assert!(matches!(
            result.results[1],
            HookRunResult::Skipped {
                reason: HookSkipReason::PriorBlock,
                ..
            }
        ));
        assert!(matches!(
            result.results[2],
            HookRunResult::Skipped {
                reason: HookSkipReason::PriorBlock,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn stop_prevent_continuation_overrides_blocks() {
        let registry = registry_from_specs(vec![
            stop_spec(
                "stopper",
                "echo '{\"continue\":false,\"stopReason\":\"enough\"}'",
            ),
            stop_spec(
                "blocker",
                "echo '{\"decision\":\"block\",\"reason\":\"keep going\"}'",
            ),
        ]);
        let result =
            dispatch_stop(&registry, HookEventName::Stop, &stop_envelope(), &run_ctx()).await;
        assert!(!result.wants_continuation());
        let prevent = result
            .prevent_continuation
            .expect("continue:false captured");
        assert_eq!(prevent.hook_name, "stopper");
        assert_eq!(prevent.reason, "enough");
        assert!(result.blocks.is_empty());
        assert!(matches!(
            result.results[1],
            HookRunResult::Skipped {
                reason: HookSkipReason::PriorBlock,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn stop_exit2_fail_open_and_context() {
        let registry = registry_from_specs(vec![
            stop_spec("exit2", "echo 'fix the build' >&2; exit 2"),
            stop_spec("crasher", "exit 1"),
            stop_spec("ctx", "echo '{\"additionalContext\":\"note\"}'"),
        ]);
        let result =
            dispatch_stop(&registry, HookEventName::Stop, &stop_envelope(), &run_ctx()).await;
        assert!(result.wants_continuation());
        assert_eq!(result.blocks.len(), 1);
        assert_eq!(result.blocks[0].reason, "fix the build");
        assert!(result.additional_context.is_empty());
        assert!(matches!(
            result.results[1],
            HookRunResult::Skipped {
                reason: HookSkipReason::PriorBlock,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn stop_additional_context_only_keeps_working() {
        let registry = registry_from_specs(vec![stop_spec(
            "ctx",
            "echo '{\"additionalContext\":\"run the tests\"}'",
        )]);
        let result =
            dispatch_stop(&registry, HookEventName::Stop, &stop_envelope(), &run_ctx()).await;
        assert!(
            result.wants_continuation(),
            "context alone must keep working"
        );
        assert!(result.blocks.is_empty());
        assert!(result.prevent_continuation.is_none());
        assert_eq!(result.additional_context, ["run the tests"]);
    }

    #[tokio::test]
    async fn stop_additional_context_is_not_decisive_but_later_block_is() {
        let registry = registry_from_specs(vec![
            stop_spec("ctx", "echo '{\"additionalContext\":\"run the tests\"}'"),
            stop_spec(
                "block",
                "echo '{\"decision\":\"block\",\"reason\":\"failed\"}'",
            ),
            stop_spec("later", "echo ok"),
        ]);
        let result =
            dispatch_stop(&registry, HookEventName::Stop, &stop_envelope(), &run_ctx()).await;

        assert_eq!(result.additional_context, ["run the tests"]);
        assert_eq!(result.blocks.len(), 1);
        assert_eq!(result.blocks[0].reason, "failed");
        assert!(matches!(
            result.results[2],
            HookRunResult::Skipped {
                reason: HookSkipReason::PriorBlock,
                ..
            }
        ));
    }

    /// A timed-out stop hook fails open: stdout of a killed hook is never
    /// interpreted, so a block written before hanging is ignored.
    #[tokio::test]
    async fn stop_timeout_fails_open() {
        let mut spec = stop_spec(
            "slow",
            "echo '{\"decision\":\"block\",\"reason\":\"late\"}'; sleep 5",
        );
        spec.timeout_ms = 200;
        let registry = registry_from_specs(vec![spec]);
        let result =
            dispatch_stop(&registry, HookEventName::Stop, &stop_envelope(), &run_ctx()).await;
        assert!(
            !result.wants_continuation(),
            "timeout must not block the stop"
        );
        assert!(
            matches!(&result.results[0], HookRunResult::TimedOut { .. }),
            "the timeout is recorded as a failure, got {:?}",
            result.results[0]
        );
    }

    #[tokio::test]
    async fn stop_empty_and_allowing_registries_allow_stop() {
        let registry = registry_from_specs(vec![]);
        let result =
            dispatch_stop(&registry, HookEventName::Stop, &stop_envelope(), &run_ctx()).await;
        assert!(!result.wants_continuation());
        assert!(result.results.is_empty());

        let registry = registry_from_specs(vec![stop_spec("ok", "echo done")]);
        let result =
            dispatch_stop(&registry, HookEventName::Stop, &stop_envelope(), &run_ctx()).await;
        assert!(!result.wants_continuation());
    }

    #[tokio::test]
    async fn subagent_stop_matcher_filters_by_agent_type() {
        let mut reviewer = make_command_spec(
            "reviewer",
            Some("code-reviewer"),
            true,
            "echo '{\"decision\":\"block\",\"reason\":\"from reviewer\"}'",
        );
        reviewer.event = HookEventName::SubagentStop;
        let mut explorer = make_command_spec(
            "explorer",
            Some("explore"),
            true,
            "echo '{\"decision\":\"block\",\"reason\":\"from explorer\"}'",
        );
        explorer.event = HookEventName::SubagentStop;
        let registry = registry_from_specs(vec![reviewer, explorer]);

        let mut envelope = stop_envelope();
        envelope.hook_event_name = HookEventName::SubagentStop;
        envelope.payload = HookPayload::SubagentStop {
            phase: crate::event::SubagentStopPhase::Gate,
            subagent_id: "sub-1".into(),
            subagent_type: "explore".into(),
            stop_hook_active: Some(false),
            last_assistant_message: None,
        };
        let result = dispatch_stop(
            &registry,
            HookEventName::SubagentStop,
            &envelope,
            &run_ctx(),
        )
        .await;
        assert_eq!(result.blocks.len(), 1, "only the matching spec runs");
        assert_eq!(result.blocks[0].reason, "from explorer");
    }

    #[tokio::test]
    async fn non_blocking_empty_registry() {
        let registry = registry_from_specs(vec![]);
        let envelope = session_start_envelope();
        let results = dispatch_non_blocking(
            &registry,
            HookEventName::SessionStart,
            &envelope,
            &run_ctx(),
        )
        .await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn non_blocking_disabled_hook_skipped() {
        let mut spec = make_command_spec("disabled", None, false, "echo ok");
        spec.event = HookEventName::SessionStart;
        let registry = registry_from_specs(vec![spec]);
        let envelope = session_start_envelope();
        let results = dispatch_non_blocking(
            &registry,
            HookEventName::SessionStart,
            &envelope,
            &run_ctx(),
        )
        .await;
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0], HookRunResult::Skipped { .. }));
    }

    #[tokio::test]
    async fn non_blocking_failure_does_not_stop_chain() {
        let mut spec1 = make_command_spec("crasher", None, true, "exit 1");
        spec1.event = HookEventName::SessionStart;
        let mut spec2 = make_command_spec("ok", None, true, "echo ok");
        spec2.event = HookEventName::SessionStart;
        let registry = registry_from_specs(vec![spec1, spec2]);
        let envelope = session_start_envelope();
        let results = dispatch_non_blocking(
            &registry,
            HookEventName::SessionStart,
            &envelope,
            &run_ctx(),
        )
        .await;
        assert_eq!(results.len(), 2);
        assert!(matches!(results[0], HookRunResult::Failed { .. }));
        assert!(matches!(results[1], HookRunResult::Success { .. }));
    }
}

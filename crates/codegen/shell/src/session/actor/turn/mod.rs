//! Turn-execution concern for `SessionActor` (`handle_prompt`, turn-end,
//! sampling loop).
use super::*;
mod sampling;
use sampling::*;
mod settlement;
use crate::session::behavior::BehaviorChangeOutcome;
use settlement::*;
mod admission;
pub(in crate::session::actor) use admission::should_capture_implicit_goal_objective;
#[cfg(test)]
use admission::{UserEchoMode, user_echo_mode};

/// Synthetic tool the model calls to return its schema-constrained final answer
/// on backends that can't constrain output natively (Messages API). Intercepted
/// in the loop, never executed as a real tool.
const STRUCTURED_OUTPUT_TOOL: &str = "StructuredOutput";
/// Max times the model may re-call `StructuredOutput` with non-conforming args
/// before the turn ends with the last validation error.
const STRUCTURED_OUTPUT_MAX_RETRIES: u32 = 3;
/// What a `StructuredOutput` tool call means for the turn (see
/// `handle_structured_output_tool_call`).
enum StructuredOutputStep {
    /// Accepted, or retries exhausted: the carried result is the final output.
    Complete(Result<serde_json::Value, String>),
    /// Non-conforming args; a corrective tool_result was pushed — re-sample.
    Retry,
    /// No sole StructuredOutput call (absent, or co-emitted with real tools that
    /// should run this round).
    Proceed,
}
/// Parse `raw` as JSON and validate it against a `validator` compiled once per
/// turn. Returns the value on success, or a human-readable error (surfaced to
/// the model on retry and to the client as `structuredOutputError`). A `validator`
/// of `Err` means the user's schema itself was invalid.
fn validate_structured_output(
    validator: &Result<sampling_types::OutputSchemaValidator, String>,
    raw: &str,
) -> Result<serde_json::Value, String> {
    let validator = validator.as_ref().map_err(Clone::clone)?;
    let value: serde_json::Value = serde_json::from_str(raw.trim())
        .map_err(|e| format!("model output was not valid JSON: {e}"))?;
    sampling_types::validate_output_value(validator, &value).map(|()| value)
}
/// Result of the turn-end usage drain (and cancel's no-drain snapshot).
///
/// **Ledger marks** only when [`Self::fail_closed`]. Sticky and background
/// live are **report-level only** (tokens still land on the session ledger).
pub(super) struct UsageDrainOutcome {
    /// Query failure, FG still live after timeout/cancel. Marks both
    /// the prompt and session bills incomplete. (True apply-miss stains
    /// ledgers at fold time via `mark_apply_miss_incomplete`, not here.)
    pub(super) fail_closed: bool,
    /// A background child is still running: only this prompt's report is
    /// incomplete; its spend reaches the session ledger at completion.
    pub(super) background_live: bool,
    /// Pin-scoped sticky (session-only attribution or apply-miss report).
    /// Report incomplete only — does not stain ledgers by itself.
    pub(super) sticky_report: bool,
}
impl UsageDrainOutcome {
    /// Wire / attach incomplete: fail-closed ∪ background ∪ sticky.
    pub(super) fn report_incomplete(&self) -> bool {
        self.fail_closed || self.background_live || self.sticky_report
    }
    /// Map an outstanding reply without a multi-second drain (cancel path).
    /// Same policy as freeze's terminal outcome: FG live → fail-closed;
    /// sticky and background → report only.
    pub(super) fn from_outstanding_reply(
        reply: Option<&tools::implementations::grow_build::task::types::SubagentOutstandingReply>,
    ) -> Self {
        match reply {
            None => Self {
                fail_closed: true,
                background_live: false,
                sticky_report: false,
            },
            Some(r) => Self {
                fail_closed: !r.live_ids.is_empty(),
                background_live: r.background_live,
                sticky_report: r.subagent_usage_not_applied,
            },
        }
    }
}
/// Accumulates a turn's per-call token usage and tool-call presence across the
/// agentic loop's model calls, recording running totals on the turn span. Kept
/// out of the loop body so diagnostics bookkeeping doesn't obscure control flow.
#[derive(Default)]
struct TurnSpanTotals {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    has_tool_call: bool,
}
impl TurnSpanTotals {
    /// Fold one model response into the totals (tokens sum — each call is billed
    /// its full prompt; has_tool_call OR-s — the final call has none) and update
    /// the span. `stop_reason` is last-wins (the terminal reason), not summed.
    fn record(&mut self, span: &tracing::Span, response: &ConversationResponse) {
        if let Some(u) = response.usage.as_ref() {
            self.input_tokens += i64::from(u.prompt_tokens);
            self.output_tokens += i64::from(u.completion_tokens);
            self.cache_read_tokens += i64::from(u.cached_prompt_tokens);
            span.record("input_tokens", self.input_tokens);
            span.record("output_tokens", self.output_tokens);
            span.record("cache_read_tokens", self.cache_read_tokens);
        }
        if let Some(sr) = response.stop_reason {
            span.record("stop_reason", sr.as_str());
        }
        self.has_tool_call |= !response.tool_calls().is_empty();
        span.record("response.has_tool_call", self.has_tool_call);
    }
}
/// How the turn's per-block user-message echo is published to clients /
/// `updates.jsonl`.
///
/// Every turn consumes a `prompt_index`. Timeline owns rewind boundaries;
/// `updates.jsonl` retains every echo only for client replay. Turns whose
/// content must not render as a user prompt
/// (notification drain) are hidden by the *pager* via the
/// `hideFromScrollback` chunk meta, not by omitting the persisted line.
impl SessionActor {
    /// Wait for turn-blocking subagents (up to 120s on the turn task),
    /// snapshot, clear sticky. Background children never gate the drain: the
    /// prompt report is marked incomplete immediately and their spend reaches
    /// the session ledger when they finish.
    /// Cancel intentionally skips this multi-second drain (actor-loop safety).
    pub(super) async fn freeze_prompt_usage(
        &self,
        prompt_id: &str,
    ) -> Option<crate::extensions::notification::PromptUsage> {
        const DRAIN: std::time::Duration = std::time::Duration::from_secs(120);
        self.freeze_prompt_usage_bounded(prompt_id, DRAIN).await
    }
    /// [`freeze_prompt_usage`] with an explicit drain bound, for tests.
    pub(super) async fn freeze_prompt_usage_bounded(
        &self,
        prompt_id: &str,
        max_wait: std::time::Duration,
    ) -> Option<crate::extensions::notification::PromptUsage> {
        let drain = self
            .drain_subagent_usage_for_prompt_bounded(prompt_id, max_wait)
            .await;
        self.finalize_usage_from_outcome(prompt_id, drain).await
    }
    /// Waits for turn-blocking folds only.
    /// `fail_closed` on timeout or query failure; sticky and `background_live`
    /// are report-level only (no ledger mark). Must run on the turn task (not
    /// the session actor loop) so folds can land.
    pub(super) async fn drain_subagent_usage_for_prompt_bounded(
        &self,
        prompt_id: &str,
        max_wait: std::time::Duration,
    ) -> UsageDrainOutcome {
        const POLL: std::time::Duration = std::time::Duration::from_millis(50);
        let deadline = std::time::Instant::now() + max_wait;
        loop {
            let reply = self.outstanding_reply_for_prompt(prompt_id).await;
            match reply.as_ref() {
                None => {
                    tracing::warn!(
                        prompt_id,
                        "outstanding subagent query failed; treating usage as incomplete"
                    );
                    return UsageDrainOutcome {
                        fail_closed: true,
                        background_live: false,
                        sticky_report: false,
                    };
                }
                Some(r) if r.live_ids.is_empty() => {
                    return UsageDrainOutcome {
                        fail_closed: false,
                        background_live: r.background_live,
                        sticky_report: r.subagent_usage_not_applied,
                    };
                }
                Some(r) => {
                    if std::time::Instant::now() >= deadline {
                        tracing::warn!(
                            prompt_id,
                            count = r.live_ids.len(),
                            max_wait_ms = max_wait.as_millis() as u64,
                            "subagent usage drain timed out; usage may under-count"
                        );
                        return UsageDrainOutcome {
                            fail_closed: true,
                            background_live: r.background_live,
                            sticky_report: r.subagent_usage_not_applied,
                        };
                    }
                }
            }
            tokio::time::sleep(POLL).await;
        }
    }
    pub(super) async fn snapshot_prompt_usage(
        &self,
    ) -> Option<crate::extensions::notification::PromptUsage> {
        self.snapshot_prompt_usage_marked(false).await
    }
    pub(super) async fn snapshot_prompt_usage_marked(
        &self,
        incomplete: bool,
    ) -> Option<crate::extensions::notification::PromptUsage> {
        let actor_background_spend = self
            .unattributed_background_usage
            .swap(false, std::sync::atomic::Ordering::Relaxed);
        let shared_background_spend = self
            .tool_context
            .unattributed_background_usage
            .swap(false, std::sync::atomic::Ordering::Relaxed);
        let incomplete = incomplete || actor_background_spend || shared_background_spend;
        match self.chat_state_handle.try_get_prompt_usage().await {
            Ok(ledger) => {
                let incomplete = incomplete || ledger.as_ref().is_some_and(|l| l.incomplete);
                crate::extensions::notification::PromptUsage::project_from_ledger(
                    ledger.as_ref(),
                    incomplete,
                )
            }
            Err(()) => {
                crate::extensions::notification::PromptUsage::project_from_ledger(None, true)
            }
        }
    }
    /// When freeze did not attach: incomplete if billed or may under-count; else omit.
    pub(super) async fn error_path_usage_fallback(
        &self,
        prompt_id: &str,
    ) -> Option<crate::extensions::notification::PromptUsage> {
        let may_undercount = Self::usage_incomplete_from_reply(
            self.outstanding_reply_for_prompt(prompt_id).await.as_ref(),
        );
        match self.chat_state_handle.try_get_prompt_usage().await {
            Ok(ledger) => crate::extensions::notification::PromptUsage::for_error_path(
                ledger.as_ref(),
                may_undercount,
            ),
            Err(()) => crate::extensions::notification::PromptUsage::for_error_path(None, true),
        }
    }
    /// Sticky incomplete for `prompt_id`, or the live pin when `None`.
    /// Returns true only if the coordinator acked the mark.
    pub(super) async fn mark_subagent_usage_not_applied(&self, prompt_id: Option<&str>) -> bool {
        let resolved = prompt_id
            .map(str::to_owned)
            .or_else(|| self.current_prompt_id.lock().ok().and_then(|g| g.clone()));
        let Some(pid) = resolved else {
            self.unattributed_background_usage
                .store(true, std::sync::atomic::Ordering::Relaxed);
            self.tool_context
                .unattributed_background_usage
                .store(true, std::sync::atomic::Ordering::Relaxed);
            return false;
        };
        let Some(tx) = &self.tool_context.subagent_event_tx else {
            return false;
        };
        use tools::implementations::grow_build::task::types::{
            SubagentEvent, SubagentMarkUsageNotAppliedRequest,
        };
        let (respond_to, ack) = tokio::sync::oneshot::channel();
        if tx
            .send(SubagentEvent::MarkUsageNotApplied(
                SubagentMarkUsageNotAppliedRequest {
                    parent_session_id: self.session_id_string(),
                    prompt_id: pid,
                    respond_to,
                },
            ))
            .is_err()
        {
            return false;
        }
        ack.await.is_ok()
    }
    /// Account Goal state at the regular turn boundary and wake the single
    /// idle arbiter. Stage scheduling never happens inline with completion.
    pub(crate) async fn handle_turn_end(
        self: &std::sync::Arc<Self>,
        prompt_id: &str,
        suppress_goal_continuation: bool,
    ) {
        let goal_active_now = laziness_injection_active(
            self.goal_runtime_available(),
            self.goal_tracker.lock().status(),
        );
        if !goal_active_now {
            return;
        }
        let _ = self
            .enforce_goal_spending_limit_for_prompt(Some(prompt_id))
            .await;
        if !suppress_goal_continuation {
            self.idle_arbiter.notify_one();
        }
    }

    /// Start a successor Step after its predecessor's boundary has already
    /// been closed and all eligible controls have settled. Steering that races
    /// final response settlement and completion recovery share this exact
    /// admission path, so neither can sample without a causal `StepStarted`.
    async fn start_step_after_control_boundary(
        &self,
        prompt_id: &str,
        injected_context: Option<ConversationItem>,
    ) -> bool {
        let _boundary = self.step_control_gate.lock().await;
        self.refresh_goal_step_resources().await;
        let admission = self.state.lock().await;
        if !admission.can_continue_regular_turn(prompt_id) || self.events.has_active_step() {
            return false;
        }
        if let Some(item) = injected_context {
            self.chat_state_handle.push_user_message(item);
        }
        self.emit_event(crate::session::events::Event::LoopStarted {
            loop_index: self.events.next_step_index(),
        });
        true
    }

    /// Wraps `process_conversation_turn` with auto-recovery for agents that opt in.
    ///
    /// Agents with a `completion_requirement` in their definition require the model
    /// to call a specific tool before finishing. If a prompt turn ends without that
    /// tool having been called, this method injects the recovery prompt and re-runs
    /// the turn with exponential backoff.
    ///
    /// Agents without `completion_requirement` bypass this entirely.
    #[tracing::instrument(
        name = "session.process_conversation_turn_with_recovery",
        skip_all,
        err,
        fields(req_id = %req_id, session_id = %self.session_info.id.0)
    )]
    pub(super) async fn process_conversation_turn_with_recovery(
        self: &Arc<Self>,
        req_id: &str,
        origin: super::super::PromptOrigin,
        json_schema: Option<serde_json::Value>,
        step_already_started: bool,
    ) -> Result<TurnOutcome, acp::Error> {
        let mut result = self
            .process_conversation_turn(req_id, &origin, json_schema.clone(), step_already_started)
            .await;
        let mut attempt = 0u32;
        let mut recovery_key = None;
        let mut delay_satisfied_for = None;
        let mut result_precedes_current_agent = false;
        loop {
            let outcome = match &result {
                Ok(TurnOutcome::Completed { .. }) => "completed",
                Ok(TurnOutcome::ControlEnded { .. }) => "control_boundary",
                Ok(TurnOutcome::GoalSpendingStopped { .. }) => "goal_spending_stopped",
                Ok(TurnOutcome::Cancelled { .. }) => "cancelled",
                Ok(TurnOutcome::MaxTurnsReached { .. }) => "max_turns",
                Ok(TurnOutcome::StationarityEnded { .. }) => "stationarity",
                Err(_) => "error",
            };
            let (agent_changed, behavior_changed) =
                if let Some(boundary) = self.end_step_control_boundary(outcome).await {
                    let (_, agent_changed, behavior_changed) =
                        self.apply_pending_controls_at_step_boundary(boundary).await;
                    (agent_changed, behavior_changed)
                } else {
                    // `process_conversation_turn` may already have closed its
                    // final Step for a control/budget boundary. Never consume
                    // controls admitted after that horizon without another
                    // StepEnded fact.
                    (false, false)
                };
            result_precedes_current_agent |= agent_changed;
            // Completion recovery is a new model step just as surely as the
            // ordinary tool loop is.  Usage from the response above may have
            // exhausted the active Goal, so fence recovery here after
            // StepEnded and before its reminder / StepStarted pair.  Without
            // this edge an Agent completion requirement could spend at least
            // one provider call beyond the Goal budget.
            if self
                .enforce_goal_spending_limit_for_prompt(Some(req_id))
                .await
            {
                let snapshot = match &result {
                    Ok(TurnOutcome::Completed { snapshot, .. }) => snapshot.clone(),
                    _ => Box::new(None),
                };
                return Ok(TurnOutcome::GoalSpendingStopped { snapshot });
            }
            if behavior_changed {
                let snapshot = match result {
                    Ok(TurnOutcome::Completed { snapshot, .. })
                    | Ok(TurnOutcome::ControlEnded { snapshot })
                    | Ok(TurnOutcome::GoalSpendingStopped { snapshot })
                    | Ok(TurnOutcome::StationarityEnded { snapshot }) => snapshot,
                    Ok(TurnOutcome::Cancelled { .. })
                    | Ok(TurnOutcome::MaxTurnsReached { .. })
                    | Err(_) => Box::new(None),
                };
                // The terminal/error belonged to the Behavior admitted for
                // the old turn. Once a new Behavior is durably accepted at
                // StepEnded, it must not inherit that result (most notably an
                // old Normal error must not auto-pause a newly reactivated
                // Goal during settlement).
                return Ok(TurnOutcome::ControlEnded { snapshot });
            }
            if matches!(
                result,
                Ok(TurnOutcome::ControlEnded { .. })
                    | Ok(TurnOutcome::GoalSpendingStopped { .. })
                    | Ok(TurnOutcome::Cancelled { .. })
                    | Ok(TurnOutcome::MaxTurnsReached { .. })
                    | Ok(TurnOutcome::StationarityEnded { .. })
            ) {
                return result;
            }
            if matches!(result, Ok(TurnOutcome::Completed { .. }))
                && self.close_steering_and_drain(req_id).await
            {
                tracing::info!(
                    "Drained steering that raced final response settlement — continuing"
                );
                attempt = 0;
                recovery_key = None;
                delay_satisfied_for = None;
                result_precedes_current_agent = false;
                if !self.start_step_after_control_boundary(req_id, None).await {
                    return result;
                }
                result = self
                    .process_conversation_turn(req_id, &origin, json_schema.clone(), true)
                    .await;
                continue;
            }
            if matches!(result, Ok(TurnOutcome::Completed { .. })) && result_precedes_current_agent
            {
                // The response was sampled under the previous Agent epoch.
                // Applying a queued AgentRole at StepEnded cannot retroactively
                // turn that successful response into a failure of the new
                // Agent's completion contract. The new contract starts with
                // the first response actually sampled under that Agent.
                return result;
            }
            let requirement = {
                let agent = self.agent.borrow();
                agent
                    .completion_requirement()
                    .cloned()
                    .map(|requirement| (agent.definition().selector_identity(), requirement))
            };
            let Some((agent_name, requirement)) = requirement else {
                return result;
            };
            let Some(recovery) = requirement.recovery.clone() else {
                return result;
            };
            if let Ok(TurnOutcome::Completed {
                ref tools_called, ..
            }) = result
                && !result_precedes_current_agent
                && tools_called.iter().any(|name| name == &requirement.tool)
            {
                tracing::info!(
                    agent = %agent_name,
                    tool = %requirement.tool,
                    attempts = attempt,
                    "completion requirement satisfied"
                );
                return result;
            }
            // Completion recovery keeps the same foreground turn alive. Reopen
            // steering before backoff so user input can preempt the retry.
            self.reopen_steering(req_id).await;
            let next_key = (
                agent_name.clone(),
                requirement.tool.clone(),
                requirement.reminder.clone(),
                recovery.max_retries,
                recovery.base_delay_ms,
                recovery.max_delay_ms,
            );
            if recovery_key.as_ref() != Some(&next_key) {
                recovery_key = Some(next_key.clone());
                attempt = 0;
                delay_satisfied_for = None;
            }
            let error_desc = match &result {
                Ok(_) => "Agent finished without completing required task".into(),
                Err(e) => format!("{e:?}"),
            };
            if delay_satisfied_for.as_ref() != Some(&next_key) {
                attempt += 1;
                if attempt > recovery.max_retries {
                    tracing::error!(
                        "Auto-recovery exhausted after {attempt} attempts for session {}: {error_desc}",
                        self.session_info.id.0,
                    );
                    self.send_grow_notification(GrowSessionUpdate::RetryState(
                        crate::extensions::notification::RetryState::Exhausted {
                            attempts: attempt,
                            reason: error_desc,
                            is_rate_limited: false,
                        },
                    ))
                    .await;
                    return result;
                }
                let delay_ms = std::cmp::min(
                    recovery.base_delay_ms * 2u64.pow(attempt.saturating_sub(1)),
                    recovery.max_delay_ms,
                );
                let delay = std::time::Duration::from_millis(delay_ms);
                tracing::warn!(
                    agent = %agent_name,
                    "Auto-recovery attempt {}/{} for session {}: {error_desc}. Retrying in {}ms",
                    attempt,
                    recovery.max_retries,
                    self.session_info.id.0,
                    delay.as_millis(),
                );
                self.send_grow_notification(GrowSessionUpdate::RetryState(
                    crate::extensions::notification::RetryState::Retrying {
                        attempt,
                        max_retries: recovery.max_retries,
                        reason: error_desc,
                    },
                ))
                .await;
                sleep(delay).await;
                delay_satisfied_for = Some(next_key);
                continue;
            }

            // The preceding Step boundary already froze its admission horizon.
            // Controls accepted afterwards intentionally stay pending for the
            // recovery Step starting here.
            let recovery_step_started = self
                .start_step_after_control_boundary(
                    req_id,
                    Some(ConversationItem::auto_recovery(
                        requirement.reminder.clone(),
                    )),
                )
                .await;
            if !recovery_step_started {
                return result;
            }
            result = self
                .process_conversation_turn(req_id, &origin, None, true)
                .await;
            delay_satisfied_for = None;
            result_precedes_current_agent = false;
        }
    }
    /// Compute the first-turn memory reminder, if one should be injected.
    ///
    /// A block persisted by an earlier session segment (a prior `--resume`
    /// process, or a turn before a compaction) is reused verbatim — see
    /// [`conversation_has_memory_context`] for why re-searching is harmful.
    ///
    /// [`conversation_has_memory_context`]: crate::session::helpers::memory_context::conversation_has_memory_context
    pub(crate) async fn first_turn_memory_reminder(&self) -> Option<String> {
        if self
            .memory
            .context_injected
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return None;
        }
        self.memory
            .context_injected
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if !self.memory.initial_injection_config.enabled {
            tracing::info!(
                target: ::diagnostics::memory_log::TARGET,
                "MEMORY_INJECT: first-turn injection disabled by config"
            );
            return None;
        }
        let (Some(storage), Some(params)) =
            (self.memory.storage(), self.memory.backend_params.as_ref())
        else {
            return None;
        };
        let conversation = self.chat_state_handle.get_conversation().await;
        if crate::session::helpers::memory_context::conversation_has_memory_context(&conversation) {
            tracing::info!(
                target: ::diagnostics::memory_log::TARGET,
                "MEMORY_INJECT: typed memory-context item already present -- skipping re-injection"
            );
            return None;
        }
        use tools::types::memory_backend::MemoryBackend as _;
        let (injection_params, configured_min_score) =
            build_initial_injection_backend_params(params, &self.memory.initial_injection_config);
        let backend = memory::MemoryBackendImpl::from_session_params(storage, &injection_params);
        let raw_query =
            crate::session::helpers::session_compact::extract_last_real_user_query(&conversation)
                .unwrap_or_default();
        let was_greeting = raw_query.is_empty()
            || raw_query.len() < 20
            || crate::session::helpers::memory_context::is_greeting(&raw_query);
        let query = if was_greeting {
            "project conventions preferences architecture".to_string()
        } else {
            raw_query
        };
        let inject_start = std::time::Instant::now();
        let inject_results = backend.search(&query, 6, configured_min_score).await.ok();
        let result_count = inject_results.as_ref().map_or(0, |r| r.len());
        let top_score = inject_results
            .as_ref()
            .and_then(|r| r.first())
            .map_or(0.0, |r| r.score);
        let total_snippet_chars: usize = inject_results
            .as_ref()
            .map_or(0, |r| r.iter().map(|s| s.snippet.len()).sum());
        tracing::info!(
            target: ::diagnostics::memory_log::TARGET,
            configured_min_score,
            "MEMORY_INJECT_SEARCH: results={result_count}"
        );
        ::diagnostics::session_ctx::log_event(::diagnostics::memory_events::MemoryInjection {
            session_id: self.session_info.id.to_string(),
            was_greeting_fallback: was_greeting,
            result_count,
            total_snippet_chars,
            top_score,
            configured_min_score,
            injection_duration_ms: inject_start.elapsed().as_millis() as u64,
        });
        inject_results.and_then(|results| {
            crate::session::helpers::memory_context::format_memory_reminder(&results)
        })
    }
    /// Inspect `tool_calls` for a `StructuredOutput` call and decide the turn's
    /// next step, pushing the call's `tool_result` (correction / retry error /
    /// terminal) as a side effect. Validates the args against `validator` and
    /// bumps `retries` on a non-conforming retry.
    async fn handle_structured_output_tool_call(
        &self,
        tool_calls: &mut Vec<sampling_types::conversation::ToolCall>,
        validator: &Result<sampling_types::OutputSchemaValidator, String>,
        retries: &mut u32,
    ) -> StructuredOutputStep {
        let Some(pos) = tool_calls
            .iter()
            .position(|tc| tc.name == STRUCTURED_OUTPUT_TOOL)
        else {
            return StructuredOutputStep::Proceed;
        };
        if tool_calls.len() > 1 {
            for tc in tool_calls
                .iter()
                .filter(|tc| tc.name == STRUCTURED_OUTPUT_TOOL)
            {
                self.chat_state_handle
                    .push_tool_result(ConversationItem::tool_result(
                        tc.id.as_ref().to_owned(),
                        "Call StructuredOutput alone, exactly once, after all other tools finish.",
                    ));
            }
            tool_calls.retain(|tc| tc.name != STRUCTURED_OUTPUT_TOOL);
            return StructuredOutputStep::Proceed;
        }
        let call_id = tool_calls[pos].id.as_ref().to_owned();
        let validated = validate_structured_output(validator, &tool_calls[pos].arguments);
        if let Err(err) = &validated
            && *retries < STRUCTURED_OUTPUT_MAX_RETRIES
        {
            *retries += 1;
            self.chat_state_handle
                .push_tool_result(ConversationItem::tool_result(
                    call_id,
                    format!("{err}\nFix the arguments and call StructuredOutput again."),
                ));
            return StructuredOutputStep::Retry;
        }
        self.chat_state_handle
            .push_tool_result(ConversationItem::tool_result(
                call_id,
                match &validated {
                    Ok(_) => "Structured output accepted.".to_string(),
                    Err(err) => err.clone(),
                },
            ));
        StructuredOutputStep::Complete(validated)
    }
    /// Single shell tool call whose parsed command is `true` (via ToolBridge).
    async fn is_run_true_step(
        &self,
        tool_calls: &[sampling_types::conversation::ToolCall],
    ) -> bool {
        let [tc] = tool_calls else {
            return false;
        };
        let Ok(args) = serde_json::from_str::<serde_json::Value>(tc.arguments.as_ref()) else {
            return false;
        };
        let Ok(input) = self.tool_bridge_handle().try_parse(&tc.name, args).await else {
            return false;
        };
        match input {
            ToolInput::Bash(b) => command_is_true(&b.command),
            _ => false,
        }
    }
    /// Shared turn-completion bookkeeping (plan cleanup, local signals snapshot,
    /// persistence, feedback prompt). Runs identically for
    /// the native and StructuredOutput-tool completion paths. Returns the
    /// turn-end snapshot for `TurnOutcome::Completed`.
    async fn finalize_turn_bookkeeping(
        &self,
        req_id: &str,
        conv_turn_start: std::time::Instant,
        turn_span_totals: &TurnSpanTotals,
        model_fingerprint: Option<String>,
    ) -> Option<TurnDeltaSnapshot> {
        self.emit_turn_end_plan_cleanup().await;
        self.signals_handle().record_turn_complete();
        let mut snapshot = self.signals_handle().take_turn_end_snapshot().await;
        if let Some(snap) = snapshot.as_mut() {
            self.apply_behavior_to_snapshot(snap);
            snap.turn_input_tokens = turn_span_totals.input_tokens.max(0) as u64;
            snap.turn_output_tokens = turn_span_totals.output_tokens.max(0) as u64;
            snap.turn_cached_input_tokens = turn_span_totals.cache_read_tokens.max(0) as u64;
            for pr in &snap.delta.prs_created_this_turn {
                ::diagnostics::session_ctx::log_event(::diagnostics::events::PrCreated {
                    source: pr.source,
                    had_commit_in_session: pr.had_commit_in_session,
                });
            }
        }
        if let Some(snap) = snapshot.as_ref() {
            match snap.current.timeline_kind() {
                Ok(kind) => self.chat_state_handle.record_timeline_event(kind),
                Err(error) => tracing::error!(%error, "failed to encode session signals event"),
            }
        }
        snapshot
    }
    #[tracing::instrument(
        name = "session.process_conversation_turn",
        skip_all,
        err,
        fields(
            session_id = %self.session_info.id.0,
            model_id,
            turn_tool_count,
            turn_model_calls,
            input_tokens = tracing::field::Empty,
            output_tokens = tracing::field::Empty,
            cache_read_tokens = tracing::field::Empty,
            stop_reason = tracing::field::Empty,
            response.has_tool_call = tracing::field::Empty,
            request_id = tracing::field::Empty,
            ttft_ms = tracing::field::Empty,
            mcp_server.name = tracing::field::Empty,
            mcp_tool.name = tracing::field::Empty,
            agent.name = tracing::field::Empty,
            skill.name = tracing::field::Empty,
            query_source = tracing::field::Empty,
            effort = tracing::field::Empty,
            attempt = tracing::field::Empty,
            parent_agent_id = tracing::field::Empty,
        )
    )]
    async fn process_conversation_turn(
        self: &Arc<Self>,
        req_id: &str,
        origin: &super::super::PromptOrigin,
        json_schema: Option<serde_json::Value>,
        mut step_already_started: bool,
    ) -> Result<TurnOutcome, acp::Error> {
        let conv_turn_start = std::time::Instant::now();
        self.repair_missing_control_contexts_durably()
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "active Control context could not be restored before sampling: {error}"
                ))
            })?;
        if self.state.lock().await.terminal_preemption_pending {
            return Ok(TurnOutcome::Cancelled {
                category: None,
                context: Some(serde_json::json!({
                    "reason": "control boundary preempted provider admission"
                })),
            });
        }
        self.maybe_compact_on_model_switch().await?;
        self.chat_state_handle
            .record_turn_start(chrono::Utc::now().timestamp_millis());
        {
            let span = tracing::Span::current();
            span.record("agent.name", self.agent.borrow().name());
            if let Some(skill) = self.active_skill.lock().clone() {
                span.record("skill.name", skill.as_str());
            }
            span.record(
                "query_source",
                if self.startup_hints.is_subagent {
                    "subagent"
                } else {
                    "main"
                },
            );
            if let Some(parent) = self.startup_hints.parent_session_id.as_deref() {
                span.record("parent_agent_id", parent);
            }
        }
        if let Some(cfg) = self.chat_state_handle.get_sampling_config().await {
            let span = tracing::Span::current();
            span.record("model_id", self.current_catalog_model_id());
            span.record(
                "effort",
                cfg.reasoning_effort
                    .map(|effort| effort.as_str())
                    .unwrap_or("none"),
            );
        }
        let mut prompt_timing = Some(crate::session::prompt_timing::PromptTiming::start());
        let tool_prep_start = std::time::Instant::now();
        let (mut tool_definitions, mcp_wait_ms) = self.prepare_tool_definitions_timed().await;
        let total_prep_ms = tool_prep_start.elapsed().as_millis() as u64;
        if let Some(ref mut pt) = prompt_timing {
            pt.record_tool_prep(mcp_wait_ms, total_prep_ms);
        }
        ::diagnostics::unified_log::info(
            "shell.turn.tool_prep_done",
            Some(self.session_info.id.0.as_ref()),
            Some(serde_json::json!({
                "tool_count": tool_definitions.len(),
                "mcp_wait_ms": mcp_wait_ms,
                "total_prep_ms": total_prep_ms,
                "elapsed_since_turn_start_ms": conv_turn_start.elapsed().as_millis() as u64,
            })),
        );
        self.record_turn_model().await;
        let mut metrics_drop_guard = TurnMetrics::new();
        let mut turn_tools_called: Vec<String> = Vec::new();
        let mut tool_turn_count: usize = 1;
        let mut loop_index = if step_already_started {
            self.events.next_step_index().saturating_sub(1)
        } else {
            self.events.next_step_index()
        };
        let mut identical_tool_calls = IdenticalToolCallRun::default();
        let mut auth_retry_schedule = AuthRetrySchedule::new();
        let mut turn_span_totals = TurnSpanTotals::default();
        let mut model_fingerprint: Option<String> = None;
        let mut structured_output_retries: u32 = 0;
        let mut image_projection_retries: u8 = 0;
        let mut pending_forced_compaction: Option<compaction::AutoCompactTriggerInfo> = None;
        let mut plan_handoff_resample_pending = false;
        // True only between a provider overflow and the first successful
        // non-overflow response after its forced compaction. Long Goal turns
        // may legitimately compact again after later successful Steps grow
        // the context; only an immediate post-compaction overflow is terminal.
        let mut context_overflow_recovery_pending = false;
        let structured_output_validator = json_schema.as_ref().map(|schema| {
            sampling_types::compile_output_schema(schema)
                .map_err(|error| format!("invalid output schema: {error}"))
        });
        let schema_ok = matches!(structured_output_validator, Some(Ok(_)));
        // Once a turn has exposed the fallback tool, keep that protocol for
        // the rest of the turn. A later step may switch to a native-schema
        // backend, but the durable reminder already tells the model to call
        // StructuredOutput; removing the tool would make that context false.
        let mut structured_output_tool_mode = false;
        let mut structured_output_reminder_installed = false;
        loop {
            let using_prestarted_step = std::mem::take(&mut step_already_started);
            let mut agent_changed_for_step = false;
            if !using_prestarted_step {
                let control_boundary =
                    if self.events.next_step_index() == 0 && !self.events.has_active_step() {
                        self.initial_step_control_boundary(req_id).await
                    } else {
                        self.end_step_control_boundary("continued").await
                    };
                let Some(control_boundary) = control_boundary else {
                    return Ok(TurnOutcome::Cancelled {
                        category: None,
                        context: Some(serde_json::json!({
                            "reason": "the next Step could not establish its control boundary",
                        })),
                    });
                };
                let (model_changed, agent_changed, behavior_changed) = self
                    .apply_pending_controls_at_step_boundary(control_boundary)
                    .await;
                // Every accepted definition/route control is now durable
                // and live. Fence the new Goal budget before any
                // model-switch compaction, forced compaction, recovery, or
                // next-step provider request can spend under it.
                if self
                    .enforce_goal_spending_limit_for_prompt(Some(req_id))
                    .await
                {
                    let snapshot = self
                        .finalize_turn_bookkeeping(
                            req_id,
                            conv_turn_start,
                            &turn_span_totals,
                            model_fingerprint.clone(),
                        )
                        .await;
                    return Ok(TurnOutcome::GoalSpendingStopped {
                        snapshot: Box::new(snapshot),
                    });
                }
                if behavior_changed {
                    let snapshot = self
                        .finalize_turn_bookkeeping(
                            req_id,
                            conv_turn_start,
                            &turn_span_totals,
                            model_fingerprint.clone(),
                        )
                        .await;
                    return Ok(TurnOutcome::ControlEnded {
                        snapshot: Box::new(snapshot),
                    });
                }
                if plan_handoff_resample_pending {
                    self.consume_live_plan_handoff_for_next_step()
                        .await
                        .map_err(|error| {
                            acp::Error::internal_error().data(format!(
                                "failed to consume the live Plan handoff before resampling: {error}"
                            ))
                        })?;
                    plan_handoff_resample_pending = false;
                }
                if model_changed || agent_changed {
                    if let Some(stale) = pending_forced_compaction.take() {
                        tracing::info!(
                            source = stale.source,
                            "discarded forced compaction from the previous request projection epoch"
                        );
                    }
                    context_overflow_recovery_pending = false;
                }
                if model_changed {
                    // A smaller model window must compact before the first
                    // request that uses it, not at the next outer turn.
                    self.maybe_compact_on_model_switch().await?;
                    self.record_turn_model().await;
                    auth_retry_schedule.reset();
                    structured_output_retries = 0;
                    image_projection_retries = 0;
                    model_fingerprint = None;
                }
                if let Some(trigger_info) = pending_forced_compaction.take() {
                    if let Err(e) = self.run_compact_only(trigger_info).await {
                        tracing::error!(error = %e, "Between-step compaction failed");
                        if Self::is_auth_compact_error(&e) {
                            return Err(self.surface_compact_auth_failure(e).await);
                        }
                        return Err(e);
                    }
                }
                if model_changed || agent_changed {
                    // The request projection epoch changed. A provider
                    // overflow on the replacement route/Agent gets its own
                    // single recovery attempt.
                    identical_tool_calls = IdenticalToolCallRun::default();
                    let span = tracing::Span::current();
                    span.record("agent.name", self.agent.borrow().name());
                    if let Some(cfg) = self.chat_state_handle.get_sampling_config().await {
                        span.record("model_id", self.current_catalog_model_id());
                        span.record(
                            "effort",
                            cfg.reasoning_effort
                                .map(|effort| effort.as_str())
                                .unwrap_or("none"),
                        );
                    }
                }
                if agent_changed {
                    // Completion requirements belong to the Agent epoch. A
                    // similarly named tool called under the previous role
                    // cannot satisfy the replacement Agent's contract.
                    turn_tools_called.clear();
                    structured_output_retries = 0;
                    agent_changed_for_step = true;
                }
                if identical_tool_calls.run_len >= identical_tool_calls.hard_stop_threshold() {
                    let run_len = identical_tool_calls.run_len;
                    let tool_name = identical_tool_calls.tool_name.clone();
                    let true_noop = identical_tool_calls.is_true_noop_run;
                    tracing::warn!(
                        session_id = %self.session_info.id,
                        tool_name = %tool_name,
                        run_len,
                        true_noop,
                        "action stationarity: ending turn after repeated identical tool calls"
                    );
                    ::diagnostics::unified_log::warn(
                        "shell.turn.action_stationarity_stop",
                        Some(self.session_info.id.0.as_ref()),
                        Some(serde_json::json!({
                            "loop_index": loop_index,
                            "tool_name": tool_name,
                            "run_len": run_len,
                            "true_noop": true_noop,
                        })),
                    );
                    ::diagnostics::session_ctx::log_event(
                        ::diagnostics::events::ActionStationarityStop {
                            true_noop,
                            run_len,
                            tool_name: tool_name.clone(),
                        },
                    );
                    let snapshot = self
                        .finalize_turn_bookkeeping(
                            req_id,
                            conv_turn_start,
                            &turn_span_totals,
                            model_fingerprint.clone(),
                        )
                        .await;
                    return Ok(TurnOutcome::StationarityEnded {
                        snapshot: Box::new(snapshot),
                    });
                }
                let step_started = {
                    let _boundary = self.step_control_gate.lock().await;
                    self.refresh_goal_step_resources().await;
                    let admission = self.state.lock().await;
                    if !admission.can_continue_regular_turn(req_id) {
                        false
                    } else {
                        // The ended Step's immutable admission horizon was
                        // captured before controls were applied. Requests
                        // accepted afterwards remain pending for the Step
                        // starting here, even if they arrived before this
                        // append acquired the gate.
                        self.emit_event(crate::session::events::Event::LoopStarted { loop_index });
                        true
                    }
                };
                if !step_started {
                    return Ok(TurnOutcome::Cancelled {
                        category: None,
                        context: Some(serde_json::json!({
                            "reason": "foreground ownership ended at the step boundary",
                        })),
                    });
                }
            }
            loop_index += 1;
            if (!using_prestarted_step && loop_index > 1) || agent_changed_for_step {
                // Capability grants become visible at the next model sample, not
                // at the next outer user turn. Refresh only after the previous
                // tool batch has fully completed, so a model response cannot
                // request a capability and forge a newly exposed call in the
                // same batch.
                tool_definitions = self.prepare_tool_definitions_inner().await;
            }
            let native_backend = if json_schema.is_some() {
                match self.chat_state_handle.get_sampling_config().await {
                    Some(c) => c.api_backend.supports_native_schema(),
                    None => {
                        tracing::warn!(
                            "structured output: no sampling config; using StructuredOutput tool"
                        );
                        false
                    }
                }
            } else {
                false
            };
            structured_output_tool_mode |= schema_ok && !native_backend;
            let structured_output_tool = schema_ok && structured_output_tool_mode;
            let structured_output_native = schema_ok && native_backend && !structured_output_tool;
            if structured_output_tool && !structured_output_reminder_installed {
                self.push_system_reminder(
                    "A response schema is required. After any tool use, call the \
                     `StructuredOutput` tool exactly once with your final answer as its \
                     arguments; do not return the answer as text.",
                );
                structured_output_reminder_installed = true;
            }
            if identical_tool_calls.take_nudge() {
                let run_len = identical_tool_calls.run_len;
                let tool_name = identical_tool_calls.tool_name.clone();
                tracing::warn!(
                    session_id = %self.session_info.id,
                    tool_name = %tool_name,
                    run_len,
                    "action stationarity: nudging model to break repeated identical tool calls"
                );
                ::diagnostics::unified_log::warn(
                    "shell.turn.action_stationarity_nudge",
                    Some(self.session_info.id.0.as_ref()),
                    Some(serde_json::json!({
                        "loop_index": loop_index,
                        "tool_name": tool_name,
                        "run_len": run_len,
                    })),
                );
                let reminder = self
                    .tool_bridge_handle()
                    .render_prompt(
                        ACTION_STATIONARITY_NUDGE_TEMPLATE,
                        &serde_json::json!({
                            "tool_name": tool_name,
                            "run_len": run_len,
                        }),
                    )
                    .await
                    .unwrap_or_else(|| ACTION_STATIONARITY_NUDGE_TEMPLATE.to_string());
                self.push_system_reminder(&reminder);
            }
            self.drain_pending_interjections().await;
            self.drain_deferred_completions().await;
            self.flush_pending_system_reminders().await;
            self.drain_active_notifications().await;
            let memory_reminder = self.first_turn_memory_reminder().await;
            if memory_reminder.is_some() {
                self.memory
                    .injection_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tracing::info!(
                    target: ::diagnostics::memory_log::TARGET,
                    "MEMORY_INJECT: first-turn memory context injected"
                );
            }
            self.maybe_inject_mcp_reminder().await;
            if self.tool_context.task_output_token_budget.is_none() {
                self.refresh_byok_credential().await;
            }
            let projected_images = self.project_images_for_known_text_model().await?;
            if projected_images.total_images() > 0 {
                tracing::info!(
                    described_images = projected_images.described_images,
                    "installed irreversible model-facing ImageShadows"
                );
            }
            let mut effective_tools: Vec<ToolSpec> = self.turn_base_tool_specs(&tool_definitions);
            if structured_output_tool && let Some(schema) = json_schema.clone() {
                effective_tools.push(ToolSpec {
                    name: STRUCTURED_OUTPUT_TOOL.to_string(),
                    description: Some(
                        "Return your final answer as JSON matching the required schema. \
                         Call this exactly once, at the end."
                            .to_string(),
                    ),
                    parameters: schema,
                });
            }
            let active_goal = self.active_goal_directive_tag();
            let request_json_output = structured_output_native
                .then(|| {
                    json_schema
                        .clone()
                        .map(sampling_types::JsonOutputFormat::JsonSchema)
                })
                .flatten();
            let build_req_start = std::time::Instant::now();
            let mut request = self
                .chat_state_handle
                .build_request(
                    self.session_info.id.0.as_ref(),
                    effective_tools,
                    memory_reminder,
                    active_goal,
                    request_json_output,
                )
                .await
                .map_err(|error| {
                    acp::Error::internal_error()
                        .data(format!("failed to durably prepare model context: {error}"))
                })?;
            ::diagnostics::unified_log::debug(
                "shell.turn.build_request_done",
                Some(self.session_info.id.0.as_ref()),
                Some(serde_json::json!({
                    "build_request_ms": build_req_start.elapsed().as_millis() as u64,
                    "loop_index": loop_index,
                })),
            );
            if self.tool_context.task_output_token_budget.is_none()
                && let Some(trigger_info) = self.check_auto_compact_needed().await
            {
                // Compaction is a provider call of its own. Move it across the
                // StepEnded fence so response settlement, Goal limits and any
                // queued route/definition controls become authoritative before
                // the sideband is admitted.
                pending_forced_compaction = Some(trigger_info);
                continue;
            }
            if request.image_count() > 0
                && let Some(model) = self.unsupported_current_model_for_images().await
            {
                image_projection_retries = image_projection_retries.saturating_add(1);
                if image_projection_retries > 2 {
                    return Err(acp::Error::internal_error().data(format!(
                        "text-only model {model} still has unprojected image input after two Surface retries"
                    )));
                }
                tracing::info!(
                    model,
                    image_projection_retries,
                    "Surface changed after ImageShadow installation; rebuilding request projection"
                );
                continue;
            }
            image_projection_retries = 0;
            // Request assembly may itself spend Goal tokens (most notably
            // irreversible image-description and compaction Sidebands), and a
            // descendant can settle usage concurrently. Wait for every older
            // admitted provider attempt before rechecking at the final
            // provider-admission edge. Stop does not await this fence; only a
            // later model call does.
            let usage_owner = self.session_id_string();
            let usage_epoch = super::tasks_cancel::turn_usage_epoch_or(
                self.goal_usage_window.owner_epoch(&usage_owner),
            );
            self.goal_usage_window
                .wait_for_owner_settlements_through(&usage_owner, usage_epoch)
                .await;
            if self.goal_provider_admission_closed() {
                let snapshot = self
                    .finalize_turn_bookkeeping(
                        req_id,
                        conv_turn_start,
                        &turn_span_totals,
                        model_fingerprint.clone(),
                    )
                    .await;
                return Ok(TurnOutcome::GoalSpendingStopped {
                    snapshot: Box::new(snapshot),
                });
            }
            request.max_output_tokens = self
                .tool_context
                .clamp_task_model_request(request.max_output_tokens)
                .map_err(|message| acp::Error::internal_error().data(message))?;
            let provider_admitted = {
                let _boundary = self.step_control_gate.lock().await;
                let admission = self.state.lock().await;
                admission.can_continue_regular_turn(req_id) && self.events.has_active_step()
            };
            if !provider_admitted {
                return Ok(TurnOutcome::Cancelled {
                    category: None,
                    context: Some(serde_json::json!({
                        "reason": "provider admission closed before sampling",
                    })),
                });
            }
            ::diagnostics::unified_log::info(
                "shell.turn.inference_start",
                Some(self.session_info.id.0.as_ref()),
                Some(serde_json::json!({
                    "loop_index": loop_index,
                    "elapsed_since_turn_start_ms": conv_turn_start.elapsed().as_millis() as u64,
                })),
            );
            let model_timer = std::time::Instant::now();
            let (mut response, latency) = match self.run_turn_via_sampler(request.clone()).await {
                Ok(SamplerTurnOutcome::Response(r, latency)) => (r, latency),
                Err(error) => {
                    self.tool_context.fail_task_output_usage_closed();
                    return Err(error);
                }
                Ok(SamplerTurnOutcome::GoalSpendingStopped) => {
                    let snapshot = self
                        .finalize_turn_bookkeeping(
                            req_id,
                            conv_turn_start,
                            &turn_span_totals,
                            model_fingerprint.clone(),
                        )
                        .await;
                    return Ok(TurnOutcome::GoalSpendingStopped {
                        snapshot: Box::new(snapshot),
                    });
                }
                Ok(SamplerTurnOutcome::CompactAndResubmit(trigger_info)) => {
                    if context_overflow_recovery_pending {
                        return Err(self
                            .surface_repeated_context_overflow(
                                trigger_info.tokens_used,
                                trigger_info.context_window,
                            )
                            .await);
                    }
                    context_overflow_recovery_pending = true;
                    auth_retry_schedule.reset();
                    pending_forced_compaction = Some(trigger_info);
                    continue;
                }
                Ok(SamplerTurnOutcome::ImageInputUnsupportedAndResubmit) => {
                    // Irreversible ImageShadows changed the provider-facing
                    // projection, so a later overflow belongs to a new input
                    // epoch rather than the prior compaction attempt.
                    context_overflow_recovery_pending = false;
                    auth_retry_schedule.reset();
                    continue;
                }
                Ok(SamplerTurnOutcome::RefreshByokAndResubmit { credential }) => {
                    match auth_retry_schedule.on_recovered_401(credential) {
                        AuthRetryDecision::UnchargedResubmit { resubmit } => {
                            tracing::info!(
                                resubmit,
                                "BYOK 401 retry: request carried no credential; retrying without charging rejection budget"
                            );
                            sleep(std::time::Duration::from_millis(100)).await;
                            continue;
                        }
                        AuthRetryDecision::Backoff { attempt, delay } => {
                            let delay_ms = delay.as_millis() as u64;
                            ::diagnostics::unified_log::warn(
                                "shell.turn.byok_retry_backoff",
                                Some(self.session_info.id.0.as_ref()),
                                Some(serde_json::json!({
                                    "loop_index": loop_index,
                                    "attempt": attempt,
                                    "max_retries": AuthRetrySchedule::MAX_RETRIES,
                                    "delay_ms": delay_ms,
                                })),
                            );
                            self.send_grow_notification(GrowSessionUpdate::RetryState(
                                crate::extensions::notification::RetryState::Retrying {
                                    attempt,
                                    max_retries: AuthRetrySchedule::MAX_RETRIES,
                                    reason: "BYOK credential reloaded after 401; retrying request"
                                        .to_string(),
                                },
                            ))
                            .await;
                            sleep(delay).await;
                            continue;
                        }
                        AuthRetryDecision::Exhausted => {
                            let msg = format!(
                                "BYOK credential remained rejected after {} retries",
                                AuthRetrySchedule::MAX_RETRIES
                            );
                            return Err(acp::Error::internal_error().data(
                                crate::sampling::error::error_data_with_status(msg, Some(401)),
                            ));
                        }
                        AuthRetryDecision::RunawayGuard { resubmits } => {
                            let msg = format!(
                                "BYOK credential was still missing after {resubmits} resubmits"
                            );
                            return Err(acp::Error::internal_error().data(
                                crate::sampling::error::error_data_with_status(msg, Some(401)),
                            ));
                        }
                    }
                }
                Ok(SamplerTurnOutcome::Steered) => {
                    auth_retry_schedule.reset();
                    continue;
                }
            };
            auth_retry_schedule.reset();
            let model_elapsed_ms = model_timer.elapsed().as_millis() as u64;
            let usage = response.usage.as_ref();
            let prompt_tokens = usage.map(|u| u.prompt_tokens);
            let cached_prompt_tokens = usage.map(|u| u.cached_prompt_tokens);
            let completion_tokens = usage.map(|u| u.completion_tokens);
            let reasoning_tokens = usage.map(|u| u.reasoning_tokens);
            let ttft_ms = latency.time_to_first_token_ms;
            let tokens_per_sec = match completion_tokens {
                Some(ct) if ct > 0 => {
                    let decode_ms = match ttft_ms {
                        Some(ttft) if model_elapsed_ms > ttft => model_elapsed_ms - ttft,
                        _ => model_elapsed_ms,
                    };
                    (decode_ms > 0).then(|| {
                        let tps = f64::from(ct) * 1000.0 / decode_ms as f64;
                        (tps * 10.0).round() / 10.0
                    })
                }
                _ => None,
            };
            ::diagnostics::unified_log::info(
                "shell.turn.inference_done",
                Some(self.session_info.id.0.as_ref()),
                Some(serde_json::json!({
                    "loop_index": loop_index,
                    "model_elapsed_ms": model_elapsed_ms,
                    "elapsed_since_turn_start_ms": conv_turn_start.elapsed().as_millis() as u64,
                    "ttft_ms": ttft_ms,
                    "itl_p50_ms": latency.itl_p50_ms,
                    "attempts": latency.attempts,
                    "prompt_tokens": prompt_tokens,
                    "cached_prompt_tokens": cached_prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "reasoning_tokens": reasoning_tokens,
                    "tokens_per_sec": tokens_per_sec,
                })),
            );
            turn_span_totals.record(&tracing::Span::current(), &response);
            let _ = self.compaction.auto_compact_suppressed.compare_exchange(
                crate::session::compaction_config::SUPPRESS_UNTIL_SUCCESS,
                crate::session::compaction_config::SUPPRESS_NONE,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            );
            self.clear_auth_compact_suppression();
            let model_duration_ms = model_timer.elapsed().as_millis() as u64;
            {
                let model_id = self.current_catalog_model_id();
                ::diagnostics::session_ctx::log_event(
                    ::diagnostics::events::ModelResponseReceived {
                        model_id,
                        duration_ms: model_duration_ms,
                        stop_reason: response
                            .stop_reason
                            .as_ref()
                            .map(|r| format!("{r:?}").to_ascii_lowercase()),
                        prompt_tokens: response.usage.as_ref().map(|u| u.prompt_tokens),
                        completion_tokens: response.usage.as_ref().map(|u| u.completion_tokens),
                        reasoning_tokens: response.usage.as_ref().map(|u| u.reasoning_tokens),
                        cached_prompt_tokens: response
                            .usage
                            .as_ref()
                            .map(|u| u.cached_prompt_tokens),
                    },
                );
            }
            let response_completed = self.response_completed_update(&response);
            if let Some(pt) = prompt_timing.take() {
                let mcp_count = self.mcp_state.lock().await.configs.len() as u32;
                let mcp_tools = self
                    .agent
                    .borrow()
                    .tool_bridge()
                    .tool_definitions()
                    .await
                    .iter()
                    .filter(|t| t.function.name.contains("__"))
                    .count() as u32;
                let turn_index = self
                    .chat_state_handle
                    .get_prompt_index()
                    .await
                    .saturating_sub(1) as u32;
                pt.emit(
                    model_duration_ms,
                    turn_index,
                    mcp_count,
                    mcp_tools,
                    self.mcp.strategy,
                    self.current_catalog_model_id(),
                );
            }
            let mut tool_calls = response.tool_calls().to_vec();
            metrics_drop_guard.record_model_response(tool_calls.len());
            if let Some(fp) = response
                .assistant()
                .and_then(|a| a.model_fingerprint.clone())
            {
                model_fingerprint = Some(fp);
            }
            let fallback_text = response.fallback_text();
            let stop_reason = response.stop_reason;
            let response_is_empty = response.is_empty();
            let turn_refused = stop_reason == Some(sampling_types::StopReason::ContentFilter);
            let refusal_explanation = response.stop_message.clone();
            let final_answer_text = json_schema.is_some().then(|| response.assistant_text());
            let response_model_id = response.assistant().and_then(|item| item.model_id.clone());
            let persisted_items = response.items.len();
            let response_items = std::mem::take(&mut response.items);
            for item in response_items {
                match item {
                    sampling_types::ConversationItem::Assistant(_) => {
                        self.record_assistant_response(item).await;
                    }
                    _ => {
                        self.chat_state_handle.push_tool_result(item);
                    }
                }
            }
            // The response Surface facts must precede the provider anchor.
            // With usage, the anchor replaces their local estimates; without
            // usage, the estimates remain as fail-safe context pressure.
            self.record_response_token_usage(
                &response,
                Some(model_duration_ms),
                response_model_id,
                None,
            )
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!("Goal usage settlement failed: {error}"))
            })?;
            if response.usage.is_some() {
                self.send_available_commands_update().await;
            }
            if let Some(text) = fallback_text {
                tracing::warn!(
                    text_len = text.len(),
                    "emitting fallback AgentMessageChunk — no text chunks were streamed"
                );
                self.send_update(
                    acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                        acp::ContentBlock::Text(acp::TextContent::new(text)),
                    )),
                    None,
                )
                .await;
            }
            if turn_refused && response_is_empty {
                tracing::warn!(
                    has_explanation = refusal_explanation.is_some(),
                    "model response was a provider refusal — emitting UI-only notice"
                );
                self.send_lifecycle_notice(
                    "provider",
                    crate::extensions::notification::UiNoticeTone::Warning,
                    "The model provider refused to generate a response for this turn.",
                    Some(match refusal_explanation.as_deref() {
                        Some(explanation) => format!(
                            "Reason: content filter. Provider explanation: {explanation}\nRecovery: revise the prompt or switch to an appropriate model, then retry."
                        ),
                        None => "Reason: content filter.\nRecovery: revise the prompt or switch to an appropriate model, then retry."
                            .to_string(),
                    }),
                )
                .await;
            }
            // Truncation-recovery branches (Task 3): the items loop above has
            // already persisted the partial assistant content into chat state,
            // and the next loop iteration rebuilds the request from it
            // (`build_request` clones the full conversation). These branches
            // only pick the recovery strategy for the next sampling cycle. A
            // completed `tool_use` wins over any of these stop reasons — the
            // turn falls through to tool execution below.
            // Any accepted response other than a bare input-overflow proves
            // the preceding compaction converged for this request epoch. A
            // future overflow after more tools/output is a new recovery cycle.
            if !matches!(
                stop_reason,
                Some(sampling_types::StopReason::ModelContextWindowExceeded)
            ) || !tool_calls.is_empty()
            {
                context_overflow_recovery_pending = false;
            }
            match stop_reason {
                Some(sampling_types::StopReason::Length) if tool_calls.is_empty() => {
                    // max_tokens truncation: inject the continue prompt and
                    // resample. No count limit and no config toggle (user
                    // decision D2).
                    self.chat_state_handle.push_user_message(
                        sampling_types::ConversationItem::truncation_continue(
                            chat_state::compaction_utils::TRUNCATION_CONTINUE_PROMPT.to_string(),
                        ),
                    );
                    tracing::info!(
                        session_id = %self.session_info.id.0,
                        prompt_id = %req_id,
                        loop_index,
                        persisted_items,
                        "max_tokens truncation: partial response persisted, continue prompt injected, resampling"
                    );
                    ::diagnostics::unified_log::info(
                        "shell.turn.truncation_continue",
                        Some(self.session_info.id.0.as_ref()),
                        Some(serde_json::json!({
                            "prompt_id": req_id,
                            "loop_index": loop_index,
                            "persisted_items": persisted_items,
                        })),
                    );
                    continue;
                }
                Some(sampling_types::StopReason::ModelContextWindowExceeded)
                    if tool_calls.is_empty() =>
                {
                    // Input-side context exhaustion: the server reported the
                    // overflow, so compaction is triggered unconditionally
                    // (client-side estimation can under-count) — not gated on
                    // `check_auto_compact_needed()`.
                    let total_tokens = self.chat_state_handle.get_projected_tokens().await;
                    let context_window = self
                        .chat_state_handle
                        .get_sampling_config()
                        .await
                        .map(|cfg| cfg.context_window.get())
                        .unwrap_or(0);
                    let percentage =
                        token_estimation::usage_percentage_u8(total_tokens, context_window);
                    let trigger_info = compaction::AutoCompactTriggerInfo {
                        tokens_used: total_tokens,
                        context_window,
                        percentage,
                        source: "context_window_exceeded",
                    };
                    tracing::info!(
                        session_id = %self.session_info.id.0,
                        prompt_id = %req_id,
                        loop_index,
                        total_tokens,
                        context_window,
                        "model_context_window_exceeded: triggering compaction before resampling"
                    );
                    ::diagnostics::unified_log::info(
                        "shell.turn.context_window_exceeded_compact",
                        Some(self.session_info.id.0.as_ref()),
                        Some(serde_json::json!({
                            "prompt_id": req_id,
                            "loop_index": loop_index,
                            "tokens_used": total_tokens,
                            "context_window": context_window,
                            "percentage": percentage,
                        })),
                    );
                    if context_overflow_recovery_pending {
                        return Err(self
                            .surface_repeated_context_overflow(total_tokens, context_window)
                            .await);
                    }
                    context_overflow_recovery_pending = true;
                    pending_forced_compaction = Some(trigger_info);
                    continue;
                }
                Some(sampling_types::StopReason::PauseTurn) if tool_calls.is_empty() => {
                    // Anthropic server-tool iteration limit: resend the
                    // persisted assistant content as-is (no continue prompt —
                    // Anthropic's resend-to-continue semantics).
                    tracing::info!(
                        session_id = %self.session_info.id.0,
                        prompt_id = %req_id,
                        loop_index,
                        persisted_items,
                        "pause_turn: resending persisted assistant content to continue"
                    );
                    ::diagnostics::unified_log::info(
                        "shell.turn.pause_turn_resend",
                        Some(self.session_info.id.0.as_ref()),
                        Some(serde_json::json!({
                            "prompt_id": req_id,
                            "loop_index": loop_index,
                            "persisted_items": persisted_items,
                        })),
                    );
                    continue;
                }
                _ => {}
            }
            self.send_buffered_grow_update(response_completed).await;
            if tool_calls.is_empty() {
                if self.drain_pending_interjections().await
                    || self.drain_deferred_completions().await
                {
                    tracing::info!(
                        "Drained foreground event(s) before turn completion — continuing"
                    );
                    continue;
                }
                let snapshot = self
                    .finalize_turn_bookkeeping(
                        req_id,
                        conv_turn_start,
                        &turn_span_totals,
                        model_fingerprint.clone(),
                    )
                    .await;
                if self.drain_pending_interjections().await
                    || self.drain_deferred_completions().await
                {
                    tracing::info!(
                        "Drained late foreground event(s) during turn-end bookkeeping — continuing"
                    );
                    continue;
                }
                let structured_output = match (
                    structured_output_validator.as_ref(),
                    final_answer_text.as_ref(),
                ) {
                    (Some(validator), Some(text)) => {
                        Some(validate_structured_output(validator, text))
                    }
                    _ => None,
                };
                return Ok(TurnOutcome::Completed {
                    snapshot: Box::new(snapshot),
                    tools_called: turn_tools_called,
                    structured_output,
                    refusal: turn_refused.then(|| refusal_explanation.clone().unwrap_or_default()),
                });
            }
            if structured_output_tool && let Some(validator) = structured_output_validator.as_ref()
            {
                match self
                    .handle_structured_output_tool_call(
                        &mut tool_calls,
                        validator,
                        &mut structured_output_retries,
                    )
                    .await
                {
                    StructuredOutputStep::Complete(validated) => {
                        turn_tools_called.push(STRUCTURED_OUTPUT_TOOL.to_string());
                        let snapshot = self
                            .finalize_turn_bookkeeping(
                                req_id,
                                conv_turn_start,
                                &turn_span_totals,
                                model_fingerprint.clone(),
                            )
                            .await;
                        return Ok(TurnOutcome::Completed {
                            snapshot: Box::new(snapshot),
                            tools_called: turn_tools_called,
                            structured_output: Some(validated),
                            refusal: None,
                        });
                    }
                    StructuredOutputStep::Retry => continue,
                    StructuredOutputStep::Proceed => {}
                }
            }
            for tc in &tool_calls {
                if let Some((server, tool)) =
                    crate::session::mcp_servers::parse_mcp_tool_name(&tc.name)
                {
                    let span = tracing::Span::current();
                    span.record("mcp_server.name", server.as_str());
                    span.record("mcp_tool.name", tool.as_str());
                }
                turn_tools_called.push(tc.name.clone());
            }
            let step_signature = tool_calls
                .iter()
                .map(|tc| format!("{}\u{1f}{}", tc.name, tc.arguments.as_ref()))
                .collect::<Vec<_>>()
                .join("\u{1e}");
            let step_tool_name = tool_calls
                .first()
                .map(|tc| tc.name.clone())
                .unwrap_or_default();
            let is_true_noop = self.is_run_true_step(&tool_calls).await;
            identical_tool_calls.observe(&step_signature, &step_tool_name, is_true_noop);
            if is_true_noop {
                ::diagnostics::session_ctx::log_event(::diagnostics::events::ShellTrueNoop {
                    tool_name: step_tool_name.clone(),
                });
            }
            let tool_call_responses: Vec<ToolCallResponse> = tool_calls
                .into_iter()
                .map(|tc| ToolCallResponse {
                    id: tc.id.as_ref().to_owned(),
                    kind: "function".to_string(),
                    function: crate::sampling::types::ToolCallFunction {
                        name: tc.name,
                        arguments: tc.arguments.as_ref().to_owned(),
                    },
                })
                .collect();
            let execute_tool_calls_result = self.execute_tool_calls(tool_call_responses).await;
            let resample_after_control = match execute_tool_calls_result {
                Ok(ToolLoop::PermissionReject { tool_name, reason }) => {
                    return Ok(TurnOutcome::Cancelled {
                        category: Some(
                            crate::session::events::CancellationCategory::PermissionRejected,
                        ),
                        context: Some(serde_json::json!({
                            "tool_name": tool_name,
                            "reason": reason,
                        })),
                    });
                }
                Ok(ToolLoop::HookDenied { .. }) => false,
                Ok(ToolLoop::Control(ControlDisposition::ResampleStep)) => true,
                Ok(ToolLoop::Control(ControlDisposition::EndTurn)) => {
                    let snapshot = self
                        .finalize_turn_bookkeeping(
                            req_id,
                            conv_turn_start,
                            &turn_span_totals,
                            model_fingerprint.clone(),
                        )
                        .await;
                    return Ok(TurnOutcome::ControlEnded {
                        snapshot: Box::new(snapshot),
                    });
                }
                Ok(ToolLoop::Cancelled) => {
                    return Ok(TurnOutcome::Cancelled {
                        category: Some(
                            crate::session::events::CancellationCategory::PermissionCancelled,
                        ),
                        context: None,
                    });
                }
                Ok(ToolLoop::PermissionTimedOut { tool_name }) => {
                    return Ok(TurnOutcome::Cancelled {
                        category: Some(
                            crate::session::events::CancellationCategory::PermissionTimedOut,
                        ),
                        context: Some(serde_json::json!({
                            "tool_name": tool_name,
                            "reason": "permission request timed out",
                        })),
                    });
                }
                Ok(ToolLoop::FollowupMessage(followup_message)) => {
                    self.add_followup_message_as_user_turn(&followup_message)
                        .await;
                    continue;
                }
                _ => false,
            };
            let next_turn = tool_turn_count + 1;
            if let Some(limit) = self.max_turns
                && next_turn > limit
            {
                tracing::info!(
                    session_id = %self.session_info.id,
                    tool_turn_count,
                    limit,
                    "max-turns limit reached, stopping"
                );
                return Ok(TurnOutcome::MaxTurnsReached { limit });
            }
            tool_turn_count = next_turn;
            if resample_after_control {
                // Plan phase transitions stay inside the admitted Plan Turn,
                // but the next Step must see the newly active phase contract.
                plan_handoff_resample_pending = true;
                self.inject_behavior_reminders().await?;
            }
            if self.tool_context.task_output_token_budget.is_none()
                && let Some(trigger_info) = self.check_preflight_overflow().await
            {
                pending_forced_compaction = Some(trigger_info);
                continue;
            }
        }
    }
}
const MAX_CONSECUTIVE_IDENTICAL_TOOL_CALLS: u32 = 16;
const NUDGE_AFTER_IDENTICAL_TOOL_CALLS: u32 = 8;
const MAX_CONSECUTIVE_TRUE_NOOPS: u32 = 4;
const _: () = assert!(NUDGE_AFTER_IDENTICAL_TOOL_CALLS < MAX_CONSECUTIVE_IDENTICAL_TOOL_CALLS);
const _: () = assert!(MAX_CONSECUTIVE_TRUE_NOOPS < NUDGE_AFTER_IDENTICAL_TOOL_CALLS);
const ACTION_STATIONARITY_NUDGE_TEMPLATE: &str = "You have called the same tool \
     (`${{ tool_name }}`) with the exact same arguments ${{ run_len }} times in a row — \
     you appear to be stuck in a polling loop. Stop repeating this call. If you are \
     waiting on a long-running job or command, use a background task${%- if tools.by_kind.monitor %} \
     or the `${{ tools.by_kind.monitor }}` tool${%- endif %}, or run a single `sleep` and \
     then check once — do not poll in a tight loop. If you cannot make progress, stop and \
     tell the user what you are waiting for. This turn will be halted automatically if the \
     identical call keeps repeating.";
fn hash_step_signature(signature: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    signature.hash(&mut hasher);
    hasher.finish()
}
fn command_is_true(cmd: &str) -> bool {
    cmd.trim().eq_ignore_ascii_case("true")
}
#[derive(Default)]
struct IdenticalToolCallRun {
    last_signature_hash: Option<u64>,
    tool_name: String,
    run_len: u32,
    is_true_noop_run: bool,
    nudged: bool,
}
impl IdenticalToolCallRun {
    fn observe(&mut self, signature: &str, tool_name: &str, is_true_noop: bool) -> u32 {
        let hash = hash_step_signature(if is_true_noop {
            "\0true_noop"
        } else {
            signature
        });
        if self.last_signature_hash == Some(hash) {
            self.run_len += 1;
        } else {
            self.run_len = 1;
            self.last_signature_hash = Some(hash);
            self.is_true_noop_run = is_true_noop;
            self.nudged = false;
        }
        self.tool_name = tool_name.to_string();
        self.run_len
    }
    /// Once per identical run at/after the nudge threshold. Call only after results are committed.
    fn take_nudge(&mut self) -> bool {
        let fire = self.run_len >= NUDGE_AFTER_IDENTICAL_TOOL_CALLS && !self.nudged;
        self.nudged |= fire;
        fire
    }
    fn hard_stop_threshold(&self) -> u32 {
        if self.is_true_noop_run {
            MAX_CONSECUTIVE_TRUE_NOOPS
        } else {
            MAX_CONSECUTIVE_IDENTICAL_TOOL_CALLS
        }
    }
}
#[cfg(test)]
mod continuation_step_tests {
    use super::*;
    use crate::session::actor::tests::support::{begin_test_active_causal_turn, build_actor};

    #[tokio::test]
    async fn successor_sampling_step_is_started_after_an_already_closed_boundary() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = build_actor().await;
                begin_test_active_causal_turn(&actor).await;
                let boundary = actor
                    .end_step_control_boundary("completed")
                    .await
                    .expect("the original Step ends");
                assert_eq!(
                    actor
                        .apply_pending_controls_at_step_boundary(boundary)
                        .await,
                    (false, false, false)
                );
                assert!(!actor.events.has_active_step());

                assert!(
                    actor
                        .start_step_after_control_boundary("test-active-turn", None)
                        .await
                );
                assert!(actor.events.has_active_step());

                let events = actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .expect("Timeline events");
                let step_events = events
                    .iter()
                    .filter_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Step(step) => Some(step),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert!(matches!(
                    step_events.as_slice(),
                    [
                        chat_state::StepEvent::Started { .. },
                        chat_state::StepEvent::Ended { .. },
                        chat_state::StepEvent::Started { .. }
                    ]
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn first_sampling_step_uses_an_initial_control_horizon_without_step_ended() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = build_actor().await;
                actor.events.begin_turn();
                actor
                    .events
                    .start_turn(crate::session::events::Event::TurnStarted {
                        session_id: actor.session_id_string(),
                        turn_number: 1,
                        identity: chat_state::TurnIdentity {
                            origin: "user".into(),
                            turn_kind: "regular".into(),
                            goal_id: None,
                            goal_definition_revision: None,
                            stage_id: None,
                        },
                        model_id: "test".into(),
                        permission_mode: actor.permissions.mode(),
                        conversation_message_count: 0,
                        prompt_index: Some(0),
                        prompt_text: Some("hello".into()),
                        input_kind: chat_state::TurnInputKind::Prompt,
                        session_relationship: crate::session::events::SessionRelationship::Primary,
                        schema_version: crate::session::events::EVENT_SCHEMA_VERSION.into(),
                        redirect_kind: None,
                    })
                    .await
                    .unwrap();
                actor.state.lock().await.foreground = ForegroundState::RegularTurn(
                    crate::session::actor::tests::support::running_task_stub("initial-step"),
                );

                let boundary = actor
                    .initial_step_control_boundary("initial-step")
                    .await
                    .expect("a fresh turn has an initial control horizon");
                assert_eq!(
                    actor
                        .apply_pending_controls_at_step_boundary(boundary)
                        .await,
                    (false, false, false)
                );
                assert!(
                    actor
                        .start_step_after_control_boundary("initial-step", None)
                        .await
                );

                let events = actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .expect("Timeline events");
                let step_events = events
                    .iter()
                    .filter_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Step(step) => Some(step),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert!(matches!(
                    step_events.as_slice(),
                    [chat_state::StepEvent::Started { .. }]
                ));
            })
            .await;
    }
}
#[cfg(test)]
mod identical_tool_call_run_tests {
    use super::{
        IdenticalToolCallRun, MAX_CONSECUTIVE_IDENTICAL_TOOL_CALLS, MAX_CONSECUTIVE_TRUE_NOOPS,
        NUDGE_AFTER_IDENTICAL_TOOL_CALLS, command_is_true,
    };
    #[test]
    fn identical_non_true_resets_and_caps_at_16() {
        let mut run = IdenticalToolCallRun::default();
        assert_eq!(run.observe("a", "a", false), 1);
        assert_eq!(run.observe("a", "a", false), 2);
        assert_eq!(run.observe("b", "b", false), 1);
        let mut last = 0;
        for _ in 0..MAX_CONSECUTIVE_IDENTICAL_TOOL_CALLS {
            last = run.observe("same", "same", false);
        }
        assert_eq!(last, MAX_CONSECUTIVE_IDENTICAL_TOOL_CALLS);
        assert_eq!(
            run.hard_stop_threshold(),
            MAX_CONSECUTIVE_IDENTICAL_TOOL_CALLS
        );
    }
    #[test]
    fn true_noops_chain_across_args_and_stop_at_4() {
        let mut run = IdenticalToolCallRun::default();
        for i in 1..=4 {
            assert_eq!(run.observe(&format!("sig{i}"), "bash", true), i);
        }
        assert!(run.is_true_noop_run);
        assert_eq!(run.hard_stop_threshold(), MAX_CONSECUTIVE_TRUE_NOOPS);
        assert_eq!(run.observe("squeue", "bash", false), 1);
        assert!(!run.is_true_noop_run);
    }
    #[test]
    fn command_is_true_trim_and_case() {
        assert!(command_is_true("true"));
        assert!(command_is_true(" TRUE "));
        assert!(!command_is_true("true && echo hi"));
        assert!(!command_is_true("lisa status"));
    }
    #[test]
    fn nudge_latch_fires_once_per_run_after_threshold() {
        let mut run = IdenticalToolCallRun::default();
        for i in 1..NUDGE_AFTER_IDENTICAL_TOOL_CALLS {
            assert_eq!(run.observe("poll", "get_task_output", false), i);
            assert!(
                !run.take_nudge(),
                "must not nudge before threshold; run_len={i}"
            );
        }
        assert_eq!(
            run.observe("poll", "get_task_output", false),
            NUDGE_AFTER_IDENTICAL_TOOL_CALLS
        );
        assert!(run.take_nudge());
        assert!(!run.take_nudge());
        assert_eq!(
            run.observe("poll", "get_task_output", false),
            NUDGE_AFTER_IDENTICAL_TOOL_CALLS + 1
        );
        assert!(!run.take_nudge());
        assert_eq!(run.observe("other", "bash", false), 1);
        assert!(!run.nudged);
        assert!(!run.take_nudge());
    }
}
#[cfg(test)]
mod user_echo_broadcast_tests {
    use super::{UserEchoMode, user_echo_mode};
    use crate::session::PromptOrigin;
    /// Notification-drain: persisted (rewind/fork count user-chunk runs as
    /// turn boundaries) but never broadcast live; the pager hides it via the
    /// `hideFromScrollback` chunk meta.
    #[test]
    fn notification_drain_turn_is_persist_only() {
        assert_eq!(
            user_echo_mode(&PromptOrigin::NotificationDrain),
            UserEchoMode::PersistOnly
        );
    }
    /// Real user prompts and other turns still broadcast live so multi-client
    /// and dashboard viewers stay in sync.
    #[test]
    fn user_and_completion_turns_broadcast_live() {
        assert_eq!(user_echo_mode(&PromptOrigin::User), UserEchoMode::Broadcast);
        assert_eq!(
            user_echo_mode(&PromptOrigin::TaskCompleted {
                task_id: "bg-1".to_string(),
            }),
            UserEchoMode::Broadcast
        );
        assert_eq!(
            user_echo_mode(&PromptOrigin::SubagentCompleted {
                subagent_id: "xyz".to_string(),
            }),
            UserEchoMode::Broadcast
        );
    }
}
#[cfg(test)]
mod structured_output_validation_tests {
    use super::validate_structured_output;
    fn validator() -> Result<sampling_types::OutputSchemaValidator, String> {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"name": {"type": "string"}, "age": {"type": "integer"}},
            "required": ["name", "age"],
            "additionalProperties": false,
        });
        sampling_types::compile_output_schema(&schema)
    }
    #[test]
    fn accepts_conforming_json() {
        let v = validate_structured_output(&validator(), r#"{"name":"alice","age":30}"#).unwrap();
        assert_eq!(v["name"], "alice");
    }
    #[test]
    fn rejects_non_json() {
        let err = validate_structured_output(&validator(), "not json").unwrap_err();
        assert!(err.starts_with("model output was not valid JSON: "));
    }
    #[test]
    fn rejects_schema_violation() {
        let err = validate_structured_output(&validator(), r#"{"name":"alice"}"#).unwrap_err();
        assert!(err.starts_with("output does not match the required schema: "));
    }
    #[test]
    fn surfaces_invalid_schema_error() {
        let bad: Result<sampling_types::OutputSchemaValidator, String> =
            Err("invalid output schema: boom".into());
        let err = validate_structured_output(&bad, r#"{"name":"alice","age":1}"#).unwrap_err();
        assert_eq!(err, "invalid output schema: boom");
    }
}

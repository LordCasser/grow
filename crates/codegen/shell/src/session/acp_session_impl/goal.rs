//! Goal runtime. Stages are background work; only implementer and final
//! reporting turns may own the foreground.

use super::prompt_queue::RunningPromptDisplay;
use super::*;
use tools::implementations::grow_build::task::backend::{ChannelBackend, SubagentBackend};
use tools::implementations::grow_build::task::types::{
    SubagentOwner, SubagentRequest, SubagentRuntimeOverrides,
};

#[derive(serde::Deserialize)]
struct VerifierResponse {
    verdict: String,
    #[serde(default)]
    feedback: String,
    #[serde(default)]
    fingerprint: String,
}

impl SessionActor {
    pub(super) async fn initialize_goal_runtime(
        self: &std::sync::Arc<Self>,
        objective: &str,
        token_budget: Option<i64>,
    ) {
        if let Some((_, cancel)) = self.goal_stage_cancel.lock().take() {
            cancel.cancel();
        }
        let token_baseline = self.chat_state_handle.get_total_tokens().await as i64;
        self.goal_tracker.lock().create_goal(
            uuid::Uuid::now_v7().to_string(),
            objective.to_string(),
            token_budget,
            token_baseline,
            chrono::Utc::now().to_rfc3339(),
            None,
        );
        self.goal_turn_task_ids.lock().clear();
        self.subagent_token_records.lock().clear();
        let (used, finished) = self.goal_tokens(token_baseline);
        self.goal_notify_sender()
            .emit_goal_updated(&mut self.goal_tracker.lock(), used, finished);
        self.idle_arbiter.notify_one();
    }

    pub(super) async fn setup_goal(
        self: &std::sync::Arc<Self>,
        objective: &str,
        token_budget: Option<i64>,
    ) -> String {
        self.initialize_goal_runtime(objective, token_budget).await;
        self.render_goal_start_reminder().await
    }

    pub(super) async fn render_goal_start_reminder(&self) -> String {
        let Some(goal) = self.goal_tracker.lock().snapshot().cloned() else {
            return "No Goal is active.".to_string();
        };
        format!(
            "Goal accepted. Planning runs in the background and does not block user turns.\nObjective: {}\nUse get_goal to read the current Markdown plan.",
            goal.objective
        )
    }

    pub(super) async fn resume_goal(self: &std::sync::Arc<Self>) -> String {
        let outcome = {
            let mut tracker = self.goal_tracker.lock();
            match tracker.status() {
                None => return "No goal is currently set.".into(),
                Some(crate::session::goal_tracker::GoalStatus::Complete) => {
                    return "Goal is already complete.".into();
                }
                Some(crate::session::goal_tracker::GoalStatus::BudgetLimited) => {
                    return "Goal is budget-limited. Set a new budget before resuming.".into();
                }
                Some(crate::session::goal_tracker::GoalStatus::Active) => false,
                Some(_) => tracker.resume(),
            }
        };
        if outcome {
            let current = self.chat_state_handle.get_total_tokens().await as i64;
            let (used, finished) = self.goal_tokens(current);
            self.goal_notify_sender().emit_goal_updated(
                &mut self.goal_tracker.lock(),
                used,
                finished,
            );
        }
        self.idle_arbiter.notify_one();
        if outcome {
            "Goal resumed. Background stages and idle continuation will continue.".into()
        } else {
            "Goal is already active.".into()
        }
    }

    pub(crate) async fn auto_pause_goal_if_active(
        &self,
        reason: crate::session::goal_tracker::GoalPauseReason,
    ) {
        self.auto_pause_goal_if_active_with_message(reason, reason.history_detail().to_string())
            .await;
    }

    pub(crate) async fn auto_pause_goal_if_active_with_message(
        &self,
        reason: crate::session::goal_tracker::GoalPauseReason,
        message: String,
    ) -> bool {
        let changed = self.goal_tracker.lock().pause_with_message(reason, message);
        if !changed {
            return false;
        }
        if let Some((_, cancel)) = self.goal_stage_cancel.lock().take() {
            cancel.cancel();
        }
        let current = self.chat_state_handle.get_total_tokens().await as i64;
        let (used, finished) = self.goal_tokens(current);
        self.goal_notify_sender()
            .emit_goal_updated(&mut self.goal_tracker.lock(), used, finished);
        true
    }

    /// Cancel the background stage only when it still belongs to `lease`.
    ///
    /// A cancelled stage reports its terminal result asynchronously. Matching
    /// by lease prevents that late result from taking the cancellation handle
    /// of a newer planner/verifier.
    fn cancel_goal_stage_for_lease(
        &self,
        lease: &crate::session::goal_tracker::StageLease,
    ) -> bool {
        let cancel = {
            let mut running = self.goal_stage_cancel.lock();
            if running
                .as_ref()
                .is_some_and(|(running_lease, _)| running_lease == lease)
            {
                running.take().map(|(_, cancel)| cancel)
            } else {
                None
            }
        };
        let Some(cancel) = cancel else {
            return false;
        };
        cancel.cancel();
        true
    }

    pub(super) async fn enforce_goal_token_budget(&self, current_tokens: i64) -> bool {
        let used = self.goal_tokens_used(current_tokens);
        let exhausted = self
            .goal_tracker
            .lock()
            .token_budget()
            .is_some_and(|budget| used >= budget);
        if !exhausted {
            return false;
        }
        let changed = self.goal_tracker.lock().budget_limit();
        if changed {
            if let Some((_, cancel)) = self.goal_stage_cancel.lock().take() {
                cancel.cancel();
            }
            let (used, finished) = self.goal_tokens(current_tokens);
            self.goal_notify_sender().emit_goal_updated(
                &mut self.goal_tracker.lock(),
                used,
                finished,
            );
        }
        changed
    }

    pub(super) fn render_goal_continuation(&self, _current_tokens: i64) -> Option<String> {
        let goal = self.goal_tracker.lock().snapshot()?.clone();
        if goal.status != crate::session::goal_tracker::GoalStatus::Active
            || goal.phase != crate::session::goal_tracker::GoalPhase::Executing
        {
            return None;
        }
        let feedback = goal
            .verifier_feedback
            .as_deref()
            .map(|feedback| format!("\nLatest verifier feedback:\n{feedback}\n"))
            .unwrap_or_default();
        Some(format!(
            "Continue the active Goal using the latest blackboard. User messages may arrive and must be handled normally.\n\nObjective (rev {}):\n{}\n\nPlan (rev {}):\n{}{}\nWhen the work is genuinely complete, call update_goal with action=candidate_complete; do not merely stop.",
            goal.objective_revision,
            goal.objective,
            goal.plan.revision,
            goal.plan.markdown,
            feedback,
        ))
    }

    async fn run_goal_subagent(
        &self,
        goal_id: &str,
        prompt: String,
        description: &str,
        role: &str,
        cancel_token: tokio_util::sync::CancellationToken,
        fork_context: bool,
    ) -> Result<String, String> {
        let Some(event_tx) = self.tool_context.subagent_event_tx.clone() else {
            return Err("subagent coordinator unavailable".into());
        };
        let request = SubagentRequest {
            id: uuid::Uuid::now_v7().to_string(),
            prompt,
            description: description.to_string(),
            subagent_type: "general-purpose".to_string(),
            parent_session_id: self.session_id_string(),
            parent_prompt_id: None,
            resume_from: None,
            cwd: Some(self.tool_context.cwd.as_str().to_owned()),
            runtime_overrides: SubagentRuntimeOverrides::default(),
            run_in_background: false,
            surface_completion: false,
            await_to_completion: false,
            fork_context,
            owner: SubagentOwner::goal(goal_id),
            cancel_token,
        };
        let result = ChannelBackend::new(event_tx)
            .spawn(request)
            .await
            .map_err(|error| error.to_string())?;
        if result.success {
            Ok(result.output.to_string())
        } else {
            Err(result.error.unwrap_or_else(|| format!("{role} failed")))
        }
    }

    fn spawn_planner_stage(self: &std::sync::Arc<Self>) {
        let (lease, objective) = {
            let mut tracker = self.goal_tracker.lock();
            let Some(lease) =
                tracker.claim_stage(crate::session::goal_tracker::GoalPhase::Planning)
            else {
                return;
            };
            let objective = tracker
                .snapshot()
                .map(|goal| goal.objective.clone())
                .unwrap_or_default();
            if let Some(goal) = tracker.snapshot_mut() {
                goal.history
                    .push(crate::session::goal_tracker::GoalHistoryEntry::new(
                        crate::session::goal_tracker::GoalEvent::PlanningStarted,
                        None,
                    ));
            }
            (lease, objective)
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        *self.goal_stage_cancel.lock() = Some((lease.clone(), cancel.clone()));
        let actor = std::sync::Arc::clone(self);
        tokio::task::spawn_local(async move {
            let prompt = format!(
                "Create the durable Markdown blackboard for this Goal. Inspect the workspace as needed. Return ONLY the complete Markdown plan, including acceptance criteria, concrete tasks, and verification steps. Do not write plan.md.\n\nOBJECTIVE:\n{objective}"
            );
            let outcome = actor
                .run_goal_subagent(
                    &lease.goal_id,
                    prompt,
                    "Goal planner",
                    "planner",
                    cancel,
                    true,
                )
                .await
                .and_then(|markdown| {
                    (!markdown.trim().is_empty())
                        .then_some(markdown)
                        .ok_or_else(|| "planner returned an empty blackboard".to_string())
                });
            let _ = actor.event_tx.send(SessionEvent::GoalStageCompleted(
                crate::session::replay_events::GoalStageCompletion {
                    lease,
                    kind: crate::session::replay_events::GoalStageKind::Planner(outcome),
                },
            ));
        });
    }

    fn spawn_verifier_stage(self: &std::sync::Arc<Self>) {
        let (lease, goal) = {
            let mut tracker = self.goal_tracker.lock();
            let Some(lease) =
                tracker.claim_stage(crate::session::goal_tracker::GoalPhase::Verifying)
            else {
                return;
            };
            let Some(goal) = tracker.snapshot().cloned() else {
                return;
            };
            (lease, goal)
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        *self.goal_stage_cancel.lock() = Some((lease.clone(), cancel.clone()));
        let actor = std::sync::Arc::clone(self);
        tokio::task::spawn_local(async move {
            let prompt = format!(
                "Independently verify the Goal against the current workspace evidence. Do not trust the candidate claim. Return ONLY JSON: {{\"verdict\":\"achieved|not_achieved|blocked\",\"feedback\":\"actionable evidence/gaps\",\"fingerprint\":\"stable normalized gap key\"}}. Use blocked only when the same work cannot be completed in this environment.\n\nOBJECTIVE (rev {}):\n{}\n\nPLAN (rev {}):\n{}\n\nCANDIDATE SUMMARY:\n{}",
                goal.objective_revision,
                goal.objective,
                goal.plan.revision,
                goal.plan.markdown,
                goal.candidate_summary.as_deref().unwrap_or_default(),
            );
            let outcome = actor
                .run_goal_subagent(
                    &lease.goal_id,
                    prompt,
                    "Goal verifier",
                    "verifier",
                    cancel,
                    false,
                )
                .await
                .and_then(|raw| {
                    let mut body = raw.trim();
                    if let Some(stripped) = body.strip_prefix("```json") {
                        body = stripped.trim();
                    }
                    if let Some(stripped) = body.strip_suffix("```") {
                        body = stripped.trim();
                    }
                    let parsed: VerifierResponse =
                        serde_json::from_str(body).map_err(|error| error.to_string())?;
                    match parsed.verdict.as_str() {
                        "achieved" => {
                            Ok(crate::session::replay_events::GoalVerifierOutcome::Achieved)
                        }
                        "not_achieved" => Ok(
                            crate::session::replay_events::GoalVerifierOutcome::NotAchieved {
                                fingerprint: if parsed.fingerprint.trim().is_empty() {
                                    parsed.feedback.to_lowercase()
                                } else {
                                    parsed.fingerprint
                                },
                                feedback: parsed.feedback,
                            },
                        ),
                        "blocked" => Ok(
                            crate::session::replay_events::GoalVerifierOutcome::Blocked {
                                message: parsed.feedback,
                            },
                        ),
                        other => Err(format!("unknown verifier verdict: {other}")),
                    }
                });
            let _ = actor.event_tx.send(SessionEvent::GoalStageCompleted(
                crate::session::replay_events::GoalStageCompletion {
                    lease,
                    kind: crate::session::replay_events::GoalStageKind::Verifier(outcome),
                },
            ));
        });
    }

    pub(super) async fn handle_goal_stage_completed(
        self: &std::sync::Arc<Self>,
        completion: crate::session::replay_events::GoalStageCompletion,
    ) {
        use crate::session::goal_tracker::GoalPhase;
        use crate::session::replay_events::{GoalStageKind, GoalVerifierOutcome};
        {
            let mut running = self.goal_stage_cancel.lock();
            if running
                .as_ref()
                .is_some_and(|(lease, _)| lease == &completion.lease)
            {
                running.take();
            }
        }
        let applied = match completion.kind {
            GoalStageKind::Planner(Ok(markdown)) => self
                .goal_tracker
                .lock()
                .apply_planner_result(&completion.lease, markdown),
            GoalStageKind::Planner(Err(message)) => self
                .goal_tracker
                .lock()
                .planner_failed(&completion.lease, message),
            GoalStageKind::Verifier(Ok(GoalVerifierOutcome::Achieved)) => self
                .goal_tracker
                .lock()
                .verification_achieved(&completion.lease),
            GoalStageKind::Verifier(Ok(GoalVerifierOutcome::NotAchieved {
                feedback,
                fingerprint,
            })) => self.goal_tracker.lock().verification_not_achieved(
                &completion.lease,
                feedback,
                fingerprint,
            ),
            GoalStageKind::Verifier(Ok(GoalVerifierOutcome::Blocked { message })) => self
                .goal_tracker
                .lock()
                .verification_blocked(&completion.lease, message),
            GoalStageKind::Verifier(Err(message)) => {
                let current = self
                    .goal_tracker
                    .lock()
                    .lease_is_current(&completion.lease, GoalPhase::Verifying);
                if current {
                    self.goal_tracker.lock().pause_with_message(
                        crate::session::goal_tracker::GoalPauseReason::Infra,
                        format!("Verification unavailable: {message}"),
                    )
                } else {
                    false
                }
            }
        };
        if applied {
            let current = self.chat_state_handle.get_total_tokens().await as i64;
            let (used, finished) = self.goal_tokens(current);
            self.goal_notify_sender().emit_goal_updated(
                &mut self.goal_tracker.lock(),
                used,
                finished,
            );
            self.idle_arbiter.notify_one();
        }
    }

    pub(super) async fn drive_goal_on_idle(
        self: std::sync::Arc<Self>,
        completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
    ) {
        if !self.goal_harness_enabled() {
            return;
        }
        let phase = {
            let tracker = self.goal_tracker.lock();
            if tracker.status() != Some(crate::session::goal_tracker::GoalStatus::Active) {
                return;
            }
            tracker.phase()
        };
        match phase {
            Some(crate::session::goal_tracker::GoalPhase::Planning) => {
                self.spawn_planner_stage();
            }
            Some(crate::session::goal_tracker::GoalPhase::Verifying) => {
                self.spawn_verifier_stage();
            }
            Some(crate::session::goal_tracker::GoalPhase::Executing) => {
                {
                    let state = self.state.lock().await;
                    if !state.foreground.is_idle() || !state.pending_inputs.is_empty() {
                        return;
                    }
                }
                let current = self.chat_state_handle.get_total_tokens().await as i64;
                if self.enforce_goal_token_budget(current).await {
                    return;
                }
                let Some(directive) = self.render_goal_continuation(current) else {
                    return;
                };
                self.start_goal_internal_turn(directive, false, completion_tx)
                    .await;
            }
            Some(crate::session::goal_tracker::GoalPhase::Summarizing) => {
                {
                    let state = self.state.lock().await;
                    if !state.foreground.is_idle() || !state.pending_inputs.is_empty() {
                        return;
                    }
                }
                let Some(goal) = self.goal_tracker.lock().snapshot().cloned() else {
                    return;
                };
                let directive = format!(
                    "Produce the final user-facing Goal report now. Summarize concrete changes, verification evidence, and any caveats. Do not invoke a separate summarizer.\n\nObjective:\n{}\n\nVerified candidate:\n{}",
                    goal.objective,
                    goal.candidate_summary.as_deref().unwrap_or_default(),
                );
                self.start_goal_internal_turn(directive, true, completion_tx)
                    .await;
            }
            None => {}
        }
    }

    async fn start_goal_internal_turn(
        self: &std::sync::Arc<Self>,
        directive: String,
        finalization: bool,
        completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
    ) {
        let mut state = self.state.lock().await;
        if !state.foreground.is_idle() || !state.pending_inputs.is_empty() {
            return;
        }
        let (goal_id, stage_id) = {
            let mut tracker = self.goal_tracker.lock();
            let Some(goal_id) = tracker.snapshot().map(|goal| goal.goal_id.clone()) else {
                return;
            };
            if !finalization && !tracker.worker_started() {
                return;
            }
            let stage_id = tracker
                .snapshot()
                .map(|goal| goal.total_worker_rounds as u64 + u64::from(finalization))
                .unwrap_or_default();
            (goal_id, stage_id)
        };
        let prompt_id = uuid::Uuid::now_v7().to_string();
        let origin = if finalization {
            crate::session::PromptOrigin::GoalFinalization { goal_id, stage_id }
        } else {
            crate::session::PromptOrigin::GoalContinuation { goal_id, stage_id }
        };
        *self
            .current_prompt_id
            .lock()
            .expect("current_prompt_id mutex poisoned") = Some(prompt_id.clone());
        let display = RunningPromptDisplay {
            id: prompt_id.clone(),
            text: String::new(),
            kind: if finalization {
                "goal_finalization"
            } else {
                "goal_continuation"
            }
            .into(),
            origin: origin.wire_name().into(),
            turn_kind: crate::session::TurnKind::Internal.wire_name().into(),
            combined_texts: None,
        };
        self.broadcast_queue_changed_promoting(&state, display);
        state.foreground = ForegroundState::RegularTurn(AgentTask::new_prompt(
            self.clone(),
            prompt_id,
            origin,
            crate::session::TurnKind::Internal,
            vec![acp::ContentBlock::Text(acp::TextContent::new(directive))],
            crate::session::behavior::PromptMode::Agent,
            None,
            None,
            true,
            None,
            completion_tx,
            None,
            None,
        ));
    }

    pub(super) async fn finalize_goal_finalization_turn(&self) {
        // Charge the final reporting turn while the Goal is still Active.
        // Completion freezes the receipt so later Normal turns cannot inflate
        // its persisted token total on reload.
        self.settle_live_goal_subagent_tokens();
        let current = self.chat_state_handle.get_total_tokens().await as i64;
        let (used, finished) = self.goal_tokens(current);
        if !self.goal_tracker.lock().complete_verified() {
            return;
        }
        self.goal_notify_sender()
            .emit_goal_updated(&mut self.goal_tracker.lock(), used, finished);
        // Messages queued while the final report was running were captured in
        // Goal mode. Completion exits Goal before FIFO promotion, so rebase
        // those user-owned rows now or their stale mode would recreate a Goal
        // from ordinary follow-up text.
        self.retag_queued_goal_user_prompts(crate::session::behavior::PromptMode::Agent)
            .await;
        self.behavior.lock().select_behavior(None);
        *self.current_prompt_mode.lock() = crate::session::behavior::PromptMode::Agent;
        self.persist_behavior_state();
        self.enqueue_current_mode_update(agent_client_protocol::SessionModeId::new(
            tools::types::SessionMode::Default.as_id(),
        ));
    }

    pub(crate) async fn handle_goal_command(
        &self,
        command: tools::implementations::grow_build::update_goal::GoalCommand,
    ) {
        use crate::session::goal_tracker::{GoalPauseReason, GoalPlanAuthor};
        use tools::implementations::grow_build::update_goal::{
            GoalCommand, GoalView, UpdateGoalAction,
        };

        let current_tokens = self.chat_state_handle.get_total_tokens().await as i64;
        match command {
            GoalCommand::Get { respond_to } => {
                let used = self.goal_tokens_used(current_tokens);
                let response = self
                    .goal_tracker
                    .lock()
                    .snapshot()
                    .map(|goal| GoalView {
                        goal_id: goal.goal_id.clone(),
                        objective: goal.objective.clone(),
                        objective_revision: goal.objective_revision,
                        status: format!("{:?}", goal.status).to_ascii_lowercase(),
                        phase: format!("{:?}", goal.phase).to_ascii_lowercase(),
                        token_budget: goal.token_budget,
                        tokens_used: used,
                        plan_revision: goal.plan.revision,
                        plan_markdown: goal.plan.markdown.clone(),
                        verifier_feedback: goal.verifier_feedback.clone(),
                    })
                    .ok_or_else(|| "No Goal is set.".to_string());
                let _ = respond_to.send(response);
                return;
            }
            GoalCommand::ReplacePlan { input, respond_to } => {
                // Capture the verifier lease under the same tracker lock that
                // commits the revision. No async work can observe a successful
                // revision while the old lease is still authoritative.
                let (changed, invalidated_verifier) = {
                    let mut tracker = self.goal_tracker.lock();
                    let invalidated_verifier = tracker
                        .snapshot()
                        .filter(|goal| {
                            goal.phase == crate::session::goal_tracker::GoalPhase::Verifying
                        })
                        .and_then(|goal| goal.in_flight_stage.clone());
                    let changed =
                        tracker.replace_plan(input.markdown, GoalPlanAuthor::Agent, input.reason);
                    (changed, changed.then_some(invalidated_verifier).flatten())
                };
                let verifier_cancelled = invalidated_verifier
                    .as_ref()
                    .is_some_and(|lease| self.cancel_goal_stage_for_lease(lease));
                let response = changed
                    .then(|| {
                        if verifier_cancelled {
                            "Goal plan replaced; prior verification was cancelled and execution will continue from the new revision."
                                .to_string()
                        } else {
                            "Goal plan replaced; execution will continue from the new revision."
                                .to_string()
                        }
                    })
                    .ok_or_else(|| {
                        "The Goal is not active or the Markdown plan is empty.".to_string()
                    });
                let _ = respond_to.send(response);
                if !changed {
                    return;
                }
            }
            GoalCommand::Update { input, respond_to } => {
                let message = input.message.trim().to_string();
                if message.is_empty() {
                    let _ = respond_to.send(Err("A non-empty message is required.".into()));
                    return;
                }
                let (changed, summary) = match input.action {
                    UpdateGoalAction::CandidateComplete => (
                        self.goal_tracker.lock().candidate_complete(message),
                        "Completion candidate accepted; independent verification will run in the background."
                            .to_string(),
                    ),
                    UpdateGoalAction::Blocked => (
                        self.goal_tracker
                            .lock()
                            .pause_with_message(GoalPauseReason::Verification, message.clone()),
                        format!("Goal blocked: {message}"),
                    ),
                };
                let response = changed.then_some(summary).ok_or_else(|| {
                    "The Goal is not in a phase that accepts this update.".to_string()
                });
                let _ = respond_to.send(response);
                if !changed {
                    return;
                }
            }
        }

        let (used, finished) = self.goal_tokens(current_tokens);
        self.goal_notify_sender()
            .emit_goal_updated(&mut self.goal_tracker.lock(), used, finished);
        self.idle_arbiter.notify_one();
    }
}

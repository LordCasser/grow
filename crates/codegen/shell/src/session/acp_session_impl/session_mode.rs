//! Session Behavior transitions, reminders, and persistence.
use super::*;
use crate::session::behavior::BehaviorChangeOutcome;
pub(super) fn prompt_mode_from_session_mode_id(session_mode_id: &acp::SessionModeId) -> PromptMode {
    use tools::types::SessionMode;
    match SessionMode::from_id(session_mode_id.0.as_ref()) {
        SessionMode::Plan => PromptMode::Plan,
        SessionMode::Ask => PromptMode::Ask,
        SessionMode::Workflow => PromptMode::Workflow,
        SessionMode::DeepResearch => PromptMode::DeepResearch,
        SessionMode::Goal => PromptMode::Goal,
        SessionMode::Default => PromptMode::Agent,
    }
}
pub(super) fn session_mode_from_prompt_mode(mode: PromptMode) -> tools::types::SessionMode {
    match mode {
        PromptMode::Agent => tools::types::SessionMode::Default,
        PromptMode::Ask => tools::types::SessionMode::Ask,
        PromptMode::Plan => tools::types::SessionMode::Plan,
        PromptMode::Workflow => tools::types::SessionMode::Workflow,
        PromptMode::DeepResearch => tools::types::SessionMode::DeepResearch,
        PromptMode::Goal => tools::types::SessionMode::Goal,
    }
}
/// Plan is a frozen, human-approved execution protocol. The Static Workflow
/// launcher is therefore not advertised in any Plan phase; the runtime gate
/// remains as defense in depth for stale or forged calls.
pub(super) fn filter_cursor_tools_by_plan_mode(
    defs: Vec<ToolDefinition>,
    plan_active: bool,
) -> Vec<ToolDefinition> {
    if !plan_active {
        return defs;
    }
    defs.into_iter()
        .filter(|def| {
            def.function.name != tools::implementations::grow_build::workflow::WORKFLOW_TOOL_NAME
        })
        .collect()
}
impl SessionActor {
    /// Rebase queued user messages that were captured under Goal when Goal is
    /// intentionally exited. Queue entries retain their submission mode in
    /// general, but a terminal/cleared Goal can no longer consume Goal-bound
    /// supplements; leaving them pinned would immediately switch the session
    /// back to a completed or missing Goal when they promote.
    pub(super) async fn retag_queued_goal_user_prompts(&self, target: PromptMode) {
        let mut state = self.state.lock().await;
        for input in &mut state.pending_inputs {
            if matches!(input.origin, crate::session::PromptOrigin::User)
                && input.prompt_mode == PromptMode::Goal
            {
                input.prompt_mode = target;
            }
        }
    }

    /// Synchronize the selected primary-session Behavior into the fixed system
    /// prompt layer: Mandatory Core → Audience → Role → Behavior → Runtime.
    pub(super) async fn sync_active_behavior_prompt(&self) {
        use crate::session::behavior::{
            BehaviorState, clarify_reminder_template, deep_research_reminder_template,
            goal_reminder_template, plan_behavior_template, static_workflow_reminder_template,
        };
        let instructions = match self.behavior.lock().state() {
            BehaviorState::Normal => None,
            BehaviorState::Clarify => Some(clarify_reminder_template()),
            BehaviorState::Plan(_) => Some(plan_behavior_template()),
            BehaviorState::Workflow => Some(static_workflow_reminder_template()),
            BehaviorState::DeepResearch { .. } => Some(deep_research_reminder_template()),
            BehaviorState::Goal => Some(goal_reminder_template()),
        }
        .map(str::to_owned);
        let system_prompt = self
            .agent
            .borrow_mut()
            .set_behavior_instructions(instructions)
            .await;
        let mut conversation = self.chat_state_handle.get_conversation().await;
        for item in conversation.iter_mut() {
            if let ConversationItem::System(system) = item {
                system.content = std::sync::Arc::<str>::from(system_prompt);
                break;
            }
        }
        self.chat_state_handle.replace_conversation(conversation);
    }

    pub(super) fn apply_prompt_modes_to_snapshot(&self, snapshot: &mut TurnDeltaSnapshot) {
        snapshot.start_prompt_mode = Some(self.turn_start_prompt_mode.lock().to_string());
        snapshot.end_prompt_mode = Some(self.turn_prompt_mode.lock().to_string());
    }
    /// `false` twin: this template integration is not compiled into this
    /// build, so no session runs it. Keeps ungated call sites compiling in
    /// both configurations.
    pub(super) fn is_cursor_harness(&self) -> bool {
        false
    }
    pub(super) async fn request_behavior_change(
        &self,
        session_mode_id: acp::SessionModeId,
    ) -> BehaviorChangeOutcome {
        use tools::types::SessionMode;
        let previous_prompt_mode = *self.current_prompt_mode.lock();
        let Some(mode) = SessionMode::try_from_id(session_mode_id.0.as_ref()) else {
            let message = format!(
                "Unknown Behavior id: {}. Agent Roles must be selected through the Agent interface.",
                session_mode_id.0
            );
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(
                    session_mode_from_prompt_mode(previous_prompt_mode).as_id(),
                ),
                serde_json::json!({ "status": "rejected", "message": message }),
            );
            return BehaviorChangeOutcome::Rejected { message };
        };
        let prompt_mode = match mode {
            SessionMode::Plan => PromptMode::Plan,
            SessionMode::Ask => PromptMode::Ask,
            SessionMode::Workflow => PromptMode::Workflow,
            SessionMode::DeepResearch => PromptMode::DeepResearch,
            SessionMode::Goal => PromptMode::Goal,
            SessionMode::Default => PromptMode::Agent,
        };
        let current_behavior = self.behavior.lock().behavior();
        let target_behavior = mode.behavior();
        let completed_goal_receipt = self.goal_tracker.lock().status()
            == Some(crate::session::goal_tracker::GoalStatus::Complete);
        if current_behavior == target_behavior
            && !(mode != SessionMode::Default && completed_goal_receipt)
        {
            let cleared = self.behavior.lock().clear_pending_switch();
            if cleared {
                self.enqueue_current_mode_update(acp::SessionModeId::new(mode.as_id()));
            }
            return BehaviorChangeOutcome::Applied;
        }
        if mode == SessionMode::Goal && (!self.goal_enabled || !self.goal_classifier_enabled) {
            let message = "Goal behavior requires goal orchestration and an independent verifier."
                .to_string();
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(
                    session_mode_from_prompt_mode(previous_prompt_mode).as_id(),
                ),
                serde_json::json!({ "status": "rejected", "message": message }),
            );
            return BehaviorChangeOutcome::Rejected { message };
        }
        if mode == SessionMode::DeepResearch && !self.background_workflows_enabled {
            let message =
                "Deep Research behavior requires the background Workflow runtime.".to_string();
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(
                    session_mode_from_prompt_mode(previous_prompt_mode).as_id(),
                ),
                serde_json::json!({ "status": "rejected", "message": message }),
            );
            return BehaviorChangeOutcome::Rejected { message };
        }

        if mode == SessionMode::Plan
            && self
                .agent
                .borrow()
                .tool_bridge()
                .tool_for_kind(tools::types::tool::ToolKind::PlanControl)
                .await
                .is_none()
        {
            let message =
                "Plan behavior is unavailable because PlanControl is not registered.".to_string();
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(
                    session_mode_from_prompt_mode(previous_prompt_mode).as_id(),
                ),
                serde_json::json!({ "status": "rejected", "message": message }),
            );
            return BehaviorChangeOutcome::Rejected { message };
        }
        if mode == SessionMode::Workflow
            && self
                .agent
                .borrow()
                .tool_bridge()
                .tool_for_kind(tools::types::tool::ToolKind::Workflow)
                .await
                .is_none()
        {
            let message = "Static Workflow behavior is unavailable in this session.".to_string();
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(
                    session_mode_from_prompt_mode(previous_prompt_mode).as_id(),
                ),
                serde_json::json!({ "status": "rejected", "message": message }),
            );
            return BehaviorChangeOutcome::Rejected { message };
        }

        let owned_deep_research_run = self
            .behavior
            .lock()
            .deep_research_run_id()
            .map(str::to_owned);
        if matches!(
            mode,
            SessionMode::Plan | SessionMode::Goal | SessionMode::DeepResearch
        ) {
            let has_unrelated_live_workflow = self
                .workflow_tracker()
                .await
                .lock()
                .list()
                .iter()
                .any(|run| {
                    !run.status.is_terminal()
                        && owned_deep_research_run.as_deref() != Some(run.run_id.as_str())
                });
            if has_unrelated_live_workflow {
                let message = format!(
                    "{} behavior is unavailable while an unrelated Workflow run is active; wait for it or stop it explicitly.",
                    mode.as_id()
                );
                self.enqueue_current_mode_update_with_behavior_change(
                    acp::SessionModeId::new(
                        session_mode_from_prompt_mode(previous_prompt_mode).as_id(),
                    ),
                    serde_json::json!({ "status": "rejected", "message": message }),
                );
                return BehaviorChangeOutcome::Rejected { message };
            }
        }

        let goal_active = current_behavior == Some(tool_types::BehaviorId::Goal)
            && self.goal_tracker.lock().status()
                == Some(crate::session::goal_tracker::GoalStatus::Active);
        let deep_research_active = if current_behavior == Some(tool_types::BehaviorId::DeepResearch)
        {
            match owned_deep_research_run.as_deref() {
                Some(run_id) => self
                    .workflow_tracker()
                    .await
                    .lock()
                    .get(run_id)
                    .is_some_and(|run| !run.status.is_terminal()),
                None => false,
            }
        } else {
            false
        };
        let interrupts_work = self.behavior.lock().is_plan() || goal_active || deep_research_active;
        if interrupts_work {
            const CONFIRM_WINDOW: std::time::Duration = std::time::Duration::from_secs(8);
            if !self
                .behavior
                .lock()
                .confirm_interrupting_switch(target_behavior, CONFIRM_WINDOW)
            {
                let remaining_ms = self
                    .behavior
                    .lock()
                    .pending_switch()
                    .map(|(_, _, ms)| ms)
                    .unwrap_or(8_000);
                let message = format!(
                    "Switching to {} will interrupt the active {} work. Press Enter to confirm the switch, or press Esc to cancel.",
                    mode.display_label(),
                    current_behavior
                        .map(|behavior| format!("{behavior:?}"))
                        .unwrap_or_else(|| "session".to_string())
                );
                self.enqueue_current_mode_update_with_behavior_change(
                    acp::SessionModeId::new(
                        session_mode_from_prompt_mode(previous_prompt_mode).as_id(),
                    ),
                    serde_json::json!({
                        "status": "confirmation_required",
                        "source": current_behavior.map(|x| format!("{x:?}").to_lowercase()),
                        "target": mode.as_id(),
                        "message": message,
                        "remainingMs": remaining_ms,
                    }),
                );
                return BehaviorChangeOutcome::ConfirmationRequired {
                    message,
                    remaining_ms,
                };
            }

            if self.behavior.lock().is_plan() {
                self.cancel_running_task(true, false, false, Some("behavior_switch".to_string()))
                    .await;
                self.behavior.lock().finish_plan();
            }
            if goal_active {
                use crate::session::goal_tracker::GoalPauseReason;
                // Preserve steering that arrived just before the confirmed
                // switch. It can no longer enter the cancelled Goal turn, so
                // convert it to queued user work; the target-mode retag below
                // makes it run under the Behavior the user selected.
                self.flush_stranded_interjections().await;
                self.cancel_running_task(true, false, false, Some("behavior_switch".to_string()))
                    .await;
                let changed = self.goal_tracker.lock().pause(GoalPauseReason::User);
                if changed {
                    let current_tokens = self.chat_state_handle.get_total_tokens().await as i64;
                    let (tokens_used, finished) = self.goal_tokens(current_tokens);
                    self.goal_notify_sender().emit_goal_updated(
                        &mut self.goal_tracker.lock(),
                        tokens_used,
                        finished,
                    );
                }
            }
            if deep_research_active && let Some(run_id) = owned_deep_research_run.as_deref() {
                self.cancel_deep_research_with_report(run_id).await;
            }
        }

        if mode == SessionMode::Plan && self.state.lock().await.running_task.is_some() {
            self.cancel_running_task(true, false, false, Some("behavior_switch".to_string()))
                .await;
        }

        // A completed Goal is a retained display receipt, not an active
        // Behavior. Normal therefore leaves it visible. An explicit switch to
        // any special Behavior starts a new control context and atomically
        // retires that receipt so it cannot override the selected mode in the
        // pager or reappear after session reload.
        let clear_completed_goal = mode != SessionMode::Default
            && self.goal_tracker.lock().status()
                == Some(crate::session::goal_tracker::GoalStatus::Complete);
        if clear_completed_goal {
            let (respond_to, deleted) = tokio::sync::oneshot::channel();
            if self
                .notifications
                .persistence_tx
                .send(
                    crate::session::persistence::PersistenceMsg::DeleteGoalModeState { respond_to },
                )
                .is_err()
                || !matches!(deleted.await, Ok(Ok(())))
            {
                let message = format!(
                    "Could not durably retire the completed Goal; {} Behavior was not changed.",
                    mode.display_label()
                );
                self.enqueue_current_mode_update_with_behavior_change(
                    acp::SessionModeId::new(
                        session_mode_from_prompt_mode(previous_prompt_mode).as_id(),
                    ),
                    serde_json::json!({ "status": "rejected", "message": message }),
                );
                return BehaviorChangeOutcome::Rejected { message };
            }
            self.goal_tracker.lock().clear();
            self.retire_goal_plan_scope().await;
            self.goal_continuation_streak
                .store(0, std::sync::atomic::Ordering::Relaxed);
            self.goal_blocked_streak
                .store(0, std::sync::atomic::Ordering::Relaxed);
            self.goal_turn_task_ids.lock().clear();
            self.subagent_token_records.lock().clear();
            self.clear_pending_classifier_completions();
            self.send_grow_notification(crate::session::goal_orchestrator::build_goal_cleared())
                .await;
        }
        if target_behavior == Some(tool_types::BehaviorId::Goal)
            && self.goal_tracker.lock().snapshot().is_some()
        {
            self.enter_goal_plan_scope().await;
        } else if current_behavior == Some(tool_types::BehaviorId::Goal)
            && target_behavior != Some(tool_types::BehaviorId::Goal)
            && self.goal_tracker.lock().snapshot().is_some()
        {
            self.deactivate_goal_plan_scope().await;
        }
        *self.current_prompt_mode.lock() = prompt_mode;
        self.behavior.lock().select_behavior(prompt_mode.behavior());
        if current_behavior == Some(tool_types::BehaviorId::Goal)
            && target_behavior != Some(tool_types::BehaviorId::Goal)
        {
            self.retag_queued_goal_user_prompts(prompt_mode).await;
        }
        self.persist_behavior_state();
        self.enqueue_current_mode_update(session_mode_id.clone());
        BehaviorChangeOutcome::Applied
    }
    /// Inject active Behavior guidance into the conversation.
    ///
    /// Called once per turn before the user's message. Drafting and amending
    /// use the mutable candidate artifact; executing uses the frozen approved
    /// artifact. The phase itself is the edit gate—there is no hidden pending
    /// or re-entry state.
    pub(super) async fn inject_behavior_reminders(&self) {
        use crate::session::behavior::{
            BehaviorState, PlanPhase, plan_execution_reminder_template,
            plan_mode_reminder_full_template, plan_mode_reminder_sparse_template,
        };
        let push_reminder = |this: &Self, content: &str| {
            this.push_system_reminder_with_tag(content, this.reminder_wrapper_tag());
        };
        let plan = {
            let controller = self.behavior.lock();
            match controller.state() {
                BehaviorState::Plan(PlanPhase::Executing) => Some((
                    plan_execution_reminder_template(),
                    controller.approved_plan_file_path().to_path_buf(),
                )),
                BehaviorState::Plan(
                    PlanPhase::Drafting | PlanPhase::AwaitingApproval | PlanPhase::Amending,
                ) => {
                    let template = if controller.should_use_full_reminder() {
                        plan_mode_reminder_full_template()
                    } else {
                        plan_mode_reminder_sparse_template()
                    };
                    Some((template, controller.plan_file_path().to_path_buf()))
                }
                _ => None,
            }
        };
        let Some((template, plan_path)) = plan else {
            return;
        };
        let plan_has_content = crate::session::behavior::plan_file_has_content(&plan_path).await;
        if let Some(rendered) = self
            .render_plan_template(template, &plan_path, plan_has_content)
            .await
        {
            push_reminder(self, &rendered);
            self.behavior.lock().record_reminder_injected();
            self.persist_behavior_state();
        }
    }
    /// Render a plan mode template via the tool bridge's `TemplateRenderer`.
    ///
    /// The host reads the session artifact and injects its content directly;
    /// the Agent does not need Read or Edit access to that storage location.
    pub(super) async fn render_plan_template(
        &self,
        template: &str,
        plan_path: &std::path::Path,
        plan_has_content: bool,
    ) -> Option<String> {
        let plan_content = if plan_has_content {
            tokio::fs::read_to_string(plan_path)
                .await
                .ok()
                .map(|content| content.trim().to_owned())
                .filter(|content| !content.is_empty())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let extra = serde_json::json!({ "plan_content": plan_content });
        self.agent
            .borrow()
            .tool_bridge()
            .render_prompt(template, &extra)
            .await
    }
    /// Persist the current Behavior state after each transition.
    pub(super) fn persist_behavior_state(&self) {
        let snapshot = self.behavior.lock().snapshot();
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::BehaviorState(snapshot));
    }
}

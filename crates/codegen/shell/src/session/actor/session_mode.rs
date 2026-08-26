//! Session Behavior transitions, reminders, and persistence.
use super::*;
use crate::session::behavior::BehaviorChangeOutcome;

/// Plan is a frozen, human-approved execution protocol. The Workflow
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
    pub(super) fn workflow_behavior_context(&self) -> Result<String, acp::Error> {
        let workspace = crate::session::workflow::workspace::WorkflowWorkspace::open_in_session(
            &self.session_directory,
            std::path::Path::new(self.session_info.cwd.as_str()),
        )
        .map_err(|error| {
            acp::Error::internal_error()
                .data(format!("active Workflow workspace is unavailable: {error}"))
        })?;
        Ok(workspace.compact_context(std::path::Path::new(self.session_info.cwd.as_str())))
    }

    pub(super) async fn behavior_capability_support(&self) -> (bool, bool, bool) {
        let bridge = self.agent.borrow().tool_bridge().clone();
        let tool_names: Vec<String> = bridge
            .tool_definitions_builtins_only()
            .await
            .into_iter()
            .map(|definition| definition.function.name)
            .collect();
        let plan_supported = bridge
            .tool_for_kind(tools::types::tool::ToolKind::PlanControl)
            .await
            .is_some();
        let workflow_supported = bridge
            .tool_for_kind(tools::types::tool::ToolKind::Workflow)
            .await
            .is_some();
        let goal_supported =
            super::goal_support::goal_runtime_available_from_tools(self.goal_enabled, &tool_names);
        (plan_supported, workflow_supported, goal_supported)
    }

    pub(super) fn behavior_availability_from_tracker(
        &self,
        workflow_tracker: &crate::session::workflow::tracker::WorkflowTracker,
        (plan_supported, workflow_supported, goal_supported): (bool, bool, bool),
    ) -> tool_types::BehaviorAvailability {
        use crate::session::behavior::BehaviorSwitchFacts;
        use tool_types::{BehaviorAvailability, BehaviorId};

        let current = self.behavior.lock().behavior();
        let goal_status = self.goal_tracker.lock().status();
        let active_goal = goal_status == Some(crate::session::goal_tracker::GoalStatus::Active);
        let public_workflow_active = workflow_tracker.has_active_run();
        let source_owned_work_active = current == BehaviorId::Plan
            || (current == BehaviorId::Workflow && public_workflow_active);
        let controller = self.behavior.lock();
        let choices = [
            BehaviorId::Normal,
            BehaviorId::Clarify,
            BehaviorId::Plan,
            BehaviorId::Workflow,
            BehaviorId::Goal,
        ]
        .into_iter()
        .map(|target| {
            let unavailable_reason = match target {
                BehaviorId::Plan if !plan_supported => Some(
                    "Plan behavior is unavailable because PlanControl is not registered."
                        .to_string(),
                ),
                BehaviorId::Workflow if !workflow_supported => {
                    Some("Workflow behavior is unavailable in this session.".to_string())
                }
                BehaviorId::Goal if !goal_supported => {
                    Some("Goal behavior is unavailable in this session.".to_string())
                }
                BehaviorId::Goal
                    if goal_status.is_some_and(|status| {
                        status != crate::session::goal_tracker::GoalStatus::Active
                    }) => Some(match goal_status {
                        Some(crate::session::goal_tracker::GoalStatus::Complete) =>
                            "The saved Goal is complete. Use /goal edit to reactivate it, or /goal clear before starting another Goal."
                                .to_string(),
                        Some(crate::session::goal_tracker::GoalStatus::BudgetLimited) =>
                            "The saved Goal is budget-limited. Raise or remove its budget, then restart it."
                                .to_string(),
                        _ =>
                            "The saved Goal is stopped. Use /goal restart to reactivate it before selecting Goal Behavior."
                                .to_string(),
                    }),
                _ => None,
            };
            controller.switch_availability(
                target,
                &BehaviorSwitchFacts {
                    unavailable_reason,
                    active_goal,
                    public_workflow_active,
                    source_owned_work_active,
                },
            )
        })
        .collect();
        BehaviorAvailability { current, choices }
    }

    /// Capture the Shell-authoritative Behavior choice projection from one
    /// control-plane snapshot. Pager clients render this value but every
    /// transition is revalidated by calling this method again.
    pub(super) async fn behavior_availability_projection(
        &self,
    ) -> tool_types::BehaviorAvailability {
        let support = self.behavior_capability_support().await;
        let workflow_tracker = self.workflow_tracker().await;
        let workflow_tracker = workflow_tracker.lock();
        self.behavior_availability_from_tracker(&workflow_tracker, support)
    }

    pub(super) fn apply_behavior_to_snapshot(&self, snapshot: &mut TurnDeltaSnapshot) {
        let behavior = self.turn_behavior.lock().to_string();
        snapshot.admitted_behavior = Some(behavior.clone());
        snapshot.completed_behavior = Some(behavior);
    }
    pub(super) async fn request_behavior_change(
        &self,
        session_mode_id: acp::SessionModeId,
    ) -> Result<BehaviorChangeOutcome, acp::Error> {
        self.request_behavior_change_with_admission(session_mode_id, false)
            .await
    }

    /// Goal entry is an out-of-band control operation: from Normal/Clarify it
    /// may commit while their regular turn is still active, after which the
    /// command-plane caller cancels that exact foreground turn. The generic
    /// Behavior picker remains idle-only, and Plan/Workflow ownership rules
    /// are unchanged.
    pub(super) async fn request_goal_behavior_entry(
        &self,
    ) -> Result<BehaviorChangeOutcome, acp::Error> {
        self.request_behavior_change_with_admission(acp::SessionModeId::new("goal"), true)
            .await
    }

    async fn request_behavior_change_with_admission(
        &self,
        session_mode_id: acp::SessionModeId,
        allow_out_of_band_goal_entry: bool,
    ) -> Result<BehaviorChangeOutcome, acp::Error> {
        use crate::session::behavior::{BehaviorEffect, BehaviorSwitchFacts};
        use tool_types::BehaviorAvailabilityDisposition;
        use tools::types::BehaviorId;

        // Behavior admission and next-step Agent/model controls share one
        // linearization boundary. An Agent candidate is built asynchronously
        // after leaving the pending deque, so inspecting that deque alone
        // cannot prove there is no in-flight route transition.
        let step_control = self.step_control_gate.lock().await;

        // Workflow launch and special-Behavior admission share the manager
        // lock as their linearization point. Every public launch rechecks the
        // selected Behavior while holding this lock; keeping it through the
        // durable control commit and in-memory selection prevents both races:
        // a run appearing after the conflict snapshot, or a launch admitted
        // against the old Behavior after the new one was committed.
        let support = self.behavior_capability_support().await;
        let mut workflow_admission = self.workflow_manager.lock().await;
        let availability = {
            let workflow_tracker = workflow_admission.tracker();
            let workflow_tracker = workflow_tracker.lock();
            self.behavior_availability_from_tracker(&workflow_tracker, support)
        };
        let previous_behavior = availability.current;
        let Some(mode) = BehaviorId::try_from_id(session_mode_id.0.as_ref()) else {
            let message = format!(
                "Unknown Behavior id: {}. Agent Roles must be selected through the Agent interface.",
                session_mode_id.0
            );
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(previous_behavior.as_id()),
                serde_json::json!({
                    "status": "rejected",
                    "source": previous_behavior.as_id(),
                    "target": session_mode_id.0.as_ref(),
                    "message": message,
                }),
            );
            return Ok(BehaviorChangeOutcome::Rejected { message });
        };
        let Some(choice) = availability.choice(mode) else {
            let message = format!("{} behavior is unavailable.", mode.display_label());
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(previous_behavior.as_id()),
                serde_json::json!({
                    "status": "rejected",
                    "source": previous_behavior.as_id(),
                    "target": mode.as_id(),
                    "message": message,
                }),
            );
            return Ok(BehaviorChangeOutcome::Rejected { message });
        };

        // Confirmation is durable control-plane intent, not a side effect of
        // an inadmissible request. Reject a busy foreground before touching
        // the coordinator's confirmation latch.
        let mut foreground_admission = self.state.lock().await;
        let host_control_turn = foreground_admission
            .foreground
            .regular()
            .is_some_and(|turn| turn.origin == crate::session::PromptOrigin::HostCommand);
        let goal_control_turn = allow_out_of_band_goal_entry
            && mode == BehaviorId::Goal
            && matches!(previous_behavior, BehaviorId::Normal | BehaviorId::Clarify)
            && foreground_admission.foreground.regular().is_some();
        if !foreground_admission.pending_step_controls.is_empty() {
            let message = format!(
                "{} Behavior cannot overtake an earlier model or Agent control. Retry after the current step reaches its boundary.",
                mode.display_label()
            );
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(previous_behavior.as_id()),
                serde_json::json!({
                    "status": "rejected",
                    "source": previous_behavior.as_id(),
                    "target": mode.as_id(),
                    "message": message,
                }),
            );
            return Ok(BehaviorChangeOutcome::Rejected { message });
        }
        if !foreground_admission.foreground.is_idle() && !host_control_turn && !goal_control_turn {
            let message = format!(
                "Stop the active foreground work before selecting {} Behavior.",
                mode.display_label()
            );
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(previous_behavior.as_id()),
                serde_json::json!({
                    "status": "rejected",
                    "source": previous_behavior.as_id(),
                    "target": mode.as_id(),
                    "message": message,
                }),
            );
            return Ok(BehaviorChangeOutcome::Rejected { message });
        }
        let decision = self.behavior.lock().decide_switch(
            mode,
            BehaviorSwitchFacts {
                unavailable_reason: (choice.disposition
                    == BehaviorAvailabilityDisposition::Unavailable)
                    .then(|| {
                        choice.reason.clone().unwrap_or_else(|| {
                            format!("{} behavior is unavailable.", mode.display_label())
                        })
                    }),
                active_goal: false,
                public_workflow_active: false,
                source_owned_work_active: choice.disposition
                    == BehaviorAvailabilityDisposition::ConfirmationRequired,
            },
            std::time::Duration::from_secs(8),
        );
        if !matches!(&decision.outcome, BehaviorChangeOutcome::Applied) {
            let meta = match &decision.outcome {
                BehaviorChangeOutcome::ConfirmationRequired {
                    message,
                    remaining_ms,
                } => serde_json::json!({
                    "status": "confirmation_required",
                    "source": previous_behavior.as_id(),
                    "target": mode.as_id(),
                    "message": message,
                    "remainingMs": remaining_ms,
                }),
                BehaviorChangeOutcome::Rejected { message } => {
                    serde_json::json!({
                        "status": "rejected",
                        "source": previous_behavior.as_id(),
                        "target": mode.as_id(),
                        "message": message,
                    })
                }
                BehaviorChangeOutcome::Applied => unreachable!(),
            };
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(previous_behavior.as_id()),
                meta,
            );
            return Ok(decision.outcome);
        }

        if !decision.effects.is_empty() {
            let persisted_goal = self.goal_tracker.lock().snapshot().cloned();
            if let Err(error) = self
                .persist_behavior_transition_durably(
                    crate::session::behavior::BehaviorSnapshot::selected(mode),
                    persisted_goal,
                )
                .await
            {
                let message = format!(
                    "Could not durably select {} Behavior.",
                    mode.display_label()
                );
                self.enqueue_current_mode_update_with_behavior_change(
                    acp::SessionModeId::new(previous_behavior.as_id()),
                    serde_json::json!({
                        "status": "rejected",
                        "source": previous_behavior.as_id(),
                        "target": mode.as_id(),
                        "message": message,
                    }),
                );
                return Err(crate::session::commands::fatal_turn_boundary_error(
                    "Behavior control",
                    error.to_string(),
                ));
            }
        }

        // Publish the new ownership identity before either admission lock is
        // released. The next foreground therefore captures exactly the
        // Behavior whose Control context is already durable in Surface.
        if let Some(target) = decision.effects.iter().find_map(|effect| match effect {
            BehaviorEffect::Select(target) => Some(*target),
            _ => None,
        }) {
            self.behavior.lock().select_behavior(target);
            if target != previous_behavior && foreground_admission.foreground.regular().is_some() {
                foreground_admission.terminal_preemption_pending = true;
            }
        }
        drop(foreground_admission);
        drop(workflow_admission);
        drop(step_control);

        for effect in decision.effects {
            match effect {
                BehaviorEffect::CancelSourceForeground(source) => {
                    let source_owns_foreground =
                        self.state.lock().await.foreground.regular().is_some()
                            && *self.turn_behavior.lock() == source;
                    if source_owns_foreground {
                        self.cancel_running_task(
                            true,
                            false,
                            false,
                            Some("behavior_switch".to_string()),
                        )
                        .await?;
                    }
                }
                BehaviorEffect::Select(_) => {}
            }
        }
        self.enqueue_current_mode_update(session_mode_id.clone());
        self.send_available_commands_update().await;
        Ok(BehaviorChangeOutcome::Applied)
    }
    /// Inject active Behavior guidance into the conversation.
    ///
    /// Called once per turn before the user's message. Drafting and amending
    /// use the mutable candidate artifact; executing uses the frozen approved
    /// artifact. The phase itself is the edit gate—there is no hidden pending
    /// or re-entry state.
    pub(super) async fn inject_behavior_reminders(&self) -> Result<(), acp::Error> {
        use crate::session::behavior::{
            BehaviorState, PlanPhase, plan_execution_reminder_template,
            plan_mode_reminder_full_template, plan_mode_reminder_sparse_template,
        };
        let admitted = *self.turn_behavior.lock();
        if admitted == tool_types::BehaviorId::Workflow {
            let context = self.workflow_behavior_context()?;
            self.push_system_reminder_with_tag(&context, self.reminder_wrapper_tag());
            return Ok(());
        }
        if admitted != tool_types::BehaviorId::Plan {
            return Ok(());
        }
        let push_reminder = |this: &Self, content: &str| {
            this.push_system_reminder_with_tag(content, this.reminder_wrapper_tag());
        };
        let plan = {
            let controller = self.behavior.lock();
            match controller.state() {
                BehaviorState::Plan(PlanPhase::Executing) => Some((
                    plan_execution_reminder_template(),
                    controller.plan_artifact_hash().map(str::to_owned),
                )),
                BehaviorState::Plan(
                    PlanPhase::Drafting | PlanPhase::AwaitingApproval | PlanPhase::Amending,
                ) => {
                    let template = if controller.should_use_full_reminder() {
                        plan_mode_reminder_full_template()
                    } else {
                        plan_mode_reminder_sparse_template()
                    };
                    Some((template, controller.plan_artifact_hash().map(str::to_owned)))
                }
                _ => None,
            }
        };
        let Some((template, artifact_hash)) = plan else {
            return Ok(());
        };
        let plan_content = match artifact_hash {
            Some(hash) => {
                let session = self.session_directory.clone();
                tokio::task::spawn_blocking(move || {
                    crate::session::behavior::read_plan_artifact(&session, &hash)
                })
                .await
                .map_err(|error| {
                    acp::Error::internal_error()
                        .data(format!("failed to join Plan artifact read: {error}"))
                })?
                .map_err(|error| {
                    acp::Error::internal_error()
                        .data(format!("active Plan artifact failed validation: {error}"))
                })?
            }
            None => String::new(),
        };
        if let Some(rendered) = self.render_plan_template(template, &plan_content).await {
            push_reminder(self, &rendered);
            self.behavior.lock().record_reminder_injected();
            self.record_control_snapshot();
        }
        Ok(())
    }
    /// Render a plan mode template via the tool bridge's `TemplateRenderer`.
    ///
    /// The host reads the session artifact and injects its content directly;
    /// the Agent does not need Read or Edit access to that storage location.
    pub(super) async fn render_plan_template(
        &self,
        template: &str,
        plan_content: &str,
    ) -> Option<String> {
        let extra = serde_json::json!({ "plan_content": plan_content.trim() });
        self.agent
            .borrow()
            .tool_bridge()
            .render_prompt(template, &extra)
            .await
    }
    /// Append a buffered control snapshot for best-effort bookkeeping changes.
    /// User-visible transitions use the durable transaction method below.
    pub(super) fn record_control_snapshot(&self) {
        let revision = self
            .control_revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .saturating_add(1);
        let snapshot = crate::session::control::SessionControlSnapshot::new(
            revision,
            self.agent.borrow().definition().selector_identity(),
            self.behavior.lock().snapshot(),
            self.goal_tracker.lock().snapshot().cloned(),
        );
        match snapshot.timeline_kind() {
            Ok(kind) => self.chat_state_handle.record_timeline_event(kind),
            Err(error) => tracing::error!(%error, "failed to encode session control event"),
        }
    }

    /// Commit a Behavior transition through the same atomic control snapshot
    /// used by Goal. A failed persistence barrier restores the in-memory
    /// coordinator, so callers never publish a transition that cannot survive
    /// reconnect.
    pub(super) async fn commit_behavior_mutation_or_restore(
        &self,
        previous: crate::session::behavior::BehaviorSnapshot,
    ) -> Result<(), String> {
        let next = self.behavior.lock().snapshot();
        let goal = self.goal_tracker.lock().snapshot().cloned();
        let selection_changed = previous.behavior() != next.behavior();
        let persisted = if selection_changed {
            self.persist_behavior_transition_durably(next, goal).await
        } else {
            self.persist_control_snapshot_durably(next, goal).await
        };
        if let Err(error) = persisted {
            *self.behavior.lock() =
                crate::session::behavior::BehaviorCoordinator::from_snapshot(previous);
            return Err(format!("Behavior control state was not persisted: {error}"));
        }
        Ok(())
    }
}

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
    async fn behavior_capability_support(&self) -> (bool, bool, bool) {
        let bridge = self.agent.borrow().tool_bridge().clone();
        let tool_names: Vec<String> = bridge
            .tool_definitions()
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
            super::goal_support::goal_slash_and_harness_available(self.goal_enabled, &tool_names);
        (plan_supported, workflow_supported, goal_supported)
    }

    fn behavior_availability_from_tracker(
        &self,
        workflow_tracker: &crate::session::workflow::tracker::WorkflowTracker,
        (plan_supported, workflow_supported, goal_supported): (bool, bool, bool),
    ) -> tool_types::BehaviorAvailability {
        use crate::session::behavior::BehaviorSwitchFacts;
        use tool_types::{BehaviorAvailability, BehaviorId};

        let current = self.behavior.lock().behavior();
        let unfinished_goal = self
            .goal_tracker
            .lock()
            .status()
            .is_some_and(|status| status != crate::session::goal_tracker::GoalStatus::Complete);
        let owned_deep_research_run = self
            .behavior
            .lock()
            .deep_research_run_id()
            .map(str::to_owned);
        let public_workflow_active = workflow_tracker.has_active_public_run();
        let deep_research_active = current == BehaviorId::DeepResearch
            && owned_deep_research_run.as_deref().is_some_and(|run_id| {
                workflow_tracker
                    .get(run_id)
                    .is_some_and(|run| !run.status.is_terminal())
            });
        let source_owned_work_active = current == BehaviorId::Plan
            || deep_research_active
            || (current == BehaviorId::Workflow && public_workflow_active);
        let controller = self.behavior.lock();
        let choices = [
            BehaviorId::Normal,
            BehaviorId::Clarify,
            BehaviorId::Plan,
            BehaviorId::Workflow,
            BehaviorId::DeepResearch,
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
                BehaviorId::DeepResearch if !self.background_workflows_enabled => Some(
                    "Deep Research behavior requires the background Workflow runtime.".to_string(),
                ),
                BehaviorId::Goal if !goal_supported => {
                    Some("Goal behavior is unavailable in this session.".to_string())
                }
                _ => None,
            };
            controller.switch_availability(
                target,
                &BehaviorSwitchFacts {
                    unavailable_reason,
                    unfinished_goal,
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

    /// Synchronize the selected primary-session Behavior into the fixed system
    /// prompt layer: Mandatory Core → Audience → Role → Behavior → Runtime.
    pub(super) async fn sync_active_behavior_prompt(&self, admitted: tool_types::BehaviorId) {
        use crate::session::behavior::{
            clarify_reminder_template, deep_research_reminder_template, goal_reminder_template,
            plan_behavior_template, workflow_reminder_template,
        };
        let instructions = match admitted {
            tool_types::BehaviorId::Normal => None,
            tool_types::BehaviorId::Clarify => Some(clarify_reminder_template()),
            tool_types::BehaviorId::Plan => Some(plan_behavior_template()),
            tool_types::BehaviorId::Workflow => Some(workflow_reminder_template()),
            tool_types::BehaviorId::DeepResearch => Some(deep_research_reminder_template()),
            tool_types::BehaviorId::Goal => Some(goal_reminder_template()),
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

    pub(super) fn apply_behavior_to_snapshot(&self, snapshot: &mut TurnDeltaSnapshot) {
        let behavior = self.turn_behavior.lock().to_string();
        snapshot.admitted_behavior = Some(behavior.clone());
        snapshot.completed_behavior = Some(behavior);
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
        use crate::session::behavior::{BehaviorEffect, BehaviorSwitchFacts};
        use tool_types::BehaviorAvailabilityDisposition;
        use tools::types::BehaviorId;

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
                serde_json::json!({ "status": "rejected", "message": message }),
            );
            return BehaviorChangeOutcome::Rejected { message };
        };
        let Some(choice) = availability.choice(mode) else {
            let message = format!("{} behavior is unavailable.", mode.display_label());
            return BehaviorChangeOutcome::Rejected { message };
        };
        let owned_deep_research_run = self
            .behavior
            .lock()
            .deep_research_run_id()
            .map(str::to_owned);
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
                unfinished_goal: false,
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
                    serde_json::json!({ "status": "rejected", "message": message })
                }
                BehaviorChangeOutcome::Applied => unreachable!(),
            };
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(previous_behavior.as_id()),
                meta,
            );
            return decision.outcome;
        }

        if !decision.effects.is_empty() {
            let persisted_goal = self.goal_tracker.lock().snapshot().cloned();
            if self
                .persist_control_snapshot_durably(
                    crate::session::behavior::BehaviorSnapshot::selected(mode),
                    persisted_goal,
                )
                .await
                .is_err()
            {
                let message = format!(
                    "Could not durably select {} Behavior; no runtime was interrupted.",
                    mode.display_label()
                );
                self.enqueue_current_mode_update_with_behavior_change(
                    acp::SessionModeId::new(previous_behavior.as_id()),
                    serde_json::json!({ "status": "rejected", "message": message }),
                );
                return BehaviorChangeOutcome::Rejected { message };
            }
        }

        // Deep Research cancellation is part of the same ownership transfer:
        // remove the owned run before releasing Workflow admission so it
        // cannot briefly become an unowned public run or race a new launch.
        let cancelled_deep_research_report = if decision
            .effects
            .contains(&BehaviorEffect::CancelDeepResearchRun)
        {
            owned_deep_research_run.as_deref().map(|run_id| {
                let tracker = workflow_admission.tracker();
                let query = tracker
                    .lock()
                    .get(run_id)
                    .map(|run| run.objective.clone())
                    .unwrap_or_default();
                workflow_admission.cancel(run_id);
                super::workflow_run::deep_research_terminal_report(
                    &query,
                    &workflow::WorkflowOutcome::Cancelled,
                    None,
                )
            })
        } else {
            None
        };

        // Publish the new ownership identity before releasing Workflow
        // admission. The admitted foreground keeps its own immutable
        // `turn_behavior`, so this does not mutate a running turn's policy.
        if let Some(target) = decision.effects.iter().find_map(|effect| match effect {
            BehaviorEffect::Select(target) => Some(*target),
            _ => None,
        }) {
            self.behavior.lock().select_behavior(target);
        }
        drop(workflow_admission);

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
                        .await;
                    }
                }
                BehaviorEffect::CancelDeepResearchRun => {
                    if let Some(report) = cancelled_deep_research_report.as_deref() {
                        self.send_host_turn_slash_command_output(report).await;
                    }
                }
                BehaviorEffect::Select(_) => {}
            }
        }
        self.enqueue_current_mode_update(session_mode_id.clone());
        self.send_available_commands_update().await;
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
        let admitted = *self.turn_behavior.lock();
        if admitted == tool_types::BehaviorId::Workflow {
            let session_dir = crate::session::persistence::session_dir(&self.session_info);
            let context = crate::session::workflow::workspace::WorkflowWorkspace::open(
                &session_dir,
                std::path::Path::new(self.session_info.cwd.as_str()),
            )
            .map(|workspace| {
                workspace.compact_context(std::path::Path::new(self.session_info.cwd.as_str()))
            })
            .unwrap_or_else(|error| format!("Workflow workspace unavailable: {error}"));
            self.push_system_reminder_with_tag(&context, self.reminder_wrapper_tag());
            return;
        }
        if admitted != tool_types::BehaviorId::Plan {
            return;
        }
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
        let revision = self
            .control_revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .saturating_add(1);
        let snapshot = crate::session::control::SessionControlSnapshot::new(
            revision,
            self.behavior.lock().snapshot(),
            self.goal_tracker.lock().snapshot().cloned(),
        );
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::SessionControl(snapshot));
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
        if let Err(error) = self.persist_control_snapshot_durably(next, goal).await {
            let session_dir = crate::session::persistence::session_dir(&self.session_info);
            *self.behavior.lock() =
                crate::session::behavior::BehaviorCoordinator::from_snapshot(session_dir, previous);
            return Err(format!("Behavior control state was not persisted: {error}"));
        }
        Ok(())
    }
}

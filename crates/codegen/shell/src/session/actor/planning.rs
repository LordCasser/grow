//! Session-owned local planning. Tool Resources are read-only projections.

use super::*;
use tool_types::planning::{
    DependencyKind, LocalPlan, PlanLink, PlanPriority, PlanStep, PlanStepStatus, PlanUpdateMode,
    PlanWait, ReplanOutcome, ReplanProgress, ReviewDecision, WaitCondition,
};
use tools::implementations::grow_build::todo::runtime::{
    LocalPlanProjection, PlanningAction, PlanningCommand,
};
use tools::implementations::grow_build::todo::{TodoPriority, TodoStatus, TodoWriteInput};

const PLANNING_DELIVERY_INITIAL_BACKOFF: Duration = Duration::from_millis(100);
const PLANNING_DELIVERY_MAX_BACKOFF: Duration = Duration::from_secs(2);

pub(crate) enum PlannedDependenciesOutcome {
    NotWaiting,
    ContinueTurn,
    YieldGoalForFifo,
}

impl SessionActor {
    /// Request-only, bounded view of the latest owner facts. It goes through
    /// normal request construction, so token accounting includes the view.
    pub(crate) async fn local_planning_context(&self) -> Result<Option<String>, String> {
        let Some(snapshot) = self.chat_state_handle.planning_state().await? else {
            return Ok(None);
        };
        let plan = snapshot.plan;
        let steps: Vec<_> = plan
            .steps
            .iter()
            .filter(|step| !step.removed)
            .take(12)
            .map(|step| {
                json!({
                    "id":step.id,"status":step.status,"priority":step.priority,
                    "content":step.content.chars().take(256).collect::<String>(),
                })
            })
            .collect();
        let links: Vec<_> = plan.links.iter().rev().take(12).map(|link| json!({
            "task_id":link.id,"step_id":link.parent_step_id,"child_session_id":link.child_session_id,
            "terminal":link.terminal,"required":link.required,"result_ref":link.result_id,
            "pending_source_revision":link.pending_source_revision,
        })).collect();
        let replans: Vec<_> = plan.replans.iter().filter(|change| !matches!(change.progress, ReplanProgress::Handled{..}|ReplanProgress::Unapplied{..}))
            .take(12).map(|change| json!({"id":change.id,"source_revision":change.source_revision,"progress":change.progress})).collect();
        Ok(Some(format!(
            "Current local work (guidance, not permission). Use todo_write to inspect or change the complete local plan.\n{}",
            json!({
                "session_id":plan.session_id,"scope_id":plan.scope_id,"revision":plan.revision,
                "timeline_seq":snapshot.seq,"steps":steps,"step_count":plan.steps.len(),
                "links":links,"link_count":plan.links.len(),"pending_replans":replans,
                "wait_ids":plan.waits.iter().take(12).map(|wait| &wait.id).collect::<Vec<_>>(),
            })
        )))
    }
    pub(crate) async fn accept_planning_task_result(
        &self,
        task_id: &str,
        expected_parent_step_id: Option<&str>,
        result_ref: Option<&chat_state::TimelineRangeRef>,
    ) -> Result<(), String> {
        let _gate = self.step_control_gate.lock().await;
        let Some(expected_parent_step_id) = expected_parent_step_id else {
            return Ok(());
        };
        let Some(snapshot) = self.chat_state_handle.planning_state().await? else {
            return Err("expected local plan association is unavailable".into());
        };
        let mut plan = snapshot.plan;
        if plan.session_id != self.session_info.id.0.as_ref() {
            return Err("expected local plan association belongs to a different session".into());
        }
        let Some(child_session_id) = plan
            .links
            .iter()
            .find(|link| link.id == task_id && link.parent_step_id == expected_parent_step_id)
            .map(|link| link.child_session_id.clone())
        else {
            return Err("expected local plan task/step association is missing".into());
        };
        let result_ref = result_ref.ok_or_else(|| {
            "planning result receipt is missing its canonical child reference".to_string()
        })?;
        if result_ref.timeline_id != child_session_id {
            return Err("planning result reference belongs to a different child session".into());
        }
        crate::agent::subagent::validate_planning_result_reference(
            self.session_info.id.0.as_ref(),
            task_id,
            &child_session_id,
            result_ref,
            &self.chat_state_handle,
        )
        .await?;
        let reference = Some(serde_json::to_string(result_ref).map_err(|error| error.to_string())?);
        let link = plan
            .links
            .iter_mut()
            .find(|link| link.id == task_id)
            .expect("the planning link was captured above");
        if link.terminal && link.result_id == reference {
            return Ok(());
        }
        link.result_id = reference;
        plan.mark_link_terminal(
            task_id,
            "execution ended before further parent updates could be confirmed",
        )
        .map_err(|error| format!("{error:?}"))?;
        self.commit_local_plan(Some(snapshot.seq), plan).await?;
        Ok(())
    }
    pub(crate) fn schedule_planning_delivery(self: &Arc<Self>) {
        self.planning_delivery_dirty.set(true);
        if self.planning_delivery.is_running() {
            return;
        }
        let Some(activity) = self.session_activities.try_start("planning_delivery") else {
            return;
        };
        let session = self.clone();
        self.planning_delivery
            .arm(tokio::task::spawn_local(async move {
                let _activity = activity;
                let mut acknowledged_handled = std::collections::HashSet::new();
                let mut attempt = 0usize;
                let mut backoff = PLANNING_DELIVERY_INITIAL_BACKOFF;
                loop {
                    attempt = attempt.saturating_add(1);
                    session.planning_delivery_dirty.set(false);
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                    if let Err(error) = session
                        .deliver_planning_changes(deadline, &mut acknowledged_handled)
                        .await
                    {
                        tracing::warn!(%error, attempt, "planning delivery remains recoverable");
                    }
                    let retryable = match session
                        .planning_delivery_needs_retry(&acknowledged_handled)
                        .await
                    {
                        Ok(retryable) => retryable,
                        Err(error) => {
                            tracing::warn!(%error, "could not inspect durable planning delivery state");
                            true
                        }
                    };
                    let rescheduled = session.planning_delivery_dirty.replace(false);
                    if !retryable && !rescheduled {
                        break;
                    }
                    tokio::select! {
                        _ = session.background_service_shutdown.cancelled() => break,
                        _ = tokio::time::sleep(backoff) => {}
                    }
                    backoff = Duration::from_millis(
                        (backoff.as_millis() as u64 * 2)
                            .min(PLANNING_DELIVERY_MAX_BACKOFF.as_millis() as u64),
                    );
                }
            }));
    }

    async fn planning_delivery_needs_retry(
        &self,
        acknowledged_handled: &std::collections::HashSet<String>,
    ) -> Result<bool, String> {
        let Some(snapshot) = self.chat_state_handle.planning_state().await? else {
            return Ok(false);
        };
        Ok(snapshot.plan.replans.iter().any(|change| {
            if change.child_session_id == self.session_info.id.0.as_ref() {
                matches!(change.progress, ReplanProgress::Handled { .. })
                    && !acknowledged_handled.contains(&change.id)
            } else {
                matches!(
                    change.progress,
                    ReplanProgress::Pending | ReplanProgress::Dispatching
                )
            }
        }))
    }

    async fn deliver_planning_changes(
        &self,
        deadline: tokio::time::Instant,
        acknowledged_handled: &mut std::collections::HashSet<String>,
    ) -> Result<(), String> {
        use tools::implementations::grow_build::task::interaction::AgentInteraction;
        let bridge = self.agent.borrow().tool_bridge().clone();
        let Some(backend) = bridge
            .read_resource::<tools::implementations::grow_build::task::backend::SubagentBackendResource>().await else { return Ok(()); };
        let Some(snapshot) = self.chat_state_handle.planning_state().await? else {
            return Ok(());
        };
        for mut change in snapshot.plan.replans {
            if !self.state.lock().await.execution_open() {
                break;
            }
            if change.child_session_id == self.session_info.id.0.as_ref() {
                if let ReplanProgress::Handled { outcome } = &change.progress
                    && !acknowledged_handled.contains(&change.id)
                {
                    let receipts = self
                        .chat_state_handle
                        .parent_message_receipts()
                        .await
                        .unwrap_or_default();
                    let reference = receipts.iter().find_map(|event| {
                        let chat_state::TimelineEventKind::Notification(
                            chat_state::NotificationEvent::Received { source, .. },
                        ) = &event.kind
                        else {
                            return None;
                        };
                        let (parent, message_id, _, _) = source.agent_message()?;
                        (message_id == replan_message_id(&change.id)).then(|| {
                            sampling_types::AgentMessageRef {
                                source_session_id: parent.to_owned(),
                                message_id: message_id.to_owned(),
                            }
                        })
                    });
                    if let Some(reply_to) = reference {
                        let cancellation = self.background_service_shutdown.child_token();
                        let response = tokio::select! {
                            _ = self.background_service_shutdown.cancelled() => {
                                cancellation.cancel();
                                break;
                            }
                            response = tokio::time::timeout_at(
                                deadline,
                                backend.backend().interact(
                                format!("handled-{}", blake3::hash(change.id.as_bytes()).to_hex()),
                                None,
                                AgentInteraction::ReplanHandled {
                                    change_id: change.id.clone(),
                                    outcome: outcome.clone(),
                                    reply_to,
                                },
                                cancellation.clone(),
                                ),
                            ) => response,
                        };
                        if matches!(response, Ok(Ok(ref result)) if result.status == "received") {
                            acknowledged_handled.insert(change.id.clone());
                        }
                    }
                }
                continue;
            }
            if !matches!(
                change.progress,
                ReplanProgress::Pending | ReplanProgress::Dispatching | ReplanProgress::Received
            ) {
                continue;
            }
            if matches!(change.progress, ReplanProgress::Pending) {
                let _gate = self.step_control_gate.lock().await;
                let Some(current) = self.chat_state_handle.planning_state().await? else {
                    continue;
                };
                let mut plan = current.plan;
                if plan
                    .advance_replan(&change.id, ReplanProgress::Dispatching)
                    .is_err()
                {
                    continue;
                }
                self.commit_local_plan(Some(current.seq), plan).await?;
                // The message must carry the state just made durable by the
                // parent owner, rather than the stale Pending snapshot used
                // to discover that this change needs dispatch.
                change.progress = ReplanProgress::Dispatching;
            }
            let cancellation = self.background_service_shutdown.child_token();
            let response = tokio::select! {
                _ = self.background_service_shutdown.cancelled() => {
                    cancellation.cancel();
                    break;
                }
                response = tokio::time::timeout_at(
                    deadline,
                    backend.backend().interact(
                    replan_message_id(&change.id),
                    Some(change.link_id.clone()),
                    AgentInteraction::Replan {
                        change: change.clone(),
                    },
                    cancellation.clone(),
                    ),
                ) => response,
            };
            if matches!(response, Ok(Ok(ref result)) if result.status == "received") {
                let _gate = self.step_control_gate.lock().await;
                let Some(current) = self.chat_state_handle.planning_state().await? else {
                    continue;
                };
                let mut plan = current.plan;
                if plan
                    .advance_replan(&change.id, ReplanProgress::Received)
                    .is_ok()
                {
                    self.commit_local_plan(Some(current.seq), plan).await?;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
        }
        Ok(())
    }

    pub(crate) async fn receive_replan_handled(
        &self,
        child_id: &str,
        id: &str,
        outcome: ReplanOutcome,
    ) -> Result<(), String> {
        let _gate = self.step_control_gate.lock().await;
        let Some(mut snapshot) = self.chat_state_handle.planning_state().await? else {
            return Err("planning owner is unavailable".into());
        };
        loop {
            let mut plan = snapshot.plan;
            let record = plan
                .replans
                .iter()
                .find(|change| change.id == id && change.child_session_id == child_id)
                .ok_or_else(|| "handling does not belong to this delegation".to_string())?;
            if record.progress
                == (ReplanProgress::Handled {
                    outcome: outcome.clone(),
                })
            {
                return Ok(());
            }
            // A Handled receipt also proves the child's earlier stages, but
            // each causal transition must remain durable on the parent. An
            // interrupted receipt retry continues from the last stored stage.
            let next = match record.progress {
                ReplanProgress::Pending | ReplanProgress::Dispatching => ReplanProgress::Received,
                ReplanProgress::Received => ReplanProgress::Consumed,
                _ => ReplanProgress::Handled {
                    outcome: outcome.clone(),
                },
            };
            plan.advance_replan(id, next)
                .map_err(|error| format!("{error:?}"))?;
            self.commit_local_plan(Some(snapshot.seq), plan).await?;
            snapshot = self
                .chat_state_handle
                .planning_state()
                .await?
                .ok_or_else(|| "planning owner is unavailable".to_string())?;
        }
    }
    pub(crate) async fn receive_plan_guidance(
        &self,
        parent_session_id: String,
        mut change: tool_types::ReplanChange,
    ) -> Result<String, crate::session::notification_inbox::AgentMessageDeliveryError> {
        use crate::session::notification_inbox::AgentMessageDeliveryError;
        if change.child_session_id != self.session_info.id.0.as_ref() {
            return Err(AgentMessageDeliveryError::rejected(
                "wrong_replan_target",
                "Replanning belongs to a different child",
            ));
        }
        change.progress = ReplanProgress::Received;
        let body = serde_json::to_string(&change).map_err(|error| {
            AgentMessageDeliveryError::rejected("invalid_replan", error.to_string())
        })?;
        let message_id = replan_message_id(&change.id);
        let receipt = self.receive_agent_message(parent_session_id, message_id, format!(
            "The delegating agent changed a linked TODO. Re-evaluate your own local plan at a safe boundary. Preserve valid completed work; acknowledge this source with todo_write HandleReplans, or record a reasoned no-change decision. Do not restart commands solely because the parent changed.\n{body}"
        ), true, None).await?;
        let _boundary = self.step_control_gate.lock().await;
        let snapshot = self
            .chat_state_handle
            .planning_state()
            .await
            .map_err(|error| {
                AgentMessageDeliveryError::unconfirmed("planning_unavailable", error)
            })?;
        let previous = snapshot.as_ref().map(|snapshot| snapshot.seq);
        let prompt_id = self
            .state
            .lock()
            .await
            .running_prompt_id()
            .unwrap_or("delegated")
            .to_owned();
        let mut plan = snapshot.map_or_else(
            || {
                LocalPlan::new(
                    self.session_info.id.0.to_string(),
                    format!("work:{prompt_id}"),
                )
            },
            |snapshot| snapshot.plan,
        );
        if plan.receive_replan(change).map_err(|error| {
            AgentMessageDeliveryError::rejected("replan_conflict", format!("{error:?}"))
        })? {
            self.commit_local_plan(previous, plan)
                .await
                .map_err(|error| {
                    AgentMessageDeliveryError::unconfirmed("planning_unconfirmed", error)
                })?;
        }
        Ok(receipt)
    }
    pub(crate) async fn hydrate_local_plan(&self) -> Result<(), String> {
        let bridge = self.agent.borrow().tool_bridge().clone();
        let plan = self.bind_local_planning_runtime(&bridge).await?;
        self.send_update(
            acp::SessionUpdate::Plan(
                crate::session::acp_conversion::acp_plan_update_from_local_plan(plan.as_ref()),
            ),
            None,
        )
        .await;
        Ok(())
    }

    /// Bind each harness to the session owner and its committed plan before
    /// activation. Replacing an Agent is not a plan commit or a UI update.
    pub(super) async fn bind_local_planning_runtime(
        &self,
        bridge: &tools::bridge::ToolBridge,
    ) -> Result<Option<LocalPlan>, String> {
        let snapshot = self.chat_state_handle.planning_state().await?;
        let plan = snapshot.as_ref().map(|snapshot| snapshot.plan.clone());
        let todo = plan.as_ref().map_or_else(
            tools::implementations::grow_build::todo::TodoState::default,
            tools::implementations::grow_build::todo::project_todo_state,
        );
        bridge
            .update_resource(
                tools::implementations::grow_build::todo::runtime::PlanningRuntimeHandle(
                    self.planning_command_tx.clone(),
                ),
            )
            .await;
        bridge
            .update_resource(LocalPlanProjection(plan.clone()))
            .await;
        bridge
            .update_resource(tools::types::resources::State(todo))
            .await;
        Ok(plan)
    }

    async fn commit_local_plan(
        &self,
        previous: Option<chat_state::EventSeq>,
        plan: LocalPlan,
    ) -> Result<LocalPlan, String> {
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                chat_state::timeline::PlanningEvent {
                    previous,
                    plan: Box::new(plan.clone()),
                },
            ))
            .await
            .map_err(|error| format!("planning commit unconfirmed: {error}"))?;
        self.hydrate_local_plan().await?;
        Ok(plan)
    }

    pub(crate) async fn handle_planning_command(self: &Arc<Self>, command: PlanningCommand) {
        let _boundary = self.step_control_gate.lock().await;
        match command {
            PlanningCommand::Write {
                prompt_id,
                input,
                respond_to,
            } => {
                let result = self.apply_local_plan_command(&prompt_id, input).await;
                let _ = respond_to.send(result);
            }
            PlanningCommand::Link {
                prompt_id,
                step_id,
                task_id,
                required,
                respond_to,
            } => {
                let result = async {
                    if !self
                        .state
                        .lock()
                        .await
                        .can_continue_regular_turn(&prompt_id)
                    {
                        return Err("delegation no longer belongs to an admitted turn".into());
                    }
                    let Some(snapshot) = self.chat_state_handle.planning_state().await? else {
                        return Err("create the local parent step before linking a task".into());
                    };
                    let mut plan = snapshot.plan;
                    if !plan
                        .steps
                        .iter()
                        .any(|step| step.id == step_id && !step.removed)
                    {
                        return Err("parent step is not in the current local work scope".into());
                    }
                    if task_id == self.session_info.id.0.as_ref()
                        || plan.links.iter().any(|link| link.id == task_id)
                    {
                        return Err("task identity is already bound".into());
                    }
                    plan.links.push(PlanLink {
                        id: task_id.clone(),
                        parent_step_id: step_id,
                        child_session_id: task_id,
                        terminal: false,
                        required,
                        pending_source_revision: None,
                        review_required_revision: plan.revision,
                        result_id: None,
                        dispatch_failure: None,
                    });
                    self.commit_local_plan(Some(snapshot.seq), plan).await
                }
                .await;
                let _ = respond_to.send(result);
            }
            PlanningCommand::Undispatched {
                task_id,
                reason,
                respond_to,
            } => {
                let result = async {
                    let snapshot =
                        self.chat_state_handle
                            .planning_state()
                            .await?
                            .ok_or_else(|| {
                                "planning link disappeared before dispatch failure was recorded"
                                    .to_owned()
                            })?;
                    let mut plan = snapshot.plan;
                    let changed = plan
                        .mark_link_undispatched(&task_id, reason)
                        .map_err(|error| format!("cannot record undispatched task: {error:?}"))?;
                    if changed {
                        self.commit_local_plan(Some(snapshot.seq), plan).await?;
                    }
                    Ok(())
                }
                .await;
                let _ = respond_to.send(result);
            }
        }
        drop(_boundary);
        self.schedule_planning_delivery();
    }

    async fn apply_local_plan_command(
        &self,
        prompt_id: &str,
        input: TodoWriteInput,
    ) -> Result<LocalPlan, String> {
        if !self.state.lock().await.can_continue_regular_turn(prompt_id) {
            return Err("planning command no longer belongs to an admitted turn".into());
        }
        let snapshot = self.chat_state_handle.planning_state().await?;
        let mut previous = snapshot.as_ref().map(|snapshot| snapshot.seq);
        let scope = self
            .goal_tracker
            .lock()
            .snapshot()
            .map(|goal| format!("goal:{}:{}", goal.goal_id, goal.definition_revision))
            .unwrap_or_else(|| format!("work:{prompt_id}"));
        let mut plan = snapshot.map_or_else(
            || LocalPlan::new(self.session_info.id.0.to_string(), scope),
            |snapshot| snapshot.plan,
        );
        let mut steps = Vec::with_capacity(input.todos.len());
        for update in input.todos {
            let existing = plan.steps.iter().find(|step| step.id == update.id);
            let content = update
                .content
                .clone()
                .or_else(|| existing.map(|step| step.content.clone()))
                .ok_or_else(|| format!("new TODO {} requires content", update.id))?;
            let status = update
                .status
                .map(plan_status)
                .or_else(|| existing.map(|step| step.status))
                .unwrap_or_default();
            let priority = update
                .priority
                .map(plan_priority)
                .or_else(|| existing.map(|step| step.priority))
                .unwrap_or_default();
            steps.push(PlanStep {
                id: update.id,
                content,
                status,
                priority,
                requirements: update
                    .requirements
                    .or_else(|| existing.and_then(|step| step.requirements.clone())),
                removed: update.removed.unwrap_or(false),
            });
        }
        // Persist model-context consumption before a command can also change
        // the plan revision and acknowledge the replan. Timeline validation
        // deliberately requires the durable Received -> Consumed -> Handled
        // progression; combining both transitions in one Planning event would
        // skip the auditable consumption fact.
        let mut replan_ids = Vec::new();
        for action in &input.actions {
            if let PlanningAction::HandleReplans {
                ids,
                no_change_reason,
            } = action
            {
                if ids.is_empty() {
                    return Err("handling requires source identities".into());
                }
                if no_change_reason
                    .as_ref()
                    .is_some_and(|reason| reason.trim().is_empty())
                {
                    return Err("handling a replan needs a non-empty no-change reason".into());
                }
                replan_ids.extend(ids.iter().cloned());
            }
        }
        if !replan_ids.is_empty() {
            let consumed = self
                .chat_state_handle
                .consumed_agent_messages(
                    replan_ids.iter().map(|id| replan_message_id(id)).collect(),
                )
                .await?;
            let mut advanced = false;
            for id in &replan_ids {
                let record = plan
                    .replans
                    .iter()
                    .find(|change| change.id == *id)
                    .ok_or_else(|| format!("replan {id} was not received"))?;
                if record.child_session_id != self.session_info.id.0.as_ref() {
                    return Err("only the receiving child may acknowledge replanning".into());
                }
                if !consumed.contains(&replan_message_id(id)) {
                    return Err("replanning guidance has not been consumed as model context".into());
                }
                match record.progress {
                    ReplanProgress::Received => {
                        plan.advance_replan(id, ReplanProgress::Consumed)
                            .map_err(|error| format!("{error:?}"))?;
                        advanced = true;
                    }
                    ReplanProgress::Consumed => {}
                    _ => return Err(format!("replan {id} is not awaiting handling")),
                }
            }
            if advanced {
                self.commit_local_plan(previous, plan).await?;
                let current = self
                    .chat_state_handle
                    .planning_state()
                    .await?
                    .ok_or_else(|| "consumed replan plan disappeared".to_string())?;
                previous = Some(current.seq);
                plan = current.plan;
            }
        }
        if !steps.is_empty() || !input.merge {
            plan.update_steps(
                if input.merge {
                    PlanUpdateMode::Merge
                } else {
                    PlanUpdateMode::Replace
                },
                steps,
            )
            .map_err(|error| format!("invalid planning update: {error:?}"))?;
        }
        for action in input.actions {
            match action {
                PlanningAction::Wait { wait } => {
                    if wait.step_ids.iter().any(|id| {
                        !plan
                            .steps
                            .iter()
                            .any(|step| step.id == *id && !step.removed)
                    }) {
                        return Err("wait refers to a missing local step".into());
                    }
                    self.validate_plan_wait(&wait).await?;
                    plan.waits.retain(|old| old.id != wait.id);
                    plan.waits.push(wait);
                }
                PlanningAction::ClearWait { id } => {
                    plan.waits.retain(|wait| wait.id != id);
                }
                PlanningAction::HandleReplans {
                    ids,
                    no_change_reason,
                } => {
                    if ids.is_empty() {
                        return Err("handling requires source identities".into());
                    }
                    let consumed = self
                        .chat_state_handle
                        .consumed_agent_messages(
                            ids.iter().map(|id| replan_message_id(id)).collect(),
                        )
                        .await?;
                    for id in ids {
                        let record = plan
                            .replans
                            .iter()
                            .find(|change| change.id == id)
                            .ok_or_else(|| format!("replan {id} was not received"))?;
                        if record.child_session_id != self.session_info.id.0.as_ref() {
                            return Err(
                                "only the receiving child may acknowledge replanning".into()
                            );
                        }
                        if !consumed.contains(&replan_message_id(&id)) {
                            return Err(
                                "replanning guidance has not been consumed as model context".into(),
                            );
                        }
                        if matches!(record.progress, ReplanProgress::Received) {
                            plan.advance_replan(&id, ReplanProgress::Consumed)
                                .map_err(|error| format!("{error:?}"))?;
                        }
                        let outcome = no_change_reason.as_ref().map_or(
                            ReplanOutcome::Updated {
                                plan_revision: plan.revision,
                            },
                            |reason| ReplanOutcome::NoChange {
                                reason: reason.clone(),
                            },
                        );
                        plan.advance_replan(&id, ReplanProgress::Handled { outcome })
                            .map_err(|error| format!("replan handling rejected: {error:?}"))?;
                    }
                }
                PlanningAction::Review { review } => {
                    let link = plan
                        .links
                        .iter()
                        .find(|link| link.id == review.link_id)
                        .ok_or_else(|| {
                            "review does not name a locally delegated task".to_string()
                        })?;
                    if review.decision == ReviewDecision::Released {
                        if review
                            .reason
                            .as_deref()
                            .is_none_or(|reason| reason.trim().is_empty())
                        {
                            return Err("releasing a required deliverable needs a reason".into());
                        }
                    } else if !link.terminal
                        || link.result_id.is_none()
                        || link.result_id != review.result_id
                    {
                        return Err("review requires the exact delivered terminal result".into());
                    }
                    if review.reviewed_source_revision > plan.revision {
                        return Err("review cannot claim a future source revision".into());
                    }
                    plan.reviews.push(review);
                }
                PlanningAction::SelectScope { scope_id } => {
                    if scope_id.trim().is_empty() || scope_id.len() > 128 {
                        return Err("invalid work scope identity".into());
                    }
                    if !plan.waits.is_empty()
                        || plan.links.iter().any(|link| !link.terminal)
                        || !plan.unresolved_required_deliveries().is_empty()
                    {
                        return Err("dispose old executions, waits and mandatory deliveries before switching scope".into());
                    }
                    plan = LocalPlan::new(self.session_info.id.0.to_string(), scope_id);
                }
            }
        }
        self.commit_local_plan(previous, plan).await
    }

    async fn validate_plan_wait(&self, wait: &PlanWait) -> Result<(), String> {
        if wait.id.trim().is_empty()
            || wait.id.len() > 128
            || wait.dependencies.is_empty()
            || wait.dependencies.len() > 256
        {
            return Err("wait requires a bounded identity and actual dependencies".into());
        }
        for dependency in &wait.dependencies {
            if dependency.id == self.session_info.id.0.as_ref()
                || self.startup_hints.parent_session_id.as_deref() == Some(dependency.id.as_str())
            {
                return Err("cannot wait for this execution or its delegating ancestor".into());
            }
            self.plan_dependency_ready(dependency).await?;
        }
        Ok(())
    }

    async fn plan_dependency_ready(
        &self,
        dependency: &tool_types::planning::WaitDependency,
    ) -> Result<bool, String> {
        match dependency.kind {
            DependencyKind::Task => {
                let bridge = self.agent.borrow().tool_bridge().clone();
                let backend = bridge
                    .read_resource::<tools::implementations::grow_build::task::backend::SubagentBackendResource>()
                    .await.ok_or_else(|| "task backend unavailable".to_string())?;
                let snapshot = backend
                    .backend()
                    .query(&dependency.id, false, None)
                    .await
                    .map_err(|error| format!("dependency observation failed: {error}"))?
                    .ok_or_else(|| format!("task dependency {} does not exist", dependency.id))?;
                Ok(snapshot.status.is_terminal())
            }
            DependencyKind::Command => {
                let bridge = self.agent.borrow().tool_bridge().clone();
                let command = bridge
                    .background_task(&dependency.id)
                    .await
                    .ok_or_else(|| {
                        format!("command dependency {} does not exist", dependency.id)
                    })?;
                if command.owner_session_id.as_deref() != Some(self.session_info.id.0.as_ref()) {
                    return Err("command dependency belongs to another session".into());
                }
                Ok(command.completed)
            }
            DependencyKind::Question | DependencyKind::Permission => {
                let pending = self
                    .pending_interactions
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let expected = match dependency.kind {
                    DependencyKind::Question => {
                        crate::session::pending_interaction::PendingKind::Question
                    }
                    _ => crate::session::pending_interaction::PendingKind::Permission,
                };
                if pending.get(&dependency.id) != Some(&expected) {
                    return Err("human dependency has no matching active request owner".into());
                }
                Ok(false)
            }
        }
    }

    pub(crate) async fn planning_waits_for_event(&self) -> bool {
        let Ok(Some(snapshot)) = self.chat_state_handle.planning_state().await else {
            return false;
        };
        if snapshot.plan.waits.is_empty() {
            return false;
        }
        // Scope is explicit: a whole-work wait has no step IDs. A step wait
        // cannot silently freeze other unfinished steps.
        if !snapshot
            .plan
            .waits
            .iter()
            .any(|wait| wait.step_ids.is_empty())
            && snapshot.plan.steps.iter().any(|step| {
                !step.removed
                    && matches!(
                        step.status,
                        PlanStepStatus::Pending | PlanStepStatus::InProgress
                    )
                    && !snapshot
                        .plan
                        .waits
                        .iter()
                        .any(|wait| wait.step_ids.contains(&step.id))
            })
        {
            return false;
        }
        for wait in &snapshot.plan.waits {
            let mut ready = Vec::new();
            for dependency in &wait.dependencies {
                // A missing/failed observation must wake a decision, not hang forever.
                ready.push(self.plan_dependency_ready(dependency).await.unwrap_or(true));
            }
            if match wait.condition {
                WaitCondition::Any => ready.iter().any(|value| *value),
                WaitCondition::All => ready.iter().all(|value| *value),
            } {
                return false;
            }
        }
        true
    }

    /// Keep the admitted work and its cumulative limits alive while no model
    /// step is useful. Existing backend waiters own completion notifications.
    pub(crate) async fn await_planned_dependencies(
        &self,
        origin: &crate::session::PromptOrigin,
    ) -> Result<PlannedDependenciesOutcome, acp::Error> {
        let Some(initial) = self
            .chat_state_handle
            .planning_state()
            .await
            .map_err(|error| acp::Error::internal_error().data(error))?
        else {
            return Ok(PlannedDependenciesOutcome::NotWaiting);
        };
        if initial.plan.waits.is_empty() {
            return Ok(PlannedDependenciesOutcome::NotWaiting);
        }
        // Subscribe before re-reading the dependency/queue state so an
        // answer racing with the first check cannot be missed.
        let mut interaction_changes = self.pending_interactions.subscribe_changes();
        loop {
            let idle_notification = self.idle_arbiter.notified();
            tokio::pin!(idle_notification);
            idle_notification.as_mut().enable();
            let queued_user_input = {
                let state = self.state.lock().await;
                state
                    .pending_inputs
                    .iter()
                    .any(|input| !input.origin.is_synthetic())
            };
            if queued_user_input
                && matches!(
                    origin,
                    crate::session::PromptOrigin::GoalContinuation { .. }
                )
            {
                // Keep the plan wait durable and release only the automatic
                // Goal continuation's foreground turn. The actor can then
                // admit FIFO input before scheduling Goal again.
                return Ok(PlannedDependenciesOutcome::YieldGoalForFifo);
            }
            let snapshot = self
                .chat_state_handle
                .planning_state()
                .await
                .map_err(|error| acp::Error::internal_error().data(error))?;
            let Some(snapshot) = snapshot else {
                break;
            };
            if snapshot.plan.waits.is_empty() {
                break;
            }
            let mut ready_waits = Vec::new();
            for wait in &snapshot.plan.waits {
                let mut ready = Vec::new();
                for dependency in &wait.dependencies {
                    ready.push(self.plan_dependency_ready(dependency).await.unwrap_or(true));
                }
                if match wait.condition {
                    WaitCondition::Any => ready.iter().any(|value| *value),
                    WaitCondition::All => ready.iter().all(|value| *value),
                } {
                    ready_waits.push(wait.id.clone());
                }
            }
            if !ready_waits.is_empty() {
                // Resolve once in the same owner ledger. Leaving a ready wait
                // registered would re-sample forever after every final answer.
                let _gate = self.step_control_gate.lock().await;
                let Some(current) = self
                    .chat_state_handle
                    .planning_state()
                    .await
                    .map_err(|error| acp::Error::internal_error().data(error))?
                else {
                    break;
                };
                if current.seq != snapshot.seq {
                    continue;
                }
                let mut plan = current.plan;
                plan.waits.retain(|wait| !ready_waits.contains(&wait.id));
                self.commit_local_plan(Some(current.seq), plan)
                    .await
                    .map_err(|error| acp::Error::internal_error().data(error))?;
                break;
            }
            let mut waiters: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>> =
                Vec::new();
            let bridge = self.agent.borrow().tool_bridge().clone();
            let backend = bridge
                .read_resource::<tools::implementations::grow_build::task::backend::SubagentBackendResource>().await;
            for wait in &snapshot.plan.waits {
                for dependency in &wait.dependencies {
                    // An already-terminal member of an All wait must not
                    // become an immediately-ready future in every iteration.
                    if self.plan_dependency_ready(dependency).await.unwrap_or(true) {
                        continue;
                    }
                    let id = dependency.id.clone();
                    match dependency.kind {
                        DependencyKind::Task => {
                            if let Some(backend) = backend.clone() {
                                waiters.push(Box::pin(async move {
                                    let _ = backend.backend().query(&id, true, Some(25_000)).await;
                                }));
                            }
                        }
                        DependencyKind::Command => {
                            let terminal = bridge.clone();
                            waiters.push(Box::pin(async move {
                                let _ = terminal
                                    .wait_background_task(&id, Duration::from_secs(25))
                                    .await;
                            }));
                        }
                        DependencyKind::Question | DependencyKind::Permission => {}
                    }
                }
            }
            let mut parent_signal = self.parent_message_interrupt.subscribe();
            tokio::select! {
                biased;
                _ = wait_for_pending_interjection(&self.pending_interjections) => break,
                _ = parent_signal.wait_for(|pending| *pending) => break,
                _ = interaction_changes.changed() => {},
                _ = &mut idle_notification => {},
                _ = async {
                    if waiters.is_empty() {
                        tokio::time::sleep(Duration::from_secs(25)).await;
                    } else {
                        let _ = futures_util::future::select_all(waiters).await;
                    }
                } => {}
            }
        }
        if matches!(
            origin,
            crate::session::PromptOrigin::GoalContinuation { .. }
        ) {
            let state = self.state.lock().await;
            if state
                .pending_inputs
                .iter()
                .any(|input| !input.origin.is_synthetic())
            {
                return Ok(PlannedDependenciesOutcome::YieldGoalForFifo);
            }
        }
        self.chat_state_handle.push_user_message_durably(ConversationItem::system_reminder(
            "A declared local-work wait changed or received steering. Inspect the current dependency outcome and latest guidance, clear resolved waits with todo_write, and continue the same admitted work. A failed, interrupted or unknown operation is not success and must not be blindly replayed.",
        )).await.map_err(|error| crate::session::commands::fatal_turn_boundary_error("planning wakeup", error.to_string()))?;
        Ok(PlannedDependenciesOutcome::ContinueTurn)
    }

    pub(crate) async fn planning_completion_blockers(&self) -> Result<Vec<String>, String> {
        let Some(snapshot) = self.chat_state_handle.planning_state().await? else {
            return Ok(Vec::new());
        };
        let mut blockers: Vec<_> = snapshot
            .plan
            .unresolved_required_deliveries()
            .iter()
            .map(|link| format!("required deliverable {} is not accepted/released", link.id))
            .collect();
        for wait in &snapshot.plan.waits {
            blockers.push(format!(
                "explicit wait {} must be resolved or cleared",
                wait.id
            ));
        }
        for link in snapshot
            .plan
            .links
            .iter()
            .filter(|link| link.required && link.pending_source_revision.is_some())
        {
            blockers.push(format!(
                "required task {} has an unhandled parent update",
                link.id
            ));
        }
        for change in snapshot.plan.replans.iter().filter(|change| {
            change.child_session_id == self.session_info.id.0.as_ref()
                && !matches!(
                    change.progress,
                    ReplanProgress::Handled { .. } | ReplanProgress::Unapplied { .. }
                )
        }) {
            blockers.push(format!("received parent update {} must be handled with a local plan or a reasoned no-change decision", change.id));
        }
        for review in snapshot
            .plan
            .reviews
            .iter()
            .filter(|review| review.decision == ReviewDecision::Released)
        {
            if snapshot.plan.links.iter().any(|link| {
                link.id == review.link_id && !link.terminal && link.dispatch_failure.is_none()
            }) {
                blockers.push(format!(
                    "released task {} still needs explicit execution and wait disposition",
                    review.link_id
                ));
            }
        }
        Ok(blockers)
    }
}

fn replan_message_id(id: &str) -> String {
    format!("replan-{}", blake3::hash(id.as_bytes()).to_hex())
}

fn plan_status(status: TodoStatus) -> PlanStepStatus {
    match status {
        TodoStatus::Pending => PlanStepStatus::Pending,
        TodoStatus::InProgress => PlanStepStatus::InProgress,
        TodoStatus::Completed => PlanStepStatus::Completed,
        TodoStatus::Cancelled => PlanStepStatus::Cancelled,
    }
}

fn plan_priority(priority: TodoPriority) -> PlanPriority {
    match priority {
        TodoPriority::High => PlanPriority::High,
        TodoPriority::Medium => PlanPriority::Normal,
        TodoPriority::Low => PlanPriority::Low,
    }
}

//! Small shared primitives for the Goal runtime.

use super::*;

#[derive(Debug, Clone)]
pub(crate) struct SubagentTokenRecord {
    pub goal_id: Option<String>,
    pub resume_anchor_cumulative: u64,
    pub settled_cumulative: u64,
    pub last_cumulative_reported: u64,
    pub model: Option<String>,
    pub finished: bool,
}

impl SubagentTokenRecord {
    pub fn marginal(&self) -> u64 {
        self.last_cumulative_reported
            .saturating_sub(self.resume_anchor_cumulative)
    }

    pub fn unsettled(&self) -> u64 {
        self.last_cumulative_reported
            .saturating_sub(self.settled_cumulative)
    }
}

pub(crate) fn goal_runtime_available_from_tools(goal_enabled: bool, tool_names: &[String]) -> bool {
    use tools::implementations::grow_build::{
        CREATE_GOAL_TOOL_NAME, GET_GOAL_TOOL_NAME, UPDATE_GOAL_TOOL_NAME,
    };
    goal_enabled
        && [
            CREATE_GOAL_TOOL_NAME,
            GET_GOAL_TOOL_NAME,
            UPDATE_GOAL_TOOL_NAME,
        ]
        .into_iter()
        .all(|required| tool_names.iter().any(|name| name == required))
}

pub(crate) fn laziness_injection_active(
    goal_runtime_available: bool,
    goal_status: Option<crate::session::goal_tracker::GoalStatus>,
) -> bool {
    goal_runtime_available && goal_status == Some(crate::session::goal_tracker::GoalStatus::Active)
}

fn fold_tokens_by_model<'a>(
    records: impl IntoIterator<Item = &'a SubagentTokenRecord>,
    goal_id: &str,
    current_model_id: &str,
) -> Vec<(String, u64)> {
    let mut by_model = std::collections::HashMap::<String, u64>::new();
    for record in records {
        if record.goal_id.as_deref() != Some(goal_id) {
            continue;
        }
        let model = record
            .model
            .as_deref()
            .filter(|model| !model.trim().is_empty())
            .unwrap_or(current_model_id)
            .to_string();
        let entry = by_model.entry(model).or_default();
        *entry = entry.saturating_add(record.marginal());
    }
    let mut result: Vec<_> = by_model
        .into_iter()
        .filter(|(_, value)| *value > 0)
        .collect();
    result.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    result
}

impl SessionActor {
    /// Publish the immutable ownership snapshot consumed by tools in one
    /// admitted turn. Callers install a gated `RegularTurn` before awaiting
    /// this method, then release the gate after it returns.
    pub(super) async fn publish_turn_scope_resources(
        &self,
        prompt_id: String,
        origin: &crate::session::PromptOrigin,
        admitted_behavior: tool_types::BehaviorId,
    ) {
        let bridge = self.agent.borrow().tool_bridge().clone();
        // A delegated Goal child receives an immutable objective view, not a
        // second Goal runtime. Preserve that ownership through descendants so
        // nested work cannot mutate lifecycle state merely because child
        // sessions use Normal as their visible Behavior.
        let inherited_goal_context = bridge
            .read_resource::<tools::implementations::grow_build::update_goal::GoalContextSnapshotResource>()
            .await
            .and_then(|resource| resource.0);
        let expected_goal_id = match origin {
            crate::session::PromptOrigin::GoalContinuation { goal_id } => Some(goal_id.as_str()),
            crate::session::PromptOrigin::User
                if admitted_behavior == tool_types::BehaviorId::Goal =>
            {
                None
            }
            _ => Some(""),
        };
        let (subagent_owner, delegation_snapshot) = if let Some(context) = inherited_goal_context {
            (
                tools::implementations::grow_build::task::types::SubagentOwner::goal(
                    &context.view.goal_id,
                ),
                Some(context.view),
            )
        } else {
            let goal_snapshot = (expected_goal_id != Some(""))
                .then(|| self.goal_tracker.lock().snapshot().cloned())
                .flatten()
                .filter(|goal| {
                    goal.status == crate::session::goal_tracker::GoalStatus::Active
                        && expected_goal_id.is_none_or(|expected| expected == goal.goal_id)
                });
            goal_snapshot
                .as_ref()
                .map(|goal| {
                    (
                        tools::implementations::grow_build::task::types::SubagentOwner::goal(
                            &goal.goal_id,
                        ),
                        Some(super::goal::goal_view_from_snapshot(
                            goal,
                            0,
                            goal.elapsed_ms,
                        )),
                    )
                })
                .unwrap_or_default()
        };

        *self
            .current_prompt_id
            .lock()
            .expect("current_prompt_id mutex poisoned") = Some(prompt_id.clone());
        bridge
            .update_resource(
                tools::implementations::grow_build::task::types::CurrentPromptIdResource(prompt_id),
            )
            .await;
        bridge
            .update_resource(
                tools::implementations::grow_build::task::types::CurrentSubagentOwnerResource(
                    subagent_owner,
                ),
            )
            .await;
        bridge
            .update_resource(
                tools::implementations::grow_build::update_goal::GoalDelegationSnapshotResource(
                    delegation_snapshot,
                ),
            )
            .await;
    }

    pub(super) fn goal_notify_sender(&self) -> crate::session::goal_notification::GoalNotifySender {
        crate::session::goal_notification::GoalNotifySender::new(
            self.session_info.id.clone(),
            self.notifications.gateway.clone(),
            self.notifications.persistence_tx.clone(),
        )
    }

    pub(super) async fn persist_control_snapshot_durably(
        &self,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: Option<crate::session::goal_tracker::GoalState>,
    ) -> std::io::Result<()> {
        self.persist_control_snapshot_with_context_durably(behavior, goal, None, None)
            .await
    }

    pub(super) async fn persist_behavior_transition_durably(
        &self,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: Option<crate::session::goal_tracker::GoalState>,
    ) -> std::io::Result<()> {
        let context = crate::session::behavior::behavior_transition_context(behavior.behavior());
        self.persist_control_snapshot_with_context_durably(
            behavior,
            goal,
            None,
            Some((
                chat_state::ControlContextLayer::Behavior,
                chat_state::ControlContextActivation::Transition,
                context,
            )),
        )
        .await
    }

    pub(super) async fn persist_agent_transition_durably(
        &self,
        agent_name: &str,
        role_prompt: Option<&str>,
    ) -> std::io::Result<()> {
        let context =
            crate::session::control::agent_role_transition_context(agent_name, role_prompt);
        self.persist_control_snapshot_with_context_durably(
            self.behavior.lock().snapshot(),
            self.goal_tracker.lock().snapshot().cloned(),
            Some(agent_name),
            Some((
                chat_state::ControlContextLayer::AgentRole,
                chat_state::ControlContextActivation::Transition,
                context,
            )),
        )
        .await
    }

    pub(super) async fn reproject_control_contexts_durably(
        &self,
        contexts: impl IntoIterator<
            Item = (
                chat_state::ControlContextLayer,
                sampling_types::ConversationItem,
            ),
        >,
    ) -> std::io::Result<()> {
        for (layer, context) in contexts {
            let agent_name = self.agent.borrow().name().to_owned();
            self.persist_control_snapshot_with_context_durably(
                self.behavior.lock().snapshot(),
                self.goal_tracker.lock().snapshot().cloned(),
                Some(&agent_name),
                Some((
                    layer,
                    chat_state::ControlContextActivation::Reprojection,
                    context.text_content(),
                )),
            )
            .await?;
        }
        Ok(())
    }

    /// Restore active Control layers whose latest model context was shadowed
    /// by a committed compaction. The Control snapshot remains the fact source;
    /// this appends a new typed projection rather than reviving an older item.
    pub(super) async fn repair_missing_control_contexts_durably(&self) -> std::io::Result<()> {
        let Some(materialized) = self
            .chat_state_handle
            .materialize_timeline(self.session_id_string())
            .await
        else {
            return Ok(());
        };
        let current = materialized
            .surface_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let missing = materialized
            .active_control_contexts
            .into_iter()
            .filter_map(|(layer, context)| {
                (!current.contains(&context.surface_id)).then_some((layer, context.item))
            })
            .collect::<Vec<_>>();
        self.reproject_control_contexts_durably(missing).await
    }

    async fn persist_control_snapshot_with_context_durably(
        &self,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: Option<crate::session::goal_tracker::GoalState>,
        agent_name: Option<&str>,
        model_context: Option<(
            chat_state::ControlContextLayer,
            chat_state::ControlContextActivation,
            String,
        )>,
    ) -> std::io::Result<()> {
        let revision = self
            .control_revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .saturating_add(1);
        let agent_name = agent_name
            .map(str::to_owned)
            .unwrap_or_else(|| self.agent.borrow().name().to_owned());
        let state = crate::session::control::SessionControlSnapshot::new(
            revision,
            agent_name,
            behavior,
            goal,
        );
        let kind = match model_context {
            Some((layer, activation, context)) => {
                state.timeline_kind_with_model_context(layer, activation, context)?
            }
            None => state.timeline_kind()?,
        };
        self.chat_state_handle
            .record_timeline_event_durably(kind)
            .await
            .map(|_| ())
            .map_err(std::io::Error::other)
    }

    pub(super) async fn delete_goal_state_durably(&self) -> std::io::Result<()> {
        let behavior = self.behavior.lock().snapshot();
        self.persist_control_snapshot_durably(behavior, None).await
    }

    pub(super) async fn commit_goal_mutation_or_restore(
        &self,
        previous: crate::session::goal_tracker::GoalState,
    ) -> Result<(), String> {
        let next = self.goal_tracker.lock().snapshot().cloned();
        let behavior = self.behavior.lock().snapshot();
        if let Err(error) = self.persist_control_snapshot_durably(behavior, next).await {
            self.goal_tracker.lock().restore_runtime_snapshot(previous);
            return Err(format!("Goal control state was not persisted: {error}"));
        }
        Ok(())
    }

    /// Snapshot the only Goal state that is allowed to survive actor teardown,
    /// then let the caller issue the persistence barrier.
    pub(super) async fn checkpoint_goal_before_shutdown(&self) {
        if self.goal_tracker.lock().snapshot().is_none() {
            return;
        }
        self.settle_live_goal_subagent_tokens();
        let current_tokens = self.chat_state_handle.get_total_tokens().await as i64;
        let _ = self.goal_tokens(current_tokens);
        self.goal_tracker.lock().account_elapsed();
        let behavior = self.behavior.lock().snapshot();
        let goal = self.goal_tracker.lock().snapshot().cloned();
        if let Err(error) = self.persist_control_snapshot_durably(behavior, goal).await {
            tracing::warn!(%error, "failed to checkpoint Goal control state before shutdown");
        }
    }

    pub(super) fn goal_runtime_available(&self) -> bool {
        self.goal_runtime_available
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(super) fn goal_loop_active(&self) -> bool {
        self.goal_runtime_available()
            && self.goal_tracker.lock().status()
                == Some(crate::session::goal_tracker::GoalStatus::Active)
    }

    pub(super) fn record_goal_turn_task_ids(&self, ids: impl IntoIterator<Item = String>) {
        if self.goal_loop_active() {
            self.goal_turn_task_ids.lock().extend(ids);
        }
    }

    pub(super) fn record_reparented_goal_turn_task_ids(
        &self,
        ids: impl IntoIterator<Item = String>,
    ) {
        if self.goal_runtime_available() {
            self.goal_turn_task_ids.lock().extend(ids);
        }
    }

    fn set_goal_runtime_availability_from_tools(&self, tool_names: &[String]) -> bool {
        let enabled = goal_runtime_available_from_tools(self.goal_enabled, tool_names);
        self.goal_runtime_available
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        enabled
    }

    pub(super) async fn refresh_goal_runtime_availability(&self) -> bool {
        let tool_names = self.registered_tool_names().await;
        let enabled = self.set_goal_runtime_availability_from_tools(&tool_names);
        if !enabled {
            self.auto_pause_goal_if_active_with_message(
                crate::session::goal_tracker::GoalPauseReason::RuntimeUnavailable,
                "Goal runtime paused because one or more required Goal tools are unavailable. Re-enable create_goal, get_goal, and update_goal before restarting."
                    .to_string(),
            )
            .await;
        }
        enabled
    }

    pub(super) fn active_goal_directive_tag(&self) -> Option<sampling_types::GoalDirectiveTag> {
        if self.behavior.lock().behavior() != tool_types::BehaviorId::Goal {
            return None;
        }
        let tracker = self.goal_tracker.lock();
        let goal = tracker.snapshot()?;
        if goal.status != crate::session::goal_tracker::GoalStatus::Active {
            return None;
        }
        Some(sampling_types::GoalDirectiveTag {
            goal_id: goal.goal_id.clone(),
            definition_revision: goal.definition_revision,
        })
    }

    pub(super) fn goal_directive_item(
        &self,
        content: impl Into<String>,
        reason: sampling_types::SyntheticReason,
    ) -> ConversationItem {
        match self.active_goal_directive_tag() {
            Some(tag) => ConversationItem::goal_directive(content, reason, tag),
            None => ConversationItem::system_reminder(content),
        }
    }

    pub(super) async fn inject_stopped_goal_interaction_directive(&self) {
        if self.behavior.lock().behavior() == tool_types::BehaviorId::Goal
            && self
                .goal_tracker
                .lock()
                .status()
                .is_some_and(|status| {
                    matches!(
                        status,
                        crate::session::goal_tracker::GoalStatus::Paused
                            | crate::session::goal_tracker::GoalStatus::Blocked
                            | crate::session::goal_tracker::GoalStatus::UsageLimited
                            | crate::session::goal_tracker::GoalStatus::BudgetLimited
                    )
                })
        {
            self.chat_state_handle
                .push_user_message(ConversationItem::system_reminder(
                    "The Goal is stopped. Answer the user's current message normally; do not restart autonomous work unless the user explicitly restarts it."
                        .to_string(),
                ));
        }
    }

    pub(crate) fn goal_tokens(&self, current_session_tokens: i64) -> (i64, i64) {
        let goal_id = match self.goal_tracker.lock().snapshot() {
            Some(goal) => goal.goal_id.clone(),
            None => return (0, 0),
        };
        let active_subagents = self
            .subagent_token_records
            .lock()
            .values()
            .filter(|record| {
                !record.finished && record.goal_id.as_deref() == Some(goal_id.as_str())
            })
            .fold(0i64, |all, record| {
                let value = i64::try_from(record.marginal()).unwrap_or(i64::MAX);
                all.saturating_add(value)
            });
        let mut tracker = self.goal_tracker.lock();
        let parent = tracker.account_parent_tokens(current_session_tokens);
        let finished = tracker.subagent_tokens_spent();
        let total = parent
            .saturating_add(finished)
            .saturating_add(active_subagents);
        (total, finished)
    }

    /// Settle one terminal subagent exactly once into the durable Goal token
    /// counter while retaining its record as a resume anchor.
    pub(super) fn settle_goal_subagent_tokens(
        &self,
        subagent_id: &str,
        reported_tokens: u64,
    ) -> Option<i64> {
        let (current_goal_id, complete) = self
            .goal_tracker
            .lock()
            .snapshot()
            .map(|goal| {
                (
                    Some(goal.goal_id.clone()),
                    goal.status == crate::session::goal_tracker::GoalStatus::Complete,
                )
            })
            .unwrap_or((None, false));
        let delta = {
            let mut records = self.subagent_token_records.lock();
            if complete {
                records.remove(subagent_id);
                return None;
            }
            let record = records.get_mut(subagent_id)?;
            record.last_cumulative_reported = record.last_cumulative_reported.max(reported_tokens);
            if record.goal_id != current_goal_id {
                records.remove(subagent_id);
                return None;
            }
            record.finished = true;
            let delta = record.unsettled();
            record.settled_cumulative = record.last_cumulative_reported;
            i64::try_from(delta).unwrap_or(i64::MAX)
        };
        let _ = self.goal_tracker.lock().settle_subagent_tokens(delta);
        Some(delta)
    }

    /// Charge delegated Goal work at graceful shutdown before the persistence
    /// barrier so a reload cannot reuse the same budget.
    pub(super) fn settle_live_goal_subagent_tokens(&self) -> i64 {
        let Some(goal_id) = self
            .goal_tracker
            .lock()
            .snapshot()
            .map(|goal| goal.goal_id.clone())
        else {
            return 0;
        };
        let delta = {
            let mut records = self.subagent_token_records.lock();
            records
                .values_mut()
                .filter(|record| {
                    !record.finished && record.goal_id.as_deref() == Some(goal_id.as_str())
                })
                .fold(0i64, |total, record| {
                    record.finished = true;
                    let delta = record.unsettled();
                    record.settled_cumulative = record.last_cumulative_reported;
                    total.saturating_add(i64::try_from(delta).unwrap_or(i64::MAX))
                })
        };
        if self.goal_tracker.lock().settle_subagent_tokens(delta) {
            delta
        } else {
            0
        }
    }

    pub(crate) fn goal_tokens_used(&self, current_session_tokens: i64) -> i64 {
        self.goal_tokens(current_session_tokens).0
    }

    pub(crate) fn goal_tokens_by_model(&self, current_model_id: &str) -> Vec<(String, u64)> {
        let goal_id = match self.goal_tracker.lock().snapshot() {
            Some(goal) => goal.goal_id.clone(),
            None => return Vec::new(),
        };
        let records = self.subagent_token_records.lock();
        fold_tokens_by_model(
            records.values().filter(|record| !record.finished),
            &goal_id,
            current_model_id,
        )
    }

    pub(super) async fn set_goal_loop_active_resource(&self, active: bool) {
        self.tool_context
            .goal_loop_active_gate
            .store(active, std::sync::atomic::Ordering::Relaxed);
        self.agent
            .borrow()
            .tool_bridge()
            .update_resource(
                tools::implementations::grow_build::task::types::GoalLoopActive(active),
            )
            .await;
    }
}

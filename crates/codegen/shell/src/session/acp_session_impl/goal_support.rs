//! Small shared primitives for the Goal runtime.

use super::*;

#[derive(Debug)]
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

pub(crate) fn goal_slash_and_harness_available(goal_enabled: bool, tool_names: &[String]) -> bool {
    use tools::implementations::grow_build::{
        GET_GOAL_TOOL_NAME, UPDATE_GOAL_PLAN_TOOL_NAME, UPDATE_GOAL_TOOL_NAME,
    };
    goal_enabled
        && [
            GET_GOAL_TOOL_NAME,
            UPDATE_GOAL_PLAN_TOOL_NAME,
            UPDATE_GOAL_TOOL_NAME,
        ]
        .into_iter()
        .all(|required| tool_names.iter().any(|name| name == required))
}

pub(crate) fn laziness_injection_active(
    goal_harness_enabled: bool,
    goal_status: Option<crate::session::goal_tracker::GoalStatus>,
) -> bool {
    goal_harness_enabled && goal_status == Some(crate::session::goal_tracker::GoalStatus::Active)
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
    pub(super) fn goal_notify_sender(&self) -> crate::session::goal_orchestrator::GoalNotifySender {
        crate::session::goal_orchestrator::GoalNotifySender::new(
            self.session_info.id.clone(),
            self.notifications.gateway.clone(),
            self.notifications.persistence_tx.clone(),
        )
    }

    pub(super) async fn delete_goal_state_durably(&self) -> std::io::Result<()> {
        let (respond_to, deleted) = tokio::sync::oneshot::channel();
        self.notifications
            .persistence_tx
            .send(crate::session::persistence::PersistenceMsg::DeleteGoalModeState { respond_to })
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "persistence actor closed")
            })?;
        deleted.await.map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "persistence actor dropped Goal deletion acknowledgement",
            )
        })?
    }

    /// Snapshot the only Goal state that is allowed to survive actor teardown,
    /// then let the caller issue the persistence barrier.
    pub(super) async fn checkpoint_goal_before_shutdown(&self) {
        if self.goal_tracker.lock().snapshot().is_none() {
            return;
        }
        if let Some((_, cancel)) = self.goal_stage_cancel.lock().take() {
            cancel.cancel();
        }
        self.settle_live_goal_subagent_tokens();
        let current_tokens = self.chat_state_handle.get_total_tokens().await as i64;
        let _ = self.goal_tokens(current_tokens);
        let mut tracker = self.goal_tracker.lock();
        tracker.account_elapsed();
        self.goal_notify_sender().persist_goal_state(&tracker);
    }

    pub(super) fn goal_harness_enabled(&self) -> bool {
        self.goal_harness_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(super) fn goal_loop_active(&self) -> bool {
        self.goal_harness_enabled()
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
        if self.goal_harness_enabled() {
            self.goal_turn_task_ids.lock().extend(ids);
        }
    }

    fn set_goal_harness_from_tools(&self, tool_names: &[String]) -> bool {
        let enabled = goal_slash_and_harness_available(self.goal_enabled, tool_names);
        self.goal_harness_enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        enabled
    }

    pub(super) async fn refresh_goal_harness_enabled(&self) -> bool {
        let tool_names = self.registered_tool_names().await;
        let enabled = self.set_goal_harness_from_tools(&tool_names);
        if !enabled {
            self.auto_pause_goal_if_active_with_message(
                crate::session::goal_tracker::GoalPauseReason::Infra,
                "Goal runtime paused because one or more required Goal tools are unavailable. Re-enable get_goal, update_goal_plan, and update_goal before resuming."
                    .to_string(),
            )
            .await;
        }
        enabled
    }

    pub(super) fn current_goal_directive_tag(
        &self,
        kind: sampling_types::GoalDirectiveKind,
    ) -> Option<sampling_types::GoalDirectiveTag> {
        let tracker = self.goal_tracker.lock();
        let goal = tracker.snapshot()?;
        Some(sampling_types::GoalDirectiveTag {
            goal_id: goal.goal_id.clone(),
            definition_revision: goal.objective_revision,
            kind,
        })
    }

    pub(super) fn goal_directive_item(
        &self,
        content: impl Into<String>,
        reason: sampling_types::SyntheticReason,
        kind: sampling_types::GoalDirectiveKind,
    ) -> ConversationItem {
        match self.current_goal_directive_tag(kind) {
            Some(tag) => ConversationItem::goal_directive(content, reason, tag),
            None => ConversationItem::system_reminder(content),
        }
    }

    pub(super) async fn inject_paused_goal_interaction_directive(&self) {
        if self.behavior.lock().behavior() == Some(tool_types::BehaviorId::Goal)
            && self
                .goal_tracker
                .lock()
                .status()
                .is_some_and(crate::session::goal_tracker::GoalStatus::is_paused)
        {
            self.chat_state_handle
                .push_user_message(ConversationItem::system_reminder(
                    "The Goal is paused. Answer the user's current message normally; do not resume autonomous work unless the user explicitly resumes it."
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
            let Some(record) = records.get_mut(subagent_id) else {
                return None;
            };
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

    /// Graceful shutdown abandons live Goal stages. Charge their latest known
    /// progress before the persistence barrier so a reload cannot reuse the
    /// same budget.
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

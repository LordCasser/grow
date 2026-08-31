//! Small shared primitives for the Goal runtime.

use super::*;

/// Process-local view of the Goal usage window shared by the root session and
/// every descendant session it creates. Each provider call captures the active
/// Goal immediately before admission. Settlement uses that immutable id, so a
/// pause cannot erase already-admitted work and a restart cannot claim work
/// that was admitted while paused.
#[derive(Debug, Clone)]
pub(crate) struct GoalUsageWindow {
    state: std::sync::Arc<parking_lot::Mutex<GoalUsageWindowState>>,
    settlement_changed: std::sync::Arc<tokio::sync::Notify>,
    root_cmd_tx: tokio::sync::mpsc::WeakUnboundedSender<crate::session::commands::SessionCommand>,
}

#[derive(Debug, Default)]
struct GoalUsageWindowState {
    provider_window: GoalProviderWindow,
    owner_epochs: std::collections::HashMap<String, u64>,
    pending_attempts: std::collections::HashMap<String, GoalUsageAttemptOwner>,
}

#[derive(Debug, Default)]
enum GoalProviderWindow {
    #[default]
    Inactive,
    Active(String),
    /// Durable lifecycle is still Active until StepEnded, but the known usage
    /// already reached its budget. This state rejects every new request across
    /// root, descendants and sidebands instead of masquerading as no Goal.
    Exhausted(String),
    UsageIncomplete(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GoalUsageIncompleteApply {
    Ignored,
    Recorded,
    Stopped,
}

impl GoalUsageIncompleteApply {
    pub(super) fn applied(self) -> bool {
        self != Self::Ignored
    }
}

#[derive(Debug, Clone)]
struct GoalUsageAttemptOwner {
    goal_id: String,
    session_id: String,
    epoch: u64,
    /// `None` means the provider attempt has not reported an outcome yet.
    /// `Some(None)` is fail-closed unknown usage; `Some(Some(tokens))` is the
    /// exact normalized Goal charge. The first report wins permanently.
    settlement: Option<Option<i64>>,
}

impl GoalUsageWindow {
    pub(crate) fn new(
        root_cmd_tx: tokio::sync::mpsc::UnboundedSender<crate::session::commands::SessionCommand>,
        active_goal_id: Option<String>,
    ) -> Self {
        Self {
            state: std::sync::Arc::new(parking_lot::Mutex::new(GoalUsageWindowState {
                provider_window: active_goal_id
                    .map(GoalProviderWindow::Active)
                    .unwrap_or_default(),
                ..GoalUsageWindowState::default()
            })),
            settlement_changed: std::sync::Arc::new(tokio::sync::Notify::new()),
            root_cmd_tx: root_cmd_tx.downgrade(),
        }
    }

    pub(crate) fn sync(&self, active_goal_id: Option<String>) {
        self.sync_with_goal_state(active_goal_id, false, false);
    }

    fn sync_with_goal_state(
        &self,
        active_goal_id: Option<String>,
        exhausted: bool,
        usage_incomplete: bool,
    ) {
        self.state.lock().provider_window = match (active_goal_id, exhausted, usage_incomplete) {
            (Some(goal_id), _, true) => GoalProviderWindow::UsageIncomplete(goal_id),
            (Some(goal_id), true, false) => GoalProviderWindow::Exhausted(goal_id),
            (Some(goal_id), false, false) => GoalProviderWindow::Active(goal_id),
            (None, _, _) => GoalProviderWindow::Inactive,
        };
    }

    pub(crate) fn active_goal_id(&self) -> Option<String> {
        match &self.state.lock().provider_window {
            GoalProviderWindow::Active(goal_id) => Some(goal_id.clone()),
            GoalProviderWindow::Inactive
            | GoalProviderWindow::Exhausted(_)
            | GoalProviderWindow::UsageIncomplete(_) => None,
        }
    }

    pub(crate) fn provider_admission_closed(&self) -> bool {
        matches!(
            &self.state.lock().provider_window,
            GoalProviderWindow::Exhausted(_) | GoalProviderWindow::UsageIncomplete(_)
        )
    }

    pub(crate) fn usage_incomplete_goal_id(&self) -> Option<String> {
        match &self.state.lock().provider_window {
            GoalProviderWindow::UsageIncomplete(goal_id) => Some(goal_id.clone()),
            GoalProviderWindow::Inactive
            | GoalProviderWindow::Active(_)
            | GoalProviderWindow::Exhausted(_) => None,
        }
    }

    /// Stop admitting new provider work for an exhausted Goal without changing
    /// its durable lifecycle inside the current step. Attempts that already
    /// captured the Goal remain in `pending_attempts` and still settle exactly
    /// once; StepEnded owns the later BudgetLimited Control transition.
    pub(crate) fn close_goal_admission(&self, goal_id: &str) -> bool {
        let mut state = self.state.lock();
        if !matches!(
            &state.provider_window,
            GoalProviderWindow::Active(active) if active == goal_id
        ) {
            return false;
        }
        state.provider_window = GoalProviderWindow::Exhausted(goal_id.to_owned());
        true
    }

    pub(crate) fn owner_epoch(&self, session_id: &str) -> u64 {
        self.state
            .lock()
            .owner_epochs
            .get(session_id)
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn advance_owner_epoch(&self, session_id: &str) -> u64 {
        let mut state = self.state.lock();
        let epoch = state.owner_epochs.entry(session_id.to_owned()).or_default();
        *epoch = epoch.saturating_add(1);
        *epoch
    }

    /// Admit one real provider attempt into the Goal usage ledger. The opaque
    /// id, rather than the mutable active Goal id, follows the attempt through
    /// retries, Stop, and owner-future destruction. A later provider admission
    /// can wait for this id to settle without keeping the cancelling UI open.
    pub(crate) fn begin_model_attempt(
        &self,
        session_id: &str,
        epoch: u64,
        expected_goal_id: Option<&str>,
    ) -> Result<Option<String>, String> {
        let mut state = self.state.lock();
        let current_epoch = state.owner_epochs.get(session_id).copied().unwrap_or(0);
        if epoch != current_epoch {
            return Err(format!(
                "provider admission belongs to closed session epoch {epoch} (current {current_epoch})"
            ));
        }
        let active_goal_id = match &state.provider_window {
            GoalProviderWindow::Active(goal_id) => Some(goal_id.clone()),
            GoalProviderWindow::Inactive => None,
            GoalProviderWindow::Exhausted(goal_id) => {
                return Err(format!(
                    "provider admission is closed because Goal {goal_id} exhausted its token budget"
                ));
            }
            GoalProviderWindow::UsageIncomplete(goal_id) => {
                return Err(format!(
                    "provider admission is closed because Goal {goal_id} has incomplete token usage"
                ));
            }
        };
        let goal_id = match (expected_goal_id, active_goal_id) {
            (Some(expected), Some(active)) if expected == active => active,
            (Some(expected), active) => {
                return Err(format!(
                    "provider admission belongs to closed Goal {expected} (active Goal: {})",
                    active.as_deref().unwrap_or("none")
                ));
            }
            (None, Some(active)) => active,
            (None, None) => return Ok(None),
        };
        let attempt_id = uuid::Uuid::now_v7().to_string();
        state.pending_attempts.insert(
            attempt_id.clone(),
            GoalUsageAttemptOwner {
                goal_id,
                session_id: session_id.to_owned(),
                epoch,
                settlement: None,
            },
        );
        Ok(Some(attempt_id))
    }

    pub(crate) async fn wait_for_owner_settlements_through(&self, session_id: &str, epoch: u64) {
        loop {
            let changed = self.settlement_changed.notified();
            let pending_prior = self
                .state
                .lock()
                .pending_attempts
                .values()
                .any(|attempt| attempt.session_id == session_id && attempt.epoch <= epoch);
            if !pending_prior {
                return;
            }
            changed.await;
        }
    }

    pub(crate) fn attempt_goal_id(&self, attempt_id: &str) -> Option<String> {
        self.state
            .lock()
            .pending_attempts
            .get(attempt_id)
            .map(|attempt| attempt.goal_id.clone())
    }

    pub(crate) fn finish_attempt(&self, attempt_id: &str) -> bool {
        let removed = self
            .state
            .lock()
            .pending_attempts
            .remove(attempt_id)
            .is_some();
        if removed {
            self.settlement_changed.notify_waiters();
        }
        removed
    }

    pub(super) fn claim_attempt_settlement(&self, attempt_id: &str, tokens: Option<i64>) -> bool {
        let mut state = self.state.lock();
        let Some(attempt) = state.pending_attempts.get_mut(attempt_id) else {
            return false;
        };
        if attempt.settlement.is_none() {
            // `Some(0)` is an exact, provider-confirmed no-inference result
            // (for example a strict request-capability rejection). Preserve
            // it distinctly from `None`, which means usage is unknown and
            // must close the Goal window.
            attempt.settlement = Some(tokens.map(|tokens| tokens.max(0)));
        }
        true
    }

    pub(super) fn attempt_settlement(&self, attempt_id: &str) -> Option<(String, Option<i64>)> {
        let state = self.state.lock();
        let attempt = state.pending_attempts.get(attempt_id)?;
        Some((attempt.goal_id.clone(), attempt.settlement?))
    }

    /// Close the global active window and claim every unreported provider call
    /// as usage-incomplete. Calls that already reported exact usage retain that
    /// result. The root actor owns the returned ids and must durably settle
    /// them before exiting.
    fn close_and_claim_pending_for_shutdown(&self) -> Vec<String> {
        let mut state = self.state.lock();
        state.provider_window = GoalProviderWindow::Inactive;
        for attempt in state.pending_attempts.values_mut() {
            if attempt.settlement.is_none() {
                attempt.settlement = Some(None);
            }
        }
        state.pending_attempts.keys().cloned().collect()
    }

    /// Claim only one descendant session's unresolved attempts. Descendant
    /// teardown must never close the root-owned active window or consume a
    /// sibling's accounting state.
    fn claim_pending_for_owner_shutdown(&self, session_id: &str) -> Vec<String> {
        let mut state = self.state.lock();
        state
            .pending_attempts
            .iter_mut()
            .filter_map(|(attempt_id, attempt)| {
                if attempt.session_id != session_id {
                    return None;
                }
                if attempt.settlement.is_none() {
                    attempt.settlement = Some(None);
                }
                Some(attempt_id.clone())
            })
            .collect()
    }

    /// Settle an attempt through the lifecycle root. This path is independent
    /// of the turn/Sideband future that issued the provider request, so a hard
    /// Stop can drop that owner without losing Known/Incomplete accounting.
    pub(crate) async fn settle_attempt_via_root(
        &self,
        attempt_id: String,
        tokens: Option<i64>,
    ) -> Result<bool, String> {
        if !self.claim_attempt_settlement(&attempt_id, tokens) {
            return Ok(false);
        }
        let (respond_to, response) = tokio::sync::oneshot::channel();
        self.root_cmd_tx
            .upgrade()
            .ok_or_else(|| "root Goal accounting actor is unavailable".to_owned())?
            .send(
                crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                    attempt_id,
                    respond_to,
                },
            )
            .map_err(|_| "root Goal accounting actor is unavailable".to_owned())?;
        response
            .await
            .map_err(|_| "root Goal accounting acknowledgement was lost".to_owned())?
    }

    pub(crate) fn settle_attempt_detached(&self, attempt_id: String, tokens: Option<i64>) {
        // Claim synchronously before yielding to the detached acknowledgement
        // task. A subsequent admission in the same owner epoch can now see the
        // fence immediately and cannot overtake this settlement.
        if !self.claim_attempt_settlement(&attempt_id, tokens) {
            return;
        }
        let window = self.clone();
        tokio::spawn(async move {
            if let Err(error) = window.settle_attempt_via_root(attempt_id, tokens).await {
                tracing::error!(%error, "detached Goal provider-attempt settlement failed");
            }
        });
    }

    /// Submit usage produced outside the root actor (descendant main loops and
    /// sideband calls). The captured Goal id is the time-window authority; the
    /// root may process the mailbox item after a pause without losing usage
    /// that was settled while the Goal was still active.
    pub(crate) async fn submit(&self, tokens: i64) -> Result<bool, String> {
        if tokens <= 0 {
            return Ok(false);
        }
        let Some(goal_id) = self.active_goal_id() else {
            return Ok(false);
        };
        self.submit_captured(goal_id, tokens).await
    }

    pub(crate) async fn submit_captured(
        &self,
        goal_id: String,
        tokens: i64,
    ) -> Result<bool, String> {
        if tokens <= 0 {
            return Ok(false);
        }
        let (respond_to, response) = tokio::sync::oneshot::channel();
        self.root_cmd_tx
            .upgrade()
            .ok_or_else(|| "root Goal accounting actor is unavailable".to_owned())?
            .send(crate::session::commands::SessionCommand::RecordGoalUsage {
                goal_id,
                tokens,
                respond_to,
            })
            .map_err(|_| "root Goal accounting actor is unavailable".to_owned())?;
        response
            .await
            .map_err(|_| "root Goal accounting acknowledgement was lost".to_owned())?
    }

    pub(crate) async fn submit_incomplete(&self, goal_id: String) -> Result<bool, String> {
        let (respond_to, response) = tokio::sync::oneshot::channel();
        self.root_cmd_tx
            .upgrade()
            .ok_or_else(|| "root Goal accounting actor is unavailable".to_owned())?
            .send(
                crate::session::commands::SessionCommand::RecordGoalUsageIncomplete {
                    goal_id,
                    respond_to,
                },
            )
            .map_err(|_| "root Goal accounting actor is unavailable".to_owned())?;
        response
            .await
            .map_err(|_| "root Goal accounting acknowledgement was lost".to_owned())?
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
            "todo_write",
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

impl SessionActor {
    pub(super) fn goal_provider_admission_closed(&self) -> bool {
        self.goal_usage_window.provider_admission_closed()
    }

    /// Publish the lifecycle boundary only after its durable Control commit.
    /// Descendant model responses read this shared value at settlement time.
    pub(super) fn sync_goal_usage_window(&self) {
        let (active_goal_id, exhausted, usage_incomplete) = {
            let tracker = self.goal_tracker.lock();
            let goal = tracker.snapshot();
            let active =
                goal.filter(|goal| goal.status == crate::session::goal_tracker::GoalStatus::Active);
            (
                active.map(|goal| goal.goal_id.clone()),
                active
                    .and_then(|goal| goal.token_budget)
                    .is_some_and(|budget| tracker.tokens_used() >= budget),
                active.is_some_and(|goal| {
                    goal.usage_incomplete && !goal.usage_incomplete_acknowledged
                }),
            )
        };
        self.goal_usage_window
            .sync_with_goal_state(active_goal_id, exhausted, usage_incomplete);
    }

    /// Root-side accounting authority for one usage record whose Goal id was
    /// captured while the shared window was Active.
    pub(super) async fn apply_captured_goal_usage(
        &self,
        goal_id: &str,
        tokens: i64,
    ) -> Result<bool, String> {
        let _transaction = self.goal_transaction_gate.lock().await;
        let Some(previous) = self.goal_tracker.lock().snapshot().cloned() else {
            return Ok(false);
        };
        if !self.goal_tracker.lock().account_tokens(goal_id, tokens) {
            return Ok(false);
        }
        let next = self.goal_tracker.lock().snapshot().cloned();
        let behavior = self.behavior.lock().snapshot();
        if let Err(error) = self.persist_control_snapshot_durably(behavior, next).await {
            self.goal_tracker.lock().restore_runtime_snapshot(previous);
            return Err(format!("Goal usage was not persisted: {error}"));
        }
        let (tokens_used, exhausted) = {
            let tracker = self.goal_tracker.lock();
            let snapshot = tracker.snapshot();
            (
                tracker.tokens_used(),
                snapshot
                    .filter(|goal| {
                        goal.goal_id == goal_id
                            && goal.status == crate::session::goal_tracker::GoalStatus::Active
                    })
                    .and_then(|goal| goal.token_budget)
                    .is_some_and(|budget| tracker.tokens_used() >= budget),
            )
        };
        if exhausted {
            self.goal_usage_window.close_goal_admission(goal_id);
        }
        self.goal_notify_sender()
            .emit_goal_updated(&self.goal_tracker.lock(), tokens_used);
        Ok(true)
    }

    pub(super) async fn apply_captured_goal_usage_incomplete(
        &self,
        goal_id: &str,
    ) -> Result<bool, String> {
        let _boundary = self.step_control_gate.lock().await;
        let outcome = self
            .apply_captured_goal_usage_incomplete_outcome(goal_id)
            .await;
        self.finish_goal_usage_apply_at_step_boundary(outcome).await
    }

    /// Apply the turn-terminal effect of a Goal usage settlement while the
    /// caller still owns `step_control_gate`.
    pub(super) async fn finish_goal_usage_apply_at_step_boundary(
        &self,
        outcome: Result<GoalUsageIncompleteApply, String>,
    ) -> Result<bool, String> {
        let outcome = outcome?;
        if outcome == GoalUsageIncompleteApply::Stopped {
            self.arm_terminal_preemption_if_running().await;
        }
        Ok(outcome.applied())
    }

    pub(super) async fn apply_captured_goal_usage_incomplete_outcome(
        &self,
        goal_id: &str,
    ) -> Result<GoalUsageIncompleteApply, String> {
        let _transaction = self.goal_transaction_gate.lock().await;
        let Some(previous) = self.goal_tracker.lock().snapshot().cloned() else {
            return Ok(GoalUsageIncompleteApply::Ignored);
        };
        if previous.goal_id != goal_id {
            return Ok(GoalUsageIncompleteApply::Ignored);
        }
        let already_recorded = previous.usage_incomplete && !previous.usage_incomplete_acknowledged;
        let definition_revision = previous.definition_revision;
        if !already_recorded && !self.goal_tracker.lock().mark_usage_incomplete(goal_id) {
            return Ok(GoalUsageIncompleteApply::Ignored);
        }
        if !self.events.has_active_step() {
            if self.goal_tracker.lock().pause_for_incomplete_usage(goal_id) {
                self.commit_goal_stop_or_restore(previous).await?;
                let tokens_used = self.goal_tokens_used();
                self.goal_notify_sender()
                    .emit_goal_updated(&self.goal_tracker.lock(), tokens_used);
                self.retire_goal_owned_work(goal_id, definition_revision, None)
                    .await;
                return Ok(GoalUsageIncompleteApply::Stopped);
            }
            if previous.status == crate::session::goal_tracker::GoalStatus::Paused {
                return Ok(GoalUsageIncompleteApply::Stopped);
            }
        }
        if already_recorded {
            return Ok(GoalUsageIncompleteApply::Recorded);
        }
        let next = self.goal_tracker.lock().snapshot().cloned();
        let behavior = self.behavior.lock().snapshot();
        if let Err(error) = self.persist_control_snapshot_durably(behavior, next).await {
            self.goal_tracker.lock().restore_runtime_snapshot(previous);
            return Err(format!("incomplete Goal usage was not persisted: {error}"));
        }
        self.sync_goal_usage_window();
        let tokens_used = self.goal_tokens_used();
        self.goal_notify_sender()
            .emit_goal_updated(&self.goal_tracker.lock(), tokens_used);
        Ok(GoalUsageIncompleteApply::Recorded)
    }

    /// Root-only, idempotent settlement authority for provider attempts. The
    /// outcome is read from the shared window rather than the mailbox payload,
    /// so a retried command cannot change Known usage into Incomplete or apply
    /// the same response twice.
    pub(super) async fn settle_claimed_goal_usage_attempt(
        &self,
        attempt_id: &str,
    ) -> Result<bool, String> {
        let _boundary = self.step_control_gate.lock().await;
        let outcome = self
            .settle_claimed_goal_usage_attempt_outcome(attempt_id)
            .await;
        self.finish_goal_usage_apply_at_step_boundary(outcome).await
    }

    pub(super) async fn settle_claimed_goal_usage_attempt_outcome(
        &self,
        attempt_id: &str,
    ) -> Result<GoalUsageIncompleteApply, String> {
        let Some((goal_id, tokens)) = self.goal_usage_window.attempt_settlement(attempt_id) else {
            return Ok(GoalUsageIncompleteApply::Ignored);
        };
        let applied = match tokens {
            Some(tokens) => {
                if self.apply_captured_goal_usage(&goal_id, tokens).await? {
                    GoalUsageIncompleteApply::Recorded
                } else {
                    GoalUsageIncompleteApply::Ignored
                }
            }
            None => {
                self.apply_captured_goal_usage_incomplete_outcome(&goal_id)
                    .await?
            }
        };
        self.goal_usage_window.finish_attempt(attempt_id);
        Ok(applied)
    }

    /// Shutdown is the final root lifecycle barrier. No mailbox consumer exists
    /// after it returns, so it must settle every admitted attempt in-process.
    pub(super) async fn settle_goal_usage_for_shutdown(&self) -> Result<(), String> {
        if self.startup_hints.is_subagent {
            let owner_id = self.session_id_string();
            let attempt_ids = self
                .goal_usage_window
                .claim_pending_for_owner_shutdown(&owner_id);
            for attempt_id in attempt_ids {
                self.goal_usage_window
                    .settle_attempt_via_root(attempt_id, None)
                    .await?;
            }
            return Ok(());
        }
        let attempt_ids = self
            .goal_usage_window
            .close_and_claim_pending_for_shutdown();
        for attempt_id in attempt_ids {
            self.settle_claimed_goal_usage_attempt(&attempt_id).await?;
        }
        Ok(())
    }

    /// A foreground owner that unwinds cannot report any provider attempts it
    /// still owns. Claim only that session's attempts as usage-incomplete and
    /// settle them before closing its Step/Turn, so the next owner epoch never
    /// waits forever on work whose future no longer exists. The caller owns
    /// `step_control_gate`; use the outcome API so panic recovery cannot
    /// recursively acquire the same terminal fence.
    pub(super) async fn settle_goal_usage_for_owner_failure(&self) -> Result<(), String> {
        let owner_id = self.session_id_string();
        let attempt_ids = self
            .goal_usage_window
            .claim_pending_for_owner_shutdown(&owner_id);
        for attempt_id in attempt_ids {
            if self.startup_hints.is_subagent {
                self.goal_usage_window
                    .settle_attempt_via_root(attempt_id, None)
                    .await?;
            } else {
                let outcome = self
                    .settle_claimed_goal_usage_attempt_outcome(&attempt_id)
                    .await;
                self.finish_goal_usage_apply_at_step_boundary(outcome)
                    .await?;
            }
        }
        Ok(())
    }

    /// Account a main-loop response. Primary responses can update their Goal
    /// ledger immediately before turn-end budget enforcement; descendants
    /// submit the same captured charge to the lifecycle root.
    pub(super) async fn record_goal_model_usage(
        &self,
        admitted_goal_id: Option<&str>,
        tokens: i64,
    ) -> Result<bool, String> {
        if tokens <= 0 {
            return Ok(false);
        }
        let Some(goal_id) = admitted_goal_id else {
            return Ok(false);
        };
        if self.startup_hints.is_subagent {
            self.goal_usage_window
                .submit_captured(goal_id.to_owned(), tokens)
                .await
        } else {
            self.apply_captured_goal_usage(goal_id, tokens).await
        }
    }

    pub(super) async fn record_goal_usage_incomplete(&self, goal_id: &str) -> Result<bool, String> {
        if self.startup_hints.is_subagent {
            self.goal_usage_window
                .submit_incomplete(goal_id.to_owned())
                .await
        } else {
            self.apply_captured_goal_usage_incomplete(goal_id).await
        }
    }

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
            crate::session::PromptOrigin::GoalContinuation { goal_id, .. } => {
                Some(goal_id.as_str())
            }
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
                    context.view.definition_revision,
                ),
                Some(context.view),
            )
        } else {
            let (goal_snapshot, used, elapsed_ms) = if expected_goal_id != Some("") {
                let tracker = self.goal_tracker.lock();
                (
                    tracker.snapshot().cloned(),
                    tracker.tokens_used(),
                    tracker.elapsed_ms(),
                )
            } else {
                (None, 0, 0)
            };
            let goal_snapshot = goal_snapshot.filter(|goal| {
                goal.status == crate::session::goal_tracker::GoalStatus::Active
                    && expected_goal_id.is_none_or(|expected| expected == goal.goal_id)
            });
            goal_snapshot
                .as_ref()
                .map(|goal| {
                    (
                        tools::implementations::grow_build::task::types::SubagentOwner::goal(
                            &goal.goal_id,
                            goal.definition_revision,
                        ),
                        Some(super::goal::goal_view_from_snapshot(goal, used, elapsed_ms)),
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

    pub(super) async fn publish_goal_mutation_authority(&self, prompt_id: &str, prompt_index: u64) {
        let bridge = self.agent.borrow().tool_bridge().clone();
        let goal = self
            .goal_tracker
            .lock()
            .snapshot()
            .map(|goal| (goal.goal_id.clone(), goal.definition_revision));
        let authority = tools::implementations::grow_build::update_goal::GoalMutationAuthority {
            prompt_id: prompt_id.to_string(),
            prompt_index,
            control_revision: self
                .control_revision
                .load(std::sync::atomic::Ordering::SeqCst),
            goal,
        };
        bridge
            .update_resource(
                tools::implementations::grow_build::update_goal::GoalMutationAuthorityResource(
                    Some(authority),
                ),
            )
            .await;
    }

    /// Rebind Goal-derived tool authority after a definition Control reaches a
    /// step boundary. The prompt identity stays fixed for the outer turn, while
    /// the objective revision and delegation snapshot advance together for the
    /// next model sample. Delegated child sessions keep their immutable
    /// inherited snapshot and never enter this root-only refresh path.
    pub(super) async fn refresh_goal_step_resources(&self) {
        let bridge = self.agent.borrow().tool_bridge().clone();
        if bridge
            .read_resource::<tools::implementations::grow_build::update_goal::GoalContextSnapshotResource>()
            .await
            .is_some_and(|resource| resource.0.is_some())
        {
            return;
        }
        let Some(mut authority) = bridge
            .read_resource::<tools::implementations::grow_build::update_goal::GoalMutationAuthorityResource>()
            .await
            .and_then(|resource| resource.0)
        else {
            return;
        };
        let (goal, used, elapsed_ms) = {
            let tracker = self.goal_tracker.lock();
            (
                tracker.snapshot().cloned(),
                tracker.tokens_used(),
                tracker.elapsed_ms(),
            )
        };
        let active = goal.filter(|goal| {
            goal.status == crate::session::goal_tracker::GoalStatus::Active
                && self.behavior.lock().behavior() == tool_types::BehaviorId::Goal
        });
        authority.control_revision = self
            .control_revision
            .load(std::sync::atomic::Ordering::SeqCst);
        authority.goal = active
            .as_ref()
            .map(|goal| (goal.goal_id.clone(), goal.definition_revision));
        let (owner, delegation) = active
            .as_ref()
            .map(|goal| {
                (
                    tools::implementations::grow_build::task::types::SubagentOwner::goal(
                        &goal.goal_id,
                        goal.definition_revision,
                    ),
                    Some(super::goal::goal_view_from_snapshot(goal, used, elapsed_ms)),
                )
            })
            .unwrap_or_default();
        bridge
            .update_resource(
                tools::implementations::grow_build::update_goal::GoalMutationAuthorityResource(
                    Some(authority),
                ),
            )
            .await;
        bridge
            .update_resource(
                tools::implementations::grow_build::task::types::CurrentSubagentOwnerResource(
                    owner,
                ),
            )
            .await;
        bridge
            .update_resource(
                tools::implementations::grow_build::update_goal::GoalDelegationSnapshotResource(
                    delegation,
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
        let behavior_id = behavior.behavior();
        let context = crate::session::behavior::behavior_transition_context(behavior_id);
        let mut contexts = vec![(
            chat_state::ControlContextLayer::Behavior,
            chat_state::ControlContextActivation::Transition,
            sampling_types::ConversationItem::system_reminder(context),
        )];
        if let Some(plan_phase) = self.plan_phase_control_context(&behavior)? {
            contexts.push(plan_phase);
        }
        self.persist_control_snapshot_with_contexts_durably(behavior, goal, None, contexts)
            .await
    }

    pub(super) async fn persist_behavior_transition_for_control_durably(
        &self,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: Option<crate::session::goal_tracker::GoalState>,
        intent: crate::session::ControlIntent,
    ) -> std::io::Result<()> {
        let behavior_id = behavior.behavior();
        let context = crate::session::behavior::behavior_transition_context(behavior_id);
        let mut contexts = vec![(
            chat_state::ControlContextLayer::Behavior,
            chat_state::ControlContextActivation::Transition,
            sampling_types::ConversationItem::system_reminder(context),
        )];
        if let Some(plan_phase) = self.plan_phase_control_context(&behavior)? {
            contexts.push(plan_phase);
        }
        self.persist_control_snapshot_with_contexts_and_receipt_durably(
            behavior,
            goal,
            None,
            contexts,
            Some(crate::session::control::DurableControlReceipt {
                domain: crate::extensions::notification::ControlDomain::Behavior,
                intent,
                target: crate::extensions::notification::ControlTarget::Behavior {
                    behavior_id: behavior_id.as_id().to_owned(),
                },
            }),
        )
        .await
    }

    pub(super) async fn persist_plan_phase_transition_durably(
        &self,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: Option<crate::session::goal_tracker::GoalState>,
    ) -> std::io::Result<()> {
        let context = self.plan_phase_control_context(&behavior)?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Plan phase transition requires active Plan behavior",
            )
        })?;
        self.persist_control_snapshot_with_contexts_durably(behavior, goal, None, vec![context])
            .await
    }

    fn plan_phase_control_context(
        &self,
        behavior: &crate::session::behavior::BehaviorSnapshot,
    ) -> std::io::Result<
        Option<(
            chat_state::ControlContextLayer,
            chat_state::ControlContextActivation,
            sampling_types::ConversationItem,
        )>,
    > {
        if behavior.behavior() != tool_types::BehaviorId::Plan {
            return Ok(None);
        }
        let plan_content = match behavior.plan_artifact_hash.as_deref() {
            Some(hash) => {
                crate::session::behavior::read_plan_artifact(&self.session_directory, hash)?
            }
            None => String::new(),
        };
        let context = crate::session::behavior::plan_phase_model_context(behavior, &plan_content)
            .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "active Plan snapshot has no Plan phase context",
            )
        })?;
        Ok(Some((
            chat_state::ControlContextLayer::PlanPhase,
            chat_state::ControlContextActivation::Transition,
            sampling_types::ConversationItem::system_reminder(context),
        )))
    }

    pub(super) async fn persist_applied_control_receipt_durably(
        &self,
        domain: crate::extensions::notification::ControlDomain,
        target: crate::extensions::notification::ControlTarget,
        intent: crate::session::ControlIntent,
    ) -> std::io::Result<()> {
        self.persist_control_snapshot_with_contexts_and_receipt_durably(
            self.behavior.lock().snapshot(),
            self.goal_tracker.lock().snapshot().cloned(),
            None,
            Vec::new(),
            Some(crate::session::control::DurableControlReceipt {
                domain,
                intent,
                target,
            }),
        )
        .await
    }

    pub(super) async fn persist_behavior_and_goal_transition_durably(
        &self,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: crate::session::goal_tracker::GoalState,
        goal_context: sampling_types::ConversationItem,
    ) -> std::io::Result<()> {
        let behavior_context =
            crate::session::behavior::behavior_transition_context(behavior.behavior());
        self.persist_control_snapshot_with_contexts_durably(
            behavior,
            Some(goal),
            None,
            vec![
                (
                    chat_state::ControlContextLayer::Behavior,
                    chat_state::ControlContextActivation::Transition,
                    sampling_types::ConversationItem::system_reminder(behavior_context),
                ),
                (
                    chat_state::ControlContextLayer::GoalDefinition,
                    chat_state::ControlContextActivation::Transition,
                    goal_context,
                ),
            ],
        )
        .await
    }

    pub(super) async fn persist_agent_transition_durably(
        &self,
        agent_name: &str,
        role_prompt: Option<&str>,
        capability_catalog: Option<&str>,
    ) -> std::io::Result<()> {
        let context = crate::session::control::agent_role_transition_context(
            agent_name,
            role_prompt,
            capability_catalog,
        );
        let behavior = self.behavior.lock().snapshot();
        let goal = self.goal_tracker.lock().snapshot().cloned();
        self.persist_control_snapshot_with_context_durably(
            behavior,
            goal,
            Some(agent_name),
            Some((
                chat_state::ControlContextLayer::AgentRole,
                chat_state::ControlContextActivation::Transition,
                sampling_types::ConversationItem::system_reminder(context),
            )),
        )
        .await
    }

    pub(super) async fn persist_agent_transition_for_control_durably(
        &self,
        agent_name: &str,
        role_prompt: Option<&str>,
        capability_catalog: Option<&str>,
        intent: crate::session::ControlIntent,
    ) -> std::io::Result<()> {
        let context = crate::session::control::agent_role_transition_context(
            agent_name,
            role_prompt,
            capability_catalog,
        );
        self.persist_control_snapshot_with_contexts_and_receipt_durably(
            self.behavior.lock().snapshot(),
            self.goal_tracker.lock().snapshot().cloned(),
            Some(agent_name),
            vec![(
                chat_state::ControlContextLayer::AgentRole,
                chat_state::ControlContextActivation::Transition,
                sampling_types::ConversationItem::system_reminder(context),
            )],
            Some(crate::session::control::DurableControlReceipt {
                domain: crate::extensions::notification::ControlDomain::Agent,
                intent,
                target: crate::extensions::notification::ControlTarget::Agent {
                    agent_name: agent_name.to_owned(),
                },
            }),
        )
        .await
    }

    pub(super) async fn persist_goal_definition_transition_durably(
        &self,
        goal: crate::session::goal_tracker::GoalState,
        context: sampling_types::ConversationItem,
    ) -> std::io::Result<()> {
        let behavior = self.behavior.lock().snapshot();
        self.persist_control_snapshot_with_context_durably(
            behavior,
            Some(goal),
            None,
            Some((
                chat_state::ControlContextLayer::GoalDefinition,
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
        let _transaction = self.goal_transaction_gate.lock().await;
        let contexts = contexts
            .into_iter()
            .map(|(layer, context)| {
                (
                    layer,
                    chat_state::ControlContextActivation::Reprojection,
                    context,
                )
            })
            .collect::<Vec<_>>();
        if contexts.is_empty() {
            return Ok(());
        }
        let agent_name = self.agent.borrow().definition().selector_identity();
        let behavior = self.behavior.lock().snapshot();
        let goal = self.goal_tracker.lock().snapshot().cloned();
        self.persist_control_snapshot_with_contexts_durably(
            behavior,
            goal,
            Some(&agent_name),
            contexts,
        )
        .await
    }

    /// Restore active Control layers whose latest model context was shadowed
    /// by a committed compaction. The Control snapshot remains the fact source;
    /// this appends a new typed projection rather than reviving an older item.
    pub(super) async fn repair_missing_control_contexts_durably(&self) -> std::io::Result<()> {
        let materialized = self
            .chat_state_handle
            .materialize_timeline(self.session_id_string())
            .await
            .ok_or_else(|| {
                std::io::Error::other(
                    "Timeline materialization is unavailable during Control context reconciliation",
                )
            })?;
        let current = materialized
            .surface_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let mut missing = materialized
            .active_control_contexts
            .into_iter()
            .filter_map(|(layer, context)| {
                (!current.contains(&context.surface_id)).then_some((
                    context.surface_id,
                    layer,
                    context.item,
                ))
            })
            .collect::<Vec<_>>();
        // Layer ordering is an implementation detail; replay the surviving
        // authorities in the order their original Surface anchors became
        // effective. This matters across layers: for example an Agent change
        // accepted after a Behavior transition must remain later after
        // compaction, and an active Goal's Behavior/definition pair must keep
        // the exact order committed by its atomic Control event.
        missing.sort_by_key(|(source, _, _)| *source);
        let missing = missing
            .into_iter()
            .map(|(_, layer, item)| (layer, item))
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
            sampling_types::ConversationItem,
        )>,
    ) -> std::io::Result<()> {
        let model_contexts = model_context.into_iter().collect();
        self.persist_control_snapshot_with_contexts_durably(
            behavior,
            goal,
            agent_name,
            model_contexts,
        )
        .await
    }

    pub(super) async fn persist_control_snapshot_with_contexts_durably(
        &self,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: Option<crate::session::goal_tracker::GoalState>,
        agent_name: Option<&str>,
        model_contexts: Vec<(
            chat_state::ControlContextLayer,
            chat_state::ControlContextActivation,
            sampling_types::ConversationItem,
        )>,
    ) -> std::io::Result<()> {
        self.persist_control_snapshot_with_contexts_and_receipt_durably(
            behavior,
            goal,
            agent_name,
            model_contexts,
            None,
        )
        .await
    }

    async fn persist_control_snapshot_with_contexts_and_receipt_durably(
        &self,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: Option<crate::session::goal_tracker::GoalState>,
        agent_name: Option<&str>,
        model_contexts: Vec<(
            chat_state::ControlContextLayer,
            chat_state::ControlContextActivation,
            sampling_types::ConversationItem,
        )>,
        applied_control: Option<crate::session::control::DurableControlReceipt>,
    ) -> std::io::Result<()> {
        let revision = self
            .control_revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .saturating_add(1);
        let agent_name = agent_name
            .map(str::to_owned)
            .unwrap_or_else(|| self.agent.borrow().definition().selector_identity());
        let mut state = crate::session::control::SessionControlSnapshot::new(
            revision, agent_name, behavior, goal,
        );
        state.applied_control = applied_control;
        let contexts = model_contexts
            .into_iter()
            .map(|(layer, activation, item)| chat_state::ControlContext {
                layer,
                activation,
                item,
            })
            .collect();
        let kind = state.timeline_kind_with_model_context_items(contexts)?;
        self.chat_state_handle
            .record_timeline_event_durably(kind)
            .await
            .map(|_| ())
            .map_err(std::io::Error::other)
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
        self.sync_goal_usage_window();
        self.send_available_commands_update().await;
        Ok(())
    }

    /// Snapshot the only Goal state that is allowed to survive actor teardown,
    /// then let the caller issue the persistence barrier.
    pub(super) async fn checkpoint_goal_before_shutdown(&self) {
        let _transaction = self.goal_transaction_gate.lock().await;
        if self.goal_tracker.lock().snapshot().is_none() {
            return;
        }
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

    /// Bind task ids to the immutable Goal owner captured when their producing
    /// tool batch was admitted. Delayed results must never re-sample whichever
    /// Goal happens to be active when they arrive.
    pub(super) fn record_goal_owned_task_ids(
        &self,
        goal_id: &str,
        definition_revision: u64,
        ids: impl IntoIterator<Item = String>,
    ) {
        self.goal_turn_task_ids.lock().extend(
            ids.into_iter()
                .map(|task_id| (task_id, (goal_id.to_owned(), definition_revision))),
        );
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
                "Goal runtime paused because one or more required Goal tools are unavailable. Re-enable create_goal, get_goal, update_goal, and todo_write before restarting."
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

    /// Revoke every runtime descendant and background terminal task admitted
    /// under one Goal owner. Cancellation admission completes inline, while
    /// coordinator drain is observed by a detached waiter: child shutdown must
    /// remain free to submit its final usage fold through the Session mailbox.
    pub(super) async fn cancel_goal_owned_work(&self, goal_id: &str, definition_revision: u64) {
        if let Some(event_tx) = self.tool_context.subagent_event_tx.clone() {
            use tools::implementations::grow_build::task::types::{
                SubagentCancelRequest, SubagentCancelTarget, SubagentEvent,
            };
            let (respond_to, response) = tokio::sync::oneshot::channel();
            if event_tx
                .send(SubagentEvent::Cancel(SubagentCancelRequest {
                    parent_session_id: Some(self.session_id_string()),
                    target: SubagentCancelTarget::Goal {
                        goal_id: goal_id.to_owned(),
                        definition_revision,
                    },
                    respond_to,
                }))
                .is_ok()
            {
                let owner = goal_id.to_owned();
                tokio::task::spawn_local(async move {
                    match tokio::time::timeout(std::time::Duration::from_secs(30), response).await {
                        Ok(Ok(_)) => {}
                        Ok(Err(_)) => {
                            tracing::warn!(goal_id = owner, "Goal cancellation drain was dropped");
                        }
                        Err(_) => {
                            tracing::error!(
                                goal_id = owner,
                                "timed out draining Goal-owned subagents"
                            );
                        }
                    }
                });
            }
        }

        self.sweep_goal_owned_terminal_work(goal_id, definition_revision)
            .await;
    }

    pub(super) async fn sweep_goal_owned_terminal_work(
        &self,
        goal_id: &str,
        definition_revision: u64,
    ) {
        let task_ids = self
            .goal_turn_task_ids
            .lock()
            .iter()
            .filter_map(|(task_id, owner)| {
                (owner == &(goal_id.to_owned(), definition_revision)).then(|| task_id.clone())
            })
            .collect::<std::collections::HashSet<_>>();
        let bridge = self.agent.borrow().tool_bridge().clone();
        let goal_id = goal_id.to_owned();
        // Goal lifecycle and its provider-admission tombstone are already
        // durable here. Process termination is cleanup, not part of that
        // transaction: awaiting N terminal kills on the root actor would
        // starve Stop and provider-usage settlement for up to N reap
        // timeouts. Discover late task handles and kill the whole set
        // concurrently in a detached local task instead.
        tokio::task::spawn_local(async move {
            let mut task_ids = task_ids;
            if let Some(tasks) = bridge.list_tasks().await {
                task_ids.extend(tasks.into_iter().filter_map(|task| {
                    (task.goal_id.as_deref() == Some(goal_id.as_str())
                        && task.goal_definition_revision == Some(definition_revision)
                        && task.is_outstanding())
                    .then_some(task.task_id)
                }));
            }
            futures_util::future::join_all(task_ids.into_iter().map(|task_id| {
                let bridge = bridge.clone();
                let goal_id = goal_id.clone();
                async move {
                    if let Err(error) = bridge.kill_background_task(&task_id).await {
                        tracing::warn!(
                            %error,
                            goal_id,
                            task_id,
                            "failed to stop Goal-owned task"
                        );
                    }
                }
            }))
            .await;
        });
    }

    /// Close the exact producing turn before sweeping the Goal owner. The
    /// prompt cancellation is an epoch-scoped coordinator admission tombstone: a
    /// detached Task spawn that arrives after the Goal sweep is rejected
    /// instead of resurrecting work from the retired turn.
    pub(super) async fn retire_goal_owned_work(
        &self,
        goal_id: &str,
        definition_revision: u64,
        parent_prompt_id: Option<&str>,
    ) {
        if let Some(parent_prompt_id) = parent_prompt_id {
            self.cancel_running_turn_subagents(parent_prompt_id);
        }
        self.cancel_goal_owned_work(goal_id, definition_revision)
            .await;
    }

    pub(crate) fn goal_tokens_used(&self) -> i64 {
        self.goal_tracker.lock().tokens_used()
    }

    pub(super) fn set_goal_loop_active(&self, active: bool) {
        self.tool_context
            .goal_loop_active_gate
            .store(active, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::actor::tests::support::{
        begin_test_active_causal_turn_with_origin, build_actor,
    };

    async fn build_active_goal_turn(
        prompt_id: &str,
        origin: crate::session::PromptOrigin,
        turn_kind: crate::session::TurnKind,
    ) -> std::sync::Arc<SessionActor> {
        let (actor, _gateway_rx) = build_actor().await;
        actor
            .goal_tracker
            .lock()
            .create_goal(
                "goal-1".into(),
                "finish".into(),
                None,
                "2026-08-27T00:00:00Z".into(),
            )
            .unwrap();
        actor
            .behavior
            .lock()
            .select_behavior(tool_types::BehaviorId::Goal);
        actor.sync_goal_usage_window();
        begin_test_active_causal_turn_with_origin(&actor, prompt_id, origin, turn_kind).await;
        actor
    }

    #[tokio::test(flavor = "current_thread")]
    async fn known_usage_waits_for_the_step_budget_fence() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = build_actor().await;
                actor
                    .goal_tracker
                    .lock()
                    .create_goal(
                        "goal-1".into(),
                        "finish".into(),
                        Some(10),
                        "2026-08-27T00:00:00Z".into(),
                    )
                    .unwrap();
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Goal);
                actor.sync_goal_usage_window();
                crate::session::actor::tests::support::begin_test_causal_turn(&actor).await;
                let attempt_id = actor
                    .goal_usage_window
                    .begin_model_attempt(&actor.session_id_string(), 0, Some("goal-1"))
                    .unwrap()
                    .unwrap();
                assert!(
                    actor
                        .goal_usage_window
                        .claim_attempt_settlement(&attempt_id, Some(12))
                );

                assert!(
                    actor
                        .settle_claimed_goal_usage_attempt(&attempt_id)
                        .await
                        .unwrap()
                );
                assert_eq!(
                    actor.goal_tracker.lock().status(),
                    Some(crate::session::goal_tracker::GoalStatus::Active),
                    "usage settlement must not mutate Goal lifecycle inside an active step"
                );
                assert_eq!(actor.goal_usage_window.active_goal_id(), None);
                assert!(
                    actor
                        .goal_usage_window
                        .begin_model_attempt(&actor.session_id_string(), 0, Some("goal-1"),)
                        .is_err(),
                    "known budget exhaustion must close provider admission before StepEnded"
                );
                assert!(
                    actor
                        .goal_usage_window
                        .begin_model_attempt(&actor.session_id_string(), 0, None)
                        .is_err(),
                    "an unbound descendant or sideband cannot bypass the exhausted global window"
                );
                assert!(
                    !actor
                        .settle_claimed_goal_usage_attempt(&attempt_id)
                        .await
                        .unwrap(),
                    "late duplicate settlement remains idempotent"
                );

                assert!(actor.events.end_step("goal_budget_exhausted"));
                assert!(actor.enforce_goal_spending_limit().await);
                assert_eq!(
                    actor.goal_tracker.lock().status(),
                    Some(crate::session::goal_tracker::GoalStatus::BudgetLimited)
                );
                let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
                let step_end = timeline
                    .iter()
                    .position(|event| {
                        matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Step(
                                chat_state::StepEvent::Ended { .. }
                            )
                        )
                    })
                    .unwrap();
                let terminal_control = timeline
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Control(control)
                                if control.model_contexts.iter().any(|context| {
                                    context.layer == chat_state::ControlContextLayer::Behavior
                                })
                        )
                    })
                    .unwrap();
                assert!(step_end < terminal_control);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn incomplete_usage_closes_admission_then_pauses_after_step_end() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = build_actor().await;
                actor
                    .goal_tracker
                    .lock()
                    .create_goal(
                        "goal-1".into(),
                        "finish".into(),
                        None,
                        "2026-08-27T00:00:00Z".into(),
                    )
                    .unwrap();
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Goal);
                actor.sync_goal_usage_window();
                crate::session::actor::tests::support::begin_test_causal_turn(&actor).await;
                let attempt_id = actor
                    .goal_usage_window
                    .begin_model_attempt(&actor.session_id_string(), 0, Some("goal-1"))
                    .unwrap()
                    .unwrap();
                assert!(
                    actor
                        .goal_usage_window
                        .claim_attempt_settlement(&attempt_id, None)
                );

                actor
                    .settle_claimed_goal_usage_attempt(&attempt_id)
                    .await
                    .unwrap();
                let pending = actor.goal_tracker.lock().snapshot().cloned().unwrap();
                assert!(pending.usage_incomplete);
                assert_eq!(
                    pending.status,
                    crate::session::goal_tracker::GoalStatus::Active
                );
                assert_eq!(
                    actor.behavior.lock().behavior(),
                    tool_types::BehaviorId::Goal
                );
                assert!(
                    actor
                        .goal_usage_window
                        .begin_model_attempt(&actor.session_id_string(), 0, None)
                        .is_err()
                );

                assert!(actor.events.end_step("goal_usage_incomplete"));
                let _ = actor.enforce_goal_spending_limit().await;
                assert_eq!(
                    actor.goal_tracker.lock().status(),
                    Some(crate::session::goal_tracker::GoalStatus::Paused)
                );
                assert_eq!(
                    actor.behavior.lock().behavior(),
                    tool_types::BehaviorId::Normal
                );
                let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
                let step_end = timeline
                    .iter()
                    .position(|event| {
                        matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Step(
                                chat_state::StepEvent::Ended { .. }
                            )
                        )
                    })
                    .unwrap();
                let terminal_control = timeline
                    .iter()
                    .position(|event| {
                        matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Control(control)
                                if control.model_contexts.iter().any(|context| {
                                    context.layer == chat_state::ControlContextLayer::Behavior
                                })
                        )
                    })
                    .unwrap();
                assert!(step_end < terminal_control);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn incomplete_usage_in_step_gap_preempts_user_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let prompt_id = "user-goal-turn";
                let actor = build_active_goal_turn(
                    prompt_id,
                    crate::session::PromptOrigin::User,
                    crate::session::TurnKind::User,
                )
                .await;
                actor
                    .end_step_control_boundary("usage_settled")
                    .await
                    .expect("the active Step must end before the settlement gap");

                let gate = actor.step_control_gate.lock().await;
                let outcome = actor
                    .apply_captured_goal_usage_incomplete_outcome("goal-1")
                    .await
                    .unwrap();
                assert_eq!(outcome, GoalUsageIncompleteApply::Stopped);
                assert!(
                    actor
                        .finish_goal_usage_apply_at_step_boundary(Ok(outcome))
                        .await
                        .unwrap()
                );
                drop(gate);

                assert_eq!(actor.events.current_goal_id(), None);
                assert_eq!(
                    actor.goal_tracker.lock().status(),
                    Some(crate::session::goal_tracker::GoalStatus::Paused)
                );
                assert_eq!(
                    actor.behavior.lock().behavior(),
                    tool_types::BehaviorId::Normal
                );
                let state = actor.state.lock().await;
                assert!(state.terminal_preemption_pending);
                assert!(!state.can_continue_regular_turn(prompt_id));
                assert!(!actor.events.has_active_step());
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn recorded_incomplete_retry_in_step_gap_preempts_goal_continuation() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let prompt_id = "goal-continuation";
                let actor = build_active_goal_turn(
                    prompt_id,
                    crate::session::PromptOrigin::GoalContinuation {
                        goal_id: "goal-1".into(),
                        definition_revision: 1,
                    },
                    crate::session::TurnKind::Internal,
                )
                .await;

                let gate = actor.step_control_gate.lock().await;
                let recorded = actor
                    .apply_captured_goal_usage_incomplete_outcome("goal-1")
                    .await
                    .unwrap();
                assert_eq!(recorded, GoalUsageIncompleteApply::Recorded);
                assert!(
                    actor
                        .finish_goal_usage_apply_at_step_boundary(Ok(recorded))
                        .await
                        .unwrap()
                );
                assert!(!actor.state.lock().await.terminal_preemption_pending);
                drop(gate);

                actor
                    .end_step_control_boundary("usage_settled")
                    .await
                    .expect("the active Step must end before the retry gap");
                let gate = actor.step_control_gate.lock().await;
                let stopped = actor
                    .apply_captured_goal_usage_incomplete_outcome("goal-1")
                    .await
                    .unwrap();
                assert_eq!(stopped, GoalUsageIncompleteApply::Stopped);
                assert!(
                    actor
                        .finish_goal_usage_apply_at_step_boundary(Ok(stopped))
                        .await
                        .unwrap()
                );
                drop(gate);

                assert_eq!(actor.events.current_goal_id().as_deref(), Some("goal-1"));
                let state = actor.state.lock().await;
                assert!(state.terminal_preemption_pending);
                assert!(!state.can_continue_regular_turn(prompt_id));
                assert!(!actor.events.has_active_step());
            })
            .await;
    }

    #[tokio::test]
    async fn descendant_usage_is_emitted_only_inside_active_windows() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let window = GoalUsageWindow::new(tx.clone(), Some("goal-1".into()));

        let submitted = tokio::spawn({
            let window = window.clone();
            async move { window.submit(30).await }
        });
        let respond_to = match rx.recv().await.expect("usage command") {
            crate::session::commands::SessionCommand::RecordGoalUsage {
                goal_id,
                tokens: 30,
                respond_to,
            } if goal_id == "goal-1" => respond_to,
            _ => panic!("unexpected Goal usage command"),
        };
        let _ = respond_to.send(Ok(true));
        assert!(submitted.await.unwrap().unwrap());

        window.sync(None);
        assert!(!window.submit(40).await.unwrap());
        assert!(rx.try_recv().is_err(), "paused usage must not be emitted");

        window.sync(Some("goal-1".into()));
        let submitted = tokio::spawn({
            let window = window.clone();
            async move { window.submit(50).await }
        });
        let respond_to = match rx.recv().await.expect("usage command") {
            crate::session::commands::SessionCommand::RecordGoalUsage {
                goal_id,
                tokens: 50,
                respond_to,
            } if goal_id == "goal-1" => respond_to,
            _ => panic!("unexpected Goal usage command"),
        };
        let _ = respond_to.send(Ok(true));
        assert!(submitted.await.unwrap().unwrap());
    }

    #[tokio::test]
    async fn later_provider_admission_waits_for_the_exact_attempt_ack() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let window = GoalUsageWindow::new(tx.clone(), Some("goal-1".into()));
        let attempt_id = window
            .begin_model_attempt("root", 0, Some("goal-1"))
            .unwrap()
            .expect("active Goal attempt");
        let next_epoch = window.advance_owner_epoch("root");
        window.sync(None);

        let waiter = tokio::spawn({
            let window = window.clone();
            async move {
                window
                    .wait_for_owner_settlements_through("root", next_epoch)
                    .await
            }
        });
        tokio::task::yield_now().await;
        assert!(
            !waiter.is_finished(),
            "unsettled usage must fence admission"
        );

        let settlement_attempt_id = attempt_id.clone();
        let settlement = tokio::spawn({
            let window = window.clone();
            async move {
                window
                    .settle_attempt_via_root(settlement_attempt_id, Some(17))
                    .await
            }
        });
        let respond_to = match rx.recv().await.expect("usage command") {
            crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                attempt_id: command_attempt_id,
                respond_to,
            } if command_attempt_id == attempt_id => respond_to,
            _ => panic!("unexpected Goal usage command"),
        };
        assert_eq!(
            window.attempt_settlement(&attempt_id),
            Some(("goal-1".into(), Some(17)))
        );
        window.finish_attempt(&attempt_id);
        respond_to.send(Ok(true)).unwrap();
        assert!(settlement.await.unwrap().unwrap());
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("admission fence released")
            .unwrap();
    }

    #[tokio::test]
    async fn detached_drop_claims_before_same_epoch_admission_can_overtake() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let window = GoalUsageWindow::new(tx.clone(), Some("goal-1".into()));
        let _mailbox_owner = tx;
        let attempt_id = window
            .begin_model_attempt("root", 0, Some("goal-1"))
            .unwrap()
            .unwrap();

        window.settle_attempt_detached(attempt_id.clone(), None);
        assert_eq!(
            window.attempt_settlement(&attempt_id),
            Some(("goal-1".into(), None)),
            "Drop must claim incomplete usage synchronously"
        );
        let waiter = tokio::spawn({
            let window = window.clone();
            async move { window.wait_for_owner_settlements_through("root", 0).await }
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished(), "same-epoch work stays fenced");

        let respond_to = match rx.recv().await.expect("detached settlement command") {
            crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                attempt_id: command_attempt_id,
                respond_to,
            } if command_attempt_id == attempt_id => respond_to,
            _ => panic!("unexpected Goal usage command"),
        };
        window.finish_attempt(&attempt_id);
        respond_to.send(Ok(true)).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("same-epoch fence released after root acknowledgement")
            .unwrap();
    }

    #[tokio::test]
    async fn usage_window_does_not_keep_the_root_mailbox_alive() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let window = GoalUsageWindow::new(tx.clone(), Some("goal-1".into()));
        drop(tx);

        assert!(rx.recv().await.is_none());
        assert_eq!(
            window.submit(10).await,
            Err("root Goal accounting actor is unavailable".into())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn descendant_shutdown_settles_only_its_owner_without_closing_the_goal_window() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut actor, _gateway_rx) = build_actor().await;
                let actor_mut =
                    std::sync::Arc::get_mut(&mut actor).expect("test fixture has one actor owner");
                actor_mut.startup_hints.is_subagent = true;
                let (root_tx, mut root_rx) = tokio::sync::mpsc::unbounded_channel();
                let window = GoalUsageWindow::new(root_tx.clone(), Some("goal-1".into()));
                let _root_mailbox_owner = root_tx;
                actor_mut.goal_usage_window = window.clone();
                let child_id = actor_mut.session_id_string();

                let root_attempt = window
                    .begin_model_attempt("root-session", 0, Some("goal-1"))
                    .unwrap()
                    .unwrap();
                let child_attempt = window
                    .begin_model_attempt(&child_id, 0, Some("goal-1"))
                    .unwrap()
                    .unwrap();

                let child_shutdown = tokio::task::spawn_local({
                    let actor = actor.clone();
                    async move { actor.settle_goal_usage_for_shutdown().await }
                });
                let command = root_rx.recv().await.expect("child settlement command");
                let crate::session::commands::SessionCommand::SettleGoalUsageAttempt {
                    attempt_id,
                    respond_to,
                } = command
                else {
                    panic!("unexpected child settlement command");
                };
                assert_eq!(attempt_id, child_attempt);
                assert_eq!(
                    window.attempt_settlement(&child_attempt),
                    Some(("goal-1".into(), None))
                );
                assert!(window.attempt_goal_id(&root_attempt).is_some());
                assert_eq!(window.active_goal_id().as_deref(), Some("goal-1"));
                assert!(
                    window
                        .begin_model_attempt("root-session", 0, Some("goal-1"))
                        .unwrap()
                        .is_some(),
                    "child teardown must not close root provider admission"
                );

                window.finish_attempt(&child_attempt);
                respond_to.send(Ok(true)).unwrap();
                child_shutdown.await.unwrap().unwrap();
            })
            .await;
    }

    #[test]
    fn root_shutdown_preserves_claimed_known_usage_and_fail_closes_unreported_calls() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let window = GoalUsageWindow::new(tx, Some("goal-1".into()));
        let known = window
            .begin_model_attempt("root", 0, Some("goal-1"))
            .unwrap()
            .unwrap();
        let unknown = window
            .begin_model_attempt("child", 0, Some("goal-1"))
            .unwrap()
            .unwrap();
        assert!(window.claim_attempt_settlement(&known, Some(73)));

        let claimed = window.close_and_claim_pending_for_shutdown();
        assert!(claimed.contains(&known));
        assert!(claimed.contains(&unknown));
        assert_eq!(
            window.attempt_settlement(&known),
            Some(("goal-1".into(), Some(73)))
        );
        assert_eq!(
            window.attempt_settlement(&unknown),
            Some(("goal-1".into(), None))
        );
        assert_eq!(window.active_goal_id(), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn control_reconciliation_fails_when_timeline_cannot_be_materialized() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut actor, _gateway_rx) = build_actor().await;
                std::sync::Arc::get_mut(&mut actor)
                    .expect("test fixture must have unique actor ownership")
                    .chat_state_handle = chat_state::ChatStateHandle::noop();

                let error = actor
                    .repair_missing_control_contexts_durably()
                    .await
                    .expect_err("missing Timeline materialization must fail closed");

                assert!(
                    error
                        .to_string()
                        .contains("Timeline materialization is unavailable")
                );
            })
            .await;
    }
}

//! Outbound update emission concern for `SessionActor`: `send_update` and
//! its buffered/transient/direct variants, Grow-notification handling, and
//! the gateway-bridge dispatch shims.
use super::*;
use crate::extensions::notification::SessionUpdate as GrowSessionUpdate;

fn timeline_hook_event_name(event: chat_state::HookEventType) -> &'static str {
    match event {
        chat_state::HookEventType::SessionStart => "session_start",
        chat_state::HookEventType::UserPromptSubmit => "user_prompt_submit",
        chat_state::HookEventType::PreToolUse => "pre_tool_use",
        chat_state::HookEventType::PostToolUse => "post_tool_use",
        chat_state::HookEventType::PostToolUseFailure => "post_tool_use_failure",
        chat_state::HookEventType::PermissionDenied => "permission_denied",
        chat_state::HookEventType::Stop => "stop",
        chat_state::HookEventType::StopFailure => "stop_failure",
        chat_state::HookEventType::StopCancelled => "stop_cancelled",
        chat_state::HookEventType::Notification => "notification",
        chat_state::HookEventType::SubagentStart => "subagent_start",
        chat_state::HookEventType::SubagentStop => "subagent_stop",
        chat_state::HookEventType::PreCompact => "pre_compact",
        chat_state::HookEventType::PostCompact => "post_compact",
        chat_state::HookEventType::SessionEnd => "session_end",
    }
}
/// Result of applying a subagent fold into parent ledgers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SubagentUsageApply {
    /// Tokens attributed to the live open prompt (and session).
    AttributedToPrompt,
    /// Tokens landed on the session ledger only (pin mismatch / no live pin).
    /// Sticky report only — do not stain ledgers for "missing" spend.
    SessionOnly,
}
impl SessionActor {
    /// Apply subagent usage. `Ok` after chat-state acked; `Err` if apply failed.
    pub(super) async fn record_subagent_usage(
        &self,
        _subagent_id: &str,
        by_model: &[(String, chat_state::UsageTotals)],
        parent_prompt_id: Option<&str>,
        incomplete: bool,
    ) -> Result<SubagentUsageApply, ()> {
        let current = self
            .current_prompt_id
            .lock()
            .expect("current_prompt_id mutex poisoned")
            .clone();
        let attributable = parent_prompt_id.is_some() && parent_prompt_id == current.as_deref();
        if (!by_model.is_empty() || incomplete)
            && !self
                .chat_state_handle
                .record_subagent_usage(by_model.to_vec(), attributable, incomplete)
                .await
        {
            return Err(());
        }
        Ok(if attributable {
            SubagentUsageApply::AttributedToPrompt
        } else {
            SubagentUsageApply::SessionOnly
        })
    }

    /// True-miss / unpinned fail-closed: sticky for freeze report + pin-aware
    /// ledger marks. Prompt ledger is stained only when the stamped pin is the
    /// live open prompt (never stain a different live turn). Session always.
    pub(super) async fn mark_apply_miss_incomplete(&self, stamped_pin: Option<&str>) -> bool {
        let sticky = self.mark_subagent_usage_not_applied(stamped_pin).await;
        let live = self.current_prompt_id.lock().ok().and_then(|g| g.clone());
        let stain_prompt = match (stamped_pin, live.as_deref()) {
            (Some(pin), Some(live_id)) => pin == live_id,
            (Some(_), None) => false,
            (None, Some(_)) => true,
            (None, None) => false,
        };
        let ledger_ok = self
            .chat_state_handle
            .mark_usage_incomplete(stain_prompt, true)
            .await;
        sticky || ledger_ok
    }
    /// Shared freeze/cancel finalize: ledger marks only on `fail_closed`;
    /// sticky/bg are report-only. Clears sticky after snapshot.
    pub(super) async fn finalize_usage_from_outcome(
        &self,
        prompt_id: &str,
        outcome: super::turn::UsageDrainOutcome,
    ) -> Option<crate::extensions::notification::PromptUsage> {
        if outcome.fail_closed {
            let _ = self
                .chat_state_handle
                .mark_usage_incomplete(true, true)
                .await;
        }
        let usage = self
            .snapshot_prompt_usage_marked(outcome.report_incomplete())
            .await;
        self.clear_subagent_usage_not_applied(prompt_id);
        usage
    }
    /// Sends an update to the persistence layer and the gateway.
    /// Optionally includes a `chunk_index` for LLM streaming chunk tracking.
    pub(super) async fn send_update(&self, update: acp::SessionUpdate, chunk_index: Option<u64>) {
        self.send_update_full(update, chunk_index, None, false)
            .await;
    }
    async fn send_update_full(
        &self,
        update: acp::SessionUpdate,
        chunk_index: Option<u64>,
        agent_timestamp_ms_override: Option<i64>,
        is_replay: bool,
    ) {
        self.close_rewind_window().await;
        if let acp::SessionUpdate::ToolCall(tool_call) = &update
            && matches!(tool_call.kind, acp::ToolKind::Edit)
        {
            let cwd = self.tool_context.cwd.as_path();
            for loc in &tool_call.locations {
                let mut p = loc.path.clone();
                if p.is_absolute() {
                    if let Ok(rel) = p.strip_prefix(cwd) {
                        p = rel.to_path_buf();
                    } else {
                        continue;
                    }
                }
                if !p.as_os_str().is_empty() {
                    self.chat_state_handle
                        .record_agent_edited_path(p.to_string_lossy().to_string());
                }
            }
        }
        let total_tokens = self.chat_state_handle.get_projected_tokens().await;
        let meta_info = self.chat_state_handle.get_notification_meta().await;
        let (stream_start_ms, turn_start_ms) = meta_info
            .map(|m| (m.stream_start_ms, m.turn_start_ms))
            .unwrap_or((None, None));
        let event_id = self.generate_event_id();
        let agent_timestamp_ms =
            agent_timestamp_ms_override.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        let (update_type, update_params) = Self::extract_update_info(&update);
        let mut meta = json!({
            "totalTokens": total_tokens,
            "eventId": event_id,
            "agentTimestampMs": agent_timestamp_ms,
        });
        let obj = meta
            .as_object_mut()
            .expect("json! literal is always an Object");
        if let Some(pid) = self.current_prompt_id.lock().ok().and_then(|g| g.clone()) {
            obj.insert("promptId".to_string(), pid.into());
        }
        if let Some(ms) = stream_start_ms {
            obj.insert("streamStartMs".to_string(), ms.into());
        }
        if let Some(ms) = turn_start_ms {
            obj.insert("turnStartMs".to_string(), ms.into());
        }
        if let Some(update_type) = update_type {
            obj.insert("updateType".to_string(), update_type.into());
        }
        if let Some(update_params) = update_params {
            obj.insert("updateParams".to_string(), update_params);
        }
        if let Some(idx) = chunk_index {
            obj.insert("chunkId".to_string(), idx.into());
        }
        if is_replay {
            obj.insert("isReplay".to_string(), true.into());
        }
        let notification = acp::SessionNotification::new(self.session_info.id.clone(), update)
            .meta(meta.as_object().cloned());
        let _ = self
            .event_tx
            .send(SessionEvent::Notification(notification.into()));
    }
    /// Producer for the **high-frequency streaming path** with an Grow
    /// extension payload. Routes through `event_tx` -> `ReplayBuffer` ->
    /// `emit_buffered` so chunks get merged + debounced + emitted.
    ///
    /// For one-shot Grow events (RetryState, ImageCompressed,
    /// AutoCompactCompleted, etc.), use `send_grow_notification` instead.
    ///
    /// The frequency-based split (`send_buffered_grow_update` vs `send_grow_notification`)
    /// mirrors the ACP-side split between `send_update` (high-frequency,
    /// buffered) and `emit_notification_direct` (low-frequency, direct).
    pub(super) async fn send_buffered_grow_update(&self, update: GrowSessionUpdate) {
        self.close_rewind_window().await;
        let notification = GrowSessionNotification {
            session_id: self.session_info.id.clone(),
            update,
            meta: None,
        };
        let _ = self
            .event_tx
            .send(SessionEvent::Notification(notification.into()));
    }
    /// Enqueue a `CurrentModeUpdate` on the FIFO event pipeline, stamped at
    /// enqueue time like `send_update`, so its id is minted in delivery order
    /// relative to already-queued chunks. A direct `emit_notification_direct`
    /// here would mint a HIGHER id that is delivered BEFORE those chunks, and
    /// the client's in-order dedup would then drop the chunks as stale —
    /// silent text loss when a mode update follows queued output. Persist + broadcast
    /// happen when the actor loop drains the event through `emit_buffered`.
    pub(super) fn enqueue_current_mode_update(&self, current_mode_id: acp::SessionModeId) {
        self.enqueue_current_mode_update_inner(current_mode_id, None);
    }
    pub(super) fn enqueue_current_mode_update_with_behavior_change(
        &self,
        current_mode_id: acp::SessionModeId,
        behavior_change: serde_json::Value,
    ) {
        self.enqueue_current_mode_update_inner(current_mode_id, Some(behavior_change));
    }
    fn enqueue_current_mode_update_inner(
        &self,
        current_mode_id: acp::SessionModeId,
        behavior_change: Option<serde_json::Value>,
    ) {
        let behavior_meta = serde_json::json!({
            "grow/behavior": match self.behavior.lock().behavior() {
                tool_types::BehaviorId::Normal => "normal",
                tool_types::BehaviorId::Clarify => "clarify",
                tool_types::BehaviorId::Plan => "plan",
                tool_types::BehaviorId::Workflow => "workflow",
                tool_types::BehaviorId::Goal => "goal",
            },
            "grow/planPhase": self.behavior.lock().plan_phase_label(),
            "grow/behaviorChange": behavior_change,
        });
        let notification = acp::SessionNotification::new(
            self.session_info.id.clone(),
            acp::SessionUpdate::CurrentModeUpdate(
                acp::CurrentModeUpdate::new(current_mode_id)
                    .meta(behavior_meta.as_object().cloned()),
            ),
        )
        .meta(self.build_notification_meta().as_object().cloned());
        let _ = self
            .event_tx
            .send(SessionEvent::Notification(notification.into()));
    }
    /// Emit a notification that has come out of the **high-frequency
    /// streaming path** (after the `ReplayBuffer` has decided to flush
    /// it). Single dispatch point that routes by inner protocol kind:
    ///
    /// - **ACP** (`AgentMessageChunk`, `AgentThoughtChunk`) ->
    ///   delegates to `emit_notification_direct` (persists + gateway).
    /// - **Grow** (`ToolCallDeltaChunk`) -> inlines a gateway
    ///   forward as `ExtNotification` only. Two deliberate omissions:
    ///   (1) no persistence -- per-chunk deltas have no replay value
    ///   because the canonical `acp::SessionUpdate::ToolCall` (with
    ///   assembled `raw_input`) is persisted at end-of-turn and is the
    ///   source of truth for replay; (2) no hook dispatch.
    pub(super) async fn emit_buffered(&self, notification: SessionNotification) {
        match notification {
            SessionNotification::Acp(n) => {
                self.emit_notification_direct(*n).await;
            }
            SessionNotification::Grow(n) => {
                self.log_outbound_grow_buffered(&n);
                if self
                    .notifications
                    .gateway_enabled
                    .load(std::sync::atomic::Ordering::Relaxed)
                    && let Ok(value) = serde_json::to_value(&*n)
                    && let Ok(params) = serde_json::value::to_raw_value(&value)
                {
                    self.notifications
                        .gateway
                        .forward_fire_and_forget(acp::ExtNotification::new(
                            "grow/session_notification",
                            params.into(),
                        ));
                }
            }
        }
    }
    /// Tracing log for buffered Grow notifications emerging from
    /// emit_buffered. Mirrors `log_outbound_notification` for ACP.
    /// Visible with `RUST_LOG=acp_event=info`.
    fn log_outbound_grow_buffered(&self, notification: &GrowSessionNotification) {
        if !matches!(
            notification.update,
            GrowSessionUpdate::ToolCallDeltaChunk { .. }
        ) {
            return;
        }
        tracing::info!(
            target: "acp_event",
            event = "grow_buffered_notification_sent",
            session_id = %self.session_info.id,
            "Sending buffered Grow session notification"
        );
    }
    fn log_outbound_notification(&self, notification: &acp::SessionNotification) {
        let meta = notification.meta.as_ref();
        let event_id = meta
            .and_then(|m| m.get("eventId"))
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        let agent_timestamp_ms = meta
            .and_then(|m| m.get("agentTimestampMs"))
            .and_then(|v| v.as_i64().or_else(|| v.as_u64().map(|u| u as i64)))
            .unwrap_or(0);
        let update_type = meta
            .and_then(|m| m.get("updateType"))
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        let chunk_index = meta
            .and_then(|m| m.get("chunkIndex"))
            .and_then(|v| v.as_u64());
        tracing::info!(
            target: "acp_event",
            event = "agent_message_sent",
            event_id = %event_id,
            session_id = %self.session_info.id,
            agent_timestamp_ms = agent_timestamp_ms,
            update_type = %update_type,
            chunk_index = ?chunk_index,
            "Sending session update"
        );
    }
    pub(crate) async fn emit_notification_direct(
        &self,
        mut notification: acp::SessionNotification,
    ) {
        crate::util::event_id::ensure_event_id_meta(
            &self.session_info.id.0,
            &mut notification.meta,
        );
        self.log_outbound_notification(&notification);
        if !matches!(
            notification.update,
            acp::SessionUpdate::AvailableCommandsUpdate(_)
        ) {
            let _ = self
                .notifications
                .persistence_tx
                .send(PersistenceMsg::Update(
                    crate::session::storage::SessionUpdate::Acp(Box::new(notification.clone())),
                ));
        }
        if self
            .notifications
            .gateway_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            self.notifications
                .gateway
                .forward_fire_and_forget(notification);
        }
    }
    /// Send a notification to the live client **without persisting** it.
    ///
    /// Use this for cosmetic/transient UI updates (e.g., turn-end plan
    /// cleanup) that should NOT be replayed on session reload.  The
    /// underlying resource state is the source of truth; this only
    /// adjusts what the live client sees right now.
    pub(super) fn emit_transient_notification(&self, notification: acp::SessionNotification) {
        self.log_outbound_notification(&notification);
        if self
            .notifications
            .gateway_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            self.notifications
                .gateway
                .forward_fire_and_forget(notification);
        }
    }

    /// Publish the canonical request-pressure projection to the live client.
    /// This is ephemeral UI state: it must neither enter model context nor be
    /// replayed as immutable transcript history.
    pub(super) fn emit_context_pressure_update(&self, projected_tokens: u64) {
        let update = acp::SessionInfoUpdate::new().meta(
            serde_json::json!({ "grow/contextPressure": true })
                .as_object()
                .cloned(),
        );
        let notification = acp::SessionNotification::new(
            self.session_info.id.clone(),
            acp::SessionUpdate::SessionInfoUpdate(update),
        )
        .meta(
            serde_json::json!({
                "totalTokens": projected_tokens,
                "agentTimestampMs": chrono::Utc::now().timestamp_millis(),
                "transient": true,
            })
            .as_object()
            .cloned(),
        );
        self.emit_transient_notification(notification);
    }
    /// Flush buffered notifications and drain the persistence merge buffer to
    /// disk. Blocks until the persistence actor confirms the write is complete.
    ///
    /// Must NOT be called from within `run_session()` — the flush goes through
    /// `event_tx`, which is consumed by the same select loop (deadlock / 5s timeout).
    pub(super) async fn flush_to_disk(&self) {
        if let Err(e) = crate::session::replay_events::flush_replay_actor(&self.event_tx).await {
            tracing::warn!(?e, "flush_replay_actor failed");
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        if self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::FlushAndAck { respond_to: tx })
            .is_ok()
        {
            let _ = rx.await;
        }
    }
    /// Extracts the update type name and relevant parameters for logging
    fn extract_update_info(
        update: &acp::SessionUpdate,
    ) -> (Option<String>, Option<serde_json::Value>) {
        match update {
            acp::SessionUpdate::UserMessageChunk(_) => (Some("UserMessageChunk".to_string()), None),
            acp::SessionUpdate::AgentMessageChunk(_) => {
                (Some("AgentMessageChunk".to_string()), None)
            }
            acp::SessionUpdate::AgentThoughtChunk(_) => {
                (Some("AgentThoughtChunk".to_string()), None)
            }
            acp::SessionUpdate::ToolCall(tool_call) => (
                Some("ToolCall".to_string()),
                Some(json!({
                    "toolCallId": tool_call.tool_call_id.0,
                    "title": tool_call.title,
                    "kind": format!("{:?}", tool_call.kind),
                    "status": format!("{:?}", tool_call.status),
                })),
            ),
            acp::SessionUpdate::ToolCallUpdate(tool_update) => (
                Some("ToolCallUpdate".to_string()),
                Some(json!({
                    "toolCallId": tool_update.tool_call_id.0,
                    "status": tool_update.fields.status.as_ref().map(|s| format!("{:?}", s)),
                })),
            ),
            acp::SessionUpdate::Plan(plan) => (
                Some("Plan".to_string()),
                Some(json!({
                    "planSteps": plan.entries.len(),
                })),
            ),
            acp::SessionUpdate::AvailableCommandsUpdate(update) => (
                Some("AvailableCommandsUpdate".to_string()),
                Some(json!({
                    "commandsCount": update.available_commands.len(),
                })),
            ),
            acp::SessionUpdate::CurrentModeUpdate(update) => (
                Some("CurrentModeUpdate".to_string()),
                Some(json!({
                    "currentModeId": update.current_mode_id,
                })),
            ),
            _ => (None, None),
        }
    }
    /// Generates a unique event ID for correlation across agent and client
    fn generate_event_id(&self) -> String {
        crate::util::event_id::generate_event_id(&self.session_info.id.0)
    }
    /// Builds notification meta with event ID and timestamp.
    /// Use this for all notifications (including user message chunks) to ensure
    /// consistent event ID format for client-side deduplication.
    pub(super) fn build_notification_meta(&self) -> serde_json::Value {
        let event_id = self.generate_event_id();
        let agent_timestamp_ms = chrono::Utc::now().timestamp_millis();
        json!({
            "eventId": event_id,
            "agentTimestampMs": agent_timestamp_ms,
        })
    }
    /// Handle Grow session notifications - store them in persistence
    /// These are client-side events (like diff reviews) that should be part of session history.
    /// Exception: `SubagentProgress` ticks are transient and return before the store.
    pub(super) async fn handle_grow_session_notification(
        self: &std::sync::Arc<Self>,
        mut notification: GrowSessionNotification,
    ) -> Result<(), chat_state::TimelineWriteError> {
        if !matches!(
            notification.update,
            GrowSessionUpdate::SubagentProgress { .. }
        ) {
            tracing::debug!("storing Grow session notification");
        }
        {
            let mut meta_map = notification.meta.take().and_then(|v| match v {
                serde_json::Value::Object(m) => Some(m),
                _ => None,
            });
            crate::util::event_id::ensure_event_id_meta(&self.session_info.id.0, &mut meta_map);
            notification.meta = meta_map.map(serde_json::Value::Object);
        }
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::Update(
                crate::session::storage::SessionUpdate::Grow(Box::new(notification.clone())),
            ));
        if let GrowSessionUpdate::SubagentSpawned {
            subagent_id,
            subagent_type,
            description,
            ..
        } = &notification.update
        {
            self.dispatch_observe_hook(
                ::hooks::event::HookEventName::SubagentStart,
                chat_state::HookCause::Subagent {
                    subagent_id: subagent_id.clone(),
                },
                ::hooks::event::HookPayload::SubagentStart {
                    subagent_id: subagent_id.clone(),
                    subagent_type: subagent_type.clone(),
                    description: Some(description.clone()),
                },
                None,
            )
            .await?;
        }
        match &notification.update {
            GrowSessionUpdate::SubagentSpawned { goal_id, .. } => {
                let goal_owned = goal_id.as_deref().is_some_and(|owner_goal_id| {
                    self.goal_tracker
                        .lock()
                        .snapshot()
                        .is_some_and(|goal| goal.goal_id == owner_goal_id)
                });
                if self.goal_runtime_available() && goal_owned {
                    let tokens_used = self.goal_tokens_used();
                    let notify = self.goal_notify_sender();
                    notify.emit_goal_updated(&self.goal_tracker.lock(), tokens_used);
                }
            }
            GrowSessionUpdate::SubagentProgress { .. } => {
                // Progress reports current child context pressure and remains
                // a transient UI hint. Model-settlement usage is delivered by
                // the shared Goal usage window instead.
                return Ok(());
            }
            _ => {}
        }
        Ok(())
    }
    /// Persist an Grow extension notification to `updates.jsonl` **without** sending it
    /// to the gateway/UI. `RewindMarker` preserves the UI replay branch; it
    /// never participates in agent-state recovery.
    pub(super) fn persist_update_only(&self, update: GrowSessionUpdate) {
        let notification = GrowSessionNotification {
            session_id: self.session_info.id.clone(),
            update,
            meta: Some(self.build_notification_meta()),
        };
        if self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::Update(
                crate::session::storage::SessionUpdate::Grow(Box::new(notification)),
            ))
            .is_err()
        {
            tracing::warn!("Failed to send Grow update to persistence channel");
        }
    }
    /// Dispatch a `Notification` hook for a user-attention event.
    pub(super) async fn dispatch_notification_hook(
        &self,
        notification_type: &str,
        message: Option<String>,
        title: Option<String>,
        level: Option<String>,
    ) -> Result<(), chat_state::TimelineWriteError> {
        self.dispatch_observe_hook(
            ::hooks::event::HookEventName::Notification,
            chat_state::HookCause::Notification {
                notification_id: uuid::Uuid::now_v7().to_string(),
            },
            ::hooks::event::HookPayload::Notification {
                notification_type: notification_type.to_string(),
                message,
                title,
                level,
            },
            None,
        )
        .await
    }
    /// Send an Grow extension notification to the client
    #[tracing::instrument(skip_all)]
    pub(super) async fn send_grow_notification(&self, update: GrowSessionUpdate) {
        self.send_grow_notification_with_extra_meta(update, None)
            .await;
    }

    /// Forward ephemeral UI state without putting it in the immutable replay
    /// log. Pending/applying control phases use this path; reconnect asks the
    /// actor for a fresh authoritative snapshot instead of replaying stale
    /// progress events.
    pub(super) async fn send_transient_grow_notification(&self, update: GrowSessionUpdate) {
        let mut notification = self.build_grow_notification(update, None);
        if let Some(meta) = notification
            .meta
            .as_mut()
            .and_then(|meta| meta.as_object_mut())
        {
            // A transient event cannot be a reconnect cursor: it has no
            // durable line for replay to resolve. Keep only timing/debug
            // metadata and let the next load request a fresh snapshot.
            meta.remove("eventId");
            meta.insert("transient".to_string(), serde_json::Value::Bool(true));
        }
        self.forward_grow_notification(notification).await;
    }

    /// Forward a transient projection produced by a Hook occurrence without
    /// recursively treating that projection as a fresh Notification Hook.
    pub(super) async fn send_transient_hook_notification(&self, update: GrowSessionUpdate) {
        self.close_rewind_window().await;
        self.send_transient_passive_notification(update);
    }

    /// Re-publish already durable UI state without another audit event, hook,
    /// or interaction boundary. A transient snapshot cannot be a replay cursor.
    pub(super) fn send_transient_passive_notification(&self, update: GrowSessionUpdate) {
        let mut notification = self.build_grow_notification(update, None);
        if let Some(meta) = notification
            .meta
            .as_mut()
            .and_then(|meta| meta.as_object_mut())
        {
            meta.remove("eventId");
            meta.insert("transient".to_string(), serde_json::Value::Bool(true));
        }
        self.forward_grow_notification_unhooked(notification);
    }

    /// Re-publish Hook display projections after `session/load` rebuilt the
    /// client transcript. Hook transports stay transient: completed Timeline
    /// occurrences are queried again instead of copied to `updates.jsonl`.
    pub(super) async fn publish_completed_hook_projections(&self) {
        for projection in self.chat_state_handle.completed_hook_projections().await {
            self.send_hook_execution(
                timeline_hook_event_name(projection.event),
                None,
                None,
                &projection,
            )
            .await;
        }
    }

    /// Persist an audit fact without injecting a second live UI notification.
    pub(super) async fn persist_grow_audit_notification(
        &self,
        update: GrowSessionUpdate,
    ) -> Result<(), crate::session::persistence::DurableAppendError> {
        self.append_grow_notification_exact(self.build_grow_notification(update, None))
            .await
    }

    /// Persist and forward a passive UI/audit update without changing rewind
    /// interaction state. UI projections such as permission audit and command
    /// output are observable facts, not conversation or user-action boundaries.
    pub(super) async fn send_grow_passive_notification(
        &self,
        durable_update: GrowSessionUpdate,
        live_update: GrowSessionUpdate,
    ) -> Result<(), crate::session::persistence::DurableAppendError> {
        let durable_notification = self.build_grow_notification(durable_update, None);
        // Both projections are one logical event. Reusing the stamped metadata
        // keeps the live reconnect cursor resolvable against the durable line.
        let mut live_notification = durable_notification.clone();
        live_notification.update = live_update;
        self.append_grow_notification_exact(durable_notification)
            .await?;
        self.forward_grow_notification(live_notification).await;
        Ok(())
    }

    /// Append one already-stamped immutable UI fact until its durability is
    /// known. Retrying the exact event id resolves acknowledgement loss
    /// idempotently; a cursor-bearing live projection is never emitted first.
    async fn append_grow_notification_exact(
        &self,
        notification: GrowSessionNotification,
    ) -> Result<(), crate::session::persistence::DurableAppendError> {
        use crate::session::persistence::DurableAppendError;
        let mut attempts = 0_u32;
        loop {
            let append = self.notifications.append_update_durably(
                crate::session::storage::SessionUpdate::Grow(Box::new(notification.clone())),
            );
            let result = tokio::select! {
                biased;
                _ = self.durable_ui_cancel.cancelled() => {
                    return Err(DurableAppendError::NotCommitted(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "session stopped before durable UI event append",
                    )));
                }
                result = append => result,
            };
            match result {
                Ok(()) => return Ok(()),
                Err(error) if error.retry_exact() => {
                    attempts = attempts.saturating_add(1);
                    if attempts == 1 || attempts % 10 == 0 {
                        tracing::warn!(attempts, %error, "durable UI event append uncertain; retrying exact event");
                    }
                    tokio::select! {
                        biased;
                        _ = self.durable_ui_cancel.cancelled() => {
                            return Err(DurableAppendError::NotCommitted(std::io::Error::new(
                                std::io::ErrorKind::Interrupted,
                                "session stopped during durable UI event retry",
                            )));
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Persist a derived UI branch fact with the same durable barrier used by
    /// visible passive events, but do not forward it to the live client.
    pub(super) async fn persist_update_only_durably(
        &self,
        update: GrowSessionUpdate,
    ) -> Result<(), crate::session::persistence::DurableAppendError> {
        let notification = GrowSessionNotification {
            session_id: self.session_info.id.clone(),
            update,
            meta: Some(self.build_notification_meta()),
        };
        self.append_grow_notification_exact(notification).await
    }
    /// [`Self::send_grow_notification`] with caller-supplied `_meta` keys merged
    /// Build the per-response boundary update, projecting the response's usage
    /// into the Messages API `message.usage` shape (uncached `input_tokens`).
    pub(super) fn response_completed_update(
        &self,
        response: &sampling_types::ConversationResponse,
    ) -> GrowSessionUpdate {
        let usage =
            response
                .usage
                .as_ref()
                .map(|u| crate::extensions::notification::ResponseUsage {
                    input_tokens: u64::from(
                        u.prompt_tokens
                            .saturating_sub(u.cached_prompt_tokens)
                            .saturating_sub(u.cache_creation_prompt_tokens),
                    ),
                    output_tokens: u64::from(u.completion_tokens),
                    cache_read_input_tokens: u64::from(u.cached_prompt_tokens),
                    cache_creation_input_tokens: u64::from(u.cache_creation_prompt_tokens),
                    reasoning_tokens: u64::from(u.reasoning_tokens),
                });
        let signature = response
            .reasoning_items()
            .find_map(|r| r.encrypted_content.clone());
        GrowSessionUpdate::ResponseCompleted {
            message_id: response.message_id.clone(),
            stop_reason: response.raw_stop_reason.clone(),
            usage,
            signature,
            stop_sequence: response.stop_sequence.clone(),
        }
    }
    /// [`Self::send_grow_notification`] with caller-supplied `_meta` keys merged
    /// into the standard eventId/timestamp meta. Caller keys win on collision.
    #[tracing::instrument(skip_all)]
    pub(super) async fn send_grow_notification_with_extra_meta(
        &self,
        update: GrowSessionUpdate,
        extra_meta: Option<serde_json::Map<String, serde_json::Value>>,
    ) {
        self.close_rewind_window().await;
        let notification = self.build_grow_notification(update, extra_meta);
        let _ = self
            .notifications
            .persistence_tx
            .send(PersistenceMsg::Update(
                crate::session::storage::SessionUpdate::Grow(Box::new(notification.clone())),
            ));
        self.forward_grow_notification(notification).await;
    }

    pub(super) fn build_grow_notification(
        &self,
        update: GrowSessionUpdate,
        extra_meta: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> GrowSessionNotification {
        let mut meta = self.build_notification_meta();
        if let (Some(obj), Some(extra)) = (meta.as_object_mut(), extra_meta) {
            obj.extend(extra);
        }
        GrowSessionNotification {
            session_id: self.session_info.id.clone(),
            update,
            meta: Some(meta),
        }
    }

    pub(super) async fn forward_grow_notification(&self, notification: GrowSessionNotification) {
        if let Some((notification_type, message, title, level)) =
            notification_hook_for_update(&notification.update)
            && let Err(error) = self
                .dispatch_notification_hook(&notification_type, message, title, level)
                .await
        {
            tracing::error!(%error, "notification hook lifecycle was not durable; withholding notification");
            return;
        }
        self.forward_grow_notification_unhooked(notification);
    }

    fn forward_grow_notification_unhooked(&self, notification: GrowSessionNotification) {
        let params = serde_json::to_value(&notification)
            .and_then(|v| serde_json::value::to_raw_value(&v))
            .ok();
        if let Some(params) = params {
            let ext_notification =
                acp::ExtNotification::new("grow/session_notification", params.into());
            self.notifications
                .gateway
                .forward_fire_and_forget(ext_notification);
        }
    }
}

#[cfg(test)]
fn acking_persistence_channel() -> (
    tokio::sync::mpsc::UnboundedSender<PersistenceMsg>,
    tokio::sync::mpsc::UnboundedReceiver<PersistenceMsg>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (observed_tx, observed_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_local(async move {
        while let Some(message) = rx.recv().await {
            match message {
                PersistenceMsg::TimelineDurablyAndAck { event, respond_to } => {
                    let (observed_reply, _observed_ack) = tokio::sync::oneshot::channel();
                    let _ = observed_tx.send(PersistenceMsg::TimelineDurablyAndAck {
                        event,
                        respond_to: observed_reply,
                    });
                    let _ = respond_to.send(Ok(()));
                }
                other => {
                    let _ = observed_tx.send(other);
                }
            }
        }
    });
    (tx, observed_rx)
}

#[cfg(test)]
mod grow_event_id_stamping_tests {
    use super::super::tests::support::create_test_actor;
    use super::*;
    async fn persisted_grow_event_id(
        prx: &mut tokio::sync::mpsc::UnboundedReceiver<PersistenceMsg>,
    ) -> String {
        loop {
            match prx.recv().await.expect("an Grow line must be persisted") {
                PersistenceMsg::Update(crate::session::storage::SessionUpdate::Grow(notif)) => {
                    return notif
                        .meta
                        .as_ref()
                        .and_then(|m| m.get("eventId"))
                        .and_then(|v| v.as_str())
                        .expect("persisted Grow lines must carry an eventId")
                        .to_string();
                }
                _ => continue,
            }
        }
    }
    async fn persisted_acp_lines(
        prx: &mut tokio::sync::mpsc::UnboundedReceiver<PersistenceMsg>,
        expected: usize,
    ) -> Vec<acp::SessionNotification> {
        let mut persisted = Vec::with_capacity(expected);
        while persisted.len() < expected {
            let message = tokio::time::timeout(std::time::Duration::from_secs(1), prx.recv())
                .await
                .expect("persisted ACP lines must arrive")
                .expect("persistence observation channel must remain open");
            if let PersistenceMsg::Update(crate::session::storage::SessionUpdate::Acp(n)) = message
            {
                persisted.push(*n);
            }
        }
        persisted
    }
    /// Persisted⇒stamped chokepoint at the actor: both actor persist paths —
    /// `send_grow_notification` (own emission) and
    /// `handle_grow_session_notification` (inbound/forwarded, meta-less) —
    /// must put an `eventId` on the persisted line. An id-less line degrades
    /// every later cursor reconnect of the session to a full replay.
    #[tokio::test]
    async fn actor_persisted_grow_lines_carry_event_id() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, mut prx) =
                    tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
                let actor = std::sync::Arc::new(
                    create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await,
                );
                actor
                    .send_grow_notification(GrowSessionUpdate::RetryState(
                        crate::extensions::notification::RetryState::Retrying {
                            attempt: 1,
                            max_retries: 2,
                            reason: "own emission".into(),
                        },
                    ))
                    .await;
                let own_id = persisted_grow_event_id(&mut prx).await;
                assert!(own_id.starts_with("test-actor-"));
                actor
                    .handle_grow_session_notification(GrowSessionNotification {
                        session_id: acp::SessionId::new("test-actor"),
                        update: GrowSessionUpdate::RetryState(
                            crate::extensions::notification::RetryState::Retrying {
                                attempt: 1,
                                max_retries: 2,
                                reason: "inbound".into(),
                            },
                        ),
                        meta: None,
                    })
                    .await
                    .unwrap();
                let inbound_id = persisted_grow_event_id(&mut prx).await;
                assert!(inbound_id.starts_with("test-actor-"));
                assert_ne!(own_id, inbound_id);
                actor.persist_update_only(GrowSessionUpdate::RetryState(
                    crate::extensions::notification::RetryState::Retrying {
                        attempt: 1,
                        max_retries: 2,
                        reason: "persist-only".into(),
                    },
                ));
                let persist_only_id = persisted_grow_event_id(&mut prx).await;
                assert!(persist_only_id.starts_with("test-actor-"));
                assert_ne!(inbound_id, persist_only_id);
            })
            .await;
    }
    /// `emit_notification_direct` is the actor's ACP persist/broadcast fork:
    /// it must stamp any direct caller that didn't stamp at enqueue (none
    /// exist today — this is the safety net for the next one), so every
    /// persisted ACP line stays cursor-addressable.
    #[tokio::test]
    async fn emit_notification_direct_stamps_unstamped_acp_lines() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, mut prx) =
                    tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
                let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
                actor
                    .emit_notification_direct(acp::SessionNotification::new(
                        acp::SessionId::new("test-actor"),
                        acp::SessionUpdate::CurrentModeUpdate(acp::CurrentModeUpdate::new(
                            acp::SessionModeId::new("plan"),
                        )),
                    ))
                    .await;
                match prx.recv().await.expect("must persist") {
                    PersistenceMsg::Update(crate::session::storage::SessionUpdate::Acp(notif)) => {
                        assert!(
                            notif
                                .meta
                                .as_ref()
                                .and_then(|m| m.get("eventId"))
                                .and_then(|v| v.as_str())
                                .is_some_and(|id| id.starts_with("test-actor-")),
                            "the chokepoint must stamp meta-less ACP notifications"
                        );
                    }
                    _ => panic!("expected Acp update"),
                }
            })
            .await;
    }
    /// A plan-mode `CurrentModeUpdate` must ride the FIFO event pipeline
    /// BEHIND already-queued chunks, with its id
    /// minted at ENQUEUE time. A direct emit would mint a higher id yet
    /// deliver/persist first, and the client's in-order ACP dedup would then
    /// drop the queued chunks as stale (silent text loss).
    ///
    /// Pins the enter AND exit legs of `request_behavior_change` (each must emit —
    /// dropping either `enqueue_current_mode_update` call loses the client's
    /// mode confirmation). The abandoned site shares the same helper but needs
    /// an `ext_method` round-trip harness to drive, so it is not pinned here.
    #[tokio::test]
    async fn behavior_current_mode_update_rides_event_pipeline_in_id_order() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, mut prx) = super::acking_persistence_channel();
                let (actor, mut event_rx) = super::super::tests::support::create_test_actor_ex(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                *actor.agent.borrow_mut() =
                    super::super::tests::support::test_agent_with_plan_tools().await;
                actor
                    .send_update(
                        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                            acp::ContentBlock::Text(acp::TextContent::new("queued text")),
                        )),
                        None,
                    )
                    .await;
                actor
                    .request_behavior_change(acp::SessionModeId::new("plan"))
                    .await
                    .expect("entering plan mode must succeed in this fixture");
                while let Ok(msg) = prx.try_recv() {
                    if let PersistenceMsg::Update(crate::session::storage::SessionUpdate::Acp(
                        notification,
                    )) = msg
                    {
                        assert!(
                            !matches!(
                                notification.update,
                                acp::SessionUpdate::CurrentModeUpdate(_)
                            ),
                            "the mode update must not short-circuit the event queue"
                        );
                    }
                }
                let mut queued = Vec::new();
                while let Ok(event) = event_rx.try_recv() {
                    match event {
                        SessionEvent::Notification(n) => queued.push(n),
                        other => {
                            panic!("expected only Notification events, got {other:?}")
                        }
                    }
                }
                assert_eq!(
                    queued.len(),
                    3,
                    "chunk + mode update + refreshed command projection must be queued"
                );
                match &queued[1] {
                    SessionNotification::Acp(n) => {
                        assert!(matches!(n.update, acp::SessionUpdate::CurrentModeUpdate(_)));
                        assert!(
                            n.meta.as_ref().and_then(|m| m.get("eventId")).is_some(),
                            "the queued mode update must already carry its enqueue-time id"
                        );
                    }
                    other => {
                        panic!("expected the mode update behind the chunk, got {other:?}")
                    }
                }
                let mut replay_buffer = crate::agent::update_chunk_merge::ReplayBuffer::new(
                    actor.buffering_settings.clone(),
                );
                for notification in queued {
                    if let Some((primary, secondary)) = replay_buffer.consume_chunk(notification) {
                        actor.emit_buffered(primary).await;
                        if let Some(extra) = secondary {
                            actor.emit_buffered(extra).await;
                        }
                    }
                }
                let numeric_seq = |n: &acp::SessionNotification| -> u64 {
                    n.meta
                        .as_ref()
                        .and_then(|m| m.get("eventId"))
                        .and_then(|v| v.as_str())
                        .and_then(|id| id.rsplit('-').next())
                        .and_then(|s| s.parse().ok())
                        .expect("persisted ACP lines must carry a numeric eventId")
                };
                let persisted = persisted_acp_lines(&mut prx, 2).await;
                assert_eq!(
                    persisted.len(),
                    2,
                    "chunk and mode update must persist; command availability is transient"
                );
                assert!(matches!(
                    persisted[0].update,
                    acp::SessionUpdate::AgentMessageChunk(_)
                ));
                assert!(matches!(
                    persisted[1].update,
                    acp::SessionUpdate::CurrentModeUpdate(_)
                ));
                assert!(
                    numeric_seq(&persisted[0]) < numeric_seq(&persisted[1]),
                    "delivery order must match id order — the dedup premise"
                );
                actor
                    .send_update(
                        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                            acp::ContentBlock::Text(acp::TextContent::new("queued before exit")),
                        )),
                        None,
                    )
                    .await;
                actor
                    .request_behavior_change(acp::SessionModeId::new("normal"))
                    .await
                    .expect("first normal-mode request must succeed in this fixture");
                actor
                    .request_behavior_change(acp::SessionModeId::new("normal"))
                    .await
                    .expect("confirming normal mode must succeed in this fixture");
                while let Ok(msg) = prx.try_recv() {
                    if let PersistenceMsg::Update(crate::session::storage::SessionUpdate::Acp(
                        notification,
                    )) = msg
                    {
                        assert!(
                            !matches!(
                                notification.update,
                                acp::SessionUpdate::CurrentModeUpdate(_)
                            ),
                            "the exit mode update must not short-circuit the event queue"
                        );
                    }
                }
                let mut queued = Vec::new();
                while let Ok(event) = event_rx.try_recv() {
                    match event {
                        SessionEvent::Notification(n) => queued.push(n),
                        other => {
                            panic!("expected only Notification events, got {other:?}")
                        }
                    }
                }
                assert_eq!(
                    queued.len(),
                    4,
                    "chunk + confirmation + applied mode update + command projection must be queued"
                );
                match &queued[2] {
                    SessionNotification::Acp(n) => match &n.update {
                        acp::SessionUpdate::CurrentModeUpdate(cmu) => {
                            assert_eq!(
                                cmu.current_mode_id.0.as_ref(),
                                "normal",
                                "the exit emission must carry the new mode id"
                            );
                            assert!(
                                n.meta.as_ref().and_then(|m| m.get("eventId")).is_some(),
                                "the queued exit mode update must carry its enqueue-time id"
                            );
                        }
                        other => panic!("expected CurrentModeUpdate, got {other:?}"),
                    },
                    other => {
                        panic!("expected the mode update behind the chunk, got {other:?}")
                    }
                }
                for notification in queued {
                    if let Some((primary, secondary)) = replay_buffer.consume_chunk(notification) {
                        actor.emit_buffered(primary).await;
                        if let Some(extra) = secondary {
                            actor.emit_buffered(extra).await;
                        }
                    }
                }
                let persisted = persisted_acp_lines(&mut prx, 3).await;
                assert_eq!(
                    persisted.len(),
                    3,
                    "exit leg persists the chunk, confirmation, and applied update only"
                );
                assert!(matches!(
                    persisted[2].update,
                    acp::SessionUpdate::CurrentModeUpdate(_)
                ));
                assert!(
                    numeric_seq(&persisted[0]) < numeric_seq(&persisted[1]),
                    "exit-leg delivery order must match id order too"
                );
                assert!(numeric_seq(&persisted[1]) < numeric_seq(&persisted[2]));
            })
            .await;
    }
    /// An interrupting Behavior switch parks on the first request and applies
    /// on the immediately-following same-target selection. Pins the 8-second
    /// confirmation window while keeping confirmation in the Shell coordinator;
    /// ordinary Pager input never confirms or cancels a Behavior transition.
    #[tokio::test]
    async fn interrupting_behavior_switch_parks_then_confirms_on_second_request() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _prx) = super::acking_persistence_channel();
                let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
                *actor.agent.borrow_mut() =
                    super::super::tests::support::test_agent_with_plan_tools().await;
                // Enter Plan first (Normal → Plan is not an interrupting switch).
                let entered = actor
                    .request_behavior_change(acp::SessionModeId::new("plan"))
                    .await;
                assert!(matches!(
                    entered,
                    Ok(crate::session::behavior::BehaviorChangeOutcome::Applied)
                ));
                // Leaving an active Plan interrupts work: the first request parks
                // the switch and asks for explicit confirmation.
                let first = actor
                    .request_behavior_change(acp::SessionModeId::new("normal"))
                    .await;
                let Ok(crate::session::behavior::BehaviorChangeOutcome::ConfirmationRequired {
                    message,
                    remaining_ms,
                }) = &first
                else {
                    panic!("expected ConfirmationRequired, got {first:?}");
                };
                assert!(
                    message.contains("Select it again to confirm"),
                    "the confirmation message must carry the Enter/Esc hint: {message}"
                );
                assert!(
                    (7_500..=8_000).contains(remaining_ms),
                    "the confirmation window must be 8 seconds, got {remaining_ms}ms"
                );
                // The second same-target request within the window confirms.
                let second = actor
                    .request_behavior_change(acp::SessionModeId::new("normal"))
                    .await;
                assert!(
                    matches!(
                        second,
                        Ok(crate::session::behavior::BehaviorChangeOutcome::Applied)
                    ),
                    "the same-target re-request must apply the switch, got {second:?}"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn behavior_switch_rejects_non_idle_foreground_without_surface_append() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _prx) = super::acking_persistence_channel();
                let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
                *actor.agent.borrow_mut() =
                    super::super::tests::support::test_agent_with_plan_tools().await;
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Plan);
                actor.state.lock().await.foreground = ForegroundState::RegularTurn(
                    super::super::tests::support::running_task_stub("active-turn"),
                );
                let surface_before = actor.chat_state_handle.get_conversation().await;

                let outcome = actor
                    .request_behavior_change(acp::SessionModeId::new("normal"))
                    .await;
                let Ok(crate::session::behavior::BehaviorChangeOutcome::Rejected { message }) =
                    outcome
                else {
                    panic!("active foreground switch must be rejected");
                };
                assert!(message.contains("Stop the active foreground work"));
                assert_eq!(
                    actor.behavior.lock().behavior(),
                    tool_types::BehaviorId::Plan
                );
                assert!(actor.behavior.lock().pending_switch().is_none());
                assert_eq!(
                    serde_json::to_value(actor.chat_state_handle.get_conversation().await).unwrap(),
                    serde_json::to_value(surface_before).unwrap(),
                    "a rejected switch must not append Control model context"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn completed_goal_receipt_survives_every_behavior_switch_until_clear() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _persistence_rx) = super::acking_persistence_channel();
                let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
                {
                    let mut tracker = actor.goal_tracker.lock();
                    tracker
                        .create_goal("completed-goal".into(), "done".into(), None, "now".into())
                        .unwrap();
                    assert!(tracker.complete());
                }

                let normal = actor
                    .request_behavior_change(acp::SessionModeId::new("normal"))
                    .await;
                assert!(matches!(
                    normal,
                    Ok(crate::session::behavior::BehaviorChangeOutcome::Applied)
                ));
                assert_eq!(
                    actor.goal_tracker.lock().status(),
                    Some(crate::session::goal_tracker::GoalStatus::Complete),
                    "Normal keeps the completed Goal receipt visible"
                );

                let ask = actor
                    .request_behavior_change(acp::SessionModeId::new("ask"))
                    .await;
                assert!(matches!(
                    ask,
                    Ok(crate::session::behavior::BehaviorChangeOutcome::Applied)
                ));
                assert_eq!(
                    actor.goal_tracker.lock().status(),
                    Some(crate::session::goal_tracker::GoalStatus::Complete),
                    "Behavior switching must not replace explicit Goal receipt clearing"
                );
                assert_eq!(
                    actor.behavior.lock().behavior(),
                    tool_types::BehaviorId::Clarify
                );
            })
            .await;
    }

    #[tokio::test]
    async fn goal_usage_follows_active_pause_restart_windows() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _persistence_rx) = super::acking_persistence_channel();
                let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
                actor
                    .goal_tracker
                    .lock()
                    .create_goal(
                        "goal-1".into(),
                        "finish delegated work".into(),
                        Some(10_000),
                        "now".into(),
                    )
                    .unwrap();
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Goal);
                actor.sync_goal_usage_window();
                actor
                    .record_goal_model_usage(Some("goal-1"), 380)
                    .await
                    .unwrap();
                assert_eq!(actor.goal_tokens_used(), 380);

                assert!(
                    actor
                        .goal_tracker
                        .lock()
                        .pause(crate::session::goal_tracker::GoalPauseReason::User)
                );
                actor.sync_goal_usage_window();
                actor.record_goal_model_usage(None, 120).await.unwrap();
                assert_eq!(actor.goal_tokens_used(), 380);

                assert!(actor.goal_tracker.lock().restart());
                actor.sync_goal_usage_window();
                actor
                    .record_goal_model_usage(Some("goal-1"), 25)
                    .await
                    .unwrap();
                assert_eq!(actor.goal_tokens_used(), 405);
            })
            .await;
    }
}

/// Synthetic auto-wake prompts (background subagent / bash / monitor /
/// workflow completions, notification drain) are completion notifications,
/// NOT Behavior-switch requests. `handle_prompt` must run them under the
/// session's current Behavior: previously the hardcoded `BehaviorId::Normal`
/// tripped the interrupting-switch gate while Plan work was active, failing
/// the wake turn with "Turn failed: switching to default will interrupt the
/// active Plan work", and could even auto-confirm a parked user-initiated
/// switch. See the `handle_prompt` gate for the full contract.
#[cfg(test)]
mod synthetic_prompt_behavior_tests {
    use super::super::tests::support::create_test_actor;
    use super::*;

    /// A `subagent-completed-*` wake arriving while Plan is active must not
    /// fail the behavior gate: the turn starts under Plan and the Behavior
    /// state is untouched.
    #[tokio::test]
    async fn synthetic_subagent_completion_runs_under_plan_without_switch() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _prx) = super::acking_persistence_channel();
                let actor = std::sync::Arc::new(
                    create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await,
                );
                *actor.agent.borrow_mut() =
                    super::super::tests::support::test_agent_with_plan_tools().await;
                // Enter Plan first (Normal → Plan is not an interrupting switch).
                let entered = actor
                    .request_behavior_change(acp::SessionModeId::new("plan"))
                    .await;
                assert!(matches!(
                    entered,
                    Ok(crate::session::behavior::BehaviorChangeOutcome::Applied)
                ));
                assert!(actor.behavior.lock().is_plan());
                assert_eq!(
                    actor.behavior.lock().behavior(),
                    tool_types::BehaviorId::Plan
                );

                // A background subagent completes: the shell injects
                // `subagent-completed-*` hardcoded to BehaviorId::Normal. With
                // the bug this failed the wake turn with the behaviorChange
                // `confirmation_required` meta; now it must pass the gate and
                // record the CURRENT (Plan) turn mode before the turn blocks
                // on the noop test sampler.
                let wake = {
                    let actor = actor.clone();
                    tokio::task::spawn_local(async move {
                        actor
                            .handle_prompt(
                                "subagent-completed-sa-1",
                                Vec::new(),
                                crate::session::PromptOrigin::SubagentCompleted {
                                    subagent_id: "sa-1".to_string(),
                                },
                                Vec::new(),
                                crate::session::TurnKind::Internal,
                                vec![acp::ContentBlock::Text(acp::TextContent::new(
                                    "subagent sa-1 finished",
                                ))],
                                tool_types::BehaviorId::Plan,
                                None,
                                None,
                                true,
                                None,
                                None,
                            )
                            .await
                    })
                };
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    loop {
                        if *actor.turn_behavior.lock() == tool_types::BehaviorId::Plan {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("the synthetic wake must pass the behavior gate and start under Plan");
                wake.abort();

                // The wake must not have switched Behavior.
                assert!(
                    actor.behavior.lock().is_plan(),
                    "the synthetic wake must leave Plan active"
                );
                assert_eq!(
                    actor.behavior.lock().behavior(),
                    tool_types::BehaviorId::Plan,
                    "the synthetic wake must inherit the current prompt mode"
                );
            })
            .await;
    }

    /// A parked interrupting switch (user-initiated, awaiting Enter/Esc)
    /// must survive a synthetic wake: it is neither auto-confirmed (the
    /// pre-fix behavior — the wake's Agent request matched the parked
    /// `default` target) nor cleared.
    #[tokio::test]
    async fn synthetic_wake_does_not_resolve_parked_switch() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _prx) = super::acking_persistence_channel();
                let actor = std::sync::Arc::new(
                    create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await,
                );
                *actor.agent.borrow_mut() =
                    super::super::tests::support::test_agent_with_plan_tools().await;
                // Enter Plan, then park a user-initiated switch to default.
                assert!(matches!(
                    actor
                        .request_behavior_change(acp::SessionModeId::new("plan"))
                        .await,
                    Ok(crate::session::behavior::BehaviorChangeOutcome::Applied)
                ));
                assert!(matches!(
                    actor
                        .request_behavior_change(acp::SessionModeId::new("normal"))
                        .await,
                    Ok(
                        crate::session::behavior::BehaviorChangeOutcome::ConfirmationRequired { .. }
                    )
                ));
                assert!(
                    actor.behavior.lock().pending_switch().is_some(),
                    "the interrupting switch must be parked"
                );

                // A synthetic wake lands while the switch is parked.
                let wake = {
                    let actor = actor.clone();
                    tokio::task::spawn_local(async move {
                        actor
                            .handle_prompt(
                                "subagent-completed-sa-2",
                                Vec::new(),
                                crate::session::PromptOrigin::SubagentCompleted {
                                    subagent_id: "sa-2".to_string(),
                                },
                                Vec::new(),
                                crate::session::TurnKind::Internal,
                                vec![acp::ContentBlock::Text(acp::TextContent::new(
                                    "subagent sa-2 finished",
                                ))],
                                tool_types::BehaviorId::Plan,
                                None,
                                None,
                                true,
                                None,
                                None,
                            )
                            .await
                    })
                };
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    loop {
                        if *actor.turn_behavior.lock() == tool_types::BehaviorId::Plan {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .expect("the synthetic wake must pass the behavior gate and start under Plan");
                wake.abort();

                // The parked switch must still be waiting for the user.
                assert!(
                    actor.behavior.lock().pending_switch().is_some(),
                    "a synthetic wake must not resolve the parked switch"
                );
                assert!(
                    actor.behavior.lock().is_plan(),
                    "the synthetic wake must not switch Behavior"
                );
            })
            .await;
    }
}

//! Timeline-backed notification admission and queued-prompt promotion.

use super::*;

impl SessionActor {
    /// Stream write-ahead payload candidates and reconcile each bounded batch
    /// against the current Timeline projection within this writer epoch.
    pub(super) async fn reconcile_notification_payloads(&self) {
        let directory = match self.session_directory.try_clone() {
            Ok(directory) => directory,
            Err(error) => {
                tracing::warn!(%error, "notification payload reconciliation directory unavailable");
                return;
            }
        };
        let (batch_tx, mut batch_rx) = tokio::sync::mpsc::channel::<Vec<String>>(1);
        let producer = tokio::task::spawn_blocking(move || {
            crate::session::notification_inbox::visit_payload_hash_batches(
                &directory,
                || batch_tx.is_closed(),
                |batch| {
                    batch_tx.blocking_send(batch).map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "notification payload reconciler stopped",
                        )
                    })
                },
            )
        });
        let mut removed_total = 0usize;
        while let Some(mut hashes) = batch_rx.recv().await {
            let _artifact_guard = self.notification_artifact_gate.lock().await;
            let Some(pending) = self.chat_state_handle.pending_notifications().await else {
                tracing::warn!(
                    "notification payload reconciliation stopped because Timeline is unavailable"
                );
                break;
            };
            let retained_hashes = pending
                .into_iter()
                .map(|notification| notification.payload_ref.blake3)
                .collect::<std::collections::BTreeSet<_>>();
            hashes.retain(|hash| !retained_hashes.contains(hash));
            if hashes.is_empty() {
                continue;
            }
            let directory = match self.session_directory.try_clone() {
                Ok(directory) => directory,
                Err(error) => {
                    tracing::warn!(%error, "notification payload cleanup directory unavailable");
                    break;
                }
            };
            match tokio::task::spawn_blocking(move || {
                crate::session::notification_inbox::remove_payload_hashes(&directory, &hashes)
            })
            .await
            {
                Ok(Ok(removed)) => removed_total += removed,
                Ok(Err(error)) => {
                    tracing::warn!(%error, "notification payload reconciliation failed");
                    break;
                }
                Err(error) => {
                    tracing::warn!(%error, "notification payload reconciliation task failed");
                    break;
                }
            }
            drop(_artifact_guard);
            tokio::task::yield_now().await;
        }
        drop(batch_rx);
        match producer.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::BrokenPipe => {}
            Ok(Err(error)) => tracing::warn!(%error, "notification payload enumeration failed"),
            Err(error) => tracing::warn!(%error, "notification payload enumerator failed"),
        }
        if removed_total > 0 {
            tracing::info!(
                removed = removed_total,
                "reclaimed orphaned notification payloads"
            );
        }
    }

    /// Remove payload artifacts only after their resolving Timeline fact is
    /// durable, and only when no remaining receipt references the same
    /// content-addressed blob.
    async fn cleanup_notification_payloads(
        &self,
        mut candidates: Vec<chat_state::NotificationPayloadRef>,
    ) {
        if candidates.is_empty() {
            return;
        }
        let Some(pending) = self.chat_state_handle.pending_notifications().await else {
            tracing::warn!("notification payload cleanup skipped because Timeline is unavailable");
            return;
        };
        let retained_hashes = pending
            .into_iter()
            .map(|notification| notification.payload_ref.blake3)
            .collect::<std::collections::BTreeSet<_>>();
        candidates.retain(|payload| !retained_hashes.contains(&payload.blake3));
        candidates.sort_by(|left, right| left.blake3.cmp(&right.blake3));
        candidates.dedup_by(|left, right| left.blake3 == right.blake3);
        if candidates.is_empty() {
            return;
        }
        let directory = match self.session_directory.try_clone() {
            Ok(directory) => directory,
            Err(error) => {
                tracing::warn!(%error, "notification payload cleanup directory unavailable");
                return;
            }
        };
        match tokio::task::spawn_blocking(move || {
            for payload in candidates {
                crate::session::notification_inbox::remove_payload(&directory, &payload)?;
            }
            Ok::<_, std::io::Error>(())
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "notification payload cleanup failed"),
            Err(error) => tracing::warn!(%error, "notification payload cleanup task failed"),
        }
    }

    /// Commit a receipt resolution and reclaim its shell-owned payloads. The
    /// Timeline remains the sole delivery state; artifact deletion is a
    /// post-commit garbage-collection side effect.
    pub(super) async fn record_notification_resolution_durably(
        &self,
        resolution: chat_state::NotificationEvent,
    ) -> Result<chat_state::TimelineEvent, chat_state::TimelineWriteError> {
        let _artifact_guard = self.notification_artifact_gate.lock().await;
        let notification_ids = match &resolution {
            chat_state::NotificationEvent::Consumed {
                notification_ids, ..
            }
            | chat_state::NotificationEvent::Dismissed {
                notification_ids, ..
            } => notification_ids,
            chat_state::NotificationEvent::Received { .. } => {
                return Err(chat_state::TimelineWriteError::Invalid(
                    chat_state::TimelineError::InvalidNotification,
                ));
            }
        };
        let notification_ids = notification_ids
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        let payloads = self
            .chat_state_handle
            .pending_notifications()
            .await
            .ok_or(chat_state::TimelineWriteError::AcknowledgementLost)?
            .into_iter()
            .filter(|notification| notification_ids.contains(notification.id.as_str()))
            .map(|notification| notification.payload_ref)
            .collect::<Vec<_>>();
        let event = self
            .chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Notification(resolution))
            .await?;
        self.cleanup_notification_payloads(payloads).await;
        Ok(event)
    }

    pub(super) async fn receive_notification(
        &self,
        source: chat_state::NotificationSource,
        source_version: chat_state::NotificationSourceVersion,
        body: String,
    ) -> Result<String, String> {
        let _artifact_guard = self.notification_artifact_gate.lock().await;
        let pending_before = self
            .chat_state_handle
            .pending_notifications()
            .await
            .unwrap_or_default();
        let directory = self
            .session_directory
            .try_clone()
            .map_err(|error| error.to_string())?;
        let payload_ref = tokio::task::spawn_blocking(move || {
            crate::session::notification_inbox::write_payload(&directory, &body)
        })
        .await
        .map_err(|error| format!("notification payload writer failed: {error}"))?
        .map_err(|error| error.to_string())?;
        let received_payload = payload_ref.clone();
        let event = match self
            .chat_state_handle
            .receive_notification_durably(
                self.session_info.id.0.to_string(),
                source,
                source_version,
                payload_ref,
            )
            .await
        {
            Ok(event) => event,
            Err(error) => {
                self.cleanup_notification_payloads(vec![received_payload])
                    .await;
                return Err(error.to_string());
            }
        };
        let pending_after = self
            .chat_state_handle
            .pending_notifications()
            .await
            .unwrap_or_default();
        let retained_ids = pending_after
            .iter()
            .map(|notification| notification.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let mut superseded = pending_before
            .iter()
            .filter(|notification| !retained_ids.contains(notification.id.as_str()))
            .map(|notification| notification.payload_ref.clone())
            .collect::<Vec<_>>();
        if let chat_state::TimelineEventKind::Notification(
            chat_state::NotificationEvent::Received { id, .. },
        ) = &event.kind
            && !retained_ids.contains(id.as_str())
        {
            superseded.push(received_payload);
        }
        self.cleanup_notification_payloads(superseded).await;
        match event.kind {
            chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Received { id, .. },
            ) => Ok(id),
            _ => Err("notification receipt returned an unrelated Timeline fact".into()),
        }
    }

    pub(super) async fn maybe_start_running_task(
        self: Arc<Self>,
        completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
    ) {
        // Fast path under the lock: nothing to promote.
        let may_combine;
        {
            let state = self.state.lock().await;
            if !state.foreground.is_idle() {
                let queue_depth = state.pending_inputs.len();
                if queue_depth > 0 {
                    ::diagnostics::unified_log::debug(
                        "shell.prompt.start_blocked",
                        Some(self.session_info.id.0.as_ref()),
                        Some(serde_json::json!({
                            "reason": if matches!(state.foreground, ForegroundState::Compaction) { "compaction_running" } else { "task_already_running" },
                            "queue_depth": queue_depth,
                        })),
                    );
                    tracing::debug!(
                        target: "qtrace",
                        pid = std::process::id(),
                        event = "server_start_blocked",
                        queue_depth,
                        front_prompt_id = state
                            .pending_inputs
                            .front()
                            .map(|i| i.prompt_id.as_str())
                            .unwrap_or(""),
                        session = self.session_info.id.0.as_ref(),
                        "maybe_start_running_task blocked: a turn is already running",
                    );
                }
                return;
            }
            if state.pending_inputs.is_empty() {
                return;
            }
            // A merge needs 2+ queued prompts; sample here so the common
            // single-prompt promote skips the config disk read below.
            may_combine = state.pending_inputs.len() >= 2;
        }

        // Config I/O outside the state lock, and only when a merge is even
        // possible — keeps the single-prompt promote (the common case) off disk.
        let combine_queued = may_combine
            && crate::util::config::load_config()
                .await
                .ui
                .combine_queued_prompts
                .unwrap_or(false);

        let mut state = self.state.lock().await;
        // Re-check after the await gap.
        if !state.foreground.is_idle() || state.pending_inputs.is_empty() {
            return;
        }

        // Note: Auto-compact is now handled inline during process_conversation_turn,
        // so we no longer need to check for queued auto-compact here.

        // GC stale edit-holds: an id that is no longer queued (promoted,
        // removed, or whose fire-and-forget `release_edit` was dropped) can
        // never be edited again, so drop it to bound the set over a long session.
        if !state.combine_edit_holds.is_empty() {
            let live: std::collections::HashSet<String> = state
                .pending_inputs
                .iter()
                .map(|i| i.prompt_id.clone())
                .collect();
            state.combine_edit_holds.retain(|id| live.contains(id));
        }

        if combine_queued {
            let holds: Vec<String> = state.combine_edit_holds.iter().cloned().collect();
            let skip: Vec<&str> = holds.iter().map(String::as_str).collect();
            SessionActor::combine_front_pending_inputs(&mut state.pending_inputs, &skip);
        }

        // Start the next pending user prompt. Pull all needed fields from the
        // queue head in one `front_mut` scope so we can mutate `state` again
        // (e.g. `rewindable`) without overlapping borrows.
        let (
            persist_ack,
            prompt_id,
            prompt_blocks,
            client_identifier,
            screen_mode,
            verbatim,
            json_schema,
            origin,
            notification_ids,
            turn_kind,
            running_display,
        ) = {
            let Some(front) = state.pending_inputs.front_mut() else {
                return;
            };
            let running_display = SessionActor::running_display_from_item(front);
            (
                front.persist_ack.take(),
                front.prompt_id.clone(),
                front.prompt_blocks.clone(),
                front.client_identifier.clone(),
                front.screen_mode.clone(),
                front.verbatim,
                front.json_schema.clone(),
                front.origin.clone(),
                front.notification_ids.clone(),
                front.turn_kind,
                running_display,
            )
        };
        if matches!(origin, super::PromptOrigin::User) {
            state.notifications_suppressed = false;
            ::diagnostics::unified_log::info(
                "shell.task_wake.gate_cleared",
                Some(self.session_info.id.0.as_ref()),
                Some(serde_json::json!({ "reason": "queued_user_promotion" })),
            );
        }
        state.rewindable = true;

        // This is the admission linearization point: capture Behavior and
        // Goal ownership, then install the foreground owner without yielding.
        // A concurrent Behavior command may affect the next queued message,
        // but can never retag this turn after it owns foreground.
        let admitted_behavior = self.behavior.lock().behavior();
        tracing::debug!(
            target: "qtrace",
            pid = std::process::id(),
            event = "server_promote",
            prompt_id = %prompt_id,
            combined_segs = running_display
                .combined_texts
                .as_ref()
                .map(|v| v.len())
                .unwrap_or(0),
            remaining_queued = state.pending_inputs.len().saturating_sub(1),
            session = self.session_info.id.0.as_ref(),
            "promoting front of pending_inputs to the running turn",
        );
        // Promote broadcast before spawn so clients paint the structured
        // message identity before its user-message chunk can race in.
        self.broadcast_queue_changed_promoting(&state, running_display);

        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        state.foreground = ForegroundState::RegularTurn(AgentTask::new_prompt(
            self.clone(),
            prompt_id.clone(),
            origin.clone(),
            notification_ids,
            turn_kind,
            prompt_blocks,
            admitted_behavior,
            client_identifier,
            screen_mode,
            verbatim,
            json_schema,
            Some(start_rx),
            completion_tx,
            persist_ack,
        ));
        drop(state);

        // The installed task waits on `start_rx`, so it cannot observe stale
        // turn-scoped ownership while these resources are published.
        self.publish_turn_scope_resources(prompt_id, &origin, admitted_behavior)
            .await;
        let _ = start_tx.send(());
    }

    /// Consume pending receipts into the active turn at the next safe sampling
    /// boundary. A completion that arrives while
    /// the Agent is already working augments that turn instead of scheduling a
    /// redundant follow-up turn.
    pub(super) async fn drain_active_notifications(&self) -> bool {
        self.drain_active_notifications_excluding(&[]).await
    }

    /// Consume pending receipts except those already admitted as the primary
    /// input of this turn. This lets resume context keep its historical
    /// position before the user's first input without double-consuming the
    /// completion receipt that opened an autonomous turn.
    pub(super) async fn drain_active_notifications_excluding(
        &self,
        excluded_notification_ids: &[String],
    ) -> bool {
        let Some(turn) = self.events.current_turn() else {
            return false;
        };
        let notifications = self
            .chat_state_handle
            .pending_notifications()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|notification| !excluded_notification_ids.contains(&notification.id))
            .collect::<Vec<_>>();
        if notifications.is_empty() {
            return false;
        }
        let notification_ids = notifications
            .iter()
            .map(|notification| notification.id.clone())
            .collect::<Vec<_>>();
        let displayed_notifications = Self::coalesce_running_task_checkpoints(&notifications);
        let Some(payloads) = self
            .read_notification_payloads(&displayed_notifications, "active turn")
            .await
        else {
            return false;
        };
        let blocks = Self::notification_blocks(
            &displayed_notifications,
            &payloads,
            Some(&self.tool_context.task_output_tool_name),
        );
        let body = blocks
            .into_iter()
            .filter_map(|block| match block {
                acp::ContentBlock::Text(text) => Some(text.text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if body.is_empty() {
            return false;
        }
        let mut input = sampling_types::ConversationItem::notification_drain(body);
        input.set_prompt_index(self.chat_state_handle.get_prompt_index().await);
        if let Err(error) = self
            .record_notification_resolution_durably(chat_state::NotificationEvent::Consumed {
                notification_ids,
                turn,
                input: Some(input),
            })
            .await
        {
            tracing::error!(%error, "failed to consume active notifications");
            return false;
        }
        tracing::info!(
            count = notifications.len(),
            "delivered durable notifications to the active turn"
        );
        true
    }

    /// Promote the durable inbox into one model turn. User FIFO admission is
    /// checked both before and after artifact I/O; losing either race leaves
    /// the Timeline receipts pending for the next idle edge.
    pub(super) async fn maybe_drain_notifications(
        self: Arc<Self>,
        completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
    ) {
        {
            let state = self.state.lock().await;
            if !is_session_idle_for_injection(&state) {
                return;
            }
        }

        let mut notifications = self
            .chat_state_handle
            .pending_notifications()
            .await
            .unwrap_or_default();
        if notifications.is_empty() {
            return;
        }
        notifications.sort_by_key(|notification| {
            let priority = match notification.source {
                chat_state::NotificationSource::MonitorProgress { .. } => 1u8,
                chat_state::NotificationSource::TaskStillRunning { .. } => 2u8,
                chat_state::NotificationSource::TaskCompleted { .. }
                | chat_state::NotificationSource::SubagentCompleted { .. }
                | chat_state::NotificationSource::WorkflowCompleted { .. } => 0u8,
            };
            (priority, notification.received_seq)
        });

        let goal_task_ids = self.goal_turn_task_ids.lock().clone();
        let (dismissed, surfaceable): (Vec<_>, Vec<_>) = notifications
            .into_iter()
            .partition(|notification| Self::goal_owned_autostart(&goal_task_ids, notification));
        if !dismissed.is_empty() {
            let notification_ids = dismissed
                .iter()
                .map(|notification| notification.id.clone())
                .collect::<Vec<_>>();
            if let Err(error) = self
                .record_notification_resolution_durably(chat_state::NotificationEvent::Dismissed {
                    notification_ids,
                    reason: chat_state::NotificationDismissReason::GoalOwnedAutostart,
                })
                .await
            {
                tracing::error!(%error, "failed to dismiss Goal-owned notification autostart");
                return;
            }
            tracing::info!(
                count = dismissed.len(),
                "dismissed autonomous wake for Goal-owned background work"
            );
        }
        notifications = surfaceable;
        if notifications.is_empty() {
            return;
        }
        // Resume checkpoints are context for the next real turn, not a reason
        // to spend a model turn. If a terminal receipt is also present, fold
        // the checkpoint context into that already-necessary turn.
        if !notifications.iter().any(Self::notification_autostarts) {
            return;
        }

        let notification_ids = notifications
            .iter()
            .map(|notification| notification.id.clone())
            .collect::<Vec<_>>();
        let displayed_notifications = Self::coalesce_running_task_checkpoints(&notifications);
        let Some(payloads) = self
            .read_notification_payloads(&displayed_notifications, "idle drain")
            .await
        else {
            return;
        };

        let prompt_blocks = Self::notification_blocks(&displayed_notifications, &payloads, None);
        if prompt_blocks.is_empty() {
            return;
        }
        let (origin, turn_kind) = Self::notification_turn_identity(&notifications);
        let (respond_to, _) = tokio::sync::oneshot::channel();
        {
            let mut state = self.state.lock().await;
            if !is_session_idle_for_injection(&state) {
                return;
            }
            state.pending_inputs.push_back(InputItem {
                prompt_id: format!("notifications-{}", uuid::Uuid::now_v7()),
                turn_kind,
                prompt_blocks,
                client_identifier: None,
                screen_mode: None,
                verbatim: true,
                json_schema: None,
                origin,
                notification_ids,
                respond_to,
                persist_ack: None,
                queue_meta: None,
            });
        }

        SessionActor::maybe_start_running_task(self, completion_tx).await;
    }

    /// Notifies extensions when the session settles idle (nothing running, nothing queued).
    /// The idle check stays host-side; extensions only get the event.
    pub(super) async fn emit_session_idle_if_idle(&self) {
        {
            let state = self.state.lock().await;
            if !is_session_idle_for_injection(&state) {
                return;
            }
        }
        if let Some(extension) = &self.idle_prompt_extension {
            extension.on_session_idle();
        }
    }

    async fn read_notification_payloads(
        &self,
        notifications: &[chat_state::PendingNotification],
        operation: &'static str,
    ) -> Option<Vec<String>> {
        let directory = match self.session_directory.try_clone() {
            Ok(directory) => directory,
            Err(error) => {
                tracing::error!(%error, operation, "cannot open durable notification inbox");
                return None;
            }
        };
        let payload_refs = notifications
            .iter()
            .map(|notification| notification.payload_ref.clone())
            .collect::<Vec<_>>();
        match tokio::task::spawn_blocking(move || {
            payload_refs
                .iter()
                .map(|payload| {
                    crate::session::notification_inbox::read_payload(&directory, payload)
                })
                .collect::<std::io::Result<Vec<_>>>()
        })
        .await
        {
            Ok(Ok(payloads)) => Some(payloads),
            Ok(Err(error)) => {
                tracing::error!(%error, operation, "notification payload is missing or corrupt");
                None
            }
            Err(error) => {
                tracing::error!(%error, operation, "notification payload reader failed");
                None
            }
        }
    }

    fn goal_owned_autostart(
        goal_task_ids: &std::collections::HashSet<String>,
        notification: &chat_state::PendingNotification,
    ) -> bool {
        match &notification.source {
            chat_state::NotificationSource::MonitorProgress { task_id }
            | chat_state::NotificationSource::TaskCompleted { task_id, .. } => {
                goal_task_ids.contains(task_id)
            }
            chat_state::NotificationSource::TaskStillRunning { .. }
            | chat_state::NotificationSource::SubagentCompleted { .. }
            | chat_state::NotificationSource::WorkflowCompleted { .. } => false,
        }
    }

    fn notification_autostarts(notification: &chat_state::PendingNotification) -> bool {
        !matches!(
            notification.source,
            chat_state::NotificationSource::TaskStillRunning { .. }
        )
    }

    /// Repeated graceful exits can checkpoint the same still-running task
    /// before any real turn has consumed the previous checkpoint. Preserve
    /// every receipt for audit and consume them together, but show only the
    /// newest snapshot for each task so resume context does not duplicate.
    fn coalesce_running_task_checkpoints(
        notifications: &[chat_state::PendingNotification],
    ) -> Vec<chat_state::PendingNotification> {
        let mut latest = std::collections::HashMap::new();
        for notification in notifications {
            if let chat_state::NotificationSource::TaskStillRunning { task_id, .. } =
                &notification.source
            {
                latest.insert(task_id.clone(), notification.received_seq);
            }
        }
        notifications
            .iter()
            .filter(|notification| match &notification.source {
                chat_state::NotificationSource::TaskStillRunning { task_id, .. } => {
                    latest.get(task_id) == Some(&notification.received_seq)
                }
                _ => true,
            })
            .cloned()
            .collect()
    }

    fn notification_blocks(
        notifications: &[chat_state::PendingNotification],
        payloads: &[String],
        monitor_task_output_name: Option<&str>,
    ) -> Vec<acp::ContentBlock> {
        use tools::implementations::grow_build::monitor::types::MonitorEventNotification;

        let mut monitor_events: Vec<MonitorEventNotification> = Vec::new();
        let mut sections: Vec<Vec<acp::ContentBlock>> = Vec::new();
        let mut monitor_section_idx: Option<usize> = None;
        for (notification, payload) in notifications.iter().zip(payloads) {
            match &notification.source {
                chat_state::NotificationSource::MonitorProgress { task_id } => {
                    monitor_events.push(MonitorEventNotification {
                        task_id: task_id.clone(),
                        event_text: payload.clone(),
                    });
                    if monitor_section_idx.is_none() {
                        monitor_section_idx = Some(sections.len());
                        sections.push(Vec::new());
                    }
                }
                chat_state::NotificationSource::TaskCompleted { .. }
                | chat_state::NotificationSource::TaskStillRunning { .. }
                | chat_state::NotificationSource::SubagentCompleted { .. }
                | chat_state::NotificationSource::WorkflowCompleted { .. } => {
                    sections.push(vec![acp::ContentBlock::Text(acp::TextContent::new(
                        payload.clone(),
                    ))]);
                }
            }
        }
        if let (Some(index), Some(batch)) = (
            monitor_section_idx,
            tools::reminders::task_completion::format_monitor_events(
                &monitor_events,
                monitor_task_output_name,
            ),
        ) {
            sections[index] = vec![acp::ContentBlock::Text(acp::TextContent::new(batch))];
        }

        let mut blocks = Vec::new();
        for (index, section) in sections.iter().enumerate() {
            if index > 0 {
                blocks.push(acp::ContentBlock::Text(acp::TextContent::new("---")));
            }
            blocks.extend(section.iter().cloned());
        }
        blocks
    }

    fn notification_turn_identity(
        notifications: &[chat_state::PendingNotification],
    ) -> (super::PromptOrigin, crate::session::TurnKind) {
        if let [notification] = notifications {
            match &notification.source {
                chat_state::NotificationSource::TaskCompleted { task_id, .. } => {
                    return (
                        super::PromptOrigin::TaskCompleted {
                            task_id: task_id.clone(),
                        },
                        crate::session::TurnKind::Internal,
                    );
                }
                chat_state::NotificationSource::SubagentCompleted { subagent_id } => {
                    return (
                        super::PromptOrigin::SubagentCompleted {
                            subagent_id: subagent_id.clone(),
                        },
                        crate::session::TurnKind::Internal,
                    );
                }
                chat_state::NotificationSource::WorkflowCompleted { run_id } => {
                    let revision = match notification.source_version {
                        chat_state::NotificationSourceVersion::Ordinal { value } => value,
                        _ => 0,
                    };
                    return (
                        super::PromptOrigin::WorkflowCompleted {
                            completion_id: format!("{run_id}-{revision}"),
                        },
                        crate::session::TurnKind::Internal,
                    );
                }
                chat_state::NotificationSource::MonitorProgress { .. } => {}
                chat_state::NotificationSource::TaskStillRunning { .. } => {}
            }
        }
        (
            super::PromptOrigin::NotificationDrain,
            crate::session::TurnKind::Internal,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_notification_admission_reclaims_its_write_ahead_payload() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                let source = chat_state::NotificationSource::TaskCompleted {
                    task_id: "conflicting-retry".into(),
                    task_kind: chat_state::NotificationTaskKind::Task,
                };
                let version = chat_state::NotificationSourceVersion::Ordinal { value: 1 };
                actor
                    .receive_notification(
                        source.clone(),
                        version.clone(),
                        "canonical result".into(),
                    )
                    .await
                    .expect("initial notification");
                let pending = actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .expect("pending notifications");
                let retained_path = actor
                    .session_dir
                    .join("artifacts/notifications")
                    .join(format!("{}.txt", pending[0].payload_ref.blake3));
                let orphan_hash = blake3::hash(b"conflicting result").to_hex();
                let orphan_path = actor
                    .session_dir
                    .join("artifacts/notifications")
                    .join(format!("{orphan_hash}.txt"));

                assert!(
                    actor
                        .receive_notification(source, version, "conflicting result".into())
                        .await
                        .is_err()
                );
                assert!(retained_path.exists());
                assert!(
                    !orphan_path.exists(),
                    "failed admission must not strand its write-ahead payload"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn reconciliation_streams_past_unknown_files_and_keeps_pending_payloads() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                actor
                    .receive_notification(
                        chat_state::NotificationSource::TaskCompleted {
                            task_id: "retained-notification".into(),
                            task_kind: chat_state::NotificationTaskKind::Task,
                        },
                        chat_state::NotificationSourceVersion::Ordinal { value: 1 },
                        "retained result".into(),
                    )
                    .await
                    .expect("pending notification");
                let retained = actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .expect("pending projection")[0]
                    .payload_ref
                    .clone();
                let orphan = crate::session::notification_inbox::write_payload(
                    &actor.session_directory,
                    "orphaned result",
                )
                .unwrap();
                let artifact_dir = actor.session_dir.join("artifacts/notifications");
                for index in 0..300 {
                    std::fs::write(artifact_dir.join(format!("unknown-{index}")), b"keep").unwrap();
                }

                actor.reconcile_notification_payloads().await;

                assert_eq!(
                    crate::session::notification_inbox::read_payload(
                        &actor.session_directory,
                        &retained,
                    )
                    .unwrap(),
                    "retained result"
                );
                assert!(matches!(
                    crate::session::notification_inbox::read_payload(
                        &actor.session_directory,
                        &orphan,
                    ),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn monitor_progress_window_prunes_superseded_payload_artifacts() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let temp = tempfile::tempdir().unwrap();
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
                let mut actor = crate::session::actor::tests::support::create_test_actor(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                actor.session_dir = temp.path().join("session");
                std::fs::create_dir_all(&actor.session_dir).unwrap();
                actor.session_directory = std::sync::Arc::new(
                    crate::session::storage::ContainedDirectory::open(
                        temp.path(),
                        std::path::Path::new("session"),
                        "monitor notification session",
                        false,
                    )
                    .unwrap(),
                );
                let actor = std::sync::Arc::new(actor);
                for index in 0..(chat_state::MAX_PENDING_MONITOR_PROGRESS_PER_TASK + 3) {
                    actor
                        .receive_notification(
                            chat_state::NotificationSource::MonitorProgress {
                                task_id: "monitor-bounded".into(),
                            },
                            chat_state::NotificationSourceVersion::Opaque {
                                value: format!("event-{index}"),
                            },
                            format!("unique monitor payload {index}"),
                        )
                        .await
                        .expect("receive monitor progress");
                }
                assert_eq!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .expect("pending notifications")
                        .len(),
                    chat_state::MAX_PENDING_MONITOR_PROGRESS_PER_TASK,
                );
                let artifacts = actor.session_dir.join("artifacts/notifications");
                assert_eq!(
                    std::fs::read_dir(artifacts).unwrap().count(),
                    chat_state::MAX_PENDING_MONITOR_PROGRESS_PER_TASK,
                );
            })
            .await;
    }

    #[tokio::test]
    async fn active_monitor_progress_is_durably_consumed_into_current_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                crate::session::actor::tests::support::begin_test_causal_turn(&actor).await;
                actor
                    .receive_notification(
                        chat_state::NotificationSource::MonitorProgress {
                            task_id: "monitor-1".into(),
                        },
                        chat_state::NotificationSourceVersion::Opaque {
                            value: "event-1".into(),
                        },
                        "<monitor-event description=\"deploy\" task_id=\"monitor-1\">\nhealthy\n</monitor-event>".into(),
                    )
                    .await
                    .expect("receive monitor notification");
                let pending = actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .expect("pending notifications");
                assert_eq!(pending.len(), 1);
                let payload_path = actor
                    .session_dir
                    .join("artifacts/notifications")
                    .join(format!("{}.txt", pending[0].payload_ref.blake3));
                assert!(payload_path.exists());

                assert!(actor.drain_active_notifications().await);
                assert!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .expect("pending notifications")
                        .is_empty()
                );
                assert!(
                    !payload_path.exists(),
                    "durably consumed notification payload must be reclaimed"
                );
                let conversation = actor.chat_state_handle.get_conversation().await;
                let delivered = conversation.last().expect("monitor input materialized");
                assert!(delivered.text_content().contains("healthy"));
                let sampling_types::ConversationItem::User(delivered) = delivered else {
                    panic!("notification input must use the user role");
                };
                assert_eq!(
                    delivered.synthetic_reason.as_ref(),
                    Some(&sampling_types::SyntheticReason::NotificationDrain)
                );
            })
            .await;
    }

    #[tokio::test]
    async fn active_task_completion_joins_the_current_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                crate::session::actor::tests::support::begin_test_causal_turn(&actor).await;
                actor
                    .receive_notification(
                        chat_state::NotificationSource::TaskCompleted {
                            task_id: "build-1".into(),
                            task_kind: chat_state::NotificationTaskKind::Task,
                        },
                        chat_state::NotificationSourceVersion::Ordinal { value: 1 },
                        "build completed successfully".into(),
                    )
                    .await
                    .expect("receive task completion");

                assert!(actor.drain_active_notifications().await);
                let conversation = actor.chat_state_handle.get_conversation().await;
                let delivered = conversation.last().expect("completion input materialized");
                assert!(
                    delivered
                        .text_content()
                        .contains("build completed successfully")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn idle_goal_owned_task_is_dismissed_without_starting_a_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                actor.goal_turn_task_ids.lock().insert("goal-build".into());
                actor
                    .receive_notification(
                        chat_state::NotificationSource::TaskCompleted {
                            task_id: "goal-build".into(),
                            task_kind: chat_state::NotificationTaskKind::Task,
                        },
                        chat_state::NotificationSourceVersion::Ordinal { value: 1 },
                        "goal build completed".into(),
                    )
                    .await
                    .expect("receive Goal task completion");
                let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();

                actor.clone().maybe_drain_notifications(completion_tx).await;

                assert!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .expect("pending notifications")
                        .is_empty()
                );
                let state = actor.state.lock().await;
                assert!(state.foreground.is_idle());
                assert!(state.pending_inputs.is_empty());
                drop(state);
                assert!(matches!(
                    actor.chat_state_handle.get_conversation().await.as_slice(),
                    [sampling_types::ConversationItem::System(_)]
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn running_task_checkpoint_waits_for_a_real_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                actor
                    .receive_notification(
                        chat_state::NotificationSource::TaskStillRunning {
                            task_id: "build-1".into(),
                            task_kind: chat_state::NotificationTaskKind::Task,
                        },
                        chat_state::NotificationSourceVersion::Opaque {
                            value: "checkpoint-1".into(),
                        },
                        "build was still running at shutdown".into(),
                    )
                    .await
                    .expect("receive running-task checkpoint");
                let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();

                actor.clone().maybe_drain_notifications(completion_tx).await;

                assert_eq!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .expect("pending notifications")
                        .len(),
                    1
                );
                assert!(actor.state.lock().await.foreground.is_idle());
                assert!(matches!(
                    actor.chat_state_handle.get_conversation().await.as_slice(),
                    [sampling_types::ConversationItem::System(_)]
                ));

                crate::session::actor::tests::support::begin_test_causal_turn(&actor).await;
                assert!(actor.drain_active_notifications().await);
                assert!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .expect("pending notifications")
                        .is_empty()
                );
                assert!(
                    actor
                        .chat_state_handle
                        .get_conversation()
                        .await
                        .last()
                        .expect("checkpoint input materialized")
                        .text_content()
                        .contains("still running at shutdown")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn pre_input_checkpoint_drain_does_not_consume_the_admitted_completion() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                crate::session::actor::tests::support::begin_test_causal_turn(&actor).await;
                let completion_id = actor
                    .receive_notification(
                        chat_state::NotificationSource::TaskCompleted {
                            task_id: "completed-1".into(),
                            task_kind: chat_state::NotificationTaskKind::Task,
                        },
                        chat_state::NotificationSourceVersion::Ordinal { value: 1 },
                        "completed result".into(),
                    )
                    .await
                    .expect("receive completion");
                actor
                    .receive_notification(
                        chat_state::NotificationSource::TaskStillRunning {
                            task_id: "running-1".into(),
                            task_kind: chat_state::NotificationTaskKind::Task,
                        },
                        chat_state::NotificationSourceVersion::Opaque {
                            value: "checkpoint-1".into(),
                        },
                        "resume context".into(),
                    )
                    .await
                    .expect("receive checkpoint");

                assert!(
                    actor
                        .drain_active_notifications_excluding(std::slice::from_ref(&completion_id))
                        .await
                );

                let pending = actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .expect("pending notifications");
                assert_eq!(pending.len(), 1);
                assert_eq!(pending[0].id, completion_id);
                assert!(
                    actor
                        .chat_state_handle
                        .get_conversation()
                        .await
                        .last()
                        .expect("checkpoint input materialized")
                        .text_content()
                        .contains("resume context")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn repeated_running_task_checkpoints_surface_only_the_latest_snapshot() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                crate::session::actor::tests::support::begin_test_causal_turn(&actor).await;
                for (epoch, body) in [
                    ("checkpoint-1", "stale checkpoint body"),
                    ("checkpoint-2", "latest checkpoint body"),
                ] {
                    actor
                        .receive_notification(
                            chat_state::NotificationSource::TaskStillRunning {
                                task_id: "build-1".into(),
                                task_kind: chat_state::NotificationTaskKind::Task,
                            },
                            chat_state::NotificationSourceVersion::Opaque {
                                value: epoch.into(),
                            },
                            body.into(),
                        )
                        .await
                        .expect("receive checkpoint");
                }

                assert!(actor.drain_active_notifications().await);

                assert!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .expect("pending notifications")
                        .is_empty()
                );
                let delivered = actor
                    .chat_state_handle
                    .get_conversation()
                    .await
                    .last()
                    .expect("checkpoint input materialized")
                    .text_content();
                assert!(delivered.contains("latest checkpoint body"));
                assert!(!delivered.contains("stale checkpoint body"));
            })
            .await;
    }
}

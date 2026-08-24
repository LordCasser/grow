//! Timeline-backed notification admission and queued-prompt promotion.

use super::*;

impl SessionActor {
    pub(super) async fn receive_notification(
        &self,
        source: chat_state::NotificationSource,
        source_version: chat_state::NotificationSourceVersion,
        body: String,
    ) -> Result<String, String> {
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
        let event = self
            .chat_state_handle
            .receive_notification_durably(
                self.session_info.id.0.to_string(),
                source,
                source_version,
                payload_ref,
            )
            .await
            .map_err(|error| error.to_string())?;
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
            .chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Consumed {
                    notification_ids,
                    turn,
                    input: Some(input),
                },
            ))
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
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Notification(
                    chat_state::NotificationEvent::Dismissed {
                        notification_ids,
                        reason: chat_state::NotificationDismissReason::GoalOwnedAutostart,
                    },
                ))
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
    async fn active_monitor_progress_is_durably_consumed_into_current_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::acp_session::support::build_actor().await;
                crate::session::acp_session::support::begin_test_causal_turn(&actor).await;
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
                assert_eq!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .expect("pending notifications")
                        .len(),
                    1
                );

                assert!(actor.drain_active_notifications().await);
                assert!(
                    actor
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .expect("pending notifications")
                        .is_empty()
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
                    crate::session::acp_session::support::build_actor().await;
                crate::session::acp_session::support::begin_test_causal_turn(&actor).await;
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
                    crate::session::acp_session::support::build_actor().await;
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
                assert!(actor.chat_state_handle.get_conversation().await.is_empty());
            })
            .await;
    }

    #[tokio::test]
    async fn running_task_checkpoint_waits_for_a_real_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::acp_session::support::build_actor().await;
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
                assert!(actor.chat_state_handle.get_conversation().await.is_empty());

                crate::session::acp_session::support::begin_test_causal_turn(&actor).await;
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
                    crate::session::acp_session::support::build_actor().await;
                crate::session::acp_session::support::begin_test_causal_turn(&actor).await;
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
                    crate::session::acp_session::support::build_actor().await;
                crate::session::acp_session::support::begin_test_causal_turn(&actor).await;
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

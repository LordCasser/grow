//! Execution suspension is independent of business cancellation and Turn end.
use super::*;
use chat_state::session_runtime::{
    CommandSettlementStatus, RuntimeCommandSettlement, RuntimeNodeRef, RuntimePhase,
    SessionRuntimeEvent, SuspendedOperation, SuspendedOperationState, TurnResumePoint,
};

impl SessionActor {
    /// A closed handoff gate transfers execution to the suspension worker.
    /// Keep the old future parked until that owner drops it; returning a
    /// normal cancellation here would manufacture a logical Turn terminal.
    pub(super) async fn park_during_runtime_handoff(&self) {
        let suspended = self.state.lock().await.runtime_suspend.clone();
        if suspended.load(std::sync::atomic::Ordering::Acquire) {
            std::future::pending::<()>().await;
        }
    }

    async fn runtime_delegation_events(&self) -> Result<Vec<chat_state::TimelineEvent>, String> {
        let runtime = self.chat_state_handle.runtime_state().await?;
        let root_id = runtime
            .root_session_id
            .unwrap_or_else(|| self.session_id_string());
        if root_id == self.session_id_string() {
            return self
                .chat_state_handle
                .timeline_events()
                .await
                .ok_or("root Timeline unavailable".into());
        }
        tokio::task::spawn_blocking(move || {
            let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
            let opened = storage
                .open_session_by_id(&root_id)
                .map_err(|error| error.to_string())?
                .ok_or("original lifecycle root is missing")?;
            Ok(opened
                .validated_timeline(&root_id)
                .map_err(|error| error.to_string())?
                .timeline
                .events()
                .to_vec())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    async fn restored_child_planning_summary(
        &self,
        spawn: &chat_state::SubagentSpawnEvent,
        task_id: &str,
    ) -> Result<Option<tool_types::planning::SubagentPlanningSummary>, String> {
        // The original snapshot identifies the child to await, but its Ended
        // fact can be committed only after the restored child finishes.
        let root_events = self.runtime_delegation_events().await?;
        let root = chat_state::Timeline::from_events(root_events.clone())
            .map_err(|error| format!("invalid lifecycle-root Timeline: {error}"))?;
        let terminal = root
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(end))
                    if end.subagent_id == task_id
                        && end.child_session_id == spawn.child_session_id =>
                {
                    Some(end.clone())
                }
                _ => None,
            })
            .ok_or_else(|| "completed child has no durable root terminal".to_string())?;
        let result_ref = terminal.result_ref.as_ref().ok_or_else(|| {
            "completed child terminal has no canonical result reference".to_string()
        })?;
        crate::agent::subagent::validate_planning_result_reference_in_events(
            &spawn.security_parent_session_id,
            task_id,
            &spawn.child_session_id,
            result_ref,
            &root_events,
        )
        .await?;
        crate::agent::subagent::durable_child_planning_summary(spawn, &terminal)
    }

    fn add_restored_planning_summary(
        mut body: serde_json::Value,
        summary: Option<tool_types::planning::SubagentPlanningSummary>,
    ) -> Result<String, String> {
        if let Some(summary) = summary {
            summary.validate().map_err(|error| error.to_string())?;
            body["planning_summary"] = serde_json::to_value(summary)
                .map_err(|error| format!("cannot serialize child planning summary: {error}"))?;
        }
        Ok(body.to_string())
    }

    async fn settle_restored_child_completion_receipt(
        &self,
        root_timeline_id: &str,
        spawn_seq: chat_state::EventSeq,
        spawn: &chat_state::SubagentSpawnEvent,
        terminal: &chat_state::SubagentTerminalEvent,
    ) -> Result<(), String> {
        if !spawn.surface_completion || terminal.outcome == chat_state::SubagentOutcome::Cancelled {
            return Ok(());
        }
        let owner = if spawn.security_parent_session_id != root_timeline_id {
            chat_state::NotificationOwner::Session
        } else {
            crate::agent::subagent::subagent_notification_owner(spawn)
        };
        let source = chat_state::NotificationSource::SubagentCompleted {
            subagent_id: spawn.subagent_id.clone(),
            owner,
        };
        let source_version = chat_state::NotificationSourceVersion::Ordinal { value: 1 };
        let body = crate::agent::subagent::completion_receipt_body_from_durable_facts(
            root_timeline_id,
            spawn_seq,
            spawn,
            terminal,
        )?;

        // Materialize the canonical source receipt before completing the
        // restored ToolResult. This makes a later producer retry idempotent
        // even when its original receipt had not reached this Timeline yet.
        self.receive_notification(source.clone(), source_version.clone(), body)
            .await?;
        let notification_ids = self
            .chat_state_handle
            .pending_notifications()
            .await
            .ok_or_else(|| "restored child notification inbox is unavailable".to_string())?
            .into_iter()
            .filter(|notification| {
                notification.source == source && notification.source_version == source_version
            })
            .map(|notification| notification.id)
            .collect::<Vec<_>>();
        if notification_ids.is_empty() {
            return Ok(());
        }
        let turn = self
            .events
            .current_turn()
            .ok_or_else(|| "restored child result has no active original Turn".to_string())?;
        self.consume_notifications_durably(notification_ids, turn, None)
            .await
            .map_err(|error| format!("cannot consume restored child receipt: {error}"))
    }

    pub(super) async fn await_restored_children(&self) -> Result<(), acp::Error> {
        use tools::implementations::grow_build::task::types::SubagentSnapshotStatus;
        let operations = self
            .turn_continuation
            .borrow()
            .as_ref()
            .map(|cursor| cursor.operations.clone())
            .unwrap_or_default();
        for operation in operations {
            if operation.state != SuspendedOperationState::AwaitChild {
                continue;
            }
            let task_id = operation.dependency_id.ok_or_else(|| {
                acp::Error::internal_error().data("restored child wait has no identity")
            })?;
            let surface = self.chat_state_handle.get_conversation().await;
            let result_already_present = surface.iter().any(|item| {
                matches!(item, ConversationItem::ToolResult(result) if result.tool_call_id == operation.call_id)
            });
            let events = self
                .runtime_delegation_events()
                .await
                .map_err(|error| acp::Error::internal_error().data(error))?;
            let (spawn_seq, spawn) = events
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Spawned(spawn),
                    ) if spawn.subagent_id == task_id
                        && spawn.origin_tool_call_id.as_deref()
                            == Some(operation.call_id.as_str())
                        && spawn.security_parent_session_id == self.session_id_string() =>
                    {
                        Some((event.seq, spawn))
                    }
                    _ => None,
                })
                .ok_or_else(|| {
                    acp::Error::internal_error()
                        .data("original child spawn identity is unavailable")
                })?;
            let spawn = spawn.clone();
            let recipe = tools::implementations::grow_build::task::types::SubagentExecutionRecipe::from_value(&spawn.execution_recipe)
                .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
            if result_already_present && recipe.run_in_background {
                continue;
            }
            let body = if recipe.run_in_background {
                match self.tool_context.task_output_tool_name.as_deref() {
                    Some(output_tool) => tool_types::format_subagent_started_background(
                        &task_id,
                        &spawn.subagent_type,
                        &spawn.description,
                        output_tool,
                    ),
                    None => format!(
                        "Original background subagent restored. subagent_id: {task_id}. Its completion remains owned by the original coordinator."
                    ),
                }
            } else {
                let root_id = self
                    .chat_state_handle
                    .runtime_state()
                    .await
                    .map_err(|error| acp::Error::internal_error().data(error))?
                    .root_session_id
                    .unwrap_or_else(|| self.session_id_string());
                let canonical_events = self
                    .runtime_delegation_events()
                    .await
                    .map_err(|error| acp::Error::internal_error().data(error))?;
                if result_already_present
                    && matches!(
                        crate::agent::subagent::durable_child_operation_in_events(
                            &root_id,
                            &task_id,
                            canonical_events.clone(),
                        )
                        .map_err(|error| acp::Error::internal_error().data(error))?,
                        crate::agent::subagent::DurableChildOperation::Completed(_)
                    )
                {
                    if let Some(terminal) =
                        canonical_events.iter().find_map(|event| match &event.kind {
                            chat_state::TimelineEventKind::Subagent(
                                chat_state::SubagentEvent::Ended(terminal),
                            ) if terminal.subagent_id == task_id
                                && terminal.child_session_id == spawn.child_session_id =>
                            {
                                Some(terminal.clone())
                            }
                            _ => None,
                        })
                    {
                        self.settle_restored_child_completion_receipt(
                            &root_id, spawn_seq, &spawn, &terminal,
                        )
                        .await
                        .map_err(|error| acp::Error::internal_error().data(error))?;
                    }
                }
                if result_already_present {
                    continue;
                }
                let backend = self.tool_bridge_handle()
                    .read_resource::<tools::implementations::grow_build::task::backend::SubagentBackendResource>().await
                    .ok_or_else(|| acp::Error::internal_error().data("original child coordinator is unavailable"))?;
                loop {
                // An ended child is deliberately not adopted into a fresh
                // coordinator. Its canonical result still satisfies the
                // original wait, including completion during a partial restore.
                let canonical_events = self.runtime_delegation_events().await
                    .map_err(|error| acp::Error::internal_error().data(error))?;
                let status = match crate::agent::subagent::durable_child_operation_in_events(
                    &root_id,
                    &task_id,
                    canonical_events.clone(),
                ).map_err(|error| acp::Error::internal_error().data(error))? {
                    crate::agent::subagent::DurableChildOperation::Completed(result) => {
                        if result.output_unavailable {
                            return Err(acp::Error::internal_error().data("original child output is unavailable"));
                        }
                        if result.success {
                            SubagentSnapshotStatus::Completed {
                                output: result.output.to_string(),
                                tool_calls: result.tool_calls,
                                turns: result.turns,
                                tokens_used: result.tokens_used,
                                worktree_path: spawn.worktree_path.clone(),
                            }
                        } else if result.cancelled {
                            SubagentSnapshotStatus::Cancelled { reason: result.error }
                        } else {
                            SubagentSnapshotStatus::Failed {
                                error: result.error.unwrap_or_else(|| "original child failed".into()),
                            }
                        }
                    }
                    crate::agent::subagent::DurableChildOperation::Missing => {
                        return Err(acp::Error::internal_error().data("original child spawn is unavailable"));
                    }
                    crate::agent::subagent::DurableChildOperation::Open => backend
                    .backend()
                    .query(&task_id, true, Some(25_000))
                    .await
                    .map_err(|error| {
                        acp::Error::internal_error()
                            .data(format!("original child wait remains unconfirmed: {error}"))
                    })?
                    .ok_or_else(|| {
                        acp::Error::internal_error().data(
                            "original child is absent; it will not be replaced by a new spawn",
                        )
                    })?.status,
                };
                match status {
                    SubagentSnapshotStatus::Completed {
                        output,
                        tool_calls,
                        turns,
                        tokens_used,
                        worktree_path,
                    } => {
                        if let Some(terminal) = canonical_events.iter().find_map(|event| {
                            match &event.kind {
                                chat_state::TimelineEventKind::Subagent(
                                    chat_state::SubagentEvent::Ended(terminal),
                                ) if terminal.subagent_id == task_id
                                    && terminal.child_session_id == spawn.child_session_id =>
                                {
                                    Some(terminal.clone())
                                }
                                _ => None,
                            }
                        }) {
                            self.settle_restored_child_completion_receipt(
                                &root_id,
                                spawn_seq,
                                &spawn,
                                &terminal,
                            )
                            .await
                            .map_err(|error| acp::Error::internal_error().data(error))?;
                        }
                        let summary = self
                            .restored_child_planning_summary(&spawn, &task_id)
                            .await
                            .map_err(|error| acp::Error::internal_error().data(error))?;
                        break Self::add_restored_planning_summary(
                            json!({ "subagent_id": task_id, "status": "completed", "output": output, "tool_calls": tool_calls, "turns": turns, "tokens_used": tokens_used, "worktree_path": worktree_path }),
                            summary,
                        )
                        .map_err(|error| acp::Error::internal_error().data(error))?;
                    }
                    SubagentSnapshotStatus::Failed { error } => {
                        if let Some(terminal) = canonical_events.iter().find_map(|event| {
                            match &event.kind {
                                chat_state::TimelineEventKind::Subagent(
                                    chat_state::SubagentEvent::Ended(terminal),
                                ) if terminal.subagent_id == task_id
                                    && terminal.child_session_id == spawn.child_session_id =>
                                {
                                    Some(terminal.clone())
                                }
                                _ => None,
                            }
                        }) {
                            self.settle_restored_child_completion_receipt(
                                &root_id,
                                spawn_seq,
                                &spawn,
                                &terminal,
                            )
                            .await
                            .map_err(|error| acp::Error::internal_error().data(error))?;
                        }
                        let summary = self
                            .restored_child_planning_summary(&spawn, &task_id)
                            .await
                            .map_err(|error| acp::Error::internal_error().data(error))?;
                        break Self::add_restored_planning_summary(
                            json!({ "subagent_id": task_id, "status": "failed", "error": error }),
                            summary,
                        )
                        .map_err(|error| acp::Error::internal_error().data(error))?;
                    }
                    SubagentSnapshotStatus::Cancelled { reason } => {
                        let summary = self
                            .restored_child_planning_summary(&spawn, &task_id)
                            .await
                            .map_err(|error| acp::Error::internal_error().data(error))?;
                        break Self::add_restored_planning_summary(
                            json!({ "subagent_id": task_id, "status": "cancelled", "reason": reason }),
                            summary,
                        )
                        .map_err(|error| acp::Error::internal_error().data(error))?;
                    }
                    SubagentSnapshotStatus::CompletedOutputUnavailable { error, .. } => {
                        return Err(acp::Error::internal_error()
                            .data(format!("original child output is unavailable: {error}")));
                    }
                    SubagentSnapshotStatus::Suspended { .. } => {
                        return Err(acp::Error::internal_error()
                            .data("tree release left an original child suspended"));
                    }
                    SubagentSnapshotStatus::Running { .. }
                    | SubagentSnapshotStatus::Initializing
                    | SubagentSnapshotStatus::FinalizationPending { .. } => {}
                }
            }.to_string()
            };
            self.chat_state_handle
                .push_tool_result_durably(ConversationItem::tool_result(
                    operation.call_id.clone(),
                    body,
                ))
                .await
                .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
            self.events
                .tool_completed_durably(
                    &operation.call_id,
                    "completed".into(),
                    Some(json!({"rebound_child": task_id})),
                )
                .await
                .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
        }
        Ok(())
    }

    pub(super) async fn prepare_runtime_restore(
        self: &Arc<Self>,
        operation_id: &str,
        instance_id: &str,
        completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
    ) -> Result<(), String> {
        let runtime = self.chat_state_handle.runtime_state().await?;
        if runtime.operation_id.as_deref() != Some(operation_id)
            || !matches!(
                runtime.phase,
                RuntimePhase::Suspended | RuntimePhase::Restoring
            )
            || (runtime.phase == RuntimePhase::Restoring
                && runtime.instance_id.as_deref() != Some(instance_id))
        {
            return Err("restoration does not own this checkpoint".into());
        }
        let checkpoint = runtime
            .checkpoint
            .as_ref()
            .ok_or("runtime checkpoint missing")?;
        if self.runtime_source_hash().await? != checkpoint.runtime_source_hash {
            return Err(
                "original workspace, Agent source or model route changed; tree remains suspended"
                    .into(),
            );
        }
        let events = self
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or("Timeline unavailable")?;
        let mut tool_names = std::collections::BTreeMap::new();
        let mut goal_id = None;
        let mut turn_kind = crate::session::TurnKind::Internal;
        let mut original_input_ids = Vec::new();
        let mut original_prompt_text = String::new();
        for event in &events {
            match &event.kind {
                chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Started {
                    call_id,
                    name,
                    ..
                }) => {
                    tool_names.insert(call_id.clone(), name.clone());
                }
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started {
                    prompt_id,
                    id: turn,
                    identity,
                    input_ids,
                    prompt_text,
                    ..
                }) if checkpoint.turn.as_ref().is_some_and(|cursor| {
                    cursor.turn_id == *turn && cursor.prompt_id.as_str() == prompt_id.as_str()
                }) =>
                {
                    original_input_ids = input_ids.clone();
                    original_prompt_text = prompt_text.clone();
                    goal_id = identity.goal_id.clone();
                    turn_kind = match identity.turn_kind.as_str() {
                        "user" => crate::session::TurnKind::User,
                        "internal" => crate::session::TurnKind::Internal,
                        _ => return Err("checkpoint Turn kind invalid".into()),
                    };
                }
                _ => {}
            }
        }
        if let Some(cursor) = &checkpoint.turn {
            if cursor.resume_point == TurnResumePoint::Input && events.iter().any(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Started { turn, .. }) if *turn == cursor.turn_id)) {
                return Err("input preparation contains a previous tool preflight; its outcome must be reconciled before reloading this input".into());
            }
            if let Some(response) = &cursor.accepted_response {
                let admitted = self
                    .chat_state_handle
                    .admitted_response(response.identity.clone())
                    .await
                    .ok_or_else(|| {
                        "checkpoint accepted response is absent from canonical history".to_owned()
                    })?;
                let expected_event = if self.startup_hints.is_subagent {
                    chat_state::HookEventType::SubagentStop
                } else {
                    chat_state::HookEventType::Stop
                };
                let subagent_id = self
                    .startup_hints
                    .is_subagent
                    .then(|| self.session_id_string());
                super::stop_gate::validate_restored_stop_gate(
                    &events,
                    admitted.event_seq,
                    cursor.turn_id,
                    expected_event,
                    subagent_id.as_deref(),
                )?;
            }
            let _: PromptOrigin = serde_json::from_str(&cursor.origin)
                .map_err(|error| format!("invalid original prompt origin: {error}"))?;
        }
        let mut original_blocks = Vec::new();
        if checkpoint
            .turn
            .as_ref()
            .is_some_and(|cursor| cursor.resume_point == TurnResumePoint::Input)
        {
            if original_input_ids.is_empty() {
                original_blocks.push(acp::ContentBlock::Text(acp::TextContent::new(
                    original_prompt_text,
                )));
            } else {
                for input_id in &original_input_ids {
                    let payload_ref = events
                        .iter()
                        .find_map(|event| match &event.kind {
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted {
                                    input_id: id,
                                    payload_ref,
                                    ..
                                },
                            ) if id == input_id => Some(payload_ref.clone()),
                            _ => None,
                        })
                        .ok_or_else(|| format!("original input {input_id} is missing"))?;
                    let directory = self
                        .session_directory
                        .try_clone()
                        .map_err(|error| error.to_string())?;
                    let payload = tokio::task::spawn_blocking(move || {
                        crate::session::input_inbox::read_payload(&directory, &payload_ref)
                    })
                    .await
                    .map_err(|error| error.to_string())?
                    .map_err(|error| error.to_string())?;
                    let crate::session::input_inbox::InputPayload::Prompt { prompt_blocks, .. } =
                        payload;
                    original_blocks.extend(prompt_blocks);
                }
            }
        }
        self.workflow_manager
            .lock()
            .await
            .validate_suspended_executions()?;
        if self.restoration_release.borrow().is_some() || self.turn_continuation.borrow().is_some()
        {
            return Ok(());
        }
        if runtime.phase == RuntimePhase::Suspended {
            self.chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                    SessionRuntimeEvent::RestorePrepared {
                        operation_id: operation_id.to_owned(),
                        instance_id: instance_id.to_owned(),
                    },
                ))
                .await
                .map_err(|error| error.to_string())?;
        }
        self.events
            .restore_checkpoint(checkpoint, goal_id, &tool_names)?;
        if let Some(cursor) = checkpoint.turn.clone() {
            let origin: PromptOrigin =
                serde_json::from_str(&cursor.origin).map_err(|error| error.to_string())?;
            *self.sampling_continuation.borrow_mut() = Some(sampler::RecoveryBudget::restore(
                sampler::RecoveryBudgetSnapshot {
                    attempts: cursor.sampling_attempts_used,
                    max_attempts: cursor.sampling_max_attempts,
                    replay_safe: cursor.sampling_replay_safe,
                    deadline_ms: cursor.sampling_deadline_ms,
                },
            )?);
            if let (Some((spent, incomplete)), Some(budget)) = (
                cursor.task_output_usage,
                &self.tool_context.task_output_token_budget,
            ) {
                budget.record_reported_output(spent);
                if incomplete {
                    budget.mark_incomplete_and_exhaust();
                }
            }
            *self.restored_response.borrow_mut() = cursor.accepted_response.clone();
            *self.turn_continuation.borrow_mut() = Some(cursor.clone());
            let behavior = self.behavior.lock().behavior();
            let (release, wait) = oneshot::channel();
            let mut state = self.state.lock().await;
            if !state.foreground.is_idle() {
                return Err("restoration encountered another foreground owner".into());
            }
            state.foreground = ForegroundState::RegularTurn(AgentTask::new_prompt(
                self.clone(),
                cursor.prompt_id.clone(),
                if cursor.resume_point != TurnResumePoint::Model {
                    original_input_ids.clone()
                } else {
                    Vec::new()
                },
                origin.clone(),
                None,
                Vec::new(),
                turn_kind,
                original_blocks,
                behavior,
                None,
                None,
                true,
                cursor.output_schema.clone(),
                Some(wait),
                None,
                completion_tx,
                None,
            ));
            *self.restoration_release.borrow_mut() = Some(release);
            drop(state);
            self.publish_turn_scope_resources(cursor.prompt_id, &origin, behavior)
                .await;
        }
        Ok(())
    }

    pub(super) async fn commit_runtime_resume(
        &self,
        operation_id: String,
        instance_id: String,
    ) -> Result<(), String> {
        let runtime = self.chat_state_handle.runtime_state().await?;
        if runtime.resume_committed
            && runtime.instance_id.as_ref() == Some(&instance_id)
            && runtime.operation_id.as_ref() == Some(&operation_id)
        {
            return Ok(());
        }
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                SessionRuntimeEvent::Resumed {
                    operation_id,
                    instance_id,
                },
            ))
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(super) async fn release_runtime_restore(
        self: &Arc<Self>,
        instance_id: &str,
    ) -> Result<(), String> {
        let runtime = self.chat_state_handle.runtime_state().await?;
        if !runtime.resume_committed
            || runtime.instance_id.as_deref() != Some(instance_id)
            || !matches!(
                runtime.phase,
                RuntimePhase::Restoring | RuntimePhase::Running
            )
        {
            return Err("runtime restoration has no committed execution right".into());
        }
        if !self
            .state
            .lock()
            .await
            .runtime_suspend
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Ok(());
        }
        if runtime.phase == RuntimePhase::Restoring {
            self.chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                    SessionRuntimeEvent::ExecutionReleased {
                        operation_id: runtime
                            .operation_id
                            .clone()
                            .ok_or("restored operation identity missing")?,
                        instance_id: instance_id.to_owned(),
                    },
                ))
                .await
                .map_err(|error| error.to_string())?;
        }
        // Keep the original Turn and Session admission closed until every
        // physical Workflow owner has crossed its own durable Resumed
        // boundary. A partial failure can retry this same root release: Runs
        // already resumed are no longer candidates and keep their original id.
        let mut manager = self.workflow_manager.lock().await;
        manager.reopen_after_runtime_restore();
        manager.restore_suspended_executions().await?;
        drop(manager);
        self.state
            .lock()
            .await
            .runtime_suspend
            .store(false, std::sync::atomic::Ordering::Release);
        self.goal_tracker.lock().resume_runtime_clock();
        if let Some(release) = self.restoration_release.borrow_mut().take() {
            let _ = release.send(());
        }
        for (operation, waiter) in self.runtime_release_waiters.borrow_mut().drain(..) {
            let _ = waiter.send(if runtime.operation_id.as_deref() == Some(&operation) {
                Ok(())
            } else {
                Err("another runtime operation owns this session".into())
            });
        }
        self.idle_arbiter.notify_one();
        self.schedule_planning_delivery();
        self.send_available_commands_update().await;
        Ok(())
    }

    pub(super) async fn retire_suspended_runtime(
        &self,
        operation_id: &str,
        deadline: tokio::time::Instant,
    ) -> Result<(), String> {
        let runtime = self.chat_state_handle.runtime_state().await?;
        if runtime.phase != RuntimePhase::Suspended
            || runtime.operation_id.as_deref() != Some(operation_id)
            || !self.session_activities.is_idle()
        {
            return Err("runtime has not handed off a settled checkpoint".into());
        }
        // No SessionEnd hook, Goal close, child cancellation, or FIFO dismissal
        // belongs to this physical retirement. Those change logical work.
        self.sideband_cancel.cancel();
        self.finalizer_sideband_cancel.cancel();
        self.sideband_repair_cancel.cancel();
        self.durable_ui_cancel.cancel();
        self.signals_handle.shutdown();
        let (respond_to, ack) = oneshot::channel();
        self.notifications
            .persistence_tx
            .send(PersistenceMsg::FlushAndAck { respond_to })
            .map_err(|_| "suspended persistence owner is unavailable".to_string())?;
        tokio::time::timeout_at(deadline, ack)
            .await
            .map_err(|_| "checkpoint saved; writer retirement is not confirmed".to_string())?
            .map_err(|_| "suspended writer flush acknowledgement was lost".to_string())?;
        Ok(())
    }

    pub(super) async fn request_node_suspension(
        self: &Arc<Self>,
        operation_id: String,
        root_session_id: String,
        deadline: tokio::time::Instant,
        respond_to: oneshot::Sender<Result<RuntimeNodeRef, String>>,
    ) {
        if let Some(reason) = self.runtime_suspension_fault.borrow().as_ref() {
            let _ = respond_to.send(Err(reason.clone()));
            return;
        }
        self.state
            .lock()
            .await
            .runtime_suspend
            .store(true, std::sync::atomic::Ordering::Release);
        let runtime = match self.chat_state_handle.runtime_state().await {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = respond_to.send(Err(error));
                return;
            }
        };
        if runtime.phase != RuntimePhase::Running
            && (runtime.operation_id.as_deref() != Some(&operation_id)
                || runtime.root_session_id.as_deref() != Some(&root_session_id))
        {
            let _ = respond_to.send(Err("another suspension operation owns this session".into()));
            return;
        }
        if runtime.phase == RuntimePhase::Suspended {
            let result = runtime
                .checkpoint_seq
                .map(|checkpoint_seq| RuntimeNodeRef {
                    session_id: self.session_id_string(),
                    checkpoint_seq,
                })
                .ok_or_else(|| "suspended node has no checkpoint".to_string());
            let _ = respond_to.send(result);
            return;
        }
        if self.runtime_suspension_worker.is_running() {
            let _ = respond_to.send(Err(
                "suspension is still settling; retry the same operation identity".into(),
            ));
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = respond_to.send(Err(
                "suspension request deadline elapsed before admission; retry with a fresh deadline"
                    .into(),
            ));
            return;
        }
        // Fence new auxiliary work before the worker waits for its first durable ACK.
        self.session_activities.close_admission();
        let session = self.clone();
        self.runtime_suspension_worker
            .arm(tokio::task::spawn_local(async move {
                let result = session
                    .suspend_node(operation_id, root_session_id, deadline)
                    .await;
                let _ = respond_to.send(result);
            }));
    }

    async fn suspend_node(
        &self,
        operation_id: String,
        root_session_id: String,
        deadline: tokio::time::Instant,
    ) -> Result<RuntimeNodeRef, String> {
        let started = std::time::Instant::now();
        tracing::info!(session_id = %self.session_info.id.0, %operation_id, phase = "started", "session suspension progress");
        // Keep this owner alive when the caller's shared exit deadline expires.
        // No checkpoint ACK is returned while a physical producer is unjoined.
        let pending = self
            .pending_interactions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let pending_attempts = self.pending_interactions.snapshot_attempts();
        if self.chat_state_handle.runtime_state().await?.phase == RuntimePhase::Running {
            self.chat_state_handle
                .record_timeline_event_durably_before(
                    chat_state::TimelineEventKind::Runtime(SessionRuntimeEvent::SuspendRequested {
                        operation_id: operation_id.clone(),
                        root_session_id,
                    }),
                    deadline,
                )
                .await
                .map_err(|error| error.to_string())?;
        }
        self.workflow_manager.lock().await.close_admission();
        self.workflow_service_shutdown.cancel();
        self.sideband_cancel.cancel();
        self.finalizer_sideband_cancel.cancel();
        self.sideband_repair_cancel.cancel();
        // Let an active compaction owner observe cancellation and join its
        // Sideband/Goal settlement before we consider aborting its turn.
        self.compaction.cancel.request_cancel();
        let task = self
            .state
            .lock()
            .await
            .foreground
            .regular()
            .map(|task| (task.handle.clone(), task.stopped.clone()));
        if let Some((handle, stopped)) = task {
            if self.compaction.lease.is_in_flight() {
                tokio::time::timeout_at(deadline, stopped)
                    .await
                    .map_err(|_| {
                        "active compaction owner did not settle before the suspension deadline"
                            .to_string()
                    })?
                    .map_err(|_| {
                        "active compaction owner stopped without acknowledgement".to_string()
                    })?;
            } else {
                handle.abort();
                let _ = stopped.await;
            }
        }
        self.workflow_manager
            .lock()
            .await
            .suspend_all_and_drain(deadline)
            .await?;
        self.goal_drive
            .abort_and_join()
            .await
            .map_err(|error| error.to_string())?;
        self.restored_plan_approval
            .abort_and_join()
            .await
            .map_err(|error| error.to_string())?;
        self.deferred_prefix
            .abort_and_join()
            .await
            .map_err(|error| error.to_string())?;
        quiesce_control_workers(self).await?;
        self.cancel_background_compaction_before("session_suspended", Some(deadline))
            .await
            .map_err(|error| error.to_string())?;
        self.sampler_handle.close();
        if let Some(mut owner) = self.sampler_owner.borrow_mut().take() {
            // This worker owns the real join after a caller times out. The
            // generic shutdown_bounded aborts the sampler on timeout, which
            // would discard settlement evidence and cannot authorize a retry.
            if let Err(error) = owner.shutdown().await {
                let reason = format!("sampler settlement was not confirmed: {error}");
                *self.runtime_suspension_fault.borrow_mut() = Some(reason.clone());
                return Err(reason);
            }
        }
        if let Some(drainer) = self.sampler_event_drainer.take() {
            if let Err(error) = drainer.await {
                let reason = format!("sampler settlement drainer failed: {error}");
                *self.runtime_suspension_fault.borrow_mut() = Some(reason.clone());
                return Err(reason);
            }
        }
        self.events.settle_requests_for_suspension().await;
        tracing::info!(session_id = %self.session_info.id.0, %operation_id, phase = "sampler_settled", elapsed_ms = started.elapsed().as_millis() as u64, "session suspension progress");
        let previous_commands = self.chat_state_handle.runtime_state().await?.commands;
        let commands = reuse_or_capture_command_settlements(previous_commands, || async {
            let bridge = self.tool_bridge_handle();
            let mut command_evidence = bridge
                .suspend_owned_commands(&self.session_id_string(), deadline)
                .await
                .map_err(|error| format!("command retirement is not confirmed: {error}"))?;
            command_evidence
                .sort_by(|left, right| left.snapshot.task_id.cmp(&right.snapshot.task_id));
            let commands = command_evidence
                .iter()
                .map(|evidence| {
                    let command = &evidence.snapshot;
                    RuntimeCommandSettlement {
                        command_id: command.task_id.clone(),
                        call_id: evidence.tool_call_id.clone(),
                        outcome: if evidence.interrupted {
                            CommandSettlementStatus::Interrupted
                        } else {
                            CommandSettlementStatus::Completed
                        },
                        output_file: command.output_file.to_string_lossy().into_owned(),
                        exit_code: command.exit_code,
                        signal: command.signal.clone(),
                        output: sampling_types::truncate_bytes(&command.output, 64 * 1024)
                            .to_owned(),
                        is_backgrounded: command.is_backgrounded,
                        owner: match (&command.goal_id, command.goal_definition_revision) {
                            (Some(goal_id), Some(definition_revision)) => {
                                chat_state::NotificationOwner::Goal {
                                    goal_id: goal_id.clone(),
                                    definition_revision,
                                }
                            }
                            _ => chat_state::NotificationOwner::Session,
                        },
                        task_kind: if command.kind == tools::computer::types::TaskKind::Monitor {
                            chat_state::NotificationTaskKind::Monitor
                        } else {
                            chat_state::NotificationTaskKind::Task
                        },
                    }
                })
                .collect::<Vec<_>>();
            self.chat_state_handle
                .record_timeline_event_durably_before(
                    chat_state::TimelineEventKind::Runtime(SessionRuntimeEvent::CommandsSettled {
                        operation_id: operation_id.clone(),
                        commands: commands.clone(),
                    }),
                    deadline,
                )
                .await
                .map_err(|error| error.to_string())?;
            Ok(commands)
        })
        .await?;
        tracing::info!(session_id = %self.session_info.id.0, %operation_id, phase = "commands_settled", elapsed_ms = started.elapsed().as_millis() as u64, "session suspension progress");
        for command in &commands {
            if !command.is_backgrounded {
                continue;
            }
            self.receive_notification_before(
                chat_state::NotificationSource::TaskCompleted {
                    task_id: command.command_id.clone(),
                    task_kind: command.task_kind,
                    owner: command.owner.clone(),
                },
                chat_state::NotificationSourceVersion::Opaque {
                    value: format!("session-suspend:{operation_id}"),
                },
                command_suspension_result(command)?,
                Some(deadline),
            )
            .await?;
        }
        let events = self
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or("suspension evidence unavailable")?;
        let surface = self.chat_state_handle.get_conversation().await;
        let delegation_events = self.runtime_delegation_events().await?;
        let mut cursor = self.turn_continuation.borrow().clone();
        if cursor.as_ref().is_some_and(|cursor| events.iter().any(|event| matches!(&event.kind,
            chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended { id, .. }) if *id == cursor.turn_id))) {
            // Explicit cancellation and genuine completion already own their
            // terminal. A stale in-memory cursor cannot resurrect that work.
            cursor = None;
        }
        let canonical_response_seq = match cursor
            .as_ref()
            .and_then(|turn| turn.accepted_response.as_ref())
        {
            Some(accepted) => self
                .chat_state_handle
                .admitted_response(accepted.identity.clone())
                .await
                .map(|response| response.event_seq),
            None => None,
        };
        if let Some(turn) = cursor.as_mut() {
            align_suspension_cursor(
                turn,
                &events,
                canonical_response_seq,
                &self.session_id_string(),
            )?;
            if turn.resume_point == TurnResumePoint::Input && events.iter().any(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed { turn: input_turn, .. }) if *input_turn == turn.turn_id)) {
                turn.resume_point = TurnResumePoint::Model;
            }
            turn.task_output_usage = self
                .tool_context
                .task_output_token_budget
                .as_ref()
                .map(|budget| budget.usage());
            turn.operations.clear();
            turn.next_step_index = self.events.next_step_index();
            if let Some(budget) = self.sampling_continuation.borrow().as_ref() {
                let budget = budget.snapshot();
                turn.sampling_attempts_used = budget.attempts;
                turn.sampling_max_attempts = budget.max_attempts;
                turn.sampling_deadline_ms = budget.deadline_ms;
                turn.sampling_replay_safe = budget.replay_safe;
            }
            let mut calls = std::collections::BTreeMap::new();
            for item in &surface {
                if let ConversationItem::Assistant(assistant) = item {
                    for call in &assistant.tool_calls {
                        calls.insert(call.id.to_string(), call.name.to_string());
                    }
                }
            }
            for item in &surface {
                if let ConversationItem::ToolResult(result) = item {
                    calls.remove(&result.tool_call_id);
                }
            }
            let model_call_ids = calls
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            for (call_id, _name) in calls {
                let child = delegation_events
                    .iter()
                    .rev()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Subagent(
                            chat_state::SubagentEvent::Spawned(spawn),
                        ) if spawn.origin_tool_call_id.as_deref() == Some(&call_id)
                            && spawn.security_parent_session_id.as_str()
                                == self.session_info.id.0.as_ref() =>
                        {
                            Some(spawn)
                        }
                        _ => None,
                    });
                let started_input = events.iter().rev().find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Started {
                        call_id: id,
                        input,
                        ..
                    }) if id == &call_id => Some(input),
                    _ => None,
                });
                let command = commands.iter().find(|command| command.call_id == call_id);
                let (state, dependency_id) = if let Some(child) = child {
                    (
                        SuspendedOperationState::AwaitChild,
                        Some(child.subagent_id.clone()),
                    )
                } else if let Some(kind) = pending.get(&call_id) {
                    match kind {
                        crate::session::pending_interaction::PendingKind::Question => {
                            (SuspendedOperationState::AwaitUser, Some(call_id.clone()))
                        }
                        crate::session::pending_interaction::PendingKind::Permission => (
                            SuspendedOperationState::AwaitPermission,
                            Some(call_id.clone()),
                        ),
                        crate::session::pending_interaction::PendingKind::PlanApproval => {
                            (SuspendedOperationState::AwaitUser, Some(call_id.clone()))
                        }
                    }
                } else if started_input.is_none() {
                    (SuspendedOperationState::NotDispatched, None)
                } else if let Some(command) = command {
                    (
                        if command.outcome == CommandSettlementStatus::Interrupted {
                            SuspendedOperationState::Interrupted
                        } else {
                            SuspendedOperationState::Completed
                        },
                        Some(command.command_id.clone()),
                    )
                } else {
                    (SuspendedOperationState::OutcomeUnknown, None)
                };
                if matches!(
                    state,
                    SuspendedOperationState::Interrupted
                        | SuspendedOperationState::OutcomeUnknown
                        | SuspendedOperationState::Completed
                ) {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(
                            "suspension deadline expired before operation settlement".into()
                        );
                    }
                    let message = if let Some(command) = command {
                        command_suspension_result(command)?
                    } else {
                        format!(
                            "Session suspended this operation ({state:?}). It may have made partial changes. Inspect the current state before deciding on a new operation; this call will not be automatically replayed."
                        )
                    };
                    self.chat_state_handle
                        .push_tool_result_durably(ConversationItem::tool_result(
                            call_id.clone(),
                            message,
                        ))
                        .await
                        .map_err(|error| error.to_string())?;
                    self.events
                        .tool_completed_durably(
                            &call_id,
                            if state == SuspendedOperationState::Completed {
                                "completed"
                            } else if state == SuspendedOperationState::Interrupted {
                                "interrupted"
                            } else {
                                "outcome_unknown"
                            }
                            .into(),
                            None,
                        )
                        .await
                        .map_err(|error| error.to_string())?;
                    self.send_suspension_tool_update(&call_id, &state, command.map(command_suspension_result).transpose()?.unwrap_or_else(|| format!("Session suspended: {state:?}; this operation will not be replayed."))).await;
                }
                turn.operations.push(SuspendedOperation {
                    call_id,
                    state,
                    dependency_id,
                });
            }
            // Host operations and user Skill loads have Tool facts but no
            // assistant tool call. Settle their existing cards without adding
            // an orphan model ToolResult. A published Skill body is already
            // canonical; an uncommitted preparation remains explicitly unknown.
            let mut host_calls = std::collections::BTreeSet::new();
            for event in &events {
                match &event.kind {
                    chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Started {
                        call_id,
                        turn: id,
                        ..
                    }) if *id == turn.turn_id => {
                        host_calls.insert(call_id.clone());
                    }
                    chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Completed {
                        call_id,
                        ..
                    }) => {
                        host_calls.remove(call_id);
                    }
                    _ => {}
                }
            }
            for call_id in host_calls.difference(&model_call_ids) {
                let command = commands.iter().find(|command| command.call_id == *call_id);
                let published_skill = surface.iter().any(|item| matches!(item, ConversationItem::SkillLoad(skill) if skill.call_id == *call_id));
                let outcome = if published_skill
                    || command.is_some_and(|command| {
                        command.outcome != CommandSettlementStatus::Interrupted
                    }) {
                    SuspendedOperationState::Completed
                } else if command.is_some() {
                    SuspendedOperationState::Interrupted
                } else {
                    SuspendedOperationState::OutcomeUnknown
                };
                self.events
                    .tool_completed_durably(
                        call_id,
                        match outcome {
                            SuspendedOperationState::Completed => "completed",
                            SuspendedOperationState::Interrupted => "interrupted",
                            _ => "outcome_unknown",
                        }
                        .into(),
                        Some(json!({"session_suspended": true})),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                self.send_suspension_tool_update(call_id, &outcome, command.map(command_suspension_result).transpose()?.unwrap_or_else(|| format!("Session suspended: {outcome:?}; this operation will not be replayed."))).await;
            }
        }
        // All Goal state survives. Only its execution-time clock is frozen.
        self.goal_tracker.lock().freeze_runtime_clock();
        let (behavior, goal) = self.capture_control_authorities();
        self.persist_control_snapshot_durably(behavior, goal)
            .await
            .map_err(|error| error.to_string())?;
        // Background storage workers must relinquish their exact ownership;
        // stopping the caller's timeout cannot detach them behind this frontier.
        self.background_service_shutdown.cancel();
        tracing::info!(session_id = %self.session_info.id.0, %operation_id, phase = "draining_services", elapsed_ms = started.elapsed().as_millis() as u64, "session suspension progress");
        stop_session_background_services(self, false).await?;
        tracing::info!(session_id = %self.session_info.id.0, %operation_id, phase = "draining_auxiliary_owners", elapsed_ms = started.elapsed().as_millis() as u64, "session suspension progress");
        self.session_activities.wait_idle().await;
        stop_permission_manager_and_drain_audit(self).await?;
        tracing::info!(session_id = %self.session_info.id.0, %operation_id, phase = "owners_drained", elapsed_ms = started.elapsed().as_millis() as u64, "session suspension progress");
        // Physical drain may outlive the caller. Do not start a new durable
        // checkpoint write with its expired deadline and poison a healthy writer.
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "suspension owners settled after the deadline; retry the same operation identity"
                    .into(),
            );
        }
        let mut open_interactions = std::collections::BTreeSet::new();
        for event in self
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or("suspension interaction evidence unavailable")?
        {
            match &event.kind {
                chat_state::TimelineEventKind::Interaction(
                    chat_state::InteractionEvent::Opened {
                        tool_call_id,
                        attempt_id,
                        kind,
                    },
                ) => {
                    open_interactions.insert((tool_call_id.clone(), attempt_id.clone(), *kind));
                }
                chat_state::TimelineEventKind::Interaction(
                    chat_state::InteractionEvent::Resolved {
                        tool_call_id,
                        attempt_id,
                        kind,
                        ..
                    },
                ) => {
                    open_interactions.remove(&(tool_call_id.clone(), attempt_id.clone(), *kind));
                }
                _ => {}
            }
        }
        for (call_id, pending_kind, attempt_id) in &pending_attempts {
            let kind = match pending_kind {
                crate::session::pending_interaction::PendingKind::Question => {
                    chat_state::InteractionKind::Question
                }
                crate::session::pending_interaction::PendingKind::Permission => {
                    chat_state::InteractionKind::Permission
                }
                crate::session::pending_interaction::PendingKind::PlanApproval => {
                    chat_state::InteractionKind::PlanApproval
                }
            };
            if open_interactions.remove(&(call_id.clone(), attempt_id.clone(), kind)) {
                self.chat_state_handle
                    .record_timeline_event_durably_before(
                        chat_state::TimelineEventKind::Interaction(
                            chat_state::InteractionEvent::Resolved {
                                tool_call_id: call_id.clone(),
                                attempt_id: attempt_id.clone(),
                                kind,
                                resolution: chat_state::InteractionResolution::Suspended,
                            },
                        ),
                        deadline,
                    )
                    .await
                    .map_err(|error| {
                        format!("interaction suspension could not be durably recorded: {error}")
                    })?;
            }
        }
        self.chat_state_handle
            .settle_hooks_for_suspension(operation_id.clone(), deadline)
            .await
            .map_err(|error| error.to_string())?;
        let checkpoint = self
            .chat_state_handle
            .checkpoint_runtime(
                operation_id.clone(),
                self.runtime_source_hash().await?,
                cursor,
                self.events.active_step_id(),
                deadline,
            )
            .await
            .map_err(|error| error.to_string())?;
        let node = RuntimeNodeRef {
            session_id: self.session_id_string(),
            checkpoint_seq: checkpoint.seq,
        };
        tracing::info!(session_id = %self.session_info.id.0, %operation_id, phase = "checkpoint_committed", elapsed_ms = started.elapsed().as_millis() as u64, "session suspension progress");
        self.resolve_runtime_suspended_waiters(node.clone());
        for watcher in self.runtime_completion_watchers.borrow_mut().drain(..) {
            let _ = watcher.send(Ok(crate::session::commands::PromptTurnOk {
                stop_reason: acp::StopReason::Cancelled,
                total_tokens: 0,
                turn_snapshot: None,
                completion_kind: PromptCompletionKind::Suspended {
                    operation_id: operation_id.clone(),
                    checkpoint_seq: checkpoint.seq.get(),
                },
                structured_output: None,
                usage: None,
            }));
        }
        // Resolve the old RPC without consuming/dismissing the input or ending
        // its logical Turn. The new runtime will bind a fresh transport waiter.
        let mut state = self.state.lock().await;
        let running = state.running_prompt_id().map(str::to_owned);
        if let Some(input) = state
            .pending_inputs
            .iter_mut()
            .find(|input| Some(input.prompt_id.as_str()) == running.as_deref())
        {
            let (replacement, _) = oneshot::channel();
            let responder = std::mem::replace(&mut input.respond_to, replacement);
            let _ = responder.send(Ok(crate::session::commands::PromptTurnOk {
                stop_reason: acp::StopReason::Cancelled,
                total_tokens: 0,
                turn_snapshot: None,
                completion_kind: PromptCompletionKind::Suspended {
                    operation_id,
                    checkpoint_seq: checkpoint.seq.get(),
                },
                structured_output: None,
                usage: None,
            }));
        }
        Ok(node)
    }

    async fn send_suspension_tool_update(
        &self,
        call_id: &str,
        outcome: &SuspendedOperationState,
        message: String,
    ) {
        self.send_update(
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                acp::ToolCallId::new(call_id.to_owned()),
                acp::ToolCallUpdateFields::new()
                    .status(Some(if *outcome == SuspendedOperationState::Completed {
                        acp::ToolCallStatus::Completed
                    } else {
                        acp::ToolCallStatus::Failed
                    }))
                    .content(Some(vec![acp::ToolCallContent::from(
                        acp::ContentBlock::Text(acp::TextContent::new(message)),
                    )])),
            )),
            None,
        )
        .await;
    }
}

/// Align the in-memory cursor with the durable Timeline prefix. A committed
/// next Step is authoritative even when the caller has not received its ACK.
fn align_suspension_cursor(
    turn: &mut chat_state::session_runtime::TurnContinuation,
    events: &[chat_state::TimelineEvent],
    canonical_response_seq: Option<chat_state::EventSeq>,
    session_id: &str,
) -> Result<(), &'static str> {
    if turn.accepted_response.is_some() {
        let response_seq = canonical_response_seq;
        let response_has_been_consumed = response_seq.is_some_and(|response_seq| {
            events.iter().any(|event| {
                event.seq > response_seq
                    && matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Step(
                            chat_state::StepEvent::Started { id }
                        ) if id.turn == turn.turn_id
                    )
            })
        });
        if response_seq.is_none() || response_has_been_consumed {
            turn.accepted_response = None;
        }
    }

    let Some(turn_started_seq) = events.iter().find_map(|event| match &event.kind {
        chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { id, .. })
            if *id == turn.turn_id =>
        {
            Some(event.seq)
        }
        _ => None,
    }) else {
        return Ok(());
    };
    let turn_end_seq = events.iter().find_map(|event| match &event.kind {
        chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended { id, .. })
            if *id == turn.turn_id && event.seq > turn_started_seq =>
        {
            Some(event.seq)
        }
        chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { id, .. })
            if *id != turn.turn_id && event.seq > turn_started_seq =>
        {
            Some(event.seq)
        }
        _ => None,
    });
    let in_turn = |seq: chat_state::EventSeq| {
        seq > turn_started_seq && turn_end_seq.is_none_or(|end| seq < end)
    };

    let mut stop_occurrences = std::collections::BTreeSet::new();
    let mut pending_keep_working = 0usize;
    let mut committed_continuations = 0usize;
    for event in events {
        if !in_turn(event.seq) {
            continue;
        }
        match &event.kind {
            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                occurrence_id,
                event,
                cause,
                gate: chat_state::HookGateKind::Stop,
                ..
            }) if (matches!(event, chat_state::HookEventType::Stop)
                && matches!(cause, chat_state::HookCause::Turn { turn: id } if *id == turn.turn_id))
                || (matches!(event, chat_state::HookEventType::SubagentStop)
                    && matches!(cause, chat_state::HookCause::Subagent { subagent_id } if subagent_id == session_id)) =>
            {
                stop_occurrences.insert(occurrence_id.as_str());
            }
            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                occurrence_id,
                decision:
                    chat_state::HookAggregateDecision::Stop {
                        decision: chat_state::HookStopDecision::KeepWorking { .. },
                    },
            }) if stop_occurrences.contains(occurrence_id.as_str()) => {
                pending_keep_working += 1;
            }
            chat_state::TimelineEventKind::Step(chat_state::StepEvent::Started { id })
                if id.turn == turn.turn_id =>
            {
                committed_continuations += pending_keep_working;
                pending_keep_working = 0;
            }
            _ => {}
        }
    }

    let committed_continuations =
        u32::try_from(committed_continuations).map_err(|_| "Stop continuation count overflow")?;
    turn.stop_continuations_used = turn.stop_continuations_used.max(committed_continuations);
    Ok(())
}

#[cfg(test)]
mod cursor_alignment_tests {
    use super::{SessionActor, align_suspension_cursor, reuse_or_capture_command_settlements};
    use chat_state::session_runtime::{
        AcceptedResponseContinuation, CommandSettlementStatus, RuntimeCommandSettlement,
        TurnContinuation, TurnResumePoint,
    };
    use chat_state::{
        EventSeq, HookAggregateDecision, HookCause, HookEvent, HookEventType, HookGateKind,
        HookStopDecision, StepId, TimelineEvent, TimelineEventKind, TurnEvent, TurnId,
        TurnIdentity, TurnInputKind, TurnTerminal, TurnTerminalSource,
    };

    const SESSION_ID: &str = "subagent-session";

    #[tokio::test]
    async fn command_settlement_retry_reuses_evidence_after_terminal_eviction() {
        let settled = vec![RuntimeCommandSettlement {
            command_id: "cmd-1".into(),
            call_id: "call-1".into(),
            outcome: CommandSettlementStatus::Interrupted,
            output_file: "/tmp/cmd-1.log".into(),
            exit_code: None,
            signal: Some("SIGKILL".into()),
            output: "preserved stdout".into(),
            is_backgrounded: true,
            owner: chat_state::NotificationOwner::Session,
            task_kind: chat_state::NotificationTaskKind::Task,
        }];
        let replayed = reuse_or_capture_command_settlements(Some(settled.clone()), || async {
            Err("terminal snapshot expired".into())
        })
        .await
        .expect("retry must not depend on the expired terminal snapshot");
        assert_eq!(replayed, settled);
    }

    #[test]
    fn restored_child_tool_result_keeps_only_the_canonical_bounded_summary() {
        let body = serde_json::json!({ "subagent_id": "child-1", "status": "completed" });
        let unchanged = SessionActor::add_restored_planning_summary(body.clone(), None)
            .expect("summary-free child remains valid");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&unchanged).unwrap(),
            body
        );

        let summary = tool_types::planning::SubagentPlanningSummary {
            planning_seq: 17,
            scope_id: "work:child".into(),
            revision: 3,
            reviews: Vec::new(),
            review_count: 0,
            reviews_truncated: 0,
            unresolved_required_deliveries: Vec::new(),
            unresolved_required_delivery_count: 0,
            unresolved_required_deliveries_truncated: 0,
        };
        let restored = SessionActor::add_restored_planning_summary(body, Some(summary.clone()))
            .expect("canonical summary must be serializable");
        let restored: serde_json::Value = serde_json::from_str(&restored).unwrap();
        assert_eq!(
            serde_json::from_value::<tool_types::planning::SubagentPlanningSummary>(
                restored["planning_summary"].clone()
            )
            .unwrap(),
            summary
        );
    }

    fn event(seq: u64, kind: TimelineEventKind) -> TimelineEvent {
        TimelineEvent {
            version: chat_state::TIMELINE_SCHEMA_VERSION,
            seq: EventSeq::new(seq),
            at_ms: 0,
            kind,
        }
    }

    fn turn_started(seq: u64, turn: TurnId) -> TimelineEvent {
        event(
            seq,
            TimelineEventKind::Turn(TurnEvent::Started {
                prompt_id: "fixture-prompt".into(),
                id: turn,
                input_ids: vec![],
                identity: TurnIdentity {
                    origin: "test".into(),
                    turn_kind: "user".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "test".into(),
                input_message_count: 0,
                prompt_index: 0,
                prompt_text: "test".into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }),
        )
    }

    fn turn_ended(seq: u64, turn: TurnId) -> TimelineEvent {
        event(
            seq,
            TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "completed".into(),
                duration_ms: 0,
                tool_count: 0,
                terminal: TurnTerminal {
                    source: TurnTerminalSource::Host,
                    stop_reason: "stop".into(),
                    completion_kind: "test".into(),
                },
                cancellation_category: None,
                details: None,
            }),
        )
    }

    fn step_started(seq: u64, turn: TurnId) -> TimelineEvent {
        event(
            seq,
            TimelineEventKind::Step(chat_state::StepEvent::Started {
                id: StepId { turn, index: 1 },
            }),
        )
    }

    fn stop_triggered(
        seq: u64,
        occurrence: &str,
        event_type: HookEventType,
        cause: HookCause,
    ) -> TimelineEvent {
        event(
            seq,
            TimelineEventKind::Hook(HookEvent::Triggered {
                occurrence_id: occurrence.into(),
                event: event_type,
                gate: HookGateKind::Stop,
                cause,
                config_generation: 1,
                handlers: Vec::new(),
            }),
        )
    }

    fn keep_working_completed(seq: u64, occurrence: &str) -> TimelineEvent {
        event(
            seq,
            TimelineEventKind::Hook(HookEvent::Completed {
                occurrence_id: occurrence.into(),
                decision: HookAggregateDecision::Stop {
                    decision: HookStopDecision::KeepWorking {
                        reasons: Vec::new(),
                        additional_context: Vec::new(),
                    },
                },
            }),
        )
    }

    fn cursor(turn_id: TurnId, accepted_response: bool, saved_count: u32) -> TurnContinuation {
        TurnContinuation {
            turn_id,
            prompt_id: "prompt".into(),
            prompt_index: 0,
            origin: "user".into(),
            next_step_index: 1,
            tool_turns_used: 0,
            tool_calls_used: 0,
            stop_continuations_used: saved_count,
            output_schema: None,
            resume_point: TurnResumePoint::Model,
            task_output_usage: None,
            accepted_response: accepted_response.then(|| AcceptedResponseContinuation {
                identity: chat_state::ResponseAdmissionIdentity {
                    request_id: "request".into(),
                    attempt: 1,
                },
                stop_reason: None,
                stop_message: None,
            }),
            structured_output_retries: 0,
            sampling_attempts_used: 0,
            sampling_max_attempts: None,
            sampling_replay_safe: true,
            sampling_deadline_ms: None,
            operations: Vec::new(),
        }
    }

    #[test]
    fn next_step_ack_gap_consumes_response_and_counts_committed_stop() {
        let turn_id = TurnId(1);
        let mut cursor = cursor(turn_id, true, 1);
        let events = vec![
            turn_started(1, turn_id),
            event(
                2,
                TimelineEventKind::Hook(HookEvent::Triggered {
                    occurrence_id: "stop".into(),
                    event: HookEventType::Stop,
                    gate: HookGateKind::Stop,
                    cause: HookCause::Turn { turn: turn_id },
                    config_generation: 1,
                    handlers: Vec::new(),
                }),
            ),
            keep_working_completed(3, "stop"),
            stop_triggered(
                4,
                "stop-2",
                HookEventType::Stop,
                HookCause::Turn { turn: turn_id },
            ),
            keep_working_completed(5, "stop-2"),
            step_started(6, turn_id),
        ];

        align_suspension_cursor(&mut cursor, &events, Some(EventSeq::new(1)), SESSION_ID)
            .expect("timeline count fits");

        assert!(cursor.accepted_response.is_none());
        assert_eq!(cursor.stop_continuations_used, 2);
    }

    #[test]
    fn accepted_response_stays_frozen_when_no_next_step_is_committed() {
        let turn_id = TurnId(1);
        let mut cursor = cursor(turn_id, true, 0);
        let events = vec![turn_started(1, turn_id)];

        align_suspension_cursor(&mut cursor, &events, Some(EventSeq::new(2)), SESSION_ID)
            .expect("timeline count fits");

        assert!(cursor.accepted_response.is_some());
        assert_eq!(cursor.stop_continuations_used, 0);
    }

    #[test]
    fn another_turns_stop_cannot_consume_or_count_for_this_cursor() {
        let turn_id = TurnId(1);
        let other_turn = TurnId(2);
        let mut cursor = cursor(turn_id, true, 0);
        let events = vec![
            turn_started(1, turn_id),
            turn_ended(2, turn_id),
            turn_started(3, other_turn),
            stop_triggered(
                4,
                "other-stop",
                HookEventType::Stop,
                HookCause::Turn { turn: other_turn },
            ),
            keep_working_completed(5, "other-stop"),
            step_started(6, other_turn),
        ];

        align_suspension_cursor(&mut cursor, &events, Some(EventSeq::new(1)), SESSION_ID)
            .expect("timeline count fits");

        assert!(cursor.accepted_response.is_some());
        assert_eq!(cursor.stop_continuations_used, 0);
    }

    #[test]
    fn subagent_stop_counts_only_inside_the_cursor_turn_boundary() {
        let turn_id = TurnId(1);
        let mut cursor = cursor(turn_id, false, 0);
        let events = vec![
            turn_started(1, turn_id),
            stop_triggered(
                2,
                "subagent-stop",
                HookEventType::SubagentStop,
                HookCause::Subagent {
                    subagent_id: SESSION_ID.into(),
                },
            ),
            keep_working_completed(3, "subagent-stop"),
            step_started(4, turn_id),
        ];

        align_suspension_cursor(&mut cursor, &events, None, SESSION_ID)
            .expect("timeline count fits");

        assert_eq!(cursor.stop_continuations_used, 1);
    }
}

fn command_suspension_result(command: &RuntimeCommandSettlement) -> Result<String, String> {
    serde_json::to_string(&json!({
        "status": if command.outcome == CommandSettlementStatus::Interrupted { "interrupted" } else { "completed" },
        "command_id": command.command_id, "exit_code": command.exit_code,
        "signal": command.signal, "output": command.output,
        "output_file": command.output_file,
        "note": if command.outcome == CommandSettlementStatus::Interrupted {
            "The command was stopped for session suspension and may have made partial changes. Inspect its effects before choosing a new command. This call will not be replayed automatically."
        } else { "The original command already completed; this is its existing result." }
    })).map_err(|error| error.to_string())
}

async fn reuse_or_capture_command_settlements<F, Fut>(
    existing: Option<Vec<RuntimeCommandSettlement>>,
    capture: F,
) -> Result<Vec<RuntimeCommandSettlement>, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Vec<RuntimeCommandSettlement>, String>>,
{
    if let Some(existing) = existing {
        // Durable command evidence is authoritative across retry. In
        // particular, don't ask Terminal to recreate snapshots that may
        // already have passed its completed-task retention window.
        Ok(existing)
    } else {
        capture().await
    }
}

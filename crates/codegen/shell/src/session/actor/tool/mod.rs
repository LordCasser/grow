//! Tool-call execution concern for `SessionActor`: the model-output →
//! tool-execution pipeline (`execute_tool_calls`, `prepare_tool_call`,
//! tool-call start/success/error notifications, and sampling-event handling).
//!
//! Child module of the session actor so this `impl SessionActor` block retains
//! access to the actor's private fields and the parent module's helpers.
use super::*;
mod authorization;
mod dispatch;
mod parse;
mod preparation;
mod result;
use crate::extensions::notification::SessionUpdate as GrowSessionUpdate;
use crate::tools::tool_context::BlockingWaitGuard;
use authorization::*;
use dispatch::*;
pub(in crate::session::actor) use dispatch::{
    HTTP_STATUS_DETAILS_KEY, MAX_ARGS_IN_ERROR, build_tool_parse_error_message, lock_path_for_args,
    resolve_session_shell, should_show_resolved_model,
};
use futures::StreamExt;
use parse::*;
use preparation::*;
pub(in crate::session::actor) use result::wait_for_pending_interjection;
use result::*;
use tracing::Instrument;

fn retain_batch_terminal_result(slot: &mut Option<ToolLoop>, candidate: ToolLoop) {
    if slot.is_none()
        && matches!(
            &candidate,
            ToolLoop::Control(_)
                | ToolLoop::PermissionReject { .. }
                | ToolLoop::Cancelled
                | ToolLoop::PermissionTimedOut { .. }
                | ToolLoop::FollowupMessage(_)
        )
    {
        *slot = Some(candidate);
    }
}

impl SessionActor {
    #[tracing::instrument(
        name = "tools.execute",
        skip_all,
        fields(
            tool_count = tool_calls.len(),
            model_id,
            session_id = %self.session_info.id.0
        )
    )]
    pub(super) async fn execute_tool_calls(
        &self,
        tool_calls: Vec<crate::sampling::types::ToolCallResponse>,
    ) -> Result<ToolLoop, acp::Error> {
        if tool_calls
            .iter()
            .any(|call| call.id.trim().is_empty() || call.function.name.trim().is_empty())
        {
            return Err(acp::Error::invalid_params()
                .data("tool dispatch requires a durably admitted, nonempty call id and name"));
        }
        tracing::Span::current().record("model_id", self.current_catalog_model_id());
        let mut final_result: Option<ToolLoop> = None;
        let mut deferred_followups: Vec<ConversationItem> = Vec::new();
        if tool_calls.len() > 1 {
            let isolates_batch_preflight = |name: &str| {
                self.agent
                    .borrow()
                    .tool_bridge()
                    .isolates_batch_preflight(name)
            };
            let mut remaining = tool_calls;
            loop {
                let (control, siblings) =
                    split_control_preflight_barrier(remaining, isolates_batch_preflight);
                if let Some(control) = control {
                    self.execute_tool_calls_batch(
                        vec![control],
                        &mut deferred_followups,
                        &mut final_result,
                    )
                    .await?;
                    if final_result.is_some() {
                        if !siblings.is_empty() {
                            self.execute_tool_calls_batch(
                                siblings,
                                &mut deferred_followups,
                                &mut final_result,
                            )
                            .await?;
                        }
                        break;
                    }
                    if siblings.is_empty() {
                        break;
                    }
                    // An invalid or failed control leaves authority unchanged.
                    // Re-run the barrier selection over the remaining calls so
                    // a later control still precedes every ordinary sibling.
                    remaining = siblings;
                } else {
                    self.execute_tool_calls_batch(
                        siblings,
                        &mut deferred_followups,
                        &mut final_result,
                    )
                    .await?;
                    break;
                }
            }
        } else {
            self.execute_tool_calls_batch(tool_calls, &mut deferred_followups, &mut final_result)
                .await?;
        }
        {
            let _span = if !deferred_followups.is_empty() {
                Some(
                    tracing::info_span!(
                        "tools.deferred_followups",
                        count = deferred_followups.len()
                    )
                    .entered(),
                )
            } else {
                None
            };
            for chat in deferred_followups {
                self.chat_state_handle.push_user_message(chat);
            }
        }
        self.drain_pending_interjections().await;
        self.drain_deferred_completions().await;
        self.flush_pending_system_reminders().await;
        if let Some(final_result) = final_result {
            return Ok(final_result);
        }
        Ok(ToolLoop::Continue)
    }
    /// Prepare → dispatch → post-flight. Caller owns the outer tail flush.
    async fn execute_tool_calls_batch(
        &self,
        tool_calls: Vec<crate::sampling::types::ToolCallResponse>,
        deferred_followups: &mut Vec<ConversationItem>,
        final_result: &mut Option<ToolLoop>,
    ) -> Result<(), acp::Error> {
        let mut approved: Vec<PreparedToolCall> = Vec::new();
        for call in tool_calls.into_iter() {
            let frozen_input = serde_json::from_str::<serde_json::Value>(
                crate::session::helpers::tool_input_parsing::normalize_empty_arguments(
                    &call.function.arguments,
                ),
            )
            .unwrap_or_else(
                |_| serde_json::json!({ "raw_arguments": call.function.arguments.clone() }),
            );
            if let Err(error) = self
                .events
                .tool_started(
                    call.function.name.clone(),
                    call.id.clone(),
                    Some(frozen_input),
                )
                .await
            {
                self.events.cancel_active_tool();
                return Err(acp::Error::internal_error()
                    .data(format!("tool call was not durably recorded: {error}")));
            }
            if final_result.is_some() {
                let message = match &*final_result {
                    Some(ToolLoop::PermissionReject { .. }) => {
                        format!(
                            "Tool execution cancelled due to earlier permission rejection for tool `{}`",
                            call.function.name
                        )
                    }
                    Some(ToolLoop::Cancelled) => {
                        format!(
                            "Tool execution cancelled due to earlier user cancellation for tool `{}`",
                            call.function.name
                        )
                    }
                    Some(ToolLoop::PermissionTimedOut { .. }) => {
                        format!(
                            "Tool execution cancelled due to an earlier permission timeout for tool `{}`",
                            call.function.name
                        )
                    }
                    Some(ToolLoop::FollowupMessage(_)) => {
                        format!(
                            "Tool execution cancelled due to earlier user followup message for tool `{}`",
                            call.function.name
                        )
                    }
                    _ => {
                        format!("Tool execution cancelled for tool `{}`", call.function.name)
                    }
                };
                self.chat_state_handle
                    .push_tool_result(ConversationItem::tool_result(call.id.clone(), message));
                self.events
                    .tool_completed_durably(
                        &call.id,
                        "cancelled".into(),
                        Some(serde_json::json!({
                            "dispatched": false,
                            "stage": "batch_cancelled",
                        })),
                    )
                    .await
                    .map_err(|error| {
                        acp::Error::internal_error().data(format!(
                            "cancelled tool call was not durably closed: {error}"
                        ))
                    })?;
                continue;
            }
            let call_id = call.id.clone();
            let call_name = call.function.name.clone();
            let tool_call_id = acp::ToolCallId::new(Arc::from(call_id.clone()));
            let prepared = match self.prepare_tool_call(call, deferred_followups).await {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.handle_tool_not_executed(
                        &call_id,
                        &tool_call_id,
                        format!(
                            "Tool `{call_name}` could not be admitted because the session runtime failed."
                        ),
                    )
                    .await?;
                    self.events
                        .tool_completed_durably(
                            &call_id,
                            "error".into(),
                            Some(serde_json::json!({
                                "dispatched": false,
                                "stage": "preflight",
                            })),
                        )
                        .await
                        .map_err(|timeline_error| {
                            acp::Error::internal_error().data(format!(
                                "failed tool admission was not durably closed: {timeline_error}"
                            ))
                        })?;
                    return Err(error);
                }
            };
            match prepared {
                ToolPreflight::Dispatch(prepared) => approved.push(prepared),
                ToolPreflight::Resolved {
                    loop_result,
                    post_terminal_hook,
                } => {
                    self.events
                        .tool_completed_durably(
                            &call_id,
                            undispatched_tool_outcome(&loop_result).into(),
                            Some(serde_json::json!({
                                "dispatched": false,
                                "stage": "preflight",
                            })),
                        )
                        .await
                        .map_err(|error| {
                            acp::Error::internal_error().data(format!(
                                "undispatched tool call was not durably closed: {error}"
                            ))
                        })?;
                    if let Some(hook) = post_terminal_hook {
                        self.dispatch_observe_hook(
                            hook.event,
                            hook.cause,
                            hook.payload,
                            hook.prompt_id,
                        )
                        .await
                        .map_err(|error| {
                            acp::Error::internal_error().data(format!(
                                "post-terminal hook lifecycle was not durable: {error}"
                            ))
                        })?;
                    }
                    retain_batch_terminal_result(final_result, loop_result);
                }
            }
        }
        if final_result.is_some() && !approved.is_empty() {
            let reason = match final_result.as_ref() {
                Some(ToolLoop::Control(_)) => {
                    "Tool execution cancelled because an earlier control changed session state"
                }
                Some(ToolLoop::Cancelled) => {
                    "Tool execution cancelled before batch dispatch by the user"
                }
                Some(ToolLoop::PermissionReject { .. }) => {
                    "Tool execution cancelled before batch dispatch after permission rejection"
                }
                Some(ToolLoop::PermissionTimedOut { .. }) => {
                    "Tool execution cancelled before batch dispatch after permission timeout"
                }
                Some(ToolLoop::FollowupMessage(_)) => {
                    "Tool execution cancelled before batch dispatch by the user's follow-up"
                }
                _ => "Tool execution cancelled before batch dispatch",
            };
            for prepared in approved.drain(..) {
                self.handle_tool_not_executed(
                    &prepared.call_id,
                    &prepared.tool_call_id,
                    format!("{reason}: `{}` was not executed", prepared.tool_name),
                )
                .await?;
                self.events
                    .tool_completed_durably(
                        &prepared.call_id,
                        "cancelled".into(),
                        Some(serde_json::json!({
                            "dispatched": false,
                            "stage": "batch_cancelled",
                        })),
                    )
                    .await
                    .map_err(|error| {
                        acp::Error::internal_error().data(format!(
                            "cancelled tool call was not durably closed: {error}"
                        ))
                    })?;
            }
            return Ok(());
        }
        let write_paths: std::collections::HashSet<String> = approved
            .iter()
            .filter(|prepared| prepared.required_access.requires_write())
            .filter_map(|prepared| lock_path_for_args(&prepared.parsed_args).map(str::to_owned))
            .collect();
        let file_locks = {
            let mut map: std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>> =
                std::collections::HashMap::new();
            for prepared in &approved {
                if let Some(fp) = lock_path_for_args(&prepared.parsed_args)
                    && write_paths.contains(fp)
                {
                    map.entry(fp.to_owned())
                        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())));
                }
            }
            map
        };
        let dispatch_authority = ToolDispatchAuthority {
            bridge: self.agent.borrow().tool_bridge().clone(),
            subagent: self.subagent_capabilities.clone(),
            mcp_state: Arc::clone(&self.mcp_state),
            cwd: self.tool_context.cwd.as_path().to_path_buf(),
            actor_source: if self.startup_hints.is_subagent {
                format!("child:{}", self.session_info.id.0)
            } else {
                format!("primary:{}", self.turn_behavior.lock().as_id())
            },
        };
        let workflow_manager = self.workflow_manager.clone();
        let behavior = self.behavior.clone();
        let pending_interjections = self.pending_interjections.clone();
        let completion_delivery = self.completion_delivery.clone();
        let goal_active = self.goal_loop_active();
        let wait_owner_turn = self
            .current_prompt_id
            .lock()
            .expect("current_prompt_id mutex poisoned")
            .clone();
        let session_id: Arc<str> = Arc::from(&*self.session_info.id.0);
        let dispatch_futures: Vec<_> = approved
            .iter()
            .enumerate()
            .map(|(idx, prepared)| {
                let prepared = Arc::new(prepared.clone());
                let dispatch_authority = dispatch_authority.clone();
                let workflow_manager = workflow_manager.clone();
                let behavior = behavior.clone();
                let session_id = session_id.clone();
                let pending_interjections = pending_interjections.clone();
                let completion_delivery = completion_delivery.clone();
                let wait_owner_turn = wait_owner_turn.clone();
                let blocking_wait_depth = self.tool_context.blocking_wait_depth.clone();
                let interruptible =
                    is_interruptible_wait_tool(&prepared.tool_name, &prepared.parsed_args);
                let tracked_task_ids = if goal_active {
                    super::completion_delivery::wait_task_ids(&prepared.parsed_args)
                } else {
                    Default::default()
                };
                let lock = lock_path_for_args(&prepared.parsed_args)
                    .and_then(|fp| file_locks.get(fp).cloned());
                let tools_execute_span = tracing::Span::current();
                async move {
                    let exec_start = std::time::Instant::now();
                    let tool_span = tool_execution_span(
                        &tools_execute_span,
                        session_id.as_ref(),
                        &prepared,
                        &prepared.call_id,
                        false,
                    );
                    let tool_span_for_record = tool_span.clone();
                    let run_tool = || {
                        let prepared = Arc::clone(&prepared);
                        let session_id = session_id.clone();
                        let lock = lock.clone();
                        async move {
                            let _guard = if let Some(ref l) = lock {
                                Some(l.lock().await)
                            } else {
                                None
                            };
                            if prepared.workflow_draft_write {
                                let _admission = workflow_manager.lock().await;
                                if behavior.lock().behavior()
                                    != tool_types::BehaviorId::Workflow
                                {
                                    return Err(tool_runtime::ToolError::custom(
                                        "workflow_behavior_required",
                                        "Workflow draft writes require live Workflow behavior. Use /workflow [prompt].",
                                    ));
                                }
                                dispatch_tool(&dispatch_authority, &prepared, &session_id)
                                .await
                            } else {
                                dispatch_tool(&dispatch_authority, &prepared, &session_id)
                                .await
                            }
                        }
                    };
                    let result = if interruptible {
                        let _wait_guard = BlockingWaitGuard::enter(blocking_wait_depth.clone());
                        completion_delivery
                            .begin_wait(wait_owner_turn.as_deref(), &tracked_task_ids);
                        async {
                            tokio::select! {
                                biased;
                                result = run_tool() => {
                                    completion_delivery.finish_wait(&tracked_task_ids);
                                    result
                                },
                                _ = wait_for_pending_interjection(&pending_interjections) => {
                                    // Transfer ownership before dropping the
                                    // wait future. Completion sources can now
                                    // race safely with waiter teardown.
                                    completion_delivery.defer_wait(&tracked_task_ids);
                                    tracing::info!(
                                        tool = %prepared.tool_name,
                                        task_ids = ?tracked_task_ids,
                                        "abort wait tool: interjection pending"
                                    );
                                    let result = if tracked_task_ids.is_empty() {
                                        interrupted_wait_tool_result_with_msg(
                                            &prepared.parsed_args,
                                            "Wait ended early because the user sent a message.",
                                        )
                                    } else {
                                        interrupted_wait_tool_result(&prepared.parsed_args)
                                    };
                                    Ok(result)
                                }
                            }
                        }
                        .instrument(tool_span)
                        .await
                    } else {
                        run_tool().instrument(tool_span).await
                    };
                    let duration_ms = exec_start.elapsed().as_millis() as u64;
                    let success = record_tool_span_outcome(tool_span_for_record, &result);
                    ::diagnostics::unified_log::info(
                        "shell.tool.exec_done",
                        Some(session_id.as_ref()),
                        Some(serde_json::json!({
                            "tool_name": prepared.tool_name.as_str(),
                            "tool_call_id": prepared.call_id.as_str(),
                            "elapsed_ms": duration_ms,
                            "success": success,
                        })),
                    );
                    (idx, result, duration_ms)
                }
            })
            .collect();
        tokio::task::yield_now().await;
        let mut dispatch_stream = futures::stream::FuturesUnordered::new();
        for fut in dispatch_futures {
            dispatch_stream.push(fut);
        }
        let mut approved_slots: Vec<Option<PreparedToolCall>> =
            approved.into_iter().map(Some).collect();
        let (dispatch_tx, mut dispatch_rx) = tokio::sync::mpsc::unbounded_channel::<(
            usize,
            Result<ToolRunResult, tool_runtime::ToolError>,
            u64,
        )>();
        let drainer = tokio::spawn(
            async move {
                while let Some(item) = dispatch_stream.next().await {
                    if dispatch_tx.send(item).is_err() {
                        break;
                    }
                }
            }
            .in_current_span(),
        );
        let _drainer_guard = crate::util::AbortOnDrop(drainer);
        while let Some((idx, mut result, mut duration_ms)) = dispatch_rx.recv().await {
            let prepared = approved_slots[idx]
                .take()
                .expect("dispatch index should match an approved slot exactly once");
            if result
                .as_ref()
                .is_ok_and(|tool_result| !tool_result.output.is_error())
                && let Some(expected) = prepared.plan_exit_on_success.as_ref()
            {
                let commit = self.finish_plan_to_default_if(expected).await;
                if !matches!(commit, Ok(true)) {
                    let message = match commit {
                        Ok(false) => "Plan changed before the completed control could be committed"
                            .to_owned(),
                        Err(message) => message,
                        Ok(true) => unreachable!(),
                    };
                    result = Err(tool_runtime::ToolError::custom(
                        "plan_control_commit_failed",
                        message,
                    ));
                }
            }
            self.signals_handle().record_tool_call(&prepared.tool_name);
            let tool_call_id = prepared.call_id.clone();
            let mut post_tool_use_result: Option<serde_json::Value> = None;
            let mut post_tool_use_failure: Option<String> = None;
            let tool_result_size_bytes = match &result {
                Ok(tool_result) => tool_result.prompt_text.len() as i64,
                Err(_) => 0,
            };
            let tool_failed = match &result {
                Ok(tool_result) => tool_result.output.is_error(),
                Err(_) => true,
            };
            let tool_loop = match result {
                Ok(tool_result) => {
                    let effective_tool_name = tool_result
                        .effective_tool_name
                        .clone()
                        .or_else(|| prepared.dispatch_target_name.clone())
                        .unwrap_or_else(|| prepared.tool_name.clone());
                    if tool_result.output.is_error() {
                        post_tool_use_failure = Some(tool_result.prompt_text.clone());
                    } else {
                        post_tool_use_result = Some(
                            serde_json::to_value(&tool_result.output)
                                .unwrap_or(serde_json::Value::Null),
                        );
                    }
                    let followups = self
                        .handle_bridge_tool_success(
                            &prepared.tool_call_id,
                            &prepared.call_id,
                            &prepared.tool_name,
                            &effective_tool_name,
                            tool_result,
                            prepared.concatenated_json_count,
                            &prepared.model_id,
                            &prepared.parsed_args,
                        )
                        .await?;
                    deferred_followups.extend(followups);
                    if prepared.tool_name == "search_tool" {
                        let pi = self.chat_state_handle.get_prompt_index().await as i64;
                        self.last_search_prompt_index
                            .store(pi, std::sync::atomic::Ordering::Relaxed);
                    }
                    // A successful PlanControl is a state transition just like
                    // a GoalLifecycle update.  The remaining calls in this
                    // provider batch were sampled against the pre-transition
                    // state and must be durably cancelled before the next
                    // sample.  Failed/invalid controls stay Continue so an
                    // otherwise valid sibling can still run.
                    if !tool_failed && let Some(disposition) = prepared.success_control {
                        ToolLoop::Control(disposition)
                    } else {
                        ToolLoop::Continue
                    }
                }
                Err(err) => {
                    let consumed_completion_id =
                        consumed_completion_id_from_tool_error(&err).map(str::to_owned);
                    let err: anyhow::Error = err.into();
                    let err_followups = self
                        .handle_tool_error(
                            &prepared.tool_call_id,
                            &prepared.call_id,
                            &prepared.tool_name,
                            prepared.dispatch_target_name.as_deref(),
                            &err,
                            &prepared.model_id,
                        )
                        .await;
                    deferred_followups.extend(err_followups);
                    if let Some(consumed_completion_id) = consumed_completion_id.as_deref() {
                        let consumed_ids = [consumed_completion_id];
                        self.completion_delivery.consume(&consumed_ids);
                        self.drop_pending_items_for_consumed_completions(&consumed_ids)
                            .await;
                        self.acknowledge_consumed_notifications(&consumed_ids).await;
                    }
                    post_tool_use_failure = Some(format!("{err:#}"));
                    ToolLoop::Continue
                }
            };
            let tool_outcome = match &tool_loop {
                _ if tool_failed => crate::session::events::ToolOutcome::Error,
                ToolLoop::Continue | ToolLoop::Control(_) => {
                    crate::session::events::ToolOutcome::Success
                }
                ToolLoop::PermissionReject { .. } => {
                    crate::session::events::ToolOutcome::PermissionRejected
                }
                ToolLoop::Cancelled => crate::session::events::ToolOutcome::PermissionCancelled,
                ToolLoop::PermissionTimedOut { .. } => {
                    crate::session::events::ToolOutcome::PermissionTimedOut
                }
                ToolLoop::FollowupMessage(_) => crate::session::events::ToolOutcome::Followup,
                ToolLoop::HookDenied { .. } => crate::session::events::ToolOutcome::HookDenied,
                ToolLoop::NonExistingTool | ToolLoop::ToolParsingError => {
                    crate::session::events::ToolOutcome::InvalidTool
                }
            };
            // The external tool result is the source fact for PostToolUse. It
            // must be durable before the hook occurrence can be triggered.
            self.events
                .tool_completed_durably(
                    &tool_call_id,
                    <&'static str>::from(tool_outcome).to_owned(),
                    None,
                )
                .await
                .map_err(|error| {
                    acp::Error::internal_error()
                        .data(format!("tool completion was not durably recorded: {error}"))
                })?;
            {
                let bridge = self.agent.borrow().tool_bridge().clone();
                if let Some(effects) = bridge.apply_pending_skill_update().await {
                    if let Some(item) = self.wrap_skill_reminder(&effects) {
                        deferred_followups.push(item);
                    }
                    if effects.send_available_commands {
                        self.send_available_commands_update().await;
                    }
                }
            }
            if let Some(error) = post_tool_use_failure {
                let raw_input: serde_json::Value = serde_json::from_str(&prepared.raw_arguments)
                    .unwrap_or(serde_json::Value::Null);
                let (tool_input_value, tool_input_truncated) =
                    ::hooks::event::truncate_payload(raw_input);
                let hook_tool_name = prepared.hook_tool_name();
                self.dispatch_observe_hook(
                    ::hooks::event::HookEventName::PostToolUseFailure,
                    chat_state::HookCause::Tool {
                        call_id: prepared.call_id.clone(),
                    },
                    ::hooks::event::HookPayload::PostToolUseFailure {
                        tool_name: hook_tool_name.to_owned(),
                        tool_use_id: prepared.call_id.clone(),
                        tool_input: tool_input_value,
                        tool_input_truncated,
                        error,
                        subagent_type: self.subagent_type_label(),
                    },
                    None,
                )
                .await
                .map_err(|error| {
                    acp::Error::internal_error().data(format!(
                        "post-tool-failure hook lifecycle was not durable: {error}"
                    ))
                })?;
            } else if let Some(tool_result_value) = post_tool_use_result {
                let raw_input: serde_json::Value = serde_json::from_str(&prepared.raw_arguments)
                    .unwrap_or(serde_json::Value::Null);
                let (tool_input_value, tool_input_truncated) =
                    ::hooks::event::truncate_payload(raw_input);
                let (tool_result_val, tool_result_truncated) =
                    ::hooks::event::truncate_payload(tool_result_value);
                let hook_tool_name = prepared.hook_tool_name();
                self.dispatch_observe_hook(
                    ::hooks::event::HookEventName::PostToolUse,
                    chat_state::HookCause::Tool {
                        call_id: prepared.call_id.clone(),
                    },
                    ::hooks::event::HookPayload::PostToolUse {
                        tool_name: hook_tool_name.to_owned(),
                        tool_use_id: prepared.call_id.clone(),
                        tool_input: tool_input_value,
                        tool_result: tool_result_val,
                        tool_input_truncated,
                        tool_result_truncated,
                        duration_ms: None,
                        is_backgrounded: false,
                        subagent_type: self.subagent_type_label(),
                    },
                    None,
                )
                .await
                .map_err(|error| {
                    acp::Error::internal_error()
                        .data(format!("post-tool hook lifecycle was not durable: {error}"))
                })?;
            }
            self.signals_handle().record_tool_duration(
                &prepared.tool_name,
                &tool_call_id,
                duration_ms,
            );
            ::diagnostics::session_ctx::log_event(::diagnostics::events::ToolCallCompleted {
                tool_name: prepared.tool_name.clone(),
                outcome: tool_outcome.into(),
                duration_ms,
            });
            tracing::info_span!(
                "tool.execution",
                tool_name = %prepared.tool_name,
                tool_use_id = %prepared.call_id,
                tool_input_size_bytes = prepared.raw_arguments.len() as i64,
                tool_result_size_bytes = tool_result_size_bytes,
                success = matches!(tool_outcome, crate::session::events::ToolOutcome::Success),
                outcome = <&'static str>::from(tool_outcome),
            )
            .in_scope(|| {});
            retain_batch_terminal_result(final_result, tool_loop);
        }
        Ok(())
    }
    /// Issue the `grow/plan_approval` reverse request and await the user's
    /// decision. Shared by the PlanControl intercept and the resume
    /// re-park. Marks approval transport as pending while the request is
    /// outstanding; the decision branches clear it only as part of their
    /// phase-transition CAS.
    pub(super) async fn request_plan_approval(
        &self,
        tool_call_id: &acp::ToolCallId,
        plan_content: String,
    ) -> Result<tools::implementations::grow_build::plan_control::PlanApprovalExtResponse, acp::Error>
    {
        use acp_transport::AcpClientHandler as _;
        use tools::implementations::grow_build::plan_control::{
            PlanApprovalExtRequest, PlanApprovalExtResponse,
        };
        let ext_req = PlanApprovalExtRequest {
            session_id: self.session_id_string(),
            tool_call_id: tool_call_id.to_string(),
            plan_content,
        };
        debug_assert!(
            !ext_req.session_id.is_empty(),
            "Plan approval request must carry a non-empty sessionId"
        );
        let ext_request = acp::ExtRequest::new(
            "grow/plan_approval",
            serde_json::value::to_raw_value(&ext_req)
                .expect("PlanApprovalExtRequest serialization should not fail")
                .into(),
        );
        self.dispatch_notification_hook(
            "permission_prompt",
            Some("Plan approval requested".into()),
            None,
            Some("info".into()),
        )
        .await
        .map_err(|error| {
            acp::Error::internal_error().data(format!(
                "plan approval notification hook lifecycle was not durable: {error}"
            ))
        })?;
        debug_assert!(self.behavior.lock().approval_pending());
        let resp = {
            let _pending_guard = crate::session::pending_interaction::PendingInteractionGuard::new(
                self.pending_interactions.clone(),
                self.notifications.gateway.clone(),
                self.session_info.id.clone(),
                tool_call_id.to_string(),
                crate::session::pending_interaction::PendingKind::PlanApproval,
            );
            self.notifications.gateway.ext_method(ext_request).await
        };
        let raw = match resp {
            Ok(raw) => raw,
            Err(err) => {
                return Err(err);
            }
        };
        serde_json::from_str::<PlanApprovalExtResponse>(raw.0.get()).map_err(|error| {
            acp::Error::invalid_params()
                .data(format!("invalid grow/plan_approval response: {error}"))
        })
    }
    /// Leave Plan and tell the client to show Normal.
    async fn finish_plan_to_default(&self) -> Result<(), String> {
        let previous_behavior = self.behavior.lock().snapshot();
        let deactivated = self.behavior.lock().finish_plan();
        if deactivated {
            self.commit_behavior_mutation_or_restore(previous_behavior)
                .await?;
            self.enqueue_current_mode_update(acp::SessionModeId::new(
                tools::types::BehaviorId::Normal.as_id(),
            ));
            self.send_available_commands_update().await;
        }
        Ok(())
    }

    /// Finish Plan only if the pending approval still describes the exact
    /// state captured before the reverse request. A stale abandon decision
    /// must not terminate a newer Plan.
    async fn finish_plan_to_default_if(
        &self,
        expected: &crate::session::behavior::BehaviorSnapshot,
    ) -> Result<bool, String> {
        let previous_behavior = self.behavior.lock().snapshot();
        let finished = {
            let mut behavior = self.behavior.lock();
            if behavior.snapshot() != *expected {
                false
            } else {
                behavior.finish_plan()
            }
        };
        if !finished {
            return Ok(false);
        }
        self.commit_behavior_mutation_or_restore(previous_behavior)
            .await?;
        if self.behavior.lock().snapshot() != crate::session::behavior::BehaviorSnapshot::normal() {
            return Ok(false);
        }
        self.enqueue_current_mode_update(acp::SessionModeId::new(
            tools::types::BehaviorId::Normal.as_id(),
        ));
        self.send_available_commands_update().await;
        Ok(true)
    }

    /// Validate the parked approval before load-session is acknowledged. A
    /// submitted Plan without its immutable artifact cannot remain in
    /// AwaitingApproval; normalize it to Normal through a durable Control
    /// barrier so the restored actor is always able to accept another turn.
    pub(super) async fn reconcile_restored_plan_approval(&self) -> Result<bool, String> {
        let snapshot = self.behavior.lock().snapshot();
        if !snapshot.approval_pending {
            return Ok(false);
        }
        if self
            .behavior
            .lock()
            .pending_plan_approval_snapshot()
            .is_none()
        {
            // A persisted transport bit without an approval-capable phase is
            // stale state. Clear only the bit while preserving the current
            // Plan phase, then durably publish that normalization.
            if !self.behavior.lock().clear_approval_pending_if(&snapshot) {
                return Ok(false);
            }
            self.commit_behavior_mutation_or_restore(snapshot).await?;
            return Ok(false);
        }
        let artifact_hash = self.behavior.lock().plan_artifact_hash().map(str::to_owned);
        let artifact_valid = match artifact_hash {
            Some(hash) => read_plan_artifact_async(self.session_directory.clone(), hash)
                .await
                .is_ok_and(|content| !content.trim().is_empty()),
            None => false,
        };
        if artifact_valid {
            return Ok(true);
        }
        tracing::warn!(
            "plan_control restore: submitted artifact is unavailable; normalizing Plan to Normal"
        );
        let _ = self.finish_plan_to_default_if(&snapshot).await?;
        Ok(false)
    }

    pub(super) async fn reconcile_restored_plan_handoff_notification(&self) -> Result<(), String> {
        let snapshot = self.behavior.lock().snapshot();
        self.admit_plan_handoff_notification(&snapshot).await
    }

    pub(super) async fn admit_plan_handoff_notification(
        &self,
        snapshot: &crate::session::behavior::BehaviorSnapshot,
    ) -> Result<(), String> {
        let Some(handoff) = snapshot.last_plan_handoff.as_ref() else {
            return Ok(());
        };
        let source = chat_state::NotificationSource::PlanHandoff {
            artifact_hash: handoff.artifact_hash.clone(),
            artifact_revision: handoff.artifact_revision,
            handoff: handoff.kind,
        };
        let source_version = chat_state::NotificationSourceVersion::Ordinal {
            value: handoff.artifact_revision,
        };
        match self
            .chat_state_handle
            .received_notification_id(source.clone(), source_version.clone())
            .await
        {
            Some(Some(_)) => return Ok(()),
            Some(None) => {}
            None => return Err("Plan handoff receipt fold is unavailable".into()),
        }
        let body = match handoff.kind {
            chat_state::PlanHandoffKind::Execute => {
                "The approved Plan is now in the Executing phase. Continue from the frozen Plan contract."
            }
            chat_state::PlanHandoffKind::Revise => {
                "The Plan is now in a revision phase. Revise the frozen Plan contract before resubmitting it."
            }
        };
        self.receive_notification(source, source_version, body.into())
            .await
            .map(|_| ())
    }

    /// Resume hook: re-issue the parked Plan approval
    /// after a session restored with `approval_pending == true`, so the
    /// client re-shows approval chrome over a real live waiter. Handles the
    /// decision with no in-flight turn — approve: enter execution + start an
    /// implement turn; request-changes: stay in Plan + feed the comments
    /// back as a turn; abandon: leave plan mode and wait for the user.
    pub(super) async fn resume_plan_approval(
        self: Arc<Self>,
        completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
    ) {
        let approval_snapshot = self.behavior.lock().pending_plan_approval_snapshot();
        let Some(approval_snapshot) = approval_snapshot else {
            return;
        };
        if crate::session::pending_interaction::has_parked_plan_approval(&self.pending_interactions)
        {
            tracing::debug!("plan_control resume: approval already pending; skip re-park");
            return;
        }
        match self.reconcile_restored_plan_approval().await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::error!(%error, "plan_control resume normalization was not durable");
                return;
            }
        }
        let artifact_hash = self.behavior.lock().plan_artifact_hash().map(str::to_owned);
        let plan_content = match artifact_hash {
            Some(hash) => {
                match read_plan_artifact_async(self.session_directory.clone(), hash).await {
                    Ok(content) if !content.trim().is_empty() => content,
                    _ => {
                        tracing::info!(
                            "plan_control resume: candidate artifact disappeared after reconciliation"
                        );
                        if let Err(error) = self.finish_plan_to_default().await {
                            tracing::error!(%error, "failed to durably normalize missing Plan artifact");
                        }
                        return;
                    }
                }
            }
            _ => {
                tracing::info!(
                    "plan_control resume: candidate Plan disappeared after reconciliation"
                );
                if let Err(error) = self.finish_plan_to_default().await {
                    tracing::error!(%error, "failed to durably normalize missing Plan artifact");
                }
                return;
            }
        };
        let tool_call_id = acp::ToolCallId::new(Arc::from(
            format!("plan-approval-resume-{}", self.session_info.id.0).as_str(),
        ));
        tracing::info!(
            tool_call_id = %tool_call_id,
            "plan_control: re-parking approval after resume"
        );
        let parsed = match self
            .request_plan_approval(&tool_call_id, plan_content.clone())
            .await
        {
            Ok(parsed) => parsed,
            Err(err) => {
                tracing::debug!(%err, "resumed Plan approval request failed");
                return;
            }
        };
        let decision_feedback = parsed.feedback;
        match parsed.outcome {
            PlanApprovalOutcome::Abandoned => {
                tracing::info!("plan_control resume: user abandoned Plan");
                match self.finish_plan_to_default_if(&approval_snapshot).await {
                    Ok(true) => {}
                    Ok(false) => {
                        tracing::info!("plan_control resume: dropping stale abandon decision")
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to persist abandoned Plan on resume")
                    }
                }
            }
            PlanApprovalOutcome::Cancelled => {
                tracing::info!("plan_control resume: user requested changes");
                let previous_behavior = self.behavior.lock().snapshot();
                let transitioned = self.behavior.lock().reject_submitted_plan_if_with_feedback(
                    &approval_snapshot,
                    decision_feedback.clone(),
                );
                if !transitioned {
                    tracing::info!("plan_control resume: dropping stale request-changes decision");
                    return;
                }
                if self
                    .commit_behavior_mutation_or_restore(previous_behavior)
                    .await
                    .is_ok()
                {
                    let next = self.behavior.lock().snapshot();
                    if let Err(error) = self.admit_plan_handoff_notification(&next).await {
                        tracing::warn!(%error, "Plan revision handoff receipt will be reconciled after restore");
                    }
                    SessionActor::maybe_drain_notifications(self.clone(), completion_tx).await;
                }
            }
            PlanApprovalOutcome::Approved => {
                tracing::info!("plan_control resume: user approved Plan");
                let previous_behavior = self.behavior.lock().snapshot();
                let transitioned = self
                    .behavior
                    .lock()
                    .approve_submitted_plan_if_with_feedback(
                        &approval_snapshot,
                        decision_feedback.clone(),
                    );
                if !transitioned {
                    tracing::info!("plan_control resume: dropping stale approval decision");
                    return;
                }
                if self
                    .commit_behavior_mutation_or_restore(previous_behavior)
                    .await
                    .is_ok()
                {
                    let next = self.behavior.lock().snapshot();
                    if let Err(error) = self.admit_plan_handoff_notification(&next).await {
                        tracing::warn!(%error, "Plan execution handoff receipt will be reconciled after restore");
                    }
                    SessionActor::maybe_drain_notifications(self.clone(), completion_tx).await;
                }
            }
        }
    }
}
#[cfg(test)]
mod tool_call_pipeline_tests {
    use super::{
        ToolLoop, consumed_completion_id_from_tool_error, execute_tool_call_parts,
        undispatched_tool_outcome,
    };
    use std::path::Path;

    #[test]
    fn preflight_terminals_keep_their_causal_outcome() {
        assert_eq!(
            undispatched_tool_outcome(&ToolLoop::Continue),
            "not_dispatched"
        );
        assert_eq!(
            undispatched_tool_outcome(&ToolLoop::ToolParsingError),
            "invalid_tool"
        );
        assert_eq!(
            undispatched_tool_outcome(&ToolLoop::PermissionReject {
                tool_name: "bash".into(),
                reason: "denied".into(),
            }),
            "permission_rejected"
        );
        assert_eq!(
            undispatched_tool_outcome(&ToolLoop::HookDenied {
                hook_name: "guard".into(),
            }),
            "hook_denied"
        );
    }

    #[test]
    fn terminal_task_failure_carries_its_receipt_identity() {
        let error =
            tool_runtime::ToolError::new(tool_runtime::ToolErrorKind::Execution, "child failed")
                .with_details(serde_json::json!({
                    "consumed_completion_task_id": "goal-child",
                }));
        assert_eq!(
            consumed_completion_id_from_tool_error(&error),
            Some("goal-child")
        );
    }

    #[test]
    fn peels_redundant_session_cd_from_title() {
        let (title, ..) =
            execute_tool_call_parts("cd /proj && echo hi", Some("desc"), Path::new("/proj"));
        assert_eq!(title, "Execute `echo hi`");
    }
    #[test]
    fn keeps_command_when_cd_not_redundant() {
        let (title, ..) = execute_tool_call_parts("cd /other && ls", None, Path::new("/proj"));
        assert_eq!(title, "Execute `cd /other && ls`");
    }
}
#[cfg(test)]
mod rwx_projection_tests {
    use super::{hash_canonical_json, project_call_access, shell_required_access};
    use tool_protocol::ToolAccess;
    use tool_types::{KillTaskToolInput, TaskToolInput};
    use tools::implementations::BashToolInput;
    use tools::implementations::grow_build::workflow::{
        WorkflowDefinitionId, WorkflowDraftSource, WorkflowRunControl, WorkflowScope,
        WorkflowToolInput,
    };
    use tools::types::ToolInput;

    #[test]
    fn shell_projection_distinguishes_observation_mutation_and_opaque_syntax() {
        assert_eq!(
            shell_required_access("rg TODO src"),
            ToolAccess::ReadExecute
        );
        assert_eq!(
            shell_required_access("cat README.md && git status --short"),
            ToolAccess::ReadExecute
        );
        assert_eq!(shell_required_access("echo x > out"), ToolAccess::All);
        assert_eq!(shell_required_access("cargo fmt"), ToolAccess::All);
        assert_eq!(shell_required_access("git add ."), ToolAccess::All);
        assert_eq!(shell_required_access("make format"), ToolAccess::All);
        assert_eq!(
            shell_required_access("./project-tool inspect"),
            ToolAccess::All
        );
        assert_eq!(
            shell_required_access("curl https://example.com"),
            ToolAccess::All
        );
        assert_eq!(shell_required_access("echo '"), ToolAccess::All);
        assert_eq!(
            shell_required_access("bash -c \"$COMMAND\""),
            ToolAccess::All
        );
    }

    #[test]
    fn workflow_projection_narrows_the_all_descriptor_by_action() {
        let id = WorkflowDefinitionId::new("project:review");
        let draft_id = WorkflowDefinitionId::new("session:draft");
        for (input, expected) in [
            (
                WorkflowToolInput::Search {
                    query: "review".into(),
                    limit: None,
                },
                ToolAccess::Read,
            ),
            (
                WorkflowToolInput::Draft {
                    name: None,
                    source: WorkflowDraftSource::Inline {
                        script: "complete(#{})".into(),
                    },
                },
                ToolAccess::Write,
            ),
            (
                WorkflowToolInput::Draft {
                    name: None,
                    source: WorkflowDraftSource::File {
                        path: ".grow/workflows/review.rhai".into(),
                    },
                },
                ToolAccess::ReadWrite,
            ),
            (
                WorkflowToolInput::Draft {
                    name: None,
                    source: WorkflowDraftSource::Definition {
                        definition_id: id.clone(),
                    },
                },
                ToolAccess::ReadWrite,
            ),
            (
                WorkflowToolInput::Inspect {
                    definition_id: id.clone(),
                    include_source: true,
                },
                ToolAccess::ReadWrite,
            ),
            (
                WorkflowToolInput::Validate {
                    definition_id: id.clone(),
                    args: None,
                    agent_budget: None,
                },
                ToolAccess::All,
            ),
            (
                WorkflowToolInput::Run {
                    definition_id: id.clone(),
                    args: None,
                    max_concurrency: None,
                    agent_budget: None,
                },
                ToolAccess::All,
            ),
            (
                WorkflowToolInput::Publish {
                    definition_id: draft_id.clone(),
                    scope: WorkflowScope::Project,
                },
                ToolAccess::ReadWrite,
            ),
            (
                WorkflowToolInput::Discard {
                    definition_id: draft_id,
                },
                ToolAccess::Write,
            ),
            (
                WorkflowToolInput::ControlRun {
                    run_id: "review".into(),
                    operation: WorkflowRunControl::Pause,
                    agent_budget: None,
                },
                ToolAccess::WriteExecute,
            ),
            (
                WorkflowToolInput::ControlRun {
                    run_id: "review".into(),
                    operation: WorkflowRunControl::Resume,
                    agent_budget: Some(16),
                },
                ToolAccess::WriteExecute,
            ),
            (
                WorkflowToolInput::ControlRun {
                    run_id: "review".into(),
                    operation: WorkflowRunControl::Stop,
                    agent_budget: None,
                },
                ToolAccess::WriteExecute,
            ),
        ] {
            let required = project_call_access(&ToolInput::Workflow(input), ToolAccess::All, None);
            assert_eq!(required, expected);
            assert!(ToolAccess::All.covers(required));
        }
    }

    #[test]
    fn dynamic_and_mcp_calls_never_fall_back_to_read() {
        let dynamic = ToolInput::Dynamic(serde_json::json!({"query": "x"}));
        assert_eq!(
            project_call_access(&dynamic, ToolAccess::All, None),
            ToolAccess::All
        );
        let mcp = ToolInput::MCPTool(tools::types::MCPToolInput {
            tool_name: "docs__search".into(),
            tool_input: serde_json::json!({"query": "x"}),
        });
        assert_eq!(
            project_call_access(&mcp, ToolAccess::All, Some(ToolAccess::ReadWrite)),
            ToolAccess::ReadWrite
        );
    }

    #[test]
    fn canonical_argument_hash_ignores_object_key_order_but_not_values() {
        let left = serde_json::json!({"b": [2, 3], "a": 1});
        let right = serde_json::json!({"a": 1, "b": [2, 3]});
        let changed = serde_json::json!({"a": 1, "b": [3, 2]});
        assert_eq!(hash_canonical_json(&left), hash_canonical_json(&right));
        assert_ne!(hash_canonical_json(&left), hash_canonical_json(&changed));
    }

    #[test]
    fn bash_descriptor_covers_every_projected_invocation() {
        let input = ToolInput::Bash(BashToolInput {
            command: "cat README.md".into(),
            timeout: None,
            description: "read".into(),
            is_background: false,
        });
        let required = project_call_access(&input, ToolAccess::All, None);
        assert_eq!(required, ToolAccess::ReadExecute);
        assert!(ToolAccess::All.covers(required));
    }

    #[test]
    fn delegation_grant_is_authorized_while_owner_cleanup_is_framework_control() {
        let inherited = ToolInput::Task(TaskToolInput {
            prompt: "inspect".into(),
            description: "inspect".into(),
            subagent_type: "explore".into(),
            run_in_background: true,
            capability_mode: None,
            isolation: None,
            resume_from: None,
            cwd: None,
            model: None,
            task_id: None,
        });
        let read_only = ToolInput::Task(TaskToolInput {
            capability_mode: Some(tool_types::SubagentCapabilityMode::ReadOnly),
            ..match inherited.clone() {
                ToolInput::Task(task) => task,
                _ => unreachable!(),
            }
        });
        let kill = ToolInput::KillTask(KillTaskToolInput {
            task_id: "owned-task".into(),
        });
        assert_eq!(
            project_call_access(&inherited, ToolAccess::All, None),
            ToolAccess::All,
            "an omitted capability may resolve to an all-capable Agent role"
        );
        assert_eq!(
            project_call_access(&read_only, ToolAccess::All, None),
            ToolAccess::Read
        );
        assert_eq!(
            project_call_access(&kill, ToolAccess::None, None),
            ToolAccess::None
        );
    }
}
#[cfg(test)]
mod state_control_batch_tests {
    use super::{
        ControlDisposition, ToolLoop, retain_batch_terminal_result, split_control_preflight_barrier,
    };
    fn call(name: &str, args: &str) -> crate::sampling::types::ToolCallResponse {
        crate::sampling::types::ToolCallResponse {
            id: format!("call_{name}"),
            kind: "function".into(),
            function: crate::sampling::types::ToolCallFunction::new(name, args),
        }
    }
    /// Test double for the explicit metadata declared by control tools.
    fn isolates_batch_preflight(name: &str) -> bool {
        matches!(
            name,
            "plan_control" | "PlanControl" | "create_goal" | "update_goal"
        )
    }
    #[test]
    fn lifecycle_control_is_the_only_admitted_call_in_a_mixed_batch() {
        let write = call(
            "search_replace",
            r#"{"file_path":"/tmp/plan.md","old_string":"a","new_string":"b"}"#,
        );
        let exit = call("plan_control", "{}");
        let unknown_alias = call("FinishPlan", "{}");
        let proposal = call(
            "SubmitProposal",
            r#"{"name":"p","overview":"o","plan":"plan body","todos":[]}"#,
        );
        let create_goal = call("create_goal", r#"{"objective":"ship"}"#);
        for calls in [
            vec![write.clone(), exit.clone()],
            vec![exit.clone(), write.clone()],
            vec![write.clone(), create_goal],
            vec![write.clone(), exit.clone(), proposal.clone()],
        ] {
            let count = calls.len();
            let (control, siblings) =
                split_control_preflight_barrier(calls, isolates_batch_preflight);
            assert!(control.is_some());
            assert_eq!(siblings.len(), count - 1);
        }
        let (control, siblings) = split_control_preflight_barrier(
            vec![write.clone(), unknown_alias],
            isolates_batch_preflight,
        );
        assert!(control.is_none());
        assert_eq!(siblings.len(), 2);
        let (control, siblings) = split_control_preflight_barrier(
            vec![write.clone(), proposal],
            isolates_batch_preflight,
        );
        assert!(control.is_none());
        assert_eq!(siblings.len(), 2);

        let second_control = call("update_goal", r#"{"status":"complete"}"#);
        let (control, siblings) = split_control_preflight_barrier(
            vec![exit, second_control, write],
            isolates_batch_preflight,
        );
        assert_eq!(control.unwrap().function.name, "plan_control");
        assert_eq!(
            siblings.len(),
            2,
            "later lifecycle controls are siblings too"
        );
    }

    #[test]
    fn only_successful_control_makes_the_batch_terminal() {
        let mut final_result = None;
        retain_batch_terminal_result(&mut final_result, ToolLoop::Continue);
        assert!(
            final_result.is_none(),
            "an invalid or rejected control returns Continue so siblings remain admissible"
        );

        retain_batch_terminal_result(
            &mut final_result,
            ToolLoop::Control(ControlDisposition::ResampleStep),
        );
        assert!(matches!(
            &final_result,
            Some(ToolLoop::Control(ControlDisposition::ResampleStep))
        ));

        retain_batch_terminal_result(
            &mut final_result,
            ToolLoop::Control(ControlDisposition::EndTurn),
        );
        assert!(
            matches!(
                &final_result,
                Some(ToolLoop::Control(ControlDisposition::ResampleStep))
            ),
            "the first terminal decision owns sibling cancellation"
        );
    }
}
#[cfg(test)]
mod plan_mode_edit_gate_tests {
    use super::{
        PlanEditGate, mcp_call_max_access, plan_mode_edit_gate, public_workflow_conflict,
        saved_workflow_definition_write, session_workflow_definition_write,
        workflow_definition_write, workflow_run_snapshot_write,
    };
    use crate::session::behavior::BehaviorCoordinator;
    use crate::session::mcp_servers::McpState;
    use std::sync::Arc;
    use tokio::sync::Mutex as TokioMutex;
    use tools::types::ToolInput;
    use workspace::permission::AccessKind;
    /// Tracker in Plan Drafting with the session artifact at
    /// `/tmp/gate-session/plan.md`.
    fn active_tracker() -> BehaviorCoordinator {
        let mut t = BehaviorCoordinator::new();
        assert!(t.select_behavior(tool_types::BehaviorId::Plan));
        t
    }
    #[test]
    fn workflow_creation_observes_both_turn_and_current_behavior() {
        use tool_types::BehaviorId::*;
        assert_eq!(public_workflow_conflict(Normal, Plan), Some(Normal));
        assert_eq!(public_workflow_conflict(Goal, Normal), Some(Goal));
        assert_eq!(public_workflow_conflict(Normal, Normal), Some(Normal));
        assert_eq!(public_workflow_conflict(Workflow, Clarify), Some(Clarify));
        assert_eq!(public_workflow_conflict(Workflow, Workflow), None);
    }
    #[test]
    fn workflow_definition_writes_are_recognized_without_blocking_reads() {
        let cwd = std::path::Path::new("/tmp/project");
        let session_dir = std::path::Path::new("/tmp/session");
        assert!(workflow_definition_write(
            &AccessKind::Edit(".grow/workflows/review.rhai".into()),
            session_dir,
            cwd,
            None
        ));
        assert!(saved_workflow_definition_write(
            &AccessKind::Edit(".grow/workflows/review.rhai".into()),
            cwd,
            None
        ));
        assert!(!session_workflow_definition_write(
            &AccessKind::Edit(".grow/workflows/review.rhai".into()),
            session_dir,
            cwd,
            None
        ));
        assert!(workflow_definition_write(
            &AccessKind::Bash("tee .grow/workflows/review.rhai".into()),
            session_dir,
            cwd,
            None
        ));
        assert!(workflow_definition_write(
            &AccessKind::Bash("cd .grow && tee workflows/review.rhai".into()),
            session_dir,
            cwd,
            None
        ));
        assert!(workflow_definition_write(
            &AccessKind::Bash("env -C .grow tee workflows/review.rhai".into()),
            session_dir,
            cwd,
            None
        ));
        assert!(workflow_definition_write(
            &AccessKind::Bash("bash -c 'cd .grow && tee workflows/review.rhai'".into()),
            session_dir,
            cwd,
            None
        ));
        assert!(workflow_definition_write(
            &AccessKind::Bash("tee\t.grow/workflows/review.rhai".into()),
            session_dir,
            cwd,
            None
        ));
        assert!(!workflow_definition_write(
            &AccessKind::Bash("sed -n '1,20p' .grow/workflows/review.rhai".into()),
            session_dir,
            cwd,
            None
        ));
        assert!(workflow_definition_write(
            &AccessKind::Bash("rm -r /tmp/session/workflow-workspace".into()),
            session_dir,
            cwd,
            None
        ));
        assert!(session_workflow_definition_write(
            &AccessKind::Edit("/tmp/session/workflow-workspace/drafts/a.rhai".into()),
            session_dir,
            cwd,
            None
        ));
        assert!(!saved_workflow_definition_write(
            &AccessKind::Edit("/tmp/session/workflow-workspace/drafts/a.rhai".into()),
            cwd,
            None
        ));
        assert!(workflow_run_snapshot_write(
            &AccessKind::Edit("/tmp/session/workflows/wf_1/script.rhai".into()),
            session_dir,
            cwd,
            None,
        ));
        assert!(!workflow_run_snapshot_write(
            &AccessKind::Bash("sed -n '1,20p' /tmp/session/workflows/wf_1/script.rhai".into()),
            session_dir,
            cwd,
            None,
        ));
        assert!(workflow_run_snapshot_write(
            &AccessKind::Bash("tee ../session/workflows/wf_1/script.rhai".into()),
            session_dir,
            cwd,
            None,
        ));
    }

    #[cfg(unix)]
    #[test]
    fn workflow_edit_gate_follows_symlinked_aliases_and_relative_run_paths() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("project");
        let session_dir = root.path().join("session");
        let saved = cwd.join(".grow/workflows");
        let runs = session_dir.join("workflows");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&saved).unwrap();
        std::fs::create_dir_all(&runs).unwrap();
        let saved_alias = root.path().join("saved-alias");
        let runs_alias = root.path().join("runs-alias");
        symlink(&saved, &saved_alias).unwrap();
        symlink(&runs, &runs_alias).unwrap();

        assert!(saved_workflow_definition_write(
            &AccessKind::Edit(saved_alias.join("review.rhai").display().to_string()),
            &cwd,
            None,
        ));
        assert!(saved_workflow_definition_write(
            &AccessKind::Bash("tee ../saved-alias/review.rhai".into()),
            &cwd,
            None,
        ));
        assert!(workflow_run_snapshot_write(
            &AccessKind::Edit(runs_alias.join("wf-1/script.rhai").display().to_string()),
            &session_dir,
            &cwd,
            None,
        ));
        assert!(workflow_run_snapshot_write(
            &AccessKind::Edit("../session/workflows/wf-1/args.json".into()),
            &session_dir,
            &cwd,
            None,
        ));
        assert!(workflow_run_snapshot_write(
            &AccessKind::Bash("cp /tmp/replacement ../runs-alias/wf-1/args.json".into()),
            &session_dir,
            &cwd,
            None,
        ));
    }
    /// Non-MCP inputs resolve no read-only classification (`None`).
    fn gate(tracker: &BehaviorCoordinator, input: &ToolInput) -> PlanEditGate {
        plan_mode_edit_gate(
            tracker,
            input,
            &AccessKind::from_tool_call("test", input),
            None,
        )
    }
    /// MCP inputs carry the call-site-resolved trust-domain RWX ceiling.
    fn gate_mcp(
        tracker: &BehaviorCoordinator,
        input: &ToolInput,
        max_access: tool_protocol::ToolAccess,
    ) -> PlanEditGate {
        plan_mode_edit_gate(
            tracker,
            input,
            &AccessKind::from_tool_call("test", input),
            Some(max_access),
        )
    }
    fn mcp_tool(qualified_name: &str) -> ToolInput {
        use tools::implementations::use_tool::UseToolInput;
        ToolInput::UseTool(UseToolInput {
            tool_name: qualified_name.into(),
            tool_input: serde_json::json!({}),
        })
    }
    fn search_replace(path: &str) -> ToolInput {
        use tools::implementations::grow_build::search_replace::SearchReplaceInput;
        ToolInput::SearchReplace(SearchReplaceInput {
            file_path: path.into(),
            old_string: "a".into(),
            new_string: "b".into(),
            replace_all: false,
        })
    }
    fn write(path: &str) -> ToolInput {
        use tools::implementations::grow_build::write::WriteInput;
        ToolInput::Write(WriteInput {
            file_path: path.into(),
            content: "x".into(),
        })
    }
    /// Every ordinary edit is rejected while Plan is active.
    #[test]
    fn grow_edits_outside_plan_file_rejected() {
        let t = active_tracker();
        assert_eq!(
            gate(&t, &search_replace("/tmp/src/main.rs")),
            PlanEditGate::RejectEdit
        );
        assert_eq!(gate(&t, &write("/tmp/README.md")), PlanEditGate::RejectEdit);
    }
    /// The session artifact path has no Edit carve-out.
    #[test]
    fn plan_artifact_edit_is_rejected() {
        let t = active_tracker();
        assert_eq!(
            gate(&t, &search_replace("/tmp/gate-session/plan.md")),
            PlanEditGate::RejectEdit
        );
        assert_eq!(
            gate(&t, &write("/tmp/gate-session/plan.md")),
            PlanEditGate::RejectEdit
        );
    }
    /// Drafting rejects shell execution as potentially mutating; read-only
    /// discovery continues through the normal permission path.
    #[test]
    fn bash_is_gated_during_drafting() {
        use tools::implementations::BashToolInput;
        let t = active_tracker();
        assert_eq!(
            gate(
                &t,
                &ToolInput::Bash(BashToolInput {
                    command: "echo hi > /tmp/f".into(),
                    timeout: None,
                    description: "write via bash".into(),
                    is_background: false,
                })
            ),
            PlanEditGate::RejectEdit
        );
    }
    /// A config-declared read-only server's MCP tools pass while drafting;
    /// every other MCP tool fails closed (unconfigured server, or a
    /// `Some(false)` classification from the call-site lookup).
    #[test]
    fn read_only_mcp_tools_allowed_while_drafting() {
        let t = active_tracker();
        assert_eq!(
            gate_mcp(
                &t,
                &mcp_tool("docs__search_docs"),
                tool_protocol::ToolAccess::ReadWrite
            ),
            PlanEditGate::Allow
        );
        assert_eq!(
            gate_mcp(
                &t,
                &mcp_tool("unknown__search_docs"),
                tool_protocol::ToolAccess::All
            ),
            PlanEditGate::RejectEdit
        );
    }
    #[test]
    fn executing_allows_mcp_tools_regardless_of_read_only() {
        use tools::implementations::grow_build::workflow::WorkflowToolInput;
        let mut t = active_tracker();
        t.record_plan_artifact("# approved plan");
        assert!(t.submit_initial_plan());
        assert!(t.approve_submitted_plan());
        assert_eq!(gate(&t, &write("/tmp/src/main.rs")), PlanEditGate::Allow);
        // MCP tools are unrestricted in Executing; the read-only classification
        // only narrows non-executing phases.
        assert_eq!(
            gate_mcp(&t, &mcp_tool("any__tool"), tool_protocol::ToolAccess::All),
            PlanEditGate::Allow
        );
        assert_eq!(
            gate_mcp(
                &t,
                &mcp_tool("any__tool"),
                tool_protocol::ToolAccess::ReadWrite,
            ),
            PlanEditGate::Allow
        );
        let workflow = ToolInput::Workflow(WorkflowToolInput::Search {
            query: "review".into(),
            limit: None,
        });
        assert_eq!(gate(&t, &workflow), PlanEditGate::RejectWorkflow);
    }
    /// The call-site lookup: parse `server__tool`, hit the cached read-only
    /// set, and fail closed for unparseable names and unconfigured servers.
    #[tokio::test]
    async fn mcp_scope_lookup_hits_map_and_fails_closed() {
        let mcp_state = Arc::new(TokioMutex::new(McpState::new(vec![])));
        mcp_state
            .lock()
            .await
            .mcp_server_max_access
            .insert("docs".to_string(), tool_protocol::ToolAccess::ReadWrite);

        // Non-MCP access kinds resolve to `None` (gate ignores it).
        assert_eq!(
            mcp_call_max_access(&mcp_state, &AccessKind::Read(None)).await,
            None
        );
        let mcp = |name: &str| AccessKind::MCPTool {
            name: name.to_string(),
            input: serde_json::json!({}),
        };
        // Configured read-only server.
        assert_eq!(
            mcp_call_max_access(&mcp_state, &mcp("docs__search_docs")).await,
            Some(tool_protocol::ToolAccess::ReadWrite)
        );
        // Configured-but-not-read-only server fails closed.
        assert_eq!(
            mcp_call_max_access(&mcp_state, &mcp("linear__create_issue")).await,
            Some(tool_protocol::ToolAccess::All)
        );
        // Unparseable qualified name (missing `__` delimiter) fails closed.
        assert_eq!(
            mcp_call_max_access(&mcp_state, &mcp("not_qualified")).await,
            Some(tool_protocol::ToolAccess::All)
        );
    }
    /// Drafting: an MCP tool from a config-declared read-only server is
    /// allowed end-to-end (lookup + gate); an unconfigured server is rejected.
    #[tokio::test]
    async fn drafting_read_only_server_tool_passes_gate_end_to_end() {
        let mcp_state = Arc::new(TokioMutex::new(McpState::new(vec![])));
        mcp_state
            .lock()
            .await
            .mcp_server_max_access
            .insert("docs".to_string(), tool_protocol::ToolAccess::ReadWrite);
        let tracker = active_tracker();
        for (qualified, expected) in [
            ("docs__search_docs", PlanEditGate::Allow),
            ("other__search_docs", PlanEditGate::RejectEdit),
        ] {
            let input = mcp_tool(qualified);
            let access_kind = AccessKind::from_tool_call("workflow", &input);
            let scope = mcp_call_max_access(&mcp_state, &access_kind).await;
            assert_eq!(
                plan_mode_edit_gate(&tracker, &input, &access_kind, scope),
                expected,
                "unexpected gate outcome for {qualified}"
            );
        }
    }
    /// Normal allows edits; a selected Drafting Plan already narrows them.
    #[test]
    fn inactive_allows_edits_but_pending_plan_rejects_them() {
        let inactive = BehaviorCoordinator::new();
        assert_eq!(
            gate(&inactive, &search_replace("/tmp/src/main.rs")),
            PlanEditGate::Allow
        );
        let mut pending = BehaviorCoordinator::new();
        assert!(pending.select_behavior(tool_types::BehaviorId::Plan));
        assert_eq!(
            gate(&pending, &search_replace("/tmp/src/main.rs")),
            PlanEditGate::RejectEdit
        );
    }
}
#[cfg(test)]
mod plan_approval_helper_tests {
    use super::ext_method_no_client;
    #[test]
    fn ext_method_no_client_defaults_false_for_untagged_error() {
        assert!(!ext_method_no_client(&acp_transport::acp_internal_error(
            "unrelated internal error"
        )));
    }
}

#[cfg(test)]
mod plan_finish_projection_tests {
    use agent_client_protocol::schema::v1 as acp;

    #[tokio::test]
    async fn finishing_plan_refreshes_available_commands_with_normal_behavior() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let (actor, mut event_rx) =
                    crate::session::actor::tests::support::create_test_actor_ex(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await;
                *actor.agent.borrow_mut() =
                    crate::session::actor::tests::support::test_agent_with_plan_tools().await;
                actor
                    .behavior
                    .lock()
                    .select_behavior(tool_types::BehaviorId::Plan);
                while event_rx.try_recv().is_ok() {}

                actor.finish_plan_to_default().await.unwrap();

                let mut available_commands = None;
                while let Ok(event) = event_rx.try_recv() {
                    let crate::session::replay_events::SessionEvent::Notification(notification) =
                        event
                    else {
                        continue;
                    };
                    let crate::session::replay_events::SessionNotification::Acp(notification) =
                        notification
                    else {
                        continue;
                    };
                    if let acp::SessionUpdate::AvailableCommandsUpdate(update) = notification.update
                    {
                        available_commands = Some(update);
                    }
                }
                let update = available_commands.expect("AvailableCommandsUpdate after Plan exit");
                let meta = update.meta.expect("command metadata");
                assert_eq!(
                    meta.get("grow/behaviorAvailability")
                        .and_then(|value| value.get("current"))
                        .and_then(serde_json::Value::as_str),
                    Some("normal")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn missing_restored_plan_artifact_normalizes_to_durable_normal() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, _gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let (actor, _event_rx) =
                    crate::session::actor::tests::support::create_test_actor_ex(
                        0,
                        256_000,
                        85,
                        gateway_tx,
                        persistence_tx,
                    )
                    .await;
                {
                    let mut behavior = actor.behavior.lock();
                    behavior.select_behavior(tool_types::BehaviorId::Plan);
                    behavior.record_plan_artifact("# artifact that is not on disk");
                    assert!(behavior.submit_initial_plan());
                }

                assert!(!actor.reconcile_restored_plan_approval().await.unwrap());
                assert_eq!(
                    actor.behavior.lock().behavior(),
                    tool_types::BehaviorId::Normal
                );
                let events = actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .expect("Timeline events");
                assert!(matches!(
                    events.last().map(|event| &event.kind),
                    Some(chat_state::TimelineEventKind::Control(_))
                ));
            })
            .await;
    }
}
#[cfg(test)]
mod wait_interrupt_tests {
    use super::{
        BlockingWaitGuard, interrupted_wait_tool_result, interrupted_wait_tool_result_with_msg,
        is_interruptible_wait_tool, wait_for_pending_interjection,
    };
    use tool_types::TaskOutputOutput;
    use tools::types::output::ToolOutput;
    /// The interruptible-wait select arms: a pending interjection aborts an
    /// in-flight wait, and `biased` prefers an already-completed wait result
    /// over the abort. (Unit-level: the full dispatch loop has no test seam.)
    #[tokio::test(start_paused = true)]
    async fn pending_interjection_aborts_in_flight_wait() {
        use super::InterjectionBuffer;
        use super::PendingInterjection;
        let buf: InterjectionBuffer<agent_client_protocol::schema::v1::ImageContent> =
            InterjectionBuffer::default();
        let out = tokio::select! {
            biased;
            r = async { "wait-result" } => r,
            _ = wait_for_pending_interjection(&buf) => "aborted",
        };
        assert_eq!(out, "wait-result");
        buf.push(PendingInterjection {
            text: "user message".into(),
            attachments: Vec::new(),
            requeue: None,
        });
        let out = tokio::select! {
            biased;
            r = async {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                "wait-result"
            } => r,
            _ = wait_for_pending_interjection(&buf) => "aborted",
        };
        assert_eq!(out, "aborted");
        let out = tokio::select! {
            biased;
            r = async { "wait-result" } => r,
            _ = wait_for_pending_interjection(&buf) => "aborted",
        };
        assert_eq!(out, "wait-result");
    }
    #[test]
    fn interruptible_wait_tool_only_when_timeout_positive() {
        assert!(is_interruptible_wait_tool(
            "get_command_or_subagent_output",
            &serde_json::json!({"task_ids": ["t"], "timeout_ms": 120_000})
        ));
        assert!(!is_interruptible_wait_tool(
            "get_task_output",
            &serde_json::json!({"task_ids": ["t"], "timeout_ms": 0})
        ));
        assert!(!is_interruptible_wait_tool(
            "get_task_output",
            &serde_json::json!({"task_ids": ["t"]})
        ));
        assert!(!is_interruptible_wait_tool(
            "read_file",
            &serde_json::json!({"target_file": "/tmp/x"})
        ));
    }
    #[test]
    fn interrupted_task_wait_result_keeps_task_running() {
        let r = interrupted_wait_tool_result(&serde_json::json!({
            "task_ids": ["bg-9"],
            "timeout_ms": 60_000
        }));
        assert!(
            r.prompt_text
                .contains("Wait moved to background because the user sent a message.")
        );
        match &r.output {
            ToolOutput::TaskOutput(TaskOutputOutput::Result(res)) => {
                assert_eq!(res.task_id, "bg-9");
                assert_eq!(res.status, "running");
            }
            other => panic!("expected TaskOutput Result, got {other:?}"),
        }
        assert!(!r.output.is_error());
    }
    #[test]
    fn pure_timing_wait_does_not_claim_a_background_completion() {
        let r = interrupted_wait_tool_result_with_msg(
            &serde_json::json!({"duration_ms": 60_000}),
            "Wait ended early because the user sent a message.",
        );
        assert!(r.prompt_text.contains("Wait ended early"));
        assert!(!r.prompt_text.contains("still running"));
        assert!(!r.prompt_text.contains("delivered automatically"));
    }
    /// `BlockingWaitGuard` counts nested waits; drop always decrements.
    #[test]
    fn blocking_wait_guard_counts_and_restores_on_drop() {
        use std::sync::Arc;
        let depth = Arc::new(crate::tools::tool_context::BlockingWaitState::new());
        {
            let _g1 = BlockingWaitGuard::enter(depth.clone());
            assert_eq!(depth.depth(), 1);
            {
                let _g2 = BlockingWaitGuard::enter(depth.clone());
                assert_eq!(depth.depth(), 2);
            }
            assert_eq!(depth.depth(), 1);
        }
        assert_eq!(depth.depth(), 0, "drop must restore");
    }
    /// An aborted wait future must not leak the depth count.
    #[tokio::test(start_paused = true)]
    async fn blocking_wait_guard_decrements_when_future_aborted() {
        use std::sync::Arc;
        let depth = Arc::new(crate::tools::tool_context::BlockingWaitState::new());
        let inner = depth.clone();
        let task = tokio::spawn(async move {
            let _g = BlockingWaitGuard::enter(inner);
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        });
        tokio::task::yield_now().await;
        assert_eq!(depth.depth(), 1);
        task.abort();
        let _ = task.await;
        assert_eq!(depth.depth(), 0, "abort must not leak");
    }
    #[test]
    fn blocking_wait_guard_reset_is_generation_scoped() {
        use std::sync::Arc;
        let depth = Arc::new(crate::tools::tool_context::BlockingWaitState::new());
        let old = BlockingWaitGuard::enter(depth.clone());
        assert_eq!(depth.depth(), 1);
        depth.reset();
        let new = BlockingWaitGuard::enter(depth.clone());
        assert_eq!(depth.depth(), 1);
        drop(old);
        assert_eq!(
            depth.depth(),
            1,
            "old-generation drop must not consume the new wait"
        );
        drop(new);
        assert_eq!(depth.depth(), 0);
    }
}

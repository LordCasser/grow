//! Tool result settlement and sampler event delivery for SessionActor.

use super::*;

/// Whether a tool name is an MCP `create_pull_request` (qualified
/// `server__create_pull_request` or bare).
pub(super) fn is_mcp_create_pull_request(tool_name: &str) -> bool {
    match crate::session::mcp_servers::parse_mcp_tool_name(tool_name) {
        Some((_, tool)) => tool == "create_pull_request",
        None => tool_name == "create_pull_request",
    }
}
/// One `tool.execution` span, wrapping a single dispatch attempt.
///
/// Outcome fields are declared `Empty` here because `record` on a field the span
/// never declared is silently dropped; [`record_tool_span_outcome`] fills them in
/// once the result is known.
pub(super) fn tool_execution_span(
    parent: &tracing::Span,
    session_id: &str,
    prepared: &PreparedToolCall,
    tool_call_id: &str,
    retry: bool,
) -> tracing::Span {
    tracing::info_span!(
        parent: parent,
        "tool.execution",
        session_id = %session_id,
        tool_name = %prepared.tool_name,
        // Same value under both names: `tool_call_id` is the join key, `tool_use_id`
        // is kept for existing queries.
        tool_use_id = %tool_call_id,
        tool_call_id = %tool_call_id,
        retry,
        success = tracing::field::Empty,
        outcome = tracing::field::Empty,
        tool_input_size_bytes = prepared.raw_arguments.len() as i64,
        tool_result_size_bytes = tracing::field::Empty,
    )
}
/// Stamp the dispatch outcome on `span` and close it, returning whether the call
/// succeeded. Takes the span by value: these fields are recorded exactly once.
pub(super) fn record_tool_span_outcome(
    span: tracing::Span,
    result: &Result<ToolRunResult, tool_runtime::ToolError>,
) -> bool {
    let (success, result_size) = match result {
        Ok(tool_result) => (
            !tool_result.output.is_error(),
            tool_result.prompt_text.len() as i64,
        ),
        Err(_) => (false, 0),
    };
    span.record("success", success);
    span.record("outcome", if success { "success" } else { "error" });
    span.record("tool_result_size_bytes", result_size);
    success
}

/// A terminal Task failure is still the unique delivery surface for its
/// durable completion receipt. The tool carries this internal correlation in
/// error metadata so the failed tool_result can acknowledge the receipt after
/// it is committed, without changing the model-visible failure semantics.
pub(super) fn consumed_completion_id_from_tool_error(
    error: &tool_runtime::ToolError,
) -> Option<&str> {
    error
        .details
        .as_ref()?
        .get("consumed_completion_task_id")?
        .as_str()
        .filter(|id| !id.trim().is_empty())
}

pub(super) fn undispatched_tool_outcome(action: &ToolLoop) -> &'static str {
    match action {
        ToolLoop::Continue | ToolLoop::ControlBoundary => "not_dispatched",
        ToolLoop::NonExistingTool | ToolLoop::ToolParsingError => "invalid_tool",
        ToolLoop::PermissionReject { .. } => "permission_rejected",
        ToolLoop::Cancelled => "permission_cancelled",
        ToolLoop::PermissionTimedOut { .. } => "permission_timed_out",
        ToolLoop::FollowupMessage(_) => "followup",
        ToolLoop::HookDenied { .. } => "hook_denied",
    }
}
/// Blocking wait tools that should abort when a mid-turn interjection is pending.
pub(super) fn is_interruptible_wait_tool(tool_name: &str, args: &serde_json::Value) -> bool {
    match tool_name {
        "get_task_output"
        | "get_command_or_subagent_output"
        | "get_task_or_subagent_output"
        | "get_terminal_command_output" => tool_types::task_output_waits_from_json(args),
        "Await" | "AwaitShell" => true,
        _ => false,
    }
}
pub(in crate::session::actor) async fn wait_for_pending_interjection(
    buf: &InterjectionBuffer<acp::ImageContent>,
) {
    buf.wait_nonempty().await;
}
use crate::tools::tool_context::BlockingWaitGuard;
/// Model-facing result when a wait is aborted for a pending interjection.
pub(super) fn interrupted_wait_tool_result(args: &serde_json::Value) -> ToolRunResult {
    interrupted_wait_tool_result_with_msg(
        args,
        "Wait moved to background because the user sent a message. The task is still running and its completion will be delivered automatically.",
    )
}
/// [`interrupted_wait_tool_result`] with a caller-chosen model-facing message.
pub(super) fn interrupted_wait_tool_result_with_msg(
    args: &serde_json::Value,
    msg: &str,
) -> ToolRunResult {
    use tool_types::{TaskOutputOutput, TaskOutputResult};
    let task_id = args
        .get("task_ids")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .or_else(|| args.get("task_id").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let status = if task_id.is_empty() {
        "cancelled"
    } else {
        "running"
    };
    let result = TaskOutputResult {
        task_id,
        command: String::new(),
        status: status.to_string(),
        exit_code: None,
        started: String::new(),
        ended: None,
        duration_secs: 0.0,
        output: msg.to_string(),
        output_file: String::new(),
        truncated: false,
        truncation_hint: String::new(),
        raw_output_bytes: msg.len(),
    };
    ToolRunResult {
        output: ToolsToolOutput::TaskOutput(TaskOutputOutput::Result(result)),
        prompt_text: msg.to_string(),
        effective_tool_name: None,
    }
}

impl SessionActor {
    pub(super) async fn drop_pending_items_for_consumed_completions(&self, consumed_ids: &[&str]) {
        if consumed_ids.is_empty() {
            return;
        }
        let mut state = self.state.lock().await;
        let dropped = state.sweep_pending_inputs(|i| {
            i.origin
                .completion_id()
                .is_some_and(|id| consumed_ids.contains(&id))
        });
        let dropped_inputs = dropped.len();
        drop(state);
        if dropped_inputs > 0 {
            tracing::info!(
                dropped_inputs,
                consumed_ids = ?consumed_ids,
                "auto-wake: dropped queued synthetic items for consumed completions"
            );
        }
    }

    /// Link durable completion receipts to the active turn after the tool
    /// result that exposed them has been appended. `input: None` is not a
    /// second model message; it records that this turn's tool result is the
    /// consumption surface.
    pub(super) async fn acknowledge_consumed_notifications(&self, consumed_ids: &[&str]) {
        if consumed_ids.is_empty() {
            return;
        }
        let Some(turn) = self.events.current_turn() else {
            tracing::error!(consumed_ids = ?consumed_ids, "cannot acknowledge notifications without an active turn");
            return;
        };
        let notification_ids = self
            .chat_state_handle
            .pending_notifications()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|notification| match &notification.source {
                chat_state::NotificationSource::TaskCompleted { task_id, .. } => {
                    consumed_ids.contains(&task_id.as_str())
                }
                chat_state::NotificationSource::SubagentCompleted { subagent_id, .. } => {
                    consumed_ids.contains(&subagent_id.as_str())
                }
                chat_state::NotificationSource::MonitorProgress { .. }
                | chat_state::NotificationSource::TaskStillRunning { .. }
                | chat_state::NotificationSource::WorkflowCompleted { .. } => false,
            })
            .map(|notification| notification.id)
            .collect::<Vec<_>>();
        if notification_ids.is_empty() {
            return;
        }
        if let Err(error) = self
            .consume_notifications_durably(notification_ids, turn, None)
            .await
        {
            tracing::error!(%error, "failed to acknowledge tool-consumed notifications");
        }
    }
    /// Drain queued synthetic prompts during shutdown. Durable notifications
    /// remain in Timeline and are replayed on the next session load.
    ///
    /// Called from `SessionCommand::Shutdown` as a defensive backstop
    /// so a synthetic prompt that slipped past the per-tool-result
    /// sweep cannot be accepted into Timeline after the actor returns. Real
    /// user inputs are preserved.
    pub(in crate::session::actor) async fn drop_pending_synthetic_items(&self) {
        let mut state = self.state.lock().await;
        let mut kept = VecDeque::with_capacity(state.pending_inputs.len());
        for input in std::mem::take(&mut state.pending_inputs) {
            if !input.origin.is_synthetic() {
                kept.push_back(input);
            }
        }
        state.pending_inputs = kept;
        drop(state);
    }

    /// Record git/PR ops from a successful tool result into session signals
    /// (`turn_result.json`) and diagnostics. Detection runs here at the shell's
    /// tool-result chokepoint over the command + prompt output (nothing is
    /// wired through the tool's output schema): successful foreground bash
    /// commands, plus MCP `create_pull_request` results (url/number parsed
    /// from the result text). Backgrounded commands are not scanned.
    fn record_git_pr_signals(&self, effective_tool_name: &str, result: &ToolRunResult) {
        use ::diagnostics::enums::PrCreationSource;
        use tools::util::git_detect;
        match &result.output {
            tools::types::output::ToolOutput::Bash(b) if b.exit_code == 0 => {
                let Some(ops) = git_detect::detect_git_ops(&b.command, &b.output_for_prompt) else {
                    return;
                };
                if ops.committed {
                    self.signals_handle().record_git_commit();
                }
                if let Some(pr) = ops.pr_created {
                    self.record_pr_created(pr, PrCreationSource::Bash);
                }
                if ops.pr_merged {
                    self.signals_handle().record_pr_merged();
                    ::diagnostics::session_ctx::log_event(::diagnostics::events::PrMerged {});
                }
            }
            tools::types::output::ToolOutput::MCP(m)
                if !m.is_error && is_mcp_create_pull_request(effective_tool_name) =>
            {
                let pr = git_detect::PrRef::find_in(&result.prompt_text).unwrap_or_default();
                self.record_pr_created(pr, PrCreationSource::Mcp);
            }
            _ => {}
        }
    }
    /// Record a PR creation into session signals.
    ///
    /// `had_commit_in_session` is provisional here: the signals actor
    /// reconciles it at `TakeTurnEndSnapshot`, after every event of the turn
    /// has been processed, so out-of-order parallel tool results (a create
    /// landing before a sibling commit) cannot mis-attribute. The reconciled
    /// result is recorded during `finalize_turn_bookkeeping`.
    fn record_pr_created(
        &self,
        pr: tools::util::git_detect::PrRef,
        source: ::diagnostics::enums::PrCreationSource,
    ) {
        self.signals_handle()
            .record_pr_created(crate::session::signals::PrCreatedSignal {
                url: pr.url,
                number: pr.number,
                source,
                had_commit_in_session: false,
            });
    }

    pub(in crate::session::actor) async fn handle_bridge_tool_success(
        &self,
        tool_call_id: &acp::ToolCallId,
        call_id: &str,
        requested_tool_name: &str,
        effective_tool_name: &str,
        result: ToolRunResult,
        concatenated_json_count: usize,
        model_id: &str,
        tool_parsed_args: &serde_json::Value,
    ) -> Result<Vec<ConversationItem>, acp::Error> {
        use crate::session::acp_conversion::{acp_plan_update, acp_tool_update, maybe_rewrite};
        let consumed_ids =
            tools::reminders::task_completion::consumed_completion_ids(&result.output);
        if !consumed_ids.is_empty() {
            self.completion_delivery.consume(&consumed_ids);
            self.drop_pending_items_for_consumed_completions(&consumed_ids)
                .await;
        }
        if matches!(
            &result.output,
            ToolsToolOutput::SearchReplace(
                tools::types::output::SearchReplaceOutput::EditsApplied(_)
            ) | ToolsToolOutput::Bash(_)
        ) {
            self.maybe_notify_git_branch().await;
        }
        if let tools::types::output::ToolOutput::Bash(ref b) = result.output
            && b.was_bare_echo
        {
            self.signals_handle().record_bare_echo();
        }
        self.record_git_pr_signals(effective_tool_name, &result);
        let path_rewriter = self.path_rewriter();
        let tool_meta = {
            let state = self.mcp_state.lock().await;
            state.mcp_tool_meta.get(effective_tool_name).cloned()
        };
        if let Some(mut tool_update) =
            acp_tool_update(&result.output, call_id, path_rewriter.as_ref(), tool_meta)
        {
            if tool_update.fields.status == Some(acp::ToolCallStatus::Failed) {
                tracing::error!(
                    session_id = %self.session_info.id.0,
                    tool_name = requested_tool_name,
                    effective_tool_name = effective_tool_name,
                    model_id = model_id,
                    error_kind = "tool_output_error",
                    "tool_error: tool_output_error"
                );
                self.signals_handle()
                    .record_tool_failure(requested_tool_name);
            } else {
                self.signals_handle()
                    .record_tool_success(requested_tool_name);
            }
            if matches!(
                &result.output,
                tools::types::output::ToolOutput::PlanControl(_)
            ) {
                let plan_ref = self
                    .behavior
                    .lock()
                    .plan_artifact_ref()
                    .unwrap_or_else(|| "artifact:plan:unavailable".to_string());
                if let Some(ref mut content) = tool_update.fields.content {
                    for item in content.iter_mut() {
                        if let acp::ToolCallContent::Content(acp::Content {
                            content: acp::ContentBlock::Text(t),
                            ..
                        }) = item
                        {
                            t.text = format!("Plan artifact: {plan_ref}");
                        }
                    }
                }
            }
            tool_update.tool_call_id = tool_call_id.clone();
            self.send_update(acp::SessionUpdate::ToolCallUpdate(tool_update), None)
                .await;
        } else {
            self.signals_handle()
                .record_tool_success(requested_tool_name);
        }
        if let Some(acp_plan) = acp_plan_update(&result.output) {
            self.send_update(acp::SessionUpdate::Plan(acp_plan), None)
                .await;
        }
        let context_recall_coordinates = match &result.output {
            ToolsToolOutput::ContextRecall(output) => Some((
                output.frozen_surface_revision,
                output.context_window,
                output.max_result_tokens,
            )),
            _ => None,
        };
        let mut prompt_text = if concatenated_json_count > 0 {
            let remaining = concatenated_json_count - 1;
            format!(
                "{}\n\n<system-reminder>\nIMPORTANT: Your tool call contained {} concatenated JSON \
                 objects, but only the best-matching one was executed. The remaining {} \
                 were ignored. You MUST use separate tool calls (one per operation) \
                 instead of concatenating multiple JSON objects in a single call's \
                 arguments. Make {} individual tool call{} for the remaining \
                 operations.\n</system-reminder>",
                result.prompt_text,
                concatenated_json_count,
                remaining,
                remaining,
                if remaining == 1 { "" } else { "s" },
            )
        } else {
            result.prompt_text
        };
        let mut inline_images: Vec<ContentPart> = Vec::new();
        let extraction = if !matches!(
            result.output,
            ToolsToolOutput::ReadFile(ReadFileOutput::ImageContent(_))
        ) {
            tools::util::base64_images::extract_base64_images(prompt_text)
        } else {
            tools::util::base64_images::ExtractionResult {
                text: prompt_text,
                images: Vec::new(),
            }
        };
        let mut extracted_images = extraction.images;
        let prompt_text = extraction.text;
        if let ToolsToolOutput::ReadFile(ReadFileOutput::FileContent(ref fc)) = result.output {
            extracted_images.extend(fc.extracted_images.iter().cloned());
        }
        let mut prompt_text = maybe_rewrite(path_rewriter.as_ref(), prompt_text);
        if let ToolsToolOutput::ReadFile(ReadFileOutput::ImageContent(ref image_content)) =
            result.output
        {
            use crate::session::image_normalize::{InlineAttachVerdict, inline_attach_verdict};
            match inline_attach_verdict(&image_content.data) {
                InlineAttachVerdict::TooSmall => {
                    prompt_text =
                        "[Image was not attached: too small for vision models]".to_owned();
                }
                InlineAttachVerdict::Unreadable => {
                    prompt_text =
                        "[Image was not attached: invalid or unreadable image data]".to_owned();
                }
                InlineAttachVerdict::Attach => {
                    let url = format!(
                        "data:{};base64,{}",
                        image_content.mime_type, image_content.data
                    );
                    inline_images.push(ContentPart::Image {
                        url: std::sync::Arc::<str>::from(url),
                    });
                    prompt_text = "Read image file.".to_owned();
                }
            }
        }
        let tool_chat = if inline_images.is_empty() {
            ConversationItem::tool_result(call_id.to_string(), prompt_text)
        } else {
            ConversationItem::tool_result_with_images(
                call_id.to_string(),
                prompt_text,
                inline_images,
            )
        };
        if let Some((expected_surface_revision, context_window, max_result_tokens)) =
            context_recall_coordinates
        {
            let rejection_item = ConversationItem::tool_result(
                call_id.to_string(),
                "Context recall was not inserted because the active context changed or no longer has safe headroom. Re-run context_recall if the evidence is still needed.",
            );
            let max_context_tokens =
                crate::session::actor::context_recall::context_recall_max_context_tokens(
                    context_window,
                );
            let outcome = self
                .chat_state_handle
                .push_tool_result_conditionally(
                    tool_chat,
                    rejection_item,
                    expected_surface_revision,
                    max_context_tokens,
                    max_result_tokens,
                )
                .await
                .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
            if outcome != chat_state::ConditionalToolResultOutcome::Accepted {
                tracing::info!(
                    session_id = %self.session_info.id,
                    ?outcome,
                    "context recall result rejected at the canonical Surface commit point"
                );
            }
        } else {
            self.chat_state_handle.push_tool_result(tool_chat);
        }
        self.acknowledge_consumed_notifications(&consumed_ids).await;
        let mut deferred_followups = Vec::new();
        if !extracted_images.is_empty() {
            let count = extracted_images.len();
            tracing::info!(
                session_id = %self.session_info.id,
                tool = requested_tool_name,
                count,
                "base64 images extracted from tool result",
            );
            let acp_images: Vec<agent_client_protocol::schema::v1::ImageContent> = extracted_images
                .into_iter()
                .map(|img| {
                    agent_client_protocol::schema::v1::ImageContent::new(img.data, img.mime_type)
                })
                .collect();
            let mut norm_result =
                crate::session::image_normalize::normalize_images(acp_images).await;
            if !norm_result.re_encode_fallbacks.is_empty() {
                tracing::warn!(
                    session_id = %self.session_info.id,
                    notes = %norm_result.re_encode_fallbacks.join(" "),
                    "Extracted tool image kept original after re-encode failure",
                );
            }
            if let Some((notice, notes)) = crate::session::image_normalize::dropped_to_envelope(
                std::mem::take(&mut norm_result.dropped),
            ) {
                deferred_followups.push(ConversationItem::user(notice));
                self.send_grow_notification(GrowSessionUpdate::ImageDropped { notes })
                    .await;
            }
            let normalized_count = norm_result.images.len();
            if normalized_count > 0 {
                let mut image_msg = ConversationItem::user(format!(
                    "[{normalized_count} images extracted from the tool result above, in attachment order]"
                ));
                for norm in norm_result.images {
                    let url = format!("data:{};base64,{}", norm.mime_type, norm.data);
                    image_msg.add_image(url);
                }
                deferred_followups.push(image_msg);
            }
        }
        Ok(deferred_followups)
    }

    /// Handle a hard tool execution error (dispatch/validation failure).
    ///
    /// Emits the failed tool_result to the client and records failure signals.
    /// Tool failures are not fed to the doom-loop detector (error-count streaks
    /// were removed), so this never warns/terminates and returns no deferred
    /// follow-ups today.
    pub(super) async fn handle_tool_error(
        &self,
        tool_call_id: &acp::ToolCallId,
        call_id: &str,
        requested_tool_name: &str,
        effective_tool_name: Option<&str>,
        err: &anyhow::Error,
        model_id: &str,
    ) -> Vec<ConversationItem> {
        tracing::error!(
            session_id = %self.session_info.id.0,
            tool_name = requested_tool_name,
            effective_tool_name = effective_tool_name,
            model_id = model_id,
            error_kind = "execution_failure",
            error_message = %err,
            "tool_error: execution_failure"
        );
        self.signals_handle()
            .record_tool_failure(requested_tool_name);
        let rewriter = self.path_rewriter();
        let err_str = match rewriter.as_ref() {
            Some(rw) => rw.rewrite(&err.to_string()),
            None => err.to_string(),
        };
        let message = match effective_tool_name {
            Some(effective) if effective != requested_tool_name => {
                format!("Tool `{effective}` failed via `{requested_tool_name}`: {err_str}")
            }
            _ => format!("Tool `{requested_tool_name}` failed: {err_str}"),
        };
        self.send_update(
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                tool_call_id.clone(),
                acp::ToolCallUpdateFields::new()
                    .status(Some(acp::ToolCallStatus::Failed))
                    .content(Some(vec![acp::ToolCallContent::from(
                        acp::ContentBlock::Text(acp::TextContent::new(message.clone())),
                    )]))
                    .raw_output(Some(json!({
                        "error": "tool_execution_failed",
                        "message": err_str,
                    }))),
            )),
            None,
        )
        .await;
        let tool_chat = ConversationItem::tool_result(call_id.to_string(), message);
        self.chat_state_handle.push_tool_result(tool_chat);
        vec![]
    }

    async fn send_thought_chunk(&self, text: String, chunk_index: u64) {
        self.send_update(
            acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text),
            ))),
            Some(chunk_index),
        )
        .await;
    }
    /// Translate one [`sampler::SamplingEvent`] from the
    /// per-session sampler actor into the corresponding ACP / shell
    /// side-effects (notifications, signal recording, model-metadata
    /// refresh, etc.).
    ///
    /// Called from the drainer task spawned in `spawn_session_actor`,
    /// which loops `while let Some(event) = sampler_event_rx.recv().await`.
    /// Pure event mapping. Semantic recovery (compaction, friendly
    /// errors) lives in [`Self::handle_sampling_failure`] and runs in
    /// the turn loop, not here, because it depends on per-turn state
    /// and may need to call back into `sampler_handle.update_config`
    /// or resubmit.
    pub(crate) async fn handle_sampling_event(self: &Arc<Self>, event: sampler::SamplingEvent) {
        use sampler::{SamplingChannel, SamplingEvent};
        match event {
            SamplingEvent::StreamStarted { timestamp_ms, .. } => {
                self.chat_state_handle.record_stream_start(timestamp_ms);
            }
            SamplingEvent::FirstToken { .. } => {}
            SamplingEvent::ChannelToken {
                channel,
                text,
                chunk_index,
                ..
            } => match channel {
                SamplingChannel::Text => {
                    self.send_update(
                        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                            acp::ContentBlock::Text(acp::TextContent::new(text)),
                        )),
                        Some(chunk_index),
                    )
                    .await;
                }
                SamplingChannel::Reasoning => {
                    self.send_thought_chunk(text, chunk_index).await;
                }
            },
            SamplingEvent::ToolCallDelta {
                tool_index,
                id,
                name,
                arguments_delta,
                ..
            } => {
                self.send_buffered_grow_update(GrowSessionUpdate::ToolCallDeltaChunk {
                    tool_call_id: id,
                    tool_index,
                    name,
                    arguments_delta,
                })
                .await;
            }
            SamplingEvent::ResponseStarted {
                message_id,
                model,
                input_tokens,
                cache_read_input_tokens,
                cache_creation_input_tokens,
                ..
            } => {
                self.send_buffered_grow_update(GrowSessionUpdate::ResponseStarted {
                    message_id: Some(message_id),
                    model: Some(model),
                    input_tokens,
                    cache_read_input_tokens,
                    cache_creation_input_tokens,
                })
                .await;
            }
            SamplingEvent::ReasoningCompleted { signature, .. } => {
                self.send_buffered_grow_update(GrowSessionUpdate::ReasoningCompleted {
                    signature: Some(signature),
                })
                .await;
            }
            SamplingEvent::Completed {
                request_id,
                response,
                metrics,
            } => {
                let usage = response.usage.as_ref();
                self.events.request_completed(
                    request_id.as_str(),
                    metrics.time_to_first_token_ms,
                    chat_state::RequestUsage {
                        input_tokens: usage.map(|usage| u64::from(usage.prompt_tokens)),
                        output_tokens: usage.map(|usage| u64::from(usage.completion_tokens)),
                        cache_read_tokens: usage.map(|usage| u64::from(usage.cached_prompt_tokens)),
                        cache_write_tokens: usage
                            .map(|usage| u64::from(usage.cache_creation_prompt_tokens)),
                    },
                    response.items.len(),
                );
                if let Some(tx) = self.turn_stream_drained.lock().take() {
                    let _ = tx.send(());
                }
                if let Some(policy) = self.doom_loop_recovery {
                    let triggers = policy.confident_triggers(&response.doom_loop_signals);
                    if !triggers.is_empty() {
                        let attempts = {
                            let mut tally = self.doom_loop_turn_tally.lock();
                            if tally.attempts == 0 {
                                None
                            } else {
                                tally.accepted_after_budget = true;
                                tally.merge_triggers(&triggers);
                                Some(tally.attempts)
                            }
                        };
                        if attempts.is_some() {
                            self.signals_handle()
                                .record_doom_loop_accepted_after_budget(triggers);
                        }
                    }
                }
                self.record_api_request_time();
                self.signals_handle().record_inference_metrics(metrics);
            }
            SamplingEvent::ModelMetadata { metadata, .. } => {
                self.handle_model_metadata_update(metadata).await;
            }
            SamplingEvent::Retrying {
                request_id,
                attempt,
                max_retries,
                kind,
                reason,
                doom_loop_triggers,
                doom_loop_aborted_at_chunk,
            } => {
                self.events
                    .request_retrying(chat_state::RequestEvent::Retrying {
                        id: request_id.as_str().to_string(),
                        attempt,
                        max_retries,
                        reason: crate::util::truncate(&reason, 500).to_string(),
                    });
                if kind == sampler::SamplingErrorKind::DoomLoopDetected {
                    let triggers = doom_loop_triggers.unwrap_or_default();
                    {
                        let mut tally = self.doom_loop_turn_tally.lock();
                        tally.attempts += 1;
                        tally.merge_triggers(&triggers);
                    }
                    self.signals_handle()
                        .record_doom_loop_recovery_attempt(triggers, doom_loop_aborted_at_chunk);
                }
                ::diagnostics::unified_log::warn(
                    "shell.turn.inference_retry",
                    Some(self.session_info.id.0.as_ref()),
                    Some(serde_json::json!({
                        "sampler_request_id": request_id.as_str(),
                        "attempt": attempt,
                        "max_retries": max_retries,
                        "kind": kind.as_str(),
                        "reason": crate::util::truncate(&reason, 300),
                    })),
                );
                self.send_grow_notification(GrowSessionUpdate::RetryState(
                    crate::extensions::notification::RetryState::Retrying {
                        attempt,
                        max_retries,
                        reason,
                    },
                ))
                .await;
            }
            SamplingEvent::Failed { request_id, error } => {
                let timeline_error = crate::util::truncate(&error.message, 500).to_string();
                self.events.request_failed(
                    request_id.as_str(),
                    error.kind.as_str(),
                    &timeline_error,
                    error.is_retryable,
                );
                if let Some(tx) = self.turn_stream_drained.lock().take() {
                    let _ = tx.send(());
                }
                if error.message == "request cancelled"
                    && self.goal_loop_active()
                    && !self.pending_interjections.is_empty()
                {
                    tracing::info!(
                        sampler_request_id = request_id.as_str(),
                        "ignored expected sampler cancellation from Goal soft preemption"
                    );
                    return;
                }
                ::diagnostics::unified_log::error(
                    "shell.turn.inference_failed",
                    Some(self.session_info.id.0.as_ref()),
                    Some(serde_json::json!({
                        "sampler_request_id": request_id.as_str(),
                        "kind": error.kind.as_str(),
                        "status_code": error.status_code,
                        "is_retryable": error.is_retryable,
                        "message": crate::util::truncate(&error.message, 300),
                    })),
                );
                self.signals_handle()
                    .record_error_typed(error.kind.as_str());
                if let Some(ref ctx) = error.empty_response_context {
                    tracing::info!(
                        empty_response = true,
                        empty_reason = ctx.reason.as_str(),
                        had_reasoning = ctx.had_reasoning,
                        finish_reason = ctx.finish_reason_str(),
                        model = %ctx.model,
                        "sampler reported empty response (will retry if retryable)",
                    );
                }
            }
        }
    }

    /// Model-facing rejection for an ordinary file edit while Plan is active.
    pub(super) async fn plan_mode_edit_rejected_message(&self) -> String {
        self.render_plan_template(
            crate::session::behavior::plan_mode_edit_rejected_template(),
            "",
        )
        .await
        .unwrap_or_else(|| {
            "Rejected: ordinary file editing is prohibited while Plan behavior is active."
                .to_string()
        })
    }
    pub(in crate::session::actor) async fn handle_tool_not_executed(
        &self,
        model_call_id: &str,
        tool_call_id: &acp::ToolCallId,
        reason: String,
    ) -> Result<(), acp::Error> {
        let tool_update = acp::ToolCallUpdate::new(
            tool_call_id.clone(),
            acp::ToolCallUpdateFields::new()
                .status(Some(acp::ToolCallStatus::Failed))
                .content(Some(vec![acp::ToolCallContent::from(
                    acp::ContentBlock::Text(acp::TextContent::new(reason.clone())),
                )])),
        );
        self.send_update(acp::SessionUpdate::ToolCallUpdate(tool_update), None)
            .await;
        let tool_chat = ConversationItem::tool_result(model_call_id.to_owned(), reason);
        self.chat_state_handle.push_tool_result(tool_chat);
        Ok(())
    }
}

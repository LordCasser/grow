//! Tool-call preparation and client-facing ToolCall updates.

use super::parse::execute_tool_call_parts;
use super::*;

struct TimelinePermissionObserver {
    pending: crate::session::pending_interaction::PendingInteractions,
    gateway: acp_transport::AcpAgentGatewaySender,
    session_id: acp::SessionId,
    call_id: String,
    attempt_id: String,
    chat_state: chat_state::ChatStateHandle,
    runtime_suspend: std::sync::Arc<std::sync::atomic::AtomicBool>,
    guard: std::sync::Mutex<Option<crate::session::pending_interaction::PendingInteractionGuard>>,
}

impl std::fmt::Debug for TimelinePermissionObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimelinePermissionObserver")
            .field("session_id", &self.session_id)
            .field("call_id", &self.call_id)
            .field("attempt_id", &self.attempt_id)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl workspace::permission::types::HumanPermissionInteractionObserver
    for TimelinePermissionObserver
{
    async fn opened(&self) -> Result<(), String> {
        let mut guard =
            crate::session::pending_interaction::PendingInteractionGuard::new_with_attempt(
                self.pending.clone(),
                self.gateway.clone(),
                self.session_id.clone(),
                self.call_id.clone(),
                crate::session::pending_interaction::PendingKind::Permission,
                self.attempt_id.clone(),
            );
        guard.open_durably(&self.chat_state).await?;
        *self.guard.lock().unwrap_or_else(|error| error.into_inner()) = Some(guard);
        Ok(())
    }

    async fn resolved(
        &self,
        outcome: workspace::permission::types::HumanInteractionResolution,
    ) -> Result<(), String> {
        let resolution = if self
            .runtime_suspend
            .load(std::sync::atomic::Ordering::Acquire)
        {
            chat_state::InteractionResolution::Suspended
        } else {
            match outcome {
                workspace::permission::types::HumanInteractionResolution::Answered => {
                    chat_state::InteractionResolution::Answered
                }
                workspace::permission::types::HumanInteractionResolution::Allowed => {
                    chat_state::InteractionResolution::Allowed
                }
                workspace::permission::types::HumanInteractionResolution::Rejected => {
                    chat_state::InteractionResolution::Rejected
                }
                workspace::permission::types::HumanInteractionResolution::Cancelled => {
                    chat_state::InteractionResolution::Cancelled
                }
                workspace::permission::types::HumanInteractionResolution::TimedOut => {
                    chat_state::InteractionResolution::TimedOut
                }
                workspace::permission::types::HumanInteractionResolution::Failed => {
                    chat_state::InteractionResolution::Failed
                }
            }
        };
        let guard = self
            .guard
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .ok_or("permission interaction resolved before it was opened")?;
        guard
            .resolve_durably(&self.chat_state, resolution)
            .await
            .map(|_| ())
    }
}

fn restored_waiting_operation(
    continuation: Option<&chat_state::session_runtime::TurnContinuation>,
    call_id: &str,
) -> Result<bool, String> {
    let Some(continuation) = continuation else {
        return Ok(false);
    };
    let mut matches = continuation
        .operations
        .iter()
        .filter(|operation| operation.call_id == call_id);
    let Some(operation) = matches.next() else {
        return Ok(false);
    };
    if matches.next().is_some() {
        return Err(format!(
            "restored turn repeats tool call identity {call_id}"
        ));
    }
    match &operation.state {
        chat_state::session_runtime::SuspendedOperationState::AwaitUser
        | chat_state::session_runtime::SuspendedOperationState::AwaitPermission => Ok(true),
        // A call that had not been dispatched has no existing ToolStarted to
        // reuse, so it still follows the ordinary new-call path.
        chat_state::session_runtime::SuspendedOperationState::NotDispatched => Ok(false),
        state => Err(format!(
            "restored tool call {call_id} is not resumable from state {state:?}"
        )),
    }
}

fn restored_pre_tool_decision_from_facts(
    continuation: Option<&chat_state::session_runtime::TurnContinuation>,
    active_tool_ids: &[String],
    call_id: &str,
    tool_name: &str,
    tool_input: &serde_json::Value,
    events: &[chat_state::TimelineEvent],
) -> Result<Option<::hooks::result::HookDecision>, String> {
    if !restored_waiting_operation(continuation, call_id)? {
        return Ok(None);
    }
    if !active_tool_ids.iter().any(|active| active == call_id) {
        return Err(format!(
            "restored waiting tool {call_id} is not active in the event tracker"
        ));
    }
    let starts: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Started {
                call_id: started_id,
                name,
                input,
                ..
            }) if started_id == call_id => Some((name.as_str(), input.as_ref())),
            _ => None,
        })
        .collect();
    if starts.len() != 1 || starts[0].0 != tool_name || starts[0].1 != Some(tool_input) {
        return Err(format!(
            "restored waiting tool {call_id} has missing, repeated, or conflicting Timeline start or input"
        ));
    }
    if events.iter().any(|event| {
        matches!(
            &event.kind,
            chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Completed {
                call_id: completed_id,
                ..
            }) if completed_id == call_id
        )
    }) {
        return Err(format!(
            "restored waiting tool {call_id} already has a Timeline completion"
        ));
    }

    let occurrences: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                occurrence_id,
                event: chat_state::HookEventType::PreToolUse,
                gate: chat_state::HookGateKind::Tool,
                cause: chat_state::HookCause::Tool { call_id: hooked_id },
                ..
            }) if hooked_id == call_id => Some(occurrence_id.as_str()),
            _ => None,
        })
        .collect();
    if occurrences.len() != 1 {
        return Err(format!(
            "restored waiting tool {call_id} has no unique PreToolUse occurrence"
        ));
    }
    let occurrence_id = occurrences[0];
    if events.iter().any(|event| {
        matches!(
            &event.kind,
            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::RunFinished {
                occurrence_id: finished_id,
                outcome: chat_state::HookRunOutcome::InterruptedOutcomeUnknown,
                ..
            }) if finished_id == occurrence_id
        )
    }) {
        return Err(format!(
            "restored waiting tool {call_id} has an unknown PreToolUse side-effect outcome"
        ));
    }
    let completions: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                occurrence_id: completed_id,
                decision,
            }) if completed_id == occurrence_id => Some(decision),
            _ => None,
        })
        .collect();
    if completions.len() != 1 {
        return Err(format!(
            "restored waiting tool {call_id} has no unique completed PreToolUse decision"
        ));
    }
    let chat_state::HookAggregateDecision::Tool { decision } = completions[0] else {
        return Err(format!(
            "restored waiting tool {call_id} has a conflicting PreToolUse decision kind"
        ));
    };
    Ok(Some(match decision {
        chat_state::HookGateDecision::Allow => ::hooks::result::HookDecision::Allow,
        chat_state::HookGateDecision::Block { reason } => ::hooks::result::HookDecision::Deny {
            hook_name: "restored_pre_tool_use".to_owned(),
            reason: reason.clone(),
        },
    }))
}

impl SessionActor {
    pub(super) async fn recovered_pre_tool_hook_decision(
        &self,
        call_id: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> Result<Option<::hooks::result::HookDecision>, String> {
        let continuation = self.turn_continuation.borrow().clone();
        if !restored_waiting_operation(continuation.as_ref(), call_id)? {
            return Ok(None);
        }
        let events = self
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or_else(|| "cannot read Timeline for restored PreToolUse decision".to_owned())?;
        restored_pre_tool_decision_from_facts(
            continuation.as_ref(),
            &self.events.active_tool_ids(),
            call_id,
            tool_name,
            tool_input,
            &events,
        )
    }

    /// Merge the canonical `grow/tool` identity envelope into a tool-call
    /// event's `_meta`, resolving the tool from the live toolset by wire name.
    pub(super) fn stamp_tool_meta(
        &self,
        existing: Option<acp::Meta>,
        wire_name: &str,
        parsed: Option<&ToolInput>,
    ) -> Option<acp::Meta> {
        let toolset = self.agent.borrow().tool_bridge().toolset();
        tools::normalization::merge_tool_meta(
            &toolset,
            existing.map(serde_json::Value::Object),
            wire_name,
            parsed,
        )
        .and_then(|v| v.as_object().cloned())
    }

    pub(super) fn stamp_tool_call_authority_meta(
        &self,
        existing: Option<acp::Meta>,
        wire_name: &str,
        parsed: &ToolInput,
        descriptor_max: tool_protocol::ToolAccess,
        required_access: tool_protocol::ToolAccess,
        permit: Option<&ToolCallPermit>,
    ) -> Option<acp::Meta> {
        let mut meta = self
            .stamp_tool_meta(existing, wire_name, Some(parsed))
            .unwrap_or_default();
        let mut authority = json!({
            "version": 1,
            "descriptor_max_access": descriptor_max,
            "required_access": required_access,
        });
        if let Some(permit) = permit {
            authority["permit"] = permit.trajectory_meta();
        }
        meta.insert("grow/tool_call".to_owned(), authority);
        Some(meta)
    }

    pub(super) async fn send_tool_call_start(
        &self,
        tool_call_id: &acp::ToolCallId,
        wire_name: &str,
        tool_call_input: ToolInput,
        descriptor_max: tool_protocol::ToolAccess,
        required_access: tool_protocol::ToolAccess,
    ) -> Result<(String, acp::ToolKind, serde_json::Value), acp::Error> {
        #[allow(unused_mut)]
        let mut raw_input = serde_json::to_value(&tool_call_input)?;
        let canonical_meta = self.stamp_tool_call_authority_meta(
            None,
            wire_name,
            &tool_call_input,
            descriptor_max,
            required_access,
            None,
        );
        let (title, kind, locations, content) = match tool_call_input {
            ToolInput::ListDir(list_dir) => (
                format!("List `{}`", list_dir.target_directory),
                acp::ToolKind::Other,
                vec![acp::ToolCallLocation::new(
                    list_dir.target_directory.clone(),
                )],
                vec![],
            ),
            ToolInput::SearchReplace(sr) => {
                let display_path = self.tool_context.cwd.join(&sr.file_path).to_path_buf();
                let meta = if !sr.old_string.is_empty() {
                    let _span = tracing::info_span!("tool.sr_line_lookup").entered();
                    self.tool_context
                        .fs
                        .read_to_string(&display_path)
                        .await
                        .ok()
                        .and_then(|file_content| {
                            let pos = file_content.find(&sr.old_string)?;
                            let line = file_content[..pos].matches('\n').count() + 1;
                            serde_json::json!({ "old_line" : line, "new_line" : line, })
                                .as_object()
                                .cloned()
                        })
                } else {
                    None
                };
                (
                    format!("Edit `{}`", sr.file_path.as_str()),
                    acp::ToolKind::Edit,
                    vec![acp::ToolCallLocation::new(sr.file_path.clone())],
                    vec![acp::ToolCallContent::from(
                        acp::Diff::new(display_path, sr.new_string)
                            .old_text(Some(sr.old_string))
                            .meta(meta),
                    )],
                )
            }
            ToolInput::Bash(bash_tool) => execute_tool_call_parts(
                &bash_tool.command,
                Some(bash_tool.description.as_str()),
                self.tool_context.cwd.as_path(),
            ),
            ToolInput::ReadFile(read_file) => {
                (
                    format!("Read `{}`", read_file.path.clone()),
                    acp::ToolKind::Read,
                    vec![
                        acp::ToolCallLocation::new(read_file.path)
                            // Same normalization as the canonical `_meta` input, so one
                            // event can't show two start lines.
                            .line(
                                tools::normalization::norm_offset_i64(read_file.offset)
                                    .map(|l| l as u32),
                            ),
                    ],
                    Vec::new(),
                )
            }
            ToolInput::TodoWrite(_) => (
                "Updating plan".to_string(),
                acp::ToolKind::Think,
                Vec::new(),
                Vec::new(),
            ),
            ToolInput::Grep(gs) => (gs.pattern.clone(), acp::ToolKind::Search, vec![], vec![]),
            ToolInput::Lsp(lsp) => {
                let title = match &lsp.file_path {
                    Some(path) => format!("Code intelligence: {} `{path}`", lsp.operation),
                    None => format!("Code intelligence: {}", lsp.operation),
                };
                (title, acp::ToolKind::Other, vec![], vec![])
            }
            ToolInput::MCPTool(mcp_tool) => (
                mcp_tool.tool_name.to_owned(),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::TaskOutput(task_output) => {
                let ids = task_output.resolved_task_ids();
                let label = match ids.as_slice() {
                    [] => "Get task output".to_string(),
                    [one] => format!("Get task output: {one}"),
                    many => format!("Get task output: {} tasks", many.len()),
                };
                (label, acp::ToolKind::Other, vec![], vec![])
            }
            ToolInput::KillTask(kill_task) => (
                format!("Kill task: {}", kill_task.task_id),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::Dynamic(_) => (wire_name.to_string(), acp::ToolKind::Other, vec![], vec![]),
            ToolInput::MemorySearch(ms) => {
                let end = ms
                    .query
                    .char_indices()
                    .nth(60)
                    .map_or(ms.query.len(), |(i, _)| i);
                let display = &ms.query[..end];
                (
                    format!("Memory search: \"{display}\""),
                    acp::ToolKind::Other,
                    vec![],
                    vec![],
                )
            }
            ToolInput::MemoryGet(mg) => (
                format!("Memory read: {}", mg.path),
                acp::ToolKind::Read,
                vec![],
                vec![],
            ),
            ToolInput::ContextRecall(input) => (
                format!("Recall context: {}", input.query),
                acp::ToolKind::Read,
                vec![],
                vec![],
            ),
            ToolInput::HashlineEdit(he) => (
                format!("Edit `{}`", he.file_path),
                acp::ToolKind::Edit,
                vec![acp::ToolCallLocation::new(he.file_path.clone())],
                vec![],
            ),
            ToolInput::Task(task) => (
                task.description.clone(),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::PlanControl(input) => (
                format!("Plan: {:?}", input.action),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::AskUserQuestion(ref ask) => {
                let title = if ask.questions.len() == 1 {
                    format!("Ask: {}", ask.questions[0].question)
                } else {
                    format!("Ask {} questions", ask.questions.len())
                };
                (title, acp::ToolKind::Other, vec![], vec![])
            }
            ToolInput::WebFetch(wf) => (
                format!("Fetch: {}", wf.url),
                acp::ToolKind::Fetch,
                vec![],
                vec![],
            ),
            ToolInput::SearchTool(st) => (
                format!("Search tools: \"{}\"", st.query),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::UseTool(ut) => (ut.tool_name.clone(), acp::ToolKind::Other, vec![], vec![]),
            ToolInput::Write(ref w) => (
                format!("Write `{}`", w.file_path),
                acp::ToolKind::Edit,
                vec![acp::ToolCallLocation::new(w.file_path.clone())],
                vec![acp::ToolCallContent::from(
                    acp::Diff::new(
                        self.tool_context.cwd.join(&w.file_path).to_path_buf(),
                        w.content.clone(),
                    )
                    .old_text(Some(String::new())),
                )],
            ),
            ToolInput::Workflow(ref w) => {
                let title = format!("Workflow: {}", w.action_label());
                (title, acp::ToolKind::Other, vec![], vec![])
            }
            ToolInput::CreateGoal(ref goal) => (
                format!("Goal: create — {}", goal.objective),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::UpdateGoal(ref ug) => {
                let title = match ug.status {
                    tools::implementations::grow_build::update_goal::GoalUpdateStatus::Complete => {
                        "Goal: complete".to_string()
                    }
                    tools::implementations::grow_build::update_goal::GoalUpdateStatus::Blocked => {
                        "Goal: blocked".to_string()
                    }
                };
                (title, acp::ToolKind::Other, vec![], vec![])
            }
            ToolInput::GetGoal(_) => (
                "Goal: read status".to_string(),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::Monitor(ref m) => (
                format!("Start monitor: {}", m.description),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::SchedulerCreate(ref sc) => {
                let title = match (&sc.task_id, &sc.interval) {
                    (Some(id), Some(interval)) => {
                        format!("Update scheduled task {id} (every {interval})")
                    }
                    (Some(id), None) => format!("Update scheduled task {id}"),
                    (None, Some(interval)) => {
                        format!("Create scheduled task (every {interval})")
                    }
                    (None, None) => "Create scheduled task".to_string(),
                };
                (title, acp::ToolKind::Other, vec![], vec![])
            }
            ToolInput::SchedulerDelete(ref sd) => (
                format!("Delete scheduled task: {}", sd.id),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::SchedulerList(_) => (
                "List scheduled tasks".to_string(),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::ListActiveSessions(_) => (
                "list_active_sessions".to_owned(),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::AskParent(_) => (
                "ask_parent → parent agent".to_owned(),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::AskSubagent(ref inquiry) => (
                format!("ask_subagent → subagent「{}」", inquiry.subagent_id),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::SendSubagentMessage(ref message) => (
                format!(
                    "send_subagent_message → {}",
                    message
                        .subagent_id
                        .as_deref()
                        .or_else(|| message
                            .reply_to
                            .as_ref()
                            .map(|reference| reference.source_session_id.as_str()))
                        .unwrap_or("agent")
                ),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::AskSession(ref inquiry) => (
                format!("ask_session: {}", inquiry.target_session_id),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
            ToolInput::GetInquiry(ref inquiry) => (
                format!("get_inquiry: {}", inquiry.inquiry_id),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
        };
        let tool_call_update = acp::ToolCallUpdate::new(
            tool_call_id.clone(),
            acp::ToolCallUpdateFields::new()
                .title(Some(title.clone()))
                .kind(Some(kind))
                .locations(Some(locations))
                .content(if content.is_empty() {
                    None
                } else {
                    Some(content)
                })
                .raw_input(Some(raw_input.clone())),
        )
        .meta(canonical_meta);
        self.send_update(acp::SessionUpdate::ToolCallUpdate(tool_call_update), None)
            .await;
        Ok((title, kind, raw_input))
    }
}

impl SessionActor {
    /// Phase 1: pre-flight (MCP, args, hooks, permission, PlanControl).
    pub(crate) async fn prepare_tool_call(
        &self,
        call: crate::sampling::types::ToolCallResponse,
        deferred_followups: &mut Vec<ConversationItem>,
    ) -> Result<ToolPreflight, acp::Error> {
        self.prepare_tool_call_with_hook_decision(call, deferred_followups, None)
            .await
    }

    pub(super) async fn prepare_tool_call_with_hook_decision(
        &self,
        call: crate::sampling::types::ToolCallResponse,
        deferred_followups: &mut Vec<ConversationItem>,
        recovered_hook_decision: Option<::hooks::result::HookDecision>,
    ) -> Result<ToolPreflight, acp::Error> {
        let tool_call_id = acp::ToolCallId::new(Arc::from(call.id.clone()));
        let model_id_str = self.current_catalog_model_id();
        tracing::info!(
            target: SESSION_LOG,
            "Model requesting tool: name='{}', call_id='{}'",
            call.function.name,
            call.id,
        );
        {
            let _span = tracing::info_span!("tool.register").entered();
            let early_raw_input =
                serde_json::from_str::<serde_json::Value>(&call.function.arguments).ok();
            let subagent_background = matches!(
                call.function.name.as_str(),
                "task" | "Task" | "spawn_subagent"
            )
            .then(|| {
                early_raw_input
                    .as_ref()
                    .and_then(|v| v.get("run_in_background"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true)
            });
            let mut meta = self.stamp_tool_meta(None, &call.function.name, None);
            if let Some(bg) = subagent_background {
                meta.get_or_insert_with(serde_json::Map::new).insert(
                    "subagentBackground".to_string(),
                    serde_json::Value::Bool(bg),
                );
            }
            self.send_update(
                acp::SessionUpdate::ToolCall(
                    acp::ToolCall::new(tool_call_id.clone(), call.function.name.clone())
                        .kind(acp::ToolKind::Other)
                        .status(acp::ToolCallStatus::Pending)
                        .raw_input(early_raw_input)
                        .meta(meta),
                ),
                None,
            )
            .await;
        }
        let mcp_parts = parse_mcp_tool_name(&call.function.name);
        let is_mcp_tool = mcp_parts.is_some();
        if is_mcp_tool && !self.mcp_state.lock().await.is_initialized() {
            match self.mcp.strategy {
                McpInitStrategy::Blocking => {
                    let _span = tracing::info_span!("tool.wait_mcp_init").entered();
                    self.wait_for_mcp_initialized().await;
                }
                McpInitStrategy::Progressive => {
                    let err = anyhow::anyhow!(
                        "Tool not available. Use search_tool to find available tools."
                    );
                    let followups = self
                        .handle_tool_error(
                            &tool_call_id,
                            &call.id,
                            &call.function.name,
                            None,
                            &err,
                            &model_id_str,
                        )
                        .await;
                    deferred_followups.extend(followups);
                    return Ok(ToolPreflight::resolved(ToolLoop::NonExistingTool));
                }
            }
        }
        let args_str = crate::session::helpers::tool_input_parsing::normalize_empty_arguments(
            &call.function.arguments,
        );
        let parse_result = serde_json::from_str::<serde_json::Value>(args_str);
        let mut concatenated_json_count: usize = 0;
        let raw_input = match &parse_result {
            Ok(value) => value.clone(),
            Err(e) => {
                if let Some(objects) =
                crate::session::helpers::tool_input_parsing::try_extract_concatenated_json_objects(
                    &call.function.arguments,
                )
            {
                let total_count = objects.len();
                if objects.is_empty() {
                    json!({ "raw": call.function.arguments.clone() })
                } else {
                    let best_match = objects[0].clone();
                    let mut selected_index = 0;
                    let mut matched_tool = false;
                    let bridge = self.agent.borrow().tool_bridge().clone();
                    for (idx, obj) in objects.iter().enumerate() {
                        if bridge
                            .try_parse(&call.function.name, obj.clone())
                            .await
                            .is_ok()
                        {
                            selected_index = idx;
                            matched_tool = true;
                            break;
                        }
                    }
                    tracing::warn!(
                        tool_name = %call.function.name,
                        call_id = %call.id,
                        total_objects = total_count,
                        selected_index,
                        matched_named_tool = matched_tool,
                        "Detected concatenated JSON in tool arguments — \
                        extracting best matching object (index {selected_index}/{total_count}). \
                        The model should use separate tool calls instead of \
                        concatenating JSON objects."
                    );
                    concatenated_json_count = total_count;
                    best_match
                }
            } else {
                tracing::warn!(
                    "Failed to parse arguments as JSON ({}), wrapping in 'raw' field",
                    e
                );
                json!({ "raw": call.function.arguments.clone() })
            }
            }
        };
        let tool_input = match self
            .agent
            .borrow()
            .tool_bridge()
            .try_parse(&call.function.name, raw_input.clone())
            .await
        {
            Ok(input) => input,
            Err(err) => {
                if err.kind == tool_runtime::ToolErrorKind::NotFound {
                    tracing::error!(
                        session_id = %self.session_info.id.0,
                        tool_name = %call.function.name,
                        model_id = %model_id_str,
                        error_kind = "tool_not_found",
                        "tool_error: tool_not_found"
                    );
                    self.signals_handle()
                        .record_tool_failure(&call.function.name);
                    self.handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        format!(
                            "Tool `{}` is not available in this request and was not executed. Use an exact tool name from the supplied tool list.",
                            call.function.name
                        ),
                    )
                    .await?;
                    return Ok(ToolPreflight::resolved(ToolLoop::NonExistingTool));
                }
                self.handle_tool_parse_error(
                    &tool_call_id,
                    &call.id,
                    &call.function.name,
                    err,
                    &call.function.arguments,
                    &model_id_str,
                )
                .await?;
                return Ok(ToolPreflight::resolved(ToolLoop::ToolParsingError));
            }
        };
        let (tool_kind, descriptor_max) = {
            let agent = self.agent.borrow();
            let bridge = agent.tool_bridge();
            (
                bridge.tool_kind(&call.function.name),
                bridge
                    .max_access(&call.function.name)
                    .unwrap_or(tool_protocol::ToolAccess::All),
            )
        };
        let skill_selection = if tool_kind == Some(tools::types::tool::ToolKind::Skill) {
            let bridge = self.agent.borrow().tool_bridge().clone();
            let input: tools::implementations::grow_build::load_skill::LoadSkillInput =
                serde_json::from_value(match &tool_input {
                    ToolInput::Dynamic(value) => value.clone(),
                    _ => {
                        return Err(acp::Error::internal_error()
                            .data("Skill loader has an invalid input projection"));
                    }
                })?;
            let user_selection = super::super::skill_loading::user_skill_selection();
            let user_invocation = user_selection.is_some();
            match bridge.resolve_skill(&input.name, user_invocation).await {
                Ok(skill) => {
                    let resource_path = match tools::implementations::grow_build::load_skill::resolve_skill_resource(&skill, input.resource.as_deref()).await {
                        Ok(path) => path,
                        Err(error) => {
                            self.handle_tool_not_executed(&call.id, &tool_call_id, error.to_string()).await?;
                            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                        }
                    };
                    if let Some(frozen) = user_selection {
                        if skill.path != frozen.skill.path
                            || skill.plugin_name != frozen.skill.plugin_name
                            || skill.plugin_root != frozen.skill.plugin_root
                            || skill.plugin_data != frozen.skill.plugin_data
                            || skill.scope != frozen.skill.scope
                        {
                            self.handle_tool_not_executed(
                                &call.id,
                                &tool_call_id,
                                "The selected skill source changed before loading".into(),
                            )
                            .await?;
                            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                        }
                        Some(frozen)
                    } else {
                        Some(
                            tools::implementations::grow_build::load_skill::SkillLoadSelection {
                                skill,
                                user_invocation: false,
                                resource_path,
                            },
                        )
                    }
                }
                Err(error) => {
                    self.handle_tool_not_executed(&call.id, &tool_call_id, error.to_string())
                        .await?;
                    return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                }
            }
        } else {
            None
        };
        let access_kind = skill_selection.as_ref().map_or_else(
            || AccessKind::from_tool_call(&call.function.name, &tool_input),
            |selected| {
                AccessKind::Read(
                    selected
                        .resource_path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned())
                        .or_else(|| {
                            (selected.skill.body.is_none() && !selected.skill.path.contains("://"))
                                .then(|| selected.skill.path.clone())
                        }),
                )
            },
        );
        let mcp_max_access = mcp_call_max_access(&self.mcp_state, &access_kind).await;
        let required_access = project_call_access(&tool_input, descriptor_max, mcp_max_access);
        if !descriptor_max.covers(required_access) {
            let message = format!(
                "Rejected: tool descriptor contract violation for `{}`: call requires {:?}, descriptor ceiling is {:?}.",
                call.function.name, required_access, descriptor_max
            );
            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                .await?;
            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
        }
        let within_capability_fence = if let Some(capabilities) = &self.subagent_capabilities {
            let mcp_target = match &tool_input {
                ToolInput::UseTool(input) => Some(input.tool_name.as_str()),
                ToolInput::MCPTool(input) => Some(input.tool_name.as_str()),
                _ => None,
            };
            let hard_eligible = mcp_target.map_or_else(
                || capabilities.native_call_eligible(&call.function.name, required_access),
                |target| capabilities.mcp_tool_eligible(target),
            );
            if !hard_eligible {
                let message = if mcp_target.is_some() {
                    "Rejected: this exact MCP target is outside the subagent's inherited transport eligibility or the live binding changed; it cannot be approved on this child. Use ask_parent to report the concrete dependency."
                } else {
                    "Rejected: this exact tool identity is outside the subagent's authored eligibility ceiling or immutable child policy; it cannot be approved on this child. Use ask_parent to report the concrete need so the parent can handle it or reassign the task."
                };
                self.handle_tool_not_executed(&call.id, &tool_call_id, message.to_owned())
                    .await?;
                return Ok(ToolPreflight::resolved(ToolLoop::Continue));
            }
            mcp_target.map_or_else(
                || capabilities.native_call_available(&call.function.name, required_access),
                |target| capabilities.mcp_tool_available(target, required_access),
            )
        } else {
            false
        };
        let admitted_behavior = *self.turn_behavior.lock();
        let session_dir = &self.session_dir;
        let cwd = self.tool_context.cwd.as_path();
        let display_cwd = self.display_cwd.get().map(std::path::Path::new);
        let saved_workflow_write = saved_workflow_definition_write(&access_kind, cwd, display_cwd);
        let workflow_draft_write =
            session_workflow_definition_write(&access_kind, &session_dir, cwd, display_cwd);
        let current_behavior = self.behavior.lock().behavior();
        if let Some(conflict) = matches!(tool_input, ToolInput::Workflow(_))
            .then(|| public_workflow_conflict(admitted_behavior, current_behavior))
            .flatten()
        {
            let message = format!(
                "Rejected: public Workflow cannot run inside {} behavior.",
                conflict.display_label()
            );
            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                .await?;
            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
        }
        if (saved_workflow_write || workflow_draft_write)
            && (admitted_behavior != tool_types::BehaviorId::Workflow
                || current_behavior != tool_types::BehaviorId::Workflow)
        {
            let message = "Rejected: Grow may create or modify public Workflow Definitions only in Workflow behavior. Use /workflow [prompt]. External editor changes remain allowed and will be rediscovered."
                .to_string();
            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                .await?;
            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
        }
        if saved_workflow_write {
            let message = "Rejected: saved Workflow Definitions are replaced only by publishing a validated session draft. Derive the saved Definition into the Workflow workspace, edit the draft, then publish it; the saved Definition remains usable until then."
                .to_string();
            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                .await?;
            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
        }
        if workflow_run_snapshot_write(&access_kind, &session_dir, cwd, display_cwd) {
            let message = "Rejected: Workflow Run scripts, args, and journals are immutable snapshots. Modify or derive the Definition and start a new Run instead."
                .to_string();
            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                .await?;
            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
        }
        // Lock order: resolve the read-only MCP classification from the
        // (async) `mcp_state` BEFORE taking the `behavior` lock — never hold
        // one lock while awaiting the other.
        let plan_gate = if admitted_behavior == tool_types::BehaviorId::Plan {
            plan_mode_edit_gate(
                &self.behavior.lock(),
                &tool_input,
                &access_kind,
                mcp_max_access,
            )
        } else {
            PlanEditGate::Allow
        };
        if plan_gate != PlanEditGate::Allow {
            tracing::info_span!(
                "tool.decision",
                tool_name = %call.function.name,
                tool_use_id = %call.id,
                decision = "deny",
                source = "plan_mode",
                wait_ms = 0_i64,
            )
            .in_scope(|| {});
            let msg = match plan_gate {
                PlanEditGate::RejectWorkflow => "Rejected: Workflow cannot be launched while Plan behavior is active. Complete or cancel the approved Plan first.".to_owned(),
                PlanEditGate::RejectEdit => self.plan_mode_edit_rejected_message().await,
                PlanEditGate::Allow => unreachable!(),
            };
            self.handle_tool_not_executed(&call.id, &tool_call_id, msg)
                .await?;
            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
        }
        let tool_call_display = self
            .send_tool_call_start(
                &tool_call_id,
                &call.function.name,
                tool_input.clone(),
                descriptor_max,
                required_access,
            )
            .await;
        let _recovered_raw_input = if concatenated_json_count > 0 {
            Some(raw_input.clone())
        } else {
            None
        };
        let dispatch_target_name = tool_input.dispatch_target_name();
        let resolved_tool_name = dispatch_target_name
            .clone()
            .unwrap_or_else(|| call.function.name.clone());
        let (hook_tool_input, hook_tool_input_truncated) =
            ::hooks::event::truncate_payload(raw_input.clone());
        let envelope = self.make_hook_envelope(
            ::hooks::event::HookEventName::PreToolUse,
            None,
            ::hooks::event::HookPayload::PreToolUse {
                tool_name: resolved_tool_name.clone(),
                tool_use_id: call.id.clone(),
                tool_input: hook_tool_input,
                tool_input_truncated: hook_tool_input_truncated,
                subagent_type: self.subagent_type_label(),
            },
        );
        let decision = if let Some(decision) = recovered_hook_decision {
            decision
        } else {
            self.dispatch_hook_occurrence(
                ::hooks::event::HookEventName::PreToolUse,
                chat_state::HookCause::Tool {
                    call_id: call.id.clone(),
                },
                envelope,
                ::hooks::event::GateKind::Tool,
                super::super::hook_dispatch::HookDispatchPolicy::Execute,
            )
            .await
            .map_err(|error| {
                acp::Error::internal_error()
                    .data(format!("pre-tool hook lifecycle was not durable: {error}"))
            })?
            .into_tool_decision()
        };
        if let ::hooks::result::HookDecision::Deny { hook_name, reason } = decision {
            return Ok(ToolPreflight::resolved(
                self.deny_tool(&call.id, &tool_call_id, hook_name, reason)
                    .await?,
            ));
        }
        {
            let (perm_title, perm_kind, perm_raw_input) = tool_call_display
                .as_ref()
                .map(|(t, k, r)| (Some(t.clone()), Some(*k), Some(r.clone())))
                .unwrap_or((None, None, None));
            let tool_call_update = acp::ToolCallUpdate::new(
                tool_call_id.clone(),
                acp::ToolCallUpdateFields::new()
                    .title(perm_title)
                    .kind(perm_kind)
                    .raw_input(perm_raw_input),
            )
            .meta(self.stamp_tool_call_authority_meta(
                None,
                &call.function.name,
                &tool_input,
                descriptor_max,
                required_access,
                None,
            ));
            let (diagnostics_access_kind, _access_detail) = match &access_kind {
                workspace::permission::AccessKind::Read(p) => (
                    ::diagnostics::events::AccessKind::Read,
                    p.clone().unwrap_or_default(),
                ),
                workspace::permission::AccessKind::Edit(p) => {
                    (::diagnostics::events::AccessKind::Edit, p.clone())
                }
                workspace::permission::AccessKind::Bash(cmd) => {
                    (::diagnostics::events::AccessKind::Bash, cmd.clone())
                }
                workspace::permission::AccessKind::Grep { path, glob } => (
                    ::diagnostics::events::AccessKind::Grep,
                    path.clone().or_else(|| glob.clone()).unwrap_or_default(),
                ),
                workspace::permission::AccessKind::MCPTool { name, .. } => {
                    (::diagnostics::events::AccessKind::Mcp, name.clone())
                }
                workspace::permission::AccessKind::WebFetch(u) => {
                    (::diagnostics::events::AccessKind::Web, u.clone())
                }
                workspace::permission::AccessKind::InternalControl { name } => {
                    (::diagnostics::events::AccessKind::Control, name.clone())
                }
            };
            let subagent_session_id = if self.startup_hints.is_subagent {
                Some(self.session_id_string())
            } else {
                None
            };
            let diagnostic_subagent_type = self.subagent_type_label();
            let child_permission_mode = self.startup_hints.permission_request_mode();
            let effective_mode = self
                .permissions
                .effective_request_mode(child_permission_mode);
            let perm_mode = match effective_mode {
                workspace::permission::types::EffectivePermissionMode::AlwaysApprove => {
                    ::diagnostics::enums::PermissionMode::AlwaysApprove
                }
                workspace::permission::types::EffectivePermissionMode::Auto => {
                    ::diagnostics::enums::PermissionMode::Auto
                }
                workspace::permission::types::EffectivePermissionMode::Ask => {
                    ::diagnostics::enums::PermissionMode::Ask
                }
            };
            let emit_permission_diagnostics = !within_capability_fence;
            let perm_start = if emit_permission_diagnostics {
                ::diagnostics::session_ctx::log_event(::diagnostics::events::PermissionPrompted {
                    tool_name: call.function.name.clone(),
                    access_kind: diagnostics_access_kind,
                    permission_mode: perm_mode,
                    subagent_session_id: subagent_session_id.clone(),
                    subagent_type: diagnostic_subagent_type.clone(),
                });
                self.events.permission_requested(&call.function.name)
            } else {
                std::time::Instant::now()
            };
            debug_assert!(
                !self.session_info.id.0.is_empty(),
                "permission reverse-request must carry a non-empty sessionId (design §5.4)"
            );
            if effective_mode
                != workspace::permission::types::EffectivePermissionMode::AlwaysApprove
                && !within_capability_fence
            {
                self.dispatch_notification_hook(
                    "permission_prompt",
                    Some("Tool permission requested".into()),
                    None,
                    Some("info".into()),
                )
                .await
                .map_err(|error| {
                    acp::Error::internal_error().data(format!(
                        "permission notification hook lifecycle was not durable: {error}"
                    ))
                })?;
            }
            let classifier_turns = if effective_mode
                == workspace::permission::types::EffectivePermissionMode::Auto
                && !within_capability_fence
            {
                let authority_context = self
                    .chat_state_handle
                    .materialize_timeline(self.session_info.id.to_string())
                    .await
                    .map(|materialized| materialized.permission_context)
                    .unwrap_or_default();
                let turns = super::build_classifier_turns(
                    &authority_context,
                    super::CLASSIFIER_REFRESH_TURNS,
                );
                Some(turns)
            } else {
                None
            };
            let edit_path_context = matches!(&access_kind, AccessKind::Edit(_)).then(|| {
                workspace::permission::types::EditPathContext {
                    real_cwd: std::path::PathBuf::from(self.session_info.cwd.as_str()),
                    display_cwd: self
                        .display_cwd
                        .get()
                        .map(|cwd| std::path::PathBuf::from(cwd.as_str())),
                }
            });
            let runtime_suspend = self.state.lock().await.runtime_suspend.clone();
            let decision = self
                .permissions
                .request_with_context(
                    access_kind.clone(),
                    tool_call_update,
                    edit_path_context,
                    workspace::permission::types::PermissionRequestContext {
                        source: if self.startup_hints.is_subagent {
                            workspace::permission::types::PermissionRequestSource::Child {
                                session_id: self.session_info.id.0.to_string(),
                                subagent_type: self.subagent_type_label(),
                                subagent_description: self
                                    .startup_hints
                                    .subagent_description
                                    .clone(),
                            }
                        } else {
                            workspace::permission::types::PermissionRequestSource::Primary {
                                session_id: Some(self.session_info.id.0.to_string()),
                            }
                        },
                        request_mode: child_permission_mode,
                        within_capability_fence,
                        execution_cwd: Some(std::path::PathBuf::from(
                            self.session_info.cwd.as_str(),
                        )),
                        classifier_turns,
                        call_evidence: self.startup_hints.is_subagent.then(|| {
                            permission_call_evidence(&call.function.name, &raw_input, &tool_input)
                        }),
                        human_interaction: Some(std::sync::Arc::new(TimelinePermissionObserver {
                            pending: self.pending_interactions.clone(),
                            gateway: self.notifications.gateway.clone(),
                            session_id: self.session_info.id.clone(),
                            call_id: tool_call_id.to_string(),
                            attempt_id: uuid::Uuid::now_v7().to_string(),
                            chat_state: self.chat_state_handle.clone(),
                            runtime_suspend: runtime_suspend.clone(),
                            guard: std::sync::Mutex::new(None),
                        })),
                    },
                )
                .await;
            if emit_permission_diagnostics {
                self.events.permission_resolved(
                    &call.function.name,
                    {
                        use crate::session::event_types::PermissionDecision;
                        match &decision {
                            Decision::Allow | Decision::Ask => PermissionDecision::Allow,
                            Decision::Reject(_) | Decision::PolicyDeny(_) => {
                                PermissionDecision::Deny
                            }
                            Decision::Cancelled => PermissionDecision::Cancelled,
                            Decision::TimedOut => PermissionDecision::TimedOut,
                            Decision::FollowupMessage(_) => PermissionDecision::Followup,
                        }
                    },
                    perm_start,
                );
            }
            let wait_ms = perm_start.elapsed().as_millis() as u64;
            let (decision_outcome, _reject_reason) = match &decision {
                Decision::Allow | Decision::Ask => {
                    (::diagnostics::events::PermissionOutcome::Allow, None)
                }
                Decision::Reject(reason) | Decision::PolicyDeny(reason) => (
                    ::diagnostics::events::PermissionOutcome::Deny,
                    Some(reason.to_string()),
                ),
                Decision::Cancelled => (::diagnostics::events::PermissionOutcome::Cancelled, None),
                Decision::TimedOut => (::diagnostics::events::PermissionOutcome::TimedOut, None),
                Decision::FollowupMessage(_) => {
                    (::diagnostics::events::PermissionOutcome::Followup, None)
                }
            };
            tracing::info_span!(
                "tool.decision",
                tool_name = %call.function.name,
                tool_use_id = %call.id,
                decision = decision_outcome.as_str(),
                source = crate::session::diagnostics::permission_decision_source(
                    &decision,
                    effective_mode
                        == workspace::permission::types::EffectivePermissionMode::AlwaysApprove,
                ),
                wait_ms = wait_ms as i64,
            )
            .in_scope(|| {});
            if emit_permission_diagnostics {
                ::diagnostics::session_ctx::log_event(
                    ::diagnostics::events::PermissionDecisionPayload {
                        tool_name: call.function.name.clone(),
                        access_kind: diagnostics_access_kind,
                        decision: decision_outcome,
                        wait_ms,
                        permission_mode: perm_mode,
                        source: Some(
                            crate::session::diagnostics::permission_decision_source(
                                &decision,
                                effective_mode
                                    == workspace::permission::types::EffectivePermissionMode::AlwaysApprove,
                            )
                            .to_owned(),
                        ),
                        subagent_session_id: subagent_session_id.clone(),
                        subagent_type: diagnostic_subagent_type,
                    },
                );
            }
            match decision {
                Decision::PolicyDeny(ref reason) | Decision::Reject(ref reason) => {
                    let is_policy_deny = matches!(&decision, Decision::PolicyDeny(_));
                    let child_nonterminal = self.startup_hints.is_subagent;
                    let mut message = if is_policy_deny {
                        format!("Tool `{}` was not executed: {reason}", call.function.name)
                    } else {
                        format!("{reason} for tool `{}`", call.function.name)
                    };
                    if child_nonterminal {
                        message.push_str(
                            ". Continue with the tools and permissions that remain available. \
                             Do not retry this exact action blindly; if the missing permission \
                             prevents completion, explain the limitation in your final report",
                        );
                    }
                    self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                        .await?;
                    let (tool_input_value, tool_input_truncated) =
                        ::hooks::event::truncate_payload(raw_input.clone());
                    let permission_denied_hook = DeferredObserveHook {
                        event: ::hooks::event::HookEventName::PermissionDenied,
                        cause: chat_state::HookCause::Tool {
                            call_id: call.id.clone(),
                        },
                        payload: ::hooks::event::HookPayload::PermissionDenied {
                            tool_name: resolved_tool_name.clone(),
                            tool_use_id: tool_call_id.to_string(),
                            tool_input: tool_input_value,
                            tool_input_truncated,
                        },
                        prompt_id: None,
                    };
                    let loop_action = if is_policy_deny || child_nonterminal {
                        ToolLoop::Continue
                    } else {
                        ToolLoop::PermissionReject {
                            tool_name: call.function.name.clone(),
                            reason: reason.clone(),
                        }
                    };
                    return Ok(ToolPreflight::resolved_with_post_terminal_hook(
                        loop_action,
                        permission_denied_hook,
                    ));
                }
                Decision::Cancelled => {
                    let message = format!(
                        "User cancelled the execution for tool `{}`",
                        call.function.name
                    );
                    self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                        .await?;
                    return Ok(ToolPreflight::resolved(ToolLoop::Cancelled));
                }
                Decision::TimedOut => {
                    let child_nonterminal = self.startup_hints.is_subagent;
                    let message = if child_nonterminal {
                        format!(
                            "Permission request timed out; tool `{}` was not executed. \
                             Continue with the tools and permissions that remain available. \
                             Do not retry this exact action blindly; if the missing permission \
                             prevents completion, explain the limitation in your final report",
                            call.function.name
                        )
                    } else {
                        format!(
                            "Permission request timed out; tool `{}` was not executed",
                            call.function.name
                        )
                    };
                    self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                        .await?;
                    if child_nonterminal {
                        return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                    }
                    return Ok(ToolPreflight::resolved(ToolLoop::PermissionTimedOut {
                        tool_name: call.function.name.clone(),
                    }));
                }
                Decision::FollowupMessage(followup_message) => {
                    let message = format!(
                        "The user elected to avoid running the {} tool. The tool was not executed. \
                         Please refer to the user's message for next steps.",
                        call.function.name
                    );
                    self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                        .await?;
                    return Ok(ToolPreflight::resolved(ToolLoop::FollowupMessage(
                        followup_message,
                    )));
                }
                Decision::Allow | Decision::Ask => {}
            }
        }
        if let ToolInput::PlanControl(input) = &tool_input {
            use tools::implementations::grow_build::plan_control::{
                PlanApprovalOutcome, PlanControlAction,
            };
            if let Err(message) = input.validate() {
                self.handle_tool_not_executed(
                    &call.id,
                    &tool_call_id,
                    format!("Rejected: {message}."),
                )
                .await?;
                return Ok(ToolPreflight::resolved(ToolLoop::Continue));
            }
            if matches!(
                input.action,
                PlanControlAction::Complete | PlanControlAction::Cancel
            ) {
                let valid = match input.action {
                    PlanControlAction::Complete => {
                        self.behavior.lock().state()
                            == crate::session::behavior::BehaviorState::Plan(
                                crate::session::behavior::PlanPhase::Executing,
                            )
                    }
                    PlanControlAction::Cancel => self.behavior.lock().is_plan(),
                    PlanControlAction::Submit | PlanControlAction::Amend => unreachable!(),
                };
                if !valid {
                    self.handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        format!(
                            "Rejected: Plan action `{:?}` is not valid in the current phase.",
                            input.action
                        ),
                    )
                    .await?;
                    return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                }
            } else {
                // A checkpoint retains the original submitted tool call. Rebind
                // that approval against its committed submission instead of
                // submitting again or rejecting AwaitingApproval as Drafting.
                let restoring_submission = self
                    .turn_continuation
                    .borrow()
                    .as_ref()
                    .and_then(|turn| {
                        turn.operations
                            .iter()
                            .find(|operation| operation.call_id == call.id)
                    })
                    .is_some_and(|operation| {
                        matches!(
                            operation.state,
                            chat_state::session_runtime::SuspendedOperationState::AwaitUser
                        )
                    });
                let valid = match input.action {
                    PlanControlAction::Submit => self.behavior.lock().is_drafting_plan(),
                    PlanControlAction::Amend => {
                        matches!(
                            self.behavior.lock().state(),
                            crate::session::behavior::BehaviorState::Plan(
                                crate::session::behavior::PlanPhase::Executing
                                    | crate::session::behavior::PlanPhase::Amending
                            )
                        )
                    }
                    PlanControlAction::Complete | PlanControlAction::Cancel => unreachable!(),
                };
                if !valid && !restoring_submission {
                    self.handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        format!(
                            "Rejected: Plan action `{:?}` is not valid in the current phase.",
                            input.action
                        ),
                    )
                    .await?;
                    return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                }
                let plan_content = input
                    .plan
                    .as_deref()
                    .expect("validated submit/amend plan")
                    .trim()
                    .to_owned();

                if restoring_submission {
                    let snapshot = self.behavior.lock().snapshot();
                    let expected_phase = match input.action {
                        PlanControlAction::Submit => {
                            crate::session::behavior::PlanPhase::AwaitingApproval
                        }
                        PlanControlAction::Amend => crate::session::behavior::PlanPhase::Amending,
                        _ => unreachable!(),
                    };
                    if !snapshot.approval_pending
                        || snapshot.state
                            != crate::session::behavior::BehaviorState::Plan(expected_phase)
                        || snapshot.plan_submission_id.is_none()
                        || !self.behavior.lock().plan_artifact_is_valid(&plan_content)
                    {
                        return Err(crate::session::commands::fatal_turn_boundary_error(
                            "plan-approval-restore",
                            "restored Plan call conflicts with the committed pending submission",
                        ));
                    }
                    let body = read_plan_artifact_async(
                        self.session_directory.clone(),
                        snapshot
                            .plan_artifact_hash
                            .expect("validated artifact hash"),
                    )
                    .await
                    .map_err(|error| {
                        crate::session::commands::fatal_turn_boundary_error(
                            "plan-approval-restore",
                            error.to_string(),
                        )
                    })?;
                    if body != plan_content {
                        return Err(crate::session::commands::fatal_turn_boundary_error(
                            "plan-approval-restore",
                            "restored Plan body differs from its artifact",
                        ));
                    }
                } else {
                    // Persist the control-plane artifact before opening approval UI.
                    // This is not a workspace edit and does not grant the Agent an
                    // Edit tool.
                    let artifact_hash = match write_plan_artifact_async(
                        self.session_directory.clone(),
                        plan_content.clone(),
                    )
                    .await
                    {
                        Ok(hash) => hash,
                        Err(error) => {
                            tracing::warn!(
                                %error,
                                "failed to persist submitted plan before approval"
                            );
                            self.handle_tool_not_executed(
                                &call.id,
                                &tool_call_id,
                                format!("Failed to persist the plan artifact: {error}"),
                            )
                            .await?;
                            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                        }
                    };
                    let previous_behavior = self.behavior.lock().snapshot();
                    self.behavior.lock().record_plan_artifact(&plan_content);
                    debug_assert_eq!(
                        self.behavior.lock().plan_artifact_hash(),
                        Some(artifact_hash.as_str())
                    );
                    let submitted = match input.action {
                        PlanControlAction::Submit => self.behavior.lock().submit_initial_plan(),
                        PlanControlAction::Amend => self.behavior.lock().submit_plan_amendment(),
                        PlanControlAction::Complete | PlanControlAction::Cancel => unreachable!(),
                    };
                    if !submitted {
                        self.handle_tool_not_executed(
                    &call.id,
                    &tool_call_id,
                    "Rejected: a plan can only be submitted while drafting or while executing an approved plan that needs amendment.".to_owned(),
                )
                .await?;
                        return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                    }
                    if let Err(message) = self
                        .commit_behavior_mutation_or_restore(previous_behavior)
                        .await
                    {
                        self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                            .await?;
                        return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                    }
                }
                let approval_snapshot = self.behavior.lock().snapshot();

                tracing::info!(
                    tool_call_id = %tool_call_id,
                    "plan_control intercepted; requesting user approval"
                );
                let resp = self
                    .request_plan_approval(&tool_call_id, plan_content.clone())
                    .await;
                match resp {
                    Ok(parsed) => {
                        if let Err(message) =
                            self.accept_plan_decision(&approval_snapshot, &parsed).await
                        {
                            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                                .await?;
                            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                        }
                        match parsed.outcome {
                            PlanApprovalOutcome::Abandoned => {
                                tracing::info!("plan_control: user abandoned Plan");
                                self.enqueue_current_mode_update(acp::SessionModeId::new(
                                    tool_types::BehaviorId::Normal.as_id(),
                                ));
                                self.send_available_commands_update().await;
                                let message = format!(
                                    "The user chose to abandon the plan entirely (via the Abandon option in the plan approval dialog). Plan mode has been disabled. Do not call {} again unless the user explicitly asks to re-enter plan mode.",
                                    call.function.name
                                );
                                let tool_update = acp::ToolCallUpdate::new(
                                    tool_call_id.clone(),
                                    acp::ToolCallUpdateFields::new()
                                        .status(Some(acp::ToolCallStatus::Completed))
                                        .content(Some(vec![acp::ToolCallContent::from(
                                            acp::ContentBlock::Text(acp::TextContent::new(
                                                message.clone(),
                                            )),
                                        )])),
                                );
                                self.send_update(
                                    acp::SessionUpdate::ToolCallUpdate(tool_update),
                                    None,
                                )
                                .await;
                                let tool_chat =
                                    ConversationItem::tool_result(call.id.clone(), message);
                                self.chat_state_handle.push_tool_result(tool_chat);
                                return Ok(ToolPreflight::resolved(ToolLoop::Control(
                                    ControlDisposition::EndTurn,
                                )));
                            }
                            PlanApprovalOutcome::Cancelled => {
                                let next = self.behavior.lock().snapshot();
                                if let Err(error) =
                                    self.admit_plan_handoff_notification(&next).await
                                {
                                    tracing::warn!(%error, "Plan revision handoff receipt will be reconciled after restore");
                                }
                                let message = "The Plan is now in the revision phase.".to_owned();
                                let tool_update = acp::ToolCallUpdate::new(
                                    tool_call_id.clone(),
                                    acp::ToolCallUpdateFields::new()
                                        .status(Some(acp::ToolCallStatus::Completed))
                                        .content(Some(vec![acp::ToolCallContent::from(
                                            acp::ContentBlock::Text(acp::TextContent::new(
                                                message.clone(),
                                            )),
                                        )])),
                                );
                                self.send_update(
                                    acp::SessionUpdate::ToolCallUpdate(tool_update),
                                    None,
                                )
                                .await;
                                let tool_chat =
                                    ConversationItem::tool_result(call.id.clone(), message);
                                self.chat_state_handle.push_tool_result(tool_chat);
                                return Ok(ToolPreflight::resolved(ToolLoop::Control(
                                    ControlDisposition::ResampleStep,
                                )));
                            }
                            PlanApprovalOutcome::Approved => {
                                let next = self.behavior.lock().snapshot();
                                if let Err(error) =
                                    self.admit_plan_handoff_notification(&next).await
                                {
                                    tracing::warn!(%error, "Plan execution handoff receipt will be reconciled after restore");
                                }
                                self.enqueue_current_mode_update(acp::SessionModeId::new(
                                    tools::types::BehaviorId::Plan.as_id(),
                                ));
                                tracing::info!("plan_control: user approved frozen Plan contract");
                            }
                            PlanApprovalOutcome::Deferred => {
                                let message = "The user retained this Plan for later review. Execution has not been approved. This planning interaction ends now; do not resubmit or continue execution until the user explicitly reviews it again.".to_owned();
                                self.chat_state_handle
                                    .push_tool_result_durably(ConversationItem::tool_result(
                                        call.id.clone(),
                                        message.clone(),
                                    ))
                                    .await
                                    .map_err(|error| {
                                        crate::session::commands::fatal_turn_boundary_error(
                                            "Plan defer result",
                                            error.to_string(),
                                        )
                                    })?;
                                let update = acp::ToolCallUpdate::new(
                                    tool_call_id.clone(),
                                    acp::ToolCallUpdateFields::new()
                                        .status(Some(acp::ToolCallStatus::Completed))
                                        .content(Some(vec![acp::ToolCallContent::from(
                                            acp::ContentBlock::Text(acp::TextContent::new(message)),
                                        )])),
                                );
                                self.send_update(acp::SessionUpdate::ToolCallUpdate(update), None)
                                    .await;
                                return Ok(ToolPreflight::resolved(ToolLoop::Control(
                                    ControlDisposition::EndTurn,
                                )));
                            }
                        }
                    }
                    Err(err) => {
                        if ext_method_no_client(&err) {
                            tracing::warn!(%err, "plan_control: no approval client; failing closed");
                            let previous_behavior = self.behavior.lock().snapshot();
                            if !self
                                .behavior
                                .lock()
                                .fail_pending_plan_approval_if(&approval_snapshot)
                            {
                                tracing::info!("plan_control: dropping stale no-client decision");
                                self.handle_tool_not_executed(
                                    &call.id,
                                    &tool_call_id,
                                    "Plan approval was stale and was discarded.".to_owned(),
                                )
                                .await?;
                                return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                            }
                            if let Err(message) = self
                                .commit_behavior_mutation_or_restore(previous_behavior)
                                .await
                            {
                                self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                                    .await?;
                                return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                            }
                            self.handle_tool_not_executed(
                            &call.id,
                            &tool_call_id,
                            "Plan approval requires an interactive user. No approval client is connected, so execution remains blocked.".to_owned(),
                        )
                        .await?;
                            return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                        } else if matches!(
                            acp_transport::acp_channel_failure(&err),
                            Some(acp_transport::AcpChannelFailure::RecvFailed)
                        ) {
                            tracing::info!(
                                %err,
                                "plan_control: client disconnected mid-approval; Plan stays active"
                            );
                            let message = "Plan approval could not be completed because the \
                             client disconnected. Plan mode remains active; the approval \
                             will reappear on reconnect."
                                .to_string();
                            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                                .await?;
                            return Ok(ToolPreflight::resolved(ToolLoop::Cancelled));
                        } else {
                            tracing::warn!(
                                %err,
                                "plan_control: client returned an invalid approval response"
                            );
                            let message = "Plan approval could not be completed because the client returned an invalid response. Plan mode remains active and no approval decision was applied.".to_string();
                            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                                .await?;
                            return Ok(ToolPreflight::resolved(ToolLoop::Cancelled));
                        }
                    }
                }
            }
        }
        let permit = match self
            .issue_tool_call_permit(
                &call.id,
                &call.function.name,
                dispatch_target_name.clone(),
                &raw_input,
                &access_kind,
                descriptor_max,
                required_access,
            )
            .await
        {
            Ok(permit) => permit,
            Err(message) => {
                self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                    .await?;
                return Ok(ToolPreflight::resolved(ToolLoop::Continue));
            }
        };
        let success_control = match &tool_input {
            ToolInput::PlanControl(input) => {
                use tools::implementations::grow_build::plan_control::PlanControlAction;
                Some(match input.action {
                    PlanControlAction::Submit
                    | PlanControlAction::Amend
                    | PlanControlAction::Complete => ControlDisposition::ResampleStep,
                    PlanControlAction::Cancel => ControlDisposition::EndTurn,
                })
            }
            // Preserve the existing Goal lifecycle fence explicitly. Goal may
            // refine its per-action continuation independently of Plan.
            ToolInput::CreateGoal(_) | ToolInput::UpdateGoal(_) => {
                Some(ControlDisposition::EndTurn)
            }
            _ => None,
        };
        let plan_exit_on_success = match &tool_input {
            ToolInput::PlanControl(input)
                if matches!(
                    input.action,
                    tools::implementations::grow_build::plan_control::PlanControlAction::Complete
                        | tools::implementations::grow_build::plan_control::PlanControlAction::Cancel
                ) => Some(self.behavior.lock().snapshot()),
            _ => None,
        };
        self.send_update(
            acp::SessionUpdate::ToolCallUpdate(
                acp::ToolCallUpdate::new(tool_call_id.clone(), acp::ToolCallUpdateFields::new())
                    .meta(self.stamp_tool_call_authority_meta(
                        None,
                        &call.function.name,
                        &tool_input,
                        descriptor_max,
                        required_access,
                        Some(&permit),
                    )),
            ),
            None,
        )
        .await;
        let prepared = PreparedToolCall {
            skill_selection,
            call_id: call.id.clone(),
            tool_call_id,
            tool_name: call.function.name.clone(),
            raw_arguments: call.function.arguments.clone(),
            parsed_args: raw_input.clone(),
            model_id: model_id_str,
            concatenated_json_count,
            dispatch_target_name,
            required_access,
            permit,
            workflow_draft_write,
            success_control,
            plan_exit_on_success,
        };
        Ok(ToolPreflight::Dispatch(prepared))
    }
}

#[cfg(test)]
mod coordination_tool_start_tests {
    use super::*;

    async fn assert_tool_start(
        actor: &SessionActor,
        call_id: &str,
        wire_name: &str,
        input: ToolInput,
        expected_title_fragments: &[&str],
        descriptor_access: tool_protocol::ToolAccess,
        expected_fields: &[(&str, &str)],
    ) {
        let expected_raw_input = serde_json::to_value(&input).expect("tool input serializes");
        let (title, kind, raw_input) = actor
            .send_tool_call_start(
                &acp::ToolCallId::new(call_id),
                wire_name,
                input,
                descriptor_access,
                descriptor_access,
            )
            .await
            .expect("tool start update succeeds");

        for fragment in expected_title_fragments {
            assert!(
                title.contains(fragment),
                "tool title {title:?} should contain {fragment:?}"
            );
        }
        assert!(matches!(kind, acp::ToolKind::Other));
        assert_eq!(raw_input, expected_raw_input);
        for (field, expected) in expected_fields {
            assert_eq!(
                raw_input[*field].as_str(),
                Some(*expected),
                "raw input field {field}"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn parent_child_tool_starts_keep_real_titles_and_raw_inputs() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;

                assert_tool_start(
                    &actor,
                    "ask-parent-call",
                    "ask_parent",
                    ToolInput::AskParent(
                        tools::implementations::grow_build::task::interaction::AskParentInput {
                            question: "Which boundary owns this update?".into(),
                        },
                    ),
                    &["ask_parent", "parent"],
                    tool_protocol::ToolAccess::Read,
                    &[("question", "Which boundary owns this update?")],
                )
                .await;

                assert_tool_start(
                    &actor,
                    "ask-subagent-call",
                    "ask_subagent",
                    ToolInput::AskSubagent(
                        tools::implementations::grow_build::task::interaction::AskSubagentInput {
                            subagent_id: "child-7".into(),
                            question: "Which files did you inspect?".into(),
                        },
                    ),
                    &["ask_subagent", "child-7"],
                    tool_protocol::ToolAccess::Read,
                    &[
                        ("subagent_id", "child-7"),
                        ("question", "Which files did you inspect?"),
                    ],
                )
                .await;

                assert_tool_start(
                    &actor,
                    "send-subagent-call",
                    "send_subagent_message",
                    ToolInput::SendSubagentMessage(
                        tools::implementations::grow_build::task::interaction::
                            SendSubagentMessageInput {
                                subagent_id: Some("child-7".into()),
                                reply_to: None,
                                message: "Continue at the next safe boundary.".into(),
                                interrupt: true,
                            },
                    ),
                    &["send_subagent_message", "child-7"],
                    tool_protocol::ToolAccess::Write,
                    &[("subagent_id", "child-7")],
                )
                .await;
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lsp_and_dynamic_tool_starts_keep_specific_identity() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) =
                    crate::session::actor::tests::support::build_actor().await;
                assert_tool_start(
                    &actor,
                    "lsp-call",
                    "lsp",
                    ToolInput::Lsp(tools::implementations::lsp::LspToolInput {
                        operation: tools::implementations::lsp::LspOperation::Hover,
                        file_path: Some("/tmp/main.rs".into()),
                        line: Some(3),
                        character: Some(4),
                        query: None,
                    }),
                    &["Code intelligence", "hover", "/tmp/main.rs"],
                    tool_protocol::ToolAccess::Read,
                    &[],
                )
                .await;
                assert_tool_start(
                    &actor,
                    "dynamic-call",
                    "custom_lookup",
                    ToolInput::Dynamic(serde_json::json!({"query": "x"})),
                    &["custom_lookup"],
                    tool_protocol::ToolAccess::Read,
                    &[],
                )
                .await;
            })
            .await;
    }
}

#[cfg(test)]
mod restored_pre_tool_hook_tests {
    use super::*;
    use chat_state::session_runtime::{
        SuspendedOperation, SuspendedOperationState, TurnContinuation,
    };

    fn continuation(state: SuspendedOperationState) -> TurnContinuation {
        TurnContinuation {
            turn_id: chat_state::TurnId(7),
            prompt_id: "prompt-7".into(),
            prompt_index: 0,
            origin: "user".into(),
            next_step_index: 1,
            tool_turns_used: 1,
            tool_calls_used: 1,
            stop_continuations_used: 0,
            output_schema: None,
            resume_point: chat_state::session_runtime::TurnResumePoint::Model,
            task_output_usage: None,
            accepted_response: None,
            structured_output_retries: 0,
            sampling_attempts_used: 1,
            sampling_max_attempts: None,
            sampling_replay_safe: false,
            sampling_deadline_ms: None,
            operations: vec![SuspendedOperation {
                call_id: "call-1".into(),
                state,
                dependency_id: Some("question-or-permission".into()),
            }],
        }
    }

    fn event(seq: u64, kind: chat_state::TimelineEventKind) -> chat_state::TimelineEvent {
        chat_state::TimelineEvent {
            version: chat_state::TIMELINE_SCHEMA_VERSION,
            seq: chat_state::EventSeq::new(seq),
            at_ms: seq as i64,
            kind,
        }
    }

    fn facts(decision: chat_state::HookGateDecision) -> Vec<chat_state::TimelineEvent> {
        use chat_state::{
            HookAggregateDecision, HookCause, HookEvent, HookEventType, HookGateKind, StepId,
            TimelineEventKind, ToolEvent,
        };
        vec![
            event(
                0,
                TimelineEventKind::Tool(ToolEvent::Started {
                    call_id: "call-1".into(),
                    turn: chat_state::TurnId(7),
                    step: StepId {
                        turn: chat_state::TurnId(7),
                        index: 0,
                    },
                    name: "ask_user_question".into(),
                    input: Some(serde_json::json!({"questions": ["continue?"]})),
                }),
            ),
            event(
                1,
                TimelineEventKind::Hook(HookEvent::Triggered {
                    occurrence_id: "hook-call-1".into(),
                    event: HookEventType::PreToolUse,
                    gate: HookGateKind::Tool,
                    cause: HookCause::Tool {
                        call_id: "call-1".into(),
                    },
                    config_generation: 1,
                    handlers: Vec::new(),
                }),
            ),
            event(
                2,
                TimelineEventKind::Hook(HookEvent::Completed {
                    occurrence_id: "hook-call-1".into(),
                    decision: HookAggregateDecision::Tool { decision },
                }),
            ),
        ]
    }

    #[test]
    fn exact_restored_call_reuses_completed_hook_without_replaying_it() {
        let continuation = continuation(SuspendedOperationState::AwaitUser);
        let events = facts(chat_state::HookGateDecision::Allow);
        let decision = restored_pre_tool_decision_from_facts(
            Some(&continuation),
            &["call-1".into()],
            "call-1",
            "ask_user_question",
            &serde_json::json!({"questions": ["continue?"]}),
            &events,
        )
        .expect("completed pre-tool decision is reusable");
        assert_eq!(decision, Some(::hooks::result::HookDecision::Allow));
        assert_eq!(
            events.len(),
            3,
            "recovery does not append a second hook lifecycle"
        );

        let unknown = restored_pre_tool_decision_from_facts(
            Some(&continuation),
            &["call-1".into()],
            "different-call",
            "ask_user_question",
            &serde_json::json!({"questions": ["continue?"]}),
            &events,
        )
        .expect("an unrelated call is ordinary work");
        assert!(unknown.is_none());
    }

    #[test]
    fn restored_waiting_call_without_unique_completed_hook_fails_closed() {
        let waiting_continuation = continuation(SuspendedOperationState::AwaitPermission);
        let start_only = facts(chat_state::HookGateDecision::Allow)
            .into_iter()
            .take(1)
            .collect::<Vec<_>>();
        let missing = restored_pre_tool_decision_from_facts(
            Some(&waiting_continuation),
            &["call-1".into()],
            "call-1",
            "ask_user_question",
            &serde_json::json!({"questions": ["continue?"]}),
            &start_only,
        );
        assert!(missing.unwrap_err().contains("PreToolUse occurrence"));

        let complete = facts(chat_state::HookGateDecision::Block {
            reason: "denied by policy".into(),
        });
        let not_active = restored_pre_tool_decision_from_facts(
            Some(&waiting_continuation),
            &[],
            "call-1",
            "ask_user_question",
            &serde_json::json!({"questions": ["continue?"]}),
            &complete,
        );
        assert!(
            not_active
                .unwrap_err()
                .contains("not active in the event tracker")
        );

        let changed_questions = restored_pre_tool_decision_from_facts(
            Some(&waiting_continuation),
            &["call-1".into()],
            "call-1",
            "ask_user_question",
            &serde_json::json!({"questions": ["different question?"]}),
            &complete,
        );
        assert!(
            changed_questions
                .unwrap_err()
                .contains("conflicting Timeline start or input")
        );

        let not_dispatched = continuation(SuspendedOperationState::NotDispatched);
        assert!(
            !restored_waiting_operation(Some(&not_dispatched), "call-1")
                .expect("not-yet-started call uses ordinary path")
        );
        let outcome_unknown = continuation(SuspendedOperationState::OutcomeUnknown);
        assert!(
            restored_waiting_operation(Some(&outcome_unknown), "call-1")
                .unwrap_err()
                .contains("not resumable")
        );
    }

    #[test]
    fn unknown_pre_tool_side_effect_cannot_be_reused_as_a_restored_decision() {
        use chat_state::{
            HookAggregateDecision, HookEvent, HookGateDecision, HookRunControl, HookRunOutcome,
            TimelineEventKind,
        };
        let mut events = facts(HookGateDecision::Allow);
        events.pop();
        events.push(event(
            2,
            TimelineEventKind::Hook(HookEvent::RunStarted {
                occurrence_id: "hook-call-1".into(),
                run_id: "hook-run".into(),
                handler_index: 0,
            }),
        ));
        events.push(event(
            3,
            TimelineEventKind::Hook(HookEvent::RunFinished {
                occurrence_id: "hook-call-1".into(),
                run_id: "hook-run".into(),
                handler_index: 0,
                elapsed_ms: 0,
                outcome: HookRunOutcome::InterruptedOutcomeUnknown,
                control: HookRunControl::None,
            }),
        ));
        events.push(event(
            4,
            TimelineEventKind::Hook(HookEvent::Completed {
                occurrence_id: "hook-call-1".into(),
                decision: HookAggregateDecision::Tool {
                    decision: HookGateDecision::Allow,
                },
            }),
        ));
        let error = restored_pre_tool_decision_from_facts(
            Some(&continuation(SuspendedOperationState::AwaitPermission)),
            &["call-1".into()],
            "call-1",
            "ask_user_question",
            &serde_json::json!({"questions": ["continue?"]}),
            &events,
        )
        .unwrap_err();
        assert!(error.contains("unknown PreToolUse side-effect outcome"));
    }
}

//! Tool-call preparation and client-facing ToolCall updates.

use super::parse::execute_tool_call_parts;
use super::*;

impl SessionActor {
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
            ToolInput::Skill(skill) => {
                ::diagnostics::session_ctx::log_event(::diagnostics::events::SkillDispatched {
                    skill_name: skill.skill.clone(),
                    plugin_source: None,
                });
                tracing::info_span!(
                    "skill.activated",
                    skill_name = %skill.skill,
                    invocation_trigger = "skill_tool",
                )
                .in_scope(|| {});
                (
                    format!("Skill: {}", skill.skill),
                    acp::ToolKind::Other,
                    vec![],
                    vec![],
                )
            }
            ToolInput::Dynamic(_) => (
                "Dynamic tool call".to_string(),
                acp::ToolKind::Other,
                vec![],
                vec![],
            ),
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
            #[allow(unreachable_patterns)]
            _ => (
                "Tool call".to_string(),
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
        let access_kind = AccessKind::from_tool_call(&call.function.name, &tool_input);
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
                let message = "Rejected: this exact tool identity is outside the subagent's authored eligibility ceiling or its inherited MCP transport is no longer eligible.";
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
        let aggregate = self
            .dispatch_hook_occurrence(
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
            })?;
        let decision = aggregate.into_tool_decision();
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
            let decision = {
                let _pending_guard =
                    crate::session::pending_interaction::PendingInteractionGuard::new(
                        self.pending_interactions.clone(),
                        self.notifications.gateway.clone(),
                        self.session_info.id.clone(),
                        tool_call_id.to_string(),
                        crate::session::pending_interaction::PendingKind::Permission,
                    );
                self.permissions
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
                        },
                    )
                    .await
            };
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
                let plan_content = input
                    .plan
                    .as_deref()
                    .expect("validated submit/amend plan")
                    .trim()
                    .to_owned();

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
                let approval_snapshot = self.behavior.lock().snapshot();

                tracing::info!(
                    tool_call_id = %tool_call_id,
                    "plan_control intercepted; requesting user approval"
                );
                let resp = self
                    .request_plan_approval(&tool_call_id, plan_content.clone())
                    .await;
                match resp {
                    Ok(parsed) => match parsed.outcome {
                        PlanApprovalOutcome::Abandoned => {
                            tracing::info!("plan_control: user abandoned Plan");
                            match self.finish_plan_to_default_if(&approval_snapshot).await {
                                Ok(true) => {}
                                Ok(false) => {
                                    tracing::info!("plan_control: dropping stale abandon decision");
                                    self.handle_tool_not_executed(
                                        &call.id,
                                        &tool_call_id,
                                        "Plan approval was stale and was discarded.".to_owned(),
                                    )
                                    .await?;
                                    return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                                }
                                Err(message) => {
                                    self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                                        .await?;
                                    return Ok(ToolPreflight::resolved(ToolLoop::Continue));
                                }
                            }
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
                            self.send_update(acp::SessionUpdate::ToolCallUpdate(tool_update), None)
                                .await;
                            let tool_chat = ConversationItem::tool_result(call.id.clone(), message);
                            self.chat_state_handle.push_tool_result(tool_chat);
                            return Ok(ToolPreflight::resolved(ToolLoop::Control(
                                ControlDisposition::EndTurn,
                            )));
                        }
                        PlanApprovalOutcome::Cancelled => {
                            let previous_behavior = self.behavior.lock().snapshot();
                            if !self.behavior.lock().reject_submitted_plan_if_with_feedback(
                                &approval_snapshot,
                                parsed.feedback.clone(),
                            ) {
                                tracing::info!(
                                    "plan_control: dropping stale request-changes decision"
                                );
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
                            let next = self.behavior.lock().snapshot();
                            if let Err(error) = self.admit_plan_handoff_notification(&next).await {
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
                            self.send_update(acp::SessionUpdate::ToolCallUpdate(tool_update), None)
                                .await;
                            let tool_chat = ConversationItem::tool_result(call.id.clone(), message);
                            self.chat_state_handle.push_tool_result(tool_chat);
                            return Ok(ToolPreflight::resolved(ToolLoop::Control(
                                ControlDisposition::ResampleStep,
                            )));
                        }
                        PlanApprovalOutcome::Approved => {
                            let previous_behavior = self.behavior.lock().snapshot();
                            if !self
                                .behavior
                                .lock()
                                .approve_submitted_plan_if_with_feedback(
                                    &approval_snapshot,
                                    parsed.feedback.clone(),
                                )
                            {
                                self.handle_tool_not_executed(
                                    &call.id,
                                    &tool_call_id,
                                    "Plan approval arrived in an invalid phase.".to_owned(),
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
                            let next = self.behavior.lock().snapshot();
                            if let Err(error) = self.admit_plan_handoff_notification(&next).await {
                                tracing::warn!(%error, "Plan execution handoff receipt will be reconciled after restore");
                            }
                            self.enqueue_current_mode_update(acp::SessionModeId::new(
                                tools::types::BehaviorId::Plan.as_id(),
                            ));
                            tracing::info!("plan_control: user approved frozen Plan contract");
                        }
                    },
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
                    PlanControlAction::Submit | PlanControlAction::Amend => {
                        ControlDisposition::ResampleStep
                    }
                    PlanControlAction::Complete | PlanControlAction::Cancel => {
                        ControlDisposition::EndTurn
                    }
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

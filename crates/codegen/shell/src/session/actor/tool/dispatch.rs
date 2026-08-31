//! Tool dispatch helpers for `SessionActor`: `dispatch_tool` and its lock /
//! display helpers, direct bash-mode execution, and tool argument
//! parse-error formatting.

use super::*;

/// Number of output lines to show in final bash mode output summary
const BASH_MODE_FINAL_OUTPUT_LINES: usize = 10;
const BASH_MODE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Phase 2: consume the call-bound permit, then dispatch through the exact
/// [`ToolBridge`] that issued it. Workspace keeps a reference to this same
/// finalized toolset for hooks and session lifecycle, but is deliberately not
/// an execution authority.
pub(super) async fn dispatch_tool(
    authority: &ToolDispatchAuthority,
    prepared: &PreparedToolCall,
    session_id: &str,
) -> Result<ToolRunResult, tool_runtime::ToolError> {
    validate_tool_call_permit(authority, prepared).await?;
    tracing::debug!(
        tool = %prepared.tool_name,
        call_id = %prepared.tool_call_id.0,
        session = %session_id,
        mode = "local",
        "dispatch_tool"
    );
    authority
        .bridge
        .call(
            &prepared.tool_name,
            prepared.parsed_args.clone(),
            &prepared.tool_call_id.0,
        )
        .await
}

async fn validate_tool_call_permit(
    authority: &ToolDispatchAuthority,
    prepared: &PreparedToolCall,
) -> Result<(), tool_runtime::ToolError> {
    use std::sync::atomic::Ordering;

    let stale = |reason: &str| {
        tool_runtime::ToolError::custom(
            "stale_tool_permit",
            format!("Tool call authorization is no longer valid: {reason}"),
        )
    };
    prepared
        .permit
        .consumed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| stale("the one-shot permit was already consumed"))?;
    if prepared.permit.call_id != prepared.call_id
        || prepared.permit.tool_name != prepared.tool_name
        || prepared.permit.dispatch_target_name != prepared.dispatch_target_name
        || prepared.permit.required_access != prepared.required_access
    {
        return Err(stale("call identity or projected access changed"));
    }
    if prepared.permit.cwd != authority.cwd {
        return Err(stale("execution cwd changed"));
    }
    if prepared.permit.actor_source != authority.actor_source {
        return Err(stale("actor source changed"));
    }
    if prepared.permit.canonical_args_hash != super::hash_canonical_json(&prepared.parsed_args) {
        return Err(stale("canonical arguments changed"));
    }
    let current_max = authority
        .bridge
        .max_access(&prepared.tool_name)
        .unwrap_or(tool_protocol::ToolAccess::All);
    if current_max != prepared.permit.descriptor_max
        || !current_max.covers(prepared.required_access)
    {
        return Err(stale(
            "tool descriptor changed or no longer covers the call",
        ));
    }

    if let Some(state) = authority.subagent.as_ref() {
        if prepared.permit.actor_epoch != Some(state.authorization_epoch()) {
            return Err(stale("child authorization epoch changed"));
        }
        if let Some(binding) = prepared.permit.mcp.as_ref() {
            let target = prepared
                .dispatch_target_name
                .as_deref()
                .unwrap_or(prepared.tool_name.as_str());
            if !state.mcp_tool_eligible(target) {
                return Err(stale("MCP inherited eligibility was revoked"));
            }
            let _ = binding;
        } else if !state.native_call_eligible(&prepared.tool_name, prepared.required_access) {
            return Err(stale("native eligibility changed"));
        }
    }

    if let Some(binding) = prepared.permit.mcp.as_ref() {
        let state = authority.mcp_state.lock().await;
        let current_client_id = state
            .get_client(&binding.server)
            .map(|client| client.client_id());
        let current_access = state
            .mcp_server_max_access
            .get(&binding.server)
            .copied()
            .unwrap_or(tool_protocol::ToolAccess::All);
        if state.generation() != binding.generation
            || current_client_id != Some(binding.client_id)
            || current_access != binding.max_access
        {
            return Err(stale(
                "MCP transport generation or trust-domain mask changed",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod permit_tests {
    use super::*;

    fn prepared() -> PreparedToolCall {
        let args = serde_json::json!({"path": "README.md"});
        PreparedToolCall {
            call_id: "call-1".to_owned(),
            tool_call_id: acp::ToolCallId::new("call-1"),
            tool_name: "fixture".to_owned(),
            raw_arguments: args.to_string(),
            parsed_args: args.clone(),
            model_id: "fixture-model".to_owned(),
            concatenated_json_count: 0,
            dispatch_target_name: None,
            required_access: tool_protocol::ToolAccess::All,
            permit: ToolCallPermit {
                call_id: "call-1".to_owned(),
                tool_name: "fixture".to_owned(),
                dispatch_target_name: None,
                canonical_args_hash: super::hash_canonical_json(&args),
                cwd: PathBuf::from("/workspace"),
                descriptor_max: tool_protocol::ToolAccess::All,
                required_access: tool_protocol::ToolAccess::All,
                actor_source: "primary:normal".to_owned(),
                actor_epoch: None,
                mcp: None,
                consumed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            },
            workflow_draft_write: false,
            success_control: None,
            plan_exit_on_success: None,
        }
    }

    fn authority() -> ToolDispatchAuthority {
        ToolDispatchAuthority {
            bridge: Arc::new(tools::bridge::ToolBridge::for_test()),
            subagent: None,
            mcp_state: Arc::new(TokioMutex::new(McpState::new(vec![]))),
            cwd: PathBuf::from("/workspace"),
            actor_source: "primary:normal".to_owned(),
        }
    }

    fn assert_stale(error: tool_runtime::ToolError, reason: &str) {
        assert_eq!(
            error.details,
            Some(serde_json::json!({"code": "stale_tool_permit"}))
        );
        assert!(
            error.detail.contains(reason),
            "unexpected stale-permit detail: {}",
            error.detail
        );
    }

    #[tokio::test]
    async fn permit_is_consumed_exactly_once() {
        let prepared = prepared();
        let authority = authority();
        validate_tool_call_permit(&authority, &prepared)
            .await
            .expect("first dispatch must consume the permit");
        assert_stale(
            validate_tool_call_permit(&authority, &prepared)
                .await
                .expect_err("a replay must fail before execution"),
            "already consumed",
        );
    }

    #[tokio::test]
    async fn frozen_arguments_and_authority_are_revalidated() {
        let mut changed_args = prepared();
        changed_args.parsed_args = serde_json::json!({"path": "different"});
        assert_stale(
            validate_tool_call_permit(&authority(), &changed_args)
                .await
                .expect_err("argument mutation must invalidate the permit"),
            "canonical arguments changed",
        );

        let changed_cwd = prepared();
        let mut moved = authority();
        moved.cwd = PathBuf::from("/other-workspace");
        assert_stale(
            validate_tool_call_permit(&moved, &changed_cwd)
                .await
                .expect_err("cwd mutation must invalidate the permit"),
            "execution cwd changed",
        );

        let changed_actor = prepared();
        let mut switched = authority();
        switched.actor_source = "primary:goal".to_owned();
        assert_stale(
            validate_tool_call_permit(&switched, &changed_actor)
                .await
                .expect_err("actor mutation must invalidate the permit"),
            "actor source changed",
        );
    }

    #[tokio::test]
    async fn child_authorization_epoch_change_invalidates_permit() {
        let bridge = tools::bridge::ToolBridge::for_test();
        let child = crate::session::subagent_capability::SubagentCapabilityState::from_bridge(
            &bridge,
            &tools::registry::types::ToolServerConfig { tools: vec![] },
            tool_types::SubagentCapabilityMode::ReadOnly,
            None,
            Default::default(),
        )
        .await;
        let mut prepared = prepared();
        prepared.permit.actor_epoch = Some(child.authorization_epoch());
        let authority = ToolDispatchAuthority {
            bridge: Arc::new(bridge),
            subagent: Some(child.clone()),
            ..authority()
        };
        child.replace_bound_mcp_client_ids(std::collections::HashMap::from([(
            "reconnected".to_owned(),
            7,
        )]));
        assert_stale(
            validate_tool_call_permit(&authority, &prepared)
                .await
                .expect_err("a changed child authority must invalidate the permit"),
            "authorization epoch changed",
        );
    }
}

/// First string-valued argument among `keys`, in priority order.
fn str_arg<'a>(args: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| args.get(*k)?.as_str())
}

/// Extract the workspace path that a tool call targets, for the purpose of
/// serializing concurrent same-file edits inside `execute_tool_calls`.
///
/// Different tools advertise the path under different JSON keys:
/// - `file_path` — grow_build (`search_replace`, `write`) and
///   grow_build_hashline (`hashline_edit`)
/// - `path` — alternate edit/read tools
/// - `target_file` — grow_build (`read_file`, via `#[serde(rename)]`)
///
/// Returning the same string for two calls in a batch causes them to share a
/// `tokio::sync::Mutex` and therefore run sequentially in model-emitted order.
/// Returning `None` lets the call run fully concurrently with everything else.
///
/// `target_directory` is deliberately omitted — a directory listing isn't an
/// edit and must not bucket into a file lock.
pub(in crate::session::actor) fn lock_path_for_args(args: &serde_json::Value) -> Option<&str> {
    str_arg(args, &["file_path", "path", "target_file"])
}

/// Temporary gate: only expose resolved model ID to the user for these models.
pub(in crate::session::actor) fn should_show_resolved_model(
    requested: &str,
    resolved: &str,
) -> bool {
    requested != resolved && super::acp_types::is_coding_model_slug(requested)
}

/// Resolve the shell name for the system prompt `Shell:` field.
///
/// Unix: basename of `$SHELL` (e.g. "zsh", "bash").
/// Windows: name from the `detect_windows_shell` cascade
/// (pwsh > powershell.exe > Git Bash > cmd.exe), since `$SHELL` is absent.
pub(in crate::session::actor) fn resolve_session_shell() -> String {
    #[cfg(unix)]
    {
        std::env::var("SHELL")
            .ok()
            .and_then(|s| {
                std::path::Path::new(&s)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| "bash".to_string())
    }

    #[cfg(not(unix))]
    {
        config::shell::detect_windows_shell().name().to_string()
    }
}

/// Key in `ToolError::details` that carries the HTTP status code.
/// Used by tool error producers and
/// the `is_auth_tool_error` classifier to avoid accidental key mismatch.
pub(in crate::session::actor) const HTTP_STATUS_DETAILS_KEY: &str = "status";

impl SessionActor {
    /// Extract bash command from prompt blocks if present in meta.
    /// Returns Some(command) if the prompt is a direct bash command, None otherwise.
    pub(in crate::session::actor) fn extract_bash_command(
        prompt_blocks: &[acp::ContentBlock],
    ) -> Option<String> {
        use crate::extensions::prompt_meta::PromptBlockMeta;
        for block in prompt_blocks {
            if let acp::ContentBlock::Text(text) = block
                && let Some(meta_val) = &text.meta
                && let Some(meta) = PromptBlockMeta::from_value(meta_val)
            {
                return Some(meta.bash_command);
            }
        }
        None
    }

    /// Handle a direct bash command from bash mode.
    /// Runs the command with streaming output and sends updates to the TUI.
    pub(in crate::session::actor) async fn handle_direct_bash_command(
        self: &Arc<Self>,
        _prompt_id: &str,
        command: String,
        prompt_blocks: &[acp::ContentBlock],
    ) -> PromptTurnResult {
        tracing::info!("Handling direct bash command");

        // Send user message chunks to scrollback (so user sees their command)
        let model_id = self.current_catalog_model_id();
        let user_chunk_meta = serde_json::json!({ "modelId": model_id })
            .as_object()
            .cloned();
        for block in prompt_blocks.iter() {
            let update = acp::SessionUpdate::UserMessageChunk(
                acp::ContentChunk::new(block.clone()).meta(user_chunk_meta.clone()),
            );
            let notification_meta = self.build_notification_meta();
            let _ = self
                .notifications
                .persistence_tx
                .send(PersistenceMsg::Update(SessionUpdate::Acp(Box::new(
                    acp::SessionNotification::new(self.session_info.id.clone(), update)
                        .meta(notification_meta.as_object().cloned()),
                ))));
        }

        // Run the bash command with streaming enabled
        let tool_call_id = acp::ToolCallId::from(format!("bash-mode-{}", uuid::Uuid::new_v4()));
        let tool_call_id_string = tool_call_id.to_string();

        // Send initial ToolCall to register with TUI

        use tools::types::ToolInput;
        // Use the stripped command as description so pager chrome shows the
        // real command (not a generic label) while still satisfying the required field.
        let title_command =
            tools::util::strip_redundant_session_cd(&command, self.tool_context.cwd.as_path());
        let tool_input = ToolInput::Bash(BashToolInput {
            command: command.clone(),
            timeout: None,
            description: title_command.clone().into_owned(),
            is_background: false,
        });
        // Bash mode has no model-issued wire name; resolve the toolset's
        // execute tool by kind so the grow/tool identity still stamps.
        let bash_marker = serde_json::json!({"bash_mode": true}).as_object().cloned();
        let exec_wire = {
            let agent = self.agent.borrow();
            agent
                .tool_bridge()
                .toolset()
                .tool_name_for_kind(tools::types::tool::ToolKind::Execute)
        };
        let bash_meta = match exec_wire {
            Some(wire) => self.stamp_tool_meta(bash_marker.clone(), &wire, Some(&tool_input)),
            None => bash_marker,
        };
        self.events
            .emit(crate::session::events::Event::LoopStarted { loop_index: 0 });
        self.events
            .tool_started(
                "direct_bash".to_owned(),
                tool_call_id_string.clone(),
                serde_json::to_value(&tool_input).ok(),
            )
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "direct command intent was not durably recorded: {error}"
                ))
            })?;
        self.send_update(
            acp::SessionUpdate::ToolCall(
                acp::ToolCall::new(tool_call_id.clone(), format!("Execute `{title_command}`"))
                    .kind(acp::ToolKind::Execute)
                    .status(acp::ToolCallStatus::InProgress)
                    .content(Vec::new())
                    .locations(Vec::new())
                    .raw_input(serde_json::to_value(&tool_input).ok())
                    .meta(bash_meta),
            ),
            None,
        )
        .await;

        let request = TerminalRunRequest {
            tool_call_id: tool_call_id.clone(),
            command: command.clone(),
            cwd: self.tool_context.cwd.clone(),
            env: self.tool_context.session_env.as_ref().clone(),
            timeout: BASH_MODE_TIMEOUT,
            output_byte_limit: 1_048_576, // 1 MiB
            stream: true,                 // Enable streaming for bash mode
            output_file: None,            // No file logging for interactive bash mode
        };

        let result = self.tool_context.terminal.run(request).await;

        // Format the output
        let (output, exit_code, timed_out, signal) = match result {
            Ok(res) => (
                res.combined_output,
                res.exit_code.unwrap_or(-1),
                res.timed_out,
                res.signal,
            ),
            Err(e) => (format!("Error running command: {}", e), -1, false, None),
        };

        // Create final summary with last N lines
        // Format: "... (X lines)\nlast\nfew\nlines"
        let lines: Vec<&str> = output.lines().collect();
        let total_lines = lines.len();
        let displayed_output = if total_lines > BASH_MODE_FINAL_OUTPUT_LINES {
            let start = total_lines - BASH_MODE_FINAL_OUTPUT_LINES;
            let last_lines = lines[start..].join("\n");
            format!("... ({} lines)\n{}", total_lines, last_lines)
        } else {
            output.trim_end().to_string()
        };

        let is_backgrounded = signal.as_deref() == Some("backgrounded");

        // Build the final response text with output summary and exit code
        let mut response_text = displayed_output.clone();
        if is_backgrounded {
            response_text.push_str("\n\n[command running in background]");
        } else if timed_out {
            response_text.push_str("\n\n[command timed out]");
        } else if let Some(ref sig) = signal {
            response_text.push_str(&format!("\n\n[killed by signal {}]", sig));
        } else {
            response_text.push_str(&format!("\n\n[exit code: {}]", exit_code));
        }

        let outcome = if is_backgrounded {
            "backgrounded"
        } else if timed_out {
            "timed_out"
        } else if signal.is_some() {
            "signalled"
        } else if exit_code == 0 {
            "completed"
        } else {
            "failed"
        };
        self.events
            .tool_completed_durably(
                &tool_call_id_string,
                outcome.to_owned(),
                Some(serde_json::json!({
                    "command": command.clone(),
                    "output": displayed_output.clone(),
                    "exit_code": exit_code,
                    "timed_out": timed_out,
                    "signal": signal.clone(),
                })),
            )
            .await
            .map_err(|error| {
                acp::Error::internal_error().data(format!(
                    "direct command outcome was not durably recorded: {error}"
                ))
            })?;

        // Send final tool call update
        // For backgrounded commands, don't mark as completed/failed - let the background task do that
        if !is_backgrounded {
            let final_status = if exit_code == 0 && signal.is_none() {
                acp::ToolCallStatus::Completed
            } else {
                acp::ToolCallStatus::Failed
            };
            let bash_output = BashOutput {
                output_for_prompt: BashOutput::make_output_for_prompt(&displayed_output),
                output: displayed_output.as_bytes().to_vec(),
                exit_code,
                command: command.clone(),
                truncated: total_lines > BASH_MODE_FINAL_OUTPUT_LINES,
                signal: signal.clone(),
                timed_out,
                description: None,
                current_dir: self.tool_context.cwd.to_string(),
                output_file: String::new(),
                total_bytes: displayed_output.len(),
                output_delta: None,
                was_bare_echo: false,
            };
            self.send_update(
                acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                    tool_call_id,
                    acp::ToolCallUpdateFields::new()
                        .status(Some(final_status))
                        .raw_output(serde_json::to_value(ToolsToolOutput::Bash(bash_output)).ok()),
                )),
                None,
            )
            .await;
        }

        // NOTE: The redundant AgentMessageChunk summary that was previously
        // sent here has been removed. The execute block already contains the
        // full command output — sending it again as an agent message created
        // a noisy duplicate scrollback entry. Old sessions that have it will
        // still replay fine; new sessions are cleaner.

        // Build one Surface item that includes command, output, and exit code.
        let user_message = format!(
            "I executed a terminal command: `{}`\n\nOutput:\n```\n{}\n```\n\n[exit code: {}]",
            command, displayed_output, exit_code
        );

        // Append it to the canonical Surface as a user message only.
        if let Err(error) = self
            .chat_state_handle
            .push_user_message_durably(ConversationItem::user(&user_message))
            .await
        {
            return Err(acp::Error::internal_error()
                .data(format!("direct command was not durably recorded: {error}")));
        }
        let title_source = prompt_blocks
            .iter()
            .filter_map(|block| match block {
                acp::ContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.schedule_session_title(title_source).await;

        self.chat_state_handle.flush();

        self.flush_to_disk().await;

        let total_tokens = self.chat_state_handle.get_projected_tokens().await;
        ok_end_turn(total_tokens, None)
    }
}

// ── Tool argument error formatting ─────────────────────────────────────

// Re-use the UTF-8-safe truncation helper from sampling-types rather
// than duplicating it here (R3).

/// Maximum bytes of `raw_arguments` included in a parse-error tool_result.
///
/// The model already holds the arguments in its recent context window, so
/// echoing the full string (potentially 8 KB+) would grow every subsequent
/// turn by that many tokens for no additional benefit.  The JSON error
/// position (e.g. `line 1 column 81`) is usually sufficient to locate the
/// typo; we include a prefix for orientation.
///
/// Note: when the JSON syntax error falls past this byte limit, the column
/// hint will reference text that was truncated from the message.  The model
/// should still have the full arguments in its context window from the
/// turn it generated them.
pub(in crate::session::actor) const MAX_ARGS_IN_ERROR: usize = 2_000;

/// Build the user-facing error message shown when tool arguments cannot be
/// parsed.  The message is stored as a `tool_result` in the conversation
/// history, so the model sees it on the very next turn.
///
/// The message intentionally includes:
///
/// 1. The normal error description (so the model knows *what* failed).
/// 2. The **original arguments string** the model produced (capped at
///    [`MAX_ARGS_IN_ERROR`] bytes).  Without this, shell would sanitize
///    the arguments to `"{}"` before forwarding them to the provider (to
///    avoid 400 errors), so the model would only see an empty object and have
///    to regenerate all its work from scratch.
/// 3. A JSON-level parse error (position + reason) when the arguments string
///    is itself invalid JSON — e.g. a missing `"` before a key name.  This
///    lets the model fix a one-character typo rather than regenerating a
///    thousand-line file.
pub(in crate::session::actor) fn build_tool_parse_error_message(
    function_name: &str,
    err: &tool_runtime::ToolError,
    raw_arguments: &str,
) -> String {
    let mut msg = format!("Failed to parse arguments for tool `{function_name}`: {err}");

    if raw_arguments.is_empty() {
        return msg;
    }

    // Append the original arguments (capped) so the model knows what it sent.
    // Use truncate_bytes to avoid panicking on a multi-byte UTF-8 boundary.
    msg.push_str("\n\nYour original arguments:\n");
    let prefix = truncate_bytes(raw_arguments, MAX_ARGS_IN_ERROR);
    msg.push_str(prefix);
    if prefix.len() < raw_arguments.len() {
        msg.push_str("\n... (truncated)");
    }

    // If the arguments string is not valid JSON, surface the exact position
    // of the syntax error so the model can fix it directly.
    // Use `IgnoredAny` — we only need the error, not a DOM.
    if let Err(json_err) = serde_json::from_str::<serde::de::IgnoredAny>(raw_arguments) {
        msg.push_str(&format!(
            "\n\nNote: the arguments above contain invalid JSON — {json_err}\n\
             Please fix the syntax and retry."
        ));
    }

    msg
}

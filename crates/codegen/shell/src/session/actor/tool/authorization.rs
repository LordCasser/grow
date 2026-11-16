//! Tool-call authorization projection and Plan/MCP admission helpers.

use super::*;

pub(super) fn public_workflow_conflict(
    admitted: tool_types::BehaviorId,
    current: tool_types::BehaviorId,
) -> Option<tool_types::BehaviorId> {
    if admitted != tool_types::BehaviorId::Workflow {
        Some(admitted)
    } else if current != tool_types::BehaviorId::Workflow {
        Some(current)
    } else {
        None
    }
}

/// Conservative write hint used by hook and Definition path protections.
/// RWX projection below is stricter and additionally rejects every unknown
/// executable; these call sites still need the concrete write predicate.
pub(super) fn recognizable_shell_write(command: &str) -> bool {
    let parsed_write = workspace::permission::bash_command_splitting::try_parse_shell(command)
        .is_some_and(|tree| {
            !workspace::permission::command_write_paths_in_tree(tree.root_node(), command)
                .is_empty()
        });
    if parsed_write {
        return true;
    }
    let command = command.to_ascii_lowercase();
    command.contains('>')
        || [
            "tee ",
            "sed -i",
            "perl -i",
            "rm ",
            "mv ",
            "cp ",
            "touch ",
            "mkdir ",
            "install ",
            "truncate ",
            "dd ",
            "python ",
            "python3 ",
            "ruby ",
            "node ",
        ]
        .iter()
        .any(|token| command.contains(token))
}

/// Project a frozen shell invocation into RWX. The parser is shared with the
/// hard permission boundary: only its built-in observational command set gets
/// RX. Unknown executables are opaque effects even when the Bash surface has no
/// redirection, so they fail closed as All together with writes and network
/// launchers.
pub(super) fn shell_required_access(command: &str) -> tool_protocol::ToolAccess {
    let Some(tree) = workspace::permission::bash_command_splitting::try_parse_shell(command) else {
        return tool_protocol::ToolAccess::All;
    };
    if tree.root_node().has_error()
        || workspace::permission::tree_has_opaque_shell(tree.root_node(), command)
    {
        return tool_protocol::ToolAccess::All;
    }
    let lower = command.to_ascii_lowercase();
    let externally_emitting = [
        "curl ",
        "wget ",
        "ssh ",
        "scp ",
        "rsync ",
        "nc ",
        "ncat ",
        "socat ",
        "git push",
        "git fetch",
        "git pull",
        "git clone",
        "gh ",
        "kubectl ",
        "docker push",
        "docker pull",
        "npm publish",
        "cargo publish",
    ]
    .iter()
    .any(|token| lower.starts_with(token) || lower.contains(&format!(" {token}")));
    if externally_emitting || !workspace::permission::command_is_known_observational(command) {
        tool_protocol::ToolAccess::All
    } else {
        tool_protocol::ToolAccess::ReadExecute
    }
}

/// The one native call projector. Descriptors declare an eligibility ceiling;
/// this function narrows frozen typed arguments to the authority required by
/// this invocation. MCP calls use their config-owned trust-domain ceiling.
pub(super) fn project_call_access(
    input: &ToolInput,
    descriptor_max: tool_protocol::ToolAccess,
    mcp_max: Option<tool_protocol::ToolAccess>,
) -> tool_protocol::ToolAccess {
    use tool_protocol::ToolAccess;
    match input {
        ToolInput::ReadFile(_)
        | ToolInput::Grep(_)
        | ToolInput::ListDir(_)
        | ToolInput::TaskOutput(_)
        | ToolInput::SubagentList(_)
        | ToolInput::SubagentStatus(_)
        | ToolInput::SubagentResult(_)
        | ToolInput::SubagentWait(_)
        | ToolInput::MemorySearch(_)
        | ToolInput::MemoryGet(_)
        | ToolInput::ContextRecall(_)
        | ToolInput::Lsp(_)
        | ToolInput::SchedulerList(_)
        | ToolInput::ListActiveSessions(_)
        | ToolInput::AskParent(_)
        | ToolInput::AskSubagent(_)
        | ToolInput::AskSession(_)
        | ToolInput::GetInquiry(_)
        | ToolInput::GetGoal(_) => ToolAccess::Read,
        ToolInput::SearchReplace(_) | ToolInput::HashlineEdit(_) => ToolAccess::ReadWrite,
        ToolInput::SendSubagentMessage(input) if input.reply_to.is_some() => ToolAccess::None,
        ToolInput::Write(_) | ToolInput::SendSubagentMessage(_) => ToolAccess::Write,
        ToolInput::Bash(input) => shell_required_access(&input.command),
        ToolInput::Monitor(input) => shell_required_access(&input.command),
        ToolInput::KillTask(_) => ToolAccess::None,
        // A Task call is the authority grant for the child's initial RWX.
        // Model arguments are requests, not authority by themselves; project
        // them through the parent's ordinary permission gate before the child
        // gets the corresponding fast-path ceiling.
        ToolInput::Task(task) => match task.capability_mode {
            Some(tool_types::SubagentCapabilityMode::ReadOnly) => ToolAccess::Read,
            Some(tool_types::SubagentCapabilityMode::ReadWrite) => ToolAccess::ReadWrite,
            Some(tool_types::SubagentCapabilityMode::Execute) => ToolAccess::ReadExecute,
            Some(tool_types::SubagentCapabilityMode::All) | None => ToolAccess::All,
        },
        ToolInput::WebFetch(_) => ToolAccess::ReadWrite,
        ToolInput::SchedulerCreate(_) | ToolInput::SchedulerDelete(_) => ToolAccess::WriteExecute,
        ToolInput::CreateGoal(_) | ToolInput::UpdateGoal(_) => ToolAccess::WriteExecute,
        ToolInput::Workflow(input) => {
            use tools::implementations::grow_build::workflow::{
                WorkflowDraftSource, WorkflowToolInput,
            };
            match input {
                WorkflowToolInput::Search { .. } => ToolAccess::Read,
                // Inspect persists the selected Definition as workspace focus.
                WorkflowToolInput::Inspect { .. } => ToolAccess::ReadWrite,
                WorkflowToolInput::Draft {
                    source: WorkflowDraftSource::Inline { .. },
                    ..
                } => ToolAccess::Write,
                WorkflowToolInput::Draft {
                    source:
                        WorkflowDraftSource::File { .. } | WorkflowDraftSource::Definition { .. },
                    ..
                } => ToolAccess::ReadWrite,
                // Validation records the validated content hash; Run also
                // materializes and starts a durable Run.
                WorkflowToolInput::Validate { .. } | WorkflowToolInput::Run { .. } => {
                    ToolAccess::All
                }
                WorkflowToolInput::Publish { .. } => ToolAccess::ReadWrite,
                WorkflowToolInput::Discard { .. } => ToolAccess::Write,
                WorkflowToolInput::ControlRun { .. } => ToolAccess::WriteExecute,
            }
        }
        ToolInput::MCPTool(_) | ToolInput::UseTool(_) => mcp_max.unwrap_or(ToolAccess::All),
        ToolInput::Dynamic(_) => mcp_max.unwrap_or(descriptor_max),
        ToolInput::TodoWrite(_)
        | ToolInput::TodoRead(_)
        | ToolInput::SearchTool(_)
        | ToolInput::PlanControl(_)
        | ToolInput::SetSubagentModel(_)
        | ToolInput::AskUserQuestion(_) => ToolAccess::None,
    }
}

pub(in crate::session::actor) fn hash_canonical_json(value: &serde_json::Value) -> String {
    fn write(value: &serde_json::Value, out: &mut String) {
        match value {
            serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_) => out.push_str(
                &serde_json::to_string(value).expect("scalar JSON serialization is infallible"),
            ),
            serde_json::Value::Array(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write(value, out);
                }
                out.push(']');
            }
            serde_json::Value::Object(values) => {
                out.push('{');
                let mut keys = values.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(
                        &serde_json::to_string(key)
                            .expect("object key JSON serialization is infallible"),
                    );
                    out.push(':');
                    write(&values[key], out);
                }
                out.push('}');
            }
        }
    }
    let mut canonical = String::new();
    write(value, &mut canonical);
    blake3::hash(canonical.as_bytes()).to_hex().to_string()
}

const WRITE_PERMISSION_PREVIEW_BYTES: usize = 1_024;

/// Capture review evidence from the frozen wire identity and parsed arguments.
/// The operation payload is deliberately bounded and remains untrusted model
/// content; it explains the proposed effect but grants no authority.
pub(super) fn permission_call_evidence(
    tool_name: &str,
    parsed_args: &serde_json::Value,
    input: &ToolInput,
) -> workspace::permission::types::PermissionCallEvidence {
    let operation = match input {
        ToolInput::Write(write) => {
            let truncated = write.content.len() > WRITE_PERMISSION_PREVIEW_BYTES;
            let preview = tools::util::truncate_str_with_marker(
                &write.content,
                WRITE_PERMISSION_PREVIEW_BYTES,
            )
            .into_owned();
            Some(json!({
                "operation": "replace_entire_file",
                "path": write.file_path,
                "content_bytes": write.content.len(),
                "content_blake3": blake3::hash(write.content.as_bytes()).to_hex().to_string(),
                "content_preview": preview,
                "content_preview_truncated": truncated,
                "content_preview_max_bytes": WRITE_PERMISSION_PREVIEW_BYTES,
            }))
        }
        _ => None,
    };
    workspace::permission::types::PermissionCallEvidence {
        tool_name: tool_name.to_owned(),
        canonical_args_hash: hash_canonical_json(parsed_args),
        operation,
    }
}

impl ToolCallPermit {
    pub(super) fn trajectory_meta(&self) -> serde_json::Value {
        let mcp = self.mcp.as_ref().map(|binding| {
            json!({
                "server": binding.server,
                "client_id": binding.client_id,
                "generation": binding.generation,
                "max_access": binding.max_access,
            })
        });
        let identity = json!({
            "call_id": self.call_id,
            "tool_name": self.tool_name,
            "dispatch_target_name": self.dispatch_target_name,
            "canonical_args_hash": self.canonical_args_hash,
            "cwd": self.cwd.to_string_lossy(),
            "descriptor_max_access": self.descriptor_max,
            "required_access": self.required_access,
            "actor_source": self.actor_source,
            "actor_epoch": self.actor_epoch,
            "mcp": mcp,
        });
        json!({
            "id": hash_canonical_json(&identity),
            "args_hash": self.canonical_args_hash,
            "actor_source": self.actor_source,
            "actor_epoch": self.actor_epoch,
            "mcp": identity["mcp"].clone(),
        })
    }
}

impl SessionActor {
    pub(super) async fn issue_tool_call_permit(
        &self,
        call_id: &str,
        tool_name: &str,
        dispatch_target_name: Option<String>,
        parsed_args: &serde_json::Value,
        access_kind: &AccessKind,
        descriptor_max: tool_protocol::ToolAccess,
        required_access: tool_protocol::ToolAccess,
    ) -> Result<ToolCallPermit, String> {
        let mcp = if let AccessKind::MCPTool { name, .. } = access_kind {
            let Some((_, server, _)) = ::mcp::servers::parse_mcp_qualified_name(name) else {
                return Err(format!(
                    "MCP target `{name}` is not a canonical server-qualified tool name"
                ));
            };
            let state = self.mcp_state.lock().await;
            let Some(client) = state.get_client(server) else {
                return Err(format!("MCP server `{server}` has no live transport"));
            };
            let max_access = state
                .mcp_server_max_access
                .get(server)
                .copied()
                .unwrap_or(tool_protocol::ToolAccess::All);
            if max_access != required_access {
                return Err(format!(
                    "MCP trust-domain access changed while authorizing `{name}`"
                ));
            }
            Some(McpPermitBinding {
                server: server.to_owned(),
                client_id: client.client_id(),
                generation: state.generation(),
                max_access,
            })
        } else {
            None
        };
        Ok(ToolCallPermit {
            call_id: call_id.to_owned(),
            tool_name: tool_name.to_owned(),
            dispatch_target_name,
            canonical_args_hash: hash_canonical_json(parsed_args),
            cwd: self.tool_context.cwd.as_path().to_path_buf(),
            descriptor_max,
            required_access,
            actor_source: if self.startup_hints.is_subagent {
                format!("child:{}", self.session_info.id.0)
            } else {
                format!("primary:{}", self.turn_behavior.lock().as_id())
            },
            actor_epoch: self
                .subagent_capabilities
                .as_ref()
                .map(|state| state.authorization_epoch()),
            mcp,
            consumed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }
}

pub(super) fn recognizable_shell_write_paths(command: &str, cwd: &std::path::Path) -> Vec<String> {
    workspace::permission::bash_command_splitting::try_parse_shell(command)
        .map(|tree| {
            workspace::permission::command_write_paths_with_cwd_in_tree(
                tree.root_node(),
                command,
                cwd,
            )
        })
        .unwrap_or_default()
}

pub(super) fn shell_write_path(
    value: &str,
    cwd: &std::path::Path,
    display_cwd: Option<&std::path::Path>,
) -> std::path::PathBuf {
    let value = std::path::Path::new(value);
    if value.is_absolute() {
        return tools::types::resources::resolve_model_path(
            cwd,
            display_cwd,
            value.to_string_lossy().as_ref(),
        );
    }
    cwd.join(value)
}

pub(super) fn recognizable_shell_write_under(
    command: &str,
    roots: &[std::path::PathBuf],
    cwd: &std::path::Path,
    display_cwd: Option<&std::path::Path>,
) -> bool {
    recognizable_shell_write_paths(command, cwd)
        .into_iter()
        .map(|path| shell_write_path(&path, cwd, display_cwd))
        .any(|path| roots.iter().any(|root| normalized_path_under(root, &path)))
}

pub(super) fn workflow_path_write(
    access_kind: &AccessKind,
    is_definition_path: impl Fn(&str) -> bool,
) -> bool {
    match access_kind {
        AccessKind::Edit(path) => is_definition_path(path),
        AccessKind::Bash(command) => {
            is_definition_path(command) && recognizable_shell_write(command)
        }
        _ => false,
    }
}

pub(super) fn normalized_path_under(root: &std::path::Path, path: &std::path::Path) -> bool {
    let root = resolve_existing_ancestors(root);
    let path = resolve_existing_ancestors(path);
    path == root || path.starts_with(&root)
}

/// Canonicalize the longest existing prefix and append the missing tail. This
/// makes admission follow the same directory symlinks that a later write will
/// follow, including when the final file does not exist yet.
pub(super) fn resolve_existing_ancestors(path: &std::path::Path) -> std::path::PathBuf {
    let normalized = paths::normalize_lexically(path);
    let mut existing = normalized.as_path();
    let mut tail = Vec::new();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = existing.file_name() else {
                    return normalized;
                };
                tail.push(name.to_os_string());
                let Some(parent) = existing.parent() else {
                    return normalized;
                };
                existing = parent;
            }
            Err(_) => return normalized,
        }
    }
    let Ok(mut resolved) = dunce::canonicalize(existing) else {
        return normalized;
    };
    for component in tail.iter().rev() {
        resolved.push(component);
    }
    resolved
}

pub(super) fn definition_edit_path(
    value: &str,
    cwd: &std::path::Path,
    display_cwd: Option<&std::path::Path>,
) -> std::path::PathBuf {
    tools::types::resources::resolve_model_path(cwd, display_cwd, value)
}

pub(super) fn saved_workflow_definition_write(
    access_kind: &AccessKind,
    cwd: &std::path::Path,
    display_cwd: Option<&std::path::Path>,
) -> bool {
    let project = crate::session::workflow::registry::project_root(cwd)
        .join(".grow")
        .join("workflows");
    let user = crate::session::workflow::registry::user_workflow_dir();
    match access_kind {
        AccessKind::Edit(path) => {
            let path = definition_edit_path(path, cwd, display_cwd);
            normalized_path_under(&project, &path) || normalized_path_under(&user, &path)
        }
        // Shell access is intentionally limited to recognizable writes. Match
        // both the configured roots and the conventional project spelling;
        // the permission parser still owns precise shell operand resolution.
        AccessKind::Bash(command) if recognizable_shell_write(command) => {
            if recognizable_shell_write_under(
                command,
                &[project.clone(), user.clone()],
                cwd,
                display_cwd,
            ) {
                return true;
            }
            let normalized = command.replace('\\', "/").to_ascii_lowercase();
            let roots = [project, user];
            normalized.contains(".grow/workflows/")
                || normalized.ends_with(".grow/workflows")
                || roots.iter().any(|root| {
                    let root = root
                        .to_string_lossy()
                        .replace('\\', "/")
                        .to_ascii_lowercase();
                    normalized == root || normalized.contains(&format!("{root}/"))
                })
        }
        _ => false,
    }
}

pub(super) fn session_workflow_definition_write(
    access_kind: &AccessKind,
    session_dir: &std::path::Path,
    cwd: &std::path::Path,
    display_cwd: Option<&std::path::Path>,
) -> bool {
    if let AccessKind::Edit(path) = access_kind {
        return normalized_path_under(
            &session_dir.join("workflow-workspace"),
            &definition_edit_path(path, cwd, display_cwd),
        );
    }
    if let AccessKind::Bash(command) = access_kind
        && recognizable_shell_write(command)
        && recognizable_shell_write_under(
            command,
            &[session_dir.join("workflow-workspace")],
            cwd,
            display_cwd,
        )
    {
        return true;
    }
    workflow_path_write(access_kind, |value| {
        let normalized = value.replace('\\', "/").to_ascii_lowercase();
        normalized.contains("workflow-workspace/") || normalized.ends_with("workflow-workspace")
    })
}

pub(super) fn workflow_definition_write(
    access_kind: &AccessKind,
    session_dir: &std::path::Path,
    cwd: &std::path::Path,
    display_cwd: Option<&std::path::Path>,
) -> bool {
    saved_workflow_definition_write(access_kind, cwd, display_cwd)
        || session_workflow_definition_write(access_kind, session_dir, cwd, display_cwd)
}

pub(super) fn workflow_run_snapshot_write(
    access_kind: &AccessKind,
    session_dir: &std::path::Path,
    cwd: &std::path::Path,
    display_cwd: Option<&std::path::Path>,
) -> bool {
    let root_path = session_dir.join("workflows");
    let root = root_path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let contains_snapshot_path = |value: &str| {
        let normalized = value.replace('\\', "/").to_ascii_lowercase();
        normalized == root || normalized.contains(&format!("{root}/"))
    };
    match access_kind {
        AccessKind::Edit(path) => {
            normalized_path_under(&root_path, &definition_edit_path(path, cwd, display_cwd))
        }
        AccessKind::Bash(command) if contains_snapshot_path(command) => {
            recognizable_shell_write(command)
        }
        AccessKind::Bash(command) if recognizable_shell_write(command) => {
            recognizable_shell_write_under(command, &[root_path], cwd, display_cwd)
        }
        _ => false,
    }
}
/// Select the first lifecycle mutation as the batch barrier. Every other call
/// is a sibling sampled against the pre-transition state and must be durably
/// cancelled; this applies even when the provider emitted a sibling first.
pub(super) fn split_control_preflight_barrier(
    mut calls: Vec<crate::sampling::types::ToolCallResponse>,
    isolates_batch_preflight: impl Fn(&str) -> bool,
) -> (
    Option<crate::sampling::types::ToolCallResponse>,
    Vec<crate::sampling::types::ToolCallResponse>,
) {
    let Some(index) = calls
        .iter()
        .position(|call| isolates_batch_preflight(&call.function.name))
    else {
        return (None, calls);
    };
    let control = calls.remove(index);
    (Some(control), calls)
}
/// Verdict for a tool call evaluated against the Plan/Workflow gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlanEditGate {
    /// Continue to the ordinary permission path.
    Allow,
    /// Public Workflow is incompatible with an active Plan contract.
    RejectWorkflow,
}
/// Keep Plan/Workflow mutual exclusion in front of ordinary permissions.
///
/// Drafting, AwaitingApproval, Amending, and Executing do not add a Bash,
/// Edit, MCP, or Task capability ban. Those calls use the same allow, ask,
/// and deny path as Normal. Policy deny, hooks, sandbox, child capability,
/// and Workflow definition protection stay on that ordinary path.
pub(super) fn plan_mode_edit_gate(
    tracker: &crate::session::behavior::BehaviorCoordinator,
    tool_input: &ToolInput,
) -> PlanEditGate {
    if tracker.is_plan() && matches!(tool_input, ToolInput::Workflow(_)) {
        PlanEditGate::RejectWorkflow
    } else {
        PlanEditGate::Allow
    }
}

/// Resolve the config-declared scope for an MCP tool call.
///
/// Returns `None` for non-MCP access kinds (the gate ignores it), `Some(true)`
/// Unparseable qualified names and unconfigured servers fail closed as
/// `Write`.
///
/// Async because the cached set lives in `McpState` (tokio Mutex). Callers
/// must run this BEFORE acquiring the `behavior` lock so neither lock is held
/// across an await of the other.
pub(super) async fn mcp_call_max_access(
    mcp_state: &TokioMutex<McpState>,
    access_kind: &AccessKind,
) -> Option<tool_protocol::ToolAccess> {
    let AccessKind::MCPTool { name, .. } = access_kind else {
        return None;
    };
    let server = ::mcp::servers::parse_mcp_qualified_name(name).map(|(_, server, _)| server);
    let mcp_state = mcp_state.lock().await;
    Some(
        server
            .and_then(|name| mcp_state.mcp_server_max_access.get(name).copied())
            .unwrap_or(tool_protocol::ToolAccess::All),
    )
}

pub(super) use tools::implementations::grow_build::plan_control::PlanApprovalOutcome;
/// Classify an `ext_method` failure: `true` when the reverse-request could not
/// be DELIVERED to any client (no interactive client wired — headless / SDK),
/// `false` when it was delivered but the client went away before answering
/// (quit / disconnect / leader restart).
///
/// Uses `acp`'s TYPED [`AcpChannelFailure`](acp_transport::AcpChannelFailure)
/// discriminant (carried in the error's `data`) rather than substring-matching
/// another crate's message text: `SendFailed` (enqueue failed → no connection) →
/// `true`; `RecvFailed` (delivered then dropped) → `false`. Any other error
/// (including a non-`acp_send` error) defaults to `false` so the approval is
/// kept pending and never auto-approved.
pub(super) fn ext_method_no_client(err: &acp::Error) -> bool {
    matches!(
        acp_transport::acp_channel_failure(err),
        Some(acp_transport::AcpChannelFailure::SendFailed)
    )
}
pub(super) async fn write_plan_artifact_async(
    session: std::sync::Arc<crate::session::storage::ContainedDirectory>,
    markdown: String,
) -> std::io::Result<String> {
    tokio::task::spawn_blocking(move || {
        crate::session::behavior::write_plan_artifact(&session, &markdown)
    })
    .await
    .map_err(std::io::Error::other)?
}

#[cfg(test)]
thread_local! {
    pub(super) static PLAN_ARTIFACT_READ_BARRIER: std::cell::RefCell<Option<(
        tokio::sync::oneshot::Sender<()>, tokio::sync::oneshot::Receiver<()>,
    )>> = const { std::cell::RefCell::new(None) };
}

pub(super) async fn read_plan_artifact_async(
    session: std::sync::Arc<crate::session::storage::ContainedDirectory>,
    hash: String,
) -> std::io::Result<String> {
    let result = tokio::task::spawn_blocking(move || {
        crate::session::behavior::read_plan_artifact(&session, &hash)
    })
    .await
    .map_err(std::io::Error::other)?;
    #[cfg(test)]
    if let Some((ready, release)) =
        PLAN_ARTIFACT_READ_BARRIER.with(|probe| probe.borrow_mut().take())
    {
        let _ = ready.send(());
        let _ = release.await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_read_projects_no_filesystem_or_process_authority() {
        use tool_protocol::ToolAccess;
        use tool_types::planning::{PlanReadInput, PlanReadTarget};
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let input = ToolInput::TodoRead(PlanReadInput {
                target,
                ..Default::default()
            });
            assert_eq!(
                project_call_access(&input, ToolAccess::None, None),
                ToolAccess::None
            );
        }
        assert_eq!(
            tool_runtime::Tool::capabilities(&tools::implementations::grow_build::TodoReadTool)
                .max_access,
            ToolAccess::None
        );
    }

    #[test]
    fn write_permission_evidence_is_exact_canonical_and_bounded() {
        let content = "你".repeat(500);
        let parsed = json!({
            "content": content,
            "file_path": "src/important.rs",
        });
        let input = ToolInput::Write(tools::implementations::grow_build::WriteInput {
            file_path: "src/important.rs".to_owned(),
            content: "你".repeat(500),
        });

        let evidence = permission_call_evidence("write", &parsed, &input);
        assert_eq!(evidence.tool_name, "write");
        assert_eq!(
            evidence.canonical_args_hash,
            hash_canonical_json(&json!({
                "file_path": "src/important.rs",
                "content": "你".repeat(500),
            }))
        );
        let operation = evidence.operation.expect("write operation summary");
        assert_eq!(operation["operation"], "replace_entire_file");
        assert_eq!(operation["path"], "src/important.rs");
        assert_eq!(operation["content_bytes"], 1_500);
        assert_eq!(operation["content_preview_truncated"], true);
        assert_eq!(operation["content_preview_max_bytes"], 1_024);
        assert!(
            operation["content_blake3"]
                .as_str()
                .is_some_and(|v| v.len() == 64)
        );
        assert!(
            operation["content_preview"]
                .as_str()
                .is_some_and(|v| v.len() <= WRITE_PERMISSION_PREVIEW_BYTES && v.ends_with('…'))
        );
    }

    /// Ordinary permission chain reached by Normal and every Plan phase.
    ///
    /// The Plan gate is applied first. One real permission manager resolves
    /// every call: an explicit `Bash(git:*)` allow records `policy_allow`
    /// (`git remote -v` is a real investigation command outside the
    /// always-safe list), `pwd` records the always-safe `safe_command`
    /// auto-allow, an explicit deny records `policy_deny`, and Ask is the
    /// same manager reaching its gateway prompt. Phase is not an input to
    /// any of those decisions.
    #[tokio::test(flavor = "current_thread")]
    async fn plan_phases_share_ordinary_bash_permission_decisions() {
        // The real permission manager spawns its actor with spawn_local, so
        // the whole test body must run inside a LocalSet (same shape as
        // subagent_bash_permission_tests.rs).
        tokio::task::LocalSet::new()
            .run_until(async {
                use std::sync::{Arc, Mutex};

                use crate::session::behavior::BehaviorCoordinator;
                use acp_transport::protocol as acp;
                use acp_transport::{AcpAgentGatewaySender, AcpClientMessage};
                use paths::AbsPathBuf;
                use tools::implementations::BashToolInput;
                use workspace::permission::types::{
                    PermissionConfig, PermissionRule, PromptPolicy, RuleAction, ToolFilter,
                };
                use workspace::permission::{
                    AccessKind, ClientType, Decision, PermissionEvent, spawn_permission_manager,
                };

                fn bash(command: &str) -> ToolInput {
                    ToolInput::Bash(BashToolInput {
                        command: command.into(),
                        timeout: None,
                        description: "ordinary permission parity".into(),
                        is_background: false,
                    })
                }

                fn phases() -> Vec<(&'static str, BehaviorCoordinator)> {
                    let normal = BehaviorCoordinator::new();
                    let mut drafting = BehaviorCoordinator::new();
                    assert!(drafting.select_behavior(tool_types::BehaviorId::Plan));
                    let mut awaiting = BehaviorCoordinator::new();
                    assert!(awaiting.select_behavior(tool_types::BehaviorId::Plan));
                    awaiting.record_plan_artifact("# submitted");
                    assert!(awaiting.submit_initial_plan());
                    let mut executing = BehaviorCoordinator::new();
                    assert!(executing.select_behavior(tool_types::BehaviorId::Plan));
                    executing.record_plan_artifact("# approved");
                    assert!(executing.submit_initial_plan());
                    assert!(executing.approve_submitted_plan());
                    let mut amending = BehaviorCoordinator::new();
                    assert!(amending.select_behavior(tool_types::BehaviorId::Plan));
                    amending.record_plan_artifact("# approved");
                    assert!(amending.submit_initial_plan());
                    assert!(amending.approve_submitted_plan());
                    assert!(amending.submit_plan_amendment());
                    vec![
                        ("normal", normal),
                        ("drafting", drafting),
                        ("awaiting_approval", awaiting),
                        ("amending", amending),
                        ("executing", executing),
                    ]
                }

                let (gateway_tx, mut gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                let prompts = Arc::new(Mutex::new(Vec::<String>::new()));
                let recorded = Arc::clone(&prompts);
                tokio::task::spawn_local(async move {
                    while let Some(message) = gateway_rx.recv().await {
                        if let AcpClientMessage::RequestPermission(args) = message {
                            recorded
                                .lock()
                                .expect("prompt log")
                                .push(args.request.tool_call.tool_call_id.0.to_string());
                            let response = acp::RequestPermissionResponse::new(
                                acp::RequestPermissionOutcome::Selected(
                                    acp::SelectedPermissionOutcome::new(
                                        acp::PermissionOptionId::new(Arc::from("allow-once")),
                                    ),
                                ),
                            );
                            let _ = args.response_tx.send(Ok(response));
                        }
                    }
                });

                let cwd = AbsPathBuf::new(std::env::temp_dir()).expect("temporary cwd");
                let config = PermissionConfig {
                    rules: vec![
                        // `git remote -v` is outside the always-safe list, so
                        // this explicit rule is what records `policy_allow`
                        // (same shape proven by the manager's own git-allow
                        // regression test). `pwd` stays rule-free: the
                        // always-safe list resolves it as `safe_command`.
                        workspace::permission::rules::parse_permission_rule(
                            "Bash(git:*)",
                            RuleAction::Allow,
                        )
                        .expect("valid git allow rule"),
                        PermissionRule {
                            action: RuleAction::Deny,
                            tool: ToolFilter::Bash,
                            pattern: Some("rm -rf /".into()),
                            pattern_mode: Default::default(),
                        },
                    ],
                    prompt_policy: PromptPolicy::Ask,
                };
                let (permissions, mut events) = spawn_permission_manager(
                    acp::SessionId::new("plan-ordinary-permission"),
                    AcpAgentGatewaySender::new(gateway_tx),
                    cwd,
                    ClientType::Generic,
                    std::time::Duration::from_secs(5),
                    Some(config),
                    Vec::new(),
                    Vec::new(),
                    ::diagnostics::enums::PermissionMode::Ask,
                    None,
                    false,
                );

                let mut policy_allows = Vec::new();
                let mut safe_allows = Vec::new();
                let mut denies = Vec::new();
                let mut asks = Vec::new();
                for (phase, tracker) in phases() {
                    let policy_allowed = bash("git remote -v");
                    let safe_allowed = bash("pwd");
                    let denied = bash("rm -rf /");
                    let asked = bash("cargo check");
                    for input in [&policy_allowed, &safe_allowed, &denied, &asked] {
                        assert_eq!(
                            plan_mode_edit_gate(&tracker, input),
                            PlanEditGate::Allow,
                            "{phase} must reach ordinary authorization"
                        );
                    }

                    let request = |command: &str, id: String| {
                        permissions.request_with_context(
                            AccessKind::Bash(command.into()),
                            acp::ToolCallUpdate::new(acp::ToolCallId::new(id), Default::default()),
                            None,
                            workspace::permission::types::PermissionRequestContext {
                                source:
                                    workspace::permission::types::PermissionRequestSource::Primary {
                                        session_id: None,
                                    },
                                request_mode: None,
                                within_capability_fence: false,
                                execution_cwd: None,
                                classifier_turns: None,
                                call_evidence: None,
                                human_interaction: None,
                            },
                        )
                    };
                    policy_allows.push((
                        phase,
                        request("git remote -v", format!("{phase}-policy-allow")).await,
                    ));
                    safe_allows.push((phase, request("pwd", format!("{phase}-safe-allow")).await));
                    denies.push((phase, request("rm -rf /", format!("{phase}-deny")).await));
                    asks.push((phase, request("cargo check", format!("{phase}-ask")).await));
                }

                let policy_allow_decision = policy_allows[0].1.clone();
                let safe_allow_decision = safe_allows[0].1.clone();
                let deny_decision = denies[0].1.clone();
                let ask_decision = asks[0].1.clone();
                assert!(
                    matches!(policy_allow_decision, Decision::Allow),
                    "baseline policy allow: {policy_allow_decision:?}"
                );
                assert!(
                    matches!(safe_allow_decision, Decision::Allow),
                    "baseline safe allow: {safe_allow_decision:?}"
                );
                assert!(
                    matches!(deny_decision, Decision::PolicyDeny(_)),
                    "baseline deny: {deny_decision:?}"
                );
                assert!(
                    matches!(ask_decision, Decision::Allow),
                    "ask path must receive the gateway selection: {ask_decision:?}"
                );
                for (phase, decision) in &policy_allows {
                    assert_eq!(
                        decision, &policy_allow_decision,
                        "{phase} policy allow diverged"
                    );
                }
                for (phase, decision) in &safe_allows {
                    assert_eq!(
                        decision, &safe_allow_decision,
                        "{phase} safe allow diverged"
                    );
                }
                for (phase, decision) in &denies {
                    assert_eq!(decision, &deny_decision, "{phase} deny diverged");
                }
                for (phase, decision) in &asks {
                    assert_eq!(decision, &ask_decision, "{phase} ask diverged");
                }

                let mut seen = std::collections::BTreeMap::<String, PermissionEvent>::new();
                while seen.len() < 20 {
                    let event =
                        tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
                            .await
                            .expect("permission event")
                            .expect("permission manager");
                    seen.insert(event.tool_id.clone(), event);
                }
                for phase in [
                    "normal",
                    "drafting",
                    "awaiting_approval",
                    "amending",
                    "executing",
                ] {
                    let policy_allowed = &seen[&format!("{phase}-policy-allow")];
                    let safe_allowed = &seen[&format!("{phase}-safe-allow")];
                    let denied = &seen[&format!("{phase}-deny")];
                    let asked = &seen[&format!("{phase}-ask")];
                    assert_eq!(policy_allowed.decision, "allow");
                    assert_eq!(
                        policy_allowed.decision_reason.as_deref(),
                        Some("policy_allow")
                    );
                    assert!(!policy_allowed.user_prompted);
                    assert_eq!(safe_allowed.decision, "allow");
                    assert_eq!(
                        safe_allowed.decision_reason.as_deref(),
                        Some("safe_command")
                    );
                    assert!(!safe_allowed.user_prompted);
                    assert_eq!(denied.decision, "reject");
                    assert_eq!(denied.decision_reason.as_deref(), Some("policy_deny"));
                    assert!(!denied.user_prompted);
                    assert_eq!(asked.decision, "allow");
                    assert!(asked.user_prompted, "{phase} did not reach the Ask gateway");
                }
                let prompted = prompts.lock().expect("prompt log");
                assert_eq!(
                    prompted.len(),
                    5,
                    "every phase prompts exactly once: {prompted:?}"
                );
            })
            .await;
    }
}

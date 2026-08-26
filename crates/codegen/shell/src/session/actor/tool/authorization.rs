//! Tool-call authorization projection and Plan/MCP admission helpers.

use super::*;

/// Keeps the persisted approval transport flag tied to the exact submitted
/// Plan snapshot while the [`SessionActor::request_plan_approval`] await is
/// live. A resolved decision clears it only through the matching phase CAS;
/// a dropped request rejects the same snapshot so a newer submission cannot
/// be stranded or changed by an old waiter.
///
/// It is deliberately preserved on the client-disconnect
/// (quit) path: there the approval is genuinely still pending, so the bit must
/// stay `true` on disk for the next resume to re-park it.
/// `BehaviorState` writes are immediate (no debounce), so writing `false` here
/// would race the quit and lose the gate.
pub(super) struct AwaitingApprovalGuard<'a> {
    actor: &'a SessionActor,
    expected: crate::session::behavior::BehaviorSnapshot,
    armed: bool,
}
impl AwaitingApprovalGuard<'_> {
    pub(super) fn new(actor: &SessionActor) -> AwaitingApprovalGuard<'_> {
        let expected = actor.behavior.lock().snapshot();
        AwaitingApprovalGuard {
            actor,
            expected,
            armed: true,
        }
    }

    /// A decision arrived. The caller still owns the phase transition and the
    /// pending bit remains set until that transition's CAS succeeds.
    pub(super) fn resolve(mut self) {
        self.armed = false;
    }

    /// Keep the approval durable so reconnect can re-park it.
    pub(super) fn preserve_for_resume(mut self) {
        self.armed = false;
    }
}
impl Drop for AwaitingApprovalGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut controller = self.actor.behavior.lock();
        if controller.reject_submitted_plan_if(&self.expected) {
            drop(controller);
            self.actor.record_control_snapshot();
        }
    }
}
pub(super) fn is_plan_control_kind(kind: Option<tools::types::tool::ToolKind>) -> bool {
    matches!(kind, Some(tools::types::tool::ToolKind::PlanControl))
}

pub(super) fn is_state_control_kind(kind: Option<tools::types::tool::ToolKind>) -> bool {
    matches!(
        kind,
        Some(
            tools::types::tool::ToolKind::PlanControl
                | tools::types::tool::ToolKind::GoalLifecycleUpdate
        )
    )
}

pub(super) fn is_goal_lifecycle_kind(kind: Option<tools::types::tool::ToolKind>) -> bool {
    matches!(
        kind,
        Some(tools::types::tool::ToolKind::GoalLifecycleUpdate)
    )
}

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
        | ToolInput::Skill(_)
        | ToolInput::TaskOutput(_)
        | ToolInput::MemorySearch(_)
        | ToolInput::MemoryGet(_)
        | ToolInput::ContextRecall(_)
        | ToolInput::Lsp(_)
        | ToolInput::SchedulerList(_)
        | ToolInput::GetGoal(_) => ToolAccess::Read,
        ToolInput::SearchReplace(_) | ToolInput::HashlineEdit(_) => ToolAccess::ReadWrite,
        ToolInput::Write(_) => ToolAccess::Write,
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
                WorkflowToolInput::Search { .. } | WorkflowToolInput::Inspect { .. } => {
                    ToolAccess::Read
                }
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
        | ToolInput::SearchTool(_)
        | ToolInput::PlanControl(_)
        | ToolInput::AskUserQuestion(_) => ToolAccess::None,
    }
}

pub(super) fn hash_canonical_json(value: &serde_json::Value) -> String {
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
pub(super) fn split_state_control_barrier(
    mut calls: Vec<crate::sampling::types::ToolCallResponse>,
    kind_of: impl Fn(&str) -> Option<tools::types::tool::ToolKind>,
) -> (
    Option<crate::sampling::types::ToolCallResponse>,
    Vec<crate::sampling::types::ToolCallResponse>,
) {
    let Some(index) = calls
        .iter()
        .position(|call| is_state_control_kind(kind_of(&call.function.name)))
    else {
        return (None, calls);
    };
    let control = calls.remove(index);
    (Some(control), calls)
}
/// Verdict for a tool call evaluated against the plan-mode edit gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlanEditGate {
    /// Execute normally (plan mode inactive or not an edit).
    Allow,
    /// Any ordinary file edit while Plan behavior is active.
    RejectEdit,
    /// Dynamic sub-planning is incompatible with an approved Plan contract.
    RejectWorkflow,
}
/// Gate edit-class tool calls while plan mode is active.
///
/// Every potentially mutating access class is rejected before the normal
/// permission flow. MCP calls are allowed only when the call's server is
/// config-declared query-only (`read_write`, or the stricter `read` mask);
/// unknown or write-capable MCP calls fail closed
/// while drafting/amending. Plan artifact
/// persistence is performed only by the shell control plane; Behavior
/// never grants an edit capability and never bypasses session permissions.
///
/// Read, grep, web fetch, and Plan control/question calls continue to the
/// ordinary permission path. Bash, edits, and non-read-only MCP calls are
/// blocked until the approved Plan enters Executing.
///
/// `mcp_scope`: the declared MCP server scope, or `None` for
/// non-MCP access kinds. Callers resolve it from `McpState` (async) BEFORE
/// taking the `behavior` lock.
pub(super) fn plan_mode_edit_gate(
    tracker: &crate::session::behavior::BehaviorCoordinator,
    tool_input: &ToolInput,
    access_kind: &AccessKind,
    mcp_max_access: Option<tool_protocol::ToolAccess>,
) -> PlanEditGate {
    if !tracker.is_plan() {
        return PlanEditGate::Allow;
    }
    if matches!(tool_input, ToolInput::Workflow(_)) {
        return PlanEditGate::RejectWorkflow;
    }
    if tracker.plan_allows_edits() {
        return PlanEditGate::Allow;
    }
    if let ToolInput::Task(task) = tool_input {
        return if task.capability_mode.unwrap_or_default()
            == tool_types::SubagentCapabilityMode::ReadOnly
        {
            PlanEditGate::Allow
        } else {
            PlanEditGate::RejectEdit
        };
    }
    match access_kind {
        AccessKind::Edit(_) | AccessKind::Bash(_) => PlanEditGate::RejectEdit,
        AccessKind::MCPTool { .. } => {
            if matches!(
                mcp_max_access,
                Some(tool_protocol::ToolAccess::Read | tool_protocol::ToolAccess::ReadWrite,)
            ) {
                PlanEditGate::Allow
            } else {
                PlanEditGate::RejectEdit
            }
        }
        AccessKind::Read(_)
        | AccessKind::Grep { .. }
        | AccessKind::WebFetch(_)
        | AccessKind::InternalControl { .. } => PlanEditGate::Allow,
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

/// Typed view of a Plan approval decision. The wire type carries `outcome` as
/// a string; both the mid-turn
/// intercept and the resume re-park match on this enum instead. Unknown /
/// unrecognized outcomes map to [`Cancelled`](Self::Cancelled) so the session
/// fails CLOSED (stays in plan mode) rather than auto-approving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlanApprovalOutcome {
    Approved,
    Cancelled,
    Abandoned,
}
impl PlanApprovalOutcome {
    pub(super) fn from_response(
        resp: &tools::implementations::grow_build::plan_control::PlanApprovalExtResponse,
    ) -> Self {
        match resp.outcome.as_str() {
            "approved" => Self::Approved,
            "abandoned" => Self::Abandoned,
            _ => Self::Cancelled,
        }
    }
}
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
/// Model-facing turn injected after a resumed plan is approved.
pub(super) const PLAN_APPROVED_IMPLEMENT_MESSAGE: &str =
    "The user approved the submitted plan. Proceed using the approved plan in context.";
/// Shared "revise the plan" message for the request-changes outcome, used by
/// both the mid-turn intercept and the resume re-park.
pub(super) fn revise_plan_message(feedback: &str) -> String {
    let feedback = feedback.trim();
    if feedback.is_empty() {
        "The user wants to revise the plan. \
         Ask the user what changes they would like to make."
            .to_string()
    } else {
        format!("The user wants to revise the plan. The user said:\n{feedback}")
    }
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

pub(super) async fn read_plan_artifact_async(
    session: std::sync::Arc<crate::session::storage::ContainedDirectory>,
    hash: String,
) -> std::io::Result<String> {
    tokio::task::spawn_blocking(move || {
        crate::session::behavior::read_plan_artifact(&session, &hash)
    })
    .await
    .map_err(std::io::Error::other)?
}
/// What the resume re-park does with the user's decision. Extracted
/// from `resume_plan_approval` so the branch logic is unit-testable without
/// driving a real turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResumeAction {
    /// Approved: leave plan mode and start an implement turn (Agent mode).
    LeaveAndImplement,
    /// Request changes: stay in plan mode and start a revise turn (Plan mode).
    StayAndRevise(String),
    /// Abandoned: leave plan mode and wait for the user (no turn).
    LeaveOnly,
}
pub(super) fn resume_action_for(
    outcome: PlanApprovalOutcome,
    feedback: Option<String>,
) -> ResumeAction {
    match outcome {
        PlanApprovalOutcome::Approved => ResumeAction::LeaveAndImplement,
        PlanApprovalOutcome::Cancelled => {
            ResumeAction::StayAndRevise(revise_plan_message(feedback.as_deref().unwrap_or("")))
        }
        PlanApprovalOutcome::Abandoned => ResumeAction::LeaveOnly,
    }
}

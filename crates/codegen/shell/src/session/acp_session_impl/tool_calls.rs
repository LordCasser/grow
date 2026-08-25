//! Tool-call execution concern for `SessionActor`: the model-output →
//! tool-execution pipeline (`execute_tool_calls`, `prepare_tool_call`,
//! tool-call start/success/error notifications, and sampling-event handling).
//!
//! `#[path]` child of `acp_session` (see the module comments there) so this
//! `impl SessionActor` block retains access to the actor's private fields and
//! the parent module's private helpers.
use super::*;
use crate::extensions::notification::SessionUpdate as GrowSessionUpdate;
use futures::StreamExt;
use tracing::Instrument;
/// Whether a tool name is an MCP `create_pull_request` (qualified
/// `server__create_pull_request` or bare).
fn is_mcp_create_pull_request(tool_name: &str) -> bool {
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
fn tool_execution_span(
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
fn record_tool_span_outcome(
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
fn consumed_completion_id_from_tool_error(error: &tool_runtime::ToolError) -> Option<&str> {
    error
        .details
        .as_ref()?
        .get("consumed_completion_task_id")?
        .as_str()
        .filter(|id| !id.trim().is_empty())
}

fn undispatched_tool_outcome(action: &ToolLoop) -> &'static str {
    match action {
        ToolLoop::Continue => "not_dispatched",
        ToolLoop::NonExistingTool | ToolLoop::ToolParsingError => "invalid_tool",
        ToolLoop::PermissionReject { .. } => "permission_rejected",
        ToolLoop::Cancelled => "permission_cancelled",
        ToolLoop::PermissionTimedOut { .. } => "permission_timed_out",
        ToolLoop::FollowupMessage(_) => "followup",
        ToolLoop::HookDenied { .. } => "hook_denied",
    }
}
/// Blocking wait tools that should abort when a mid-turn interjection is pending.
fn is_interruptible_wait_tool(tool_name: &str, args: &serde_json::Value) -> bool {
    match tool_name {
        "get_task_output"
        | "get_command_or_subagent_output"
        | "get_task_or_subagent_output"
        | "get_terminal_command_output" => tool_types::task_output_waits_from_json(args),
        "Await" | "AwaitShell" => true,
        _ => false,
    }
}
pub(super) async fn wait_for_pending_interjection(buf: &InterjectionBuffer<acp::ImageContent>) {
    buf.wait_nonempty().await;
}
use crate::tools::tool_context::BlockingWaitGuard;
/// Model-facing result when a wait is aborted for a pending interjection.
fn interrupted_wait_tool_result(args: &serde_json::Value) -> ToolRunResult {
    interrupted_wait_tool_result_with_msg(
        args,
        "Wait moved to background because the user sent a message. The task is still running and its completion will be delivered automatically.",
    )
}
/// [`interrupted_wait_tool_result`] with a caller-chosen model-facing message.
fn interrupted_wait_tool_result_with_msg(args: &serde_json::Value, msg: &str) -> ToolRunResult {
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
/// Clears the persisted approval transport flag when the
/// [`SessionActor::request_plan_approval`] await **resolves** (a decision came
/// back) or is **dropped** (the model turn was cancelled) — so a cancelled
/// in-session approval can never strand the bit `true`.
///
/// It is deliberately preserved on the client-disconnect
/// (quit) path: there the approval is genuinely still pending, so the bit must
/// stay `true` on disk for the next resume to re-park it.
/// `BehaviorState` writes are immediate (no debounce), so writing `false` here
/// would race the quit and lose the gate.
struct AwaitingApprovalGuard<'a> {
    actor: &'a SessionActor,
    armed: bool,
}
impl AwaitingApprovalGuard<'_> {
    fn new(actor: &SessionActor) -> AwaitingApprovalGuard<'_> {
        AwaitingApprovalGuard { actor, armed: true }
    }

    /// A decision arrived. The caller still owns the phase transition.
    fn resolve(mut self) {
        self.actor.behavior.lock().set_approval_pending(false);
        self.armed = false;
    }

    /// Keep the approval durable so reconnect can re-park it.
    fn preserve_for_resume(mut self) {
        self.armed = false;
    }
}
impl Drop for AwaitingApprovalGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut controller = self.actor.behavior.lock();
        if !controller.reject_submitted_plan() {
            controller.set_approval_pending(false);
        }
        drop(controller);
        self.actor.record_control_snapshot();
    }
}
pub(super) fn is_plan_control_kind(kind: Option<tools::types::tool::ToolKind>) -> bool {
    matches!(kind, Some(tools::types::tool::ToolKind::PlanControl))
}

fn public_workflow_conflict(
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
fn recognizable_shell_write(command: &str) -> bool {
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
fn shell_required_access(command: &str) -> tool_protocol::ToolAccess {
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
fn project_call_access(
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
        // Delegation and owner-scoped cancellation are exact-identity
        // framework controls. Reality-facing work performed by a child or a
        // background command remains independently gated at its origin.
        ToolInput::KillTask(_) | ToolInput::Task(_) => ToolAccess::None,
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
    fn trajectory_meta(&self) -> serde_json::Value {
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
    async fn issue_tool_call_permit(
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

fn recognizable_shell_write_paths(command: &str, cwd: &std::path::Path) -> Vec<String> {
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

fn shell_write_path(
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

fn recognizable_shell_write_under(
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

fn workflow_path_write(
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

fn normalized_path_under(root: &std::path::Path, path: &std::path::Path) -> bool {
    let root = resolve_existing_ancestors(root);
    let path = resolve_existing_ancestors(path);
    path == root || path.starts_with(&root)
}

/// Canonicalize the longest existing prefix and append the missing tail. This
/// makes admission follow the same directory symlinks that a later write will
/// follow, including when the final file does not exist yet.
fn resolve_existing_ancestors(path: &std::path::Path) -> std::path::PathBuf {
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

fn definition_edit_path(
    value: &str,
    cwd: &std::path::Path,
    display_cwd: Option<&std::path::Path>,
) -> std::path::PathBuf {
    tools::types::resources::resolve_model_path(cwd, display_cwd, value)
}

fn saved_workflow_definition_write(
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

fn session_workflow_definition_write(
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

fn workflow_definition_write(
    access_kind: &AccessKind,
    session_dir: &std::path::Path,
    cwd: &std::path::Path,
    display_cwd: Option<&std::path::Path>,
) -> bool {
    saved_workflow_definition_write(access_kind, cwd, display_cwd)
        || session_workflow_definition_write(access_kind, session_dir, cwd, display_cwd)
}

fn workflow_run_snapshot_write(
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
/// Run Plan lifecycle transitions after every ordinary call in the batch.
fn split_plan_control_tail(
    calls: Vec<crate::sampling::types::ToolCallResponse>,
    kind_of: impl Fn(&str) -> Option<tools::types::tool::ToolKind>,
) -> (
    Vec<crate::sampling::types::ToolCallResponse>,
    Vec<crate::sampling::types::ToolCallResponse>,
) {
    calls
        .into_iter()
        .partition(|call| !is_plan_control_kind(kind_of(&call.function.name)))
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
/// config-declared `max_access = "read_write"`;
/// unknown MCP calls fail closed while drafting/amending. Plan artifact
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
    match access_kind {
        AccessKind::Edit(_) | AccessKind::Bash(_) => PlanEditGate::RejectEdit,
        AccessKind::MCPTool { .. } => {
            if mcp_max_access == Some(tool_protocol::ToolAccess::ReadWrite) {
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
async fn mcp_call_max_access(
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
    fn from_response(
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
fn ext_method_no_client(err: &acp::Error) -> bool {
    matches!(
        acp_transport::acp_channel_failure(err),
        Some(acp_transport::AcpChannelFailure::SendFailed)
    )
}
/// Model-facing turn injected after a resumed plan is approved.
const PLAN_APPROVED_IMPLEMENT_MESSAGE: &str =
    "The user approved the submitted plan. Proceed using the approved plan in context.";
/// Shared "revise the plan" message for the request-changes outcome, used by
/// both the mid-turn intercept and the resume re-park.
fn revise_plan_message(feedback: &str) -> String {
    let feedback = feedback.trim();
    if feedback.is_empty() {
        "The user wants to revise the plan. \
         Ask the user what changes they would like to make."
            .to_string()
    } else {
        format!("The user wants to revise the plan. The user said:\n{feedback}")
    }
}

async fn write_plan_artifact_async(
    session: std::sync::Arc<crate::session::storage::ContainedDirectory>,
    markdown: String,
) -> std::io::Result<String> {
    tokio::task::spawn_blocking(move || {
        crate::session::behavior::write_plan_artifact(&session, &markdown)
    })
    .await
    .map_err(std::io::Error::other)?
}

async fn read_plan_artifact_async(
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
fn resume_action_for(outcome: PlanApprovalOutcome, feedback: Option<String>) -> ResumeAction {
    match outcome {
        PlanApprovalOutcome::Approved => ResumeAction::LeaveAndImplement,
        PlanApprovalOutcome::Cancelled => {
            ResumeAction::StayAndRevise(revise_plan_message(feedback.as_deref().unwrap_or("")))
        }
        PlanApprovalOutcome::Abandoned => ResumeAction::LeaveOnly,
    }
}
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

    fn stamp_tool_call_authority_meta(
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
        if let Some(cfg) = self.chat_state_handle.get_sampling_config().await {
            tracing::Span::current().record("model_id", cfg.model.as_str());
        }
        let mut final_result: Option<ToolLoop> = None;
        let mut deferred_followups: Vec<ConversationItem> = Vec::new();
        if tool_calls.len() > 1 {
            let kind_of = |name: &str| self.agent.borrow().tool_bridge().tool_kind(name);
            let (body, tail) = split_plan_control_tail(tool_calls, kind_of);
            if !body.is_empty() {
                self.execute_tool_calls_batch(body, &mut deferred_followups, &mut final_result)
                    .await?;
            }
            if !tail.is_empty() {
                self.execute_tool_calls_batch(tail, &mut deferred_followups, &mut final_result)
                    .await?;
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
        for mut call in tool_calls.into_iter() {
            if call.id.is_empty() {
                call.id = format!("synthetic-{}", uuid::Uuid::now_v7());
            }
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
                Ok(prepared) => approved.push(prepared),
                Err(tool_loop) => {
                    self.events
                        .tool_completed_durably(
                            &call_id,
                            undispatched_tool_outcome(&tool_loop).into(),
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
                    if matches!(
                        tool_loop,
                        ToolLoop::PermissionReject { .. }
                            | ToolLoop::Cancelled
                            | ToolLoop::PermissionTimedOut { .. }
                            | ToolLoop::FollowupMessage(_)
                    ) && final_result.is_none()
                    {
                        *final_result = Some(tool_loop);
                    }
                }
            }
        }
        if final_result.is_some() && !approved.is_empty() {
            let reason = match final_result.as_ref() {
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
                        post_tool_use_result = self
                            .hook_event_active(::hooks::event::HookEventName::PostToolUse)
                            .then(|| {
                                serde_json::to_value(&tool_result.output)
                                    .unwrap_or(serde_json::Value::Null)
                            });
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
                    ToolLoop::Continue
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
            if let Some(error) = post_tool_use_failure
                && self.hook_event_active(::hooks::event::HookEventName::PostToolUseFailure)
            {
                let raw_input: serde_json::Value = serde_json::from_str(&prepared.raw_arguments)
                    .unwrap_or(serde_json::Value::Null);
                let (tool_input_value, tool_input_truncated) =
                    ::hooks::event::truncate_payload(raw_input);
                let hook_tool_name = prepared.hook_tool_name();
                self.dispatch_hook(
                    ::hooks::event::HookEventName::PostToolUseFailure,
                    ::hooks::event::HookPayload::PostToolUseFailure {
                        tool_name: hook_tool_name.to_owned(),
                        tool_use_id: prepared.call_id.clone(),
                        tool_input: tool_input_value,
                        tool_input_truncated,
                        error,
                        subagent_type: self.subagent_type_label(),
                    },
                    None,
                    Some(hook_tool_name),
                )
                .await;
            } else if let Some(tool_result_value) = post_tool_use_result {
                let raw_input: serde_json::Value = serde_json::from_str(&prepared.raw_arguments)
                    .unwrap_or(serde_json::Value::Null);
                let (tool_input_value, tool_input_truncated) =
                    ::hooks::event::truncate_payload(raw_input);
                let (tool_result_val, tool_result_truncated) =
                    ::hooks::event::truncate_payload(tool_result_value);
                let hook_tool_name = prepared.hook_tool_name();
                self.dispatch_hook(
                    ::hooks::event::HookEventName::PostToolUse,
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
                    Some(hook_tool_name),
                )
                .await;
            }
            let tool_outcome = match &tool_loop {
                _ if tool_failed => crate::session::events::ToolOutcome::Error,
                ToolLoop::Continue => crate::session::events::ToolOutcome::Success,
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
            self.signals_handle().record_tool_duration(
                &prepared.tool_name,
                &tool_call_id,
                duration_ms,
            );
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
            match &tool_loop {
                ToolLoop::PermissionReject { .. }
                | ToolLoop::Cancelled
                | ToolLoop::PermissionTimedOut { .. }
                | ToolLoop::FollowupMessage(_) => {
                    if final_result.is_none() {
                        *final_result = Some(tool_loop);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// Phase 1: pre-flight (MCP, args, hooks, permission, PlanControl).
    pub(crate) async fn prepare_tool_call(
        &self,
        call: crate::sampling::types::ToolCallResponse,
        deferred_followups: &mut Vec<ConversationItem>,
    ) -> Result<Result<PreparedToolCall, ToolLoop>, acp::Error> {
        let tool_call_id = acp::ToolCallId::new(Arc::from(call.id.clone()));
        let model_id_str = self.current_model_id().await;
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
                    return Ok(Err(ToolLoop::NonExistingTool));
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
                if let Some(objects) = crate::session::helpers::tool_input_parsing::try_extract_concatenated_json_objects(
                    &call.function.arguments,
                ) {
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
                return Ok(Err(ToolLoop::ToolParsingError));
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
            return Ok(Err(ToolLoop::Continue));
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
                return Ok(Err(ToolLoop::Continue));
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
        if admitted_behavior == tool_types::BehaviorId::DeepResearch
            && required_access != tool_protocol::ToolAccess::Read
            && tool_kind != Some(tools::types::tool::ToolKind::WebFetch)
            && !(matches!(&access_kind, AccessKind::MCPTool { .. })
                && mcp_max_access == Some(tool_protocol::ToolAccess::ReadWrite))
        {
            let message = "Rejected: Deep Research foreground turns are read-only.".to_string();
            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                .await?;
            return Ok(Err(ToolLoop::Continue));
        }
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
            return Ok(Err(ToolLoop::Continue));
        }
        if (saved_workflow_write || workflow_draft_write)
            && (admitted_behavior != tool_types::BehaviorId::Workflow
                || current_behavior != tool_types::BehaviorId::Workflow)
        {
            let message = "Rejected: Grow may create or modify public Workflow Definitions only in Workflow behavior. Use /workflow [prompt]. External editor changes remain allowed and will be rediscovered."
                .to_string();
            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                .await?;
            return Ok(Err(ToolLoop::Continue));
        }
        if saved_workflow_write {
            let message = "Rejected: saved Workflow Definitions are replaced only by publishing a validated session draft. Derive the saved Definition into the Workflow workspace, edit the draft, then publish it; the saved Definition remains usable until then."
                .to_string();
            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                .await?;
            return Ok(Err(ToolLoop::Continue));
        }
        if workflow_run_snapshot_write(&access_kind, &session_dir, cwd, display_cwd) {
            let message = "Rejected: Workflow Run scripts, args, and journals are immutable snapshots. Modify or derive the Definition and start a new Run instead."
                .to_string();
            self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                .await?;
            return Ok(Err(ToolLoop::Continue));
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
            return Ok(Err(ToolLoop::Continue));
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
        if self.hook_event_active(::hooks::event::HookEventName::PreToolUse) {
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
            let hook_registry_snapshot = self.hooks.registry.borrow().clone();
            if let Some(registry) = hook_registry_snapshot {
                let ctx = self.hook_run_ctx();
                let pre_result =
                    ::hooks::dispatcher::dispatch_pre_tool_use(&registry, &envelope, &ctx).await;
                self.send_hook_execution(
                    "pre_tool_use",
                    Some(&resolved_tool_name),
                    None,
                    &pre_result.results,
                )
                .await;
                self.emit_hook_executed_diagnostics(
                    "pre_tool_use",
                    Some(&resolved_tool_name),
                    &pre_result.results,
                )
                .await;
                if let ::hooks::result::HookDecision::Deny { reason, hook_name } =
                    pre_result.decision
                {
                    return Ok(Err(self
                        .deny_tool(
                            &call.id,
                            &tool_call_id,
                            resolved_tool_name.clone(),
                            hook_name,
                            reason,
                        )
                        .await?));
                }
            }
            if let Some(denied) = self
                .run_pre_tool_use_client_hook(&call, &tool_call_id, &envelope)
                .await?
            {
                return Ok(Err(denied));
            }
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
                .await;
            }
            let classifier_turns = if effective_mode
                == workspace::permission::types::EffectivePermissionMode::Auto
                && !within_capability_fence
            {
                let conv = self.chat_state_handle.get_conversation().await;
                let turns = super::build_classifier_turns(&conv, super::CLASSIFIER_REFRESH_TURNS);
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
                    self.dispatch_hook(
                        ::hooks::event::HookEventName::PermissionDenied,
                        ::hooks::event::HookPayload::PermissionDenied {
                            tool_name: resolved_tool_name.clone(),
                            tool_use_id: tool_call_id.to_string(),
                            tool_input: tool_input_value,
                            tool_input_truncated,
                        },
                        None,
                        Some(&resolved_tool_name),
                    )
                    .await;
                    let loop_action = if is_policy_deny || child_nonterminal {
                        ToolLoop::Continue
                    } else {
                        ToolLoop::PermissionReject {
                            tool_name: call.function.name.clone(),
                            reason: reason.clone(),
                        }
                    };
                    return Ok(Err(loop_action));
                }
                Decision::Cancelled => {
                    let message = format!(
                        "User cancelled the execution for tool `{}`",
                        call.function.name
                    );
                    self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                        .await?;
                    return Ok(Err(ToolLoop::Cancelled));
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
                        return Ok(Err(ToolLoop::Continue));
                    }
                    return Ok(Err(ToolLoop::PermissionTimedOut {
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
                    return Ok(Err(ToolLoop::FollowupMessage(followup_message)));
                }
                Decision::Allow | Decision::Ask => {}
            }
        }
        if let ToolInput::PlanControl(input) = &tool_input {
            use tools::implementations::grow_build::plan_control::PlanControlAction;
            if matches!(
                input.action,
                PlanControlAction::Complete | PlanControlAction::Cancel
            ) {
                if input.plan.is_some() {
                    self.handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        "Rejected: plan_control complete/cancel must not include `plan`."
                            .to_owned(),
                    )
                    .await?;
                    return Ok(Err(ToolLoop::Continue));
                }
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
                    return Ok(Err(ToolLoop::Continue));
                }
                if let Err(message) = self.finish_plan_to_default().await {
                    self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                        .await?;
                    return Ok(Err(ToolLoop::Continue));
                }
            } else {
                let valid = match input.action {
                    PlanControlAction::Submit => self.behavior.lock().is_drafting_plan(),
                    PlanControlAction::Amend => {
                        self.behavior.lock().state()
                            == crate::session::behavior::BehaviorState::Plan(
                                crate::session::behavior::PlanPhase::Executing,
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
                    return Ok(Err(ToolLoop::Continue));
                }
                let plan_content = input.plan.as_deref().unwrap_or_default().trim().to_owned();
                if plan_content.is_empty() {
                    self.handle_tool_not_executed(
                        &call.id,
                        &tool_call_id,
                        "Rejected: plan_control submit/amend requires a non-empty `plan` argument."
                            .to_owned(),
                    )
                    .await?;
                    return Ok(Err(ToolLoop::Continue));
                }

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
                        return Ok(Err(ToolLoop::Continue));
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
                    return Ok(Err(ToolLoop::Continue));
                }
                if let Err(message) = self
                    .commit_behavior_mutation_or_restore(previous_behavior)
                    .await
                {
                    self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                        .await?;
                    return Ok(Err(ToolLoop::Continue));
                }

                tracing::info!(
                    tool_call_id = %tool_call_id,
                    "plan_control intercepted; requesting user approval"
                );
                let resp = self
                    .request_plan_approval(&tool_call_id, plan_content.clone())
                    .await;
                match resp {
                    Ok(parsed) => match PlanApprovalOutcome::from_response(&parsed) {
                        PlanApprovalOutcome::Abandoned => {
                            tracing::info!("plan_control: user abandoned Plan");
                            if let Err(message) = self.finish_plan_to_default().await {
                                self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                                    .await?;
                                return Ok(Err(ToolLoop::Continue));
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
                            return Ok(Err(ToolLoop::Continue));
                        }
                        PlanApprovalOutcome::Cancelled => {
                            let previous_behavior = self.behavior.lock().snapshot();
                            self.behavior.lock().reject_submitted_plan();
                            if let Err(message) = self
                                .commit_behavior_mutation_or_restore(previous_behavior)
                                .await
                            {
                                self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                                    .await?;
                                return Ok(Err(ToolLoop::Continue));
                            }
                            let message =
                                revise_plan_message(parsed.feedback.as_deref().unwrap_or(""));
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
                            return Ok(Err(ToolLoop::Continue));
                        }
                        PlanApprovalOutcome::Approved => {
                            let previous_behavior = self.behavior.lock().snapshot();
                            if !self.behavior.lock().approve_submitted_plan() {
                                self.handle_tool_not_executed(
                                    &call.id,
                                    &tool_call_id,
                                    "Plan approval arrived in an invalid phase.".to_owned(),
                                )
                                .await?;
                                return Ok(Err(ToolLoop::Continue));
                            }
                            if let Err(message) = self
                                .commit_behavior_mutation_or_restore(previous_behavior)
                                .await
                            {
                                self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                                    .await?;
                                return Ok(Err(ToolLoop::Continue));
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
                            self.behavior.lock().reject_submitted_plan();
                            if let Err(message) = self
                                .commit_behavior_mutation_or_restore(previous_behavior)
                                .await
                            {
                                self.handle_tool_not_executed(&call.id, &tool_call_id, message)
                                    .await?;
                                return Ok(Err(ToolLoop::Continue));
                            }
                            self.handle_tool_not_executed(
                            &call.id,
                            &tool_call_id,
                            "Plan approval requires an interactive user. No approval client is connected, so execution remains blocked.".to_owned(),
                        )
                        .await?;
                            return Ok(Err(ToolLoop::Continue));
                        } else {
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
                            return Ok(Err(ToolLoop::Cancelled));
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
                return Ok(Err(ToolLoop::Continue));
            }
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
        };
        Ok(Ok(prepared))
    }
    /// Issue the `grow/plan_approval` reverse request and await the user's
    /// decision. Shared by the PlanControl intercept and the resume
    /// re-park. Marks approval transport as pending while the request is
    /// outstanding and clears it on every exit path via [`AwaitingApprovalGuard`].
    pub(super) async fn request_plan_approval(
        &self,
        tool_call_id: &acp::ToolCallId,
        plan_content: String,
    ) -> Result<tools::implementations::grow_build::plan_control::PlanApprovalExtResponse, acp::Error>
    {
        use agent_client_protocol::Client as _;
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
        .await;
        debug_assert!(self.behavior.lock().approval_pending());
        let approval_guard = AwaitingApprovalGuard::new(self);
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
                approval_guard.preserve_for_resume();
                return Err(err);
            }
        };
        let parsed =
            serde_json::from_str::<PlanApprovalExtResponse>(raw.0.get()).unwrap_or_else(|_| {
                PlanApprovalExtResponse {
                    outcome: "cancelled".into(),
                    feedback: None,
                }
            });
        approval_guard.resolve();
        Ok(parsed)
    }
    /// Leave plan mode (approved/abandoned) and tell the client to show the
    /// Default mode. Mirrors the mid-turn exit so the resume re-park
    /// drives the mode change through the same path.
    async fn finish_plan_to_default(&self) -> Result<(), String> {
        let previous_behavior = self.behavior.lock().snapshot();
        let deactivated = self.behavior.lock().finish_plan();
        if deactivated {
            self.commit_behavior_mutation_or_restore(previous_behavior)
                .await?;
            self.enqueue_current_mode_update(acp::SessionModeId::new(
                tools::types::BehaviorId::Normal.as_id(),
            ));
        }
        Ok(())
    }
    /// Resume hook: re-issue the parked Plan approval
    /// after a session restored with `approval_pending == true`, so the
    /// client re-shows approval chrome over a real live waiter. Handles the
    /// decision with no in-flight turn — approve: leave plan mode + start an
    /// implement turn; request-changes: stay in plan mode + feed the comments
    /// back as a turn; abandon: leave plan mode and wait for the user.
    pub(super) async fn resume_plan_approval(
        self: Arc<Self>,
        completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
    ) {
        if !self.behavior.lock().approval_pending() {
            return;
        }
        if crate::session::pending_interaction::has_parked_plan_approval(&self.pending_interactions)
        {
            tracing::debug!("plan_control resume: approval already pending; skip re-park");
            return;
        }
        let artifact_hash = self.behavior.lock().plan_artifact_hash().map(str::to_owned);
        let plan_content = match artifact_hash {
            Some(hash) => {
                match read_plan_artifact_async(self.session_directory.clone(), hash).await {
                    Ok(content) if !content.trim().is_empty() => content,
                    _ => {
                        tracing::info!(
                            "plan_control resume: candidate artifact is unavailable; clearing approval state"
                        );
                        self.behavior.lock().set_approval_pending(false);
                        self.record_control_snapshot();
                        return;
                    }
                }
            }
            _ => {
                tracing::info!("plan_control resume: no candidate plan; clearing approval state");
                self.behavior.lock().set_approval_pending(false);
                self.record_control_snapshot();
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
        match resume_action_for(PlanApprovalOutcome::from_response(&parsed), parsed.feedback) {
            ResumeAction::LeaveOnly => {
                tracing::info!("plan_control resume: user abandoned Plan");
                if let Err(error) = self.finish_plan_to_default().await {
                    tracing::warn!(%error, "failed to persist abandoned Plan on resume");
                }
            }
            ResumeAction::StayAndRevise(text) => {
                tracing::info!("plan_control resume: user requested changes");
                let previous_behavior = self.behavior.lock().snapshot();
                self.behavior.lock().reject_submitted_plan();
                if self
                    .commit_behavior_mutation_or_restore(previous_behavior)
                    .await
                    .is_ok()
                {
                    self.start_resume_turn(text, completion_tx).await;
                }
            }
            ResumeAction::LeaveAndImplement => {
                tracing::info!("plan_control resume: user approved Plan");
                let previous_behavior = self.behavior.lock().snapshot();
                self.behavior.lock().approve_submitted_plan();
                if self
                    .commit_behavior_mutation_or_restore(previous_behavior)
                    .await
                    .is_ok()
                {
                    self.start_resume_turn(
                        PLAN_APPROVED_IMPLEMENT_MESSAGE.to_string(),
                        completion_tx,
                    )
                    .await;
                }
            }
        }
    }
    /// Inject a synthetic user turn after a resumed plan decision and kick the
    /// scheduler (no in-flight turn exists on resume to continue).
    async fn start_resume_turn(
        self: Arc<Self>,
        text: String,
        completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
    ) {
        let prompt_id = format!("plan-resume-{}", chrono::Utc::now().timestamp_millis());
        let prompt_blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(text))];
        let (respond_to, _rx) = oneshot::channel();
        self.queue_input(
            prompt_blocks,
            prompt_id,
            crate::session::PromptOrigin::PlanResume,
            crate::session::TurnKind::Internal,
            None,
            None,
            false,
            None,
            respond_to,
            None,
        )
        .await;
        SessionActor::maybe_start_running_task(self.clone(), completion_tx).await;
    }
    /// Refine the initial (minimal) ToolCall that was registered during
    /// tool preparation.  Now that we have a fully parsed `ToolInput`
    /// we can send a `ToolCallUpdate` with a human-readable title, the correct
    /// kind, file locations, and the serialised raw input.
    ///
    /// Returns `(title, kind, raw_input)` so callers can reuse them (e.g. in
    /// the permission-request update for subagent sessions whose prior
    /// `SessionUpdate` events the client may have suppressed).
    async fn send_tool_call_start(
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
    async fn handle_tool_parse_error(
        &self,
        tool_call_id: &acp::ToolCallId,
        call_id: &str,
        function_name: &str,
        err: tool_runtime::ToolError,
        raw_arguments: &str,
        model_id: &str,
    ) -> Result<(), acp::Error> {
        tracing::error!(
            session_id = %self.session_info.id.0,
            tool_name = function_name,
            model_id = model_id,
            error_kind = "parse_failure",
            error_message = %err,
            "tool_error: parse_failure"
        );
        self.signals_handle().record_tool_failure(function_name);
        let message = build_tool_parse_error_message(function_name, &err, raw_arguments);
        self.send_update(
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                tool_call_id.clone(),
                acp::ToolCallUpdateFields::new()
                    .status(Some(acp::ToolCallStatus::Failed))
                    .content(Some(vec![acp::ToolCallContent::from(
                        acp::ContentBlock::Text(acp::TextContent::new(message.clone())),
                    )])),
            )),
            None,
        )
        .await;
        let tool_chat = ConversationItem::tool_result(call_id.to_string(), message);
        self.chat_state_handle.push_tool_result(tool_chat);
        Ok(())
    }
    /// Remove queued inbox promotions for completion IDs that a successful
    /// tool result is about to surface. The durable receipts themselves are
    /// acknowledged only after that tool result reaches chat state.
    ///
    /// The exhaustive ID classification is centralized in
    /// `tools::reminders::task_completion::consumed_completion_ids`.
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
    async fn acknowledge_consumed_notifications(&self, consumed_ids: &[&str]) {
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
                chat_state::NotificationSource::SubagentCompleted { subagent_id } => {
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
            .record_notification_resolution_durably(chat_state::NotificationEvent::Consumed {
                notification_ids,
                turn,
                input: None,
            })
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
    pub(super) async fn drop_pending_synthetic_items(&self) {
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

    pub(super) async fn handle_bridge_tool_success(
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
        if let ToolsToolOutput::BackgroundTaskStarted(ref bg) = result.output {
            self.record_goal_turn_task_ids([bg.task_id.clone()]);
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
            let path = tool_parsed_args
                .get("target_file")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            use crate::session::image_normalize::{InlineAttachVerdict, inline_attach_verdict};
            match inline_attach_verdict(&image_content.data) {
                InlineAttachVerdict::TooSmall => {
                    prompt_text = format!(
                        "[Image from {path} was not attached: too small for vision models]"
                    );
                }
                InlineAttachVerdict::Unreadable => {
                    prompt_text = format!(
                        "[Image from {path} was not attached: invalid or unreadable image data]"
                    );
                }
                InlineAttachVerdict::Attach => {
                    let url = format!(
                        "data:{};base64,{}",
                        image_content.mime_type, image_content.data
                    );
                    inline_images.push(ContentPart::Image {
                        url: std::sync::Arc::<str>::from(url),
                    });
                    prompt_text = format!("Read image file: {path}");
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
                crate::session::context_recall::context_recall_max_context_tokens(context_window);
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
            let acp_images: Vec<agent_client_protocol::ImageContent> = extracted_images
                .into_iter()
                .map(|img| agent_client_protocol::ImageContent::new(img.data, img.mime_type))
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
    pub(super) async fn handle_tool_not_executed(
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
/// Execute tool-call display parts. The title peels a redundant leading
/// `cd <cwd>` for chrome only; `raw_input` is serialized separately and stays full.
fn execute_tool_call_parts(
    command: &str,
    description: Option<&str>,
    cwd: &std::path::Path,
) -> (
    String,
    acp::ToolKind,
    Vec<acp::ToolCallLocation>,
    Vec<acp::ToolCallContent>,
) {
    let display = tools::util::strip_redundant_session_cd(command, cwd);
    (
        format!("Execute `{display}`"),
        acp::ToolKind::Execute,
        Vec::new(),
        vec![acp::ToolCallContent::from(acp::ContentBlock::Text(
            acp::TextContent::new(description.unwrap_or_default().to_string()),
        ))],
    )
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
        WorkflowDefinitionId, WorkflowDraftSource, WorkflowToolInput,
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
                WorkflowToolInput::Inspect {
                    definition_id: id.clone(),
                    include_source: true,
                },
                ToolAccess::Read,
            ),
            (
                WorkflowToolInput::Validate {
                    definition_id: id,
                    args: None,
                    agent_budget: None,
                },
                ToolAccess::All,
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
    fn delegation_and_owner_cleanup_are_framework_controls() {
        let task = ToolInput::Task(TaskToolInput {
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
        let kill = ToolInput::KillTask(KillTaskToolInput {
            task_id: "owned-task".into(),
        });
        assert_eq!(
            project_call_access(&task, ToolAccess::None, None),
            ToolAccess::None
        );
        assert_eq!(
            project_call_access(&kill, ToolAccess::None, None),
            ToolAccess::None
        );
    }
}
#[cfg(test)]
mod plan_control_tail_predicate_tests {
    use super::{is_plan_control_kind, split_plan_control_tail};
    use tools::types::ToolInput;
    use tools::types::tool::ToolKind;
    fn call(name: &str, args: &str) -> crate::sampling::types::ToolCallResponse {
        crate::sampling::types::ToolCallResponse {
            id: format!("call_{name}"),
            kind: "function".into(),
            function: crate::sampling::types::ToolCallFunction::new(name, args),
        }
    }
    /// Wire name does not matter — only [`ToolKind::PlanControl`].
    fn kind_of(name: &str) -> Option<ToolKind> {
        match name {
            "plan_control" | "PlanControl" => Some(ToolKind::PlanControl),
            _ => None,
        }
    }
    #[test]
    fn plan_control_kind_is_protocol_boundary() {
        assert!(is_plan_control_kind(Some(ToolKind::PlanControl)));
        assert!(!is_plan_control_kind(Some(ToolKind::Edit)));
        assert!(!is_plan_control_kind(None));
    }
    fn mixed(calls: Vec<crate::sampling::types::ToolCallResponse>) -> bool {
        let (body, tail) = split_plan_control_tail(calls, kind_of);
        !body.is_empty() && !tail.is_empty()
    }
    #[test]
    fn split_puts_plan_control_in_tail() {
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
        assert!(mixed(vec![write.clone(), exit.clone()]));
        assert!(mixed(vec![exit.clone(), write.clone()]));
        assert!(!mixed(vec![write.clone(), unknown_alias]));
        assert!(!mixed(vec![exit.clone()]));
        assert!(!mixed(vec![write.clone()]));
        assert!(!mixed(vec![write.clone(), proposal.clone()]));
        assert!(mixed(vec![write, exit, proposal]));
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
    use super::{
        PlanApprovalOutcome, ResumeAction, ext_method_no_client, resume_action_for,
        revise_plan_message,
    };
    use tools::implementations::grow_build::plan_control::PlanApprovalExtResponse;
    fn resp(outcome: &str) -> PlanApprovalExtResponse {
        PlanApprovalExtResponse {
            outcome: outcome.into(),
            feedback: None,
        }
    }
    #[test]
    fn outcome_from_response_maps_known_and_fails_closed() {
        assert_eq!(
            PlanApprovalOutcome::from_response(&resp("approved")),
            PlanApprovalOutcome::Approved
        );
        assert_eq!(
            PlanApprovalOutcome::from_response(&resp("abandoned")),
            PlanApprovalOutcome::Abandoned
        );
        assert_eq!(
            PlanApprovalOutcome::from_response(&resp("cancelled")),
            PlanApprovalOutcome::Cancelled
        );
        assert_eq!(
            PlanApprovalOutcome::from_response(&resp("approve")),
            PlanApprovalOutcome::Cancelled
        );
        assert_eq!(
            PlanApprovalOutcome::from_response(&resp("")),
            PlanApprovalOutcome::Cancelled
        );
    }
    #[test]
    fn ext_method_no_client_defaults_false_for_untagged_error() {
        assert!(!ext_method_no_client(&acp_transport::acp_internal_error(
            "unrelated internal error"
        )));
    }
    #[test]
    fn revise_plan_message_includes_feedback_when_present() {
        assert!(revise_plan_message("").contains("Ask the user what changes"));
        assert!(revise_plan_message("   ").contains("Ask the user what changes"));
        let with = revise_plan_message("use async");
        assert!(with.contains("The user said:"));
        assert!(with.contains("use async"));
    }
    #[test]
    fn resume_action_maps_each_outcome() {
        assert_eq!(
            resume_action_for(PlanApprovalOutcome::Approved, None),
            ResumeAction::LeaveAndImplement
        );
        assert_eq!(
            resume_action_for(PlanApprovalOutcome::Abandoned, Some("ignored".into())),
            ResumeAction::LeaveOnly
        );
        match resume_action_for(PlanApprovalOutcome::Cancelled, Some("tweak it".into())) {
            ResumeAction::StayAndRevise(text) => assert!(text.contains("tweak it")),
            other => panic!("expected StayAndRevise, got {other:?}"),
        }
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
        let buf: InterjectionBuffer<agent_client_protocol::ImageContent> =
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
            auto_promoted: None,
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

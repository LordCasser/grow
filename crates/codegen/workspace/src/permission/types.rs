use agent_client_protocol as acp;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
/// A permission event capturing the decision made for a tool call.
/// Used for diagnostics to track permission patterns and user behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionEvent {
    /// Process-local sequence used to order the asynchronous audit bridge
    /// before a primary TurnCompleted boundary. It is transport metadata, not
    /// part of the durable diagnostic schema.
    #[serde(skip)]
    pub audit_sequence: u64,
    /// Tool call ID from the model
    pub tool_id: String,
    /// Name of the tool being executed
    pub tool_name: String,
    /// Type of access requested (read, edit, bash, mcp)
    pub access_kind: String,
    /// Additional context (e.g., file path for edit, command for bash)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_detail: Option<String>,
    /// Whether this was auto-approved (by always-approve mode or policy rules)
    pub auto_approved: bool,
    /// Whether the user was prompted for this decision
    pub user_prompted: bool,
    /// The final decision (allow, reject, cancelled, timed_out, followup)
    pub decision: String,
    /// The prompt resolution (allow_once, reject_once, timed_out, etc.);
    /// None on auto/non-prompt decisions. The trigger lives in `decision_reason`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_outcome: Option<String>,
    /// Rejection reason if rejected
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reject_reason: Option<String>,
    /// When this decision was made
    pub timestamp: DateTime<Utc>,
    /// If this permission was requested by a subagent, the subagent's session ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_session_id: Option<String>,
    /// If this permission was requested by a subagent, its type (e.g. "explore").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    /// If this permission was requested by a subagent, its description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_description: Option<String>,
    /// Effective permission mode governing this decision (not the trigger):
    /// "ask" | "auto" | "always-approve". Hyphenated to match
    /// `config.ui.permission_mode` in the same trace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// Request-local route before resolving `follow` against the primary
    /// session: "ask" | "auto" | "always-approve" | "follow".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_permission_mode: Option<String>,
    /// Structured capability target for capability-grant events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_target: Option<String>,
    /// Child-provided task purpose for capability-grant events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_purpose: Option<String>,
    /// The trigger that produced this decision, distinct from `prompt_outcome`
    /// (which records the user's choice when prompted). Lets a trace show *why*
    /// a request reached a prompt even when `user_prompted=true`. Values:
    /// always_approve, policy_allow, policy_deny, policy_ask, bash_command_gate_ask,
    /// shell_file_gate_ask, auto_fast_path,
    /// auto_classifier_allow, auto_classifier_block, auto_classifier_deny,
    /// auto_classifier_timeout, auto_classifier_unavailable, auto_denial_limit,
    /// sandbox_auto, persisted_grant, session_grant, static_allowlist, safe_command,
    /// session_deny, prompt_deny, needs_user, bash_request_floor, opaque_shell,
    /// requester_gone, permission_timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_reason: Option<String>,
    /// Auto-classifier path: "llm" | "heuristic" | "timeout" |
    /// "transport_error" | "fast_path".
    /// Absent when auto mode did not classify or take its fast path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier_source: Option<String>,
    /// Structured classifier verdict before it is mapped to the final
    /// permission decision: "allow" | "block" | "unavailable".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier_verdict: Option<String>,
    /// Concise model/failure reason. The model's hidden reasoning is never
    /// retained in permission events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier_reason: Option<String>,
    /// Elapsed milliseconds spent in classification alone, including heuristic work;
    /// absent when no classifier ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier_latency_ms: Option<u64>,
    /// Consecutive auto-classifier denials at decision time; absent outside auto mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_denials_consecutive: Option<u32>,
    /// Total auto-classifier denials at decision time; absent outside auto mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_denials_total: Option<u32>,
    /// Elapsed milliseconds from the actor dequeuing this request to the decision
    /// resolving. The timer starts at dequeue, so it excludes time the request
    /// waited in the channel behind others; small for fast auto paths but
    /// non-trivial when an auto classifier side-query runs before the decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
    /// Concurrent in-flight permission requests (this one included) at emit time,
    /// counted across the shared handle so overlapping subagent requests show up.
    /// The per-turn "hit yes N times" count is instead the number of
    /// `user_prompted=true` events in the turn, not this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_depth: Option<u32>,
}
/// Identifies the type of client connecting to the agent.
/// Used to determine which permission UI features to enable
/// and which feedback/experiment client type to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ClientType {
    /// Generic client - show simple permission options with full command text
    #[default]
    #[serde(rename = "generic")]
    Generic,
    /// Grow TUI client - show fancy options with interactive bash term selection
    #[serde(rename = "grow-tui")]
    GrowTUI,
    /// Grow Web client - identified by clientIdentifier "grow-web"
    #[serde(rename = "grow-web")]
    GrowWeb,
    /// Named client (`"nebula"`) — uses the generic permission UI
    #[serde(rename = "nebula")]
    Nebula,
    /// IDE extension client (VS Code and similar) - identified by clientIdentifier "grow-code-extension"
    #[serde(rename = "extension")]
    Extension,
    /// Grow Pager client - TUI-like terminal pager with interactive permission UI.
    /// Treated identically to GrowTUI for permission options (gets bash highlights +
    /// interactive selection). Reports as "grow-pager" for diagnostics attribution.
    #[serde(rename = "grow-pager")]
    GrowPager,
}
impl ClientType {
    /// Product token for the `User-Agent` header (e.g. `grow-pager`).
    pub fn user_agent_label(&self) -> &'static str {
        match self {
            Self::Generic => "grow-shell",
            Self::GrowTUI => "grow-tui",
            Self::GrowWeb => "grow-web",
            Self::Nebula => "nebula",
            Self::Extension => "grow-code-extension",
            Self::GrowPager => "grow-pager",
        }
    }
    /// Resolve from an ACP `clientIdentifier` string.
    pub fn from_client_identifier(id: Option<&str>) -> Self {
        match id {
            Some("grow-web") => Self::GrowWeb,
            Some("nebula") => Self::Nebula,
            Some("grow-code-extension") => Self::Extension,
            Some("grow-pager") => Self::GrowPager,
            _ => Self::Generic,
        }
    }
    /// Label for feedback reporting and experiment filtering.
    pub fn feedback_label(&self) -> &'static str {
        match self {
            Self::GrowTUI | Self::GrowPager => "tui",
            Self::GrowWeb => "web",
            Self::Nebula => "nebula",
            Self::Extension => "extension",
            Self::Generic => "agent",
        }
    }
}
#[derive(Clone, Debug)]
pub enum AccessKind {
    Read(Option<String>),
    Grep {
        path: Option<String>,
        glob: Option<String>,
    },
    Edit(String),
    Bash(String),
    /// An MCP tool call: the tool name plus its raw JSON args. The args are
    /// carried so the auto-mode classifier (and diagnostics) can judge what the
    /// call actually does, not just its name.
    MCPTool {
        name: String,
        input: serde_json::Value,
    },
    WebFetch(String),
    /// A subagent request to expose a capability that is already inside its
    /// hard eligibility ceiling. The eventual tool call is authorized again.
    CapabilityGrant {
        target: String,
        purpose: String,
    },
}

/// Request-local permission mode used by subagent sessions.
///
/// The permission actor remains shared with the primary session so policy,
/// prompting, and the auto classifier have one implementation. A child may
/// nevertheless choose an independent decision route for each request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestPermissionMode {
    Ask,
    #[default]
    Auto,
    AlwaysApprove,
    /// Resolve against the shared primary session's live mode.
    Follow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectivePermissionMode {
    Ask,
    Auto,
    AlwaysApprove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionRequestSource {
    Primary {
        session_id: Option<String>,
    },
    Child {
        session_id: String,
        subagent_type: Option<String>,
        subagent_description: Option<String>,
    },
}

impl Default for PermissionRequestSource {
    fn default() -> Self {
        Self::Primary { session_id: None }
    }
}

impl PermissionRequestSource {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Primary { session_id } => session_id.as_deref(),
            Self::Child { session_id, .. } => Some(session_id),
        }
    }

    pub fn child_session_id(&self) -> Option<&str> {
        match self {
            Self::Primary { .. } => None,
            Self::Child { session_id, .. } => Some(session_id),
        }
    }

    pub fn subagent_type(&self) -> Option<&str> {
        match self {
            Self::Primary { .. } => None,
            Self::Child { subagent_type, .. } => subagent_type.as_deref(),
        }
    }

    pub fn subagent_description(&self) -> Option<&str> {
        match self {
            Self::Primary { .. } => None,
            Self::Child {
                subagent_description,
                ..
            } => subagent_description.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PermissionRequestContext {
    pub source: PermissionRequestSource,
    pub request_mode: Option<RequestPermissionMode>,
    /// The child capability authority already admitted this concrete tool
    /// call. The permission manager still enforces managed deny/ask rules and
    /// hard safety floors, but ordinary in-fence calls must not invoke the
    /// child Auto classifier or produce approval audit noise.
    pub within_capability_fence: bool,
    /// Filesystem base used by the eventual tool call. Child sessions may run
    /// in a worktree or explicit cwd that differs from the shared manager's.
    pub execution_cwd: Option<std::path::PathBuf>,
    /// Request-local transcript. `Some(empty)` deliberately clears stale
    /// context for this source; `None` retains the last source-local snapshot.
    pub classifier_turns: Option<Vec<super::auto_mode::ClassifierTurn>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// A policy `ask` rule matched; prompt the user.
    Ask,
    FollowupMessage(String),
    Reject(String),
    /// A policy deny rule matched. Distinguished from `Reject` (user-initiated)
    /// so the caller can return the error to the LLM instead of cancelling
    /// the turn — the agent should see the denial and adapt.
    PolicyDeny(String),
    /// The user cancelled the turn (e.g. Cmd+C during permission prompt).
    /// Distinguished from `Reject` so the caller can return `StopReason::Cancelled`.
    Cancelled,
    /// The permission client did not answer before the session deadline.
    /// The tool must not execute. Primary turns treat this as terminal; child
    /// turns convert it to a failed tool result and continue sampling.
    TimedOut,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditPolicy {
    #[default]
    Ask,
    Reject,
}
impl Serialize for EditPolicy {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::Ask => "ask",
            Self::Reject => "reject",
        })
    }
}
impl<'de> Deserialize<'de> for EditPolicy {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl serde::de::Visitor<'_> for V {
            type Value = EditPolicy;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("one of: ask, reject")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<EditPolicy, E> {
                match v {
                    "ask" => Ok(EditPolicy::Ask),
                    "reject" => Ok(EditPolicy::Reject),
                    other => Err(E::unknown_variant(other, &["ask", "reject"])),
                }
            }
        }
        deserializer.deserialize_str(V)
    }
}
#[derive(Debug, Clone)]
pub struct EditPathContext {
    pub real_cwd: std::path::PathBuf,
    pub display_cwd: Option<std::path::PathBuf>,
}
#[allow(clippy::large_enum_variant)]
pub enum PermissionCommand {
    Request {
        access: AccessKind,
        tool_call_update: acp::ToolCallUpdate,
        edit_path_context: Option<EditPathContext>,
        respond_to: oneshot::Sender<Decision>,
        context: PermissionRequestContext,
    },
    /// Atomically select the canonical permission mode.
    SetMode(diagnostics::enums::PermissionMode),
    /// Install or replace the permission classifier used in auto mode.
    SetClassifier(Option<std::sync::Arc<dyn super::auto_mode::PermissionClassifier>>),
    /// Project AGENTS.md instructions for classifier context (None clears).
    SetProjectInstructions(Option<String>),
    /// Drop every child-local permission and classifier state when the live
    /// child session ends.
    ReleaseChild { session_id: String },
    /// Reset per-tool permission state back to defaults.
    ResetState,
    /// Stop accepting requests. The shared shutdown token cancels an active
    /// judgment/prompt immediately; requests already queued before this
    /// command are returned as cancelled rather than approved during teardown.
    /// The acknowledgement means the actor reached its drain boundary;
    /// dropping its event sender then lets the audit bridge reach EOF.
    Shutdown { respond_to: oneshot::Sender<()> },
}
impl From<&tools::types::ToolInput> for AccessKind {
    fn from(input: &tools::types::ToolInput) -> Self {
        use tools::types::ToolInput;
        match input {
            ToolInput::ReadFile(r) => AccessKind::Read(Some(r.path.clone())),
            ToolInput::ListDir(l) => AccessKind::Read(Some(l.target_directory.clone())),
            ToolInput::Grep(g) => AccessKind::Grep {
                path: g.path.clone(),
                glob: g.glob.clone(),
            },
            ToolInput::TodoWrite(_)
            | ToolInput::TaskOutput(_)
            | ToolInput::KillTask(_)
            | ToolInput::Skill(_)
            | ToolInput::ContextRecall(_) => AccessKind::Read(None),
            ToolInput::SearchReplace(search_replace) => {
                AccessKind::Edit(search_replace.file_path.to_string())
            }
            ToolInput::HashlineEdit(he) => AccessKind::Edit(he.file_path.to_string()),
            ToolInput::Write(w) => AccessKind::Edit(w.file_path.clone()),
            ToolInput::Bash(bash) => AccessKind::Bash(bash.command.to_string()),
            ToolInput::Monitor(m) => AccessKind::Bash(m.command.clone()),
            ToolInput::MCPTool(mcp) => AccessKind::MCPTool {
                name: mcp.tool_name.to_string(),
                input: mcp.tool_input.clone(),
            },
            ToolInput::UseTool(u) => AccessKind::MCPTool {
                name: u.tool_name.clone(),
                input: u.tool_input.clone(),
            },
            ToolInput::WebFetch(wf) => AccessKind::WebFetch(wf.url.clone()),
            ToolInput::Dynamic(_) => AccessKind::Read(None),
            #[allow(unreachable_patterns)]
            _ => AccessKind::Read(None),
        }
    }
}
/// Permission policy configuration (duplicated from util/config.rs for Phase 1 move independence; identical).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PermissionConfig {
    pub rules: Vec<PermissionRule>,
    /// What to do when no rule or pre-decision resolves a tool call.
    #[serde(default)]
    pub prompt_policy: PromptPolicy,
}
impl PermissionConfig {
    pub fn new(rules: Vec<PermissionRule>) -> Self {
        Self {
            rules,
            prompt_policy: PromptPolicy::Ask,
        }
    }
}
/// What to do when the permission manager would normally prompt the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptPolicy {
    /// Prompt the user for approval (default).
    #[default]
    Ask,
    /// Deny without prompting (`permissions.defaultMode: "dontAsk"`).
    Deny,
    /// Use the auto-mode classifier (`permissions.defaultMode: "auto"`).
    /// Seeded into the permission manager's auto flag at session start.
    Auto,
}
/// A single permission rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub action: RuleAction,
    #[serde(default)]
    pub tool: ToolFilter,
    pub pattern: Option<String>,
    #[serde(default)]
    pub pattern_mode: PatternMode,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PatternMode {
    #[default]
    Glob,
    Domain,
}
/// Action to take when rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Allow,
    #[default]
    Deny,
    Ask,
}
/// Tool filter for permission rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolFilter {
    #[default]
    Any,
    Bash,
    Edit,
    Read,
    Grep,
    Mcp,
    WebFetch,
}
/// Where a requirement or permission was loaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequirementSource {
    Unknown,
    /// User-writable `~/.grow/requirements.toml` — untrusted for keeping a
    /// catch-all allow under the pin (a restricted user can edit it).
    Requirements {
        path: std::path::PathBuf,
    },
    /// Root-owned system-dir `requirements.toml`. Distinguished at load time
    /// (`RequirementsLayer::is_system`), never inferred from `path`.
    SystemRequirements {
        path: std::path::PathBuf,
    },
    /// Defaults tier; never an admin source.
    ManagedConfig {
        path: std::path::PathBuf,
    },
    Config {
        path: std::path::PathBuf,
    },
}
impl std::fmt::Display for RequirementSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => f.write_str("<unknown>"),
            Self::Requirements { path } => write!(f, "{} (requirements)", path.display()),
            Self::SystemRequirements { path } => {
                write!(f, "{} (system requirements)", path.display())
            }
            Self::ManagedConfig { path } => {
                write!(f, "{} (managed config)", path.display())
            }
            Self::Config { path } => write!(f, "{} (config)", path.display()),
        }
    }
}
/// A value paired with its source (duplicated).
#[derive(Debug, Clone)]
pub struct Sourced<T> {
    pub value: T,
    pub source: RequirementSource,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permission_event_subagent_fields_default_to_none() {
        let json = r#"{
            "tool_id": "tc1",
            "tool_name": "bash",
            "access_kind": "bash",
            "auto_approved": false,
            "user_prompted": true,
            "decision": "allow",
            "timestamp": "2026-03-24T00:00:00Z"
        }"#;
        let event: PermissionEvent = serde_json::from_str(json).unwrap();
        assert!(event.subagent_session_id.is_none());
        assert!(event.subagent_type.is_none());
        assert!(event.subagent_description.is_none());
        assert!(event.permission_mode.is_none());
        assert!(event.requested_permission_mode.is_none());
        assert!(event.capability_target.is_none());
        assert!(event.capability_purpose.is_none());
        assert!(event.decision_reason.is_none());
        assert!(event.classifier_source.is_none());
        assert!(event.classifier_latency_ms.is_none());
        assert!(event.auto_denials_consecutive.is_none());
        assert!(event.auto_denials_total.is_none());
        assert!(event.wait_ms.is_none());
        assert!(event.queue_depth.is_none());
    }
    #[test]
    fn permission_event_with_subagent_attribution() {
        let event = PermissionEvent {
            audit_sequence: 0,
            tool_id: "tc1".into(),
            tool_name: "bash".into(),
            access_kind: "bash".into(),
            access_detail: None,
            auto_approved: false,
            user_prompted: true,
            decision: "allow".into(),
            prompt_outcome: None,
            reject_reason: None,
            timestamp: Utc::now(),
            subagent_session_id: Some("child-1".into()),
            subagent_type: Some("explore".into()),
            subagent_description: Some("Find endpoints".into()),
            permission_mode: Some("ask".into()),
            requested_permission_mode: Some("follow".into()),
            capability_target: None,
            capability_purpose: None,
            decision_reason: Some("needs_user".into()),
            classifier_source: Some("llm".into()),
            classifier_verdict: Some("allow".into()),
            classifier_reason: Some("required for the task".into()),
            classifier_latency_ms: Some(42),
            auto_denials_consecutive: Some(2),
            auto_denials_total: Some(5),
            wait_ms: Some(1234),
            queue_depth: Some(3),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["subagent_session_id"], "child-1");
        assert_eq!(json["subagent_type"], "explore");
        assert_eq!(json["subagent_description"], "Find endpoints");
        assert_eq!(json["permission_mode"], "ask");
        assert_eq!(json["requested_permission_mode"], "follow");
        assert_eq!(json["decision_reason"], "needs_user");
        assert_eq!(json["classifier_source"], "llm");
        assert_eq!(json["classifier_verdict"], "allow");
        assert_eq!(json["classifier_reason"], "required for the task");
        assert_eq!(json["classifier_latency_ms"], 42);
        assert_eq!(json["auto_denials_consecutive"], 2);
        assert_eq!(json["auto_denials_total"], 5);
        assert_eq!(json["wait_ms"], 1234);
        assert_eq!(json["queue_depth"], 3);
    }
    #[test]
    fn permission_event_skips_none_optional_fields() {
        let event = PermissionEvent {
            audit_sequence: 0,
            tool_id: "tc1".into(),
            tool_name: "bash".into(),
            access_kind: "bash".into(),
            access_detail: None,
            auto_approved: true,
            user_prompted: false,
            decision: "allow".into(),
            prompt_outcome: None,
            reject_reason: None,
            timestamp: Utc::now(),
            subagent_session_id: None,
            subagent_type: None,
            subagent_description: None,
            permission_mode: None,
            requested_permission_mode: None,
            capability_target: None,
            capability_purpose: None,
            decision_reason: None,
            classifier_source: None,
            classifier_verdict: None,
            classifier_reason: None,
            classifier_latency_ms: None,
            auto_denials_consecutive: None,
            auto_denials_total: None,
            wait_ms: None,
            queue_depth: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("subagent_session_id"));
        assert!(!json.contains("subagent_type"));
        assert!(!json.contains("permission_mode"));
        assert!(!json.contains("decision_reason"));
        assert!(!json.contains("classifier_source"));
        assert!(!json.contains("classifier_latency_ms"));
        assert!(!json.contains("auto_denials_consecutive"));
        assert!(!json.contains("auto_denials_total"));
        assert!(!json.contains("wait_ms"));
        assert!(!json.contains("queue_depth"));
    }
    #[test]
    fn hashline_edit_maps_to_edit_access() {
        use tools::implementations::grow_build_hashline::edit::types::HashlineEditInput;
        use tools::types::ToolInput;
        let input = ToolInput::HashlineEdit(HashlineEditInput {
            file_path: "src/main.rs".into(),
            edits: vec![],
        });
        let access = AccessKind::from(&input);
        assert!(
            matches!(access, AccessKind::Edit(ref p) if p == "src/main.rs"),
            "HashlineEdit should produce AccessKind::Edit with the file path, got {access:?}"
        );
    }
    #[test]
    fn bash_maps_to_bash_access() {
        use tools::implementations::grow_build::bash::BashToolInput;
        use tools::types::ToolInput;
        let input = ToolInput::Bash(BashToolInput {
            command: "cargo test".into(),
            timeout: None,
            description: "run tests".into(),
            is_background: false,
        });
        let access = AccessKind::from(&input);
        assert!(
            matches!(access, AccessKind::Bash(ref cmd) if cmd == "cargo test"),
            "Bash should produce AccessKind::Bash with the command, got {access:?}"
        );
    }
    #[test]
    fn use_tool_maps_to_mcp_tool_access() {
        use tools::implementations::use_tool::UseToolInput;
        use tools::types::ToolInput;
        let input = ToolInput::UseTool(UseToolInput {
            tool_name: "linear__save_issue".into(),
            tool_input: serde_json::json!({ "title" : "test" }),
        });
        let access = AccessKind::from(&input);
        assert!(
            matches!(
                access,
                AccessKind::MCPTool { ref name, ref input }
                    if name == "linear__save_issue" && input["title"] == "test"
            ),
            "UseTool should produce AccessKind::MCPTool carrying the inner tool name and args, got {access:?}"
        );
    }
    #[test]
    fn monitor_maps_to_bash_access() {
        use tools::implementations::grow_build::monitor::types::MonitorInput;
        use tools::types::ToolInput;
        let input = ToolInput::Monitor(MonitorInput {
            command: "tail -f /var/log/syslog".into(),
            description: "watch syslog".into(),
            timeout_ms: None,
            persistent: false,
        });
        let access = AccessKind::from(&input);
        assert!(
            matches!(access, AccessKind::Bash(ref cmd) if cmd == "tail -f /var/log/syslog"),
            "Monitor runs shell and must map to AccessKind::Bash (not Read), got {access:?}"
        );
    }
    #[test]
    fn search_replace_maps_to_edit_access() {
        use tools::implementations::grow_build::search_replace::SearchReplaceInput;
        use tools::types::ToolInput;
        let input = ToolInput::SearchReplace(SearchReplaceInput {
            file_path: "lib.rs".into(),
            old_string: "old".into(),
            new_string: "new".into(),
            replace_all: false,
        });
        let access = AccessKind::from(&input);
        assert!(
            matches!(access, AccessKind::Edit(ref p) if p == "lib.rs"),
            "SearchReplace should produce AccessKind::Edit, got {access:?}"
        );
    }
    #[test]
    fn web_fetch_maps_to_web_fetch_access() {
        use tools::implementations::grow_build::web_fetch::WebFetchInput;
        use tools::types::ToolInput;
        let input = ToolInput::WebFetch(WebFetchInput {
            url: "https://custom.example.com/api".into(),
        });
        let access = AccessKind::from(&input);
        assert!(
            matches!(access, AccessKind::WebFetch(ref u) if u == "https://custom.example.com/api"),
            "WebFetch should produce AccessKind::WebFetch with the URL, got {access:?}"
        );
    }
    #[test]
    fn write_tool_maps_to_edit_access() {
        use tools::implementations::grow_build::write::WriteInput;
        use tools::types::ToolInput;
        let input = ToolInput::Write(WriteInput {
            file_path: "/tmp/secret.txt".into(),
            content: "overwritten".into(),
        });
        let access = AccessKind::from(&input);
        assert!(
            matches!(access, AccessKind::Edit(ref p) if p == "/tmp/secret.txt"),
            "Write should produce AccessKind::Edit with the file path, got {access:?}"
        );
    }
    #[test]
    fn client_type_requires_canonical_wire_name() {
        assert_eq!(
            serde_json::from_value::<ClientType>("generic".into()).unwrap(),
            ClientType::Generic,
        );
        for obsolete in [
            "grow-shell",
            "grow_shell",
            "grow_tui",
            "grow_web",
            "grow_pager",
        ] {
            assert!(serde_json::from_value::<ClientType>(obsolete.into()).is_err());
        }
    }
}

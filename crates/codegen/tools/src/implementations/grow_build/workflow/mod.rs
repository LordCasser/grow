use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};

use super::task::types::SubagentDepthCounter;

pub use crate::slash_commands::WORKFLOW_TOOL_NAME;

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct WorkflowDefinitionId(pub String);

impl WorkflowDefinitionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl std::fmt::Display for WorkflowDefinitionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowScope {
    Session,
    Project,
    User,
    Builtin,
}

impl WorkflowScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Project => "project",
            Self::User => "user",
            Self::Builtin => "builtin",
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunControl {
    Pause,
    Resume,
    Stop,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowDraftSource {
    Inline {
        #[schemars(
            description = "Inline Rhai source used to create the session draft. Author it per the Workflow behavior's Rhai authoring reference: first statement `let meta = #{ name, description, when_to_use?, phases? };`, then orchestrate with phase()/agent()/parallel()/complete()/pause(); validate representative args before publishing."
        )]
        script: String,
    },
    File {
        #[schemars(description = "Trusted Rhai file used to create the session draft.")]
        path: String,
    },
    Definition {
        #[schemars(description = "Existing saved Definition to derive into a session draft.")]
        definition_id: WorkflowDefinitionId,
    },
}

/// Public Workflow API. Definitions, drafts, and runs are intentionally
/// separate actions so a run can never be confused with an editable source.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum WorkflowToolInput {
    Search {
        #[schemars(
            description = "Task description matched against workflow name, description, and when_to_use metadata."
        )]
        query: String,
        #[serde(default)]
        #[schemars(range(min = 1, max = 50))]
        limit: Option<usize>,
    },
    Inspect {
        definition_id: WorkflowDefinitionId,
        #[serde(default)]
        include_source: bool,
    },
    Draft {
        #[serde(default)]
        #[schemars(
            description = "Optional expected name. Required only when the source cannot provide metadata itself."
        )]
        name: Option<String>,
        source: WorkflowDraftSource,
    },
    Validate {
        definition_id: WorkflowDefinitionId,
        #[serde(default)]
        args: Option<serde_json::Value>,
        #[serde(default)]
        #[schemars(range(min = 1, max = 1024))]
        agent_budget: Option<u64>,
    },
    Run {
        definition_id: WorkflowDefinitionId,
        #[serde(default)]
        args: Option<serde_json::Value>,
        #[serde(default)]
        #[schemars(range(min = 1, max = 16))]
        max_concurrency: Option<u16>,
        #[serde(default)]
        #[schemars(range(min = 1, max = 1024))]
        agent_budget: Option<u64>,
    },
    Publish {
        #[schemars(description = "A session draft Definition id.")]
        definition_id: WorkflowDefinitionId,
        #[schemars(
            description = "Required destination. Only project and user are publishable scopes."
        )]
        scope: WorkflowScope,
    },
    Discard {
        #[schemars(description = "A session draft Definition id.")]
        definition_id: WorkflowDefinitionId,
    },
    ControlRun {
        #[schemars(description = "Session-unique run handle or internal run id.")]
        run_id: String,
        operation: WorkflowRunControl,
        #[serde(default)]
        #[schemars(
            description = "Required only when resuming a budget-limited run with a higher cap."
        )]
        agent_budget: Option<u64>,
    },
}

impl WorkflowToolInput {
    pub const MAX_AGENT_BUDGET: u64 = 1_024;
    pub const DEFAULT_MAX_CONCURRENCY: u16 = 3;
    pub const MAX_CONCURRENCY: u16 = 16;

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Search { query, limit } => {
                if query.trim().is_empty() {
                    return Err("`query` must not be blank".into());
                }
                if limit.is_some_and(|value| !(1..=50).contains(&value)) {
                    return Err("`limit` must be from 1 through 50".into());
                }
            }
            Self::Inspect { definition_id, .. }
            | Self::Validate { definition_id, .. }
            | Self::Run { definition_id, .. }
            | Self::Publish { definition_id, .. }
            | Self::Discard { definition_id } => validate_definition_id(definition_id)?,
            Self::Draft { source, .. } => match source {
                WorkflowDraftSource::Inline { script } if script.trim().is_empty() => {
                    return Err("draft inline `script` must not be blank".into());
                }
                WorkflowDraftSource::File { path } if path.trim().is_empty() => {
                    return Err("draft file `path` must not be blank".into());
                }
                WorkflowDraftSource::Definition { definition_id } => {
                    validate_definition_id(definition_id)?;
                }
                WorkflowDraftSource::Inline { .. } | WorkflowDraftSource::File { .. } => {}
            },
            Self::ControlRun {
                run_id,
                agent_budget,
                ..
            } => {
                if run_id.trim().is_empty() {
                    return Err("`run_id` must not be blank".into());
                }
                validate_budget(*agent_budget)?;
            }
        }
        match self {
            Self::Validate { agent_budget, .. } | Self::Run { agent_budget, .. } => {
                validate_budget(*agent_budget)?
            }
            _ => {}
        }
        if let Self::Run {
            max_concurrency: Some(value),
            ..
        } = self
            && !(1..=Self::MAX_CONCURRENCY).contains(value)
        {
            return Err(format!(
                "`max_concurrency` must be from 1 through {}",
                Self::MAX_CONCURRENCY
            ));
        }
        if let Self::Publish { scope, .. } = self
            && !matches!(scope, WorkflowScope::Project | WorkflowScope::User)
        {
            return Err("`scope` must be `project` or `user` when publishing".into());
        }
        if let Self::Discard { definition_id } = self
            && !definition_id.0.starts_with("session:")
        {
            return Err("only a session draft Definition can be discarded".into());
        }
        Ok(())
    }

    pub fn action_label(&self) -> &'static str {
        match self {
            Self::Search { .. } => "search",
            Self::Inspect { .. } => "inspect",
            Self::Draft { .. } => "draft",
            Self::Validate { .. } => "validate",
            Self::Run { .. } => "run",
            Self::Publish { .. } => "publish",
            Self::Discard { .. } => "discard",
            Self::ControlRun { .. } => "control run",
        }
    }
}

fn validate_definition_id(id: &WorkflowDefinitionId) -> Result<(), String> {
    let Some((scope, local_id)) = id.0.split_once(':') else {
        return Err("`definition_id` must be a stable scoped id such as `project:review`".into());
    };
    if !matches!(scope, "session" | "project" | "user" | "builtin")
        || local_id.is_empty()
        || local_id.contains(':')
    {
        return Err(
            "`definition_id` must use session, project, user, or builtin scope and a non-empty local id"
                .into(),
        );
    }
    Ok(())
}

fn validate_budget(value: Option<u64>) -> Result<(), String> {
    if value.is_some_and(|budget| budget == 0 || budget > WorkflowToolInput::MAX_AGENT_BUDGET) {
        Err(format!(
            "`agent_budget` must be from 1 through {}",
            WorkflowToolInput::MAX_AGENT_BUDGET
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug)]
pub struct WorkflowRequest {
    pub input: WorkflowToolInput,
    /// Behavior captured when the owning foreground turn was admitted. This is
    /// stamped by the trusted session resource and is never model input.
    pub admitted_behavior: tool_types::BehaviorId,
}

#[derive(Debug)]
pub enum WorkflowAck {
    Completed(WorkflowToolOutput),
    Rejected { code: &'static str, detail: String },
}

pub type WorkflowEnvelope = (WorkflowRequest, tokio::sync::oneshot::Sender<WorkflowAck>);

pub struct WorkflowHandle {
    pub sender: tokio::sync::mpsc::UnboundedSender<WorkflowEnvelope>,
    pub admitted_behavior: std::sync::Arc<parking_lot::Mutex<tool_types::BehaviorId>>,
}

impl std::fmt::Debug for WorkflowHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkflowHandle").finish()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct WorkflowDefinitionSummary {
    pub definition_id: WorkflowDefinitionId,
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    pub scope: WorkflowScope,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_definition_id: Option<WorkflowDefinitionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    pub content_hash: String,
    pub focused: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct WorkflowDiagnostic {
    pub scope: WorkflowScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum WorkflowToolOutput {
    Search {
        matches: Vec<WorkflowDefinitionSummary>,
        diagnostics: Vec<WorkflowDiagnostic>,
        message: String,
    },
    Inspect {
        definition: WorkflowDefinitionSummary,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
        message: String,
    },
    Draft {
        definition: WorkflowDefinitionSummary,
        message: String,
    },
    Validated {
        definition: WorkflowDefinitionSummary,
        phases: usize,
        summary: String,
        message: String,
    },
    RunStarted {
        definition: WorkflowDefinitionSummary,
        run_id: String,
        run_handle: String,
        content_hash: String,
        message: String,
    },
    Published {
        definition: WorkflowDefinitionSummary,
        message: String,
    },
    Discarded {
        definition_id: WorkflowDefinitionId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        focused_definition: Option<WorkflowDefinitionSummary>,
        message: String,
    },
    RunControlled {
        run_id: String,
        run_handle: String,
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition_id: Option<WorkflowDefinitionId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition_scope: Option<WorkflowScope>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_hash: Option<String>,
        message: String,
    },
}

impl WorkflowToolOutput {
    pub fn message(&self) -> &str {
        match self {
            Self::Search { message, .. }
            | Self::Inspect { message, .. }
            | Self::Draft { message, .. }
            | Self::Validated { message, .. }
            | Self::RunStarted { message, .. }
            | Self::Published { message, .. }
            | Self::Discarded { message, .. }
            | Self::RunControlled { message, .. } => message,
        }
    }
}

impl tool_runtime::ToolOutput for WorkflowToolOutput {}

#[derive(Debug, Default)]
pub struct WorkflowTool;

impl crate::types::tool_metadata::ToolMetadata for WorkflowTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Workflow
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        r##"Manage public Workflow Definitions and immutable Runs inside Workflow Behavior. Use `search` before assuming the focused Definition is relevant; `inspect` loads candidate details; `draft` creates or derives one session draft and focuses it; `validate` smoke-checks representative args; `run` starts a Definition snapshot and automatically preflights an unvalidated hash; `publish` atomically saves a draft to the required project or user scope; `discard` removes only a session draft; `control_run` pauses, resumes, or stops a specific Run. Definitions use stable scoped ids such as `session:<id>`, `project:<name>`, `user:<name>`, or `builtin:<name>`. A Run keeps its Definition id, scope, source hash, script, and args immutable. Changes affect only later Runs."##
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl tool_runtime::Tool for WorkflowTool {
    type Args = WorkflowToolInput;
    type Output = WorkflowToolOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(WORKFLOW_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &::tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            WORKFLOW_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            tool_scope: tool_protocol::ToolScope::Write,
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "new_tool.workflow", skip_all)]
    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: WorkflowToolInput,
    ) -> Result<WorkflowToolOutput, tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;
        input
            .validate()
            .map_err(|detail| tool_runtime::ToolError::custom("workflow_invalid_input", detail))?;

        let (depth, sender) = {
            let resources = resources.lock().await;
            (
                resources
                    .get::<SubagentDepthCounter>()
                    .map(|depth| depth.0)
                    .unwrap_or(0),
                resources
                    .get::<WorkflowHandle>()
                    .map(|handle| (handle.sender.clone(), *handle.admitted_behavior.lock())),
            )
        };
        if depth > 0 {
            return Err(tool_runtime::ToolError::custom(
                "workflow_depth_exceeded",
                "Workflow actions are available only from a top-level session",
            ));
        }
        let (sender, admitted_behavior) = sender.ok_or_else(|| {
            tool_runtime::ToolError::custom(
                "workflow_not_available",
                "Workflow workspace is not available in this session",
            )
        })?;
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        sender
            .send((
                WorkflowRequest {
                    input,
                    admitted_behavior,
                },
                ack_tx,
            ))
            .map_err(|_| {
                tool_runtime::ToolError::custom(
                    "workflow_channel_closed",
                    "Workflow workspace channel closed while the session was shutting down",
                )
            })?;
        match ack_rx.await {
            Ok(WorkflowAck::Completed(output)) => Ok(output),
            Ok(WorkflowAck::Rejected { code, detail }) => {
                Err(tool_runtime::ToolError::custom(code, detail))
            }
            Err(_) => Err(tool_runtime::ToolError::custom(
                "workflow_no_ack",
                "The session dropped the Workflow request before answering",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_actions_validate_their_own_boundaries() {
        assert!(
            WorkflowToolInput::Search {
                query: "review changes".into(),
                limit: Some(5),
            }
            .validate()
            .is_ok()
        );
        assert!(
            WorkflowToolInput::Draft {
                name: None,
                source: WorkflowDraftSource::File { path: " ".into() },
            }
            .validate()
            .is_err()
        );
        assert!(
            WorkflowToolInput::Publish {
                definition_id: WorkflowDefinitionId::new("session:abc"),
                scope: WorkflowScope::Session,
            }
            .validate()
            .is_err()
        );
        assert!(
            WorkflowToolInput::Run {
                definition_id: WorkflowDefinitionId::new("project:review"),
                args: None,
                max_concurrency: Some(WorkflowToolInput::MAX_CONCURRENCY + 1),
                agent_budget: None,
            }
            .validate()
            .is_err()
        );
        assert!(
            WorkflowToolInput::Discard {
                definition_id: WorkflowDefinitionId::new("session:draft"),
            }
            .validate()
            .is_ok()
        );
        assert!(
            WorkflowToolInput::Discard {
                definition_id: WorkflowDefinitionId::new("project:saved"),
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn serde_uses_action_tag() {
        let value = serde_json::to_value(WorkflowToolInput::Inspect {
            definition_id: WorkflowDefinitionId::new("user:review"),
            include_source: false,
        })
        .unwrap();
        assert_eq!(value["action"], "inspect");
        assert!(value.get("name").is_none());

        let draft = serde_json::to_value(WorkflowToolInput::Draft {
            name: None,
            source: WorkflowDraftSource::Definition {
                definition_id: WorkflowDefinitionId::new("project:review"),
            },
        })
        .unwrap();
        assert_eq!(draft["action"], "draft");
        assert_eq!(draft["source"]["kind"], "definition");
        assert_eq!(draft["source"]["definition_id"], "project:review");
    }
}

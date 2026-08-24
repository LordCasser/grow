//! Tool types and post-execution reminders.
//!
//! The tool runtime contract (`Tool` trait) lives in `tool_runtime`.
//! Tool metadata (kind, namespace, fingerprinting, etc.) lives in
//! `crate::types::tool_metadata::ToolMetadata`.
//!
//! This module provides:
//! - `ToolNamespace`, `ToolKind` — classification enums
//! - `Reminder` — post-execution system reminders (per-tool + cross-cutting)
use crate::types::output::ToolOutput;
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::SharedResources;
/// The toolset a tool belongs to.
///
/// Serializes to one canonical snake_case vocabulary (`grow`, `mcp`, …) for
/// the tool `_meta` wire contract. The `Display` impl remains PascalCase for
/// qualified runtime ids (for example `"Grow:read_file"`); only the serde form
/// goes on the wire.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    derive_more::Display,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    strum::EnumIter,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolNamespace {
    Grow,
    GrowConcise,
    GrowHashline,
    #[serde(rename = "mcp")]
    MCP,
}
/// Categorizes what a tool does at a high level.
///
/// Serializes as snake_case strings (e.g. `"read"`, `"list_dir"`, `"web_fetch"`).
/// `Other` is an explicit category for tools that do not fit elsewhere. The
/// wire vocabulary is closed so templates, UI grouping, and discovery cannot
/// silently drift. Runtime authorization is descriptor-owned and does not
/// derive from this taxonomy.
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
    strum::EnumIter,
    strum::IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ToolKind {
    Read,
    Edit,
    Delete,
    ListDir,
    Write,
    Move,
    Search,
    Lsp,
    Execute,
    Plan,
    WebFetch,
    BackgroundTaskAction,
    KillTaskAction,
    List,
    Skill,
    MemorySearch,
    MemoryGet,
    ContextRecall,
    Task,
    PlanControl,
    AskUser,
    DeployApp,
    SearchTool,
    UseTool,
    Monitor,
    GoalRead,
    GoalLifecycleUpdate,
    Workflow,
    Other,
}
impl ToolKind {
    /// Stable snake_case key for this kind (the `tools.by_kind.<key>` template key).
    pub fn as_key(self) -> &'static str {
        self.into()
    }
}
/// System reminders that fire after a tool call completes.
///
/// Implemented by:
/// - **Per-tool reminders** on tool structs (e.g., `ReadFileTool`: empty
///   file, offset past end).
/// - **Cross-cutting reminders** on standalone structs (e.g.,
///   `SkillDiscoveryReminder`) that react to any tool call.
#[async_trait::async_trait]
pub trait Reminder {
    /// Requirements for this reminder to be active.
    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
    /// Collect reminders after a tool execution completes.
    async fn collect_reminders(
        &self,
        _resources: SharedResources,
        _tool_output: &ToolOutput,
    ) -> Vec<String> {
        vec![]
    }
}

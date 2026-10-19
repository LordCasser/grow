//! TodoWrite — new-architecture implementation.
//!
//! Writes are commands to the owning SessionActor. `TodoState` remains only
//! as a compatibility projection for existing tool output consumers.

use std::fmt::Write;

use crate::types::output::{TodoWriteOutput, TodoWriteSuccess};
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};
use tool_types::{DependencyKind, LocalPlan, PlanPriority, PlanStepStatus};
#[cfg(test)]
use tool_types::{WaitCondition, WaitDependency};

pub mod runtime;
pub use runtime::{LocalPlanProjection, PlanningAction, PlanningCommand, PlanningRuntimeHandle};

#[derive(thiserror::Error, Debug)]
pub enum TodoError {
    #[error("Missing Todo content in mode: {0}")]
    MissingTodoContent(String),

    #[error("Missing Todo ID in mode: {0}")]
    MissingTodoID(String),

    #[error("Duplicate Todo ID in response: {0}")]
    DuplicateTodoID(String),
}

pub(crate) fn validate_no_duplicate_ids(updates: &[TodoUpdate]) -> Result<(), TodoError> {
    use std::collections::HashSet;
    let mut seen = HashSet::with_capacity(updates.len());
    if let Some(dup) = updates.iter().map(|u| &u.id).find(|id| !seen.insert(*id)) {
        return Err(TodoError::DuplicateTodoID(dup.to_owned()));
    }
    Ok(())
}

/// `merge=false`: the incoming list fully replaces the existing todo state.
/// If `content` is omitted for an item, the `id` is used as a fallback.
/// If `status` is omitted, it defaults to `Pending`.
#[cfg(test)]
pub(crate) fn apply_replace(
    state: &mut TodoState,
    updates: &[TodoUpdate],
) -> Result<(), TodoError> {
    state.clear();
    for u in updates {
        let content = if u.has_no_content() {
            u.id.clone()
        } else {
            u.content.clone().unwrap()
        };
        let status = u.status.unwrap_or(TodoStatus::Pending);
        state.push(
            u.id.clone(),
            TodoItem {
                content,
                priority: TodoPriority::default(),
                status,
                meta: None,
            },
        );
    }
    Ok(())
}

/// `merge=true`: updates are merged into the existing state.
/// - **Existing items**: `content` is optional — if omitted the previous
///   value is kept. This lets the model mark an item from `in_progress` →
///   `completed` without echoing the content back.
/// - **New items** (id not yet in state): if `content` is omitted the `id`
///   is used as a fallback so the tool never errors on a merge call. This
///   makes the tool resilient to state being lost between calls.
#[cfg(test)]
pub(crate) fn apply_merge(state: &mut TodoState, updates: &[TodoUpdate]) -> Result<(), TodoError> {
    for u in updates {
        if state.update(&u.id, u.content.as_deref(), u.status) {
            // Existing item – partial update succeeded, content was optional.
            continue;
        }
        let content = if u.has_no_content() {
            u.id.clone()
        } else {
            u.content.clone().unwrap()
        };
        let status = u.status.unwrap_or(TodoStatus::Pending);
        state.push(
            u.id.clone(),
            TodoItem {
                content,
                priority: TodoPriority::default(),
                status,
                meta: None,
            },
        );
    }
    Ok(())
}

pub(crate) fn summarize_todo_state(state: &TodoState) -> String {
    if state.is_empty() {
        "No tasks currently tracked.".into()
    } else {
        let mut out = String::new();
        for (id, t) in state.todo_items_with_ids() {
            writeln!(&mut out, "- {} {id}: {}", t.status.tag(), t.content).ok();
        }
        out
    }
}

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type TodoId = String;

// diff from acp: default to medium
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoPriority {
    High,
    #[default]
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

impl TodoStatus {
    pub const fn tag(&self) -> &str {
        match self {
            Self::Pending => "[pending]",
            Self::InProgress => "[in_progress]",
            Self::Completed => "[completed]",
            Self::Cancelled => "[cancelled]",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub content: String,
    #[serde(default)]
    pub priority: TodoPriority,
    pub status: TodoStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TodoState {
    todos: IndexMap<TodoId, TodoItem>,
}

// Kept registered for existing read-only projections; TodoWrite no longer
// obtains or mutates this state.
crate::register_resource!("grow_build", "Todo", TodoState);

impl TodoState {
    pub fn push(&mut self, id: TodoId, todo: TodoItem) {
        self.todos.insert(id, todo);
    }

    pub fn clear(&mut self) {
        self.todos.clear();
    }

    pub fn update(
        &mut self,
        id: &TodoId,
        content: Option<&str>,
        status: Option<TodoStatus>,
    ) -> bool {
        let Some(todo) = self.todos.get_mut(id) else {
            return false;
        };
        if let Some(content) = content
            && !content.is_empty()
        {
            todo.content = content.into();
        }
        if let Some(status) = status {
            todo.status = status;
        }
        true
    }

    pub fn todo_items(&self) -> impl Iterator<Item = &TodoItem> + '_ {
        self.todos.values()
    }

    pub fn todo_items_with_ids(&self) -> impl Iterator<Item = (&TodoId, &TodoItem)> + '_ {
        self.todos.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.todos.is_empty()
    }

    pub fn has_id(&self, id: &str) -> bool {
        self.todos.contains_key(id)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TodoUpdate {
    #[schemars(description = "Unique identifier for the todo item")]
    pub id: String,

    #[schemars(description = "The description/content of the todo item")]
    pub content: Option<String>,

    #[schemars(
        description = "The status of the todo item: pending, in_progress, completed, or cancelled"
    )]
    pub status: Option<TodoStatus>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Optional priority: high, medium, or low")]
    pub priority: Option<TodoPriority>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<bool>,
}

impl TodoUpdate {
    /// True when the update carries no meaningful content (None or empty string).
    #[cfg(test)]
    fn has_no_content(&self) -> bool {
        self.content.as_deref().is_none_or(str::is_empty)
    }
}

const fn default_merge() -> bool {
    true
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TodoWriteInput {
    /// When true (the default), merge the provided todos into the existing
    /// list by id (partial updates are allowed — leave unchanged fields
    /// undefined). When explicitly set to false, the provided todos replace
    /// the existing list entirely.
    #[serde(
        default = "default_merge",
        deserialize_with = "crate::types::schema::deserialize_lenient_bool"
    )]
    #[schemars(
        description = "Optional. When true (default), merges the provided todos into the existing list by id — send only the items you are changing, and to flip status without changing content send just id + status. When false, the provided todos replace the existing list."
    )]
    pub merge: bool,

    #[serde(default)]
    #[schemars(description = "Array of todo items to write to the current local plan")]
    pub todos: Vec<TodoUpdate>,

    /// Structured waiting, replan confirmation, review, or scope action to
    /// commit with this plan update.
    #[serde(default)]
    pub actions: Vec<PlanningAction>,
}

/// New-architecture `TodoWrite` tool.
///
/// Params: `()` — no per-tool configuration. The SessionActor owns the plan.
#[derive(Debug, Default)]
pub struct TodoWriteTool;

impl crate::types::tool_metadata::ToolMetadata for TodoWriteTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Plan
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        r#"Create and manage a structured task list. The user sees this list live — it is your primary way to show progress.

Use for any task with 3+ steps. Skip for trivial single-step work."#
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl tool_runtime::Tool for TodoWriteTool {
    type Args = TodoWriteInput;
    type Output = TodoWriteOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("todo_write").expect("valid tool id")
    }

    fn description(&self, _ctx: &::tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            "todo_write",
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::None,
            ..Default::default()
        }
    }

    #[tracing::instrument(
        name = "new_tool.todo_write",
        skip_all,
        fields(merge = %input.merge, todo_count = input.todos.len())
    )]
    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: TodoWriteInput,
    ) -> Result<TodoWriteOutput, tool_runtime::ToolError> {
        // Validate IDs upfront — return an error-as-output variant so the
        // Python side can distinguish this from infra errors.
        if let Err(TodoError::DuplicateTodoID(id)) = validate_no_duplicate_ids(&input.todos) {
            return Ok(TodoWriteOutput::DuplicateId(format!(
                "Duplicate todo ID in request: \"{id}\". Each todo item must have a unique ID."
            )));
        }

        let (prompt_id, sender) = planning_runtime(&ctx).await?;
        let (respond_to, response) = tokio::sync::oneshot::channel();
        sender
            .send(PlanningCommand::Write {
                prompt_id,
                input,
                respond_to,
            })
            .map_err(|_| planning_channel_error())?;
        let plan = response
            .await
            .map_err(|_| planning_channel_error())?
            .map_err(|message| tool_runtime::ToolError::custom("planning_rejected", message))?;
        Ok(TodoWriteOutput::TodosUpdated(todo_write_success(&plan)))
    }
}

async fn planning_runtime(
    ctx: &tool_runtime::ToolCallContext,
) -> Result<(String, tokio::sync::mpsc::UnboundedSender<PlanningCommand>), tool_runtime::ToolError>
{
    use crate::implementations::grow_build::task::types::CurrentPromptIdResource;
    use crate::types::tool_metadata::shared_resources;

    let resources = shared_resources(ctx)?;
    let resources = resources.lock().await;
    let prompt_id = resources
        .get::<CurrentPromptIdResource>()
        .map(|resource| resource.0.clone())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            tool_runtime::ToolError::custom(
                "planning_unavailable",
                "Current prompt identity is unavailable.",
            )
        })?;
    let sender = resources
        .get::<PlanningRuntimeHandle>()
        .map(|handle| handle.0.clone())
        .ok_or_else(|| {
            tool_runtime::ToolError::custom(
                "planning_unavailable",
                "Planning runtime is unavailable.",
            )
        })?;
    Ok((prompt_id, sender))
}

fn planning_channel_error() -> tool_runtime::ToolError {
    tool_runtime::ToolError::custom(
        "planning_channel_closed",
        "Planning runtime stopped before completing the request.",
    )
}

/// Project the actor-owned plan into the legacy TodoState output shape.
pub fn project_todo_state(plan: &LocalPlan) -> TodoState {
    const MAX_META_LINKS: usize = 8;
    const MAX_META_WAITS: usize = 8;

    let mut state = TodoState::default();
    for step in plan.steps.iter().filter(|step| !step.removed) {
        let link_count = plan
            .links
            .iter()
            .filter(|link| link.parent_step_id == step.id)
            .count();
        let links: Vec<_> = plan
            .links
            .iter()
            .filter(|link| link.parent_step_id == step.id)
            .take(MAX_META_LINKS)
            .map(|link| {
                serde_json::json!({
                    "id": link.id,
                    "childSessionId": link.child_session_id,
                    "terminal": link.terminal,
                    "required": link.required,
                })
            })
            .collect();
        let wait_count = plan
            .waits
            .iter()
            .filter(|wait| {
                wait.dependencies.iter().any(|dependency| {
                    dependency.kind == DependencyKind::Task
                        && plan.links.iter().any(|link| {
                            link.parent_step_id == step.id && link.child_session_id == dependency.id
                        })
                })
            })
            .count();
        let wait_ids: Vec<_> = plan
            .waits
            .iter()
            .filter(|wait| {
                wait.dependencies.iter().any(|dependency| {
                    dependency.kind == DependencyKind::Task
                        && plan.links.iter().any(|link| {
                            link.parent_step_id == step.id && link.child_session_id == dependency.id
                        })
                })
            })
            .take(MAX_META_WAITS)
            .map(|wait| wait.id.as_str())
            .collect();
        let meta = serde_json::json!({
            "sessionId": plan.session_id,
            "scopeId": plan.scope_id,
            "stepId": step.id,
            "revision": plan.revision,
            "links": links,
            "linkCount": link_count,
            "linksTruncated": link_count > MAX_META_LINKS,
            "waitIds": wait_ids,
            "waitCount": wait_count,
            "waitIdsTruncated": wait_count > MAX_META_WAITS,
        });
        state.push(
            step.id.clone(),
            TodoItem {
                content: step.content.clone(),
                priority: match step.priority {
                    PlanPriority::High => TodoPriority::High,
                    PlanPriority::Normal => TodoPriority::Medium,
                    PlanPriority::Low => TodoPriority::Low,
                },
                status: match step.status {
                    PlanStepStatus::Pending => TodoStatus::Pending,
                    PlanStepStatus::InProgress => TodoStatus::InProgress,
                    PlanStepStatus::Completed => TodoStatus::Completed,
                    PlanStepStatus::Cancelled => TodoStatus::Cancelled,
                },
                meta: Some(meta),
            },
        );
    }
    state
}

pub fn todo_write_success(plan: &LocalPlan) -> TodoWriteSuccess {
    let state = project_todo_state(plan);
    let mut summary_for_prompt = summarize_todo_state(&state);
    append_plan_obligations_summary(&mut summary_for_prompt, plan);
    TodoWriteSuccess {
        summary_for_prompt,
        todos: state.todo_items().cloned().collect(),
        state,
    }
}

fn append_plan_obligations_summary(summary: &mut String, plan: &LocalPlan) {
    const LIMIT: usize = 8;
    let incoming: Vec<_> = plan
        .replans
        .iter()
        .filter(|change| {
            matches!(
                change.progress,
                tool_types::ReplanProgress::Received | tool_types::ReplanProgress::Consumed
            )
        })
        .collect();
    let waits = &plan.waits;
    let required = plan.unresolved_required_deliveries();

    if incoming.is_empty() && waits.is_empty() && required.is_empty() {
        return;
    }
    use std::fmt::Write as _;
    summary.push_str("\n\nOpen local-plan responsibilities:");
    if !incoming.is_empty() {
        write!(summary, "\n- Incoming replans: {}", incoming.len()).ok();
        for change in incoming.iter().take(LIMIT) {
            write!(
                summary,
                "\n  - {} (step {}, link {}, source revision {})",
                change.id, change.step_id, change.link_id, change.source_revision
            )
            .ok();
        }
        if incoming.len() > LIMIT {
            write!(
                summary,
                "\n  - {} additional replan references remain open; do not treat them as handled.",
                incoming.len() - LIMIT
            )
            .ok();
        }
    }
    if !waits.is_empty() {
        write!(summary, "\n- Declared waits: {}", waits.len()).ok();
        for wait in waits.iter().take(LIMIT) {
            write!(summary, "\n  - {} ({:?})", wait.id, wait.condition).ok();
        }
        if waits.len() > LIMIT {
            write!(
                summary,
                "\n  - {} additional wait references remain active.",
                waits.len() - LIMIT
            )
            .ok();
        }
    }
    if !required.is_empty() {
        write!(
            summary,
            "\n- Required delivery gates still open: {}",
            required.len()
        )
        .ok();
        for link in required.iter().take(LIMIT) {
            write!(
                summary,
                "\n  - {} (step {}, child {})",
                link.id, link.parent_step_id, link.child_session_id
            )
            .ok();
        }
        if required.len() > LIMIT {
            write!(
                summary,
                "\n  - {} additional required delivery references remain open.",
                required.len() - LIMIT
            )
            .ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::output::TodoWriteOutput;
    use crate::types::resources::Resources;
    use crate::types::tool_metadata::test_ctx;

    // -- Helpers --

    fn make_update(id: &str, content: Option<&str>, status: Option<TodoStatus>) -> TodoUpdate {
        TodoUpdate {
            id: id.to_owned(),
            content: content.map(str::to_owned),
            status,
            priority: None,
            requirements: None,
            removed: None,
        }
    }

    /// Unwrap a `TodoWriteOutput` expecting the `TodosUpdated` variant.
    fn expect_success(output: TodoWriteOutput) -> TodoWriteSuccess {
        match output {
            TodoWriteOutput::TodosUpdated(s) => s,
            other => panic!("expected TodosUpdated, got {other:?}"),
        }
    }

    // -- Tests --

    #[test]
    fn name_and_description() {
        use crate::types::tool_metadata::ToolMetadata;
        let tool = TodoWriteTool;
        assert_eq!(tool_runtime::Tool::id(&tool).as_str(), "todo_write");
        assert!(ToolMetadata::description_template(&tool).contains("task list"));
    }

    fn runtime_context(
        prompt_id: &str,
        sender: tokio::sync::mpsc::UnboundedSender<PlanningCommand>,
    ) -> tool_runtime::ToolCallContext {
        use crate::implementations::grow_build::task::types::CurrentPromptIdResource;

        let mut resources = Resources::new();
        resources.insert(CurrentPromptIdResource(prompt_id.to_owned()));
        resources.insert(PlanningRuntimeHandle(sender));
        test_ctx(resources.into_shared())
    }

    #[tokio::test]
    async fn write_is_sent_to_actor_and_projects_its_response() {
        use tool_types::PlanStep;

        let tool = TodoWriteTool;
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let ctx = runtime_context("prompt-17", sender);
        let input = TodoWriteInput {
            merge: false,
            todos: vec![TodoUpdate {
                id: "build".into(),
                content: Some("Build the feature".into()),
                status: Some(TodoStatus::InProgress),
                priority: Some(TodoPriority::High),
                requirements: Some(vec!["tests pass".into()]),
                removed: Some(false),
            }],
            actions: vec![PlanningAction::HandleReplans {
                ids: vec!["change-1".into()],
                no_change_reason: Some("existing plan already covers it".into()),
            }],
        };
        let actor = tokio::spawn(async move {
            match receiver.recv().await.expect("write command") {
                PlanningCommand::Write {
                    prompt_id,
                    input,
                    respond_to,
                } => {
                    assert_eq!(prompt_id, "prompt-17");
                    assert_eq!(input.todos[0].priority, Some(TodoPriority::High));
                    assert_eq!(
                        input.todos[0].requirements.as_ref().unwrap()[0],
                        "tests pass"
                    );
                    assert_eq!(input.actions.len(), 1);
                    let mut plan = LocalPlan::new("session-1", "scope-1");
                    plan.steps.push(PlanStep {
                        id: "build".into(),
                        content: "Build the feature".into(),
                        priority: PlanPriority::High,
                        status: PlanStepStatus::InProgress,
                        requirements: Some(vec!["tests pass".into()]),
                        removed: false,
                    });
                    respond_to.send(Ok(plan)).unwrap();
                }
                PlanningCommand::Link { .. } => panic!("unexpected link command"),
                PlanningCommand::Undispatched { .. } => {
                    panic!("unexpected undispatched command")
                }
            }
        });

        let output = expect_success(tool_runtime::Tool::run(&tool, ctx, input).await.unwrap());
        actor.await.unwrap();
        assert_eq!(output.todos.len(), 1);
        assert_eq!(output.todos[0].priority, TodoPriority::High);
        assert!(output.summary_for_prompt.contains("Build the feature"));
        assert_eq!(output.state.todo_items().count(), 1);
    }

    #[tokio::test]
    async fn missing_runtime_is_an_explicit_error() {
        use crate::implementations::grow_build::task::types::CurrentPromptIdResource;

        let tool = TodoWriteTool;
        let mut resources = Resources::new();
        resources.insert(CurrentPromptIdResource("prompt-1".into()));
        let error = tool_runtime::Tool::run(
            &tool,
            test_ctx(resources.into_shared()),
            TodoWriteInput {
                merge: true,
                todos: vec![],
                actions: vec![],
            },
        )
        .await
        .unwrap_err();
        assert!(format!("{error:?}").contains("Planning runtime is unavailable."));
    }

    #[tokio::test]
    async fn closed_runtime_channel_is_an_explicit_error() {
        let tool = TodoWriteTool;
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        drop(receiver);
        let error = tool_runtime::Tool::run(
            &tool,
            runtime_context("prompt-1", sender),
            TodoWriteInput {
                merge: true,
                todos: vec![],
                actions: vec![],
            },
        )
        .await
        .unwrap_err();
        assert!(format!("{error:?}").contains("Planning runtime stopped"));
    }

    #[test]
    fn projection_omits_tombstones() {
        let mut plan = LocalPlan::new("session-1", "scope-1");
        plan.steps.push(tool_types::PlanStep {
            id: "removed".into(),
            content: "removed item".into(),
            priority: PlanPriority::Normal,
            status: PlanStepStatus::Cancelled,
            requirements: None,
            removed: true,
        });
        assert!(project_todo_state(&plan).is_empty());
    }

    #[test]
    fn projection_meta_carries_bounded_step_identity_links_and_wait_ids() {
        use tool_types::{PlanLink, PlanStep, PlanWait};

        let mut plan = LocalPlan::new("session-1", "scope-2");
        plan.revision = 7;
        plan.steps.push(PlanStep {
            id: "step-3".into(),
            content: "Run delegated checks".into(),
            priority: PlanPriority::Normal,
            status: PlanStepStatus::InProgress,
            requirements: None,
            removed: false,
        });
        plan.links.push(PlanLink {
            id: "link-4".into(),
            parent_step_id: "step-3".into(),
            child_session_id: "child-5".into(),
            terminal: false,
            required: true,
            pending_source_revision: Some(7),
            review_required_revision: 0,
            result_id: None,
            dispatch_failure: None,
        });
        plan.waits.push(PlanWait {
            id: "wait-6".into(),
            step_ids: vec!["step-3".into()],
            dependencies: vec![WaitDependency {
                kind: DependencyKind::Task,
                id: "child-5".into(),
            }],
            condition: WaitCondition::All,
        });

        let state = project_todo_state(&plan);
        let meta = get_item(&state, "step-3").meta.as_ref().unwrap();
        assert_eq!(meta["sessionId"], "session-1");
        assert_eq!(meta["scopeId"], "scope-2");
        assert_eq!(meta["stepId"], "step-3");
        assert_eq!(meta["revision"], 7);
        assert_eq!(meta["links"][0]["id"], "link-4");
        assert_eq!(meta["links"][0]["childSessionId"], "child-5");
        assert_eq!(meta["waitIds"][0], "wait-6");
        assert!(meta.get("resultId").is_none());
    }

    #[test]
    fn successful_write_summary_exposes_open_replans_waits_and_required_gates_without_todos() {
        use tool_types::{PlanLink, PlanWait, ReplanChange, ReplanProgress};

        let mut plan = LocalPlan::new("session-1", "scope-1");
        let change = ReplanChange {
            id: "replan-1".into(),
            link_id: "link-1".into(),
            child_session_id: "child-1".into(),
            step_id: "step-1".into(),
            source_revision: 2,
            content: "updated request".into(),
            requirements: None,
            priority: PlanPriority::Normal,
            status: PlanStepStatus::InProgress,
            removed: false,
            progress: ReplanProgress::Received,
        };
        plan.receive_replan(change.clone()).unwrap();
        plan.advance_replan("replan-1", ReplanProgress::Consumed)
            .unwrap();
        plan.links.push(PlanLink {
            id: "link-required".into(),
            parent_step_id: "step-required".into(),
            child_session_id: "child-required".into(),
            terminal: false,
            required: true,
            pending_source_revision: None,
            review_required_revision: 0,
            result_id: None,
            dispatch_failure: None,
        });
        plan.waits.push(PlanWait {
            id: "wait-1".into(),
            step_ids: vec![],
            dependencies: vec![],
            condition: WaitCondition::Any,
        });
        let result = todo_write_success(&plan);
        assert!(result.todos.is_empty());
        assert!(result.summary_for_prompt.contains("Incoming replans: 1"));
        assert!(
            result
                .summary_for_prompt
                .contains("replan-1 (step step-1, link link-1, source revision 2)")
        );
        assert!(result.summary_for_prompt.contains("wait-1 (Any)"));
        assert!(
            result
                .summary_for_prompt
                .contains("link-required (step step-required, child child-required)")
        );
    }

    fn seed_state(items: &[(&str, &str, TodoStatus)]) -> TodoState {
        let mut state = TodoState::default();
        for (id, content, status) in items {
            state.push(
                id.to_string(),
                TodoItem {
                    content: content.to_string(),
                    priority: TodoPriority::default(),
                    status: *status,
                    meta: None,
                },
            );
        }
        state
    }

    fn get_item<'a>(state: &'a TodoState, id: &str) -> &'a TodoItem {
        state
            .todo_items_with_ids()
            .find(|(i, _)| *i == id)
            .map(|(_, item)| item)
            .unwrap_or_else(|| panic!("item {id} not found in state"))
    }

    // ── replace (merge=false) ────────────────────────────────────────

    #[test]
    fn replace_without_content_falls_back_to_id() {
        let mut state = TodoState::default();
        let updates = vec![make_update(
            "build_project",
            None,
            Some(TodoStatus::Pending),
        )];
        apply_replace(&mut state, &updates).unwrap();

        let item = get_item(&state, "build_project");
        assert_eq!(item.content, "build_project"); // id used as fallback
        assert_eq!(item.status, TodoStatus::Pending);
    }

    #[test]
    fn replace_without_content_or_status_defaults() {
        let mut state = TodoState::default();
        let updates = vec![make_update("task_1", None, None)];
        apply_replace(&mut state, &updates).unwrap();

        let item = get_item(&state, "task_1");
        assert_eq!(item.content, "task_1");
        assert_eq!(item.status, TodoStatus::Pending);
    }

    #[test]
    fn replace_with_content_succeeds() {
        let mut state = TodoState::default();
        let updates = vec![
            make_update("1", Some("Task A"), Some(TodoStatus::Pending)),
            make_update("2", Some("Task B"), Some(TodoStatus::InProgress)),
        ];
        apply_replace(&mut state, &updates).unwrap();

        assert_eq!(get_item(&state, "1").content, "Task A");
        assert_eq!(get_item(&state, "1").status, TodoStatus::Pending);
        assert_eq!(get_item(&state, "2").content, "Task B");
        assert_eq!(get_item(&state, "2").status, TodoStatus::InProgress);
    }

    #[test]
    fn replace_clears_previous_state_unit() {
        let mut state = seed_state(&[("old", "Old task", TodoStatus::Completed)]);
        let updates = vec![make_update(
            "new",
            Some("New task"),
            Some(TodoStatus::Pending),
        )];
        apply_replace(&mut state, &updates).unwrap();

        // Old item is gone.
        assert!(!state.todo_items_with_ids().any(|(id, _)| *id == "old"));
        assert_eq!(get_item(&state, "new").content, "New task");
    }

    // ── merge (merge=true) ───────────────────────────────────────────

    #[test]
    fn merge_existing_item_status_only() {
        // The core use-case: mark in_progress → completed without sending content.
        let mut state = seed_state(&[("1", "Build the project", TodoStatus::InProgress)]);
        let updates = vec![make_update("1", None, Some(TodoStatus::Completed))];
        apply_merge(&mut state, &updates).unwrap();

        let item = get_item(&state, "1");
        assert_eq!(item.status, TodoStatus::Completed);
        assert_eq!(item.content, "Build the project"); // unchanged
    }

    #[test]
    fn merge_existing_item_content_and_status() {
        let mut state = seed_state(&[("1", "Old text", TodoStatus::Pending)]);
        let updates = vec![make_update(
            "1",
            Some("New text"),
            Some(TodoStatus::InProgress),
        )];
        apply_merge(&mut state, &updates).unwrap();

        let item = get_item(&state, "1");
        assert_eq!(item.content, "New text");
        assert_eq!(item.status, TodoStatus::InProgress);
    }

    #[test]
    fn merge_existing_item_no_fields_is_noop() {
        let mut state = seed_state(&[("1", "Keep me", TodoStatus::Pending)]);
        let updates = vec![make_update("1", None, None)];
        apply_merge(&mut state, &updates).unwrap();

        let item = get_item(&state, "1");
        assert_eq!(item.content, "Keep me");
        assert_eq!(item.status, TodoStatus::Pending);
    }

    #[test]
    fn merge_new_item_without_content_uses_id_fallback() {
        // When state is empty (e.g. lost between calls) and content is None,
        // the id is used as fallback content instead of erroring.
        let mut state = TodoState::default();
        let updates = vec![make_update(
            "explore_codebase",
            None,
            Some(TodoStatus::Completed),
        )];
        apply_merge(&mut state, &updates).unwrap();

        let item = get_item(&state, "explore_codebase");
        assert_eq!(item.content, "explore_codebase"); // id used as fallback
        assert_eq!(item.status, TodoStatus::Completed);
    }

    #[test]
    fn merge_new_item_without_content_or_status_defaults_to_pending() {
        let mut state = TodoState::default();
        let updates = vec![make_update("task_1", None, None)];
        apply_merge(&mut state, &updates).unwrap();

        let item = get_item(&state, "task_1");
        assert_eq!(item.content, "task_1");
        assert_eq!(item.status, TodoStatus::Pending);
    }

    #[test]
    fn merge_new_item_with_content_succeeds() {
        let mut state = TodoState::default();
        let updates = vec![make_update(
            "1",
            Some("Fresh task"),
            Some(TodoStatus::Pending),
        )];
        apply_merge(&mut state, &updates).unwrap();

        assert_eq!(get_item(&state, "1").content, "Fresh task");
    }

    #[test]
    fn merge_mixed_existing_and_new() {
        let mut state = seed_state(&[("exist", "Existing task", TodoStatus::InProgress)]);
        let updates = vec![
            // Update existing — content omitted, just flip status.
            make_update("exist", None, Some(TodoStatus::Completed)),
            // Brand-new item — content required.
            make_update("fresh", Some("New task"), Some(TodoStatus::Pending)),
        ];
        apply_merge(&mut state, &updates).unwrap();

        let existing = get_item(&state, "exist");
        assert_eq!(existing.status, TodoStatus::Completed);
        assert_eq!(existing.content, "Existing task"); // preserved

        let fresh = get_item(&state, "fresh");
        assert_eq!(fresh.content, "New task");
        assert_eq!(fresh.status, TodoStatus::Pending);
    }

    // ── duplicate id validation ──────────────────────────────────────

    #[test]
    fn duplicate_ids_rejected_unit() {
        let updates = vec![
            make_update("dup", Some("A"), Some(TodoStatus::Pending)),
            make_update("dup", Some("B"), Some(TodoStatus::Pending)),
        ];
        let err = validate_no_duplicate_ids(&updates).unwrap_err();
        assert!(matches!(err, TodoError::DuplicateTodoID(ref id) if id == "dup"));
    }

    #[test]
    fn unique_ids_accepted() {
        let updates = vec![
            make_update("a", Some("A"), Some(TodoStatus::Pending)),
            make_update("b", Some("B"), Some(TodoStatus::Pending)),
        ];
        validate_no_duplicate_ids(&updates).unwrap();
    }

    // ── regression: merge with null content should never error ────────

    #[test]
    fn merge_after_replace_status_update_with_null_content() {
        // Reproduces the exact scenario from the bug report:
        // 1. Replace creates 3 items
        // 2. Merge updates 2 items with content=null, status changed
        let mut state = TodoState::default();

        // Step 1: replace (merge=false)
        let initial = vec![
            make_update(
                "explore_codebase",
                Some("Explore django/db/backends/sqlite3/"),
                Some(TodoStatus::InProgress),
            ),
            make_update(
                "analyze_and_propose",
                Some("Analyze current SQLite min version"),
                Some(TodoStatus::Pending),
            ),
            make_update(
                "implementation",
                Some("Update version checks"),
                Some(TodoStatus::Pending),
            ),
        ];
        apply_replace(&mut state, &initial).unwrap();

        // Step 2: merge (merge=true) — content=null, just status changes
        let updates = vec![
            make_update("explore_codebase", None, Some(TodoStatus::Completed)),
            make_update("analyze_and_propose", None, Some(TodoStatus::InProgress)),
        ];
        apply_merge(&mut state, &updates).unwrap();

        // Statuses flipped, content preserved from step 1.
        assert_eq!(
            get_item(&state, "explore_codebase").status,
            TodoStatus::Completed
        );
        assert_eq!(
            get_item(&state, "explore_codebase").content,
            "Explore django/db/backends/sqlite3/"
        );
        assert_eq!(
            get_item(&state, "analyze_and_propose").status,
            TodoStatus::InProgress
        );
        assert_eq!(
            get_item(&state, "analyze_and_propose").content,
            "Analyze current SQLite min version"
        );
        // Third item unchanged.
        assert_eq!(
            get_item(&state, "implementation").status,
            TodoStatus::Pending
        );
    }

    // ── regression: empty-string content must not wipe existing content ──

    #[test]
    fn merge_existing_item_empty_string_content_preserves_original() {
        // Model sends content: "" instead of omitting it. Must not wipe.
        let mut state = seed_state(&[("1", "Build the project", TodoStatus::InProgress)]);
        let updates = vec![make_update("1", Some(""), Some(TodoStatus::Completed))];
        apply_merge(&mut state, &updates).unwrap();

        let item = get_item(&state, "1");
        assert_eq!(item.status, TodoStatus::Completed);
        assert_eq!(item.content, "Build the project"); // unchanged
    }

    #[test]
    fn replace_empty_string_content_falls_back_to_id() {
        let mut state = TodoState::default();
        let updates = vec![make_update("task_1", Some(""), Some(TodoStatus::Pending))];
        apply_replace(&mut state, &updates).unwrap();

        assert_eq!(get_item(&state, "task_1").content, "task_1");
    }

    #[test]
    fn merge_new_item_empty_string_content_falls_back_to_id() {
        let mut state = TodoState::default();
        let updates = vec![make_update("task_1", Some(""), Some(TodoStatus::Pending))];
        apply_merge(&mut state, &updates).unwrap();

        assert_eq!(get_item(&state, "task_1").content, "task_1");
    }

    #[test]
    fn merge_with_null_content_and_lost_state() {
        // Same scenario but state was lost between calls (empty state).
        // The tool should still not error — falls back to id as content.
        let mut state = TodoState::default();

        let updates = vec![
            make_update("explore_codebase", None, Some(TodoStatus::Completed)),
            make_update("analyze_and_propose", None, Some(TodoStatus::InProgress)),
        ];
        apply_merge(&mut state, &updates).unwrap();

        assert_eq!(
            get_item(&state, "explore_codebase").content,
            "explore_codebase"
        );
        assert_eq!(
            get_item(&state, "explore_codebase").status,
            TodoStatus::Completed
        );
        assert_eq!(
            get_item(&state, "analyze_and_propose").content,
            "analyze_and_propose"
        );
        assert_eq!(
            get_item(&state, "analyze_and_propose").status,
            TodoStatus::InProgress
        );
    }
}

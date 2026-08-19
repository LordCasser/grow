//! New tool I/O types for the spec architecture.
//!
//! These types exist alongside the old `tool_input::ToolInput` and
//! `output::ToolOutput`. They will replace the old types once all tool
//! implementations are migrated to the new `Tool` trait.
//!
//! ## Design
//!
//! - `ToolInput` — one variant per built-in tool + `Dynamic(Value)`.
//!   `TryInto` derive generates `TryFrom<ToolInput>` for each inner type.
//! - `ToolOutput` — one variant per built-in tool + `Dynamic(Value)`.
//!   `From` derive generates `From<TypedOutput>` for each inner type.
use crate::implementations::BashToolInput;
use crate::implementations::context_recall::ContextRecallInput;
use crate::implementations::grow_build::ask_user_question::AskUserQuestionInput;
use crate::implementations::grow_build::grep::GrepSearchInput;
use crate::implementations::grow_build::list_dir::ListDirInput;
use crate::implementations::grow_build::plan_control::PlanControlInput;
use crate::implementations::grow_build::read_file::ReadFileInput;
use crate::implementations::grow_build::request_tool_access::RequestToolAccessInput;
use crate::implementations::grow_build::search_replace::SearchReplaceInput;
use crate::implementations::grow_build::todo::TodoWriteInput;
use crate::implementations::grow_build::web_fetch::WebFetchInput;
use crate::implementations::grow_build::write::WriteInput;
use crate::implementations::lsp::LspToolInput;
use crate::implementations::memory::types::{MemoryGetInput, MemorySearchInput};
use crate::implementations::search_tool::SearchToolInput;
use crate::implementations::skills::skill::SkillInput;
use crate::implementations::use_tool::UseToolInput;
use serde::{Deserialize, Serialize};
use tool_types::KillTaskToolInput;
use tool_types::TaskOutputToolInput;
use tool_types::TaskToolInput;
/// Raw input for an MCP (Model Context Protocol) tool call.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MCPToolInput {
    pub tool_name: String,
    pub tool_input: serde_json::Value,
}
/// Typed tool input — one variant per built-in tool, plus `Dynamic` for
/// MCP/runtime-registered tools.
///
/// Each variant wraps the tool's existing input struct. The new `Tool` trait
/// will use `TryFrom<ToolInput>` to extract the typed input.
///
/// `derive_more::TryInto` generates `TryFrom<ToolInput> for T` for each
/// inner type, so e.g. `ReadFileInput::try_from(input)` extracts the `ReadFileInput`
/// variant or returns an error.
#[derive(Debug, Clone, Serialize, Deserialize, derive_more::TryInto, derive_more::From)]
#[serde(tag = "variant")]
pub enum ToolInput {
    ReadFile(ReadFileInput),
    SearchReplace(SearchReplaceInput),
    Bash(BashToolInput),
    Grep(GrepSearchInput),
    ListDir(ListDirInput),
    TodoWrite(TodoWriteInput),
    Skill(SkillInput),
    MCPTool(MCPToolInput),
    TaskOutput(TaskOutputToolInput),
    KillTask(KillTaskToolInput),
    Task(TaskToolInput),
    WebFetch(WebFetchInput),
    Write(WriteInput),
    HashlineEdit(crate::implementations::grow_build_hashline::edit::types::HashlineEditInput),
    MemorySearch(MemorySearchInput),
    MemoryGet(MemoryGetInput),
    ContextRecall(ContextRecallInput),
    SearchTool(SearchToolInput),
    UseTool(UseToolInput),
    PlanControl(PlanControlInput),
    AskUserQuestion(AskUserQuestionInput),
    Lsp(LspToolInput),
    Monitor(crate::implementations::grow_build::monitor::types::MonitorInput),
    SchedulerCreate(crate::implementations::grow_build::scheduler::create::SchedulerCreateInput),
    SchedulerDelete(crate::implementations::grow_build::scheduler::delete::SchedulerDeleteInput),
    SchedulerList(crate::implementations::grow_build::scheduler::list::SchedulerListInput),
    GetGoal(crate::implementations::grow_build::update_goal::GetGoalInput),
    UpdateGoal(crate::implementations::grow_build::update_goal::UpdateGoalInput),
    UpdateGoalProgress(crate::implementations::grow_build::update_goal::UpdateGoalProgressInput),
    RequestGoalReplan(crate::implementations::grow_build::update_goal::RequestGoalReplanInput),
    SubmitGoalPlanSection(
        crate::implementations::grow_build::update_goal::SubmitGoalPlanSectionInput,
    ),
    FinalizeGoalPlan(crate::implementations::grow_build::update_goal::FinalizeGoalPlanInput),
    Workflow(crate::implementations::grow_build::workflow::WorkflowToolInput),
    RequestToolAccess(RequestToolAccessInput),
    /// Dynamic input for runtime-registered tools (MCP, etc.)
    Dynamic(serde_json::Value),
}
impl ToolInput {
    /// The real target tool for *meta-dispatch* tools whose wire `function.name`
    /// is only the wrapper (`use_tool`), or `None` for
    /// ordinary tools (already named by `function.name`). Single source of truth
    /// for hook matching / diagnostics; callers fall back to `function.name` on
    /// `None`. Add any new dispatcher here.
    pub fn dispatch_target_name(&self) -> Option<String> {
        match self {
            ToolInput::UseTool(input) => Some(input.tool_name.clone()),
            _ => None,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn try_into_input_succeeds_for_matching_variant() {
        let input = ToolInput::ListDir(ListDirInput {
            target_directory: "/tmp".to_string(),
        });
        let result: Result<ListDirInput, _> = input.try_into();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().target_directory, "/tmp");
    }
    #[test]
    fn dispatch_target_name_resolves_meta_dispatch_tools() {
        let use_tool = ToolInput::UseTool(UseToolInput {
            tool_name: "linear__save_issue".to_string(),
            tool_input: serde_json::json!({}),
        });
        assert_eq!(
            use_tool.dispatch_target_name().as_deref(),
            Some("linear__save_issue")
        );
        let ordinary = ToolInput::ListDir(ListDirInput {
            target_directory: "/tmp".to_string(),
        });
        assert_eq!(ordinary.dispatch_target_name(), None);
    }
    #[test]
    fn try_into_input_fails_for_mismatched_variant() {
        let input = ToolInput::ListDir(ListDirInput {
            target_directory: "/tmp".to_string(),
        });
        let result: Result<ReadFileInput, _> = input.try_into();
        assert!(result.is_err());
    }
    #[test]
    fn try_into_all_input_variants() {
        let rf: Result<ReadFileInput, _> = ToolInput::ReadFile(ReadFileInput {
            path: "x".into(),
            offset: None,
            limit: None,
            pages: None,
            format: None,
        })
        .try_into();
        assert_eq!(rf.unwrap().path, "x");
        let bash: Result<BashToolInput, _> = ToolInput::Bash(BashToolInput {
            command: "ls".into(),
            timeout: None,
            description: "list files".into(),
            is_background: false,
        })
        .try_into();
        assert_eq!(bash.unwrap().command, "ls");
        let grep: Result<GrepSearchInput, _> = ToolInput::Grep(GrepSearchInput {
            pattern: "test".into(),
            path: None,
            glob: None,
            output_mode: None,
            before_context: None,
            after_context: None,
            context: None,
            case_insensitive: false,
            head_limit: None,
            multiline: false,
            r#type: None,
        })
        .try_into();
        assert_eq!(grep.unwrap().pattern, "test");
        let kill: Result<KillTaskToolInput, _> = ToolInput::KillTask(KillTaskToolInput {
            task_id: "t1".into(),
        })
        .try_into();
        assert_eq!(kill.unwrap().task_id, "t1");
    }
    #[test]
    fn dynamic_input_holds_arbitrary_json() {
        let input = ToolInput::Dynamic(serde_json::json!({"custom": "data"}));
        match input {
            ToolInput::Dynamic(v) => {
                assert_eq!(v["custom"], "data");
            }
            _ => panic!("Expected Dynamic variant"),
        }
    }
}

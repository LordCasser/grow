//! New-architecture tool implementations (NewTool trait).
//!
//! Each sub-module here contains a tool that implements `NewTool` instead
//! of the old `Tool` trait. During migration, old implementations live in
//! `implementations/<tool>/` and new implementations live in
//! `implementations/grow_build/<tool>/`.
//!
//! The [`register_all()`] function is the single entry-point for wiring up
//! the standard toolset. It inserts shared resources (`Terminal`,
//! `AvailableSkills`, `BashParams`) and registers every built-in tool.
pub mod ask_user_question;
pub mod bash;
#[path = "deploy_app_stub.rs"]
pub mod deploy_app;
pub mod grep;
pub mod kill_task;
pub mod list_dir;
pub mod lsp;
pub mod monitor;
pub mod plan_control;
pub mod read_file;
pub mod request_tool_access;
pub mod scheduler;
pub mod search_replace;
pub(crate) mod storage;
pub mod task;
pub mod task_output;
pub mod todo;
pub mod update_goal;
pub mod web_fetch;
pub mod workflow;
pub mod write;
pub use ask_user_question::AskUserQuestionTool;
pub use bash::BashTool;
pub use deploy_app::{AppBuilderDeployerConfig, DEPLOY_APP_TOOL_NAME};
pub use grep::GrepTool;
pub use kill_task::{KillTaskTool, KillTerminalCommandTool};
pub use list_dir::ListDirTool;
pub use lsp::LspTool;
pub use monitor::tool::MonitorTool;
pub use plan_control::PlanControlTool;
pub use read_file::ReadFileTool;
pub use request_tool_access::RequestToolAccessTool;
pub use scheduler::create::{
    LoopFireMode, SCHEDULER_CREATE_TOOL_NAME, SchedulerCreateTool, loop_schedule_instruction,
    loop_usage_message,
};
pub use scheduler::delete::{SCHEDULER_DELETE_TOOL_NAME, SchedulerDeleteTool};
pub use scheduler::list::SchedulerListTool;
pub use search_replace::SearchReplaceTool;
pub use task::TaskTool;
pub use task_output::{GetTerminalCommandOutputTool, TaskOutputTool, WaitTasksTool};
pub use todo::TodoWriteTool;
pub use update_goal::{
    GET_GOAL_TOOL_NAME, GetGoalInput, GetGoalTool, REQUEST_GOAL_REPLAN_TOOL_NAME,
    RequestGoalReplanTool, UPDATE_GOAL_PROGRESS_TOOL_NAME, UPDATE_GOAL_TOOL_NAME,
    UpdateGoalProgressTool, UpdateGoalTool,
};
pub use web_fetch::{WebFetchClient, WebFetchConfig, WebFetchParams, WebFetchTool};
pub use workflow::{WORKFLOW_TOOL_NAME, WorkflowTool};
pub use write::{WriteInput, WriteTool};

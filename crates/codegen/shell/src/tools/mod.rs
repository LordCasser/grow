//! Tool infrastructure for shell.
//!
//! All tool execution goes through `tools` via the `ToolBridge`.
//! Types (ToolOutput, ToolInput, TodoState, etc.) come from `tools` directly.

pub mod config;
pub mod notification_bridge;
pub mod todo;
pub mod tool_context;

pub use self::{
    config::{BashToolConfig, FileToolset, ShellToolsetConfig},
    tool_context::ToolContext,
};

// Re-export key types from tools for convenience
pub use self::todo::{TodoId, TodoItem, TodoPriority, TodoStatus};
pub use tools::types::output::ToolOutput;
pub use tools::types::{MCPToolInput, ToolInput};

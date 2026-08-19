//! Canonical data contracts for workspace RPCs and workspace-observed events.
//!
//! The crate contains no runtime, transport envelope, or alternate streaming
//! protocol. Each operation is represented once under [`rpc`].

pub mod events;
pub mod rpc;

/// MCP tool name delimiter: server names are qualified as `"server__tool"`.
pub const MCP_TOOL_NAME_DELIMITER: &str = "__";

pub use crate::events::{FsEventKind, WorkspaceEvent};

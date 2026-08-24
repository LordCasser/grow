//! Shared types for Grow's in-process tool runtime.

#![forbid(unsafe_code)]

mod capabilities;
mod ids;
pub mod turn_hook;

pub use capabilities::{HookKind, StreamingSpec, ToolAccess, ToolCapabilities};
pub use ids::{IdError, ToolCallId, ToolId};

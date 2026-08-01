//! ToolBridge: re-exported from `tools`.
//!
//! The bridge implementation now lives in `tools::bridge`.
//! This module re-exports everything for backward compatibility.

pub use tools::bridge::{ToolBridge, ToolBridgeResult};

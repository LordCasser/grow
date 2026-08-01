//! API-agnostic conversation representation.
//!
//! The canonical types now live in `sampling_types::conversation`.
//! This module re-exports them and adds shell-specific types
//! (`ConversationRequestTrace`) that depend on internal crate types.

// Re-export everything from the standalone crate.
pub use sampling_types::conversation::*;

// Tests for conversation types now live in sampling-types crate.

//! Typed requests and responses for local workspace operations.
//!
//! These types live outside `workspace` so shell and UI crates can share
//! the contract without depending on the workspace implementation.

use serde::Serialize;
use serde::de::DeserializeOwned;

pub mod agents_md;
pub mod code_nav;
pub mod deploy;
pub mod export_github;
pub mod fs;
pub mod git;
pub mod hooks;
pub mod hunks;
pub mod search;
pub mod session;
pub mod skills;
pub mod workspace;
pub mod worktree;

/// Marker trait for typed local workspace requests.
pub trait WorkspaceRpc: Serialize {
    /// Stable operation name (e.g. `"workspace.git_status_ext"`).
    const METHOD: &'static str;
    type Response: Serialize + DeserializeOwned + Send;
}

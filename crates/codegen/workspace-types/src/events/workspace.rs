//! Events emitted from external state observed by the workspace.
//!
//! Sampler-caused state does not belong here: request results and Timeline are
//! the canonical path for prompt, tool, compaction, and agent lifecycle facts.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Filesystem change kind emitted by the workspace watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsEventKind {
    Created,
    Modified,
    Removed,
    Renamed,
}

/// A current workspace-observed fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum WorkspaceEvent {
    FsChanged { path: PathBuf, kind: FsEventKind },
    CodebaseIndexUpdated { files_indexed: u64 },
    ToolsChanged { session_id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip() {
        for event in [
            WorkspaceEvent::FsChanged {
                path: PathBuf::from("/workspace/src/lib.rs"),
                kind: FsEventKind::Modified,
            },
            WorkspaceEvent::CodebaseIndexUpdated { files_indexed: 42 },
            WorkspaceEvent::ToolsChanged {
                session_id: "session-1".into(),
            },
        ] {
            let json = serde_json::to_string(&event).unwrap();
            assert_eq!(
                serde_json::from_str::<WorkspaceEvent>(&json).unwrap(),
                event
            );
        }
    }
}

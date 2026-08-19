//! Workspace identity operation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::WorkspaceRpc;

/// Return the current workspace environment snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceInfoReq {}

impl WorkspaceRpc for WorkspaceInfoReq {
    const METHOD: &'static str = "workspace.info";
    type Response = Value;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_constant() {
        assert_eq!(WorkspaceInfoReq::METHOD, "workspace.info");
    }
}

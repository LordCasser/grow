//! Wire mirror of the `workspace.hook_registry` response, kept byte-identical to
//! the upstream serde shape so this lean crate avoids the heavy `hooks` dep.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::WorkspaceRpc;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookRegistryReq {}

impl WorkspaceRpc for HookRegistryReq {
    const METHOD: &'static str = "workspace.hook_registry";
    type Response = HookRegistryWire;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookRegistryWire {
    pub hooks: HashMap<HookEventNameWire, Vec<HookSpecWire>>,
}

/// Compiled `matcher` omitted; drift-guarded by `hook_spec_wire_covers_all_upstream_fields`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookSpecWire {
    pub name: String,
    pub event: HookEventNameWire,
    pub handler_type: String,
    pub configured_matcher: Option<String>,
    pub enabled: bool,
    pub command: Option<PathBuf>,
    pub command_raw: Option<String>,
    pub url: Option<String>,
    pub url_raw: Option<String>,
    pub timeout_ms: u64,
    pub source_dir: PathBuf,
    pub extra_env: HashMap<String, String>,
    pub layer: String,
}

/// Snake_case JSON map key for the closed hook-event protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEventNameWire {
    SessionStart,
    SessionEnd,
    Stop,
    StopFailure,
    StopCancelled,
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    PermissionDenied,
    UserPromptSubmit,
    Notification,
    SubagentStart,
    SubagentStop,
    PreCompact,
    PostCompact,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_constant() {
        assert_eq!(HookRegistryReq::METHOD, "workspace.hook_registry");
    }

    #[test]
    fn hook_event_name_wire_snake_case_round_trip() {
        // All variants (mirrors upstream `event_name_deser_all_variants`).
        for (variant, wire) in [
            (HookEventNameWire::SessionStart, "session_start"),
            (HookEventNameWire::SessionEnd, "session_end"),
            (HookEventNameWire::Stop, "stop"),
            (HookEventNameWire::StopFailure, "stop_failure"),
            (HookEventNameWire::StopCancelled, "stop_cancelled"),
            (HookEventNameWire::PreToolUse, "pre_tool_use"),
            (HookEventNameWire::PostToolUse, "post_tool_use"),
            (
                HookEventNameWire::PostToolUseFailure,
                "post_tool_use_failure",
            ),
            (HookEventNameWire::PermissionDenied, "permission_denied"),
            (HookEventNameWire::UserPromptSubmit, "user_prompt_submit"),
            (HookEventNameWire::Notification, "notification"),
            (HookEventNameWire::SubagentStart, "subagent_start"),
            (HookEventNameWire::SubagentStop, "subagent_stop"),
            (HookEventNameWire::PreCompact, "pre_compact"),
            (HookEventNameWire::PostCompact, "post_compact"),
        ] {
            assert_eq!(
                serde_json::to_value(&variant).unwrap(),
                serde_json::json!(wire)
            );
            let parsed: HookEventNameWire =
                serde_json::from_value(serde_json::json!(wire)).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn hook_event_name_wire_rejects_unknown_value() {
        assert!(
            serde_json::from_value::<HookEventNameWire>(serde_json::json!("future_event")).is_err()
        );
    }

    #[test]
    fn hook_registry_wire_round_trips_server_json() {
        // A representative server-side `HookRegistry` serialization.
        let json = serde_json::json!({
            "hooks": {
                "pre_tool_use": [{
                    "name": "global/safety",
                    "event": "pre_tool_use",
                    "handler_type": "command",
                    "configured_matcher": "Bash",
                    "enabled": true,
                    "command": "/bin/check.sh",
                    "command_raw": "${X}/check.sh",
                    "url": null,
                    "url_raw": null,
                    "timeout_ms": 5000,
                    "source_dir": "/home/u/.grow/hooks",
                    "extra_env": { "FOO": "bar" },
                    "layer": "file"
                }]
            }
        });
        let wire: HookRegistryWire = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&wire).unwrap(), json);
    }
}

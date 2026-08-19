//! Discovery methods (`workspace.discover_skills`).
//!
//! SYNC: [`SkillInfo`] / [`SkillScope`] mirror the serde shape of
//! `tools/src/implementations/skills/types.rs` (the type the
//! server serializes); the fixture tests below pin the contract.
//!
//! This module owns the workspace skill-discovery wire schema.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::WorkspaceRpc;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverSkillsReq {}

impl WorkspaceRpc for DiscoverSkillsReq {
    const METHOD: &'static str = "workspace.discover_skills";
    type Response = Vec<SkillInfo>;
}

/// Scope/priority of a skill based on where it was discovered.
/// Lower values have higher priority.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillScope {
    /// cwd/.grow/skills
    Local,
    /// repo_root/.grow/skills
    Repo,
    /// ~/.grow/skills
    User,
    /// ~/.grow/server-skills (synced from the skill store)
    Server,
    /// platform built-in skills
    Bundled,
    /// plugin-provided skills
    Plugin,
}

const fn default_true() -> bool {
    true
}

/// A discovered skill as serialized by `workspace.discover_skills`.
/// See the module SYNC note for the source of truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub description: String,
    #[serde(default)]
    pub has_user_specified_description: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
    pub path: String,
    pub scope: SkillScope,
    /// Raw JSON: the shape is the tools crate's `ConfigSource` tagged
    /// enum, which RPC clients have no need to interpret structurally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_source: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default = "default_true")]
    pub user_invocable: bool,
    #[serde(default)]
    pub disable_model_invocation: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_info_deserializes_minimal_payload() {
        let raw = serde_json::json!({
            "name": "my-skill",
            "description": "A test skill",
            "path": "/workspace/.grow/skills/my-skill/SKILL.md",
            "scope": "local",
        });
        let info: SkillInfo = serde_json::from_value(raw).unwrap();
        assert_eq!(info.name, "my-skill");
        assert_eq!(info.scope, SkillScope::Local);
        assert!(info.user_invocable, "default_true");
        assert!(info.enabled, "default_true");
        assert!(!info.has_user_specified_description);
        assert!(info.config_source.is_none());
    }

    // Fixture mirrored field-for-field from the tools SkillInfo
    // serialization; refresh from a captured live response when the wire
    // shape is in question.
    #[test]
    fn skill_info_deserializes_full_payload() {
        let raw = serde_json::json!({
            "name": "deploy",
            "display_name": "Deploy Helper",
            "description": "Deploys the app",
            "has_user_specified_description": true,
            "paths": ["infra/**"],
            "when_to_use": "Use when deploying",
            "short_description": "Deploy",
            "author": "someone",
            "argument_hint": "environment name",
            "license": "Apache-2.0",
            "compatibility": "Requires kubectl",
            "metadata": {"team": "infra"},
            "path": "/root/.grow/server-skills/deploy/SKILL.md",
            "scope": "server",
            "config_source": {"type": "user", "path": "/root/.grow/skills"},
            "plugin_name": "infra-plugin",
            "plugin_version": "1.0.0",
            "plugin_root": "/root/.grow/plugins/infra-plugin",
            "plugin_data": "/root/.grow/plugin-data/infra-plugin",
            "allowed_tools": ["bash"],
            "model": "grow-4",
            "effort": "high",
            "user_invocable": true,
            "disable_model_invocation": false,
            "enabled": true,
            "body": "# Deploy\n",
        });
        let info: SkillInfo = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(info.scope, SkillScope::Server);
        assert_eq!(info.display_name.as_deref(), Some("Deploy Helper"));
        assert_eq!(info.plugin_name.as_deref(), Some("infra-plugin"));
        assert_eq!(
            info.config_source.as_ref().and_then(|v| v["type"].as_str()),
            Some("user")
        );

        // Re-serializing must reproduce the input (Value equality is
        // order-insensitive; this pins field presence via
        // skip_serializing_if and every value).
        let round = serde_json::to_value(&info).unwrap();
        assert_eq!(round, raw);
    }

    #[test]
    fn skill_scope_known_values() {
        for (raw, expected) in [
            ("local", SkillScope::Local),
            ("repo", SkillScope::Repo),
            ("user", SkillScope::User),
            ("server", SkillScope::Server),
            ("bundled", SkillScope::Bundled),
            ("plugin", SkillScope::Plugin),
        ] {
            let v: SkillScope = serde_json::from_value(serde_json::json!(raw)).unwrap();
            assert_eq!(v, expected, "scope {raw}");
        }
    }

    #[test]
    fn skill_scope_rejects_unknown_value() {
        assert!(serde_json::from_value::<SkillScope>(serde_json::json!("galactic")).is_err());
    }

    #[test]
    fn skill_scope_known_values_round_trip() {
        for raw in ["local", "repo", "user", "server", "bundled", "plugin"] {
            let v: SkillScope = serde_json::from_value(serde_json::json!(raw)).unwrap();
            assert_eq!(serde_json::to_value(&v).unwrap(), serde_json::json!(raw));
        }
    }

    #[test]
    fn skill_info_rejects_unknown_fields() {
        let raw = serde_json::json!({
            "name": "n",
            "description": "d",
            "path": "/p/SKILL.md",
            "scope": "repo",
            "brand_new_field": {"nested": true},
        });
        assert!(serde_json::from_value::<SkillInfo>(raw).is_err());
    }

    #[test]
    fn method_constant() {
        assert_eq!(DiscoverSkillsReq::METHOD, "workspace.discover_skills");
    }
}

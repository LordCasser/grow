//! Plugin manifest parsing and validation.
//!
//! Every plugin is rooted by one required `plugin.json`. Component paths may
//! use the standard directories, but discovery never invents plugin identity
//! from a directory name or probes alternate manifest locations.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Maximum length of a plugin name (kebab-case identifier).
const MAX_PLUGIN_NAME_LEN: usize = 64;

/// Regex pattern for valid plugin names: lowercase alphanumeric + hyphens.
fn is_valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_PLUGIN_NAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

/// Author metadata from a plugin manifest.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Author {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// Resolve component directories relative to a plugin root.
/// Paths that escape the plugin root are rejected.
fn resolve_paths(paths: &[String], plugin_root: &Path) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|path| plugin_root.join(path))
        .filter(|resolved| {
            if is_path_contained(resolved, plugin_root) {
                true
            } else {
                tracing::warn!(
                    path = %resolved.display(),
                    plugin_root = %plugin_root.display(),
                    "manifest path escapes plugin root; skipping"
                );
                false
            }
        })
        .collect()
}

/// Check whether a resolved path stays within the plugin root.
///
/// Canonicalizes both sides (resolving symlinks and `..`) before the prefix check.
fn is_path_contained(resolved: &Path, plugin_root: &Path) -> bool {
    let canonical_root =
        dunce::canonicalize(plugin_root).unwrap_or_else(|_| plugin_root.to_path_buf());
    let canonical_resolved =
        dunce::canonicalize(resolved).unwrap_or_else(|_| resolved.to_path_buf());
    // Fail-closed >MAX_PATH caveat: see workspace clippy.toml.
    canonical_resolved.starts_with(&canonical_root)
}

/// Resolve a plugin component path (hooks, MCP, LSP) from a manifest field.
///
/// If the field is `Path(p)`, resolves relative to plugin root with containment check.
/// If `Inline(_)`, returns `None` (caller reads inline value directly).
/// If `None`, checks for `default_file` at the plugin root.
fn resolve_component_path(
    field: &Option<PathOrInline>,
    plugin_root: &Path,
    default_file: &str,
    label: &str,
) -> Option<PathBuf> {
    match field {
        Some(PathOrInline::Path(p)) => {
            let resolved = plugin_root.join(p);
            if !is_path_contained(&resolved, plugin_root) {
                tracing::warn!(
                    path = %resolved.display(),
                    plugin_root = %plugin_root.display(),
                    "{label} path escapes plugin root; skipping"
                );
                return None;
            }
            resolved.is_file().then_some(resolved)
        }
        Some(PathOrInline::Inline(_)) => None,
        None => {
            let default = plugin_root.join(default_file);
            default.is_file().then_some(default)
        }
    }
}

/// A value that can be either a file path (string) or an inline JSON object.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PathOrInline {
    Path(String),
    Inline(serde_json::Value),
}

/// Parsed plugin manifest from `plugin.json`.
///
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifest {
    /// User-facing plugin namespace (kebab-case).  Required.
    pub name: String,
    /// Semver version string.
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<Author>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,

    // ── Component path overrides (supplement convention dirs) ──────
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub commands: Option<Vec<String>>,
    #[serde(default)]
    pub agents: Option<Vec<String>>,
    #[serde(default)]
    pub hooks: Option<PathOrInline>,
    #[serde(default)]
    pub mcp_servers: Option<PathOrInline>,
    #[serde(default)]
    pub lsp_servers: Option<PathOrInline>,
}

impl PluginManifest {
    /// Validate the parsed manifest.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if !is_valid_plugin_name(&self.name) {
            return Err(ManifestError::InvalidName {
                name: self.name.clone(),
                reason: format!(
                    "must be 1-{MAX_PLUGIN_NAME_LEN} chars, lowercase alphanumeric + hyphens, \
                     no leading/trailing hyphens"
                ),
            });
        }
        Ok(())
    }

    pub fn skill_dirs(&self, plugin_root: &Path) -> Vec<PathBuf> {
        resolve_dirs(&self.skills, plugin_root, "skills")
    }

    pub fn command_dirs(&self, plugin_root: &Path) -> Vec<PathBuf> {
        resolve_dirs(&self.commands, plugin_root, "commands")
    }

    pub fn agent_dirs(&self, plugin_root: &Path) -> Vec<PathBuf> {
        resolve_dirs(&self.agents, plugin_root, "agents")
    }

    /// Resolve the hooks path from the manifest.
    /// Returns the manifest-specified path or the default `hooks/hooks.json`.
    pub fn hooks_path(&self, plugin_root: &Path) -> Option<PathBuf> {
        resolve_component_path(&self.hooks, plugin_root, "hooks/hooks.json", "hooks")
    }

    pub fn mcp_config_path(&self, plugin_root: &Path) -> Option<PathBuf> {
        if matches!(self.mcp_servers, Some(PathOrInline::Inline(_))) {
            let default = plugin_root.join(".mcp.json");
            return default.is_file().then_some(default);
        }
        resolve_component_path(&self.mcp_servers, plugin_root, ".mcp.json", "MCP config")
    }

    /// Get inline hooks JSON value, if the manifest uses inline hooks.
    ///
    /// Inline hooks are fully supported — the runtime parses and executes them
    /// via `parse_plugin_hooks_from_value()`. This accessor is used during
    /// `LoadedPlugin` construction and by the hooks adapter.
    pub fn inline_hooks(&self) -> Option<&serde_json::Value> {
        match &self.hooks {
            Some(PathOrInline::Inline(v)) => Some(v),
            _ => None,
        }
    }

    /// Get inline MCP servers JSON value, if the manifest uses inline MCP.
    ///
    /// Inline MCP servers are fully supported — the runtime parses and starts
    /// them via `load_plugin_mcp_servers_from_value()`. This accessor is used
    /// during `LoadedPlugin` construction and by the MCP merger.
    pub fn inline_mcp_servers(&self) -> Option<&serde_json::Value> {
        match &self.mcp_servers {
            Some(PathOrInline::Inline(v)) => Some(v),
            _ => None,
        }
    }

    pub fn lsp_config_path(&self, plugin_root: &Path) -> Option<PathBuf> {
        resolve_component_path(&self.lsp_servers, plugin_root, ".lsp.json", "LSP config")
    }

    pub fn inline_lsp_servers(&self) -> Option<&serde_json::Value> {
        match &self.lsp_servers {
            Some(PathOrInline::Inline(v)) => Some(v),
            _ => None,
        }
    }

    /// Log informational messages about manifest features.
    ///
    /// Called during discovery. Inline hooks and MCP servers are now
    /// fully supported; this method logs when they are detected.
    pub fn log_inline_features(&self, plugin_name: &str) {
        if self.inline_hooks().is_some() {
            tracing::info!(plugin = plugin_name, "plugin uses inline hooks in manifest");
        }
        if self.inline_mcp_servers().is_some() {
            tracing::info!(
                plugin = plugin_name,
                "plugin uses inline mcpServers in manifest"
            );
        }
        if self.inline_lsp_servers().is_some() {
            tracing::info!(
                plugin = plugin_name,
                "plugin uses inline lspServers in manifest"
            );
        }
    }
}

/// Resolve directories from a manifest field or fall back to a default subdirectory.
fn resolve_dirs(
    field: &Option<Vec<String>>,
    plugin_root: &Path,
    default_name: &str,
) -> Vec<PathBuf> {
    match field {
        Some(paths) => resolve_paths(paths, plugin_root),
        None => {
            let default = plugin_root.join(default_name);
            if default.is_dir() {
                vec![default]
            } else {
                vec![]
            }
        }
    }
}

// ── Manifest loading ──────────────────────────────────────────────────

/// Load a plugin manifest from the given plugin root directory.
pub fn load_manifest(plugin_root: &Path) -> Result<Box<PluginManifest>, ManifestError> {
    let manifest_path = plugin_root.join("plugin.json");
    if !manifest_path.is_file() {
        return Err(ManifestError::Missing {
            path: manifest_path,
        });
    }
    let content =
        std::fs::read_to_string(&manifest_path).map_err(|source| ManifestError::IoError {
            path: manifest_path.clone(),
            source,
        })?;
    let manifest: PluginManifest =
        serde_json::from_str(&content).map_err(|error| ManifestError::ParseError {
            path: manifest_path,
            message: error.to_string(),
        })?;
    manifest.validate()?;
    manifest.log_inline_features(&manifest.name);
    Ok(Box::new(manifest))
}

/// Perform plugin-token substitution in a string.
///
/// Replaces `${GROW_PLUGIN_ROOT}` and `${GROW_PLUGIN_DATA}` with the provided values.
///
/// Delegates to [`tools::util::substitute_plugin_tokens`], the single
/// source of truth shared with plugin skill/command body substitution.
pub fn substitute_env_vars(s: &str, plugin_root: &str, plugin_data: &str) -> String {
    tools::util::substitute_plugin_tokens(s, Some(plugin_root), Some(plugin_data))
}

pub fn normalize_inline_mcp_servers(value: &serde_json::Value) -> serde_json::Value {
    let inner = match value.get("mcpServers") {
        Some(servers) if servers.is_object() => servers.clone(),
        _ => value.clone(),
    };
    serde_json::json!({ "mcpServers": inner })
}

// ── Errors ────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("required plugin manifest not found at {path}")]
    Missing { path: PathBuf },

    #[error("invalid plugin name {name:?}: {reason}")]
    InvalidName { name: String, reason: String },

    #[error("failed to read {path}: {source}")]
    IoError {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse {path}: {message}")]
    ParseError { path: PathBuf, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_plugin_names() {
        assert!(is_valid_plugin_name("my-plugin"));
        assert!(is_valid_plugin_name("a"));
        assert!(is_valid_plugin_name("deployment-tools"));
        assert!(is_valid_plugin_name("plugin123"));
        assert!(is_valid_plugin_name("a-b-c"));
    }

    #[test]
    fn invalid_plugin_names() {
        assert!(!is_valid_plugin_name(""));
        assert!(!is_valid_plugin_name("-start"));
        assert!(!is_valid_plugin_name("end-"));
        assert!(!is_valid_plugin_name("UPPER"));
        assert!(!is_valid_plugin_name("has space"));
        assert!(!is_valid_plugin_name("has_underscore"));
        assert!(!is_valid_plugin_name("has.dot"));
        assert!(!is_valid_plugin_name(&"a".repeat(65)));
    }

    #[test]
    fn parse_minimal_manifest() {
        let json = r#"{"name": "my-plugin"}"#;
        let manifest: PluginManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.name, "my-plugin");
        assert!(manifest.version.is_none());
        assert!(manifest.description.is_none());
        assert!(manifest.skills.is_none());
        manifest.validate().unwrap();
    }

    #[test]
    fn parse_full_manifest() {
        let json = r#"{
            "name": "deployment-tools",
            "version": "1.2.0",
            "description": "Tools for deployment",
            "author": {"name": "Test", "email": "test@example.com"},
            "homepage": "https://example.com",
            "repository": "https://github.com/example/plugin",
            "license": "MIT",
            "keywords": ["ci-cd", "deploy"],
            "skills": ["./custom/skills/"],
            "agents": ["./custom-agents/"],
            "hooks": "./config/hooks.json",
            "mcpServers": "./mcp-config.json"
        }"#;
        let manifest: PluginManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.name, "deployment-tools");
        assert_eq!(manifest.version.as_deref(), Some("1.2.0"));
        assert_eq!(manifest.keywords, vec!["ci-cd", "deploy"]);
        assert_eq!(manifest.skills, Some(vec!["./custom/skills/".to_string()]));
        manifest.validate().unwrap();
    }

    #[test]
    fn parse_manifest_rejects_unknown_fields() {
        let json = r#"{
            "name": "my-plugin",
            "marketplace": true,
            "installState": "active",
            "futureField": {"nested": "value"},
            "outputStyles": "./styles/"
        }"#;
        assert!(serde_json::from_str::<PluginManifest>(json).is_err());
    }

    #[test]
    fn parse_manifest_inline_hooks() {
        let json = r#"{
            "name": "my-plugin",
            "hooks": {
                "hooks": {
                    "post_tool_use": [{"hooks": [{"type": "command", "command": "lint"}]}]
                }
            }
        }"#;
        let manifest: PluginManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.inline_hooks().is_some());
    }

    #[test]
    fn parse_manifest_inline_mcp() {
        let json = r#"{
            "name": "my-plugin",
            "mcpServers": {
                "mcpServers": {
                    "database": {
                        "command": "./servers/db-server",
                        "args": ["--config", "./config.json"]
                    }
                }
            }
        }"#;
        let manifest: PluginManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.inline_mcp_servers().is_some());
    }

    #[test]
    fn parse_manifest_multiple_skill_paths() {
        let json = r#"{
            "name": "my-plugin",
            "skills": ["./skills-a/", "./skills-b/"]
        }"#;
        let manifest: PluginManifest = serde_json::from_str(json).unwrap();
        let paths = manifest.skills.unwrap();
        assert_eq!(paths, vec!["./skills-a/", "./skills-b/"]);
    }

    #[test]
    fn load_manifest_from_tempdir() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_root = tmp.path().join("my-plugin");
        std::fs::create_dir_all(&plugin_root).unwrap();

        assert!(matches!(
            load_manifest(&plugin_root),
            Err(ManifestError::Missing { .. })
        ));

        // Write root plugin.json
        let manifest_path = plugin_root.join("plugin.json");
        std::fs::write(
            &manifest_path,
            r#"{"name": "my-plugin", "version": "0.1.0"}"#,
        )
        .unwrap();

        let manifest = load_manifest(&plugin_root).unwrap();
        assert_eq!(manifest.name, "my-plugin");
        assert_eq!(manifest.version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn manifest_rejects_invalid_name() {
        let json = r#"{"name": "INVALID_NAME"}"#;
        let manifest: PluginManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn substitute_env_vars_replaces_all() {
        let input = "${GROW_PLUGIN_ROOT}/bin:${GROW_PLUGIN_DATA}/cache";
        let result = substitute_env_vars(input, "/home/user/plugin", "/home/user/.data/plugin");
        assert_eq!(
            result,
            "/home/user/plugin/bin:/home/user/.data/plugin/cache"
        );
    }

    #[test]
    fn skill_dirs_default_convention() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("test-plugin");
        std::fs::create_dir_all(root.join("skills")).unwrap();

        let manifest = PluginManifest {
            name: "test-plugin".into(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            skills: None,
            commands: None,
            agents: None,
            hooks: None,
            mcp_servers: None,
            lsp_servers: None,
        };
        let dirs = manifest.skill_dirs(&root);
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].ends_with("skills"));
    }

    #[test]
    fn skill_dirs_no_default_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("no-skills");
        std::fs::create_dir_all(&root).unwrap();

        let manifest = PluginManifest {
            name: "no-skills".into(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            skills: None,
            commands: None,
            agents: None,
            hooks: None,
            mcp_servers: None,
            lsp_servers: None,
        };
        let dirs = manifest.skill_dirs(&root);
        assert!(dirs.is_empty());
    }

    #[test]
    fn path_escape_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("contained");
        std::fs::create_dir_all(&root).unwrap();
        // Create an outside directory
        let outside = tmp.path().join("outside-skills");
        std::fs::create_dir_all(&outside).unwrap();

        let manifest = PluginManifest {
            name: "escape-test".into(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            skills: Some(vec!["../outside-skills".to_string()]),
            commands: None,
            agents: None,
            hooks: None,
            mcp_servers: None,
            lsp_servers: None,
        };
        let dirs = manifest.skill_dirs(&root);
        assert!(
            dirs.is_empty(),
            "path escaping plugin root should be rejected"
        );
    }

    #[test]
    fn path_within_root_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("plugin");
        std::fs::create_dir_all(root.join("custom-skills")).unwrap();

        let manifest = PluginManifest {
            name: "within-test".into(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            skills: Some(vec!["custom-skills".to_string()]),
            commands: None,
            agents: None,
            hooks: None,
            mcp_servers: None,
            lsp_servers: None,
        };
        let dirs = manifest.skill_dirs(&root);
        assert_eq!(dirs.len(), 1, "path within plugin root should be accepted");
    }

    #[test]
    fn hooks_path_escape_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("plugin");
        std::fs::create_dir_all(&root).unwrap();
        // Create a hooks file outside the plugin root
        let outside = tmp.path().join("outside-hooks.json");
        std::fs::write(&outside, r#"{"hooks":{}}"#).unwrap();

        let manifest = PluginManifest {
            name: "escape-hooks".into(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            skills: None,
            commands: None,
            agents: None,
            hooks: Some(PathOrInline::Path("../outside-hooks.json".to_string())),
            mcp_servers: None,
            lsp_servers: None,
        };
        assert!(
            manifest.hooks_path(&root).is_none(),
            "hooks path escaping plugin root should be rejected"
        );
    }

    #[test]
    fn mcp_path_escape_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("plugin");
        std::fs::create_dir_all(&root).unwrap();
        let outside = tmp.path().join("outside-mcp.json");
        std::fs::write(&outside, r#"{"mcpServers":{}}"#).unwrap();

        let manifest = PluginManifest {
            name: "escape-mcp".into(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            skills: None,
            commands: None,
            agents: None,
            hooks: None,
            mcp_servers: Some(PathOrInline::Path("../outside-mcp.json".to_string())),
            lsp_servers: None,
        };
        assert!(
            manifest.mcp_config_path(&root).is_none(),
            "MCP path escaping plugin root should be rejected"
        );
    }

    fn manifest_with_inline_mcp(servers: serde_json::Value) -> PluginManifest {
        PluginManifest {
            name: "sentry".into(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            skills: None,
            commands: None,
            agents: None,
            hooks: None,
            mcp_servers: Some(PathOrInline::Inline(servers)),
            lsp_servers: None,
        }
    }

    #[test]
    fn normalize_inline_mcp_servers_wraps_direct_map() {
        let direct = serde_json::json!({
            "sentry": { "type": "http", "url": "https://mcp.sentry.dev/mcp" }
        });
        let normalized = normalize_inline_mcp_servers(&direct);
        let servers = normalized
            .get("mcpServers")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(servers.len(), 1);
        assert!(servers.contains_key("sentry"));
    }

    #[test]
    fn normalize_inline_mcp_servers_idempotent_for_wrapped() {
        let wrapped = serde_json::json!({
            "mcpServers": { "sentry": { "type": "http", "url": "https://mcp.sentry.dev/mcp" } }
        });
        assert_eq!(normalize_inline_mcp_servers(&wrapped), wrapped);
    }

    #[test]
    fn mcp_config_path_inline_does_not_suppress_sibling_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("sentry");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"sentry":{"type":"http","url":"https://mcp.sentry.dev/mcp"}}}"#,
        )
        .unwrap();

        let manifest = manifest_with_inline_mcp(serde_json::json!({
            "sentry": { "type": "http", "url": "https://mcp.sentry.dev/mcp" }
        }));

        let resolved = manifest.mcp_config_path(&root);
        assert!(
            resolved.as_ref().is_some_and(|p| p.ends_with(".mcp.json")),
            "inline mcpServers must not hide a sibling .mcp.json"
        );
        assert!(manifest.inline_mcp_servers().is_some());
    }

    #[test]
    fn mcp_config_path_inline_without_file_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("inline-only");
        std::fs::create_dir_all(&root).unwrap();

        let manifest = manifest_with_inline_mcp(serde_json::json!({
            "foo": { "command": "./server" }
        }));
        assert!(manifest.mcp_config_path(&root).is_none());
    }
}

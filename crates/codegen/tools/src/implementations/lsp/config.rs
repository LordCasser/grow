//! LSP server configuration from `.grow/lsp.json`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const DEFAULT_STARTUP_TIMEOUT_MS: u64 = 15_000;
pub const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 5_000;
pub const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Load LSP servers from user/project config, merge plugin-provided configs, and
/// return the [`ConfigSource`](crate::types::config_source::ConfigSource) of each.
///
/// Plugin configs fill gaps (new server names) but never override user/project config.
/// This is the canonical merge function — both session startup and `grow inspect` call it.
/// Accepts both file-based `.lsp.json` paths and inline `lspServers` JSON values
/// from plugin manifests (`plugin.json`). Execution callers must pass their
/// project trust verdict; inspection can include excluded sources for display.
pub fn load_servers_with_plugins_sourced(
    cwd: &Path,
    include_project: bool,
    plugin_lsp_paths: &[PathBuf],
    plugin_inline_lsp: &[&serde_json::Value],
    plugin_names: &[&str],
    inline_plugin_names: &[&str],
) -> BTreeMap<String, (LspServerConfig, crate::types::config_source::ConfigSource)> {
    use crate::types::config_source::ConfigSource;

    let user_path = crate::util::grow_home::grow_home().join("lsp.json");
    let project_path = cwd.join(".grow").join("lsp.json");

    // User-level servers
    let mut servers: BTreeMap<String, (LspServerConfig, ConfigSource)> = load_file(&user_path)
        .into_iter()
        .map(|(name, cfg)| {
            (
                name,
                (
                    cfg,
                    ConfigSource::User {
                        path: user_path.clone(),
                    },
                ),
            )
        })
        .collect();

    // Excluded project sources must not shadow permitted user/plugin
    // configs: filtering the merged map cannot recover those fallbacks.
    if include_project {
        for (name, cfg) in load_file(&project_path) {
            servers.insert(
                name,
                (
                    cfg,
                    ConfigSource::Project {
                        path: project_path.clone(),
                    },
                ),
            );
        }
    }

    for (name, sourced) in load_plugin_servers_sourced(
        plugin_lsp_paths,
        plugin_inline_lsp,
        plugin_names,
        inline_plugin_names,
    ) {
        servers.entry(name).or_insert(sourced);
    }

    servers
}

/// Parse plugin-only LSP sources with the same file-before-inline precedence
/// used by execution. Diagnostics can inspect inactive plugins independently
/// without letting them shadow the permitted configuration.
pub fn load_plugin_servers_sourced(
    plugin_lsp_paths: &[PathBuf],
    plugin_inline_lsp: &[&serde_json::Value],
    plugin_names: &[&str],
    inline_plugin_names: &[&str],
) -> BTreeMap<String, (LspServerConfig, crate::types::config_source::ConfigSource)> {
    use crate::types::config_source::ConfigSource;
    debug_assert!(
        plugin_names.is_empty() || plugin_names.len() == plugin_lsp_paths.len(),
        "plugin_names must be empty or parallel to plugin_lsp_paths"
    );
    let mut servers = BTreeMap::new();
    // Plugin file-based configs
    for (i, lsp_path) in plugin_lsp_paths.iter().enumerate() {
        let pname = plugin_names.get(i).copied().unwrap_or("unknown");
        for (name, cfg) in load_file(lsp_path) {
            servers.entry(name).or_insert_with(|| {
                (
                    cfg,
                    ConfigSource::Plugin {
                        plugin_name: pname.to_string(),
                        path: lsp_path.clone(),
                    },
                )
            });
        }
    }

    // Plugin inline configs
    for (i, inline) in plugin_inline_lsp.iter().enumerate() {
        let pname = inline_plugin_names.get(i).copied().unwrap_or("unknown");
        match serde_json::from_value::<BTreeMap<String, LspServerConfig>>((*inline).clone()) {
            Ok(parsed) => {
                for (name, cfg) in parsed {
                    servers.entry(name).or_insert_with(|| {
                        (
                            cfg,
                            ConfigSource::Plugin {
                                plugin_name: pname.to_string(),
                                path: PathBuf::new(),
                            },
                        )
                    });
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to parse inline lspServers from plugin manifest");
            }
        }
    }

    servers
}

/// Drop repo-local (project-scoped) LSP servers from a sourced map when the
/// workspace is untrusted; keep user/plugin. Warns per drop. The trust verdict is
/// passed in (the folder-trust engine lives in the shell, out of this crate).
///
/// Single source of truth for the folder-trust LSP load gate, shared by the
/// workspace build path and the shell's per-session gate.
pub fn filter_project_lsp_when_untrusted(
    sourced: BTreeMap<String, (LspServerConfig, crate::types::config_source::ConfigSource)>,
    project_trusted: bool,
) -> BTreeMap<String, LspServerConfig> {
    use crate::types::config_source::ConfigSource;
    sourced
        .into_iter()
        .filter_map(|(name, (cfg, source))| {
            if !project_trusted && matches!(source, ConfigSource::Project { .. }) {
                tracing::warn!(
                    server = %name,
                    "folder untrusted: skipping repo-local (project-scoped) LSP server"
                );
                None
            } else {
                Some((name, cfg))
            }
        })
        .collect()
}

/// Load LSP server configs from `~/.grow/lsp.json` and `<cwd>/.grow/lsp.json`.
/// Project config overrides user config for the same server name.
pub fn load_servers(cwd: &Path) -> BTreeMap<String, LspServerConfig> {
    let user_path = crate::util::grow_home::grow_home().join("lsp.json");
    let project_path = cwd.join(".grow").join("lsp.json");

    let mut merged = load_file(&user_path);
    let project = load_file(&project_path);

    if !merged.is_empty() {
        tracing::info!(
            source = "user",
            path = %user_path.display(),
            servers = ?merged.keys().collect::<Vec<_>>(),
            "loaded user lsp.json"
        );
    }
    if !project.is_empty() {
        tracing::info!(
            source = "project",
            path = %project_path.display(),
            servers = ?project.keys().collect::<Vec<_>>(),
            "loaded project lsp.json"
        );
    }

    for (key, val) in project {
        merged.insert(key, val);
    }

    let mut ext_owners: HashMap<&str, &str> = HashMap::new();
    for (server_name, server_cfg) in &merged {
        for ext in server_cfg.extensions.keys() {
            if let Some(prev) = ext_owners.insert(ext.as_str(), server_name.as_str()) {
                tracing::warn!(
                    extension = ext,
                    server_a = prev,
                    server_b = server_name,
                    "extension claimed by multiple LSP servers; \
                     '{prev}' will handle it (first alphabetically)"
                );
            }
        }
    }

    if merged.is_empty() {
        tracing::info!(
            user = %user_path.display(),
            project = %project_path.display(),
            "no LSP servers configured"
        );
    }
    merged
}

/// Load LSP server configs from a JSON file. Returns empty map on missing/invalid file.
pub fn load_file(path: &Path) -> BTreeMap<String, LspServerConfig> {
    let s = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return BTreeMap::new(),
        Err(e) => {
            tracing::warn!(error = %e,"failed to read lsp.json");
            return BTreeMap::new();
        }
    };

    serde_json::from_str(&s).unwrap_or_else(|e| {
        tracing::warn!(?e, "failed to parse lsp.json");
        BTreeMap::new()
    })
}

/// Resolve which LSP server handles a file based on extension.
pub fn resolve_server(
    servers: &BTreeMap<String, LspServerConfig>,
    path: &Path,
) -> Option<(String, String)> {
    let ext = path.extension()?.to_str()?;
    let dot_ext = format!(".{ext}");
    for (server_name, server_cfg) in servers {
        if let Some(lang_id) = server_cfg.extensions.get(&dot_ext) {
            return Some((server_name.clone(), lang_id.clone()));
        }
    }
    None
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LspTransport {
    #[default]
    Stdio,
    Socket,
}

/// Which solution or projects the server should load once it is running.
///
/// Some servers do not derive their workspace from `rootUri`/`workspaceFolders`
/// and instead load it through a protocol extension. Roslyn is the notable one:
/// left alone it treats every file as a loose "miscellaneous file" and reports
/// no project-level diagnostics at all, until it is sent `solution/open` or
/// `project/open`. Wrappers such as `roslyn-language-server` do this for you; a
/// bare `Microsoft.CodeAnalysis.LanguageServer` does not.
///
/// Paths may be absolute or relative to the workspace root.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceOpen {
    /// A single solution file, sent as `solution/open`.
    #[serde(default)]
    pub solution: Option<String>,
    /// Project files, sent as `project/open`.
    #[serde(default)]
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LspServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub transport: LspTransport,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub extensions: HashMap<String, String>,
    #[serde(default)]
    pub initialization_options: Option<serde_json::Value>,
    #[serde(default)]
    pub settings: Option<serde_json::Value>,
    #[serde(default)]
    pub workspace_folder: Option<String>,
    #[serde(default)]
    pub workspace_open: Option<WorkspaceOpen>,
    #[serde(default)]
    pub startup_timeout: Option<u64>,
    #[serde(default)]
    pub shutdown_timeout: Option<u64>,
    #[serde(default)]
    pub restart_on_crash: Option<bool>,
    #[serde(default)]
    pub max_restarts: Option<u32>,
}

impl LspServerConfig {
    pub fn startup_timeout_ms(&self) -> u64 {
        self.startup_timeout.unwrap_or(DEFAULT_STARTUP_TIMEOUT_MS)
    }

    pub fn shutdown_timeout_ms(&self) -> u64 {
        self.shutdown_timeout.unwrap_or(DEFAULT_SHUTDOWN_TIMEOUT_MS)
    }

    pub fn restart_on_crash(&self) -> bool {
        self.restart_on_crash.unwrap_or(false)
    }

    /// Maximum restart attempts across the lifetime of a server monitor.
    /// This is a lifetime restart budget, not a per-crash-episode counter.
    pub fn max_restarts(&self) -> u32 {
        self.max_restarts.unwrap_or(3)
    }

    /// The directory this server should treat as its workspace: the per-server
    /// override if there is one, otherwise the session cwd. Everything that
    /// needs to name the server's root — `rootUri`, `workspaceFolders`,
    /// `workspace_open` — resolves it here so they cannot drift apart.
    pub fn effective_root<'a>(
        &'a self,
        workspace_root: &'a std::path::Path,
    ) -> &'a std::path::Path {
        self.workspace_folder
            .as_deref()
            .map(std::path::Path::new)
            .unwrap_or(workspace_root)
    }
}

#[cfg(test)]
mod tests {
    use super::{LspServerConfig, filter_project_lsp_when_untrusted};
    use crate::types::config_source::ConfigSource;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn untrusted_project_does_not_shadow_plugin_servers() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        std::fs::create_dir(cwd.join(".grow")).unwrap();
        let prefix = uuid::Uuid::new_v4().to_string();
        let file_name = format!("{prefix}-file");
        let inline_name = format!("{prefix}-inline");
        let independent = format!("{prefix}-independent");
        let project_only = format!("{prefix}-project");
        let project = serde_json::json!({
            (file_name.clone()): {"command": "project-file"},
            (inline_name.clone()): {"command": "project-inline"},
            (project_only.clone()): {"command": "project-only"}
        });
        std::fs::write(cwd.join(".grow/lsp.json"), project.to_string()).unwrap();
        let plugin_path = cwd.join("plugin-lsp.json");
        std::fs::write(
            &plugin_path,
            serde_json::json!({
                (file_name.clone()): {"command": "plugin-file"},
                (independent.clone()): {"command": "plugin-independent"}
            })
            .to_string(),
        )
        .unwrap();
        let inline = serde_json::json!({ (inline_name.clone()): {"command": "plugin-inline"} });
        for trusted in [false, true] {
            let sourced = super::load_servers_with_plugins_sourced(
                cwd,
                trusted,
                std::slice::from_ref(&plugin_path),
                &[&inline],
                &["file-plugin"],
                &["inline-plugin"],
            );
            let servers = filter_project_lsp_when_untrusted(sourced, trusted);
            for (name, plugin_command, project_command) in [
                (&file_name, "plugin-file", "project-file"),
                (&inline_name, "plugin-inline", "project-inline"),
            ] {
                assert_eq!(
                    servers.get(name).map(|cfg| cfg.command.as_str()),
                    Some(if trusted {
                        project_command
                    } else {
                        plugin_command
                    }),
                    "untrusted project cannot remove an allowed fallback"
                );
            }
            assert_eq!(
                servers.get(&independent).unwrap().command,
                "plugin-independent"
            );
            assert_eq!(servers.contains_key(&project_only), trusted);
        }
    }

    #[test]
    fn plugin_only_loading_preserves_file_precedence_and_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.json");
        let second = dir.path().join("second.json");
        std::fs::write(&first, r#"{"shared":{"command":"first-file"}}"#).unwrap();
        std::fs::write(&second, r#"{"shared":{"command":"second-file"}}"#).unwrap();
        let inline = serde_json::json!({"shared":{"command":"inline"}, "inline-only":{"command":"inline-only"}});
        let result = super::load_plugin_servers_sourced(
            &[first.clone(), second],
            &[&inline],
            &["first", "second"],
            &["inline"],
        );
        assert_eq!(result["shared"].0.command, "first-file");
        assert!(
            matches!(&result["shared"].1, ConfigSource::Plugin {plugin_name, path} if plugin_name == "first" && path == &first)
        );
        assert_eq!(result["inline-only"].0.command, "inline-only");
        assert!(
            matches!(&result["inline-only"].1, ConfigSource::Plugin {plugin_name, path} if plugin_name == "inline" && path.as_os_str().is_empty())
        );
    }

    fn sourced() -> BTreeMap<String, (LspServerConfig, ConfigSource)> {
        let mut m = BTreeMap::new();
        m.insert(
            "proj".to_string(),
            (
                LspServerConfig::default(),
                ConfigSource::Project {
                    path: PathBuf::from("/repo/.grow/lsp.json"),
                },
            ),
        );
        m.insert(
            "usr".to_string(),
            (
                LspServerConfig::default(),
                ConfigSource::User {
                    path: PathBuf::from("/home/.grow/lsp.json"),
                },
            ),
        );
        m.insert(
            "plug".to_string(),
            (
                LspServerConfig::default(),
                ConfigSource::Plugin {
                    plugin_name: "p".to_string(),
                    path: PathBuf::from("/plug/lsp.json"),
                },
            ),
        );
        m
    }

    #[test]
    fn untrusted_drops_only_project_keeps_user_and_plugin() {
        let kept = filter_project_lsp_when_untrusted(sourced(), false);
        assert_eq!(kept.len(), 2);
        assert!(!kept.contains_key("proj"));
        assert!(kept.contains_key("usr"));
        assert!(kept.contains_key("plug"));
    }

    #[test]
    fn trusted_keeps_all_including_project() {
        let kept = filter_project_lsp_when_untrusted(sourced(), true);
        assert_eq!(kept.len(), 3);
        assert!(kept.contains_key("proj"));
        assert!(kept.contains_key("usr"));
        assert!(kept.contains_key("plug"));
    }

    #[test]
    fn server_config_requires_canonical_snake_case_fields() {
        let canonical: LspServerConfig = serde_json::from_value(serde_json::json!({
            "command": "rust-analyzer",
            "initialization_options": {"cargo": {"allFeatures": true}},
            "workspace_folder": "/repo",
            "startup_timeout": 1000,
            "restart_on_crash": true,
            "max_restarts": 2
        }))
        .unwrap();
        assert_eq!(canonical.command, "rust-analyzer");
        assert_eq!(canonical.workspace_folder.as_deref(), Some("/repo"));

        for obsolete in [
            "extensionToLanguage",
            "extensionToLanguageId",
            "initializationOptions",
            "workspaceFolder",
            "workspaceOpen",
            "startupTimeout",
            "shutdownTimeout",
            "restartOnCrash",
            "maxRestarts",
        ] {
            let mut value = serde_json::json!({"command": "server"});
            value[obsolete] = serde_json::json!({});
            assert!(
                serde_json::from_value::<LspServerConfig>(value).is_err(),
                "obsolete field {obsolete} must be rejected"
            );
        }
    }
}

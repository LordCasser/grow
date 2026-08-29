use agent::prompt::skills::SkillsConfig;
use agent_client_protocol as acp;
use anyhow::Result;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::PathBuf;
use toml::Value as TomlValue;
use toml::map::Map as TomlMap;

// MCP server config value types extracted to `config-types` (config
// dependency inversion); re-exported so `crate::util::config::*` paths keep working.
pub use config_types::{
    KNOWN_MCP_SERVER_FIELDS, McpPreferenceSource, McpPreferencesFile, McpServerConfig,
    McpServerConfigProblem, McpServerPreferences, McpServerProblemSeverity,
    McpServerTransportConfig, McpSetupConfig, McpSetupDerivedValue, McpSetupField,
    McpSetupFieldType, McpSetupOption, McpSetupResolution,
};
// Permission-policy value types likewise extracted; re-exported to keep paths stable.
pub use config_types::{PatternMode, PermissionConfig, PermissionRule, RuleAction, ToolFilter};
// MCP-config value types shared by runtime resolution and persistence.
pub use config_types::McpConfig;
// Worktree-pool config value type extracted; re-exported to keep paths stable.
pub use config_types::PoolConfig;

/// TUI/CLI settings. Composed from typed section configs defined in `agent::config`.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub cli: crate::agent::config::CliConfig,
    pub models: crate::agent::config::ModelsConfig,
    pub ui: crate::agent::config::UiConfig,
    pub skills: SkillsConfig,
    /// Management API key from `[endpoints]`.
    pub management_api_key: Option<String>,
    /// Permission policy rules loaded from `[permission]` section in config.toml.
    pub permission: Option<PermissionConfig>,
    pub diagnostics: crate::agent::config::DiagnosticsConfig,
    /// `[session]` section — round-tripped through `merge_section` so
    /// pager setters can persist session fields (e.g. auto-compact threshold).
    pub session: crate::agent::config::SessionConfig,
    /// `[toolset.ask_user_question]` sub-table — the only `[toolset]` piece
    /// the settings modal writes; the rest of `[toolset]` never round-trips
    /// (it carries runtime-only structs whose defaults must not hit disk).
    pub ask_user_question: crate::tools::config::AskUserQuestionToolConfig,
}

pub fn get_mcp_server_config(name: &str) -> Option<McpServerConfig> {
    let root: TomlValue = crate::config::load_effective_config().ok()?;
    let configs = parse_mcp_servers_from_toml(&root);
    configs.get(name).cloned()
}

/// Get MCP server config by name, checking project-scoped configs first.
/// Walks from cwd up to the git repo root checking `.grow/config.toml` at each level.
/// Project-scoped `.grow/config.toml` entries override global `~/.grow/config.toml`
/// entries entirely (no deep merge of individual fields).
/// Closer directories (cwd) take priority over further ones (repo root).
pub fn get_mcp_server_config_with_project(
    name: &str,
    cwd: &std::path::Path,
) -> Option<McpServerConfig> {
    // Check project-scoped configs from cwd (highest priority) to repo root
    let project_configs = crate::config::find_project_configs(cwd);
    for config_path in project_configs.iter().rev() {
        if let Ok(root) = crate::config::load_config_file(config_path) {
            let configs = parse_mcp_servers_from_toml(&root);
            if let Some(config) = configs.get(name) {
                return Some(config.clone());
            }
        }
    }

    // Fall back to global config
    get_mcp_server_config(name)
}

/// Scope tags for an MCP server definition. Single source of truth shared by
/// the scope producers ([`mcp_server_scope`],
/// [`load_mcp_server_configs_with_project`]) and the folder-trust gate that
/// filters project-scoped names, so a retag can't silently desync the gate.
/// `MCP_SCOPE_PROJECT` is `pub(crate)` for the gate consumer in `folder_trust`;
/// `MCP_SCOPE_USER` stays private (only used here).
pub(crate) const MCP_SCOPE_PROJECT: &str = "project";
const MCP_SCOPE_USER: &str = "user";

/// Scope an MCP server resolves at: project when defined in any project-scoped
/// `.grow/config.toml`, otherwise user (global config). See
/// [`MCP_SCOPE_PROJECT`] / [`MCP_SCOPE_USER`].
pub(crate) fn mcp_server_scope(name: &str, cwd: &std::path::Path) -> &'static str {
    for config_path in crate::config::find_project_configs(cwd) {
        if let Ok(root) = crate::config::load_config_file(&config_path)
            && parse_mcp_servers_from_toml(&root).contains_key(name)
        {
            return MCP_SCOPE_PROJECT;
        }
    }
    MCP_SCOPE_USER
}

/// Load the worktree pool configuration from config.toml.
/// Returns the default config if the section is missing.
pub fn worktree_pool_from_toml(root: &TomlValue) -> PoolConfig {
    if let TomlValue::Table(table) = root
        && let Some(pool_val) = table.get("worktree_pool")
    {
        // Try to deserialize the section; fall back to defaults on error
        pool_val
            .clone()
            .try_into::<PoolConfig>()
            .unwrap_or_default()
    } else {
        PoolConfig::default()
    }
}

/// Load MCP servers with project-scoped overrides from `.grow/config.toml`.
///
/// Merge strategy:
/// 1. Load MCP servers from global `~/.grow/config.toml`
/// 2. Walk from git repo root down to `cwd`, loading `.grow/config.toml` at each level
///    (matching the convention used by skills and AGENTS.md discovery)
/// 3. Each level's entries replace entries with the same name entirely
///    (no deep merge — omitted fields fall back to defaults)
/// 4. Closer directories (cwd) take priority over further ones (repo root)
pub fn load_mcp_servers(cwd: &std::path::Path) -> Vec<acp::McpServer> {
    let global_config = crate::config::load_effective_config()
        .unwrap_or_else(|_| TomlValue::Table(toml::map::Map::new()));
    reload_mcp_servers_merged(&global_config, cwd)
}

/// Merge MCP servers from a pre-parsed global config with project-scoped overrides.
///
/// Same merge strategy as [`load_mcp_servers_with_project`] but takes the global
/// config as a pre-parsed `toml::Value` instead of re-reading from disk. Project
/// configs are still read from disk because the watcher signals paths, not content.
pub(crate) fn reload_mcp_servers_merged(
    global_config: &TomlValue,
    cwd: &std::path::Path,
) -> Vec<acp::McpServer> {
    let mut servers: IndexMap<String, McpServerConfig> = IndexMap::new();

    for (name, config) in parse_mcp_servers_from_toml(global_config) {
        servers.insert(name, config);
    }

    let project_configs = crate::config::find_project_configs(cwd);
    for config_path in &project_configs {
        if let Ok(root) = crate::config::load_config_file(config_path) {
            let project_servers = parse_mcp_servers_from_toml(&root);
            if !project_servers.is_empty() {
                tracing::info!(
                    count = project_servers.len(),
                    path = %config_path.display(),
                    "Loaded project-scoped MCP servers from .grow/config.toml"
                );
                for (name, config) in project_servers {
                    servers.insert(name, config);
                }
            }
        }
    }
    let preferences = load_mcp_preferences().file();
    let sub = &crate::config::expand_env_vars_in_string;
    servers
        .into_iter()
        .filter_map(|(name, config)| {
            let mut config = match config.resolve_setup(preferences.servers.get(&name)) {
                McpSetupResolution::Resolved(config) => config,
                McpSetupResolution::Required(_) => return None,
                McpSetupResolution::Invalid(reason) => {
                    tracing::warn!(server = %name, error = %reason, "MCP setup config is invalid");
                    return None;
                }
            };
            config.expand_strings(sub);
            config.to_acp_mcp_server(name)
        })
        .collect()
}

/// All server names from config.toml (including `enabled = false`).
pub fn all_toml_mcp_server_names(cwd: &std::path::Path) -> std::collections::HashSet<String> {
    load_all_mcp_configs(cwd).keys().cloned().collect()
}

pub fn mcp_preferences_path() -> PathBuf {
    config::grow_home().join("mcp_preferences.json")
}

/// Result of loading prefs. Corrupt files are readable as empty for resolution
/// but must not be overwritten (would clobber other servers).
#[derive(Debug, Clone)]
pub enum McpPreferencesLoad {
    Ok(McpPreferencesFile),
    Missing,
    Corrupt,
}

impl McpPreferencesLoad {
    pub fn file(&self) -> McpPreferencesFile {
        match self {
            Self::Ok(f) => f.clone(),
            Self::Missing | Self::Corrupt => McpPreferencesFile::default(),
        }
    }

    pub fn is_writable(&self) -> bool {
        !matches!(self, Self::Corrupt)
    }
}

pub fn load_mcp_preferences() -> McpPreferencesLoad {
    load_mcp_preferences_from(&mcp_preferences_path())
}

pub fn load_mcp_preferences_from(path: &std::path::Path) -> McpPreferencesLoad {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return McpPreferencesLoad::Missing,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to read MCP preferences");
            return McpPreferencesLoad::Corrupt;
        }
    };
    match serde_json::from_str(&content) {
        Ok(file) => McpPreferencesLoad::Ok(file),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to parse MCP preferences");
            McpPreferencesLoad::Corrupt
        }
    }
}

pub async fn save_mcp_preferences(prefs: &McpPreferencesFile) -> Result<()> {
    save_mcp_preferences_to(&mcp_preferences_path(), prefs).await
}

pub async fn save_mcp_preferences_to(
    path: &std::path::Path,
    prefs: &McpPreferencesFile,
) -> Result<()> {
    if matches!(load_mcp_preferences_from(path), McpPreferencesLoad::Corrupt) {
        anyhow::bail!(
            "refusing to overwrite unreadable MCP preferences at {}",
            path.display()
        );
    }
    let json = serde_json::to_string_pretty(prefs)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension(format!(
        "json.tmp.{}{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    tokio::fs::write(&tmp, &json).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|e| anyhow::anyhow!("failed to set mcp preferences permissions: {e}"))?;
    }
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

/// Restore a single server key after a failed setup (best-effort).
pub async fn restore_mcp_preference_server(
    server_name: &str,
    previous: Option<McpServerPreferences>,
) -> Result<()> {
    let load = load_mcp_preferences();
    if !load.is_writable() {
        return Ok(());
    }
    let mut prefs = load.file();
    match previous {
        Some(entry) => {
            prefs.servers.insert(server_name.to_string(), entry);
        }
        None => {
            prefs.servers.remove(server_name);
        }
    }
    save_mcp_preferences(&prefs).await
}

/// Unresolved setup-bearing MCP config collected for `/mcps` list and auth.
#[derive(Debug, Clone)]
pub struct McpSetupServerEntry {
    pub name: String,
    pub config: McpServerConfig,
    pub source: McpPreferenceSource,
}

/// Collect MCP configs that declare a `setup` schema from config and plugins.
/// Used to surface setup-required rows and drive `grow/mcp/setup`.
pub fn collect_mcp_setup_configs(
    cwd: &std::path::Path,
    plugin_registry: Option<&agent::plugins::PluginRegistry>,
) -> IndexMap<String, McpSetupServerEntry> {
    let mut result = IndexMap::new();
    for (name, (config, scope)) in load_mcp_server_configs_with_project(cwd) {
        if !config.enabled || config.setup.is_none() {
            continue;
        }
        result.insert(
            name.clone(),
            McpSetupServerEntry {
                name,
                config,
                source: McpPreferenceSource {
                    kind: "config".to_string(),
                    plugin: None,
                    scope: Some(scope.to_string()),
                },
            },
        );
    }
    if let Some(registry) = plugin_registry {
        let toml_claimed_names = all_toml_mcp_server_names(cwd);
        for plugin in registry.active_plugins() {
            // File first, then inline; first-wins matches runtime plugin load.
            let mut plugin_configs = IndexMap::new();
            if let Some(ref mcp_path) = plugin.mcp_config_path
                && let Some(config) = read_mcp_json(mcp_path)
            {
                for (name, server) in config.mcp_servers {
                    plugin_configs.entry(name).or_insert(server);
                }
            }
            if let Some(ref inline_value) = plugin.inline_mcp_servers {
                let normalized =
                    agent::plugins::manifest::normalize_inline_mcp_servers(inline_value);
                for (name, server) in mcp_config_from_json_value(&normalized).mcp_servers {
                    plugin_configs.entry(name).or_insert(server);
                }
            }
            for (name, config) in plugin_configs {
                if toml_claimed_names.contains(&name) || !config.enabled || config.setup.is_none() {
                    continue;
                }
                result.entry(name.clone()).or_insert(McpSetupServerEntry {
                    name,
                    config,
                    source: McpPreferenceSource {
                        kind: "plugin".to_string(),
                        plugin: Some(plugin.name.clone()),
                        scope: None,
                    },
                });
            }
        }
    }
    result
}

/// Persist `disabled_tools` for a server under `[disabled_mcp_tools]` in config.toml.
///
/// Uses a dedicated top-level section (not `[mcp_servers]`) to avoid creating
/// incomplete server entries that fail to deserialize for configured servers.
pub async fn save_mcp_disabled_tools(server_name: &str, disabled_tools: &[String]) -> Result<()> {
    let path = config_path();
    let mut root: TomlValue = match tokio::fs::read_to_string(&path).await {
        Ok(s) => toml::from_str(&s).unwrap_or(TomlValue::Table(TomlMap::new())),
        Err(_) => TomlValue::Table(TomlMap::new()),
    };
    let table = root
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("config root is not a table"))?;

    let section = table
        .entry("disabled_mcp_tools")
        .or_insert_with(|| TomlValue::Table(TomlMap::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("disabled_mcp_tools is not a table"))?;

    if disabled_tools.is_empty() {
        section.remove(server_name);
        if section.is_empty() {
            table.remove("disabled_mcp_tools");
        }
    } else {
        let arr = disabled_tools
            .iter()
            .map(|s| TomlValue::String(s.clone()))
            .collect();
        section.insert(server_name.to_string(), TomlValue::Array(arr));
    }

    let toml_str = toml::to_string_pretty(&root)?;
    let tmp = path.with_extension("toml.tmp");
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tokio::fs::write(&tmp, &toml_str).await?;
    tokio::fs::rename(&tmp, &path).await?;
    Ok(())
}

/// Persist enable/disable for one MCP server in user config.
///
/// Always updates user `disabled_mcp_servers` (and user
/// `[mcp_servers.<name>].enabled` when present). On **enable** only, also clears
/// sticky `enabled = false` on the **nearest** project definition that defines
/// the server (cwd-nearest wins; shadowed ancestors are left alone) — never
/// writes project configs on disable.
///
/// Uses process cwd for project unstick; session callers should prefer
/// [`save_mcp_server_enabled_in`] with the session cwd.
pub async fn save_mcp_server_enabled(server_name: &str, enabled: bool) -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    save_mcp_server_enabled_in(server_name, enabled, &cwd)
        .await
        .map(|_| ())
}

/// Like [`save_mcp_server_enabled`], with explicit cwd for project config walks.
pub async fn save_mcp_server_enabled_in(
    server_name: &str,
    enabled: bool,
    cwd: &std::path::Path,
) -> Result<Vec<PathBuf>> {
    let mut modified = Vec::new();

    let user_path = config_path();
    if write_toml_table_if_changed(&user_path, |table| {
        apply_mcp_server_enabled(table, server_name, enabled);
    })
    .await?
    {
        modified.push(user_path);
    }

    // Enable-only: unstick the nearest (winning) project def with
    // `enabled = false` via toml_edit (preserves comments/layout). Disable is
    // personal (user `disabled_mcp_servers`) and must not dirty shared files.
    if enabled
        && let Some(path) = nearest_project_mcp_definition(cwd, server_name)
        && clear_sticky_project_disabled_at(&path, server_name).await?
    {
        modified.push(path);
    }

    Ok(modified)
}

/// User config only — no project unstick.
///
/// Use after delete (or similar) when the toggle path dirtied
/// `disabled_mcp_servers` but shared project configs must stay untouched.
pub async fn save_user_mcp_server_enabled(server_name: &str, enabled: bool) -> Result<()> {
    write_toml_table_if_changed(&config_path(), |table| {
        apply_mcp_server_enabled(table, server_name, enabled);
    })
    .await
    .map(|_| ())
}

/// Nearest project config defining `server_name` (cwd last → reverse; nearest wins).
pub(crate) fn nearest_project_mcp_definition(
    cwd: &std::path::Path,
    server_name: &str,
) -> Option<PathBuf> {
    crate::config::find_project_configs(cwd)
        .into_iter()
        .rev()
        .find(|path| mcp_server_defined_at(path, server_name))
}

/// Apply `f`, write only if the serialized table changed. Returns whether written.
///
/// Aligns with [`super::persist::save_config`] safety: refuse unparseable
/// files (no wipe-to-empty), unique tmp + mode preserve via
/// [`super::persist::atomic_write_string`], and the user-config write lock.
async fn write_toml_table_if_changed(
    path: &std::path::Path,
    f: impl FnOnce(&mut TomlMap<String, TomlValue>),
) -> Result<bool> {
    let is_user = path == config_path().as_path();
    let _guard = if is_user {
        Some(super::persist::lock_config_writes().await)
    } else {
        None
    };

    let original = match tokio::fs::read_to_string(path).await {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && is_user => String::new(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(anyhow::anyhow!("failed to read {}: {e}", path.display()));
        }
    };
    let mut root: TomlValue = if original.is_empty() {
        TomlValue::Table(TomlMap::new())
    } else {
        match toml::from_str(&original) {
            Ok(v) => v,
            Err(parse_err) => {
                return Err(anyhow::anyhow!(
                    "refusing to overwrite unparseable {}: {}; fix the syntax before retrying",
                    path.display(),
                    parse_err
                ));
            }
        }
    };
    let table = root
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("config root is not a table"))?;
    f(table);
    let toml_str = toml::to_string_pretty(&root)?;
    // Normalize empty original so first enable/disable still writes when needed.
    let before = if original.is_empty() {
        toml::to_string_pretty(&TomlValue::Table(TomlMap::new()))?
    } else {
        // Re-serialize original for stable comparison (ignore formatting noise).
        match toml::from_str::<TomlValue>(&original) {
            Ok(v) => toml::to_string_pretty(&v).unwrap_or(original),
            Err(_) => original,
        }
    };
    if before == toml_str {
        return Ok(false);
    }
    super::persist::atomic_write_string(path, &toml_str)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    Ok(true)
}

/// Flip sticky project `enabled = false` → true with toml_edit (comments kept).
async fn clear_sticky_project_disabled_at(
    path: &std::path::Path,
    server_name: &str,
) -> Result<bool> {
    let original = match tokio::fs::read_to_string(path).await {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(anyhow::anyhow!("failed to read {}: {e}", path.display()));
        }
    };
    let mut doc: toml_edit::DocumentMut = original
        .parse()
        .map_err(|e| anyhow::anyhow!("refusing to rewrite unparseable {}: {e}", path.display()))?;

    let Some(servers) = doc
        .get_mut("mcp_servers")
        .and_then(|item| item.as_table_like_mut())
    else {
        return Ok(false);
    };
    let Some(entry) = servers.get_mut(server_name) else {
        return Ok(false);
    };
    let Some(server_table) = entry.as_table_like_mut() else {
        return Ok(false);
    };
    if server_table.get("enabled").and_then(|v| v.as_bool()) != Some(false) {
        return Ok(false);
    }
    server_table.insert("enabled", toml_edit::value(true));

    let updated = doc.to_string();
    if updated == original {
        return Ok(false);
    }
    super::persist::atomic_write_string(path, &updated)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    Ok(true)
}

/// Update user `disabled_mcp_servers` and, if present, per-server `enabled`.
fn apply_mcp_server_enabled(
    table: &mut TomlMap<String, TomlValue>,
    server_name: &str,
    enabled: bool,
) {
    let mut disabled_list: Vec<String> = table
        .get("disabled_mcp_servers")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if enabled {
        disabled_list.retain(|n| n != server_name);
    } else if !disabled_list.contains(&server_name.to_string()) {
        disabled_list.push(server_name.to_string());
    }

    if disabled_list.is_empty() {
        table.remove("disabled_mcp_servers");
    } else {
        let arr = disabled_list
            .iter()
            .map(|s| TomlValue::String(s.clone()))
            .collect();
        table.insert("disabled_mcp_servers".to_string(), TomlValue::Array(arr));
    }

    set_mcp_server_enabled_field(table, server_name, enabled);
}

fn set_mcp_server_enabled_field(
    table: &mut TomlMap<String, TomlValue>,
    server_name: &str,
    enabled: bool,
) {
    if let Some(servers) = table.get_mut("mcp_servers").and_then(|v| v.as_table_mut())
        && let Some(entry) = servers.get_mut(server_name)
        && let Some(server_table) = entry.as_table_mut()
    {
        server_table.insert("enabled".to_string(), TomlValue::Boolean(enabled));
    }
}

/// Upsert an MCP server entry in `~/.grow/config.toml`.
///
/// Creates or replaces `[mcp_servers.<name>]` with the given config.
/// Also removes the server from `disabled_mcp_servers` if present (a newly
/// defined server should start enabled).
pub async fn save_mcp_server_config(server_name: &str, config: &McpServerConfig) -> Result<()> {
    save_mcp_server_config_at(&config_path(), server_name, config).await
}

/// Upsert an MCP server entry in the config file at `path`.
///
/// Same semantics as [`save_mcp_server_config`] but targets an explicit
/// config file, e.g. a project-scoped `.grow/config.toml`.
pub async fn save_mcp_server_config_at(
    path: &std::path::Path,
    server_name: &str,
    config: &McpServerConfig,
) -> Result<()> {
    let mut root: TomlValue = match tokio::fs::read_to_string(&path).await {
        Ok(s) => toml::from_str(&s).unwrap_or(TomlValue::Table(TomlMap::new())),
        Err(_) => TomlValue::Table(TomlMap::new()),
    };
    let table = root
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("config root is not a table"))?;

    let servers = table
        .entry("mcp_servers")
        .or_insert_with(|| TomlValue::Table(TomlMap::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("mcp_servers is not a table"))?;

    let serialized = toml::Value::try_from(config)
        .map_err(|e| anyhow::anyhow!("failed to serialize MCP server config: {e}"))?;
    servers.insert(server_name.to_string(), serialized);

    // Ensure the server isn't in the disabled list.
    if let Some(arr) = table
        .get_mut("disabled_mcp_servers")
        .and_then(|v| v.as_array_mut())
    {
        arr.retain(|v| v.as_str() != Some(server_name));
        if arr.is_empty() {
            table.remove("disabled_mcp_servers");
        }
    }

    let toml_str = toml::to_string_pretty(&root)?;
    let tmp = path.with_extension("toml.tmp");
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tokio::fs::write(&tmp, &toml_str).await?;
    tokio::fs::rename(&tmp, &path).await?;
    Ok(())
}

/// Delete an MCP server entry from `~/.grow/config.toml`.
///
/// Removes `[mcp_servers.<name>]`, cleans up `disabled_mcp_servers` and
/// `[disabled_mcp_tools.<name>]` entries. Returns `true` if the entry existed.
pub async fn delete_mcp_server_config(server_name: &str) -> Result<bool> {
    delete_mcp_server_config_at(&config_path(), server_name).await
}

/// Delete an MCP server entry from the config file at `path`.
///
/// Same semantics as [`delete_mcp_server_config`] but targets an explicit
/// config file, e.g. a project-scoped `.grow/config.toml`.
pub async fn delete_mcp_server_config_at(
    path: &std::path::Path,
    server_name: &str,
) -> Result<bool> {
    let mut root: TomlValue = match tokio::fs::read_to_string(&path).await {
        Ok(s) => toml::from_str(&s).unwrap_or(TomlValue::Table(TomlMap::new())),
        Err(_) => return Ok(false),
    };
    let table = root
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("config root is not a table"))?;

    let existed = table
        .get_mut("mcp_servers")
        .and_then(|v| v.as_table_mut())
        .and_then(|servers| servers.remove(server_name))
        .is_some();

    if !existed {
        return Ok(false);
    }

    // Clean up empty mcp_servers table.
    if table
        .get("mcp_servers")
        .and_then(|v| v.as_table())
        .is_some_and(|t| t.is_empty())
    {
        table.remove("mcp_servers");
    }

    // Remove from disabled_mcp_servers list.
    if let Some(arr) = table
        .get_mut("disabled_mcp_servers")
        .and_then(|v| v.as_array_mut())
    {
        arr.retain(|v| v.as_str() != Some(server_name));
        if arr.is_empty() {
            table.remove("disabled_mcp_servers");
        }
    }

    // Remove disabled_mcp_tools entry.
    if let Some(section) = table
        .get_mut("disabled_mcp_tools")
        .and_then(|v| v.as_table_mut())
    {
        section.remove(server_name);
        if section.is_empty() {
            table.remove("disabled_mcp_tools");
        }
    }

    let toml_str = toml::to_string_pretty(&root)?;
    let tmp = path.with_extension("toml.tmp");
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tokio::fs::write(&tmp, &toml_str).await?;
    tokio::fs::rename(&tmp, &path).await?;

    Ok(true)
}

/// Load disabled_tools for all MCP servers from `[disabled_mcp_tools]` in config.toml.
pub fn get_all_mcp_disabled_tools(
    _cwd: &std::path::Path,
) -> std::collections::HashMap<String, std::collections::HashSet<String>> {
    let root = match crate::config::load_effective_config() {
        Ok(r) => r,
        Err(_) => return std::collections::HashMap::new(),
    };
    let Some(section) = root.get("disabled_mcp_tools").and_then(|v| v.as_table()) else {
        return std::collections::HashMap::new();
    };
    section
        .iter()
        .filter_map(|(server, val)| {
            let tools: std::collections::HashSet<String> = val
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if tools.is_empty() {
                None
            } else {
                Some((server.clone(), tools))
            }
        })
        .collect()
}

/// Load the declared RWX ceiling for every `[mcp_servers.<name>]` entry.
///
/// Single source of truth for the plan-mode read-only MCP classification:
/// while a Plan is in a non-executing phase, tools from a listed server are
/// allowed through the edit gate; every other MCP tool is rejected. MCP
/// `annotations`/`readOnlyHint` are never consulted. The session caches the
/// trust-filtered result in `McpState` at init instead of re-reading per tool
/// call.
pub fn get_mcp_server_max_access(
    cwd: &std::path::Path,
) -> std::collections::HashMap<String, tool_protocol::ToolAccess> {
    let project_allowed = crate::agent::folder_trust::project_scope_allowed(cwd);
    let servers = load_mcp_server_configs_with_project(cwd);
    mcp_server_max_access_from_scoped_config(&servers, project_allowed)
}

fn mcp_server_max_access_from_scoped_config(
    servers: &IndexMap<String, (McpServerConfig, &'static str)>,
    project_allowed: bool,
) -> std::collections::HashMap<String, tool_protocol::ToolAccess> {
    servers
        .iter()
        .filter(|(_, (_, scope))| *scope != MCP_SCOPE_PROJECT || project_allowed)
        .map(|(name, (config, _))| (name.clone(), config.max_access))
        .collect()
}

/// Load all configured MCP servers as `(name, config)` pairs.
///
/// Reads from `load_effective_config()`, which loads the user config layer
/// only. Use [`load_mcp_server_configs_with_project`] for a view that also
/// includes project-scoped `.grow/config.toml` files.
pub fn load_mcp_server_configs() -> IndexMap<String, McpServerConfig> {
    let root =
        crate::config::load_effective_config().unwrap_or_else(|_| TomlValue::Table(TomlMap::new()));
    parse_mcp_servers_from_toml(&root)
}

/// Deserialize one `[mcp_servers.<name>]` table, also returning any unrecognized keys.
fn deserialize_mcp_server_config(
    value: &TomlValue,
) -> Result<(McpServerConfig, Vec<String>), String> {
    let unknown_fields = value.as_table().map_or_else(Vec::new, |table| {
        table
            .keys()
            .filter(|field| !KNOWN_MCP_SERVER_FIELDS.contains(&field.as_str()))
            .cloned()
            .collect()
    });
    let config = toml::Value::try_into::<McpServerConfig>(value.clone())
        .map_err(|error| error.to_string())?;
    Ok((config, unknown_fields))
}

/// Turn a failed `[mcp_servers.<name>]` entry into an actionable problem. The
/// transport-less case is steered to `disabled_mcp_servers`, Grow's real
/// disable mechanism.
fn diagnose_invalid_entry(name: &str, value: &TomlValue, error: &str) -> McpServerConfigProblem {
    let has_command = value.get("command").is_some();
    let has_url = value.get("url").is_some();
    let message = if !has_command && !has_url {
        format!(
            "`mcp_servers.{name}` has no transport. To run it, set `command = \"...\"` or \
             `url = \"...\"`. To turn it off, add \"{name}\" to `disabled_mcp_servers` instead of \
             leaving an entry with no transport. \
             See ~/.grow/docs/user-guide/07-mcp-servers.md"
        )
    } else {
        format!(
            "`mcp_servers.{name}` has an invalid transport: {error}. \
             See ~/.grow/docs/user-guide/07-mcp-servers.md"
        )
    };
    McpServerConfigProblem {
        server: name.to_string(),
        field: None,
        severity: McpServerProblemSeverity::Error,
        message,
    }
}

pub(crate) struct ParsedMcpServers {
    pub servers: IndexMap<String, McpServerConfig>,
    pub problems: Vec<McpServerConfigProblem>,
}

/// Parse `[mcp_servers.*]` without ever failing the whole config: valid servers
/// load, invalid entries are reported (GBT-4128).
pub(crate) fn parse_mcp_servers_with_problems(root: &TomlValue) -> ParsedMcpServers {
    let mut servers = IndexMap::new();
    let mut problems = Vec::new();

    let entries = match root {
        TomlValue::Table(table) => match table.get("mcp_servers") {
            Some(TomlValue::Table(mcp_servers)) => mcp_servers,
            _ => return ParsedMcpServers { servers, problems },
        },
        _ => return ParsedMcpServers { servers, problems },
    };

    for (name, value) in entries {
        match deserialize_mcp_server_config(value) {
            Ok((config, unknown_fields)) => {
                for field in unknown_fields {
                    problems.push(McpServerConfigProblem {
                        server: name.clone(),
                        field: Some(field.clone()),
                        severity: McpServerProblemSeverity::Warning,
                        message: format!(
                            "`mcp_servers.{name}` has an unrecognized field `{field}`; it is \
                             ignored. See ~/.grow/docs/user-guide/07-mcp-servers.md"
                        ),
                    });
                }
                if config.enabled
                    && let Some(field) = config.blank_transport_field()
                {
                    problems.push(McpServerConfigProblem {
                        server: name.clone(),
                        field: Some(field.to_string()),
                        severity: McpServerProblemSeverity::Error,
                        message: format!(
                            "`mcp_servers.{name}` is enabled but its `{field}` is blank. \
                             Set a value, or add \"{name}\" to `disabled_mcp_servers` to turn it \
                             off. See ~/.grow/docs/user-guide/07-mcp-servers.md"
                        ),
                    });
                    continue;
                }
                servers.insert(name.clone(), config);
            }
            Err(error) => problems.push(diagnose_invalid_entry(name, value, &error)),
        }
    }
    ParsedMcpServers { servers, problems }
}

/// Wrapper that logs problems and returns only the valid servers.
pub(crate) fn parse_mcp_servers_from_toml(root: &TomlValue) -> IndexMap<String, McpServerConfig> {
    let ParsedMcpServers { servers, problems } = parse_mcp_servers_with_problems(root);
    for problem in &problems {
        tracing::warn!(server = %problem.server, "{}", problem.message);
    }
    servers
}

pub fn load_mcp_json_file(path: &std::path::Path) -> Vec<acp::McpServer> {
    if !path.is_file() {
        return vec![];
    }
    let Some(value) = read_mcp_json(path) else {
        return vec![];
    };
    let label = path.display().to_string();
    parse_mcp_config(&value, &label, &crate::config::expand_env_vars_in_string)
}
pub(crate) fn parse_mcp_config(
    config: &McpConfig,
    source_label: &str,
    sub: &dyn Fn(&str) -> String,
) -> Vec<acp::McpServer> {
    let preferences = load_mcp_preferences().file();
    let mut servers = Vec::new();
    for (name, server_config) in &config.mcp_servers {
        let mut server_config = match server_config.resolve_setup(preferences.servers.get(name)) {
            McpSetupResolution::Resolved(config) => config,
            McpSetupResolution::Required(_) => continue,
            McpSetupResolution::Invalid(reason) => {
                tracing::warn!(
                    source = source_label,
                    server = %name,
                    error = %reason,
                    "MCP setup config is invalid"
                );
                continue;
            }
        };
        server_config.expand_strings(sub);
        if let Some(server) = server_config.to_acp_mcp_server(name.clone()) {
            servers.push(server);
        } else {
            tracing::warn!(
                source = source_label,
                server = name,
                "MCP server has no 'command' (stdio) or 'url' (http/sse); skipping"
            );
        }
    }

    if !servers.is_empty() {
        tracing::info!(
            source = source_label,
            count = servers.len(),
            "loaded MCP servers"
        );
    }

    servers
}

/// Build an `McpConfig` from a JSON value, skipping any `mcpServers` entry that
/// fails to deserialize instead of dropping the whole file. Mirrors the
/// per-entry tolerance of [`parse_mcp_servers_with_problems`] for TOML, so one
/// bad entry in a plugin MCP JSON file cannot take out its siblings.
fn mcp_config_from_json_value(value: &serde_json::Value) -> McpConfig {
    let mut mcp_servers = IndexMap::new();
    if let Some(entries) = value.get("mcpServers").and_then(|v| v.as_object()) {
        for (name, entry) in entries {
            match serde_json::from_value::<McpServerConfig>(entry.clone()) {
                Ok(config) => {
                    mcp_servers.insert(name.clone(), config);
                }
                Err(error) => tracing::warn!(
                    server = %name,
                    error = %error,
                    "skipping invalid MCP server entry in JSON config"
                ),
            }
        }
    }
    McpConfig { mcp_servers }
}

/// Read and parse a JSON file. Returns `None` on I/O or top-level parse errors
/// (logged); individual bad `mcpServers` entries are skipped, not fatal.
pub(crate) fn read_mcp_json(path: &std::path::Path) -> Option<McpConfig> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| {
            tracing::warn!(error = %e, "failed to read MCP JSON");
        })
        .ok()?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| {
            tracing::warn!(error = %e, "failed to parse MCP JSON");
        })
        .ok()?;
    Some(mcp_config_from_json_value(&value))
}

/// Like `load_mcp_servers_with_project` but returns raw configs without filtering by `enabled`.
fn load_all_mcp_configs(cwd: &std::path::Path) -> IndexMap<String, McpServerConfig> {
    load_mcp_server_configs_with_project(cwd)
        .into_iter()
        .map(|(name, (config, _))| (name, config))
        .collect()
}

/// Load all configured MCP servers with the scope each definition came from
/// (`"user"` or `"project"`).
///
/// Overlays project-scoped `.grow/config.toml` files from `cwd` up to the
/// repo root onto the user-tier config, nearest definition winning — the same
/// override semantics as [`get_mcp_server_config_with_project`].
pub fn load_mcp_server_configs_with_project(
    cwd: &std::path::Path,
) -> IndexMap<String, (McpServerConfig, &'static str)> {
    let global_config = crate::config::load_effective_config()
        .unwrap_or_else(|_| TomlValue::Table(toml::map::Map::new()));

    let mut servers: IndexMap<String, (McpServerConfig, &'static str)> =
        parse_mcp_servers_from_toml(&global_config)
            .into_iter()
            .map(|(name, config)| (name, (config, MCP_SCOPE_USER)))
            .collect();

    // find_project_configs is repo-root-first, so nearer files overwrite.
    for config_path in crate::config::find_project_configs(cwd) {
        if let Ok(root) = crate::config::load_config_file(&config_path) {
            for (name, config) in parse_mcp_servers_from_toml(&root) {
                servers.insert(name, (config, MCP_SCOPE_PROJECT));
            }
        }
    }

    servers
}

/// MCP config problems across the same layers as
/// [`load_mcp_server_configs_with_project`], for `grow inspect`.
pub fn load_mcp_server_problems_with_project(cwd: &std::path::Path) -> Vec<McpServerConfigProblem> {
    let mut problems = Vec::new();
    if let Ok(global_config) = crate::config::load_effective_config() {
        problems.extend(parse_mcp_servers_with_problems(&global_config).problems);
    }
    for config_path in crate::config::find_project_configs(cwd) {
        if let Ok(root) = crate::config::load_config_file(&config_path) {
            problems.extend(parse_mcp_servers_with_problems(&root).problems);
        }
    }
    problems
}

/// MCP server names with `enabled = false` in config.toml (including project overrides).
pub fn disabled_mcp_server_names(cwd: &std::path::Path) -> std::collections::HashSet<String> {
    let mut disabled: std::collections::HashSet<String> = load_all_mcp_configs(cwd)
        .into_iter()
        .filter(|(_, cfg)| !cfg.enabled)
        .map(|(name, _)| name)
        .collect();

    // Also check the `disabled_mcp_servers` array in config.toml.
    if let Ok(root) = crate::config::load_effective_config()
        && let Some(arr) = root.get("disabled_mcp_servers").and_then(|v| v.as_array())
    {
        for val in arr {
            if let Some(name) = val.as_str() {
                disabled.insert(name.to_string());
            }
        }
    }

    disabled
}

/// Names `grow mcp enable`/`disable` may target: user/project TOML (including
/// setup-required/invalid entries that session merge drops), the user
/// `disabled_mcp_servers` list, and plugin MCP servers.
///
pub fn cli_known_mcp_server_names(cwd: &std::path::Path) -> std::collections::HashSet<String> {
    let mut names = disabled_mcp_server_names(cwd);
    // Full TOML key set (list parity) — merge drops setup-required/invalid.
    names.extend(all_toml_mcp_server_names(cwd));

    // Doctor/Space path: resolved Grow TOML plus enabled plugins.
    let registry = load_cli_plugin_registry(cwd);
    for (server, _) in crate::session::mcp_catalog::merge_mcp_servers_sourced(cwd, Some(&registry))
    {
        let name = crate::session::mcp_catalog::mcp_server_name(&server);
        if !name.is_empty() {
            names.insert(name.to_string());
        }
    }
    names
}

/// Plugin registry for one-shot CLI discovery (matches mcp doctor gating).
fn load_cli_plugin_registry(cwd: &std::path::Path) -> agent::plugins::PluginRegistry {
    let trust_store = agent::plugins::TrustStore::load();
    let mut plugins_cfg: crate::agent::config::PluginsConfig =
        crate::config::load_effective_config()
            .ok()
            .and_then(|t| t.get("plugins").and_then(|v| v.clone().try_into().ok()))
            .unwrap_or_default();
    let mut plugin_config = plugins_cfg.to_discovery_config();
    let project_trusted = crate::agent::folder_trust::resolve_and_record(cwd, None, false);
    let discovered =
        agent::plugins::discover_plugins(Some(cwd), &plugin_config, &trust_store, project_trusted);
    plugin_config.populate_plugin_lists(&discovered);
    agent::plugins::PluginRegistry::from_discovered(
        discovered,
        &plugin_config.disabled,
        &plugin_config.enabled,
    )
}

fn config_path() -> PathBuf {
    crate::util::grow_home::grow_home().join("config.toml")
}

/// Path to the user-level config file (`~/.grow/config.toml`).
pub fn user_config_path() -> PathBuf {
    config_path()
}

/// Path to a project-level config file (`<dir>/.grow/config.toml`).
pub fn project_config_path(dir: &std::path::Path) -> PathBuf {
    dir.join(".grow").join("config.toml")
}

/// True when the config file at `path` defines `[mcp_servers.<name>]`.
///
/// Checks raw key presence rather than deserializing, so malformed entries
/// (the ones users most need `mcp remove` for) are still reported.
pub fn mcp_server_defined_at(path: &std::path::Path, server_name: &str) -> bool {
    let Ok(root) = crate::config::load_config_file(path) else {
        return false;
    };
    root.get("mcp_servers")
        .and_then(|v| v.as_table())
        .is_some_and(|servers| servers.contains_key(server_name))
}

/// Synchronously load just the management_api_key from the config file.
/// This is intended for use in contexts where async is not available.
pub fn load_management_api_key_sync() -> Option<String> {
    let root: TomlValue = crate::config::load_effective_config().ok()?;
    if let TomlValue::Table(table) = root
        && let Some(TomlValue::Table(endpoints)) = table.get("endpoints")
    {
        endpoints
            .get("management_api_key")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    }
}

/// Returns `None` when `[cli] use_leader` is not set in the config
/// (allowing a remote settings fallback), or `Some(true/false)` when
/// explicitly configured. This distinction lets callers fall through
/// to a remote flag when the user hasn't expressed a local preference.
pub fn use_leader_from_toml_opt(root: &TomlValue) -> Option<bool> {
    if let TomlValue::Table(table) = root
        && let Some(TomlValue::Table(cli)) = table.get("cli")
    {
        cli.get("use_leader").and_then(|v| v.as_bool())
    } else {
        None
    }
}

/// Check if leader mode is enabled in the config.
/// When true, the agent will connect to a shared leader process instead of
/// running the agent directly. This allows multiple agent instances to share one backend.
/// Defaults to false when not explicitly set.
pub fn use_leader_from_toml(root: &TomlValue) -> bool {
    use_leader_from_toml_opt(root).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use toml::Value as TomlValue;

    #[test]
    fn mcp_server_defined_at_checks_raw_key_presence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // `urll` fails McpServerConfig deserialization; the raw key must
        // still be reported so `mcp remove` can delete broken entries.
        std::fs::write(
            &path,
            "[mcp_servers.broken]\nurll = \"https://x.example\"\n",
        )
        .unwrap();

        assert!(mcp_server_defined_at(&path, "broken"));
        assert!(!mcp_server_defined_at(&path, "other"));
        assert!(!mcp_server_defined_at(
            &dir.path().join("missing.toml"),
            "broken"
        ));
    }

    /// Covers all canonical wire values plus the unknown/corrupt fallback.
    #[test]
    fn parse_mcp_servers_skips_unparseable_entries() {
        let root = toml::from_str::<TomlValue>(
            r#"
mcp_servers.broken = "not-a-table"

[mcp_servers.also_broken]
enabled = "yes"

[mcp_servers.ok]
command = "echo"
args = ["hi"]
"#,
        )
        .unwrap();
        let servers = parse_mcp_servers_from_toml(&root);
        assert!(!servers.contains_key("broken"));
        assert!(!servers.contains_key("also_broken"));
        assert!(servers.contains_key("ok"));
    }

    #[test]
    fn parse_mcp_server_config_reports_unknown_fields() {
        let value = toml::from_str::<TomlValue>(
            r#"
command = "echo"
enabeld = false
"#,
        )
        .unwrap();
        let (config, unknown_fields) = deserialize_mcp_server_config(&value).unwrap();
        assert!(
            config.enabled,
            "the misspelled field must not silently disable the server"
        );
        assert_eq!(unknown_fields, vec!["enabeld"]);
    }

    #[test]
    fn parse_mcp_servers_drops_transport_less_entry() {
        let root = toml::from_str::<TomlValue>(
            r#"
[mcp_servers.github]
enabled = false

[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
"#,
        )
        .unwrap();
        let ParsedMcpServers { servers, problems } = parse_mcp_servers_with_problems(&root);
        assert!(
            !servers.contains_key("github"),
            "transport-less entry is dropped, not kept"
        );
        assert!(servers.contains_key("linear"));
        assert!(servers["linear"].enabled);
        let problem = problems
            .iter()
            .find(|p| p.server == "github")
            .expect("github problem reported");
        assert_eq!(problem.severity, McpServerProblemSeverity::Error);
        assert!(
            problem.message.contains("disabled_mcp_servers"),
            "{problem:?}"
        );
    }

    #[test]
    fn parse_mcp_servers_rejects_enabled_without_transport() {
        let root = toml::from_str::<TomlValue>(
            r#"
[mcp_servers.half]
enabled = true
"#,
        )
        .unwrap();
        let ParsedMcpServers { servers, problems } = parse_mcp_servers_with_problems(&root);
        assert!(
            !servers.contains_key("half"),
            "enabled without command/url must be dropped"
        );
        assert!(problems.iter().any(|p| p.server == "half"));
    }

    #[test]
    fn parse_mcp_servers_rejects_blank_transport() {
        let root = toml::from_str::<TomlValue>(
            r#"
[mcp_servers.blank_url]
url = "  "

[mcp_servers.blank_cmd]
command = ""
"#,
        )
        .unwrap();
        let ParsedMcpServers { servers, problems } = parse_mcp_servers_with_problems(&root);
        assert!(!servers.contains_key("blank_url"), "blank url dropped");
        assert!(!servers.contains_key("blank_cmd"), "blank command dropped");
        assert_eq!(
            problems
                .iter()
                .filter(|p| p.severity == McpServerProblemSeverity::Error)
                .count(),
            2,
            "both blank transports reported: {problems:?}"
        );
    }

    #[test]
    fn json_map_skips_bad_entry_and_keeps_the_rest() {
        // One transport-less plugin entry must not drop its siblings.
        let value = serde_json::json!({
            "mcpServers": {
                "bad": { "enabled": false },
                "good": { "command": "npx", "args": ["-y", "pkg"] }
            }
        });
        let config = mcp_config_from_json_value(&value);
        assert!(!config.mcp_servers.contains_key("bad"));
        assert!(config.mcp_servers.contains_key("good"));
        assert!(config.mcp_servers["good"].enabled);
    }

    #[test]
    fn mcp_server_max_access_matches_config() {
        let mut servers = IndexMap::new();
        for (name, max_access) in [
            ("docs", "read_write"),
            ("search", "read_write"),
            ("linear", "all"),
            ("defaults", "all"),
        ] {
            let config: McpServerConfig = serde_json::from_value(serde_json::json!({
                "command": "npx",
                "max_access": max_access,
            }))
            .unwrap();
            servers.insert(name.to_string(), (config, MCP_SCOPE_USER));
        }
        let access = mcp_server_max_access_from_scoped_config(&servers, true);
        assert_eq!(access["docs"], tool_protocol::ToolAccess::ReadWrite);
        assert_eq!(access["linear"], tool_protocol::ToolAccess::All);
    }

    #[test]
    fn mcp_server_max_access_includes_trusted_project_winner() {
        let config: McpServerConfig = serde_json::from_value(serde_json::json!({
            "command": "npx",
            "max_access": "read_write",
        }))
        .unwrap();
        let servers = IndexMap::from([("linear".to_owned(), (config, MCP_SCOPE_PROJECT))]);

        let access = mcp_server_max_access_from_scoped_config(&servers, true);
        assert_eq!(access["linear"], tool_protocol::ToolAccess::ReadWrite);
    }

    #[test]
    fn mcp_server_max_access_drops_untrusted_project_winner_without_global_fallback() {
        let config: McpServerConfig = serde_json::from_value(serde_json::json!({
            "command": "npx",
            "max_access": "read_write",
        }))
        .unwrap();
        // The scoped map represents the final winner: a project definition
        // shadows the global entry, so an untrusted project must not fall back
        // to the shadowed global ceiling.
        let servers = IndexMap::from([("linear".to_owned(), (config, MCP_SCOPE_PROJECT))]);

        let access = mcp_server_max_access_from_scoped_config(&servers, false);
        assert!(!access.contains_key("linear"));
    }

    #[test]
    fn test_parse_mcp_servers_empty() {
        let root = toml::from_str::<TomlValue>("").unwrap();
        let servers = parse_mcp_servers_from_toml(&root);
        assert!(servers.is_empty());
    }

    #[test]
    fn test_parse_mcp_servers_stdio() {
        let toml_str = r#"
[mcp_servers.test_server]
command = "node"
args = ["server.js"]
"#;
        let root = toml::from_str::<TomlValue>(toml_str).unwrap();
        let servers = parse_mcp_servers_from_toml(&root);
        assert_eq!(servers.len(), 1);
        assert!(servers.contains_key("test_server"));
        let config = servers.get("test_server").unwrap();
        assert!(config.enabled);
        match &config.transport {
            McpServerTransportConfig::Stdio { command, args, .. } => {
                assert_eq!(command, "node");
                assert_eq!(args, &["server.js"]);
            }
            _ => panic!("Expected Stdio transport"),
        }
    }

    #[test]
    fn test_use_leader_parsing_true() {
        // Test that we can parse a config with use_leader = true
        let toml_str = r#"
[cli]
use_leader = true
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        assert!(use_leader_from_toml(&root));
    }

    #[test]
    fn test_use_leader_parsing_false() {
        // Test that we can parse a config with use_leader = false
        let toml_str = r#"
[cli]
use_leader = false
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        assert!(!use_leader_from_toml(&root));
    }

    #[test]
    fn test_use_leader_default_false() {
        // Test that missing use_leader defaults to false
        let toml_str = r#"
[cli]
auto_update = true
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        assert!(!use_leader_from_toml(&root));
    }

    #[test]
    fn test_use_leader_no_cli_section() {
        // Test with no cli section at all
        let toml_str = r#"
[models]
default = "grow-code-fast-1"
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        if let TomlValue::Table(ref table) = root {
            let has_cli = table.get("cli").is_some();
            assert!(!has_cli);
        }
        // use_leader_from_toml() should default to false when no cli section
        assert!(!use_leader_from_toml(&root));
    }

    #[test]
    fn test_use_leader_opt_returns_some_true() {
        let toml_str = r#"
[cli]
use_leader = true
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        assert_eq!(use_leader_from_toml_opt(&root), Some(true));
    }

    #[test]
    fn test_use_leader_opt_returns_some_false() {
        let toml_str = r#"
[cli]
use_leader = false
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        assert_eq!(use_leader_from_toml_opt(&root), Some(false));
    }

    #[test]
    fn test_use_leader_opt_returns_none_when_absent() {
        let toml_str = r#"
[cli]
auto_update = true
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        assert_eq!(use_leader_from_toml_opt(&root), None);
    }

    #[test]
    fn test_use_leader_opt_returns_none_when_no_cli_section() {
        let toml_str = r#"
[models]
default = "grow-code-fast-1"
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        assert_eq!(use_leader_from_toml_opt(&root), None);
    }

    // WorktreeType tests
    #[test]
    fn test_project_scoped_mcp_override_replaces_entirely() {
        // Simulate global config with timeouts
        let global_toml = r#"
[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
enabled = true
startup_timeout_sec = 10
tool_timeout_sec = 60
"#;
        let global_root = toml::from_str::<TomlValue>(global_toml).unwrap();
        let global_servers = parse_mcp_servers_from_toml(&global_root);

        // Simulate project config WITHOUT timeouts
        let project_toml = r#"
[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
enabled = true
"#;
        let project_root = toml::from_str::<TomlValue>(project_toml).unwrap();
        let project_servers = parse_mcp_servers_from_toml(&project_root);

        // Global config should have timeouts
        let global_linear = global_servers.get("linear").unwrap();
        assert_eq!(global_linear.startup_timeout_sec, Some(10));
        assert_eq!(global_linear.tool_timeout_sec, Some(60));

        // Project config should NOT have timeouts (defaults apply)
        let project_linear = project_servers.get("linear").unwrap();
        assert_eq!(project_linear.startup_timeout_sec, None);
        assert_eq!(project_linear.tool_timeout_sec, None);

        // Merge: project overrides global entirely
        let mut merged: IndexMap<String, McpServerConfig> = IndexMap::new();
        for (name, config) in &global_servers {
            merged.insert(name.clone(), config.clone());
        }
        for (name, config) in &project_servers {
            merged.insert(name.clone(), config.clone());
        }

        // After merge, the project config should have replaced the global one entirely
        let merged_linear = merged.get("linear").unwrap();
        assert_eq!(merged_linear.startup_timeout_sec, None);
        assert_eq!(merged_linear.tool_timeout_sec, None);
    }

    #[test]
    fn test_project_scoped_mcp_adds_new_servers() {
        let global_toml = r#"
[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
enabled = true
"#;
        let global_root = toml::from_str::<TomlValue>(global_toml).unwrap();
        let global_servers = parse_mcp_servers_from_toml(&global_root);

        let project_toml = r#"
[mcp_servers.buildkite]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.buildkite.com/mcp"]
enabled = true
"#;
        let project_root = toml::from_str::<TomlValue>(project_toml).unwrap();
        let project_servers = parse_mcp_servers_from_toml(&project_root);

        // Merge: project adds new server
        let mut merged: IndexMap<String, McpServerConfig> = IndexMap::new();
        for (name, config) in &global_servers {
            merged.insert(name.clone(), config.clone());
        }
        for (name, config) in &project_servers {
            merged.insert(name.clone(), config.clone());
        }

        assert_eq!(merged.len(), 2);
        assert!(merged.contains_key("linear"));
        assert!(merged.contains_key("buildkite"));
    }

    #[test]
    fn test_project_scoped_mcp_can_disable_server() {
        let global_toml = r#"
[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
enabled = true
"#;
        let global_root = toml::from_str::<TomlValue>(global_toml).unwrap();
        let global_servers = parse_mcp_servers_from_toml(&global_root);
        assert!(global_servers.get("linear").unwrap().enabled);

        // Project config disables the server
        let project_toml = r#"
[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
enabled = false
"#;
        let project_root = toml::from_str::<TomlValue>(project_toml).unwrap();
        let project_servers = parse_mcp_servers_from_toml(&project_root);

        let mut merged: IndexMap<String, McpServerConfig> = IndexMap::new();
        for (name, config) in &global_servers {
            merged.insert(name.clone(), config.clone());
        }
        for (name, config) in &project_servers {
            merged.insert(name.clone(), config.clone());
        }

        // After merge, the server should be disabled by project config
        assert!(!merged.get("linear").unwrap().enabled);
    }

    #[test]
    fn skills_config_default_is_empty() {
        let cfg = SkillsConfig::default();
        assert!(cfg.paths.is_empty());
        assert!(cfg.ignore.is_empty());
    }

    #[test]
    fn skills_config_parses_paths_and_ignore() {
        let root = toml::from_str::<TomlValue>(
            r#"
[skills]
paths = ["~/.grow/skills", "~/.grow/skills/special/SKILL.md"]
ignore = ["~/.grow/skills/noisy/SKILL.md"]
"#,
        )
        .unwrap();
        let TomlValue::Table(ref table) = root else {
            panic!()
        };
        let cfg = table
            .get("skills")
            .and_then(|v| v.clone().try_into::<SkillsConfig>().ok())
            .unwrap_or_default();
        assert_eq!(
            cfg.paths,
            vec!["~/.grow/skills", "~/.grow/skills/special/SKILL.md"]
        );
        assert_eq!(cfg.ignore, vec!["~/.grow/skills/noisy/SKILL.md"]);
    }

    #[test]
    fn test_project_scoped_mcp_preserves_unrelated_global_servers() {
        let global_toml = r#"
[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
enabled = true

[mcp_servers.buildkite]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.buildkite.com/mcp"]
enabled = true
"#;
        let global_root = toml::from_str::<TomlValue>(global_toml).unwrap();
        let global_servers = parse_mcp_servers_from_toml(&global_root);

        // Project only overrides linear
        let project_toml = r#"
[mcp_servers.linear]
command = "npx"
args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
enabled = false
"#;
        let project_root = toml::from_str::<TomlValue>(project_toml).unwrap();
        let project_servers = parse_mcp_servers_from_toml(&project_root);

        let mut merged: IndexMap<String, McpServerConfig> = IndexMap::new();
        for (name, config) in &global_servers {
            merged.insert(name.clone(), config.clone());
        }
        for (name, config) in &project_servers {
            merged.insert(name.clone(), config.clone());
        }

        // buildkite should be preserved from global config
        assert_eq!(merged.len(), 2);
        assert!(merged.get("buildkite").unwrap().enabled);
        assert!(!merged.get("linear").unwrap().enabled);
    }

    #[test]
    fn test_mcp_server_config_parses_tool_timeouts() {
        let toml_str = r#"
[mcp_servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
tool_timeout_sec = 60
tool_timeouts = { create_issue = 120, search_repositories = 30 }
"#;
        let root = toml::from_str::<TomlValue>(toml_str).unwrap();
        let servers = parse_mcp_servers_from_toml(&root);
        let github = servers.get("github").unwrap();

        assert_eq!(github.tool_timeout_sec, Some(60));
        let tt = github.tool_timeouts.as_ref().unwrap();
        assert_eq!(tt.get("create_issue"), Some(&120));
        assert_eq!(tt.get("search_repositories"), Some(&30));
        assert_eq!(tt.get("nonexistent"), None);
    }

    #[test]
    fn test_mcp_server_config_tool_timeouts_defaults_to_none() {
        let toml_str = r#"
[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
"#;
        let root = toml::from_str::<TomlValue>(toml_str).unwrap();
        let servers = parse_mcp_servers_from_toml(&root);
        let fs = servers.get("filesystem").unwrap();

        assert!(fs.tool_timeouts.is_none());
        assert!(fs.tool_timeout_sec.is_none());
        assert!(fs.expose_image_base64.is_none());
    }

    #[test]
    fn test_mcp_server_config_parses_expose_image_base64() {
        let toml_str = r#"
[mcp_servers.grafana]
url = "https://grafana.example/mcp"
expose_image_base64 = true
"#;
        let root = toml::from_str::<TomlValue>(toml_str).unwrap();
        let servers = parse_mcp_servers_from_toml(&root);
        let grafana = servers.get("grafana").unwrap();
        assert_eq!(grafana.expose_image_base64, Some(true));
    }

    #[test]
    fn plugin_mcp_json_parses() {
        let dir = tempfile::tempdir().unwrap();
        let mcp_json_path = dir.path().join("mcp.json");
        std::fs::write(
            &mcp_json_path,
            r#"{
                "mcpServers": {
                    "test_server": {
                        "command": "node",
                        "args": ["server.js"]
                    }
                }
            }"#,
        )
        .unwrap();
        let config = read_mcp_json(&mcp_json_path).expect("should parse plugin MCP config");
        assert_eq!(config.mcp_servers.len(), 1);
        assert!(config.mcp_servers.contains_key("test_server"));
    }

    #[test]
    fn plugin_mcp_json_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mcp_json_path = dir.path().join("mcp.json");
        // File does not exist — should not panic, just return None.
        assert!(read_mcp_json(&mcp_json_path).is_none());
    }

    #[test]
    fn plugin_mcp_json_env_var_default_value() {
        let tmp = tempfile::tempdir().unwrap();
        let mcp_path = tmp.path().join("plugin-mcp.json");
        std::fs::write(
            &mcp_path,
            r#"{
                "mcpServers": {
                    "api": {
                        "url": "${GROW_TEST_MCP_UNSET_VAR_12345:-https://fallback.example.com}/mcp"
                    }
                }
            }"#,
        )
        .unwrap();

        let servers = load_mcp_json_file(&mcp_path);
        assert_eq!(servers.len(), 1);
        match &servers[0] {
            acp::McpServer::Http(acp::McpServerHttp { url, .. }) => {
                assert_eq!(url, "https://fallback.example.com/mcp");
            }
            other => panic!("expected Http, got {:?}", other),
        }
    }

    #[test]
    fn mcp_json_all_toml_names_includes_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let grow_dir = tmp.path().join(".grow");
        std::fs::create_dir_all(&grow_dir).unwrap();
        std::fs::write(
            grow_dir.join("config.toml"),
            r#"
[mcp_servers.enabled_one]
url = "https://example.com"

[mcp_servers.disabled_one]
command = "/ignored"
enabled = false
"#,
        )
        .unwrap();
        git2::Repository::init(tmp.path()).unwrap();

        let names = all_toml_mcp_server_names(tmp.path());
        assert!(names.contains("enabled_one"));
        assert!(names.contains("disabled_one"));
    }

    #[tokio::test]
    async fn mcp_preferences_missing_malformed_and_save_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp_preferences.json");
        assert!(matches!(
            load_mcp_preferences_from(&path),
            McpPreferencesLoad::Missing
        ));
        assert!(load_mcp_preferences_from(&path).file().servers.is_empty());

        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(
            load_mcp_preferences_from(&path),
            McpPreferencesLoad::Corrupt
        ));
        let prefs = McpPreferencesFile {
            version: 1,
            servers: HashMap::from([(
                "acme".to_string(),
                McpServerPreferences {
                    values: HashMap::from([("site".to_string(), "us5".to_string())]),
                    source: Some(McpPreferenceSource {
                        kind: "plugin".to_string(),
                        plugin: Some("acme".to_string()),
                        scope: None,
                    }),
                    updated_at: Some("2026-06-19T00:00:00Z".to_string()),
                },
            )]),
        };
        assert!(save_mcp_preferences_to(&path, &prefs).await.is_err());

        std::fs::remove_file(&path).unwrap();
        save_mcp_preferences_to(&path, &prefs).await.unwrap();
        let loaded = load_mcp_preferences_from(&path).file();
        assert_eq!(loaded.servers["acme"].values["site"], "us5");
        assert_eq!(
            loaded.servers["acme"]
                .source
                .as_ref()
                .unwrap()
                .plugin
                .as_deref(),
            Some("acme")
        );
    }

    #[test]
    fn apply_mcp_server_enabled_updates_array_and_per_server_field() {
        let mut root: TomlValue = toml::from_str(
            r#"
[mcp_servers.local]
command = "npx"
enabled = true
"#,
        )
        .unwrap();
        let table = root.as_table_mut().unwrap();

        apply_mcp_server_enabled(table, "local", false);
        let disabled = table
            .get("disabled_mcp_servers")
            .and_then(|v| v.as_array())
            .expect("disabled_mcp_servers array");
        assert_eq!(disabled.len(), 1);
        assert_eq!(disabled[0].as_str(), Some("local"));
        assert_eq!(
            table
                .get("mcp_servers")
                .and_then(|v| v.as_table())
                .and_then(|s| s.get("local"))
                .and_then(|v| v.as_table())
                .and_then(|s| s.get("enabled"))
                .and_then(|v| v.as_bool()),
            Some(false)
        );

        apply_mcp_server_enabled(table, "local", true);
        assert!(table.get("disabled_mcp_servers").is_none());
        assert_eq!(
            table
                .get("mcp_servers")
                .and_then(|v| v.as_table())
                .and_then(|s| s.get("local"))
                .and_then(|v| v.as_table())
                .and_then(|s| s.get("enabled"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn apply_mcp_server_enabled_managed_name_only_updates_array() {
        let mut root: TomlValue = toml::from_str("disabled_mcp_servers = []\n").unwrap();
        let table = root.as_table_mut().unwrap();

        apply_mcp_server_enabled(table, "slack", false);
        let disabled = table
            .get("disabled_mcp_servers")
            .and_then(|v| v.as_array())
            .expect("disabled_mcp_servers array");
        assert_eq!(disabled.len(), 1);
        assert_eq!(disabled[0].as_str(), Some("slack"));
        assert!(table.get("mcp_servers").is_none());

        apply_mcp_server_enabled(table, "slack", true);
        assert!(table.get("disabled_mcp_servers").is_none());
        assert!(table.get("mcp_servers").is_none());
    }

    #[tokio::test]
    async fn clear_sticky_project_disabled_at_only_flips_false() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[mcp_servers.proj]
command = "npx"
enabled = false
"#,
        )
        .unwrap();
        assert!(
            clear_sticky_project_disabled_at(&path, "proj")
                .await
                .unwrap()
        );
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("enabled = true"), "{body}");
        // Already true: no write.
        assert!(
            !clear_sticky_project_disabled_at(&path, "proj")
                .await
                .unwrap()
        );
    }

    /// Nested project configs both sticky-disable the same server; enable
    /// unstick must rewrite cwd-nearest only (not the shadowed ancestor).
    #[tokio::test]
    async fn enable_unstick_only_touches_nearest_project_definition() {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let nested = tmp.path().join("pkg");
        std::fs::create_dir_all(nested.join(".grow")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".grow")).unwrap();

        let sticky = r#"
# keep me
[mcp_servers.svc]
command = "npx"
enabled = false
"#;
        let ancestor = tmp.path().join(".grow").join("config.toml");
        let nearer = nested.join(".grow").join("config.toml");
        std::fs::write(&ancestor, sticky).unwrap();
        std::fs::write(&nearer, sticky).unwrap();

        assert_eq!(
            nearest_project_mcp_definition(&nested, "svc").as_ref(),
            Some(&nearer)
        );

        let path = nearest_project_mcp_definition(&nested, "svc").unwrap();
        clear_sticky_project_disabled_at(&path, "svc")
            .await
            .unwrap();

        let nearer_body = std::fs::read_to_string(&nearer).unwrap();
        assert!(
            nearer_body.contains("enabled = true"),
            "nearest should be unstuck: {nearer_body}"
        );
        assert!(
            nearer_body.contains("# keep me"),
            "toml_edit must preserve comments: {nearer_body}"
        );
        let ancestor_body = std::fs::read_to_string(&ancestor).unwrap();
        assert!(
            ancestor_body.contains("enabled = false"),
            "shadowed ancestor must stay sticky: {ancestor_body}"
        );
    }

    #[tokio::test]
    async fn write_toml_table_if_changed_refuses_unparseable() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("broken.toml");
        std::fs::write(&path, "not = [valid\n").unwrap();
        let err = write_toml_table_if_changed(&path, |_t| {})
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("unparseable"),
            "expected refuse, got {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "not = [valid\n",
            "file must be left intact"
        );
    }

    // === merge_section tests ===
}

//! `grow inspect` — configuration introspection.
//!
//! Shows everything Grow discovers in the current directory: project
//! instructions, permissions, hooks, skills, agents, plugins, MCP servers,
//! LSP config, and config.toml sources. Supports `--json` for machine output.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use tools::types::config_source::ConfigSource;
use tools::util::truncate::estimate_tokens;

const TREE: &str = "\u{2514}";

/// Coarse scope label for project instructions and plugin entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Project,
    User,
    Global,
    Plugin,
    Builtin,
    Cli,
    Config,
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Project => "project",
            Self::User => "user",
            Self::Global => "global",
            Self::Plugin => "plugin",
            Self::Builtin => "builtin",
            Self::Cli => "cli",
            Self::Config => "config",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectReport {
    pub version: String,
    pub channel: String,
    pub cwd: String,
    pub project_root: Option<String>,
    /// Folder-trust verdict for `cwd`: when false, repo-local project hooks,
    /// plugins, and MCP/LSP entries are gated out of the listings below.
    pub project_trusted: bool,
    pub project_instructions: Vec<InstructionFile>,
    pub permissions: PermissionsReport,
    pub hooks: Vec<HookEntry>,
    pub skills: Vec<SkillEntry>,
    pub agents: Vec<AgentEntry>,
    pub plugins: Vec<PluginEntry>,
    pub marketplaces: Vec<MarketplaceSourceEntry>,
    pub mcp_servers: Vec<McpServerEntry>,
    pub lsp_servers: Vec<LspServerEntry>,
    pub config_sources: ConfigSources,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub config_warnings: Vec<crate::agent::config_model_override_parse::ConfigWarning>,
    /// Invalid or ignored `[mcp_servers.*]` entries.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mcp_config_problems: Vec<crate::util::config::McpServerConfigProblem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionFile {
    pub path: String,
    pub scope: Scope,
    pub file_type: String,
    pub size_bytes: usize,
    /// Estimated token count (chars / 4).
    pub approx_tokens: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsReport {
    pub sources: Vec<String>,
    pub loaded: usize,
    pub skipped: Vec<SkippedRule>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedRule {
    pub rule: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEntry {
    pub event: String,
    pub hook_type: String,
    pub target: String,
    pub source: ConfigSource,
    pub matcher: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub source: ConfigSource,
    pub user_invocable: bool,
    /// True when disabled by `[skills].disabled` config.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEntry {
    pub name: String,
    pub description: String,
    pub source: ConfigSource,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry {
    pub name: String,
    pub scope: Scope,
    pub path: String,
    pub enabled: bool,
    pub provides: PluginProvides,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginProvides {
    pub skills: usize,
    pub agents: usize,
    pub hooks: bool,
    pub mcp_servers: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSourceEntry {
    pub name: String,
    pub source: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerEntry {
    pub name: String,
    pub transport: String,
    pub target: String,
    pub source: ConfigSource,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspServerEntry {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub source: ConfigSource,
    pub extensions: Vec<String>,
    /// True when this project-scoped server would be skipped (untrusted folder).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub untrusted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSources {
    /// The global config.toml and trusted project .grow/config.toml files.
    pub layers: Vec<ConfigLayer>,
}

/// A single config layer entry for `grow inspect`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigLayer {
    /// Logical role of the layer: "user" or "project".
    pub role: String,
    pub path: String,
    /// "empty" or "parse error" when the on-disk file does not contribute
    /// effective config (after the real loader's processing). Omitted when
    /// the layer is present and contributes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub async fn inspect(cwd: &Path, json: bool) -> anyhow::Result<()> {
    let report = build_report(cwd).await;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human(&report);
    }

    Ok(())
}

async fn build_report(cwd: &Path) -> InspectReport {
    let effective_config = crate::config::load_effective_config()
        .as_ref()
        .cloned()
        .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()));
    let parsed_config = crate::agent::config::Config::new_from_toml_cfg(&effective_config).ok();

    let git_root = git2::Repository::discover(cwd)
        .ok()
        .and_then(|r| r.workdir().map(|p| p.to_path_buf()));

    // Route through the live folder-trust gate rather than a raw store read; no
    // session resolve has run for a one-shot `inspect`. The single verdict drives
    // the top-level flag and gates the hooks, plugins, and MCP/LSP listings so
    // they reflect runtime gating.
    crate::agent::folder_trust::resolve_and_record(cwd, None, false);
    let project_trusted = crate::agent::folder_trust::project_scope_allowed(cwd);

    let trust_store = agent::plugins::TrustStore::load();
    let mut plugins_cfg: crate::agent::config::PluginsConfig = effective_config
        .get("plugins")
        .and_then(|v| v.clone().try_into().ok())
        .unwrap_or_default();
    let mut plugin_config = plugins_cfg.to_discovery_config();
    // Project plugins gate on the same folder-trust verdict as hooks and the live
    // session/doctor sites, so the listing's `enabled` flags match runtime gating.
    let discovered_plugins =
        agent::plugins::discover_plugins(Some(cwd), &plugin_config, &trust_store, project_trusted);
    plugin_config.populate_plugin_lists(&discovered_plugins);

    let plugin_registry = agent::plugins::PluginRegistry::from_discovered(
        discovered_plugins.clone(),
        &plugin_config.disabled,
        &plugin_config.enabled,
    );

    // Same `[skills]` table the runtime loads, so `paths` skills appear,
    // `ignore`d ones are hidden, and `disabled` ones surface as disabled.
    let skills_config = crate::config::parse_skills_config(&effective_config);

    let (instructions, permissions, skills) = tokio::join!(
        list_instructions(cwd),
        list_permissions(cwd, project_trusted),
        list_skills(cwd, &plugin_registry, &skills_config),
    );

    let hooks = list_hooks(git_root.as_deref(), project_trusted, &discovered_plugins);
    let agents = list_agents(cwd, &plugin_registry);
    let plugins = list_plugins(&discovered_plugins);
    let marketplaces = list_marketplaces();
    let mcp = list_mcp_servers(cwd, &plugin_registry);
    let lsp = list_lsp_servers(cwd, &discovered_plugins);
    let configs = list_config_sources(cwd);
    let config_warnings = parsed_config
        .as_ref()
        .map(|c| c.config_warnings.clone())
        .unwrap_or_default();
    let mcp_config_problems = crate::util::config::load_mcp_server_problems_with_project(cwd);

    InspectReport {
        version: version::VERSION.to_string(),
        channel: crate::util::config::channel_name_from_cache()
            .unwrap_or("unknown")
            .to_string(),
        cwd: cwd.display().to_string(),
        project_root: git_root.map(|p| p.display().to_string()),
        project_trusted,
        project_instructions: instructions,
        permissions,
        hooks,
        skills,
        agents,
        plugins,
        marketplaces,
        mcp_servers: mcp,
        lsp_servers: lsp,
        config_sources: configs,
        config_warnings,
        mcp_config_problems,
    }
}

/// Read `[paths] extra_rule_dirs` from the effective config. Returns empty
/// on any read/parse failure so misconfiguration never breaks classification.
fn extra_rule_dirs_from_config() -> Vec<String> {
    let Ok(root) = crate::config::load_effective_config() else {
        return Vec::new();
    };
    root.get("paths")
        .and_then(|v| v.get("extra_rule_dirs"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn has_rules_directory(file_path: &str, config_dir: &str) -> bool {
    let mut previous = None;
    for component in file_path
        .split(['/', '\\'])
        .filter(|component| !component.is_empty())
    {
        if previous == Some(config_dir) && component == "rules" {
            return true;
        }
        previous = Some(component);
    }
    false
}

fn instruction_scope(file_path: &str, grow_home: &Path, workspace_root: &Path) -> Scope {
    if crate::util::is_user_instruction_path(Path::new(file_path), grow_home, Some(workspace_root))
    {
        Scope::Global
    } else {
        Scope::Project
    }
}

fn instruction_file_type(
    file_path: &str,
    grow_home: &Path,
    extra_rule_prefixes: &[PathBuf],
) -> &'static str {
    let path = Path::new(file_path);
    if path
        .parent()
        .is_some_and(|parent| parent == grow_home.join("rules"))
        || has_rules_directory(file_path, ".grow")
        || extra_rule_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
    {
        "rules"
    } else {
        "agents_md"
    }
}

/// Wraps the production instruction discovery (`agents_md::read_agents_config_with_paths`).
async fn list_instructions(cwd: &Path) -> Vec<InstructionFile> {
    let configs =
        agent::prompt::agents_md::read_agents_config_with_paths(&cwd.display().to_string()).await;

    let grow_home = crate::util::grow_home::grow_home();
    let workspace_root = git2::Repository::discover(cwd)
        .ok()
        .and_then(|repo| repo.workdir().map(Path::to_path_buf))
        .unwrap_or_else(|| cwd.to_path_buf());

    let extra_rule_dirs = extra_rule_dirs_from_config();
    // Pre-expand `~/` and resolve once, so the per-config-file matching loop
    // can use a clean prefix check. Empty/invalid paths fall
    // through to a no-op match.
    //
    // TODO(phase-3): `extra_rule_dirs` only re-classifies files that
    // `agent::prompt::agents_md::read_agents_config_with_paths`
    // has already discovered. Plumbing `extra_rule_dirs` through to that
    // discovery (so files in arbitrary user-configured dirs are surfaced as
    // rules instead of being missed entirely) is out of scope for this stack
    // (intentional wontfix for now).
    // Skills (`extensions/skills.rs`) take the typed-scan path so they don't
    // have this limitation; rules need the same treatment in a follow-up.
    let extra_rule_prefixes: Vec<std::path::PathBuf> = extra_rule_dirs
        .iter()
        .map(|d| crate::util::expand_home(d))
        .collect();

    configs
        .into_iter()
        .map(|c| {
            let file_type = instruction_file_type(&c.file_path, &grow_home, &extra_rule_prefixes);
            let scope = instruction_scope(&c.file_path, &grow_home, &workspace_root);
            let size = c.content.len();
            InstructionFile {
                size_bytes: size,
                approx_tokens: estimate_tokens(&c.content),
                path: c.file_path,
                scope,
                file_type: file_type.to_string(),
                disabled: false,
            }
        })
        .collect()
}

/// Calls the canonical native permission resolver.
async fn list_permissions(cwd: &Path, project_trusted: bool) -> PermissionsReport {
    use workspace::permission::resolution;

    let Some(resolved) =
        resolution::resolve_permissions_with_provenance(cwd, project_trusted).await
    else {
        return PermissionsReport {
            sources: vec![],
            loaded: 0,
            skipped: vec![],
        };
    };

    let mut sources: Vec<String> = resolved.sources.iter().map(|s| s.to_string()).collect();
    sources.dedup();

    let skipped = resolved
        .skipped
        .into_iter()
        .map(|s| SkippedRule {
            rule: s.rule,
            reason: s.reason,
        })
        .collect();

    PermissionsReport {
        sources,
        loaded: resolved.config.rules.len(),
        skipped,
    }
}

/// Discovers canonical Grow hooks.
fn list_hooks(
    git_root: Option<&Path>,
    project_trusted: bool,
    discovered_plugins: &[agent::plugins::DiscoveredPlugin],
) -> Vec<HookEntry> {
    // Route through the same assembly as session startup so config-layer hooks
    // appear in `/hooks` status alongside file hooks.
    let config_layers = config::hook_config_layers();
    let (registry, _errors) =
        crate::util::hooks::assemble_hooks(&config_layers, git_root, project_trusted);

    let mut entries: Vec<HookEntry> = registry
        .all_hooks()
        .into_iter()
        .map(|h| {
            // Classify via the shared `hook_origin`, the same classifier used by
            // diagnostics, so both surfaces agree on provenance.
            use ::hooks::config::HookOrigin as O;
            let path = h.source_dir.clone();
            let source = match ::hooks::config::hook_origin(h) {
                O::UserConfig => ConfigSource::ConfigToml { path },
                O::ProjectFile => ConfigSource::Project { path },
                // File/plugin/agent/unknown hooks are user-scoped for display.
                O::UserFile | O::Plugin | O::Agent | O::Unknown => ConfigSource::User { path },
            };
            HookEntry {
                event: h.event.to_string(),
                hook_type: h.handler_type.as_str().to_string(),
                target: h
                    .command
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .or_else(|| h.url.clone())
                    .unwrap_or_default(),
                source,
                matcher: h.configured_matcher.clone(),
                disabled: false,
            }
        })
        .collect();

    // Plugin hooks
    for p in discovered_plugins {
        if !p.trusted {
            continue;
        }
        let source = ConfigSource::Plugin {
            plugin_name: p.manifest.name.clone(),
            path: p.root.clone(),
        };
        if let Some(ref hooks_path) = p.hooks_path {
            entries.push(HookEntry {
                event: "(plugin)".to_string(),
                hook_type: "file".to_string(),
                target: hooks_path.display().to_string(),
                source,
                matcher: None,
                disabled: false,
            });
        } else if p.manifest.inline_hooks().is_some() {
            entries.push(HookEntry {
                event: "(plugin)".to_string(),
                hook_type: "inline".to_string(),
                target: String::new(),
                source,
                matcher: None,
                disabled: false,
            });
        }
    }

    entries
}

async fn list_skills(
    cwd: &Path,
    plugin_registry: &agent::plugins::PluginRegistry,
    skills_config: &agent::prompt::skills::SkillsConfig,
) -> Vec<SkillEntry> {
    let skills = agent::prompt::skills::list_skills_with_plugins(
        Some(&cwd.display().to_string()),
        skills_config,
        Some(plugin_registry),
    )
    .await;

    skills
        .into_iter()
        .map(|s| {
            let source = skill_entry_source(&s);
            SkillEntry {
                name: s.label().to_string(),
                description: s.description,
                source,
                user_invocable: s.user_invocable,
                disabled: !s.enabled,
            }
        })
        .collect()
}

/// Resolve the inspect-facing source for a discovered skill.
///
/// Prefers the discovery-stamped `config_source` (plugin skills,
/// `[skills].paths` entries), then falls back to the discovered scope.
fn skill_entry_source(s: &agent::prompt::skills::SkillInfo) -> ConfigSource {
    use tools::implementations::skills::types::SkillScope;

    if let Some(source) = s.config_source.clone() {
        return source;
    }
    let path = PathBuf::from(&s.path);
    match s.scope {
        SkillScope::Local | SkillScope::Repo => ConfigSource::Project { path },
        SkillScope::User => ConfigSource::User { path },
        SkillScope::Server => ConfigSource::Server { path },
        SkillScope::Bundled => ConfigSource::Bundled { path },
        SkillScope::Plugin => ConfigSource::Plugin {
            plugin_name: String::new(),
            path,
        },
    }
}

fn list_agents(cwd: &Path, plugin_registry: &agent::plugins::PluginRegistry) -> Vec<AgentEntry> {
    let agents =
        agent::discovery::all_subagents_with_plugins(cwd, &HashMap::new(), Some(plugin_registry));

    agents
        .into_iter()
        .map(|a| AgentEntry {
            name: a.name,
            description: a.description,
            source: a.config_source,
        })
        .collect()
}

/// Maps pre-discovered plugins (from `discover_plugins`) to inspect entries.
fn list_plugins(discovered: &[agent::plugins::DiscoveredPlugin]) -> Vec<PluginEntry> {
    discovered
        .iter()
        .map(|p| {
            let scope = match p.scope {
                agent::plugins::PluginScope::CliOverride => Scope::Cli,
                agent::plugins::PluginScope::Project => Scope::Project,
                agent::plugins::PluginScope::User => Scope::User,
                agent::plugins::PluginScope::ConfigPath => Scope::Config,
            };
            PluginEntry {
                name: p.manifest.name.clone(),
                scope,
                path: p.root.display().to_string(),
                enabled: p.trusted,
                provides: PluginProvides {
                    // Count actual SKILL.md files discovered (root-level or in
                    // subdirs), not the number of configured skill dirs, so the
                    // reported count matches what the skills registry loads.
                    skills: agent::plugins::registry::skill_md_paths(&p.skill_dirs).len(),
                    agents: p.agent_dirs.len(),
                    hooks: p.hooks_path.is_some(),
                    mcp_servers: if p.mcp_config_path.is_some() { 1 } else { 0 },
                },
            }
        })
        .collect()
}

/// Report the canonical marketplace sources configured in Grow.
fn list_marketplaces() -> Vec<MarketplaceSourceEntry> {
    crate::plugin::load_marketplace_sources()
        .into_iter()
        .map(|marketplace| MarketplaceSourceEntry {
            name: marketplace.name,
            source: match marketplace.kind {
                plugin_marketplace::SourceKind::Git { url, .. } => url,
                plugin_marketplace::SourceKind::Local { path } => path.display().to_string(),
            },
        })
        .collect()
}

/// Discovers canonical Grow MCP sources.
fn list_mcp_servers(
    cwd: &Path,
    plugin_registry: &agent::plugins::PluginRegistry,
) -> Vec<McpServerEntry> {
    let sourced =
        crate::session::mcp_catalog::merge_mcp_servers_sourced(cwd, Some(plugin_registry));

    sourced
        .into_iter()
        .map(|(server, source)| {
            let (name, transport, target) = match &server {
                agent_client_protocol::schema::v1::McpServer::Stdio(
                    agent_client_protocol::schema::v1::McpServerStdio { name, command, .. },
                ) => (name.clone(), "stdio", command.display().to_string()),
                agent_client_protocol::schema::v1::McpServer::Http(
                    agent_client_protocol::schema::v1::McpServerHttp { name, url, .. },
                ) => (name.clone(), "http", url.clone()),
                agent_client_protocol::schema::v1::McpServer::Sse(
                    agent_client_protocol::schema::v1::McpServerSse { name, url, .. },
                ) => (name.clone(), "sse", url.clone()),
                // TODO(acp-0.10): `McpServer` is #[non_exhaustive].
                _ => ("unknown".to_string(), "unknown", String::new()),
            };
            McpServerEntry {
                name,
                transport: transport.to_string(),
                target,
                source,
                disabled: false,
                disabled_reason: None,
            }
        })
        .collect()
}

/// Wraps the production LSP loader (`load_servers_with_plugins_sourced`).
fn list_lsp_servers(
    cwd: &Path,
    discovered_plugins: &[agent::plugins::DiscoveredPlugin],
) -> Vec<LspServerEntry> {
    let trusted: Vec<_> = discovered_plugins.iter().filter(|p| p.trusted).collect();
    let plugin_lsp_paths: Vec<std::path::PathBuf> = trusted
        .iter()
        .filter_map(|p| p.lsp_config_path.clone())
        .collect();
    let plugin_names: Vec<&str> = trusted
        .iter()
        .filter(|p| p.lsp_config_path.is_some())
        .map(|p| p.manifest.name.as_str())
        .collect();
    let plugin_inline_lsp: Vec<(&serde_json::Value, &str)> = trusted
        .iter()
        .filter_map(|p| {
            p.manifest
                .inline_lsp_servers()
                .map(|v| (v, p.manifest.name.as_str()))
        })
        .collect();
    let inline_values: Vec<&serde_json::Value> =
        plugin_inline_lsp.iter().map(|(v, _)| *v).collect();
    let inline_names: Vec<&str> = plugin_inline_lsp.iter().map(|(_, n)| *n).collect();

    let servers = tools::implementations::lsp::config::load_servers_with_plugins_sourced(
        cwd,
        &plugin_lsp_paths,
        &inline_values,
        &plugin_names,
        &inline_names,
    );

    // Folder-trust gate (display-only): inspect never spawns servers, but mark the
    // repo-local (project-scoped) entries a session would skip in an untrusted
    // Clone so the listing matches the live gate. Standalone inspection has no
    // live session state.
    crate::agent::folder_trust::resolve_and_record(cwd, None, false);
    let project_allowed = crate::agent::folder_trust::project_scope_allowed(cwd);

    servers
        .into_iter()
        .map(|(name, (cfg, source))| {
            let untrusted = !project_allowed && matches!(source, ConfigSource::Project { .. });
            LspServerEntry {
                name,
                command: cfg.command,
                args: cfg.args,
                source,
                extensions: cfg.extensions.keys().cloned().collect(),
                untrusted,
            }
        })
        .collect()
}

/// Locates the global `config.toml` and project `.grow/config.toml` files. Only
/// on-disk files are emitted; the primary global config always gets a
/// "User: (none)" line in the human view when absent.
/// `note` distinguishes files that exist but contribute nothing after the
/// real loader's processing (stripping, version overrides, fail_closed, etc).
/// Parse errors are reported distinctly rather than as "empty".
fn list_config_sources(cwd: &Path) -> ConfigSources {
    let mut layers: Vec<ConfigLayer> = vec![];

    // User config.toml (primary user layer; shown as (none) when absent)
    if let Some(home) = crate::config::user_grow_home() {
        let p = home.join("config.toml");
        if let Some((path_s, note)) = describe_config_file(&p) {
            layers.push(ConfigLayer {
                role: "user".to_string(),
                path: path_s,
                note,
            });
        }
    }

    // Project configs (from git root up); each is its own "project" role entry
    for p in crate::config::find_project_configs(cwd) {
        if p.exists()
            && let Some((path_s, note)) = describe_config_file(&p)
        {
            layers.push(ConfigLayer {
                role: "project".to_string(),
                path: path_s,
                note,
            });
        }
    }

    ConfigSources { layers }
}

/// For global / project config files: use `load_config_file` (the
/// production path for those layers) so `note` reflects post-processing
/// (version overrides stripped) and distinguishes parse failure.
fn describe_config_file(path: &Path) -> Option<(String, Option<String>)> {
    if !path.exists() {
        return None;
    }
    let path_s = path.display().to_string();
    match crate::config::load_config_file(path) {
        Ok(v) => {
            let empty = v.as_table().is_none_or(|t| t.is_empty());
            Some((
                path_s,
                if empty {
                    Some("empty".to_string())
                } else {
                    None
                },
            ))
        }
        Err(_) => Some((path_s, Some("parse error".to_string()))),
    }
}

fn print_section<T>(title: &str, items: &[T], format_item: impl Fn(&T) -> String) {
    println!();
    println!("  {} ({})", title, items.len());
    if items.is_empty() {
        println!("  {TREE} (none)");
    }
    for item in items {
        println!("  {TREE} {}", format_item(item));
    }
}

/// Print items in a two-column layout: name on the left, source label on the right.
fn print_columns<T>(
    title: &str,
    items: &[T],
    name: impl Fn(&T) -> String,
    label: impl Fn(&T) -> String,
) {
    println!();
    println!("  {} ({})", title, items.len());
    if items.is_empty() {
        println!("  {TREE} (none)");
        return;
    }
    let names: Vec<String> = items.iter().map(&name).collect();
    let pad = names.iter().map(|n| n.len()).max().unwrap_or(0).min(50);
    for (item, n) in items.iter().zip(&names) {
        println!("  {TREE} {:<pad$}  {}", n, label(item));
    }
}

fn disabled_tag(disabled: bool) -> &'static str {
    if disabled { " [disabled]" } else { "" }
}

fn render_config_warnings(
    warnings: &[crate::agent::config_model_override_parse::ConfigWarning],
) -> String {
    use std::fmt::Write as _;

    if warnings.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n  Config Warnings\n");
    let _ = writeln!(out, "  {TREE} {} warning(s)", warnings.len());
    for w in warnings {
        let field = w.field().map(|f| format!(" {f}")).unwrap_or_default();
        let _ = writeln!(
            out,
            "    {TREE} [{}]{field} — {}",
            w.target.label(),
            w.reason
        );
    }
    out
}

fn render_mcp_config_problems(problems: &[crate::util::config::McpServerConfigProblem]) -> String {
    use crate::util::config::McpServerProblemSeverity;
    use std::fmt::Write as _;

    if problems.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n  MCP Config Problems\n");
    let _ = writeln!(out, "  {TREE} {} problem(s)", problems.len());
    for p in problems {
        let severity = match p.severity {
            McpServerProblemSeverity::Error => "error",
            McpServerProblemSeverity::Warning => "warning",
        };
        let _ = writeln!(out, "    {TREE} [{severity}] {}", p.message);
    }
    out
}

fn print_human(r: &InspectReport) {
    println!();
    println!("  Environment");
    println!("  {TREE} Version: {} [{}]", r.version, r.channel);
    println!("  {TREE} CWD: {}", r.cwd);
    if let Some(ref root) = r.project_root {
        println!("  {TREE} Git root: {}", root);
    }
    println!(
        "  {TREE} Project trusted: {}",
        if r.project_trusted { "yes" } else { "no" }
    );

    print_section("Project Instructions", &r.project_instructions, |f| {
        let status = disabled_tag(f.disabled);
        format!(
            "{} ({}, ~{} tokens){}",
            f.path, f.scope, f.approx_tokens, status,
        )
    });

    println!();
    println!("  Permissions");
    if r.permissions.sources.is_empty() {
        println!("  {TREE} Source: (none)");
    } else {
        for src in &r.permissions.sources {
            println!("  {TREE} Source: {src}");
        }
    }
    println!(
        "  {TREE} {} loaded, {} skipped",
        r.permissions.loaded,
        r.permissions.skipped.len()
    );
    for s in &r.permissions.skipped {
        println!("    {TREE} {} -- {}", s.rule, s.reason);
    }

    print_columns(
        "Skills",
        &r.skills,
        |s| s.name.clone(),
        |s| format!("{}{}", s.source.display_label(), disabled_tag(s.disabled)),
    );

    print_columns(
        "Agents",
        &r.agents,
        |a| a.name.clone(),
        |a| a.source.display_label(),
    );

    print_columns(
        "Plugins",
        &r.plugins,
        |p| {
            let status = if p.enabled { "enabled" } else { "disabled" };
            format!("{} ({}, {})", p.name, p.scope, status)
        },
        |p| {
            let mut parts = Vec::new();
            if p.provides.skills > 0 {
                parts.push(format!("{} skills", p.provides.skills));
            }
            if p.provides.agents > 0 {
                parts.push(format!("{} agents", p.provides.agents));
            }
            if p.provides.hooks {
                parts.push("hooks".into());
            }
            if p.provides.mcp_servers > 0 {
                parts.push(format!("{} MCPs", p.provides.mcp_servers));
            }
            if parts.is_empty() {
                "-".into()
            } else {
                parts.join(", ")
            }
        },
    );

    print_section("Marketplaces", &r.marketplaces, |m| {
        format!("{} ({})", m.name, m.source)
    });

    if r.mcp_servers.is_empty() {
        println!();
        println!("  MCP Servers (0)");
        println!("  {TREE} (none) \u{2014} see `grow mcp add --help`");
    } else {
        print_columns(
            "MCP Servers",
            &r.mcp_servers,
            |m| {
                if let Some(ref reason) = m.disabled_reason {
                    format!("{} ({}) [BLOCKED: {}]", m.name, m.transport, reason)
                } else {
                    format!("{} ({})", m.name, m.transport)
                }
            },
            |m| format!("{}{}", m.source.display_label(), disabled_tag(m.disabled)),
        );
    }

    print_columns(
        "LSP Servers",
        &r.lsp_servers,
        |l| format!("{} ({} {})", l.name, l.command, l.args.join(" ")),
        |l| {
            let untrusted = if l.untrusted { " [untrusted]" } else { "" };
            format!("{}{}", l.source.display_label(), untrusted)
        },
    );

    print_columns(
        "Hooks",
        &r.hooks,
        |h| {
            let matcher = h
                .matcher
                .as_ref()
                .map(|m| format!(" matcher={}", m))
                .unwrap_or_default();
            format!("{}{}", h.hook_type, matcher)
        },
        |h| format!("{}{}", h.source.display_label(), disabled_tag(h.disabled)),
    );

    println!();
    println!("  Config Sources");
    // User is always emitted (with (none) when absent) for the primary user config.
    if let Some(user_l) = r.config_sources.layers.iter().find(|l| l.role == "user") {
        let tag = match user_l.note.as_deref() {
            Some("empty") => " (empty)",
            Some("parse error") => " (parse error)",
            _ => "",
        };
        println!("  {TREE} User: {}{}", user_l.path, tag);
    } else {
        println!("  {TREE} User: (none)");
    }
    for layer in &r.config_sources.layers {
        if layer.role == "user" {
            continue;
        }
        let tag = match layer.note.as_deref() {
            Some("empty") => " (empty)",
            Some("parse error") => " (parse error)",
            _ => "",
        };
        let label = match layer.role.as_str() {
            "user" => "User",
            "project" => "Project",
            other => other,
        };
        println!("  {TREE} {}: {}{}", label, layer.path, tag);
    }
    if !r.config_sources.layers.iter().any(|l| l.role == "project") {
        println!("  {TREE} Project: (none)");
    }

    print!("{}", render_config_warnings(&r.config_warnings));
    print!("{}", render_mcp_config_problems(&r.mcp_config_problems));
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::prompt::skills::{SkillInfo, SkillsConfig};
    use tools::implementations::skills::types::SkillScope;

    #[test]
    fn grow_home_nested_in_workspace_keeps_direct_surfaces_global() {
        let grow_home = Path::new("/repo/config");
        let workspace = Path::new("/repo");
        for path in ["/repo/config/AGENTS.md", "/repo/config/rules/global.md"] {
            assert!(matches!(
                instruction_scope(path, grow_home, workspace),
                Scope::Global
            ));
        }
        for path in [
            "/repo/config/.grow/rules/project.md",
            "/repo/config/src/AGENTS.md",
        ] {
            assert!(matches!(
                instruction_scope(path, grow_home, workspace),
                Scope::Project
            ));
        }
    }

    #[test]
    fn workspace_scope_wins_inside_grow_home() {
        let grow_home = Path::new("/custom/grow");
        let workspace = Path::new("/custom/grow/worktrees/repo");
        for path in [
            "/custom/grow/worktrees/repo/.cursor/rules/project.md",
            "/custom/grow/worktrees/repo/src/AGENTS.md",
        ] {
            assert!(matches!(
                instruction_scope(path, grow_home, workspace),
                Scope::Project
            ));
        }
        assert!(matches!(
            instruction_scope("/custom/grow/rules/global.md", grow_home, workspace,),
            Scope::Global
        ));
    }

    #[test]
    fn custom_grow_home_rules_are_classified_as_rules() {
        assert_eq!(
            instruction_file_type(
                "/custom/config/rules/team.md",
                Path::new("/custom/config"),
                &[],
            ),
            "rules"
        );
        assert_eq!(
            instruction_file_type("/custom/config/AGENTS.md", Path::new("/custom/config"), &[],),
            "agents_md"
        );
    }

    #[test]
    fn describe_config_file_flags_empty_and_parse_error() {
        let dir = tempfile::tempdir().unwrap();

        // Missing file: describe returns None (no layer entry).
        let missing = dir.path().join("missing.toml");
        assert!(describe_config_file(&missing).is_none());

        // Comment-only and whitespace-only files parse to an empty table after load.
        let comment_only = dir.path().join("comment.toml");
        std::fs::write(&comment_only, "# nothing enforced here\n").unwrap();
        let (_, note) = describe_config_file(&comment_only).unwrap();
        assert_eq!(note.as_deref(), Some("empty"));

        let blank = dir.path().join("blank.toml");
        std::fs::write(&blank, "\n\n").unwrap();
        let (_, note) = describe_config_file(&blank).unwrap();
        assert_eq!(note.as_deref(), Some("empty"));

        // A file with real content contributes config and has no note.
        let with_content = dir.path().join("content.toml");
        std::fs::write(&with_content, "[diagnostics]\nmode = \"disabled\"\n").unwrap();
        let (_, note) = describe_config_file(&with_content).unwrap();
        assert!(note.is_none());

        // Malformed TOML is flagged as parse error (distinct from empty).
        let bad = dir.path().join("bad.toml");
        std::fs::write(&bad, "[[[ this is not valid toml").unwrap();
        let (_, note) = describe_config_file(&bad).unwrap();
        assert_eq!(note.as_deref(), Some("parse error"));
    }

    /// Public provider/model warnings flow from an effective config through
    /// `Config` to the human renderer and the JSON report.
    #[test]
    fn config_warnings_inspect_smoke() {
        let effective: toml::Value = toml::from_str(
            r#"
            [provider.gateway]
            api_backend = "responses"

            [provider.gateway.options]
            base_url = "https://gateway.example/v1"

            [provider.gateway.options.auth]
            type = "command"
            command = ""
            token_ttl_secs = 10

            [provider.gateway.models.model-a]
            reasoning_effort = "not-a-level"
            "#,
        )
        .unwrap();
        let cfg = crate::agent::config::Config::new_from_toml_cfg(&effective).unwrap();
        let warnings = cfg.config_warnings;
        assert!(
            warnings.iter().any(|w| w.field() == Some("auth.command")),
            "invalid inline auth should warn: {warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.field() == Some("reasoning_effort")),
            "invalid enum should warn: {warnings:?}"
        );
        assert!(cfg.config_models.contains_key("gateway/model-a"));

        let human = render_config_warnings(&warnings);
        assert!(human.contains("Config Warnings"), "{human}");
        assert!(
            human.contains("[provider.\"gateway\".options] auth.command"),
            "{human}"
        );
        assert!(
            human.contains("[provider.\"gateway\".models.\"model-a\"] reasoning_effort"),
            "{human}"
        );
        // Auth-provider warnings render under their own table syntax.
        let provider_warning =
            crate::agent::config_model_override_parse::ConfigWarning::auth_provider(
                "litellm",
                Some("command"),
                crate::agent::config_model_override_parse::ConfigWarningKind::InvalidValue,
                "missing or empty command".to_owned(),
            );
        let human = render_config_warnings(&[provider_warning]);
        assert!(
            human.contains("[auth_provider.\"litellm\"] command"),
            "{human}"
        );
        // A dotted provider name renders whole; the field splits off the
        // right.
        let dotted = crate::agent::config_model_override_parse::ConfigWarning::auth_provider(
            "corp.gateway",
            Some("token_ttl_secs"),
            crate::agent::config_model_override_parse::ConfigWarningKind::InvalidValue,
            "at or below the refresh margin".to_owned(),
        );
        let human = render_config_warnings(&[dotted]);
        assert!(
            human.contains("[auth_provider.\"corp.gateway\"] token_ttl_secs"),
            "{human}"
        );
        assert_eq!(render_config_warnings(&[]), "");

        let json = serde_json::to_value(&warnings).unwrap();
        let auth_warning = json
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["field"] == "auth.command")
            .expect("inline auth warning present in JSON");
        assert_eq!(auth_warning["target"], "provider");
        assert_eq!(auth_warning["id"], "gateway");
        assert_eq!(auth_warning["kind"], "invalid-value");
        assert!(
            auth_warning["reason"]
                .as_str()
                .is_some_and(|r| !r.is_empty())
        );
    }

    // ── skill source mapping (skill_entry_source) ─────────────────────────

    fn skill_fixture(name: &str, path: &str, scope: SkillScope) -> SkillInfo {
        SkillInfo {
            name: name.to_string(),
            description: format!("desc for {name}"),
            path: path.to_string(),
            scope,
            ..SkillInfo::default()
        }
    }

    #[test]
    fn skill_entry_source_maps_scopes() {
        let s = skill_fixture("a", "/repo/.grow/skills/a/SKILL.md", SkillScope::Local);
        assert!(matches!(
            skill_entry_source(&s),
            ConfigSource::Project { .. }
        ));

        let s = skill_fixture("b", "/repo/.grow/skills/b/SKILL.md", SkillScope::Repo);
        assert!(matches!(
            skill_entry_source(&s),
            ConfigSource::Project { .. }
        ));

        let s = skill_fixture("c", "/home/u/.grow/skills/c/SKILL.md", SkillScope::User);
        assert!(matches!(skill_entry_source(&s), ConfigSource::User { .. }));

        let s = skill_fixture(
            "d",
            "/home/u/.grow/server-skills/d/SKILL.md",
            SkillScope::Server,
        );
        assert!(matches!(
            skill_entry_source(&s),
            ConfigSource::Server { .. }
        ));

        let s = skill_fixture("e", "/home/u/.grow/bundled/e/SKILL.md", SkillScope::Bundled);
        assert!(matches!(
            skill_entry_source(&s),
            ConfigSource::Bundled { .. }
        ));
    }

    /// A discovery-stamped `config_source` (plugins, `[skills].paths`) wins
    /// over the scope fallback.
    #[test]
    fn skill_entry_source_prefers_stamped_config_source() {
        let mut s = skill_fixture("cfg", "/team/skills/cfg/SKILL.md", SkillScope::User);
        s.config_source = Some(ConfigSource::ConfigToml {
            path: PathBuf::from("/team/skills/cfg/SKILL.md"),
        });
        assert!(matches!(
            skill_entry_source(&s),
            ConfigSource::ConfigToml { .. }
        ));
    }

    /// `list_skills` must honor the `[skills]` table like the runtime does:
    /// `paths` skills appear (with a `configToml` source), `ignore`d skills
    /// are hidden, and `disabled` skills stay listed but flagged.
    #[tokio::test]
    async fn list_skills_honors_skills_config() {
        let write = |dir: &Path, name: &str| {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: test skill {name}\n---\n\nBody.\n"),
            )
            .unwrap();
        };
        // Test-unique names: discovery also reads this machine's real ~/.grow dirs.
        let extra = tempfile::tempdir().unwrap();
        write(&extra.path().join("inspect-cfg-extra"), "inspect-cfg-extra");
        write(
            &extra.path().join("inspect-cfg-ignored"),
            "inspect-cfg-ignored",
        );

        let cwd = tempfile::tempdir().unwrap();
        let config = SkillsConfig {
            paths: vec![extra.path().to_string_lossy().into_owned()],
            ignore: vec![
                extra
                    .path()
                    .join("inspect-cfg-ignored")
                    .to_string_lossy()
                    .into_owned(),
            ],
            disabled: vec!["inspect-cfg-extra".to_string()],
            ..Default::default()
        };
        let registry = agent::plugins::PluginRegistry::from_discovered(vec![], &[], &[]);

        let entries = list_skills(cwd.path(), &registry, &config).await;

        let extra_entry = entries
            .iter()
            .find(|e| e.name == "inspect-cfg-extra")
            .expect("[skills].paths skill should be listed");
        assert!(
            matches!(extra_entry.source, ConfigSource::ConfigToml { .. }),
            "unexpected source: {:?}",
            extra_entry.source
        );
        assert!(
            extra_entry.disabled,
            "[skills].disabled must flag the entry"
        );
        assert!(
            !entries.iter().any(|e| e.name == "inspect-cfg-ignored"),
            "[skills].ignore must hide the skill"
        );
    }
}

//! Agent definition file discovery.
//!
//! Searches project agent roots from cwd to repo root, then
//! `~/.grow/agents` (user-level). Path-derived IDs and name-based dedup keep
//! the highest-priority definition.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use tools::types::config_source::ConfigSource;

use crate::config::{AgentDefinition, AgentScope, BuiltinAgentName};
use crate::error::AgentBuildError;

const PROJECT_AGENT_SUBDIR: &str = ".grow/agents";

/// Existing project-level `.grow/agents` directories, walked
/// from `cwd` up to the git worktree root (inclusive). Returns
/// `(existing dirs, git_root)`. Mirrors [`crate::plugins::project_plugin_dirs`].
pub fn project_agent_dirs(cwd: Option<&Path>) -> (Vec<PathBuf>, Option<PathBuf>) {
    let Some(cwd) = cwd else {
        return (Vec::new(), None);
    };
    let chain = crate::repo::RepoDirChain::resolve(cwd);
    (project_agent_dirs_in(&chain.dirs), chain.git_root)
}

/// Existing project `.grow/agents` directories under each
/// dir of a precomputed cwd→git-root chain ([`crate::repo::RepoDirChain`]).
///
/// Single source of the project Agent walk: the folder-trust detector
/// (`repo_configs_present`) reuses its one shared chain here so detection can
/// never drift from discovery (adding a third project-agent dir updates both at
/// once).
pub fn project_agent_dirs_in(chain_dirs: &[PathBuf]) -> Vec<PathBuf> {
    crate::repo::existing_subdirs_along(chain_dirs, &[PROJECT_AGENT_SUBDIR])
}

// ── Subagent entry types ─────────────────────────────────────────────

/// A subagent entry for the Task tool description and spawn-time validation.
#[derive(Debug, Clone)]
pub struct SubagentEntry {
    pub name: String,
    pub description: String,
    pub source: SubagentSource,
    /// If this entry shadows a built-in, which one.
    pub shadows_builtin: Option<BuiltinAgentName>,
    pub config_source: ConfigSource,
}

/// Where a subagent entry came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentSource {
    /// One of the built-in subagent types, not shadowed by a user agent.
    Builtin(BuiltinAgentName),
    /// User-defined agent from project, user, or bundled discovery.
    UserDefined { scope: AgentScope },
}

// ── all_subagents ────────────────────────────────────────────────────

/// Build the complete list of enabled subagents.
///
/// 1. Start with built-in subagent definitions (general-purpose, explore)
/// 2. Discover user-defined agents from project and user roots
/// 3. Merge: project-level user agents shadow built-ins with the same name;
///    user-level and bundled agents with built-in names are skipped (maintains
///    `visible == callable` guarantee)
/// 4. Filter: remove agents toggled off via `[subagents.toggle]`
pub fn all_subagents(cwd: &Path, toggle: &HashMap<String, bool>) -> Vec<SubagentEntry> {
    let grow = config::user_grow_home();
    all_subagents_with_home(cwd, toggle, dirs::home_dir().as_deref(), grow.as_deref())
}

fn all_subagents_with_home(
    cwd: &Path,
    toggle: &HashMap<String, bool>,
    home: Option<&Path>,
    grow_home: Option<&Path>,
) -> Vec<SubagentEntry> {
    let discovered = discover_with_home(cwd, home, grow_home);
    merge_subagents(discovered, toggle)
}

/// Internal: merge discovered agents with built-in subagents, apply toggles.
///
/// Separated from `all_subagents()` for testability without filesystem access.
fn merge_subagents(
    discovered: Vec<AgentDefinition>,
    toggle: &HashMap<String, bool>,
) -> Vec<SubagentEntry> {
    fn discovered_scope_priority(scope: AgentScope) -> usize {
        match scope {
            AgentScope::Project => 3,
            AgentScope::User => 2,
            AgentScope::Bundled => 1,
            AgentScope::BuiltIn => 0,
        }
    }

    // 1. Seed with built-in subagents
    let mut entries: Vec<SubagentEntry> = BuiltinAgentName::subagent_variants()
        .into_iter()
        .map(|b| {
            let def = b.definition();
            SubagentEntry {
                name: def.name,
                description: def.description,
                source: SubagentSource::Builtin(b),
                shadows_builtin: None,
                config_source: ConfigSource::Builtin,
            }
        })
        .collect();

    // 2. Merge in discovered user-defined agents.
    //
    // IMPORTANT: Only project-level agents can shadow built-ins. This matches
    // the runtime spawn precedence in by_name_in_cwd():
    //   project > built-in > user > bundled
    //
    // A user-level ~/.grow/agents/explore.md does NOT shadow built-in explore
    // at spawn time, so it must not shadow it in the visible list either.
    // Otherwise: visible != callable (the guarantee would be broken).
    for def in discovered {
        if def.scope == AgentScope::BuiltIn {
            continue;
        }

        let is_builtin_name = BuiltinAgentName::from_str(&def.name)
            .ok()
            .filter(|b| b.definition().subagent_only);

        if is_builtin_name.is_some() && def.scope != AgentScope::Project {
            // User-level agent has same name as built-in subagent — skip it.
            // It cannot shadow the built-in at runtime, so don't let
            // it shadow in the visible list.
            continue;
        }

        // Check if this name already exists in entries
        if let Some(pos) = entries.iter().position(|e| e.name == def.name) {
            let should_replace = match &entries[pos].source {
                SubagentSource::Builtin(_) => true,
                SubagentSource::UserDefined { scope } => {
                    discovered_scope_priority(def.scope) > discovered_scope_priority(*scope)
                }
            };
            if should_replace {
                let cs = source_from_agent_def(&def);
                entries[pos] = SubagentEntry {
                    name: def.name,
                    description: def.description,
                    source: SubagentSource::UserDefined { scope: def.scope },
                    shadows_builtin: is_builtin_name,
                    config_source: cs,
                };
            }
        } else {
            // New unique name — append after built-ins
            let cs = source_from_agent_def(&def);
            entries.push(SubagentEntry {
                name: def.name,
                description: def.description,
                source: SubagentSource::UserDefined { scope: def.scope },
                shadows_builtin: None,
                config_source: cs,
            });
        }
    }

    // 3. Filter by toggle (omitted = enabled)
    entries
        .into_iter()
        .filter(|e| toggle.get(&e.name).copied().unwrap_or(true))
        .collect()
}

/// Discover all agent definitions from the filesystem.
///
/// Search order (highest priority first):
/// 1. `.grow/agents/` walking from `cwd` up to repo root
/// 2. `~/.grow/agents/` (user-level)
///
/// Deduplicates by name — higher-priority definitions win.
/// User-level Agent directories. `grow_home` is intentionally ignored:
/// Agent/Skill resources share stable user roots independent of the
/// runtime/config home.
pub(crate) fn user_agent_dirs(
    home: Option<&Path>,
    _grow_home: Option<&Path>,
) -> Vec<(std::path::PathBuf, AgentScope)> {
    crate::resource_roots::user_subdirs(home, "agents")
        .into_iter()
        .map(|path| (path, AgentScope::User))
        .collect()
}

pub fn discover(cwd: &Path) -> Vec<AgentDefinition> {
    let grow = config::user_grow_home();
    discover_with_home(cwd, dirs::home_dir().as_deref(), grow.as_deref())
}

/// File-defined Agents that are implicitly presented as primary profiles but
/// do not satisfy the primary capability floor. Explicit `subagentOnly`
/// definitions are intentionally omitted: their restricted capability is
/// valid and already reflected in picker visibility.
pub fn invalid_primary_agent_definitions(cwd: &Path) -> Vec<AgentDefinition> {
    discover(cwd)
        .into_iter()
        .filter(|definition| !definition.subagent_only && !definition.is_primary_agent_eligible())
        .collect()
}

fn discover_with_home(
    cwd: &Path,
    home: Option<&Path>,
    grow_home: Option<&Path>,
) -> Vec<AgentDefinition> {
    let mut definitions = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    load_project_definitions(cwd, &mut definitions, &mut seen_names);

    for (dir, scope) in user_agent_dirs(home, grow_home) {
        if dir.is_dir() {
            load_definitions_from_dir(&dir, scope, &mut definitions, &mut seen_names);
        }
    }

    definitions
}

/// Find an agent definition by name.
///
/// Checks built-ins first, then the ordered user roots.
pub fn by_name(name: &str) -> Option<AgentDefinition> {
    let grow = config::user_grow_home();
    by_name_with_home(name, dirs::home_dir().as_deref(), grow.as_deref())
}

fn by_name_with_home(
    name: &str,
    home: Option<&Path>,
    grow_home: Option<&Path>,
) -> Option<AgentDefinition> {
    // Check built-ins first — type-safe via BuiltinAgentName strum enum
    if let Ok(builtin) = BuiltinAgentName::from_str(name) {
        return Some(builtin.definition());
    }

    {
        let home_dirs = user_agent_dirs(home, grow_home);
        for (agents_dir, scope) in home_dirs {
            if let Some(def) = load_definition_by_name(
                &agents_dir,
                name,
                "Failed to parse agent definition",
                Some(scope),
            ) {
                return Some(def);
            }
        }
    }

    None
}

/// Find an agent definition by name, with project-level discovery.
///
/// Project-level `.grow/agents/` has highest priority, then falls back
/// to built-ins and the ordered user roots.
pub fn by_name_in_cwd(name: &str, cwd: &Path) -> Option<AgentDefinition> {
    let grow = config::user_grow_home();
    by_name_in_cwd_with_home(name, cwd, dirs::home_dir().as_deref(), grow.as_deref())
}

fn by_name_in_cwd_with_home(
    name: &str,
    cwd: &Path,
    home: Option<&Path>,
    grow_home: Option<&Path>,
) -> Option<AgentDefinition> {
    if let Some(def) = load_project_definition_by_name(name, cwd) {
        return Some(def);
    }

    by_name_with_home(name, home, grow_home)
}

/// Return all built-in subagent definitions.
///
/// These are the pre-defined agent profiles that can be launched via the
/// Task tool. User/project-level agent files can shadow these by name.
///
/// The list covers the core built-in agents:
/// - `general-purpose` — bounded multi-step work with its resolved tools
/// - `explore` — focused read-only workspace investigation
pub fn builtin_subagents() -> Vec<AgentDefinition> {
    BuiltinAgentName::subagent_variants()
        .into_iter()
        .map(|name| name.definition())
        .collect()
}

/// Return every built-in agent definition (all `BuiltinAgentName` variants, not
/// just the subagent-launchable subset in [`builtin_subagents`]).
///
/// Introspection helper for cross-crate coverage/manifest checks that must
/// enumerate all builtins from another crate that pins a different `strum`
/// than this crate's `BuiltinAgentName` derives and so cannot call
/// `BuiltinAgentName::iter()` itself.
pub fn all_builtin_agent_definitions() -> Vec<AgentDefinition> {
    use strum::IntoEnumIterator;
    BuiltinAgentName::iter()
        .map(BuiltinAgentName::definition)
        .collect()
}

/// Parse only YAML frontmatter from an agent file, without loading the body.
pub fn parse_agent_frontmatter_only(path: &Path) -> Result<AgentDefinition, AgentBuildError> {
    AgentDefinition::from_file_frontmatter_only(path)
}

fn source_from_agent_def(def: &AgentDefinition) -> ConfigSource {
    let path = def.source_path.clone().unwrap_or_default();
    if let Some(ref pn) = def.plugin_name {
        ConfigSource::Plugin {
            plugin_name: pn.clone(),
            path,
        }
    } else {
        match def.scope {
            AgentScope::Project => ConfigSource::Project { path },
            AgentScope::User => ConfigSource::User { path },
            AgentScope::Bundled | AgentScope::BuiltIn => ConfigSource::Builtin,
        }
    }
}

// ── Plugin-aware variants ─────────────────────────────────────────────

/// Build the complete list of enabled subagents, including plugin agents.
pub fn all_subagents_with_plugins(
    cwd: &Path,
    toggle: &HashMap<String, bool>,
    plugins: Option<&crate::plugins::PluginRegistry>,
) -> Vec<SubagentEntry> {
    let grow = config::user_grow_home();
    all_subagents_with_plugins_and_home(
        cwd,
        toggle,
        plugins,
        dirs::home_dir().as_deref(),
        grow.as_deref(),
    )
}

fn all_subagents_with_plugins_and_home(
    cwd: &Path,
    toggle: &HashMap<String, bool>,
    plugins: Option<&crate::plugins::PluginRegistry>,
    home: Option<&Path>,
    grow_home: Option<&Path>,
) -> Vec<SubagentEntry> {
    let discovered = discover_with_home(cwd, home, grow_home);
    let mut entries = merge_subagents(discovered, toggle);

    // Append plugin agents under qualified names
    if let Some(registry) = plugins {
        for plugin in registry.enabled_plugins() {
            for agent_dir in &plugin.agent_dirs {
                if !agent_dir.is_dir() {
                    continue;
                }
                let agent_entries = match std::fs::read_dir(agent_dir) {
                    Ok(entries) => entries,
                    Err(_) => continue,
                };
                for entry in agent_entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("md") {
                        continue;
                    }
                    let Some(def) = load_plugin_agent_definition(plugin, &path) else {
                        continue;
                    };

                    let qualified_name = format!("{}:{}", plugin.name, def.name);

                    // Skip if a native entry already has this qualified name
                    if entries.iter().any(|e| e.name == qualified_name) {
                        continue;
                    }

                    // Map plugin scope to agent scope
                    let agent_scope = match plugin.scope {
                        crate::plugins::PluginScope::Project => AgentScope::Project,
                        crate::plugins::PluginScope::User => AgentScope::User,
                        _ => AgentScope::User,
                    };

                    let config_source = ConfigSource::Plugin {
                        plugin_name: plugin.name.clone(),
                        path: path.clone(),
                    };
                    entries.push(SubagentEntry {
                        name: qualified_name,
                        description: def.description,
                        source: SubagentSource::UserDefined { scope: agent_scope },
                        shadows_builtin: None,
                        config_source,
                    });
                }
            }
        }
    }

    entries
}

/// Find an agent definition by name, with plugin support.
///
/// Checks project-level, built-ins, user-level, bundled, then plugin agents.
/// For plugin agents, the name can be qualified (e.g. `my-plugin:reviewer`).
pub fn by_name_in_cwd_with_plugins(
    name: &str,
    cwd: &Path,
    plugins: Option<&crate::plugins::PluginRegistry>,
) -> Option<AgentDefinition> {
    let grow = config::user_grow_home();
    by_name_in_cwd_with_plugins_and_home(
        name,
        cwd,
        plugins,
        dirs::home_dir().as_deref(),
        grow.as_deref(),
    )
}

fn by_name_in_cwd_with_plugins_and_home(
    name: &str,
    cwd: &Path,
    plugins: Option<&crate::plugins::PluginRegistry>,
    home: Option<&Path>,
    grow_home: Option<&Path>,
) -> Option<AgentDefinition> {
    // First try native resolution (project > built-in > user > bundled)
    if let Some(def) = by_name_in_cwd_with_home(name, cwd, home, grow_home) {
        return Some(def);
    }

    // Try plugin agents
    if let Some(registry) = plugins {
        // Check if name is qualified (plugin-name:agent-name)
        if let Some((plugin_name, agent_name)) = name.split_once(':')
            && let Some(plugin) = registry.get(plugin_name)
            && plugin.enabled
        {
            for agent_dir in &plugin.agent_dirs {
                let agent_file = agent_dir.join(format!("{agent_name}.md"));
                if agent_file.is_file()
                    && let Some(mut def) = load_plugin_agent_definition(plugin, &agent_file)
                {
                    substitute_plugin_vars(&mut def, plugin);
                    return Some(def);
                }
            }
        }

        // Bare name lookup: only resolve if exactly one plugin has this agent.
        // Ambiguous matches (multiple plugins with same agent name) are rejected.
        let mut matches: Vec<(&crate::plugins::registry::LoadedPlugin, std::path::PathBuf)> =
            Vec::new();
        for plugin in registry.enabled_plugins() {
            for agent_dir in &plugin.agent_dirs {
                let agent_file = agent_dir.join(format!("{name}.md"));
                if agent_file.is_file() {
                    matches.push((plugin, agent_file));
                }
            }
        }
        if matches.len() == 1 {
            let (plugin, agent_file) = &matches[0];
            if let Some(mut def) = load_plugin_agent_definition(plugin, agent_file) {
                substitute_plugin_vars(&mut def, plugin);
                return Some(def);
            }
        } else if matches.len() > 1 {
            let plugin_names: Vec<&str> = matches.iter().map(|(p, _)| p.name.as_str()).collect();
            tracing::warn!(
                agent_name = name,
                plugins = ?plugin_names,
                "ambiguous bare agent name matches multiple plugins; use qualified name (plugin:agent)"
            );
        }
    }

    None
}

/// Load one plugin-provided agent file, tagged with its owning plugin.
///
/// Untrusted plugins are parsed frontmatter-only so their prompt body never
/// reaches the model before the plugin is trusted. A parse failure drops the
/// agent from discovery entirely, so it is logged rather than swallowed.
fn load_plugin_agent_definition(
    plugin: &crate::plugins::LoadedPlugin,
    path: &Path,
) -> Option<AgentDefinition> {
    let loaded = if plugin.trusted {
        AgentDefinition::from_file(path)
    } else {
        AgentDefinition::from_file_frontmatter_only(path)
    };
    match loaded {
        Ok(mut def) => {
            def.plugin_name = Some(plugin.name.clone());
            Some(def)
        }
        Err(e) => {
            tracing::warn!(
                plugin = %plugin.name,
                path = %path.display(),
                error = %e,
                "Failed to parse plugin agent definition, skipping"
            );
            None
        }
    }
}

/// Expand `${GROW_PLUGIN_ROOT}` / `${GROW_PLUGIN_DATA}` in a plugin agent's
/// body so the model receives absolute paths,
/// matching the expected load-time resolution for these variables.
fn substitute_plugin_vars(def: &mut AgentDefinition, plugin: &crate::plugins::LoadedPlugin) {
    // Untrusted plugins are loaded frontmatter-only (body is None). Skip
    // computing root/data paths when there is no Agent role to expand.
    if def.prompt_body.is_none() {
        return;
    }
    let (root, data) = (plugin.root_str(), plugin.data_dir_str());
    if let Some(body) = def.prompt_body.take() {
        def.prompt_body = Some(crate::plugins::manifest::substitute_env_vars(
            &body, &root, &data,
        ));
    }
}

/// Load project Agent definitions from every `.grow/agents` directory along
/// the cwd→git-root walk, via the shared [`project_agent_dirs`] SSOT.
fn load_project_definitions(
    cwd: &Path,
    definitions: &mut Vec<AgentDefinition>,
    seen_names: &mut std::collections::HashSet<String>,
) {
    for agents_dir in project_agent_dirs(Some(cwd)).0 {
        load_definitions_from_dir(&agents_dir, AgentScope::Project, definitions, seen_names);
    }
}

/// First project agent named `name` along the cwd→git-root walk (the shared
/// [`project_agent_dirs`] SSOT), highest-priority dir first.
fn load_project_definition_by_name(name: &str, cwd: &Path) -> Option<AgentDefinition> {
    for agents_dir in project_agent_dirs(Some(cwd)).0 {
        let agent_file = agents_dir.join(format!("{name}.md"));
        if let Some(def) = load_definition_from_path(
            &agent_file,
            name,
            "Failed to parse project agent definition",
            Some(AgentScope::Project),
        ) {
            return Some(def);
        }
    }
    None
}

fn load_definition_by_name(
    dir: &Path,
    name: &str,
    error_message: &str,
    scope_override: Option<AgentScope>,
) -> Option<AgentDefinition> {
    let agent_file = dir.join(format!("{}.md", name));
    load_definition_from_path(&agent_file, name, error_message, scope_override)
}

fn load_definition_from_path(
    agent_file: &Path,
    name: &str,
    error_message: &str,
    scope_override: Option<AgentScope>,
) -> Option<AgentDefinition> {
    if !agent_file.is_file() {
        return None;
    }

    match AgentDefinition::from_file(agent_file) {
        Ok(mut def) => {
            def.name = name.to_owned();
            if let Some(scope) = scope_override {
                def.scope = scope;
            }
            Some(def)
        }
        Err(e) => {
            tracing::warn!(
                name = name,
                path = %agent_file.display(),
                error = %e,
                context = error_message,
                "Failed to parse agent definition"
            );
            None
        }
    }
}

fn load_definitions_from_dir(
    dir: &Path,
    scope: AgentScope,
    definitions: &mut Vec<AgentDefinition>,
    seen_names: &mut std::collections::HashSet<String>,
) {
    for path in agent_markdown_files(dir) {
        let Some(name) = agent_id_from_path(dir, &path) else {
            continue;
        };

        match AgentDefinition::from_file(&path) {
            Ok(mut def) => {
                def.name = name;
                def.scope = scope;
                // Dedup by name — first occurrence (highest priority) wins
                if seen_names.insert(def.name.clone()) {
                    definitions.push(def);
                }
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "Failed to parse agent definition, skipping"
                );
            }
        }
    }
}

/// Recursively collect Markdown definitions in stable path order. A nested
/// relative path is the Agent ID namespace.
fn agent_markdown_files(dir: &Path) -> Vec<PathBuf> {
    fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                visit(&path, out);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }

    let mut files = Vec::new();
    visit(dir, &mut files);
    files
}

fn agent_id_from_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?.with_extension("");
    let parts: Vec<&str> = relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<_>>()?;
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::discovery::{PluginId, PluginScope};
    use crate::plugins::manifest::PluginManifest;
    use crate::plugins::{LoadedPlugin, PluginRegistry};
    use std::fs;
    use std::path::PathBuf;

    fn test_origin_for_scope(scope: PluginScope) -> crate::plugins::PluginOrigin {
        use crate::plugins::PluginOrigin;
        match scope {
            PluginScope::CliOverride => PluginOrigin::CliOverride,
            PluginScope::Project => PluginOrigin::ProjectGrow,
            PluginScope::User => PluginOrigin::UserGrow,
            PluginScope::ConfigPath => PluginOrigin::ConfigPath,
        }
    }

    /// Helper: create a valid agent .md file
    fn write_agent_file(dir: &std::path::Path, filename: &str, name: &str, desc: &str) {
        let content = format!("---\nname: {name}\ndescription: {desc}\n---\n");
        fs::write(dir.join(filename), content).unwrap();
    }

    fn make_plugin_registry(
        plugin_name: &str,
        scope: PluginScope,
        agent_dirs: Vec<PathBuf>,
    ) -> PluginRegistry {
        let root = PathBuf::from(format!("/tmp/{plugin_name}"));
        let loaded = LoadedPlugin {
            name: plugin_name.to_string(),
            id: PluginId::new(scope, &root, plugin_name),
            root: root.clone(),
            canonical_root: root.clone(),
            scope,
            origin: test_origin_for_scope(scope),
            trusted: true,
            enabled: true,
            version: Some("1.0.0".to_string()),
            description: Some(format!("Plugin {plugin_name}")),
            skill_dirs: vec![],
            command_dirs: vec![],
            agent_dirs,
            hooks_path: None,
            mcp_config_path: None,
            skill_count: 0,
            agent_count: 0,
            skill_names: vec![],
            agent_names: vec![],
            has_hooks: false,
            hook_count: 0,
            has_inline_hooks_only: false,
            lsp_config_path: None,
            mcp_server_count: 0,
            has_inline_mcp_only: false,
            lsp_server_count: 0,
            has_inline_lsp_only: false,
            inline_hooks: None,
            inline_mcp_servers: None,
            inline_lsp_servers: None,
            conflict: None,
        };

        let LoadedPlugin { agent_dirs, .. } = loaded;
        let discovered = crate::plugins::DiscoveredPlugin {
            manifest: PluginManifest {
                name: plugin_name.to_string(),
                version: Some("1.0.0".to_string()),
                description: Some(format!("Plugin {plugin_name}")),
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
            },
            id: PluginId::new(scope, &root, plugin_name),
            root: root.clone(),
            canonical_root: root,
            scope,
            origin: test_origin_for_scope(scope),
            trusted: true,
            skill_dirs: vec![],
            command_dirs: vec![],
            agent_dirs,
            hooks_path: None,
            mcp_config_path: None,
            lsp_config_path: None,
            conflict: None,
        };
        PluginRegistry::from_discovered(vec![discovered], &[], &[plugin_name.to_string()])
    }

    #[test]
    fn user_agent_dirs_use_grow_only() {
        let home = Path::new("/home/u");
        let grow = Path::new("/custom/growhome");
        let paths: Vec<_> = user_agent_dirs(Some(home), Some(grow))
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        assert_eq!(paths, vec![home.join(".grow/agents")]);
    }

    #[test]
    fn test_by_name_unknown_agent_is_not_builtin() {
        // Arbitrary names are not built-ins; should return None unless a
        // project/user-level agent file exists with that name.
        let def = by_name("not-a-builtin-agent");
        assert!(def.is_none());
    }

    #[test]
    fn test_by_name_builtin_browser_use() {
        let def = by_name("browser-use");
        assert!(def.is_some());
        assert_eq!(def.unwrap().name, "browser-use");
    }

    #[test]
    fn test_by_name_builtin_grow() {
        let def = by_name("grow");
        assert!(def.is_some());
        assert_eq!(def.unwrap().name, "grow");
    }

    #[test]
    fn test_by_name_unknown_returns_none() {
        let def = by_name("nonexistent-agent-xyz");
        assert!(def.is_none());
    }

    #[test]
    fn test_discover_finds_md_files() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".grow").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        write_agent_file(&agents_dir, "test-agent.md", "test-agent", "A test");
        write_agent_file(&agents_dir, "another.md", "another", "Another");

        let defs = discover_with_home(tmp.path(), None, None);
        assert_eq!(defs.len(), 2);
        let names: Vec<_> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"test-agent"));
        assert!(names.contains(&"another"));
    }

    #[test]
    fn test_discover_ignores_non_md_files() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".grow").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        write_agent_file(&agents_dir, "valid.md", "valid", "Valid agent");
        fs::write(agents_dir.join("readme.txt"), "not an agent").unwrap();
        fs::write(agents_dir.join("config.yaml"), "key: value").unwrap();

        let defs = discover_with_home(tmp.path(), None, None);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "valid");
    }

    #[test]
    fn test_discover_invalid_md_logged_not_error() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".grow").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        write_agent_file(&agents_dir, "good.md", "good", "Good agent");
        // Invalid: no frontmatter
        fs::write(agents_dir.join("bad.md"), "just text, no frontmatter").unwrap();

        let defs = discover_with_home(tmp.path(), None, None);
        // Should still find the good one, skip the bad one
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "good");
    }

    #[test]
    fn test_discover_dedup_by_name() {
        let tmp = tempfile::tempdir().unwrap();

        // Create two directories with same-named agents at different levels
        let inner_dir = tmp.path().join("subdir");
        fs::create_dir_all(&inner_dir).unwrap();

        let agents_dir_1 = tmp.path().join(".grow").join("agents");
        let agents_dir_2 = inner_dir.join(".grow").join("agents");
        fs::create_dir_all(&agents_dir_1).unwrap();
        fs::create_dir_all(&agents_dir_2).unwrap();

        write_agent_file(&agents_dir_1, "dup.md", "dup", "Parent version");
        write_agent_file(&agents_dir_2, "dup.md", "dup", "Child version");

        // Discover from the inner dir — inner should win (discovered first)
        let defs = discover_with_home(&inner_dir, None, None);
        let dup_defs: Vec<_> = defs.iter().filter(|d| d.name == "dup").collect();
        assert_eq!(dup_defs.len(), 1, "Should dedup by name");
    }

    #[test]
    fn user_level_loads_agents_not_legacy_agent_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("workspace");
        let home = tmp.path().join("home");
        let grow = home.join(".grow/agents");
        let legacy = home.join(".agent/agents");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&grow).unwrap();
        fs::create_dir_all(&legacy).unwrap();
        write_agent_file(&grow, "reviewer.md", "ignored", "Grow reviewer");
        write_agent_file(&legacy, "reviewer.md", "ignored", "Legacy reviewer");
        write_agent_file(&legacy, "writer.md", "ignored", "Legacy writer");

        let defs = discover_with_home(&cwd, Some(&home), None);
        assert_eq!(
            defs.iter()
                .find(|def| def.name == "reviewer")
                .unwrap()
                .description,
            "Grow reviewer"
        );
        assert!(
            defs.iter().all(|def| def.name != "writer"),
            "~/.agent must not be scanned: {defs:?}"
        );
    }

    #[test]
    fn recursive_agent_id_comes_from_relative_path() {
        let tmp = tempfile::tempdir().unwrap();
        let agents = tmp.path().join(".grow/agents/review");
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("security.md"),
            "---\ndescription: Security reviewer\ncolor: cyan\n---\nReview carefully.\n",
        )
        .unwrap();

        let defs = discover_with_home(tmp.path(), None, None);
        let def = defs
            .iter()
            .find(|def| def.name == "review/security")
            .unwrap();
        assert_eq!(def.description, "Security reviewer");
        assert_eq!(def.color, Some(crate::config::AgentColor::Cyan));
        assert_eq!(def.prompt_body.as_deref(), Some("Review carefully."));
    }

    #[test]
    fn test_by_name_in_cwd_project_shadows_builtin() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".grow").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        // Create a project-level "grow" that shadows the built-in
        write_agent_file(&agents_dir, "grow.md", "grow", "Custom grow");

        let def = by_name_in_cwd("grow", tmp.path());
        assert!(def.is_some());
        let def = def.unwrap();
        assert_eq!(def.name, "grow");
        assert_eq!(def.description, "Custom grow");
    }

    #[test]
    fn test_by_name_in_cwd_falls_back_to_builtin() {
        let tmp = tempfile::tempdir().unwrap();
        // No .grow/agents/ directory — should fall back to built-in

        let def = by_name_in_cwd("grow", tmp.path());
        assert!(def.is_some());
        let def = def.unwrap();
        assert_eq!(def.name, "grow");
        // Should be the built-in, not a custom one
        assert_eq!(def.scope, AgentScope::BuiltIn);
    }

    // ── all_subagents / merge_subagents tests ───────────────────────

    /// Helper: build a minimal synthetic AgentDefinition for testing merge logic.
    fn synthetic_agent(name: &str, desc: &str, scope: AgentScope) -> AgentDefinition {
        AgentDefinition {
            name: name.to_string(),
            description: desc.to_string(),
            scope,
            agents_md: false,
            ..AgentDefinition::general_purpose()
        }
    }

    #[test]
    fn test_merge_returns_3_builtins_when_no_user_agents() {
        let entries = merge_subagents(vec![], &HashMap::new());
        assert_eq!(entries.len(), 3);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"general-purpose"));
        assert!(names.contains(&"explore"));
        assert!(names.contains(&"browser-use"));
        // All should be Builtin source
        for entry in &entries {
            assert!(
                matches!(&entry.source, SubagentSource::Builtin(_)),
                "expected Builtin source for '{}'",
                entry.name
            );
            assert!(entry.shadows_builtin.is_none());
        }
    }

    #[test]
    fn test_merge_filters_toggled_off_builtins() {
        let toggle = HashMap::from([("explore".to_string(), false)]);
        let entries = merge_subagents(vec![], &toggle);
        assert_eq!(entries.len(), 2);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"general-purpose"));
        assert!(names.contains(&"browser-use"));
        assert!(!names.contains(&"explore"));
    }

    #[test]
    fn test_merge_includes_user_defined_agents() {
        let discovered = vec![synthetic_agent(
            "code-reviewer",
            "Reviews code",
            AgentScope::Project,
        )];
        let entries = merge_subagents(discovered, &HashMap::new());
        assert_eq!(entries.len(), 4); // 3 built-ins + 1 user
        let cr = entries.iter().find(|e| e.name == "code-reviewer").unwrap();
        assert_eq!(cr.description, "Reviews code");
        assert_eq!(
            cr.source,
            SubagentSource::UserDefined {
                scope: AgentScope::Project
            }
        );
        assert!(cr.shadows_builtin.is_none());
    }

    #[test]
    fn test_merge_filters_toggled_off_user_agents() {
        let discovered = vec![synthetic_agent(
            "code-reviewer",
            "Reviews code",
            AgentScope::Project,
        )];
        let toggle = HashMap::from([("code-reviewer".to_string(), false)]);
        let entries = merge_subagents(discovered, &toggle);
        assert_eq!(entries.len(), 3); // only built-ins
        assert!(entries.iter().all(|e| e.name != "code-reviewer"));
    }

    #[test]
    fn test_merge_project_agent_shadows_builtin() {
        let discovered = vec![synthetic_agent(
            "explore",
            "Custom explore agent",
            AgentScope::Project,
        )];
        let entries = merge_subagents(discovered, &HashMap::new());
        assert_eq!(entries.len(), 3); // still 3 — replaced, not appended
        let explore = entries.iter().find(|e| e.name == "explore").unwrap();
        assert_eq!(explore.description, "Custom explore agent");
        assert_eq!(
            explore.source,
            SubagentSource::UserDefined {
                scope: AgentScope::Project
            }
        );
    }

    #[test]
    fn test_merge_shadowed_entry_has_shadows_builtin() {
        let discovered = vec![synthetic_agent(
            "explore",
            "Custom explore",
            AgentScope::Project,
        )];
        let entries = merge_subagents(discovered, &HashMap::new());
        let explore = entries.iter().find(|e| e.name == "explore").unwrap();
        assert_eq!(
            explore.shadows_builtin,
            Some(BuiltinAgentName::Explore),
            "shadowed explore should record shadows_builtin"
        );
    }

    #[test]
    fn test_merge_user_level_builtin_name_is_skipped() {
        // A user-level (~/.grow/agents/) agent named "explore" should NOT shadow
        // the built-in — only project-level can do that.
        let discovered = vec![synthetic_agent(
            "explore",
            "User-level explore",
            AgentScope::User,
        )];
        let entries = merge_subagents(discovered, &HashMap::new());
        assert_eq!(entries.len(), 3); // still 3 built-ins
        let explore = entries.iter().find(|e| e.name == "explore").unwrap();
        // Should still be the built-in, not the user-level agent
        assert!(
            matches!(
                &explore.source,
                SubagentSource::Builtin(BuiltinAgentName::Explore)
            ),
            "user-level explore should not shadow built-in"
        );
    }

    #[test]
    fn test_merge_bundled_builtin_name_is_skipped() {
        let discovered = vec![synthetic_agent(
            "explore",
            "Bundled explore",
            AgentScope::Bundled,
        )];
        let entries = merge_subagents(discovered, &HashMap::new());
        let explore = entries.iter().find(|e| e.name == "explore").unwrap();
        assert!(matches!(
            &explore.source,
            SubagentSource::Builtin(BuiltinAgentName::Explore)
        ));
    }

    #[test]
    fn test_merge_user_unique_name_appended() {
        let discovered = vec![synthetic_agent(
            "migration-helper",
            "Helps with migrations",
            AgentScope::User,
        )];
        let entries = merge_subagents(discovered, &HashMap::new());
        assert_eq!(entries.len(), 4); // 3 built-ins + 1 user
        // Verify ordering: built-ins first, then user
        assert!(matches!(&entries[0].source, SubagentSource::Builtin(_)));
        assert!(matches!(&entries[1].source, SubagentSource::Builtin(_)));
        assert!(matches!(&entries[2].source, SubagentSource::Builtin(_)));
        assert_eq!(entries[3].name, "migration-helper");
        assert_eq!(
            entries[3].source,
            SubagentSource::UserDefined {
                scope: AgentScope::User
            }
        );
    }

    #[test]
    fn test_merge_bundled_unique_name_appended() {
        let discovered = vec![synthetic_agent(
            "bundled-helper",
            "Helps from bundle",
            AgentScope::Bundled,
        )];
        let entries = merge_subagents(discovered, &HashMap::new());
        assert_eq!(entries[3].name, "bundled-helper");
        assert_eq!(
            entries[3].source,
            SubagentSource::UserDefined {
                scope: AgentScope::Bundled
            }
        );
    }

    #[test]
    fn test_merge_all_toggled_off_returns_empty() {
        let toggle = HashMap::from([
            ("general-purpose".to_string(), false),
            ("explore".to_string(), false),
            ("browser-use".to_string(), false),
        ]);
        let entries = merge_subagents(vec![], &toggle);
        assert!(entries.is_empty(), "all toggled off should return empty");
    }

    #[test]
    fn test_merge_invalid_user_agent_preserves_builtin() {
        // Simulate: discover() skips invalid files (returns empty for that file).
        // So if a user's explore.md is invalid, discover() won't include it,
        // and the built-in explore remains.
        let discovered = vec![]; // no valid user agents discovered
        let entries = merge_subagents(discovered, &HashMap::new());
        assert_eq!(entries.len(), 3);
        let explore = entries.iter().find(|e| e.name == "explore").unwrap();
        assert!(matches!(
            &explore.source,
            SubagentSource::Builtin(BuiltinAgentName::Explore)
        ));
    }

    #[test]
    fn test_merge_project_user_duplicate_project_wins() {
        let discovered = vec![
            synthetic_agent("my-agent", "Project version", AgentScope::Project),
            synthetic_agent("my-agent", "User version", AgentScope::User),
        ];
        let entries = merge_subagents(discovered, &HashMap::new());
        let my_agent: Vec<_> = entries.iter().filter(|e| e.name == "my-agent").collect();
        assert_eq!(my_agent.len(), 1, "should dedup by name");
        assert_eq!(
            my_agent[0].source,
            SubagentSource::UserDefined {
                scope: AgentScope::Project
            }
        );
    }

    #[test]
    fn test_merge_user_beats_bundled_for_visible_equals_callable() {
        let discovered = vec![
            synthetic_agent("reviewer", "Bundled reviewer", AgentScope::Bundled),
            synthetic_agent("reviewer", "User reviewer", AgentScope::User),
        ];
        let entries = merge_subagents(discovered, &HashMap::new());
        let reviewer = entries.iter().find(|e| e.name == "reviewer").unwrap();
        assert_eq!(reviewer.description, "User reviewer");
        assert_eq!(
            reviewer.source,
            SubagentSource::UserDefined {
                scope: AgentScope::User
            }
        );
    }

    #[test]
    fn test_all_subagents_with_project_agent_file() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".grow").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        write_agent_file(
            &agents_dir,
            "test-agent.md",
            "test-agent",
            "A test subagent",
        );

        let entries = all_subagents_with_home(tmp.path(), &HashMap::new(), None, None);
        assert_eq!(entries.len(), 4);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["general-purpose", "explore", "browser-use", "test-agent"]
        );
    }

    #[test]
    fn test_all_subagents_with_plugins_preserves_native_visibility() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("workspace");
        let home = tmp.path().join("home");
        let user_dir = home.join(".grow/agents");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&user_dir).unwrap();

        write_agent_file(&user_dir, "reviewer.md", "reviewer", "User reviewer");

        let plugin_root = tempfile::tempdir().unwrap();
        let plugin_agents = plugin_root.path().join("agents");
        fs::create_dir_all(&plugin_agents).unwrap();
        write_agent_file(&plugin_agents, "reviewer.md", "reviewer", "Plugin reviewer");

        let registry = make_plugin_registry("plugin-one", PluginScope::User, vec![plugin_agents]);
        let entries = all_subagents_with_plugins_and_home(
            &cwd,
            &HashMap::new(),
            Some(&registry),
            Some(&home),
            Some(&home.join(".grow")),
        );

        let native = entries.iter().find(|e| e.name == "reviewer").unwrap();
        assert_eq!(native.description, "User reviewer");
        assert_eq!(
            native.source,
            SubagentSource::UserDefined {
                scope: AgentScope::User
            }
        );
        assert!(entries.iter().any(|e| e.name == "plugin-one:reviewer"));
    }

    #[test]
    fn plugin_agent_with_unknown_color_keeps_the_agent_without_an_accent() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("workspace");
        let home = tmp.path().join("home");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&home).unwrap();

        let plugin_root = tempfile::tempdir().unwrap();
        let plugin_agents = plugin_root.path().join("agents");
        fs::create_dir_all(&plugin_agents).unwrap();
        fs::write(
            plugin_agents.join("painter.md"),
            "---\nname: painter\ndescription: Plugin painter\ncolor: chartreuse\n---\nBody.\n",
        )
        .unwrap();

        let registry = make_plugin_registry("plugin-one", PluginScope::User, vec![plugin_agents]);
        let entries = all_subagents_with_plugins_and_home(
            &cwd,
            &HashMap::new(),
            Some(&registry),
            Some(&home),
            Some(&home.join(".grow")),
        );
        assert!(entries.iter().any(|entry| entry.name == "plugin-one:painter"));

        let definition = by_name_in_cwd_with_plugins_and_home(
            "plugin-one:painter",
            &cwd,
            Some(&registry),
            Some(&home),
            Some(&home.join(".grow")),
        )
        .expect("Agent lookup should ignore an unknown presentation color");
        assert_eq!(definition.color, None);
    }

    #[test]
    fn test_by_name_in_cwd_with_plugins_prefers_native_over_plugin_bare_name() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("workspace");
        let home = tmp.path().join("home");
        let user_dir = home.join(".grow/agents");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&user_dir).unwrap();
        write_agent_file(&user_dir, "reviewer.md", "reviewer", "User reviewer");

        let plugin_root = tempfile::tempdir().unwrap();
        let plugin_agents = plugin_root.path().join("agents");
        fs::create_dir_all(&plugin_agents).unwrap();
        write_agent_file(&plugin_agents, "reviewer.md", "reviewer", "Plugin reviewer");

        let registry = make_plugin_registry("plugin-one", PluginScope::User, vec![plugin_agents]);
        let def = by_name_in_cwd_with_plugins_and_home(
            "reviewer",
            &cwd,
            Some(&registry),
            Some(&home),
            Some(&home.join(".grow")),
        )
        .unwrap();

        assert_eq!(def.scope, AgentScope::User);
        assert_eq!(def.description, "User reviewer");
        assert!(def.plugin_name.is_none());
    }

    #[test]
    fn test_plugin_agent_body_resolves_plugin_root() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("workspace");
        let home = tmp.path().join("home");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&home).unwrap();

        let plugin_dir = tempfile::tempdir().unwrap();
        let plugin_agents = plugin_dir.path().join("agents");
        fs::create_dir_all(&plugin_agents).unwrap();
        // Mirrors how enterprise plugin-dev agents reference the plugin root.
        let content = "---\nname: runner\ndescription: Runs a tool\n---\n\
            Run python3 \"${GROW_PLUGIN_ROOT}/tools/x.py\"\n";
        fs::write(plugin_agents.join("runner.md"), content).unwrap();

        let registry = make_plugin_registry("plugin-one", PluginScope::User, vec![plugin_agents]);
        let expected_root = registry.get("plugin-one").unwrap().root_str();
        let resolved = format!("{expected_root}/tools/x.py");

        // Bare-name resolution path.
        let bare = by_name_in_cwd_with_plugins_and_home(
            "runner",
            &cwd,
            Some(&registry),
            Some(&home),
            Some(&home.join(".grow")),
        )
        .unwrap();
        let bare_body = bare.prompt_body.as_deref().unwrap();
        assert!(
            bare_body.contains(&resolved),
            "expected resolved root in: {bare_body}"
        );
        assert!(
            !bare_body.contains("${GROW_PLUGIN_ROOT}"),
            "literal token must be gone: {bare_body}"
        );

        // Qualified-name resolution path.
        let qualified = by_name_in_cwd_with_plugins_and_home(
            "plugin-one:runner",
            &cwd,
            Some(&registry),
            Some(&home),
            Some(&home.join(".grow")),
        )
        .unwrap();
        let qualified_body = qualified.prompt_body.as_deref().unwrap();
        assert!(
            qualified_body.contains(&resolved),
            "expected resolved root in: {qualified_body}"
        );
        assert!(!qualified_body.contains("${GROW_PLUGIN_ROOT}"));
    }

    #[test]
    fn test_all_subagents_toggle_filters_project_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let agents_dir = tmp.path().join(".grow").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();

        write_agent_file(
            &agents_dir,
            "test-agent.md",
            "test-agent",
            "A test subagent",
        );

        let toggle = HashMap::from([("test-agent".to_string(), false)]);
        let entries = all_subagents_with_home(tmp.path(), &toggle, None, None);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["general-purpose", "explore", "browser-use"]);
    }
}

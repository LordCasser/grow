//! Filesystem scanning for plugin directories.
//!
//! Discovers plugins from the canonical Grow sources in priority order:
//! 1. CLI `--plugin-dir` paths (scope: `CliOverride`)
//! 2. `.grow/plugins/*/` (scope: `Project`, walked from cwd to worktree root)
//! 3. `$GROW_HOME/plugins/*/` (scope: `User`)
//! 4. `$GROW_HOME/installed-plugins/*/` (scope: `User`, registry installs)
//! 5. Paths from `[plugins].paths` in config (scope: `ConfigPath`)
//!
//! Deduplicates by canonical path and resolves name conflicts via
//! the canonical source precedence.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::manifest::{PluginManifest, load_manifest};
use super::trust::TrustStore;

// ── Public types ──────────────────────────────────────────────────────

/// Where a plugin was discovered from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PluginScope {
    /// `--plugin-dir` (highest priority, always trusted)
    CliOverride = 0,
    /// `.grow/plugins/` in project (requires trust)
    Project = 1,
    /// `$GROW_HOME/plugins/` or registry installs (always trusted)
    User = 2,
    /// `[plugins].paths` in config (trust depends on location)
    ConfigPath = 3,
}

impl PluginScope {
    /// Label used in `PluginId` format.
    pub fn id_label(&self) -> &'static str {
        match self {
            Self::CliOverride => "cli",
            Self::Project => "project",
            Self::User => "user",
            Self::ConfigPath => "config",
        }
    }
}

impl std::fmt::Display for PluginScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CliOverride => write!(f, "cli"),
            Self::Project => write!(f, "project"),
            Self::User => write!(f, "user"),
            Self::ConfigPath => write!(f, "config"),
        }
    }
}

/// The concrete discovery source a plugin came from.
///
/// Finer-grained than [`PluginScope`]: recorded at scan time so consumers
/// (e.g. the pager's plugins list) don't have to re-derive provenance from
/// paths. Not part of [`PluginId`], which stays scope-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginOrigin {
    /// CLI `--plugin-dir`.
    CliOverride,
    /// Project `.grow/plugins/`.
    ProjectGrow,
    /// `$GROW_HOME/plugins/`.
    UserGrow,
    /// Grow's install registry (`~/.grow/installed-plugins`).
    MarketplaceInstall {
        /// Marketplace source display name (None for direct git/local installs).
        source_name: Option<String>,
        /// Git URL of the installed repo (None for local installs).
        git_url: Option<String>,
    },
    /// `[plugins].paths` in config.
    ConfigPath,
}

/// Stable internal identity for a plugin.
///
/// Format: `<scope>/<hex8>/<name>`
/// - `<scope>`: lowercase scope string (cli, project, user, config)
/// - `<hex8>`: first 8 hex chars of SHA-256 of the canonical plugin root path
/// - `<name>`: the plugin_name
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginId(pub String);

impl PluginId {
    /// Construct a plugin ID from its components.
    pub fn new(scope: PluginScope, canonical_root: &Path, name: &str) -> Self {
        let path_str = canonical_root.to_string_lossy();
        let mut hasher = Sha256::new();
        hasher.update(path_str.as_bytes());
        let hash = hasher.finalize();
        let hex8 = format!(
            "{:02x}{:02x}{:02x}{:02x}",
            hash[0], hash[1], hash[2], hash[3]
        );
        Self(format!("{}/{}/{}", scope.id_label(), hex8, name))
    }
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A plugin candidate discovered on the filesystem.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    /// Parsed canonical manifest.
    pub manifest: PluginManifest,
    /// Stable internal identity.
    pub id: PluginId,
    /// Absolute path to the plugin root directory.
    pub root: PathBuf,
    /// Canonical (symlink-resolved) root path.
    pub canonical_root: PathBuf,
    /// Where this plugin was discovered.
    pub scope: PluginScope,
    /// The concrete discovery source this plugin came from.
    pub origin: PluginOrigin,
    /// Whether the plugin is trusted for executable operations.
    pub trusted: bool,
    /// Resolved skill directories.
    pub skill_dirs: Vec<PathBuf>,
    pub command_dirs: Vec<PathBuf>,
    /// Resolved agent directories.
    pub agent_dirs: Vec<PathBuf>,
    /// Resolved hooks file path.
    pub hooks_path: Option<PathBuf>,
    /// Resolved MCP config file path.
    pub mcp_config_path: Option<PathBuf>,
    /// Resolved LSP config file path.
    pub lsp_config_path: Option<PathBuf>,
    /// Warning message when this plugin won a name collision.
    pub conflict: Option<String>,
}

impl DiscoveredPlugin {
    /// User-facing plugin name (from manifest or directory name).
    pub fn plugin_name(&self) -> &str {
        &self.manifest.name
    }
}

/// Configuration for plugin discovery.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryConfig {
    /// CLI `--plugin-dir` paths.
    pub cli_plugin_dirs: Vec<PathBuf>,
    /// `[plugins].paths` from config.
    pub config_paths: Vec<PathBuf>,
    /// `[plugins].disabled` plugin IDs or names.
    pub disabled: Vec<String>,
    /// `[plugins].enabled` plugin IDs or names (overrides default-disabled for project plugins).
    pub enabled: Vec<String>,
}

impl DiscoveryConfig {
    /// Ensure every discovered plugin appears in either `enabled` or `disabled`.
    ///
    /// Plugins from auto-enabled scopes (`CliOverride`, `ConfigPath`) are added
    /// to `enabled`. All others (`User`, `Project`) are added to `disabled`.
    /// Plugins already present in either list are left untouched.
    pub fn populate_plugin_lists(&mut self, discovered: &[DiscoveredPlugin]) {
        for dp in discovered {
            let name = &dp.manifest.name;
            let already_listed = self.enabled.iter().any(|e| e == name || e == &dp.id.0)
                || self.disabled.iter().any(|d| d == name || d == &dp.id.0);
            if already_listed {
                tracing::debug!(plugin = %name, id = %dp.id.0, "plugin already in enabled/disabled list");
                continue;
            }
            if matches!(dp.scope, PluginScope::CliOverride | PluginScope::ConfigPath) {
                tracing::debug!(plugin = %name, scope = ?dp.scope, "auto-adding to enabled list");
                self.enabled.push(name.clone());
            } else {
                tracing::debug!(plugin = %name, scope = ?dp.scope, "auto-adding to disabled list");
                self.disabled.push(name.clone());
            }
        }
    }
}

// ── Discovery entry point ─────────────────────────────────────────────

/// The one user plugin directory owned by Grow.
fn user_plugin_dirs(grow: Option<&Path>) -> Vec<(PathBuf, PluginOrigin)> {
    grow.into_iter()
        .map(|root| (root.join("plugins"), PluginOrigin::UserGrow))
        .collect()
}

/// Project-scoped plugin parent dirs (`.grow/plugins`) that
/// exist along the `cwd`→git-worktree-root walk (inclusive), or just `cwd`'s own
/// when `cwd` is not inside a git repo, paired with the resolved git worktree
/// root (when any). This is the exact set [`discover_plugins`] scans for
/// `PluginScope::Project`; the folder-trust gate reuses the same chain via
/// [`project_plugin_dirs_in`] so detection and discovery can never drift. The
pub fn project_plugin_dirs(cwd: Option<&Path>) -> Vec<PathBuf> {
    let Some(cwd) = cwd else {
        return Vec::new();
    };
    let chain = crate::repo::RepoDirChain::resolve(cwd);
    project_plugin_dirs_in(&chain.dirs)
}

/// Existing project plugin parent dirs (`.grow/plugins`)
/// under each dir of a precomputed cwd→git-root chain
/// ([`crate::repo::RepoDirChain`]). The folder-trust gate reuses its one shared
/// chain here so detection and discovery can never drift.
pub fn project_plugin_dirs_in(chain_dirs: &[PathBuf]) -> Vec<PathBuf> {
    crate::repo::existing_subdirs_along(chain_dirs, &[".grow/plugins"])
}

/// Discover all plugins from the filesystem.
///
/// `cwd` is used to find the git worktree root for project-scope plugins.
/// `project_trusted` is the folder-trust verdict for `cwd`; it gates
/// `Project`-scope plugins (CLI/User/ConfigPath scopes are unaffected).
/// Returns plugins deduplicated by canonical path, with name conflicts
/// resolved by scope precedence.
pub fn discover_plugins(
    cwd: Option<&Path>,
    config: &DiscoveryConfig,
    trust_store: &TrustStore,
    project_trusted: bool,
) -> Vec<DiscoveredPlugin> {
    let _plugin_discovery_timer = crate::timing::timer("plugin_discovery");
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let mut candidates: Vec<DiscoveredPlugin> = Vec::new();

    // 1. CLI --plugin-dir paths
    for dir in &config.cli_plugin_dirs {
        if dir.is_dir() {
            collect_plugin(
                dir,
                PluginScope::CliOverride,
                PluginOrigin::CliOverride,
                trust_store,
                project_trusted,
                &mut seen_paths,
                &mut candidates,
            );
        } else {
            tracing::warn!(path = %dir.display(), "CLI --plugin-dir path is not a directory; skipping");
        }
    }

    // 2. Project plugins (.grow/plugins/) — scan the SAME
    // dirs the folder-trust gate detects, via the shared `project_plugin_dirs`
    // walk (cwd→git root), so discovery and gating can never drift.
    if let Some(cwd) = cwd {
        let project_dirs = project_plugin_dirs(Some(cwd));
        for plugins_dir in project_dirs {
            scan_plugin_dir(
                &plugins_dir,
                PluginScope::Project,
                PluginOrigin::ProjectGrow,
                trust_store,
                project_trusted,
                &mut seen_paths,
                &mut candidates,
            );
        }
    }

    // 3. User plugins: $GROW_HOME/plugins.
    // Gate the grow plugins dir on user_grow_home() so a project's .grow/plugins
    // is never scanned as user-global when no home resolves.
    let grow = config::user_grow_home();
    let plugin_dirs = user_plugin_dirs(grow.as_deref());
    for (plugins_dir, origin) in plugin_dirs {
        if plugins_dir.is_dir() {
            scan_plugin_dir(
                &plugins_dir,
                PluginScope::User,
                origin,
                trust_store,
                project_trusted,
                &mut seen_paths,
                &mut candidates,
            );
        }
    }

    // 4. Installed plugins from Grow's install registry.
    {
        // Installed plugins are always User scope (auto-trusted).
        // The user explicitly installed them via marketplace or CLI,
        // so they should be trusted regardless of install_dir location.
        let registry = super::install_registry::InstallRegistry::load();
        collect_installed_plugins(
            &registry,
            PluginScope::User,
            trust_store,
            project_trusted,
            &mut seen_paths,
            &mut candidates,
        );
    }

    // 5. Config-path plugins
    for dir in &config.config_paths {
        if dir.is_dir() {
            collect_plugin(
                dir,
                PluginScope::ConfigPath,
                PluginOrigin::ConfigPath,
                trust_store,
                project_trusted,
                &mut seen_paths,
                &mut candidates,
            );
        } else {
            tracing::warn!(path = %dir.display(), "[plugins].paths entry is not a directory; skipping");
        }
    }

    // Resolve name conflicts: within the same plugin_name, highest-priority
    // scope wins. Within same scope, first-found (alphabetical by canonical
    // path) wins.
    resolve_name_conflicts(&mut candidates);

    for p in &candidates {
        tracing::info!(
            name = %p.manifest.name,
            scope = %p.scope,
            root = %p.root.display(),
            skills = p.manifest.skill_dirs(&p.root).len(),
            agents = p.manifest.agent_dirs(&p.root).len(),
            has_hooks = p.hooks_path.is_some(),
            has_mcp = p.mcp_config_path.is_some(),
            has_lsp = p.lsp_config_path.is_some(),
            "plugin discovered"
        );
    }
    tracing::info!(
        discovered = candidates.len(),
        cli = candidates
            .iter()
            .filter(|p| p.scope == PluginScope::CliOverride)
            .count(),
        project = candidates
            .iter()
            .filter(|p| p.scope == PluginScope::Project)
            .count(),
        user = candidates
            .iter()
            .filter(|p| p.scope == PluginScope::User)
            .count(),
        config = candidates
            .iter()
            .filter(|p| p.scope == PluginScope::ConfigPath)
            .count(),
        "plugins: discovery complete"
    );

    candidates
}

// ── Internal helpers ──────────────────────────────────────────────────

/// Scan a plugins parent directory (e.g. `~/.grow/plugins/`) and collect
/// each subdirectory as a plugin candidate.
fn scan_plugin_dir(
    plugins_dir: &Path,
    scope: PluginScope,
    origin: PluginOrigin,
    trust_store: &TrustStore,
    project_trusted: bool,
    seen_paths: &mut HashSet<PathBuf>,
    candidates: &mut Vec<DiscoveredPlugin>,
) {
    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(
                path = %plugins_dir.display(),
                error = %e,
                "failed to read plugins directory"
            );
            return;
        }
    };

    let mut subdirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir()) // follows symlinks
        .map(|e| e.path())
        .collect();

    // Sort for deterministic ordering within same scope
    subdirs.sort();

    for subdir in subdirs {
        collect_plugin(
            &subdir,
            scope,
            origin.clone(),
            trust_store,
            project_trusted,
            seen_paths,
            candidates,
        );
    }
}

fn collect_installed_plugins(
    registry: &super::install_registry::InstallRegistry,
    scope: PluginScope,
    trust_store: &TrustStore,
    project_trusted: bool,
    seen_paths: &mut HashSet<PathBuf>,
    candidates: &mut Vec<DiscoveredPlugin>,
) {
    for (_key, repo) in registry.list() {
        let origin = PluginOrigin::MarketplaceInstall {
            source_name: repo
                .marketplace
                .as_ref()
                .map(|mp| mp.source_display_name.clone()),
            git_url: match &repo.kind {
                super::install_registry::InstallKind::Git { url, .. } => Some(url.clone()),
                super::install_registry::InstallKind::Local { .. } => None,
            },
        };
        for plugin in repo.plugins.values() {
            let plugin_root = match plugin.subdir.as_deref() {
                Some(sub) => {
                    if subdir_escapes(sub) {
                        tracing::warn!(
                            repo = %repo.path.display(),
                            subdir = sub,
                            "skipping installed plugin: registry subdir escapes repo root"
                        );
                        continue;
                    }
                    repo.path.join(sub)
                }
                None => repo.path.clone(),
            };
            if plugin_root.is_dir() {
                collect_plugin(
                    &plugin_root,
                    scope,
                    origin.clone(),
                    trust_store,
                    project_trusted,
                    seen_paths,
                    candidates,
                );
            }
        }
    }
}

fn subdir_escapes(subdir: &str) -> bool {
    let p = Path::new(subdir);
    p.is_absolute()
        || p.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
}

/// Attempt to load a single plugin directory and add it to candidates.
fn collect_plugin(
    plugin_root: &Path,
    scope: PluginScope,
    origin: PluginOrigin,
    trust_store: &TrustStore,
    project_trusted: bool,
    seen_paths: &mut HashSet<PathBuf>,
    candidates: &mut Vec<DiscoveredPlugin>,
) {
    // Canonicalize for dedup
    let canonical = match dunce::canonicalize(plugin_root) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                path = %plugin_root.display(),
                error = %e,
                "failed to canonicalize plugin root; skipping"
            );
            return;
        }
    };

    // Deduplicate by canonical path
    if !seen_paths.insert(canonical.clone()) {
        tracing::debug!(
            path = %plugin_root.display(),
            canonical = %canonical.display(),
            "plugin directory already discovered via another path; skipping"
        );
        return;
    }

    // A manifest is the plugin identity boundary. Directories without one are
    // not plugins and are never interpreted by component heuristics.
    let manifest = match load_manifest(plugin_root) {
        Ok(manifest) => *manifest,
        Err(e) => {
            tracing::warn!(
                path = %plugin_root.display(),
                error = %e,
                "failed to load plugin manifest; skipping"
            );
            return;
        }
    };

    // Determine trust status. Exhaustive match so a new PluginScope variant is a
    // compile error rather than a silent default.
    let trusted = match scope {
        PluginScope::CliOverride | PluginScope::User => true,
        PluginScope::ConfigPath => {
            TrustStore::is_config_path_auto_trusted(plugin_root)
                || trust_store.is_trusted(plugin_root)
        }
        // Project trust now comes from folder-trust (passed by the caller).
        PluginScope::Project => project_trusted,
    };

    // Build PluginId
    let id = PluginId::new(scope, &canonical, &manifest.name);

    // Resolve component paths
    let skill_dirs = manifest.skill_dirs(plugin_root);
    let command_dirs = manifest.command_dirs(plugin_root);
    let agent_dirs = manifest.agent_dirs(plugin_root);
    let hooks_path = manifest.hooks_path(plugin_root);
    let mcp_config_path = manifest.mcp_config_path(plugin_root);
    let lsp_config_path = manifest.lsp_config_path(plugin_root);

    candidates.push(DiscoveredPlugin {
        manifest,
        id,
        root: plugin_root.to_path_buf(),
        canonical_root: canonical,
        scope,
        origin,
        trusted,
        skill_dirs,
        command_dirs,
        agent_dirs,
        hooks_path,
        mcp_config_path,
        lsp_config_path,
        conflict: None,
    });
}

/// Resolve plugin_name conflicts across scopes.
///
/// Within each name group, keep only the highest-priority candidate
/// (lowest scope ordinal). Log warnings for dropped duplicates.
fn resolve_name_conflicts(candidates: &mut Vec<DiscoveredPlugin>) {
    let mut name_map: HashMap<String, usize> = HashMap::new();
    let mut to_remove: Vec<usize> = Vec::new();
    // (winner_idx, conflict message) pairs to apply after the scan.
    let mut conflict_msgs: Vec<(usize, String)> = Vec::new();

    for (idx, candidate) in candidates.iter().enumerate() {
        let name = candidate.manifest.name.clone();
        match name_map.get(&name) {
            Some(&existing_idx) => {
                let existing = &candidates[existing_idx];
                // Lower scope ordinal = higher priority
                if (candidate.scope as u8) < (existing.scope as u8) {
                    // New candidate wins
                    tracing::warn!(
                        plugin_name = %name,
                        winner = %candidate.root.display(),
                        loser = %existing.root.display(),
                        "plugin name collision resolved by scope precedence"
                    );
                    let msg = format!(
                        "Name collision: shadowing \"{}\" from {}",
                        name,
                        existing.root.display()
                    );
                    conflict_msgs.push((idx, msg));
                    to_remove.push(existing_idx);
                    name_map.insert(name, idx);
                } else {
                    // Existing wins
                    tracing::warn!(
                        plugin_name = %name,
                        winner = %existing.root.display(),
                        loser = %candidate.root.display(),
                        "plugin name collision resolved by scope precedence"
                    );
                    let msg = format!(
                        "Name collision: shadowing \"{}\" from {}",
                        name,
                        candidate.root.display()
                    );
                    conflict_msgs.push((existing_idx, msg));
                    to_remove.push(idx);
                }
            }
            None => {
                name_map.insert(name, idx);
            }
        }
    }

    // Apply conflict messages to winners.
    for (idx, msg) in conflict_msgs {
        candidates[idx].conflict = Some(msg);
    }

    // Remove losers (reverse order to preserve indices)
    to_remove.sort_unstable();
    to_remove.dedup();
    for idx in to_remove.into_iter().rev() {
        candidates.remove(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manifest_plugin(tmp: &Path, name: &str) -> PathBuf {
        let plugin_dir = tmp.join(name);
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("plugin.json"),
            format!(r#"{{"name": "{}"}}"#, name),
        )
        .unwrap();
        plugin_dir
    }

    fn make_convention_plugin(tmp: &Path, name: &str) -> PathBuf {
        let plugin_dir = tmp.join(name);
        std::fs::create_dir_all(plugin_dir.join("skills")).unwrap();
        plugin_dir
    }

    #[test]
    fn user_plugin_dirs_are_owned_by_grow_home() {
        let grow = Path::new("/custom/growhome");
        assert_eq!(
            user_plugin_dirs(Some(grow)),
            vec![(grow.join("plugins"), PluginOrigin::UserGrow)]
        );
    }

    #[test]
    fn user_plugin_dirs_empty_without_home_or_grow() {
        assert!(user_plugin_dirs(None).is_empty());
    }

    #[test]
    fn discover_cli_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = make_manifest_plugin(tmp.path(), "cli-tool");

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        collect_plugin(
            &plugin_dir,
            PluginScope::CliOverride,
            PluginOrigin::CliOverride,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].plugin_name(), "cli-tool");
        assert_eq!(candidates[0].scope, PluginScope::CliOverride);
        assert_eq!(candidates[0].origin, PluginOrigin::CliOverride);
        assert!(candidates[0].trusted);
    }

    #[test]
    fn discover_user_plugins() {
        let tmp = tempfile::tempdir().unwrap();

        // Create ~/.grow/plugins/ structure
        let grow_plugins = tmp.path().join(".grow").join("plugins");
        std::fs::create_dir_all(&grow_plugins).unwrap();
        make_manifest_plugin(&grow_plugins, "user-tool");

        // Override home dir by directly scanning
        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        scan_plugin_dir(
            &grow_plugins,
            PluginScope::User,
            PluginOrigin::UserGrow,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].plugin_name(), "user-tool");
        assert!(candidates[0].trusted);
    }

    #[test]
    fn discover_rejects_plugin_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let plugins_dir = tmp.path().join("plugins");
        make_convention_plugin(&plugins_dir, "my-tool");

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        scan_plugin_dir(
            &plugins_dir,
            PluginScope::User,
            PluginOrigin::UserGrow,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );

        assert!(candidates.is_empty());
    }

    #[test]
    fn installed_plugins_load_at_registered_subdir_and_root() {
        use crate::plugins::install_registry::{
            InstallKind, InstallRegistry, InstalledRepo, RepoPlugin,
        };
        use std::collections::HashMap;

        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("installed-plugins");

        let sub_key = "acme-deadbeef";
        let sub_repo_path = install_dir.join(sub_key);
        let sub_plugin_dir = sub_repo_path.join("plugins").join("acme");
        std::fs::create_dir_all(&sub_plugin_dir).unwrap();
        std::fs::write(sub_plugin_dir.join("plugin.json"), r#"{"name": "acme"}"#).unwrap();

        let root_key = "cloud-cafef00d";
        let root_repo_path = install_dir.join(root_key);
        std::fs::create_dir_all(&root_repo_path).unwrap();
        std::fs::write(root_repo_path.join("plugin.json"), r#"{"name": "cloud"}"#).unwrap();

        let mut registry = InstallRegistry::empty(install_dir);
        registry.insert(
            sub_key.to_string(),
            InstalledRepo {
                kind: InstallKind::Local {
                    source_path: sub_repo_path.clone(),
                    subdir: None,
                },
                installed_at: String::new(),
                updated_at: String::new(),
                path: sub_repo_path,
                plugins: HashMap::from([(
                    "acme".to_string(),
                    RepoPlugin {
                        subdir: Some("plugins/acme".to_string()),
                        version: None,
                    },
                )]),
                marketplace: None,
            },
        );
        registry.insert(
            root_key.to_string(),
            InstalledRepo {
                kind: InstallKind::Local {
                    source_path: root_repo_path.clone(),
                    subdir: None,
                },
                installed_at: String::new(),
                updated_at: String::new(),
                path: root_repo_path,
                plugins: HashMap::from([(
                    "cloud".to_string(),
                    RepoPlugin {
                        subdir: None,
                        version: None,
                    },
                )]),
                marketplace: None,
            },
        );

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        collect_installed_plugins(
            &registry,
            PluginScope::User,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );

        let names: Vec<&str> = candidates.iter().map(|p| p.plugin_name()).collect();
        assert_eq!(candidates.len(), 2, "got {names:?}");
        assert!(
            names.contains(&"acme"),
            "subdir plugin should load, got {names:?}"
        );
        assert!(
            names.contains(&"cloud"),
            "root plugin should load, got {names:?}"
        );
        for p in &candidates {
            assert_eq!(
                p.origin,
                PluginOrigin::MarketplaceInstall {
                    source_name: None,
                    git_url: None,
                },
                "direct local installs carry neither source name nor git URL"
            );
        }
    }

    #[test]
    fn installed_plugins_record_marketplace_and_git_origin() {
        use crate::plugins::install_registry::{
            InstallKind, InstallRegistry, InstalledRepo, MarketplaceProvenance, RepoPlugin,
        };
        use std::collections::HashMap;

        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("installed-plugins");

        let mp_repo_path = install_dir.join("mp-plugin-11111111");
        std::fs::create_dir_all(&mp_repo_path).unwrap();
        std::fs::write(mp_repo_path.join("plugin.json"), r#"{"name": "mp-plugin"}"#).unwrap();

        let git_repo_path = install_dir.join("git-plugin-22222222");
        std::fs::create_dir_all(&git_repo_path).unwrap();
        std::fs::write(
            git_repo_path.join("plugin.json"),
            r#"{"name": "git-plugin"}"#,
        )
        .unwrap();

        let mut registry = InstallRegistry::empty(install_dir);
        registry.insert(
            "mp-plugin-11111111".to_string(),
            InstalledRepo {
                kind: InstallKind::Git {
                    url: "https://example.com/mp.git".to_string(),
                    git_ref: None,
                    commit: "abc".to_string(),
                    subdir: None,
                },
                installed_at: String::new(),
                updated_at: String::new(),
                path: mp_repo_path,
                plugins: HashMap::from([(
                    "mp-plugin".to_string(),
                    RepoPlugin {
                        subdir: None,
                        version: None,
                    },
                )]),
                marketplace: Some(MarketplaceProvenance {
                    source_url_or_path: "https://example.com/mp.git".to_string(),
                    source_display_name: "Demo Marketplace".to_string(),
                    plugin_subdir: "plugins/mp-plugin".to_string(),
                }),
            },
        );
        registry.insert(
            "git-plugin-22222222".to_string(),
            InstalledRepo {
                kind: InstallKind::Git {
                    url: "https://github.com/owner/repo.git".to_string(),
                    git_ref: None,
                    commit: "def".to_string(),
                    subdir: None,
                },
                installed_at: String::new(),
                updated_at: String::new(),
                path: git_repo_path,
                plugins: HashMap::from([(
                    "git-plugin".to_string(),
                    RepoPlugin {
                        subdir: None,
                        version: None,
                    },
                )]),
                marketplace: None,
            },
        );

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        collect_installed_plugins(
            &registry,
            PluginScope::User,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );

        let mp = candidates
            .iter()
            .find(|p| p.plugin_name() == "mp-plugin")
            .unwrap();
        assert_eq!(
            mp.origin,
            PluginOrigin::MarketplaceInstall {
                source_name: Some("Demo Marketplace".to_string()),
                git_url: Some("https://example.com/mp.git".to_string()),
            }
        );

        let git = candidates
            .iter()
            .find(|p| p.plugin_name() == "git-plugin")
            .unwrap();
        assert_eq!(
            git.origin,
            PluginOrigin::MarketplaceInstall {
                source_name: None,
                git_url: Some("https://github.com/owner/repo.git".to_string()),
            }
        );
    }

    #[test]
    fn installed_plugins_skip_subdir_that_escapes_repo() {
        use crate::plugins::install_registry::{
            InstallKind, InstallRegistry, InstalledRepo, RepoPlugin,
        };
        use std::collections::HashMap;

        let tmp = tempfile::tempdir().unwrap();
        let install_dir = tmp.path().join("installed-plugins");

        let escaped_dir = install_dir.join("escaped");
        std::fs::create_dir_all(&escaped_dir).unwrap();
        std::fs::write(
            escaped_dir.join("plugin.json"),
            r#"{"name": "escaped-evil"}"#,
        )
        .unwrap();

        let evil_key = "evil-deadbeef";
        let evil_repo_path = install_dir.join(evil_key);
        std::fs::create_dir_all(&evil_repo_path).unwrap();

        let good_key = "good-cafef00d";
        let good_repo_path = install_dir.join(good_key);
        let good_plugin_dir = good_repo_path.join("plugins").join("good");
        std::fs::create_dir_all(&good_plugin_dir).unwrap();
        std::fs::write(good_plugin_dir.join("plugin.json"), r#"{"name": "good"}"#).unwrap();

        let mut registry = InstallRegistry::empty(install_dir);
        registry.insert(
            evil_key.to_string(),
            InstalledRepo {
                kind: InstallKind::Local {
                    source_path: evil_repo_path.clone(),
                    subdir: None,
                },
                installed_at: String::new(),
                updated_at: String::new(),
                path: evil_repo_path,
                plugins: HashMap::from([(
                    "escaped-evil".to_string(),
                    RepoPlugin {
                        subdir: Some("../escaped".to_string()),
                        version: None,
                    },
                )]),
                marketplace: None,
            },
        );
        registry.insert(
            good_key.to_string(),
            InstalledRepo {
                kind: InstallKind::Local {
                    source_path: good_repo_path.clone(),
                    subdir: None,
                },
                installed_at: String::new(),
                updated_at: String::new(),
                path: good_repo_path,
                plugins: HashMap::from([(
                    "good".to_string(),
                    RepoPlugin {
                        subdir: Some("plugins/good".to_string()),
                        version: None,
                    },
                )]),
                marketplace: None,
            },
        );

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        collect_installed_plugins(
            &registry,
            PluginScope::User,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );

        let names: Vec<&str> = candidates.iter().map(|p| p.plugin_name()).collect();
        assert!(
            names.contains(&"good"),
            "legit subdir plugin should load, got {names:?}"
        );
        assert!(
            !names.contains(&"escaped-evil"),
            "escaping subdir must be skipped, got {names:?}"
        );
    }

    #[test]
    fn dedup_by_canonical_path() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = make_manifest_plugin(tmp.path(), "dup-test");

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        // Add same plugin twice
        collect_plugin(
            &plugin_dir,
            PluginScope::CliOverride,
            PluginOrigin::CliOverride,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );
        collect_plugin(
            &plugin_dir,
            PluginScope::CliOverride,
            PluginOrigin::CliOverride,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );

        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn name_conflict_higher_priority_wins() {
        let tmp = tempfile::tempdir().unwrap();

        // Create two plugins with the same name but different scopes
        let cli_dir = tmp.path().join("cli");
        std::fs::create_dir_all(&cli_dir).unwrap();
        std::fs::write(cli_dir.join("plugin.json"), r#"{"name": "my-plugin"}"#).unwrap();

        let user_dir = tmp.path().join("user-plugins");
        std::fs::create_dir_all(&user_dir).unwrap();
        let user_plugin = user_dir.join("my-plugin");
        std::fs::create_dir_all(&user_plugin).unwrap();
        std::fs::write(user_plugin.join("plugin.json"), r#"{"name": "my-plugin"}"#).unwrap();

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();

        // Add CLI plugin first (higher priority)
        collect_plugin(
            &cli_dir,
            PluginScope::CliOverride,
            PluginOrigin::CliOverride,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );
        // Add user plugin (lower priority)
        collect_plugin(
            &user_plugin,
            PluginScope::User,
            PluginOrigin::UserGrow,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );

        resolve_name_conflicts(&mut candidates);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].scope, PluginScope::CliOverride);
    }

    #[test]
    fn plugin_id_format() {
        let id = PluginId::new(
            PluginScope::User,
            Path::new("/home/user/.grow/plugins/my-plugin"),
            "my-plugin",
        );
        assert!(id.0.starts_with("user/"));
        assert!(id.0.ends_with("/my-plugin"));
        // Format: user/<hex8>/my-plugin
        let parts: Vec<&str> = id.0.split('/').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "user");
        assert_eq!(parts[1].len(), 8); // 8 hex chars
        assert_eq!(parts[2], "my-plugin");
    }

    #[test]
    fn plugin_id_deterministic() {
        let id1 = PluginId::new(PluginScope::User, Path::new("/same/path"), "test");
        let id2 = PluginId::new(PluginScope::User, Path::new("/same/path"), "test");
        assert_eq!(id1, id2);
    }

    #[test]
    fn plugin_id_different_paths() {
        let id1 = PluginId::new(PluginScope::User, Path::new("/path/a"), "test");
        let id2 = PluginId::new(PluginScope::User, Path::new("/path/b"), "test");
        assert_ne!(id1, id2);
    }

    #[test]
    fn empty_dir_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let empty_dir = tmp.path().join("empty");
        std::fs::create_dir_all(&empty_dir).unwrap();

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        collect_plugin(
            &empty_dir,
            PluginScope::User,
            PluginOrigin::UserGrow,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );

        // No manifest + no skills/agents/mcp/hooks = skipped
        assert!(candidates.is_empty());
    }

    #[test]
    fn project_plugin_untrusted_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = make_manifest_plugin(tmp.path(), "project-tool");

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        // Untrusted folder (project_trusted = false) blocks the Project plugin.
        collect_plugin(
            &plugin_dir,
            PluginScope::Project,
            PluginOrigin::ProjectGrow,
            &trust,
            false,
            &mut seen,
            &mut candidates,
        );

        assert_eq!(candidates.len(), 1);
        assert!(!candidates[0].trusted);
    }

    #[test]
    fn project_plugin_trusted_when_folder_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = make_manifest_plugin(tmp.path(), "trusted-tool");

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        // Trusted folder (project_trusted = true) allows the Project plugin.
        collect_plugin(
            &plugin_dir,
            PluginScope::Project,
            PluginOrigin::ProjectGrow,
            &trust,
            true,
            &mut seen,
            &mut candidates,
        );

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].trusted);
    }

    #[test]
    fn non_project_scopes_unaffected_by_project_trusted() {
        // project_trusted = false gates Project scope only: CLI/User stay
        // auto-trusted and ConfigPath keeps using its own trust store.
        let tmp = tempfile::tempdir().unwrap();
        let cli_dir = make_manifest_plugin(tmp.path(), "cli-tool");
        let user_dir = make_manifest_plugin(tmp.path(), "user-tool");
        let config_dir = make_manifest_plugin(tmp.path(), "config-tool");

        let mut trust = TrustStore::load_from(tmp.path().join("trust"));
        trust.grant_trust(&config_dir).unwrap();

        let mut seen = HashSet::new();
        let mut candidates = Vec::new();
        for (dir, scope, origin) in [
            (
                &cli_dir,
                PluginScope::CliOverride,
                PluginOrigin::CliOverride,
            ),
            (&user_dir, PluginScope::User, PluginOrigin::UserGrow),
            (
                &config_dir,
                PluginScope::ConfigPath,
                PluginOrigin::ConfigPath,
            ),
        ] {
            collect_plugin(
                dir,
                scope,
                origin,
                &trust,
                false,
                &mut seen,
                &mut candidates,
            );
        }

        assert_eq!(candidates.len(), 3);
        assert!(
            candidates.iter().all(|c| c.trusted),
            "CLI/User/ConfigPath plugins must stay trusted under project_trusted=false"
        );
    }

    #[test]
    fn discover_real_project_plugin_gated_on_project_trusted() {
        // End-to-end through discover_plugins: a repo-local `.grow/plugins/<x>/`
        // plugin with an MCP component is trusted iff the folder-trust verdict
        // (project_trusted) allows it. Found by name so any user-scoped plugins
        // on the test host are irrelevant.
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join(".grow").join("plugins").join("proj-mcp");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(plugin_dir.join("plugin.json"), r#"{"name": "proj-mcp"}"#).unwrap();
        std::fs::write(plugin_dir.join(".mcp.json"), r#"{"mcpServers":{}}"#).unwrap();

        let trust = TrustStore::load_from(tmp.path().join("trust"));
        let config = DiscoveryConfig::default();

        // Untrusted folder: the project plugin comes back blocked.
        let untrusted = discover_plugins(Some(tmp.path()), &config, &trust, false);
        let p = untrusted
            .iter()
            .find(|p| p.manifest.name == "proj-mcp")
            .expect("project plugin discovered");
        assert_eq!(p.scope, PluginScope::Project);
        assert_eq!(p.origin, PluginOrigin::ProjectGrow);
        assert!(!p.trusted, "untrusted folder must block the project plugin");

        // Trusted folder: the same plugin is allowed.
        let trusted = discover_plugins(Some(tmp.path()), &config, &trust, true);
        let p = trusted
            .iter()
            .find(|p| p.manifest.name == "proj-mcp")
            .expect("project plugin discovered");
        assert!(p.trusted, "trusted folder must allow the project plugin");
    }
}

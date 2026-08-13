use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use super::watcher::ConfigChangeEvent;

/// Typed, `Send`-safe messages for the agent to apply inside its `LocalSet`.
#[derive(Debug)]
pub enum ConfigUpdate {
    /// A **broadcast** MCP reload — applies to every active session
    /// regardless of cwd. Fires for two cases:
    ///
    /// 1. The global `[mcp_servers]` table in `~/.grow/config.toml`
    ///    changed.
    /// 2. The user's home-level `~/.claude.json` changed.
    ///    `load_claude_json_mcp_servers_as_configs` reads this file
    ///    for every session, so the reload cannot be narrowed by cwd.
    ///
    /// Project-scoped changes (`<cwd>/.grow/config.toml`,
    /// `<cwd>/.mcp.json`, project-level `<cwd>/.claude.json`) emit
    /// [`Self::ProjectMcpServersChanged`] instead so the reload can
    /// be narrowed to matching cwds.
    ///
    /// Deliberately kept as a unit variant.
    /// Adding a payload here would force pattern-match updates across
    /// (`<cwd>/.grow/config.toml`, `<cwd>/.mcp.json`, or
    /// `mvp_agent`, `app`, `session/handle`, etc.
    McpServersChanged,
    /// A **project-scoped** MCP config file changed
    /// `<cwd>/.claude.json`). Agent should reload MCP only for
    /// sessions whose cwd matches `cwd` (or sits beneath it).
    ///
    /// Strictly additive to [`Self::McpServersChanged`] — the unit
    /// variant continues to fire for global-config edits. The two
    /// cases are split so per-project reloads don't
    /// grow process sharing the home dir). The agent should consult the cache
    /// thrash unrelated sessions.
    ProjectMcpServersChanged {
        /// The project root whose `.grow/`, `.mcp.json`, or
        /// `.claude.json` file was edited. Sessions whose cwd equals
        /// this path — or is a descendant of it — are the reload
        /// targets.
        cwd: PathBuf,
    },
    /// Updated memory config (boxed to avoid large enum variant).
    Memory(Box<crate::config::MemoryConfig>),
    /// Updated skills discovery config.
    Skills(agent::prompt::skills::SkillsConfig),
    /// Updated `[compat]` vendor-compatibility config. Applied on the
    /// next agent (re)build, which re-resolves `compat_resolved`.
    Compat(Box<tools::types::compat::CompatConfigToml>),
    /// The `[provider.*.models.*]` entries changed. Agent should re-resolve
    /// its model list (BYOK models added/removed, default or surprise changed).
    ModelsChanged,
    /// Final local announcement snapshot changed. The receiver forwards it to
    /// connected clients through `grow/announcements/update`.
    Announcements(Vec<announcements::Announcement>),
    /// Updated UI settings — agent broadcasts `grow/config_changed` to IPC clients.
    Ui {
        theme: Option<String>,
        yolo: bool,
        fork_secondary_model: Option<String>,
    },
}

/// Runs on `tokio::spawn` (`Send`). Receives raw [`ConfigChangeEvent`]s from
/// the file watcher, diffs against last-known state, and sends [`ConfigUpdate`]
/// messages to the agent via an `mpsc` channel.
pub struct ConfigReloader {
    last_global_config: toml::Value,
    last_announcements: Vec<announcements::Announcement>,
    /// Per-cwd content hash of the project MCP config files, used to ignore
    /// mtime-only touches (see `hash_project_mcp_config`).
    last_project_mcp_hashes: HashMap<PathBuf, u64>,
    remote_settings: Option<crate::util::config::RemoteSettings>,
    config_update_tx: mpsc::UnboundedSender<ConfigUpdate>,
    /// Whether --experimental-memory was passed at startup. Persists across config reloads.
    experimental_memory: bool,
    /// Whether --no-memory was passed at startup. Persists across config reloads.
    no_memory: bool,
}

impl ConfigReloader {
    pub fn new(
        initial_config: toml::Value,
        remote_settings: Option<crate::util::config::RemoteSettings>,
        config_update_tx: mpsc::UnboundedSender<ConfigUpdate>,
        experimental_memory: bool,
        no_memory: bool,
    ) -> Self {
        let last_announcements = crate::util::config::resolve_announcements(&initial_config);
        Self {
            last_global_config: initial_config,
            last_announcements,
            last_project_mcp_hashes: HashMap::new(),
            remote_settings,
            config_update_tx,
            experimental_memory,
            no_memory,
        }
    }

    /// Main loop. Batches all events from each debounce tick before processing.
    pub async fn run(
        mut self,
        mut events: mpsc::UnboundedReceiver<ConfigChangeEvent>,
        cancel: CancellationToken,
    ) {
        loop {
            let first = tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                evt = events.recv() => match evt {
                    Some(e) => e,
                    None => break,
                },
            };

            // Drain additional events that arrived in the same tick
            let mut batch = vec![first];
            while let Ok(evt) = events.try_recv() {
                batch.push(evt);
            }

            let has_global_config = batch
                .iter()
                .any(|e| matches!(e, ConfigChangeEvent::GlobalConfigChanged));
            let has_project_config = batch
                .iter()
                .any(|e| matches!(e, ConfigChangeEvent::ProjectConfigChanged { .. }));
            // `~/.claude.json` is loaded by every
            // session (it does NOT live in a project root), so its
            // reload must broadcast through the legacy unit
            // `McpServersChanged` arm. Routing it through the per-
            // cwd variant would silently miss sessions outside `$HOME`.
            let has_home_claude_json = batch
                .iter()
                .any(|e| matches!(e, ConfigChangeEvent::HomeClaudeJsonChanged));
            let has_config = has_global_config || has_project_config;

            // Collect the unique cwds whose project
            // files changed so we can emit one
            // `ConfigUpdate::ProjectMcpServersChanged { cwd }` per
            // project root (rather than the legacy unit
            // `McpServersChanged` that swept every session).
            let project_cwds = collect_project_cwds(&batch);

            if has_config {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.reload_config()));
                match result {
                    Ok(Err(e)) => {
                        error!(error = %e, "config hot-reload failed, keeping last-known-good");
                    }
                    Err(_) => {
                        error!("panic in config reload handler, keeping last-known-good");
                    }
                    Ok(Ok(())) => {}
                }
            }

            // NB: the legacy fall-through that emitted a unit
            // `McpServersChanged` for any project `.mcp.json` /
            // `.claude.json` change is replaced by the
            // per-cwd fan-out below — `collect_project_cwds` already
            // includes every `McpConfigChanged` path in `project_cwds`,
            // so a separate emit here would double-dispatch. Global
            // `[mcp_servers]` edits are dispatched inside `reload_config`.

            // Home-level `~/.claude.json` must
            // broadcast to every session through the unit variant —
            // sessions outside `$HOME` would otherwise be silently
            // skipped by the per-cwd `cwd_matches` filter.
            if has_home_claude_json {
                info!("~/.claude.json change detected — broadcasting MCP reload");
                let _ = self.config_update_tx.send(ConfigUpdate::McpServersChanged);
            }

            // Fan out one
            // `ProjectMcpServersChanged { cwd }` per affected project
            // root. The legacy unit `McpServersChanged` above stays
            // for global-config edits — both variants can fire in the
            // same tick (e.g. `~/.grow/config.toml` AND
            // `<cwd>/.mcp.json` edited together).
            for cwd in project_cwds {
                // Skip the dispatch when the project config bytes are
                // unchanged (the watcher fires on mtime-only touches).
                // On any uncertainty we dispatch; see
                // `hash_project_mcp_config`.
                let new_hash = hash_project_mcp_config(&cwd);
                let unchanged = match (new_hash, self.last_project_mcp_hashes.get(&cwd)) {
                    (Some(new), Some(&prev)) => new == prev,
                    _ => false,
                };
                if unchanged {
                    debug!(
                        cwd = %cwd.display(),
                        "project MCP config event with unchanged content, skipping reload"
                    );
                    continue;
                }
                if let Some(h) = new_hash {
                    self.last_project_mcp_hashes.insert(cwd.clone(), h);
                }
                info!("project MCP config change detected");
                let _ = self
                    .config_update_tx
                    .send(ConfigUpdate::ProjectMcpServersChanged { cwd });
            }
        }
    }

    fn reload_config(&mut self) -> anyhow::Result<()> {
        let effective = crate::config::load_effective_config()?;
        let announcements = crate::util::config::resolve_announcements(&effective);
        if announcements != self.last_announcements {
            info!(
                count = announcements.len(),
                "local announcements config change detected"
            );
            self.last_announcements = announcements.clone();
            let _ = self
                .config_update_tx
                .send(ConfigUpdate::Announcements(announcements));
        }

        // `has_project_config` parameter dropped —
        // project-scoped reloads are dispatched via
        // `ProjectMcpServersChanged { cwd }` in the caller's
        // `collect_project_cwds` fan-out, so this function only
        // needs to diff the global toml.
        let new_global = match crate::config::load_from_disk() {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e, "failed to parse config.toml, keeping last-known-good");
                return Ok(());
            }
        };

        // MCP servers — compare [mcp_servers] table in the **global**
        // config (`~/.grow/config.toml`) via toml::Value. Project-
        // scoped changes (`<cwd>/.grow/config.toml`,
        // `<cwd>/.mcp.json`) are dispatched separately via
        // `ConfigUpdate::ProjectMcpServersChanged { cwd }` (see
        // `collect_project_cwds`) so they don't sweep
        // unrelated sessions.
        let old_mcp_table = self.last_global_config.get("mcp_servers");
        let new_mcp_table = new_global.get("mcp_servers");
        let mcp_changed = old_mcp_table != new_mcp_table;
        if mcp_changed {
            info!("Global MCP server config change detected");
            let _ = self.config_update_tx.send(ConfigUpdate::McpServersChanged);
        }

        // Memory config
        let old_mem = crate::config::MemoryConfig::resolve(
            self.experimental_memory,
            self.no_memory,
            &self.last_global_config,
            self.remote_settings.as_ref(),
        );
        let new_mem = crate::config::MemoryConfig::resolve(
            self.experimental_memory,
            self.no_memory,
            &new_global,
            self.remote_settings.as_ref(),
        );
        if old_mem != new_mem {
            info!("memory config change detected");
            let _ = self
                .config_update_tx
                .send(ConfigUpdate::Memory(Box::new(new_mem)));
        }

        // Skills config
        let old_skills = parse_skills_config(&self.last_global_config);
        let new_skills = parse_skills_config(&new_global);
        if old_skills != new_skills {
            info!("skills config change detected");
            let _ = self.config_update_tx.send(ConfigUpdate::Skills(new_skills));
        }

        // Compat config ([compat] vendor toggles)
        let old_compat = parse_compat_config(&self.last_global_config);
        let new_compat = parse_compat_config(&new_global);
        if old_compat != new_compat {
            info!("compat config change detected");
            let _ = self
                .config_update_tx
                .send(ConfigUpdate::Compat(Box::new(new_compat)));
        }

        // Models — compare provider definitions, global selection, and the
        // per-model overrides consumed by `Config::config_models`.
        if model_config_changed(&self.last_global_config, &new_global) {
            info!("model config change detected");
            let _ = self.config_update_tx.send(ConfigUpdate::ModelsChanged);
        }

        // UI fields (theme, yolo, fork_secondary_model)
        let old_ui = extract_ui_fields(&self.last_global_config);
        let new_ui = extract_ui_fields(&new_global);
        if old_ui != new_ui {
            info!("UI config change detected");
            let _ = self.config_update_tx.send(ConfigUpdate::Ui {
                theme: new_ui.0,
                yolo: new_ui.1,
                fork_secondary_model: new_ui.2,
            });
        }

        self.last_global_config = new_global;
        Ok(())
    }
}

/// Start the file watcher and its [`ConfigReloader`] task as one runtime.
///
/// The returned watcher owns the OS watches and must be retained for as long
/// as hot reload is expected to remain active. Keeping this pairing here makes
/// both the leader and the in-process pager use the same debounce, config
/// baseline, and change-detection path.
pub fn start_config_reload(
    grow_home: &Path,
    extra_paths: &[PathBuf],
    cwd: Option<&Path>,
    remote_settings: Option<crate::util::config::RemoteSettings>,
    config_update_tx: mpsc::UnboundedSender<ConfigUpdate>,
    experimental_memory: bool,
    no_memory: bool,
    cancel: CancellationToken,
) -> Option<super::watcher::ConfigFileWatcher> {
    let (watcher, events_rx) =
        super::watcher::ConfigFileWatcher::start(grow_home, extra_paths, cwd, None)?;
    let initial_config = crate::config::load_effective_config()
        .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()));
    let reloader = ConfigReloader::new(
        initial_config,
        remote_settings,
        config_update_tx,
        experimental_memory,
        no_memory,
    );
    tokio::spawn(reloader.run(events_rx, cancel));
    Some(watcher)
}

/// Derive the unique project cwds whose files were touched in this
/// debounce window. Used to fan out one
/// [`ConfigUpdate::ProjectMcpServersChanged`] per project root rather
/// than one legacy `McpServersChanged` that reloads every active
/// session.
///
/// Path-to-cwd mapping:
///
/// | `ConfigChangeEvent`        | path shape              | cwd               |
/// |----------------------------|-------------------------|-------------------|
/// | `ProjectConfigChanged`     | `<cwd>/.grow/config.toml` | `<cwd>`           |
/// | `McpConfigChanged`         | `<cwd>/.mcp.json`         | `<cwd>`           |
/// | `McpConfigChanged`         | `<cwd>/.claude.json`      | `<cwd>`           |
///
/// Order-preserving de-dup (a `Vec` rather than a `HashSet`) so the
/// downstream emit order is deterministic in tests.
fn collect_project_cwds(batch: &[ConfigChangeEvent]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for evt in batch {
        let cwd = match evt {
            ConfigChangeEvent::ProjectConfigChanged { path } => {
                // <cwd>/.grow/config.toml → <cwd>
                path.parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf())
            }
            ConfigChangeEvent::McpConfigChanged { path } => {
                // <cwd>/.mcp.json or <cwd>/.claude.json → <cwd>
                path.parent().map(|p| p.to_path_buf())
            }
            _ => None,
        };
        if let Some(cwd) = cwd
            && !out.contains(&cwd)
        {
            out.push(cwd);
        }
    }
    out
}

/// Content hash of the cwd-dependent MCP config files a
/// `ProjectMcpServersChanged { cwd }` reload re-reads. Walks ancestors
/// up to the git root exactly as the loaders do (`find_project_configs`
/// for `.grow/config.toml`, `find_mcp_json_files` for `.mcp.json`) so
/// the hash can't drift from the set the merge actually reads, plus
/// `<cwd>/.claude.json` (watched at the project root). A stable hash
/// means the reload would be a no-op. Home-level sources
/// (`~/.grow/config.toml`, `~/.claude.json`, `~/.cursor/mcp.json`)
/// change through their own events.
///
/// Returns `None` on a non-`NotFound` read error so the caller
/// dispatches rather than risk suppressing a real edit.
fn hash_project_mcp_config(cwd: &Path) -> Option<u64> {
    let mut paths = crate::config::find_project_configs(cwd);
    paths.extend(crate::util::config::find_mcp_json_files(cwd));
    paths.push(cwd.join(".claude.json"));

    let mut hasher = DefaultHasher::new();
    paths.len().hash(&mut hasher);
    for f in &paths {
        f.to_string_lossy().hash(&mut hasher);
        match std::fs::read(f) {
            Ok(bytes) => {
                1u8.hash(&mut hasher); // present
                bytes.hash(&mut hasher);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                0u8.hash(&mut hasher); // absent
            }
            Err(_) => return None, // can't read confidently → dispatch
        }
    }
    Some(hasher.finish())
}

/// Extract the `[skills]` table from an effective config.
///
/// Consumers: the reload dispatch above (change detection →
/// `ConfigUpdate::Skills`) and `grow inspect` (via the `crate::config`
/// re-export), so both honor the same paths/ignore/disabled as a live
/// session. Session spawn parses the same table separately through the typed
/// `Config.skills` (agent/config.rs) — keep these in sync rather than adding
/// a fourth parse path.
pub(crate) fn parse_skills_config(config: &toml::Value) -> agent::prompt::skills::SkillsConfig {
    config
        .get("skills")
        .and_then(|v| v.clone().try_into().ok())
        .unwrap_or_default()
}

fn parse_compat_config(config: &toml::Value) -> tools::types::compat::CompatConfigToml {
    config
        .get("compat")
        .and_then(|v| v.clone().try_into().ok())
        .unwrap_or_default()
}

fn model_config_changed(old: &toml::Value, new: &toml::Value) -> bool {
    ["provider", "models", "model", "auth_provider"]
        .into_iter()
        .any(|section| old.get(section) != new.get(section))
}

fn extract_ui_fields(config: &toml::Value) -> (Option<String>, bool, Option<String>) {
    let ui = config.get("ui").and_then(|v| v.as_table());
    let theme = ui
        .and_then(|u| u.get("theme"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let yolo = ui
        .and_then(|u| u.get("yolo"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let fork = ui
        .and_then(|u| u.get("fork_secondary_model"))
        .and_then(|v| v.as_str())
        .map(String::from);
    (theme, yolo, fork)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A project event with unchanged bytes must not re-dispatch a
    /// reload; the first event and a later real edit must both dispatch.
    #[tokio::test]
    async fn reloader_dedupes_unchanged_project_mcp_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let cwd = tmp.path().to_path_buf();
        let mcp_json = cwd.join(".mcp.json");
        std::fs::write(&mcp_json, r#"{"mcpServers":{}}"#).unwrap();

        let (tx, mut rx) = mpsc::unbounded_channel();
        let empty_config = toml::Value::Table(toml::map::Map::new());
        let reloader = ConfigReloader::new(empty_config, None, tx, false, false);

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let handle = tokio::spawn(reloader.run(event_rx, cancel.clone()));

        let evt = || ConfigChangeEvent::McpConfigChanged {
            path: mcp_json.clone(),
        };

        // First event → dispatch (no prior hash for this cwd).
        event_tx.send(evt()).unwrap();
        let update = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("first event should dispatch within 2s")
            .expect("channel open");
        assert!(
            matches!(update, ConfigUpdate::ProjectMcpServersChanged { cwd: ref c } if *c == cwd),
            "first project event must dispatch"
        );

        // Second event, identical bytes → must be suppressed.
        event_tx.send(evt()).unwrap();
        let res = tokio::time::timeout(std::time::Duration::from_millis(400), rx.recv()).await;
        assert!(
            res.is_err(),
            "unchanged project config must not re-dispatch a reload"
        );

        // Real content change → dispatch again.
        std::fs::write(
            &mcp_json,
            r#"{"mcpServers":{"x":{"url":"http://localhost"}}}"#,
        )
        .unwrap();
        event_tx.send(evt()).unwrap();
        let update = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("changed content should dispatch within 2s")
            .expect("channel open");
        assert!(
            matches!(update, ConfigUpdate::ProjectMcpServersChanged { cwd: ref c } if *c == cwd),
            "changed project config must dispatch"
        );

        cancel.cancel();
        let _ = handle.await;
    }

    /// `hash_project_mcp_config` is stable for identical content and
    /// changes on create/edit.
    #[test]
    fn hash_project_mcp_config_detects_create_and_change() {
        let tmp = tempfile::TempDir::new().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let cwd = tmp.path();

        let empty = hash_project_mcp_config(cwd).expect("readable");
        assert_eq!(empty, hash_project_mcp_config(cwd).expect("stable"));

        std::fs::write(cwd.join(".mcp.json"), "a").unwrap();
        let created = hash_project_mcp_config(cwd).expect("readable");
        assert_ne!(empty, created, "creating a config file changes the hash");

        std::fs::write(cwd.join(".mcp.json"), "b").unwrap();
        let changed = hash_project_mcp_config(cwd).expect("readable");
        assert_ne!(created, changed, "editing content changes the hash");
    }

    /// The hash must reflect ancestor `.grow/config.toml` and `.mcp.json`
    /// under `cwd` — otherwise an ancestor edit would be wrongly
    /// must be a distinct variant from the unit `McpServersChanged`
    /// suppressed.
    #[test]
    fn hash_project_mcp_config_covers_ancestors() {
        let tmp = tempfile::TempDir::new().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let child = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&child).unwrap();

        let h0 = hash_project_mcp_config(&child).expect("readable");

        std::fs::write(tmp.path().join(".mcp.json"), "a").unwrap();
        let h1 = hash_project_mcp_config(&child).expect("readable");
        assert_ne!(h0, h1, "ancestor .mcp.json create must change the hash");

        std::fs::write(tmp.path().join(".mcp.json"), "b").unwrap();
        let h2 = hash_project_mcp_config(&child).expect("readable");
        assert_ne!(h1, h2, "ancestor .mcp.json edit must change the hash");

        std::fs::create_dir_all(tmp.path().join(".grow")).unwrap();
        std::fs::write(tmp.path().join(".grow").join("config.toml"), "x = 1").unwrap();
        let h3 = hash_project_mcp_config(&child).expect("readable");
        assert_ne!(
            h2, h3,
            "ancestor .grow/config.toml create must change the hash"
        );
    }

    #[test]
    fn parse_skills_config_empty() {
        let config = toml::Value::Table(toml::map::Map::new());
        let skills = parse_skills_config(&config);
        assert_eq!(skills, agent::prompt::skills::SkillsConfig::default());
    }

    #[test]
    fn parse_skills_config_with_paths() {
        let config: toml::Value = toml::from_str(
            r#"
[skills]
paths = ["/home/user/.grow/skills"]
ignore = ["/tmp"]
"#,
        )
        .unwrap();
        let skills = parse_skills_config(&config);
        assert_eq!(skills.paths, vec!["/home/user/.grow/skills".to_string()]);
        assert_eq!(skills.ignore, vec!["/tmp".to_string()]);
    }

    #[test]
    fn memory_config_diff_detects_enabled_change() {
        let empty = toml::Value::Table(toml::map::Map::new());
        let enabled: toml::Value = toml::from_str("[memory]\nenabled = true").unwrap();

        let old = crate::config::MemoryConfig::resolve(false, false, &empty, None);
        let new = crate::config::MemoryConfig::resolve(false, false, &enabled, None);
        assert_ne!(old, new, "should detect enabled field change");
    }

    #[test]
    fn memory_config_diff_detects_search_param_change() {
        let a: toml::Value = toml::from_str("[memory.search]\nmax_results = 6").unwrap();
        let b: toml::Value = toml::from_str("[memory.search]\nmax_results = 10").unwrap();

        let old = crate::config::MemoryConfig::resolve(false, false, &a, None);
        let new = crate::config::MemoryConfig::resolve(false, false, &b, None);
        assert_ne!(old, new, "should detect search param change");
    }

    #[test]
    fn extract_ui_fields_empty() {
        let config = toml::Value::Table(toml::map::Map::new());
        let (theme, yolo, fork) = extract_ui_fields(&config);
        assert_eq!(theme, None);
        assert!(!yolo);
        assert_eq!(fork, None);
    }

    #[test]
    fn extract_ui_fields_with_values() {
        let config: toml::Value = toml::from_str(
            r#"
[ui]
theme = "dark"
yolo = true
fork_secondary_model = "grow-4.5"
"#,
        )
        .unwrap();
        let (theme, yolo, fork) = extract_ui_fields(&config);
        assert_eq!(theme.as_deref(), Some("dark"));
        assert!(yolo);
        assert_eq!(fork.as_deref(), Some("grow-4.5"));
    }

    #[test]
    fn extract_ui_fields_diff_detects_theme_change() {
        let a: toml::Value = toml::from_str("[ui]\ntheme = \"light\"").unwrap();
        let b: toml::Value = toml::from_str("[ui]\ntheme = \"dark\"").unwrap();
        assert_ne!(extract_ui_fields(&a), extract_ui_fields(&b));
    }

    #[test]
    fn extract_ui_fields_diff_detects_yolo_change() {
        let a: toml::Value = toml::from_str("[ui]\nyolo = false").unwrap();
        let b: toml::Value = toml::from_str("[ui]\nyolo = true").unwrap();
        assert_ne!(extract_ui_fields(&a), extract_ui_fields(&b));
    }

    #[test]
    fn models_changed_detects_new_model_override() {
        let a = toml::Value::Table(toml::map::Map::new());
        let b: toml::Value = toml::from_str(
            r#"
[model.my-custom]
model = "grow-4.5"
base_url = "https://api.example.com/v1"
"#,
        )
        .unwrap();
        assert!(model_config_changed(&a, &b));
    }

    #[test]
    fn models_changed_detects_default_change() {
        let a: toml::Value = toml::from_str("[models]\ndefault = \"grow-code-fast-1\"").unwrap();
        let b: toml::Value = toml::from_str("[models]\ndefault = \"grow-code-slow-1\"").unwrap();
        assert!(model_config_changed(&a, &b));
    }

    #[test]
    fn models_changed_detects_provider_catalog_change() {
        let a = toml::Value::Table(toml::map::Map::new());
        let b: toml::Value = toml::from_str(
            r#"
[provider.deepseek]
api_backend = "chat_completions"

[provider.deepseek.models.v4-pro]
name = "DeepSeek V4 Pro"
"#,
        )
        .unwrap();
        assert!(model_config_changed(&a, &b));
    }

    #[test]
    fn models_changed_detects_auth_provider_change() {
        let a = toml::Value::Table(toml::map::Map::new());
        let b: toml::Value = toml::from_str(
            r#"
[auth_provider.corp]
type = "oauth"
"#,
        )
        .unwrap();
        assert!(model_config_changed(&a, &b));
    }

    #[test]
    fn unrelated_config_does_not_report_models_changed() {
        let a: toml::Value = toml::from_str("[ui]\ntheme = \"light\"").unwrap();
        let b: toml::Value = toml::from_str("[ui]\ntheme = \"dark\"").unwrap();
        assert!(!model_config_changed(&a, &b));
    }

    #[test]
    fn mcp_servers_changed_detects_new_server() {
        let a = toml::Value::Table(toml::map::Map::new());
        let b: toml::Value = toml::from_str(
            r#"
[mcp_servers.test]
command = "/bin/test"
"#,
        )
        .unwrap();
        assert_ne!(a.get("mcp_servers"), b.get("mcp_servers"));
    }

    /// `ConfigUpdate::ProjectMcpServersChanged { cwd }`
    /// so the two paths route through different match arms in
    /// `app.rs`. Guards against an accidental merge that would force
    /// fan-out — it must NOT contribute a cwd to
    /// per-cwd reloads through the legacy sweep-all-sessions arm.
    #[test]
    fn project_variant_dispatches_separately() {
        let cwd = PathBuf::from("/tmp/proj-x");
        let global: ConfigUpdate = ConfigUpdate::McpServersChanged;
        let project = ConfigUpdate::ProjectMcpServersChanged { cwd: cwd.clone() };

        // Each variant must be matched by its own arm — fall-through
        // would indicate a single arm handling both.
        let mut routed_global = false;
        let mut routed_project = None;
        for u in [global, project] {
            match u {
                ConfigUpdate::McpServersChanged => routed_global = true,
                ConfigUpdate::ProjectMcpServersChanged { cwd } => routed_project = Some(cwd),
                _ => panic!("unexpected variant"),
            }
        }
        assert!(
            routed_global,
            "global variant must route through its own arm"
        );
        assert_eq!(routed_project.as_deref(), Some(cwd.as_path()));
    }

    /// `HomeClaudeJsonChanged` is **not** part of the per-cwd
    /// `collect_project_cwds` (otherwise sessions outside `$HOME`
    /// would be silently skipped). The reloader broadcasts it via
    /// the unit `McpServersChanged` variant; this test locks that
    /// `ProjectConfigChanged` (`<cwd>/.grow/config.toml`) and
    /// invariant at the helper layer.
    #[test]
    fn collect_project_cwds_excludes_home_claude_json() {
        let batch = vec![
            ConfigChangeEvent::HomeClaudeJsonChanged,
            ConfigChangeEvent::ProjectConfigChanged {
                path: PathBuf::from("/repo/x/.grow/config.toml"),
            },
        ];
        let cwds = collect_project_cwds(&batch);
        // Only the project entry contributes; the home-level `.claude.json`
        // entry is silently dropped because it routes through the
        // broadcast arm instead.
        assert_eq!(cwds, vec![PathBuf::from("/repo/x")]);
    }

    /// `collect_project_cwds` extracts `<cwd>` from
    /// `McpConfigChanged` (`<cwd>/.mcp.json`), de-duplicates while
    /// `McpConfigChanged` (`<cwd>/.mcp.json`), de-duplicates while
    /// preserving order.
    #[test]
    fn collect_project_cwds_dedupes_and_extracts() {
        let batch = vec![
            ConfigChangeEvent::ProjectConfigChanged {
                path: PathBuf::from("/repo/a/.grow/config.toml"),
            },
            ConfigChangeEvent::McpConfigChanged {
                path: PathBuf::from("/repo/a/.mcp.json"),
            },
            ConfigChangeEvent::ProjectConfigChanged {
                path: PathBuf::from("/repo/b/.grow/config.toml"),
            },
        ];
        let cwds = collect_project_cwds(&batch);
        assert_eq!(
            cwds,
            vec![PathBuf::from("/repo/a"), PathBuf::from("/repo/b")]
        );
    }

    #[test]
    fn mcp_servers_unchanged_same_config() {
        let cfg: toml::Value = toml::from_str(
            r#"
[mcp_servers.test]
command = "/bin/test"
"#,
        )
        .unwrap();
        assert_eq!(cfg.get("mcp_servers"), cfg.get("mcp_servers"));
    }
}

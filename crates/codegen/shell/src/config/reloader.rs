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
    /// The canonical MCP catalog changed. `project_root = None` reloads every
    /// active session (global config or home-level imports); `Some(root)`
    /// reloads only sessions rooted at or beneath that project.
    McpCatalogChanged { project_root: Option<PathBuf> },
    /// Updated memory config (boxed to avoid large enum variant).
    Memory(Box<crate::config::MemoryConfig>),
    /// Updated skills discovery config.
    Skills(agent::prompt::skills::SkillsConfig),
    /// The `[provider.*.models.*]` entries changed. Agent should re-resolve
    /// its model list (BYOK models added/removed, default or surprise changed).
    ModelsChanged,
    /// Final local announcement snapshot changed. The receiver forwards it to
    /// connected clients through `grow/announcements/update`.
    Announcements(Vec<announcements::Announcement>),
    /// Updated UI settings — agent broadcasts `grow/config_changed` to IPC clients.
    Ui {
        theme: Option<String>,
        permission_mode: Option<String>,
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
            let has_config = has_global_config || has_project_config;

            // Collect the unique project roots touched in this debounce batch.
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
                let _ = self.config_update_tx.send(ConfigUpdate::McpCatalogChanged {
                    project_root: Some(cwd),
                });
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

        // Project-scoped reloads are dispatched by the caller's root fan-out;
        // this function only diffs the global TOML.
        let new_global = match crate::config::load_from_disk() {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e, "failed to parse config.toml, keeping last-known-good");
                return Ok(());
            }
        };

        // MCP servers — compare [mcp_servers] table in the **global**
        // config (`~/.grow/config.toml`) via toml::Value. Project-
        // scoped changes are dispatched separately with `project_root` set.
        let old_mcp_table = self.last_global_config.get("mcp_servers");
        let new_mcp_table = new_global.get("mcp_servers");
        let mcp_changed = old_mcp_table != new_mcp_table;
        if mcp_changed {
            info!("Global MCP server config change detected");
            let _ = self
                .config_update_tx
                .send(ConfigUpdate::McpCatalogChanged { project_root: None });
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

        // Models — compare provider definitions, global selection, and the
        // per-model overrides consumed by `Config::config_models`.
        if model_config_changed(&self.last_global_config, &new_global) {
            info!("model config change detected");
            let _ = self.config_update_tx.send(ConfigUpdate::ModelsChanged);
        }

        // UI fields (theme, permission_mode, fork_secondary_model)
        let old_ui = extract_ui_fields(&self.last_global_config);
        let new_ui = extract_ui_fields(&new_global);
        if old_ui != new_ui {
            info!("UI config change detected");
            let _ = self.config_update_tx.send(ConfigUpdate::Ui {
                theme: new_ui.0,
                permission_mode: new_ui.1,
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
/// [`ConfigUpdate::McpCatalogChanged`] per project root.
///
/// Path-to-cwd mapping:
///
/// | `ConfigChangeEvent`        | path shape              | cwd               |
/// |----------------------------|-------------------------|-------------------|
/// | `ProjectConfigChanged`     | `<cwd>/.grow/config.toml` | `<cwd>`           |
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
/// A project-scoped catalog reload re-reads ancestor `.grow/config.toml`
/// files up to the git root. A stable hash means the reload would be a no-op.
///
/// Returns `None` on a non-`NotFound` read error so the caller
/// dispatches rather than risk suppressing a real edit.
fn hash_project_mcp_config(cwd: &Path) -> Option<u64> {
    let paths = crate::config::find_project_configs(cwd);

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

fn model_config_changed(old: &toml::Value, new: &toml::Value) -> bool {
    ["provider", "models", "model", "auth_provider"]
        .into_iter()
        .any(|section| old.get(section) != new.get(section))
}

fn extract_ui_fields(config: &toml::Value) -> (Option<String>, Option<String>, Option<String>) {
    let ui = config.get("ui").and_then(|v| v.as_table());
    let theme = ui
        .and_then(|u| u.get("theme"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let permission_mode = ui
        .and_then(|u| u.get("permission_mode"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let fork = ui
        .and_then(|u| u.get("fork_secondary_model"))
        .and_then(|v| v.as_str())
        .map(String::from);
    (theme, permission_mode, fork)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_project_mcp_config_tracks_canonical_ancestor_config() {
        let tmp = tempfile::TempDir::new().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        let child = tmp.path().join("nested");
        std::fs::create_dir_all(&child).unwrap();

        let initial = hash_project_mcp_config(&child).expect("readable");
        assert_eq!(initial, hash_project_mcp_config(&child).expect("stable"),);

        let grow = tmp.path().join(".grow");
        std::fs::create_dir_all(&grow).unwrap();
        let config = grow.join("config.toml");
        std::fs::write(&config, "[mcp_servers.one]\ncommand = \"one\"\n").unwrap();
        let created = hash_project_mcp_config(&child).expect("readable");
        assert_ne!(initial, created);

        std::fs::write(&config, "[mcp_servers.two]\ncommand = \"two\"\n").unwrap();
        let changed = hash_project_mcp_config(&child).expect("readable");
        assert_ne!(created, changed);
    }

    #[test]
    fn collect_project_cwds_is_ordered_and_deduplicated() {
        let batch = vec![
            ConfigChangeEvent::ProjectConfigChanged {
                path: PathBuf::from("/repo/a/.grow/config.toml"),
            },
            ConfigChangeEvent::GlobalConfigChanged,
            ConfigChangeEvent::ProjectConfigChanged {
                path: PathBuf::from("/repo/a/.grow/config.toml"),
            },
            ConfigChangeEvent::ProjectConfigChanged {
                path: PathBuf::from("/repo/b/.grow/config.toml"),
            },
        ];
        assert_eq!(
            collect_project_cwds(&batch),
            vec![PathBuf::from("/repo/a"), PathBuf::from("/repo/b")],
        );
    }

    #[test]
    fn skills_and_model_diffs_are_section_scoped() {
        let empty = toml::Value::Table(toml::map::Map::new());
        assert_eq!(
            parse_skills_config(&empty),
            agent::prompt::skills::SkillsConfig::default(),
        );

        let models: toml::Value =
            toml::from_str("[provider.local]\nbase_url = \"http://localhost\"\n").unwrap();
        assert!(model_config_changed(&empty, &models));

        let light: toml::Value = toml::from_str("[ui]\ntheme = \"light\"\n").unwrap();
        let dark: toml::Value = toml::from_str("[ui]\ntheme = \"dark\"\n").unwrap();
        assert!(!model_config_changed(&light, &dark));
    }

    #[test]
    fn ui_fields_use_canonical_names() {
        let config: toml::Value = toml::from_str(
            r#"
[ui]
theme = "dark"
permission_mode = "auto"
fork_secondary_model = "fast"
"#,
        )
        .unwrap();
        assert_eq!(
            extract_ui_fields(&config),
            (
                Some("dark".to_owned()),
                Some("auto".to_owned()),
                Some("fast".to_owned()),
            ),
        );
    }
}

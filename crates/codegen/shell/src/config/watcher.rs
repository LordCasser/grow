use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer_opt};
use tokio::sync::mpsc;

const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(1000);

/// A [`notify::Watcher`] that drops `EventKind::Access` before it reaches the
/// debouncer, breaking the MCP/skills reload storm.
///
/// `notify`'s inotify backend emits an `Access` event on every *read*, and the
/// leader re-reads the files it watches on each reload — so unfiltered, a
/// reload's own reads schedule the next reload, a ~1/sec self-sustaining loop.
/// Dropping `Access` is safe: writes still emit `Modify`/`Create` and chmod
/// emits `Modify(Metadata)`; only reads are `Access`-only.
pub struct AccessFilteredWatcher(notify::RecommendedWatcher);

impl notify::Watcher for AccessFilteredWatcher {
    fn new<F: notify::EventHandler>(
        mut event_handler: F,
        config: notify::Config,
    ) -> notify::Result<Self>
    where
        Self: Sized,
    {
        let inner = notify::RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| match &res {
                Ok(event) if matches!(event.kind, notify::EventKind::Access(_)) => {}
                _ => event_handler.handle_event(res),
            },
            config,
        )?;
        Ok(Self(inner))
    }

    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> notify::Result<()> {
        self.0.watch(path, recursive_mode)
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        self.0.unwatch(path)
    }

    fn configure(&mut self, option: notify::Config) -> notify::Result<bool> {
        self.0.configure(option)
    }

    fn kind() -> notify::WatcherKind
    where
        Self: Sized,
    {
        notify::RecommendedWatcher::kind()
    }
}

/// `new_debouncer` equivalent that builds the debouncer on top of
/// [`AccessFilteredWatcher`] instead of the raw `RecommendedWatcher`.
fn new_filtered_debouncer<F: notify_debouncer_mini::DebounceEventHandler>(
    timeout: Duration,
    event_handler: F,
) -> Result<Debouncer<AccessFilteredWatcher>, notify::Error> {
    let config = notify_debouncer_mini::Config::default().with_timeout(timeout);
    new_debouncer_opt::<F, AccessFilteredWatcher>(config, event_handler)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigChangeEvent {
    GlobalConfigChanged,
    ProjectConfigChanged { path: PathBuf },
}

/// Watches `~/.grow/` for `config.toml`
/// changes, plus project `.grow/config.toml` paths provided at startup.
///
/// Uses `notify-debouncer-mini` for built-in debounce that coalesces rapid
/// editor writes (including write-then-rename patterns).
///
/// Self-write suppression is intentionally omitted. When the agent writes
/// `config.toml`, the watcher will fire and the
/// [`ConfigReloader`](super::reloader::ConfigReloader) will re-read the file.
/// The reloader's own content-based deduplication (toml value
/// comparison) skips the update when nothing actually changed, so the
/// redundant read is harmless. This avoids a class of bugs where an
/// optimistic suppression window accidentally swallows writes from external
/// processes.
///
/// Adds narrow **non-recursive** watches for `<cwd>/` and `<cwd>/.grow/`
/// so `<cwd>/.grow/config.toml` changes can be observed. Recursing on `<cwd>`
/// would walk `node_modules/`, `target/`, `.git/`, etc. and blow through
/// `fs.inotify.max_user_watches` on large repos. Use [`Self::watch_path`]
/// to register additional cwds at runtime when new sessions open in
/// previously-unwatched directories.
pub struct ConfigFileWatcher {
    state: std::sync::Arc<parking_lot::Mutex<ConfigWatchState>>,
}

struct ConfigWatchState {
    // Installed after callback construction; callback holds only a Weak state.
    debouncer: Option<Debouncer<AccessFilteredWatcher>>,
    watched_cwds: HashSet<PathBuf>,
}

impl ConfigFileWatcher {
    /// Start watching. Returns `None` if the OS watcher fails to initialize.
    ///
    /// `cwd`, when `Some`, adds two non-recursive watches: `<cwd>/` and
    /// `<cwd>/.grow/`. Use [`Self::watch_path`] later to register additional
    /// project cwds for sessions that open in previously-unwatched
    /// directories.
    pub fn start(
        grow_home: &Path,
        extra_paths: &[PathBuf],
        cwd: Option<&Path>,
        debounce: Option<Duration>,
    ) -> Option<(Self, mpsc::UnboundedReceiver<ConfigChangeEvent>)> {
        let debounce = debounce.unwrap_or(DEFAULT_DEBOUNCE);
        let (tx, rx) = mpsc::unbounded_channel();
        let grow_home_buf = grow_home.to_path_buf();
        let state = std::sync::Arc::new(parking_lot::Mutex::new(ConfigWatchState {
            debouncer: None,
            watched_cwds: HashSet::new(),
        }));
        let weak_state = std::sync::Arc::downgrade(&state);
        let mut debouncer = new_filtered_debouncer(debounce, move |res: DebounceEventResult| {
            let Ok(events) = res else { return };
            let Some(state) = weak_state.upgrade() else {
                return;
            };
            // Serialize directory maintenance with explicit watch/unwatch.
            // The debouncer callback is separate from the raw notify handler.
            let mut state = state.lock();

            let mut batch_events: Vec<ConfigChangeEvent> = Vec::new();
            for event in events {
                let path = &event.path;
                let name = path.file_name().and_then(|n| n.to_str());
                let parent = path.parent();

                let change = match name {
                    Some(".grow") if parent.is_some_and(|p| state.watched_cwds.contains(p)) => {
                        if let Some(debouncer) = &mut state.debouncer {
                            // A replacement directory must not inherit a watch on the old inode.
                            let _ = debouncer.watcher().unwatch(path);
                            if path.is_dir() {
                                if let Err(error) =
                                    debouncer.watcher().watch(path, RecursiveMode::NonRecursive)
                                {
                                    log_watch_error(
                                        &error,
                                        "failed to attach project config directory",
                                    );
                                }
                            }
                        }
                        Some(ConfigChangeEvent::ProjectConfigChanged {
                            path: path.join("config.toml"),
                        })
                    }
                    Some("config.toml") if parent == Some(grow_home_buf.as_path()) => {
                        Some(ConfigChangeEvent::GlobalConfigChanged)
                    }
                    Some("config.toml") => {
                        Some(ConfigChangeEvent::ProjectConfigChanged { path: path.clone() })
                    }
                    _ => None,
                };

                if let Some(evt) = change
                    && !batch_events.contains(&evt)
                {
                    batch_events.push(evt);
                }
            }
            for evt in batch_events {
                let _ = tx.send(evt);
            }
        })
        .map_err(|e| tracing::warn!(error = %e, "failed to create config file watcher"))
        .ok()?;

        debouncer
            .watcher()
            .watch(grow_home, RecursiveMode::NonRecursive)
            .map_err(|e| {
                tracing::warn!(
                    path = %grow_home.display(),
                    error = %e,
                    "failed to watch grow home directory"
                )
            })
            .ok()?;

        for p in extra_paths {
            if let Some(parent) = p.parent() {
                let _ = debouncer
                    .watcher()
                    .watch(parent, RecursiveMode::NonRecursive);
            }
        }

        // Add the two narrow non-recursive cwd watches
        // promoted to first-class watch targets. Both are non-fatal —
        // the parent watch handles a later `.grow` creation and attaches its watch.
        //
        // When the leader's own cwd is also covered by
        // `extra_paths` (e.g. `find_project_configs(cwd)` already
        // includes `<cwd>/.grow/config.toml` so the loop above
        // watches `<cwd>/.grow/`), the call below installs a
        // duplicate watch on the same directory. `notify` dedupes
        // silently in its `RecommendedWatcher` (last-write-wins for
        // the recursion mode), so this is cosmetic — both
        // additions remain non-recursive, no event amplification.
        let mut watched_cwds = HashSet::new();
        if let Some(cwd) = cwd {
            watch_cwd_dirs(&mut debouncer, cwd);
            watched_cwds.insert(cwd.to_path_buf());
        }

        tracing::info!(
            grow_home = %grow_home.display(),
            extra_paths = extra_paths.len(),
            cwd = ?cwd,
            debounce_ms = debounce.as_millis(),
            "config file watcher started"
        );

        {
            let mut current = state.lock();
            current.debouncer = Some(debouncer);
            current.watched_cwds = watched_cwds;
        }
        Some((Self { state }, rx))
    }

    /// Register `<cwd>/` and `<cwd>/.grow/` as **non-recursive** watch
    /// targets, in addition to whatever was passed to [`Self::start`].
    ///
    /// Intended for the session-open path: when a session opens in a cwd
    /// the leader hasn't seen before, calling this method ensures edits to
    /// `<cwd>/.grow/config.toml` triggers a
    /// [`ConfigChangeEvent`] (and downstream project-scoped
    /// [`ConfigUpdate::McpCatalogChanged`](super::reloader::ConfigUpdate::McpCatalogChanged))
    /// within the debounce window.
    ///
    /// **Non-recursive by design.** Watching `<cwd>` recursively would
    /// walk `node_modules/`, `target/`, `.git/`, etc. and easily exhaust
    /// the per-user inotify quota (`fs.inotify.max_user_watches`,
    /// commonly 8192 by default) on a large repo. If `notify` cannot register the watch (e.g.
    /// the directory doesn't exist yet, or the OS quota is reached) the
    /// error is logged and swallowed — the leader continues to rely on
    /// the user-triggered refresh as the fallback.
    pub fn watch_path(&mut self, cwd: &Path) {
        // Idempotent at our layer: skip the redundant
        // `notify` watch-add when this cwd is already registered, so
        // re-opening sessions in the same directory doesn't churn the
        // OS watcher. `notify` de-dups internally too, but tracking the
        // set here also enables `unwatch_path`.
        let mut state = self.state.lock();
        if state.watched_cwds.contains(cwd) {
            return;
        }
        watch_cwd_dirs(state.debouncer.as_mut().expect("initialized watcher"), cwd);
        state.watched_cwds.insert(cwd.to_path_buf());
    }

    /// Remove the two non-recursive watches (`<cwd>/` and
    /// `<cwd>/.grow/`) previously registered for `cwd` via
    /// [`Self::start`] / [`Self::watch_path`].
    ///
    /// Best-effort and idempotent: a `cwd` that was never registered
    /// (or already unwatched) is a no-op. Intended for the
    /// session-teardown path so a long-lived leader that opens sessions
    /// across many directories doesn't accumulate inotify watches for
    /// cwds with no live sessions. **Callers must ref-count**: only
    /// unwatch once the *last* session sharing this cwd closes —
    /// `ConfigFileWatcher` tracks distinct cwds, not session counts.
    pub fn unwatch_path(&mut self, cwd: &Path) {
        let mut state = self.state.lock();
        if !state.watched_cwds.remove(cwd) {
            return;
        }
        unwatch_cwd_dirs(state.debouncer.as_mut().expect("initialized watcher"), cwd);
    }
}

/// Add the two non-recursive watches for a project root.
///
/// Both watches are best-effort and log-and-continue on failure (missing
/// directory, quota exhausted, permission denied, etc.) — the caller has
/// no reasonable recovery path beyond the existing user-triggered refresh.
///
/// If `.grow` is missing, the cwd watch observes its later creation and the
/// callback attaches the non-recursive directory watch before publishing a reload.
fn watch_cwd_dirs(debouncer: &mut Debouncer<AccessFilteredWatcher>, cwd: &Path) {
    if let Err(e) = debouncer.watcher().watch(cwd, RecursiveMode::NonRecursive) {
        log_watch_error(&e, "failed to watch project cwd (non-recursive)");
    }
    let grow_dir = cwd.join(".grow");
    if let Err(e) = debouncer
        .watcher()
        .watch(&grow_dir, RecursiveMode::NonRecursive)
    {
        log_watch_error(
            &e,
            "failed to watch project .grow directory (non-recursive)",
        );
    }
}

/// Remove the two non-recursive watches added by [`watch_cwd_dirs`].
/// Best-effort: a `WatchNotFound` (never watched / already removed) is
/// expected and logged at `debug!`.
fn unwatch_cwd_dirs(debouncer: &mut Debouncer<AccessFilteredWatcher>, cwd: &Path) {
    if let Err(e) = debouncer.watcher().unwatch(cwd) {
        tracing::debug!(error = %e, "failed to unwatch project cwd");
    }
    let grow_dir = cwd.join(".grow");
    if let Err(e) = debouncer.watcher().unwatch(&grow_dir) {
        tracing::debug!(error = %e, "failed to unwatch project .grow directory");
    }
}

/// Log a `notify` watch failure, distinguishing the benign
/// "directory doesn't exist yet" case (logged at `debug!` — it's
/// expected for a freshly-opened session whose `<cwd>/.grow/` hasn't
/// been created) from genuinely actionable failures like
/// `fs.inotify.max_user_watches` exhaustion or permission denied
/// (logged at `warn!` — these mean live edits will be silently
/// missed). Don't swallow every error at the same level.
fn log_watch_error(err: &notify::Error, msg: &str) {
    let not_found = matches!(err.kind, notify::ErrorKind::PathNotFound)
        || matches!(&err.kind, notify::ErrorKind::Io(io) if io.kind() == std::io::ErrorKind::NotFound);
    if not_found {
        tracing::debug!(error = %err, "{msg} (path not found)");
    } else {
        tracing::warn!(error = %err, "{msg}");
    }
}

const SKILLS_DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryChange {
    Skills,
    Workflows,
}

#[cfg(test)]
fn discovery_change_for_path(path: &Path) -> Option<DiscoveryChange> {
    let grow_root = path.ancestors().find(|ancestor| {
        ancestor
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| GROW_CONFIG_ROOT_NAMES.contains(&name))
    })?;
    discovery_change_under_root(path, grow_root)
}

fn discovery_change_under_root(path: &Path, grow_root: &Path) -> Option<DiscoveryChange> {
    let relative = path.strip_prefix(grow_root).ok()?;
    let Some(section) = relative.components().next() else {
        return Some(DiscoveryChange::Skills);
    };
    match section.as_os_str().to_str() {
        Some("skills" | "commands") => Some(DiscoveryChange::Skills),
        Some("workflows") => Some(DiscoveryChange::Workflows),
        _ => None,
    }
}

/// Canonical Grow config root basename.
const GROW_CONFIG_ROOT_NAMES: &[&str] = &[".grow"];

/// Grow roots (by name or `grow_home`) must use scoped watches — they can
/// contain large non-skill trees (`worktrees/`, etc.).
fn is_grow_config_root(dir: &Path, grow_home: &Path) -> bool {
    if paths_equal(dir, grow_home) {
        return true;
    }
    dir.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| GROW_CONFIG_ROOT_NAMES.contains(&n))
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let canon = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

fn dirs_contain(dirs: &[PathBuf], target: &Path) -> bool {
    dirs.iter().any(|dir| paths_equal(dir, target))
}

fn grow_discovery_refresh_dirs(config_dir: &Path) -> [(PathBuf, RecursiveMode); 3] {
    [
        (config_dir.join("skills"), RecursiveMode::Recursive),
        (config_dir.join("commands"), RecursiveMode::NonRecursive),
        (config_dir.join("workflows"), RecursiveMode::NonRecursive),
    ]
}

fn project_grow_refresh_dirs(project_root: &Path) -> Vec<(PathBuf, RecursiveMode)> {
    let project_grow = project_root.join(".grow");
    let mut dirs = vec![(project_grow.clone(), RecursiveMode::NonRecursive)];
    dirs.extend(grow_discovery_refresh_dirs(&project_grow));
    dirs
}

fn attach_new_refresh_dirs(
    debouncer: &mut Debouncer<AccessFilteredWatcher>,
    refresh_dirs: &[(PathBuf, RecursiveMode)],
    refreshed_dirs: &mut HashMap<PathBuf, same_file::Handle>,
    err_msg: &str,
) -> bool {
    let mut changed = false;
    for (dir, mode) in refresh_dirs {
        let current = if dir.is_dir() {
            match same_file::Handle::from_path(dir) {
                Ok(handle) => Some(handle),
                Err(error) => {
                    tracing::warn!(path = %dir.display(), %error, "failed to identify discovery directory");
                    continue;
                }
            }
        } else {
            None
        };
        let previous = refreshed_dirs
            .keys()
            .find(|path| paths_equal(path, dir))
            .cloned();
        if let Some(path) = previous {
            if current.as_ref() == refreshed_dirs.get(&path) {
                continue;
            }
            let _ = debouncer.watcher().unwatch(&path);
            refreshed_dirs.remove(&path);
        }
        let Some(current) = current else { continue };
        match debouncer.watcher().watch(dir, *mode) {
            Ok(()) => {
                refreshed_dirs.insert(dir.clone(), current);
                changed = true;
            }
            Err(error) => log_watch_error(&error, err_msg),
        }
    }
    changed
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillsWatchPlan {
    grow_roots: Vec<PathBuf>,
    recursive_roots: Vec<PathBuf>,
    /// Non-recursive parent observes creation and replacement of the project Grow root.
    project_parent_watch: Option<PathBuf>,
    refresh_dirs: Vec<(PathBuf, RecursiveMode)>,
}

/// Pure composition: classify discovery roots and seed mid-session refresh targets.
fn plan_skills_watch_targets(
    dirs_to_watch: &[PathBuf],
    grow_home: &Path,
    project_root: Option<&Path>,
) -> SkillsWatchPlan {
    let mut grow_roots = Vec::new();
    let mut recursive_roots = Vec::new();
    let mut refresh_dirs = Vec::new();

    for dir in dirs_to_watch {
        if is_grow_config_root(dir, grow_home) {
            grow_roots.push(dir.clone());
            refresh_dirs.push((dir.clone(), RecursiveMode::NonRecursive));
            refresh_dirs.extend(grow_discovery_refresh_dirs(dir));
        } else {
            recursive_roots.push(dir.clone());
        }
    }

    let mut project_parent_watch = None;
    if let Some(project_root) = project_root {
        for name in GROW_CONFIG_ROOT_NAMES {
            let grow_root = project_root.join(name);
            if !dirs_contain(dirs_to_watch, &grow_root) {
                refresh_dirs.push((grow_root.clone(), RecursiveMode::NonRecursive));
                refresh_dirs.extend(grow_discovery_refresh_dirs(&grow_root));
            }
        }
        if !dirs_contain(dirs_to_watch, project_root) {
            project_parent_watch = Some(project_root.to_path_buf());
        }
    }

    SkillsWatchPlan {
        grow_roots,
        recursive_roots,
        project_parent_watch,
        refresh_dirs,
    }
}

/// Watches project `.grow` skills/commands/workflows for mid-session discovery.
///
/// After a [`DiscoveryChange`], call [`Self::refresh_new_dirs`] so newly created
/// seed dirs get watches attached.
pub struct ProjectDiscoveryWatcher {
    debouncer: Debouncer<AccessFilteredWatcher>,
    refresh_dirs: Vec<(PathBuf, RecursiveMode)>,
    refreshed_dirs: HashMap<PathBuf, same_file::Handle>,
}

impl ProjectDiscoveryWatcher {
    pub fn start(cwd: &Path) -> Option<(Self, mpsc::UnboundedReceiver<DiscoveryChange>)> {
        let project_root = crate::session::workflow::registry::project_root(cwd);
        let project_grow = project_root.join(".grow");
        let (tx, rx) = mpsc::unbounded_channel();
        let project_grow_for_events = project_grow.clone();
        let mut debouncer =
            new_filtered_debouncer(SKILLS_DEBOUNCE, move |res: DebounceEventResult| {
                let Ok(events) = res else { return };
                let mut change = None;
                for event in events
                    .iter()
                    .filter(|event| event.path.starts_with(&project_grow_for_events))
                {
                    let Some(next) =
                        discovery_change_under_root(&event.path, &project_grow_for_events)
                    else {
                        continue;
                    };
                    if next == DiscoveryChange::Skills {
                        change = Some(next);
                        break;
                    }
                    change = Some(next);
                }
                if let Some(change) = change {
                    let _ = tx.send(change);
                }
            })
            .map_err(|error| tracing::warn!(%error, "failed to create project workflow watcher"))
            .ok()?;

        if let Err(error) = debouncer
            .watcher()
            .watch(&project_root, RecursiveMode::NonRecursive)
        {
            log_watch_error(&error, "failed to watch project workflow parent");
            return None;
        }
        let refresh_dirs = project_grow_refresh_dirs(&project_root);
        let mut refreshed_dirs = HashMap::new();
        attach_new_refresh_dirs(
            &mut debouncer,
            &refresh_dirs,
            &mut refreshed_dirs,
            "failed to watch project discovery dir",
        );
        Some((
            Self {
                debouncer,
                refresh_dirs,
                refreshed_dirs,
            },
            rx,
        ))
    }

    /// Reconcile watches for created or replaced directories after a discovery event.
    pub fn refresh_new_dirs(&mut self) {
        attach_new_refresh_dirs(
            &mut self.debouncer,
            &self.refresh_dirs,
            &mut self.refreshed_dirs,
            "failed to watch newly-created project workflow dir",
        );
    }
}

/// Watches skill/command/workflow discovery dirs and classifies disk changes.
pub struct SkillsFileWatcher {
    debouncer: Debouncer<AccessFilteredWatcher>,
    refresh_dirs: Vec<(PathBuf, RecursiveMode)>,
    refreshed_dirs: HashMap<PathBuf, same_file::Handle>,
}

impl SkillsFileWatcher {
    /// Start watching discovery dirs from
    /// [`collect_skill_config_dirs`](agent::prompt::skills::collect_skill_config_dirs).
    ///
    /// After a [`DiscoveryChange`], call [`Self::refresh_new_discovery_dirs`] so
    /// newly created seed dirs get watches attached.
    pub fn start(
        cwd: Option<&Path>,
        monorepo_user_dir: Option<&Path>,
        config_paths: &[String],
    ) -> Option<(Self, mpsc::UnboundedReceiver<DiscoveryChange>)> {
        let grow_home = tools::util::grow_home::grow_home();
        let user_skill_roots = agent::prompt::skills::user_skill_roots();
        let dirs_to_watch = agent::prompt::skills::collect_skill_config_dirs(
            cwd,
            monorepo_user_dir,
            &user_skill_roots,
            config_paths,
        );
        let project_root = cwd.map(crate::session::workflow::registry::project_root);
        Self::start_with_dirs(&dirs_to_watch, &grow_home, project_root.as_deref())
    }

    /// Start with explicit discovery roots (benches and isolated tests).
    ///
    /// Production code should prefer [`Self::start`], which collects the same
    /// dir set discovery uses. After a [`DiscoveryChange`], call
    /// [`Self::refresh_new_discovery_dirs`].
    pub fn start_with_dirs(
        dirs_to_watch: &[PathBuf],
        grow_home: &Path,
        project_root: Option<&Path>,
    ) -> Option<(Self, mpsc::UnboundedReceiver<DiscoveryChange>)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let plan = plan_skills_watch_targets(dirs_to_watch, grow_home, project_root);
        let mut classified_grow_roots = plan.grow_roots.clone();
        if let Some(project_root) = project_root {
            classified_grow_roots.push(project_root.join(".grow"));
        }
        let recursive_skill_roots = plan.recursive_roots.clone();

        let mut debouncer =
            new_filtered_debouncer(SKILLS_DEBOUNCE, move |res: DebounceEventResult| {
                let Ok(events) = res else { return };
                let mut change = None;
                for event in &events {
                    let next = classified_grow_roots
                        .iter()
                        .find_map(|root| discovery_change_under_root(&event.path, root))
                        .or_else(|| {
                            recursive_skill_roots
                                .iter()
                                .any(|root| event.path.starts_with(root))
                                .then_some(DiscoveryChange::Skills)
                        });
                    let Some(next) = next else { continue };
                    if next == DiscoveryChange::Skills {
                        change = Some(next);
                        break;
                    }
                    change = Some(next);
                }
                if let Some(change) = change {
                    let _ = tx.send(change);
                }
            })
            .map_err(|e| tracing::warn!(error = %e, "failed to create skills file watcher"))
            .ok()?;

        let mut refreshed_dirs = HashMap::new();
        attach_new_refresh_dirs(
            &mut debouncer,
            &plan.refresh_dirs,
            &mut refreshed_dirs,
            "failed to watch discovery directory",
        );
        let mut watched = refreshed_dirs.len();
        for dir in &plan.recursive_roots {
            match debouncer.watcher().watch(dir, RecursiveMode::Recursive) {
                Ok(()) => {
                    watched += 1;
                }
                Err(e) => log_watch_error(&e, "failed to watch directory for skill changes"),
            }
        }
        if let Some(parent_watch) = &plan.project_parent_watch {
            match debouncer
                .watcher()
                .watch(parent_watch, RecursiveMode::NonRecursive)
            {
                Ok(()) => {
                    watched += 1;
                }
                Err(error) => log_watch_error(
                    &error,
                    "failed to watch workflow discovery parent directory",
                ),
            }
        }

        if watched == 0 {
            tracing::debug!("no config directories found to watch for skills");
            return None;
        }

        tracing::info!(dirs = watched, "skills file watcher started");

        Some((
            Self {
                debouncer,
                refresh_dirs: plan.refresh_dirs,
                refreshed_dirs,
            },
            rx,
        ))
    }

    /// Reconcile watches for created or replaced directories after a discovery event.
    /// Returns true if any new or replacement watch was attached.
    pub fn refresh_new_discovery_dirs(&mut self) -> bool {
        attach_new_refresh_dirs(
            &mut self.debouncer,
            &self.refresh_dirs,
            &mut self.refreshed_dirs,
            "failed to watch newly-created discovery directory",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_classifies_only_canonical_grow_paths() {
        assert_eq!(
            discovery_change_for_path(Path::new("/repo/.grow/skills/review/SKILL.md")),
            Some(DiscoveryChange::Skills)
        );
        assert_eq!(
            discovery_change_for_path(Path::new("/repo/.grow/commands/review.md")),
            Some(DiscoveryChange::Skills)
        );
        assert_eq!(
            discovery_change_for_path(Path::new("/repo/.grow/workflows/release.toml")),
            Some(DiscoveryChange::Workflows)
        );
        for path in [
            "/repo/.claude/skills/review/SKILL.md",
            "/repo/.cursor/commands/review.md",
            "/repo/.agents/workflows/release.toml",
        ] {
            assert_eq!(discovery_change_for_path(Path::new(path)), None, "{path}");
        }
    }

    #[test]
    fn discovery_classifies_sections_relative_to_an_explicit_grow_root() {
        let root = Path::new("/tmp/custom-grow-home");
        assert_eq!(
            discovery_change_under_root(
                Path::new("/tmp/custom-grow-home/skills/review/SKILL.md"),
                root
            ),
            Some(DiscoveryChange::Skills)
        );
        assert_eq!(
            discovery_change_under_root(
                Path::new("/tmp/custom-grow-home/workflows/release.toml"),
                root
            ),
            Some(DiscoveryChange::Workflows)
        );
        assert_eq!(
            discovery_change_under_root(Path::new("/tmp/custom-grow-home/worktrees/x"), root),
            None
        );
    }

    #[test]
    fn grow_config_root_is_exact() {
        let grow_home = Path::new("/home/user/.grow");
        assert!(is_grow_config_root(grow_home, grow_home));
        assert!(is_grow_config_root(Path::new("/repo/.grow"), grow_home));
        assert!(!is_grow_config_root(Path::new("/repo/.claude"), grow_home));
        assert!(!is_grow_config_root(Path::new("/repo/config"), grow_home));
    }

    #[test]
    fn project_refresh_plan_seeds_only_grow() {
        let project = Path::new("/repo");
        let grow_home = Path::new("/home/user/.grow");
        let plan = plan_skills_watch_targets(&[], grow_home, Some(project));

        assert!(plan.grow_roots.is_empty());
        assert!(plan.recursive_roots.is_empty());
        assert_eq!(plan.project_parent_watch.as_deref(), Some(project));
        assert_eq!(plan.refresh_dirs, project_grow_refresh_dirs(project));
        assert!(
            plan.refresh_dirs
                .iter()
                .all(|(path, _)| path.starts_with(project.join(".grow")))
        );
    }

    #[test]
    fn explicit_grow_root_is_scoped_and_custom_root_is_recursive() {
        let grow_home = PathBuf::from("/home/user/.grow");
        let project_grow = PathBuf::from("/repo/.grow");
        let custom = PathBuf::from("/repo/custom-skills");
        let plan = plan_skills_watch_targets(
            &[grow_home.clone(), project_grow.clone(), custom.clone()],
            &grow_home,
            Some(Path::new("/repo")),
        );

        assert_eq!(
            plan.grow_roots,
            vec![grow_home.clone(), project_grow.clone()]
        );
        assert_eq!(plan.recursive_roots, vec![custom]);
        assert_eq!(
            plan.project_parent_watch.as_deref(),
            Some(Path::new("/repo"))
        );
        let expected = [(grow_home.clone(), RecursiveMode::NonRecursive)]
            .into_iter()
            .chain(grow_discovery_refresh_dirs(&grow_home))
            .chain([(project_grow.clone(), RecursiveMode::NonRecursive)])
            .chain(grow_discovery_refresh_dirs(&project_grow))
            .collect::<Vec<_>>();
        assert_eq!(plan.refresh_dirs, expected);
    }

    #[test]
    fn project_refresh_modes_are_bounded() {
        let dirs = project_grow_refresh_dirs(Path::new("/repo"));
        assert_eq!(
            dirs,
            vec![
                (PathBuf::from("/repo/.grow"), RecursiveMode::NonRecursive),
                (
                    PathBuf::from("/repo/.grow/skills"),
                    RecursiveMode::Recursive,
                ),
                (
                    PathBuf::from("/repo/.grow/commands"),
                    RecursiveMode::NonRecursive,
                ),
                (
                    PathBuf::from("/repo/.grow/workflows"),
                    RecursiveMode::NonRecursive,
                ),
            ]
        );
    }
}

#[cfg(test)]
mod late_config_directory_tests {
    use super::*;

    async fn expect_project_change(
        rx: &mut mpsc::UnboundedReceiver<ConfigChangeEvent>,
        path: &Path,
    ) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("watcher channel closed");
                if event
                    == (ConfigChangeEvent::ProjectConfigChanged {
                        path: path.to_owned(),
                    })
                {
                    break;
                }
            }
        })
        .await
        .expect("project config event missing");
    }

    #[tokio::test]
    async fn config_watch_late_directory_and_recreation() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let home = root.join("home");
        let project = root.join("project");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&project).unwrap();
        let (mut watcher, mut rx) =
            ConfigFileWatcher::start(&home, &[], Some(&project), Some(Duration::from_millis(40)))
                .unwrap();
        let grow = project.join(".grow");
        let config = grow.join("config.toml");
        std::fs::create_dir(&grow).unwrap();
        std::fs::write(&config, "first = true").unwrap();
        expect_project_change(&mut rx, &config).await;
        while rx.try_recv().is_ok() {}
        std::fs::write(&config, "second = true").unwrap();
        expect_project_change(&mut rx, &config).await;
        std::fs::remove_dir_all(&grow).unwrap();
        expect_project_change(&mut rx, &config).await;
        std::fs::create_dir(&grow).unwrap();
        std::fs::write(&config, "third = true").unwrap();
        expect_project_change(&mut rx, &config).await;
        while rx.try_recv().is_ok() {}
        std::fs::write(&config, "fourth = true").unwrap();
        expect_project_change(&mut rx, &config).await;
        watcher.unwatch_path(&project);
        while rx.try_recv().is_ok() {}
        std::fs::remove_dir_all(&grow).unwrap();
        std::fs::create_dir(&grow).unwrap();
        std::fs::write(&config, "unwatched = true").unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(400), rx.recv())
                .await
                .is_err(),
            "unwatch must prevent revival"
        );
    }
}

#[cfg(test)]
mod replacement_discovery_tests {
    use super::*;
    async fn next_change(rx: &mut mpsc::UnboundedReceiver<DiscoveryChange>) {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("replacement directory change missing")
            .expect("watcher closed");
    }
    #[test]
    fn refresh_detects_atomic_directory_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let grow = root.join(".grow");
        std::fs::create_dir_all(grow.join("skills")).unwrap();
        let (mut watcher, _rx) =
            SkillsFileWatcher::start_with_dirs(&[grow.clone()], &grow, Some(&root)).unwrap();
        assert!(!watcher.refresh_new_discovery_dirs());
        std::fs::rename(grow.join("skills"), grow.join("old-skills")).unwrap();
        std::fs::create_dir(grow.join("skills")).unwrap();
        assert!(
            watcher.refresh_new_discovery_dirs(),
            "replacement needs a new watch"
        );
        assert!(!watcher.refresh_new_discovery_dirs());
        std::fs::write(grow.join("skills/SKILL.md"), "ordinary edit").unwrap();
        assert!(!watcher.refresh_new_discovery_dirs());
        std::fs::rename(&grow, root.join("old-grow")).unwrap();
        std::fs::create_dir_all(grow.join("skills")).unwrap();
        assert!(watcher.refresh_new_discovery_dirs());
        assert!(!watcher.refresh_new_discovery_dirs());
    }

    #[tokio::test]
    async fn skills_discovery_observes_recreated_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let grow = root.join(".grow");
        std::fs::create_dir_all(grow.join("skills")).unwrap();
        let (mut watcher, mut rx) =
            SkillsFileWatcher::start_with_dirs(&[grow.clone()], &grow, Some(&root)).unwrap();
        std::fs::remove_dir_all(&grow).unwrap();
        next_change(&mut rx).await;
        watcher.refresh_new_discovery_dirs();
        while rx.try_recv().is_ok() {}
        std::fs::create_dir_all(grow.join("skills")).unwrap();
        next_change(&mut rx).await;
        assert!(watcher.refresh_new_discovery_dirs());
        while rx.try_recv().is_ok() {}
        std::fs::create_dir_all(grow.join("skills/nested")).unwrap();
        std::fs::write(grow.join("skills/nested/SKILL.md"), "updated").unwrap();
        next_change(&mut rx).await;
        assert!(!watcher.refresh_new_discovery_dirs());
    }

    #[tokio::test]
    async fn project_discovery_observes_recreated_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let grow = root.join(".grow");
        std::fs::create_dir_all(grow.join("workflows")).unwrap();
        let (mut watcher, mut rx) = ProjectDiscoveryWatcher::start(&root).unwrap();
        std::fs::remove_dir_all(&grow).unwrap();
        next_change(&mut rx).await;
        watcher.refresh_new_dirs();
        while rx.try_recv().is_ok() {}
        std::fs::create_dir_all(grow.join("workflows")).unwrap();
        std::fs::write(grow.join("workflows/check.rhai"), "first").unwrap();
        next_change(&mut rx).await;
        watcher.refresh_new_dirs();
        while rx.try_recv().is_ok() {}
        std::fs::write(grow.join("workflows/check.rhai"), "second").unwrap();
        next_change(&mut rx).await;
    }
}

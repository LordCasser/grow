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
pub struct AccessFilteredWatcher<W = notify::RecommendedWatcher>(W);

impl<W: notify::Watcher> notify::Watcher for AccessFilteredWatcher<W> {
    fn new<F: notify::EventHandler>(
        mut event_handler: F,
        config: notify::Config,
    ) -> notify::Result<Self>
    where
        Self: Sized,
    {
        let inner = W::new(
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
        W::kind()
    }
}

// FSEvents registration can block in FSEventStreamStart even on its first path.
// Discovery uses metadata polling on macOS without reading arbitrary root files.
#[cfg(target_os = "macos")]
type DiscoveryWatchBackend = notify::PollWatcher;
#[cfg(not(target_os = "macos"))]
type DiscoveryWatchBackend = notify::RecommendedWatcher;
type DiscoveryDebouncer = Debouncer<AccessFilteredWatcher<DiscoveryWatchBackend>>;

/// Build discovery debounce over the platform backend, retaining Access filtering.
fn new_filtered_debouncer<F: notify_debouncer_mini::DebounceEventHandler>(
    timeout: Duration,
    event_handler: F,
) -> Result<DiscoveryDebouncer, notify::Error> {
    let config = notify_debouncer_mini::Config::default()
        .with_timeout(timeout)
        .with_notify_config(
            notify::Config::default()
                .with_poll_interval(timeout)
                .with_compare_contents(false),
        );
    new_debouncer_opt::<F, AccessFilteredWatcher<DiscoveryWatchBackend>>(config, event_handler)
}

// FSEvents rebuilds its stream for every path mutation. Config directories
// need stable observation while sessions register/release other paths.
#[cfg(target_os = "macos")]
type ConfigWatchBackend = MacConfigWatcher;
#[cfg(not(target_os = "macos"))]
type ConfigWatchBackend = notify::RecommendedWatcher;
type ConfigDebouncer = Debouncer<AccessFilteredWatcher<ConfigWatchBackend>>;

// Directory polling detects new config paths; content hashing is restricted
// to config.toml, so arbitrary large files in a project cwd are never read.
#[cfg(target_os = "macos")]
struct MacConfigWatcher {
    directories: notify::PollWatcher,
    files: std::sync::Arc<parking_lot::Mutex<notify::PollWatcher>>,
}

#[cfg(target_os = "macos")]
impl notify::Watcher for MacConfigWatcher {
    fn new<F: notify::EventHandler>(handler: F, config: notify::Config) -> notify::Result<Self> {
        let handler = std::sync::Arc::new(parking_lot::Mutex::new(handler));
        let file_handler = handler.clone();
        let files = std::sync::Arc::new(parking_lot::Mutex::new(notify::PollWatcher::new(
            move |event| file_handler.lock().handle_event(event),
            config.with_compare_contents(true),
        )?));
        let files_for_discovery = files.clone();
        let directories = notify::PollWatcher::new(
            move |event: notify::Result<notify::Event>| {
                if let Ok(event) = &event {
                    for path in &event.paths {
                        if path.file_name().is_some_and(|name| name == "config.toml")
                            && path.is_file()
                        {
                            let _ = files_for_discovery
                                .lock()
                                .watch(path, RecursiveMode::NonRecursive);
                        }
                    }
                }
                handler.lock().handle_event(event);
            },
            config.with_compare_contents(false),
        )?;
        Ok(Self { directories, files })
    }

    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        self.directories.watch(path, mode)?;
        let config = path.join("config.toml");
        if config.is_file() {
            self.files
                .lock()
                .watch(&config, RecursiveMode::NonRecursive)?;
        }
        Ok(())
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        // Stop directory discovery first so an in-flight scan cannot reinstall
        // the content watch after it has been released.
        self.directories.unwatch(path)?;
        let _ = self.files.lock().unwatch(&path.join("config.toml"));
        Ok(())
    }

    fn kind() -> notify::WatcherKind {
        notify::WatcherKind::PollWatcher
    }
}

fn new_config_debouncer<F: notify_debouncer_mini::DebounceEventHandler>(
    timeout: Duration,
    event_handler: F,
) -> Result<ConfigDebouncer, notify::Error> {
    let config = notify_debouncer_mini::Config::default()
        .with_timeout(timeout)
        .with_notify_config(notify::Config::default().with_poll_interval(timeout));
    new_debouncer_opt::<F, AccessFilteredWatcher<ConfigWatchBackend>>(config, event_handler)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigChangeEvent {
    GlobalConfigChanged,
    ProjectConfigChanged { path: PathBuf },
}

/// Ordered updates from resident session owners to the runtime watcher.
#[derive(Debug)]
pub enum SessionConfigWatchUpdate {
    Register { session_id: String, cwd: PathBuf },
    Release { session_id: String },
}

/// A resident resource: retained across actor rebuild, released on teardown.
pub struct SessionConfigWatchRegistration {
    session_id: String,
    tx: mpsc::UnboundedSender<SessionConfigWatchUpdate>,
}

impl SessionConfigWatchRegistration {
    pub fn new(
        session_id: String,
        cwd: &Path,
        tx: mpsc::UnboundedSender<SessionConfigWatchUpdate>,
    ) -> Self {
        let registration = Self { session_id, tx };
        registration.update(cwd);
        registration
    }

    pub fn update(&self, cwd: &Path) {
        let _ = self.tx.send(SessionConfigWatchUpdate::Register {
            session_id: self.session_id.clone(),
            cwd: cwd.to_path_buf(),
        });
    }
}

impl Drop for SessionConfigWatchRegistration {
    fn drop(&mut self) {
        let _ = self.tx.send(SessionConfigWatchUpdate::Release {
            session_id: self.session_id.clone(),
        });
    }
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
/// Adds narrow **non-recursive** watches for the cwd→repository-root directory
/// chain used by project config discovery, including each `.grow/` directory.
/// Recursing on `<cwd>`
/// would walk `node_modules/`, `target/`, `.git/`, etc. and blow through
/// `fs.inotify.max_user_watches` on large repos. Use [`Self::watch_path`]
/// to register additional cwds at runtime when new sessions open in
/// previously-unwatched directories.
pub struct ConfigFileWatcher {
    session_cwds: HashMap<String, PathBuf>,
    bootstrap_cwd: Option<PathBuf>,
    state: std::sync::Arc<parking_lot::Mutex<ConfigWatchState>>,
}

struct ConfigWatchState {
    // Installed after callback construction; callback holds only a Weak state.
    debouncer: Option<ConfigDebouncer>,
    watched_cwds: HashSet<PathBuf>,
    // One registration may consume configs from several repository ancestors.
    // Retain shared directory watches until their last cwd owner unregisters.
    project_roots: HashMap<PathBuf, Vec<PathBuf>>,
    // Global/explicit sources own their parent watches independently of cwds.
    permanent_dirs: Vec<PathBuf>,
    config_dirs: HashMap<PathBuf, same_file::Handle>,
}

impl ConfigWatchState {
    fn change_for_path(&mut self, path: &Path, grow_home: &Path) -> Option<ConfigChangeEvent> {
        let name = path.file_name().and_then(|n| n.to_str());
        let parent = path.parent();

        match name {
            Some(".grow") if parent.is_some_and(|p| self.watched_cwds.contains(p)) => {
                if let Some(debouncer) = &mut self.debouncer {
                    attach_new_refresh_dirs(
                        debouncer,
                        &[(path.to_path_buf(), RecursiveMode::NonRecursive)],
                        &mut self.config_dirs,
                        "failed to attach project config directory",
                    );
                }
                Some(ConfigChangeEvent::ProjectConfigChanged {
                    path: path.join("config.toml"),
                })
            }
            Some("config.toml") if parent.is_some_and(|p| paths_equal(p, grow_home)) => {
                Some(ConfigChangeEvent::GlobalConfigChanged)
            }
            Some("config.toml")
                if parent.is_some_and(|dir| {
                    dirs_contain(&self.permanent_dirs, dir)
                        || (dir.file_name().and_then(|name| name.to_str()) == Some(".grow")
                            && dir
                                .parent()
                                .is_some_and(|root| self.watched_cwds.contains(root)))
                }) =>
            {
                Some(ConfigChangeEvent::ProjectConfigChanged {
                    path: path.to_path_buf(),
                })
            }
            _ => None,
        }
    }
}

impl ConfigFileWatcher {
    /// Start watching. Returns `None` if the OS watcher fails to initialize.
    ///
    /// `cwd`, when `Some`, registers its project config directory chain.
    /// Use [`Self::watch_path`] later to register additional
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
            project_roots: HashMap::new(),
            config_dirs: HashMap::new(),
            permanent_dirs: std::iter::once(grow_home_buf.clone())
                .chain(
                    extra_paths
                        .iter()
                        .filter_map(|path| path.parent().map(Path::to_path_buf)),
                )
                .collect(),
        }));
        let weak_state = std::sync::Arc::downgrade(&state);
        let mut debouncer = new_config_debouncer(debounce, move |res: DebounceEventResult| {
            let Ok(events) = res else { return };
            let Some(state) = weak_state.upgrade() else {
                return;
            };
            // Serialize directory maintenance with explicit watch/unwatch.
            // The debouncer callback is separate from the raw notify handler.
            let mut state = state.lock();

            let mut batch_events: Vec<ConfigChangeEvent> = Vec::new();
            for event in events {
                let change = state.change_for_path(&event.path, &grow_home_buf);

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
        }
        let bootstrap_cwd =
            cwd.map(|cwd| dunce::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf()));
        let mut watcher = Self {
            state,
            session_cwds: HashMap::new(),
            bootstrap_cwd,
        };
        if let Some(cwd) = watcher.bootstrap_cwd.clone() {
            watcher.watch_registered_path(cwd);
        }
        Some((watcher, rx))
    }

    /// Direct distinct-cwd API: callers coordinate all owners themselves and
    /// must not mix this with session-owned registrations for the same key.
    /// Production resident owners use `apply_session_update` exclusively.
    /// Register the project config chain from cwd to repository root as
    /// **non-recursive** watch
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
        let cwd = dunce::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        self.watch_registered_path(cwd);
    }

    fn watch_registered_path(&mut self, cwd: PathBuf) {
        let roots = agent::repo::RepoDirChain::resolve(&cwd).dirs;
        // Idempotent at our layer: skip the redundant
        // `notify` watch-add when this cwd is already registered, so
        // re-opening sessions in the same directory doesn't churn the
        // OS watcher. `notify` de-dups internally too, but tracking the
        // set here also enables `unwatch_path`.
        let mut state = self.state.lock();
        if state.project_roots.contains_key(&cwd) {
            return;
        }
        for root in &roots {
            if state.watched_cwds.insert(root.clone()) {
                let ConfigWatchState {
                    debouncer,
                    config_dirs,
                    ..
                } = &mut *state;
                watch_cwd_dirs(
                    debouncer.as_mut().expect("initialized watcher"),
                    root,
                    config_dirs,
                );
            }
        }
        state.project_roots.insert(cwd, roots);
    }

    /// Reconcile a resident owner without revoking shared or bootstrap paths.
    pub fn apply_session_update(&mut self, update: SessionConfigWatchUpdate) {
        let previous = match update {
            SessionConfigWatchUpdate::Register { session_id, cwd } => {
                let cwd = dunce::canonicalize(&cwd).unwrap_or(cwd);
                if self.session_cwds.get(&session_id) == Some(&cwd) {
                    return;
                }
                self.watch_registered_path(cwd.clone());
                self.session_cwds.insert(session_id, cwd)
            }
            SessionConfigWatchUpdate::Release { session_id } => {
                self.session_cwds.remove(&session_id)
            }
        };
        if let Some(previous) = previous
            && self.bootstrap_cwd.as_ref() != Some(&previous)
            && !self.session_cwds.values().any(|cwd| cwd == &previous)
        {
            // Release the acquired key even when its old alias no longer exists.
            self.unwatch_registered_path(&previous);
        }
    }

    /// Keep native watch registration, removal and destruction off async workers.
    pub fn spawn_session_updates(
        self,
        updates: Option<mpsc::UnboundedReceiver<SessionConfigWatchUpdate>>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> (
        tokio::task::JoinHandle<()>,
        tokio::sync::oneshot::Receiver<()>,
    ) {
        spawn_watch_updates(self, updates, cancel, Self::apply_session_update)
    }

    /// Release the project config chain previously registered for `cwd` via
    /// [`Self::start`] / [`Self::watch_path`].
    ///
    /// Best-effort and idempotent: a `cwd` that was never registered
    /// (or already unwatched) is a no-op. Intended for the
    /// session-teardown path so a long-lived leader that opens sessions
    /// across many directories doesn't accumulate inotify watches for
    /// cwds with no live sessions. **Callers must ref-count**: only
    /// unwatch once the *last* session sharing this cwd closes —
    /// `ConfigFileWatcher` tracks distinct cwd owners, not session counts;
    /// shared ancestor watches remain until the last cwd owner releases them;
    /// global and explicit config sources retain their own directory watches.
    pub fn unwatch_path(&mut self, cwd: &Path) {
        let cwd = dunce::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        self.unwatch_registered_path(&cwd);
    }

    fn unwatch_registered_path(&mut self, cwd: &Path) {
        let mut state = self.state.lock();
        let Some(roots) = state.project_roots.remove(cwd) else {
            return;
        };
        for root in roots {
            if !state
                .project_roots
                .values()
                .any(|roots| roots.contains(&root))
            {
                state.watched_cwds.remove(&root);
                for path in [&root, &root.join(".grow")] {
                    if dirs_contain(&state.permanent_dirs, path) {
                        continue;
                    }
                    state.config_dirs.remove(path);
                    if let Err(error) = state
                        .debouncer
                        .as_mut()
                        .expect("initialized watcher")
                        .watcher()
                        .unwatch(path)
                    {
                        tracing::debug!(error = %error, "failed to unwatch project config directory");
                    }
                }
            }
        }
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
fn watch_cwd_dirs(
    debouncer: &mut ConfigDebouncer,
    cwd: &Path,
    config_dirs: &mut HashMap<PathBuf, same_file::Handle>,
) {
    if let Err(e) = debouncer.watcher().watch(cwd, RecursiveMode::NonRecursive) {
        log_watch_error(&e, "failed to watch project cwd (non-recursive)");
    }
    attach_new_refresh_dirs(
        debouncer,
        &[(cwd.join(".grow"), RecursiveMode::NonRecursive)],
        config_dirs,
        "failed to watch project .grow directory (non-recursive)",
    );
}

fn spawn_watch_updates<W, F>(
    mut watcher: W,
    mut updates: Option<mpsc::UnboundedReceiver<SessionConfigWatchUpdate>>,
    cancel: tokio_util::sync::CancellationToken,
    mut apply: F,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::oneshot::Receiver<()>,
)
where
    W: Send + 'static,
    F: FnMut(&mut W, SessionConfigWatchUpdate) + Send + 'static,
{
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let runtime = tokio::runtime::Handle::current();
    let worker_cancel = cancel.child_token();
    let observer_guard = worker_cancel.clone().drop_guard();
    let native_cancel = worker_cancel.clone();
    let worker = tokio::task::spawn_blocking(move || {
        if let Some(updates) = &mut updates {
            while let Ok(update) = updates.try_recv() {
                if native_cancel.is_cancelled() {
                    return;
                }
                apply(&mut watcher, update);
            }
        }
        let _ = ready_tx.send(());
        loop {
            let update = runtime.block_on(async {
                tokio::select! {
                    biased;
                    _ = native_cancel.cancelled() => None,
                    update = async {
                        match &mut updates {
                            Some(updates) => updates.recv().await,
                            None => std::future::pending().await,
                        }
                    } => update,
                }
            });
            match update {
                Some(update) => apply(&mut watcher, update),
                None if native_cancel.is_cancelled() => break,
                // A closed resident-interest queue does not revoke global watches.
                None => updates = None,
            }
        }
        // The native owner is destroyed on this blocking worker, also on unwind.
    });
    let observer = tokio::spawn(async move {
        let _guard = observer_guard;
        tokio::select! {
            biased;
            _ = worker_cancel.cancelled() => {},
            result = worker => {
                if let Err(error) = result {
                    tracing::warn!(%error, "config watch worker failed");
                }
            },
        }
    });
    (observer, ready_rx)
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

fn attach_new_refresh_dirs<W: notify::Watcher>(
    debouncer: &mut Debouncer<W>,
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
    debouncer: DiscoveryDebouncer,
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
    debouncer: DiscoveryDebouncer,
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

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn discovery_poll_uses_metadata_without_comparing_large_file_contents() {
        use notify::Watcher;
        use std::io::Write;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large-file");
        let mut file = std::fs::File::create(&path).unwrap();
        file.set_len(64 * 1024 * 1024).unwrap();
        let modified = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut debouncer = new_filtered_debouncer(
            Duration::from_millis(40),
            move |events: DebounceEventResult| {
                let _ = tx.send(events);
            },
        )
        .unwrap();
        assert_eq!(
            DiscoveryWatchBackend::kind(),
            notify::WatcherKind::PollWatcher
        );
        debouncer
            .watcher()
            .watch(temp.path(), RecursiveMode::Recursive)
            .unwrap();

        // Changing bytes while preserving metadata must not trigger hashing.
        file.write_all(b"changed contents").unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(400), rx.recv())
                .await
                .is_err(),
            "discovery must not compare arbitrary file contents"
        );
        file.set_times(std::fs::FileTimes::new().set_modified(modified + Duration::from_secs(2)))
            .unwrap();
        let events = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("metadata change missing")
            .expect("watcher closed")
            .expect("polling failed");
        assert!(events.iter().any(|event| event.path == path));
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

    #[test]
    fn queued_project_paths_are_rechecked_after_unregistration() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let home = root.join("home");
        let project = root.join("project");
        let grow = project.join(".grow");
        let config = grow.join("config.toml");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir_all(&grow).unwrap();
        let (mut watcher, _rx) =
            ConfigFileWatcher::start(&home, &[], Some(&project), None).unwrap();
        assert_eq!(
            watcher.state.lock().change_for_path(&config, &home),
            Some(ConfigChangeEvent::ProjectConfigChanged {
                path: config.clone()
            })
        );
        watcher.unwatch_path(&project);
        // Deliver the already queued paths through the production callback's
        // path handler after the unregister boundary, without OS timing guesses.
        for path in [&grow, &config] {
            assert_eq!(watcher.state.lock().change_for_path(path, &home), None);
        }
    }

    #[tokio::test]
    async fn explicit_and_global_config_watches_outlive_project_registration() {
        for global_shares_project in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = dunce::canonicalize(temp.path()).unwrap();
            let home = root.join("home");
            let project = if global_shares_project {
                home.clone()
            } else {
                root.join("project")
            };
            let grow = project.join(".grow");
            let config = grow.join("config.toml");
            std::fs::create_dir(&home).unwrap();
            std::fs::create_dir_all(&grow).unwrap();
            let (mut watcher, mut rx) = ConfigFileWatcher::start(
                &home,
                std::slice::from_ref(&config),
                Some(&project),
                Some(Duration::from_millis(40)),
            )
            .unwrap();
            watcher.unwatch_path(&project);
            std::fs::write(&config, "explicit = true").unwrap();
            expect_project_change(&mut rx, &config).await;
            std::fs::write(home.join("config.toml"), "global = true").unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                while rx.recv().await.unwrap() != ConfigChangeEvent::GlobalConfigChanged {}
            })
            .await
            .expect("global config watch must remain active");
        }
    }

    #[tokio::test]
    async fn ancestor_config_watch_survives_other_cwd_unregistration() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let home = root.join("home");
        let repo = root.join("repo");
        let a = repo.join("a");
        let b = repo.join("b");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        git2::Repository::init(&repo).unwrap();
        let (mut watcher, mut events) =
            ConfigFileWatcher::start(&home, &[], Some(&a), Some(Duration::from_millis(40)))
                .unwrap();
        watcher.watch_path(&b);
        watcher.unwatch_path(&a);
        let grow = repo.join(".grow");
        let config = grow.join("config.toml");
        std::fs::create_dir(&grow).unwrap();
        std::fs::write(&config, "first = true").unwrap();
        expect_project_change(&mut events, &config).await;
        while events.try_recv().is_ok() {}
        std::fs::write(&config, "second = true").unwrap();
        expect_project_change(&mut events, &config).await;
        watcher.unwatch_path(&b);
        while events.try_recv().is_ok() {}
        std::fs::write(&config, "unwatched = true").unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(400), events.recv())
                .await
                .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn config_watch_alias_attaches_late_directory_and_unwatches() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let home = root.join("home");
        let project = root.join("project");
        let alias = root.join("alias");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&project).unwrap();
        std::os::unix::fs::symlink(&project, &alias).unwrap();
        let (mut watcher, mut rx) =
            ConfigFileWatcher::start(&home, &[], None, Some(Duration::from_millis(40))).unwrap();
        watcher.watch_path(&alias);
        let grow = project.join(".grow");
        let config = grow.join("config.toml");
        std::fs::create_dir(&grow).unwrap();
        std::fs::write(&config, "first = true").unwrap();
        expect_project_change(&mut rx, &config).await;
        while rx.try_recv().is_ok() {}
        std::fs::write(&config, "second = true").unwrap();
        expect_project_change(&mut rx, &config).await;
        watcher.unwatch_path(&alias);
        while rx.try_recv().is_ok() {}
        std::fs::write(&config, "unwatched = true").unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(400), rx.recv())
                .await
                .is_err()
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn config_poll_observes_same_timestamp_atomic_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let home = root.join("home");
        let project = root.join("project");
        let grow = project.join(".grow");
        let config = grow.join("config.toml");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir_all(&grow).unwrap();
        std::fs::write(&config, "first=1\n").unwrap();
        let modified = std::fs::metadata(&config).unwrap().modified().unwrap();
        let (mut watcher, mut events) =
            ConfigFileWatcher::start(&home, &[], Some(&project), Some(Duration::from_millis(40)))
                .unwrap();
        // Register and release another cwd without resetting existing observation.
        let other = root.join("other");
        std::fs::create_dir(&other).unwrap();
        watcher.watch_path(&other);
        watcher.unwatch_path(&other);
        let replacement = grow.join("replacement");
        std::fs::write(&replacement, "other=2\n").unwrap();
        std::fs::File::open(&replacement)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        std::fs::rename(replacement, &config).unwrap();
        assert_eq!(
            std::fs::metadata(&config).unwrap().modified().unwrap(),
            modified
        );
        expect_project_change(&mut events, &config).await;
    }

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
        let root = dunce::canonicalize(temp.path()).unwrap();
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
        let root = dunce::canonicalize(temp.path()).unwrap();
        let grow = root.join(".grow");
        std::fs::create_dir_all(grow.join("skills")).unwrap();
        let (mut watcher, _rx) =
            SkillsFileWatcher::start_with_dirs(std::slice::from_ref(&grow), &grow, Some(&root))
                .unwrap();
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
    async fn custom_skill_root_observes_create_modify_and_delete() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let custom = root.join("custom-skills");
        std::fs::create_dir(&custom).unwrap();
        let (_watcher, mut rx) = SkillsFileWatcher::start_with_dirs(
            std::slice::from_ref(&custom),
            &root.join(".grow"),
            None,
        )
        .unwrap();
        let skill = custom.join("SKILL.md");
        std::fs::write(&skill, "first").unwrap();
        next_change(&mut rx).await;
        while rx.try_recv().is_ok() {}
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&skill)
            .unwrap();
        let modified = file.metadata().unwrap().modified().unwrap();
        std::fs::write(&skill, "updated").unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified + Duration::from_secs(2)))
            .unwrap();
        next_change(&mut rx).await;
        while rx.try_recv().is_ok() {}
        drop(file);
        std::fs::remove_file(&skill).unwrap();
        next_change(&mut rx).await;
    }

    #[tokio::test]
    async fn skills_discovery_observes_recreated_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let grow = root.join(".grow");
        std::fs::create_dir_all(grow.join("skills")).unwrap();
        let (mut watcher, mut rx) =
            SkillsFileWatcher::start_with_dirs(std::slice::from_ref(&grow), &grow, Some(&root))
                .unwrap();
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
        let root = dunce::canonicalize(temp.path()).unwrap();
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

#[cfg(test)]
mod session_watch_tests {
    use super::*;

    fn register(watcher: &mut ConfigFileWatcher, session: &str, cwd: &Path) {
        watcher.apply_session_update(SessionConfigWatchUpdate::Register {
            session_id: session.to_owned(),
            cwd: cwd.to_path_buf(),
        });
    }

    fn release(watcher: &mut ConfigFileWatcher, session: &str) {
        watcher.apply_session_update(SessionConfigWatchUpdate::Release {
            session_id: session.to_owned(),
        });
    }

    #[test]
    fn resident_config_watch_shares_replaces_and_releases_acquired_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let home = root.join("home");
        let repo = root.join("repo");
        let a = repo.join("a");
        let b = repo.join("b");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir_all(a.join(".grow")).unwrap();
        std::fs::create_dir(&b).unwrap();
        git2::Repository::init(&repo).unwrap();
        let (mut watcher, _events) = ConfigFileWatcher::start(&home, &[], None, None).unwrap();
        register(&mut watcher, "one", &a);
        register(&mut watcher, "two", &a.join("."));
        register(&mut watcher, "one", &a);
        assert_eq!(watcher.session_cwds.len(), 2);
        release(&mut watcher, "one");
        assert!(watcher.state.lock().project_roots.contains_key(&a));
        register(&mut watcher, "two", &b);
        assert!(!watcher.state.lock().project_roots.contains_key(&a));
        assert!(watcher.state.lock().watched_cwds.contains(&repo));
        // The old directory may disappear before teardown; use the acquired key.
        std::fs::remove_dir(&b).unwrap();
        release(&mut watcher, "two");
        release(&mut watcher, "two");
        assert!(watcher.session_cwds.is_empty());
        assert!(watcher.state.lock().project_roots.is_empty());
        assert!(watcher.state.lock().watched_cwds.is_empty());
        assert_eq!(
            watcher
                .state
                .lock()
                .change_for_path(&a.join(".grow/config.toml"), &home),
            None
        );
    }

    #[test]
    fn resident_config_watch_retains_bootstrap_and_independent_sources() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let home = root.join("home");
        let bootstrap = root.join("bootstrap");
        let project = root.join("project");
        let config = project.join(".grow/config.toml");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&bootstrap).unwrap();
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let (mut watcher, _events) =
            ConfigFileWatcher::start(&home, std::slice::from_ref(&config), Some(&bootstrap), None)
                .unwrap();
        register(&mut watcher, "one", &bootstrap);
        register(&mut watcher, "two", &project);
        release(&mut watcher, "one");
        release(&mut watcher, "two");
        assert!(watcher.state.lock().project_roots.contains_key(&bootstrap));
        assert!(!watcher.state.lock().project_roots.contains_key(&project));
        assert_eq!(
            watcher
                .state
                .lock()
                .change_for_path(&home.join("config.toml"), &home),
            Some(ConfigChangeEvent::GlobalConfigChanged)
        );
        assert_eq!(
            watcher.state.lock().change_for_path(&config, &home),
            Some(ConfigChangeEvent::ProjectConfigChanged {
                path: config.clone()
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn resident_config_watch_releases_old_alias_after_retarget() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let home = root.join("home");
        let a = root.join("a");
        let b = root.join("b");
        let alias = root.join("alias");
        for path in [&home, &a, &b] {
            std::fs::create_dir(path).unwrap();
        }
        std::os::unix::fs::symlink(&a, &alias).unwrap();
        let (mut watcher, _events) = ConfigFileWatcher::start(&home, &[], None, None).unwrap();
        register(&mut watcher, "one", &alias);
        register(&mut watcher, "two", &a);
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&b, &alias).unwrap();
        register(&mut watcher, "one", &alias);
        release(&mut watcher, "two");
        assert!(!watcher.state.lock().project_roots.contains_key(&a));
        assert!(watcher.state.lock().project_roots.contains_key(&b));
        release(&mut watcher, "one");
        assert!(watcher.state.lock().project_roots.is_empty());
    }
}

#[cfg(test)]
mod dynamic_config_owner_tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

    struct Owner {
        drop_tx: Option<tokio::sync::oneshot::Sender<std::thread::ThreadId>>,
        seen: std::sync::Arc<parking_lot::Mutex<Vec<String>>>,
    }

    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = self
                .drop_tx
                .take()
                .unwrap()
                .send(std::thread::current().id());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocked_native_update_yields_and_cancel_keeps_drop_on_blocking_owner() {
        for abort in [false, true] {
            let local_thread = std::thread::current().id();
            let (drop_tx, drop_rx) = tokio::sync::oneshot::channel();
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let (tx, rx) = mpsc::unbounded_channel();
            tx.send(SessionConfigWatchUpdate::Release {
                session_id: "one".into(),
            })
            .unwrap();
            let cancel = CancellationToken::new();
            let mut entered_tx = Some(entered_tx);
            let (task, ready) = spawn_watch_updates(
                Owner {
                    drop_tx: Some(drop_tx),
                    seen: Default::default(),
                },
                Some(rx),
                cancel.clone(),
                move |_, _| {
                    let _ = entered_tx.take().unwrap().send(());
                    let _ = release_rx.recv();
                },
            );
            tokio::time::timeout(Duration::from_secs(2), entered_rx)
                .await
                .unwrap()
                .unwrap();
            // A runtime-local future and shutdown both progress during native work.
            tokio::task::yield_now().await;
            if abort {
                task.abort();
            } else {
                cancel.cancel();
            }
            let result = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap();
            if abort {
                assert!(result.unwrap_err().is_cancelled());
            } else {
                result.unwrap();
            }
            drop(ready);
            release_tx.send(()).unwrap();
            let drop_thread = tokio::time::timeout(Duration::from_secs(2), drop_rx)
                .await
                .unwrap()
                .unwrap();
            assert_ne!(drop_thread, local_thread);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn initial_interest_order_precedes_ready_and_queue_close_retains_owner() {
        let (drop_tx, mut drop_rx) = tokio::sync::oneshot::channel();
        let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::unbounded_channel();
        for id in ["register", "release"] {
            tx.send(SessionConfigWatchUpdate::Release {
                session_id: id.into(),
            })
            .unwrap();
        }
        drop(tx);
        let cancel = CancellationToken::new();
        let (task, ready) = spawn_watch_updates(
            Owner {
                drop_tx: Some(drop_tx),
                seen: seen.clone(),
            },
            Some(rx),
            cancel.clone(),
            |owner, update| match update {
                SessionConfigWatchUpdate::Release { session_id } => {
                    owner.seen.lock().push(session_id)
                }
                _ => unreachable!(),
            },
        );
        tokio::time::timeout(Duration::from_secs(2), ready)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&*seen.lock(), &["register", "release"]);
        assert!(matches!(
            drop_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        cancel.cancel();
        task.await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), drop_rx)
            .await
            .unwrap()
            .unwrap();
    }
}

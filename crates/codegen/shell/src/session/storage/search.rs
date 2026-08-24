//! Session search orchestration: querying and background indexing.
//!
//! Mirrors the memory system's architecture:
//! - `execute_search()` runs queries via `search_fts::SessionSearchIndex`
//! - `SearchIndexManager` indexes sessions in the background (debounced)
//! - `notify_session_updated()` is the public hook for session save paths
//!
//! The index is bootstrapped (all sessions indexed) on first search.
//! After that, individual sessions are re-indexed on save/title update
//! via `notify_session_updated()`. Because the SQLite DB is shared with
//! other concurrently running grow processes (which may wipe or downgrade
//! it — older binaries drop-and-restamp the schema on open), every
//! subsequent search re-verifies the on-disk completed-bootstrap marker
//! and re-runs the full bootstrap when it is missing.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use config_types::{BoolFlag, ConfigSource, Resolved, SessionSearchConfig};
use tokio::sync::{Semaphore, mpsc};
use tokio::time::Instant;

use super::search_fts::{self, SessionDoc, SessionSearchIndex, SessionSearchRow};
use super::search_recovery;
use super::{StorageAdapter, TimelineLedgerReader};
use crate::session::info::Info;
use crate::session::persistence::Summary;
use agent_client_protocol as acp;
use chat_state::Timeline;
use sampling_types::ConversationItem;

const SEARCH_INDEX_DEBOUNCE_MS: u64 = 500;
const SEARCH_CONTENT_CHAR_LIMIT: usize = 200_000;
const BOOTSTRAP_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const BOOTSTRAP_POLL_INTERVAL: Duration = Duration::from_millis(50);

const BOOTSTRAP_MAX_CONCURRENT: usize = 4;
const BOOTSTRAP_PER_SESSION_TIMEOUT: Duration = Duration::from_secs(30);
const BOOTSTRAP_MAX_FILE_SIZE: u64 = 30 * 1024 * 1024;

// Bootstrap lease coordination timing. The lease must outlive several
// refreshes, and a waiter must poll at least once within its wait window.
const BOOTSTRAP_LEASE_DURATION: Duration = Duration::from_secs(30);
const BOOTSTRAP_LEASE_REFRESH: Duration = Duration::from_secs(10);
const BOOTSTRAP_LEASE_PEER_WAIT: Duration = Duration::from_secs(30);
const BOOTSTRAP_LEASE_POLL: Duration = Duration::from_secs(1);
const _: () = assert!(BOOTSTRAP_LEASE_REFRESH.as_millis() < BOOTSTRAP_LEASE_DURATION.as_millis());
const _: () = assert!(BOOTSTRAP_LEASE_POLL.as_millis() < BOOTSTRAP_LEASE_PEER_WAIT.as_millis());

// ---------------------------------------------------------------------------
// Process-level gate: whether this process may keep a session-search index.
//
// Mirrors the upstream `search_gate`: an `AtomicU8` latch with three states.
// `UNAPPLIED` means "no one resolved the setting yet" — the first reader
// resolves the disk/env tiers once and applies the result. Once `CLOSED`,
// the gate can never reopen in this process: the completed-bootstrap marker
// outlives the time spent off, so re-opening mid-process would serve a
// half-index that misses everything written while search was off.
// ---------------------------------------------------------------------------

const SEARCH_GATE_UNAPPLIED: u8 = 0;
const SEARCH_GATE_OPEN: u8 = 1;
const SEARCH_GATE_CLOSED: u8 = 2;

/// One latch for the process, so the first workspace to turn search off
/// turns it off for every workspace hosted beside it.
static SEARCH_GATE: AtomicU8 = AtomicU8::new(SEARCH_GATE_UNAPPLIED);

/// The config tier that turned search off; set exactly once, before the
/// latch flips to `CLOSED`, so the off state stays diagnosable even when a
/// later lower-tier resolve disagrees.
static SEARCH_CLOSED_BY: OnceLock<ConfigSource> = OnceLock::new();

/// Apply a resolved setting to the latch. `false` closes the gate for the
/// process (recording the source); `true` only opens an unapplied gate.
fn apply_search_gate(setting: &Resolved<bool>) {
    if !setting.value {
        let _ = SEARCH_CLOSED_BY.set(setting.source);
        if SEARCH_GATE.swap(SEARCH_GATE_CLOSED, Ordering::AcqRel) != SEARCH_GATE_CLOSED {
            tracing::info!(
                source = %setting.source,
                "session search index turned off for this process"
            );
        }
        return;
    }
    let opened = SEARCH_GATE.compare_exchange(
        SEARCH_GATE_UNAPPLIED,
        SEARCH_GATE_OPEN,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
    if opened == Err(SEARCH_GATE_CLOSED) {
        tracing::info!(
            source = %setting.source,
            "session search stays off until the next launch"
        );
    }
}

/// Names the setting that turned search off, for a message like
/// `off (a requirements.toml pin)`.
fn session_search_off_reason(source: ConfigSource) -> &'static str {
    match source {
        ConfigSource::Requirement => "a requirements.toml pin or an MDM policy",
        ConfigSource::Env => "the GROW_SESSION_SEARCH environment variable",
        ConfigSource::Config
        | ConfigSource::UserConfig
        | ConfigSource::ManagedConfig
        | ConfigSource::SystemManagedConfig => "the session_search key in a Grow config file",
        // Neither can resolve to off: the default is on and no CLI flag
        // sets this key.
        ConfigSource::Cli | ConfigSource::Remote | ConfigSource::Default => "a local setting",
    }
}

/// Which tier closed the gate, if any.
fn search_closed_by() -> Option<ConfigSource> {
    SEARCH_CLOSED_BY.get().copied()
}

/// Resolve the session search setting: requirements pin > `GROW_SESSION_SEARCH`
/// env var > `[session_search]` config file > default (`true`).
fn resolve_session_search_setting(
    requirement: Option<bool>,
    config: Option<SessionSearchConfig>,
) -> Resolved<bool> {
    BoolFlag::env("GROW_SESSION_SEARCH")
        .requirement(requirement)
        .config(config.and_then(|c| c.enabled))
        .default(true)
        .resolve()
}

/// Read the disk tiers that stand on their own: the requirements pin and
/// the `[session_search]` config table. A corrupt user config must not
/// disarm a pin, so each tier is read independently.
fn load_session_search_disk_tiers() -> (Option<bool>, Option<SessionSearchConfig>) {
    let pin = crate::config::load_merged_requirements().and_then(|req| {
        req.get("features")
            .and_then(|features| features.get("session_search"))
            .and_then(|value| value.as_bool())
    });
    let config = match crate::config::load_from_disk() {
        Ok(toml) => toml
            .get("session_search")
            .and_then(|table| table.clone().try_into::<SessionSearchConfig>().ok()),
        Err(e) => {
            tracing::warn!(error = %e, "could not read the config for session search");
            None
        }
    };
    (pin, config)
}

/// Cheap after the setting is resolved. The first call in a process that
/// never applied the gate reads the config files from disk and latches the
/// result, so the read happens at most once per process.
fn is_index_enabled() -> bool {
    match SEARCH_GATE.load(Ordering::Acquire) {
        SEARCH_GATE_CLOSED => false,
        SEARCH_GATE_OPEN => true,
        // Nothing has resolved the setting yet: resolve the disk/env tiers
        // here rather than assume on — a pin still outranks the environment.
        _ => {
            let (pin, config) = load_session_search_disk_tiers();
            let setting = resolve_session_search_setting(pin, config);
            tracing::debug!(
                enabled = setting.value,
                "session search resolved from disk before anything applied the setting"
            );
            // Latch it, so this work happens once, and report the latch
            // rather than the value: another thread may have closed the
            // gate while the config was loading.
            apply_search_gate(&setting);
            SEARCH_GATE.load(Ordering::Acquire) != SEARCH_GATE_CLOSED
        }
    }
}

fn should_skip_session(snapshot_len: u64, max_size: u64) -> bool {
    snapshot_len > max_size
}

/// Internal search request (deserialized from the ACP extension params).
#[derive(Debug, Clone)]
pub struct SessionSearchRequest {
    pub query: String,
    pub cwd: Option<String>,
    pub limit: usize,
    pub offset: usize,
    pub include_content: bool,
}

/// Raw search response returned to the ACP extension handler.
#[derive(Debug, Clone)]
pub struct SessionSearchResponse {
    pub results: Vec<SessionSearchRow>,
    pub next_offset: Option<usize>,
    pub total_estimate: Option<usize>,
    /// True when the FTS5 index is still being bootstrapped. Callers
    /// should re-query after a delay to get results from newly indexed
    /// sessions. Also true when another process holds the bootstrap lease
    /// without a completion marker (peer mid-rebuild or a dead claimant
    /// within its lease).
    pub bootstrapping: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionSearchKey {
    session_id: String,
    cwd: String,
}

enum SearchIndexJob {
    Upsert(SessionSearchKey),
    BootstrapAll,
    /// Dispatched for every `BootstrapOnce` after the first: re-verify the
    /// on-disk completed-bootstrap marker, then either clear the eager
    /// `bootstrapping` flag (index intact) or re-run the full bootstrap
    /// (see [`has_completed_bootstrap_marker`]).
    RecheckBootstrap,
}

enum SearchManagerCmd {
    Enqueue { root: PathBuf, job: SearchIndexJob },
    BootstrapOnce { root: PathBuf },
}

struct SearchManagerState {
    workers: HashMap<PathBuf, mpsc::UnboundedSender<SearchIndexJob>>,
    bootstrapped: HashSet<PathBuf>,
}

/// Singleton that manages background session indexing.
///
/// Requires an active tokio runtime on first access (spawns tasks).
///
/// When multiple grow processes run concurrently, they each have their own
/// `SearchIndexManager` writing to the same SQLite database. WAL mode
/// reduces corruption risk, [`search_fts`] self-heals an unusable file, and
/// the bootstrap lease (a claim row in the index's own `meta` table, see
/// [`bootstrap_with_lease`]) keeps the full reindex single-flight across
/// processes.
pub struct SearchIndexManager {
    tx: mpsc::UnboundedSender<SearchManagerCmd>,
    progress: Arc<BootstrapProgress>,
}

/// Global singleton — lazily started on first use.
pub static SEARCH_INDEX_MANAGER: LazyLock<SearchIndexManager> =
    LazyLock::new(SearchIndexManager::start);

#[derive(Default)]
pub struct BootstrapProgress {
    pub bootstrapping: AtomicBool,
    pub indexed: AtomicU64,
    pub total: AtomicU64,
    /// Sessions skipped due to size limit or timeout.
    pub skipped: AtomicU64,
    /// Sessions skipped because content hash was unchanged.
    pub unchanged: AtomicU64,
    /// Total bytes of canonical Timeline ledgers read during this bootstrap.
    pub bytes_read: AtomicU64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexStatus {
    pub bootstrapping: bool,
    pub indexed: u64,
    pub total: u64,
    /// Sessions skipped due to size limit or timeout.
    pub skipped: u64,
    /// Sessions skipped because content hash was unchanged.
    pub unchanged: u64,
}

impl SearchIndexManager {
    fn start() -> Self {
        let progress = Arc::new(BootstrapProgress::default());
        let (tx, mut rx) = mpsc::unbounded_channel::<SearchManagerCmd>();

        tokio::spawn(async move {
            let mut state = SearchManagerState {
                workers: HashMap::new(),
                bootstrapped: HashSet::new(),
            };
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    SearchManagerCmd::Enqueue { root, job } => {
                        Self::dispatch(&mut state, root, job);
                    }
                    SearchManagerCmd::BootstrapOnce { root } => {
                        if state.bootstrapped.insert(root.clone()) {
                            Self::dispatch(&mut state, root, SearchIndexJob::BootstrapAll);
                        } else {
                            // Already bootstrapped this process — but the DB
                            // is shared, so don't trust the in-memory flag:
                            // re-verify the on-disk marker (the job also
                            // undoes the eager flag set).
                            // Dispatch through the worker channel so it sequences
                            // after any in-flight BootstrapAll.
                            Self::dispatch(&mut state, root, SearchIndexJob::RecheckBootstrap);
                        }
                    }
                }
            }
        });

        Self { tx, progress }
    }

    /// Queue a bootstrap of all sessions. Idempotent per root_dir, except
    /// that repeat calls re-verify the on-disk completed-bootstrap marker
    /// and re-bootstrap when it is missing (see
    /// [`SearchIndexJob::RecheckBootstrap`]).
    ///
    /// Sets `bootstrapping` eagerly so callers polling the flag see `true`
    /// before the background task even starts processing.
    ///
    /// Does nothing while the index is switched off — no bootstrap job is
    /// dispatched and no eager flag is set.
    pub fn bootstrap_once(&self, root: PathBuf) {
        if !is_index_enabled() {
            return;
        }
        self.progress.bootstrapping.store(true, Ordering::Release);
        let _ = self.tx.send(SearchManagerCmd::BootstrapOnce { root });
    }

    /// Get current bootstrap progress status.
    pub fn status(&self) -> SearchIndexStatus {
        SearchIndexStatus {
            bootstrapping: self.progress.bootstrapping.load(Ordering::Relaxed),
            indexed: self.progress.indexed.load(Ordering::Relaxed),
            total: self.progress.total.load(Ordering::Relaxed),
            skipped: self.progress.skipped.load(Ordering::Relaxed),
            unchanged: self.progress.unchanged.load(Ordering::Relaxed),
        }
    }

    /// Queue an index update for a single session.
    pub fn enqueue(&self, root: PathBuf, session_id: String, cwd: String) {
        let key = SessionSearchKey { session_id, cwd };
        let _ = self.tx.send(SearchManagerCmd::Enqueue {
            root,
            job: SearchIndexJob::Upsert(key),
        });
    }

    fn dispatch(state: &mut SearchManagerState, root: PathBuf, job: SearchIndexJob) {
        let sender = state.workers.entry(root.clone()).or_insert_with(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            let root_owned = root.clone();
            tokio::spawn(async move {
                let storage: Box<dyn StorageAdapter> = Box::new(
                    super::jsonl::JsonlStorageAdapter::with_root(root_owned.clone()),
                );
                run_worker(&root_owned, storage.as_ref(), rx).await;
            });
            tx
        });
        if sender.send(job).is_err() {
            tracing::warn!("search worker channel closed");
        }
    }
}

/// Trigger indexing for a session that was just saved or updated.
///
/// This is the public hook to call from session persistence paths
/// (e.g., after a canonical title projection, after each prompt turn).
pub fn notify_session_updated(session_id: &str, cwd: &str) {
    let root = crate::util::grow_home::grow_home();
    SEARCH_INDEX_MANAGER.enqueue(root, session_id.to_string(), cwd.to_string());
}

/// The file the index would open, without creating anything. The
/// journal-mode classifier inspects the parent directory, so a caller that
/// means to write must go through [`search_db_path`] first.
fn search_db_path_in(root_dir: &Path) -> PathBuf {
    let path = root_dir.join("sessions").join("session_search.sqlite");
    // Pre-resolve the per-host sibling used on network mounts. Resolution is
    // idempotent, so the index opening the same path again is a no-op.
    sqlite_journal::JournalMode::for_db_path(&path).effective_db_path(&path)
}

fn search_db_path(root_dir: &Path) -> PathBuf {
    let sessions = root_dir.join("sessions");
    // Best-effort: the journal-mode classifier statfs's the parent dir.
    let _ = std::fs::create_dir_all(&sessions);
    search_db_path_in(root_dir)
}

/// Whether an index was built earlier. Creates nothing, so a switched-off
/// process can ask without leaving a fresh empty index behind.
fn search_index_exists(root_dir: &Path) -> bool {
    search_db_path_in(root_dir).exists()
}

const META_KEY_LAST_BOOTSTRAP: &str = "last_bootstrap_at";

fn try_read_last_bootstrap_at(db_path: &Path) -> Result<Option<i64>, String> {
    if !db_path.exists() {
        return Ok(None);
    }
    let index = SessionSearchIndex::open_or_create(db_path).map_err(|e| e.to_string())?;
    let value = index
        .get_meta(META_KEY_LAST_BOOTSTRAP)
        .map_err(|e| e.to_string())?;
    Ok(value.and_then(|value| value.parse::<i64>().ok()))
}

/// Test helper: stamp the completed-bootstrap marker directly. Production
/// writes go through [`write_last_bootstrap_at_if_claim_owner`].
#[cfg(test)]
fn write_last_bootstrap_at(db_path: &Path) -> io::Result<()> {
    let index = SessionSearchIndex::open_or_create(db_path).map_err(sqlite_to_io_error)?;
    index
        .set_meta(
            META_KEY_LAST_BOOTSTRAP,
            &chrono::Utc::now().timestamp().to_string(),
        )
        .map_err(sqlite_to_io_error)
}

fn clear_last_bootstrap_at(db_path: &Path) -> io::Result<()> {
    let index =
        SessionSearchIndex::open_existing(db_path).map_err(|e| io::Error::other(e.to_string()))?;
    index
        .delete_meta(META_KEY_LAST_BOOTSTRAP)
        .map_err(|e| io::Error::other(e.to_string()))
}

fn sqlite_to_io_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(format!("sqlite error: {error}"))
}

/// Rate-limits a repetitive log site: the first `cap` events go to `warn`, the
/// rest to `debug`. Resets its budget whenever the search cache is healed, so a
/// fresh cache starts logging loudly again instead of staying silent forever.
struct HealAwareLogCounter {
    count: AtomicU64,
    epoch_seen: AtomicU64,
    cap: u64,
}

impl HealAwareLogCounter {
    const fn new(cap: u64) -> Self {
        Self {
            count: AtomicU64::new(0),
            epoch_seen: AtomicU64::new(0),
            cap,
        }
    }

    fn should_warn(&self, kind: &str) -> bool {
        let epoch = search_recovery::current_epoch();
        if self.epoch_seen.swap(epoch, Ordering::Relaxed) != epoch {
            self.count.store(0, Ordering::Relaxed);
        }
        let n = self.count.fetch_add(1, Ordering::Relaxed);
        if n < self.cap {
            return true;
        }
        if n == self.cap {
            tracing::warn!(cap = self.cap, "further {kind} will be logged at debug");
        }
        false
    }
}

static INDEX_FAIL_LOG: HealAwareLogCounter = HealAwareLogCounter::new(8);
static BOOTSTRAP_TIMEOUT_LOG: HealAwareLogCounter = HealAwareLogCounter::new(8);

fn log_session_index_failure(session_id: &str, error: &io::Error, message: &str) {
    if INDEX_FAIL_LOG.should_warn("index failures") {
        tracing::warn!(error = %error, session_id = %session_id, "{message}");
    } else {
        tracing::debug!(error = %error, session_id = %session_id, "{message}");
    }
}

fn log_bootstrap_timeout(session_id: &str, timeout_secs: u64) {
    let msg = "session indexing timed out during bootstrap";
    if BOOTSTRAP_TIMEOUT_LOG.should_warn("bootstrap timeouts") {
        tracing::warn!(session_id = %session_id, timeout_secs, "{msg}");
    } else {
        tracing::debug!(session_id = %session_id, timeout_secs, "{msg}");
    }
}

/// Open the index (self-heals unusable files) and run `op`, mapping errors
/// to `io::Error` the way the rest of this module expects.
fn with_search_index<R>(
    db_path: &Path,
    op: impl Fn(&SessionSearchIndex) -> Result<R, rusqlite::Error>,
) -> io::Result<R> {
    search_fts::with_index(db_path, op).map_err(sqlite_to_io_error)
}

/// Execute a session search query.
///
/// On first call, triggers a background bootstrap that indexes all
/// existing sessions. Waits up to [`BOOTSTRAP_WAIT_TIMEOUT`] for the
/// bootstrap to complete so the query runs against a populated index.
/// Subsequent calls skip the wait (bootstrap is already done).
///
/// When the process gate is off this returns a diagnostic error instead of
/// serving an empty result set as if the index simply had no matches.
pub async fn execute_search(
    root_dir: &Path,
    req: &SessionSearchRequest,
) -> io::Result<SessionSearchResponse> {
    if !is_index_enabled() {
        let source = search_closed_by().unwrap_or(ConfigSource::Default);
        return Err(io::Error::other(format!(
            "session search is off ({})",
            session_search_off_reason(source)
        )));
    }

    let query = req.query.trim();
    if query.is_empty() {
        return Ok(SessionSearchResponse {
            results: Vec::new(),
            next_offset: None,
            total_estimate: Some(0),
            bootstrapping: false,
        });
    }

    SEARCH_INDEX_MANAGER.bootstrap_once(root_dir.to_path_buf());

    let epoch = search_recovery::CacheEpoch::now();
    let deadline = tokio::time::Instant::now() + BOOTSTRAP_WAIT_TIMEOUT;
    while SEARCH_INDEX_MANAGER
        .progress
        .bootstrapping
        .load(Ordering::Acquire)
    {
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(BOOTSTRAP_POLL_INTERVAL).await;
    }
    let db_path = search_db_path(root_dir);
    let cwd = req.cwd.clone();
    let limit = req.limit;
    let offset = req.offset;
    let include_content = req.include_content;
    let query_owned = query.to_string();

    let (qr, claim_in_flight) = tokio::task::spawn_blocking(move || {
        with_search_index(&db_path, |index| {
            let result =
                index.query(&query_owned, cwd.as_deref(), limit, offset, include_content)?;
            // A peer mid-rebuild (or a dead claimant within its lease)
            // reads as "bootstrapping" even when this process's flag is
            // clear, so callers re-query once the rebuild lands.
            let claim_in_flight = index
                .get_meta(search_fts::META_KEY_BOOTSTRAP_CLAIM)?
                .is_some()
                && index.get_meta(META_KEY_LAST_BOOTSTRAP)?.is_none();
            Ok((result, claim_in_flight))
        })
    })
    .await
    .map_err(io::Error::other)??;

    let healed = epoch.changed();
    if healed {
        SEARCH_INDEX_MANAGER.bootstrap_once(root_dir.to_path_buf());
    }

    Ok(SessionSearchResponse {
        results: qr.results,
        next_offset: qr.next_offset,
        total_estimate: qr.total_estimate,
        bootstrapping: healed
            || SEARCH_INDEX_MANAGER
                .progress
                .bootstrapping
                .load(Ordering::Relaxed)
            || claim_in_flight,
    })
}

async fn run_worker(
    root_dir: &Path,
    storage: &dyn StorageAdapter,
    mut rx: mpsc::UnboundedReceiver<SearchIndexJob>,
) {
    let debounce = std::time::Duration::from_millis(SEARCH_INDEX_DEBOUNCE_MS);
    let mut pending: HashMap<SessionSearchKey, Instant> = HashMap::new();

    loop {
        if pending.is_empty() {
            let Some(job) = rx.recv().await else { break };
            handle_job(root_dir, storage, &mut pending, job, debounce).await;
            continue;
        }

        let next_deadline = pending
            .values()
            .copied()
            .min()
            .unwrap_or_else(|| Instant::now() + debounce);

        tokio::select! {
            maybe_job = rx.recv() => {
                let Some(job) = maybe_job else { break };
                handle_job(root_dir, storage, &mut pending, job, debounce).await;
            }
            _ = tokio::time::sleep_until(next_deadline) => {
                flush_ready(root_dir, storage, &mut pending).await;
            }
        }
    }
}

fn clear_bootstrapping_flag() {
    SEARCH_INDEX_MANAGER
        .progress
        .bootstrapping
        .store(false, Ordering::Release);
}

async fn handle_job(
    root_dir: &Path,
    storage: &dyn StorageAdapter,
    pending: &mut HashMap<SessionSearchKey, Instant>,
    job: SearchIndexJob,
    debounce: std::time::Duration,
) {
    match job {
        SearchIndexJob::Upsert(key) => {
            pending.insert(key, Instant::now() + debounce);
        }
        SearchIndexJob::BootstrapAll => {
            // The lease gate clears the eager `bootstrapping` flag on every
            // path that declines a reindex (adopt/give-up); `reindex_all`
            // clears it when a reindex actually runs.
            match bootstrap_with_lease(root_dir, storage, BootstrapRole::Launch).await {
                Ok(BootstrapOutcome::Done) => {}
                Ok(BootstrapOutcome::RunAgain) => {
                    SEARCH_INDEX_MANAGER.bootstrap_once(root_dir.to_path_buf());
                }
                Err(e) => {
                    tracing::warn!(error = %e, "session search bootstrap failed");
                    clear_bootstrapping_flag();
                }
            }
        }
        SearchIndexJob::RecheckBootstrap => match has_completed_bootstrap_marker(root_dir).await {
            Some(true) => clear_bootstrapping_flag(),
            Some(false) => {
                // Marker genuinely absent (index wiped/downgraded/bootstrap
                // never completed — see `has_completed_bootstrap_marker`):
                // without a re-run this process would keep searching an
                // empty index for its whole lifetime.
                tracing::info!(
                    "session search index missing completed-bootstrap marker; re-running bootstrap"
                );
                match try_bootstrap_with_lease(root_dir, storage).await {
                    Ok(BootstrapOutcome::Done) => {}
                    Ok(BootstrapOutcome::RunAgain) => {
                        SEARCH_INDEX_MANAGER.bootstrap_once(root_dir.to_path_buf());
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "session search re-bootstrap failed");
                        clear_bootstrapping_flag();
                    }
                }
            }
            None => {
                // Transient read failure (busy/locked DB, I/O): rebuilding on
                // every such search would be a reindex storm. Skip; the next
                // search retries the probe.
                tracing::debug!(
                    "session search bootstrap marker unreadable; skipping re-bootstrap"
                );
                clear_bootstrapping_flag();
            }
        },
    }
}

/// What the caller owes after the bootstrap gate returns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BootstrapOutcome {
    /// Completed, adopted, or gave up: the caller owes nothing more.
    Done,
    /// The cache healed mid-run, so the index must be bootstrapped again.
    RunAgain,
}

/// How the caller entered the bootstrap gate.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BootstrapRole {
    Launch,
    Recheck,
}

/// Run [`reindex_all`] at most once at a time across concurrent grow
/// processes: a claim in the index's own `meta` table lets one process run
/// the full bootstrap while waiters adopt its completed-bootstrap marker.
/// A launch's first claim always reindexes, even when a completed marker
/// exists (the launch owes pruning and skipped-retry work); waiters give up
/// after [`BOOTSTRAP_LEASE_PEER_WAIT`].
async fn bootstrap_with_lease(
    root_dir: &Path,
    storage: &dyn StorageAdapter,
    role: BootstrapRole,
) -> io::Result<BootstrapOutcome> {
    bootstrap_with_lease_inner(root_dir, storage, role).await
}

/// Single claim attempt: rebuilds when the lease is free and no completed
/// marker exists, adopts the marker otherwise, and returns at once when a
/// peer holds the lease. Rechecks use this so a rebuild that outlives the
/// peer wait cannot re-block the worker on every later search.
async fn try_bootstrap_with_lease(
    root_dir: &Path,
    storage: &dyn StorageAdapter,
) -> io::Result<BootstrapOutcome> {
    bootstrap_with_lease_inner(root_dir, storage, BootstrapRole::Recheck).await
}

async fn bootstrap_with_lease_inner(
    root_dir: &Path,
    storage: &dyn StorageAdapter,
    role: BootstrapRole,
) -> io::Result<BootstrapOutcome> {
    let db_path = search_db_path(root_dir);
    let token = ClaimToken::new();
    let started = Instant::now();
    let peer_wait = match role {
        BootstrapRole::Launch => BOOTSTRAP_LEASE_PEER_WAIT,
        BootstrapRole::Recheck => Duration::ZERO,
    };
    let deadline = started + peer_wait;
    let mut peer_seen = false;
    loop {
        // Skipped on the first iteration so a launch always reindexes.
        if peer_seen && has_completed_bootstrap_marker(root_dir).await == Some(true) {
            tracing::info!(
                waited_ms = started.elapsed().as_millis() as u64,
                "adopted a peer's completed session search bootstrap"
            );
            clear_bootstrapping_flag();
            return Ok(BootstrapOutcome::Done);
        }

        if claim_bootstrap_lease(&db_path, &token, BOOTSTRAP_LEASE_DURATION).await? {
            // Only a launch's first claim ignores an existing marker (the
            // launch owes pruning and skipped retries); everyone else
            // adopts any completed marker.
            let first_launch_claim = role == BootstrapRole::Launch && !peer_seen;
            if !first_launch_claim && has_completed_bootstrap_marker(root_dir).await == Some(true) {
                release_bootstrap_claim(&db_path, &token).await;
                tracing::info!(
                    waited_ms = started.elapsed().as_millis() as u64,
                    "adopted a peer's completed session search bootstrap"
                );
                clear_bootstrapping_flag();
                return Ok(BootstrapOutcome::Done);
            }
            tracing::info!(
                token = %token,
                contended = peer_seen,
                waited_ms = started.elapsed().as_millis() as u64,
                "claimed session search bootstrap lease"
            );
            let refresher =
                spawn_claim_refresher(db_path.clone(), token.clone(), BOOTSTRAP_LEASE_REFRESH);
            let result = reindex_all(root_dir, storage, &token, refresher.claim_lost()).await;
            drop(refresher);
            release_bootstrap_claim(&db_path, &token).await;
            return result;
        }
        peer_seen = true;

        if Instant::now() >= deadline {
            // A live peer is rebuilding the shared index. Skip this pass;
            // the next search re-probes the marker and re-bootstraps if
            // the rebuild did not complete.
            tracing::info!(
                "peer process is bootstrapping the shared session search index; not waiting"
            );
            clear_bootstrapping_flag();
            return Ok(BootstrapOutcome::Done);
        }
        tokio::time::sleep(BOOTSTRAP_LEASE_POLL).await;
    }
}

/// Owner token that fences every claim-scoped write to the shared index.
#[derive(Clone)]
struct ClaimToken(String);

impl ClaimToken {
    fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ClaimToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Returns `true` when this process claimed the bootstrap lease.
async fn claim_bootstrap_lease(
    db_path: &Path,
    token: &ClaimToken,
    lease: Duration,
) -> io::Result<bool> {
    let db_path = db_path.to_path_buf();
    let token = token.as_str().to_string();
    tokio::task::spawn_blocking(move || {
        with_search_index(&db_path, |index| {
            index.try_claim_bootstrap(chrono::Utc::now().timestamp(), lease, &token)
        })
    })
    .await
    .map_err(io::Error::other)?
}

/// Aborts the refresher on drop so no detached task outlives the gate.
struct RefresherGuard {
    handle: tokio::task::JoinHandle<()>,
    claim_lost: Arc<AtomicBool>,
}

impl RefresherGuard {
    /// Latched when the refresher sees the claim held by someone else.
    fn claim_lost(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.claim_lost)
    }
}

impl Drop for RefresherGuard {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

fn spawn_claim_refresher(db_path: PathBuf, token: ClaimToken, every: Duration) -> RefresherGuard {
    let claim_lost = Arc::new(AtomicBool::new(false));
    let lost = Arc::clone(&claim_lost);
    let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The first tick fires immediately; the claim was stamped just now.
        interval.tick().await;
        loop {
            interval.tick().await;
            let refreshed = tokio::task::spawn_blocking({
                let db_path = db_path.clone();
                let token = token.as_str().to_string();
                move || {
                    with_search_index(&db_path, |index| {
                        index.refresh_bootstrap_claim(chrono::Utc::now().timestamp(), &token)
                    })
                }
            })
            .await
            .map_err(io::Error::other)
            .and_then(|r| r);
            match refreshed {
                Ok(true) => {}
                Ok(false) => {
                    // Claim expired and was taken over, or already released:
                    // fence all remaining claim-scoped writes.
                    lost.store(true, Ordering::Release);
                    tracing::warn!("bootstrap claim lost mid-reindex; a peer took over");
                    return;
                }
                Err(e) => {
                    // Transient (busy/locked DB): the lease expiry is the
                    // fallback if refreshes keep failing.
                    tracing::debug!(error = %e, "failed to refresh bootstrap claim lease");
                }
            }
        }
    });
    RefresherGuard { handle, claim_lost }
}

/// Best-effort; on any failure the lease expiry is the fallback.
async fn release_bootstrap_claim(db_path: &Path, token: &ClaimToken) {
    let db_path = db_path.to_path_buf();
    let token = token.as_str().to_string();
    let released = tokio::task::spawn_blocking(move || {
        with_search_index(&db_path, |index| index.release_bootstrap_claim(&token))
    })
    .await
    .map_err(io::Error::other)
    .and_then(|r| r);
    match released {
        Ok(true) => {}
        Ok(false) => tracing::debug!("bootstrap claim was already released or taken over"),
        Err(e) => {
            tracing::debug!(error = %e, "failed to release bootstrap claim; lease will expire");
        }
    }
}

/// Fenced marker write: returns `false` (no write) when the claim under
/// `token` was lost, so a stale claimant never asserts completion.
fn write_last_bootstrap_at_if_claim_owner(db_path: &Path, token: &str) -> io::Result<bool> {
    let now = chrono::Utc::now().timestamp();
    with_search_index(db_path, |index| {
        index.set_meta_if_claim_owner(META_KEY_LAST_BOOTSTRAP, &now.to_string(), token)
    })
}

/// Whether any process currently holds the bootstrap claim.
fn has_bootstrap_claim(db_path: &Path) -> io::Result<bool> {
    with_search_index(db_path, |index| {
        index
            .get_meta(search_fts::META_KEY_BOOTSTRAP_CLAIM)
            .map(|claim| claim.is_some())
    })
}

/// Tri-state probe for the completed-bootstrap marker (`last_bootstrap_at`
/// in the `meta` table, written at the end of [`reindex_all`]):
/// `Some(true)` marker present, `Some(false)` genuinely absent (bootstrap
/// needed), `None` transient read failure (busy/locked DB — must not be
/// mistaken for absence, or every search under contention would trigger a
/// full rebuild).
///
/// Opening the DB here is itself the healing step for a downgraded index:
/// `open_or_create` performs the upgrade drop, which deletes the marker in
/// the same transaction (see [`SessionSearchIndex::open_or_create`]), so
/// this returns `Some(false)` and the caller re-runs the full bootstrap.
async fn has_completed_bootstrap_marker(root_dir: &Path) -> Option<bool> {
    let db_path = search_db_path(root_dir);
    tokio::task::spawn_blocking(move || {
        try_read_last_bootstrap_at(&db_path)
            .map(|marker| marker.is_some())
            .ok()
    })
    .await
    .ok()
    .flatten()
}

async fn flush_ready(
    root_dir: &Path,
    storage: &dyn StorageAdapter,
    pending: &mut HashMap<SessionSearchKey, Instant>,
) {
    let now = Instant::now();
    let ready: Vec<SessionSearchKey> = pending
        .iter()
        .filter_map(|(key, deadline)| (*deadline <= now).then_some(key.clone()))
        .collect();

    for key in ready {
        pending.remove(&key);
        if let Err(e) = upsert_by_key(root_dir, storage, &key).await {
            log_session_index_failure(
                &key.session_id,
                &e,
                "failed upserting session in search index",
            );
        }
    }
}

/// Outcome of a single session upsert.
#[derive(Debug)]
enum UpsertOutcome {
    /// Content was indexed (new or changed).
    Indexed { bytes_read: u64 },
    /// Content hash matched existing index entry — no update needed.
    Unchanged { bytes_read: u64 },
    /// No Timeline file available (storage backend doesn't expose paths).
    NoContent,
}

async fn upsert_by_key(
    root_dir: &Path,
    storage: &dyn StorageAdapter,
    key: &SessionSearchKey,
) -> io::Result<()> {
    let info = Info {
        id: acp::SessionId::new(key.session_id.clone()),
        cwd: key.cwd.clone(),
    };

    match storage.load_summary(&info).await {
        Ok(summary) => {
            // The latch cannot reopen in this process, so a write declined
            // here is dropped, not held until search comes back on.
            if !is_index_enabled() {
                return Ok(());
            }
            upsert_session(root_dir, &summary, storage, &info)
                .await
                .map(|_| ())
        }
        // A missing session is a delete, not a failure. Deletes must land
        // whether or not this process still indexes (a session deleted while
        // search is off must not leave a stale row behind), so this branch
        // bypasses the gate — `delete_session` never creates the index.
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            delete_session(root_dir, &key.session_id).await
        }
        Err(e) => Err(e),
    }
}

async fn upsert_session(
    root_dir: &Path,
    summary: &Summary,
    storage: &dyn StorageAdapter,
    info: &Info,
) -> io::Result<UpsertOutcome> {
    // Search is a pure projection of the canonical Timeline ledger. The UI
    // replay stream is deliberately excluded from content reconstruction.
    let reader = storage.open_timeline_reader(info)?;
    let (content, bytes_read) =
        tokio::task::spawn_blocking(move || collect_timeline_indexable_content(reader))
            .await
            .map_err(io::Error::other)??;
    let doc = build_session_doc(summary, content);
    let db_path = search_db_path(root_dir);

    tokio::task::spawn_blocking(move || {
        with_search_index(&db_path, |index| {
            if let Ok(Some(existing_hash)) = index.get_content_hash(&doc.session_id)
                && existing_hash == doc.content_hash
            {
                return Ok(UpsertOutcome::Unchanged { bytes_read });
            }

            index.upsert_doc(&doc)?;
            Ok(UpsertOutcome::Indexed { bytes_read })
        })
    })
    .await
    .map_err(io::Error::other)?
}

async fn delete_session(root_dir: &Path, session_id: &str) -> io::Result<()> {
    // A delete must land whether or not this process still indexes, but it
    // must never create an index while search is off. Skip when nothing was
    // built; the row waits for the next bootstrap, which only runs if
    // search is on again.
    if !search_index_exists(root_dir) {
        return Ok(());
    }
    let db_path = search_db_path(root_dir);
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || {
        with_search_index(&db_path, |index| index.delete_doc(&session_id))
    })
    .await
    .map_err(io::Error::other)?
}

async fn reindex_all(
    root_dir: &Path,
    storage: &dyn StorageAdapter,
    claim_token: &ClaimToken,
    claim_lost: Arc<AtomicBool>,
) -> io::Result<BootstrapOutcome> {
    let epoch = search_recovery::CacheEpoch::now();
    let progress = &SEARCH_INDEX_MANAGER.progress;

    // Reset progress counters (bootstrapping flag already set by bootstrap_once)
    progress.indexed.store(0, Ordering::Relaxed);
    progress.skipped.store(0, Ordering::Relaxed);
    progress.unchanged.store(0, Ordering::Relaxed);
    progress.bytes_read.store(0, Ordering::Relaxed);

    let start = Instant::now();
    let summaries = storage.list_sessions(None).await?;
    progress
        .total
        .store(summaries.len() as u64, Ordering::Relaxed);
    let expected_ids: HashSet<String> = summaries.iter().map(|s| s.info.id.to_string()).collect();

    // Pin each ledger before spawning parallel work. Every task retains the
    // identity-checked file handle selected by the storage authority.
    let sessions: Vec<(Summary, TimelineLedgerReader)> = summaries
        .into_iter()
        .map(|summary| {
            storage
                .open_timeline_reader(&summary.info)
                .map(|reader| (summary, reader))
        })
        .collect::<io::Result<_>>()?;

    // Pre-scan: count sessions that will be skipped due to size cap
    let mut skipped_large = 0u64;
    for (_, reader) in &sessions {
        if should_skip_session(reader.snapshot_len(), BOOTSTRAP_MAX_FILE_SIZE) {
            skipped_large += 1;
        }
    }

    tracing::info!(
        total_sessions = sessions.len(),
        skipped_large = skipped_large,
        "session search bootstrap starting"
    );

    // Semaphore-bounded parallel indexing: spawn a task per session,
    // each acquiring a permit before doing the heavy I/O work.
    // max_concurrent (default 4) limits disk I/O contention and keeps
    // the tokio blocking thread pool available for other work.
    let semaphore = Arc::new(Semaphore::new(BOOTSTRAP_MAX_CONCURRENT.max(1)));
    let progress_arc = SEARCH_INDEX_MANAGER.progress.clone();
    let root_owned = root_dir.to_path_buf();

    let mut join_set = tokio::task::JoinSet::new();

    for (summary, timeline_reader) in sessions {
        let sem = semaphore.clone();
        let progress = progress_arc.clone();
        let root = root_owned.clone();
        let claim_lost = Arc::clone(&claim_lost);
        let timeout_dur = BOOTSTRAP_PER_SESSION_TIMEOUT;
        let max_file_size = BOOTSTRAP_MAX_FILE_SIZE;

        join_set.spawn(async move {
            // Acquire semaphore permit — this provides backpressure,
            // limiting concurrency to max_concurrent (default 4).
            // Safety: the semaphore is never closed — it lives in an Arc
            // shared only by tasks spawned in this loop, all of which
            // complete before the Arc is dropped.
            let _permit = sem.acquire().await.expect("semaphore is never closed");

            // A successor owns the index once the claim is lost. These
            // upserts are idempotent, not fenced; stopping just avoids
            // contending with it.
            if claim_lost.load(Ordering::Acquire) {
                return;
            }

            let session_id = summary.info.id.to_string();

            // File size pre-check: skip sessions with oversized Timeline ledgers.
            if should_skip_session(timeline_reader.snapshot_len(), max_file_size) {
                let file_size = timeline_reader.snapshot_len();
                tracing::debug!(
                    session_id = %session_id,
                    file_size = file_size,
                    max_size = max_file_size,
                    "skipping large session during bootstrap"
                );
                // Insert a title-only placeholder so title search still works;
                // insert-if-absent so an existing (fuller) row is never touched.
                let doc = build_session_doc(&summary, String::new());
                let db_path = search_db_path(&root);
                let title_only = tokio::task::spawn_blocking(move || {
                    with_search_index(&db_path, |index| index.insert_doc_if_absent(&doc))
                })
                .await;
                if let Err(e) = title_only.map_err(io::Error::other).and_then(|r| r) {
                    log_session_index_failure(
                        &session_id,
                        &e,
                        "failed to write title-only index row for large session",
                    );
                }
                progress.skipped.fetch_add(1, Ordering::Relaxed);
                return;
            }

            // Wrap with per-session timeout to prevent pipeline stalls.
            // The inner block is `async move` to own summary, timeline_reader,
            // and root — the outer block retains session_id and progress
            // for post-timeout error reporting.
            match tokio::time::timeout(timeout_dur, async move {
                // Collect content via one strict Timeline fold.
                let (content, bytes_read) = match tokio::task::spawn_blocking(move || {
                    collect_timeline_indexable_content(timeline_reader)
                })
                .await
                {
                    Ok(Ok(result)) => result,
                    Ok(Err(e)) => return Err(e),
                    Err(e) => return Err(io::Error::other(e)),
                };

                let doc = build_session_doc(&summary, content);
                let db_path = search_db_path(&root);

                // Each task opens its own SessionSearchIndex connection.
                // SQLite WAL mode handles concurrent readers + serialized writers.
                match tokio::task::spawn_blocking(move || {
                    with_search_index(&db_path, |index| {
                        if let Ok(Some(existing_hash)) = index.get_content_hash(&doc.session_id)
                            && existing_hash == doc.content_hash
                        {
                            return Ok(UpsertOutcome::Unchanged { bytes_read });
                        }
                        index.upsert_doc(&doc)?;
                        Ok(UpsertOutcome::Indexed { bytes_read })
                    })
                })
                .await
                {
                    Ok(result) => result,
                    Err(e) => Err(io::Error::other(e)),
                }
            })
            .await
            {
                Ok(Ok(outcome)) => match outcome {
                    UpsertOutcome::Indexed { bytes_read } => {
                        progress.bytes_read.fetch_add(bytes_read, Ordering::Relaxed);
                    }
                    UpsertOutcome::Unchanged { bytes_read } => {
                        progress.unchanged.fetch_add(1, Ordering::Relaxed);
                        progress.bytes_read.fetch_add(bytes_read, Ordering::Relaxed);
                    }
                    UpsertOutcome::NoContent => {}
                },
                Ok(Err(e)) => {
                    log_session_index_failure(
                        &session_id,
                        &e,
                        "failed to index session for search",
                    );
                    progress.skipped.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                Err(_) => {
                    // Timeout expired — the spawn_blocking task continues to
                    // completion but the pipeline moves on to the next session.
                    log_bootstrap_timeout(&session_id, timeout_dur.as_secs());
                    progress.skipped.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
            progress.indexed.fetch_add(1, Ordering::Relaxed);
        });
    }

    // Drain the JoinSet — wait for all tasks to complete
    while let Some(result) = join_set.join_next().await {
        if let Err(e) = result {
            tracing::warn!(error = %e, "session indexing task panicked");
        }
    }

    if claim_lost.load(Ordering::Acquire) {
        tracing::warn!("bootstrap claim lost; abandoning reindex without a completion marker");
        // A local heal quarantines the claim row with the file, which the
        // fenced refresh cannot tell from a takeover; only the takeover has
        // a successor that finishes the job.
        return Ok(if epoch.changed() {
            BootstrapOutcome::RunAgain
        } else {
            BootstrapOutcome::Done
        });
    }

    // Prune sessions deleted on disk. Fenced: `expected_ids` is a startup
    // snapshot, so a claimant that lost its lease must not delete rows a
    // successor indexed since; the refresh doubles as the ownership check.
    let db_path = search_db_path(root_dir);
    let prune_token = claim_token.as_str().to_string();
    let prune_expected = expected_ids.clone();
    tokio::task::spawn_blocking(move || -> io::Result<()> {
        with_search_index(&db_path, |index| {
            if !index.prune_missing_if_claim_owner(
                chrono::Utc::now().timestamp(),
                &prune_token,
                &prune_expected,
            )? {
                tracing::warn!("bootstrap claim lost; skipping stale orphan prune");
            }
            Ok(())
        })
    })
    .await
    .map_err(io::Error::other)??;

    let elapsed = start.elapsed();
    tracing::info!(
        indexed = progress.indexed.load(Ordering::Relaxed),
        skipped = progress.skipped.load(Ordering::Relaxed),
        unchanged = progress.unchanged.load(Ordering::Relaxed),
        duration_ms = elapsed.as_millis() as u64,
        bytes_read = progress.bytes_read.load(Ordering::Relaxed),
        "session search bootstrap complete"
    );

    progress.bootstrapping.store(false, Ordering::Release);

    let db_path_meta = search_db_path(root_dir);
    let mut needs_rebootstrap = epoch.changed();
    if needs_rebootstrap {
        tracing::warn!("session search cache healed during bootstrap; completion marker withheld");
    } else {
        match write_last_bootstrap_at_if_claim_owner(&db_path_meta, claim_token.as_str()) {
            Ok(true) if epoch.changed() => {
                tracing::warn!(
                    "session search cache healed while writing completion marker; clearing it"
                );
                if let Err(e) = clear_last_bootstrap_at(&db_path_meta) {
                    tracing::warn!(error = %e, "failed to clear stale completion marker after heal");
                }
                needs_rebootstrap = true;
            }
            Ok(true) => {}
            // No claim at all means the file was replaced under us (a heal
            // here or in a peer), not taken over; the fresh index is empty
            // and needs a rebuild.
            Ok(false) => match has_bootstrap_claim(&db_path_meta) {
                Ok(false) => {
                    tracing::warn!(
                        "session search index was replaced during bootstrap; rebuilding"
                    );
                    needs_rebootstrap = true;
                }
                _ => tracing::warn!("bootstrap claim lost; completion marker withheld"),
            },
            Err(e) => tracing::warn!(error = %e, "failed to write last_bootstrap_at metadata"),
        }
    }

    if needs_rebootstrap {
        return Ok(BootstrapOutcome::RunAgain);
    }

    Ok(BootstrapOutcome::Done)
}

const ASSISTANT_SEARCH_LIMIT: usize = 100_000;
const TOOL_SEARCH_LIMIT: usize = 100_000;
const TOOL_SEARCH_CALL_LIMIT: usize = 200;

/// Fold one validated Timeline into the search document for its selected
/// branch. Prompt text comes from Turn identities; assistant and tool-call
/// text comes from the uncompressed branch transcript. System instructions,
/// reasoning, synthetic directives, and tool results are intentionally absent.
fn timeline_indexable_content(timeline: &Timeline) -> String {
    let prompts = timeline
        .prompt_records()
        .into_iter()
        .map(|record| record.text)
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut assistant = String::new();
    let mut tools = String::new();
    let mut tool_calls = 0usize;

    for item in timeline.branch_transcript() {
        match item {
            ConversationItem::Assistant(item) => {
                push_search_text(&mut assistant, item.content.trim(), ASSISTANT_SEARCH_LIMIT);
                for call in item.tool_calls {
                    if tool_calls >= TOOL_SEARCH_CALL_LIMIT {
                        break;
                    }
                    tool_calls += 1;
                    push_search_text(&mut tools, &call.name, TOOL_SEARCH_LIMIT);
                    push_search_text(&mut tools, &call.arguments, TOOL_SEARCH_LIMIT);
                }
            }
            ConversationItem::BackendToolCall(item) => {
                if tool_calls < TOOL_SEARCH_CALL_LIMIT {
                    tool_calls += 1;
                    push_search_text(&mut tools, &item.text_summary(), TOOL_SEARCH_LIMIT);
                }
            }
            ConversationItem::System(_)
            | ConversationItem::User(_)
            | ConversationItem::ToolResult(_)
            | ConversationItem::Reasoning(_) => {}
        }
    }

    let mut joined = [prompts, assistant, tools].join("\n\n");
    if joined.len() > SEARCH_CONTENT_CHAR_LIMIT {
        let mut start = joined.len() - SEARCH_CONTENT_CHAR_LIMIT;
        while !joined.is_char_boundary(start) {
            start += 1;
        }
        joined = joined[start..].to_owned();
    }
    joined
}

fn push_search_text(output: &mut String, text: &str, limit: usize) {
    if text.is_empty() || output.len() >= limit {
        return;
    }
    let separator = usize::from(!output.is_empty());
    let budget = limit.saturating_sub(output.len()).saturating_sub(separator);
    if budget == 0 {
        return;
    }
    let mut take = text.len().min(budget);
    while take > 0 && !text.is_char_boundary(take) {
        take -= 1;
    }
    if separator != 0 {
        output.push('\n');
    }
    output.push_str(&text[..take]);
}

fn collect_timeline_indexable_content(
    reader: TimelineLedgerReader,
) -> io::Result<(String, u64)> {
    let bytes_read = reader.snapshot_len();
    let events = reader.read_events()?;
    let timeline = Timeline::from_events(events)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok((timeline_indexable_content(&timeline), bytes_read))
}

fn build_session_doc(summary: &Summary, content: String) -> SessionDoc {
    let title = summary.display_title().to_owned();

    let mut hasher = blake3::Hasher::new();
    hasher.update(title.as_bytes());
    hasher.update(b"\0");
    hasher.update(content.as_bytes());
    let content_hash = hasher.finalize().to_hex().to_string();

    SessionDoc {
        session_id: summary.info.id.to_string(),
        cwd: summary.info.cwd.clone(),
        updated_at_unix: summary.updated_at.timestamp(),
        title,
        content,
        content_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── process-gate test scaffolding ──────────────────────────────────────
    //
    // `SEARCH_GATE` is process-global, so any test that mutates it must be
    // `#[serial_test::serial]`, and every other test that observes it
    // (`execute_search`, `bootstrap_once`) is serial too so a temporary
    // CLOSED state can never leak into an unrelated test.

    /// Restores `SEARCH_GATE` but not `SEARCH_CLOSED_BY`, which is set once
    /// per process. Callers must be `#[serial]` so no other test observes
    /// the temporary state.
    #[must_use]
    struct IndexGateGuard {
        prior: u8,
    }

    impl IndexGateGuard {
        fn snapshot() -> Self {
            Self {
                prior: SEARCH_GATE.load(Ordering::Acquire),
            }
        }

        /// Force the unapplied state so a test can exercise the first-open
        /// transition. Only serialized gate tests may do this: the first
        /// non-serial reader that races it would resolve and latch the gate
        /// from the dev machine's disk tiers.
        fn unapplied() -> Self {
            let guard = Self::snapshot();
            SEARCH_GATE.store(SEARCH_GATE_UNAPPLIED, Ordering::Release);
            guard
        }
    }

    impl Drop for IndexGateGuard {
        fn drop(&mut self) {
            SEARCH_GATE.store(self.prior, Ordering::Release);
        }
    }

    /// Mutex serializing tests that touch the `GROW_SESSION_SEARCH` env var
    /// (env vars are process-global, so parallel tests race on them).
    static SESSION_SEARCH_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Run `f` with `GROW_SESSION_SEARCH` set to `value` (Some) or removed
    /// (None), restoring the previous value even on panic.
    fn with_session_search_env<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = SESSION_SEARCH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var("GROW_SESSION_SEARCH").ok();
        match value {
            Some(v) => unsafe { std::env::set_var("GROW_SESSION_SEARCH", v) },
            None => unsafe { std::env::remove_var("GROW_SESSION_SEARCH") },
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        match previous {
            Some(prev) => unsafe { std::env::set_var("GROW_SESSION_SEARCH", prev) },
            None => unsafe { std::env::remove_var("GROW_SESSION_SEARCH") },
        }
        result.unwrap_or_else(|p| std::panic::resume_unwind(p))
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_execute_search_empty_query() {
        let tmp = tempfile::TempDir::new().unwrap();
        let req = SessionSearchRequest {
            query: "   ".to_string(),
            cwd: None,
            limit: 10,
            offset: 0,
            include_content: false,
        };
        let resp = execute_search(tmp.path(), &req).await.unwrap();
        assert!(resp.results.is_empty());
        assert_eq!(resp.total_estimate, Some(0));
    }

    #[test]
    fn test_execute_search_returns_empty_on_fresh_db() {
        // Test the index directly instead of via `execute_search()` to avoid
        // a race with the global `SEARCH_INDEX_MANAGER` bootstrap worker that
        // concurrently opens the same SQLite DB (flaky "database is locked").
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = search_db_path(tmp.path());
        let index = SessionSearchIndex::open_or_create(&db_path).expect("open fresh DB");
        let result = index.query("hello world", None, 10, 0, false).unwrap();
        assert!(result.results.is_empty());
    }

    fn test_summary(session_id: &str, cwd: &str, title: &str) -> Summary {
        Summary {
            info: Info {
                id: acp::SessionId::new(session_id),
                cwd: cwd.to_string(),
            },
            cwd_generation: 0,
            previous_cwd: None,
            pending_cwd_switch_reminder: None,
            cwd_switch_bookkeeping_generation: 0,
            title: Some(title.to_string()),
            title_source: Some(chat_state::SessionTitleSource::User),
            title_event_seq: Some(1),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            num_messages: 0,
            current_model_id: acp::ModelId::new("test"),
            parent_session_id: None,
            forked_at: None,
            session_format_version: crate::session::persistence::SESSION_FORMAT_VERSION,
            prompt_display_cwd: None,
            session_kind: None,
            fork_context_source: None,
            fork_parent_prompt_id: None,
            hidden: None,
            source_workspace_dir: None,
            git_root_dir: None,
            git_remotes: Vec::new(),
            head_commit: None,
            head_branch: None,
            grow_home: None,
            last_active_at: None,
            worktree_label: None,
            agent_name: None,
            sandbox_profile: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn test_build_session_doc_hashes_content() {
        let summary = test_summary("test-session", "/workspace", "My session title");

        let doc = build_session_doc(&summary, "prompt text".to_string());
        assert_eq!(doc.session_id, "test-session");
        assert_eq!(doc.title, "My session title");
        assert_eq!(doc.content, "prompt text");
        assert!(!doc.content_hash.is_empty());

        // Same content + same title → same hash
        let doc2 = build_session_doc(&summary, "prompt text".to_string());
        assert_eq!(doc.content_hash, doc2.content_hash);
    }

    fn record_search_turn(
        timeline: &mut Timeline,
        id: u64,
        prompt_index: usize,
        prompt: &str,
        answer: &str,
        tool_calls: Vec<sampling_types::ToolCall>,
    ) {
        let turn = chat_state::TurnId(id);
        timeline
            .record(chat_state::TimelineEventKind::Turn(
                chat_state::TurnEvent::Started {
                    id: turn,
                    identity: chat_state::TurnIdentity {
                        origin: "user".into(),
                        turn_kind: "interactive".into(),
                        goal_id: None,
                        stage_id: None,
                    },
                    model_id: "model".into(),
                    input_message_count: timeline.surface().len(),
                    prompt_index,
                    prompt_text: prompt.into(),
                    input_kind: chat_state::TurnInputKind::Prompt,
                    redirect_kind: None,
                },
            ))
            .unwrap();
        let mut user = ConversationItem::user(prompt);
        user.set_prompt_index(prompt_index);
        timeline
            .append(user, chat_state::MessageCause::User)
            .unwrap();
        timeline
            .append(
                ConversationItem::Assistant(sampling_types::AssistantItem {
                    content: answer.into(),
                    tool_calls,
                    model_id: Some("model".into()),
                    model_fingerprint: None,
                    reasoning_effort: None,
                }),
                chat_state::MessageCause::Assistant,
            )
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Turn(
                chat_state::TurnEvent::Ended {
                    id: turn,
                    outcome: "completed".into(),
                    duration_ms: 1,
                    tool_count: 0,
                    terminal: chat_state::TurnTerminal {
                        stop_reason: "end_turn".into(),
                        completion_kind: "completed".into(),
                    },
                    cancellation_category: None,
                    details: None,
                },
            ))
            .unwrap();
    }

    #[test]
    fn timeline_index_includes_prompt_assistant_and_tool_identity() {
        let mut timeline = Timeline::default();
        record_search_turn(
            &mut timeline,
            1,
            0,
            "fix the bug\nin main.rs",
            "use \"quotes\" and café",
            vec![sampling_types::ToolCall {
                id: "call".into(),
                name: "read_file".into(),
                arguments: r#"{"path":"/tmp/foo.rs"}"#.into(),
            }],
        );

        let content = timeline_indexable_content(&timeline);
        assert!(content.contains("fix the bug\nin main.rs"));
        assert!(content.contains("use \"quotes\" and café"));
        assert!(content.contains("read_file"));
        assert!(content.contains("/tmp/foo.rs"));
    }

    #[test]
    fn timeline_index_excludes_rewound_branch() {
        let mut timeline = Timeline::default();
        record_search_turn(&mut timeline, 1, 0, "first prompt", "first reply", vec![]);
        record_search_turn(
            &mut timeline,
            2,
            1,
            "discarded prompt",
            "discarded reply",
            vec![],
        );
        let replacement = timeline.rewind_surface(1).unwrap();
        timeline
            .replace_all(replacement, chat_state::MessageCause::Rewind)
            .unwrap();
        record_search_turn(
            &mut timeline,
            3,
            1,
            "replacement prompt",
            "replacement reply",
            vec![],
        );

        let content = timeline_indexable_content(&timeline);
        assert!(content.contains("first prompt"));
        assert!(content.contains("replacement prompt"));
        assert!(!content.contains("discarded prompt"));
        assert!(!content.contains("discarded reply"));
    }

    #[test]
    fn timeline_index_caps_assistant_and_tool_content() {
        let tool_calls = (0..250)
            .map(|i| sampling_types::ToolCall {
                id: format!("call-{i}").into(),
                name: format!("tool_{i}"),
                arguments: "{}".into(),
            })
            .collect();
        let mut timeline = Timeline::default();
        record_search_turn(
            &mut timeline,
            1,
            0,
            "prompt",
            &"x".repeat(120_000),
            tool_calls,
        );

        let content = timeline_indexable_content(&timeline);
        assert!(content.chars().filter(|&ch| ch == 'x').count() <= ASSISTANT_SEARCH_LIMIT);
        assert!(content.contains("tool_199"));
        assert!(!content.contains("tool_200"));
        let mut tool_text = String::new();
        push_search_text(&mut tool_text, &"a".repeat(120_000), TOOL_SEARCH_LIMIT);
        assert_eq!(tool_text.len(), TOOL_SEARCH_LIMIT);
    }

    /// A title rename with identical content must produce a different hash,
    /// otherwise the dedup check in `upsert_session` skips the update and
    /// the old title stays in the index until the next full reindex.
    #[test]
    fn test_build_session_doc_title_change_changes_hash() {
        let old = test_summary("s1", "/workspace", "Old title");
        let new = test_summary("s1", "/workspace", "New title");
        let content = "same prompt text".to_string();

        let doc_old = build_session_doc(&old, content.clone());
        let doc_new = build_session_doc(&new, content);

        assert_ne!(
            doc_old.content_hash, doc_new.content_hash,
            "title change must produce a different hash so dedup doesn't skip the upsert"
        );
    }

    #[test]
    fn test_build_session_doc_uses_title_projection() {
        let mut summary = test_summary("s1", "/workspace", "session summary");
        summary.title = Some("Generated Title".to_string());
        let doc = build_session_doc(&summary, "content".to_string());
        assert_eq!(doc.title, "Generated Title");

        summary.title = None;
        let doc2 = build_session_doc(&summary, "content".to_string());
        assert_eq!(doc2.title, "");
    }

    // ── should_skip_session tests ──────────────────────────────────────────

    #[test]
    fn test_should_skip_session_large_file() {
        assert!(should_skip_session(1024, 512));
    }

    #[test]
    fn test_should_skip_session_small_file() {
        assert!(!should_skip_session(1024, 2048));
    }

    #[test]
    fn test_should_skip_session_exact_limit() {
        assert!(!should_skip_session(1024, 1024));
    }

    // ── progress and status tests ──────────────────────────────────────────

    #[test]
    fn test_bootstrap_progress_extended_defaults() {
        let progress = BootstrapProgress::default();
        assert!(!progress.bootstrapping.load(Ordering::Relaxed));
        assert_eq!(progress.indexed.load(Ordering::Relaxed), 0);
        assert_eq!(progress.total.load(Ordering::Relaxed), 0);
        assert_eq!(progress.skipped.load(Ordering::Relaxed), 0);
        assert_eq!(progress.unchanged.load(Ordering::Relaxed), 0);
        assert_eq!(progress.bytes_read.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_search_index_status_serialization() {
        let status = SearchIndexStatus {
            bootstrapping: true,
            indexed: 10,
            total: 20,
            skipped: 3,
            unchanged: 5,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"skipped\":3"));
        assert!(json.contains("\"unchanged\":5"));
        assert!(json.contains("\"bootstrapping\":true"));
    }

    #[test]
    fn timeline_collector_reports_bytes_and_validates_lifecycle() {
        use std::io::Write as _;

        let mut timeline = Timeline::default();
        record_search_turn(&mut timeline, 1, 0, "hello", "world", vec![]);
        let mut file = tempfile::NamedTempFile::new().unwrap();
        for event in timeline.events() {
            serde_json::to_writer(&mut file, event).unwrap();
            writeln!(file).unwrap();
        }
        file.flush().unwrap();
        let file_size = std::fs::metadata(file.path()).unwrap().len();

        let reader = TimelineLedgerReader::from_file(
            file.reopen().unwrap(),
            file.path().to_path_buf(),
        )
        .unwrap();
        let (content, bytes_read) = collect_timeline_indexable_content(reader).unwrap();
        assert!(content.contains("hello"));
        assert!(content.contains("world"));
        assert_eq!(bytes_read, file_size);
    }

    #[test]
    fn timeline_collector_reads_the_pinned_entity_after_namespace_replacement() {
        use std::io::Write as _;

        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("timeline.jsonl");
        let displaced = temp.path().join("original.jsonl");

        let mut original = Timeline::default();
        record_search_turn(&mut original, 1, 0, "original prompt", "original answer", vec![]);
        let mut file = std::fs::File::create(&path).unwrap();
        for event in original.events() {
            serde_json::to_writer(&mut file, event).unwrap();
            writeln!(file).unwrap();
        }
        file.sync_all().unwrap();

        let reader = TimelineLedgerReader::from_file(
            std::fs::File::open(&path).unwrap(),
            path.clone(),
        )
        .unwrap();

        std::fs::rename(&path, &displaced).unwrap();
        let mut decoy = Timeline::default();
        record_search_turn(&mut decoy, 2, 0, "decoy prompt", "decoy answer", vec![]);
        let mut replacement = std::fs::File::create(&path).unwrap();
        for event in decoy.events() {
            serde_json::to_writer(&mut replacement, event).unwrap();
            writeln!(replacement).unwrap();
        }
        replacement.sync_all().unwrap();

        let (content, _) = collect_timeline_indexable_content(reader).unwrap();
        assert!(content.contains("original prompt"));
        assert!(!content.contains("decoy prompt"));
    }

    // ── bootstrap_once eager flag tests ────────────────────────────────────
    // NOTE: SEARCH_INDEX_MANAGER is a process-wide singleton, so tests
    // that depend on the `bootstrapping` flag transitioning to `false`
    // are racy when run in parallel (another test's bootstrap_once()
    // can re-set the flag). Only the eager-set test is reliable because
    // the store is synchronous before the channel send.

    #[serial_test::serial]
    #[tokio::test]
    async fn test_bootstrap_once_sets_flag_eagerly() {
        let tmp = tempfile::TempDir::new().unwrap();
        SEARCH_INDEX_MANAGER.bootstrap_once(tmp.path().to_path_buf());
        assert!(
            SEARCH_INDEX_MANAGER
                .progress
                .bootstrapping
                .load(Ordering::Acquire),
            "bootstrapping flag must be true immediately after bootstrap_once()",
        );
    }

    #[serial_test::serial]
    #[tokio::test]
    async fn test_execute_search_completes_on_fresh_db() {
        let tmp = tempfile::TempDir::new().unwrap();
        let req = SessionSearchRequest {
            query: "nonexistent-query-xyzzy".to_string(),
            cwd: None,
            limit: 10,
            offset: 0,
            include_content: false,
        };
        let resp = execute_search(tmp.path(), &req).await.unwrap();
        assert!(resp.results.is_empty());
    }

    // ── bootstrap freshness recheck tests ──────────────────────────────────
    // These test the free functions directly (per-tmp-root DB state), not the
    // global SEARCH_INDEX_MANAGER, whose `bootstrapping` flag is process-wide
    // and racy across parallel tests (see NOTE above).

    /// The predicate behind `SearchIndexJob::RecheckBootstrap`: a completed
    /// bootstrap leaves `last_bootstrap_at`; the upgrade drop in
    /// `open_or_create` deletes it, so the probe's own open detects a
    /// downgraded index.
    #[tokio::test]
    async fn test_has_completed_bootstrap_marker_lifecycle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let db_path = search_db_path(root);

        // No DB file at all → genuinely absent (not a read error).
        assert_eq!(has_completed_bootstrap_marker(root).await, Some(false));

        // A completed bootstrap at the current schema version → marker set.
        write_last_bootstrap_at(&db_path).unwrap();
        assert_eq!(has_completed_bootstrap_marker(root).await, Some(true));

        // Simulate an older (pre-ratchet) binary having wiped and re-stamped
        // the DB: version row regressed below current, `last_bootstrap_at`
        // still recent.
        {
            let index = SessionSearchIndex::open_or_create(&db_path).unwrap();
            index
                .set_meta("session_search_schema_version", "3")
                .unwrap();
        }
        assert_eq!(
            has_completed_bootstrap_marker(root).await,
            Some(false),
            "a downgraded index must not count as bootstrapped even with a recent marker"
        );

        // A subsequent completed bootstrap restores the marker.
        write_last_bootstrap_at(&db_path).unwrap();
        assert_eq!(has_completed_bootstrap_marker(root).await, Some(true));
    }

    /// End-to-end recheck healing: `RecheckBootstrap` on a marker-less index
    /// re-runs the full bootstrap, which rewrites the marker on completion.
    #[serial_test::serial]
    #[tokio::test]
    async fn test_recheck_bootstrap_reruns_reindex_when_marker_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let storage: Box<dyn StorageAdapter> = Box::new(
            crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.to_path_buf()),
        );
        let mut pending: HashMap<SessionSearchKey, Instant> = HashMap::new();

        assert_eq!(has_completed_bootstrap_marker(root).await, Some(false));
        handle_job(
            root,
            storage.as_ref(),
            &mut pending,
            SearchIndexJob::RecheckBootstrap,
            Duration::from_millis(1),
        )
        .await;
        assert_eq!(
            has_completed_bootstrap_marker(root).await,
            Some(true),
            "recheck on a marker-less index must re-run the bootstrap, which rewrites the marker"
        );
    }

    /// Regression shape: a v3-era indexer silently extracted "" for
    /// sessions with JSON escapes but still recorded a content hash, so at
    /// the *same* schema version the hash dedup keeps skipping identical
    /// (buggy) re-extractions forever. Pins that the v4 upgrade drop removes
    /// the stub row and its hash, so the next bootstrap re-indexes from
    /// scratch instead of being blocked by the stale hash.
    #[test]
    fn test_upgrade_drop_clears_stub_docs_and_hashes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = search_db_path(tmp.path());

        let summary = test_summary("stub", "/ws", "");
        let stub = build_session_doc(&summary, String::new());
        {
            let index = SessionSearchIndex::open_or_create(&db_path).unwrap();
            index.upsert_doc(&stub).unwrap();
            // The empty-content stub still records a hash — re-extracting
            // the same (empty) content would dedup to Unchanged.
            assert_eq!(
                index.get_content_hash("stub").unwrap().as_deref(),
                Some(stub.content_hash.as_str())
            );
            index
                .set_meta("session_search_schema_version", "3")
                .unwrap();
        }

        let index = SessionSearchIndex::open_or_create(&db_path).unwrap();
        assert_eq!(
            index.get_content_hash("stub").unwrap(),
            None,
            "the upgrade drop must clear stub rows so their stale hashes cannot block re-indexing"
        );
    }

    // ── gate tests ─────────────────────────────────────────────────────────

    #[test]
    fn test_session_search_off_reason_mapping() {
        assert_eq!(
            session_search_off_reason(ConfigSource::Requirement),
            "a requirements.toml pin or an MDM policy"
        );
        assert_eq!(
            session_search_off_reason(ConfigSource::Env),
            "the GROW_SESSION_SEARCH environment variable"
        );
        for source in [
            ConfigSource::Config,
            ConfigSource::UserConfig,
            ConfigSource::ManagedConfig,
            ConfigSource::SystemManagedConfig,
        ] {
            assert_eq!(
                session_search_off_reason(source),
                "the session_search key in a Grow config file"
            );
        }
        for source in [
            ConfigSource::Cli,
            ConfigSource::Remote,
            ConfigSource::Default,
        ] {
            assert_eq!(session_search_off_reason(source), "a local setting");
        }
    }

    #[serial_test::serial]
    #[test]
    fn test_resolve_session_search_precedence() {
        with_session_search_env(None, || {
            // Unset everywhere → on by default.
            let r = resolve_session_search_setting(None, None);
            assert!(r.value);
            assert_eq!(r.source, ConfigSource::Default);

            // A requirements pin wins everything, including a config that
            // says on.
            let r = resolve_session_search_setting(
                Some(false),
                Some(SessionSearchConfig {
                    enabled: Some(true),
                }),
            );
            assert!(!r.value);
            assert_eq!(r.source, ConfigSource::Requirement);

            // The config file tier beats the default.
            let r = resolve_session_search_setting(
                None,
                Some(SessionSearchConfig {
                    enabled: Some(false),
                }),
            );
            assert!(!r.value);
            assert_eq!(r.source, ConfigSource::Config);

            // An unset config field falls through to the default.
            let r = resolve_session_search_setting(None, Some(SessionSearchConfig::default()));
            assert!(r.value);
            assert_eq!(r.source, ConfigSource::Default);
        });
        with_session_search_env(Some("0"), || {
            // Env off beats a config that says on.
            let r = resolve_session_search_setting(
                None,
                Some(SessionSearchConfig {
                    enabled: Some(true),
                }),
            );
            assert!(!r.value);
            assert_eq!(r.source, ConfigSource::Env);

            // A pin still outranks the environment.
            let r = resolve_session_search_setting(Some(true), None);
            assert!(r.value);
            assert_eq!(r.source, ConfigSource::Requirement);
        });
    }

    /// The off state must be diagnosable (an error naming the closing tier,
    /// never an empty result set) and must not create an index. The latch
    /// cannot reopen in this process once closed.
    #[serial_test::serial]
    #[tokio::test]
    async fn test_search_gate_off_blocks_search_and_never_reopens() {
        let _gate = IndexGateGuard::unapplied();
        apply_search_gate(&Resolved::new(false, ConfigSource::Env));

        let tmp = tempfile::TempDir::new().unwrap();
        let req = SessionSearchRequest {
            query: "hello".to_string(),
            cwd: None,
            limit: 10,
            offset: 0,
            include_content: false,
        };

        let source = search_closed_by().expect("the closing source must be recorded");
        let expected = format!(
            "session search is off ({})",
            session_search_off_reason(source)
        );

        let err = execute_search(tmp.path(), &req).await.unwrap_err();
        assert_eq!(err.to_string(), expected);
        assert!(
            !search_index_exists(tmp.path()),
            "off must not create an index"
        );

        // bootstrap_once must not arm anything while off.
        SEARCH_INDEX_MANAGER.bootstrap_once(tmp.path().to_path_buf());
        assert!(!search_index_exists(tmp.path()));

        // Even when a setting now says on, the gate stays closed for the
        // process: the completed-bootstrap marker outlives the time spent
        // off, so reopening mid-process would serve a half-index.
        apply_search_gate(&Resolved::new(true, ConfigSource::Default));
        assert!(
            !is_index_enabled(),
            "a closed gate must never reopen in-process"
        );

        let err2 = execute_search(tmp.path(), &req).await.unwrap_err();
        assert_eq!(
            err2.to_string(),
            expected,
            "the original closing source stays recorded"
        );
        assert!(!search_index_exists(tmp.path()));
    }

    #[serial_test::serial]
    #[test]
    fn test_search_gate_unapplied_opens() {
        let _gate = IndexGateGuard::unapplied();
        apply_search_gate(&Resolved::new(true, ConfigSource::Default));
        assert!(is_index_enabled());
    }

    /// A session save queued while the gate is off must be dropped, not
    /// written to (or creating) the index.
    #[serial_test::serial]
    #[tokio::test]
    async fn test_off_state_drops_queued_writes() {
        let _gate = IndexGateGuard::snapshot();
        apply_search_gate(&Resolved::new(false, ConfigSource::Env));

        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let storage: Box<dyn StorageAdapter> = Box::new(
            crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.to_path_buf()),
        );
        let info = Info {
            id: acp::SessionId::new("s1"),
            cwd: "/ws".to_string(),
        };
        storage
            .init_session(&info, acp::ModelId::new("test"))
            .await
            .unwrap();

        let key = SessionSearchKey {
            session_id: "s1".to_string(),
            cwd: "/ws".to_string(),
        };
        upsert_by_key(root, storage.as_ref(), &key).await.unwrap();
        assert!(
            !search_index_exists(root),
            "a write declined while off must not create the index"
        );
    }

    // ── delete-evict contract ──────────────────────────────────────────────

    /// A session delete must remove its index row even when no index was
    /// ever built, and must never create an index as a side effect.
    #[tokio::test]
    async fn test_evict_removes_row_and_never_creates_index() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        // No index exists: the evict path must not create one.
        delete_session(root, "s1").await.unwrap();
        assert!(!search_index_exists(root), "no index may be created");

        // Build a row, then evict through the real call point
        // (load_summary → NotFound → delete_doc): the row must be gone.
        let doc = build_session_doc(
            &test_summary("s1", "/ws", "a memorable title"),
            "indexed body text".to_string(),
        );
        with_search_index(&search_db_path(root), |index| index.upsert_doc(&doc)).unwrap();
        let hits = |query: &str| {
            with_search_index(&search_db_path(root), |index| {
                index.query(query, None, 10, 0, false)
            })
            .unwrap()
            .results
            .len()
        };
        assert_eq!(hits("memorable"), 1);

        let storage: Box<dyn StorageAdapter> = Box::new(
            crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.to_path_buf()),
        );
        upsert_by_key(
            root,
            storage.as_ref(),
            &SessionSearchKey {
                session_id: "s1".to_string(),
                cwd: "/ws".to_string(),
            },
        )
        .await
        .unwrap();

        assert_eq!(hits("memorable"), 0, "a delete must land");
    }

    // ── bootstrap lease gate tests ─────────────────────────────────────────

    /// The gate itself: a launch's first claim reindexes even when a
    /// completed marker exists (the launch owes pruning and skipped
    /// retries), rewrites the marker, and releases the claim.
    #[serial_test::serial]
    #[tokio::test]
    async fn test_launch_claimant_reindexes_even_when_marker_exists() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let db_path = search_db_path(root);
        with_search_index(&db_path, |index| {
            index.set_meta(META_KEY_LAST_BOOTSTRAP, "123")
        })
        .unwrap();

        let storage: Box<dyn StorageAdapter> = Box::new(
            crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.to_path_buf()),
        );
        let outcome = bootstrap_with_lease(root, storage.as_ref(), BootstrapRole::Launch)
            .await
            .unwrap();
        assert_eq!(outcome, BootstrapOutcome::Done);

        assert_ne!(
            with_search_index(&db_path, |index| index.get_meta(META_KEY_LAST_BOOTSTRAP)).unwrap(),
            Some("123".to_string()),
            "the reindex must rewrite the marker"
        );
        assert_eq!(
            with_search_index(&db_path, |index| {
                index.get_meta(search_fts::META_KEY_BOOTSTRAP_CLAIM)
            })
            .unwrap(),
            None,
            "the claim must be released"
        );
    }

    /// A waiter adopts a peer's completed marker without reindexing, leaving
    /// the peer's claim row alone.
    #[serial_test::serial]
    #[tokio::test]
    async fn test_launch_waiter_adopts_peer_marker_without_reindexing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let db_path = search_db_path(root);

        let now = chrono::Utc::now().timestamp();
        with_search_index(&db_path, |index| {
            index.try_claim_bootstrap(now, BOOTSTRAP_LEASE_DURATION, "peer")
        })
        .unwrap();
        with_search_index(&db_path, |index| {
            index.set_meta(META_KEY_LAST_BOOTSTRAP, "123")
        })
        .unwrap();

        let storage: Box<dyn StorageAdapter> = Box::new(
            crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.to_path_buf()),
        );
        let outcome = bootstrap_with_lease(root, storage.as_ref(), BootstrapRole::Launch)
            .await
            .unwrap();
        assert_eq!(outcome, BootstrapOutcome::Done);

        assert_eq!(
            with_search_index(&db_path, |index| index.get_meta(META_KEY_LAST_BOOTSTRAP)).unwrap(),
            Some("123".to_string()),
            "the waiter adopts the marker instead of reindexing"
        );
        assert_eq!(
            with_search_index(&db_path, |index| {
                index.get_meta(search_fts::META_KEY_BOOTSTRAP_CLAIM)
            })
            .unwrap(),
            Some(format!("{now}:peer")),
            "the waiter must not touch the peer's claim"
        );
    }

    /// A recheck (post-first search) gives up at once when a peer holds the
    /// claim, without reindexing; the next search re-probes the marker.
    #[serial_test::serial]
    #[tokio::test]
    async fn test_recheck_gives_up_when_peer_holds_claim() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let db_path = search_db_path(root);

        let now = chrono::Utc::now().timestamp();
        with_search_index(&db_path, |index| {
            index.try_claim_bootstrap(now, BOOTSTRAP_LEASE_DURATION, "peer")
        })
        .unwrap();

        let storage: Box<dyn StorageAdapter> = Box::new(
            crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.to_path_buf()),
        );
        let outcome = try_bootstrap_with_lease(root, storage.as_ref())
            .await
            .unwrap();
        assert_eq!(outcome, BootstrapOutcome::Done);

        assert_eq!(
            with_search_index(&db_path, |index| index.get_meta(META_KEY_LAST_BOOTSTRAP)).unwrap(),
            None,
            "no reindex ran, so no marker appears"
        );
        assert_eq!(
            with_search_index(&db_path, |index| {
                index.get_meta(search_fts::META_KEY_BOOTSTRAP_CLAIM)
            })
            .unwrap(),
            Some(format!("{now}:peer")),
            "the peer's live claim is left alone"
        );
    }
}

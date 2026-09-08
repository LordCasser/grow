//! Slash command MRU / recency (`$GROW_HOME/slash-mru.json`).
//!
//! Flat `command → last_used` map (canonical names). Tiebreaks use recency
//! decay (7-day half-life, 0.1 floor). Bounded to [`MAX_ENTRIES`].
//!
//! Ownership: each [`crate::slash::SlashController`] holds an
//! `Rc<RefCell<SlashMru>>` (single-threaded UI; no mutex). `AppView` owns one
//! store and injects it into every controller (agent prompts + dashboard
//! dispatch) so they stay in sync — no process-global singleton. Default and
//! test controllers get an isolated in-memory store (no disk I/O).
//!
//! Persistence: a `touch` only marks the store dirty (never blocks the UI on
//! disk). When a command is recorded, the controller hands an owned
//! [`MruSnapshot`] to [`persist_async`], which serializes writes through one
//! long-lived background thread (atomic temp-file + rename). The `Rc<RefCell>`
//! itself never crosses a thread boundary; only the `Send` snapshot does.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::util::grow_home;

const RECENCY_HALF_LIFE_SECS: f64 = 7.0 * 86_400.0;
const RECENCY_FLOOR: f64 = 0.1;
const MAX_ENTRIES: usize = 256;
const MAX_STORE_BYTES: u64 = 1_048_576;

/// Canonical on-disk format.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MruFile {
    by_command: HashMap<String, u64>,
}

#[derive(Debug)]
pub struct SlashMru {
    by_command: HashMap<String, u64>,
    loaded: bool,
    dirty: bool,
    /// When false (tests), never touch disk.
    persist_enabled: bool,
}

impl Default for SlashMru {
    fn default() -> Self {
        Self {
            by_command: HashMap::new(),
            loaded: false,
            dirty: false,
            persist_enabled: true,
        }
    }
}

impl SlashMru {
    pub fn new() -> Self {
        Self::default()
    }

    /// Isolated store for unit tests (no disk I/O).
    pub fn new_in_memory() -> Self {
        Self {
            loaded: true,
            persist_enabled: false,
            ..Self::default()
        }
    }

    fn store_path() -> PathBuf {
        grow_home().join("slash-mru.json")
    }

    fn normalize_command(command_name: &str) -> Option<String> {
        let name = command_name.trim().trim_start_matches('/');
        if name.is_empty() {
            None
        } else {
            Some(name.to_string())
        }
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Recency-with-decay tiebreak score. Pure recency (one `last_used`
    /// timestamp per command, no use-count) scaled by an exponential decay so a
    /// long-stale entry can't win ties forever; the floor keeps any prior use
    /// just above never-used.
    fn recency_score(last_used: u64, now: u64) -> u64 {
        if last_used == 0 {
            return 0;
        }
        let age = now.saturating_sub(last_used) as f64;
        let factor = (0.5_f64.powf(age / RECENCY_HALF_LIFE_SECS)).max(RECENCY_FLOOR);
        ((last_used as f64) * factor) as u64
    }

    fn ensure_loaded(&mut self) {
        if self.loaded || !self.persist_enabled {
            if !self.loaded {
                self.loaded = true;
            }
            return;
        }
        self.load_from_path(&Self::store_path());
    }

    fn load_from_path(&mut self, path: &std::path::Path) {
        match read_store(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.loaded = true;
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "slash MRU: read failed; using empty store, persistence disabled for session"
                );
                // Mark loaded so we don't re-attempt the read on every
                // `rank_score` (once per candidate per keystroke on the UI
                // thread), and disable persistence so we never clobber a file
                // we couldn't read.
                self.loaded = true;
                self.persist_enabled = false;
            }
            Ok(bytes) => match serde_json::from_slice::<MruFile>(&bytes) {
                Ok(file) => {
                    self.by_command = file.by_command;
                    self.trim_to_cap();
                    self.loaded = true;
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "slash MRU: corrupt file ignored"
                    );
                    self.loaded = true;
                }
            },
        }
    }

    fn trim_to_cap(&mut self) {
        if self.by_command.len() <= MAX_ENTRIES {
            return;
        }
        let mut entries: Vec<(String, u64)> = self.by_command.drain().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        entries.truncate(MAX_ENTRIES);
        self.by_command = entries.into_iter().collect();
    }

    /// Record use of a canonical command name (ignores typed prefix; flat model).
    pub fn touch(&mut self, _typed_prefix: &str, command_name: &str) {
        let Some(cmd) = Self::normalize_command(command_name) else {
            return;
        };
        self.ensure_loaded();
        let now = Self::now_secs();
        self.by_command.insert(cmd, now);
        self.trim_to_cap();
        if self.persist_enabled {
            self.dirty = true;
        }
    }

    pub fn last_used(&mut self, _typed_prefix: &str, command_name: &str) -> u64 {
        let Some(cmd) = Self::normalize_command(command_name) else {
            return 0;
        };
        self.ensure_loaded();
        self.by_command.get(&cmd).copied().unwrap_or(0)
    }

    pub fn rank_score(&mut self, _typed_prefix: &str, command_name: &str) -> u64 {
        let ts = self.last_used("", command_name);
        Self::recency_score(ts, Self::now_secs())
    }

    /// Take an owned, `Send` snapshot to persist when dirty; clears the dirty
    /// flag. Returns `None` when persistence is disabled (tests) or nothing
    /// changed. The snapshot is written off the UI thread by [`persist_async`].
    pub fn take_persist_snapshot(&mut self) -> Option<MruSnapshot> {
        if !self.persist_enabled || !self.dirty {
            return None;
        }
        let file = MruFile {
            by_command: self.by_command.clone(),
        };
        let bytes = serde_json::to_vec(&file).ok()?;
        self.dirty = false;
        Some(MruSnapshot {
            path: Self::store_path(),
            bytes,
        })
    }

    /// Re-flag unpersisted changes after a failed write so the next
    /// [`Self::take_persist_snapshot`] retries. No-op when persistence is off.
    pub fn mark_dirty(&mut self) {
        if self.persist_enabled {
            self.dirty = true;
        }
    }

    #[cfg(test)]
    pub fn seed_for_test(&mut self, _prefix: &str, command_name: &str, last_used: u64) {
        self.loaded = true;
        self.persist_enabled = false;
        if let Some(cmd) = Self::normalize_command(command_name) {
            self.by_command.insert(cmd, last_used);
        }
    }
}

fn read_store(path: &std::path::Path) -> io::Result<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_STORE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "MRU store is not a regular file within the byte allowance"));
    }
    read_store_bytes(file, MAX_STORE_BYTES)
}

fn read_store_bytes(reader: impl Read, allowance: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(allowance + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > allowance {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "MRU store exceeds the byte allowance"));
    }
    Ok(bytes)
}

/// An owned, `Send` snapshot of the MRU ready to write to disk. Produced on
/// the UI thread by [`SlashMru::take_persist_snapshot`]; written off-thread.
#[derive(Debug)]
pub struct MruSnapshot {
    path: PathBuf,
    bytes: Vec<u8>,
}

impl MruSnapshot {
    /// Atomic write (temp file + `fsync` + rename). Returns `true` on success.
    /// Safe on a worker thread.
    fn write(&self) -> bool {
        let write_ok = (|| -> io::Result<()> {
            let parent = self.path.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            fs::create_dir_all(parent)?;
            let mut temp = tempfile::Builder::new().prefix(".slash-mru-").tempfile_in(parent)?;
            temp.write_all(&self.bytes)?;
            temp.as_file().sync_all()?;
            temp.persist(&self.path).map_err(|error| error.error)?;
            Ok(())
        })();
        match write_ok {
            Ok(()) => true,
            Err(e) => {
                tracing::debug!(error = %e, "slash MRU: persist failed");
                false
            }
        }
    }
}

/// One pending complete snapshot, plus a bounded notification channel.
/// The worker never holds the pending lock during file IO.
struct MruWriter {
    wake: SyncSender<()>,
    pending: Arc<Mutex<Option<MruSnapshot>>>,
}

impl MruWriter {
    fn channel() -> (Self, Receiver<()>) {
        let (wake, receiver) = mpsc::sync_channel(1);
        (Self { wake, pending: Arc::new(Mutex::new(None)) }, receiver)
    }

    fn submit(&self, snapshot: MruSnapshot) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        *pending = Some(snapshot);
        match self.wake.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => true,
            Err(TrySendError::Disconnected(())) => {
                *pending = None;
                false
            }
        }
    }
}

fn write_pending_snapshots(receiver: Receiver<()>, pending: Arc<Mutex<Option<MruSnapshot>>>) {
    while receiver.recv().is_ok() {
        let snapshot = match pending.lock() {
            Ok(mut pending) => pending.take(),
            Err(_) => return,
        };
        if let Some(snapshot) = snapshot {
            snapshot.write();
        }
    }
}

/// Hand off a complete snapshot without doing file IO on the caller thread.
/// At most one latest snapshot waits behind the current write. A failed
/// handoff returns false so the controller can retain dirty state; an accepted
/// write is best effort, as before. Later command use sends the complete map
/// again, including entries from any failed write. Process exit does not flush.
pub fn persist_async(snapshot: MruSnapshot) -> bool {
    static WRITER: OnceLock<Option<MruWriter>> = OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (writer, receiver) = MruWriter::channel();
        let pending = Arc::clone(&writer.pending);
        match std::thread::Builder::new()
            .name("slash-mru-writer".to_string())
            .spawn(move || write_pending_snapshots(receiver, pending)) {
            Ok(_) => Some(writer),
            Err(e) => {
                tracing::debug!(error = %e, "slash MRU: writer thread spawn failed");
                None
            }
        }
    });
    writer.as_ref().is_some_and(|writer| writer.submit(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_snapshots_coalesce_and_worker_publishes_latest() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mru.json");
        let (writer, receiver) = MruWriter::channel();
        for timestamp in 1..=1000 {
            let bytes = serde_json::to_vec(&MruFile {
                by_command: HashMap::from([("model".to_string(), timestamp)]),
            }).unwrap();
            assert!(writer.submit(MruSnapshot { path: path.clone(), bytes }));
        }
        assert!(!path.exists(), "submission must not perform file IO");
        let latest = writer.pending.lock().unwrap().as_ref().unwrap().bytes.clone();
        assert_eq!(serde_json::from_slice::<MruFile>(&latest).unwrap().by_command["model"], 1000);
        let pending = Arc::clone(&writer.pending);
        drop(writer);
        let worker = std::thread::spawn(move || write_pending_snapshots(receiver, pending));
        worker.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), latest);
    }

    #[test]
    fn in_flight_snapshot_does_not_block_or_replace_latest_pending() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mru.json");
        let (writer, receiver) = MruWriter::channel();
        assert!(writer.submit(MruSnapshot { path: path.clone(), bytes: b"first".to_vec() }));
        receiver.recv().unwrap();
        // Hold the snapshot as a worker does after taking it, before disk IO.
        let in_flight = writer.pending.lock().unwrap().take().unwrap();
        for bytes in [b"middle".as_slice(), b"latest".as_slice()] {
            assert!(writer.submit(MruSnapshot { path: path.clone(), bytes: bytes.to_vec() }));
        }
        assert!(in_flight.write());
        assert_eq!(fs::read(&path).unwrap(), b"first");
        let pending = Arc::clone(&writer.pending);
        drop(writer);
        write_pending_snapshots(receiver, pending);
        assert_eq!(fs::read(&path).unwrap(), b"latest");
    }

    #[test]
    fn disconnected_writer_rejects_without_synchronous_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mru.json");
        fs::write(&path, b"existing").unwrap();
        let (writer, receiver) = MruWriter::channel();
        drop(receiver);
        assert!(!writer.submit(MruSnapshot { path: path.clone(), bytes: b"new".to_vec() }));
        assert!(writer.pending.lock().unwrap().is_none());
        assert_eq!(fs::read(&path).unwrap(), b"existing");
    }

    #[test]
    fn bounded_reader_handles_exact_limit_and_growth() {
        for length in [0, 8, 9, 128] {
            let mut reader = io::Cursor::new(vec![b' '; length]);
            let result = read_store_bytes(&mut reader, 8);
            assert_eq!(result.is_ok(), length <= 8);
            assert_eq!(reader.position(), length.min(9) as u64);
        }
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"1234").unwrap();
        assert_eq!(file.metadata().unwrap().len(), 4);
        let mut other_handle = file.try_clone().unwrap();
        other_handle.write_all(&[b'x'; 64]).unwrap();
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0)).unwrap();
        assert!(read_store_bytes(&mut file, 4).is_err());
        assert_eq!(file.stream_position().unwrap(), 5);
    }

    #[test]
    fn oversized_store_disables_persistence_without_overwriting() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mru.json");
        let mut bytes = br#"{"by_command":{"model":42}}"#.to_vec();
        bytes.resize(MAX_STORE_BYTES as usize, b' ');
        fs::write(&path, &bytes).unwrap();
        let mut exact = SlashMru::new();
        exact.load_from_path(&path);
        assert_eq!(exact.last_used("", "model"), 42);
        assert!(exact.persist_enabled);
        bytes.push(b' ');
        fs::write(&path, &bytes).unwrap();
        let mut over = SlashMru::new();
        over.load_from_path(&path);
        assert!(over.loaded);
        assert!(!over.persist_enabled);
        over.touch("", "plan");
        assert!(over.take_persist_snapshot().is_none());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn missing_corrupt_and_large_entry_count_keep_existing_policy() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mru.json");
        for bytes in [None, Some(b"invalid json".as_slice())] {
            if let Some(bytes) = bytes { fs::write(&path, bytes).unwrap(); }
            let mut store = SlashMru::new();
            store.load_from_path(&path);
            assert!(store.loaded && store.persist_enabled);
            store.touch("", "plan");
            assert!(store.take_persist_snapshot().is_some());
        }
        let entries = (0..300).map(|i| (format!("command-{i}"), i)).collect();
        fs::write(&path, serde_json::to_vec(&MruFile { by_command: entries }).unwrap()).unwrap();
        let mut store = SlashMru::new();
        store.load_from_path(&path);
        assert_eq!(store.by_command.len(), MAX_ENTRIES);
        assert_eq!(store.last_used("", "command-299"), 299);
        assert!(!store.by_command.contains_key("command-0"));
    }

    #[cfg(unix)]
    #[test]
    fn loader_accepts_regular_symlink_and_rejects_fifo_without_writer() {
        use std::os::unix::fs::symlink;
        use std::os::unix::ffi::OsStrExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mru.json");
        let link = directory.path().join("link");
        let fifo = directory.path().join("fifo");
        fs::write(&path, br#"{"by_command":{"model":42}}"#).unwrap();
        symlink(&path, &link).unwrap();
        let mut store = SlashMru::new();
        store.load_from_path(&link);
        assert_eq!(store.last_used("", "model"), 42);
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let dir_path = directory.path().to_path_buf();
        let worker = std::thread::spawn(move || {
            for path in [fifo, dir_path] {
                let mut store = SlashMru::new();
                store.load_from_path(&path);
                assert!(store.loaded && !store.persist_enabled);
                store.touch("", "plan");
                assert!(store.take_persist_snapshot().is_none());
            }
            tx.send(()).unwrap();
        });
        rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn snapshot_write_leaves_legacy_temporary_path_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("slash-mru.json");
        let legacy = path.with_extension("json.tmp");
        fs::write(&legacy, b"unrelated writer data").unwrap();
        let snapshot = MruSnapshot { path: path.clone(), bytes: br#"{"by_command":{"find":1}}"#.to_vec() };
        assert!(snapshot.write());
        assert_eq!(fs::read(&path).unwrap(), snapshot.bytes);
        assert_eq!(fs::read(&legacy).unwrap(), b"unrelated writer data");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn snapshot_failure_cleans_only_its_owned_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("slash-mru.json");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("keep"), b"keep").unwrap();
        let legacy = path.with_extension("json.tmp");
        fs::write(&legacy, b"keep too").unwrap();
        assert!(!MruSnapshot { path: path.clone(), bytes: b"snapshot".to_vec() }.write());
        assert_eq!(fs::read(path.join("keep")).unwrap(), b"keep");
        assert_eq!(fs::read(legacy).unwrap(), b"keep too");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[test]
    fn concurrent_snapshot_writers_publish_complete_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("slash-mru.json");
        let snapshots: Vec<Vec<u8>> = (0..4).map(|index| {
            serde_json::to_vec(&MruFile { by_command: HashMap::from([(format!("command-{index}-{}", "x".repeat(index * 512)), index as u64 + 1)]) }).unwrap()
        }).collect();
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            let jobs: Vec<_> = snapshots.iter().map(|bytes| {
                let path = &path;
                let snapshots = &snapshots;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..12 {
                        assert!(MruSnapshot { path: path.clone(), bytes: bytes.clone() }.write());
                        let saved = fs::read(path).unwrap();
                        assert!(snapshots.contains(&saved), "mixed or partial snapshot");
                        assert!(serde_json::from_slice::<MruFile>(&saved).is_ok());
                    }
                })
            }).collect();
            for job in jobs { job.join().unwrap(); }
        });
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn touch_is_flat_by_command() {
        let mut mru = SlashMru::new_in_memory();
        mru.touch("p", "pager-headless");
        mru.touch("q", "quit");
        assert!(mru.last_used("anything", "pager-headless") > 0);
        assert!(mru.last_used("x", "quit") > 0);
        // Flat: prefix does not scope records.
        assert_eq!(mru.last_used("p", "quit"), mru.last_used("q", "quit"));
    }

    #[test]
    fn strips_leading_slash_on_command() {
        let mut mru = SlashMru::new_in_memory();
        mru.touch("m", "/model");
        assert!(mru.last_used("", "model") > 0);
        assert_eq!(mru.last_used("", "/model"), mru.last_used("", "model"));
    }

    #[test]
    fn recency_decays_stale_entries() {
        let now = 1_700_000_000_u64;
        let recent = SlashMru::recency_score(now - 60, now);
        let week_old = SlashMru::recency_score(now - 7 * 86_400, now);
        let month_old = SlashMru::recency_score(now - 30 * 86_400, now);
        assert!(recent > week_old);
        assert!(week_old > month_old);
        assert!(month_old > 0);
        assert_eq!(SlashMru::recency_score(0, now), 0);
    }

    #[test]
    fn in_memory_store_never_dirties_for_disk() {
        let mut mru = SlashMru::new_in_memory();
        mru.touch("p", "plan");
        assert!(!mru.dirty);
        // In-memory stores never produce a persist snapshot (no disk I/O).
        assert!(mru.take_persist_snapshot().is_none());
    }

    #[test]
    fn dirty_store_yields_one_snapshot_then_clears() {
        let mut mru = SlashMru::new(); // persist-enabled
        mru.loaded = true; // avoid disk read in test
        mru.touch("p", "plan");
        assert!(mru.dirty);
        assert!(mru.take_persist_snapshot().is_some());
        // Dirty flag cleared; no redundant second write.
        assert!(!mru.dirty);
        assert!(mru.take_persist_snapshot().is_none());
    }

    #[test]
    fn mark_dirty_requeues_after_failed_write() {
        // A snapshot was taken (dirty cleared) but the write could not be
        // handed off; mark_dirty re-queues it so the next call retries.
        let mut mru = SlashMru::new();
        mru.loaded = true;
        mru.touch("p", "plan");
        assert!(mru.take_persist_snapshot().is_some());
        assert!(mru.take_persist_snapshot().is_none()); // nothing to retry yet
        mru.mark_dirty();
        assert!(mru.take_persist_snapshot().is_some()); // retried
    }

    #[test]
    fn mark_dirty_noop_when_persistence_disabled() {
        let mut mru = SlashMru::new_in_memory();
        mru.mark_dirty();
        assert!(!mru.dirty);
        assert!(mru.take_persist_snapshot().is_none());
    }
}

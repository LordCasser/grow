//! Centralized unified log for cross-component session observability.
//!
//! Shell queues records via [`emit()`]. Pager forwards entries
//! over ACP (`grow/log` notifications); shell receives them in
//! [`ingest_client_entries()`] and queues them on their behalf. One bounded
//! process-local worker owns disk writes.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use config::grow_home;

/// Binary version stamped into every log entry. A launcher may set the exact
/// binary version once; direct library users use this crate's build version.
static VERSION: OnceLock<String> = OnceLock::new();

/// Register the binary version (e.g. shell's `CARGO_PKG_VERSION`).
/// Call once at startup; subsequent calls are no-ops.
pub fn set_version(ver: &str) {
    let _ = VERSION.set(ver.to_owned());
}

fn current_version() -> String {
    VERSION
        .get()
        .map_or(env!("CARGO_PKG_VERSION"), String::as_str)
        .to_owned()
}

pub const LOG_DIR: &str = "logs";
const LOG_FILE: &str = "unified.jsonl";
pub const MAX_SIZE: u64 = 5 * 1024 * 1024; // 5 MB
/// Maximum size of one complete JSONL record, including its trailing LF.
const MAX_RECORD_BYTES: usize = 64 * 1024;
const MAX_RECORD_JSON_BYTES: usize = MAX_RECORD_BYTES - 1;
const WRITER_QUEUE_RECORDS: usize = 64;
const FLUSH_DEADLINE: Duration = Duration::from_secs(2);

/// ACP method name for unified log notifications.
pub const LOG_METHOD: &str = "grow/log";

// ---------------------------------------------------------------------------
// Log entry types
// ---------------------------------------------------------------------------

/// Log level for a unified log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, Serialize, Deserialize)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// Component that produced a log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, Serialize, Deserialize)]
pub enum LogSource {
    #[strum(serialize = "shell")]
    #[serde(rename = "shell")]
    Shell,
    #[strum(serialize = "grow-pager")]
    #[serde(rename = "grow-pager")]
    GrowPager,
}

/// A single unified log entry, written as one JSONL line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogEntry {
    /// RFC 3339 timestamp (millisecond precision, UTC).
    pub ts: String,
    /// Component that produced the entry.
    pub src: LogSource,
    /// OS process id of the producer. Critical for cross-process trace
    /// reconstruction because shell and pager append to the same
    /// `unified.jsonl`, so multiple shell processes' lines interleave
    /// indistinguishably without it.
    ///
    pub pid: u32,
    /// Binary version (e.g. `"0.1.211"`). Stamped by [`set_version()`]
    /// at startup so stale zombie processes are identifiable in logs.
    pub ver: String,
    /// Log level.
    pub lvl: LogLevel,
    /// Session ID, if one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    /// Human-readable message.
    pub msg: String,
    /// Structured context fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx: Option<serde_json::Value>,
}

/// Wire format for the `grow/log` ACP notification params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogNotificationParams {
    /// Source component identifier.
    pub src: LogSource,
    pub entries: Vec<ClientLogEntry>,
}

/// Entry as sent by a client (no `src` field -- shell stamps it).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientLogEntry {
    pub ts: String,
    /// Client process id. Stamped by the client when the entry is
    /// created; preserved through ACP forwarding so the on-disk log
    /// reflects the originating process.
    ///
    pub pid: u32,
    /// Binary version of the originating client.
    pub ver: String,
    pub lvl: LogLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    pub msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// How often a writer re-checks that its handle still refers to the file at
/// `path`, and that the file is still under [`MAX_SIZE`].
///
/// Time-based rather than byte-based so a low-volume process detects a stale
/// handle just as fast as a chatty one — a process logging one line a minute
/// is precisely the one that would otherwise write into an unlinked inode for
/// hours without noticing.
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(2);

struct LogWriter {
    file: File,
    path: PathBuf,
    /// Identity of the inode this handle refers to, re-checked against the
    /// path on the maintenance cadence. `None` on platforms with no cheap
    /// stable file id, where only disappearance is detectable.
    identity: Option<FileIdentity>,
    last_maintenance: Instant,
    /// Set when `path` stopped resolving to our inode **and** reopening it
    /// failed. Writes are dropped while it is set.
    ///
    /// Continuing to append to the old descriptor would be the exact failure
    /// this module was changed to end: bytes land in a file no reader can
    /// find and no process will ever trim. Dropping them is not a loss —
    /// those bytes were already unreadable — and it avoids growing an
    /// invisible file on a disk that is quite possibly full, which is one of
    /// the few ways the reopen fails in the first place. Cleared by the next
    /// successful reopen, retried on the maintenance cadence.
    detached: bool,
}

/// `(dev, ino)` on Unix. Enough to notice that the path now resolves to a
/// different inode than the one we hold open.
type FileIdentity = (u64, u64);

static WRITER: LazyLock<Mutex<Option<LogWriter>>> = LazyLock::new(|| Mutex::new(open_writer()));
static WRITE_QUEUE: OnceLock<Option<SyncSender<WriterCommand>>> = OnceLock::new();
static DROPPED_RECORDS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
enum WriterCommand {
    Record(Vec<u8>),
    Flush(SyncSender<()>),
}

fn writer_queue() -> Option<&'static SyncSender<WriterCommand>> {
    WRITE_QUEUE
        .get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel(WRITER_QUEUE_RECORDS);
            match std::thread::Builder::new()
                .name("grow-unified-log".into())
                .spawn(move || writer_loop(receiver, &WRITER, &DROPPED_RECORDS))
            {
                Ok(_) => Some(sender),
                Err(_) => None,
            }
        })
        .as_ref()
}

fn dropped_record_line(count: u64) -> Option<Vec<u8>> {
    encode_entry_line(&LogEntry {
        ts: now_ts(),
        src: LogSource::Shell,
        pid: std::process::id(),
        ver: "unknown".into(),
        lvl: LogLevel::Warn,
        sid: None,
        msg: format!("unified log dropped {count} records while its writer queue was full"),
        ctx: Some(serde_json::json!({ "event": "records_dropped", "count": count })),
    })
}

fn append_queued_record(
    writer: &Mutex<Option<LogWriter>>,
    dropped_records: &AtomicU64,
    line: Vec<u8>,
) {
    let dropped = dropped_records.swap(0, Ordering::AcqRel);
    let mut lines = if dropped == 0 {
        Vec::new()
    } else {
        dropped_record_line(dropped).unwrap_or_default()
    };
    lines.extend_from_slice(&line);
    let Ok(mut guard) = writer.lock() else { return };
    let Some(writer) = guard.as_mut() else { return };
    if let Err(error) = writer.append_lines(&lines) {
        if dropped != 0 {
            dropped_records.fetch_add(dropped, Ordering::AcqRel);
        }
        tracing::warn!(%error, "unified log write failed");
    }
}

fn writer_loop(
    receiver: mpsc::Receiver<WriterCommand>,
    writer: &Mutex<Option<LogWriter>>,
    dropped_records: &AtomicU64,
) {
    while let Ok(command) = receiver.recv() {
        match command {
            WriterCommand::Record(line) => append_queued_record(writer, dropped_records, line),
            WriterCommand::Flush(reply) => {
                if let Ok(mut guard) = writer.lock()
                    && let Some(writer) = guard.as_mut()
                {
                    let _ = writer.file.flush();
                }
                let _ = reply.send(());
            }
        }
    }
}

fn enqueue_record(sender: &SyncSender<WriterCommand>, dropped: &AtomicU64, line: Vec<u8>) -> bool {
    if sender.try_send(WriterCommand::Record(line)).is_err() {
        dropped.fetch_add(1, Ordering::AcqRel);
        false
    } else {
        true
    }
}

/// Wait for the writer to process records accepted before this call, without
/// waiting indefinitely for a blocked OS write. Crossing the barrier does not
/// prove individual appends succeeded or that bytes were synchronized to disk.
pub(crate) fn flush_pending() -> bool {
    let Some(sender) = WRITE_QUEUE.get().and_then(Option::as_ref) else {
        return true;
    };
    flush_sender(sender, Instant::now() + FLUSH_DEADLINE)
}

fn flush_sender(sender: &SyncSender<WriterCommand>, deadline: Instant) -> bool {
    let (reply, done) = mpsc::sync_channel(0);
    let mut command = WriterCommand::Flush(reply);
    loop {
        match sender.try_send(command) {
            Ok(()) => break,
            Err(TrySendError::Full(pending)) => {
                command = pending;
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
    done.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .is_ok()
}

/// See [`redirect_to_temp_for_tests`].
static TEST_REDIRECT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Redirect all subsequent unified-log writes **and** snapshot reads to a
/// per-process file under the system temp directory, so test binaries stop
/// writing synthetic events into the developer's real
/// `~/.grow/logs/unified.jsonl` (those bursts inflate exactly the counters
/// an incident responder greps for). Runtime-activated rather than a cargo
/// feature: Bazel compiles production and test targets with one shared
/// feature set, so a feature gate would leak into production builds.
///
/// Idempotent and safe at any point: an already-open writer is re-pointed,
/// so an emit that precedes the redirect cannot pin the real path. Test
/// binaries install it pre-main via `#[ctor]`.
pub fn redirect_to_temp_for_tests() {
    let _ = flush_pending();
    TEST_REDIRECT.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut guard) = WRITER.lock() {
        *guard = open_writer();
    }
}

fn log_path() -> PathBuf {
    if TEST_REDIRECT.load(std::sync::atomic::Ordering::Relaxed) {
        return test_log_dir().join(LOG_FILE);
    }
    grow_home().join(LOG_DIR).join(LOG_FILE)
}

/// Owner-only (0o700), freshly-created directory for the test redirect.
///
/// The stream carries path metadata and credential tail fragments, and the
/// system temp dir is world-writable on Linux: a pre-planted directory or
/// symlink would let another local user read the file — or make the writer
/// and [`trim_file`] operate through a symlink onto a victim file. The
/// non-recursive `create` fails on any pre-existing path instead of
/// adopting it, and the nanos component makes the name unpredictable.
/// Panicking on failure is deliberate: this branch only runs in test
/// binaries, and silently falling back would reopen the hole via
/// `open_writer_at`'s `create_dir_all`.
fn test_log_dir() -> &'static PathBuf {
    static TEST_LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
    TEST_LOG_DIR.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "grow-unified-log-test-{}-{nanos}",
            std::process::id()
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&dir)
            .expect("create private unified-log test dir");
        dir
    })
}

pub fn file_size(path: &std::path::Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Identity of whatever file currently lives at `path`, or `None` if nothing
/// does. Compared against the identity captured at open time to detect that
/// our descriptor has been orphaned by a rename or an unlink.
#[cfg(unix)]
fn path_identity(path: &std::path::Path) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::metadata(path).ok()?;
    Some((meta.dev(), meta.ino()))
}

#[cfg(unix)]
fn open_file_identity(file: &File) -> std::io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn open_file_identity(file: &File) -> std::io::Result<FileIdentity> {
    file.metadata().map(|_| (0, 0))
}

/// Windows has no comparably cheap stable id from a path stat, so this
/// degrades to presence detection: a deleted log is still healed, a replaced
/// one is not.
#[cfg(not(unix))]
fn path_identity(path: &std::path::Path) -> Option<FileIdentity> {
    fs::metadata(path).ok().map(|_| (0, 0))
}

fn open_writer() -> Option<LogWriter> {
    open_writer_at(log_path())
}

/// Open (creating if needed) a writer for an explicit path.
///
/// Split from [`open_writer`] so a writer re-points at **its own** path when
/// healing a stale handle rather than re-resolving `$GROW_HOME` — which also
/// makes the healing path testable against a temp directory.
fn open_writer_at(path: PathBuf) -> Option<LogWriter> {
    if let Some(parent) = path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        tracing::warn!("[unified_log] failed to create log dir: {e}");
        return None;
    }

    if file_size(&path) >= MAX_SIZE {
        trim_file(&path);
    }

    // The descriptor must support in-place trimming while its inode lock is
    // held. Appends explicitly seek to EOF under that lock.
    match OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
    {
        Ok(file) => LogWriter::from_open_file(file, path),
        Err(e) => {
            tracing::warn!("[unified_log] failed to open log file: {e}");
            None
        }
    }
}

impl LogWriter {
    fn from_open_file(file: File, path: PathBuf) -> Option<Self> {
        let identity = Some(open_file_identity(&file).ok()?);
        Some(Self {
            identity,
            file,
            path,
            last_maintenance: Instant::now(),
            detached: false,
        })
    }

    /// Re-point at the live file if ours was replaced or removed, and trim if
    /// the file has grown past [`MAX_SIZE`].
    ///
    /// The size check reads the **real** file rather than a per-process byte
    /// counter. A counter only sees this process's own writes, so several
    /// writers sharing one log each believed they were far below the cap while
    /// the file sailed past it — orphaned writers observed at 8.5 MB against a
    /// 5 MB cap.
    ///
    /// Returns whether the handle is safe to write to: `false` once the file
    /// has been replaced or removed and reopening it did not work, so the
    /// caller drops the entry instead of appending it somewhere unreadable.
    fn maintain(&mut self) -> bool {
        if self.last_maintenance.elapsed() < MAINTENANCE_INTERVAL {
            return !self.detached;
        }
        self.last_maintenance = Instant::now();

        if path_identity(&self.path) != self.identity {
            let Some(reopened) = open_writer_at(self.path.clone()) else {
                // Warn on entering the state, not once per tick: a broken log
                // directory would otherwise flood the diagnostic output an
                // operator is trying to read.
                if !self.detached {
                    tracing::warn!(
                        path = %self.path.display(),
                        "[unified_log] log file replaced or removed and reopen failed; \
                         dropping entries until it can be reopened"
                    );
                    self.detached = true;
                }
                return false;
            };
            *self = reopened;
            return true;
        }

        // The path resolves to our inode again — either it always did, or a
        // transient stat failure cleared.
        self.detached = false;

        if file_size(&self.path) >= MAX_SIZE {
            let _ = self.file.flush();
            trim_file(&self.path);
        }
        true
    }

    fn append_lines(&mut self, lines: &[u8]) -> std::io::Result<()> {
        if !self.maintain() {
            return Ok(());
        }

        // trim_file rewrites and truncates this inode under the same lock.
        self.file.lock()?;
        let written = self.append_locked(lines);
        let unlocked = self.file.unlock();
        written.and(unlocked)
    }

    fn append_locked(&mut self, lines: &[u8]) -> std::io::Result<()> {
        let append_len = u64::try_from(lines.len()).map_err(std::io::Error::other)?;
        if self.file.metadata()?.len().saturating_add(append_len) > MAX_SIZE {
            if path_identity(&self.path) != self.identity {
                return Err(std::io::Error::other(
                    "unified log path no longer names the locked inode",
                ));
            }
            if !trim_open_file(&mut self.file)? {
                return Err(std::io::Error::other(
                    "unified log tail has no complete line boundary",
                ));
            }
            if self.file.metadata()?.len().saturating_add(append_len) > MAX_SIZE {
                return Err(std::io::Error::other(
                    "unified log record does not fit after trimming",
                ));
            }
        }
        self.file.seek(std::io::SeekFrom::End(0))?;
        self.file.write_all(lines)
    }
}

fn write_lines(lines: Vec<u8>) {
    if let Some(sender) = writer_queue() {
        enqueue_record(sender, &DROPPED_RECORDS, lines);
    } else {
        DROPPED_RECORDS.fetch_add(1, Ordering::Relaxed);
    }
}

struct BoundedJsonWriter {
    bytes: Vec<u8>,
    exceeded_limit: bool,
}

impl BoundedJsonWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            exceeded_limit: false,
        }
    }
}

impl Write for BoundedJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > MAX_RECORD_JSON_BYTES {
            self.exceeded_limit = true;
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "unified log record exceeds its byte limit",
            ));
        }

        self.bytes
            .try_reserve_exact(bytes.len())
            .map_err(std::io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn serialize_bounded<T: Serialize>(value: &T) -> Result<Vec<u8>, (serde_json::Error, bool)> {
    let mut writer = BoundedJsonWriter::new();
    match serde_json::to_writer(&mut writer, value) {
        Ok(()) => Ok(writer.bytes),
        Err(error) => Err((error, writer.exceeded_limit)),
    }
}

fn oversized_entry_diagnostic(entry: &LogEntry) -> LogEntry {
    LogEntry {
        ts: now_ts(),
        src: entry.src,
        pid: entry.pid,
        // Do not carry through client-controlled or otherwise large metadata
        // into the replacement record.
        ver: "unknown".to_owned(),
        lvl: entry.lvl,
        sid: None,
        msg: "unified log record omitted because it exceeded the 65536-byte limit".to_owned(),
        ctx: Some(serde_json::json!({
            "event": "record_omitted",
            "reason": "serialized_record_exceeds_limit",
            "max_bytes": MAX_RECORD_BYTES,
        })),
    }
}

fn encode_entry_line(entry: &LogEntry) -> Option<Vec<u8>> {
    let mut line = match serialize_bounded(entry) {
        Ok(line) => line,
        Err((_error, true)) => serialize_bounded(&oversized_entry_diagnostic(entry)).ok()?,
        Err((_error, false)) => return None,
    };
    line.push(b'\n');
    debug_assert!(line.len() <= MAX_RECORD_BYTES);
    Some(line)
}

fn write_entry(entry: &LogEntry) {
    let Some(line) = encode_entry_line(entry) else {
        return;
    };
    write_lines(line);
}

fn read_trim_window(reader: &mut (impl Read + Seek), len: u64) -> std::io::Result<Vec<u8>> {
    let start = (len / 2).max(len.saturating_sub(MAX_SIZE / 2));
    reader.seek(std::io::SeekFrom::Start(start))?;
    let mut data = Vec::new();
    reader.take(len - start).read_to_end(&mut data)?;
    Ok(data)
}

/// Drop the oldest lines, keeping roughly the last half up to 2.5 MiB,
/// **preserving the inode**.
///
/// Rewrites the retained tail at offset 0 and truncates to match. This must
/// not go through temp + rename: every other process holds a persistent
/// descriptor on this inode, and swapping a fresh file in underneath them
/// leaves each one writing to an unlinked inode that nothing can read and
/// nothing will ever trim. That failure was silent and unbounded — a single
/// developer machine accumulated roughly 26 MB across six orphaned inodes,
/// several of them past the 5 MB cap, while the visible log held only what
/// the most recent trimming process happened to write. The unified log was
/// therefore blind during the incident it exists to explain.
///
/// Truncating in place trades the rename's crash-atomicity for the far more
/// valuable property that concurrent writers keep working. A crash between
/// the write and the `set_len` leaves the tail followed by stale bytes; for a
/// line-delimited diagnostic log that costs at most a few garbled lines,
/// against losing every sibling's output indefinitely.
///
/// Appends use the same inode lock, so an entry written successfully during a
/// concurrent trim remains after its rewrite and truncate.
///
/// The whole read-modify-write is held under an exclusive advisory lock on
/// the log itself, because trimming in place is only safe for one process at
/// a time — see the comment in the body.
///
/// If the bounded tail contains no newline, trimming is skipped rather than
/// splitting a line. The append path then refuses growth beyond the limit.
pub fn trim_file(path: &std::path::Path) {
    // One trimmer at a time, across processes. Writers decide on the real
    // on-disk size, so when the log crosses the cap every process reaches
    // this function inside the same maintenance window. Two of them
    // interleaving a multi-megabyte rewrite at offset 0 would splice one
    // tail into the other; worse, a trimmer that reads while another is
    // mid-rewrite sees new-tail-over-old-head and computes its own tail from
    // that. Temp + rename was no safer — every process used the same
    // `unified.jsonl.tmp` — it was just rarer, because the old per-process
    // byte counter meant one process did essentially all the trimming.
    //
    // `try_lock`, not `lock`: a contended trim is one somebody else is
    // already doing, so there is nothing to wait for, and waiting would park
    // this process's writer mutex on a foreign process's I/O.
    //
    // A trimmer that decided to trim just before another one finished will
    // find a freshly halved file and halve it again. Losing another half of
    // an over-budget diagnostic log is a far cheaper outcome than interleaved
    // rewrites, so the size is deliberately not re-checked here: callers
    // trim on their own terms and the unit tests trim small files directly.
    let Ok(mut file) = OpenOptions::new().read(true).write(true).open(path) else {
        return;
    };
    if file.try_lock().is_err() {
        return;
    }

    if let Err(e) = trim_open_file(&mut file) {
        tracing::warn!(%e, "unified log trim failed");
    }
    // The lock is released when `file` drops.
}

/// Trim a read/write descriptor while its inode lock is already held.
/// `false` means the bounded tail has no complete line to retain.
fn trim_open_file(file: &mut File) -> std::io::Result<bool> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Ok(false);
    }
    let data = read_trim_window(file, metadata.len())?;
    // Discard the first partial line in the admitted tail window.
    let start = match data.iter().position(|&b| b == b'\n') {
        Some(pos) => pos + 1,
        None => return Ok(false),
    };
    let tail = &data[start..];

    // Rewind rather than truncate-on-open: the tail is laid down over the
    // head first, and only then is the file shortened, so the retained bytes
    // are never absent from disk.
    file.rewind()?;
    file.write_all(tail)?;
    file.set_len(tail.len() as u64)?;
    file.flush()?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return a new timestamp string in the unified log format.
fn now_ts() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Emit a log entry from shell itself.
pub fn emit(lvl: LogLevel, msg: &str, sid: Option<&str>, ctx: Option<serde_json::Value>) {
    let entry = LogEntry {
        ts: now_ts(),
        src: LogSource::Shell,
        pid: std::process::id(),
        ver: current_version(),
        lvl,
        sid: sid.map(Into::into),
        msg: msg.into(),
        ctx,
    };
    write_entry(&entry);
}

/// Ingest a batch of log entries from a pager client.
///
/// Called by the `grow/log` notification handler. Entries from
/// [`LogSource::Shell`] are rejected to prevent spoofing.
pub fn ingest_client_entries(src: LogSource, entries: &[ClientLogEntry]) {
    if matches!(src, LogSource::Shell) || entries.is_empty() {
        return;
    }
    // Enqueue each bounded record separately so a client batch cannot create
    // an unbounded process-local pending buffer.
    for client_entry in entries {
        let entry = LogEntry {
            ts: client_entry.ts.clone(),
            src,
            pid: client_entry.pid,
            ver: client_entry.ver.clone(),
            lvl: client_entry.lvl,
            sid: client_entry.sid.clone(),
            msg: client_entry.msg.clone(),
            ctx: client_entry.ctx.clone(),
        };
        if let Some(line) = encode_entry_line(&entry) {
            write_lines(line);
        }
    }
}

/// Convenience: emit an info-level entry from shell.
pub fn info(msg: &str, sid: Option<&str>, ctx: Option<serde_json::Value>) {
    emit(LogLevel::Info, msg, sid, ctx);
}

/// Convenience: emit a warn-level entry from shell.
pub fn warn(msg: &str, sid: Option<&str>, ctx: Option<serde_json::Value>) {
    emit(LogLevel::Warn, msg, sid, ctx);
}

/// Convenience: emit an error-level entry from shell.
pub fn error(msg: &str, sid: Option<&str>, ctx: Option<serde_json::Value>) {
    emit(LogLevel::Error, msg, sid, ctx);
}

/// Convenience: emit a debug-level entry from shell.
pub fn debug(msg: &str, sid: Option<&str>, ctx: Option<serde_json::Value>) {
    emit(LogLevel::Debug, msg, sid, ctx);
}

/// Read the current unified log file and return its contents.
///
/// Returns `None` if the log file doesn't exist or can't be read.
/// Used by local diagnostic tooling to capture the log state at a point in time.
pub fn snapshot_log() -> Option<Vec<u8>> {
    let path = log_path();
    // The barrier is bounded; a slow filesystem can leave this snapshot partial.
    let _ = flush_pending();
    match fs::read(&path) {
        Ok(data) if !data.is_empty() => Some(data),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pre-main, so no test in this binary can race the lazily-opened
    /// writer onto the developer's real `~/.grow/logs/unified.jsonl`.
    #[ctor::ctor]
    fn redirect_for_tests() {
        redirect_to_temp_for_tests();
    }

    /// The redirect must cover both the writer and the snapshot readers:
    /// an emit lands in a per-process temp file, never under `grow_home()`.
    #[test]
    fn redirect_routes_writes_and_snapshots_to_process_temp_file() {
        info(
            "unified-log redirect probe",
            Some("redirect-probe-sid"),
            None,
        );
        let snapshot = snapshot_log().expect("snapshot after emit");
        assert!(
            String::from_utf8_lossy(&snapshot).contains("unified-log redirect probe"),
            "snapshot must read the same redirected file the writer appended to"
        );
        assert!(
            log_path().starts_with(std::env::temp_dir()),
            "the shared file must live under the temp dir, not grow_home(): {}",
            log_path().display()
        );
    }

    #[test]
    fn blocked_writer_drops_overflow_without_blocking_producer_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unified.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let writer = Mutex::new(LogWriter::from_open_file(file, path.clone()));
        let dropped = AtomicU64::new(0);
        let (sender, receiver) = mpsc::sync_channel(1);
        let held = writer.lock().unwrap();

        std::thread::scope(|scope| {
            let worker = scope.spawn(|| writer_loop(receiver, &writer, &dropped));
            assert!(enqueue_record(&sender, &dropped, b"first\n".to_vec()));
            let deadline = Instant::now() + Duration::from_secs(1);
            let mut second = WriterCommand::Record(b"second\n".to_vec());
            loop {
                match sender.try_send(second) {
                    Ok(()) => break,
                    Err(TrySendError::Full(pending)) => {
                        assert!(
                            Instant::now() < deadline,
                            "worker did not take first record"
                        );
                        second = pending;
                        std::thread::yield_now();
                    }
                    Err(TrySendError::Disconnected(_)) => panic!("worker stopped"),
                }
            }
            let start = Instant::now();
            assert!(!enqueue_record(&sender, &dropped, b"third\n".to_vec()));
            assert!(start.elapsed() < Duration::from_millis(500));
            assert_eq!(dropped.load(Ordering::Acquire), 1);
            drop(held);
            drop(sender);
            worker.join().unwrap();
        });

        let output = fs::read_to_string(path).unwrap();
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3, "{output}");
        assert_eq!(lines[0], "first");
        assert_eq!(lines[2], "second");
        let diagnostic: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(diagnostic["ctx"]["event"], "records_dropped");
        assert_eq!(diagnostic["ctx"]["count"], 1);
    }

    #[test]
    fn flush_barrier_stops_waiting_for_a_full_queue() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        sender
            .try_send(WriterCommand::Record(b"held\n".to_vec()))
            .unwrap();
        let start = Instant::now();
        assert!(!flush_sender(&sender, start + Duration::from_millis(30)));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn burst_appends_enforce_capacity_between_maintenance_ticks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unified.jsonl");
        let mut writer = open_writer_at(path.clone()).unwrap();
        let payload = "x".repeat(32 * 1024);

        for index in 0..200 {
            let line = format!("{{\"index\":{index},\"payload\":\"{payload}\"}}\n");
            writer.append_lines(line.as_bytes()).unwrap();
            assert!(
                file_size(&path) <= MAX_SIZE,
                "append {index} exceeded quota"
            );
        }

        let lines = fs::read_to_string(&path).unwrap();
        assert!(lines.lines().count() < 200, "oldest lines should be pruned");
        for line in lines.lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
        assert!(lines.contains("\"index\":199"));
    }

    #[test]
    fn independent_writers_share_the_capacity_decision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unified.jsonl");
        let writers = [
            open_writer_at(path.clone()).unwrap(),
            open_writer_at(path.clone()).unwrap(),
        ];
        let payload = "x".repeat(32 * 1024);
        let final_record_barrier = std::sync::Barrier::new(2);

        std::thread::scope(|scope| {
            for (id, mut writer) in writers.into_iter().enumerate() {
                let path = path.clone();
                let payload = &payload;
                let final_record_barrier = &final_record_barrier;
                scope.spawn(move || {
                    for index in 0..120 {
                        if index == 119 {
                            final_record_barrier.wait();
                        }
                        let line = format!(
                            "{{\"writer\":{id},\"index\":{index},\"payload\":\"{payload}\"}}\n"
                        );
                        writer.append_lines(line.as_bytes()).unwrap();
                        assert!(file_size(&path) <= MAX_SIZE);
                    }
                });
            }
        });

        let lines = fs::read_to_string(path).unwrap();
        for line in lines.lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
        assert!(lines.contains("\"writer\":0,\"index\":119"));
        assert!(lines.contains("\"writer\":1,\"index\":119"));
    }

    #[test]
    fn untrimmable_tail_rejects_growth() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unified.jsonl");
        let mut writer = open_writer_at(path.clone()).unwrap();
        fs::write(&path, vec![b'x'; MAX_SIZE as usize]).unwrap();
        writer.last_maintenance = Instant::now();

        assert!(writer.append_lines(b"new\n").is_err());
        assert_eq!(file_size(&path), MAX_SIZE);
    }

    #[cfg(unix)]
    #[test]
    fn replacement_path_does_not_trim_the_wrong_inode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unified.jsonl");
        let mut writer = open_writer_at(path.clone()).unwrap();
        fs::write(&path, vec![b'x'; MAX_SIZE as usize]).unwrap();
        let replacement = dir.path().join("replacement.jsonl");
        fs::write(&replacement, b"replacement\n").unwrap();
        fs::rename(&replacement, &path).unwrap();
        writer.last_maintenance = Instant::now();

        assert!(writer.append_lines(b"new\n").is_err());
        assert_eq!(writer.file.metadata().unwrap().len(), MAX_SIZE);
        assert_eq!(fs::read(path).unwrap(), b"replacement\n");
    }

    #[test]
    fn log_entry_serializes_required_identity() {
        let entry = LogEntry {
            ts: "2025-07-14T10:30:00.123Z".into(),
            src: LogSource::Shell,
            pid: 4242,
            ver: "1.0.0".into(),
            lvl: LogLevel::Info,
            sid: None,
            msg: "test".into(),
            ctx: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("sid"));
        assert!(!json.contains("ctx"));
        assert!(json.contains("\"pid\":4242"));
        assert!(json.contains("\"ver\":\"1.0.0\""));
        assert!(json.contains("\"src\":\"shell\""));
    }

    #[test]
    fn log_entry_serializes_full() {
        let entry = LogEntry {
            ts: "2025-07-14T10:30:00.123Z".into(),
            src: LogSource::GrowPager,
            pid: 4242,
            ver: "0.1.211".into(),
            lvl: LogLevel::Warn,
            sid: Some("abc123".into()),
            msg: "connection lost".into(),
            ctx: Some(serde_json::json!({"retry": 3})),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"sid\":\"abc123\""));
        assert!(json.contains("\"retry\":3"));
        assert!(json.contains("\"pid\":4242"));
        assert!(json.contains("\"ver\":\"0.1.211\""));
    }

    #[test]
    fn bounded_encoder_preserves_a_record_at_the_exact_limit() {
        let mut entry = LogEntry {
            ts: "2025-07-14T10:30:00.123Z".into(),
            src: LogSource::Shell,
            pid: 4242,
            ver: "1.0.0".into(),
            lvl: LogLevel::Info,
            sid: None,
            msg: String::new(),
            ctx: None,
        };
        let empty_len = serde_json::to_vec(&entry).unwrap().len();
        entry.msg = "x".repeat(MAX_RECORD_JSON_BYTES - empty_len);

        let expected_json = serde_json::to_vec(&entry).unwrap();
        assert_eq!(expected_json.len(), MAX_RECORD_JSON_BYTES);
        let line = encode_entry_line(&entry).expect("record exactly at budget");
        assert_eq!(line.len(), MAX_RECORD_BYTES);
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(&line[..line.len() - 1], expected_json);
    }

    #[test]
    fn oversized_record_becomes_one_bounded_valid_diagnostic_line() {
        let entry = LogEntry {
            ts: "client supplied timestamp that must not be retained".into(),
            src: LogSource::GrowPager,
            pid: 4242,
            ver: "x".repeat(MAX_RECORD_BYTES * 2),
            lvl: LogLevel::Warn,
            sid: Some("session-that-must-not-be-retained".into()),
            msg: "oversized message marker".repeat(MAX_RECORD_BYTES * 2),
            ctx: Some(serde_json::json!({"private_payload": "must not be retained"})),
        };

        let line = encode_entry_line(&entry).expect("oversized entry diagnostic");
        assert!(line.len() <= MAX_RECORD_BYTES);
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(line.iter().filter(|&&byte| byte == b'\n').count(), 1);

        let decoded: LogEntry = serde_json::from_slice(&line[..line.len() - 1]).unwrap();
        assert_eq!(decoded.src, LogSource::GrowPager);
        assert_eq!(decoded.pid, 4242);
        assert_eq!(decoded.lvl, LogLevel::Warn);
        assert_eq!(decoded.ver, "unknown");
        assert!(decoded.sid.is_none());
        assert!(decoded.msg.contains("exceeded the 65536-byte limit"));
        let context = decoded.ctx.unwrap();
        assert_eq!(context["event"], "record_omitted");
        assert_eq!(context["reason"], "serialized_record_exceeds_limit");
        assert_eq!(context["max_bytes"], MAX_RECORD_BYTES);
        assert!(
            !line
                .windows(b"oversized message marker".len())
                .any(|window| window == b"oversized message marker")
        );
    }

    #[test]
    fn client_entry_round_trip() {
        let wire = r#"{"ts":"2025-07-14T10:30:00.123Z","pid":42,"ver":"1.0.0","lvl":"info","msg":"hello"}"#;
        let entry: ClientLogEntry = serde_json::from_str(wire).unwrap();
        assert_eq!(entry.msg, "hello");
        assert!(entry.sid.is_none());
        assert!(entry.ctx.is_none());
    }

    /// The reason this incident was undiagnosable: `trim_file` used to
    /// temp+rename, which swaps the inode out from under every other process
    /// holding an `O_APPEND` descriptor. Their writes then land in an
    /// unlinked inode that no reader can ever see.
    #[cfg(unix)]
    #[test]
    fn trim_file_preserves_the_inode_so_open_handles_survive() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut content = String::new();
        for i in 0..10 {
            content.push_str(&format!("line {i}\n"));
        }
        fs::write(&path, &content).unwrap();
        let before = fs::metadata(&path).unwrap().ino();

        trim_file(&path);

        assert_eq!(
            fs::metadata(&path).unwrap().ino(),
            before,
            "trim must rewrite in place; replacing the inode strands every \
             sibling process's open log handle",
        );
    }

    /// End-to-end version of the same property: a writer that opened the file
    /// *before* a trim must still be able to append to the file a reader sees
    /// afterwards.
    #[cfg(unix)]
    #[test]
    fn writes_from_a_handle_opened_before_trim_remain_visible() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut content = String::new();
        for i in 0..10 {
            content.push_str(&format!("line {i}\n"));
        }
        fs::write(&path, &content).unwrap();

        // A sibling process's writer, opened before the trim happens.
        let mut sibling = OpenOptions::new().append(true).open(&path).unwrap();

        trim_file(&path);

        sibling.write_all(b"after trim\n").unwrap();
        sibling.flush().unwrap();

        let visible = fs::read_to_string(&path).unwrap();
        assert!(
            visible.contains("after trim"),
            "a handle opened before the trim must keep writing to the live \
             file, got: {visible:?}",
        );
    }

    /// `maintain` heals a writer whose file was replaced or deleted behind its
    /// back — an older binary still doing temp+rename, an external `rm`, or a
    /// `$TMPDIR` reaper.
    #[cfg(unix)]
    #[test]
    fn writer_initialization_tracks_open_handle_after_path_changes() {
        for replace in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("log.jsonl");
            fs::write(&path, b"old\n").unwrap();
            let old_identity = path_identity(&path);
            let file = OpenOptions::new().append(true).open(&path).unwrap();
            if replace {
                let replacement = dir.path().join("replacement");
                fs::write(&replacement, b"new\n").unwrap();
                fs::rename(replacement, &path).unwrap();
            } else {
                fs::remove_file(&path).unwrap();
            }
            let mut writer = LogWriter::from_open_file(file, path.clone()).unwrap();
            assert_eq!(writer.identity, old_identity);
            assert_ne!(writer.identity, path_identity(&path));
            writer.last_maintenance = Instant::now() - MAINTENANCE_INTERVAL;
            assert!(writer.maintain());
            writer.append_lines(b"visible\n").unwrap();
            writer.file.flush().unwrap();
            assert!(fs::read_to_string(&path).unwrap().contains("visible"));
            assert_eq!(writer.identity, path_identity(&path));
        }
    }

    #[cfg(unix)]
    #[test]
    fn maintain_reopens_after_the_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        fs::write(&path, b"original\n").unwrap();

        let mut writer = LogWriter {
            file: OpenOptions::new().append(true).open(&path).unwrap(),
            identity: path_identity(&path),
            path: path.clone(),
            // Force the maintenance cadence to fire on the next call.
            last_maintenance: Instant::now() - MAINTENANCE_INTERVAL,
            detached: false,
        };
        let original_identity = writer.identity;

        // Simulate an older binary's rename-based trim from another process.
        let replacement = dir.path().join("replacement.jsonl");
        fs::write(&replacement, b"replaced\n").unwrap();
        fs::rename(&replacement, &path).unwrap();
        assert_ne!(
            path_identity(&path),
            original_identity,
            "test setup: the path must now resolve to a new inode",
        );

        assert!(
            writer.maintain(),
            "a writer that successfully re-pointed at the live file is writable",
        );
        writer.append_lines(b"after replacement\n").unwrap();
        writer.file.flush().unwrap();

        let visible = fs::read_to_string(&path).unwrap();
        assert!(
            visible.contains("after replacement"),
            "a writer whose file was replaced must re-point at the live file \
             instead of writing into the orphaned inode, got: {visible:?}",
        );
        assert_eq!(
            writer.identity,
            path_identity(&path),
            "the healed writer must track the new inode",
        );
    }

    /// The same healing path for outright deletion, which is how a
    /// `$TMPDIR` reaper (or a stray `rm`) silences a long-lived agent.
    #[cfg(unix)]
    #[test]
    fn maintain_reopens_after_the_file_is_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        fs::write(&path, b"original\n").unwrap();

        let mut writer = LogWriter {
            file: OpenOptions::new().append(true).open(&path).unwrap(),
            identity: path_identity(&path),
            path: path.clone(),
            last_maintenance: Instant::now() - MAINTENANCE_INTERVAL,
            detached: false,
        };

        fs::remove_file(&path).unwrap();

        assert!(
            writer.maintain(),
            "a writer that successfully re-pointed at the live file is writable",
        );
        writer.append_lines(b"after deletion\n").unwrap();
        writer.file.flush().unwrap();

        let visible = fs::read_to_string(&path).expect("log must be recreated");
        assert!(
            visible.contains("after deletion"),
            "a deleted log must be recreated rather than written into the \
             void, got: {visible:?}",
        );
    }

    /// The trim decision must read the real file, not a per-process counter:
    /// with several writers sharing one log, each one's own byte count stays
    /// far below the cap while the file sails past it.
    #[cfg(unix)]
    #[test]
    fn maintain_trims_growth_this_process_did_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");

        let mut writer = LogWriter {
            file: OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap(),
            identity: path_identity(&path),
            path: path.clone(),
            last_maintenance: Instant::now() - MAINTENANCE_INTERVAL,
            detached: false,
        };

        // Someone else fills the log past the cap; this writer wrote nothing.
        let line = "x".repeat(1023);
        let mut bulk = String::new();
        while bulk.len() as u64 <= MAX_SIZE {
            bulk.push_str(&line);
            bulk.push('\n');
        }
        fs::write(&path, &bulk).unwrap();
        // Rewriting the path in place keeps the inode, so the handle is fine.
        assert_eq!(path_identity(&path), writer.identity);
        assert!(file_size(&path) >= MAX_SIZE);

        assert!(
            writer.maintain(),
            "trimming does not detach the writer; its handle stays usable",
        );

        assert!(
            file_size(&path) < MAX_SIZE,
            "a writer must trim on observed file size, not on its own \
             write counter; size is now {}",
            file_size(&path),
        );
    }

    /// Trimming in place is only safe for one process at a time, and deciding
    /// on the real file size means every writer reaches [`trim_file`] in the
    /// same maintenance window once the log crosses the cap. A trimmer that
    /// finds the log already being rewritten must leave it alone rather than
    /// interleave a second rewrite at offset 0.
    #[test]
    fn trim_file_yields_to_a_concurrent_trimmer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut content = String::new();
        for i in 0..10 {
            content.push_str(&format!("line {i}\n"));
        }
        fs::write(&path, &content).unwrap();

        // Stand in for another process midway through its own trim.
        let holder = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        holder.lock().expect("test setup: exclusive lock");

        trim_file(&path);

        // Release before reading: the lock is mandatory on Windows.
        drop(holder);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            content,
            "a contended trim must be skipped, not interleaved with the \
             rewrite already in progress",
        );

        // And it is only deferred, not lost: the next trim proceeds.
        trim_file(&path);
        assert!(fs::read_to_string(&path).unwrap().len() < content.len());
    }

    #[test]
    fn append_waits_for_trim_and_survives_truncate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        fs::write(&path, b"old\ntail\n").unwrap();
        let writer = LogWriter::from_open_file(
            OpenOptions::new().append(true).open(&path).unwrap(),
            path.clone(),
        )
        .unwrap();
        let mut trimmer = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        trimmer.lock().unwrap();

        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let append = std::thread::spawn(move || {
            let mut writer = writer;
            started_tx.send(()).unwrap();
            finished_tx.send(writer.append_lines(b"new\n")).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            finished_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "append must wait while the trim owns the inode"
        );

        trimmer.seek(std::io::SeekFrom::Start(0)).unwrap();
        trimmer.write_all(b"tail\n").unwrap();
        trimmer.set_len(5).unwrap();
        drop(trimmer);
        finished_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        append.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"tail\nnew\n");
    }

    #[test]
    fn trim_yields_while_writer_owns_inode_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        fs::write(&path, b"old\ntail\n").unwrap();
        let writer = LogWriter::from_open_file(
            OpenOptions::new().append(true).open(&path).unwrap(),
            path.clone(),
        )
        .unwrap();
        writer.file.lock().unwrap();
        trim_file(&path);
        writer.file.unlock().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"old\ntail\n");
    }

    /// The reopen can itself fail — a log directory replaced by a file, a full
    /// disk, exhausted descriptors. Appending to the old handle anyway would
    /// reproduce the orphaning this module was changed to end, so the writer
    /// drops entries until it can reach the real file again.
    #[cfg(unix)]
    #[test]
    fn maintain_stops_writing_when_the_file_cannot_be_reopened() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("logs");
        fs::create_dir_all(&log_dir).unwrap();
        let path = log_dir.join("test.jsonl");
        fs::write(&path, b"original\n").unwrap();

        let mut writer = LogWriter {
            file: OpenOptions::new().append(true).open(&path).unwrap(),
            identity: path_identity(&path),
            path: path.clone(),
            last_maintenance: Instant::now() - MAINTENANCE_INTERVAL,
            detached: false,
        };

        // Wipe the log's directory and put a regular file in its place, so
        // the path no longer resolves to our inode *and* cannot be reopened.
        fs::remove_dir_all(&log_dir).unwrap();
        fs::write(&log_dir, b"not a directory\n").unwrap();

        assert!(
            !writer.maintain(),
            "a writer that cannot reach the real log must report itself \
             unwritable instead of appending into the orphaned inode",
        );
        assert!(
            !writer.maintain(),
            "and must stay unwritable between maintenance ticks, not just on \
             the tick that discovered the problem",
        );

        // Healing: once the directory is back, the next tick reopens.
        fs::remove_file(&log_dir).unwrap();
        writer.last_maintenance = Instant::now() - MAINTENANCE_INTERVAL;
        assert!(
            writer.maintain(),
            "the writer must recover as soon as the path is usable again",
        );

        writer.append_lines(b"after recovery\n").unwrap();
        writer.file.flush().unwrap();
        let visible = fs::read_to_string(&path).unwrap();
        assert!(
            visible.contains("after recovery"),
            "the recovered writer must be attached to the visible file, \
             got: {visible:?}",
        );
    }

    #[test]
    fn trim_window_reads_only_the_tail_budget() {
        struct Counted {
            cursor: std::io::Cursor<Vec<u8>>,
            bytes: usize,
        }
        impl Read for Counted {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let count = self.cursor.read(out)?;
                self.bytes += count;
                Ok(count)
            }
        }
        impl Seek for Counted {
            fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
                self.cursor.seek(pos)
            }
        }
        let len = MAX_SIZE * 3;
        let mut reader = Counted {
            cursor: std::io::Cursor::new(vec![b'x'; len as usize + 20]),
            bytes: 0,
        };
        let tail = read_trim_window(&mut reader, len).unwrap();
        assert_eq!(tail.len() as u64, MAX_SIZE / 2);
        assert_eq!(reader.bytes as u64, MAX_SIZE / 2);
        assert_eq!(reader.cursor.position(), len);
    }

    #[test]
    fn trim_large_file_retains_recent_lines_within_budget() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.jsonl");
        let mut file = File::create(&path).unwrap();
        file.set_len(MAX_SIZE * 3).unwrap();
        file.seek(std::io::SeekFrom::End(0)).unwrap();
        file.write_all(b"\nrecent-1\nrecent-2\n").unwrap();
        let identity = path_identity(&path);
        trim_file(&path);
        assert_eq!(fs::read(&path).unwrap(), b"recent-1\nrecent-2\n");
        assert_eq!(path_identity(&path), identity);
    }

    #[test]
    fn trim_file_keeps_recent_half() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let mut content = String::new();
        for i in 0..10 {
            content.push_str(&format!("line {i}\n"));
        }
        fs::write(&path, &content).unwrap();
        trim_file(&path);
        let result = fs::read_to_string(&path).unwrap();
        // Should keep roughly the second half, starting at a line boundary.
        assert!(!result.contains("line 0"));
        assert!(result.contains("line 9"));
        assert!(result.len() < content.len());
        // Every line should be complete (no partial lines).
        for line in result.lines() {
            assert!(line.starts_with("line "));
        }
    }

    #[test]
    fn trim_file_no_newline_in_second_half_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.jsonl");
        let content = "single-line-no-newline";
        fs::write(&path, content).unwrap();
        trim_file(&path);
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn trim_file_missing_file_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.jsonl");
        trim_file(&path);
        assert!(!path.exists());
    }

    #[test]
    fn ingest_rejects_shell_src() {
        ingest_client_entries(
            LogSource::Shell,
            &[ClientLogEntry {
                ts: "2025-01-01T00:00:00.000Z".into(),
                pid: 4242,
                ver: "1.0.0".into(),
                lvl: LogLevel::Info,
                sid: None,
                msg: "sneaky".into(),
                ctx: None,
            }],
        );
    }

    #[test]
    fn unknown_src_rejected_at_deserialization() {
        for bad in &[
            r#"{"src":"evil","entries":[]}"#,
            r#"{"src":"","entries":[]}"#,
            r#"{"src":"GROW-PAGER","entries":[]}"#,
        ] {
            assert!(serde_json::from_str::<LogNotificationParams>(bad).is_err());
        }
    }

    #[test]
    fn client_log_identity_is_required() {
        let missing_pid = r#"{"ts":"2025-01-01T00:00:00Z","ver":"1.0.0","lvl":"info","msg":"x"}"#;
        let missing_version = r#"{"ts":"2025-01-01T00:00:00Z","pid":42,"lvl":"info","msg":"x"}"#;
        assert!(serde_json::from_str::<ClientLogEntry>(missing_pid).is_err());
        assert!(serde_json::from_str::<ClientLogEntry>(missing_version).is_err());
    }

    #[test]
    fn notification_params_round_trip() {
        let params = LogNotificationParams {
            src: LogSource::GrowPager,
            entries: vec![
                ClientLogEntry {
                    ts: "2025-07-14T10:30:00.123Z".into(),
                    pid: 1234,
                    ver: "1.0.0".into(),
                    lvl: LogLevel::Info,
                    sid: Some("s1".into()),
                    msg: "first".into(),
                    ctx: None,
                },
                ClientLogEntry {
                    ts: "2025-07-14T10:30:00.456Z".into(),
                    pid: 1234,
                    ver: "0.1.211".into(),
                    lvl: LogLevel::Error,
                    sid: None,
                    msg: "second".into(),
                    ctx: Some(serde_json::json!({"code": 42})),
                },
            ],
        };
        let json = serde_json::to_string(&params).unwrap();
        let parsed: LogNotificationParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].msg, "first");
        assert_eq!(parsed.entries[1].msg, "second");
    }
}

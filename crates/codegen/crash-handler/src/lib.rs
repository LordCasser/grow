//! Cross-platform crash handler with startup crash detection.
//!
//! - **Unix**: SIGBUS/SIGSEGV/SIGABRT via `sigaction(2)`. SIGABRT capture
//!   means `panic = "abort"` builds (every shipped release) leave a crash
//!   report when a Rust panic aborts the process.
//! - **Windows**: access violations via `SetUnhandledExceptionFilter`.
//!   SIGABRT capture is Unix-only — `abort()` on Windows does not route
//!   through the unhandled-exception filter.
//!
//! # Usage
//!
//! Call [`check_previous_crash`] until no more captures are available, then
//! [`install`] early in `main()`, before any async runtime or thread spawning.
//! Each installation owns a unique raw file; recovery never reads live captures.
//!
//! ```rust,no_run
//! use std::path::PathBuf;
//!
//! let crash_dir = PathBuf::from("/home/user/.myapp/crash");
//!
//! loop {
//!     match crash_handler::check_previous_crash(&crash_dir) {
//!         Ok(Some(report)) => {
//!             eprintln!("Application previously crashed: {}", report.signal_name);
//!             eprintln!("  Report: {}", report.report_path.display());
//!         }
//!         Ok(None) => break,
//!         Err(err) => {
//!             eprintln!("warning: previous crash preserved but unrecovered: {err}");
//!             return;
//!         }
//!     }
//! }
//!
//! crash_handler::install(crash_handler::CrashHandlerConfig {
//!     app_version: "0.1.0".to_string(),
//!     crash_dir: crash_dir.clone(),
//! });
//! ```

pub mod format;
mod handler;
pub mod symbolicate;
pub mod terminal;

use std::path::{Path, PathBuf};

pub use symbolicate::ResolvedFrame;

const MAX_HISTORY: usize = 5;

/// Configuration for the crash handler.
pub struct CrashHandlerConfig {
    /// Application version string (e.g. "0.1.169-alpha.2").
    pub app_version: String,
    /// Directory where crash dumps are written.
    /// Created if it does not exist.
    pub crash_dir: PathBuf,
}

/// Information about a crash from the previous session.
#[derive(Debug)]
pub struct CrashReport {
    /// Human-readable signal name (e.g. "SIGBUS (Bus error)").
    pub signal_name: &'static str,
    /// The `si_code` from `siginfo_t`.
    pub si_code: i32,
    /// The faulting memory address.
    pub faulting_address: u64,
    /// Unix timestamp of the crash.
    pub timestamp: u64,
    /// Application version at crash time.
    pub app_version: String,
    /// Raw backtrace frames. Captured addresses belong to the crashing
    /// process's address map, which recovery cannot verify against its own
    /// images, so no symbol attribution is made.
    pub backtrace: Vec<ResolvedFrame>,
    /// Path to the saved human-readable crash report.
    pub report_path: PathBuf,
}

/// Install the crash handler for SIGBUS, SIGSEGV, and SIGABRT (Unix; on
/// Windows only access violations are captured).
///
/// Must be called early in `main()`, before any async runtime or thread
/// spawning. Creates `crash_dir` if it does not exist.
///
/// Returns `true` if the handler was installed successfully.
/// On unsupported platforms, this is a no-op that returns `false`.
pub fn install(config: CrashHandlerConfig) -> bool {
    handler::install(&config.crash_dir, &config.app_version)
}

/// Install a minimal SIGSEGV/SIGBUS/SIGABRT handler that only restores the
/// terminal.
///
/// On Unix, saves the current termios state, allocates an alternate signal
/// stack, and registers a handler that writes terminal restore escape
/// sequences to stderr, restores termios, then re-raises with default
/// disposition (preserving core dumps).
///
/// On Windows, registers an unhandled-exception filter that writes restore
/// sequences; no termios equivalent.
///
/// No-op on unsupported platforms.
///
/// No crash reporting (no file I/O, no stack walking). If [`install`] is
/// called later, it replaces these handlers with full crash-reporting
/// variants.
pub fn install_terminal_restore_only() {
    handler::install_terminal_restore_only()
}

/// Upgrade SIGSEGV/SIGBUS/SIGABRT handlers to include terminal escape code
/// restoration. Call when TUI modes are enabled.
pub fn enable_terminal_escape_restore() {
    handler::enable_terminal_escape_restore()
}

/// Downgrade SIGSEGV/SIGBUS/SIGABRT handlers to termios-only restoration.
/// Call when TUI modes are disabled.
pub fn disable_terminal_escape_restore() {
    handler::disable_terminal_escape_restore()
}

/// Recover at most one inactive capture, including existing `last-crash.bin`.
///
/// Recovery first claims a dedicated lock for selection, publication, retention
/// and consumption, then claims each raw handle before reading it. Live owners
/// and competing recovery are skipped without reading or deleting their raw.
/// Empty inactive captures are cleaned up. Call until `Ok(None)` to drain all
/// currently claimable captures; `None` can also mean another recovery owns the
/// transaction lock.
///
/// Both reports use exclusive sibling temporary files, private Unix permissions,
/// atomic replacement and supported file/directory durability barriers. A raw is
/// consumed only after both publications succeed. Read, parse or publication
/// failures preserve the raw bytes and return `Err`; history failure may leave
/// the primary already published. Cleanup failure also returns `Err` without
/// rolling back either durable report.
pub fn check_previous_crash(crash_dir: &Path) -> std::io::Result<Option<CrashReport>> {
    use std::fs::OpenOptions;

    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let recovery_lock = match options.open(crash_dir.join(".recovery.lock")) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    if !try_claim(&recovery_lock)? {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    for entry in std::fs::read_dir(crash_dir)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .and_then(capture_identity)
            .is_some()
        {
            candidates.push(entry.path());
        }
    }
    candidates.sort();
    for path in candidates {
        // Normal sharing on Windows lets us open live files and ask their OS
        // lock. PermissionDenied is an actual error, never a liveness signal.
        let mut raw = match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        if !try_claim(&raw)? {
            continue;
        }
        use std::io::Read;
        let mut data = Vec::new();
        raw.read_to_end(&mut data)?;
        if data.is_empty() {
            std::fs::remove_file(&path)?;
            continue;
        }
        // Selection admitted only names with an exact capture identity.
        let identity = capture_identity(path.file_name().unwrap().to_str().unwrap()).unwrap();
        // `raw` and `recovery_lock` keep their claims through publication and
        // consumption, including all error paths, then release on drop.
        return recover_claimed(crash_dir, &path, &data, identity).map(Some);
    }
    Ok(None)
}

fn try_claim(file: &std::fs::File) -> std::io::Result<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(std::fs::TryLockError::Error(err)) => Err(err),
    }
}

fn capture_identity(name: &str) -> Option<&str> {
    if name == "last-crash.bin" {
        return Some("legacy");
    }
    let identity = name.strip_prefix("capture-")?.strip_suffix(".bin")?;
    (identity.len() == 16 && identity.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then_some(identity)
}

fn recover_claimed(
    crash_dir: &Path,
    crash_file: &Path,
    data: &[u8],
    identity: &str,
) -> std::io::Result<CrashReport> {
    let blob = format::CrashBlob::parse(data).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "nonempty crash evidence at {} failed to parse; raw bytes preserved",
                crash_file.display()
            ),
        )
    })?;

    let frames = symbolicate::resolve_frames(&blob);
    let report_text = symbolicate::format_report(&blob, &frames);
    let report_path = crash_dir.join("last-crash-report.txt");

    // Publish both promised reports before consuming the raw evidence. Each
    // publication is individually atomic; failure of either propagates and
    // leaves the raw bytes untouched for retry.
    publish_owner_only(&report_path, report_text.as_bytes())?;
    publish_history_report(crash_dir, &report_text, blob.timestamp, identity)?;

    // Post-commit cleanup: both reports are durable at this point. If removal
    // fails we surface it, but the published reports stay.
    std::fs::remove_file(crash_file)?;

    Ok(CrashReport {
        signal_name: symbolicate::signal_name(blob.signal),
        si_code: blob.si_code,
        faulting_address: blob.si_addr,
        timestamp: blob.timestamp,
        app_version: blob.app_version,
        backtrace: frames,
        report_path,
    })
}

/// Atomically publish `contents` at `path` with owner-only permissions where
/// the platform supports them.
///
/// Writes an exclusive sibling temporary file (owner-only on Unix, so crash
/// report content is never world-readable even for an instant), syncs the
/// file, atomically replaces the destination, then syncs the containing
/// directory where the OS exposes that barrier (Unix directory `fsync`).
/// Windows has no portable equivalent of opening a directory for sync; the
/// file-level `sync_all` is the supported barrier there.
fn publish_owner_only(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Set the 0600 mode on creation so the temp file never exists
        // world-readable for an instant before content is written.
        builder.permissions(std::fs::Permissions::from_mode(0o600));
    }
    let mut tmp = builder.tempfile_in(parent)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    sync_dir(parent)
}

/// Durability barrier for a directory entry, where supported. Supported
/// barrier errors propagate (the caller must keep the raw evidence); only
/// platforms without a directory-open interface treat this as unavailable.
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        // Windows cannot open a directory like a Unix dirfd for fsync; do not
        // attempt it. The file sync above is the platform's supported barrier.
        let _ = dir;
        Ok(())
    }
}

/// Publish the crash report into `crash_dir/history/` and prune archives
/// beyond [`MAX_HISTORY`]. The new archive is published before pruning.
fn publish_history_report(
    crash_dir: &Path,
    report_text: &str,
    timestamp: u64,
    identity: &str,
) -> std::io::Result<()> {
    let history_dir = crash_dir.join("history");
    std::fs::create_dir_all(&history_dir)?;
    // The history directory entry inside crash_dir is new (or must be visible
    // before the raw blob is consumed). Syncing only history_dir would not
    // persist that entry in crash_dir, so after a power loss the promised
    // history archive could be missing while the raw evidence is gone.
    // crash_dir itself pre-exists (it holds last-crash.bin); no general
    // ancestor walk is needed. A failure here propagates before publication,
    // preserving the raw bytes.
    sync_dir(crash_dir)?;

    let filename = format!("crash-{timestamp}-{identity}.txt");
    let published_path = history_dir.join(&filename);
    publish_owner_only(&published_path, report_text.as_bytes())?;

    // Prune old reports beyond MAX_HISTORY. The just-published archive must
    // never be a prune victim — even when its raw timestamp is older than the
    // existing records — otherwise recovery would consume the raw blob while
    // deleting one of the two promised publications. Prune only among the
    // other records, oldest first. Pruning failure must not fail the
    // already-durable publication.
    if let Ok(mut entries) = std::fs::read_dir(&history_dir) {
        let mut files: Vec<PathBuf> = entries
            .by_ref()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "txt") && *p != published_path)
            .collect();
        files.sort();
        if files.len() + 1 > MAX_HISTORY {
            let excess = files.len() + 1 - MAX_HISTORY;
            for old in &files[..excess] {
                let _ = std::fs::remove_file(old);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_previous_crash_missing_file_is_ok_none() {
        let dir = std::env::temp_dir().join("crash-handler-test-nonexistent");
        assert!(
            check_previous_crash(&dir)
                .expect("missing must be Ok")
                .is_none()
        );
    }

    #[test]
    fn check_previous_crash_empty_file_is_ok_none() {
        let dir = unique_tmp_dir("empty-none");
        let crash_file = dir.join("last-crash.bin");
        std::fs::write(&crash_file, b"").expect("seed empty");
        assert!(
            check_previous_crash(&dir)
                .expect("empty must be Ok")
                .is_none()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A minimal valid GCRX blob (header + one frame).
    fn valid_blob_bytes(signal: u8, timestamp: u64) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&format::MAGIC);
        b.push(format::VERSION);
        b.push(signal);
        b.extend_from_slice(&1i32.to_le_bytes()); // si_code
        b.extend_from_slice(&0xdead_beefu64.to_le_bytes()); // si_addr
        b.extend_from_slice(&4242u32.to_le_bytes()); // pid
        b.extend_from_slice(&timestamp.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // n_frames
        let mut version_field = [0u8; format::VERSION_STRING_LEN];
        version_field[..b"0.0.0-tst".len()].copy_from_slice(b"0.0.0-tst");
        b.extend_from_slice(&version_field);
        b.extend_from_slice(&0x1000u64.to_le_bytes()); // one frame address
        b
    }

    #[test]
    fn check_previous_crash_malformed_blob_is_err_and_preserves_raw() {
        let dir = unique_tmp_dir("malformed-err");
        let crash_file = dir.join("last-crash.bin");
        std::fs::write(&crash_file, b"not a crash blob").expect("seed malformed");
        let err = check_previous_crash(&dir).expect_err("malformed must Err");
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::InvalidData,
            "nonempty malformed evidence must be InvalidData"
        );
        assert_eq!(
            std::fs::read(&crash_file).expect("raw preserved"),
            b"not a crash blob",
            "raw bytes must be preserved on parse failure"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_previous_crash_success_publishes_and_consumes() {
        let dir = unique_tmp_dir("success");
        let crash_file = dir.join("last-crash.bin");
        let raw = valid_blob_bytes(11, 1_700_000_000_123);
        std::fs::write(&crash_file, &raw).expect("seed raw");
        let report = check_previous_crash(&dir)
            .expect("valid blob must be Ok")
            .expect("and Some");
        assert_eq!(report.timestamp, 1_700_000_000_123);
        assert_eq!(report.app_version, "0.0.0-tst");
        assert!(
            report.report_path.is_file(),
            "returned report path must exist"
        );
        assert!(
            dir.join("history/crash-1700000000123-legacy.txt").is_file(),
            "history archive must exist"
        );
        assert!(!crash_file.exists(), "raw must be consumed after commit");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_previous_crash_primary_blocked_preserves_raw_for_retry() {
        let dir = unique_tmp_dir("primary-blocked");
        let crash_file = dir.join("last-crash.bin");
        let raw = valid_blob_bytes(7, 1_700_000_000_456);
        std::fs::write(&crash_file, &raw).expect("seed raw");
        // Block the primary report path with a directory: atomic replace fails.
        std::fs::create_dir(dir.join("last-crash-report.txt")).expect("block primary");
        check_previous_crash(&dir).expect_err("blocked primary must Err");
        assert_eq!(
            std::fs::read(&crash_file).expect("raw preserved"),
            raw,
            "raw must be byte-identical after blocked publication"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_previous_crash_history_blocked_keeps_primary_and_retry_succeeds() {
        let dir = unique_tmp_dir("history-blocked");
        let crash_file = dir.join("last-crash.bin");
        let raw = valid_blob_bytes(6, 1_700_000_000_789);
        std::fs::write(&crash_file, &raw).expect("seed raw");
        // Block the history directory with a regular file.
        std::fs::write(dir.join("history"), b"not a dir").expect("block history");
        check_previous_crash(&dir).expect_err("blocked history must Err");
        assert!(
            crash_file.is_file(),
            "raw must survive history publication failure"
        );
        // Primary may already be published; this is not a two-path transaction.
        assert!(dir.join("last-crash-report.txt").is_file());

        // Unblock and retry: same crash identity, raw consumed.
        std::fs::remove_file(dir.join("history")).expect("unblock");
        let report = check_previous_crash(&dir)
            .expect("retry after unblock must Ok")
            .expect("and Some");
        assert_eq!(report.timestamp, 1_700_000_000_789, "same crash identity");
        assert!(report.signal_name.contains("SIGABRT"));
        assert!(
            dir.join("history/crash-1700000000789-legacy.txt").is_file(),
            "history published on retry"
        );
        assert!(!crash_file.exists(), "raw consumed after successful retry");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovery_lock_contention_preserves_all_raws() {
        let dir = tempfile::tempdir().unwrap();
        let raw = valid_blob_bytes(6, 1_700_000_000);
        let path = dir.path().join("capture-AAAAAAAAAAAAAAAA.bin");
        std::fs::write(&path, &raw).unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.path().join(".recovery.lock"))
            .unwrap();
        lock.try_lock().unwrap();
        assert!(check_previous_crash(dir.path()).unwrap().is_none());
        assert_eq!(std::fs::read(&path).unwrap(), raw);
        drop(lock);
        assert!(check_previous_crash(dir.path()).unwrap().is_some());
        assert!(!path.exists());
    }

    #[test]
    fn live_malformed_and_empty_captures_are_not_read_or_removed() {
        let dir = tempfile::tempdir().unwrap();
        let malformed = dir.path().join("capture-AAAAAAAAAAAAAAAA.bin");
        let empty = dir.path().join("capture-BBBBBBBBBBBBBBBB.bin");
        std::fs::write(&malformed, b"live bytes").unwrap();
        std::fs::write(&empty, b"").unwrap();
        let open = |path: &Path| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .unwrap()
        };
        let a = open(&malformed);
        let b = open(&empty);
        a.try_lock().unwrap();
        b.try_lock().unwrap();
        assert!(check_previous_crash(dir.path()).unwrap().is_none());
        assert!(malformed.exists());
        assert!(empty.exists());
        drop(a);
        drop(b);
        assert_eq!(std::fs::read(&malformed).unwrap(), b"live bytes");
        assert_eq!(
            check_previous_crash(dir.path()).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&malformed).unwrap(), b"live bytes");
        std::fs::remove_file(malformed).unwrap();
        assert!(check_previous_crash(dir.path()).unwrap().is_none());
        assert!(!empty.exists());
    }

    #[test]
    fn recovery_ignores_foreign_and_unpublished_names() {
        let dir = tempfile::tempdir().unwrap();
        let names = [
            "unrelated.bin",
            "capture-short.bin",
            ".capture-AAAAAAAAAAAAAAAA.bin",
            "capture-AAAAAAAAAAAAAAAA.bin.extra",
            "capture-AAAAAAAAAAAAAAA_.bin",
        ];
        for name in names {
            std::fs::write(dir.path().join(name), b"unowned bytes").unwrap();
        }
        assert!(check_previous_crash(dir.path()).unwrap().is_none());
        for name in names {
            assert_eq!(
                std::fs::read(dir.path().join(name)).unwrap(),
                b"unowned bytes"
            );
        }
    }

    #[test]
    fn new_capture_publication_failures_preserve_bytes_and_identity_for_retry() {
        for blocker in ["last-crash-report.txt", "history"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("capture-AAAAAAAAAAAAAAAA.bin");
            let raw = valid_blob_bytes(6, 1_700_000_000);
            std::fs::write(&path, &raw).unwrap();
            let blocked_path = dir.path().join(blocker);
            if blocker == "history" {
                std::fs::write(&blocked_path, b"blocked").unwrap();
            } else {
                std::fs::create_dir(&blocked_path).unwrap();
            }
            check_previous_crash(dir.path()).unwrap_err();
            assert_eq!(std::fs::read(&path).unwrap(), raw);
            if blocker == "history" {
                std::fs::remove_file(&blocked_path).unwrap();
            } else {
                std::fs::remove_dir(&blocked_path).unwrap();
            }
            assert!(check_previous_crash(dir.path()).unwrap().is_some());
            assert!(!path.exists());
            assert!(
                dir.path()
                    .join("history/crash-1700000000-AAAAAAAAAAAAAAAA.txt")
                    .is_file()
            );
            assert_eq!(
                std::fs::read_dir(dir.path().join("history"))
                    .unwrap()
                    .count(),
                1
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_capture_is_an_error_and_preserves_raw_for_retry() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("permission failure fixture requires an ordinary user; skipped for root");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capture-AAAAAAAAAAAAAAAA.bin");
        let raw = valid_blob_bytes(6, 1_700_000_000);
        std::fs::write(&path, &raw).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let result = check_previous_crash(dir.path());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(std::fs::read(&path).unwrap(), raw);
        assert!(check_previous_crash(dir.path()).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn history_directory_barrier_failure_preserves_raw_bytes_for_retry() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("permission failure fixture requires an ordinary user; skipped for root");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capture-AAAAAAAAAAAAAAAA.bin");
        let raw = valid_blob_bytes(6, 1_700_000_000);
        std::fs::write(&path, &raw).unwrap();
        let history = dir.path().join("history");
        std::fs::create_dir(&history).unwrap();
        // Write+search permits report creation/rename, while opening history
        // for the directory sync is denied. This exercises the actual barrier.
        std::fs::set_permissions(&history, std::fs::Permissions::from_mode(0o300)).unwrap();
        let result = check_previous_crash(dir.path());
        std::fs::set_permissions(&history, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(dir.path().join("last-crash-report.txt").is_file());
        assert!(
            history
                .join("crash-1700000000-AAAAAAAAAAAAAAAA.txt")
                .is_file(),
            "history rename succeeded before directory barrier"
        );
        assert_eq!(std::fs::read(&path).unwrap(), raw);
        assert!(check_previous_crash(dir.path()).unwrap().is_some());
        assert!(!path.exists());
    }

    fn unique_tmp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "crash-handler-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("create tmp dir");
        dir
    }

    #[cfg(unix)]
    #[test]
    fn publish_owner_only_creates_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = unique_tmp_dir("create-0600");
        let path = dir.join("report.txt");
        publish_owner_only(&path, b"secret").expect("publish");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "published file must be owner-only");
        assert_eq!(std::fs::read(&path).expect("read"), b"secret");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn publish_owner_only_replaces_0644_destination_with_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = unique_tmp_dir("tighten-0644");
        let path = dir.join("report.txt");
        std::fs::write(&path, b"old").expect("seed");
        let mut perms = std::fs::metadata(&path).expect("meta").permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(&path, perms).expect("set 0644");

        publish_owner_only(&path, b"new-secret").expect("republish");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "replacement must be owner-only");
        assert_eq!(std::fs::read(&path).expect("read"), b"new-secret");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn check_previous_crash_published_reports_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = unique_tmp_dir("private-modes");
        std::fs::write(
            dir.join("last-crash.bin"),
            valid_blob_bytes(11, 1_700_000_001_000),
        )
        .expect("seed raw");
        check_previous_crash(&dir).expect("Ok").expect("Some");
        for p in [
            dir.join("last-crash-report.txt"),
            dir.join("history/crash-1700000001000-legacy.txt"),
        ] {
            let mode = std::fs::metadata(&p).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{} must be owner-only", p.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_previous_crash_older_raw_keeps_published_history_and_prunes_oldest() {
        let dir = unique_tmp_dir("older-raw-prune");
        let history_dir = dir.join("history");
        std::fs::create_dir_all(&history_dir).expect("make history");
        // Preseed exactly MAX_HISTORY records, all newer than the raw blob.
        for i in 0..MAX_HISTORY {
            let ts = 1_800_000_000 + i as u64; // all genuinely newer than the raw blob
            std::fs::write(history_dir.join(format!("crash-{ts}.txt")), b"prior").expect("seed");
        }
        let crash_file = dir.join("last-crash.bin");
        std::fs::write(&crash_file, valid_blob_bytes(11, 1_700_000_000)).expect("seed raw");

        let report = check_previous_crash(&dir)
            .expect("recovery must succeed")
            .expect("and produce a report");

        // Raw consumed, primary exists, just-published history survives prune.
        assert!(!crash_file.exists(), "raw must be consumed");
        assert!(
            report.report_path.is_file(),
            "primary report must be published"
        );
        let published = history_dir.join("crash-1700000000-legacy.txt");
        assert!(
            published.is_file(),
            "just-published history must never be pruned despite older timestamp"
        );

        // Total stays at MAX_HISTORY; the oldest preseeded record was pruned.
        let names: Vec<String> = std::fs::read_dir(&history_dir)
            .expect("read history")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), MAX_HISTORY, "history must stay at the bound");
        assert!(
            !history_dir.join("crash-1800000000.txt").is_file(),
            "oldest preseeded record must be pruned"
        );
        assert!(history_dir.join("crash-1800000001.txt").is_file());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&published)
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "surviving report stays owner-only");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

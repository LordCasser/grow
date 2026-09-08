//! Corruption self-heal for the session-search SQLite cache: classify an
//! unusable file, then quarantine it under a lock so a fresh empty database
//! can be recreated. The index layer drives the retry.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::ErrorCode;
use sqlite_journal::JournalMode;

static HEAL_LOCK: Mutex<()> = Mutex::new(());

/// Bumped when quarantine or recreation changes the cache namespace, so callers can tell
/// they are now looking at a different incarnation of the on-disk index.
static CACHE_EPOCH: AtomicU64 = AtomicU64::new(0);

pub(super) fn current_epoch() -> u64 {
    CACHE_EPOCH.load(Ordering::Acquire)
}

/// A snapshot of the [`CACHE_EPOCH`], used to detect whether the cache was
/// quarantined and recreated between two points in this process.
pub(super) struct CacheEpoch(u64);

impl CacheEpoch {
    pub(super) fn now() -> Self {
        Self(CACHE_EPOCH.load(Ordering::Acquire))
    }

    pub(super) fn changed(&self) -> bool {
        CACHE_EPOCH.load(Ordering::Acquire) != self.0
    }
}

pub(super) fn is_unusable_db_error(error: &rusqlite::Error) -> bool {
    match error {
        rusqlite::Error::SqliteFailure(err, msg) => {
            if matches!(
                err.code,
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase
            ) {
                return true;
            }
            msg.as_deref().is_some_and(message_indicates_unusable_db)
        }
        _ => false,
    }
}

/// Specific phrases only: a bare "malformed" would also match a bad FTS query.
fn message_indicates_unusable_db(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("disk image is malformed")
        || lower.contains("database schema is malformed")
        || lower.contains("database is corrupt")
        || lower.contains("file is not a database")
        || lower.contains("file is encrypted or is not a database")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

pub(super) fn quarantine_db_files(db_path: &Path) -> std::io::Result<Option<PathBuf>> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let corrupt_suffix = format!(".corrupt.{ts}");
    let mut changed = false;
    let result = (|| {
        for suffix in ["-wal", "-shm", "-journal"] {
            let side = with_suffix(db_path, suffix);
            let dest = with_suffix(&side, &corrupt_suffix);
            match std::fs::rename(&side, &dest) {
                Ok(()) => changed = true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        let main_quarantine = with_suffix(db_path, &corrupt_suffix);
        match std::fs::rename(db_path, &main_quarantine) {
            Ok(()) => {
                changed = true;
                Ok(Some(main_quarantine))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    })();
    // A later rename failure does not undo earlier namespace changes.
    if changed {
        CACHE_EPOCH.fetch_add(1, Ordering::Release);
    }
    result
}

pub(super) fn heal_unusable(
    db_path: &Path,
    cause: &rusqlite::Error,
    reprobe: impl FnOnce(&Path) -> Result<bool, rusqlite::Error>,
    recreate: impl FnOnce(&Path) -> Result<(), rusqlite::Error>,
) -> bool {
    use fs2::FileExt as _;

    let _guard = HEAL_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let effective = JournalMode::for_db_path(db_path).effective_db_path(db_path);

    let lock_path = with_suffix(&effective, ".lock");
    let _lock_file = match std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) => {
            tracing::debug!(error = %e, path = %lock_path.display(), "could not open heal lock; skipping quarantine");
            return false;
        }
    };
    if let Err(e) = _lock_file.lock_exclusive() {
        tracing::debug!(error = %e, "could not acquire cross-process heal lock; skipping quarantine");
        return false;
    }

    match reprobe(&effective) {
        Ok(true) => return true,
        Ok(false) => {}
        Err(e) if is_unusable_db_error(&e) => {}
        Err(_) => return false,
    }

    let quarantine = match quarantine_db_files(&effective) {
        Ok(quarantine) => quarantine,
        Err(error) => {
            tracing::warn!(%error, path = %effective.display(), "session search quarantine failed; skipping recreation");
            return false;
        }
    };
    if let Err(error) = recreate(&effective) {
        tracing::warn!(%error, path = %effective.display(), ?quarantine, "session search recreation failed after isolation");
        return false;
    }

    CACHE_EPOCH.fetch_add(1, Ordering::Release);
    tracing::warn!(
        db_path = %effective.display(),
        ?quarantine,
        error = %cause,
        "session search index recreated after confirmed unusability"
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn has_corrupt_sibling(dir: &Path) -> bool {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"))
    }

    #[test]
    fn quarantine_failure_does_not_recreate() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("x".repeat(240));
        std::fs::write(&db, b"corrupt fixture").unwrap();
        let wal = with_suffix(&db, "-wal");
        std::fs::write(&wal, b"wal fixture").unwrap();
        let recreated = std::cell::Cell::new(false);
        heal_unusable(
            &db,
            &rusqlite::Error::QueryReturnedNoRows,
            |_| Ok(false),
            |_| {
                recreated.set(true);
                Ok(())
            },
        );
        assert!(
            !recreated.get(),
            "failed isolation must stop before recreate"
        );
        assert_eq!(std::fs::read(db).unwrap(), b"corrupt fixture");
        assert_eq!(std::fs::read(wal).unwrap(), b"wal fixture");
    }

    #[test]
    fn quarantine_moves_main_and_sidecars() {
        let _guard = HEAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("session_search.sqlite");
        std::fs::write(&db, b"main").unwrap();
        std::fs::write(tmp.path().join("session_search.sqlite-wal"), b"wal").unwrap();
        std::fs::write(tmp.path().join("session_search.sqlite-shm"), b"shm").unwrap();

        let moved = quarantine_db_files(&db)
            .unwrap()
            .expect("main db should be quarantined");

        assert!(!db.exists());
        assert!(moved.exists());
        assert!(!tmp.path().join("session_search.sqlite-wal").exists());
        assert!(!tmp.path().join("session_search.sqlite-shm").exists());
    }

    #[test]
    fn partial_quarantine_failure_invalidates_without_moving_main() {
        let _guard = HEAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("x".repeat(220));
        std::fs::write(&db, b"main").unwrap();
        let wal = with_suffix(&db, "-wal");
        let journal = with_suffix(&db, "-journal");
        std::fs::write(&wal, b"wal").unwrap();
        std::fs::write(&journal, b"journal").unwrap();
        let epoch = CacheEpoch::now();
        assert!(quarantine_db_files(&db).is_err());
        assert!(epoch.changed());
        assert!(!wal.exists());
        assert!(has_corrupt_sibling(tmp.path()));
        assert_eq!(std::fs::read(db).unwrap(), b"main");
        assert_eq!(std::fs::read(journal).unwrap(), b"journal");
    }

    #[test]
    fn recreation_failure_denies_reopen_and_keeps_quarantine() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("failed-recreate.sqlite");
        std::fs::write(&db, b"bad").unwrap();
        let reopen = heal_unusable(
            &db,
            &rusqlite::Error::InvalidQuery,
            |_| Ok(false),
            |_| Err(rusqlite::Error::InvalidQuery),
        );
        assert!(!reopen);
        assert!(!db.exists());
        assert!(has_corrupt_sibling(tmp.path()));
    }

    #[test]
    fn absent_files_allow_recreation() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("absent.sqlite");
        let recreated = std::cell::Cell::new(false);
        assert!(heal_unusable(
            &db,
            &rusqlite::Error::InvalidQuery,
            |_| Ok(false),
            |_| {
                recreated.set(true);
                Ok(())
            }
        ));
        assert!(recreated.get());
        assert!(!has_corrupt_sibling(tmp.path()));
    }

    fn quarantined_after(reprobe: impl FnOnce(&Path) -> Result<bool, rusqlite::Error>) -> bool {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("session_search.sqlite");
        std::fs::write(&db, b"looks-like-a-db").unwrap();
        heal_unusable(&db, &rusqlite::Error::QueryReturnedNoRows, reprobe, |_| {
            Ok(())
        });
        has_corrupt_sibling(tmp.path())
    }

    #[test]
    fn heal_quarantines_only_on_confirmed_corruption() {
        assert!(!quarantined_after(|_| Ok(true)), "healthy: no quarantine");
        assert!(
            !quarantined_after(|_| Err(rusqlite::Error::QueryReturnedNoRows)),
            "transient failure: no quarantine"
        );
        assert!(quarantined_after(|_| Ok(false)), "corrupt: quarantine");
    }

    #[test]
    fn classifier_ignores_bad_query_but_catches_corruption() {
        assert!(message_indicates_unusable_db(
            "database disk image is malformed"
        ));
        assert!(message_indicates_unusable_db("file is not a database"));
        assert!(!message_indicates_unusable_db("malformed MATCH expression"));
    }
}

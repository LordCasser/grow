//! Corruption self-heal for the session-search SQLite cache: classify an
//! unusable file, then quarantine it under a lock so a fresh empty database
//! can be recreated. The index layer drives the retry.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::ErrorCode;
use sqlite_journal::JournalMode;

static HEAL_LOCK: Mutex<()> = Mutex::new(());

/// Per-index epochs are process-local invalidation tokens. A repair in one
/// storage root must not invalidate a query or bootstrap for another root.
static CACHE_EPOCH: LazyLock<Mutex<HashMap<PathBuf, u64>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn cache_identity(db_path: &Path) -> PathBuf {
    let Some(file_name) = db_path.file_name() else {
        return db_path.to_path_buf();
    };
    let Some(parent) = db_path.parent() else {
        return db_path.to_path_buf();
    };
    let absolute_parent = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(parent))
            .unwrap_or_else(|_| parent.to_path_buf())
    };
    let canonical_parent = dunce::canonicalize(&absolute_parent).unwrap_or(absolute_parent);
    canonical_parent.join(file_name)
}

fn epoch_at(identity: &Path) -> u64 {
    *CACHE_EPOCH
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(identity)
        .unwrap_or(&0)
}

fn advance_epoch(db_path: &Path) {
    let identity = cache_identity(db_path);
    let mut epochs = CACHE_EPOCH
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let epoch = epochs.entry(identity).or_default();
    *epoch = epoch.saturating_add(1);
}

pub(super) fn current_epoch(db_path: &Path) -> u64 {
    epoch_at(&cache_identity(db_path))
}

/// A snapshot of the [`CACHE_EPOCH`], used to detect whether the cache was
/// quarantined and recreated between two points in this process.
pub(super) struct CacheEpoch {
    identity: PathBuf,
    epoch: u64,
}

impl CacheEpoch {
    pub(super) fn now(db_path: &Path) -> Self {
        let identity = cache_identity(db_path);
        let epoch = epoch_at(&identity);
        Self { identity, epoch }
    }

    pub(super) fn changed(&self) -> bool {
        epoch_at(&self.identity) != self.epoch
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
        advance_epoch(db_path);
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

    advance_epoch(&effective);
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
        let epoch = CacheEpoch::now(&db);
        assert!(quarantine_db_files(&db).is_err());
        assert!(epoch.changed());
        assert!(!wal.exists());
        assert!(has_corrupt_sibling(tmp.path()));
        assert_eq!(std::fs::read(db).unwrap(), b"main");
        assert_eq!(std::fs::read(journal).unwrap(), b"journal");
    }

    #[test]
    fn cache_epochs_follow_one_index_through_quarantine_and_recreation() {
        let root_a = TempDir::new().unwrap();
        let root_b = TempDir::new().unwrap();
        let db_a = root_a.path().join("session_search.sqlite");
        let db_b = root_b.path().join("session_search.sqlite");
        std::fs::write(&db_a, b"root a").unwrap();
        std::fs::write(&db_b, b"root b").unwrap();

        let a_before = CacheEpoch::now(&db_a);
        let b_before = CacheEpoch::now(&db_b);
        quarantine_db_files(&db_a).unwrap();
        assert!(a_before.changed(), "root A's quarantine must invalidate A");
        assert!(
            !b_before.changed(),
            "root A's quarantine must not invalidate B"
        );

        let a_after_quarantine = CacheEpoch::now(&db_a);
        let b_after_quarantine = CacheEpoch::now(&db_b);
        let recreated = heal_unusable(
            &db_a,
            &rusqlite::Error::InvalidQuery,
            |_| Ok(false),
            |path| {
                std::fs::write(path, b"rebuilt root a").map_err(|_| rusqlite::Error::InvalidQuery)
            },
        );
        assert!(recreated);
        assert!(
            a_after_quarantine.changed(),
            "recreation must advance A again"
        );
        assert!(
            !b_after_quarantine.changed(),
            "A's recreation must leave B stable"
        );
        assert!(
            !CacheEpoch::now(&db_a).changed(),
            "a fresh root A epoch starts from its recreated index"
        );
    }

    #[test]
    fn concurrent_root_repairs_advance_independent_epochs() {
        let root_a = TempDir::new().unwrap();
        let root_b = TempDir::new().unwrap();
        let db_a = root_a.path().join("session_search.sqlite");
        let db_b = root_b.path().join("session_search.sqlite");
        std::fs::write(&db_a, b"root a").unwrap();
        std::fs::write(&db_b, b"root b").unwrap();
        let a_before = CacheEpoch::now(&db_a);
        let b_before = CacheEpoch::now(&db_b);

        let repairs = [db_a.clone(), db_b.clone()].map(|path| {
            std::thread::spawn(move || {
                heal_unusable(
                    &path,
                    &rusqlite::Error::InvalidQuery,
                    |_| Ok(false),
                    |path| {
                        std::fs::write(path, b"rebuilt").map_err(|_| rusqlite::Error::InvalidQuery)
                    },
                )
            })
        });
        for repair in repairs {
            assert!(repair.join().unwrap());
        }

        assert!(a_before.changed());
        assert!(b_before.changed());
        assert_eq!(current_epoch(&db_a), 2);
        assert_eq!(current_epoch(&db_b), 2);
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

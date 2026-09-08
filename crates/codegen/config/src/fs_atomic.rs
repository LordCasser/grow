//! Atomic file writes shared by local state and identifier caches (e.g. the
//! diagnostics agent id).

use std::path::Path;

/// Atomic temp + rename so a torn write can't leave a half-written file. The temp
/// name is unique per writer (pid + counter) and `create_new`, so concurrent
/// writers don't collide. `mode` (unix only) is applied at temp-file creation, so
/// the final file never exists with looser permissions.
pub fn write_atomically(
    final_path: &Path,
    contents: &str,
    mode: Option<u32>,
) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static WRITE_NONCE: AtomicU64 = AtomicU64::new(0);

    let dir = final_path.parent().unwrap_or_else(|| Path::new("."));
    let name = final_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    let nonce = WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!("{name}.{}.{nonce}.tmp", std::process::id()));
    write_using_temp(final_path, &tmp, contents, mode)
}

fn write_using_temp(
    final_path: &Path,
    tmp: &Path,
    contents: &str,
    mode: Option<u32>,
) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    // Creation failure gives us no ownership of this path. In particular,
    // never remove another writer's file after an AlreadyExists error.
    let mut file = options.open(tmp)?;
    let written = file.write_all(contents.as_bytes());
    drop(file);
    let result = written.and_then(|()| std::fs::rename(tmp, final_path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collision_preserves_unowned_temp_and_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state");
        let tmp = dir.path().join("colliding.tmp");
        std::fs::write(&path, "previous").unwrap();
        std::fs::write(&tmp, "owned elsewhere").unwrap();
        let error = write_using_temp(&path, &tmp, "next", None).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous");
        assert_eq!(std::fs::read_to_string(&tmp).unwrap(), "owned elsewhere");
    }

    #[test]
    fn publication_failure_removes_only_created_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("previous"), "keep").unwrap();
        let tmp = dir.path().join("owned.tmp");
        assert!(write_using_temp(&path, &tmp, "next", None).is_err());
        assert!(!tmp.exists());
        assert_eq!(std::fs::read_to_string(path.join("previous")).unwrap(), "keep");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn public_writer_replaces_complete_state_with_requested_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state");
        std::fs::write(&path, "long previous state").unwrap();
        write_atomically(&path, "new", Some(0o600)).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}

use std::{
    future::Future,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    time::Duration,
};

use fs2::FileExt;
use same_file::Handle;
use tokio::{fs, time::sleep};

use crate::computer::types::{AsyncFileSystem, ComputerError};

/// Creates a local FS access which allows writing and reading from the local files
pub struct LocalFs;

// Keep the window short: these retries absorb brief Windows editor/indexer/AV
// races without hiding persistent locks, ACL failures, or sandbox denials.
const WRITE_RETRY_DELAYS: &[Duration] = &[
    Duration::from_millis(25),
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
];
#[cfg(any(windows, test))]
const WINDOWS_ERROR_SHARING_VIOLATION: i32 = 32;
#[cfg(any(windows, test))]
const WINDOWS_ERROR_LOCK_VIOLATION: i32 = 33;

/// Check if an IO error is a permission denial (EACCES or EPERM),
/// which indicates a sandbox violation.
fn is_permission_error(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::PermissionDenied)
}

#[cfg(any(windows, test))]
fn is_windows_transient_write_lock_raw_os_error(raw_os_error: Option<i32>) -> bool {
    matches!(
        raw_os_error,
        Some(WINDOWS_ERROR_SHARING_VIOLATION | WINDOWS_ERROR_LOCK_VIOLATION)
    )
}

fn is_transient_write_lock_error(e: &io::Error) -> bool {
    #[cfg(windows)]
    {
        is_windows_transient_write_lock_raw_os_error(e.raw_os_error())
    }

    #[cfg(not(windows))]
    {
        let _ = e;
        false
    }
}

#[cfg(test)]
fn is_test_transient_write_lock_error(e: &io::Error) -> bool {
    is_windows_transient_write_lock_raw_os_error(e.raw_os_error())
}

async fn write_file_with_transient_lock_retries(path: &Path, data: &[u8]) -> io::Result<()> {
    write_file_with_retry_hooks(
        || fs::write(path, data),
        |delay| sleep(delay),
        |retry_count| {
            tracing::debug!(
                path = %path.display(),
                retry_count,
                "file write succeeded after transient lock retries"
            );
        },
        |error, retry_count, delay| {
            tracing::debug!(
                path = %path.display(),
                error = %error,
                retry_count,
                delay_ms = delay.as_millis(),
                "file write hit transient lock; retrying"
            );
        },
        |error, retry_count| {
            tracing::debug!(
                path = %path.display(),
                error = %error,
                retry_count,
                "file write exhausted transient lock retries"
            );
        },
        is_transient_write_lock_error,
    )
    .await
}

fn changed_since_read(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("file changed since read: {}", path.display()),
    )
}

fn lock_edit_file(file: &std::fs::File, path: &Path, wait: Duration) -> io::Result<()> {
    let deadline = std::time::Instant::now() + wait;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || is_transient_write_lock_error(&error) =>
            {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("timed out waiting to edit {}", path.display()),
                    ));
                }
                std::thread::sleep(remaining.min(Duration::from_millis(25)));
            }
            Err(error) => return Err(error),
        }
    }
}

fn replace_file_if_unchanged_blocking(path: &Path, expected: &[u8], data: &[u8]) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    lock_edit_file(&file, path, Duration::from_secs(5))?;
    let source = Handle::from_file(file.try_clone()?)?;
    if Handle::from_path(path)? != source {
        return Err(changed_since_read(path));
    }
    if file.metadata()?.len() != expected.len() as u64 {
        return Err(changed_since_read(path));
    }
    let mut buffer = [0_u8; 8192];
    for chunk in expected.chunks(buffer.len()) {
        file.read_exact(&mut buffer[..chunk.len()])?;
        if buffer[..chunk.len()] != *chunk {
            return Err(changed_since_read(path));
        }
    }
    if file.metadata()?.len() != expected.len() as u64 || Handle::from_path(path)? != source {
        return Err(changed_since_read(path));
    }
    // Write through the locked descriptor. Path replacement after the check
    // cannot redirect this write to another file. The final identity check
    // suppresses a false success if the source was unlinked during the write.
    file.seek(SeekFrom::Start(0))?;
    file.write_all(data)?;
    file.set_len(data.len() as u64)?;
    if Handle::from_path(path)? != source {
        return Err(changed_since_read(path));
    }
    Ok(())
}

async fn write_file_with_retry_hooks<W, WFut, S, SFut, Success, Retry, Exhausted, IsRetryable>(
    mut write: W,
    mut sleep_for: S,
    mut on_retry_success: Success,
    mut on_retry: Retry,
    mut on_exhausted: Exhausted,
    is_retryable: IsRetryable,
) -> io::Result<()>
where
    W: FnMut() -> WFut,
    WFut: Future<Output = io::Result<()>>,
    S: FnMut(Duration) -> SFut,
    SFut: Future<Output = ()>,
    Success: FnMut(usize),
    Retry: FnMut(&io::Error, usize, Duration),
    Exhausted: FnMut(&io::Error, usize),
    IsRetryable: Fn(&io::Error) -> bool,
{
    let mut retry_count = 0usize;

    loop {
        match write().await {
            Ok(()) => {
                if retry_count > 0 {
                    on_retry_success(retry_count);
                }
                return Ok(());
            }
            Err(e) if is_retryable(&e) => {
                if retry_count >= WRITE_RETRY_DELAYS.len() {
                    on_exhausted(&e, retry_count);
                    return Err(e);
                }
                let delay = WRITE_RETRY_DELAYS[retry_count];
                retry_count += 1;
                on_retry(&e, retry_count, delay);
                sleep_for(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}

#[async_trait::async_trait]
impl AsyncFileSystem for LocalFs {
    #[tracing::instrument(name = "fs.read_file", skip_all)]
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, ComputerError> {
        match fs::read(path).await {
            Ok(data) => Ok(data),
            Err(e) => {
                if is_permission_error(&e) {
                    sandbox::log_violation(&path.display().to_string(), "read");
                }
                Err(e.into())
            }
        }
    }

    #[tracing::instrument(name = "fs.write_file", skip_all)]
    async fn write_file(&self, path: &Path, data: &[u8]) -> Result<(), ComputerError> {
        // implicitly creates the missing directories if any
        if let Some(dir) = path.parent()
            && let Err(e) = fs::create_dir_all(dir).await
        {
            if is_permission_error(&e) {
                sandbox::log_violation(&dir.display().to_string(), "mkdir");
            }
            return Err(e.into());
        }
        if let Err(e) = write_file_with_transient_lock_retries(path, data).await {
            if is_permission_error(&e) {
                sandbox::log_violation(&path.display().to_string(), "write");
            }
            return Err(e.into());
        }
        Ok(())
    }

    #[tracing::instrument(name = "fs.replace_file_if_unchanged", skip_all)]
    async fn replace_file_if_unchanged(
        &self,
        path: &Path,
        expected: &[u8],
        data: &[u8],
    ) -> Result<(), ComputerError> {
        let path = path.to_path_buf();
        let expected = expected.to_vec();
        let data = data.to_vec();
        let result = tokio::task::spawn_blocking(move || {
            replace_file_if_unchanged_blocking(&path, &expected, &data)
        })
        .await
        .map_err(|error| ComputerError::io(error.to_string()))?;
        result.map_err(Into::into)
    }

    #[tracing::instrument(name = "fs.create_file_if_absent", skip_all)]
    async fn create_file_if_absent(&self, path: &Path, data: &[u8]) -> Result<(), ComputerError> {
        let write_path = path.to_path_buf();
        let data = data.to_vec();
        let result = tokio::task::spawn_blocking(move || -> io::Result<()> {
            let parent = write_path
                .parent()
                .filter(|dir| !dir.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            std::fs::create_dir_all(parent)?;
            let mut staged = tempfile::NamedTempFile::new_in(parent)?;
            staged.write_all(&data)?;
            staged.as_file_mut().sync_all()?;
            staged
                .persist_noclobber(&write_path)
                .map_err(|error| error.error)?;
            Ok(())
        })
        .await
        .map_err(|error| ComputerError::io(error.to_string()))?;
        if let Err(error) = result {
            if is_permission_error(&error) {
                sandbox::log_violation(&path.display().to_string(), "write");
            }
            return Err(error.into());
        }
        Ok(())
    }

    #[tracing::instrument(name = "fs.delete_file", skip_all)]
    async fn delete_file(&self, path: &Path) -> Result<(), ComputerError> {
        if let Err(e) = fs::remove_file(path).await {
            if is_permission_error(&e) {
                sandbox::log_violation(&path.display().to_string(), "delete");
            }
            return Err(e.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[tokio::test]
    async fn conditional_edit_rejects_stale_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, b"before").unwrap();
        std::fs::write(&path, b"newer").unwrap();
        let error = LocalFs
            .replace_file_if_unchanged(&path, b"before", b"ours")
            .await
            .unwrap_err();
        assert_eq!(error.io_error_kind(), Some(io::ErrorKind::InvalidData));
        assert_eq!(std::fs::read(&path).unwrap(), b"newer");
    }

    #[tokio::test]
    async fn conditional_edits_on_same_file_allow_only_one_source_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, b"before").unwrap();
        let (first, second) = tokio::join!(
            LocalFs.replace_file_if_unchanged(&path, b"before", b"first"),
            LocalFs.replace_file_if_unchanged(&path, b"before", b"second"),
        );
        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes == b"first" || bytes == b"second");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn conditional_edit_through_parent_alias_checks_same_inode() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let path = real.join("file.txt");
        let alias_path = alias.join("file.txt");
        std::fs::write(&path, b"before").unwrap();
        let (first, second) = tokio::join!(
            LocalFs.replace_file_if_unchanged(&path, b"before", b"first"),
            LocalFs.replace_file_if_unchanged(&alias_path, b"before", b"second"),
        );
        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn conditional_edit_does_not_write_replacement_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, b"before").unwrap();
        let source = std::fs::File::open(&path).unwrap();
        source.lock_exclusive().unwrap();
        let edit = LocalFs.replace_file_if_unchanged(&path, b"before", b"ours");
        tokio::pin!(edit);
        // The edit is queued while the old inode is locked. It may open
        // either side of the rename; both paths must reject the stale edit.
        tokio::select! {
            result = &mut edit => panic!("edit unexpectedly completed before replacement: {result:?}"),
            _ = tokio::time::sleep(Duration::from_millis(25)) => {}
        }
        std::fs::rename(&path, dir.path().join("old.txt")).unwrap();
        std::fs::write(&path, b"replacement").unwrap();
        drop(source);
        let error = edit.await.unwrap_err();
        assert_eq!(error.io_error_kind(), Some(io::ErrorKind::InvalidData));
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
    }

    #[test]
    fn conditional_edit_lock_wait_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, b"before").unwrap();
        let held = std::fs::File::open(&path).unwrap();
        held.lock_exclusive().unwrap();
        let contender = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let error = lock_edit_file(&contender, &path, Duration::from_millis(20)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(std::fs::read(&path).unwrap(), b"before");
    }

    #[test]
    fn cooperating_processes_allow_one_commit_per_source_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, b"before").unwrap();
        let binary = std::env::current_exe().unwrap();
        let mut children = (0..2)
            .map(|index| {
                std::process::Command::new(&binary)
                    .arg("conditional_commit_child_process")
                    .env("GROW_CONDITIONAL_COMMIT_TEST_ROOT", dir.path())
                    .env("GROW_CONDITIONAL_COMMIT_TEST_INDEX", index.to_string())
                    .spawn()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !(0..2).all(|index| dir.path().join(format!("ready-{index}")).exists()) {
            assert!(
                std::time::Instant::now() < deadline,
                "child did not reach read barrier"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::write(dir.path().join("go"), b"").unwrap();
        for child in &mut children {
            assert!(child.wait().unwrap().success());
        }
        let results = (0..2)
            .map(|index| {
                std::fs::read_to_string(dir.path().join(format!("result-{index}"))).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            results
                .iter()
                .filter(|result| result.as_str() == "ok")
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| result.as_str() == "conflict")
                .count(),
            1
        );
        let final_bytes = std::fs::read(&path).unwrap();
        assert!(final_bytes == b"first" || final_bytes == b"second");
    }

    #[test]
    fn conditional_commit_child_process() {
        let Ok(root) = std::env::var("GROW_CONDITIONAL_COMMIT_TEST_ROOT") else {
            return;
        };
        let index = std::env::var("GROW_CONDITIONAL_COMMIT_TEST_INDEX").unwrap();
        let root = Path::new(&root);
        let path = root.join("file.txt");
        let expected = std::fs::read(&path).unwrap();
        std::fs::write(root.join(format!("ready-{index}")), b"").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !root.join("go").exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "parent did not release commit barrier"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let proposed: &[u8] = if index == "0" { b"first" } else { b"second" };
        let result = match replace_file_if_unchanged_blocking(&path, &expected, proposed) {
            Ok(()) => "ok",
            Err(error) if error.kind() == io::ErrorKind::InvalidData => "conflict",
            Err(error) => panic!("unexpected conditional commit error: {error}"),
        };
        std::fs::write(root.join(format!("result-{index}")), result).unwrap();
    }

    #[tokio::test]
    async fn exclusive_creation_publishes_complete_content_and_preserves_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/file.txt");
        LocalFs
            .create_file_if_absent(&path, b"first")
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first");

        let error = LocalFs
            .create_file_if_absent(&path, b"second")
            .await
            .unwrap_err();
        assert_eq!(error.io_error_kind(), Some(io::ErrorKind::AlreadyExists));
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );

        #[cfg(unix)]
        {
            let alias = dir.path().join("alias.txt");
            std::os::unix::fs::symlink(&path, &alias).unwrap();
            let error = LocalFs
                .create_file_if_absent(&alias, b"third")
                .await
                .unwrap_err();
            assert_eq!(error.io_error_kind(), Some(io::ErrorKind::AlreadyExists));
            assert_eq!(std::fs::read(&path).unwrap(), b"first");
        }
    }

    #[test]
    fn classifies_windows_transient_write_lock_errors() {
        assert!(is_windows_transient_write_lock_raw_os_error(Some(32)));
        assert!(is_windows_transient_write_lock_raw_os_error(Some(33)));
        assert!(!is_windows_transient_write_lock_raw_os_error(Some(5)));
        assert!(!is_windows_transient_write_lock_raw_os_error(None));
    }

    #[cfg(windows)]
    #[test]
    fn classifies_windows_transient_write_lock_io_errors() {
        assert!(is_transient_write_lock_error(
            &io::Error::from_raw_os_error(WINDOWS_ERROR_SHARING_VIOLATION,)
        ));
        assert!(is_transient_write_lock_error(
            &io::Error::from_raw_os_error(WINDOWS_ERROR_LOCK_VIOLATION,)
        ));
        assert!(!is_transient_write_lock_error(
            &io::Error::from_raw_os_error(5)
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_does_not_retry_windows_raw_error_numbers() {
        assert!(!is_transient_write_lock_error(
            &io::Error::from_raw_os_error(WINDOWS_ERROR_SHARING_VIOLATION,)
        ));
    }

    #[tokio::test]
    async fn first_attempt_success_does_not_fire_retry_callbacks() {
        let result = write_file_with_retry_hooks(
            || async { Ok(()) },
            |_| async { panic!("should not sleep") },
            |_| panic!("should not fire on_retry_success"),
            |_, _, _| panic!("should not fire on_retry"),
            |_, _| panic!("should not fire on_exhausted"),
            is_test_transient_write_lock_error,
        )
        .await;
        result.unwrap();
    }

    #[tokio::test]
    async fn transient_lock_errors_are_retried_until_success() {
        let attempts = Rc::new(Cell::new(0usize));
        let sleeps = Rc::new(Cell::new(0usize));
        let retry_success_count = Rc::new(Cell::new(0usize));
        let retry_log_count = Rc::new(Cell::new(0usize));

        let result = write_file_with_retry_hooks(
            {
                let attempts = Rc::clone(&attempts);
                move || {
                    let attempts = Rc::clone(&attempts);
                    async move {
                        let next = attempts.get() + 1;
                        attempts.set(next);
                        if next <= 2 {
                            Err(io::Error::from_raw_os_error(
                                WINDOWS_ERROR_SHARING_VIOLATION,
                            ))
                        } else {
                            Ok(())
                        }
                    }
                }
            },
            {
                let sleeps = Rc::clone(&sleeps);
                move |_| {
                    let sleeps = Rc::clone(&sleeps);
                    async move {
                        sleeps.set(sleeps.get() + 1);
                    }
                }
            },
            |count| retry_success_count.set(count),
            |_, _, _| retry_log_count.set(retry_log_count.get() + 1),
            |_, _| panic!("retry budget should not be exhausted"),
            is_test_transient_write_lock_error,
        )
        .await;

        result.unwrap();
        assert_eq!(attempts.get(), 3);
        assert_eq!(sleeps.get(), 2);
        assert_eq!(retry_log_count.get(), 2);
        assert_eq!(retry_success_count.get(), 2);
    }

    #[tokio::test]
    async fn non_transient_errors_are_not_retried() {
        let attempts = Rc::new(Cell::new(0usize));
        let result = write_file_with_retry_hooks(
            {
                let attempts = Rc::clone(&attempts);
                move || {
                    let attempts = Rc::clone(&attempts);
                    async move {
                        attempts.set(attempts.get() + 1);
                        Err(io::Error::new(io::ErrorKind::NotFound, "missing"))
                    }
                }
            },
            |_| async {},
            |_| panic!("write did not succeed"),
            |_, _, _| panic!("non-transient errors must not be retried"),
            |_, _| panic!("non-transient errors must not exhaust retry budget"),
            is_test_transient_write_lock_error,
        )
        .await;

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(attempts.get(), 1);
    }

    #[tokio::test]
    async fn persistent_transient_lock_exhausts_retry_budget() {
        let attempts = Rc::new(Cell::new(0usize));
        let sleeps = Rc::new(Cell::new(0usize));
        let exhausted_count = Rc::new(Cell::new(0usize));

        let result = write_file_with_retry_hooks(
            {
                let attempts = Rc::clone(&attempts);
                move || {
                    let attempts = Rc::clone(&attempts);
                    async move {
                        attempts.set(attempts.get() + 1);
                        Err(io::Error::from_raw_os_error(WINDOWS_ERROR_LOCK_VIOLATION))
                    }
                }
            },
            {
                let sleeps = Rc::clone(&sleeps);
                move |_| {
                    let sleeps = Rc::clone(&sleeps);
                    async move {
                        sleeps.set(sleeps.get() + 1);
                    }
                }
            },
            |_| panic!("write did not succeed"),
            |_, _, _| {},
            |_, count| exhausted_count.set(count),
            is_test_transient_write_lock_error,
        )
        .await;

        assert_eq!(
            result.unwrap_err().raw_os_error(),
            Some(WINDOWS_ERROR_LOCK_VIOLATION)
        );
        assert_eq!(attempts.get(), WRITE_RETRY_DELAYS.len() + 1);
        assert_eq!(sleeps.get(), WRITE_RETRY_DELAYS.len());
        assert_eq!(exhausted_count.get(), WRITE_RETRY_DELAYS.len());
    }
}

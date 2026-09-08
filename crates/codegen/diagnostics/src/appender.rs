//! Shared non-blocking file appender + worker-guard registry for diagnostics file-log layers.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

// Park every worker guard for process lifetime; dropping a guard flushes and
// shuts down that file's writer thread, so accumulate (never overwrite) to let
// multiple file-log layers coexist.
static FILE_LOG_GUARDS: OnceLock<Mutex<Vec<WorkerGuard>>> = OnceLock::new();

/// Shared non-blocking file writer for diagnostics file-log layers. Opens `path`
/// in append mode and parks the worker guard for process lifetime so buffered
/// logs aren't lost. Sibling loggers (hooks/memory/sampling/instrumentation) can
/// migrate onto this in a follow-up.
pub(crate) fn non_blocking_file_writer(path: &Path) -> std::io::Result<NonBlocking> {
    let (writer, guard) = unparked_file_writer(path)?;
    park_file_log_guard(guard);
    Ok(writer)
}

pub(crate) fn unparked_file_writer(path: &Path) -> std::io::Result<(NonBlocking, WorkerGuard)> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;

    file.try_lock_shared().map_err(std::io::Error::other)?;
    Ok(tracing_appender::non_blocking(file))
}

pub(crate) fn park_file_log_guard(guard: WorkerGuard) {
    let guards = FILE_LOG_GUARDS.get_or_init(|| Mutex::new(Vec::new()));
    // Recover from a poisoned mutex so the guard is always parked; dropping it
    // would shut down the writer thread and silently lose buffered logs.
    let mut guards = guards
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guards.push(guard);
}

/// Drop all parked worker guards, flushing their non-blocking writers. Call at
/// process exit so short-lived runs (e.g. headless `grow -p`) don't lose buffered logs.
pub(crate) fn flush_file_log_guards() {
    if let Some(m) = FILE_LOG_GUARDS.get() {
        // Recover from a poisoned mutex so exit-flush still drains the guards.
        let mut guards = m.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        guards.clear(); // dropping each WorkerGuard flushes + joins its writer thread
    }
}

#[cfg(test)]
pub(crate) fn parked_guard_count() -> usize {
    FILE_LOG_GUARDS
        .get()
        .map_or(0, |guards| guards.lock().unwrap().len())
}

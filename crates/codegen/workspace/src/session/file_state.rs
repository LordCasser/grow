//! File state tracking for session rewind functionality.
//!
//! This module provides the ability to capture and restore file states at specific
//! points during a session. Each "rewind point" corresponds to a user prompt and
//! stores snapshots of all files that were read or modified during that prompt's
//! processing.
//!
//! **Path Storage**: File paths in `FileSnapshot` and `RewindPoint` are always
//! `RelPathBuf`, relative to the session CWD. Absolute or non-UTF-8 paths are
//! rejected before they can enter durable rewind state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, BufRead, Seek as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::file_system::{AsyncFileSystem, AsyncFsWrapper, bytes_to_string};
// Minimal ToolContext for Phase 1 compile (duplicated to break shell cycle; fields/methods needed by rewind logic preserved for identical public API).
#[derive(Clone)]
pub struct ToolContext {
    pub cwd: std::path::PathBuf,
    pub fs: crate::file_system::AsyncFsWrapper,
}
impl ToolContext {
    pub fn new_local_context(
        cwd: std::path::PathBuf,
        fs: crate::file_system::AsyncFsWrapper,
        _runner: std::sync::Arc<dyn std::any::Any + Send + Sync>,
    ) -> Self {
        Self { cwd, fs }
    }
}
impl Default for ToolContext {
    fn default() -> Self {
        Self {
            cwd: std::path::PathBuf::new(),
            fs: crate::file_system::AsyncFsWrapper::new(std::sync::Arc::new(
                crate::file_system::MockFs::new(std::path::PathBuf::new()),
            )),
        }
    }
}
use paths::RelPathBuf;

/// A snapshot of a single file's content at a specific point in time.
///
/// `path` is relative to the session CWD.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSnapshot {
    pub path: RelPathBuf,
    /// The content of the file at the time of snapshot (None if file didn't exist)
    pub content: Option<String>,
    /// When this snapshot was taken
    pub captured_at: DateTime<Utc>,
}

impl FileSnapshot {
    /// Create a new file snapshot with a relative path.
    pub fn new(path: RelPathBuf, content: Option<String>) -> Self {
        Self {
            path,
            content,
            captured_at: Utc::now(),
        }
    }

    /// Get the path as a Path reference.
    pub fn as_path(&self) -> &Path {
        self.path.as_ref()
    }

    /// Convert the path to an absolute path using the given root.
    pub fn to_absolute_path(&self, root: &Path) -> PathBuf {
        self.path.to_absolute(root)
    }
}

/// A rewind point representing the state at a specific user prompt.
///
/// Contains snapshots of all files that were accessed (read or modified)
/// during the processing of that prompt.
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewindPoint {
    /// Index of the user prompt in the session (0-based)
    pub prompt_index: usize,
    /// When this rewind point was created
    pub created_at: DateTime<Utc>,
    /// File snapshots captured BEFORE any operations for this prompt.
    /// Key is the path to the file.
    pub file_snapshots: HashMap<RelPathBuf, FileSnapshot>,
    /// File snapshots captured AFTER all operations for this prompt completed.
    /// Used to detect external modifications (if current file != after_snapshots, something else changed it).
    pub after_snapshots: HashMap<RelPathBuf, FileSnapshot>,
}

impl RewindPoint {
    /// Create a new empty rewind point for the given prompt index
    pub fn new(prompt_index: usize) -> Self {
        Self {
            prompt_index,
            created_at: Utc::now(),
            file_snapshots: HashMap::new(),
            after_snapshots: HashMap::new(),
        }
    }

    /// Add a file snapshot to this rewind point (if not already present)
    pub fn add_snapshot(&mut self, snapshot: FileSnapshot) {
        // Only capture the first snapshot for each file (the state BEFORE any operations)
        self.file_snapshots
            .entry(snapshot.path.clone())
            .or_insert(snapshot);
    }

    /// Set the after-snapshot for a file (what the agent wrote)
    pub fn set_after_snapshot(&mut self, snapshot: FileSnapshot) {
        self.after_snapshots.insert(snapshot.path.clone(), snapshot);
    }

    /// Get the snapshot for a specific file path
    pub fn get_snapshot(&self, path: &RelPathBuf) -> Option<&FileSnapshot> {
        self.file_snapshots.get(path)
    }

    /// List all file paths that have snapshots in this rewind point
    pub fn snapshot_paths(&self) -> Vec<&RelPathBuf> {
        self.file_snapshots.keys().collect()
    }
}

/// Lightweight metadata for a single rewind point — what the rewind picker needs
/// (which prompts have snapshots, and when) without materializing the
/// (potentially huge) file contents. Produced by [`scan_rewind_point_metas`].
#[derive(Debug)]
pub struct RewindPointMeta {
    pub prompt_index: usize,
    pub created_at: DateTime<Utc>,
    pub num_file_snapshots: usize,
}

#[cfg(test)]
fn open_rewind_points(path: &Path) -> io::Result<Option<io::BufReader<std::fs::File>>> {
    match std::fs::File::open(path) {
        Ok(f) => Ok(Some(io::BufReader::new(f))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Stream-parse a `rewind_points.jsonl` file line-by-line (bounded memory; the
/// file can be hundreds of MB), skipping malformed lines with a `warn!`. Missing
/// file → `Ok(empty)`; a transient I/O error propagates as `Err` so callers don't
/// treat an unreadable file as empty and drop history. This is the LENIENT reader;
/// callers that rewrite the ledger first materialize this complete typed set.
#[cfg(test)]
fn read_rewind_jsonl_lines<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Vec<T>> {
    let Some(mut reader) = open_rewind_points(path)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut line = String::new();
    while reader.read_line(&mut line)? != 0 {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            match serde_json::from_str::<T>(trimmed) {
                Ok(v) => out.push(v),
                Err(e) => tracing::warn!(
                    error = %e,
                    path = %path.display(),
                    "skipping malformed rewind_points.jsonl line"
                ),
            }
        }
        line.clear();
    }
    Ok(out)
}

fn read_rewind_jsonl_from_file<T: serde::de::DeserializeOwned>(
    file: &std::fs::File,
    label: &Path,
) -> io::Result<Vec<T>> {
    let mut file = file.try_clone()?;
    file.seek(io::SeekFrom::Start(0))?;
    let mut reader = io::BufReader::new(file);
    let mut out = Vec::new();
    let mut line = String::new();
    while reader.read_line(&mut line)? != 0 {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            match serde_json::from_str::<T>(trimmed) {
                Ok(value) => out.push(value),
                Err(error) => tracing::warn!(
                    %error,
                    path = %label.display(),
                    "skipping malformed rewind_points.jsonl line"
                ),
            }
        }
        line.clear();
    }
    Ok(out)
}

/// Read all rewind points (full content) for the on-demand historical load.
#[cfg(test)]
fn read_rewind_points_file(path: &Path) -> io::Result<Vec<RewindPoint>> {
    read_rewind_jsonl_lines(path)
}

/// Counts the entries of a JSON map without allocating its keys or values.
struct MapEntryCount(usize);

impl<'de> Deserialize<'de> for MapEntryCount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = usize;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a map")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<usize, A::Error> {
                let mut n = 0;
                while map
                    .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                    .is_some()
                {
                    n += 1;
                }
                Ok(n)
            }
        }
        deserializer.deserialize_map(V).map(MapEntryCount)
    }
}

/// Cheaply scan `rewind_points.jsonl` for per-point metadata, streaming without
/// allocating file-content `String`s (`MapEntryCount` just counts `file_snapshots`;
/// other fields are skipped by serde). `file_snapshots` is required — mirroring
/// `RewindPoint` — so the picker rejects exactly the lines the on-rewind full load
/// would (never advertising a target that won't materialize).
#[cfg(test)]
fn scan_rewind_point_metas(path: &Path) -> io::Result<Vec<RewindPointMeta>> {
    #[derive(Deserialize)]
    struct MetaRow {
        prompt_index: usize,
        created_at: DateTime<Utc>,
        file_snapshots: MapEntryCount,
    }
    Ok(read_rewind_jsonl_lines::<MetaRow>(path)?
        .into_iter()
        .map(|r| RewindPointMeta {
            prompt_index: r.prompt_index,
            created_at: r.created_at,
            num_file_snapshots: r.file_snapshots.0,
        })
        .collect())
}

fn scan_rewind_point_metas_from_file(
    file: &std::fs::File,
    label: &Path,
) -> io::Result<Vec<RewindPointMeta>> {
    #[derive(Deserialize)]
    struct MetaRow {
        prompt_index: usize,
        created_at: DateTime<Utc>,
        file_snapshots: MapEntryCount,
    }
    Ok(read_rewind_jsonl_from_file::<MetaRow>(file, label)?
        .into_iter()
        .map(|row| RewindPointMeta {
            prompt_index: row.prompt_index,
            created_at: row.created_at,
            num_file_snapshots: row.file_snapshots.0,
        })
        .collect())
}

pub struct PinnedRewindSource {
    file: std::fs::File,
    /// Diagnostic label only. Reads always use the pinned file handle.
    label: PathBuf,
}

impl PinnedRewindSource {
    pub fn new(file: std::fs::File, label: PathBuf) -> Self {
        Self { file, label }
    }
}

impl std::fmt::Debug for PinnedRewindSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PinnedRewindSource")
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

/// Fold rewind points at indices `>= target_index` into the point at
/// `target_index - 1` (before-snapshots keep the earliest via `or_insert`,
/// after-snapshots the latest), drop the folded points, and return the survivors.
/// `target_index == 0` clears everything (no predecessor).
///
/// Pure (no I/O), so the in-memory tracker and the disk-authoritative persistence
/// path share it and can't diverge.
pub fn merge_rewind_points_from(
    mut points: Vec<RewindPoint>,
    target_index: usize,
) -> Vec<RewindPoint> {
    if target_index == 0 {
        return Vec::new();
    }
    points.sort_by_key(|p| p.prompt_index);
    // Enforce one point per prompt_index, guarding a corrupt/legacy file with
    // duplicate-index lines (the normal append-once-per-prompt flow never hits this).
    points.dedup_by_key(|p| p.prompt_index);
    let split = points.partition_point(|p| p.prompt_index < target_index);
    // Indices >= target_index, ascending (so after-snapshots keep the latest).
    let to_merge = points.split_off(split);
    if let Some(previous) = points
        .iter_mut()
        .find(|p| p.prompt_index == target_index - 1)
    {
        // Consume `to_merge` by value — move the large file-content snapshots into
        // `previous` instead of cloning (MEMORY.md).
        for merged in to_merge {
            for (path, snapshot) in merged.file_snapshots {
                // or_insert: we own `snapshot`; earliest before-snapshot wins.
                previous.file_snapshots.entry(path).or_insert(snapshot);
            }
            for (path, snapshot) in merged.after_snapshots {
                previous.after_snapshots.insert(path, snapshot);
            }
        }
    }
    points
}

/// Tracks file states across prompts in a session for rewind functionality.
///
/// The tracker maintains a list of rewind points, one per user prompt.
/// Each rewind point captures the state of files BEFORE they are read or modified
/// during that prompt's processing.
///
/// **Lazy historical loading**: a tracker built via [`with_lazy_file`] does NOT
/// read the (potentially huge) persisted rewind points up front, so resuming a
/// session is cheap. They load on demand the first time a rewind *operation* needs
/// them (see [`ensure_historical_loaded`]). Live capture and persisting the
/// current prompt's point (`get_rewind_point`) deliberately do NOT trigger the
/// load, so "resume then keep working" stays fast; the picker uses the
/// metadata-only [`get_rewind_point_metas`].
///
/// [`with_lazy_file`]: FileStateTracker::with_lazy_file
/// [`ensure_historical_loaded`]: FileStateTracker::ensure_historical_loaded
/// [`get_rewind_point_metas`]: FileStateTracker::get_rewind_point_metas
#[derive(Debug)]
pub struct FileStateTracker {
    /// All rewind points for this session, indexed by prompt_index
    rewind_points: Arc<Mutex<HashMap<usize, RewindPoint>>>,
    /// Current prompt index being processed
    current_prompt_index: Arc<Mutex<Option<usize>>>,
    /// Deferred historical source: one pinned file handle until the points are lazily
    /// loaded (then `None`); `None` from the start without a lazy source.
    lazy_source: Arc<Mutex<Option<PinnedRewindSource>>>,
}

impl Default for FileStateTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl FileStateTracker {
    /// Create a new file state tracker
    pub fn new() -> Self {
        Self {
            rewind_points: Arc::new(Mutex::new(HashMap::new())),
            current_prompt_index: Arc::new(Mutex::new(None)),
            lazy_source: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a tracker that lazily loads its historical rewind points from
    /// an already-opened file on first rewind access (resume path). The in-memory set starts
    /// empty and live captures win over disk on load (`or_insert`), never clobbered.
    pub fn with_lazy_file(file: std::fs::File, label: PathBuf) -> Self {
        Self::with_lazy_source(PinnedRewindSource::new(file, label))
    }

    pub fn with_lazy_source(source: PinnedRewindSource) -> Self {
        Self {
            rewind_points: Arc::new(Mutex::new(HashMap::new())),
            current_prompt_index: Arc::new(Mutex::new(None)),
            lazy_source: Arc::new(Mutex::new(Some(source))),
        }
    }

    /// Materialize the deferred historical rewind points (no-op if already loaded
    /// or no lazy source). Triggered by rewind *operations* needing full file
    /// contents; in-memory points win over disk via `or_insert`, so concurrent
    /// live captures are never lost.
    ///
    /// The `lazy_source` lock is held across the (large, blocking) read + merge:
    /// releasing it early would let a concurrent rewind observe `lazy_source ==
    /// None` mid-merge and skip/truncate historical points. The source is consumed
    /// only on a SUCCESSFUL read, so a transient error leaves it set to retry
    /// (never operating on or persisting a partial set).
    async fn ensure_historical_loaded(&self) {
        let mut source = self.lazy_source.lock().await;
        let Some(lazy_file) = source.as_ref() else {
            return; // already loaded, or never lazy
        };
        let loaded =
            match read_rewind_jsonl_from_file::<RewindPoint>(&lazy_file.file, &lazy_file.label) {
                Ok(points) => points,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %lazy_file.label.display(),
                        "deferred rewind-point load failed; leaving lazy source set to retry"
                    );
                    return;
                }
            };
        if !loaded.is_empty() {
            let mut points = self.rewind_points.lock().await;
            for p in loaded {
                points.entry(p.prompt_index).or_insert(p);
            }
        }
        // Success: consume the source so subsequent calls are no-ops.
        *source = None;
    }

    /// Start tracking a new prompt
    pub async fn begin_prompt(&self, prompt_index: usize) {
        let mut current = self.current_prompt_index.lock().await;
        *current = Some(prompt_index);

        // Create a new rewind point for this prompt if it doesn't exist
        let mut points = self.rewind_points.lock().await;
        points
            .entry(prompt_index)
            .or_insert_with(|| RewindPoint::new(prompt_index));
    }

    /// End tracking for the given prompt.
    /// This captures after-snapshots for all files that were touched during the prompt.
    ///
    /// The caller provides the explicit `prompt_index` so that end_prompt works
    /// even when `begin_prompt` was never received.
    pub async fn end_prompt(&self, fs: &AsyncFsWrapper, prompt_index: usize) {
        // Clear internal current-prompt tracking.
        {
            let mut current = self.current_prompt_index.lock().await;
            *current = None;
        }

        // Capture after-snapshots for all files that were touched
        let paths_to_capture: Vec<RelPathBuf> = {
            let points = self.rewind_points.lock().await;
            if let Some(point) = points.get(&prompt_index) {
                point.file_snapshots.keys().cloned().collect()
            } else {
                vec![]
            }
        };

        for rel_path in paths_to_capture {
            let content = fs
                .try_read_file(&rel_path)
                .await
                .and_then(|opt| opt.map(bytes_to_string).transpose())
                .unwrap_or(None);

            let snapshot = FileSnapshot::new(rel_path, content);

            let mut points = self.rewind_points.lock().await;
            if let Some(point) = points.get_mut(&prompt_index) {
                point.set_after_snapshot(snapshot);
            }
        }
    }

    /// Capture a file's current state before an operation.
    /// This should be called BEFORE reading or writing a file.
    ///
    /// `path` is the absolute path to the file. It will be converted to a `RelPathBuf`
    /// (using `cwd`) for storage. Files outside the CWD are silently skipped (they
    /// don't need rewind tracking since the agent shouldn't modify them).
    ///
    /// NOTE: This method is similar to `capture_file_state_with_fs`. They are kept
    /// separate due to type system constraints (`AsyncFileSystem` trait vs `AsyncFsWrapper`
    /// concrete type). Keep them in sync when making changes.
    pub async fn capture_file_state<F: AsyncFileSystem + ?Sized>(
        &self,
        fs: &F,
        path: &Path,
        cwd: &Path,
    ) -> Result<(), crate::file_system::FsError> {
        // Skip files outside the CWD - they don't need rewind tracking
        // (e.g., /etc/hosts, system files, files in other projects)
        let Ok(rel_path) = RelPathBuf::from_absolute(cwd, path) else {
            return Ok(());
        };

        let current = self.current_prompt_index.lock().await;
        let Some(prompt_index) = *current else {
            // Not currently processing a prompt, skip capture
            return Ok(());
        };
        drop(current); // Release lock before async operations

        // Read current file content (or None if it doesn't exist)
        let content = fs
            .try_read_file(path)
            .await?
            .map(bytes_to_string)
            .transpose()?;

        let snapshot = FileSnapshot::new(rel_path, content);

        // Add to the current rewind point
        let mut points = self.rewind_points.lock().await;
        if let Some(point) = points.get_mut(&prompt_index) {
            point.add_snapshot(snapshot);
        }

        Ok(())
    }

    /// Capture a file's current state before an operation using `AsyncFsWrapper`.
    ///
    /// This is a variant of `capture_file_state` that accepts `AsyncFsWrapper`.
    /// Files outside the CWD are silently skipped (they don't need rewind tracking).
    ///
    /// NOTE: This method is similar to `capture_file_state`. They are kept separate
    /// due to type system constraints (`AsyncFsWrapper` concrete type vs generic
    /// `AsyncFileSystem` trait). Keep them in sync when making changes.
    pub async fn capture_file_state_with_fs(
        &self,
        fs: &AsyncFsWrapper,
        path: &Path,
        cwd: &Path,
    ) -> Result<(), crate::file_system::FsError> {
        // Skip files outside the CWD - they don't need rewind tracking
        // (e.g., /etc/hosts, system files, files in other projects)
        let Ok(rel_path) = RelPathBuf::from_absolute(cwd, path) else {
            return Ok(());
        };

        let current = self.current_prompt_index.lock().await;
        let Some(prompt_index) = *current else {
            // Not currently processing a prompt, skip capture
            return Ok(());
        };
        drop(current); // Release lock before async operations

        // Read current file content (or None if it doesn't exist)
        let content = fs
            .try_read_file(path)
            .await?
            .map(bytes_to_string)
            .transpose()?;

        let snapshot = FileSnapshot::new(rel_path, content);

        // Add to the current rewind point
        let mut points = self.rewind_points.lock().await;
        if let Some(point) = points.get_mut(&prompt_index) {
            point.add_snapshot(snapshot);
        }

        Ok(())
    }

    /// Add a before-snapshot with provided content for a specific prompt.
    ///
    /// Unlike `capture_file_state`, this does NOT read from the filesystem.
    /// The caller provides the content directly (e.g., from a `FileWritten`
    /// notification that already carries `previous_content`).
    ///
    /// `path` is the absolute path. `cwd` is used for relativization.
    /// Files outside the CWD are silently skipped.
    pub async fn add_before_snapshot_for_prompt(
        &self,
        prompt_index: usize,
        path: &Path,
        cwd: &Path,
        content: Option<String>,
    ) {
        // Skip files outside the CWD
        let Ok(rel_path) = RelPathBuf::from_absolute(cwd, path) else {
            return;
        };

        let snapshot = FileSnapshot::new(rel_path, content);

        let mut points = self.rewind_points.lock().await;
        let point = points
            .entry(prompt_index)
            .or_insert_with(|| RewindPoint::new(prompt_index));
        point.add_snapshot(snapshot);
    }

    /// Get all rewind points (materializes the deferred historical set).
    pub async fn get_rewind_points(&self) -> Vec<RewindPoint> {
        self.ensure_historical_loaded().await;
        let points = self.rewind_points.lock().await;
        let mut result: Vec<RewindPoint> = points.values().cloned().collect();
        result.sort_by_key(|p| p.prompt_index);
        result
    }

    /// Lightweight metadata for every known rewind point, for the rewind picker.
    /// Combines in-memory points with a metadata-only scan of the lazy disk source
    /// — without materializing file contents and without consuming the source (a
    /// later rewind still does the full load). In-memory points win on conflict.
    ///
    /// Lock order mirrors [`ensure_historical_loaded`] (`lazy_source` outer,
    /// `rewind_points` inner): holding `lazy_source` across both the in-memory
    /// snapshot and the disk scan stops a concurrent rewind's take→read→merge from
    /// interleaving and making the picker miss points.
    pub async fn get_rewind_point_metas(&self) -> Vec<RewindPointMeta> {
        let source = self.lazy_source.lock().await;
        let mut metas: HashMap<usize, RewindPointMeta> = {
            let points = self.rewind_points.lock().await;
            points
                .values()
                .map(|p| {
                    (
                        p.prompt_index,
                        RewindPointMeta {
                            prompt_index: p.prompt_index,
                            created_at: p.created_at,
                            num_file_snapshots: p.file_snapshots.len(),
                        },
                    )
                })
                .collect()
        };
        if let Some(lazy_file) = source.as_ref() {
            match scan_rewind_point_metas_from_file(&lazy_file.file, &lazy_file.label) {
                Ok(scanned) => {
                    for meta in scanned {
                        metas.entry(meta.prompt_index).or_insert(meta);
                    }
                }
                Err(e) => tracing::warn!(
                    error = %e,
                    path = %lazy_file.label.display(),
                    "rewind-point metadata scan failed; picker shows in-memory points only"
                ),
            }
        }
        let mut result: Vec<RewindPointMeta> = metas.into_values().collect();
        result.sort_by_key(|m| m.prompt_index);
        result
    }

    /// Get a specific rewind point by prompt index. Intentionally does NOT trigger
    /// the historical load: this is the live persistence path (a just-completed
    /// prompt's point is always in memory), so resume-then-work stays fast.
    pub async fn get_rewind_point(&self, prompt_index: usize) -> Option<RewindPoint> {
        let points = self.rewind_points.lock().await;
        points.get(&prompt_index).cloned()
    }

    /// Get the current prompt index being tracked
    pub async fn current_prompt_index(&self) -> Option<usize> {
        *self.current_prompt_index.lock().await
    }

    /// Clear all rewind points after (and including) the specified prompt index.
    /// This is used when rewinding to truncate future history.
    pub async fn truncate_from(&self, prompt_index: usize) {
        self.ensure_historical_loaded().await;
        let mut points = self.rewind_points.lock().await;
        points.retain(|&idx, _| idx < prompt_index);
    }

    /// Merge rewind points at indices >= `target_index` into the previous point
    /// (`target_index - 1`), then remove the merged points.
    ///
    /// Used by ConversationOnly rewind: the conversation is rewound but files
    /// are untouched, so the file effects of the discarded prompts must be
    /// folded into the last surviving prompt's rewind point. This ensures:
    /// - `/rewind 0` can still undo all file effects (merged into point N-1)
    /// - A new prompt at `target_index` gets a fresh rewind point with correct
    ///   before-snapshots (the current disk state)
    ///
    /// For `target_index == 0` there is no previous point to merge into, so all
    /// points are simply cleared.
    pub async fn merge_and_remove_from(&self, target_index: usize) {
        self.ensure_historical_loaded().await;
        let mut points = self.rewind_points.lock().await;
        // Move the points out (no clone), merge, then rebuild the map.
        let all: Vec<RewindPoint> = std::mem::take(&mut *points).into_values().collect();
        for p in merge_rewind_points_from(all, target_index) {
            points.insert(p.prompt_index, p);
        }
    }

    /// Install the already-persisted complete rewind projection.
    pub async fn replace_rewind_points(&self, replacement: Vec<RewindPoint>) {
        let mut source = self.lazy_source.lock().await;
        let mut points = self.rewind_points.lock().await;
        points.clear();
        points.extend(
            replacement
                .into_iter()
                .map(|point| (point.prompt_index, point)),
        );
        *source = None;
    }

    /// Get the maximum prompt index that has a rewind point
    pub async fn max_prompt_index(&self) -> Option<usize> {
        self.ensure_historical_loaded().await;
        let points = self.rewind_points.lock().await;
        points.keys().max().copied()
    }
}

/// Type of external modification detected while restoring a file checkpoint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictType {
    DeletedExternally,
    CreatedExternally,
    ModifiedExternally,
}

/// A file that could not be restored because it changed outside the tracked turn.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRewindConflict {
    pub path: String,
    pub conflict_type: ConflictType,
}

/// Result of restoring the canonical shell-owned file checkpoints.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRewindResponse {
    pub success: bool,
    pub target_prompt_index: usize,
    pub reverted_files: Vec<String>,
    pub clean_files: Vec<String>,
    pub conflicts: Vec<FileRewindConflict>,
    pub error: Option<String>,
}

/// Rewind files to the state before `target_prompt_index`.
///
/// Shared implementation for local workspace and ACP session operations. Performs:
/// 1. Gather earliest before-snapshot per file from points >= target
/// 2. Detect conflicts (external modifications since the agent's writes)
/// 3. Revert files to their before-snapshot state
/// 4. Truncate rewind points from the target onward
///
/// Returns a `FileRewindResponse` with revert results.
pub async fn rewind_files(
    tracker: &FileStateTracker,
    fs: &crate::file_system::AsyncFsWrapper,
    target_prompt_index: usize,
) -> FileRewindResponse {
    let all_points = tracker.get_rewind_points().await;

    let mut reverted_files = Vec::new();
    let mut clean_files = Vec::new();
    let mut conflicts = Vec::new();
    let mut had_errors = false;

    // Collect files to revert: gather earliest before-snapshot per file
    let mut files_to_revert: HashMap<RelPathBuf, Option<String>> = HashMap::new();

    for point in all_points
        .iter()
        .filter(|p| p.prompt_index >= target_prompt_index)
    {
        for (path, before_snapshot) in &point.file_snapshots {
            files_to_revert
                .entry(path.clone())
                .or_insert_with(|| before_snapshot.content.clone());
        }
    }

    // Conflict detection + revert
    for (rel_path, content) in &files_to_revert {
        let current_content = match fs.try_read_to_string(rel_path).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(?rel_path, ?e, "rewind: failed to read current content");
                None
            }
        };
        let after_content = all_points
            .iter()
            .rev()
            .find_map(|p| p.after_snapshots.get(rel_path))
            .and_then(|s| s.content.clone());

        if current_content == after_content {
            clean_files.push(rel_path.to_string());
        } else {
            let conflict_type = if current_content.is_none() && after_content.is_some() {
                ConflictType::DeletedExternally
            } else if current_content.is_some() && after_content.is_none() {
                ConflictType::CreatedExternally
            } else {
                ConflictType::ModifiedExternally
            };
            conflicts.push(FileRewindConflict {
                path: rel_path.to_string(),
                conflict_type,
            });
        }

        // Perform the revert — AsyncFsWrapper resolves RelPathBuf against its root.
        match content {
            Some(data) => {
                if let Err(e) = fs.write_file(rel_path, data.as_bytes()).await {
                    tracing::warn!(?rel_path, ?e, "rewind: failed to restore file");
                    had_errors = true;
                    continue;
                }
            }
            None => {
                if fs.exists(rel_path).await.unwrap_or(false)
                    && let Err(e) = fs.delete_file(rel_path).await
                {
                    tracing::warn!(?rel_path, ?e, "rewind: failed to delete file");
                    had_errors = true;
                    continue;
                }
            }
        }
        reverted_files.push(rel_path.to_string());
    }

    // Truncate rewind points from the target index onward.
    // Skip truncation when errors occurred so retry data is preserved.
    if !had_errors {
        tracker.truncate_from(target_prompt_index).await;
    }

    let error = if had_errors {
        Some("Some files could not be reverted".to_string())
    } else {
        None
    };

    FileRewindResponse {
        success: !had_errors,
        target_prompt_index,
        reverted_files,
        clean_files,
        conflicts,
        error,
    }
}

/// Handle for sending file state capture requests.
/// This is a lightweight clone-able handle that can be passed to tools.
#[derive(Clone)]
pub struct FileStateHandle {
    tracker: Arc<FileStateTracker>,
}

impl FileStateHandle {
    /// Create a new handle from a tracker
    pub fn new(tracker: Arc<FileStateTracker>) -> Self {
        Self { tracker }
    }

    /// Capture file state before an operation.
    ///
    /// `path` is the absolute path to the file. `cwd` is used to convert it to
    /// a relative path for portable storage.
    pub async fn capture<F: AsyncFileSystem + ?Sized>(
        &self,
        fs: &F,
        path: &Path,
        cwd: &Path,
    ) -> Result<(), crate::file_system::FsError> {
        self.tracker.capture_file_state(fs, path, cwd).await
    }

    /// Capture file state before an operation using `AsyncFsWrapper`.
    ///
    /// `path` is the absolute path to the file. `cwd` is used to convert it to
    /// a relative path for portable storage.
    pub async fn capture_with_fs(
        &self,
        fs: &AsyncFsWrapper,
        path: &Path,
        cwd: &Path,
    ) -> Result<(), crate::file_system::FsError> {
        self.tracker.capture_file_state_with_fs(fs, path, cwd).await
    }

    /// Get the underlying tracker
    pub fn tracker(&self) -> &Arc<FileStateTracker> {
        &self.tracker
    }
}

#[cfg(test)]
mod tests {
    use super::ToolContext; // from stub above
    use super::*;
    use crate::file_system::MockFs;
    use paths::AbsPathBuf;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_rewind_point_creation() {
        let tracker = FileStateTracker::new();
        let cwd = AbsPathBuf::new(PathBuf::from("/test")).unwrap();
        let fs = Arc::new(MockFs::new(cwd.to_path_buf()));
        let fs_wrapper = crate::file_system::AsyncFsWrapper::new(fs);
        let ctx = ToolContext::new_local_context(cwd.to_path_buf(), fs_wrapper, Arc::new(()));

        // Start a prompt
        tracker.begin_prompt(0).await;
        assert_eq!(tracker.current_prompt_index().await, Some(0));

        // End the prompt
        tracker.end_prompt(&ctx.fs, 0).await;
        assert_eq!(tracker.current_prompt_index().await, None);

        // Rewind point should exist
        let point = tracker.get_rewind_point(0).await;
        assert!(point.is_some());
        assert_eq!(point.unwrap().prompt_index, 0);
    }

    #[tokio::test]
    async fn test_truncate_from() {
        let tracker = FileStateTracker::new();
        let cwd = AbsPathBuf::new(PathBuf::from("/test")).unwrap();
        let fs = Arc::new(MockFs::new(cwd.to_path_buf()));
        let fs_wrapper = crate::file_system::AsyncFsWrapper::new(fs);
        let ctx = ToolContext::new_local_context(cwd.to_path_buf(), fs_wrapper, Arc::new(()));

        // Create multiple rewind points
        for i in 0..5 {
            tracker.begin_prompt(i).await;
            tracker.end_prompt(&ctx.fs, i).await;
        }

        // Verify all points exist
        let points = tracker.get_rewind_points().await;
        assert_eq!(points.len(), 5);

        // Truncate from index 3
        tracker.truncate_from(3).await;

        // Should only have points 0, 1, 2
        let points = tracker.get_rewind_points().await;
        assert_eq!(points.len(), 3);
        assert!(tracker.get_rewind_point(0).await.is_some());
        assert!(tracker.get_rewind_point(1).await.is_some());
        assert!(tracker.get_rewind_point(2).await.is_some());
        assert!(tracker.get_rewind_point(3).await.is_none());
    }

    #[test]
    fn test_file_snapshot() {
        let snapshot = FileSnapshot::new(
            RelPathBuf::new("src/file.txt").unwrap(),
            Some("content".into()),
        );

        assert_eq!(snapshot.as_path(), Path::new("src/file.txt"));
        assert_eq!(snapshot.content, Some("content".into()));
    }

    #[test]
    fn test_rewind_point_add_snapshot() {
        let mut point = RewindPoint::new(0);

        // Add the first snapshot (using relative paths)
        let snapshot1 = FileSnapshot::new(RelPathBuf::new("src/a.txt").unwrap(), Some("v1".into()));
        point.add_snapshot(snapshot1);

        // Try to add second snapshot for same file - should be ignored
        let snapshot2 = FileSnapshot::new(RelPathBuf::new("src/a.txt").unwrap(), Some("v2".into()));
        point.add_snapshot(snapshot2);

        // Should still have v1
        let retrieved = point
            .get_snapshot(&RelPathBuf::new("src/a.txt").unwrap())
            .unwrap();
        assert_eq!(retrieved.content, Some("v1".into()));
    }

    #[test]
    fn rel_path_rejects_absolute_input() {
        assert!(RelPathBuf::new("/home/user/project/src/file.txt").is_err());
        assert!(RelPathBuf::new("src/file.txt").is_ok());
    }

    #[test]
    fn rewind_point_uses_one_relative_path_identity() {
        let mut point = RewindPoint::new(0);
        let snapshot = FileSnapshot::new(
            RelPathBuf::new("src/lib.rs").unwrap(),
            Some("pub mod foo;".into()),
        );
        point.add_snapshot(snapshot);
        let stored = point
            .get_snapshot(&RelPathBuf::new("src/lib.rs").unwrap())
            .unwrap();
        assert_eq!(stored.path.as_str(), "src/lib.rs");
    }

    #[test]
    fn deserialize_file_snapshot_rejects_absolute_path() {
        let json = r#"{
            "path": "/home/user/project/src/main.rs",
            "content": "fn main() {}",
            "captured_at": "2024-01-01T00:00:00Z"
        }"#;
        assert!(serde_json::from_str::<FileSnapshot>(json).is_err());
    }

    #[test]
    fn deserialize_file_snapshot_accepts_canonical_relative_path() {
        let json = r#"{
            "path": "src/main.rs",
            "content": "fn main() {}",
            "captured_at": "2024-01-01T00:00:00Z"
        }"#;

        let snapshot: FileSnapshot = serde_json::from_str(json).unwrap();

        assert_eq!(snapshot.path.as_str(), "src/main.rs");
    }

    #[test]
    fn deserialize_rewind_point_rejects_absolute_paths() {
        let json = r#"{
            "prompt_index": 0,
            "created_at": "2024-01-01T00:00:00Z",
            "file_snapshots": {
                "/home/user/project/src/main.rs": {
                    "path": "/home/user/project/src/main.rs",
                    "content": "fn main() {}",
                    "captured_at": "2024-01-01T00:00:00Z"
                },
                "/home/user/project/src/lib.rs": {
                    "path": "/home/user/project/src/lib.rs",
                    "content": "pub mod foo;",
                    "captured_at": "2024-01-01T00:00:00Z"
                }
            },
            "after_snapshots": {}
        }"#;

        assert!(serde_json::from_str::<RewindPoint>(json).is_err());
    }

    #[test]
    fn deserialize_rewind_point_rejects_mixed_paths() {
        let json = r#"{
            "prompt_index": 1,
            "created_at": "2024-01-01T00:00:00Z",
            "file_snapshots": {
                "/home/user/project/src/old.rs": {
                    "path": "/home/user/project/src/old.rs",
                    "content": "// old file",
                    "captured_at": "2024-01-01T00:00:00Z"
                },
                "src/new.rs": {
                    "path": "src/new.rs",
                    "content": "// new file",
                    "captured_at": "2024-01-01T00:00:00Z"
                }
            },
            "after_snapshots": {}
        }"#;

        assert!(serde_json::from_str::<RewindPoint>(json).is_err());
    }

    #[test]
    fn serialize_produces_relative_string_paths() {
        let snapshot = FileSnapshot::new(
            RelPathBuf::new("src/file.txt").unwrap(),
            Some("content".into()),
        );

        let json = serde_json::to_string(&snapshot).unwrap();

        assert!(json.contains("\"path\":\"src/file.txt\""));
    }

    // ── Lazy historical rewind-point loading ──────────────────────────────────

    /// Build a rewind point at `idx` with the given (relative path, content) files.
    fn point_with_files(idx: usize, files: &[(&str, &str)]) -> RewindPoint {
        let mut p = RewindPoint::new(idx);
        for (path, content) in files {
            p.add_snapshot(FileSnapshot::new(
                RelPathBuf::new(path).unwrap(),
                Some((*content).to_string()),
            ));
        }
        p
    }

    /// Persist rewind points to a temp `rewind_points.jsonl` (one JSON per line).
    fn write_rewind_file(points: &[RewindPoint]) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for p in points {
            writeln!(f, "{}", serde_json::to_string(p).unwrap()).unwrap();
        }
        f.flush().unwrap();
        f
    }

    /// Write raw lines (verbatim) to a temp `rewind_points.jsonl`.
    fn write_rewind_raw(body: &str) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{body}").unwrap();
        f.flush().unwrap();
        f
    }

    fn lazy_tracker(file: &tempfile::NamedTempFile) -> FileStateTracker {
        FileStateTracker::with_lazy_file(
            file.reopen().expect("reopen rewind fixture"),
            file.path().to_path_buf(),
        )
    }

    #[tokio::test]
    async fn lazy_get_rewind_point_singular_does_not_load() {
        let file = write_rewind_file(&[
            point_with_files(0, &[("a.rs", "v0")]),
            point_with_files(1, &[("b.rs", "v1")]),
        ]);
        let tracker = lazy_tracker(&file);

        // Singular lookup must NOT trigger the historical load (live-persist path).
        assert!(tracker.get_rewind_point(0).await.is_none());
        // Nothing materialized yet.
        assert!(tracker.get_rewind_point(1).await.is_none());

        // A plural query (a rewind operation) loads the full set.
        let points = tracker.get_rewind_points().await;
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].prompt_index, 0);
        assert_eq!(points[1].prompt_index, 1);
        // Now singular lookups see the loaded points.
        assert!(tracker.get_rewind_point(0).await.is_some());
    }

    #[tokio::test]
    async fn lazy_metas_scan_without_full_load() {
        let file = write_rewind_file(&[
            point_with_files(0, &[("a.rs", "v0"), ("b.rs", "v0b")]),
            point_with_files(1, &[("c.rs", "v1")]),
            point_with_files(2, &[]),
        ]);
        let tracker = lazy_tracker(&file);

        let metas = tracker.get_rewind_point_metas().await;
        assert_eq!(metas.len(), 3);
        assert_eq!(metas[0].prompt_index, 0);
        assert_eq!(metas[0].num_file_snapshots, 2);
        assert_eq!(metas[1].num_file_snapshots, 1);
        assert_eq!(metas[2].num_file_snapshots, 0);

        // The metadata scan must NOT consume the lazy source: a later rewind
        // operation still gets the full file-content snapshots.
        assert!(tracker.get_rewind_point(0).await.is_none());
        let points = tracker.get_rewind_points().await;
        assert_eq!(points.len(), 3);
        assert_eq!(
            points[0]
                .get_snapshot(&RelPathBuf::new("a.rs").unwrap())
                .and_then(|s| s.content.clone()),
            Some("v0".to_string())
        );
    }

    #[tokio::test]
    async fn lazy_keeps_new_points_and_loads_historical_for_rewind() {
        // Historical points 0,1 on disk; nothing in memory.
        let file = write_rewind_file(&[
            point_with_files(0, &[("a.rs", "h0")]),
            point_with_files(1, &[("b.rs", "h1")]),
        ]);
        let tracker = lazy_tracker(&file);

        // A new prompt during the resumed session adds an in-memory point (no load).
        let cwd = Path::new("/repo");
        tracker
            .add_before_snapshot_for_prompt(2, Path::new("/repo/c.rs"), cwd, Some("new2".into()))
            .await;
        assert!(tracker.get_rewind_point(2).await.is_some());
        // Historical still not loaded.
        assert!(tracker.get_rewind_point(0).await.is_none());

        // Rewinding to a pre-resume prompt loads the historical set and keeps the
        // new in-memory point.
        let all = tracker.get_rewind_points().await;
        assert_eq!(all.len(), 3);
        assert_eq!(
            all.iter().map(|p| p.prompt_index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        // truncate_from(1) keeps only the pre-resume prompt 0.
        tracker.truncate_from(1).await;
        let remaining = tracker.get_rewind_points().await;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].prompt_index, 0);
        assert_eq!(
            remaining[0]
                .get_snapshot(&RelPathBuf::new("a.rs").unwrap())
                .and_then(|s| s.content.clone()),
            Some("h0".to_string())
        );
    }

    #[tokio::test]
    async fn lazy_live_capture_wins_over_disk_at_conflicting_index() {
        // Disk has point 0 with content "disk".
        let file = write_rewind_file(&[point_with_files(0, &[("a.rs", "disk")])]);
        let tracker = lazy_tracker(&file);

        // A LIVE capture at the same index 0 (before any historical load) adds an
        // in-memory point 0 with different content.
        let cwd = Path::new("/repo");
        tracker
            .add_before_snapshot_for_prompt(0, Path::new("/repo/a.rs"), cwd, Some("mem".into()))
            .await;

        // The on-rewind historical load must NOT clobber the in-memory point 0
        // (`or_insert` keeps the live capture).
        let points = tracker.get_rewind_points().await;
        assert_eq!(points.len(), 1);
        assert_eq!(
            points[0]
                .get_snapshot(&RelPathBuf::new("a.rs").unwrap())
                .and_then(|s| s.content.clone()),
            Some("mem".to_string())
        );
    }

    #[tokio::test]
    async fn lazy_metas_combine_memory_and_disk() {
        let file = write_rewind_file(&[point_with_files(0, &[("a.rs", "h0")])]);
        let tracker = lazy_tracker(&file);

        // New in-memory point at index 1.
        let cwd = Path::new("/repo");
        tracker
            .add_before_snapshot_for_prompt(1, Path::new("/repo/b.rs"), cwd, Some("new".into()))
            .await;

        let metas = tracker.get_rewind_point_metas().await;
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].prompt_index, 0); // from disk
        assert_eq!(metas[0].num_file_snapshots, 1);
        assert_eq!(metas[1].prompt_index, 1); // from memory
        assert_eq!(metas[1].num_file_snapshots, 1);
    }

    #[tokio::test]
    async fn tracker_without_historical_file_is_empty() {
        let tracker = FileStateTracker::new();
        assert!(tracker.get_rewind_points().await.is_empty());
        assert!(tracker.get_rewind_point_metas().await.is_empty());
    }

    #[tokio::test]
    async fn lazy_merge_and_remove_loads_historical() {
        // ConversationOnly rewind path: merge_and_remove_from must see history.
        let file = write_rewind_file(&[
            point_with_files(0, &[("a.rs", "h0")]),
            point_with_files(1, &[("b.rs", "h1")]),
            point_with_files(2, &[("c.rs", "h2")]),
        ]);
        let tracker = lazy_tracker(&file);

        // Merge points >= 1 into point 0's predecessor (index 0).
        tracker.merge_and_remove_from(1).await;
        let points = tracker.get_rewind_points().await;
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].prompt_index, 0);
        // Point 0 should now also carry the merged files from points 1 and 2.
        assert!(
            points[0]
                .get_snapshot(&RelPathBuf::new("b.rs").unwrap())
                .is_some()
        );
        assert!(
            points[0]
                .get_snapshot(&RelPathBuf::new("c.rs").unwrap())
                .is_some()
        );
    }

    /// `get_rewind_points` is a rewind op and must trigger the historical load.
    #[tokio::test]
    async fn lazy_get_rewind_points_loads_historical() {
        let file = write_rewind_file(&[point_with_files(0, &[("a.rs", "h0")])]);
        let tracker = lazy_tracker(&file);
        let points = tracker.get_rewind_points().await;
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].prompt_index, 0);
    }

    /// `max_prompt_index` is a rewind op and must trigger the load.
    #[tokio::test]
    async fn lazy_max_prompt_index_loads_historical() {
        let file = write_rewind_file(&[point_with_files(0, &[]), point_with_files(4, &[])]);
        let tracker = lazy_tracker(&file);
        assert_eq!(tracker.max_prompt_index().await, Some(4));
    }

    /// Concurrent live capture + rewind query: must not deadlock, and the full set
    /// after both complete must contain every point.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lazy_concurrent_capture_and_rewind() {
        let file = write_rewind_file(&[
            point_with_files(0, &[("a.rs", "h0")]),
            point_with_files(1, &[("b.rs", "h1")]),
        ]);
        let tracker = Arc::new(lazy_tracker(&file));

        let t1 = tracker.clone();
        let capture = async move {
            let cwd = PathBuf::from("/repo");
            t1.add_before_snapshot_for_prompt(2, &cwd.join("c.rs"), &cwd, Some("new".into()))
                .await;
        };
        let t2 = tracker.clone();
        let query = async move { t2.get_rewind_points().await };
        let (_, points) = tokio::join!(capture, query);

        // The historical set is always visible to the query.
        assert!(points.iter().any(|p| p.prompt_index == 0));
        assert!(points.iter().any(|p| p.prompt_index == 1));

        // After both complete, every point (historical + live) is present.
        let final_all = tracker.get_rewind_points().await;
        assert_eq!(
            final_all.iter().map(|p| p.prompt_index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn scan_rewind_point_metas_reads_counts() {
        let file = write_rewind_file(&[
            point_with_files(0, &[("a.rs", "x"), ("b.rs", "y")]),
            point_with_files(5, &[("c.rs", "z")]),
        ]);
        let metas = scan_rewind_point_metas(file.path()).unwrap();
        assert_eq!(metas.len(), 2);
        assert_eq!(metas[0].prompt_index, 0);
        assert_eq!(metas[0].num_file_snapshots, 2);
        assert_eq!(metas[1].prompt_index, 5);
        assert_eq!(metas[1].num_file_snapshots, 1);
    }

    // ── pure merge_rewind_points_from branch coverage ────────────────────────

    #[test]
    fn merge_pure_target_zero_clears_all() {
        let pts = vec![
            point_with_files(0, &[("a.rs", "0")]),
            point_with_files(1, &[("b.rs", "1")]),
        ];
        assert!(merge_rewind_points_from(pts, 0).is_empty());
    }

    #[test]
    fn merge_pure_folds_before_or_insert_and_after_latest_wins() {
        // shared.rs touched by both points; only1.rs only by p1.
        let mut p0 = RewindPoint::new(0);
        p0.add_snapshot(FileSnapshot::new(
            RelPathBuf::new("shared.rs").unwrap(),
            Some("p0-before".into()),
        ));
        p0.set_after_snapshot(FileSnapshot::new(
            RelPathBuf::new("shared.rs").unwrap(),
            Some("p0-after".into()),
        ));
        let mut p1 = RewindPoint::new(1);
        p1.add_snapshot(FileSnapshot::new(
            RelPathBuf::new("shared.rs").unwrap(),
            Some("p1-before".into()),
        ));
        p1.add_snapshot(FileSnapshot::new(
            RelPathBuf::new("only1.rs").unwrap(),
            Some("p1-only".into()),
        ));
        p1.set_after_snapshot(FileSnapshot::new(
            RelPathBuf::new("shared.rs").unwrap(),
            Some("p1-after".into()),
        ));

        let merged = merge_rewind_points_from(vec![p0, p1], 1);
        assert_eq!(merged.len(), 1);
        let m0 = &merged[0];
        assert_eq!(m0.prompt_index, 0);
        // before-snapshot: earliest (p0) wins for shared.rs (or_insert keeps it).
        assert_eq!(
            m0.get_snapshot(&RelPathBuf::new("shared.rs").unwrap())
                .unwrap()
                .content,
            Some("p0-before".into())
        );
        // p1's only1.rs before-snapshot is folded in.
        assert!(
            m0.get_snapshot(&RelPathBuf::new("only1.rs").unwrap())
                .is_some()
        );
        // after-snapshot: latest (p1) wins for shared.rs (insert overwrites).
        let after_key = RelPathBuf::new("shared.rs").unwrap();
        assert_eq!(
            m0.after_snapshots.get(&after_key).unwrap().content,
            Some("p1-after".into())
        );
    }

    #[test]
    fn merge_pure_missing_predecessor_drops_merged_effects() {
        // points [0, 3], target 3 → predecessor index 2 is absent (gap), so the
        // merged point 3's file effects are dropped (matches the original).
        let merged = merge_rewind_points_from(
            vec![
                point_with_files(0, &[("a.rs", "0")]),
                point_with_files(3, &[("b.rs", "3")]),
            ],
            3,
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].prompt_index, 0);
        assert!(
            merged[0]
                .get_snapshot(&RelPathBuf::new("b.rs").unwrap())
                .is_none()
        );
    }

    #[test]
    fn merge_pure_dedups_duplicate_indices() {
        // Two lines with the same prompt_index (corrupt/legacy) collapse to one.
        let merged = merge_rewind_points_from(
            vec![
                point_with_files(0, &[("a.rs", "first")]),
                point_with_files(0, &[("a.rs", "second")]),
            ],
            5,
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].prompt_index, 0);
    }

    /// Blank/whitespace and malformed lines are skipped; both readers (full load +
    /// meta scan) recover exactly the valid points.
    #[tokio::test]
    async fn readers_recover_from_blank_and_malformed_lines() {
        let p0 = serde_json::to_string(&point_with_files(0, &[("a.rs", "v0")])).unwrap();
        let p2 = serde_json::to_string(&point_with_files(2, &[("c.rs", "v2")])).unwrap();
        let file = write_rewind_raw(&format!("\n   \n{p0}\ngarbage{{not json\n{p2}\n"));

        let full = read_rewind_points_file(file.path()).unwrap();
        assert_eq!(
            full.iter().map(|p| p.prompt_index).collect::<Vec<_>>(),
            vec![0, 2]
        );
        let metas = scan_rewind_point_metas(file.path()).unwrap();
        assert_eq!(
            metas.iter().map(|m| m.prompt_index).collect::<Vec<_>>(),
            vec![0, 2]
        );

        // Same via the tracker's lazy load.
        let tracker = lazy_tracker(&file);
        let points = tracker.get_rewind_points().await;
        assert_eq!(
            points.iter().map(|p| p.prompt_index).collect::<Vec<_>>(),
            vec![0, 2]
        );
    }

    /// A zero-byte file (distinct from a missing file) is `Ok(empty)`.
    #[test]
    fn readers_handle_zero_byte_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        assert!(read_rewind_points_file(file.path()).unwrap().is_empty());
        assert!(scan_rewind_point_metas(file.path()).unwrap().is_empty());
    }

    /// Missing → `Ok(empty)` (fresh session), but a real I/O error (here: a
    /// directory) → `Err`, so the caller keeps the lazy source set rather than
    /// treating it as empty.
    #[test]
    fn readers_distinguish_missing_from_io_error() {
        let missing = PathBuf::from("/nonexistent/dir/rewind_points.jsonl");
        assert!(read_rewind_points_file(&missing).unwrap().is_empty());
        assert!(scan_rewind_point_metas(&missing).unwrap().is_empty());

        let dir = tempfile::tempdir().unwrap();
        assert!(read_rewind_points_file(dir.path()).is_err());
        assert!(scan_rewind_point_metas(dir.path()).is_err());
    }
}

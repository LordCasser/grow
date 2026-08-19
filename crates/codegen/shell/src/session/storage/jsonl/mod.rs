use super::{PersistedData, SessionUpdateEnvelope, StorageAdapter, updates_truncate_for_prompt};
use crate::sampling::{
    ConversationItem, conversation_truncate_for_prompt, transform_conversation_cwd,
};
use crate::session::info::Info;
use crate::session::persistence::{SESSION_FORMAT_VERSION, Summary};
use agent_client_protocol as acp;
use async_trait::async_trait;
use chat_state::Timeline;
use fs2::FileExt;
use std::fs::OpenOptions;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use workspace::session::file_state::RewindPoint;
#[derive(Clone)]
enum SessionDirMode {
    FromRoot(PathBuf),
    Explicit(PathBuf),
}
#[derive(Clone, Copy)]
pub(crate) enum AppendDurability {
    Buffered,
    Durable,
}
/// JSONL storage under `{root}/sessions/{url_encoded_cwd}/{session_id}/`.
#[derive(Clone)]
pub struct JsonlStorageAdapter {
    dir_mode: SessionDirMode,
    #[cfg(test)]
    update_append_probe: Option<std::sync::Arc<AppendProbe>>,
}
#[cfg(test)]
type AppendProbe = dyn Fn(AppendDurability) -> io::Result<()> + Send + Sync;
impl Default for JsonlStorageAdapter {
    fn default() -> Self {
        Self::new()
    }
}
impl JsonlStorageAdapter {
    pub fn new() -> Self {
        Self {
            dir_mode: SessionDirMode::FromRoot(crate::util::grow_home::grow_home()),
            #[cfg(test)]
            update_append_probe: None,
        }
    }
    pub fn with_root(root_dir: PathBuf) -> Self {
        Self {
            dir_mode: SessionDirMode::FromRoot(root_dir),
            #[cfg(test)]
            update_append_probe: None,
        }
    }
    /// Create an adapter that writes directly to `session_dir`, bypassing
    /// the `{root}/sessions/{cwd}/{id}/` path computation.
    ///
    /// Used for subagent child sessions whose files live under the parent's
    /// session directory: `{parent_session_dir}/subagents/{subagent_id}/`.
    pub fn with_explicit_session_dir(session_dir: PathBuf) -> Self {
        Self {
            dir_mode: SessionDirMode::Explicit(session_dir),
            #[cfg(test)]
            update_append_probe: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn with_update_append_probe(
        session_dir: PathBuf,
        append_probe: impl Fn(AppendDurability) -> io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            dir_mode: SessionDirMode::Explicit(session_dir),
            update_append_probe: Some(std::sync::Arc::new(append_probe)),
        }
    }
    /// Read one committed ledger snapshot and derive its exact reference plus
    /// Surface. Fork/resume callers must never obtain these through separate
    /// reads because a concurrent append could make the content outrun its ref.
    pub(crate) fn materialize_timeline_from_dir(
        &self,
        dir: &std::path::Path,
        timeline_id: &str,
    ) -> std::io::Result<chat_state::TimelineMaterialization> {
        let events = self.read_timeline(dir.join(super::TIMELINE_FILE))?;
        let last_seq = events.last().map(|event| event.seq.get()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "source Timeline is empty")
        })?;
        let timeline = Timeline::from_events(events)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        crate::session::persistence::verify_timeline_prompt_blobs(dir, &timeline)?;
        Ok(chat_state::TimelineMaterialization {
            input_ref: chat_state::TimelineRangeRef {
                timeline_id: timeline_id.to_string(),
                first_seq: 0,
                last_seq,
            },
            surface_revision: timeline.surface_revision(),
            surface: timeline.surface().to_vec(),
            surface_ids: timeline.surface_ids().to_vec(),
        })
    }
    pub(crate) fn read_timeline_events_sync(
        &self,
        info: &Info,
    ) -> io::Result<Vec<chat_state::TimelineEvent>> {
        self.read_timeline(self.timeline_file(info))
    }

    pub(crate) fn read_sideband_ledgers_sync(
        &self,
        info: &Info,
        parent: &chat_state::Timeline,
    ) -> io::Result<super::SidebandLedgers> {
        Self::read_sideband_ledgers_from_dir(&self.session_dir(info), &info.id.to_string(), parent)
    }

    pub(crate) fn read_sideband_ledgers_from_dir(
        session_dir: &Path,
        parent_timeline_id: &str,
        parent: &chat_state::Timeline,
    ) -> io::Result<super::SidebandLedgers> {
        let directory = session_dir.join(super::SIDEBANDS_DIR);
        let mut ledgers = super::SidebandLedgers::new();
        if !directory.exists() {
            super::validate_sideband_ledgers(parent_timeline_id, parent, &ledgers)?;
            return Ok(ledgers);
        }
        let mut entries = std::fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            if !entry.file_type()?.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "unsupported entry in sidebands directory: {}",
                        entry.path().display()
                    ),
                ));
            }
            let sideband_id = entry.file_name().into_string().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "sideband id is not UTF-8")
            })?;
            chat_state::validate_sideband_id(&sideband_id)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let timeline_path = entry.path().join(super::TIMELINE_FILE);
            if !timeline_path.exists() {
                let children = std::fs::read_dir(entry.path())?.collect::<Result<Vec<_>, _>>()?;
                if children.is_empty()
                    || children.iter().all(|child| {
                        child.file_name().to_string_lossy()
                            == format!("{}.lock", super::TIMELINE_FILE)
                    })
                {
                    continue;
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("sideband {sideband_id} directory has no Timeline ledger"),
                ));
            }
            let events = super::read_committed_jsonl_file::<chat_state::SidebandEvent>(
                &timeline_path,
                "sideband Timeline ledger",
            )?;
            // A zero-length file can be left only before request seq 0 commits;
            // it is not a ledger fact and is therefore omitted from mirrors.
            if events.is_empty() {
                continue;
            }
            ledgers.insert(sideband_id, events);
        }
        super::validate_sideband_ledgers(parent_timeline_id, parent, &ledgers)?;
        Ok(ledgers)
    }

    /// Append a user-authored title to a dormant session without constructing
    /// a second title store. The immutable Timeline event commits first; the
    /// summary write is only a denormalized projection and is repairable on
    /// the next load if refreshing it fails.
    pub(crate) async fn append_session_title_durable(
        &self,
        info: &Info,
        title: String,
    ) -> io::Result<chat_state::TimelineEvent> {
        let events = self.read_timeline(self.timeline_file(info))?;
        let timeline = Timeline::from_events(events)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let title = chat_state::SessionTitleEvent {
            title,
            source: chat_state::SessionTitleSource::User,
        };
        let event = timeline
            .prepare(chat_state::TimelineEventKind::SessionTitle(title.clone()))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        self.append_timeline_event_with_bookkeeping(info, &event, AppendDurability::Durable)
            .await?;
        Ok(event)
    }
    fn session_dir(&self, info: &Info) -> PathBuf {
        match &self.dir_mode {
            SessionDirMode::FromRoot(root) => root
                .join("sessions")
                .join(crate::util::grow_home::encode_cwd_dirname(&info.cwd))
                .join(info.id.to_string()),
            SessionDirMode::Explicit(dir) => dir.clone(),
        }
    }
    pub(super) fn updates_file(&self, info: &Info) -> PathBuf {
        self.session_dir(info).join(super::UPDATES_FILE)
    }
    fn timeline_file(&self, info: &Info) -> PathBuf {
        self.session_dir(info).join(super::TIMELINE_FILE)
    }
    fn sideband_timeline_file(&self, info: &Info, sideband_id: &str) -> io::Result<PathBuf> {
        chat_state::validate_sideband_id(sideband_id)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        Ok(self
            .session_dir(info)
            .join(super::SIDEBANDS_DIR)
            .join(sideband_id)
            .join(super::TIMELINE_FILE))
    }
    fn summary_file(&self, info: &Info) -> PathBuf {
        self.session_dir(info).join(super::SUMMARY_FILE)
    }
    fn summary_lock_file(&self, info: &Info) -> PathBuf {
        self.session_dir(info)
            .join(format!("{}.lock", super::SUMMARY_FILE))
    }
    fn workflows_dir(&self, info: &Info) -> PathBuf {
        self.session_dir(info).join("workflows")
    }
    fn workflow_run_dir(&self, info: &Info, run_id: &str) -> io::Result<PathBuf> {
        crate::session::workflow::store::validate_run_id(run_id)?;
        Ok(self.workflows_dir(info).join(run_id))
    }
    fn workflow_run_state_file(&self, info: &Info, run_id: &str) -> io::Result<PathBuf> {
        Ok(self.workflow_run_dir(info, run_id)?.join("state.json"))
    }
    fn rewind_points_file(&self, info: &Info) -> PathBuf {
        self.session_dir(info).join("rewind_points.jsonl")
    }
    /// Enumerate all session directories, optionally filtered by cwd.
    ///
    /// Returns the path to each session directory (not the summary file).
    /// Shared by both `list_sessions` (full scan) and `list_sessions_recent`
    /// (mtime-based tail).
    fn scan_session_dirs(&self, cwd: Option<&str>) -> io::Result<Vec<PathBuf>> {
        let root_dir = match &self.dir_mode {
            SessionDirMode::FromRoot(root) => root,
            SessionDirMode::Explicit(_) => return Ok(Vec::new()),
        };
        crate::session::storage::relocation::RelocationView::load(root_dir)
            .and_then(|view| view.session_dirs(cwd))
            .map_err(io::Error::other)
    }
    fn list_sessions_sync(&self, cwd: Option<&str>) -> io::Result<Vec<Summary>> {
        let session_dirs = self.scan_session_dirs(cwd)?;
        let mut summaries = Vec::new();
        for session_dir in session_dirs {
            let summary_path = session_dir.join(super::SUMMARY_FILE);
            match std::fs::read(&summary_path) {
                Ok(bytes) => {
                    if let Ok(summary) = serde_json::from_slice::<Summary>(&bytes)
                        && summary.validate_current_format().is_ok()
                        && !summary.is_hidden()
                    {
                        summaries.push(summary);
                    }
                }
                Err(_) => continue,
            }
        }
        summaries.sort_by_cached_key(|s| {
            (
                std::cmp::Reverse(s.last_active_at.unwrap_or(s.updated_at)),
                s.info.id.0.to_string(),
            )
        });
        Ok(summaries)
    }
    /// List the N most recently modified session summaries across all
    /// workspaces.
    ///
    /// Instead of reading every `summary.json` (expensive at scale — ~12K
    /// files), this stats each file to get its mtime, sorts by mtime, and
    /// only reads the top `limit` files. On a machine with ~12K sessions
    /// this reduces cold-boot `workspace_list` from ~3s to ~200ms.
    /// Final order among candidates uses `last_active_at` else `updated_at`.
    pub async fn list_sessions_recent(&self, limit: usize) -> io::Result<Vec<Summary>> {
        let session_dirs = self.scan_session_dirs(None)?;
        let mut candidates: Vec<(PathBuf, std::time::SystemTime)> =
            Vec::with_capacity(session_dirs.len());
        for session_dir in session_dirs {
            let summary_path = session_dir.join(super::SUMMARY_FILE);
            if let Ok(meta) = std::fs::metadata(&summary_path)
                && let Ok(mtime) = meta.modified()
            {
                candidates.push((summary_path, mtime));
            }
        }
        candidates.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        candidates.truncate(limit);
        let mut summaries = Vec::with_capacity(candidates.len());
        for (summary_path, _) in candidates {
            match std::fs::read(&summary_path) {
                Ok(bytes) => {
                    if let Ok(summary) = serde_json::from_slice::<Summary>(&bytes)
                        && summary.validate_current_format().is_ok()
                        && !summary.is_hidden()
                    {
                        summaries.push(summary);
                    }
                }
                Err(_) => continue,
            }
        }
        summaries.sort_by_cached_key(|s| {
            (
                std::cmp::Reverse(s.last_active_at.unwrap_or(s.updated_at)),
                s.info.id.0.to_string(),
            )
        });
        Ok(summaries)
    }
    async fn append_jsonl<T: serde::Serialize>(&self, path: PathBuf, data: &T) -> io::Result<()> {
        self.append_jsonl_with_durability(path, data, AppendDurability::Buffered)
            .await
    }
    async fn append_jsonl_with_durability<T: serde::Serialize>(
        &self,
        path: PathBuf,
        data: &T,
        durability: AppendDurability,
    ) -> io::Result<()> {
        let mut line =
            serde_json::to_vec(data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        line.push(b'\n');
        Self::append_jsonl_line_blocking(path, line, durability).await
    }
    async fn append_jsonl_line_blocking(
        path: PathBuf,
        line: Vec<u8>,
        durability: AppendDurability,
    ) -> io::Result<()> {
        tokio::task::spawn_blocking(move || Self::append_jsonl_line_sync(&path, line, durability))
            .await
            .map_err(io::Error::other)?
    }

    async fn append_timeline_event_with_durability(
        path: PathBuf,
        event: &chat_state::TimelineEvent,
        durability: AppendDurability,
    ) -> io::Result<()> {
        let event_seq = event.seq.get();
        let mut line = serde_json::to_vec(event)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        line.push(b'\n');
        tokio::task::spawn_blocking(move || {
            Self::append_timeline_line_sync(&path, line, event_seq, durability)
        })
        .await
        .map_err(io::Error::other)?
    }

    async fn append_sideband_event_with_durability(
        path: PathBuf,
        event: &chat_state::SidebandEvent,
        durability: AppendDurability,
    ) -> io::Result<()> {
        let event_seq = event.seq;
        let mut line = serde_json::to_vec(event)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        line.push(b'\n');
        tokio::task::spawn_blocking(move || {
            Self::append_sideband_line_sync(&path, line, event_seq, durability)
        })
        .await
        .map_err(io::Error::other)?
    }

    /// Commit the canonical conversation event, then best-effort refresh the
    /// summary projection derived from it. Once Timeline commits, projection
    /// failure must not turn success back into failure: summary is rebuildable,
    /// while reporting an ambiguous canonical outcome could split live state
    /// from the ledger on retry.
    async fn append_timeline_event_with_bookkeeping(
        &self,
        info: &Info,
        event: &chat_state::TimelineEvent,
        durability: AppendDurability,
    ) -> io::Result<()> {
        Self::append_timeline_event_with_durability(self.timeline_file(info), event, durability)
            .await?;
        let mut patch = super::summary_write::SummaryPatch::default();
        match &event.kind {
            chat_state::TimelineEventKind::Messages(messages) => {
                patch.record_activity = true;
                patch.session_format_version = Some(SESSION_FORMAT_VERSION);
                patch.cwd_switch_bookkeeping_generation = messages
                    .items
                    .iter()
                    .filter_map(ConversationItem::working_directory_switch_generation)
                    .max();
            }
            chat_state::TimelineEventKind::Subagent(_)
            | chat_state::TimelineEventKind::SubagentSeed(_)
            | chat_state::TimelineEventKind::SubagentResult(_) => {
                patch.record_activity = true;
                patch.session_format_version = Some(SESSION_FORMAT_VERSION);
            }
            chat_state::TimelineEventKind::SessionTitle(title) => {
                patch.session_title = Some(super::summary_write::SessionTitleProjection {
                    event_seq: event.seq.get(),
                    title: title.title.clone(),
                    source: title.source.clone(),
                });
            }
            _ => return Ok(()),
        }
        if let Err(error) = self.apply_summary_patch(info, patch).await {
            tracing::warn!(
                %error,
                seq = event.seq.get(),
                "Timeline committed but summary projection refresh failed"
            );
        }
        Ok(())
    }

    /// Append one Timeline event with sequence-aware idempotence.
    ///
    /// A retry after a lost durability acknowledgement re-syncs the identical
    /// tail instead of duplicating it. An incomplete final record is never a
    /// committed fact and is truncated before the retry. Only the tail record
    /// is read, so append cost is independent of ledger length; strict loading
    /// remains responsible for detecting interior corruption.
    fn append_timeline_line_sync(
        path: &Path,
        line: Vec<u8>,
        event_seq: u64,
        durability: AppendDurability,
    ) -> io::Result<()> {
        debug_assert!(line.ends_with(b"\n"));
        let lock = Self::lock_append(path)?;
        let result = (|| {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)?;
            let (complete_len, last_line) = Self::read_timeline_tail(&mut file)?;
            let original_len = file.metadata()?.len();
            if complete_len != original_len {
                tracing::warn!(
                    path = %path.display(),
                    discarded_bytes = original_len - complete_len,
                    "discarding incomplete Timeline tail before append"
                );
                file.set_len(complete_len)?;
            }

            match last_line.as_deref() {
                Some(last_line) => {
                    let last: chat_state::TimelineEvent = serde_json::from_slice(last_line)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    if last.seq.get() == event_seq {
                        if last_line != &line[..line.len() - 1] {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("Timeline seq {event_seq} conflicts with persisted tail"),
                            ));
                        }
                        if matches!(durability, AppendDurability::Durable) {
                            Self::sync_file_durable(&file)?;
                            drop(file);
                            Self::sync_parent_directory(path)?;
                        }
                        return Ok(());
                    }
                    let expected = last.seq.get().checked_add(1).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "Timeline sequence overflow")
                    })?;
                    if event_seq != expected {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "Timeline append expected seq {expected}, received {event_seq}"
                            ),
                        ));
                    }
                }
                None if event_seq != 0 => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("empty Timeline requires seq 0, received {event_seq}"),
                    ));
                }
                None => {}
            }

            file.seek(io::SeekFrom::End(0))?;
            file.write_all(&line)?;
            file.flush()?;
            if matches!(durability, AppendDurability::Durable) {
                Self::sync_file_durable(&file)?;
                drop(file);
                Self::sync_parent_directory(path)?;
            }
            Ok(())
        })();
        let _ = lock.unlock();
        result
    }

    fn append_sideband_line_sync(
        path: &Path,
        line: Vec<u8>,
        event_seq: u64,
        durability: AppendDurability,
    ) -> io::Result<()> {
        debug_assert!(line.ends_with(b"\n"));
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "sideband Timeline has no parent",
            )
        })?;
        std::fs::create_dir_all(parent)?;
        let lock = Self::lock_append(path)?;
        let result = (|| {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)?;
            let (complete_len, last_line) = Self::read_timeline_tail(&mut file)?;
            let original_len = file.metadata()?.len();
            if complete_len != original_len {
                tracing::warn!(
                    path = %path.display(),
                    discarded_bytes = original_len - complete_len,
                    "discarding incomplete sideband Timeline tail before append"
                );
                file.set_len(complete_len)?;
            }

            match last_line.as_deref() {
                Some(last_line) => {
                    let last: chat_state::SidebandEvent = serde_json::from_slice(last_line)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    if last.seq == event_seq {
                        if last_line != &line[..line.len() - 1] {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "sideband Timeline seq {event_seq} conflicts with persisted tail"
                                ),
                            ));
                        }
                        if matches!(durability, AppendDurability::Durable) {
                            Self::sync_file_durable(&file)?;
                            drop(file);
                            Self::sync_parent_directory(path)?;
                        }
                        return Ok(());
                    }
                    let expected = last.seq.checked_add(1).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "sideband Timeline sequence overflow",
                        )
                    })?;
                    if event_seq != expected {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "sideband Timeline append expected seq {expected}, received {event_seq}"
                            ),
                        ));
                    }
                }
                None if event_seq != 0 => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("empty sideband Timeline requires seq 0, received {event_seq}"),
                    ));
                }
                None => {}
            }

            file.seek(io::SeekFrom::End(0))?;
            file.write_all(&line)?;
            file.flush()?;
            if matches!(durability, AppendDurability::Durable) {
                Self::sync_file_durable(&file)?;
                drop(file);
                Self::sync_parent_directory(path)?;
            }
            Ok(())
        })();
        let _ = lock.unlock();
        result?;
        if matches!(durability, AppendDurability::Durable) && event_seq == 0 {
            // `create_dir_all` may have created both `sidebands/` and the UUID
            // directory. Persist each new directory entry outward so an
            // acknowledged request survives a power loss, not only the file
            // contents inside the innermost directory.
            Self::sync_parent_directory(parent)?;
            if let Some(sidebands_dir) = parent.parent() {
                Self::sync_parent_directory(sidebands_dir)?;
            }
        }
        Ok(())
    }

    /// Return the committed byte length and final complete JSONL record without
    /// scanning the whole ledger. The file is positioned arbitrarily on return.
    fn read_timeline_tail(file: &mut std::fs::File) -> io::Result<(u64, Option<Vec<u8>>)> {
        let file_len = file.metadata()?.len();
        if file_len == 0 {
            return Ok((0, None));
        }

        file.seek(io::SeekFrom::Start(file_len - 1))?;
        let mut last_byte = [0u8; 1];
        file.read_exact(&mut last_byte)?;
        let complete_len = if last_byte[0] == b'\n' {
            file_len
        } else {
            Self::find_previous_newline(file, file_len)?.map_or(0, |position| position + 1)
        };
        if complete_len == 0 {
            return Ok((0, None));
        }

        let line_end = complete_len - 1;
        let line_start =
            Self::find_previous_newline(file, line_end)?.map_or(0, |position| position + 1);
        let line_len = usize::try_from(line_end - line_start).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Timeline tail record is too large",
            )
        })?;
        let mut line = vec![0u8; line_len];
        file.seek(io::SeekFrom::Start(line_start))?;
        file.read_exact(&mut line)?;
        Ok((complete_len, Some(line)))
    }

    fn find_previous_newline(
        file: &mut std::fs::File,
        mut end_exclusive: u64,
    ) -> io::Result<Option<u64>> {
        const CHUNK: usize = 8 * 1024;
        let mut buffer = [0u8; CHUNK];
        while end_exclusive > 0 {
            let read_len =
                usize::try_from(end_exclusive.min(CHUNK as u64)).expect("bounded by CHUNK");
            let start = end_exclusive - read_len as u64;
            file.seek(io::SeekFrom::Start(start))?;
            file.read_exact(&mut buffer[..read_len])?;
            if let Some(index) = buffer[..read_len].iter().rposition(|byte| *byte == b'\n') {
                return Ok(Some(start + index as u64));
            }
            end_exclusive = start;
        }
        Ok(None)
    }
    /// Append one JSONL record, healing a torn tail before writing.
    ///
    /// Appends are not crash-atomic: a process kill / `ENOSPC` mid-`write_all`
    /// (e.g. the auto-update leader relaunch aborting a persistence actor
    /// mid-append) leaves the file ending in a *partial* record with no
    /// trailing newline. Because append failures are logged-and-continued by
    /// the persistence actor, a plain `O_APPEND` write of the next record
    /// would concatenate it onto that partial line, producing a merged line
    /// that fails to parse (``expected `,` or `}` at line 1 column N``) and —
    /// before the readers became corruption-tolerant — bricked session resume.
    ///
    /// Before writing, check the last byte: if it isn't `\n`, prepend one so
    /// the torn record is terminated as its own (single) corrupt line. This
    /// bounds the damage of any torn write to exactly one cache record. The
    /// canonical Timeline reader remains fail-closed and never reads this cache.
    async fn sync_file_path_durable(path: PathBuf) -> io::Result<()> {
        tokio::task::spawn_blocking(move || {
            let file = OpenOptions::new().read(true).open(&path)?;
            Self::sync_file_durable(&file)
        })
        .await
        .map_err(io::Error::other)?
    }
    fn append_jsonl_line_sync(
        path: &Path,
        line: Vec<u8>,
        durability: AppendDurability,
    ) -> io::Result<()> {
        Self::append_jsonl_line_sync_with(path, line, durability, Self::sync_file_durable, || {
            Self::sync_parent_directory(path)
        })
    }
    fn append_jsonl_line_sync_with(
        path: &Path,
        mut line: Vec<u8>,
        durability: AppendDurability,
        mut sync_file: impl FnMut(&std::fs::File) -> io::Result<()>,
        mut sync_parent: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        debug_assert!(line.ends_with(b"\n"), "JSONL record must end with \\n");
        let lock = Self::lock_append(path)?;
        let result = (|| {
            let mut file = OpenOptions::new()
                .read(true)
                .create(true)
                .append(true)
                .open(path)?;
            let len = file.metadata()?.len();
            if len > 0 {
                file.seek(io::SeekFrom::Start(len - 1))?;
                let mut last = [0u8; 1];
                file.read_exact(&mut last)?;
                if last[0] != b'\n' {
                    tracing::warn!(
                        path = %path.display(),
                        "jsonl file has a torn trailing line (previous append crashed mid-write?); terminating it before appending"
                    );
                    line.insert(0, b'\n');
                }
            }
            file.write_all(&line)?;
            file.flush()?;
            if matches!(durability, AppendDurability::Durable) {
                sync_file(&file)?;
                drop(file);
                sync_parent()?;
            } else {
                drop(file);
            }
            Ok(())
        })();
        let _ = lock.unlock();
        result
    }
    /// Lock tail healing, append, and barriers through `<target>.jsonl.lock`.
    /// Full-file [`Self::write_jsonl`] atomic-rename rewrites bypass this append-only lock.
    fn lock_append(path: &Path) -> io::Result<std::fs::File> {
        Self::lock_append_with_timeout(path, std::time::Duration::from_secs(5))
    }

    fn lock_append_with_timeout(
        path: &Path,
        timeout: std::time::Duration,
    ) -> io::Result<std::fs::File> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("jsonl.lock"))?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match lock.try_lock_exclusive() {
                Ok(()) => return Ok(lock),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!(
                                "timed out waiting for JSONL append lock: {}",
                                path.with_extension("jsonl.lock").display()
                            ),
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => return Err(error),
            }
        }
    }
    fn sync_file_durable(file: &std::fs::File) -> io::Result<()> {
        super::sync_file_durable(file)
    }
    #[cfg(unix)]
    fn sync_parent_directory(path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "update has no parent"))?;
        std::fs::File::open(parent)?.sync_all()
    }
    #[cfg(windows)]
    fn sync_parent_directory(_path: &Path) -> io::Result<()> {
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    fn sync_parent_directory(_path: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "durable directory sync is unsupported on this platform",
        ))
    }

    /// Make every staged fork entry durable before the directory rename that
    /// publishes it. Files are synced before their containing directories so
    /// the published tree never depends on dirty child entries.
    fn sync_staging_tree(path: &Path) -> io::Result<()> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                Self::sync_staging_tree(&entry.path())?;
            } else if file_type.is_file() {
                let file = std::fs::File::open(entry.path())?;
                Self::sync_file_durable(&file)?;
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "unsupported entry in fork staging tree: {}",
                        entry.path().display()
                    ),
                ));
            }
        }
        #[cfg(unix)]
        std::fs::File::open(path)?.sync_all()?;
        Ok(())
    }

    /// Publish a completely built session directory at one no-replace commit
    /// point. Fresh initialization and fork copy intentionally share this path.
    fn publish_staged_session<T>(
        staging_dir: &Path,
        target_dir: &Path,
        build_result: io::Result<T>,
    ) -> io::Result<T> {
        match build_result {
            Ok(result) => {
                if let Err(error) = Self::publish_staged_directory(staging_dir, target_dir) {
                    if let Err(cleanup_error) = std::fs::remove_dir_all(staging_dir) {
                        tracing::warn!(
                            path = %staging_dir.display(),
                            %cleanup_error,
                            "failed to clean session staging directory after publication failure"
                        );
                    }
                    return Err(error);
                }
                Ok(result)
            }
            Err(error) => {
                if let Err(cleanup_error) = std::fs::remove_dir_all(staging_dir) {
                    tracing::warn!(
                        path = %staging_dir.display(),
                        %cleanup_error,
                        "failed to clean unpublished session staging directory"
                    );
                }
                Err(error)
            }
        }
    }

    /// Commit a fully built session directory. This is the single publication
    /// primitive shared by session creation, fork copy, and session import.
    pub(crate) fn publish_staged_directory(
        staging_dir: &Path,
        target_dir: &Path,
    ) -> io::Result<()> {
        Self::sync_staging_tree(staging_dir)?;
        super::rename_no_replace(staging_dir, target_dir)?;
        // Rename is the commit point. A parent-sync failure cannot be reported
        // as uncommitted because retry would collide with the visible target.
        if let Err(error) = Self::sync_parent_directory(target_dir) {
            tracing::warn!(
                path = %target_dir.display(),
                %error,
                "session published but parent-directory sync failed"
            );
        }
        Ok(())
    }
    /// Write a full JSONL file (rewriting all items), crash-atomically: serialize
    /// to a temp file then rename over the target, so a crash / `ENOSPC` mid-write
    /// can't truncate the existing file (e.g. lose `rewind_points.jsonl` history).
    async fn write_jsonl<T: serde::Serialize>(&self, path: PathBuf, items: &[T]) -> io::Result<()> {
        super::write_jsonl_atomic_async(&path, items).await
    }
    fn read_jsonl<T: serde::de::DeserializeOwned>(&self, path: PathBuf) -> io::Result<Vec<T>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = OpenOptions::new().read(true).open(&path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let mut items = Vec::new();
        for line in contents.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let item: T = serde_json::from_str(line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            items.push(item);
        }
        Ok(items)
    }

    /// Read every complete Timeline record. A final non-newline-terminated
    /// fragment was never committed and is ignored; every complete line is
    /// parsed strictly so interior corruption still fails closed.
    fn read_timeline(&self, path: PathBuf) -> io::Result<Vec<chat_state::TimelineEvent>> {
        super::read_timeline_file(&path)
    }
    /// Append a session update to the updates.jsonl file, wrapping it in an envelope with timestamp.
    pub(super) async fn append_update_to_file(
        &self,
        path: PathBuf,
        update: &super::SessionUpdate,
        durability: AppendDurability,
    ) -> io::Result<()> {
        #[cfg(test)]
        if let Some(append_probe) = &self.update_append_probe {
            append_probe(durability)?;
        }
        let envelope = SessionUpdateEnvelope::from_update(update)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut line = serde_json::to_vec(&envelope)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        line.push(b'\n');
        Self::append_jsonl_line_blocking(path, line, durability).await
    }
    async fn append_update_with_bookkeeping(
        &self,
        info: &Info,
        update: &super::SessionUpdate,
        durability: AppendDurability,
    ) -> Result<(), super::AppendUpdateError> {
        self.append_update_to_file(self.updates_file(info), update, durability)
            .await
            .map_err(super::AppendUpdateError::NotCommitted)?;
        self.apply_summary_patch(
            info,
            super::summary_write::SummaryPatch {
                record_activity: true,
                messages: Some(super::summary_write::CounterOp::Increment(1)),
                ..Default::default()
            },
        )
        .await
        .map_err(super::AppendUpdateError::Committed)
    }
    /// Read canonical envelopes from an `updates.jsonl` file.
    ///
    /// Uses direct string-to-typed deserialization (via `SessionUpdateEnvelope::from_str`)
    /// with a borrowing envelope and `&RawValue` to avoid intermediate `Value` allocation.
    ///
    /// Updates are display/replay data appended non-atomically, so a torn line (crashed or
    /// racing append) is skipped with a warning instead of failing the caller
    /// (session load, fork copy). The live replay path is already lenient;
    /// this keeps the fork path from bricking on the same corruption.
    fn read_updates_jsonl(&self, path: PathBuf) -> io::Result<Vec<super::SessionUpdate>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let contents = std::fs::read(&path)?;
        let mut skipped_lines: usize = 0;
        let mut updates = Vec::new();
        for line in contents.split(|b| *b == b'\n') {
            let line = line.trim_ascii();
            if line.is_empty() {
                continue;
            }
            let parsed = std::str::from_utf8(line)
                .map_err(|e| e.to_string())
                .and_then(|s| SessionUpdateEnvelope::from_str(s).map_err(|e| e.to_string()));
            match parsed {
                Ok(update) => updates.push(update),
                Err(error) => {
                    skipped_lines += 1;
                    if skipped_lines == 1 {
                        tracing::warn!(
                            error = %error,
                            path = %path.display(),
                            "skipping unparseable updates.jsonl line (torn append?)"
                        );
                    }
                }
            }
        }
        if skipped_lines > 0 {
            tracing::warn!(
                skipped = skipped_lines,
                loaded = updates.len(),
                path = %path.display(),
                "skipped unparseable session update lines"
            );
        }
        Ok(updates)
    }
    /// Write summary to disk atomically (sync version for `spawn_blocking`).
    ///
    /// A plain `std::fs::write` truncates before writing, so a concurrent reader
    /// may see an empty file. Temp-file + rename avoids this.
    fn write_summary_sync(&self, info: &Info, summary: &Summary) -> io::Result<()> {
        let summary_path = self.summary_file(info);
        let bytes = serde_json::to_vec_pretty(summary)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        super::write_bytes_atomic(&summary_path, &bytes)
    }
    pub(crate) fn read_summary_sync(&self, info: &Info) -> io::Result<Summary> {
        let path = self.summary_file(info);
        let bytes = std::fs::read(&path)?;
        if bytes.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("summary.json is empty (0 bytes): {}", path.display()),
            ));
        }
        let summary = serde_json::from_slice::<Summary>(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        summary.validate_current_format()?;
        Ok(summary)
    }
    fn read_optional_json_sync<T: serde::de::DeserializeOwned>(
        &self,
        path: &Path,
    ) -> io::Result<Option<T>> {
        if !path.exists() {
            return Ok(None);
        }
        match std::fs::read_to_string(path) {
            Ok(s) if s.trim().is_empty() => Ok(None),
            Ok(s) => match serde_json::from_str::<T>(&s) {
                Ok(v) => Ok(Some(v)),
                Err(e) => {
                    tracing::warn!(?e, "failed parsing json; returning None");
                    Ok(None)
                }
            },
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(?e, "failed reading json; returning None");
                }
                Ok(None)
            }
        }
    }

    fn load_workflow_runs_sync(
        &self,
        info: &Info,
        timeline: &Timeline,
    ) -> io::Result<Vec<crate::session::workflow::store::RestoredWorkflowRun>> {
        use crate::session::workflow::store::{
            MAX_RESTORED_WORKFLOW_RUNS, MAX_WORKFLOW_ARGS_BYTES, MAX_WORKFLOW_MANIFEST_BYTES,
            read_bounded_nofollow,
        };
        let workflows_dir = self.workflows_dir(info);
        match std::fs::symlink_metadata(&workflows_dir) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Ok(Vec::new());
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error),
        };
        let mut run_ids = timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Workflow(chat_state::WorkflowEvent::Spawned {
                    run_id,
                    ..
                }) => Some(run_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if run_ids.len() > MAX_RESTORED_WORKFLOW_RUNS {
            let excess = run_ids.len() - MAX_RESTORED_WORKFLOW_RUNS;
            run_ids.drain(..excess);
            tracing::warn!(
                session_id = %info.id,
                limit = MAX_RESTORED_WORKFLOW_RUNS,
                "workflow restore cap reached; restoring the most recent Timeline-owned runs"
            );
        }
        let mut restored = Vec::new();
        for run_id in run_ids {
            let run_dir = workflows_dir.join(&run_id);
            let Ok(run_meta) = std::fs::symlink_metadata(&run_dir) else {
                continue;
            };
            if run_meta.file_type().is_symlink() || !run_meta.is_dir() {
                continue;
            }
            match std::fs::symlink_metadata(run_dir.join("cleared")) {
                Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => continue,
                Ok(_) => {
                    tracing::warn!(%run_id, "skipping Workflow with invalid cleared marker");
                    continue;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            let manifest_path = run_dir.join("state.json");
            let manifest = match read_bounded_nofollow(&manifest_path, MAX_WORKFLOW_MANIFEST_BYTES)
                .and_then(|bytes| {
                    serde_json::from_slice::<crate::session::workflow::store::WorkflowRunManifest>(
                        &bytes,
                    )
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
                }) {
                Ok(manifest) => manifest,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    tracing::warn!(path = %manifest_path.display(), %error, "skipping invalid workflow manifest");
                    continue;
                }
            };
            if manifest.version != crate::session::workflow::store::WORKFLOW_RUN_MANIFEST_VERSION
                || crate::session::workflow::store::validate_run_id(&manifest.state.run_id).is_err()
                || manifest.state.run_id != run_id
            {
                tracing::warn!(path = %manifest_path.display(), "skipping unsupported or mismatched workflow manifest");
                continue;
            }
            let script_path = crate::session::workflow::store::script_revision_path(
                &run_dir,
                manifest.script_revision,
            );
            let script = match read_bounded_nofollow(
                &script_path,
                crate::session::workflow::registry::MAX_WORKFLOW_SOURCE_BYTES,
            )
            .and_then(|bytes| {
                String::from_utf8(bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
            }) {
                Ok(script) => script,
                Err(error) => {
                    tracing::warn!(path = %script_path.display(), %error, "skipping workflow with missing immutable script");
                    continue;
                }
            };
            let args_path = run_dir.join("args.json");
            let args = match read_bounded_nofollow(&args_path, MAX_WORKFLOW_ARGS_BYTES).and_then(
                |bytes| {
                    serde_json::from_slice(&bytes)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
                },
            ) {
                Ok(args) => args,
                Err(error) => {
                    tracing::warn!(path = %args_path.display(), %error, "skipping workflow with missing immutable args");
                    continue;
                }
            };
            restored.push(crate::session::workflow::store::RestoredWorkflowRun {
                manifest,
                script,
                args,
            });
        }
        Ok(restored)
    }
    /// Apply a typed [`SummaryPatch`](super::summary_write::SummaryPatch) to
    /// this session's `summary.json` under an exclusive sidecar lock, so the
    /// read-modify-write serializes against every other writer (including a
    /// second persistence actor on reconnect, or another process). This is the
    /// only path live sessions use to mutate the summary.
    pub(crate) async fn apply_summary_patch(
        &self,
        info: &Info,
        patch: super::summary_write::SummaryPatch,
    ) -> io::Result<()> {
        self.apply_summary_patch_reporting(info, patch).await?;
        Ok(())
    }

    /// Create a session using a fully prepared Summary. Summary and mandatory
    /// Timeline are built and synced in staging, then the directory is
    /// published with one atomic no-replace rename.
    pub(crate) async fn init_session_with_summary(
        &self,
        info: &Info,
        summary: Summary,
        initial_surface: Vec<ConversationItem>,
        initial_prompt_blobs: crate::session::persistence::ImmutablePromptBlobs,
        initial_facts: Vec<chat_state::TimelineEventKind>,
    ) -> io::Result<(Summary, Vec<chat_state::TimelineEvent>)> {
        let target_dir = self.session_dir(info);
        if target_dir.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("session already exists: {}", target_dir.display()),
            ));
        }
        let parent = target_dir.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "session target has no parent")
        })?;
        std::fs::create_dir_all(parent)?;
        let staging_dir = parent.join(format!(
            ".{}.{}.staging",
            info.id,
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir(&staging_dir)?;
        let staging = Self::with_explicit_session_dir(staging_dir.clone());
        let build_result = (|| {
            crate::session::persistence::write_initial_prompt_blobs(
                &initial_surface,
                &staging_dir,
                &initial_prompt_blobs,
            )?;
            let mut timeline = Timeline::from_seed(initial_surface)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            for fact in initial_facts {
                timeline
                    .record(fact)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            }
            let mut timeline_bytes = Vec::new();
            for event in timeline.events() {
                serde_json::to_writer(&mut timeline_bytes, event)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                timeline_bytes.push(b'\n');
            }
            super::write_bytes_atomic(&staging.timeline_file(info), &timeline_bytes)?;
            staging.write_summary_sync(info, &summary)?;
            Ok((summary, timeline.events().to_vec()))
        })();
        Self::publish_staged_session(&staging_dir, &target_dir, build_result)
    }
    /// Like [`Self::apply_summary_patch`], but returns whether a
    /// a newer canonical title projection was applied (see [`Summary::apply_patch`]).
    async fn apply_summary_patch_reporting(
        &self,
        info: &Info,
        patch: super::summary_write::SummaryPatch,
    ) -> io::Result<bool> {
        let summary_path = self.summary_file(info);
        let lock_path = self.summary_lock_file(info);
        tokio::task::spawn_blocking(move || {
            super::summary_write::apply_patch_locked(&summary_path, &lock_path, &patch)
        })
        .await
        .map_err(io::Error::other)?
    }

    async fn reconcile_session_title_projection(
        &self,
        info: &Info,
        mut summary: Summary,
        timeline: &Timeline,
    ) -> io::Result<Summary> {
        let Some((seq, title)) = timeline.session_title() else {
            if summary.title.is_some()
                || summary.title_source.is_some()
                || summary.title_event_seq.is_some()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "summary contains a title projection without a canonical session/title event",
                ));
            }
            return Ok(summary);
        };
        if summary.title_event_seq == Some(seq.get())
            && summary.title.as_deref() == Some(title.title.as_str())
            && summary.title_source.as_ref() == Some(&title.source)
        {
            return Ok(summary);
        }
        if summary
            .title_event_seq
            .is_some_and(|projected| projected >= seq.get())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "summary title projection conflicts with the canonical session/title event",
            ));
        }
        let applied = self
            .repair_session_title_projection(
                info,
                seq.get(),
                title.title.clone(),
                title.source.clone(),
            )
            .await?;
        if !applied {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "canonical session/title projection did not advance summary",
            ));
        }
        summary.title = Some(title.title.clone());
        summary.title_source = Some(title.source.clone());
        summary.title_event_seq = Some(seq.get());
        Ok(summary)
    }
}
/// Transform session ID in a SessionUpdate
fn transform_session_id_in_update(
    update: super::SessionUpdate,
    new_id: &acp::SessionId,
) -> super::SessionUpdate {
    match update {
        super::SessionUpdate::Acp(notification) => {
            let mut inner = (*notification).clone();
            inner.session_id = new_id.clone();
            super::SessionUpdate::Acp(Box::new(inner))
        }
        super::SessionUpdate::Grow(notification) => {
            let mut inner = (*notification).clone();
            inner.session_id = new_id.clone();
            super::SessionUpdate::Grow(Box::new(inner))
        }
    }
}
fn is_source_bound_projection_update(update: &super::SessionUpdate) -> bool {
    matches!(
        update,
        super::SessionUpdate::Grow(notification)
            if matches!(
                &notification.update,
                crate::extensions::notification::SessionUpdate::WorkflowUpdated { .. }
                    | crate::extensions::notification::SessionUpdate::GoalUpdated { .. }
                    | crate::extensions::notification::SessionUpdate::SubagentSpawned { .. }
                    | crate::extensions::notification::SessionUpdate::SubagentProgress { .. }
                    | crate::extensions::notification::SessionUpdate::SubagentFinished { .. }
            )
    )
}
/// Apply fork-safety filtering to the selected Surface before copying.
///
/// 1. Removes synthetic user messages (doom loop warnings, compaction metadata)
/// 2. Truncates at the last complete turn boundary. A complete turn runs
///    `User → Assistant → (matching ToolResults)`, possibly across multiple
///    Assistant/ToolResult cycles, with `Reasoning` siblings interleaved
///    throughout (real grow-build turns emit `[reasoning, assistant, tool
///    results, reasoning, assistant, ...]`). The scan treats everything
///    except `Assistant` as transparent and only advances the boundary when an
///    Assistant closes every tool call it made, so it survives reasoning
///    interleaving. Trailing incomplete turns — including a trailing
///    user/reasoning tail with no matching assistant response (e.g. the
///    in-flight `/goal` turn) — are removed so the child never sees an
///    incoherent partial turn.
///
/// Also used by the live parent-chat fork path (summarized fallback only — the
/// verbatim mirror path keeps items unfiltered to preserve cached synthetics).
///
/// NOTE: this is one of two reasoning-aware turn-boundary scanners that must move
/// together — the other is `count_complete_turns` in
/// `agent/subagent/resolution/context.rs` (it counts turns in the same
/// filtered list during summarization). Keep their notions of a "complete turn"
/// in sync if the turn item model changes.
pub(crate) fn fork_filter_surface(items: &mut Vec<ConversationItem>) {
    items.retain(|item| match item {
        ConversationItem::User(u) => u.synthetic_reason.is_none(),
        _ => true,
    });
    let mut last_complete_end = 0;
    let mut i = 0;
    while i < items.len() {
        match &items[i] {
            ConversationItem::System(_) => {
                last_complete_end = i + 1;
                i += 1;
            }
            ConversationItem::Assistant(asst) => {
                let expected: std::collections::HashSet<&str> =
                    asst.tool_calls.iter().map(|tc| tc.id.as_ref()).collect();
                let mut found = std::collections::HashSet::new();
                let mut j = i + 1;
                while j < items.len() {
                    match &items[j] {
                        ConversationItem::ToolResult(tr) => {
                            if expected.contains(tr.tool_call_id.as_str()) {
                                found.insert(tr.tool_call_id.as_str());
                            }
                            j += 1;
                        }
                        ConversationItem::Reasoning(_) | ConversationItem::BackendToolCall(_) => {
                            j += 1;
                        }
                        _ => break,
                    }
                }
                if found == expected {
                    last_complete_end = j;
                    i = j;
                } else {
                    break;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    items.truncate(last_complete_end);
}
fn conversation_truncate_after_prompt(
    conversation: &[ConversationItem],
    target_prompt_index: usize,
) -> usize {
    conversation_truncate_for_prompt(conversation, target_prompt_index + 1)
}

fn copy_referenced_prompt_blobs(
    surface: &[ConversationItem],
    source_session_dir: &Path,
    staging_session_dir: &Path,
) -> io::Result<usize> {
    let hashes = crate::session::persistence::referenced_prompt_blob_hashes(surface)?;
    for hash in &hashes {
        let bytes =
            crate::session::persistence::verified_prompt_blob_bytes(source_session_dir, hash)?;
        let target = staging_session_dir
            .join("prompts")
            .join(format!("{hash}.txt"));
        crate::session::persistence::write_immutable_blob(&target, &bytes)?;
    }
    Ok(hashes.len())
}

impl JsonlStorageAdapter {
    /// Fully synchronous version of `copy_session_data` for use inside
    /// `spawn_blocking`. Identical logic but uses `std::fs::write` instead
    /// of `tokio::fs::write`, so the entire copy runs on a blocking thread
    /// without nesting `spawn_blocking` calls.
    pub fn copy_session_data_sync(
        &self,
        source_info: &Info,
        target_info: &Info,
        options: super::CopySessionOptions,
    ) -> io::Result<super::CopySessionResult> {
        let target_dir = self.session_dir(target_info);
        if target_dir.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("fork target already exists: {}", target_dir.display()),
            ));
        }
        let parent = target_dir.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "fork target has no parent")
        })?;
        std::fs::create_dir_all(parent)?;
        let staging_dir = parent.join(format!(
            ".{}.{}.staging",
            target_info.id,
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir(&staging_dir)?;
        let target_storage = Self::with_explicit_session_dir(staging_dir.clone());
        let build_result = (|| {
            let source_summary = self.read_summary_sync(source_info)?;
            let source_events = self.read_timeline(self.timeline_file(source_info))?;
            let source_timeline = Timeline::from_events(source_events)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let mut surface_to_copy = source_timeline.surface().to_vec();
            let mut updates_to_copy: Vec<super::SessionUpdate> =
                self.read_updates_jsonl(self.updates_file(source_info))?;
            if let Some(target_idx) = options.target_prompt_index {
                updates_to_copy = super::filter_rewind_updates(updates_to_copy);
                updates_to_copy.truncate(updates_truncate_for_prompt(&updates_to_copy, target_idx));
                surface_to_copy.truncate(conversation_truncate_after_prompt(
                    &surface_to_copy,
                    target_idx,
                ));
            }
            if options.fork_filter {
                fork_filter_surface(&mut surface_to_copy);
                updates_to_copy.clear();
            } else {
                updates_to_copy.retain(|update| !is_source_bound_projection_update(update));
            }
            for item in &mut surface_to_copy {
                if let ConversationItem::User(user) = item {
                    user.prompt_index = None;
                }
            }
            if !options.skip_cwd_transform && source_info.cwd != target_info.cwd {
                transform_conversation_cwd(
                    &mut surface_to_copy,
                    &source_info.cwd,
                    &target_info.cwd,
                );
            }
            let prompt_blobs_copied = copy_referenced_prompt_blobs(
                &surface_to_copy,
                &self.session_dir(source_info),
                &target_storage.session_dir(target_info),
            )?;
            if options.strip_reasoning {
                surface_to_copy =
                    chat_state::compaction_utils::strip_reasoning_blocks(surface_to_copy);
            }
            let surface_items_copied = surface_to_copy.len();
            let cwd_switch_bookkeeping_generation = surface_to_copy
                .iter()
                .filter_map(ConversationItem::working_directory_switch_generation)
                .max()
                .unwrap_or(0);
            let num_messages = updates_to_copy.len();
            let target_model_id = options
                .new_model_id
                .map(acp::ModelId::new)
                .unwrap_or(source_summary.current_model_id);
            let target_summary = crate::session::persistence::Summary {
                info: target_info.clone(),
                cwd_generation: source_summary.cwd_generation,
                previous_cwd: source_summary.previous_cwd,
                pending_cwd_switch_reminder: None,
                cwd_switch_bookkeeping_generation,
                title: None,
                title_source: None,
                title_event_seq: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                num_messages,
                current_model_id: target_model_id,
                parent_session_id: options.parent_session_id,
                forked_at: Some(chrono::Utc::now()),
                session_format_version: SESSION_FORMAT_VERSION,
                prompt_display_cwd: options.prompt_display_cwd,
                session_kind: Some(options.session_kind.unwrap_or_else(|| "fork".to_string())),
                fork_context_source: options.fork_context_source,
                fork_parent_prompt_id: options.fork_parent_prompt_id,
                hidden: None,
                source_workspace_dir: options.source_workspace_dir,
                git_root_dir: None,
                git_remotes: Vec::new(),
                head_commit: source_summary.head_commit,
                head_branch: source_summary.head_branch,
                grow_home: crate::session::persistence::grow_home_string(),
                last_active_at: source_summary.last_active_at,
                worktree_label: source_summary.worktree_label,
                agent_name: source_summary.agent_name,
                sandbox_profile: source_summary.sandbox_profile,
                reasoning_effort: source_summary.reasoning_effort,
            };
            let summary_bytes = serde_json::to_vec_pretty(&target_summary)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            std::fs::write(target_storage.summary_file(target_info), summary_bytes)?;
            // A fork starts a new event lineage from the inherited surface. Source
            // replacement identities cannot be copied after truncation, filtering,
            // cwd transformation, or reasoning stripping.
            let mut fork_timeline = Timeline::from_seed(surface_to_copy.clone())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let control_event_seeded = if options.inherit_control {
                if let Some(mut control) =
                    crate::session::control::SessionControlSnapshot::latest_from_timeline(
                        source_timeline.events(),
                    )?
                {
                    // Runtime ownership never crosses a fork boundary. The child
                    // receives a new explicit control fact, not a copied parent
                    // snapshot or sidecar.
                    control.goal = None;
                    if matches!(
                        control.behavior.state,
                        crate::session::behavior::BehaviorState::Goal
                            | crate::session::behavior::BehaviorState::DeepResearch { .. }
                    ) {
                        control.behavior = crate::session::behavior::BehaviorSnapshot::normal();
                    }
                    control.control_revision = control.control_revision.saturating_add(1);
                    fork_timeline
                        .record(control.timeline_kind()?)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if let Some(announcement_state) =
                crate::session::announcement_state::AnnouncementState::latest_from_timeline(
                    source_timeline.events(),
                )?
            {
                // A fork inherits the dedup projection as a new fact in its own
                // lineage. No sidecar or copied parent event identity is involved.
                fork_timeline
                    .record(announcement_state.timeline_kind()?)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            }
            let mut timeline_content = Vec::new();
            for event in fork_timeline.events() {
                let mut line = serde_json::to_vec(event)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                line.push(b'\n');
                timeline_content.extend(line);
            }
            std::fs::write(target_storage.timeline_file(target_info), timeline_content)?;
            let transformed_updates: Vec<super::SessionUpdate> = updates_to_copy
                .into_iter()
                .map(|u| transform_session_id_in_update(u, &target_info.id))
                .collect();
            let mut updates_content = Vec::new();
            for update in &transformed_updates {
                let envelope = SessionUpdateEnvelope::from_update(update)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                let mut line = serde_json::to_vec(&envelope)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                line.push(b'\n');
                updates_content.extend(line);
            }
            std::fs::write(target_storage.updates_file(target_info), updates_content)?;
            Ok(super::CopySessionResult {
                surface_items_copied,
                updates_copied: num_messages,
                control_event_seeded,
                prompt_blobs_copied,
            })
        })();

        Self::publish_staged_session(&staging_dir, &target_dir, build_result)
    }
}
#[async_trait]
impl StorageAdapter for JsonlStorageAdapter {
    async fn init_session(&self, info: &Info, model_id: acp::ModelId) -> io::Result<Summary> {
        let dir = self.session_dir(info);
        std::fs::create_dir_all(&dir)?;
        let summary_path = self.summary_file(info);
        if Path::new(&summary_path).exists() {
            tracing::info!("Loading existing session from JSONL");
            let summary = self.read_summary_sync(info)?;
            let timeline = Timeline::from_events(self.read_timeline(self.timeline_file(info))?)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let _ = self.read_sideband_ledgers_sync(info, &timeline)?;
            Ok(summary)
        } else {
            tracing::info!("Creating new session in JSONL");
            let mut summary = Summary::new(info, model_id)?;
            summary.sandbox_profile = sandbox::configured_profile_name().map(String::from);
            // The summary is the publication marker. Commit the mandatory empty
            // ledger first so a visible session can never exist without its
            // sole causal source of truth.
            super::write_bytes_atomic(&self.timeline_file(info), &[])?;
            self.write_summary_sync(info, &summary)?;
            Ok(summary)
        }
    }
    async fn repair_session_title_projection(
        &self,
        info: &Info,
        event_seq: u64,
        title: String,
        source: chat_state::SessionTitleSource,
    ) -> io::Result<bool> {
        self.apply_summary_patch_reporting(
            info,
            super::summary_write::SummaryPatch {
                session_title: Some(super::summary_write::SessionTitleProjection {
                    event_seq,
                    title,
                    source,
                }),
                ..Default::default()
            },
        )
        .await
    }
    async fn append_update(&self, info: &Info, update: &super::SessionUpdate) -> io::Result<()> {
        self.append_update_commit_aware(info, update)
            .await
            .map_err(super::AppendUpdateError::into_io_error)
    }
    async fn append_update_commit_aware(
        &self,
        info: &Info,
        update: &super::SessionUpdate,
    ) -> Result<(), super::AppendUpdateError> {
        self.append_update_with_bookkeeping(info, update, AppendDurability::Buffered)
            .await
    }
    async fn append_update_durable_commit_aware(
        &self,
        info: &Info,
        update: &super::SessionUpdate,
    ) -> Result<(), super::AppendUpdateError> {
        self.append_update_with_bookkeeping(info, update, AppendDurability::Durable)
            .await
    }
    async fn append_timeline_event(
        &self,
        info: &Info,
        event: &chat_state::TimelineEvent,
    ) -> io::Result<()> {
        self.append_timeline_event_with_bookkeeping(info, event, AppendDurability::Buffered)
            .await
    }
    async fn append_timeline_event_durable(
        &self,
        info: &Info,
        event: &chat_state::TimelineEvent,
    ) -> io::Result<()> {
        self.append_timeline_event_with_bookkeeping(info, event, AppendDurability::Durable)
            .await
    }
    async fn append_sideband_event_durable(
        &self,
        info: &Info,
        event: &chat_state::SidebandEvent,
    ) -> io::Result<()> {
        let path = self.sideband_timeline_file(info, &event.sideband_id)?;
        Self::append_sideband_event_with_durability(path, event, AppendDurability::Durable).await
    }
    async fn update_current_model_and_agent(
        &self,
        info: &Info,
        model_id: &acp::ModelId,
        agent_name: Option<&str>,
        reasoning_effort: Option<Option<sampling_types::ReasoningEffort>>,
    ) -> io::Result<()> {
        self.apply_summary_patch(
            info,
            super::summary_write::SummaryPatch {
                model: Some(super::summary_write::ModelPatch {
                    model_id: model_id.clone(),
                    agent_name: agent_name.map(String::from),
                    reasoning_effort,
                }),
                ..Default::default()
            },
        )
        .await
    }
    async fn update_git_head(
        &self,
        info: &Info,
        commit: Option<String>,
        branch: Option<String>,
    ) -> io::Result<()> {
        self.apply_summary_patch(
            info,
            super::summary_write::SummaryPatch {
                git_head: Some(super::summary_write::GitHeadPatch { commit, branch }),
                ..Default::default()
            },
        )
        .await
    }
    async fn write_workflow_run_state(
        &self,
        info: &Info,
        manifest: &crate::session::workflow::store::WorkflowRunManifest,
    ) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(manifest)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let target = self.workflow_run_state_file(info, &manifest.state.run_id)?;
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
            if parent.join("cleared").is_file() {
                return Ok(());
            }
        }
        if target.is_file()
            && let Ok(existing) = tokio::fs::read(&target).await
            && let Ok(on_disk) = serde_json::from_slice::<
                crate::session::workflow::store::WorkflowRunManifest,
            >(&existing)
            && on_disk.state.run_id == manifest.state.run_id
            && on_disk.state.revision > manifest.state.revision
        {
            tracing::debug!(
                run_id = %manifest.state.run_id,
                on_disk_revision = on_disk.state.revision,
                incoming_revision = manifest.state.revision,
                "skipping stale workflow manifest write"
            );
            return Ok(());
        }
        let tmp = target.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            uuid::Uuid::now_v7().simple()
        ));
        tokio::fs::write(&tmp, json).await?;
        #[cfg(windows)]
        match tokio::fs::remove_file(&target).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(error);
            }
        }
        if let Err(error) = tokio::fs::rename(&tmp, &target).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(error);
        }
        Ok(())
    }
    async fn delete_workflow_run_state(&self, info: &Info, run_id: &str) -> io::Result<()> {
        let target = self.workflow_run_state_file(info, run_id)?;
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
            let cleared = parent.join("cleared");
            if !cleared.exists() {
                tokio::fs::write(cleared, []).await?;
            }
        }
        match tokio::fs::remove_file(target).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
    async fn load_session(&self, info: &Info) -> io::Result<PersistedData> {
        let summary = self.read_summary_sync(info)?;
        let timeline_events = self.read_timeline(self.timeline_file(info))?;
        let timeline = Timeline::from_events(timeline_events.clone())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        crate::session::persistence::verify_timeline_prompt_blobs(
            &self.session_dir(info),
            &timeline,
        )?;
        let _ = self.read_sideband_ledgers_sync(info, &timeline)?;
        let summary = self
            .reconcile_session_title_projection(info, summary, &timeline)
            .await?;
        let control_snapshot =
            crate::session::control::SessionControlSnapshot::latest_from_timeline(
                timeline.events(),
            )?;
        let updates = self.read_updates_jsonl(self.updates_file(info))?;
        let signals =
            crate::session::signals::SessionSignals::latest_from_timeline(timeline.events())?;
        let announcement_state =
            crate::session::announcement_state::AnnouncementState::latest_from_timeline(
                timeline.events(),
            )?;
        let workflow_runs = self.load_workflow_runs_sync(info, &timeline)?;
        let rewind_points = self.read_jsonl::<RewindPoint>(self.rewind_points_file(info))?;
        let result = PersistedData {
            summary,
            timeline_events,
            updates,
            control_snapshot,
            rewind_points,
            signals,
            announcement_state,
            workflow_runs,
        };
        tracing::info!(
            session_id = %info.id,
            timeline_events = result.timeline_events.len(),
            num_updates = result.updates.len(),
            has_signals = result.signals.is_some(),
            num_rewind_points = result.rewind_points.len(),
            session_format_version = result.summary.session_format_version,
            "Session data loaded successfully from JSONL"
        );
        Ok(result)
    }
    /// Resume path: loads everything except updates and rewind points. Rewind
    /// points can be huge (full file-content snapshots) and are needed only on an
    /// actual rewind, so they're deferred — loaded lazily by `FileStateTracker`.
    async fn load_session_without_updates(
        &self,
        info: &Info,
    ) -> io::Result<super::PersistedDataLight> {
        tracing::info!("Loading session data (without updates) from JSONL");
        let summary = self.read_summary_sync(info)?;
        let timeline_events = self.read_timeline(self.timeline_file(info))?;
        let timeline = Timeline::from_events(timeline_events.clone())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        crate::session::persistence::verify_timeline_prompt_blobs(
            &self.session_dir(info),
            &timeline,
        )?;
        let _ = self.read_sideband_ledgers_sync(info, &timeline)?;
        let summary = self
            .reconcile_session_title_projection(info, summary, &timeline)
            .await?;
        let control_snapshot =
            crate::session::control::SessionControlSnapshot::latest_from_timeline(
                timeline.events(),
            )?;
        let signals =
            crate::session::signals::SessionSignals::latest_from_timeline(timeline.events())?;
        let announcement_state =
            crate::session::announcement_state::AnnouncementState::latest_from_timeline(
                timeline.events(),
            )?;
        let workflow_runs = self.load_workflow_runs_sync(info, &timeline)?;
        let result = super::PersistedDataLight {
            summary,
            timeline_events,
            control_snapshot,
            signals,
            announcement_state,
            workflow_runs,
        };
        tracing::info!(
            session_id = %info.id,
            timeline_events = result.timeline_events.len(),
            has_signals = result.signals.is_some(),
            session_format_version = result.summary.session_format_version,
            "Session data loaded (without updates, rewind points deferred) from JSONL"
        );
        Ok(result)
    }
    async fn load_summary(&self, info: &Info) -> io::Result<Summary> {
        let info_clone = info.clone();
        let summary_handle = {
            let info = info_clone.clone();
            let adapter_clone = self.clone();
            tokio::task::spawn_blocking(move || {
                let adapter = adapter_clone;
                adapter.read_summary_sync(&info)
            })
        };
        let summary = summary_handle.await.map_err(io::Error::other)??;
        Ok(summary)
    }
    async fn list_sessions(&self, cwd: Option<&str>) -> io::Result<Vec<Summary>> {
        let adapter = self.clone();
        let cwd = cwd.map(str::to_owned);
        tokio::task::spawn_blocking(move || adapter.list_sessions_sync(cwd.as_deref()))
            .await
            .map_err(io::Error::other)?
    }
    async fn delete_session(&self, info: &Info) -> io::Result<()> {
        let dir = self.session_dir(info);
        match tokio::fs::remove_dir_all(&dir).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
    async fn append_rewind_point(&self, info: &Info, point: &RewindPoint) -> io::Result<()> {
        self.append_jsonl(self.rewind_points_file(info), point)
            .await
    }
    async fn load_rewind_points(&self, info: &Info) -> io::Result<Vec<RewindPoint>> {
        let info_clone = info.clone();
        let adapter_clone = self.clone();
        tokio::task::spawn_blocking(move || {
            let adapter = adapter_clone;
            let path = adapter.rewind_points_file(&info_clone);
            adapter.read_jsonl::<RewindPoint>(path)
        })
        .await
        .map_err(io::Error::other)?
    }
    async fn truncate_rewind_points_from(&self, info: &Info, from_index: usize) -> io::Result<()> {
        let points = self.load_rewind_points(info).await?;
        let filtered: Vec<RewindPoint> = points
            .into_iter()
            .filter(|p| p.prompt_index < from_index)
            .collect();
        self.write_jsonl(self.rewind_points_file(info), &filtered)
            .await
    }
    async fn merge_rewind_points_from(&self, info: &Info, target_index: usize) -> io::Result<()> {
        let points = self.load_rewind_points(info).await?;
        let merged = workspace::session::file_state::merge_rewind_points_from(points, target_index);
        self.write_jsonl(self.rewind_points_file(info), &merged)
            .await
    }
    async fn copy_session_data(
        &self,
        source_info: &Info,
        target_info: &Info,
        options: super::CopySessionOptions,
    ) -> io::Result<super::CopySessionResult> {
        let storage = self.clone();
        let source = source_info.clone();
        let target = target_info.clone();
        tokio::task::spawn_blocking(move || {
            storage.copy_session_data_sync(&source, &target, options)
        })
        .await
        .map_err(|e| io::Error::other(format!("spawn_blocking panicked: {e}")))?
    }
    async fn load_prompt_records(&self, info: &Info) -> io::Result<Vec<chat_state::PromptRecord>> {
        let timeline_path = self.timeline_file(info);
        tokio::task::spawn_blocking(move || {
            let events = super::read_timeline_file(&timeline_path)?;
            let timeline = chat_state::Timeline::from_events(events)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            Ok(timeline.prompt_records())
        })
        .await
        .map_err(io::Error::other)?
    }
    fn timeline_file_path(&self, info: &Info) -> Option<std::path::PathBuf> {
        Some(self.timeline_file(info))
    }
    fn updates_file_path(&self, info: &Info) -> Option<std::path::PathBuf> {
        Some(self.updates_file(info))
    }
    fn rewind_points_file_path(&self, info: &Info) -> Option<std::path::PathBuf> {
        Some(self.rewind_points_file(info))
    }
}
#[cfg(test)]
mod durable_tests;
#[cfg(test)]
mod tests;

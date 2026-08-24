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
    authority: std::sync::Arc<std::sync::OnceLock<super::ContainedDirectory>>,
    opened_sessions: std::sync::Arc<
        std::sync::Mutex<
            std::collections::BTreeMap<String, std::sync::Arc<super::ContainedDirectory>>,
        >,
    >,
    writer_leases: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, std::fs::File>>>,
    timeline_prefixes: std::sync::Arc<
        std::sync::Mutex<
            std::collections::BTreeMap<String, std::sync::Arc<std::sync::Mutex<Option<LedgerPrefix>>>>,
        >,
    >,
    #[cfg(test)]
    update_append_probe: Option<std::sync::Arc<AppendProbe>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LedgerPrefix {
    len: u64,
    hash: blake3::Hash,
}

/// One identity-checked session entity pinned to a single directory handle.
/// Every multi-file projection must use this object so a concurrent ambient
/// rename cannot splice Summary, Timeline, Sidebands, or blobs from different
/// sessions.
#[derive(Clone)]
pub(crate) struct OpenedSession {
    directory: std::sync::Arc<super::ContainedDirectory>,
    summary: Summary,
}

impl OpenedSession {
    pub(crate) fn directory(&self) -> &super::ContainedDirectory {
        &self.directory
    }

    pub(crate) fn directory_handle(&self) -> std::sync::Arc<super::ContainedDirectory> {
        self.directory.clone()
    }

    pub(crate) fn summary(&self) -> &Summary {
        &self.summary
    }

    pub(crate) fn timeline_events(&self) -> io::Result<Vec<chat_state::TimelineEvent>> {
        JsonlStorageAdapter::read_timeline_from_directory(&self.directory)
    }

    /// Read the exact committed `updates.jsonl` envelopes. Unlike replay,
    /// entity export is strict: a corrupt committed record makes the export
    /// fail instead of silently producing an incomplete mirror.
    pub(crate) fn update_envelopes(&self) -> io::Result<Vec<serde_json::Value>> {
        let path = self.directory.display_path().join(super::UPDATES_FILE);
        let file = match self.directory.open_regular(
            std::ffi::OsStr::new(super::UPDATES_FILE),
            "session updates ledger",
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let lines = super::CommittedJsonlLines::from_open_file_at(
            file,
            path,
            "session updates ledger",
            0,
        )?;
        let mut updates = Vec::new();
        for (index, line) in lines.enumerate() {
            let line = line?;
            let value: serde_json::Value = serde_json::from_slice(line.trim_ascii()).map_err(
                |error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid committed update {index}: {error}"),
                    )
                },
            )?;
            super::SessionUpdateEnvelope::from_value(value.clone()).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid committed update {index}: {error}"),
                )
            })?;
            updates.push(value);
        }
        Ok(updates)
    }

    pub(crate) fn sideband_ledgers(
        &self,
        parent_timeline_id: &str,
        parent: &chat_state::Timeline,
    ) -> io::Result<super::SidebandLedgers> {
        JsonlStorageAdapter::read_sideband_ledgers_from_directory(
            &self.directory,
            parent_timeline_id,
            parent,
        )
    }

    pub(crate) fn materialize_timeline(
        &self,
        timeline_id: &str,
    ) -> io::Result<chat_state::TimelineMaterialization> {
        let events = self.timeline_events()?;
        let last_seq = events.last().map(|event| event.seq.get()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "source Timeline is empty")
        })?;
        let timeline = Timeline::from_events(events)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        crate::session::persistence::verify_timeline_prompt_blobs_from_directory(
            &self.directory,
            &timeline,
        )?;
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
            authority: Default::default(),
            opened_sessions: Default::default(),
            writer_leases: Default::default(),
            timeline_prefixes: Default::default(),
            #[cfg(test)]
            update_append_probe: None,
        }
    }
    pub fn with_root(root_dir: PathBuf) -> Self {
        Self {
            dir_mode: SessionDirMode::FromRoot(root_dir),
            authority: Default::default(),
            opened_sessions: Default::default(),
            writer_leases: Default::default(),
            timeline_prefixes: Default::default(),
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
            authority: Default::default(),
            opened_sessions: Default::default(),
            writer_leases: Default::default(),
            timeline_prefixes: Default::default(),
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
            authority: Default::default(),
            opened_sessions: Default::default(),
            writer_leases: Default::default(),
            timeline_prefixes: Default::default(),
            update_append_probe: Some(std::sync::Arc::new(append_probe)),
        }
    }
    /// Read one committed ledger snapshot and derive its exact reference plus
    /// Surface. Fork/resume callers must never obtain these through separate
    /// reads because a concurrent append could make the content outrun its ref.
    #[cfg(test)]
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
        self.open_session(info)?.timeline_events()
    }

    /// Size of the currently pinned updates ledger. This exposes projection
    /// metadata without leaking an authoritative session path.
    pub fn updates_snapshot_len(&self, info: &Info) -> io::Result<u64> {
        self.open_session(info)?
            .directory()
            .open_regular(std::ffi::OsStr::new(super::UPDATES_FILE), "session updates ledger")?
            .metadata()
            .map(|metadata| metadata.len())
    }

    fn timeline_prefix_state(
        &self,
        info: &Info,
    ) -> io::Result<std::sync::Arc<std::sync::Mutex<Option<LedgerPrefix>>>> {
        let key = format!("{}\0{}", info.id, info.cwd);
        let mut prefixes = self
            .timeline_prefixes
            .lock()
            .map_err(|_| io::Error::other("Timeline prefix cache poisoned"))?;
        Ok(prefixes.entry(key).or_default().clone())
    }

    pub(crate) fn read_sideband_ledgers_sync(
        &self,
        info: &Info,
        parent: &chat_state::Timeline,
    ) -> io::Result<super::SidebandLedgers> {
        self.open_session(info)?
            .sideband_ledgers(&info.id.to_string(), parent)
    }

    pub(crate) fn read_sideband_ledgers_from_dir(
        session_dir: &Path,
        parent_timeline_id: &str,
        parent: &chat_state::Timeline,
    ) -> io::Result<super::SidebandLedgers> {
        let session = super::ContainedDirectory::open(
            session_dir,
            Path::new(""),
            "sideband session directory",
            false,
        )?;
        Self::read_sideband_ledgers_from_directory(
            &session,
            parent_timeline_id,
            parent,
        )
    }

    fn read_sideband_ledgers_from_directory(
        session: &super::ContainedDirectory,
        parent_timeline_id: &str,
        parent: &chat_state::Timeline,
    ) -> io::Result<super::SidebandLedgers> {
        let mut ledgers = super::SidebandLedgers::new();
        let sideband_ids = parent
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Sideband(spawn) => Some(spawn.sideband_id.clone()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        for sideband_id in sideband_ids {
            chat_state::validate_sideband_id(&sideband_id)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let sideband = match session.open_relative(
                &Path::new(super::SIDEBANDS_DIR).join(&sideband_id),
                "sideband entity directory",
                false,
            ) {
                Ok(sideband) => sideband,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let events = match super::read_committed_jsonl_from_directory::<chat_state::SidebandEvent>(
                &sideband,
                std::ffi::OsStr::new(super::TIMELINE_FILE),
                "sideband Timeline ledger",
                super::MAX_JSONL_ENTRY_BYTES,
            ) {
                Ok(events) => events,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
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

    async fn recover_interrupted_sidebands(
        &self,
        info: &Info,
        ledgers: &super::SidebandLedgers,
    ) -> io::Result<()> {
        for events in ledgers.values() {
            let mut timeline = chat_state::SidebandTimeline::from_events(events.clone())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            if timeline.is_ended() {
                continue;
            }
            let event = timeline
                .prepare(chat_state::SidebandEventKind::End(
                    chat_state::SidebandEnd {
                        outcome: chat_state::SidebandOutcome::Cancelled,
                        error: Some(
                            "process ended before sideband reached a terminal state".into(),
                        ),
                    },
                ))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let session = self.open_session(info)?.directory;
            Self::append_sideband_event_with_durability(
                session,
                &event,
                AppendDurability::Durable,
            )
            .await?;
        }
        Ok(())
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
        let events = self.open_session(info)?.timeline_events()?;
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
    fn ensure_cwd_marker(directory: &super::ContainedDirectory, cwd: &str) -> io::Result<()> {
        if cwd.len() as u64 > super::MAX_SESSION_SUMMARY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session cwd exceeds the storage metadata limit",
            ));
        }
        #[cfg(any(unix, windows))]
        match directory.write_atomic(std::ffi::OsStr::new(".cwd"), cwd.as_bytes(), true, false) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let existing = directory.read_bounded(
                    std::ffi::OsStr::new(".cwd"),
                    "session cwd marker",
                    super::MAX_SESSION_SUMMARY_BYTES,
                )?;
                if existing != cwd.as_bytes() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "session cwd marker conflicts with the requested cwd",
                    ));
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (directory, cwd);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "handle-relative session metadata is unsupported on this platform",
            ))
        }
    }
    fn ensure_storage_root(root: &Path) -> io::Result<()> {
        match std::fs::symlink_metadata(root) {
            Ok(_) => super::require_regular_directory(root, "session storage root"),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let parent = root.parent().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "session root has no parent")
                })?;
                let name = root.file_name().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "session root has no file name")
                })?;
                super::create_contained_dir_all(parent, Path::new(name), "session storage root")?;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
    pub(crate) fn ensure_session_parent(
        &self,
        info: &Info,
    ) -> io::Result<super::ContainedDirectory> {
        self.session_parent(info, true)
    }

    fn open_session_parent(&self, info: &Info) -> io::Result<super::ContainedDirectory> {
        self.session_parent(info, false)
    }

    fn session_parent(
        &self,
        info: &Info,
        create_missing: bool,
    ) -> io::Result<super::ContainedDirectory> {
        let authority = self.authority(create_missing)?;
        match &self.dir_mode {
            SessionDirMode::FromRoot(_) => {
                let encoded = crate::util::grow_home::encode_cwd_dirname(&info.cwd);
                let directory = authority.open_relative(
                    &Path::new("sessions").join(&encoded),
                    "session storage directory",
                    create_missing,
                )?;
                if encoded != urlencoding::encode(&info.cwd).as_ref() {
                    if create_missing {
                        Self::ensure_cwd_marker(&directory, &info.cwd)?;
                    } else {
                        let marker = directory.read_bounded(
                            std::ffi::OsStr::new(".cwd"),
                            "session cwd marker",
                            super::MAX_SESSION_SUMMARY_BYTES,
                        )?;
                        if marker != info.cwd.as_bytes() {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "session cwd marker conflicts with the requested cwd",
                            ));
                        }
                    }
                }
                Ok(directory)
            }
            SessionDirMode::Explicit(_) => Ok(authority),
        }
    }

    fn session_directory(
        &self,
        info: &Info,
        create_missing: bool,
    ) -> io::Result<super::ContainedDirectory> {
        let parent = if create_missing {
            self.ensure_session_parent(info)?
        } else {
            self.open_session_parent(info)?
        };
        let name = self
            .session_dir(info)
            .file_name()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "session path has no file name")
            })?
            .to_os_string();
        parent.open_relative(
            Path::new(&name),
            "session storage directory",
            create_missing,
        )
    }
    fn ensure_session_dir(&self, info: &Info) -> io::Result<PathBuf> {
        self.session_directory(info, true)
            .map(|directory| directory.display_path().to_path_buf())
    }
    pub(super) fn updates_file(&self, info: &Info) -> PathBuf {
        self.session_dir(info).join(super::UPDATES_FILE)
    }
    fn timeline_file(&self, info: &Info) -> PathBuf {
        self.session_dir(info).join(super::TIMELINE_FILE)
    }
    #[cfg(test)]
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
    fn rewind_points_file(&self, info: &Info) -> PathBuf {
        self.session_dir(info).join("rewind_points.jsonl")
    }
    /// Enumerate identity-checked session entities from the pinned storage
    /// authority. Directory names, cwd markers, and Summary identity are one
    /// indivisible admission boundary; callers never receive an ambient path.
    fn scan_opened_sessions(&self, cwd: Option<&str>) -> io::Result<Vec<OpenedSession>> {
        if !matches!(&self.dir_mode, SessionDirMode::FromRoot(_)) {
            return Ok(Vec::new());
        }
        let authority = self.authority(false)?;
        let sessions = match authority.open_relative(
            Path::new("sessions"),
            "sessions directory",
            false,
        ) {
            Ok(sessions) => sessions,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut opened = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for cwd_name in sessions.list_names()? {
            if cwd_name.to_string_lossy().starts_with('.') {
                continue;
            }
            let cwd_directory = match sessions.open_relative(
                Path::new(&cwd_name),
                "session cwd directory",
                false,
            ) {
                Ok(directory) => directory,
                Err(_) => continue,
            };
            for session_name in cwd_directory.list_names()? {
                if session_name.to_string_lossy().starts_with('.') {
                    continue;
                }
                let directory = match cwd_directory.open_relative(
                    Path::new(&session_name),
                    "session directory",
                    false,
                ) {
                    Ok(directory) => directory,
                    Err(_) => continue,
                };
                let summary = match Self::read_summary_from_directory(&directory) {
                    Ok(summary) => summary,
                    Err(_) => continue,
                };
                if Self::validate_physical_session_identity(
                    &cwd_name,
                    &cwd_directory,
                    &session_name,
                    &summary,
                )
                .is_err()
                    || summary.validate_current_format().is_err()
                    || summary.is_hidden()
                    || cwd.is_some_and(|expected| expected != summary.info.cwd)
                {
                    continue;
                }
                if !ids.insert(summary.info.id.to_string()) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("duplicate canonical session id {}", summary.info.id),
                    ));
                }
                let key = format!("{}\0{}", summary.info.id, summary.info.cwd);
                let candidate = std::sync::Arc::new(directory);
                let directory = self
                    .opened_sessions
                    .lock()
                    .map_err(|_| io::Error::other("session capability cache poisoned"))?
                    .entry(key)
                    .or_insert_with(|| candidate.clone())
                    .clone();
                let canonical_summary = Self::read_summary_from_directory(&directory)?;
                Self::validate_session_identity(&summary.info, &canonical_summary)?;
                let summary = canonical_summary;
                opened.push(OpenedSession { directory, summary });
            }
        }
        Ok(opened)
    }

    pub(crate) fn list_sessions_sync(&self, cwd: Option<&str>) -> io::Result<Vec<Summary>> {
        let mut summaries = self
            .scan_opened_sessions(cwd)?
            .into_iter()
            .map(|opened| opened.summary)
            .collect::<Vec<_>>();
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
        let mut summaries = self.list_sessions_sync(None)?;
        summaries.truncate(limit);
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
        directory: std::sync::Arc<super::ContainedDirectory>,
        prefix_state: std::sync::Arc<std::sync::Mutex<Option<LedgerPrefix>>>,
        event: &chat_state::TimelineEvent,
        durability: AppendDurability,
    ) -> io::Result<()> {
        let path = directory.display_path().join(super::TIMELINE_FILE);
        let event_seq = event.seq.get();
        let mut line = serde_json::to_vec(event)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        line.push(b'\n');
        tokio::task::spawn_blocking(move || {
            Self::append_timeline_line_in_directory_sync(
                &directory,
                &prefix_state,
                &path,
                line,
                event_seq,
                durability,
            )
        })
        .await
        .map_err(io::Error::other)?
    }

    async fn append_sideband_event_with_durability(
        session: std::sync::Arc<super::ContainedDirectory>,
        event: &chat_state::SidebandEvent,
        durability: AppendDurability,
    ) -> io::Result<()> {
        let event_seq = event.seq;
        let sideband_id = event.sideband_id.clone();
        let mut line = serde_json::to_vec(event)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        line.push(b'\n');
        tokio::task::spawn_blocking(move || {
            let parent = session.open_relative(
                &Path::new(super::SIDEBANDS_DIR).join(&sideband_id),
                "sideband Timeline directory",
                true,
            )?;
            let path = session
                .display_path()
                .join(super::SIDEBANDS_DIR)
                .join(&sideband_id)
                .join(super::TIMELINE_FILE);
            Self::append_sideband_line_sync(&parent, &path, line, event_seq, durability)
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
        self.ensure_writer_lease(info)?;
        let directory = self.open_session(info)?.directory;
        let prefix_state = self.timeline_prefix_state(info)?;
        Self::append_timeline_event_with_durability(directory, prefix_state, event, durability)
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
    fn append_timeline_line_in_directory_sync(
        directory: &super::ContainedDirectory,
        prefix_state: &std::sync::Mutex<Option<LedgerPrefix>>,
        path: &Path,
        line: Vec<u8>,
        event_seq: u64,
        durability: AppendDurability,
    ) -> io::Result<()> {
        debug_assert!(line.ends_with(b"\n"));
        Self::validate_jsonl_line_size(&line, "Timeline event")?;
        let name = std::ffi::OsStr::new(super::TIMELINE_FILE);
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (directory, name, line, event_seq, durability);
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "handle-relative Timeline storage is unsupported on this platform",
            ));
        }
        #[cfg(any(unix, windows))]
        let lock = Self::lock_append_contained(directory, name, path)?;
        #[cfg(any(unix, windows))]
        let result = (|| {
            let mut file = directory.open_read_write_create(name)?;
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

            let mut expected = prefix_state
                .lock()
                .map_err(|_| io::Error::other("Timeline prefix state poisoned"))?;
            let (actual_prefix, mut prefix_hasher) = Self::hash_timeline_prefix(
                &mut file,
                complete_len,
                expected.is_none(),
            )?;
            if let Some(expected_prefix) = *expected
                && expected_prefix != actual_prefix
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Timeline committed prefix changed: expected {} bytes/{}, found {} bytes/{}",
                        expected_prefix.len,
                        expected_prefix.hash.to_hex(),
                        actual_prefix.len,
                        actual_prefix.hash.to_hex(),
                    ),
                ));
            }
            *expected = Some(actual_prefix);

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
                            directory.sync()?;
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
            prefix_hasher.update(&line);
            *expected = Some(LedgerPrefix {
                len: complete_len.saturating_add(line.len() as u64),
                hash: prefix_hasher.finalize(),
            });
            if matches!(durability, AppendDurability::Durable) {
                Self::sync_file_durable(&file)?;
                drop(file);
                directory.sync()?;
            }
            Ok(())
        })();
        #[cfg(any(unix, windows))]
        {
            let _ = lock.unlock();
            result
        }
    }

    fn hash_timeline_prefix(
        file: &mut std::fs::File,
        complete_len: u64,
        validate_structure: bool,
    ) -> io::Result<(LedgerPrefix, blake3::Hasher)> {
        file.seek(io::SeekFrom::Start(0))?;
        let prefix_len = usize::try_from(complete_len).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Timeline prefix is too large to validate on this platform",
            )
        })?;
        let mut bytes = Vec::with_capacity(prefix_len);
        file.take(complete_len).read_to_end(&mut bytes)?;
        if bytes.len() != prefix_len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Timeline changed while its committed prefix was being validated",
            ));
        }
        if validate_structure {
            let mut events = Vec::new();
            for record in bytes.split_inclusive(|byte| *byte == b'\n') {
                let json = record.strip_suffix(b"\n").ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Timeline committed prefix contains an incomplete record",
                    )
                })?;
                events.push(
                    serde_json::from_slice::<chat_state::TimelineEvent>(json)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
                );
            }
            chat_state::Timeline::from_events(events)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        }
        let mut hasher = blake3::Hasher::new();
        hasher.update(&bytes);
        Ok((
            LedgerPrefix {
                len: complete_len,
                hash: hasher.finalize(),
            },
            hasher,
        ))
    }

    fn append_sideband_line_sync(
        directory: &super::ContainedDirectory,
        path: &Path,
        line: Vec<u8>,
        event_seq: u64,
        durability: AppendDurability,
    ) -> io::Result<()> {
        debug_assert!(line.ends_with(b"\n"));
        Self::validate_jsonl_line_size(&line, "sideband event")?;
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (directory, path, line, event_seq, durability);
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "handle-relative sideband storage is unsupported on this platform",
            ));
        }
        #[cfg(any(unix, windows))]
        let lock = Self::lock_append_contained(
            directory,
            std::ffi::OsStr::new(super::TIMELINE_FILE),
            path,
        )?;
        #[cfg(any(unix, windows))]
        let result = (|| {
            let mut file =
                directory.open_read_write_create(std::ffi::OsStr::new(super::TIMELINE_FILE))?;
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
                            directory.sync()?;
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
                directory.sync()?;
            }
            Ok(())
        })();
        #[cfg(any(unix, windows))]
        {
            let _ = lock.unlock();
            result
        }
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
        let line_len_u64 = line_end - line_start;
        if line_len_u64.saturating_add(1) > super::MAX_JSONL_ENTRY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Timeline tail record exceeds {} bytes",
                    super::MAX_JSONL_ENTRY_BYTES
                ),
            ));
        }
        let line_len = usize::try_from(line_len_u64).map_err(|_| {
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
    fn append_jsonl_line_sync(
        path: &Path,
        mut line: Vec<u8>,
        durability: AppendDurability,
    ) -> io::Result<()> {
        debug_assert!(line.ends_with(b"\n"), "JSONL record must end with \\n");
        Self::validate_jsonl_line_size(&line, "session update")?;
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "JSONL path has no parent")
        })?;
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "JSONL path has no file name")
        })?;
        let directory =
            super::ContainedDirectory::open(parent, Path::new(""), "JSONL directory", false)
                .map_err(|error| {
                    io::Error::new(error.kind(), format!("open JSONL directory: {error}"))
                })?;
        Self::append_jsonl_line_in_directory_sync(&directory, name, path, line, durability)
    }

    fn append_jsonl_line_in_directory_sync(
        directory: &super::ContainedDirectory,
        name: &std::ffi::OsStr,
        path: &Path,
        mut line: Vec<u8>,
        durability: AppendDurability,
    ) -> io::Result<()> {
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (directory, name, line, durability);
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "handle-relative JSONL storage is unsupported on this platform",
            ));
        }
        #[cfg(any(unix, windows))]
        let lock = Self::lock_append_contained(directory, name, path)
            .map_err(|error| io::Error::new(error.kind(), format!("lock JSONL append: {error}")))?;
        #[cfg(any(unix, windows))]
        let result = (|| {
            let mut file = directory.open_read_write_create(name).map_err(|error| {
                io::Error::new(error.kind(), format!("open JSONL ledger: {error}"))
            })?;
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
            file.seek(io::SeekFrom::End(0))?;
            file.write_all(&line)?;
            file.flush()?;
            if matches!(durability, AppendDurability::Durable) {
                Self::sync_file_durable(&file)?;
                drop(file);
                directory.sync()?;
            }
            Ok(())
        })();
        #[cfg(any(unix, windows))]
        {
            let _ = lock.unlock();
            result
        }
    }
    #[cfg(test)]
    fn append_jsonl_line_sync_with(
        path: &Path,
        mut line: Vec<u8>,
        durability: AppendDurability,
        mut sync_file: impl FnMut(&std::fs::File) -> io::Result<()>,
        mut sync_parent: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        debug_assert!(line.ends_with(b"\n"), "JSONL record must end with \\n");
        Self::validate_jsonl_line_size(&line, "session update")?;
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "JSONL path has no parent")
        })?;
        super::require_regular_directory(parent, "JSONL directory")?;
        let lock = Self::lock_append(path)?;
        let result = (|| {
            let mut file = super::open_read_write_create_nofollow(path)?;
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
            file.seek(io::SeekFrom::End(0))?;
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

    #[cfg(test)]
    fn append_timeline_line_sync(
        path: &Path,
        line: Vec<u8>,
        event_seq: u64,
        durability: AppendDurability,
    ) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Timeline path has no parent")
        })?;
        let directory =
            super::ContainedDirectory::open(parent, Path::new(""), "Timeline directory", false)?;
        let prefix_state = std::sync::Mutex::new(None);
        Self::append_timeline_line_in_directory_sync(
            &directory,
            &prefix_state,
            path,
            line,
            event_seq,
            durability,
        )
    }

    fn validate_jsonl_line_size(line: &[u8], description: &str) -> io::Result<()> {
        if line.len() as u64 > super::MAX_JSONL_ENTRY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{description} exceeds {} bytes",
                    super::MAX_JSONL_ENTRY_BYTES
                ),
            ));
        }
        Ok(())
    }
    /// Lock tail healing, append, and barriers through `<target>.jsonl.lock`.
    /// Full-file [`Self::write_jsonl`] atomic-rename rewrites bypass this append-only lock.
    #[cfg(test)]
    fn lock_append(path: &Path) -> io::Result<std::fs::File> {
        Self::lock_append_with_timeout(path, std::time::Duration::from_secs(5))
    }

    #[cfg(test)]
    fn lock_append_with_timeout(
        path: &Path,
        timeout: std::time::Duration,
    ) -> io::Result<std::fs::File> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "append path has no parent")
        })?;
        super::require_regular_directory(parent, "append ledger directory")?;
        let lock_path = path.with_extension("jsonl.lock");
        let lock = super::open_read_write_create_nofollow(&lock_path)?;
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

    #[cfg(any(unix, windows))]
    fn lock_append_contained(
        directory: &super::ContainedDirectory,
        target_name: &std::ffi::OsStr,
        display_path: &Path,
    ) -> io::Result<std::fs::File> {
        let mut lock_name = target_name.to_os_string();
        lock_name.push(".lock");
        let lock = directory.open_read_write_create(&lock_name)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match lock.try_lock_exclusive() {
                Ok(()) => return Ok(lock),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!(
                                "timed out waiting for JSONL append lock: {}",
                                display_path.display()
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
    /// Build a session beneath one pinned parent and publish that exact staged
    /// directory with a single handle-relative no-replace rename. Creation,
    /// fork, and import all use this transaction.
    pub(crate) fn build_and_publish_session<T>(
        parent: &super::ContainedDirectory,
        target_name: &std::ffi::OsStr,
        build: impl FnOnce(&super::ContainedDirectory) -> io::Result<T>,
    ) -> io::Result<T> {
        Self::build_and_publish_session_opened(parent, target_name, build)
            .map(|(result, _)| result)
    }

    fn build_and_publish_session_opened<T>(
        parent: &super::ContainedDirectory,
        target_name: &std::ffi::OsStr,
        build: impl FnOnce(&super::ContainedDirectory) -> io::Result<T>,
    ) -> io::Result<(T, super::ContainedDirectory)> {
        let staging_name = std::ffi::OsString::from(format!(
            ".{}.{}.staging",
            target_name.to_string_lossy(),
            uuid::Uuid::now_v7().simple()
        ));
        let staging = match parent.create_child(&staging_name, "session staging directory") {
            Ok(staging) => staging,
            Err(error) => {
                if let Err(cleanup_error) = parent.remove_tree_child(&staging_name)
                    && cleanup_error.kind() != io::ErrorKind::NotFound
                {
                    tracing::warn!(
                        path = %parent.display_path().join(&staging_name).display(),
                        %cleanup_error,
                        "failed to clean unconfirmed session staging directory"
                    );
                }
                return Err(error);
            }
        };
        let built = build(&staging).and_then(|result| {
            staging.sync_tree()?;
            Ok(result)
        });
        let result = match built {
            Ok(result) => match parent.rename_child_no_replace(&staging_name, target_name) {
                Ok(()) => {
                    // The namespace commit is already observable. A directory
                    // fsync failure is therefore committed-unknown, not a safe
                    // error to return and retry as a second entity.
                    if let Err(error) = parent.sync() {
                        tracing::warn!(
                            path = %parent.display_path().join(target_name).display(),
                            %error,
                            "session published but parent directory sync failed"
                        );
                    }
                    // Keep using the already-open staging capability. Reopening
                    // the target after the namespace commit would introduce a
                    // committed-unknown failure window (for example EMFILE) and
                    // could observe a concurrently substituted directory.
                    let published = staging.rebind_child_display_path(parent, target_name);
                    return Ok((result, published));
                }
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        if let Err(cleanup_error) = parent.remove_tree_child(&staging_name)
            && cleanup_error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(
                path = %parent.display_path().join(&staging_name).display(),
                %cleanup_error,
                "failed to clean unpublished session staging directory"
            );
        }
        result
    }

    fn build_publish_and_cache<T>(
        &self,
        info: &Info,
        parent: &super::ContainedDirectory,
        build: impl FnOnce(&super::ContainedDirectory) -> io::Result<T>,
    ) -> io::Result<T> {
        let key = format!("{}\0{}", info.id, info.cwd);
        let mut cache = self
            .opened_sessions
            .lock()
            .map_err(|_| io::Error::other("session capability cache poisoned"))?;
        if cache.contains_key(&key) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "session is already bound to this storage adapter",
            ));
        }
        let target_name = info.id.to_string();
        let (result, directory) = Self::build_and_publish_session_opened(
            parent,
            std::ffi::OsStr::new(&target_name),
            build,
        )?;
        cache.insert(key, std::sync::Arc::new(directory));
        Ok(result)
    }
    /// Write a full JSONL file (rewriting all items), crash-atomically: serialize
    /// to a temp file then rename over the target, so a crash / `ENOSPC` mid-write
    /// can't truncate the existing file (e.g. lose `rewind_points.jsonl` history).
    async fn write_jsonl<T: serde::Serialize>(&self, path: PathBuf, items: &[T]) -> io::Result<()> {
        super::write_jsonl_atomic_async(&path, items).await
    }
    fn read_jsonl<T: serde::de::DeserializeOwned>(&self, path: PathBuf) -> io::Result<Vec<T>> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "JSONL path has no parent")
        })?;
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "JSONL path has no file name")
        })?;
        let directory =
            super::ContainedDirectory::open(parent, Path::new(""), "JSONL directory", false)?;
        let file = match directory.open_regular(name, "JSONL ledger") {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        super::read_committed_jsonl_from_file(
            file,
            path,
            "JSONL ledger",
            super::MAX_JSONL_ENTRY_BYTES,
        )
    }

    fn read_jsonl_from_directory<T: serde::de::DeserializeOwned>(
        directory: &super::ContainedDirectory,
        name: &std::ffi::OsStr,
        description: &str,
    ) -> io::Result<Vec<T>> {
        let file = match directory.open_regular(name, description) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        super::read_committed_jsonl_from_file(
            file,
            directory.display_path().join(name),
            description,
            super::MAX_JSONL_ENTRY_BYTES,
        )
    }

    /// Read every complete Timeline record. A final non-newline-terminated
    /// fragment was never committed and is ignored; every complete line is
    /// parsed strictly so interior corruption still fails closed.
    fn read_timeline(&self, path: PathBuf) -> io::Result<Vec<chat_state::TimelineEvent>> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Timeline path has no parent")
        })?;
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Timeline path has no file name",
            )
        })?;
        let directory = super::ContainedDirectory::open(
            parent,
            Path::new(""),
            "Timeline session directory",
            false,
        )?;
        Self::read_timeline_from_directory(&directory)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "mandatory Timeline ledger is missing",
                )
            } else {
                error
            }
        })
    }

    fn read_timeline_from_directory(
        directory: &super::ContainedDirectory,
    ) -> io::Result<Vec<chat_state::TimelineEvent>> {
        super::read_committed_jsonl_from_directory(
            directory,
            std::ffi::OsStr::new(super::TIMELINE_FILE),
            "mandatory Timeline ledger",
            super::MAX_JSONL_ENTRY_BYTES,
        )
    }
    /// Append a session update to the updates.jsonl file, wrapping it in an envelope with timestamp.
    pub(super) async fn append_update_to_file(
        &self,
        info: &Info,
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
        let directory = self.open_session(info)?.directory;
        let path = directory.display_path().join(super::UPDATES_FILE);
        tokio::task::spawn_blocking(move || {
            Self::append_jsonl_line_in_directory_sync(
                &directory,
                std::ffi::OsStr::new(super::UPDATES_FILE),
                &path,
                line,
                durability,
            )
        })
        .await
        .map_err(io::Error::other)?
    }
    async fn append_update_with_bookkeeping(
        &self,
        info: &Info,
        update: &super::SessionUpdate,
        durability: AppendDurability,
    ) -> Result<(), super::AppendUpdateError> {
        self.ensure_writer_lease(info)
            .map_err(super::AppendUpdateError::NotCommitted)?;
        self.append_update_to_file(info, update, durability)
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
        let Some(iterator) = super::UpdatesIterator::open(&path)? else {
            return Ok(Vec::new());
        };
        Self::collect_updates(iterator, &path)
    }

    fn read_updates_from_directory(
        directory: &super::ContainedDirectory,
    ) -> io::Result<Vec<super::SessionUpdate>> {
        let path = directory.display_path().join(super::UPDATES_FILE);
        let file = match directory.open_regular(
            std::ffi::OsStr::new(super::UPDATES_FILE),
            "session updates ledger",
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        Self::collect_updates(super::UpdatesIterator::from_file(file, path.clone()), &path)
    }

    fn collect_updates(
        iterator: super::UpdatesIterator,
        path: &Path,
    ) -> io::Result<Vec<super::SessionUpdate>> {
        let mut skipped_lines: usize = 0;
        let mut updates = Vec::new();
        for parsed in iterator {
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
        let bytes = super::serialize_summary(summary)?;
        super::write_bytes_atomic(&summary_path, &bytes)
    }
    pub(crate) fn read_summary_sync(&self, info: &Info) -> io::Result<Summary> {
        Ok(self.open_session(info)?.summary)
    }

    pub(crate) fn open_session(&self, info: &Info) -> io::Result<OpenedSession> {
        let key = format!("{}\0{}", info.id, info.cwd);
        let mut cache = self
            .opened_sessions
            .lock()
            .map_err(|_| io::Error::other("session capability cache poisoned"))?;
        let directory = match cache.get(&key) {
            Some(directory) => directory.clone(),
            None => {
                let directory = self.session_directory(info, false)?;
                let summary = Self::read_summary_from_directory(&directory)?;
                Self::validate_session_identity(info, &summary)?;
                let directory = std::sync::Arc::new(directory);
                cache.insert(key, directory.clone());
                return Ok(OpenedSession { directory, summary });
            }
        };
        drop(cache);
        let summary = Self::read_summary_from_directory(&directory)?;
        Self::validate_session_identity(info, &summary)?;
        Ok(OpenedSession { directory, summary })
    }

    pub(crate) fn open_session_by_id(
        &self,
        session_id: &str,
    ) -> io::Result<Option<OpenedSession>> {
        if !matches!(&self.dir_mode, SessionDirMode::FromRoot(_)) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "id-only resolution requires the canonical storage root",
            ));
        }
        let authority = self.authority(false)?;
        let sessions = match authority.open_relative(
            Path::new("sessions"),
            "sessions directory",
            false,
        ) {
            Ok(sessions) => sessions,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut found: Option<OpenedSession> = None;
        for cwd_name in sessions.list_names()? {
            if cwd_name.to_string_lossy().starts_with('.') {
                continue;
            }
            let cwd_dir = match sessions.open_relative(
                Path::new(&cwd_name),
                "session cwd directory",
                false,
            ) {
                Ok(directory) => directory,
                Err(_) => continue,
            };
            let session = match cwd_dir.open_relative(
                Path::new(session_id),
                "session directory",
                false,
            ) {
                Ok(directory) => directory,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let summary = Self::read_summary_from_directory(&session)?;
            Self::validate_physical_session_identity(
                &cwd_name,
                &cwd_dir,
                std::ffi::OsStr::new(session_id),
                &summary,
            )?;
            if found.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("duplicate canonical session id {session_id}"),
                ));
            }
            let key = format!("{}\0{}", summary.info.id, summary.info.cwd);
            let directory = std::sync::Arc::new(session);
            self.opened_sessions
                .lock()
                .map_err(|_| io::Error::other("session capability cache poisoned"))?
                .insert(key, directory.clone());
            found = Some(OpenedSession { directory, summary });
        }
        Ok(found)
    }

    fn validate_physical_session_identity(
        cwd_name: &std::ffi::OsStr,
        cwd_directory: &super::ContainedDirectory,
        session_name: &std::ffi::OsStr,
        summary: &Summary,
    ) -> io::Result<()> {
        if session_name != std::ffi::OsStr::new(summary.info.id.0.as_ref()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session directory name conflicts with Summary id",
            ));
        }
        let expected_cwd_name = crate::util::grow_home::encode_cwd_dirname(&summary.info.cwd);
        if cwd_name != std::ffi::OsStr::new(&expected_cwd_name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session Summary cwd conflicts with its storage directory",
            ));
        }
        if expected_cwd_name != urlencoding::encode(&summary.info.cwd).as_ref() {
            let marker = cwd_directory.read_bounded(
                std::ffi::OsStr::new(".cwd"),
                "session cwd marker",
                super::MAX_SESSION_SUMMARY_BYTES,
            )?;
            if marker != summary.info.cwd.as_bytes() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session cwd marker conflicts with Summary cwd",
                ));
            }
        }
        Ok(())
    }

    fn validate_session_identity(info: &Info, summary: &Summary) -> io::Result<()> {
        if summary.info.id.0 != info.id.0 || summary.info.cwd != info.cwd {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "session identity mismatch: requested {}/{}, found {}/{}",
                    info.id, info.cwd, summary.info.id, summary.info.cwd
                ),
            ));
        }
        Ok(())
    }

    pub(crate) fn delete_session_by_id_sync(
        &self,
        session_id: &str,
        cwd: Option<&str>,
    ) -> io::Result<Option<Info>> {
        let Some(opened) = self.open_session_by_id(session_id)? else {
            return Ok(None);
        };
        if cwd.is_some_and(|expected| expected != opened.summary.info.cwd) {
            return Ok(None);
        }
        let info = opened.summary.info.clone();
        self.delete_opened_session(opened)?;
        Ok(Some(info))
    }

    fn delete_opened_session(&self, opened: OpenedSession) -> io::Result<()> {
        let info = opened.summary.info.clone();
        self.ensure_writer_lease(&info)?;
        let parent = self.open_session_parent(&info)?;
        let name_string = info.id.to_string();
        let name = std::ffi::OsStr::new(&name_string);
        let quarantine_name = std::ffi::OsString::from(format!(
            ".{}.{}.deleting",
            info.id,
            uuid::Uuid::now_v7().simple()
        ));
        parent.rename_child_no_replace(name, &quarantine_name)?;
        if let Err(error) = parent.sync() {
            tracing::warn!(
                session_id = %info.id,
                %error,
                "session quarantine rename committed but parent sync failed"
            );
        }
        let quarantined = match parent.open_relative(
            std::path::Path::new(&quarantine_name),
            "quarantined session directory",
            false,
        ) {
            Ok(directory) => directory,
            Err(error) => {
                let _ = parent.rename_child_no_replace(&quarantine_name, name);
                return Err(error);
            }
        };
        let same_entity = match opened.directory().is_same_entity(&quarantined) {
            Ok(same) => same,
            Err(error) => {
                let _ = parent.rename_child_no_replace(&quarantine_name, name);
                return Err(error);
            }
        };
        if !same_entity {
            parent
                .rename_child_no_replace(&quarantine_name, name)
                .map_err(|restore_error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "session leaf changed during delete and quarantine restoration failed: {restore_error}"
                        ),
                    )
                })?;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session leaf changed during delete; replacement entity was preserved",
            ));
        }
        quarantined.remove_all_contents()?;
        parent.remove_empty_child(&quarantine_name, true)?;
        let key = format!("{}\0{}", info.id, info.cwd);
        self.writer_leases
            .lock()
            .map_err(|_| io::Error::other("session writer lease cache poisoned"))?
            .remove(&key);
        let lease_name = format!(".{}.writer.lock", info.id);
        match parent.remove_file(std::ffi::OsStr::new(&lease_name), true) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.opened_sessions
            .lock()
            .map_err(|_| io::Error::other("session capability cache poisoned"))?
            .remove(&key);
        Ok(())
    }

    fn ensure_writer_lease(&self, info: &Info) -> io::Result<()> {
        if !self.try_acquire_writer_lease(info)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("session {} already has an active writer", info.id),
            ));
        }
        Ok(())
    }

    fn try_acquire_writer_lease(&self, info: &Info) -> io::Result<bool> {
        let key = format!("{}\0{}", info.id, info.cwd);
        let mut leases = self
            .writer_leases
            .lock()
            .map_err(|_| io::Error::other("session writer lease cache poisoned"))?;
        if leases.contains_key(&key) {
            return Ok(true);
        }
        let parent = self.open_session_parent(info)?;
        let lease_name = format!(".{}.writer.lock", info.id);
        let lease = parent.open_read_write_create(std::ffi::OsStr::new(&lease_name))?;
        match lease.try_lock_exclusive() {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error),
        }
        leases.insert(key, lease);
        Ok(true)
    }

    /// Delete only whole, identity-checked stale session entities. A live
    /// writer lease makes the entity ineligible; individual files are never
    /// aged or removed independently.
    pub(crate) fn cleanup_stale_sessions_sync(
        &self,
        ttl_days: u32,
        skip_session_dir: Option<&Path>,
    ) -> io::Result<(u32, u32)> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(ttl_days));
        let mut deleted = 0u32;
        let mut errors = 0u32;
        for opened in self.scan_opened_sessions(None)? {
            if skip_session_dir.is_some_and(|skip| opened.directory().display_path() == skip) {
                continue;
            }
            let activity = opened
                .summary()
                .last_active_at
                .unwrap_or(opened.summary().updated_at);
            if activity >= cutoff {
                continue;
            }
            match self.try_acquire_writer_lease(&opened.summary().info) {
                Ok(false) => continue,
                Ok(true) => match self.delete_opened_session(opened) {
                    Ok(()) => deleted = deleted.saturating_add(1),
                    Err(error) => {
                        errors = errors.saturating_add(1);
                        tracing::warn!(%error, "failed to delete stale session entity");
                    }
                },
                Err(error) => {
                    errors = errors.saturating_add(1);
                    tracing::warn!(%error, "failed to acquire stale session writer lease");
                }
            }
        }
        Ok((deleted, errors))
    }

    fn read_summary_from_directory(
        directory: &super::ContainedDirectory,
    ) -> io::Result<Summary> {
        let bytes = directory.read_bounded(
            std::ffi::OsStr::new(super::SUMMARY_FILE),
            "session summary",
            super::MAX_SESSION_SUMMARY_BYTES,
        )?;
        if bytes.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "summary.json is empty (0 bytes): {}",
                    directory.display_path().join(super::SUMMARY_FILE).display()
                ),
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
        if run_ids.is_empty() {
            return Ok(Vec::new());
        }
        let session = self.open_session(info)?.directory;
        let mut restored = Vec::new();
        for run_id in run_ids {
            let run_relative = Path::new("workflows").join(&run_id);
            let run_dir =
                match session.open_relative(&run_relative, "Workflow run directory", false) {
                    Ok(run_dir) => run_dir,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                };
            match run_dir.read_bounded(
                std::ffi::OsStr::new("cleared"),
                "Workflow cleared marker",
                0,
            ) {
                Ok(_) => continue,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            let manifest_path = run_dir.display_path().join("state.json");
            let manifest = match run_dir
                .read_bounded(
                    std::ffi::OsStr::new("state.json"),
                    "Workflow manifest",
                    MAX_WORKFLOW_MANIFEST_BYTES,
                )
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
            let scripts = match run_dir.open_relative(
                Path::new("scripts"),
                "Workflow scripts directory",
                false,
            ) {
                Ok(scripts) => scripts,
                Err(error) => {
                    tracing::warn!(%run_id, %error, "skipping workflow with missing scripts directory");
                    continue;
                }
            };
            let script_name = format!("{:04}.rhai", manifest.script_revision);
            let script_path = scripts.display_path().join(&script_name);
            let script = match scripts
                .read_bounded(
                    std::ffi::OsStr::new(&script_name),
                    "Workflow immutable script",
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
            let args_path = run_dir.display_path().join("args.json");
            let args = match run_dir
                .read_bounded(
                    std::ffi::OsStr::new("args.json"),
                    "Workflow immutable args",
                    MAX_WORKFLOW_ARGS_BYTES,
                )
                .and_then(|bytes| {
                    serde_json::from_slice(&bytes)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
                }) {
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
        let parent = self.ensure_session_parent(info)?;
        let result = self.build_publish_and_cache(
            info,
            &parent,
            |staging| {
            crate::session::persistence::write_initial_prompt_blobs_to_directory(
                &initial_surface,
                staging,
                &initial_prompt_blobs,
            )?;
            let mut timeline = Timeline::from_seed(initial_surface)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            for fact in initial_facts {
                timeline
                    .record(fact)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            }
            staging.write_atomic(
                std::ffi::OsStr::new(super::TIMELINE_FILE),
                &super::to_jsonl_bytes(timeline.events())?,
                true,
                false,
            )?;
            staging.write_atomic(
                std::ffi::OsStr::new(super::SUMMARY_FILE),
                &super::serialize_summary(&summary)?,
                true,
                false,
            )?;
            Ok((summary, timeline.events().to_vec()))
            },
        )?;
        self.ensure_writer_lease(info)?;
        Ok(result)
    }
    /// Like [`Self::apply_summary_patch`], but returns whether a
    /// a newer canonical title projection was applied (see [`Summary::apply_patch`]).
    async fn apply_summary_patch_reporting(
        &self,
        info: &Info,
        patch: super::summary_write::SummaryPatch,
    ) -> io::Result<bool> {
        self.ensure_writer_lease(info)?;
        let directory = self.open_session(info)?.directory;
        tokio::task::spawn_blocking(move || {
            super::summary_write::apply_patch_locked_in_directory(
                &directory,
                std::ffi::OsStr::new(super::SUMMARY_FILE),
                std::ffi::OsStr::new("summary.json.lock"),
                &patch,
            )
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

    async fn reconcile_model_projection(
        &self,
        info: &Info,
        mut summary: Summary,
        timeline: &Timeline,
    ) -> io::Result<Summary> {
        let Some((model_id, reasoning_effort)) =
            crate::session::persistence::latest_model_selection(timeline.events())?
        else {
            return Ok(summary);
        };
        if summary.current_model_id == model_id && summary.reasoning_effort == reasoning_effort {
            return Ok(summary);
        }
        self.update_current_model_and_agent(info, &model_id, None, Some(reasoning_effort))
            .await?;
        summary.current_model_id = model_id;
        summary.reasoning_effort = reasoning_effort;
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
/// 2. Truncates at chat-state's canonical complete-turn boundary. Trailing
///    incomplete turns — including a trailing user/reasoning tail with no
///    matching assistant response (e.g. the in-flight `/goal` turn) — are
///    removed so the child never sees an incoherent partial turn.
///
/// Also used by the live parent-chat fork path (summarized fallback only — the
/// verbatim mirror path keeps items unfiltered to preserve cached synthetics).
///
pub(crate) fn fork_filter_surface(items: &mut Vec<ConversationItem>) {
    items.retain(|item| match item {
        ConversationItem::User(u) => u.synthetic_reason.is_none(),
        _ => true,
    });
    let leading_system_end = items
        .iter()
        .take_while(|item| matches!(item, ConversationItem::System(_)))
        .count();
    let last_complete_end = chat_state::compaction_utils::complete_turn_ends(items.iter())
        .last()
        .copied()
        .unwrap_or(leading_system_end);
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
    source_session: &super::ContainedDirectory,
    staging_session: &super::ContainedDirectory,
) -> io::Result<usize> {
    let hashes = crate::session::persistence::referenced_prompt_blob_hashes(surface)?;
    for hash in &hashes {
        let bytes = crate::session::persistence::verified_prompt_blob_bytes_from_directory(
            source_session,
            hash,
        )?;
        let target = Path::new("prompts").join(format!("{hash}.txt"));
        crate::session::persistence::write_immutable_blob_to_directory(
            staging_session,
            &target,
            &bytes,
        )?;
    }
    Ok(hashes.len())
}

impl JsonlStorageAdapter {
    fn authority(&self, create_root: bool) -> io::Result<super::ContainedDirectory> {
        if let Some(authority) = self.authority.get() {
            return authority.try_clone();
        }
        let root = match &self.dir_mode {
            SessionDirMode::FromRoot(root) => {
                if create_root {
                    Self::ensure_storage_root(root)?;
                }
                root.as_path()
            }
            SessionDirMode::Explicit(dir) => dir.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "session path has no parent")
            })?,
        };
        let opened = super::ContainedDirectory::open(
            root,
            Path::new(""),
            "session storage authority",
            false,
        )?;
        let _ = self.authority.set(opened);
        self.authority
            .get()
            .expect("session authority initialized")
            .try_clone()
    }

    /// Fully synchronous version of `copy_session_data` for use inside
    /// `spawn_blocking`. The entire staged copy uses synchronous bounded
    /// storage primitives, without nesting `spawn_blocking` calls.
    pub fn copy_session_data_sync(
        &self,
        source_info: &Info,
        target_info: &Info,
        options: super::CopySessionOptions,
    ) -> io::Result<super::CopySessionResult> {
        let parent = self.ensure_session_parent(target_info)?;
        let source_session = self.open_session(source_info)?;
        self.build_publish_and_cache(
            target_info,
            &parent,
            |staging| {
            let source_summary = source_session.summary().clone();
            let source_events = source_session.timeline_events()?;
            let source_timeline = Timeline::from_events(source_events)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let mut surface_to_copy = source_timeline.surface().to_vec();
            let mut updates_to_copy: Vec<super::SessionUpdate> =
                Self::read_updates_from_directory(source_session.directory())?;
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
                source_session.directory(),
                staging,
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
            staging.write_atomic(
                std::ffi::OsStr::new(super::SUMMARY_FILE),
                &super::serialize_summary(&target_summary)?,
                true,
                false,
            )?;
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
                        crate::session::behavior::BehaviorState::Plan(_)
                            | crate::session::behavior::BehaviorState::Workflow
                            | crate::session::behavior::BehaviorState::DeepResearch { .. }
                            | crate::session::behavior::BehaviorState::Goal
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
            staging.write_atomic(
                std::ffi::OsStr::new(super::TIMELINE_FILE),
                &super::to_jsonl_bytes(fork_timeline.events())?,
                true,
                false,
            )?;
            let transformed_updates: Vec<super::SessionUpdate> = updates_to_copy
                .into_iter()
                .map(|u| transform_session_id_in_update(u, &target_info.id))
                .collect();
            let update_envelopes = transformed_updates
                .iter()
                .map(SessionUpdateEnvelope::from_update)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            staging.write_atomic(
                std::ffi::OsStr::new(super::UPDATES_FILE),
                &super::to_jsonl_bytes(&update_envelopes)?,
                true,
                false,
            )?;
            Ok(super::CopySessionResult {
                surface_items_copied,
                updates_copied: num_messages,
                control_event_seeded,
                prompt_blobs_copied,
            })
            },
        )
    }
}
#[async_trait]
impl StorageAdapter for JsonlStorageAdapter {
    async fn init_session(&self, info: &Info, model_id: acp::ModelId) -> io::Result<Summary> {
        match self.open_session(info) {
            Ok(opened) => {
                tracing::info!("Loading existing session from JSONL");
                let summary = opened.summary().clone();
                let timeline = Timeline::from_events(opened.timeline_events()?)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                let _ = opened.sideband_ledgers(&info.id.to_string(), &timeline)?;
                Ok(summary)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tracing::info!("Creating new session in JSONL");
                let mut summary = Summary::new(info, model_id)?;
                summary.sandbox_profile = sandbox::configured_profile_name().map(String::from);
                let (summary, _) = self
                    .init_session_with_summary(
                        info,
                        summary,
                        Vec::new(),
                        Default::default(),
                        Vec::new(),
                    )
                    .await?;
                Ok(summary)
            }
            Err(error) => Err(error),
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
        self.ensure_writer_lease(info)?;
        chat_state::validate_sideband_id(&event.sideband_id)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let session = self.open_session(info)?.directory;
        Self::append_sideband_event_with_durability(session, event, AppendDurability::Durable).await
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
        self.ensure_writer_lease(info)?;
        let session = self.open_session(info)?.directory;
        let manifest = manifest.clone();
        tokio::task::spawn_blocking(move || {
            crate::session::workflow::store::write_workflow_run_manifest_in_directory(
                &session,
                &manifest,
            )
        })
        .await
        .map_err(io::Error::other)?
    }
    async fn delete_workflow_run_state(&self, info: &Info, run_id: &str) -> io::Result<()> {
        self.ensure_writer_lease(info)?;
        let session = self.open_session(info)?.directory;
        let run_id = run_id.to_owned();
        tokio::task::spawn_blocking(move || {
            crate::session::workflow::store::tombstone_workflow_run_in_directory(
                &session,
                &run_id,
            )
        })
        .await
        .map_err(io::Error::other)?
    }
    async fn load_session(&self, info: &Info) -> io::Result<PersistedData> {
        let opened = self.open_session(info)?;
        let summary = opened.summary().clone();
        let timeline_events = opened.timeline_events()?;
        let timeline = Timeline::from_events(timeline_events.clone())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        crate::session::persistence::verify_timeline_prompt_blobs_from_directory(
            opened.directory(),
            &timeline,
        )?;
        let sidebands = opened.sideband_ledgers(&info.id.to_string(), &timeline)?;
        self.recover_interrupted_sidebands(info, &sidebands).await?;
        let summary = self
            .reconcile_session_title_projection(info, summary, &timeline)
            .await?;
        let summary = self
            .reconcile_model_projection(info, summary, &timeline)
            .await?;
        let control_snapshot =
            crate::session::control::SessionControlSnapshot::latest_from_timeline(
                timeline.events(),
            )?;
        let updates = Self::read_updates_from_directory(opened.directory())?;
        let signals =
            crate::session::signals::SessionSignals::latest_from_timeline(timeline.events())?;
        let announcement_state =
            crate::session::announcement_state::AnnouncementState::latest_from_timeline(
                timeline.events(),
            )?;
        let workflow_runs = self.load_workflow_runs_sync(info, &timeline)?;
        let rewind_points = Self::read_jsonl_from_directory::<RewindPoint>(
            opened.directory(),
            std::ffi::OsStr::new("rewind_points.jsonl"),
            "rewind points ledger",
        )?;
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
        let opened = self.open_session(info)?;
        let summary = opened.summary().clone();
        let timeline_events = opened.timeline_events()?;
        let timeline = Timeline::from_events(timeline_events.clone())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        crate::session::persistence::verify_timeline_prompt_blobs_from_directory(
            opened.directory(),
            &timeline,
        )?;
        let sidebands = opened.sideband_ledgers(&info.id.to_string(), &timeline)?;
        self.recover_interrupted_sidebands(info, &sidebands).await?;
        let summary = self
            .reconcile_session_title_projection(info, summary, &timeline)
            .await?;
        let summary = self
            .reconcile_model_projection(info, summary, &timeline)
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
        let opened = match self.open_session(info) {
            Ok(opened) => opened,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        self.delete_opened_session(opened)
    }
    async fn append_rewind_point(&self, info: &Info, point: &RewindPoint) -> io::Result<()> {
        self.ensure_writer_lease(info)?;
        let mut line = serde_json::to_vec(point)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        line.push(b'\n');
        let directory = self.open_session(info)?.directory;
        let path = directory.display_path().join("rewind_points.jsonl");
        tokio::task::spawn_blocking(move || {
            Self::append_jsonl_line_in_directory_sync(
                &directory,
                std::ffi::OsStr::new("rewind_points.jsonl"),
                &path,
                line,
                AppendDurability::Buffered,
            )
        })
        .await
        .map_err(io::Error::other)?
    }
    async fn load_rewind_points(&self, info: &Info) -> io::Result<Vec<RewindPoint>> {
        let info_clone = info.clone();
        let adapter_clone = self.clone();
        tokio::task::spawn_blocking(move || {
            let adapter = adapter_clone;
            let opened = adapter.open_session(&info_clone)?;
            Self::read_jsonl_from_directory::<RewindPoint>(
                opened.directory(),
                std::ffi::OsStr::new("rewind_points.jsonl"),
                "rewind points ledger",
            )
        })
        .await
        .map_err(io::Error::other)?
    }
    async fn replace_rewind_points(
        &self,
        info: &Info,
        points: &[RewindPoint],
    ) -> io::Result<()> {
        self.ensure_writer_lease(info)?;
        let bytes = super::to_jsonl_bytes(points)?;
        let directory = self.open_session(info)?.directory;
        tokio::task::spawn_blocking(move || {
            directory.write_atomic(
                std::ffi::OsStr::new("rewind_points.jsonl"),
                &bytes,
                true,
                true,
            )
        })
        .await
        .map_err(io::Error::other)?
    }
    async fn write_rewind_transaction(
        &self,
        info: &Info,
        transaction: &crate::session::persistence::RewindTransaction,
    ) -> io::Result<()> {
        self.ensure_writer_lease(info)?;
        transaction.validate()?;
        let bytes = serde_json::to_vec(transaction).map_err(io::Error::other)?;
        if bytes.len() as u64 > crate::session::persistence::MAX_REWIND_TRANSACTION_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "rewind transaction exceeds its byte limit",
            ));
        }
        let directory = self.open_session(info)?.directory;
        tokio::task::spawn_blocking(move || {
            directory.write_atomic(
                std::ffi::OsStr::new(crate::session::persistence::REWIND_TRANSACTION_FILE),
                &bytes,
                true,
                true,
            )
        })
        .await
        .map_err(io::Error::other)?
    }
    async fn clear_rewind_transaction(&self, info: &Info) -> io::Result<()> {
        self.ensure_writer_lease(info)?;
        let directory = self.open_session(info)?.directory;
        tokio::task::spawn_blocking(move || {
            match directory.remove_file(
                std::ffi::OsStr::new(crate::session::persistence::REWIND_TRANSACTION_FILE),
                true,
            ) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        })
        .await
        .map_err(io::Error::other)?
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
        let directory = self.open_session(info)?.directory;
        tokio::task::spawn_blocking(move || {
            let events = Self::read_timeline_from_directory(&directory)?;
            let timeline = chat_state::Timeline::from_events(events)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            Ok(timeline.prompt_records())
        })
        .await
        .map_err(io::Error::other)?
    }
    fn open_timeline_reader(&self, info: &Info) -> io::Result<super::TimelineLedgerReader> {
        let opened = self.open_session(info)?;
        let file = opened
            .directory()
            .open_regular(std::ffi::OsStr::new(super::TIMELINE_FILE), "mandatory Timeline ledger")
            .map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "mandatory Timeline ledger is missing",
                    )
                } else {
                    error
                }
            })?;
        super::TimelineLedgerReader::from_file(
            file,
            opened.directory().display_path().join(super::TIMELINE_FILE),
        )
    }
}
#[cfg(test)]
mod durable_tests;
#[cfg(test)]
mod tests;

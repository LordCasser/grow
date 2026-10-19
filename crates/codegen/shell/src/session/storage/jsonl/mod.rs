use super::{
    PersistedData, SESSION_SEARCH_INDEX_FILE, SessionUpdateEnvelope, StorageAdapter,
    updates_truncate_for_prompt,
};
use crate::sampling::{
    ConversationItem, conversation_truncate_for_prompt, transform_conversation_cwd,
};
use crate::session::info::Info;
use crate::session::persistence::{SESSION_FORMAT_VERSION, Summary};
use acp_transport::protocol as acp;
use async_trait::async_trait;
use chat_state::Timeline;
use fs2::FileExt;
use std::io::{self, BufRead, Read, Seek, Write};
use std::path::{Path, PathBuf};
use workspace::session::file_state::RewindPoint;

/// Hard limits for diagnostic and maintenance validation. Normal session
/// loading keeps its existing semantics; bounded callers use this aggregate
/// budget before doing any destructive maintenance work.
const BOUNDED_VALIDATION_MAX_BYTES: u64 = 16 * 1024 * 1024;
const BOUNDED_VALIDATION_MAX_EVENTS: usize = 50_000;
const BOUNDED_VALIDATION_MAX_REFERENCES: usize = 4_096;
const TTL_DISCOVERY_MAX_CANDIDATES: usize = 4_096;
const TTL_DISCOVERY_MAX_DIRECTORY_ENTRIES: usize = 8_192;
const BOUNDED_SESSION_ID_SCAN_ENTRIES: usize = 4_096;
const RECLAIM_INTENT_DIRECTORY: &str = ".reclaim-intents";
const RECLAIM_GATE_FILE: &str = ".reclaim-gate.lock";
const RECLAIM_INTENT_MAX_BYTES: u64 = 1024 * 1024;
const RECLAIM_INTENT_MAX_FILES: usize = 256;

fn bounded_read_exhausted(description: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        format!("{description} exceeded its bounded read budget"),
    )
}
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
/// JSONL storage under `{root}/sessions/{encoded_cwd}/{session_id}/`.
/// Long CWDs use the shared bounded slug/hash encoding.
#[derive(Clone)]
pub struct JsonlStorageAdapter {
    dir_mode: SessionDirMode,
    authority: std::sync::Arc<std::sync::OnceLock<super::ContainedDirectory>>,
    opened_sessions: std::sync::Arc<
        std::sync::Mutex<
            std::collections::BTreeMap<String, std::sync::Arc<super::ContainedDirectory>>,
        >,
    >,
    writer_leases:
        std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, std::fs::File>>>,
    timeline_prefixes: std::sync::Arc<
        std::sync::Mutex<
            std::collections::BTreeMap<
                String,
                std::sync::Arc<std::sync::Mutex<Option<LedgerPrefix>>>,
            >,
        >,
    >,
    #[cfg(test)]
    update_append_probe: Option<std::sync::Arc<AppendProbe>>,
    #[cfg(test)]
    update_sync_probe: Option<std::sync::Arc<UpdateSyncProbe>>,
    #[cfg(test)]
    timeline_sync_probe: Option<std::sync::Arc<TimelineSyncProbe>>,
}

#[derive(Clone)]
struct LedgerPrefix {
    len: u64,
    hash: blake3::Hash,
    stamp: LedgerFileStamp,
    /// Continuation state after hashing exactly `len` committed bytes. The
    /// writer lease and file stamp make this cache authoritative for an
    /// unchanged writer epoch, so normal append never rereads the historical
    /// ledger. A foreign write changes the stamp and forces a full hash check.
    hasher: blake3::Hasher,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RawExactStatus {
    Missing,
    Exact,
    Conflict,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionReclaimIntent {
    version: u32,
    id: String,
    members: Vec<SessionReclaimMember>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionReclaimMember {
    session_id: String,
    cwd: String,
    entity_volume: u64,
    entity_index: u64,
    quarantine_name: String,
}

struct SessionReclaimGate {
    intents: super::ContainedDirectory,
    _lock: std::fs::File,
}

impl SessionReclaimMember {
    fn info(&self) -> Info {
        Info {
            id: acp::SessionId::new(self.session_id.clone()),
            cwd: self.cwd.clone(),
        }
    }

    fn matches_info(&self, info: &Info) -> bool {
        self.session_id == info.id.0.as_ref() && self.cwd == info.cwd
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LedgerFileStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
    #[cfg(windows)]
    creation_time: u64,
    #[cfg(windows)]
    last_write_time: u64,
}

impl LedgerFileStamp {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt as _;
        #[cfg(windows)]
        use std::os::windows::fs::MetadataExt as _;

        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            changed_seconds: metadata.ctime(),
            #[cfg(unix)]
            changed_nanoseconds: metadata.ctime_nsec(),
            #[cfg(windows)]
            creation_time: metadata.creation_time(),
            #[cfg(windows)]
            last_write_time: metadata.last_write_time(),
        }
    }
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

/// One fully validated view of a pinned Session entity. Consumers that derive
/// model-visible Surface, resume state, or copies must start here so Timeline,
/// prompt blobs, and Sideband provenance cannot be validated independently.
pub(crate) struct ValidatedTimeline {
    pub(crate) timeline: chat_state::Timeline,
    pub(crate) sidebands: super::SidebandLedgers,
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
        JsonlStorageAdapter::read_timeline_from_directory(&self.directory).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "mandatory Timeline ledger is missing",
                )
            } else if crate::session::persistence::session_version_mismatch_from(&error).is_some() {
                error
            } else if error.kind() == io::ErrorKind::InvalidData {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "invalid Timeline ledger (expected schema version {}): {error}",
                        chat_state::TIMELINE_SCHEMA_VERSION,
                    ),
                )
            } else {
                error
            }
        })
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
        let lines =
            super::CommittedJsonlLines::from_open_file_at(file, path, "session updates ledger", 0)?;
        let mut updates = Vec::new();
        for (index, line) in lines.enumerate() {
            let line = line?;
            let value: serde_json::Value =
                serde_json::from_slice(line.trim_ascii()).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid committed update {index}: {error}"),
                    )
                })?;
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

    pub(crate) fn validated_timeline(&self, timeline_id: &str) -> io::Result<ValidatedTimeline> {
        let timeline = Timeline::from_events(self.timeline_events()?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        crate::session::persistence::verify_timeline_prompt_blobs_from_directory(
            &self.directory,
            &timeline,
        )?;
        let sidebands = self.sideband_ledgers(timeline_id, &timeline)?;
        Ok(ValidatedTimeline {
            timeline,
            sidebands,
        })
    }

    /// Validate a read-only/maintenance view under a strict aggregate IO
    /// budget. The ordinary loader intentionally continues to use
    /// `validated_timeline` without these observational limits.
    pub(crate) fn bounded_validated_timeline(
        &self,
        timeline_id: &str,
    ) -> io::Result<ValidatedTimeline> {
        let mut remaining_bytes = BOUNDED_VALIDATION_MAX_BYTES;
        let mut remaining_events = BOUNDED_VALIDATION_MAX_EVENTS;
        let timeline_events = JsonlStorageAdapter::read_timeline_bounded_from_directory(
            &self.directory,
            &mut remaining_bytes,
            &mut remaining_events,
        )
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
        let timeline = Timeline::from_events(timeline_events)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        let sideband_ids = Self::bounded_sideband_ids(&timeline)?;
        let mut sidebands = super::SidebandLedgers::new();
        for sideband_id in &sideband_ids {
            chat_state::validate_sideband_id(sideband_id)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let sideband = match self.directory.open_relative(
                &Path::new(super::SIDEBANDS_DIR).join(sideband_id),
                "sideband entity directory",
                false,
            ) {
                Ok(sideband) => sideband,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let component = format!("Sideband schema for {sideband_id}");
            let events = match JsonlStorageAdapter::read_versioned_jsonl_bounded_from_directory::<
                chat_state::SidebandEvent,
            >(
                &sideband,
                std::ffi::OsStr::new(super::TIMELINE_FILE),
                "sideband Timeline ledger",
                super::MAX_JSONL_ENTRY_BYTES,
                &component,
                u64::from(chat_state::SIDEBAND_SCHEMA_VERSION),
                &mut remaining_bytes,
                &mut remaining_events,
            ) {
                Ok(events) => events,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if !events.is_empty() {
                sidebands.insert(sideband_id.clone(), events);
            }
        }
        super::validate_sideband_ledgers(timeline_id, &timeline, &sidebands)?;

        let prompt_hashes = Self::bounded_prompt_blob_hashes(&timeline)?;
        let sampling_chunks = Self::bounded_sampling_chunk_refs(&timeline)?;
        if prompt_hashes
            .len()
            .saturating_add(sampling_chunks.len())
            .saturating_add(sideband_ids.len())
            > BOUNDED_VALIDATION_MAX_REFERENCES
        {
            return Err(bounded_read_exhausted("session artifact references"));
        }
        for hash in &prompt_hashes {
            Self::verify_prompt_blob_with_budget(&self.directory, hash, &mut remaining_bytes)?;
        }
        let sampling_worst_case = sampling_chunks
            .len()
            .saturating_mul(crate::session::sampling_evidence::CHUNK_BYTES);
        if sampling_worst_case as u64 > remaining_bytes {
            return Err(bounded_read_exhausted("sampling evidence"));
        }
        // The evidence reader bounds each chunk to CHUNK_BYTES. Reserve that
        // worst case first so file growth cannot exceed the aggregate budget.
        crate::session::sampling_evidence::verify(&self.directory, &timeline)?;

        Ok(ValidatedTimeline {
            timeline,
            sidebands,
        })
    }

    fn bounded_sideband_ids(
        timeline: &chat_state::Timeline,
    ) -> io::Result<std::collections::BTreeSet<String>> {
        let mut sideband_ids = std::collections::BTreeSet::new();
        for event in timeline.events() {
            if let chat_state::TimelineEventKind::Sideband(spawn) = &event.kind {
                sideband_ids.insert(spawn.sideband_id.clone());
                if sideband_ids.len() > BOUNDED_VALIDATION_MAX_REFERENCES {
                    return Err(bounded_read_exhausted("sideband references"));
                }
            }
        }
        Ok(sideband_ids)
    }

    fn bounded_prompt_blob_hashes(
        timeline: &chat_state::Timeline,
    ) -> io::Result<std::collections::BTreeSet<String>> {
        let mut hashes = std::collections::BTreeSet::new();
        for event in timeline.events() {
            let chat_state::TimelineEventKind::Messages(messages) = &event.kind else {
                continue;
            };
            for item in &messages.items {
                for line in item.text_content().lines() {
                    let Some(hash) = line
                        .strip_suffix('\r')
                        .unwrap_or(line)
                        .strip_prefix(crate::session::persistence::PROMPT_BLOB_REF_PREFIX)
                    else {
                        continue;
                    };
                    if hash.len() != 64
                        || !hash
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Timeline contains an invalid prompt blob reference",
                        ));
                    }
                    hashes.insert(hash.to_owned());
                    if hashes.len() > BOUNDED_VALIDATION_MAX_REFERENCES {
                        return Err(bounded_read_exhausted("prompt blob references"));
                    }
                }
            }
        }
        Ok(hashes)
    }

    fn bounded_sampling_chunk_refs(
        timeline: &chat_state::Timeline,
    ) -> io::Result<std::collections::BTreeSet<(String, u64)>> {
        let mut chunks = std::collections::BTreeSet::new();
        for event in timeline.events() {
            let chat_state::TimelineEventKind::Observation(observation) = &event.kind else {
                continue;
            };
            if observation.scope != "sampling_evidence" {
                continue;
            }
            let data = observation.data.as_ref().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "sampling evidence is missing data",
                )
            })?;
            let chunk_values = data
                .get("chunks")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "sampling evidence chunks are invalid",
                    )
                })?;
            for chunk in chunk_values {
                let hash = chunk
                    .get("blake3")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid sampling evidence hash")
                    })?;
                let bytes = chunk
                    .get("bytes")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid sampling evidence size")
                    })?;
                chunks.insert((hash.to_owned(), bytes));
                if chunks.len() > BOUNDED_VALIDATION_MAX_REFERENCES {
                    return Err(bounded_read_exhausted("sampling evidence references"));
                }
            }
        }
        // Run the canonical evidence decoder before using the collected
        // references for a budget calculation.
        crate::session::sampling_evidence::referenced_hashes(timeline)?;
        Ok(chunks)
    }

    fn verify_prompt_blob_with_budget(
        session: &super::ContainedDirectory,
        hash: &str,
        remaining_bytes: &mut u64,
    ) -> io::Result<()> {
        let prompts = session.open_relative(
            Path::new("prompts"),
            "immutable prompt blob directory",
            false,
        )?;
        let file_name = format!("{hash}.txt");
        let mut file =
            prompts.open_regular(std::ffi::OsStr::new(&file_name), "immutable prompt blob")?;
        let length = file.metadata()?.len();
        if length > crate::session::persistence::MAX_IMMUTABLE_BLOB_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "immutable prompt blob exceeds its per-file limit",
            ));
        }
        if length > *remaining_bytes {
            return Err(bounded_read_exhausted("immutable prompt blobs"));
        }
        let mut bytes = Vec::new();
        file.take((*remaining_bytes).saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > *remaining_bytes || bytes.len() as u64 != length {
            return Err(bounded_read_exhausted("immutable prompt blobs"));
        }
        if blake3::hash(&bytes).to_hex().as_str() != hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "prompt blob hash mismatch",
            ));
        }
        *remaining_bytes -= bytes.len() as u64;
        Ok(())
    }

    pub(crate) fn materialize_timeline(
        &self,
        timeline_id: &str,
    ) -> io::Result<chat_state::TimelineMaterialization> {
        let validated = self.validated_timeline(timeline_id)?;
        let last_seq = validated
            .timeline
            .events()
            .last()
            .map(|event| event.seq.get())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "source Timeline is empty")
            })?;
        let timeline = validated.timeline;
        Ok(chat_state::TimelineMaterialization {
            input_ref: chat_state::TimelineRangeRef {
                timeline_id: timeline_id.to_string(),
                first_seq: 0,
                last_seq,
            },
            surface_revision: timeline.surface_revision(),
            surface: timeline.surface().to_vec(),
            surface_ids: timeline.surface_ids().to_vec(),
            direct_user_inputs: timeline.direct_user_permission_evidence(),
            permission_context: timeline.permission_classifier_context(),
            active_control_contexts: timeline.active_control_contexts(),
        })
    }
}
#[cfg(test)]
type AppendProbe = dyn Fn(AppendDurability) -> io::Result<()> + Send + Sync;
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateSyncPoint {
    File,
    Directory,
}
#[cfg(test)]
type UpdateSyncProbe = dyn Fn(UpdateSyncPoint) -> io::Result<()> + Send + Sync;
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimelineSyncPoint {
    File,
    Directory,
}
#[cfg(test)]
type TimelineSyncProbe = dyn Fn(TimelineSyncPoint) -> io::Result<()> + Send + Sync;
impl Default for JsonlStorageAdapter {
    fn default() -> Self {
        Self::new()
    }
}
impl JsonlStorageAdapter {
    /// Claim this session's exclusive writer epoch before reading any mutable
    /// projection. The same adapter is subsequently moved into the
    /// persistence actor, so the lease covers replay, reconciliation, and all
    /// live writes without a stale-snapshot window.
    pub(crate) async fn load_session_for_write_without_updates(
        &self,
        info: &Info,
    ) -> io::Result<super::PersistedDataLight> {
        self.ensure_writer_lease(info)?;
        self.load_light_data(info, true).await
    }

    async fn load_light_data(
        &self,
        info: &Info,
        restore_for_write: bool,
    ) -> io::Result<super::PersistedDataLight> {
        tracing::info!("Loading session data (without updates) from JSONL");
        let opened = if restore_for_write {
            self.open_session(info)?
        } else {
            self.open_session_for_observation(info)?
        };
        let summary = opened.summary().clone();
        let validated = opened.validated_timeline(&info.id.to_string())?;
        let timeline = validated.timeline;
        // Keep the already parsed complete physical snapshot alive through
        // load reconciliation. Its ids remain occupied even when client
        // replay later hides rows by cursor or rewind.
        let physical_updates = if restore_for_write {
            Some(Self::read_updates_from_directory(opened.directory())?)
        } else {
            None
        };
        let max_event_id_suffix = physical_updates
            .as_deref()
            .and_then(super::max_event_id_suffix);
        if restore_for_write {
            // Reconcile helpers may append durable facts during load, so seed
            // before they can emit any new event identity.
            super::reseed_event_counter_from_projection_expectations(&timeline);
            if let Some(max_suffix) = max_event_id_suffix {
                crate::util::event_id::ensure_event_counter_at_least(max_suffix.saturating_add(1));
            }
        }
        // Only the explicit new-writer load owns crash reconciliation. Merely
        // reading a live Session cannot end its still-running sidebands.
        if restore_for_write {
            self.recover_interrupted_sidebands(info, &validated.sidebands)
                .await?;
        }
        let summary = self
            .reconcile_session_title_projection(info, summary, &timeline, restore_for_write)
            .await?;
        let summary = self
            .reconcile_model_projection(info, summary, &timeline, restore_for_write)
            .await?;
        if restore_for_write {
            let existing = physical_updates
                .as_ref()
                .expect("writer load captured physical updates");
            let reconciled = crate::session::response_projection::reconcile_response_projections(
                &info.id,
                &timeline,
                existing.clone(),
            )?;
            for update in reconciled {
                let super::SessionUpdate::ResponseReplayProjection(projection) = update else {
                    continue;
                };
                let already_present = existing.iter().any(|update| {
                    matches!(
                        update,
                        super::SessionUpdate::ResponseReplayProjection(current)
                            if current.exact_match(&projection)
                    )
                });
                if !already_present {
                    self.append_update_to_file(
                        info,
                        &super::SessionUpdate::ResponseReplayProjection(projection),
                        AppendDurability::Durable,
                    )
                    .await?;
                }
            }
        }
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
        let workflow_runs = self.load_workflow_runs_sync(info, &timeline, opened.directory())?;
        let result = super::PersistedDataLight {
            summary,
            timeline,
            control_snapshot,
            signals,
            announcement_state,
            workflow_runs,
        };
        tracing::info!(
            session_id = %info.id,
            timeline_events = result.timeline.events().len(),
            has_signals = result.signals.is_some(),
            session_format_version = result.summary.session_format_version,
            "Session data loaded (without updates, rewind points deferred) from JSONL"
        );
        Ok(result)
    }

    pub fn new() -> Self {
        Self {
            dir_mode: SessionDirMode::FromRoot(crate::util::grow_home::grow_home()),
            authority: Default::default(),
            opened_sessions: Default::default(),
            writer_leases: Default::default(),
            timeline_prefixes: Default::default(),
            #[cfg(test)]
            update_append_probe: None,
            #[cfg(test)]
            update_sync_probe: None,
            #[cfg(test)]
            timeline_sync_probe: None,
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
            #[cfg(test)]
            update_sync_probe: None,
            #[cfg(test)]
            timeline_sync_probe: None,
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
            #[cfg(test)]
            update_sync_probe: None,
            #[cfg(test)]
            timeline_sync_probe: None,
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
            update_sync_probe: None,
            timeline_sync_probe: None,
        }
    }
    #[cfg(test)]
    fn with_update_sync_probe(
        session_dir: PathBuf,
        sync_probe: impl Fn(UpdateSyncPoint) -> io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            dir_mode: SessionDirMode::Explicit(session_dir),
            authority: Default::default(),
            opened_sessions: Default::default(),
            writer_leases: Default::default(),
            timeline_prefixes: Default::default(),
            update_append_probe: None,
            update_sync_probe: Some(std::sync::Arc::new(sync_probe)),
            timeline_sync_probe: None,
        }
    }
    #[cfg(test)]
    fn with_timeline_sync_probe(
        root: PathBuf,
        sync_probe: impl Fn(TimelineSyncPoint) -> io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            dir_mode: SessionDirMode::FromRoot(root),
            authority: Default::default(),
            opened_sessions: Default::default(),
            writer_leases: Default::default(),
            timeline_prefixes: Default::default(),
            update_append_probe: None,
            update_sync_probe: None,
            timeline_sync_probe: Some(std::sync::Arc::new(sync_probe)),
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
            direct_user_inputs: timeline.direct_user_permission_evidence(),
            permission_context: timeline.permission_classifier_context(),
            active_control_contexts: timeline.active_control_contexts(),
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
            .open_regular(
                std::ffi::OsStr::new(super::UPDATES_FILE),
                "session updates ledger",
            )?
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
        Self::read_sideband_ledgers_from_directory(&session, parent_timeline_id, parent)
    }

    /// Recover a completed image transcription that reached its durable
    /// Sideband terminal boundary before the parent ImageProjection commit.
    ///
    /// This closes the crash window without treating a sideband as a second
    /// model-facing store: the returned result is still only provenance for a
    /// new parent Timeline projection.
    pub(crate) fn recover_completed_image_descriptions_from_directory(
        session: &super::ContainedDirectory,
        parent_timeline_id: &str,
        source_revision: u64,
        queries: &[(chat_state::SurfaceId, String)],
    ) -> io::Result<Vec<Option<(String, chat_state::TimelineRangeRef)>>> {
        if queries.is_empty() {
            return Ok(Vec::new());
        }
        let events = super::read_committed_jsonl_from_directory::<chat_state::TimelineEvent>(
            session,
            std::ffi::OsStr::new(super::TIMELINE_FILE),
            "session Timeline ledger",
            super::MAX_JSONL_ENTRY_BYTES,
        )?;
        let parent = Timeline::from_events(events)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let ledgers =
            Self::read_sideband_ledgers_from_directory(session, parent_timeline_id, &parent)?;
        queries
            .iter()
            .map(|(source, prompt)| {
                Self::select_completed_image_description(
                    &parent,
                    &ledgers,
                    parent_timeline_id,
                    source_revision,
                    *source,
                    prompt,
                )
            })
            .collect()
    }

    fn select_completed_image_description(
        parent: &Timeline,
        ledgers: &super::SidebandLedgers,
        parent_timeline_id: &str,
        source_revision: u64,
        source: chat_state::SurfaceId,
        prompt: &str,
    ) -> io::Result<Option<(String, chat_state::TimelineRangeRef)>> {
        let candidates = parent
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Sideband(spawn)
                    if spawn.purpose == chat_state::SidebandPurpose::ImageDescription =>
                {
                    Some(spawn.sideband_id.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for sideband_id in candidates.into_iter().rev() {
            let Some(events) = ledgers.get(sideband_id) else {
                continue;
            };
            let Some(request) = events.first().and_then(|event| match &event.kind {
                chat_state::SidebandEventKind::Request(request) => Some(request),
                _ => None,
            }) else {
                continue;
            };
            if request.purpose != chat_state::SidebandPurpose::ImageDescription
                || request.prompt != prompt
            {
                continue;
            }
            let Some((result_event, result)) = events.iter().find_map(|event| match &event.kind {
                chat_state::SidebandEventKind::Result(result) => Some((event, result)),
                _ => None,
            }) else {
                continue;
            };
            let result_index = usize::try_from(result_event.seq).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "image description result seq exceeds platform capacity",
                )
            })?;
            if !matches!(
                events
                    .get(result_index.saturating_add(1))
                    .map(|event| &event.kind),
                Some(chat_state::SidebandEventKind::End(
                    chat_state::SidebandEnd {
                        outcome: chat_state::SidebandOutcome::Completed,
                        error: None,
                    }
                ))
            ) {
                continue;
            }
            let attempt_index = usize::try_from(result.source_event_seqs[1]).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "image description attempt seq exceeds platform capacity",
                )
            })?;
            let Some(attempt) = events
                .get(attempt_index)
                .and_then(|event| match &event.kind {
                    chat_state::SidebandEventKind::Attempt(attempt) => Some(attempt),
                    _ => None,
                })
            else {
                continue;
            };
            let manifest = &attempt.assembly_manifest;
            let source_event = source.event.get();
            let source_is_proven = manifest.strategy == "image-group"
                && manifest.strategy_version == 1
                && manifest.source_revision == Some(source_revision)
                && manifest.context_surface_ids.contains(&source)
                && manifest.selected_surface_ids.contains(&source)
                && attempt.input_refs.iter().any(|range| {
                    range.timeline_id == parent_timeline_id
                        && range.first_seq <= source_event
                        && source_event <= range.last_seq
                })
                && result.evidence_refs.iter().any(|range| {
                    range.timeline_id == parent_timeline_id
                        && range.first_seq <= source_event
                        && source_event <= range.last_seq
                });
            let description = result.raw_output.trim().to_owned();
            if source_is_proven && !description.is_empty() {
                return Ok(Some((
                    description,
                    chat_state::TimelineRangeRef {
                        timeline_id: sideband_id.to_owned(),
                        first_seq: result_event.seq,
                        last_seq: result_event.seq,
                    },
                )));
            }
        }
        Ok(None)
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
            let component = format!("Sideband schema for {sideband_id}");
            let events = match Self::read_versioned_jsonl_from_directory::<chat_state::SidebandEvent>(
                &sideband,
                std::ffi::OsStr::new(super::TIMELINE_FILE),
                "sideband Timeline ledger",
                super::MAX_JSONL_ENTRY_BYTES,
                &component,
                u64::from(chat_state::SIDEBAND_SCHEMA_VERSION),
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
            Self::append_sideband_event_with_durability(session, &event, AppendDurability::Durable)
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
        self.session_parent(info, true, false)
    }

    fn open_session_parent(&self, info: &Info) -> io::Result<super::ContainedDirectory> {
        self.session_parent(info, false, false)
    }

    fn session_parent(
        &self,
        info: &Info,
        create_missing: bool,
        shared_read: bool,
    ) -> io::Result<super::ContainedDirectory> {
        let authority = self.authority(create_missing)?;
        match &self.dir_mode {
            SessionDirMode::FromRoot(_) => {
                let encoded = crate::util::grow_home::encode_cwd_dirname(&info.cwd);
                let relative = Path::new("sessions").join(&encoded);
                let directory = if shared_read {
                    authority.open_relative_shared_read(&relative, "session storage directory")?
                } else {
                    authority.open_relative(
                        &relative,
                        "session storage directory",
                        create_missing,
                    )?
                };
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
    fn rewind_points_file(&self, info: &Info) -> PathBuf {
        self.session_dir(info).join("rewind_points.jsonl")
    }
    // Missing entries and invalid entities are established exclusions.
    // Operational errors cannot establish that a partial scan is complete.
    fn is_skippable_scan_error(error: &io::Error) -> bool {
        matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::InvalidData
        )
    }

    fn is_session_search_index_artifact(name: &std::ffi::OsStr) -> bool {
        let Some(name) = name.to_str() else {
            return false;
        };
        let sidecars = ["-wal", "-shm", "-journal"];
        if name == SESSION_SEARCH_INDEX_FILE
            || sidecars
                .iter()
                .any(|suffix| name.strip_suffix(suffix) == Some(SESSION_SEARCH_INDEX_FILE))
        {
            return true;
        }

        let base_stem = SESSION_SEARCH_INDEX_FILE
            .strip_suffix(".sqlite")
            .unwrap_or(SESSION_SEARCH_INDEX_FILE);
        let Some(per_host) = name
            .strip_prefix(base_stem)
            .and_then(|rest| rest.strip_prefix(".h-"))
        else {
            return false;
        };
        let Some((host, suffix)) = per_host.split_once(".sqlite") else {
            return false;
        };
        !host.is_empty()
            && host
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && (suffix.is_empty() || sidecars.contains(&suffix))
    }

    /// Enumerate identity-checked session entities from the pinned storage
    /// authority. Directory names, cwd markers, and Summary identity are one
    /// indivisible admission boundary; callers never receive an ambient path.
    fn scan_opened_sessions<T>(
        &self,
        cwd: Option<&str>,
        project: impl FnMut(OpenedSession) -> T,
    ) -> io::Result<Vec<T>> {
        self.scan_opened_sessions_with_policy(cwd, false, None, false, project)
    }

    fn scan_opened_sessions_with_policy<T>(
        &self,
        cwd: Option<&str>,
        fail_on_invalid_candidate: bool,
        max_directory_entries: Option<usize>,
        include_hidden: bool,
        mut project: impl FnMut(OpenedSession) -> T,
    ) -> io::Result<Vec<T>> {
        if !matches!(&self.dir_mode, SessionDirMode::FromRoot(_)) {
            return Ok(Vec::new());
        }
        let authority = self.authority(false)?;
        let sessions = match authority
            .open_relative_shared_read(Path::new("sessions"), "sessions directory")
        {
            Ok(sessions) => sessions,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut opened = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        let mut discovered_entries = 0usize;
        for cwd_name in
            Self::list_names_with_budget(&sessions, max_directory_entries, &mut discovered_entries)?
        {
            if cwd_name.to_string_lossy().starts_with('.') {
                continue;
            }
            if Self::is_session_search_index_artifact(&cwd_name) {
                continue;
            }
            let cwd_directory = match sessions
                .open_relative_shared_read(Path::new(&cwd_name), "session cwd directory")
            {
                Ok(directory) => directory,
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        || (!fail_on_invalid_candidate
                            && error.kind() == io::ErrorKind::InvalidData) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            };
            for session_name in Self::list_names_with_budget(
                &cwd_directory,
                max_directory_entries,
                &mut discovered_entries,
            )? {
                if session_name.to_string_lossy().starts_with('.') {
                    continue;
                }
                let directory = match cwd_directory
                    .open_relative_shared_read(Path::new(&session_name), "session directory")
                {
                    Ok(directory) => directory,
                    Err(error)
                        if error.kind() == io::ErrorKind::NotFound
                            || (!fail_on_invalid_candidate
                                && error.kind() == io::ErrorKind::InvalidData) =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                let summary = match Self::read_summary_from_directory(&directory) {
                    Ok(summary) => summary,
                    Err(error)
                        if !fail_on_invalid_candidate && Self::is_skippable_scan_error(&error) =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                if let Err(error) = Self::validate_physical_session_identity(
                    &cwd_name,
                    &cwd_directory,
                    &session_name,
                    &summary,
                ) {
                    if !fail_on_invalid_candidate && Self::is_skippable_scan_error(&error) {
                        continue;
                    }
                    return Err(error);
                }
                if summary.validate_current_format().is_err() {
                    if fail_on_invalid_candidate {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "session {} has an unsupported or invalid summary format",
                                summary.info.id
                            ),
                        ));
                    }
                    continue;
                }
                if let Err(error) = self.ensure_not_pending_reclaim(&summary.info) {
                    if !fail_on_invalid_candidate && error.kind() == io::ErrorKind::WouldBlock {
                        continue;
                    }
                    return Err(error);
                }
                if (!include_hidden && summary.is_hidden())
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
                    .get(&key)
                    .cloned()
                    .unwrap_or(candidate);
                let canonical_summary = Self::read_summary_from_directory(&directory)?;
                Self::validate_session_identity(&summary.info, &canonical_summary)?;
                let summary = canonical_summary;
                if max_directory_entries.is_some() && opened.len() >= TTL_DISCOVERY_MAX_CANDIDATES {
                    return Err(bounded_read_exhausted("TTL session candidates"));
                }
                opened.push(project(OpenedSession { directory, summary }));
            }
        }
        Ok(opened)
    }

    fn list_names_with_budget(
        directory: &super::ContainedDirectory,
        maximum: Option<usize>,
        discovered: &mut usize,
    ) -> io::Result<Vec<std::ffi::OsString>> {
        let Some(maximum) = maximum else {
            return directory.list_names();
        };
        let mut names = Vec::new();
        directory.visit_names(|name| {
            *discovered = (*discovered).saturating_add(1);
            if *discovered > maximum {
                return Err(bounded_read_exhausted("TTL session discovery"));
            }
            names.push(name.to_os_string());
            Ok(())
        })?;
        Ok(names)
    }

    pub(crate) fn list_sessions_sync(&self, cwd: Option<&str>) -> io::Result<Vec<Summary>> {
        let mut summaries = self.scan_opened_sessions(cwd, |opened| opened.summary)?;
        Self::sort_session_summaries(&mut summaries);
        Ok(summaries)
    }

    fn list_sessions_for_search_sync(&self) -> io::Result<Vec<Summary>> {
        let mut summaries =
            self.scan_opened_sessions_with_policy(None, true, None, false, |opened| {
                opened.summary
            })?;
        Self::sort_session_summaries(&mut summaries);
        Ok(summaries)
    }

    fn sort_session_summaries(summaries: &mut [Summary]) {
        summaries.sort_by_cached_key(|s| {
            (
                std::cmp::Reverse(s.last_active_at.unwrap_or(s.updated_at)),
                s.info.id.0.to_string(),
            )
        });
    }
    /// List the N most recently modified session summaries across all
    /// workspaces.
    ///
    /// Enumerates and validates local summaries, sorts them by activity, and
    /// truncates the result to `limit`. The blocking scan is isolated from
    /// async callers by the adapter's `spawn_blocking` boundary.
    pub async fn list_sessions_recent(&self, limit: usize) -> io::Result<Vec<Summary>> {
        let adapter = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut summaries = adapter.list_sessions_sync(None)?;
            summaries.truncate(limit);
            Ok(summaries)
        })
        .await
        .map_err(io::Error::other)?
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
        writer_owner: Self,
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
            let _writer_owner = writer_owner;
            #[cfg(test)]
            let sync_probe = _writer_owner.timeline_sync_probe.as_deref();
            Self::append_timeline_line_in_directory_sync_with_probe(
                &directory,
                &prefix_state,
                &path,
                line,
                event_seq,
                durability,
                #[cfg(test)]
                sync_probe,
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
        let directory = self.bound_session_directory(info)?;
        let prefix_state = self.timeline_prefix_state(info)?;
        Self::append_timeline_event_with_durability(
            self.clone(),
            directory,
            prefix_state,
            event,
            durability,
        )
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
            chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Consumed { input: Some(_), .. },
            ) => {
                patch.record_activity = true;
                patch.session_format_version = Some(SESSION_FORMAT_VERSION);
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
    /// is read in the uncontended writer epoch, so normal append cost is
    /// independent of ledger length. A file-stamp change forces full prefix
    /// validation before the append can proceed.
    fn append_timeline_line_in_directory_sync(
        directory: &super::ContainedDirectory,
        prefix_state: &std::sync::Mutex<Option<LedgerPrefix>>,
        path: &Path,
        line: Vec<u8>,
        event_seq: u64,
        durability: AppendDurability,
    ) -> io::Result<()> {
        Self::append_timeline_line_in_directory_sync_with_probe(
            directory,
            prefix_state,
            path,
            line,
            event_seq,
            durability,
            #[cfg(test)]
            None,
        )
    }

    fn append_timeline_line_in_directory_sync_with_probe(
        directory: &super::ContainedDirectory,
        prefix_state: &std::sync::Mutex<Option<LedgerPrefix>>,
        path: &Path,
        line: Vec<u8>,
        event_seq: u64,
        durability: AppendDurability,
        #[cfg(test)] sync_probe: Option<&TimelineSyncProbe>,
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
            let current_stamp = LedgerFileStamp::from_metadata(&file.metadata()?);

            let mut expected = prefix_state
                .lock()
                .map_err(|_| io::Error::other("Timeline prefix state poisoned"))?;
            let mut prefix_hasher = match expected.as_ref() {
                Some(prefix) if prefix.len == complete_len && prefix.stamp == current_stamp => {
                    prefix.hasher.clone()
                }
                Some(prefix) if prefix.len == complete_len => {
                    let actual = Self::load_timeline_prefix(&mut file, complete_len)?;
                    if actual.hash != prefix.hash {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "Timeline committed prefix changed: expected {} bytes/{}, found {} bytes/{}",
                                prefix.len,
                                prefix.hash.to_hex(),
                                actual.len,
                                actual.hash.to_hex(),
                            ),
                        ));
                    }
                    let hasher = actual.hasher.clone();
                    *expected = Some(actual);
                    hasher
                }
                Some(prefix) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "Timeline committed prefix length changed: expected {} bytes/{}, found {} bytes",
                            prefix.len,
                            prefix.hash.to_hex(),
                            complete_len,
                        ),
                    ));
                }
                None => {
                    let prefix = Self::load_timeline_prefix(&mut file, complete_len)?;
                    let hasher = prefix.hasher.clone();
                    *expected = Some(prefix);
                    hasher
                }
            };

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
                            #[cfg(test)]
                            if let Some(probe) = sync_probe {
                                probe(TimelineSyncPoint::File)?;
                            }
                            drop(file);
                            directory.sync()?;
                            #[cfg(test)]
                            if let Some(probe) = sync_probe {
                                probe(TimelineSyncPoint::Directory)?;
                            }
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
            let hash = prefix_hasher.finalize();
            *expected = Some(LedgerPrefix {
                len: complete_len.saturating_add(line.len() as u64),
                hash,
                stamp: LedgerFileStamp::from_metadata(&file.metadata()?),
                hasher: prefix_hasher,
            });
            if matches!(durability, AppendDurability::Durable) {
                Self::sync_file_durable(&file)?;
                #[cfg(test)]
                if let Some(probe) = sync_probe {
                    probe(TimelineSyncPoint::File)?;
                }
                drop(file);
                directory.sync()?;
                #[cfg(test)]
                if let Some(probe) = sync_probe {
                    probe(TimelineSyncPoint::Directory)?;
                }
            }
            Ok(())
        })();
        #[cfg(any(unix, windows))]
        {
            let _ = lock.unlock();
            result
        }
    }

    fn load_timeline_prefix(
        file: &mut std::fs::File,
        complete_len: u64,
    ) -> io::Result<LedgerPrefix> {
        file.seek(io::SeekFrom::Start(0))?;
        let mut hasher = blake3::Hasher::new();
        let mut timeline = chat_state::Timeline::default();
        let mut consumed = 0u64;
        // Reuse one bounded record buffer. The fold retains canonical events,
        // but validation never allocates a second copy of the whole JSONL file.
        let mut record = Vec::new();
        {
            let mut reader = io::BufReader::new((&mut *file).take(complete_len));
            loop {
                record.clear();
                let read = (&mut reader)
                    .take(super::MAX_JSONL_ENTRY_BYTES.saturating_add(1))
                    .read_until(b'\n', &mut record)?;
                if read == 0 {
                    break;
                }
                Self::validate_jsonl_line_size(&record, "Timeline event")?;
                consumed += read as u64;
                let json = record.strip_suffix(b"\n").ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Timeline committed prefix contains an incomplete record",
                    )
                })?;
                let event = serde_json::from_slice::<chat_state::TimelineEvent>(json)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                timeline
                    .accept(event)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                hasher.update(&record);
            }
        }
        if consumed != complete_len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Timeline changed while its committed prefix was being validated",
            ));
        }
        Ok(LedgerPrefix {
            len: complete_len,
            hash: hasher.finalize(),
            stamp: LedgerFileStamp::from_metadata(&file.metadata()?),
            hasher,
        })
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

    fn append_jsonl_line_in_directory_sync_with_probe(
        directory: &super::ContainedDirectory,
        name: &std::ffi::OsStr,
        path: &Path,
        mut line: Vec<u8>,
        durability: AppendDurability,
        #[cfg(test)] sync_probe: Option<&UpdateSyncProbe>,
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
                #[cfg(test)]
                if let Some(sync_probe) = sync_probe {
                    sync_probe(UpdateSyncPoint::File)?;
                }
                Self::sync_file_durable(&file)?;
                drop(file);
                #[cfg(test)]
                if let Some(sync_probe) = sync_probe {
                    sync_probe(UpdateSyncPoint::Directory)?;
                }
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

    fn append_jsonl_line_in_directory_sync(
        directory: &super::ContainedDirectory,
        name: &std::ffi::OsStr,
        path: &Path,
        line: Vec<u8>,
        durability: AppendDurability,
    ) -> io::Result<()> {
        #[cfg(test)]
        {
            Self::append_jsonl_line_in_directory_sync_with_probe(
                directory, name, path, line, durability, None,
            )
        }
        #[cfg(not(test))]
        {
            Self::append_jsonl_line_in_directory_sync_with_probe(
                directory, name, path, line, durability,
            )
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
    /// directory with a single capability-confined no-replace rename. Creation,
    /// fork, and import all use this transaction.
    pub(crate) fn build_and_publish_session<T>(
        parent: &super::ContainedDirectory,
        target_name: &std::ffi::OsStr,
        build: impl FnOnce(&super::ContainedDirectory) -> io::Result<T>,
    ) -> io::Result<T> {
        Self::build_and_publish_session_opened(parent, target_name, build).map(|(result, _)| result)
    }

    fn build_and_publish_session_opened<T>(
        parent: &super::ContainedDirectory,
        target_name: &std::ffi::OsStr,
        build: impl FnOnce(&super::ContainedDirectory) -> io::Result<T>,
    ) -> io::Result<(T, super::ContainedDirectory)> {
        // Staging identity is independent of the final name. Appending a
        // UUID to that name can exceed NAME_MAX even when the final name is
        // legal, and needlessly lengthens every staged artifact's path.
        let staging_name =
            std::ffi::OsString::from(format!(".{}.staging", uuid::Uuid::now_v7().simple()));
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
            Ok(result) => match staging.publish_child_no_replace(parent, target_name) {
                Ok(published) => {
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
                    return Ok((result, published));
                }
                Err(error) => Err(error),
            },
            Err(error) => {
                // Windows pins directory identity by denying
                // FILE_SHARE_DELETE; close the staging capability before
                // removing an unpublished tree.
                drop(staging);
                Err(error)
            }
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
        let target_name = self
            .session_dir(info)
            .file_name()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "session path has no final entity name",
                )
            })?
            .to_os_string();
        let (result, directory) =
            Self::build_and_publish_session_opened(parent, &target_name, build)?;
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

    fn read_versioned_jsonl_from_directory<T: serde::de::DeserializeOwned>(
        directory: &super::ContainedDirectory,
        name: &std::ffi::OsStr,
        description: &str,
        max_entry_bytes: u64,
        component: &str,
        current_version: u64,
    ) -> io::Result<Vec<T>> {
        let path = directory.display_path().join(name);
        super::read_committed_jsonl_from_directory::<Box<serde_json::value::RawValue>>(
            directory,
            name,
            description,
            max_entry_bytes,
        )?
        .into_iter()
        .enumerate()
        .map(|(index, raw)| {
            let value: serde_json::Value = serde_json::from_str(raw.get()).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}:{}: {error}", path.display(), index + 1),
                )
            })?;
            let persisted_version = value
                .get("version")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{}:{} has no valid schema version",
                            path.display(),
                            index + 1,
                        ),
                    )
                })?;
            if persisted_version != current_version {
                return Err(crate::session::persistence::session_version_mismatch(
                    component,
                    persisted_version,
                    current_version,
                ));
            }
            serde_json::from_str(raw.get()).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}:{}: {error}", path.display(), index + 1),
                )
            })
        })
        .collect()
    }

    fn read_timeline_bounded_from_directory(
        directory: &super::ContainedDirectory,
        remaining_bytes: &mut u64,
        remaining_events: &mut usize,
    ) -> io::Result<Vec<chat_state::TimelineEvent>> {
        Self::read_versioned_jsonl_bounded_from_directory(
            directory,
            std::ffi::OsStr::new(super::TIMELINE_FILE),
            "mandatory Timeline ledger",
            super::MAX_JSONL_ENTRY_BYTES,
            "Timeline schema",
            u64::from(chat_state::TIMELINE_SCHEMA_VERSION),
            remaining_bytes,
            remaining_events,
        )
    }

    fn read_versioned_jsonl_bounded_from_directory<T: serde::de::DeserializeOwned>(
        directory: &super::ContainedDirectory,
        name: &std::ffi::OsStr,
        description: &str,
        max_entry_bytes: u64,
        component: &str,
        current_version: u64,
        remaining_bytes: &mut u64,
        remaining_events: &mut usize,
    ) -> io::Result<Vec<T>> {
        let path = directory.display_path().join(name);
        let file = directory.open_regular(name, description)?;
        let frontier = file.metadata()?.len();
        if frontier > *remaining_bytes {
            return Err(bounded_read_exhausted(description));
        }
        // Limit the underlying reader to the observed frontier. Appends after
        // metadata capture cannot extend this observational snapshot.
        let mut reader = std::io::BufReader::new(file.take(frontier));
        let mut items = Vec::new();
        let mut line = Vec::new();
        let mut bytes_read = 0u64;
        let mut complete_records = 0usize;
        loop {
            line.clear();
            let count = reader.read_until(b'\n', &mut line)?;
            if count == 0 {
                break;
            }
            bytes_read = bytes_read.saturating_add(count as u64);
            if bytes_read > *remaining_bytes {
                return Err(bounded_read_exhausted(description));
            }
            if line.last() != Some(&b'\n') {
                // The final fragment was never a committed JSONL fact.
                break;
            }
            let entry_bytes = line.len().saturating_sub(1) as u64;
            if entry_bytes > max_entry_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{}:{} exceeds the per-entry byte limit",
                        path.display(),
                        complete_records + 1
                    ),
                ));
            }
            if complete_records >= *remaining_events {
                return Err(bounded_read_exhausted(description));
            }
            let value: serde_json::Value = serde_json::from_slice(&line).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}:{}: {error}", path.display(), complete_records + 1),
                )
            })?;
            let persisted_version = value
                .get("version")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{}:{} has no valid schema version",
                            path.display(),
                            complete_records + 1
                        ),
                    )
                })?;
            if persisted_version != current_version {
                return Err(crate::session::persistence::session_version_mismatch(
                    component,
                    persisted_version,
                    current_version,
                ));
            }
            items.push(serde_json::from_value(value).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}:{}: {error}", path.display(), complete_records + 1),
                )
            })?);
            complete_records += 1;
        }
        if bytes_read != frontier {
            return Err(bounded_read_exhausted(description));
        }
        *remaining_bytes -= frontier;
        *remaining_events -= complete_records;
        Ok(items)
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
        Self::read_timeline_from_directory(&directory).map_err(|error| {
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

    pub(crate) fn read_timeline_from_directory(
        directory: &super::ContainedDirectory,
    ) -> io::Result<Vec<chat_state::TimelineEvent>> {
        Self::read_versioned_jsonl_from_directory(
            directory,
            std::ffi::OsStr::new(super::TIMELINE_FILE),
            "mandatory Timeline ledger",
            super::MAX_JSONL_ENTRY_BYTES,
            "Timeline schema",
            u64::from(chat_state::TIMELINE_SCHEMA_VERSION),
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
        let directory = self.bound_session_directory(info)?;
        let path = directory.display_path().join(super::UPDATES_FILE);
        #[cfg(test)]
        let sync_probe = self.update_sync_probe.clone();
        tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            if let Some(sync_probe) = sync_probe {
                return Self::append_jsonl_line_in_directory_sync_with_probe(
                    &directory,
                    std::ffi::OsStr::new(super::UPDATES_FILE),
                    &path,
                    line,
                    durability,
                    Some(sync_probe.as_ref()),
                );
            }
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
        let payload_free_anchor = matches!(update,
            super::SessionUpdate::Acp(notification)
                if notification.meta.as_ref().is_some_and(|meta| {
                    meta.contains_key("samplingRequestId") && meta.contains_key("samplingAttempt")
                })
                && matches!(&notification.update,
                    acp::SessionUpdate::AgentMessageChunk(chunk)
                        if matches!(&chunk.content, acp::ContentBlock::Text(text) if text.text.is_empty())));
        self.apply_summary_patch(
            info,
            super::summary_write::SummaryPatch {
                record_activity: true,
                messages: (!payload_free_anchor)
                    .then_some(super::summary_write::CounterOp::Increment(1)),
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

    fn sync_updates_file_and_directory_sync(
        directory: &super::ContainedDirectory,
        #[cfg(test)] sync_probe: Option<&UpdateSyncProbe>,
    ) -> io::Result<()> {
        let file = directory.open_read_write_create(std::ffi::OsStr::new(super::UPDATES_FILE))?;
        #[cfg(test)]
        if let Some(sync_probe) = sync_probe {
            sync_probe(UpdateSyncPoint::File)?;
        }
        Self::sync_file_durable(&file)?;
        drop(file);
        #[cfg(test)]
        if let Some(sync_probe) = sync_probe {
            sync_probe(UpdateSyncPoint::Directory)?;
        }
        directory.sync()?;
        Ok(())
    }

    async fn sync_updates_file_and_directory(&self, info: &Info) -> io::Result<()> {
        let directory = self.bound_session_directory(info)?;
        #[cfg(test)]
        let sync_probe = self.update_sync_probe.clone();
        tokio::task::spawn_blocking(move || {
            #[cfg(test)]
            {
                Self::sync_updates_file_and_directory_sync(&directory, sync_probe.as_deref())
            }
            #[cfg(not(test))]
            {
                Self::sync_updates_file_and_directory_sync(&directory)
            }
        })
        .await
        .map_err(io::Error::other)?
    }

    fn raw_acp_event_status(
        &self,
        info: &Info,
        expected: &acp::SessionNotification,
    ) -> io::Result<RawExactStatus> {
        let event_id = expected
            .meta
            .as_ref()
            .and_then(|meta| meta.get("eventId"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "interleaved response update has no eventId",
                )
            })?;
        let expected = serde_json::to_value(expected)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let directory = self.bound_session_directory(info)?;
        let mut exact = false;
        for update in Self::read_updates_from_directory(&directory)? {
            let super::SessionUpdate::Acp(existing) = update else {
                continue;
            };
            let existing_id = existing
                .meta
                .as_ref()
                .and_then(|meta| meta.get("eventId"))
                .and_then(serde_json::Value::as_str);
            if existing_id == Some(event_id) {
                let actual = serde_json::to_value(&existing)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                if actual == expected {
                    exact = true;
                } else {
                    return Ok(RawExactStatus::Conflict);
                }
            }
        }
        Ok(if exact {
            RawExactStatus::Exact
        } else {
            RawExactStatus::Missing
        })
    }

    fn raw_projection_status(
        &self,
        info: &Info,
        expected: &crate::session::response_projection::ResponseReplayProjection,
    ) -> io::Result<RawExactStatus> {
        let directory = self.bound_session_directory(info)?;
        let mut exact = false;
        for update in Self::read_updates_from_directory(&directory)? {
            let super::SessionUpdate::ResponseReplayProjection(existing) = update else {
                continue;
            };
            if existing.request_id == expected.request_id && existing.attempt == expected.attempt {
                if existing.exact_match(expected) {
                    exact = true;
                } else {
                    return Ok(RawExactStatus::Conflict);
                }
            }
        }
        Ok(if exact {
            RawExactStatus::Exact
        } else {
            RawExactStatus::Missing
        })
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
        self.ensure_not_pending_reclaim(info)?;
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

    fn open_session_for_observation(&self, info: &Info) -> io::Result<OpenedSession> {
        self.ensure_not_pending_reclaim(info)?;
        let key = format!("{}\0{}", info.id, info.cwd);
        let has_pinned_entity = self
            .opened_sessions
            .lock()
            .map_err(|_| io::Error::other("session capability cache poisoned"))?
            .contains_key(&key);
        if has_pinned_entity {
            return self.open_session(info);
        }
        let parent = self.session_parent(info, false, true)?;
        let path = self.session_dir(info);
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "session path has no file name")
        })?;
        let directory =
            parent.open_relative_shared_read(Path::new(name), "session storage directory")?;
        let summary = Self::read_summary_from_directory(&directory)?;
        Self::validate_session_identity(info, &summary)?;
        // Observation sharing must not become a capability for later writes.
        Ok(OpenedSession {
            directory: std::sync::Arc::new(directory),
            summary,
        })
    }

    /// Return the already identity-checked entity capability used by this
    /// adapter's live writer. Canonical Timeline commits must not be blocked by
    /// a later-corrupted Summary projection; an unbound adapter still performs
    /// the full Summary identity check before obtaining the capability.
    fn bound_session_directory(
        &self,
        info: &Info,
    ) -> io::Result<std::sync::Arc<super::ContainedDirectory>> {
        let key = format!("{}\0{}", info.id, info.cwd);
        if let Some(directory) = self
            .opened_sessions
            .lock()
            .map_err(|_| io::Error::other("session capability cache poisoned"))?
            .get(&key)
            .cloned()
        {
            return Ok(directory);
        }
        Ok(self.open_session(info)?.directory)
    }

    pub(crate) fn open_session_by_id(&self, session_id: &str) -> io::Result<Option<OpenedSession>> {
        self.open_session_by_id_inner(session_id, false, None)
    }

    /// Resolve a session for a separate read-only process such as Trajectory.
    /// On Windows this uses directory handles that share namespace mutation so
    /// it can coexist with a live writer's publication handle. The returned
    /// capability is deliberately not inserted into the writer cache.
    pub(crate) fn open_session_by_id_shared_read(
        &self,
        session_id: &str,
    ) -> io::Result<Option<OpenedSession>> {
        self.open_session_by_id_inner(session_id, true, None)
    }

    /// Resolve a session for diagnostics or maintenance with a strict cwd
    /// directory scan budget. An incomplete scan is an error, never evidence
    /// that the requested session is absent.
    pub(crate) fn open_session_by_id_shared_read_bounded(
        &self,
        session_id: &str,
    ) -> io::Result<Option<OpenedSession>> {
        self.open_session_by_id_inner(session_id, true, Some(BOUNDED_SESSION_ID_SCAN_ENTRIES - 1))
    }

    fn open_session_by_id_inner(
        &self,
        session_id: &str,
        shared_read: bool,
        max_cwd_names: Option<usize>,
    ) -> io::Result<Option<OpenedSession>> {
        if !matches!(&self.dir_mode, SessionDirMode::FromRoot(_)) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "id-only resolution requires the canonical storage root",
            ));
        }
        let authority = if shared_read {
            let SessionDirMode::FromRoot(root) = &self.dir_mode else {
                unreachable!("id-only resolution checked canonical storage above")
            };
            super::ContainedDirectory::open_shared_read(
                root,
                Path::new(""),
                "session storage authority",
            )?
        } else {
            self.authority(false)?
        };
        let open_directory =
            |parent: &super::ContainedDirectory, relative: &Path, description: &str| {
                if shared_read {
                    parent.open_relative_shared_read(relative, description)
                } else {
                    parent.open_relative(relative, description, false)
                }
            };
        let sessions = match open_directory(&authority, Path::new("sessions"), "sessions directory")
        {
            Ok(sessions) => sessions,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut found: Option<OpenedSession> = None;
        let mut discovered_cwd_names = 0;
        let cwd_names =
            Self::list_names_with_budget(&sessions, max_cwd_names, &mut discovered_cwd_names)?;
        for cwd_name in cwd_names {
            if cwd_name.to_string_lossy().starts_with('.') {
                continue;
            }
            let cwd_dir =
                match open_directory(&sessions, Path::new(&cwd_name), "session cwd directory") {
                    Ok(directory) => directory,
                    Err(_) => continue,
                };
            let session = match open_directory(&cwd_dir, Path::new(session_id), "session directory")
            {
                Ok(directory) => directory,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let summary = match Self::read_summary_from_directory(&session) {
                Ok(summary) => summary,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            self.ensure_not_pending_reclaim(&summary.info)?;
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
            let directory = std::sync::Arc::new(session);
            if !shared_read {
                let key = format!("{}\0{}", summary.info.id, summary.info.cwd);
                self.opened_sessions
                    .lock()
                    .map_err(|_| io::Error::other("session capability cache poisoned"))?
                    .insert(key, directory.clone());
            }
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
        self.reject_linked_single_session_delete(&opened)?;
        let timeline_id = info.id.to_string();
        let timeline = opened.bounded_validated_timeline(&timeline_id)?.timeline;
        let runtime = timeline.runtime_lifecycle();
        if runtime.phase != chat_state::session_runtime::RuntimePhase::Running
            && (runtime.checkpoint.is_some() || runtime.operation_id.is_some())
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "session has an unfinished runtime handoff; resolve it before deleting history",
            ));
        }
        let parent = self.open_session_parent(&info)?;
        let name_string = info.id.to_string();
        let name = std::ffi::OsStr::new(&name_string);
        let quarantine_name =
            std::ffi::OsString::from(format!(".{}.deleting", uuid::Uuid::now_v7().simple()));
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
        let lease_name = Self::writer_lease_name(&info.id);
        match parent.remove_file(std::ffi::OsStr::new(&lease_name), true) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.opened_sessions
            .lock()
            .map_err(|_| io::Error::other("session capability cache poisoned"))?
            .remove(&key);
        self.timeline_prefixes
            .lock()
            .map_err(|_| io::Error::other("Timeline prefix cache poisoned"))?
            .remove(&key);
        Ok(())
    }

    fn reject_linked_single_session_delete(&self, opened: &OpenedSession) -> io::Result<()> {
        let info = &opened.summary.info;
        let timeline = opened
            .bounded_validated_timeline(&info.id.to_string())?
            .timeline;
        let mut linked = std::collections::BTreeSet::new();
        if let Some(parent_id) = &opened.summary.parent_session_id {
            linked.insert(parent_id.clone());
        }
        for event in timeline.events() {
            match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) => {
                    linked.insert(spawn.child_session_id.clone());
                }
                chat_state::TimelineEventKind::SubagentSeed(seed) => {
                    linked.insert(seed.parent_timeline_id.clone());
                }
                _ => {}
            }
        }
        for child in self.scan_opened_sessions_with_policy(
            None,
            true,
            Some(TTL_DISCOVERY_MAX_DIRECTORY_ENTRIES),
            true,
            |candidate| {
                let child_id = candidate.summary().info.id.0.to_string();
                let is_child = candidate.timeline_events()?.iter().any(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::SubagentSeed(seed)
                            if seed.parent_timeline_id == info.id.0.as_ref()
                    )
                });
                Ok::<_, io::Error>((child_id, is_child))
            },
        )? {
            let (child_id, is_child) = child?;
            if child_id != info.id.0.as_ref() && is_child {
                linked.insert(child_id);
            }
        }
        linked.remove(info.id.0.as_ref());
        if linked.is_empty() {
            return Ok(());
        }
        let identities = linked.into_iter().collect::<Vec<_>>().join(", ");
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "session {} is linked to parent or child session(s) {identities}; single-session deletion would remove recovery evidence. Automatic cleanup can reclaim the linked sessions together after all members are stale and eligible",
                info.id
            ),
        ))
    }

    fn ensure_writer_lease(&self, info: &Info) -> io::Result<()> {
        if !self.try_acquire_writer_lease(info)? {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!(
                    "session {} is being written or is part of an unfinished session reclaim",
                    info.id
                ),
            ));
        }
        Ok(())
    }

    fn ensure_not_pending_reclaim(&self, info: &Info) -> io::Result<()> {
        let Some(intents) = self.open_reclaim_intent_directory(false)? else {
            return Ok(());
        };
        // Reads only inspect atomically published intent files. They do not
        // participate in the global mutation lock: an unrelated cleanup must
        // never make an ordinary session load fail just because it is
        // recording or finishing a decision. Writer admission is fenced by
        // the per-session lease below, which cleanup must hold for every
        // member before it can publish an intent or rename any entity.
        if Self::has_pending_reclaim_for(&intents, info)? {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!(
                    "session {} is part of an unfinished committed cleanup; retry after recovery",
                    info.id
                ),
            ));
        }
        Ok(())
    }

    fn open_reclaim_intent_directory(
        &self,
        create_missing: bool,
    ) -> io::Result<Option<super::ContainedDirectory>> {
        let SessionDirMode::FromRoot(_) = &self.dir_mode else {
            return Ok(None);
        };
        let authority = match self.authority(create_missing) {
            Ok(authority) => authority,
            Err(error) if !create_missing && error.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let sessions = match if create_missing {
            authority.open_relative(Path::new("sessions"), "sessions directory", true)
        } else {
            authority.open_relative_shared_read(Path::new("sessions"), "sessions directory")
        } {
            Ok(sessions) => sessions,
            Err(error) if !create_missing && error.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        let intents = if create_missing {
            sessions.open_relative(
                Path::new(RECLAIM_INTENT_DIRECTORY),
                "session reclaim intent directory",
                true,
            )
        } else {
            sessions.open_relative_shared_read(
                Path::new(RECLAIM_INTENT_DIRECTORY),
                "session reclaim intent directory",
            )
        };
        match intents {
            Ok(intents) => Ok(Some(intents)),
            Err(error) if !create_missing && error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn acquire_reclaim_gate(&self) -> io::Result<Option<SessionReclaimGate>> {
        let Some(intents) = self.open_reclaim_intent_directory(true)? else {
            return Ok(None);
        };
        let authority = self.authority(true)?;
        let sessions =
            authority.open_relative(Path::new("sessions"), "sessions directory", true)?;
        let lock = sessions.open_read_write_create(std::ffi::OsStr::new(RECLAIM_GATE_FILE))?;
        match lock.try_lock_exclusive() {
            Ok(()) => Ok(Some(SessionReclaimGate {
                intents,
                _lock: lock,
            })),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "session reclaim decision is owned by another process",
            )),
            Err(error) => Err(error),
        }
    }

    fn read_reclaim_intents(
        directory: &super::ContainedDirectory,
    ) -> io::Result<Vec<(std::ffi::OsString, SessionReclaimIntent)>> {
        let mut discovered = 0usize;
        let mut names = Vec::new();
        directory.visit_names(|name| {
            discovered = discovered.saturating_add(1);
            if discovered > RECLAIM_INTENT_MAX_FILES.saturating_mul(2) {
                return Err(bounded_read_exhausted("session reclaim intent directory"));
            }
            names.push(name.to_os_string());
            Ok(())
        })?;
        names.sort();
        let mut intents = Vec::new();
        for name in names {
            if name.to_string_lossy().starts_with('.')
                || Path::new(&name).extension() != Some(std::ffi::OsStr::new("json"))
            {
                continue;
            }
            if intents.len() >= RECLAIM_INTENT_MAX_FILES {
                return Err(bounded_read_exhausted("session reclaim intents"));
            }
            let bytes = directory.read_bounded(
                &name,
                "session reclaim intent",
                RECLAIM_INTENT_MAX_BYTES,
            )?;
            let intent: SessionReclaimIntent = serde_json::from_slice(&bytes).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("session reclaim intent is invalid: {error}"),
                )
            })?;
            if intent.version != 1
                || intent.members.is_empty()
                || intent.members.len() > TTL_DISCOVERY_MAX_CANDIDATES
                || uuid::Uuid::parse_str(&intent.id)
                    .map_or(true, |id| id.simple().to_string() != intent.id)
                || name != std::ffi::OsString::from(format!("{}.json", intent.id))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session reclaim intent has invalid identity or member count",
                ));
            }
            Self::validate_reclaim_intent(&intent)?;
            intents.push((name, intent));
        }
        Ok(intents)
    }

    fn has_pending_reclaim_for(
        directory: &super::ContainedDirectory,
        info: &Info,
    ) -> io::Result<bool> {
        Ok(Self::read_reclaim_intents(directory)?
            .iter()
            .any(|(_, intent)| {
                intent
                    .members
                    .iter()
                    .any(|member| member.matches_info(info))
            }))
    }

    fn persist_reclaim_intent(
        &self,
        intent: &SessionReclaimIntent,
    ) -> io::Result<std::ffi::OsString> {
        let gate = self.acquire_reclaim_gate()?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "session reclaim requires the canonical storage root",
            )
        })?;
        let existing = Self::read_reclaim_intents(&gate.intents)?;
        if existing.iter().any(|(_, current)| {
            current.members.iter().any(|member| {
                intent
                    .members
                    .iter()
                    .any(|candidate| candidate.matches_info(&member.info()))
            })
        }) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "a session already belongs to an unfinished reclaim decision",
            ));
        }
        if existing.len() >= RECLAIM_INTENT_MAX_FILES {
            return Err(bounded_read_exhausted("session reclaim intents"));
        }
        let bytes = serde_json::to_vec(intent).map_err(io::Error::other)?;
        if bytes.len() as u64 > RECLAIM_INTENT_MAX_BYTES {
            return Err(bounded_read_exhausted("session reclaim intent"));
        }
        let name = std::ffi::OsString::from(format!("{}.json", intent.id));
        gate.intents.write_atomic(&name, &bytes, true, false)?;
        gate.intents.sync()?;
        Ok(name)
    }

    fn recover_pending_reclaims_sync(&self) -> io::Result<Vec<Info>> {
        // A normal cleanup/read with no committed decisions must stay
        // observational: do not create the reclaim directory or lock file
        // just to discover that there is nothing to recover.
        let Some(intents) = self.open_reclaim_intent_directory(false)? else {
            return Ok(Vec::new());
        };
        if Self::read_reclaim_intents(&intents)?.is_empty() {
            return Ok(Vec::new());
        }
        let Some(gate) = self.acquire_reclaim_gate()? else {
            return Ok(Vec::new());
        };
        let intents = Self::read_reclaim_intents(&gate.intents)?;
        let mut removed = Vec::new();
        for (intent_name, intent) in intents {
            for member in &intent.members {
                if !self.try_acquire_writer_lease_raw(&member.info())? {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        format!(
                            "cannot resume session reclaim {} while session {} has a writer",
                            intent.id, member.session_id
                        ),
                    ));
                }
            }
            // Validate every remaining namespace entry before deleting any of
            // them. A replacement entity keeps the whole intent pending.
            for member in &intent.members {
                self.validate_reclaim_member_location(member)?;
            }
            for member in &intent.members {
                self.finish_reclaim_member(member)?;
                removed.push(member.info());
            }
            gate.intents.remove_file(&intent_name, true)?;
            gate.intents.sync()?;
        }
        Ok(removed)
    }

    fn validate_reclaim_intent(intent: &SessionReclaimIntent) -> io::Result<()> {
        let mut previous: Option<(&str, &str)> = None;
        let mut identities = std::collections::BTreeSet::new();
        for (index, member) in intent.members.iter().enumerate() {
            let identity = (member.cwd.as_str(), member.session_id.as_str());
            let mut path_components = Path::new(&member.session_id).components();
            let session_name_is_single_component = matches!(
                path_components.next(),
                Some(std::path::Component::Normal(_))
            ) && path_components.next().is_none();
            if member.session_id.is_empty()
                || member.cwd.is_empty()
                || !session_name_is_single_component
                || previous.is_some_and(|last| last >= identity)
                || !identities.insert(member.session_id.as_str())
                || member.quarantine_name.is_empty()
                || member.quarantine_name.contains('/')
                || member.quarantine_name.contains('\\')
                || member.quarantine_name != format!(".reclaim-{}-{index}", intent.id)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "session reclaim intent has invalid or unordered members",
                ));
            }
            previous = Some(identity);
        }
        Ok(())
    }

    fn validate_reclaim_member_location(&self, member: &SessionReclaimMember) -> io::Result<()> {
        let info = member.info();
        let parent = self.open_session_parent(&info)?;
        let original_name = std::ffi::OsStr::new(&member.session_id);
        let original = match parent.open_relative(
            Path::new(original_name),
            "reclaim candidate session directory",
            false,
        ) {
            Ok(directory) => Some(directory),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let quarantined = match parent.open_relative(
            Path::new(&member.quarantine_name),
            "reclaim quarantine directory",
            false,
        ) {
            Ok(directory) => Some(directory),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if original.is_some() && quarantined.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "session {} has both its canonical and reclaim quarantine directories",
                    member.session_id
                ),
            ));
        }
        if original
            .as_ref()
            .or(quarantined.as_ref())
            .is_some_and(|directory| {
                directory.entity_identity().ok()
                    != Some((member.entity_volume, member.entity_index))
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "session {} changed identity during a committed reclaim",
                    member.session_id
                ),
            ));
        }
        Ok(())
    }

    fn finish_reclaim_member(&self, member: &SessionReclaimMember) -> io::Result<()> {
        let info = member.info();
        let parent = self.open_session_parent(&info)?;
        let original_name = std::ffi::OsStr::new(&member.session_id);
        let original = match parent.open_relative(
            Path::new(original_name),
            "reclaim candidate session directory",
            false,
        ) {
            Ok(directory) => Some(directory),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let quarantine_name = std::ffi::OsStr::new(&member.quarantine_name);
        if let Some(directory) = original {
            if directory.entity_identity()? != (member.entity_volume, member.entity_index) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "session {} changed identity during reclaim",
                        member.session_id
                    ),
                ));
            }
            parent.rename_child_no_replace(original_name, quarantine_name)?;
            parent.sync()?;
        }
        match parent.open_relative(
            Path::new(quarantine_name),
            "reclaim quarantine directory",
            false,
        ) {
            Ok(quarantined) => {
                if quarantined.entity_identity()? != (member.entity_volume, member.entity_index) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("session {} quarantine identity changed", member.session_id),
                    ));
                }
                quarantined.remove_all_contents()?;
                parent.remove_empty_child(quarantine_name, true)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let key = format!("{}\0{}", info.id, info.cwd);
        self.writer_leases
            .lock()
            .map_err(|_| io::Error::other("session writer lease cache poisoned"))?
            .remove(&key);
        let lease_name = Self::writer_lease_name(&info.id);
        match parent.remove_file(std::ffi::OsStr::new(&lease_name), true) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.opened_sessions
            .lock()
            .map_err(|_| io::Error::other("session capability cache poisoned"))?
            .remove(&key);
        self.timeline_prefixes
            .lock()
            .map_err(|_| io::Error::other("Timeline prefix cache poisoned"))?
            .remove(&key);
        Ok(())
    }

    /// Recovery must claim the writer before inspecting and extending a
    /// prefix. A previous actor or detached persistence worker keeps this
    /// lease unavailable until it has completely exited.
    pub(crate) fn claim_recovery_writer(&self, info: &Info) -> io::Result<()> {
        self.ensure_writer_lease(info)
    }

    pub(crate) async fn sync_recovery_prefix(&self, info: &Info) -> io::Result<()> {
        self.ensure_writer_lease(info)?;
        let directory = self.bound_session_directory(info)?;
        let owner = self.clone();
        tokio::task::spawn_blocking(move || {
            let _owner = owner;
            let file =
                directory.open_read_write_create(std::ffi::OsStr::new(super::TIMELINE_FILE))?;
            Self::sync_file_durable(&file)?;
            directory.sync()
        })
        .await
        .map_err(io::Error::other)?
    }

    fn writer_lease_name(id: &acp::SessionId) -> String {
        let name = format!(".{id}.writer.lock");
        let component_len = if cfg!(windows) {
            name.encode_utf16().count()
        } else {
            name.len()
        };
        if component_len <= 255 {
            return name;
        }
        // Locks must be deterministic across adapters/processes. Keep legal
        // existing lock names; use a disjoint suffix for oversized names so a
        // short ID equal to the digest cannot alias this lease.
        format!(".writer-{}.lock", blake3::hash(id.0.as_bytes()).to_hex())
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
        drop(leases);
        if self.has_pending_reclaim(info)? {
            return Ok(false);
        }
        if !self.try_acquire_writer_lease_raw(info)? {
            return Ok(false);
        }
        // Cleanup must hold every member's writer lease before publishing its
        // intent. This post-claim check closes the race where the intent is
        // published after our first snapshot: if cleanup won, release this
        // newly acquired lease; if we won, cleanup cannot commit its intent.
        match self.has_pending_reclaim(info) {
            Ok(false) => Ok(true),
            Ok(true) => {
                self.release_writer_lease(&key)?;
                Ok(false)
            }
            Err(error) => {
                self.release_writer_lease(&key)?;
                Err(error)
            }
        }
    }

    fn has_pending_reclaim(&self, info: &Info) -> io::Result<bool> {
        let Some(intents) = self.open_reclaim_intent_directory(false)? else {
            return Ok(false);
        };
        Self::has_pending_reclaim_for(&intents, info)
    }

    fn release_writer_lease(&self, key: &str) -> io::Result<()> {
        self.writer_leases
            .lock()
            .map_err(|_| io::Error::other("session writer lease cache poisoned"))?
            .remove(key);
        Ok(())
    }

    fn try_acquire_writer_lease_raw(&self, info: &Info) -> io::Result<bool> {
        let key = format!("{}\0{}", info.id, info.cwd);
        let mut leases = self
            .writer_leases
            .lock()
            .map_err(|_| io::Error::other("session writer lease cache poisoned"))?;
        if leases.contains_key(&key) {
            return Ok(true);
        }
        let parent = self.open_session_parent(info)?;
        let lease_name = Self::writer_lease_name(&info.id);
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
        mut on_deleted: impl FnMut(&Info),
    ) -> io::Result<(u32, u32)> {
        if ttl_days == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session cleanup TTL must be positive",
            ));
        }
        let cutoff = chrono::Utc::now()
            .checked_sub_signed(chrono::Duration::days(i64::from(ttl_days)))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "session cleanup cutoff exceeds supported date range",
                )
            })?;
        let mut deleted = 0u32;
        let mut errors = 0u32;
        for info in self.recover_pending_reclaims_sync()? {
            deleted = deleted.saturating_add(1);
            on_deleted(&info);
        }

        // Finish bounded discovery before mutation. Hidden sessions remain in
        // the identity map because they can still be endpoints of a visible
        // session's recovery chain; they are never TTL candidates themselves.
        let summaries = self.scan_opened_sessions_with_policy(
            None,
            false,
            Some(TTL_DISCOVERY_MAX_DIRECTORY_ENTRIES),
            true,
            |opened: OpenedSession| opened.summary,
        )?;
        let mut known = std::collections::BTreeMap::new();
        for summary in &summaries {
            if known
                .insert(summary.info.id.0.to_string(), summary.info.clone())
                .is_some()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("duplicate canonical session id {}", summary.info.id),
                ));
            }
        }
        let stale_candidates: Vec<_> = summaries
            .into_iter()
            .filter(|summary| {
                !summary.is_hidden()
                    && !skip_session_dir.is_some_and(|skip| self.session_dir(&summary.info) == skip)
                    && summary.last_active_at.unwrap_or(summary.updated_at) < cutoff
            })
            .collect();
        let mut handled = std::collections::BTreeSet::new();
        for summary in stale_candidates {
            let start_id = summary.info.id.0.to_string();
            if handled.contains(&start_id) {
                continue;
            }
            let candidate = Self {
                opened_sessions: Default::default(),
                writer_leases: Default::default(),
                timeline_prefixes: Default::default(),
                ..self.clone()
            };
            let initial_members = match candidate.collect_reclaim_component(&summary.info, &known) {
                Ok(members) => members,
                Err(error) => {
                    errors = errors.saturating_add(1);
                    handled.insert(start_id);
                    tracing::warn!(%error, session_id = %summary.info.id, "failed to validate linked session component");
                    continue;
                }
            };
            let mut infos: Vec<_> = initial_members
                .iter()
                .map(|member| member.summary().info.clone())
                .collect();
            infos.sort_by(|left, right| {
                left.cwd
                    .cmp(&right.cwd)
                    .then_with(|| left.id.0.as_ref().cmp(right.id.0.as_ref()))
            });
            for info in &infos {
                handled.insert(info.id.0.to_string());
            }

            let mut can_reclaim = true;
            for info in &infos {
                if skip_session_dir.is_some_and(|skip| self.session_dir(info) == skip) {
                    can_reclaim = false;
                    break;
                }
                match candidate.try_acquire_writer_lease(info) {
                    Ok(true) => {}
                    Ok(false) => {
                        can_reclaim = false;
                        break;
                    }
                    Err(error) => {
                        errors = errors.saturating_add(1);
                        tracing::warn!(%error, session_id = %info.id, "failed to claim linked session writer");
                        can_reclaim = false;
                        break;
                    }
                }
            }
            if !can_reclaim {
                continue;
            }

            let mut current_members = Vec::with_capacity(initial_members.len());
            let mut identity_changed = false;
            for initial in &initial_members {
                let info = &initial.summary().info;
                let current = match candidate.open_session_parent(info).and_then(|parent| {
                    parent.open_relative(
                        Path::new(info.id.0.as_ref()),
                        "claimed session directory",
                        false,
                    )
                }) {
                    Ok(directory) => directory,
                    Err(error) => {
                        errors = errors.saturating_add(1);
                        tracing::warn!(%error, session_id = %info.id, "failed to reopen claimed session");
                        identity_changed = true;
                        break;
                    }
                };
                if !initial
                    .directory()
                    .is_same_entity(&current)
                    .unwrap_or(false)
                {
                    identity_changed = true;
                    break;
                }
                let current_summary = match Self::read_summary_from_directory(&current) {
                    Ok(summary) => summary,
                    Err(error) => {
                        errors = errors.saturating_add(1);
                        tracing::warn!(%error, session_id = %info.id, "failed to reread claimed session summary");
                        identity_changed = true;
                        break;
                    }
                };
                if Self::validate_session_identity(info, &current_summary).is_err()
                    || current_summary.validate_current_format().is_err()
                    || current_summary.is_hidden()
                    || current_summary
                        .last_active_at
                        .unwrap_or(current_summary.updated_at)
                        >= cutoff
                {
                    identity_changed = true;
                    break;
                }
                current_members.push(OpenedSession {
                    directory: std::sync::Arc::new(current),
                    summary: current_summary,
                });
            }
            if identity_changed || current_members.len() != infos.len() {
                continue;
            }
            match candidate.reclaim_component_is_closed(&current_members) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    errors = errors.saturating_add(1);
                    tracing::warn!(%error, session_id = %summary.info.id, "linked session result chain could not be validated");
                    continue;
                }
            }

            if current_members.len() == 1 {
                match candidate.delete_opened_session(current_members.into_iter().next().unwrap()) {
                    Ok(()) => {
                        deleted = deleted.saturating_add(1);
                        on_deleted(&summary.info);
                    }
                    Err(error) => {
                        errors = errors.saturating_add(1);
                        tracing::warn!(%error, session_id = %summary.info.id, "failed to delete stale unlinked session");
                    }
                }
                continue;
            }

            let intent_id = uuid::Uuid::now_v7().simple().to_string();
            current_members.sort_by(|left, right| {
                left.summary()
                    .info
                    .cwd
                    .cmp(&right.summary().info.cwd)
                    .then_with(|| {
                        left.summary()
                            .info
                            .id
                            .0
                            .as_ref()
                            .cmp(right.summary().info.id.0.as_ref())
                    })
            });
            let mut members = Vec::with_capacity(current_members.len());
            for (index, current) in current_members.iter().enumerate() {
                let (entity_volume, entity_index) = match current.directory().entity_identity() {
                    Ok(identity) => identity,
                    Err(error) => {
                        errors = errors.saturating_add(1);
                        tracing::warn!(%error, session_id = %current.summary().info.id, "session identity is unavailable for durable reclaim");
                        members.clear();
                        break;
                    }
                };
                members.push(SessionReclaimMember {
                    session_id: current.summary().info.id.0.to_string(),
                    cwd: current.summary().info.cwd.clone(),
                    entity_volume,
                    entity_index,
                    quarantine_name: format!(".reclaim-{intent_id}-{index}"),
                });
            }
            if members.len() != current_members.len() {
                continue;
            }
            let intent = SessionReclaimIntent {
                version: 1,
                id: intent_id,
                members,
            };
            Self::validate_reclaim_intent(&intent)?;
            if let Err(error) = candidate.persist_reclaim_intent(&intent) {
                errors = errors.saturating_add(1);
                tracing::warn!(%error, session_id = %summary.info.id, "failed to commit linked session reclaim");
                continue;
            }
            drop(current_members);
            drop(candidate);
            match self.recover_pending_reclaims_sync() {
                Ok(removed_members) => {
                    for info in &removed_members {
                        deleted = deleted.saturating_add(1);
                        on_deleted(info);
                    }
                }
                Err(error) => {
                    errors = errors.saturating_add(1);
                    tracing::warn!(%error, reclaim_id = %intent.id, "committed session reclaim remains pending recovery");
                }
            }
        }
        Ok((deleted, errors))
    }

    /// Recheck the same entity after gaining exclusive writer ownership.
    fn delete_if_still_stale(
        &self,
        mut opened: OpenedSession,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> io::Result<bool> {
        if !self.try_acquire_writer_lease(&opened.summary().info)? {
            tracing::debug!(session_id = %opened.summary().info.id, reason = "live_writer", "retaining stale session");
            return Ok(false);
        }
        let current = Self::read_summary_from_directory(opened.directory())?;
        Self::validate_session_identity(&opened.summary().info, &current)?;
        current.validate_current_format()?;
        if current.is_hidden() || current.last_active_at.unwrap_or(current.updated_at) >= cutoff {
            return Ok(false);
        }
        // TTL is only an activity filter. Recovery-bearing Timeline facts
        // remain authoritative while their lifecycle or receipt is open.
        let timeline_id = current.info.id.to_string();
        let timeline = opened.bounded_validated_timeline(&timeline_id)?.timeline;
        if self.timeline_has_recovery_dependencies(&timeline, &timeline_id)? {
            tracing::debug!(%timeline_id, reason = "unresolved_recovery_dependency", "retaining stale session");
            return Ok(false);
        }
        opened.summary = current;
        self.delete_opened_session(opened)?;
        Ok(true)
    }

    fn collect_reclaim_component(
        &self,
        start: &Info,
        known: &std::collections::BTreeMap<String, Info>,
    ) -> io::Result<Vec<OpenedSession>> {
        let mut pending = std::collections::VecDeque::from([start.id.0.to_string()]);
        let mut members = std::collections::BTreeMap::<String, OpenedSession>::new();
        while let Some(session_id) = pending.pop_front() {
            if members.contains_key(&session_id) {
                continue;
            }
            if members.len() >= BOUNDED_VALIDATION_MAX_REFERENCES {
                return Err(bounded_read_exhausted("linked session component"));
            }
            let info = known.get(&session_id).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("linked session {session_id} is missing from bounded discovery"),
                )
            })?;
            let opened = self.open_session(info)?;
            let timeline = opened.bounded_validated_timeline(&session_id)?.timeline;
            let mut linked = std::collections::BTreeSet::new();
            for event in timeline.events() {
                match &event.kind {
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Spawned(spawn),
                    ) => {
                        // Even an Ended fact without a result ref still names
                        // a cross-session source. Require that endpoint to be
                        // present and validate it as part of the same group.
                        linked.insert(spawn.child_session_id.clone());
                    }
                    chat_state::TimelineEventKind::SubagentSeed(seed) => {
                        linked.insert(seed.parent_timeline_id.clone());
                    }
                    _ => {}
                }
            }
            if let Some(parent_id) = &opened.summary().parent_session_id {
                linked.insert(parent_id.clone());
            }
            for linked_id in linked {
                if !known.contains_key(&linked_id) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("linked session {linked_id} is missing from bounded discovery"),
                    ));
                }
                if !members.contains_key(&linked_id) {
                    pending.push_back(linked_id);
                }
            }
            members.insert(session_id, opened);
        }
        Ok(members.into_values().collect())
    }

    fn reclaim_component_is_closed(&self, members: &[OpenedSession]) -> io::Result<bool> {
        let by_id: std::collections::BTreeMap<_, _> = members
            .iter()
            .map(|member| (member.summary().info.id.0.to_string(), member))
            .collect();
        for member in members {
            let info = &member.summary().info;
            let timeline = member
                .bounded_validated_timeline(&info.id.to_string())?
                .timeline;
            if self.timeline_has_local_recovery_dependencies(&timeline)? {
                return Ok(false);
            }
            if let Some(parent_id) = &member.summary().parent_session_id {
                // Summary parentage points at the lifecycle root, while a
                // nested child's seed can name its direct lifecycle owner.
                // Both identities must be present; the seed below validates
                // the exact direct-parent spawn edge.
                if !by_id.contains_key(parent_id) {
                    return Ok(false);
                }
            }
            for event in timeline.events() {
                match &event.kind {
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Spawned(spawn),
                    ) => {
                        let terminal =
                            timeline
                                .events()
                                .iter()
                                .find_map(|candidate| match &candidate.kind {
                                    chat_state::TimelineEventKind::Subagent(
                                        chat_state::SubagentEvent::Ended(ended),
                                    ) if ended.subagent_id == spawn.subagent_id => Some(ended),
                                    _ => None,
                                });
                        let Some(terminal) = terminal else {
                            return Ok(false);
                        };
                        if terminal.result_ref.is_some() {
                            let Some(child) = by_id.get(&spawn.child_session_id) else {
                                return Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    format!(
                                        "child {} is outside its reclaim component",
                                        spawn.child_session_id
                                    ),
                                ));
                            };
                            let child_timeline = child
                                .bounded_validated_timeline(&spawn.child_session_id)?
                                .timeline;
                            child_timeline
                                .validate_subagent_result_link(
                                    &info.id.to_string(),
                                    event.seq,
                                    spawn,
                                    terminal,
                                )
                                .map_err(|error| {
                                    io::Error::new(io::ErrorKind::InvalidData, error)
                                })?;
                        } else if by_id.contains_key(&spawn.child_session_id) {
                            return Ok(false);
                        }
                        if Self::subagent_receipt_unresolved(&timeline, spawn, terminal) {
                            return Ok(false);
                        }
                    }
                    chat_state::TimelineEventKind::SubagentSeed(seed) => {
                        let Some(parent) = by_id.get(&seed.parent_timeline_id) else {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "parent {} is outside its reclaim component",
                                    seed.parent_timeline_id
                                ),
                            ));
                        };
                        let parent_timeline = parent
                            .bounded_validated_timeline(&seed.parent_timeline_id)?
                            .timeline;
                        let Some(spawn_event) = parent_timeline.events().iter().find(|candidate| {
                            candidate.seq.get() == seed.parent_spawn_seq
                                && matches!(
                                    &candidate.kind,
                                    chat_state::TimelineEventKind::Subagent(
                                        chat_state::SubagentEvent::Spawned(spawn)
                                    ) if spawn.subagent_id == seed.subagent_id
                                )
                        }) else {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "child seed has no exact parent spawn source",
                            ));
                        };
                        let chat_state::TimelineEventKind::Subagent(
                            chat_state::SubagentEvent::Spawned(spawn),
                        ) = &spawn_event.kind
                        else {
                            unreachable!("matched parent spawn above")
                        };
                        let Some(terminal) =
                            parent_timeline
                                .events()
                                .iter()
                                .find_map(|candidate| match &candidate.kind {
                                    chat_state::TimelineEventKind::Subagent(
                                        chat_state::SubagentEvent::Ended(ended),
                                    ) if ended.subagent_id == seed.subagent_id => Some(ended),
                                    _ => None,
                                })
                        else {
                            return Ok(false);
                        };
                        if member
                            .bounded_validated_timeline(&info.id.to_string())?
                            .timeline
                            .validate_subagent_result_link(
                                &seed.parent_timeline_id,
                                spawn_event.seq,
                                spawn,
                                terminal,
                            )
                            .is_err()
                            || Self::subagent_receipt_unresolved(&parent_timeline, spawn, terminal)
                        {
                            return Ok(false);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(true)
    }

    /// Fail closed when Timeline state or a cross-session subagent result
    /// cannot be proven complete. The caller holds this candidate's writer
    /// lease for the entire check and deletion boundary.
    fn timeline_has_recovery_dependencies(
        &self,
        timeline: &Timeline,
        timeline_id: &str,
    ) -> io::Result<bool> {
        if self.timeline_has_local_recovery_dependencies(timeline)? {
            return Ok(true);
        }

        let child_spawns: Vec<_> = timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) => Some((event.seq, spawn)),
                _ => None,
            })
            .collect();
        if child_spawns.len() > 4096 {
            return Err(io::Error::other(
                "recovery dependency scan budget exhausted",
            ));
        }
        for (spawn_seq, spawn) in child_spawns {
            let Some(terminal) = timeline
                .events()
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                        ended,
                    )) if ended.subagent_id == spawn.subagent_id => Some(ended),
                    _ => None,
                })
            else {
                return Ok(true);
            };
            if Self::subagent_receipt_unresolved(timeline, spawn, terminal) {
                return Ok(true);
            }
            if terminal.result_ref.is_none() {
                // A never-published child has no nested entity to protect.
                continue;
            }
            let child_storage = Self {
                opened_sessions: Default::default(),
                writer_leases: Default::default(),
                timeline_prefixes: Default::default(),
                ..self.clone()
            };
            let Some(child) =
                child_storage.open_session_by_id_shared_read_bounded(&spawn.child_session_id)?
            else {
                return Ok(true);
            };
            if !child_storage.try_acquire_writer_lease(&child.summary().info)? {
                return Ok(true);
            }
            let child_timeline = child
                .bounded_validated_timeline(&spawn.child_session_id)?
                .timeline;
            child_timeline
                .validate_subagent_result_link(timeline_id, spawn_seq, spawn, terminal)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            if Self::timeline_has_unresolved_local_dependencies(&child_timeline) {
                return Ok(true);
            }
        }
        let seeds = timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::SubagentSeed(seed) => Some(seed),
                _ => None,
            });
        for seed in seeds {
            let parent_storage = Self {
                opened_sessions: Default::default(),
                writer_leases: Default::default(),
                timeline_prefixes: Default::default(),
                ..self.clone()
            };
            // A live parent writer owns the mutable result boundary. We must
            // not infer quiescence from a concurrent read of its Timeline.
            let Some(parent) =
                parent_storage.open_session_by_id_shared_read_bounded(&seed.parent_timeline_id)?
            else {
                return Ok(true);
            };
            if !parent_storage.try_acquire_writer_lease(&parent.summary().info)? {
                return Ok(true);
            }
            let parent_timeline = parent
                .bounded_validated_timeline(&seed.parent_timeline_id)?
                .timeline;
            let spawn_event = parent_timeline.events().iter().find(|event| {
                event.seq.get() == seed.parent_spawn_seq
                    && matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Subagent(
                            chat_state::SubagentEvent::Spawned(spawn)
                        ) if spawn.subagent_id == seed.subagent_id
                    )
            });
            let Some(spawn_event) = spawn_event else {
                return Ok(true);
            };
            let chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn)) =
                &spawn_event.kind
            else {
                unreachable!("spawn event was matched above")
            };
            let Some(terminal) =
                parent_timeline
                    .events()
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Subagent(
                            chat_state::SubagentEvent::Ended(ended),
                        ) if ended.subagent_id == seed.subagent_id => Some(ended),
                        _ => None,
                    })
            else {
                return Ok(true);
            };
            if timeline
                .validate_subagent_result_link(
                    &seed.parent_timeline_id,
                    spawn_event.seq,
                    spawn,
                    terminal,
                )
                .is_err()
                || Self::timeline_has_unresolved_local_dependencies(&parent_timeline)
                || Self::subagent_receipt_unresolved(&parent_timeline, spawn, terminal)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn timeline_has_local_recovery_dependencies(&self, timeline: &Timeline) -> io::Result<bool> {
        if Self::timeline_has_unresolved_local_dependencies(timeline) {
            return Ok(true);
        }

        let runtime = timeline.runtime_lifecycle();
        if runtime.phase != chat_state::session_runtime::RuntimePhase::Running
            && (runtime.checkpoint.is_some() || runtime.operation_id.is_some())
        {
            return Ok(true);
        }
        if let Some(planning) = timeline.planning_snapshot() {
            let plan = &planning.plan;
            let has_unresolved_replans = plan.replans.iter().any(|replan| {
                !matches!(
                    &replan.progress,
                    tool_types::planning::ReplanProgress::Handled { .. }
                        | tool_types::planning::ReplanProgress::Unapplied { .. }
                )
            });
            if !plan.waits.is_empty()
                || !plan.unresolved_required_deliveries().is_empty()
                || has_unresolved_replans
                || plan.links.iter().any(|link| !link.terminal)
            {
                return Ok(true);
            }
        }
        if let Some(control) =
            crate::session::control::SessionControlSnapshot::latest_from_timeline(
                timeline.events(),
            )?
        {
            let planning_is_open = matches!(
                control.behavior.state,
                crate::session::behavior::BehaviorState::Plan(_)
            );
            let retained_decision_is_open =
                control
                    .behavior
                    .retained_plan
                    .as_ref()
                    .is_some_and(|candidate| {
                        matches!(
                            candidate.status,
                            crate::session::behavior::RetainedPlanStatus::Deferred
                                | crate::session::behavior::RetainedPlanStatus::Reviewing
                        )
                    });
            if planning_is_open || retained_decision_is_open {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn subagent_receipt_unresolved(
        parent: &Timeline,
        spawn: &chat_state::SubagentSpawnEvent,
        terminal: &chat_state::SubagentTerminalEvent,
    ) -> bool {
        if !spawn.surface_completion || terminal.outcome == chat_state::SubagentOutcome::Cancelled {
            return false;
        }
        let owner = match (&spawn.goal_id, spawn.goal_definition_revision) {
            (Some(goal_id), Some(definition_revision)) if definition_revision > 0 => {
                chat_state::NotificationOwner::Goal {
                    goal_id: goal_id.clone(),
                    definition_revision,
                }
            }
            (None, None) => chat_state::NotificationOwner::Session,
            _ => return true,
        };
        let source = chat_state::NotificationSource::SubagentCompleted {
            subagent_id: spawn.subagent_id.clone(),
            owner,
        };
        let version = chat_state::NotificationSourceVersion::Ordinal { value: 1 };
        let Some(id) = parent.received_notification_id(&source, &version) else {
            return true;
        };
        parent
            .pending_notifications()
            .iter()
            .any(|receipt| receipt.id == id)
    }

    fn timeline_has_unresolved_local_dependencies(timeline: &Timeline) -> bool {
        let has_unresolved_input = timeline.events().iter().any(|event| {
            let chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted {
                input_id,
                ..
            }) = &event.kind
            else {
                return false;
            };
            timeline.input_state(input_id).is_some_and(|input| {
                matches!(
                    input.state,
                    chat_state::InputState::Submitted | chat_state::InputState::Allowed { .. }
                )
            })
        });
        let mut open_subagents = std::collections::BTreeSet::new();
        for event in timeline.events() {
            match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) => {
                    open_subagents.insert(spawn.subagent_id.clone());
                }
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                    ended,
                )) => {
                    open_subagents.remove(&ended.subagent_id);
                }
                _ => {}
            }
        }
        timeline.active_turn().is_some()
            || timeline.open_request_ids().next().is_some()
            || timeline.open_tool_call_ids().next().is_some()
            || has_unresolved_input
            || !timeline.pending_notifications().is_empty()
            || !open_subagents.is_empty()
    }

    fn read_summary_from_directory(directory: &super::ContainedDirectory) -> io::Result<Summary> {
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
        crate::session::persistence::decode_summary(&bytes)
    }

    fn load_workflow_runs_sync(
        &self,
        info: &Info,
        timeline: &Timeline,
        session: &super::ContainedDirectory,
    ) -> io::Result<Vec<crate::session::workflow::store::RestoredWorkflowRun>> {
        use crate::session::workflow::store::{
            MAX_RESTORED_WORKFLOW_RUNS, MAX_WORKFLOW_ARGS_BYTES, MAX_WORKFLOW_MANIFEST_BYTES,
        };
        let run_ids = timeline
            .events()
            .iter()
            .rev()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Workflow(chat_state::WorkflowEvent::Spawned {
                    run_id,
                    ..
                }) => Some(run_id.as_str()),
                _ => None,
            });
        let mut restored = Vec::new();
        // Validate newest candidates first, but count only successfully restored
        // runs toward the cap. Invalid/cleared/missing sidecars must not crowd
        // out an older valid run. Reverse again below so callers retain Timeline
        // order.
        for run_id in run_ids {
            let lifecycle = timeline.workflow_lifecycle(run_id).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Workflow Timeline spawn has no lifecycle projection: {run_id}"),
                )
            })?;
            let run_relative = Path::new("workflows").join(run_id);
            let run_dir =
                match session.open_relative_shared_read(&run_relative, "Workflow run directory") {
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
            let mut corrupt_sidecar_fingerprint = None;
            let sidecar = match run_dir.read_bounded(
                std::ffi::OsStr::new("state.json"),
                "Workflow manifest",
                MAX_WORKFLOW_MANIFEST_BYTES,
            ) {
                Ok(bytes) => {
                    match crate::session::workflow::store::decode_workflow_manifest(&bytes) {
                        Ok(manifest) => Some(manifest),
                        Err(error) => {
                            corrupt_sidecar_fingerprint = Some(
                                crate::session::workflow::store::WorkflowManifestFingerprint::of(
                                    &bytes,
                                ),
                            );
                            tracing::warn!(path = %manifest_path.display(), %error, "using the Timeline seed for an invalid Workflow manifest");
                            None
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    tracing::warn!(path = %manifest_path.display(), "using the Timeline seed for a missing Workflow manifest");
                    None
                }
                Err(error) => {
                    tracing::warn!(
                        path = %manifest_path.display(),
                        %error,
                        "skipping inaccessible Workflow manifest"
                    );
                    continue;
                }
            };
            let resolution = crate::session::workflow::store::resolve_workflow_restore_manifest(
                &run_id, &lifecycle, sidecar,
            )?;
            if resolution.used_timeline_seed {
                tracing::warn!(path = %manifest_path.display(), "using the Timeline seed instead of Workflow sidecar progress");
            }
            let manifest = resolution.manifest;
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
            if crate::session::workflow::store::workflow_script_hash(&script)
                != lifecycle.script_hash
            {
                tracing::warn!(path = %script_path.display(), "skipping Workflow with an immutable script hash mismatch");
                continue;
            }
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
            if crate::session::workflow::store::workflow_args_hash(&args)? != lifecycle.args_hash {
                tracing::warn!(path = %args_path.display(), "skipping Workflow with an immutable args hash mismatch");
                continue;
            }
            restored.push(crate::session::workflow::store::RestoredWorkflowRun {
                manifest,
                script,
                args,
                corrupt_sidecar_fingerprint,
            });
            if restored.len() == MAX_RESTORED_WORKFLOW_RUNS {
                tracing::warn!(
                    session_id = %info.id,
                    limit = MAX_RESTORED_WORKFLOW_RUNS,
                    "workflow restore cap reached; restoring the most recent valid Timeline-owned runs"
                );
                break;
            }
        }
        restored.reverse();
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
        // Claim the canonical identity before publishing it. This also makes
        // initialization contend with a committed reclaim intent rather than
        // recreating a directory that recovery has not finished removing.
        self.ensure_writer_lease(info)?;
        let result = self.build_publish_and_cache(info, &parent, |staging| {
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
        })?;
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
        let directory = self.bound_session_directory(info)?;
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
        repair: bool,
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
        // Observation derives the same canonical projection without claiming
        // a writer lease. Only an admitted replacement writer repairs disk.
        if repair {
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
        repair: bool,
    ) -> io::Result<Summary> {
        let Some((model_id, reasoning_effort, _, _)) =
            crate::session::persistence::latest_model_selection(timeline.events())?
        else {
            return Ok(summary);
        };
        if summary.current_model_id == model_id && summary.reasoning_effort == reasoning_effort {
            return Ok(summary);
        }
        if repair {
            self.update_current_model_and_agent(info, &model_id, None, Some(reasoning_effort))
                .await?;
        }
        summary.current_model_id = model_id;
        summary.reasoning_effort = reasoning_effort;
        Ok(summary)
    }
}
/// Transform session ID in a SessionUpdate
fn transform_session_id_in_update(
    update: super::SessionUpdate,
    new_id: &acp::SessionId,
) -> Vec<super::SessionUpdate> {
    match update {
        super::SessionUpdate::Acp(mut notification) => {
            notification.session_id = new_id.clone();
            vec![super::SessionUpdate::Acp(notification)]
        }
        super::SessionUpdate::Grow(mut notification) => {
            notification.session_id = new_id.clone();
            vec![super::SessionUpdate::Grow(notification)]
        }
        super::SessionUpdate::ResponseReplayProjection(projection) => {
            projection
                .into_notifications()
                .into_iter()
                .map(|mut notification| {
                    notification.session_id = new_id.clone();
                    // A fork is a new lineage. The projection is expanded into
                    // ordinary inherited ACP history, so the child must not
                    // inherit the parent's response/candidate identity metadata.
                    notification.meta = None;
                    super::SessionUpdate::Acp(Box::new(notification))
                })
                .collect()
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
                    | crate::extensions::notification::SessionUpdate::ControlStateUpdate(_)
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
        self.build_publish_and_cache(target_info, &parent, |staging| {
            let source_summary = source_session.summary().clone();
            let source_timeline = source_session
                .validated_timeline(&source_info.id.to_string())?
                .timeline;
            let mut surface_to_copy = source_timeline.surface().to_vec();
            let mut updates_to_copy: Vec<super::SessionUpdate> =
                Self::read_updates_from_directory(source_session.directory())?;
            if !options.fork_filter {
                // Reconcile against the validated parent Timeline before any
                // fork truncation. This removes stale/candidate rows, dedups
                // exact projections, and synthesizes a missing projection at
                // its canonical parent position. The projection is expanded
                // below into ordinary child history; no child admission fact
                // is fabricated.
                updates_to_copy =
                    crate::session::response_projection::reconcile_response_projections(
                        &source_info.id,
                        &source_timeline,
                        updates_to_copy,
                    )?;
            }
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
                    // A fork is a new authority domain. Historical text remains
                    // useful context, but first-party ingress evidence belongs
                    // to the parent ledger and must not authorize actions in the
                    // child. Subagent capability ceilings are delegated through
                    // their typed spawn request, never through copied messages.
                    user.permission_evidence = None;
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
            let transformed_updates: Vec<super::SessionUpdate> = updates_to_copy
                .into_iter()
                .flat_map(|u| transform_session_id_in_update(u, &target_info.id))
                .collect();
            let num_messages = transformed_updates.len();
            let target_model_id = options
                .new_model_id
                .map(crate::agent::models::ModelId::new)
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
                    control.applied_control = None;
                    let behavior_normalized = matches!(
                        control.behavior.state,
                        crate::session::behavior::BehaviorState::Plan(_)
                            | crate::session::behavior::BehaviorState::Workflow
                            | crate::session::behavior::BehaviorState::Goal
                    );
                    if behavior_normalized {
                        control.behavior = crate::session::behavior::BehaviorSnapshot::normal();
                    } else {
                        // Submission identities belong to one session lineage.
                        // A fork carries the historical projection, not the
                        // parent's ability to decide an old submission.
                        control.behavior.retained_plan = None;
                        control.behavior.last_plan_submission_id = 0;
                        control.behavior.plan_submission_id = None;
                    }
                    control.control_revision = control.control_revision.saturating_add(1);
                    let control_kind = if behavior_normalized {
                        control.timeline_kind_with_model_context(
                            chat_state::ControlContextLayer::Behavior,
                            chat_state::ControlContextActivation::Transition,
                            crate::session::behavior::behavior_transition_context(
                                tool_types::BehaviorId::Normal,
                            ),
                        )?
                    } else {
                        control.timeline_kind()?
                    };
                    fork_timeline
                        .record(control_kind)
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
        })
    }
}
#[async_trait]
impl StorageAdapter for JsonlStorageAdapter {
    async fn init_session(
        &self,
        info: &Info,
        model_id: crate::agent::models::ModelId,
    ) -> io::Result<Summary> {
        match self.open_session(info) {
            Ok(opened) => {
                tracing::info!("Loading existing session from JSONL");
                let summary = opened.summary().clone();
                let _ = opened.validated_timeline(&info.id.to_string())?;
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
    async fn append_acp_event_exact(
        &self,
        info: &Info,
        notification: &acp::SessionNotification,
    ) -> Result<(), super::AppendUpdateError> {
        self.ensure_writer_lease(info)
            .map_err(super::AppendUpdateError::NotCommitted)?;
        let update = super::SessionUpdate::Acp(Box::new(notification.clone()));
        match self
            .raw_acp_event_status(info, notification)
            .map_err(super::AppendUpdateError::NotCommitted)?
        {
            RawExactStatus::Exact => self
                .sync_updates_file_and_directory(info)
                .await
                .map_err(super::AppendUpdateError::NotCommitted),
            RawExactStatus::Conflict => {
                Err(super::AppendUpdateError::NotCommitted(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ACP eventId conflicts with persisted event",
                )))
            }
            RawExactStatus::Missing => {
                let append = self.append_update_durable_commit_aware(info, &update).await;
                match append {
                    Ok(()) => match self.raw_acp_event_status(info, notification) {
                        Ok(RawExactStatus::Exact) => Ok(()),
                        Ok(RawExactStatus::Conflict) => {
                            Err(super::AppendUpdateError::NotCommitted(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "ACP eventId conflicts with persisted event",
                            )))
                        }
                        Ok(RawExactStatus::Missing) => {
                            Err(super::AppendUpdateError::NotCommitted(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "durable ACP event append is not physically present",
                            )))
                        }
                        Err(error) => Err(super::AppendUpdateError::NotCommitted(error)),
                    },
                    Err(error) => match self.raw_acp_event_status(info, notification) {
                        Ok(RawExactStatus::Exact) => self
                            .sync_updates_file_and_directory(info)
                            .await
                            .map_err(super::AppendUpdateError::NotCommitted),
                        Ok(RawExactStatus::Conflict) => {
                            Err(super::AppendUpdateError::NotCommitted(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "ACP eventId conflicts with persisted event",
                            )))
                        }
                        Ok(RawExactStatus::Missing) | Err(_) => Err(error),
                    },
                }
            }
        }
    }
    async fn commit_response_projection(
        &self,
        info: &Info,
        projection: &crate::session::response_projection::ResponseReplayProjection,
    ) -> Result<(), super::AppendUpdateError> {
        self.ensure_writer_lease(info)
            .map_err(super::AppendUpdateError::NotCommitted)?;
        let update = super::SessionUpdate::ResponseReplayProjection(Box::new(projection.clone()));
        match self
            .raw_projection_status(info, projection)
            .map_err(super::AppendUpdateError::NotCommitted)?
        {
            RawExactStatus::Exact => self
                .sync_updates_file_and_directory(info)
                .await
                .map_err(super::AppendUpdateError::NotCommitted),
            RawExactStatus::Conflict => {
                Err(super::AppendUpdateError::NotCommitted(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "response projection identity conflicts with persisted projection",
                )))
            }
            RawExactStatus::Missing => {
                let append = self.append_update_durable_commit_aware(info, &update).await;
                match append {
                    Ok(()) => match self.raw_projection_status(info, projection) {
                        Ok(RawExactStatus::Exact) => Ok(()),
                        Ok(RawExactStatus::Conflict) => {
                            Err(super::AppendUpdateError::NotCommitted(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "response projection identity conflicts with persisted projection",
                            )))
                        }
                        Ok(RawExactStatus::Missing) => {
                            Err(super::AppendUpdateError::NotCommitted(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "durable response projection append is not physically present",
                            )))
                        }
                        Err(error) => Err(super::AppendUpdateError::NotCommitted(error)),
                    },
                    Err(error) => match self.raw_projection_status(info, projection) {
                        Ok(RawExactStatus::Exact) => self
                            .sync_updates_file_and_directory(info)
                            .await
                            .map_err(super::AppendUpdateError::NotCommitted),
                        Ok(RawExactStatus::Conflict) => {
                            Err(super::AppendUpdateError::NotCommitted(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "response projection identity conflicts with persisted projection",
                            )))
                        }
                        Ok(RawExactStatus::Missing) | Err(_) => Err(error),
                    },
                }
            }
        }
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
        model_id: &crate::agent::models::ModelId,
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
                &session, &manifest,
            )
        })
        .await
        .map_err(io::Error::other)?
    }
    async fn repair_corrupt_workflow_run_state(
        &self,
        info: &Info,
        manifest: &crate::session::workflow::store::WorkflowRunManifest,
        fingerprint: crate::session::workflow::store::WorkflowManifestFingerprint,
    ) -> io::Result<()> {
        self.ensure_writer_lease(info)?;
        let session = self.open_session(info)?.directory;
        let manifest = manifest.clone();
        tokio::task::spawn_blocking(move || {
            crate::session::workflow::store::repair_corrupt_workflow_run_manifest_in_directory(
                &session,
                &manifest,
                fingerprint,
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
            crate::session::workflow::store::tombstone_workflow_run_in_directory(&session, &run_id)
        })
        .await
        .map_err(io::Error::other)?
    }
    async fn load_session(&self, info: &Info) -> io::Result<PersistedData> {
        let opened = self.open_session_for_observation(info)?;
        let summary = opened.summary().clone();
        let validated = opened.validated_timeline(&info.id.to_string())?;
        let timeline = validated.timeline;
        let summary = self
            .reconcile_session_title_projection(info, summary, &timeline, false)
            .await?;
        let summary = self
            .reconcile_model_projection(info, summary, &timeline, false)
            .await?;
        let control_snapshot =
            crate::session::control::SessionControlSnapshot::latest_from_timeline(
                timeline.events(),
            )?;
        let updates = Self::read_updates_from_directory(opened.directory())?;
        let updates = crate::session::response_projection::reconcile_response_projections(
            &info.id, &timeline, updates,
        )?;
        let signals =
            crate::session::signals::SessionSignals::latest_from_timeline(timeline.events())?;
        let announcement_state =
            crate::session::announcement_state::AnnouncementState::latest_from_timeline(
                timeline.events(),
            )?;
        let workflow_runs = self.load_workflow_runs_sync(info, &timeline, opened.directory())?;
        let rewind_points = Self::read_jsonl_from_directory::<RewindPoint>(
            opened.directory(),
            std::ffi::OsStr::new("rewind_points.jsonl"),
            "rewind points ledger",
        )?;
        let result = PersistedData {
            summary,
            timeline_events: timeline.events().to_vec(),
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
        self.load_light_data(info, false).await
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
    async fn list_sessions_for_search(&self) -> io::Result<Vec<Summary>> {
        let adapter = self.clone();
        tokio::task::spawn_blocking(move || adapter.list_sessions_for_search_sync())
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
    async fn replace_rewind_points(&self, info: &Info, points: &[RewindPoint]) -> io::Result<()> {
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
        let opened = self.open_session(info)?;
        let timeline_id = info.id.to_string();
        tokio::task::spawn_blocking(move || {
            let timeline = opened.validated_timeline(&timeline_id)?.timeline;
            Ok(timeline.prompt_records())
        })
        .await
        .map_err(io::Error::other)?
    }
    fn open_timeline_reader(&self, info: &Info) -> io::Result<super::TimelineLedgerReader> {
        // Projection readers must not populate the adapter's writer cache.
        let directory = self.session_directory(info, false)?;
        let summary = Self::read_summary_from_directory(&directory)?;
        Self::validate_session_identity(info, &summary)?;
        let file = directory
            .open_regular(
                std::ffi::OsStr::new(super::TIMELINE_FILE),
                "mandatory Timeline ledger",
            )
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
            directory.display_path().join(super::TIMELINE_FILE),
        )
    }
}
#[cfg(test)]
mod durable_tests;
#[cfg(test)]
mod tests;

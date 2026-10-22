//! Pager-owned recovery for input which has not crossed the ACP prompt RPC.
//!
//! This is deliberately not session state.  It never replays a prompt and it
//! never stores Shell control-plane projections.  Recovery only repopulates
//! the local composer and the pre-admission Behavior latch.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::app::agent_view::AgentView;
use crate::app::session::{AgentId, ChipElement, QueueEntryKind};
use crate::views::prompt_widget::{KIND_FILE_REF, KIND_IMAGE, KIND_PASTE, StashedPrompt};

const SCHEMA_VERSION: u32 = 1;
const DEBOUNCE: Duration = Duration::from_millis(200);
const WRITE_RETRY_BACKOFF: Duration = Duration::from_secs(1);
const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_TEXT_BYTES: usize = 128 * 1024;
const MAX_CHIPS: usize = 256;
const MAX_RECORDS: usize = 64;
const MAX_QUARANTINE_RECORDS: usize = 64;
const MAX_QUARANTINE_BYTES: u128 = 16 * 1024 * 1024;
type QuarantineEntry = (SystemTime, std::ffi::OsString, PathBuf, u64);

#[cfg(test)]
#[derive(Debug)]
struct StoreGate {
    operation: &'static str,
    entered: std::sync::Barrier,
    release: std::sync::Barrier,
    fired: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl StoreGate {
    fn new(operation: &'static str) -> Arc<Self> {
        Arc::new(Self {
            operation,
            entered: std::sync::Barrier::new(2),
            release: std::sync::Barrier::new(2),
            fired: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn before(&self, operation: &'static str) {
        if self.operation == operation
            && !self.fired.swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            self.entered.wait();
            self.release.wait();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LocalDraftKey {
    Session(String),
    Cwd(PathBuf),
}

impl LocalDraftKey {
    fn session(id: &str) -> io::Result<Self> {
        if id.is_empty() || id.len() > 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid session draft key",
            ));
        }
        Ok(Self::Session(id.to_owned()))
    }

    fn cwd(path: &Path) -> io::Result<Self> {
        Ok(Self::Cwd(dunce::canonicalize(path)?))
    }

    fn filename(&self) -> String {
        let (prefix, identity) = match self {
            Self::Session(id) => ("session", id.as_bytes()),
            Self::Cwd(path) => ("cwd", path.as_os_str().as_encoded_bytes()),
        };
        format!("{prefix}-{}.json", blake3::hash(identity).to_hex())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DraftChip {
    start: usize,
    end: usize,
    kind: u8,
    display: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DraftPrompt {
    pub(crate) text: String,
    pub(crate) cursor: usize,
    chips: Vec<DraftChip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    frozen_skill_selection: Option<crate::app::session::FrozenSkillSelection>,
}

impl DraftPrompt {
    fn from_parts(text: &str, cursor: usize, chips: &[ChipElement]) -> io::Result<Self> {
        if text.len() > MAX_TEXT_BYTES || chips.len() > MAX_CHIPS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "local draft exceeds limits",
            ));
        }
        if cursor > text.len() || !text.is_char_boundary(cursor) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid local draft cursor",
            ));
        }
        let chips = chips
            .iter()
            .map(|chip| {
                let kind = if chip.kind == KIND_PASTE {
                    1
                } else if chip.kind == KIND_FILE_REF {
                    2
                } else if chip.kind == KIND_IMAGE {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "image chips are not persisted in local drafts",
                    ));
                } else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "unknown composer chip kind",
                    ));
                };
                if chip.range.start > chip.range.end
                    || chip.range.end > text.len()
                    || !text.is_char_boundary(chip.range.start)
                    || !text.is_char_boundary(chip.range.end)
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid chip range",
                    ));
                }
                let display = chip.display.as_ref().map(|line| {
                    let mut text = String::new();
                    for span in &line.spans {
                        text.push_str(span.content.as_ref());
                    }
                    text
                });
                Ok(DraftChip {
                    start: chip.range.start,
                    end: chip.range.end,
                    kind,
                    display,
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            text: text.to_owned(),
            cursor,
            chips,
            frozen_skill_selection: None,
        })
    }

    fn from_parts_with_skill(
        text: &str,
        cursor: usize,
        chips: &[ChipElement],
        frozen_skill_selection: Option<crate::app::session::FrozenSkillSelection>,
    ) -> io::Result<Self> {
        if frozen_skill_selection
            .as_ref()
            .is_some_and(|selection| !valid_frozen_skill_selection(selection))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid frozen skill selection",
            ));
        }
        let mut prompt = Self::from_parts(text, cursor, chips)?;
        prompt.frozen_skill_selection = frozen_skill_selection;
        Ok(prompt)
    }

    fn chip_elements(&self) -> Vec<ChipElement> {
        self.chips
            .iter()
            .map(|chip| ChipElement {
                range: chip.start..chip.end,
                kind: match chip.kind {
                    1 => KIND_PASTE,
                    2 => KIND_FILE_REF,
                    _ => unreachable!("validated local draft chip kind"),
                },
                display: chip.display.clone().map(ratatui::text::Line::from),
            })
            .collect()
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.chips.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LocalDraftRecord {
    version: u32,
    revision: u64,
    composer: Option<DraftPrompt>,
    staged_prompt: Option<DraftPrompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uncertain_submission: Option<UncertainSubmission>,
    deferred_session_mode: Option<tools::types::BehaviorId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UncertainSubmission {
    prompt_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_turn_id: Option<String>,
    prompt: DraftPrompt,
}

impl LocalDraftRecord {
    fn empty(revision: u64) -> Self {
        Self {
            version: SCHEMA_VERSION,
            revision,
            composer: None,
            staged_prompt: None,
            uncertain_submission: None,
            deferred_session_mode: None,
        }
    }

    fn has_payload(&self) -> bool {
        self.composer.as_ref().is_some_and(|p| !p.is_empty())
            || self.staged_prompt.as_ref().is_some_and(|p| !p.is_empty())
            || self.uncertain_submission.is_some()
            || self.deferred_session_mode.is_some()
    }
}

#[derive(Debug)]
pub(crate) struct LocalDraftStore {
    root: PathBuf,
    #[cfg(test)]
    gate: Option<Arc<StoreGate>>,
}

impl Default for LocalDraftStore {
    fn default() -> Self {
        Self::new(tools::util::grow_home::grow_home().join("pager-drafts-v1"))
    }
}

impl LocalDraftStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            #[cfg(test)]
            gate: None,
        }
    }

    fn path(&self, key: &LocalDraftKey) -> PathBuf {
        self.root.join(key.filename())
    }

    fn ensure_root(&self) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        set_mode(&self.root, 0o700)
    }

    pub(crate) fn load(&self, key: &LocalDraftKey) -> io::Result<Option<LocalDraftRecord>> {
        #[cfg(test)]
        if let Some(gate) = &self.gate {
            gate.before("load");
        }
        let path = self.path(key);
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "local draft source is not a regular file",
            ));
        }
        if metadata.len() > MAX_RECORD_BYTES as u64 {
            self.quarantine(&path, &file)?;
            return Ok(None);
        }
        self.load_from_reader(&path, &file)
    }

    fn load_from_reader(&self, path: &Path, reader: &File) -> io::Result<Option<LocalDraftRecord>> {
        let mut bytes = Vec::new();
        reader
            .take(MAX_RECORD_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_RECORD_BYTES {
            self.quarantine(path, reader)?;
            return Ok(None);
        }
        let record = serde_json::from_slice::<LocalDraftRecord>(&bytes).ok();
        let Some(record) = record.filter(|r| r.version == SCHEMA_VERSION && validate_record(r))
        else {
            self.quarantine(path, reader)?;
            return Ok(None);
        };
        Ok(Some(record))
    }

    pub(crate) fn write(&self, key: &LocalDraftKey, record: &LocalDraftRecord) -> io::Result<()> {
        #[cfg(test)]
        if let Some(gate) = &self.gate {
            gate.before("write");
        }
        if record.version != SCHEMA_VERSION || !validate_record(record) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid local draft record",
            ));
        }
        if !record.has_payload() {
            return self.remove(key);
        }
        self.ensure_root()?;
        let path = self.path(key);
        if !path.exists() && count_records(&self.root)? >= MAX_RECORDS {
            return Err(io::Error::other("local draft record limit reached"));
        }
        let bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "serialized local draft exceeds limit",
            ));
        }
        let mut tmp = tempfile::NamedTempFile::new_in(&self.root)?;
        set_mode(tmp.path(), 0o600)?;
        tmp.write_all(&bytes)?;
        tmp.as_file().sync_all()?;
        // `std::fs::rename` cannot replace an existing destination on Windows.
        // `persist` supplies the platform-specific atomic replacement required
        // after the first draft write.
        tmp.persist(&path).map_err(|error| error.error)?;
        sync_dir(&self.root)?;
        Ok(())
    }

    pub(crate) fn remove(&self, key: &LocalDraftKey) -> io::Result<()> {
        #[cfg(test)]
        if let Some(gate) = &self.gate {
            gate.before("remove");
        }
        match fs::remove_file(self.path(key)) {
            Ok(()) => sync_dir(&self.root),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Lossless atomic rename from the canonical cwd key to the bound session key.
    pub(crate) fn rekey(&self, from: &LocalDraftKey, to: &LocalDraftKey) -> io::Result<()> {
        #[cfg(test)]
        if let Some(gate) = &self.gate {
            gate.before("rekey");
        }
        if from == to {
            return Ok(());
        }
        self.ensure_root()?;
        let from_path = self.path(from);
        if !from_path.exists() {
            return Ok(());
        }
        let to_path = self.path(to);
        fs::rename(from_path, to_path)?;
        sync_dir(&self.root)
    }

    fn quarantine(&self, path: &Path, source: &File) -> io::Result<()> {
        // Recovery reads through `source`, but quarantine moves a directory
        // entry. Refuse to move a replacement which appeared after the open.
        // `metadata` intentionally follows regular-file symlinks, matching the
        // source entity whose bytes were read while leaving the symlink itself
        // available for the existing rename behavior.
        if !path_still_names_source(path, source)? {
            return Ok(());
        }
        self.ensure_root()?;
        let dir = self.root.join("quarantine");
        fs::create_dir_all(&dir)?;
        set_mode(&dir, 0o700)?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("draft");
        // Directory setup may take time. Recheck immediately before the
        // pathname operation so a replacement during setup is left untouched.
        loop {
            if !path_still_names_source(path, source)? {
                return Ok(());
            }
            let target = dir.join(format!("{name}.{}.bad", uuid::Uuid::new_v4()));
            match rename_no_replace(path, &target) {
                Ok(()) => break,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        prune_quarantine(&dir)?;
        sync_dir(&dir)?;
        sync_dir(&self.root)
    }
}

fn path_still_names_source(path: &Path, source: &File) -> io::Result<bool> {
    #[cfg(windows)]
    {
        let named = match same_file::Handle::from_path(path) {
            Ok(handle) => handle,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        let opened = same_file::Handle::from_file(source.try_clone()?)?;
        return Ok(named == opened);
    }

    #[cfg(not(windows))]
    {
        let path_metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        let source_metadata = source.metadata()?;
        Ok(same_file_identity(&path_metadata, &source_metadata))
    }
}

#[cfg(not(windows))]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (left, right);
        false
    }
}

pub(crate) fn rename_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let source = std::ffi::CString::new(source.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in source path"))?;
        let target = std::ffi::CString::new(target.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in target path"))?;
        // SAFETY: both paths are NUL-terminated and remain alive through the
        // syscall; AT_FDCWD resolves the absolute paths supplied by the store.
        let result = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                target.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        return if result == -1 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EEXIST) {
                Err(io::Error::new(io::ErrorKind::AlreadyExists, error))
            } else {
                Err(error)
            }
        } else {
            Ok(())
        };
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let source = std::ffi::CString::new(source.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in source path"))?;
        let target = std::ffi::CString::new(target.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in target path"))?;
        // SAFETY: both paths are NUL-terminated and remain alive through the
        // syscall; RENAME_EXCL prevents replacing an existing quarantine entry.
        let result = unsafe {
            libc::renameatx_np(
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                target.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EEXIST) {
                Err(io::Error::new(io::ErrorKind::AlreadyExists, error))
            } else {
                Err(error)
            }
        } else {
            Ok(())
        }
    }
    #[cfg(windows)]
    {
        // Windows MoveFile semantics used by std::fs::rename fail when the
        // destination exists, so the target cannot replace a retained entry.
        return fs::rename(source, target);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (source, target);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no-replace quarantine rename is unsupported on this platform",
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "Successful draft captures dominate typing snapshots and clones; keep their record inline instead of adding an allocation to each capture"
)]
enum DraftCapture {
    Record(LocalDraftRecord),
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DraftSnapshot {
    session_id: Option<String>,
    cwd: PathBuf,
    original: DraftCapture,
    capture: DraftCapture,
    can_restore: bool,
    ownership_epoch: u64,
    content_epoch: u64,
    restored: Option<LocalDraftRecord>,
}

impl DraftSnapshot {
    fn from_agent(agent: &AgentView, ownership_epoch: u64, content_epoch: u64) -> Self {
        let capture = match capture_agent(agent, 0) {
            Ok(record) => DraftCapture::Record(record),
            Err(error) => DraftCapture::Error(error.to_string()),
        };
        Self {
            session_id: agent.session.session_id.as_ref().map(|id| id.0.to_string()),
            cwd: agent.session.cwd.clone(),
            original: capture.clone(),
            capture,
            can_restore: agent.session.in_flight_prompt.is_none()
                && agent.prompt.text().is_empty()
                && !agent.prompt.has_local_draft_images(),
            ownership_epoch,
            content_epoch,
            restored: None,
        }
    }
}

impl DraftSubject for DraftSnapshot {
    fn draft_session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    fn draft_cwd(&self) -> &Path {
        &self.cwd
    }

    fn capture_draft(&self, revision: u64) -> io::Result<LocalDraftRecord> {
        match &self.capture {
            DraftCapture::Record(record) => {
                let mut record = record.clone();
                record.revision = revision;
                Ok(record)
            }
            DraftCapture::Error(error) => Err(io::Error::other(error.clone())),
        }
    }

    fn restore_draft(&mut self, record: &LocalDraftRecord) {
        if self.can_restore {
            self.capture = DraftCapture::Record(record.clone());
            self.restored = Some(record.clone());
        }
    }
}

type DraftSnapshotSet = indexmap::IndexMap<AgentId, DraftSnapshot>;

fn strip_transferred_inputs(
    snapshots: &mut DraftSnapshotSet,
    transferred: &HashSet<(AgentId, String)>,
) {
    for (agent_id, snapshot) in snapshots {
        let DraftCapture::Record(record) = &mut snapshot.capture else {
            continue;
        };
        if record
            .uncertain_submission
            .as_ref()
            .is_some_and(|intent| transferred.contains(&(*agent_id, intent.prompt_id.clone())))
        {
            record.uncertain_submission = None;
            snapshot.original = DraftCapture::Record(record.clone());
        }
    }
}

fn attach_input_fallback(
    snapshots: &mut DraftSnapshotSet,
    agent_id: AgentId,
    prompt_id: &str,
    expected_turn_id: Option<&str>,
    fallback: &(String, bool),
) -> io::Result<()> {
    let (text, has_structured_content) = fallback;
    if *has_structured_content {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "structured or image interjection content exceeds the local draft snapshot format",
        ));
    }
    let snapshot = snapshots.get_mut(&agent_id).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "input owner is absent from draft snapshot",
        )
    })?;
    let DraftCapture::Record(mut record) = snapshot.capture.clone() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "current draft cannot be captured alongside this input",
        ));
    };
    if let Some(existing) = &record.uncertain_submission
        && existing.prompt_id == prompt_id
        && existing.expected_turn_id.as_deref() == expected_turn_id
        && existing.prompt.text == *text
    {
        return Ok(());
    }
    let prompt = DraftPrompt::from_parts(text, text.len(), &[])?;
    if record
        .composer
        .as_ref()
        .is_some_and(|composer| composer.text == prompt.text)
    {
        record.composer = None;
    }
    record.uncertain_submission = Some(UncertainSubmission {
        prompt_id: prompt_id.to_owned(),
        expected_turn_id: expected_turn_id.map(str::to_owned),
        prompt,
    });
    let capture = DraftCapture::Record(record);
    snapshot.original = capture.clone();
    snapshot.capture = capture;
    Ok(())
}

fn draft_key_transition(before: &DraftSnapshotSet, after: &DraftSnapshotSet) -> bool {
    before.iter().any(|(id, old)| {
        after
            .get(id)
            .is_none_or(|new| old.session_id != new.session_id || old.cwd != new.cwd)
    }) || after.keys().any(|id| !before.contains_key(id))
}

type RequiredInputCheckpoint = (AgentId, String, Option<String>, Option<(String, bool)>);

enum DraftIoCommand {
    Sync {
        generation: u64,
        agents: DraftSnapshotSet,
        active: Option<AgentId>,
        now: Instant,
    },
    Boundary {
        agents: DraftSnapshotSet,
        active: Option<AgentId>,
        now: Instant,
    },
    Invalidate {
        agent_id: AgentId,
        session_id: String,
        prompt_id: String,
    },
    DiscardInputIdentity {
        agent_id: AgentId,
        prompt_id: String,
    },
    Checkpoint {
        agents: DraftSnapshotSet,
        active: Option<AgentId>,
        required_input: Option<RequiredInputCheckpoint>,
        ack: tokio::sync::oneshot::Sender<io::Result<()>>,
    },
}

struct DraftIoResult {
    generation: u64,
    restores: Vec<(AgentId, DraftSnapshot)>,
}

fn restore_unacknowledged_loads(
    agents: &mut DraftSnapshotSet,
    unresolved: &mut HashMap<AgentId, DraftSnapshot>,
) {
    unresolved.retain(|id, prior| {
        agents.get(id).is_some_and(|current| {
            current.session_id == prior.session_id
                && current.cwd == prior.cwd
                && current.ownership_epoch == prior.ownership_epoch
                && current.content_epoch == prior.content_epoch
                && current.original == prior.original
        })
    });
    for (id, current) in agents {
        if let Some(record) = unresolved.get(id).and_then(|prior| prior.restored.as_ref()) {
            current.restore_draft(record);
        }
    }
}

struct DraftIoWorker {
    tx: std::sync::mpsc::Sender<DraftIoCommand>,
    rx: tokio::sync::mpsc::UnboundedReceiver<DraftIoResult>,
    notify: Arc<tokio::sync::Notify>,
    latest: Option<(u64, DraftSnapshotSet, Option<AgentId>)>,
    in_flight: Option<u64>,
    generation: u64,
    dispatched_generation: u64,
    ownership_epochs: HashMap<AgentId, u64>,
    content_epochs: HashMap<AgentId, u64>,
}

impl std::fmt::Debug for DraftIoWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DraftIoWorker")
            .field("in_flight", &self.in_flight)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl DraftIoWorker {
    fn new(root: PathBuf) -> Self {
        Self::with_store(LocalDraftStore::new(root))
    }

    fn with_store(store: LocalDraftStore) -> Self {
        let (tx, commands) = std::sync::mpsc::channel();
        let (results, rx) = tokio::sync::mpsc::unbounded_channel();
        let notify = Arc::new(tokio::sync::Notify::new());
        let worker_notify = Arc::clone(&notify);
        std::thread::Builder::new()
            .name("pager-local-drafts".into())
            .spawn(move || {
                let mut runtime = LocalDraftRuntime::direct(store);
                let mut unresolved_restores = HashMap::<AgentId, DraftSnapshot>::new();
                loop {
                    let next = runtime.next_deadline();
                    let command = match next {
                        Some(deadline) => commands
                            .recv_timeout(deadline.saturating_duration_since(Instant::now())),
                        None => commands
                            .recv()
                            .map_err(|_| std::sync::mpsc::RecvTimeoutError::Disconnected),
                    };
                    match command {
                        Ok(DraftIoCommand::Sync {
                            generation,
                            mut agents,
                            active,
                            now,
                        }) => {
                            restore_unacknowledged_loads(&mut agents, &mut unresolved_restores);
                            runtime.sync_subjects(&mut agents, active, now);
                            let restores = agents
                                .into_iter()
                                .filter(|(_, snapshot)| snapshot.restored.is_some())
                                .collect::<Vec<_>>();
                            for (id, snapshot) in &restores {
                                unresolved_restores.insert(*id, snapshot.clone());
                            }
                            if results
                                .send(DraftIoResult {
                                    generation,
                                    restores,
                                })
                                .is_err()
                            {
                                break;
                            }
                            worker_notify.notify_one();
                        }
                        Ok(DraftIoCommand::Boundary {
                            mut agents,
                            active,
                            now,
                        }) => {
                            restore_unacknowledged_loads(&mut agents, &mut unresolved_restores);
                            runtime.sync_subjects(&mut agents, active, now);
                            for (id, snapshot) in agents {
                                if snapshot.restored.is_some() {
                                    unresolved_restores.insert(id, snapshot);
                                }
                            }
                        }
                        Ok(DraftIoCommand::Invalidate {
                            agent_id,
                            session_id,
                            prompt_id,
                        }) => {
                            unresolved_restores.remove(&agent_id);
                            runtime.transfer_prompt_rpc_identity(agent_id, &session_id, &prompt_id);
                        }
                        Ok(DraftIoCommand::DiscardInputIdentity {
                            agent_id,
                            prompt_id,
                        }) => {
                            unresolved_restores.remove(&agent_id);
                            runtime.discard_input_identity(agent_id, &prompt_id);
                        }
                        Ok(DraftIoCommand::Checkpoint {
                            mut agents,
                            active,
                            required_input,
                            ack,
                        }) => {
                            restore_unacknowledged_loads(&mut agents, &mut unresolved_restores);
                            runtime.sync_subjects(&mut agents, active, Instant::now());
                            let result = match required_input {
                                Some((agent_id, prompt_id, expected_turn_id, _fallback)) => runtime
                                    .persist_input_checkpoint(
                                        agent_id,
                                        &prompt_id,
                                        expected_turn_id.as_deref(),
                                    ),
                                None => runtime.flush_all_checked(),
                            };
                            let _ = ack.send(result);
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            runtime.flush_due(Instant::now());
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("spawn local draft I/O worker");
        Self {
            tx,
            rx,
            notify,
            latest: None,
            in_flight: None,
            generation: 0,
            dispatched_generation: 0,
            ownership_epochs: HashMap::new(),
            content_epochs: HashMap::new(),
        }
    }

    fn snapshot(&self, agents: &indexmap::IndexMap<AgentId, AgentView>) -> DraftSnapshotSet {
        agents
            .iter()
            .map(|(id, agent)| {
                (
                    *id,
                    DraftSnapshot::from_agent(
                        agent,
                        self.ownership_epochs.get(id).copied().unwrap_or(0),
                        self.content_epochs.get(id).copied().unwrap_or(0),
                    ),
                )
            })
            .collect()
    }

    fn dispatch_latest(&mut self, now: Instant) {
        if self.in_flight.is_some() {
            return;
        }
        let Some((generation, agents, active)) = &self.latest else {
            return;
        };
        if self
            .tx
            .send(DraftIoCommand::Sync {
                generation: *generation,
                agents: agents.clone(),
                active: *active,
                now,
            })
            .is_ok()
        {
            self.in_flight = Some(*generation);
            self.dispatched_generation = *generation;
        }
    }

    fn poll(&mut self, agents: &mut indexmap::IndexMap<AgentId, AgentView>) {
        while let Ok(result) = self.rx.try_recv() {
            self.in_flight = None;
            for (id, snapshot) in result.restores {
                let Some(record) = snapshot.restored.as_ref() else {
                    continue;
                };
                let Some(agent) = agents.get_mut(&id) else {
                    continue;
                };
                if self.ownership_epochs.get(&id).copied().unwrap_or(0) != snapshot.ownership_epoch
                    || self.content_epochs.get(&id).copied().unwrap_or(0) != snapshot.content_epoch
                    || agent.session.session_id.as_ref().map(|sid| sid.0.as_ref())
                        != snapshot.session_id.as_deref()
                    || agent.session.cwd != snapshot.cwd
                {
                    continue;
                }
                let current = capture_agent(agent, 0)
                    .map(DraftCapture::Record)
                    .unwrap_or_else(|error| DraftCapture::Error(error.to_string()));
                if current == snapshot.original {
                    restore_agent(agent, record);
                }
            }
            if self
                .latest
                .as_ref()
                .is_some_and(|(generation, _, _)| *generation > result.generation)
            {
                self.dispatch_latest(Instant::now());
            }
        }
    }

    fn sync(
        &mut self,
        agents: &mut indexmap::IndexMap<AgentId, AgentView>,
        active: Option<AgentId>,
        now: Instant,
        transferred_input_ids: &HashSet<(AgentId, String)>,
    ) {
        self.poll(agents);
        let mut snapshot = self.snapshot(agents);
        strip_transferred_inputs(&mut snapshot, transferred_input_ids);
        if let Some((_, prior, _)) = &self.latest {
            for (id, current) in &mut snapshot {
                if prior
                    .get(id)
                    .is_some_and(|old| old.original != current.original)
                {
                    let epoch = self.content_epochs.entry(*id).or_default();
                    *epoch = epoch.saturating_add(1);
                    current.content_epoch = *epoch;
                }
            }
        }
        let changed = self
            .latest
            .as_ref()
            .is_none_or(|(_, prior, prior_active)| prior != &snapshot || *prior_active != active);
        if changed {
            if let Some((generation, prior, prior_active)) = &self.latest
                && *generation > self.dispatched_generation
                && draft_key_transition(prior, &snapshot)
                && self
                    .tx
                    .send(DraftIoCommand::Boundary {
                        agents: prior.clone(),
                        active: *prior_active,
                        now,
                    })
                    .is_ok()
            {
                self.dispatched_generation = *generation;
            }
            self.generation = self.generation.saturating_add(1);
            self.latest = Some((self.generation, snapshot, active));
            self.dispatch_latest(now);
        }
    }

    fn invalidate(&mut self, agent_id: AgentId, session_id: &str, prompt_id: &str) {
        if let Some((_, latest, _)) = &mut self.latest {
            strip_transferred_inputs(latest, &HashSet::from([(agent_id, prompt_id.to_owned())]));
        }
        let _ = self.tx.send(DraftIoCommand::Invalidate {
            agent_id,
            session_id: session_id.to_owned(),
            prompt_id: prompt_id.to_owned(),
        });
    }

    async fn checkpoint(
        &mut self,
        agents: &indexmap::IndexMap<AgentId, AgentView>,
        active: Option<AgentId>,
        filtered_input_ids: &HashSet<(AgentId, String)>,
    ) -> io::Result<()> {
        let (ack, done) = tokio::sync::oneshot::channel();
        if let Some((generation, latest, latest_active)) = &self.latest
            && *generation > self.dispatched_generation
        {
            self.tx
                .send(DraftIoCommand::Boundary {
                    agents: latest.clone(),
                    active: *latest_active,
                    now: Instant::now(),
                })
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "local draft worker stopped")
                })?;
            self.dispatched_generation = *generation;
        }
        let mut snapshot = self.snapshot(agents);
        strip_transferred_inputs(&mut snapshot, filtered_input_ids);
        self.tx
            .send(DraftIoCommand::Checkpoint {
                agents: snapshot,
                active,
                required_input: None,
                ack,
            })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "local draft worker stopped"))?;
        done.await.map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "local draft worker stopped before checkpoint",
            )
        })?
    }

    fn checkpoint_input_intent(
        &mut self,
        agents: &indexmap::IndexMap<AgentId, AgentView>,
        active: Option<AgentId>,
        agent_id: AgentId,
        prompt_id: String,
        expected_turn_id: Option<String>,
        fallback: Option<(String, bool)>,
        filtered_input_ids: &HashSet<(AgentId, String)>,
    ) -> io::Result<tokio::sync::oneshot::Receiver<io::Result<()>>> {
        if let Some((generation, latest, latest_active)) = &self.latest
            && *generation > self.dispatched_generation
        {
            self.tx
                .send(DraftIoCommand::Boundary {
                    agents: latest.clone(),
                    active: *latest_active,
                    now: Instant::now(),
                })
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "local draft worker stopped")
                })?;
            self.dispatched_generation = *generation;
        }
        let mut snapshot = self.snapshot(agents);
        strip_transferred_inputs(&mut snapshot, filtered_input_ids);
        if let Some(fallback) = fallback.as_ref() {
            attach_input_fallback(
                &mut snapshot,
                agent_id,
                &prompt_id,
                expected_turn_id.as_deref(),
                fallback,
            )?;
        }
        let (ack, done) = tokio::sync::oneshot::channel();
        self.tx
            .send(DraftIoCommand::Checkpoint {
                agents: snapshot,
                active,
                required_input: Some((agent_id, prompt_id, expected_turn_id, fallback)),
                ack,
            })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "local draft worker stopped"))?;
        Ok(done)
    }
}

#[derive(Debug)]
struct TrackedDraft {
    record: LocalDraftRecord,
    serialized: Vec<u8>,
    due: Option<Instant>,
    last_deferred: Option<tools::types::BehaviorId>,
}

#[derive(Debug)]
pub(crate) struct LocalDraftRuntime {
    store: LocalDraftStore,
    // Process-local routing only. AgentId is never serialized and never
    // contributes to a filename; durable identity is session-or-canonical-cwd.
    keys: HashMap<AgentId, LocalDraftKey>,
    loaded: HashSet<LocalDraftKey>,
    tracked: HashMap<LocalDraftKey, TrackedDraft>,
    invalidations: HashMap<LocalDraftKey, Instant>,
    active_key: Option<LocalDraftKey>,
    next_revision: u64,
    worker: Option<DraftIoWorker>,
    ram_only_input_ids: HashSet<(AgentId, String)>,
    transferred_input_ids: HashSet<(AgentId, String)>,
}

impl Default for LocalDraftRuntime {
    fn default() -> Self {
        let store = LocalDraftStore::default();
        let worker = if cfg!(test) {
            None
        } else {
            Some(DraftIoWorker::new(store.root.clone()))
        };
        Self {
            store,
            keys: HashMap::new(),
            loaded: HashSet::new(),
            tracked: HashMap::new(),
            invalidations: HashMap::new(),
            active_key: None,
            next_revision: 0,
            worker,
            ram_only_input_ids: HashSet::new(),
            transferred_input_ids: HashSet::new(),
        }
    }
}

impl LocalDraftRuntime {
    fn direct(store: LocalDraftStore) -> Self {
        Self {
            store,
            keys: HashMap::new(),
            loaded: HashSet::new(),
            tracked: HashMap::new(),
            invalidations: HashMap::new(),
            active_key: None,
            next_revision: 0,
            worker: None,
            ram_only_input_ids: HashSet::new(),
            transferred_input_ids: HashSet::new(),
        }
    }

    pub(crate) fn ready_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
        self.worker
            .as_ref()
            .map(|worker| Arc::clone(&worker.notify))
    }

    pub(crate) fn sync(
        &mut self,
        agents: &mut indexmap::IndexMap<AgentId, AgentView>,
        active: Option<AgentId>,
        now: Instant,
    ) {
        let filtered_input_ids = self
            .transferred_input_ids
            .union(&self.ram_only_input_ids)
            .cloned()
            .collect::<HashSet<_>>();
        if let Some(worker) = &mut self.worker {
            worker.sync(agents, active, now, &filtered_input_ids);
            return;
        }
        let mut snapshots = agents
            .iter()
            .map(|(id, agent)| (*id, DraftSnapshot::from_agent(agent, 0, 0)))
            .collect::<DraftSnapshotSet>();
        strip_transferred_inputs(&mut snapshots, &filtered_input_ids);
        self.sync_subjects(&mut snapshots, active, now);
        for (id, snapshot) in snapshots {
            if let Some(record) = snapshot.restored.as_ref()
                && let Some(agent) = agents.get_mut(&id)
            {
                agent.restore_draft(record);
            }
        }
    }

    fn sync_subjects<S: DraftSubject>(
        &mut self,
        agents: &mut indexmap::IndexMap<AgentId, S>,
        active: Option<AgentId>,
        now: Instant,
    ) {
        let mut retired = HashSet::new();
        self.keys.retain(|id, key| {
            if agents.contains_key(id) {
                true
            } else {
                retired.insert(key.clone());
                false
            }
        });
        for key in retired {
            if self.keys.values().any(|live| live == &key) {
                continue;
            }
            self.flush_key(&key, now);
            self.loaded.remove(&key);
            if self.active_key.as_ref() == Some(&key) {
                self.active_key = None;
            }
            if self
                .tracked
                .get(&key)
                .is_some_and(|tracked| tracked.due.is_none())
            {
                self.tracked.remove(&key);
            }
        }
        let mut recovered_active_pending = false;
        for (id, agent) in agents.iter_mut() {
            let desired = agent
                .draft_session_id()
                .and_then(|sid| LocalDraftKey::session(sid).ok())
                .or_else(|| LocalDraftKey::cwd(agent.draft_cwd()).ok());
            let Some(key) = desired else {
                tracing::warn!(cwd = %agent.draft_cwd().display(), "local draft disabled: cwd cannot be canonicalized");
                continue;
            };
            if let Some(old) = self.keys.insert(*id, key.clone())
                && old != key
            {
                if self.invalidations.contains_key(&old) || self.invalidations.contains_key(&key) {
                    // A stale cwd file must not be migrated across an invalidation.
                    self.invalidate_key(&old, now);
                    self.invalidate_key(&key, now);
                } else {
                    self.flush_key(&old, now);
                    if let Err(error) = self.store.rekey(&old, &key) {
                        tracing::warn!(?error, "failed to rekey local draft at session bind");
                    }
                    if let Some(tracked) = self.tracked.remove(&old) {
                        self.tracked.insert(key.clone(), tracked);
                    }
                }
                self.loaded.remove(&key);
            }
            if self.loaded.insert(key.clone()) && !self.invalidations.contains_key(&key) {
                if let Some(tracked) = self
                    .tracked
                    .get(&key)
                    .filter(|tracked| tracked.due.is_some())
                {
                    // A failed closing checkpoint owns newer content than disk.
                    agent.restore_draft(&tracked.record);
                    recovered_active_pending |= active == Some(*id);
                } else {
                    match self.store.load(&key) {
                        Ok(Some(record)) => {
                            self.next_revision = self.next_revision.max(record.revision);
                            let mut fingerprint = record.clone();
                            fingerprint.revision = 0;
                            self.tracked.insert(
                                key.clone(),
                                TrackedDraft {
                                    serialized: serde_json::to_vec(&fingerprint)
                                        .unwrap_or_default(),
                                    last_deferred: record.deferred_session_mode,
                                    record: record.clone(),
                                    due: None,
                                },
                            );
                            agent.restore_draft(&record);
                        }
                        Ok(None) => {}
                        Err(error) => tracing::warn!(?error, "failed to load local draft"),
                    }
                }
            }
            match agent.capture_draft(0) {
                Ok(mut record) => {
                    if self.invalidations.contains_key(&key) {
                        if !record.has_payload() {
                            continue;
                        }
                        self.invalidations.remove(&key);
                    }
                    let serialized = serde_json::to_vec(&record).unwrap_or_default();
                    let changed = self
                        .tracked
                        .get(&key)
                        .is_none_or(|t| t.serialized != serialized);
                    if changed {
                        self.next_revision = self.next_revision.saturating_add(1);
                        record.revision = self.next_revision;
                        let force = self.tracked.get(&key).map_or_else(
                            || record.deferred_session_mode.is_some(),
                            |tracked| tracked.last_deferred != record.deferred_session_mode,
                        );
                        self.tracked.insert(
                            key.clone(),
                            TrackedDraft {
                                serialized,
                                last_deferred: record.deferred_session_mode,
                                record,
                                due: Some(if force { now } else { now + DEBOUNCE }),
                            },
                        );
                    }
                }
                Err(error) => {
                    self.invalidate_key(&key, now);
                    tracing::warn!(target: "pager::local_drafts", ?error, "local draft was not persisted");
                }
            }
        }
        let active_key = active.and_then(|id| self.keys.get(&id).cloned());
        if active_key != self.active_key {
            if let Some(old) = self.active_key.take() {
                self.flush_key(&old, now);
            }
            if let Some(key) = active_key.as_ref()
                && !recovered_active_pending
            {
                self.flush_key(key, now);
            }
            self.active_key = active_key;
        }
        self.flush_due(now);
        let live_keys: HashSet<_> = self.keys.values().cloned().collect();
        self.loaded.retain(|key| live_keys.contains(key));
        self.tracked
            .retain(|key, tracked| live_keys.contains(key) || tracked.due.is_some());
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        if self.worker.is_some() {
            return None;
        }
        self.tracked
            .values()
            .filter_map(|t| t.due)
            .chain(self.invalidations.values().copied())
            .min()
    }

    /// Transfers every ACP prompt-RPC variant out of the local recovery
    /// domain by retiring only the checkpointed input identity. The Effect
    /// classifier is the sole variant list.
    pub(crate) fn transfer_prompt_rpc_ownership(&mut self, effect: &crate::app::actions::Effect) {
        let Some((agent_id, session_id, prompt_id, _)) = effect.prompt_rpc_identity() else {
            return;
        };
        self.transfer_input_identity(agent_id, &session_id.0, prompt_id);
    }

    pub(crate) fn transfer_input_identity(
        &mut self,
        agent_id: AgentId,
        session_id: &str,
        prompt_id: &str,
    ) {
        self.ram_only_input_ids
            .remove(&(agent_id, prompt_id.to_owned()));
        if self.transferred_input_ids.len() >= 256
            && let Some(oldest) = self.transferred_input_ids.iter().next().cloned()
        {
            self.transferred_input_ids.remove(&oldest);
        }
        self.transferred_input_ids
            .insert((agent_id, prompt_id.to_owned()));
        if let Some(worker) = &mut self.worker {
            worker.invalidate(agent_id, session_id, prompt_id);
            return;
        }
        self.transfer_prompt_rpc_identity(agent_id, session_id, prompt_id);
    }

    pub(crate) fn restore_input_draft_ownership(&mut self, agent_id: AgentId, prompt_id: &str) {
        self.transferred_input_ids
            .remove(&(agent_id, prompt_id.to_owned()));
    }

    pub(crate) async fn checkpoint(
        &mut self,
        agents: &indexmap::IndexMap<AgentId, AgentView>,
        active: Option<AgentId>,
    ) -> io::Result<()> {
        let filtered_input_ids = self
            .transferred_input_ids
            .union(&self.ram_only_input_ids)
            .cloned()
            .collect::<HashSet<_>>();
        if let Some(worker) = &mut self.worker {
            worker.checkpoint(agents, active, &filtered_input_ids).await
        } else {
            self.flush_all();
            Ok(())
        }
    }

    pub(crate) fn checkpoint_input_intent(
        &mut self,
        agents: &indexmap::IndexMap<AgentId, AgentView>,
        active: Option<AgentId>,
        agent_id: AgentId,
        prompt_id: String,
        expected_turn_id: Option<String>,
        fallback: Option<(String, bool)>,
    ) -> io::Result<tokio::sync::oneshot::Receiver<io::Result<()>>> {
        if self
            .ram_only_input_ids
            .contains(&(agent_id, prompt_id.clone()))
        {
            let (ack, done) = tokio::sync::oneshot::channel();
            let _ = ack.send(Ok(()));
            return Ok(done);
        }
        let filtered_input_ids = self
            .transferred_input_ids
            .union(&self.ram_only_input_ids)
            .cloned()
            .collect::<HashSet<_>>();
        let Some(worker) = &mut self.worker else {
            let mut snapshots = agents
                .iter()
                .map(|(id, agent)| (*id, DraftSnapshot::from_agent(agent, 0, 0)))
                .collect::<DraftSnapshotSet>();
            strip_transferred_inputs(&mut snapshots, &filtered_input_ids);
            if let Some(fallback) = fallback.as_ref() {
                attach_input_fallback(
                    &mut snapshots,
                    agent_id,
                    &prompt_id,
                    expected_turn_id.as_deref(),
                    fallback,
                )?;
            }
            self.sync_subjects(&mut snapshots, active, Instant::now());
            self.persist_input_checkpoint(agent_id, &prompt_id, expected_turn_id.as_deref())?;
            let (ack, done) = tokio::sync::oneshot::channel();
            let _ = ack.send(Ok(()));
            return Ok(done);
        };
        worker.checkpoint_input_intent(
            agents,
            active,
            agent_id,
            prompt_id,
            expected_turn_id,
            fallback,
            &filtered_input_ids,
        )
    }

    pub(crate) fn mark_input_ram_only(&mut self, agent_id: AgentId, prompt_id: String) {
        if self.ram_only_input_ids.len() >= 256
            && let Some(oldest) = self.ram_only_input_ids.iter().next().cloned()
        {
            self.ram_only_input_ids.remove(&oldest);
        }
        self.ram_only_input_ids
            .insert((agent_id, prompt_id.clone()));
        if let Some(worker) = &mut self.worker {
            if let Some((_, latest, _)) = &mut worker.latest {
                strip_transferred_inputs(latest, &HashSet::from([(agent_id, prompt_id.clone())]));
            }
            let _ = worker.tx.send(DraftIoCommand::DiscardInputIdentity {
                agent_id,
                prompt_id,
            });
        } else {
            self.discard_input_identity(agent_id, &prompt_id);
        }
    }

    fn transfer_prompt_rpc_identity(
        &mut self,
        agent_id: AgentId,
        session_id: &str,
        prompt_id: &str,
    ) {
        // Session binding and the event-loop sync are intentionally decoupled.
        // Capture the runtime's current key before disarming: during that
        // window it can still be the canonical-cwd key even though the Effect
        // already carries the newly bound SessionId.
        let current_key = self.keys.get(&agent_id).cloned();
        let mut keys = current_key.into_iter().collect::<Vec<_>>();
        if let Ok(session_key) = LocalDraftKey::session(session_id)
            && !keys.contains(&session_key)
        {
            keys.push(session_key);
        }
        for key in keys {
            let mut record = self
                .tracked
                .get(&key)
                .map(|tracked| tracked.record.clone())
                .or_else(|| self.store.load(&key).ok().flatten());
            let Some(record) = record.as_mut() else {
                continue;
            };
            if record
                .uncertain_submission
                .as_ref()
                .is_none_or(|intent| intent.prompt_id != prompt_id)
            {
                continue;
            }
            record.uncertain_submission = None;
            self.persist_retired_input_record(key, record.clone(), prompt_id);
        }
    }

    fn discard_input_identity(&mut self, agent_id: AgentId, prompt_id: &str) {
        let Some(key) = self.keys.get(&agent_id).cloned() else {
            return;
        };
        let Some(mut record) = self
            .tracked
            .get(&key)
            .map(|tracked| tracked.record.clone())
            .or_else(|| self.store.load(&key).ok().flatten())
        else {
            return;
        };
        if record
            .uncertain_submission
            .as_ref()
            .is_none_or(|intent| intent.prompt_id != prompt_id)
        {
            return;
        }
        record.uncertain_submission = None;
        self.persist_retired_input_record(key, record, prompt_id);
    }

    fn persist_retired_input_record(
        &mut self,
        key: LocalDraftKey,
        record: LocalDraftRecord,
        prompt_id: &str,
    ) {
        let serialized = serde_json::to_vec(&record).unwrap_or_default();
        self.tracked.insert(
            key.clone(),
            TrackedDraft {
                last_deferred: record.deferred_session_mode,
                record: record.clone(),
                serialized,
                due: Some(Instant::now()),
            },
        );
        match self.store.write(&key, &record) {
            Ok(()) => {
                if let Some(tracked) = self.tracked.get_mut(&key) {
                    tracked.due = None;
                }
            }
            Err(error) => {
                if let Some(tracked) = self.tracked.get_mut(&key) {
                    tracked.due = Some(Instant::now() + WRITE_RETRY_BACKOFF);
                }
                tracing::warn!(?error, %prompt_id, "failed to clear stale checkpointed local input identity");
            }
        }
    }

    fn invalidate_key(&mut self, key: &LocalDraftKey, now: Instant) {
        self.tracked.remove(key);
        let due = *self.invalidations.entry(key.clone()).or_insert(now);
        if due <= now {
            self.retry_invalidation(key, now);
        }
    }

    fn retry_invalidation(&mut self, key: &LocalDraftKey, now: Instant) {
        match self.store.remove(key) {
            Ok(()) => {
                self.invalidations.remove(key);
            }
            Err(error) => {
                self.invalidations
                    .insert(key.clone(), now + WRITE_RETRY_BACKOFF);
                tracing::warn!(?error, "failed to invalidate local draft; retry scheduled");
            }
        }
    }

    pub(crate) fn flush_all(&mut self) {
        let keys = self.tracked.keys().cloned().collect::<Vec<_>>();
        let now = Instant::now();
        for key in keys {
            self.flush_key(&key, now);
        }
        for key in self.invalidations.keys().cloned().collect::<Vec<_>>() {
            self.retry_invalidation(&key, now);
        }
    }

    fn flush_all_checked(&mut self) -> io::Result<()> {
        let keys = self.tracked.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            self.flush_key_checked(&key)?;
        }
        Ok(())
    }

    fn persist_input_checkpoint(
        &mut self,
        agent_id: AgentId,
        prompt_id: &str,
        expected_turn_id: Option<&str>,
    ) -> io::Result<()> {
        let key = self.keys.get(&agent_id).cloned().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "input draft key is unavailable")
        })?;
        let tracked = self.tracked.get(&key).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "input intent was not captured")
        })?;
        let Some(intent) = &tracked.record.uncertain_submission else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "input intent is absent from the local draft snapshot",
            ));
        };
        if intent.prompt_id != prompt_id || intent.expected_turn_id.as_deref() != expected_turn_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "local draft snapshot belongs to a different input identity or route",
            ));
        }
        self.flush_key_checked(&key)
    }

    fn flush_key_checked(&mut self, key: &LocalDraftKey) -> io::Result<()> {
        let Some(tracked) = self.tracked.get_mut(key) else {
            return Ok(());
        };
        self.store.write(key, &tracked.record)?;
        tracked.due = None;
        Ok(())
    }

    fn flush_due(&mut self, now: Instant) {
        let keys = self
            .tracked
            .iter()
            .filter(|&(_key, tracked)| tracked.due.is_some_and(|at| at <= now))
            .map(|(key, _tracked)| key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            self.flush_key(&key, now);
        }
        let expired = self
            .invalidations
            .iter()
            .filter(|(_, due)| **due <= now)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in expired {
            self.retry_invalidation(&key, now);
        }
    }

    fn flush_key(&mut self, key: &LocalDraftKey, now: Instant) {
        let Some(tracked) = self.tracked.get_mut(key) else {
            return;
        };
        if tracked.due.is_none() {
            return;
        }
        match self.store.write(key, &tracked.record) {
            Ok(()) => tracked.due = None,
            Err(error) => {
                // A persistent I/O error must never leave an already-expired
                // deadline armed. `local_draft_tick` is a high-priority biased
                // select arm; an expired retry would otherwise hot-loop and
                // starve repaint, terminal input, and graceful quit.
                tracked.due = Some(now + WRITE_RETRY_BACKOFF);
                tracing::warn!(?error, "failed to persist local draft");
            }
        }
    }
}

trait DraftSubject {
    fn draft_session_id(&self) -> Option<&str>;
    fn draft_cwd(&self) -> &Path;
    fn capture_draft(&self, revision: u64) -> io::Result<LocalDraftRecord>;
    fn restore_draft(&mut self, record: &LocalDraftRecord);
}

impl DraftSubject for AgentView {
    fn draft_session_id(&self) -> Option<&str> {
        self.session.session_id.as_ref().map(|sid| sid.0.as_ref())
    }

    fn draft_cwd(&self) -> &Path {
        &self.session.cwd
    }

    fn capture_draft(&self, revision: u64) -> io::Result<LocalDraftRecord> {
        capture_agent(self, revision)
    }

    fn restore_draft(&mut self, record: &LocalDraftRecord) {
        restore_agent(self, record);
    }
}

fn capture_agent(agent: &AgentView, revision: u64) -> io::Result<LocalDraftRecord> {
    let mut record = LocalDraftRecord::empty(revision);
    let uncertain = agent
        .session
        .in_flight_prompt
        .as_ref()
        .filter(|prompt| prompt.prompt_id.is_some());
    let uncertain = uncertain
        .map(|prompt| {
            (
                prompt.prompt_id.as_deref().expect("filtered above"),
                prompt.expected_turn_id.as_deref(),
                prompt.text.as_str(),
                prompt.images.as_slice(),
                prompt.chip_elements.as_slice(),
                prompt.frozen_skill_selection.as_ref(),
            )
        })
        .or_else(|| {
            agent.session.pending_prompts.iter().find_map(|prompt| {
                Some((
                    prompt.prompt_id.as_deref()?,
                    prompt.expected_turn_id.as_deref(),
                    prompt.text.as_str(),
                    prompt.images.as_slice(),
                    prompt.chip_elements.as_slice(),
                    None,
                ))
            })
        });
    let stash = normal_stash(agent);
    record.composer = if let Some(stash) = stash {
        if !stash.images.is_empty() || !stash.image_undo_stash.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "composer contains images",
            ));
        }
        Some(DraftPrompt::from_parts(
            &stash.text,
            stash.cursor,
            &stash.chip_elements,
        )?)
    } else {
        if agent.prompt.has_local_draft_images() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "composer contains images",
            ));
        }
        let chips = agent.prompt.local_draft_chip_elements();
        Some(DraftPrompt::from_parts(
            agent.prompt.text(),
            agent.prompt.cursor(),
            &chips,
        )?)
    };
    record.composer = record.composer.filter(|p| !p.is_empty());

    if let Some((prompt_id, expected_turn_id, text, images, chips, frozen_skill_selection)) =
        uncertain
    {
        if !images.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "uncertain submission contains images outside the local draft budget",
            ));
        }
        let uncertain_prompt = DraftPrompt::from_parts_with_skill(
            text,
            text.len(),
            chips,
            frozen_skill_selection.cloned(),
        )?;
        if record
            .composer
            .as_ref()
            .is_some_and(|composer| composer.text == uncertain_prompt.text)
        {
            // The uncertain snapshot is the one durable source for an
            // unchanged retry. Do not serialize the same prompt twice.
            record.composer = None;
        }
        record.uncertain_submission = Some(UncertainSubmission {
            prompt_id: prompt_id.to_owned(),
            expected_turn_id: expected_turn_id.map(str::to_owned),
            prompt: uncertain_prompt,
        });
    }

    let mut staged = agent.session.pending_prompts.iter().filter(|prompt| {
        prompt.kind == QueueEntryKind::Prompt
            && prompt.prompt_id.is_none()
            && ((prompt.wire_blocks.is_none() && !prompt.display_as_skill)
                || (prompt.display_as_skill
                    && prompt
                        .wire_blocks
                        .as_deref()
                        .and_then(frozen_skill_selection_from_blocks)
                        .is_some()))
            && prompt.combined_texts.is_empty()
    });
    if let Some(prompt) = staged.next() {
        if staged.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "multiple staged prompts are not persisted",
            ));
        }
        if !prompt.images.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "staged prompt contains images",
            ));
        }
        let selection = prompt
            .wire_blocks
            .as_deref()
            .and_then(frozen_skill_selection_from_blocks);
        record.staged_prompt = Some(DraftPrompt::from_parts_with_skill(
            &prompt.text,
            prompt.text.len(),
            &prompt.chip_elements,
            selection,
        )?);
    }
    if record.composer.is_some() && record.staged_prompt.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "simultaneous composer and staged prompt require explicit recovery ordering",
        ));
    }
    record.deferred_session_mode = agent.session.deferred_session_mode;
    Ok(record)
}

fn normal_stash(agent: &AgentView) -> Option<&StashedPrompt> {
    agent
        .question_view
        .as_ref()
        .map(|view| &view.stashed_prompt)
        .or_else(|| {
            agent
                .plan_approval_view
                .as_ref()
                .map(|view| &view.stashed_prompt)
        })
        .or(agent.permission_stashed_prompt.as_ref())
        .or(agent.casual_stashed_prompt.as_ref())
        .or(agent.stashed_prompt.as_ref())
}

fn frozen_skill_selection_from_blocks(
    blocks: &[acp_transport::protocol::ContentBlock],
) -> Option<crate::app::session::FrozenSkillSelection> {
    blocks.iter().find_map(|block| {
        let acp_transport::protocol::ContentBlock::Text(text) = block else {
            return None;
        };
        text.meta
            .as_ref()?
            .get("grow/skillSelection")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    })
}

fn restore_agent(agent: &mut AgentView, record: &LocalDraftRecord) {
    if !agent.prompt.text().is_empty() || agent.prompt.has_local_draft_images() {
        tracing::warn!("local draft recovery skipped: live composer is not empty");
        return;
    }
    // A staged prompt never crossed ACP. Put it back in the composer rather
    // than recreating the queue, so recovery cannot send by itself.
    let staged_prompt = record.staged_prompt.as_ref();
    let prompt = staged_prompt.or(record.composer.as_ref()).or_else(|| {
        record
            .uncertain_submission
            .as_ref()
            .map(|submission| &submission.prompt)
    });
    if let Some(prompt) = prompt {
        agent.prompt.set_text(&prompt.text);
        agent.prompt.restore_chip_elements(&prompt.chip_elements());
        agent.prompt.set_cursor(prompt.cursor);
    }
    agent.session.staged_skill_selection =
        staged_prompt.and_then(|staged| staged.frozen_skill_selection.clone());
    if agent.session.deferred_session_mode.is_none() {
        agent.session.deferred_session_mode = record.deferred_session_mode;
    }
    if let Some(submission) = &record.uncertain_submission {
        agent.session.in_flight_prompt = Some(crate::app::session::InFlightPrompt {
            prompt_id: Some(submission.prompt_id.clone()),
            admission_resolved: None,
            expected_turn_id: submission.expected_turn_id.clone(),
            frozen_skill_selection: submission.prompt.frozen_skill_selection.clone(),
            text: submission.prompt.text.clone(),
            images: Vec::new(),
            scrollback_entry: crate::scrollback::EntryId::new(0),
            combined_scrollback_entries: Vec::new(),
            chip_elements: submission.prompt.chip_elements(),
        });
    }
}

fn validate_record(record: &LocalDraftRecord) -> bool {
    if record.composer.is_some() && record.staged_prompt.is_some() {
        return false;
    }
    let valid_prompt = |prompt: &DraftPrompt| {
        prompt.text.len() <= MAX_TEXT_BYTES
            && prompt.cursor <= prompt.text.len()
            && prompt.text.is_char_boundary(prompt.cursor)
            && prompt.chips.len() <= MAX_CHIPS
            && prompt.chips.iter().all(|chip| {
                matches!(chip.kind, 1 | 2)
                    && chip.start <= chip.end
                    && chip.end <= prompt.text.len()
                    && prompt.text.is_char_boundary(chip.start)
                    && prompt.text.is_char_boundary(chip.end)
            })
            && prompt
                .frozen_skill_selection
                .as_ref()
                .is_none_or(valid_frozen_skill_selection)
    };
    record.composer.as_ref().is_none_or(valid_prompt)
        && record.staged_prompt.as_ref().is_none_or(valid_prompt)
        && record
            .uncertain_submission
            .as_ref()
            .is_none_or(|submission| {
                !submission.prompt_id.is_empty()
                    && submission.prompt_id.len() <= 1024
                    && valid_prompt(&submission.prompt)
            })
}

fn valid_frozen_skill_selection(selection: &crate::app::session::FrozenSkillSelection) -> bool {
    !selection.name.is_empty()
        && selection.name.len() <= 1024
        && !selection.path.is_empty()
        && selection.path.len() <= 16 * 1024
        && selection
            .plugin_name
            .as_ref()
            .is_none_or(|name| !name.is_empty() && name.len() <= 1024)
}

fn count_records(root: &Path) -> io::Result<usize> {
    Ok(fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .count())
}

fn prune_quarantine(dir: &Path) -> io::Result<()> {
    let mut entries: Vec<QuarantineEntry> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        let file_type = metadata.file_type();
        if !file_type.is_file() && !file_type.is_symlink() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        entries.push((modified, entry.file_name(), entry.path(), metadata.len()));
    }
    entries.sort_by(compare_quarantine_entries);

    let mut retained_count = entries.len();
    let mut retained_bytes = entries
        .iter()
        .map(|entry| u128::from(entry.3))
        .sum::<u128>();
    let mut evicted = 0;
    while retained_count > MAX_QUARANTINE_RECORDS || retained_bytes > MAX_QUARANTINE_BYTES {
        let (_, _, path, bytes) = &entries[evicted];
        fs::remove_file(path)?;
        retained_count -= 1;
        retained_bytes -= u128::from(*bytes);
        evicted += 1;
    }
    Ok(())
}

fn compare_quarantine_entries(a: &QuarantineEntry, b: &QuarantineEntry) -> std::cmp::Ordering {
    a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1))
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> io::Result<()> {
    // Opening a directory through `std::fs::File` is unsupported on Windows.
    // The replacement itself is still atomically published by
    // `NamedTempFile::persist`.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (PathBuf, LocalDraftStore) {
        let root = std::env::temp_dir().join(format!("grow-pager-drafts-{}", uuid::Uuid::new_v4()));
        (root.clone(), LocalDraftStore::new(root))
    }

    fn record(text: &str, mode: Option<tools::types::BehaviorId>) -> LocalDraftRecord {
        LocalDraftRecord {
            version: SCHEMA_VERSION,
            revision: 7,
            composer: Some(DraftPrompt {
                text: text.into(),
                cursor: text.len(),
                chips: Vec::new(),
                frozen_skill_selection: None,
            }),
            staged_prompt: None,
            uncertain_submission: None,
            deferred_session_mode: mode,
        }
    }

    #[tokio::test]
    async fn rpc_transfer_retires_only_the_original_input_identity() {
        for structured in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("drafts");
            let mut app = crate::app::root::tests::test_app_with_agent();
            app.local_drafts = LocalDraftRuntime {
                store: LocalDraftStore::new(root.clone()),
                ..Default::default()
            };
            let id = *app.agents.keys().next().unwrap();
            let (cwd_key, sid) = {
                let agent = app.agents.get_mut(&id).unwrap();
                agent.session.session_id = None;
                agent.prompt.set_text("newer composer draft");
                agent.session.in_flight_prompt = Some(crate::app::session::InFlightPrompt {
                    prompt_id: Some(if structured { "blocks" } else { "text" }.into()),
                    admission_resolved: None,
                    expected_turn_id: None,
                    frozen_skill_selection: None,
                    text: "submitted request".into(),
                    images: Vec::new(),
                    scrollback_entry: crate::scrollback::EntryId::new(1),
                    combined_scrollback_entries: Vec::new(),
                    chip_elements: Vec::new(),
                });
                (
                    LocalDraftKey::cwd(&agent.session.cwd).unwrap(),
                    acp_transport::protocol::SessionId::new("invalidation-bind"),
                )
            };
            let now = Instant::now();
            app.sync_local_drafts(now);
            app.sync_local_drafts(now + DEBOUNCE);
            let session_key = LocalDraftKey::session(&sid.0).unwrap();
            app.local_drafts
                .store
                .write(&session_key, &record("stale session draft", None))
                .unwrap();
            let effect = if structured {
                crate::app::actions::Effect::SendPromptBlocks {
                    agent_id: id,
                    session_id: sid.clone(),
                    blocks: vec![],
                    images: vec![],
                    prompt_id: "blocks".into(),
                }
            } else {
                crate::app::actions::Effect::SendPrompt {
                    agent_id: id,
                    session_id: sid.clone(),
                    text: "submitted".into(),
                    prompt_id: "text".into(),
                    skill_token_ranges: vec![],
                }
            };
            let checkpoint = app
                .local_drafts
                .checkpoint_input_intent(
                    &app.agents,
                    Some(id),
                    id,
                    if structured { "blocks" } else { "text" }.into(),
                    None,
                    None,
                )
                .unwrap();
            checkpoint.await.unwrap().unwrap();
            app.agents.get_mut(&id).unwrap().session.session_id = Some(sid.clone());
            app.transfer_local_draft_ownership(&effect);
            let cwd_record = LocalDraftStore::new(root.clone())
                .load(&cwd_key)
                .unwrap()
                .unwrap();
            assert_eq!(cwd_record.composer.unwrap().text, "newer composer draft");
            assert_eq!(
                app.local_drafts
                    .store
                    .load(&session_key)
                    .unwrap()
                    .unwrap()
                    .composer
                    .unwrap()
                    .text,
                "stale session draft"
            );
            let saved = LocalDraftStore::new(root.clone())
                .load(&cwd_key)
                .unwrap()
                .unwrap();
            assert!(saved.uncertain_submission.is_none());
        }
    }

    #[test]
    fn repeated_capture_failure_keeps_deadline_and_new_draft_cancels_delete() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("drafts");
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            store: LocalDraftStore::new(root.clone()),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        let agent = app.agents.get_mut(&id).unwrap();
        agent.session.session_id = Some(acp_transport::protocol::SessionId::new("capture-delete"));
        agent.prompt.set_text("old");
        let now = Instant::now();
        app.sync_local_drafts(now);
        app.sync_local_drafts(now + DEBOUNCE);
        let key = app.local_drafts.keys[&id].clone();
        let blocked = directory.path().join("blocked");
        fs::write(&blocked, b"not a directory").unwrap();
        app.local_drafts.store = LocalDraftStore::new(blocked);
        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text(&"x".repeat(MAX_TEXT_BYTES + 1));
        let failed = now + DEBOUNCE + DEBOUNCE;
        app.sync_local_drafts(failed);
        let deadline = failed + WRITE_RETRY_BACKOFF;
        assert_eq!(app.local_drafts.next_deadline(), Some(deadline));
        app.sync_local_drafts(failed + Duration::from_millis(10));
        assert_eq!(app.local_drafts.next_deadline(), Some(deadline));
        app.local_drafts.store = LocalDraftStore::new(root);
        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text("new valid draft");
        app.sync_local_drafts(failed + Duration::from_millis(20));
        assert!(app.local_drafts.invalidations.is_empty());
        app.sync_local_drafts(deadline);
        assert_eq!(
            app.local_drafts
                .store
                .load(&key)
                .unwrap()
                .unwrap()
                .composer
                .unwrap()
                .text,
            "new valid draft"
        );
    }

    #[test]
    fn closing_and_reopening_restores_saved_draft_and_releases_clean_state() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            store: LocalDraftStore::new(directory.path().to_path_buf()),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        let sid = acp_transport::protocol::SessionId::new("reopen-draft");
        let agent = app.agents.get_mut(&id).unwrap();
        agent.session.session_id = Some(sid.clone());
        agent.prompt.set_text("saved draft");
        agent.prompt.set_cursor(3);
        agent.session.deferred_session_mode = Some(tools::types::BehaviorId::Plan);
        let now = Instant::now();
        app.sync_local_drafts(now);
        let mut closed = app.agents.shift_remove(&id).unwrap();
        app.sync_local_drafts(now + DEBOUNCE);
        assert!(app.local_drafts.keys.is_empty());
        assert!(app.local_drafts.loaded.is_empty());
        assert!(app.local_drafts.tracked.is_empty());
        closed.prompt.set_text("");
        closed.session.deferred_session_mode = None;
        let reopened = AgentId(9876);
        app.agents.insert(reopened, closed);
        app.active_view = crate::app::root::ActiveView::Agent(reopened);
        app.sync_local_drafts(now + DEBOUNCE + DEBOUNCE);
        let agent = &app.agents[&reopened];
        assert_eq!(agent.prompt.text(), "saved draft");
        assert_eq!(agent.prompt.cursor(), 3);
        assert_eq!(
            agent.session.deferred_session_mode,
            Some(tools::types::BehaviorId::Plan)
        );
        assert!(agent.session.pending_prompts.is_empty());
        assert!(
            app.local_drafts
                .store
                .load(&LocalDraftKey::session(&sid.0).unwrap())
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn closing_failed_checkpoint_keeps_latest_and_retry_deadline_on_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("drafts");
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            store: LocalDraftStore::new(root.clone()),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        let sid = acp_transport::protocol::SessionId::new("failed-close");
        let key = LocalDraftKey::session(&sid.0).unwrap();
        let agent = app.agents.get_mut(&id).unwrap();
        agent.session.session_id = Some(sid);
        agent.prompt.set_text("old");
        let now = Instant::now();
        app.sync_local_drafts(now);
        app.sync_local_drafts(now + DEBOUNCE);
        // Keep the stale disk record, but route writes through an unavailable root.
        let blocked = directory.path().join("blocked");
        fs::write(&blocked, b"not a directory").unwrap();
        app.local_drafts.store = LocalDraftStore::new(blocked);
        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text("latest unsaved");
        app.sync_local_drafts(now + DEBOUNCE + DEBOUNCE);
        let mut closed = app.agents.shift_remove(&id).unwrap();
        let closing = now + DEBOUNCE + DEBOUNCE + DEBOUNCE;
        app.sync_local_drafts(closing);
        let retry = closing + WRITE_RETRY_BACKOFF;
        assert_eq!(app.local_drafts.next_deadline(), Some(retry));
        app.sync_local_drafts(closing + Duration::from_millis(10));
        assert_eq!(app.local_drafts.next_deadline(), Some(retry));
        app.local_drafts.store = LocalDraftStore::new(root);
        closed.prompt.set_text("");
        let reopened = AgentId(9877);
        app.agents.insert(reopened, closed);
        app.active_view = crate::app::root::ActiveView::Agent(reopened);
        app.sync_local_drafts(closing + Duration::from_millis(20));
        assert_eq!(app.agents[&reopened].prompt.text(), "latest unsaved");
        assert_eq!(app.local_drafts.next_deadline(), Some(retry));
        assert_eq!(
            app.local_drafts
                .store
                .load(&key)
                .unwrap()
                .unwrap()
                .composer
                .unwrap()
                .text,
            "old"
        );
        app.sync_local_drafts(retry);
        assert_eq!(
            app.local_drafts
                .store
                .load(&key)
                .unwrap()
                .unwrap()
                .composer
                .unwrap()
                .text,
            "latest unsaved"
        );
        app.agents.shift_remove(&reopened);
        app.sync_local_drafts(retry);
        assert!(app.local_drafts.tracked.is_empty());
    }

    #[test]
    fn closing_one_shared_key_owner_keeps_runtime_state() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            store: LocalDraftStore::new(directory.path().to_path_buf()),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        let agent = app.agents.get_mut(&id).unwrap();
        agent.session.session_id = Some(acp_transport::protocol::SessionId::new("shared-draft"));
        agent.prompt.set_text("shared");
        let now = Instant::now();
        app.sync_local_drafts(now);
        let key = app.local_drafts.keys[&id].clone();
        // Model a second owner disappearing; the surviving real Agent remains.
        app.local_drafts.keys.insert(AgentId(9878), key.clone());
        app.sync_local_drafts(now + DEBOUNCE);
        assert_eq!(app.local_drafts.keys.len(), 1);
        assert!(app.local_drafts.loaded.contains(&key));
        assert!(app.local_drafts.tracked.contains_key(&key));
        assert_eq!(app.agents[&id].prompt.text(), "shared");
    }

    #[test]
    fn conflicting_prompt_sources_cannot_replace_or_remove_a_draft() {
        let directory = tempfile::tempdir().unwrap();
        let store = LocalDraftStore::new(directory.path().to_path_buf());
        let key = LocalDraftKey::session("exclusive").unwrap();
        let original = record("keep", None);
        store.write(&key, &original).unwrap();
        for text in ["conflict", ""] {
            let mut invalid = record(text, None);
            invalid.staged_prompt = invalid.composer.clone();
            assert_eq!(
                store.write(&key, &invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(store.load(&key).unwrap(), Some(original.clone()));
        }
        let mut staged_only = record("staged", None);
        staged_only.staged_prompt = staged_only.composer.take();
        store.write(&key, &staged_only).unwrap();
        assert_eq!(store.load(&key).unwrap(), Some(staged_only));
        store.write(&key, &LocalDraftRecord::empty(0)).unwrap();
        assert!(store.load(&key).unwrap().is_none());
    }

    #[test]
    fn conflicting_disk_record_is_quarantined_without_restoring() {
        let directory = tempfile::tempdir().unwrap();
        let store = LocalDraftStore::new(directory.path().to_path_buf());
        let mut app = crate::app::root::tests::test_app_with_agent();
        let agent = app.agents.values_mut().next().unwrap();
        agent.prompt.set_text("");
        agent.session.deferred_session_mode = None;
        let sid = acp_transport::protocol::SessionId::new("conflicting-draft");
        agent.session.session_id = Some(sid.clone());
        let key = LocalDraftKey::session(&sid.0).unwrap();
        let mut invalid = record("composer", Some(tools::types::BehaviorId::Plan));
        invalid.staged_prompt = record("staged", None).composer;
        let bytes = serde_json::to_vec(&invalid).unwrap();
        fs::write(store.path(&key), &bytes).unwrap();
        app.local_drafts = LocalDraftRuntime {
            store,
            ..Default::default()
        };
        app.sync_local_drafts(Instant::now());
        let agent = app.agents.values().next().unwrap();
        assert!(agent.prompt.text().is_empty());
        assert!(agent.session.pending_prompts.is_empty());
        assert!(agent.session.deferred_session_mode.is_none());
        let quarantined = fs::read_dir(directory.path().join("quarantine"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(quarantined.len(), 1);
        assert_eq!(fs::read(&quarantined[0]).unwrap(), bytes);
    }

    #[test]
    fn quarantine_leaves_a_replacement_entry_after_validating_open_source() {
        let directory = tempfile::tempdir().unwrap();
        let store = LocalDraftStore::new(directory.path().to_path_buf());
        let key = LocalDraftKey::session("replaced-draft").unwrap();
        store.ensure_root().unwrap();
        let path = store.path(&key);
        fs::write(&path, b"not json").unwrap();
        let source = File::open(&path).unwrap();

        // Replace the namespace entry while recovery still holds the original
        // corrupt inode. The replacement is valid and must remain active.
        fs::remove_file(&path).unwrap();
        let replacement = record("new valid draft", None);
        let replacement_bytes = serde_json::to_vec(&replacement).unwrap();
        let replacement_path = directory.path().join("replacement.tmp");
        fs::write(&replacement_path, &replacement_bytes).unwrap();
        fs::rename(&replacement_path, &path).unwrap();

        assert!(store.load_from_reader(&path, &source).unwrap().is_none());
        assert_eq!(store.load(&key).unwrap(), Some(replacement));
        assert!(!directory.path().join("quarantine").exists());
        assert_eq!(fs::read(&path).unwrap(), replacement_bytes);
    }

    #[test]
    fn source_byte_allowance_and_growth_quarantine() {
        let directory = tempfile::tempdir().unwrap();
        let store = LocalDraftStore::new(directory.path().to_path_buf());
        let key = LocalDraftKey::session("bounded").unwrap();
        let path = store.path(&key);
        let expected = record("draft", None);
        let mut bytes = serde_json::to_vec(&expected).unwrap();
        bytes.resize(MAX_RECORD_BYTES, b' ');
        fs::write(&path, &bytes).unwrap();
        assert_eq!(store.load(&key).unwrap(), Some(expected));
        let mut reader = File::open(&path).unwrap();
        assert_eq!(reader.metadata().unwrap().len(), MAX_RECORD_BYTES as u64);
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[b' '; 64])
            .unwrap();
        assert!(store.load_from_reader(&path, &reader).unwrap().is_none());
        use std::io::Seek;
        assert_eq!(
            reader.stream_position().unwrap(),
            MAX_RECORD_BYTES as u64 + 1
        );
        assert!(!path.exists());
        assert_eq!(
            fs::read_dir(directory.path().join("quarantine"))
                .unwrap()
                .count(),
            1
        );
        bytes.push(b' ');
        fs::write(&path, &bytes).unwrap();
        assert!(store.load(&key).unwrap().is_none());
        assert!(!path.exists());
        assert_eq!(
            fs::read_dir(directory.path().join("quarantine"))
                .unwrap()
                .count(),
            2
        );
    }

    #[cfg(unix)]
    #[test]
    fn special_sources_reject_without_blocking_or_quarantine() {
        use std::os::unix::{ffi::OsStrExt, fs::symlink};
        let directory = tempfile::tempdir().unwrap();
        let store = LocalDraftStore::new(directory.path().to_path_buf());
        let key = LocalDraftKey::session("special").unwrap();
        let path = store.path(&key);
        let target = directory.path().join("target");
        fs::write(
            &target,
            serde_json::to_vec(&record("linked", None)).unwrap(),
        )
        .unwrap();
        symlink(&target, &path).unwrap();
        assert_eq!(store.load(&key).unwrap(), Some(record("linked", None)));
        fs::remove_file(&path).unwrap();
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            for is_directory in [false, true] {
                if is_directory {
                    fs::remove_file(&path).unwrap();
                    fs::create_dir(&path).unwrap();
                }
                assert_eq!(
                    store.load(&key).unwrap_err().kind(),
                    io::ErrorKind::InvalidData
                );
                assert!(path.exists());
                assert!(!store.root.join("quarantine").exists());
            }
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn store_round_trips_and_isolates_keys() {
        let (root, store) = temp_store();
        let a = LocalDraftKey::session("a").unwrap();
        let b = LocalDraftKey::session("b").unwrap();
        store.write(&a, &record("one", None)).unwrap();
        store
            .write(&b, &record("two", Some(tools::types::BehaviorId::Plan)))
            .unwrap();
        store.write(&a, &record("updated", None)).unwrap();
        assert_eq!(store.load(&a).unwrap(), Some(record("updated", None)));
        assert_eq!(
            store.load(&b).unwrap(),
            Some(record("two", Some(tools::types::BehaviorId::Plan)))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(store.path(&a)).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_write_moves_the_deadline_out_of_the_ready_set() {
        let root = std::env::temp_dir().join(format!(
            "grow-pager-drafts-blocked-{}",
            uuid::Uuid::new_v4()
        ));
        fs::write(&root, b"not a directory").unwrap();
        let key = LocalDraftKey::session("retry").unwrap();
        let now = Instant::now();
        let mut runtime = LocalDraftRuntime {
            store: LocalDraftStore::new(root.clone()),
            ..Default::default()
        };
        runtime.tracked.insert(
            key,
            TrackedDraft {
                record: record("draft", None),
                serialized: Vec::new(),
                due: Some(now),
                last_deferred: None,
            },
        );

        runtime.flush_due(now);

        assert_eq!(runtime.next_deadline(), Some(now + WRITE_RETRY_BACKOFF));
        fs::remove_file(root).unwrap();
    }

    #[test]
    fn corrupt_record_is_quarantined() {
        let (root, store) = temp_store();
        let key = LocalDraftKey::session("bad").unwrap();
        store.ensure_root().unwrap();
        fs::write(store.path(&key), b"not json").unwrap();
        assert!(store.load(&key).unwrap().is_none());
        assert_eq!(fs::read_dir(root.join("quarantine")).unwrap().count(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn quarantine_rename_never_replaces_an_existing_entry() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let target = directory.path().join("target");
        fs::write(&source, b"source bytes").unwrap();
        fs::write(&target, b"existing quarantine entry").unwrap();

        let error = rename_no_replace(&source, &target).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&source).unwrap(), b"source bytes");
        assert_eq!(fs::read(&target).unwrap(), b"existing quarantine entry");
    }

    #[test]
    fn quarantine_retention_enforces_count_and_byte_limits() {
        let directory = tempfile::tempdir().unwrap();
        let quarantine = directory.path().join("quarantine");
        fs::create_dir(&quarantine).unwrap();
        for index in 0..=MAX_QUARANTINE_RECORDS {
            fs::write(quarantine.join(format!("{index:03}.bad")), b"bad").unwrap();
        }
        prune_quarantine(&quarantine).unwrap();
        assert_eq!(
            fs::read_dir(&quarantine).unwrap().count(),
            MAX_QUARANTINE_RECORDS
        );

        fs::remove_dir_all(&quarantine).unwrap();
        fs::create_dir(&quarantine).unwrap();
        let large = (MAX_QUARANTINE_BYTES / 2 + 1) as usize;
        fs::write(quarantine.join("first.bad"), vec![0; large]).unwrap();
        fs::write(quarantine.join("second.bad"), vec![0; large]).unwrap();
        prune_quarantine(&quarantine).unwrap();
        let entries = fs::read_dir(&quarantine)
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].metadata().unwrap().len() as u128 <= MAX_QUARANTINE_BYTES);
    }

    #[test]
    fn quarantine_reclamation_ties_are_filename_ordered() {
        let timestamp = SystemTime::UNIX_EPOCH;
        let a = (
            timestamp,
            std::ffi::OsString::from("a.bad"),
            PathBuf::from("a.bad"),
            1,
        );
        let b = (
            timestamp,
            std::ffi::OsString::from("b.bad"),
            PathBuf::from("b.bad"),
            1,
        );
        assert_eq!(compare_quarantine_entries(&a, &b), std::cmp::Ordering::Less);
        assert_eq!(
            compare_quarantine_entries(&b, &a),
            std::cmp::Ordering::Greater
        );
        let older = (
            timestamp - Duration::from_secs(1),
            std::ffi::OsString::from("z.bad"),
            PathBuf::from("z.bad"),
            1,
        );
        assert_eq!(
            compare_quarantine_entries(&older, &a),
            std::cmp::Ordering::Less
        );
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_retention_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let quarantine = directory.path().join("quarantine");
        fs::create_dir(&quarantine).unwrap();
        let target = directory.path().join("large-target");
        fs::write(&target, vec![0; MAX_QUARANTINE_BYTES as usize + 1]).unwrap();
        symlink(&target, quarantine.join("link.bad")).unwrap();

        prune_quarantine(&quarantine).unwrap();

        assert!(target.exists());
        assert!(quarantine.join("link.bad").exists());
    }

    #[test]
    fn cwd_record_is_atomically_rekeyed_to_session() {
        let (root, store) = temp_store();
        fs::create_dir_all(root.join("cwd")).unwrap();
        let from = LocalDraftKey::cwd(&root.join("cwd")).unwrap();
        let to = LocalDraftKey::session("bound").unwrap();
        store.write(&from, &record("draft", None)).unwrap();
        store.rekey(&from, &to).unwrap();
        assert!(store.load(&from).unwrap().is_none());
        assert_eq!(store.load(&to).unwrap(), Some(record("draft", None)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deferred_mode_survives_round_trip() {
        let (root, store) = temp_store();
        let key = LocalDraftKey::session("plan").unwrap();
        let value = record("draft", Some(tools::types::BehaviorId::Plan));
        store.write(&key, &value).unwrap();
        assert_eq!(
            store.load(&key).unwrap().unwrap().deferred_session_mode,
            Some(tools::types::BehaviorId::Plan)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn capture_uses_the_stashed_normal_composer() {
        let mut app = crate::app::root::tests::test_app_with_agent();
        let agent = app.agents.values_mut().next().unwrap();
        agent.prompt.set_text("modal input must not leak");
        let mut stash = StashedPrompt::default();
        stash.text = "normal draft".into();
        stash.cursor = 6;
        agent.stashed_prompt = Some(stash);
        let captured = capture_agent(agent, 1).unwrap();
        let composer = captured.composer.unwrap();
        assert_eq!(composer.text, "normal draft");
        assert_eq!(composer.cursor, 6);
    }

    #[test]
    fn uncertain_submission_restores_original_identity_without_queuing() {
        let mut first = crate::app::root::tests::test_app_with_agent();
        let agent = first.agents.values_mut().next().unwrap();
        agent.session.in_flight_prompt = Some(crate::app::session::InFlightPrompt {
            prompt_id: Some("original-prompt-id".into()),
            admission_resolved: None,
            expected_turn_id: Some("original-turn-id".into()),
            frozen_skill_selection: None,
            text: "submitted once".into(),
            images: Vec::new(),
            scrollback_entry: crate::scrollback::EntryId::new(1),
            combined_scrollback_entries: Vec::new(),
            chip_elements: Vec::new(),
        });
        let captured = capture_agent(agent, 1).unwrap();
        assert!(captured.composer.is_none());
        let (root, store) = temp_store();
        let key = LocalDraftKey::session("uncertain").unwrap();
        store.write(&key, &captured).unwrap();
        let restored = store.load(&key).unwrap().unwrap();

        let mut second = crate::app::root::tests::test_app_with_agent();
        let agent = second.agents.values_mut().next().unwrap();
        restore_agent(agent, &restored);
        assert_eq!(agent.prompt.text(), "submitted once");
        assert_eq!(
            agent
                .session
                .in_flight_prompt
                .as_ref()
                .and_then(|prompt| prompt.prompt_id.as_deref()),
            Some("original-prompt-id")
        );
        assert_eq!(
            agent
                .session
                .in_flight_prompt
                .as_ref()
                .and_then(|prompt| prompt.expected_turn_id.as_deref()),
            Some("original-turn-id")
        );
        assert!(agent.session.pending_prompts.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn uncertain_skill_submission_restores_frozen_source_with_original_identity() {
        let mut first = crate::app::root::tests::test_app_with_agent();
        let agent = first.agents.values_mut().next().unwrap();
        agent.session.session_id = Some(acp_transport::protocol::SessionId::new("skill-source"));
        let selection = crate::app::session::FrozenSkillSelection {
            name: "local:review".into(),
            path: "/repo/.grow/skills/review/SKILL.md".into(),
            plugin_name: None,
        };
        agent.session.in_flight_prompt = Some(crate::app::session::InFlightPrompt {
            prompt_id: Some("selected-skill-prompt".into()),
            admission_resolved: None,
            expected_turn_id: None,
            frozen_skill_selection: Some(selection.clone()),
            text: "/local:review inspect".into(),
            images: Vec::new(),
            scrollback_entry: crate::scrollback::EntryId::new(1),
            combined_scrollback_entries: Vec::new(),
            chip_elements: Vec::new(),
        });

        let captured = capture_agent(agent, 1).unwrap();
        let (root, store) = temp_store();
        let key = LocalDraftKey::session("skill-source").unwrap();
        store.write(&key, &captured).unwrap();
        let restored_record = store.load(&key).unwrap().unwrap();
        assert_eq!(
            restored_record
                .uncertain_submission
                .as_ref()
                .and_then(|submission| submission.prompt.frozen_skill_selection.as_ref()),
            Some(&selection)
        );

        let mut second = crate::app::root::tests::test_app_with_agent();
        let agent = second.agents.values_mut().next().unwrap();
        restore_agent(agent, &restored_record);
        let restored = agent
            .session
            .in_flight_prompt
            .as_ref()
            .expect("restored intent retains source identity");
        assert_eq!(restored.prompt_id.as_deref(), Some("selected-skill-prompt"));
        assert_eq!(restored.frozen_skill_selection.as_ref(), Some(&selection));
        assert_eq!(restored.text, "/local:review inspect");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn persisted_uncertain_fifo_retry_reuses_identity_and_late_admission_keeps_edit() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("drafts");
        let agent_id = AgentId(0);
        let session_id = acp_transport::protocol::SessionId::new("joined-input-retry");
        let prompt_id = "original-fifo-prompt";
        let original_text = "the frozen request";

        let mut first = crate::app::root::tests::test_app_with_agent();
        first.local_drafts = LocalDraftRuntime::direct(LocalDraftStore::new(root.clone()));
        first
            .agents
            .get_mut(&agent_id)
            .expect("test agent")
            .session
            .session_id = Some(session_id.clone());
        first
            .agents
            .get_mut(&agent_id)
            .expect("test agent")
            .session
            .in_flight_prompt = Some(crate::app::session::InFlightPrompt {
            prompt_id: Some(prompt_id.into()),
            admission_resolved: None,
            expected_turn_id: None,
            frozen_skill_selection: None,
            text: original_text.into(),
            images: Vec::new(),
            scrollback_entry: crate::scrollback::EntryId::new(1),
            combined_scrollback_entries: Vec::new(),
            chip_elements: Vec::new(),
        });
        let checkpoint = first
            .checkpoint_input_intent(agent_id, prompt_id.into(), None, None)
            .expect("checkpoint input intent");
        checkpoint
            .await
            .expect("checkpoint acknowledgement")
            .unwrap();

        let key = LocalDraftKey::session(&session_id.0).unwrap();
        let saved = LocalDraftStore::new(root.clone())
            .load(&key)
            .unwrap()
            .expect("durable local draft snapshot");
        let saved_input = saved
            .uncertain_submission
            .as_ref()
            .expect("uncertain submission snapshot");
        assert_eq!(saved_input.prompt_id, prompt_id);
        assert_eq!(
            saved_input.expected_turn_id, None,
            "FIFO route has no steer target"
        );
        assert_eq!(saved_input.prompt.text, original_text);

        // A fresh app loads the persisted snapshot. Loading restores state only;
        // it cannot drain a queue or issue a prompt RPC.
        let mut restored = crate::app::root::tests::test_app_with_agent();
        restored.local_drafts = LocalDraftRuntime::direct(LocalDraftStore::new(root));
        restored
            .agents
            .get_mut(&agent_id)
            .expect("fresh test agent")
            .session
            .session_id = Some(session_id.clone());
        restored.sync_local_drafts(Instant::now() + DEBOUNCE);
        let restored_agent = &restored.agents[&agent_id];
        assert_eq!(restored_agent.prompt.text(), original_text);
        assert!(restored_agent.session.pending_prompts.is_empty());
        assert_eq!(
            restored_agent
                .session
                .in_flight_prompt
                .as_ref()
                .and_then(|input| input.prompt_id.as_deref()),
            Some(prompt_id)
        );

        let effects = crate::app::root::dispatch::dispatch(
            crate::app::actions::Action::SendPrompt(original_text.into()),
            &mut restored,
        );
        let retries = effects
            .iter()
            .filter_map(|effect| match effect {
                crate::app::actions::Effect::SendPrompt {
                    prompt_id, text, ..
                } => Some((prompt_id.as_str(), text.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(retries, vec![(prompt_id, original_text)]);
        assert!(effects.iter().all(|effect| !matches!(
            effect,
            crate::app::actions::Effect::SendInterject { .. }
                | crate::app::actions::Effect::SendPromptBlocks { .. }
        )));

        restored
            .agents
            .get_mut(&agent_id)
            .expect("restored test agent")
            .prompt
            .set_text("newer user edit");
        let binding_epoch = restored.agents[&agent_id].session_binding_epoch;
        let late_admission = crate::app::root::dispatch::dispatch(
            crate::app::actions::Action::TaskComplete(
                crate::app::actions::TaskResult::InputAdmissionStatusResolved {
                    agent_id,
                    session_id: session_id.clone(),
                    binding_epoch,
                    prompt_id: prompt_id.into(),
                    response_session_id: Some(session_id.0.to_string()),
                    response_prompt_id: Some(prompt_id.into()),
                    response_source: Some("durable".into()),
                    status: Ok(crate::app::actions::InputAdmissionStatusWire::Allowed {
                        route: chat_state::InputRoute::Fifo,
                    }),
                },
            ),
            &mut restored,
        );
        assert!(late_admission.is_empty());
        let agent = &restored.agents[&agent_id];
        assert_eq!(agent.prompt.text(), "newer user edit");
        assert!(agent.session.pending_prompts.is_empty());
        let original = agent
            .session
            .in_flight_prompt
            .as_ref()
            .expect("original input identity remains observable");
        assert_eq!(original.prompt_id.as_deref(), Some(prompt_id));
        assert_eq!(original.admission_resolved, Some(true));

        assert!(
            crate::app::root::dispatch::dispatch(
                crate::app::actions::Action::SendPrompt(original_text.into()),
                &mut restored,
            )
            .is_empty()
        );
        assert_eq!(restored.agents[&agent_id].prompt.text(), "newer user edit");
    }

    #[tokio::test]
    async fn input_checkpoint_ack_requires_the_frozen_prompt_and_route() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = crate::app::root::tests::test_app_with_agent();
        let agent_id = *app.agents.keys().next().unwrap();
        let session_id = acp_transport::protocol::SessionId::new("checkpoint-session");
        let agent = app.agents.get_mut(&agent_id).unwrap();
        agent.session.session_id = Some(session_id.clone());
        agent.session.in_flight_prompt = Some(crate::app::session::InFlightPrompt {
            prompt_id: Some("checkpoint-prompt".into()),
            admission_resolved: None,
            expected_turn_id: Some("checkpoint-turn".into()),
            frozen_skill_selection: None,
            text: "frozen request".into(),
            images: Vec::new(),
            scrollback_entry: crate::scrollback::EntryId::new(1),
            combined_scrollback_entries: Vec::new(),
            chip_elements: Vec::new(),
        });
        app.local_drafts =
            LocalDraftRuntime::direct(LocalDraftStore::new(directory.path().to_path_buf()));

        let acknowledged = app
            .local_drafts
            .checkpoint_input_intent(
                &app.agents,
                Some(agent_id),
                agent_id,
                "checkpoint-prompt".into(),
                Some("checkpoint-turn".into()),
                None,
            )
            .unwrap();
        acknowledged.await.unwrap().unwrap();
        let key = LocalDraftKey::session(&session_id.0).unwrap();
        let saved = app.local_drafts.store.load(&key).unwrap().unwrap();
        assert_eq!(
            saved.uncertain_submission.as_ref().unwrap().prompt_id,
            "checkpoint-prompt"
        );
        assert_eq!(
            saved
                .uncertain_submission
                .as_ref()
                .unwrap()
                .expected_turn_id
                .as_deref(),
            Some("checkpoint-turn")
        );

        let failed = app.local_drafts.checkpoint_input_intent(
            &app.agents,
            Some(agent_id),
            agent_id,
            "checkpoint-prompt".into(),
            Some("changed-turn".into()),
            None,
        );
        assert!(matches!(
            failed,
            Err(error) if error.kind() == io::ErrorKind::InvalidData
        ));
    }

    #[tokio::test]
    async fn ram_only_input_retry_does_not_loop_through_the_failed_writer() {
        let mut app = crate::app::root::tests::test_app_with_agent();
        let agent_id = *app.agents.keys().next().unwrap();
        app.local_drafts
            .mark_input_ram_only(agent_id, "ram-only-id".into());
        let acknowledged = app
            .local_drafts
            .checkpoint_input_intent(
                &app.agents,
                Some(agent_id),
                agent_id,
                "ram-only-id".into(),
                Some("original-turn".into()),
                None,
            )
            .unwrap();
        acknowledged.await.unwrap().unwrap();
    }

    #[test]
    fn runtime_restores_composer_and_deferred_latch_without_sending() {
        let (root, store) = temp_store();
        let mut first = crate::app::root::tests::test_app_with_agent();
        first.local_drafts = LocalDraftRuntime {
            store,
            ..Default::default()
        };
        let agent = first.agents.values_mut().next().unwrap();
        agent
            .session
            .pending_prompts
            .push_back(crate::app::session::QueuedPrompt::plain(
                1,
                "resume this plan",
                QueueEntryKind::Prompt,
            ));
        agent.session.deferred_session_mode = Some(tools::types::BehaviorId::Plan);
        let now = Instant::now();
        first.sync_local_drafts(now);
        first.sync_local_drafts(now + DEBOUNCE);

        let mut second = crate::app::root::tests::test_app_with_agent();
        second.local_drafts = LocalDraftRuntime {
            store: LocalDraftStore::new(root.clone()),
            ..Default::default()
        };
        second.sync_local_drafts(now + DEBOUNCE + DEBOUNCE);
        let restored = second.agents.values().next().unwrap();
        assert_eq!(restored.prompt.text(), "resume this plan");
        assert_eq!(
            restored.session.deferred_session_mode,
            Some(tools::types::BehaviorId::Plan)
        );
        assert!(restored.session.pending_prompts.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn every_prompt_rpc_variant_clears_and_cannot_revive_the_draft() {
        for structured in [false, true] {
            let (root, store) = temp_store();
            let mut app = crate::app::root::tests::test_app_with_agent();
            app.local_drafts = LocalDraftRuntime {
                store,
                ..Default::default()
            };
            let (agent_id, session_id) = {
                let (agent_id, agent) = app.agents.iter_mut().next().unwrap();
                agent.prompt.set_text("owned by the next RPC");
                agent.session.in_flight_prompt = Some(crate::app::session::InFlightPrompt {
                    prompt_id: Some(if structured { "blocks" } else { "text" }.into()),
                    admission_resolved: None,
                    expected_turn_id: None,
                    frozen_skill_selection: None,
                    text: "owned by the next RPC".into(),
                    images: Vec::new(),
                    scrollback_entry: crate::scrollback::EntryId::new(1),
                    combined_scrollback_entries: Vec::new(),
                    chip_elements: Vec::new(),
                });
                (*agent_id, agent.session.session_id.clone().unwrap())
            };
            let now = Instant::now();
            app.sync_local_drafts(now);
            app.sync_local_drafts(now + DEBOUNCE);
            let key = LocalDraftKey::session(&session_id.0).unwrap();
            assert!(
                LocalDraftStore::new(root.clone())
                    .load(&key)
                    .unwrap()
                    .is_some()
            );

            let effect = if structured {
                crate::app::actions::Effect::SendPromptBlocks {
                    agent_id,
                    session_id,
                    blocks: Vec::new(),
                    images: Vec::new(),
                    prompt_id: "blocks".into(),
                }
            } else {
                crate::app::actions::Effect::SendPrompt {
                    agent_id,
                    session_id,
                    text: "owned by the next RPC".into(),
                    prompt_id: "text".into(),
                    skill_token_ranges: Vec::new(),
                }
            };
            app.transfer_local_draft_ownership(&effect);
            assert!(
                LocalDraftStore::new(root.clone())
                    .load(&key)
                    .unwrap()
                    .is_none()
            );

            app.agents.get_mut(&agent_id).unwrap().prompt.set_text("");
            app.sync_local_drafts(now + DEBOUNCE + DEBOUNCE);
            app.sync_local_drafts(now + DEBOUNCE + DEBOUNCE + DEBOUNCE);
            assert!(
                LocalDraftStore::new(root.clone())
                    .load(&key)
                    .unwrap()
                    .is_none()
            );
            let _ = fs::remove_dir_all(root);
        }
    }

    #[tokio::test]
    async fn ownership_transfer_clears_cwd_key_before_bind_rekey_sync() {
        let (root, store) = temp_store();
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            store,
            ..Default::default()
        };
        let (agent_id, cwd) = {
            let (agent_id, agent) = app.agents.iter_mut().next().unwrap();
            agent.session.session_id = None;
            agent.prompt.set_text("pre-bind draft");
            (*agent_id, agent.session.cwd.clone())
        };
        let cwd_key = LocalDraftKey::cwd(&cwd).unwrap();
        app.agents
            .get_mut(&agent_id)
            .unwrap()
            .session
            .in_flight_prompt = Some(crate::app::session::InFlightPrompt {
            prompt_id: Some("binding-race".into()),
            admission_resolved: None,
            expected_turn_id: None,
            frozen_skill_selection: None,
            text: "pre-bind draft".into(),
            images: Vec::new(),
            scrollback_entry: crate::scrollback::EntryId::new(1),
            combined_scrollback_entries: Vec::new(),
            chip_elements: Vec::new(),
        });
        let now = Instant::now();
        app.sync_local_drafts(now);
        app.sync_local_drafts(now + DEBOUNCE);
        let checkpoint = app
            .local_drafts
            .checkpoint_input_intent(
                &app.agents,
                Some(agent_id),
                agent_id,
                "binding-race".into(),
                None,
                None,
            )
            .unwrap();
        checkpoint.await.unwrap().unwrap();
        assert!(
            LocalDraftStore::new(root.clone())
                .load(&cwd_key)
                .unwrap()
                .is_some()
        );

        // Binding has happened in AgentSession, but LocalDraftRuntime has not
        // observed it yet and still routes this agent through `cwd_key`.
        let session_id = acp_transport::protocol::SessionId::new("bound-before-draft-sync");
        app.agents.get_mut(&agent_id).unwrap().session.session_id = Some(session_id.clone());
        let effect = crate::app::actions::Effect::SendPrompt {
            agent_id,
            session_id: session_id.clone(),
            text: "pre-bind draft".into(),
            prompt_id: "binding-race".into(),
            skill_token_ranges: Vec::new(),
        };
        app.transfer_local_draft_ownership(&effect);

        let store = LocalDraftStore::new(root.clone());
        assert!(store.load(&cwd_key).unwrap().is_none());
        assert!(
            store
                .load(&LocalDraftKey::session(&session_id.0).unwrap())
                .unwrap()
                .is_none()
        );
        app.agents.get_mut(&agent_id).unwrap().prompt.set_text("");
        app.sync_local_drafts(now + DEBOUNCE + DEBOUNCE);
        app.sync_local_drafts(now + DEBOUNCE + DEBOUNCE + DEBOUNCE);
        assert!(store.load(&cwd_key).unwrap().is_none());
        assert!(
            store
                .load(&LocalDraftKey::session(&session_id.0).unwrap())
                .unwrap()
                .is_none()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slow_draft_write_does_not_block_new_input_and_checkpoint() {
        let directory = tempfile::tempdir().unwrap();
        let gate = StoreGate::new("write");
        let mut store = LocalDraftStore::new(directory.path().to_path_buf());
        store.gate = Some(Arc::clone(&gate));
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(store)),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        let session_id = acp_transport::protocol::SessionId::new("slow-write");
        app.agents.get_mut(&id).unwrap().session.session_id = Some(session_id.clone());
        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text("old draft");
        app.sync_local_drafts(Instant::now());
        gate.entered.wait();

        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text("new draft");
        let started = Instant::now();
        app.sync_local_drafts(Instant::now());
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(app.agents[&id].prompt.text(), "new draft");
        let checkpoint = app.checkpoint_local_drafts();
        tokio::pin!(checkpoint);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut checkpoint)
                .await
                .is_err(),
            "normal quit must await the held write"
        );
        gate.release.wait();
        tokio::time::timeout(Duration::from_secs(2), &mut checkpoint)
            .await
            .unwrap()
            .unwrap();
        let saved = LocalDraftStore::new(directory.path().to_path_buf())
            .load(&LocalDraftKey::session(&session_id.0).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(saved.composer.unwrap().text, "new draft");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slow_draft_load_cannot_replace_new_edit() {
        let directory = tempfile::tempdir().unwrap();
        let session_id = acp_transport::protocol::SessionId::new("slow-load");
        let key = LocalDraftKey::session(&session_id.0).unwrap();
        LocalDraftStore::new(directory.path().to_path_buf())
            .write(&key, &record("saved old draft", None))
            .unwrap();
        let gate = StoreGate::new("load");
        let mut store = LocalDraftStore::new(directory.path().to_path_buf());
        store.gate = Some(Arc::clone(&gate));
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(store)),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        app.agents.get_mut(&id).unwrap().session.session_id = Some(session_id);
        app.sync_local_drafts(Instant::now());
        gate.entered.wait();
        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text("typed while loading");
        app.sync_local_drafts(Instant::now());
        gate.release.wait();
        tokio::time::timeout(
            Duration::from_secs(2),
            app.local_draft_ready_notify().unwrap().notified(),
        )
        .await
        .unwrap();
        app.sync_local_drafts(Instant::now());
        assert_eq!(app.agents[&id].prompt.text(), "typed while loading");
        app.checkpoint_local_drafts().await.unwrap();
        let saved = LocalDraftStore::new(directory.path().to_path_buf())
            .load(&key)
            .unwrap()
            .unwrap();
        assert_eq!(saved.composer.unwrap().text, "typed while loading");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn quit_before_recovery_ack_keeps_saved_draft() {
        let directory = tempfile::tempdir().unwrap();
        let session_id = acp_transport::protocol::SessionId::new("quit-pending-load");
        let key = LocalDraftKey::session(&session_id.0).unwrap();
        LocalDraftStore::new(directory.path().to_path_buf())
            .write(&key, &record("recoverable", None))
            .unwrap();
        let gate = StoreGate::new("load");
        let mut store = LocalDraftStore::new(directory.path().to_path_buf());
        store.gate = Some(Arc::clone(&gate));
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(store)),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        app.agents.get_mut(&id).unwrap().session.session_id = Some(session_id);
        app.sync_local_drafts(Instant::now());
        gate.entered.wait();
        let checkpoint = app.checkpoint_local_drafts();
        tokio::pin!(checkpoint);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut checkpoint)
                .await
                .is_err()
        );
        gate.release.wait();
        tokio::time::timeout(Duration::from_secs(2), &mut checkpoint)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            LocalDraftStore::new(directory.path().to_path_buf())
                .load(&key)
                .unwrap()
                .unwrap()
                .composer
                .unwrap()
                .text,
            "recoverable"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn worker_applies_saved_draft_when_binding_is_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let session_id = acp_transport::protocol::SessionId::new("async-restore");
        let key = LocalDraftKey::session(&session_id.0).unwrap();
        LocalDraftStore::new(directory.path().to_path_buf())
            .write(&key, &record("saved input", None))
            .unwrap();
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(LocalDraftStore::new(
                directory.path().to_path_buf(),
            ))),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        app.agents.get_mut(&id).unwrap().session.session_id = Some(session_id);
        app.sync_local_drafts(Instant::now());
        tokio::time::timeout(
            Duration::from_secs(2),
            app.local_draft_ready_notify().unwrap().notified(),
        )
        .await
        .unwrap();
        app.sync_local_drafts(Instant::now());
        assert_eq!(app.agents[&id].prompt.text(), "saved input");
        app.checkpoint_local_drafts().await.unwrap();
    }

    #[tokio::test]
    async fn worker_restores_pending_skill_as_unsent_draft_with_frozen_source() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("drafts");
        let session_id = acp_transport::protocol::SessionId::new("staged-skill-restore");
        let selection = crate::app::session::FrozenSkillSelection {
            name: "local:review".into(),
            path: "/repo/.grow/skills/review/SKILL.md".into(),
            plugin_name: None,
        };
        let mut first = crate::app::root::tests::test_app_with_agent();
        first.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(LocalDraftStore::new(
                root.clone(),
            ))),
            ..Default::default()
        };
        let agent_id = *first.agents.keys().next().unwrap();
        let agent = first.agents.get_mut(&agent_id).unwrap();
        agent.session.session_id = Some(session_id.clone());
        let mut block =
            acp_transport::protocol::TextContent::new("/local:review inspect".to_owned());
        block.meta = Some(
            [(
                "grow/skillSelection".into(),
                serde_json::to_value(&selection).unwrap(),
            )]
            .into_iter()
            .collect(),
        );
        agent
            .session
            .pending_prompts
            .push_back(crate::app::session::QueuedPrompt {
                wire_blocks: Some(vec![acp_transport::protocol::ContentBlock::Text(block)]),
                display_as_skill: true,
                ..crate::app::session::QueuedPrompt::plain(
                    1,
                    "/local:review inspect",
                    crate::app::session::QueueEntryKind::Prompt,
                )
            });
        first.sync_local_drafts(Instant::now());
        first.checkpoint_local_drafts().await.unwrap();

        let key = LocalDraftKey::session(&session_id.0).unwrap();
        let saved = LocalDraftStore::new(root.clone())
            .load(&key)
            .unwrap()
            .expect("pending selected skill must be checkpointed");
        assert!(saved.uncertain_submission.is_none());
        assert_eq!(
            saved
                .staged_prompt
                .as_ref()
                .and_then(|draft| draft.frozen_skill_selection.as_ref()),
            Some(&selection)
        );

        let mut restored = crate::app::root::tests::test_app_with_agent();
        restored.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(LocalDraftStore::new(root))),
            ..Default::default()
        };
        restored
            .agents
            .get_mut(&agent_id)
            .unwrap()
            .session
            .session_id = Some(session_id);
        restored.sync_local_drafts(Instant::now());
        tokio::time::timeout(
            Duration::from_secs(2),
            restored.local_draft_ready_notify().unwrap().notified(),
        )
        .await
        .unwrap();
        restored.sync_local_drafts(Instant::now());

        let agent = &restored.agents[&agent_id];
        assert_eq!(agent.prompt.text(), "/local:review inspect");
        assert_eq!(
            agent.session.staged_skill_selection.as_ref(),
            Some(&selection)
        );
        assert!(agent.session.pending_prompts.is_empty());
        assert!(agent.session.in_flight_prompt.is_none());

        crate::app::root::dispatch::dispatch(
            crate::app::actions::Action::SendPrompt("/local:review inspect".into()),
            &mut restored,
        );
        let agent = &restored.agents[&agent_id];
        let sent = agent
            .session
            .in_flight_prompt
            .as_ref()
            .expect("explicit send drains through normal input path");
        assert_eq!(sent.frozen_skill_selection.as_ref(), Some(&selection));
        assert!(
            sent.prompt_id.is_some(),
            "explicit send receives a fresh identity"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_bind_does_not_restore_staged_cwd_prompt_over_first_send_checkpoint() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("drafts");
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(LocalDraftStore::new(
                root.clone(),
            ))),
            ..Default::default()
        };
        let agent_id = *app.agents.keys().next().unwrap();
        app.agents.get_mut(&agent_id).unwrap().session.session_id = None;

        // The user submits before ACP session creation finishes. That first
        // send is kept in the queue and checkpointed under the cwd identity.
        assert!(
            crate::app::root::dispatch::dispatch(
                crate::app::actions::Action::SendPrompt("first queued request".into()),
                &mut app,
            )
            .is_empty()
        );
        assert_eq!(app.agents[&agent_id].session.pending_prompts.len(), 1);
        let cwd_key = LocalDraftKey::cwd(&app.agents[&agent_id].session.cwd).unwrap();
        app.sync_local_drafts(Instant::now());
        app.checkpoint_local_drafts().await.unwrap();
        let staged = LocalDraftStore::new(root.clone())
            .load(&cwd_key)
            .unwrap()
            .expect("pre-session queued prompt must be checkpointed");
        assert_eq!(
            staged
                .staged_prompt
                .as_ref()
                .map(|prompt| prompt.text.as_str()),
            Some("first queued request")
        );

        // SessionCreated drains the queued row into in_flight_prompt. The
        // worker then rekeys the cwd draft to the new session key while
        // validating this exact send identity.
        let session_id = acp_transport::protocol::SessionId::new("first-send-session");
        let effects = crate::app::root::dispatch::dispatch(
            crate::app::actions::Action::TaskComplete(
                crate::app::actions::TaskResult::SessionCreated {
                    agent_id,
                    session_id: session_id.clone(),
                    models: None,
                },
            ),
            &mut app,
        );
        let (send_agent, send_session, prompt_id, expected_turn_id, fallback) = {
            let send = effects
                .iter()
                .find(|effect| matches!(effect, crate::app::actions::Effect::SendPrompt { .. }))
                .expect("SessionCreated must produce the drained prompt send");
            let (send_agent, send_session, prompt_id, route) =
                send.prompt_rpc_identity().expect("send prompt identity");
            (
                send_agent,
                send_session.clone(),
                prompt_id.to_owned(),
                route.map(str::to_owned),
                send.checkpoint_fallback(),
            )
        };
        assert_eq!(send_agent, agent_id);
        assert_eq!(send_session, session_id);

        let checkpoint = app
            .checkpoint_input_intent(send_agent, prompt_id.clone(), expected_turn_id, fallback)
            .expect("input intent checkpoint must be queued");
        checkpoint
            .await
            .expect("local draft worker must acknowledge checkpoint")
            .expect("the current in-flight prompt must remain in the draft snapshot");

        let saved = LocalDraftStore::new(root)
            .load(&LocalDraftKey::session(&session_id.0).unwrap())
            .unwrap()
            .expect("session-bound draft must exist");
        let submitted = saved
            .uncertain_submission
            .as_ref()
            .expect("checkpoint must retain the in-flight identity");
        assert_eq!(submitted.prompt_id, prompt_id);
        assert_eq!(submitted.prompt.text, "first queued request");
        assert!(saved.staged_prompt.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn identity_transfer_follows_blocked_write_before_new_draft() {
        let directory = tempfile::tempdir().unwrap();
        let gate = StoreGate::new("write");
        let mut store = LocalDraftStore::new(directory.path().to_path_buf());
        store.gate = Some(Arc::clone(&gate));
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(store)),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        let session_id = acp_transport::protocol::SessionId::new("ordered-invalidation");
        let key = LocalDraftKey::session(&session_id.0).unwrap();
        let agent = app.agents.get_mut(&id).unwrap();
        agent.session.session_id = Some(session_id.clone());
        agent.prompt.set_text("submitted");
        agent.session.in_flight_prompt = Some(crate::app::session::InFlightPrompt {
            prompt_id: Some("ordered-invalidation".into()),
            admission_resolved: None,
            expected_turn_id: None,
            frozen_skill_selection: None,
            text: "submitted".into(),
            images: Vec::new(),
            scrollback_entry: crate::scrollback::EntryId::new(1),
            combined_scrollback_entries: Vec::new(),
            chip_elements: Vec::new(),
        });
        app.sync_local_drafts(Instant::now());
        gate.entered.wait();
        let effect = crate::app::actions::Effect::SendPrompt {
            agent_id: id,
            session_id,
            text: "submitted".into(),
            prompt_id: "ordered-invalidation".into(),
            skill_token_ranges: Vec::new(),
        };
        app.transfer_local_draft_ownership(&effect);
        app.agents.get_mut(&id).unwrap().prompt.set_text("");
        app.sync_local_drafts(Instant::now());
        gate.release.wait();
        app.checkpoint_local_drafts().await.unwrap();
        assert!(
            LocalDraftStore::new(directory.path().to_path_buf())
                .load(&key)
                .unwrap()
                .is_none()
        );

        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text("fresh after RPC");
        app.sync_local_drafts(Instant::now());
        app.checkpoint_local_drafts().await.unwrap();
        let saved = LocalDraftStore::new(directory.path().to_path_buf())
            .load(&key)
            .unwrap()
            .unwrap();
        assert_eq!(saved.composer.unwrap().text, "fresh after RPC");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn closing_agent_keeps_last_edit_coalesced_behind_slow_io() {
        let directory = tempfile::tempdir().unwrap();
        let gate = StoreGate::new("write");
        let mut store = LocalDraftStore::new(directory.path().to_path_buf());
        store.gate = Some(Arc::clone(&gate));
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(store)),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        let session_id = acp_transport::protocol::SessionId::new("close-behind-slow-write");
        let key = LocalDraftKey::session(&session_id.0).unwrap();
        app.agents.get_mut(&id).unwrap().session.session_id = Some(session_id);
        app.agents.get_mut(&id).unwrap().prompt.set_text("first");
        app.sync_local_drafts(Instant::now());
        gate.entered.wait();
        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text("last edit");
        app.sync_local_drafts(Instant::now());
        app.agents.shift_remove(&id);
        app.sync_local_drafts(Instant::now());
        gate.release.wait();
        app.checkpoint_local_drafts().await.unwrap();
        let saved = LocalDraftStore::new(directory.path().to_path_buf())
            .load(&key)
            .unwrap()
            .unwrap();
        assert_eq!(saved.composer.unwrap().text, "last edit");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_binding_keeps_last_cwd_edit_behind_slow_io() {
        let directory = tempfile::tempdir().unwrap();
        let gate = StoreGate::new("write");
        let mut store = LocalDraftStore::new(directory.path().to_path_buf());
        store.gate = Some(Arc::clone(&gate));
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(store)),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        let cwd = app.agents[&id].session.cwd.clone();
        app.agents.get_mut(&id).unwrap().session.session_id = None;
        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text("first cwd edit");
        app.sync_local_drafts(Instant::now());
        gate.entered.wait();
        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text("last cwd edit");
        app.sync_local_drafts(Instant::now());
        let session_id = acp_transport::protocol::SessionId::new("bound-after-slow-write");
        let session_key = LocalDraftKey::session(&session_id.0).unwrap();
        app.agents.get_mut(&id).unwrap().session.session_id = Some(session_id);
        app.sync_local_drafts(Instant::now());
        gate.release.wait();
        app.checkpoint_local_drafts().await.unwrap();
        let saved = LocalDraftStore::new(directory.path().to_path_buf())
            .load(&session_key)
            .unwrap()
            .unwrap();
        assert_eq!(saved.composer.unwrap().text, "last cwd edit");
        assert!(
            LocalDraftStore::new(directory.path().to_path_buf())
                .load(&LocalDraftKey::cwd(&cwd).unwrap())
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn worker_retries_failed_write_without_busy_ui_timer() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("drafts");
        fs::write(&root, b"blocked directory").unwrap();
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime {
            worker: Some(DraftIoWorker::with_store(LocalDraftStore::new(
                root.clone(),
            ))),
            ..Default::default()
        };
        let id = *app.agents.keys().next().unwrap();
        let session_id = acp_transport::protocol::SessionId::new("retry-after-filesystem-recovers");
        let key = LocalDraftKey::session(&session_id.0).unwrap();
        app.agents.get_mut(&id).unwrap().session.session_id = Some(session_id);
        app.agents
            .get_mut(&id)
            .unwrap()
            .prompt
            .set_text("recoverable write");
        app.sync_local_drafts(Instant::now());
        tokio::time::timeout(
            Duration::from_secs(2),
            app.local_draft_ready_notify().unwrap().notified(),
        )
        .await
        .unwrap();
        assert!(app.local_draft_deadline().is_none());
        fs::remove_file(&root).unwrap();
        fs::create_dir(&root).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if LocalDraftStore::new(root.clone())
                    .load(&key)
                    .ok()
                    .flatten()
                    .is_some()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("worker must retry after the filesystem recovers");
    }
}

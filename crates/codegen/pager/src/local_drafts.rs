//! Pager-owned recovery for input which has not crossed the ACP prompt RPC.
//!
//! This is deliberately not session state.  It never replays a prompt and it
//! never stores Shell control-plane projections.  Recovery only repopulates
//! the local composer and the pre-admission Behavior latch.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
        })
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
    deferred_session_mode: Option<tools::types::BehaviorId>,
}

impl LocalDraftRecord {
    fn empty(revision: u64) -> Self {
        Self {
            version: SCHEMA_VERSION,
            revision,
            composer: None,
            staged_prompt: None,
            deferred_session_mode: None,
        }
    }

    fn has_payload(&self) -> bool {
        self.composer.as_ref().is_some_and(|p| !p.is_empty())
            || self.staged_prompt.as_ref().is_some_and(|p| !p.is_empty())
            || self.deferred_session_mode.is_some()
    }
}

#[derive(Debug)]
pub(crate) struct LocalDraftStore {
    root: PathBuf,
}

impl Default for LocalDraftStore {
    fn default() -> Self {
        Self::new(tools::util::grow_home::grow_home().join("pager-drafts-v1"))
    }
}

impl LocalDraftStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path(&self, key: &LocalDraftKey) -> PathBuf {
        self.root.join(key.filename())
    }

    fn ensure_root(&self) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        set_mode(&self.root, 0o700)
    }

    pub(crate) fn load(&self, key: &LocalDraftKey) -> io::Result<Option<LocalDraftRecord>> {
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
            return Err(io::Error::new(io::ErrorKind::InvalidData, "local draft source is not a regular file"));
        }
        if metadata.len() > MAX_RECORD_BYTES as u64 {
            self.quarantine(&path)?;
            return Ok(None);
        }
        self.load_from_reader(&path, file)
    }

    fn load_from_reader(&self, path: &Path, reader: impl Read) -> io::Result<Option<LocalDraftRecord>> {
        let mut bytes = Vec::new();
        reader.take(MAX_RECORD_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_RECORD_BYTES {
            self.quarantine(path)?;
            return Ok(None);
        }
        let record = serde_json::from_slice::<LocalDraftRecord>(&bytes).ok();
        let Some(record) = record.filter(|r| r.version == SCHEMA_VERSION && validate_record(r))
        else {
            self.quarantine(&path)?;
            return Ok(None);
        };
        Ok(Some(record))
    }

    pub(crate) fn write(&self, key: &LocalDraftKey, record: &LocalDraftRecord) -> io::Result<()> {
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
        match fs::remove_file(self.path(key)) {
            Ok(()) => sync_dir(&self.root),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Lossless atomic rename from the canonical cwd key to the bound session key.
    pub(crate) fn rekey(&self, from: &LocalDraftKey, to: &LocalDraftKey) -> io::Result<()> {
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

    fn quarantine(&self, path: &Path) -> io::Result<()> {
        self.ensure_root()?;
        let dir = self.root.join("quarantine");
        fs::create_dir_all(&dir)?;
        set_mode(&dir, 0o700)?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("draft");
        fs::rename(
            path,
            dir.join(format!("{name}.{}.bad", uuid::Uuid::new_v4())),
        )?;
        sync_dir(&self.root)
    }
}

#[derive(Debug)]
struct TrackedDraft {
    record: LocalDraftRecord,
    serialized: Vec<u8>,
    due: Option<Instant>,
    last_deferred: Option<tools::types::BehaviorId>,
}

#[derive(Debug, Default)]
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
}

impl LocalDraftRuntime {
    pub(crate) fn sync(
        &mut self,
        agents: &mut indexmap::IndexMap<AgentId, AgentView>,
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
            if self.tracked.get(&key).is_some_and(|tracked| tracked.due.is_none()) {
                self.tracked.remove(&key);
            }
        }
        let mut recovered_active_pending = false;
        for (id, agent) in agents.iter_mut() {
            let desired = agent
                .session
                .session_id
                .as_ref()
                .and_then(|sid| LocalDraftKey::session(&sid.0).ok())
                .or_else(|| LocalDraftKey::cwd(&agent.session.cwd).ok());
            let Some(key) = desired else {
                tracing::warn!(cwd = %agent.session.cwd.display(), "local draft disabled: cwd cannot be canonicalized");
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
                if let Some(tracked) = self.tracked.get(&key).filter(|tracked| tracked.due.is_some()) {
                    // A failed closing checkpoint owns newer content than disk.
                    restore_agent(agent, &tracked.record);
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
                                serialized: serde_json::to_vec(&fingerprint).unwrap_or_default(),
                                last_deferred: record.deferred_session_mode,
                                record: record.clone(),
                                due: None,
                            },
                        );
                        restore_agent(agent, &record);
                    }
                    Ok(None) => {}
                    Err(error) => tracing::warn!(?error, "failed to load local draft"),
                }
                }
            }
            match capture_agent(agent, 0) {
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
        self.tracked.retain(|key, tracked| live_keys.contains(key) || tracked.due.is_some());
    }

    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.tracked.values().filter_map(|t| t.due).chain(self.invalidations.values().copied()).min()
    }

    /// Transfers every ACP prompt-RPC variant out of the local recovery
    /// domain. The Effect classifier is the sole variant list.
    pub(crate) fn transfer_prompt_rpc_ownership(&mut self, effect: &crate::app::actions::Effect) {
        let Some((agent_id, session_id)) = effect.prompt_rpc_identity() else {
            return;
        };
        // Session binding and the event-loop sync are intentionally decoupled.
        // Capture the runtime's current key before disarming: during that
        // window it can still be the canonical-cwd key even though the Effect
        // already carries the newly bound SessionId.
        let current_key = self.keys.get(&agent_id).cloned();
        let now = Instant::now();
        if let Some(key) = current_key.as_ref() {
            self.invalidate_key(key, now);
        }
        let Ok(session_key) = LocalDraftKey::session(&session_id.0) else {
            return;
        };
        if current_key.as_ref() != Some(&session_key) {
            self.invalidate_key(&session_key, now);
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
            Ok(()) => { self.invalidations.remove(key); }
            Err(error) => {
                self.invalidations.insert(key.clone(), now + WRITE_RETRY_BACKOFF);
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

    fn flush_due(&mut self, now: Instant) {
        let keys = self
            .tracked
            .iter()
            .filter_map(|(key, tracked)| {
                tracked.due.is_some_and(|at| at <= now).then(|| key.clone())
            })
            .collect::<Vec<_>>();
        for key in keys {
            self.flush_key(&key, now);
        }
        let expired = self.invalidations.iter()
            .filter(|(_, due)| **due <= now)
            .map(|(key, _)| key.clone()).collect::<Vec<_>>();
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

fn capture_agent(agent: &AgentView, revision: u64) -> io::Result<LocalDraftRecord> {
    let mut record = LocalDraftRecord::empty(revision);
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

    let mut staged = agent.session.pending_prompts.iter().filter(|prompt| {
        prompt.kind == QueueEntryKind::Prompt
            && prompt.wire_blocks.is_none()
            && !prompt.display_as_skill
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
        record.staged_prompt = Some(DraftPrompt::from_parts(
            &prompt.text,
            prompt.text.len(),
            &prompt.chip_elements,
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

fn restore_agent(agent: &mut AgentView, record: &LocalDraftRecord) {
    if !agent.prompt.text().is_empty() || agent.prompt.has_local_draft_images() {
        tracing::warn!("local draft recovery skipped: live composer is not empty");
        return;
    }
    // A staged prompt never crossed ACP. Put it back in the composer rather
    // than recreating the queue, so recovery cannot send by itself.
    let prompt = record.staged_prompt.as_ref().or(record.composer.as_ref());
    if let Some(prompt) = prompt {
        agent.prompt.set_text(&prompt.text);
        agent.prompt.restore_chip_elements(&prompt.chip_elements());
        agent.prompt.set_cursor(prompt.cursor);
    }
    if agent.session.deferred_session_mode.is_none() {
        agent.session.deferred_session_mode = record.deferred_session_mode;
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
    };
    record.composer.as_ref().is_none_or(valid_prompt)
        && record.staged_prompt.as_ref().is_none_or(valid_prompt)
}

fn count_records(root: &Path) -> io::Result<usize> {
    Ok(fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
        .count())
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
            }),
            staged_prompt: None,
            deferred_session_mode: mode,
        }
    }

    #[test]
    fn failed_rpc_invalidation_survives_binding_close_and_reopen() {
        for structured in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("drafts");
            let mut app = crate::app::root::tests::test_app_with_agent();
            app.local_drafts = LocalDraftRuntime { store: LocalDraftStore::new(root.clone()), ..Default::default() };
            let id = *app.agents.keys().next().unwrap();
            let agent = app.agents.get_mut(&id).unwrap();
            agent.session.session_id = None;
            agent.prompt.set_text("submitted cwd draft");
            let cwd_key = LocalDraftKey::cwd(&agent.session.cwd).unwrap();
            let now = Instant::now();
            app.sync_local_drafts(now);
            app.sync_local_drafts(now + DEBOUNCE);
            let sid = acp_transport::protocol::SessionId::new("invalidation-bind");
            let session_key = LocalDraftKey::session(&sid.0).unwrap();
            app.local_drafts.store.write(&session_key, &record("stale session draft", None)).unwrap();
            let blocked = directory.path().join("blocked");
            fs::write(&blocked, b"not a directory").unwrap();
            app.local_drafts.store = LocalDraftStore::new(blocked);
            let effect = if structured {
                crate::app::actions::Effect::SendPromptBlocks { agent_id: id, session_id: sid.clone(), blocks: vec![], images: vec![], prompt_id: "blocks".into() }
            } else {
                crate::app::actions::Effect::SendPrompt { agent_id: id, session_id: sid.clone(), text: "submitted".into(), prompt_id: "text".into(), skill_token_ranges: vec![] }
            };
            app.transfer_local_draft_ownership(&effect);
            assert_eq!(app.local_drafts.invalidations.len(), 2);
            let deadline = app.local_drafts.next_deadline().unwrap();
            let before = deadline - Duration::from_millis(100);
            app.local_drafts.store = LocalDraftStore::new(root);
            let agent = app.agents.get_mut(&id).unwrap();
            agent.session.session_id = Some(sid);
            agent.prompt.set_text("");
            app.sync_local_drafts(before);
            assert_eq!(app.local_drafts.invalidations.len(), 2);
            assert_eq!(app.local_drafts.store.load(&cwd_key).unwrap().unwrap().composer.unwrap().text, "submitted cwd draft");
            assert_eq!(app.local_drafts.store.load(&session_key).unwrap().unwrap().composer.unwrap().text, "stale session draft");
            let mut closed = app.agents.shift_remove(&id).unwrap();
            app.sync_local_drafts(before);
            closed.prompt.set_text("");
            let reopened = AgentId(9900);
            app.agents.insert(reopened, closed);
            app.active_view = crate::app::root::ActiveView::Agent(reopened);
            app.sync_local_drafts(before);
            assert!(app.agents[&reopened].prompt.text().is_empty());
            assert_eq!(app.local_drafts.next_deadline(), Some(deadline));
            app.sync_local_drafts(deadline);
            assert!(app.local_drafts.invalidations.is_empty());
            assert!(app.local_drafts.store.load(&cwd_key).unwrap().is_none());
            assert!(app.local_drafts.store.load(&session_key).unwrap().is_none());
        }
    }

    #[test]
    fn repeated_capture_failure_keeps_deadline_and_new_draft_cancels_delete() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("drafts");
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime { store: LocalDraftStore::new(root.clone()), ..Default::default() };
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
        app.agents.get_mut(&id).unwrap().prompt.set_text(&"x".repeat(MAX_TEXT_BYTES + 1));
        let failed = now + DEBOUNCE + DEBOUNCE;
        app.sync_local_drafts(failed);
        let deadline = failed + WRITE_RETRY_BACKOFF;
        assert_eq!(app.local_drafts.next_deadline(), Some(deadline));
        app.sync_local_drafts(failed + Duration::from_millis(10));
        assert_eq!(app.local_drafts.next_deadline(), Some(deadline));
        app.local_drafts.store = LocalDraftStore::new(root);
        app.agents.get_mut(&id).unwrap().prompt.set_text("new valid draft");
        app.sync_local_drafts(failed + Duration::from_millis(20));
        assert!(app.local_drafts.invalidations.is_empty());
        app.sync_local_drafts(deadline);
        assert_eq!(app.local_drafts.store.load(&key).unwrap().unwrap().composer.unwrap().text, "new valid draft");
    }

    #[test]
    fn closing_and_reopening_restores_saved_draft_and_releases_clean_state() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime { store: LocalDraftStore::new(directory.path().to_path_buf()), ..Default::default() };
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
        assert_eq!(agent.session.deferred_session_mode, Some(tools::types::BehaviorId::Plan));
        assert!(agent.session.pending_prompts.is_empty());
        assert!(app.local_drafts.store.load(&LocalDraftKey::session(&sid.0).unwrap()).unwrap().is_some());
    }

    #[test]
    fn closing_failed_checkpoint_keeps_latest_and_retry_deadline_on_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("drafts");
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime { store: LocalDraftStore::new(root.clone()), ..Default::default() };
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
        app.agents.get_mut(&id).unwrap().prompt.set_text("latest unsaved");
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
        assert_eq!(app.local_drafts.store.load(&key).unwrap().unwrap().composer.unwrap().text, "old");
        app.sync_local_drafts(retry);
        assert_eq!(app.local_drafts.store.load(&key).unwrap().unwrap().composer.unwrap().text, "latest unsaved");
        app.agents.shift_remove(&reopened);
        app.sync_local_drafts(retry);
        assert!(app.local_drafts.tracked.is_empty());
    }

    #[test]
    fn closing_one_shared_key_owner_keeps_runtime_state() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = crate::app::root::tests::test_app_with_agent();
        app.local_drafts = LocalDraftRuntime { store: LocalDraftStore::new(directory.path().to_path_buf()), ..Default::default() };
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
            assert_eq!(store.write(&key, &invalid).unwrap_err().kind(), io::ErrorKind::InvalidData);
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
        app.local_drafts = LocalDraftRuntime { store, ..Default::default() };
        app.sync_local_drafts(Instant::now());
        let agent = app.agents.values().next().unwrap();
        assert!(agent.prompt.text().is_empty());
        assert!(agent.session.pending_prompts.is_empty());
        assert!(agent.session.deferred_session_mode.is_none());
        let quarantined = fs::read_dir(directory.path().join("quarantine")).unwrap()
            .map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
        assert_eq!(quarantined.len(), 1);
        assert_eq!(fs::read(&quarantined[0]).unwrap(), bytes);
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
        fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&[b' '; 64]).unwrap();
        assert!(store.load_from_reader(&path, &mut reader).unwrap().is_none());
        use std::io::Seek;
        assert_eq!(reader.stream_position().unwrap(), MAX_RECORD_BYTES as u64 + 1);
        assert!(!path.exists());
        assert_eq!(fs::read_dir(directory.path().join("quarantine")).unwrap().count(), 1);
        bytes.push(b' ');
        fs::write(&path, &bytes).unwrap();
        assert!(store.load(&key).unwrap().is_none());
        assert!(!path.exists());
        assert_eq!(fs::read_dir(directory.path().join("quarantine")).unwrap().count(), 2);
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
        fs::write(&target, serde_json::to_vec(&record("linked", None)).unwrap()).unwrap();
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
                assert_eq!(store.load(&key).unwrap_err().kind(), io::ErrorKind::InvalidData);
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

    #[test]
    fn ownership_transfer_clears_cwd_key_before_bind_rekey_sync() {
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
        let now = Instant::now();
        app.sync_local_drafts(now);
        app.sync_local_drafts(now + DEBOUNCE);
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
}

use async_trait::async_trait;
use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::extensions::notification::SessionNotification;
use crate::sampling::ConversationItem;
use crate::session::info::Info;
use crate::session::persistence::Summary;
use crate::session::wire_tags::{
    AVAILABLE_COMMANDS_UPDATE_PREFIX, REWIND_MARKER, USER_MESSAGE_CHUNK,
};
use agent_client_protocol as acp;
use sampling_types::ReasoningEffort;
use workspace::session::file_state::RewindPoint;

pub mod jsonl;
#[allow(dead_code)] // Transaction APIs remain deferred until later protocol wiring.
pub(crate) mod relocation;
pub mod search;
pub mod search_fts;
mod search_recovery;
pub(crate) mod summary_write;

/// On-disk file names, relative to a session directory. Single source of truth for
/// the storage adapter and the session/state and session/import extensions.
pub(crate) const SUMMARY_FILE: &str = "summary.json";
pub(crate) const UPDATES_FILE: &str = "updates.jsonl";
pub(crate) const TIMELINE_FILE: &str = "timeline.jsonl";
pub(crate) const SIDEBANDS_DIR: &str = "sidebands";

pub(crate) type SidebandLedgers = BTreeMap<String, Vec<chat_state::SidebandEvent>>;

fn completed_sideband_result<'a>(
    ledgers: &'a SidebandLedgers,
    sideband_id: &str,
    result_seq: u64,
    owner: &str,
) -> io::Result<&'a chat_state::SidebandResult> {
    let result_index = usize::try_from(result_seq).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{owner} result seq exceeds platform capacity"),
        )
    })?;
    let events = ledgers.get(sideband_id).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{owner} references missing sideband {sideband_id}"),
        )
    })?;
    let result = events
        .get(result_index)
        .and_then(|event| match &event.kind {
            chat_state::SidebandEventKind::Result(result) => Some(result),
            _ => None,
        });
    let terminal_index = result_index.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{owner} result seq exceeds platform capacity"),
        )
    })?;
    let completed = matches!(
        events.get(terminal_index).map(|event| &event.kind),
        Some(chat_state::SidebandEventKind::End(
            chat_state::SidebandEnd {
                outcome: chat_state::SidebandOutcome::Completed,
                error: None,
            }
        ))
    ) && events.len() == terminal_index.saturating_add(1);
    match (result, completed) {
        (Some(result), true) => Ok(result),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{owner} references unproven result {sideband_id}/{result_seq}"),
        )),
    }
}

/// Read every committed record from the canonical Timeline ledger.
///
/// A non-newline-terminated tail was never committed and is ignored. Every
/// complete record is parsed strictly; missing ledgers and interior corruption
/// fail closed for every consumer, including resume, history, and search.
pub(crate) fn read_timeline_file(path: &Path) -> io::Result<Vec<chat_state::TimelineEvent>> {
    read_committed_jsonl_file(path, "mandatory Timeline ledger")
}

/// Read and validate the canonical Timeline for one explicit session
/// directory. This is the only cross-crate disk projection entry point.
pub fn read_timeline_in_session_dir(dir: &Path) -> io::Result<chat_state::Timeline> {
    let events = read_timeline_file(&dir.join(TIMELINE_FILE))?;
    chat_state::Timeline::from_events(events)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Read complete records from an append-only JSONL ledger. A torn final
/// record was never acknowledged and is ignored; all complete records remain
/// strict. Callers choose the ledger-specific missing-file diagnostic.
pub(crate) fn read_committed_jsonl_file<T: serde::de::DeserializeOwned>(
    path: &Path,
    description: &str,
) -> io::Result<Vec<T>> {
    let bytes = std::fs::read(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} is missing: {}", path.display()),
            )
        } else {
            error
        }
    })?;
    let complete_len = if bytes.last().is_none_or(|byte| *byte == b'\n') {
        bytes.len()
    } else {
        bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1)
    };
    bytes[..complete_len]
        .split(|byte| *byte == b'\n')
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(|(index, line)| {
            serde_json::from_slice(line).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}:{}: {error}", path.display(), index + 1),
                )
            })
        })
        .collect()
}

/// Validate every persisted independent ledger against its parent Timeline,
/// including title events whose provenance crosses the ledger boundary.
pub(crate) fn validate_sideband_ledgers(
    parent_timeline_id: &str,
    parent: &chat_state::Timeline,
    ledgers: &SidebandLedgers,
) -> io::Result<()> {
    let spawns = parent
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Sideband(spawn) => {
                Some((spawn.sideband_id.as_str(), (event.seq.get(), spawn)))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    for (sideband_id, events) in ledgers {
        let (spawn_seq, spawn) = spawns.get(sideband_id.as_str()).copied().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("sideband {sideband_id} has no parent spawn fact"),
            )
        })?;
        if events.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("sideband {sideband_id} ledger is empty"),
            ));
        }
        let timeline = chat_state::SidebandTimeline::from_events(events.clone())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        timeline
            .validate_parent(parent_timeline_id, parent, spawn_seq, spawn)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    }

    for (_, spawn) in spawns.values() {
        if let Some(source_ref) = spawn
            .source_refs
            .iter()
            .find(|source_ref| source_ref.timeline_id != parent_timeline_id)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "sideband {} references foreign Timeline {}",
                    spawn.sideband_id, source_ref.timeline_id
                ),
            ));
        }
    }

    for event in parent.events() {
        let chat_state::TimelineEventKind::Compaction(chat_state::CompactionEvent::Summary {
            result_ref,
            summary_chars,
            ..
        }) = &event.kind
        else {
            continue;
        };
        let result = completed_sideband_result(
            ledgers,
            &result_ref.timeline_id,
            result_ref.first_seq,
            "compaction/summary",
        )?;
        if result.raw_output.chars().count() != *summary_chars {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "compaction/summary character count does not match sideband {}/{}",
                    result_ref.timeline_id, result_ref.first_seq
                ),
            ));
        }
    }

    for event in parent.events() {
        let chat_state::TimelineEventKind::SessionTitle(title) = &event.kind else {
            continue;
        };
        match &title.source {
            chat_state::SessionTitleSource::User => {}
            chat_state::SessionTitleSource::Generated {
                sideband_id,
                result_seq,
            } => {
                completed_sideband_result(
                    ledgers,
                    sideband_id,
                    *result_seq,
                    "generated session/title",
                )?;
            }
            chat_state::SessionTitleSource::Fallback {
                sideband_id,
                terminal_seq,
            } => {
                let terminal_index = usize::try_from(*terminal_seq).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "fallback session/title terminal seq exceeds platform capacity",
                    )
                })?;
                let terminal = ledgers
                    .get(sideband_id)
                    .and_then(|events| events.get(terminal_index));
                if !matches!(
                    terminal.map(|event| &event.kind),
                    Some(chat_state::SidebandEventKind::End(
                        chat_state::SidebandEnd {
                            outcome: chat_state::SidebandOutcome::Failed
                                | chat_state::SidebandOutcome::Cancelled,
                            error: Some(_),
                        }
                    ))
                ) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "fallback session/title references unproven terminal event {sideband_id}/{terminal_seq}"
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Write `bytes` to `path` by writing a uniquely named sibling temp file and
/// renaming it over the target, so a crash or a concurrent writer never leaves a
/// torn file. The temp is removed on failure.
pub(crate) fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_bytes_atomic_inner(path, bytes, false)
}

pub(crate) fn write_bytes_atomic_durable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_bytes_atomic_inner(path, bytes, true)
}

fn write_bytes_atomic_inner(path: &Path, bytes: &[u8], durable: bool) -> io::Result<()> {
    let tmp = temp_sibling(path);
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        if durable {
            sync_file_durable(&file)?;
        }
        drop(file);
        replace_file(&tmp, path, durable)?;
        #[cfg(unix)]
        if durable {
            if let Some(parent) = path.parent() {
                std::fs::File::open(parent)?.sync_all()?;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path, _durable: bool) -> io::Result<()> {
    std::fs::rename(source, target)
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path, durable: bool) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows::core::PCWSTR;

    fn extended_path(path: &Path) -> io::Result<Vec<u16>> {
        let path = std::path::absolute(path)?;
        let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if wide.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains NUL",
            ));
        }
        let unc = wide.starts_with(&[b'\\' as u16, b'\\' as u16]);
        let mut result = if unc { r"\\?\UNC\" } else { r"\\?\" }
            .encode_utf16()
            .collect::<Vec<_>>();
        if unc {
            wide.drain(..2);
        }
        result.extend(wide);
        result.push(0);
        Ok(result)
    }

    let source = extended_path(source)?;
    let target = extended_path(target)?;
    let flags = if durable {
        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
    } else {
        MOVEFILE_REPLACE_EXISTING
    };
    unsafe { MoveFileExW(PCWSTR(source.as_ptr()), PCWSTR(target.as_ptr()), flags) }
        .map_err(io::Error::other)
}

#[cfg(target_os = "macos")]
pub(crate) fn sync_file_durable(file: &std::fs::File) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    file.sync_all()?;
    fullfsync_raw(file.as_raw_fd())
}

#[cfg(target_os = "macos")]
pub(crate) fn fullfsync_raw(fd: std::os::fd::RawFd) -> io::Result<()> {
    // macOS fsync may stop at volatile drive caches; F_FULLFSYNC requests stable media.
    if unsafe { libc::fcntl(fd, libc::F_FULLFSYNC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(any(all(unix, not(target_os = "macos")), windows))]
pub(crate) fn sync_file_durable(file: &std::fs::File) -> io::Result<()> {
    file.sync_all()
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn sync_file_durable(_file: &std::fs::File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "durable file sync is unsupported on this platform",
    ))
}

/// Atomically publish `source` at `target` without ever replacing an existing
/// filesystem object.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(crate) fn rename_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    use nix::fcntl::{AT_FDCWD, RenameFlags, renameat2};
    renameat2(
        AT_FDCWD,
        source,
        AT_FDCWD,
        target,
        RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|error| match error {
        nix::errno::Errno::EEXIST => {
            io::Error::new(io::ErrorKind::AlreadyExists, target.display().to_string())
        }
        nix::errno::Errno::EINVAL | nix::errno::Errno::ENOSYS | nix::errno::Errno::EOPNOTSUPP => {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "atomic no-replace rename is unsupported",
            )
        }
        error => io::Error::from(error),
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn rename_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
    let target_c = CString::new(target.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "target path contains NUL"))?;
    // SAFETY: both pointers remain live NUL-terminated path strings for this call.
    if unsafe { libc::renamex_np(source.as_ptr(), target_c.as_ptr(), libc::RENAME_EXCL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EEXIST) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            target_c.to_string_lossy().into_owned(),
        )),
        Some(code) if code == libc::EINVAL || code == libc::ENOTSUP => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "atomic no-replace rename is unsupported",
        )),
        _ => Err(error),
    }
}

#[cfg(windows)]
pub(crate) fn rename_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    // Windows rename already refuses an existing destination.
    std::fs::rename(source, target)
}

#[cfg(not(any(
    all(target_os = "linux", target_env = "gnu"),
    target_os = "macos",
    windows
)))]
pub(crate) fn rename_no_replace(_source: &Path, _target: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace rename is unsupported",
    ))
}

/// Async sibling of [`write_bytes_atomic`].
pub(crate) async fn write_bytes_atomic_async(path: &Path, bytes: Vec<u8>) -> io::Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || write_bytes_atomic(&path, &bytes))
        .await
        .map_err(io::Error::other)?
}

/// Atomically replace a control-plane file and do not acknowledge until both
/// the new file contents and its directory entry have crossed a durability
/// barrier. This is intentionally reserved for state whose caller changes
/// live ownership only after the write returns.
pub(crate) async fn write_bytes_atomic_durable_async(
    path: &Path,
    bytes: Vec<u8>,
) -> io::Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || write_bytes_atomic_inner(&path, &bytes, true))
        .await
        .map_err(io::Error::other)?
}

/// Serialize `items` to newline-delimited JSON bytes.
fn to_jsonl_bytes<T: serde::Serialize>(items: &[T]) -> io::Result<Vec<u8>> {
    let mut content = Vec::new();
    for item in items {
        serde_json::to_writer(&mut content, item)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        content.push(b'\n');
    }
    Ok(content)
}

/// Write `items` as newline-delimited JSON to `path`, atomically (see
/// [`write_bytes_atomic`]).
pub(crate) fn write_jsonl_atomic<T: serde::Serialize>(path: &Path, items: &[T]) -> io::Result<()> {
    write_bytes_atomic(path, &to_jsonl_bytes(items)?)
}

/// Async sibling of [`write_jsonl_atomic`].
pub(crate) async fn write_jsonl_atomic_async<T: serde::Serialize>(
    path: &Path,
    items: &[T],
) -> io::Result<()> {
    write_bytes_atomic_async(path, to_jsonl_bytes(items)?).await
}

/// A unique sibling temp path, e.g. `summary.json` -> `summary.json.<uuid>.tmp`.
fn temp_sibling(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}.tmp", uuid::Uuid::now_v7()));
    PathBuf::from(name)
}

/// Iterator that streams session updates from a JSONL file without loading all into memory.
/// Each call to `next()` reads and parses one line.
pub struct UpdatesIterator {
    reader: BufReader<std::fs::File>,
    line_buffer: String,
}

impl UpdatesIterator {
    /// Create a new iterator over updates in the given file.
    /// Returns None if the file doesn't exist.
    pub fn open(path: &Path) -> io::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let file = std::fs::File::open(path)?;
        Ok(Some(Self {
            reader: BufReader::new(file),
            line_buffer: String::new(),
        }))
    }

    /// Create a new iterator starting at the given byte offset.
    /// Returns None if the file doesn't exist.
    /// Used for delta replay: read only updates appended after a known offset.
    pub fn open_at(path: &Path, offset: u64) -> io::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let file = std::fs::File::open(path)?;
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(offset))?;
        Ok(Some(Self {
            reader,
            line_buffer: String::new(),
        }))
    }

    /// Returns the current byte position in the underlying file.
    /// After iterating, this is the offset of the next unread byte (i.e., EOF
    /// if all updates were consumed). Used to record the replay end offset for
    /// subsequent delta replay.
    pub fn stream_position(&mut self) -> io::Result<u64> {
        self.reader.stream_position()
    }
}

impl Iterator for UpdatesIterator {
    type Item = io::Result<SessionUpdate>;

    fn next(&mut self) -> Option<Self::Item> {
        self.line_buffer.clear();
        match self.reader.read_line(&mut self.line_buffer) {
            Ok(0) => None, // EOF
            Ok(_) => {
                let line = self.line_buffer.trim();
                if line.is_empty() {
                    return self.next();
                }
                match SessionUpdateEnvelope::from_str(line) {
                    Ok(update) => Some(Ok(update)),
                    Err(e) => Some(Err(io::Error::new(io::ErrorKind::InvalidData, e))),
                }
            }
            Err(e) => Some(Err(e)),
        }
    }
}

/// Method name for standard ACP session/update notifications.
const ACP_SESSION_UPDATE_METHOD: &str = "session/update";

/// Method name for Grow extension session/update notifications.
pub(crate) const GROW_SESSION_UPDATE_METHOD: &str = "_grow/session/update";

/// A unified session update that can be either an ACP notification or a Grow extension notification.
/// This allows storing all session updates in chronological order.
///
/// Note: The `Serialize` implementation produces a format without timestamp.
/// For local JSONL storage with timestamps, use `SessionUpdateEnvelope`.
#[derive(Debug, Clone)]
pub enum SessionUpdate {
    /// Standard ACP session/update notification (boxed due to large size)
    Acp(Box<acp::SessionNotification>),
    /// Grow extension session notification (e.g., diff_review)
    Grow(Box<SessionNotification>),
}

impl serde::Serialize for SessionUpdate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(2))?;
        match self {
            SessionUpdate::Acp(notification) => {
                map.serialize_entry("method", ACP_SESSION_UPDATE_METHOD)?;
                map.serialize_entry("params", notification)?;
            }
            SessionUpdate::Grow(notification) => {
                map.serialize_entry("method", GROW_SESSION_UPDATE_METHOD)?;
                map.serialize_entry("params", notification)?;
            }
        }
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for SessionUpdate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // `updates.jsonl` and wire consumers share one method/params envelope.
        let value = serde_json::Value::deserialize(deserializer)?;
        SessionUpdateEnvelope::from_value(value).map_err(serde::de::Error::custom)
    }
}

/// The serialized envelope for a session update, including metadata for debugging.
/// This is the typed structure that gets written to updates.jsonl (disk storage only).
///
/// Note: This is separate from `SessionUpdate`'s own serialization to avoid affecting
/// other consumers (e.g., network listeners) who don't need the timestamp metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SessionUpdateEnvelope {
    /// Unix timestamp (seconds since epoch) when this update was written.
    /// Useful for debugging timing issues in the updates.jsonl file.
    #[serde(default)]
    pub timestamp: u64,
    /// The method name identifying the update type.
    /// Either "session/update" for ACP or "_grow/session/update" for Grow extensions.
    pub method: String,
    /// The actual notification payload.
    pub params: serde_json::Value,
}

impl SessionUpdateEnvelope {
    /// Create a new envelope with the current timestamp for disk storage.
    pub(crate) fn from_update(update: &SessionUpdate) -> Result<Self, serde_json::Error> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        match update {
            SessionUpdate::Acp(notification) => Ok(Self {
                timestamp,
                method: ACP_SESSION_UPDATE_METHOD.to_string(),
                params: serde_json::to_value(notification)?,
            }),
            SessionUpdate::Grow(notification) => Ok(Self {
                timestamp,
                method: GROW_SESSION_UPDATE_METHOD.to_string(),
                params: serde_json::to_value(notification)?,
            }),
        }
    }

    /// Convert this envelope back into a SessionUpdate.
    pub(crate) fn into_update(self) -> Result<SessionUpdate, serde_json::Error> {
        match self.method.as_str() {
            GROW_SESSION_UPDATE_METHOD => {
                let notification: SessionNotification = serde_json::from_value(self.params)?;
                Ok(SessionUpdate::Grow(Box::new(notification)))
            }
            ACP_SESSION_UPDATE_METHOD => {
                let notification: acp::SessionNotification = serde_json::from_value(self.params)?;
                Ok(SessionUpdate::Acp(Box::new(notification)))
            }
            method => Err(invalid_update_envelope(format!(
                "unsupported session update method {method:?}"
            ))),
        }
    }

    /// Parse the canonical method/params envelope from a JSON value.
    pub(crate) fn from_value(value: serde_json::Value) -> Result<SessionUpdate, serde_json::Error> {
        let envelope: SessionUpdateEnvelope = serde_json::from_value(value)?;
        envelope.into_update()
    }

    /// Parse a session update directly from a JSON string, avoiding intermediate `Value` allocation.
    ///
    /// Uses a borrowing envelope with `&RawValue` for the params field so the JSON bytes
    /// for the notification payload are only parsed once (directly to the typed struct)
    /// instead of twice (str -> Value -> typed).
    pub(crate) fn from_str(line: &str) -> Result<SessionUpdate, serde_json::Error> {
        #[derive(serde::Deserialize)]
        struct BorrowedEnvelope<'a> {
            method: &'a str,
            #[serde(borrow)]
            params: &'a serde_json::value::RawValue,
        }

        let envelope = serde_json::from_str::<BorrowedEnvelope<'_>>(line)?;
        let raw_params = envelope.params.get();
        match envelope.method {
            GROW_SESSION_UPDATE_METHOD => {
                let notification: SessionNotification = serde_json::from_str(raw_params)?;
                Ok(SessionUpdate::Grow(Box::new(notification)))
            }
            ACP_SESSION_UPDATE_METHOD => {
                let notification: acp::SessionNotification = serde_json::from_str(raw_params)?;
                Ok(SessionUpdate::Acp(Box::new(notification)))
            }
            method => Err(invalid_update_envelope(format!(
                "unsupported session update method {method:?}"
            ))),
        }
    }
}

fn invalid_update_envelope(message: String) -> serde_json::Error {
    serde_json::Error::io(io::Error::new(io::ErrorKind::InvalidData, message))
}

/// All persisted data for a session
#[derive(Debug, Clone)]
pub struct PersistedData {
    pub summary: Summary,
    /// Immutable conversation facts. This is the restart source of truth.
    pub timeline_events: Vec<chat_state::TimelineEvent>,
    /// All session updates (ACP updates and Grow extension updates) in chronological order
    pub updates: Vec<SessionUpdate>,
    /// Latest Behavior/Goal projection folded from Timeline `Control` events.
    pub control_snapshot: Option<crate::session::control::SessionControlSnapshot>,
    /// Rewind points for session rewind functionality
    pub rewind_points: Vec<RewindPoint>,
    /// Latest session-signals projection folded from Timeline observations.
    pub signals: Option<crate::session::signals::SessionSignals>,
    /// Latest announcement projection folded from Timeline observations.
    pub announcement_state: Option<crate::session::announcement_state::AnnouncementState>,
    pub workflow_runs: Vec<crate::session::workflow::store::RestoredWorkflowRun>,
}

/// Persisted data WITHOUT updates - for memory-efficient session loading
#[derive(Debug, Clone)]
pub struct PersistedDataLight {
    pub summary: Summary,
    pub timeline_events: Vec<chat_state::TimelineEvent>,
    pub control_snapshot: Option<crate::session::control::SessionControlSnapshot>,
    // No `rewind_points` field: the resume path defers them (loaded lazily by
    // `FileStateTracker`). Use `load_session` for the eager set.
    /// Latest session-signals projection folded from Timeline observations.
    pub signals: Option<crate::session::signals::SessionSignals>,
    /// Latest announcement projection folded from Timeline observations.
    pub announcement_state: Option<crate::session::announcement_state::AnnouncementState>,
    pub workflow_runs: Vec<crate::session::workflow::store::RestoredWorkflowRun>,
}

/// Result of copying session data
#[derive(Debug, Clone)]
pub struct CopySessionResult {
    pub surface_items_copied: usize,
    pub updates_copied: usize,
    /// Whether a sanitized control event was seeded into the child Timeline.
    pub control_event_seeded: bool,
    /// Number of immutable large-prompt blobs referenced by the selected
    /// Surface and copied into the child lineage.
    pub prompt_blobs_copied: usize,
}

/// Options for copying session data during fork
#[derive(Debug, Clone)]
pub struct CopySessionOptions {
    /// Parent session ID to set in the forked session's summary.
    pub parent_session_id: Option<String>,
    /// Model ID override for the forked session (None = keep source model).
    pub new_model_id: Option<String>,
    /// Truncate copied history to this prompt index (0-based, inclusive).
    pub target_prompt_index: Option<usize>,
    /// When true, skip `transform_conversation_cwd` during copy.
    ///
    /// Set for forks where the child should see the original project path
    /// (e.g. worktree forks with a persisted `display_cwd`). Non-worktree
    /// forks should keep this false so conversation paths are rewritten to
    /// the new cwd.
    pub skip_cwd_transform: bool,
    /// Stable display path for fork sessions. Persisted in the forked
    /// summary so the prompt-facing cwd survives session restore/reload.
    pub prompt_display_cwd: Option<String>,

    // ── Generic fork extensions (used by subagent + worktree forks) ──
    /// Override `session_kind` in the forked summary. Defaults to `"fork"`.
    /// Subagent resume sets `"subagent_resume"`.
    pub session_kind: Option<String>,
    /// How the fork's initial context was bootstrapped: `"new"` or `"forked"`.
    pub fork_context_source: Option<String>,
    /// Parent prompt/turn ID that triggered this fork.
    pub fork_parent_prompt_id: Option<String>,
    /// Whether to seed sanitized parent control into the child Timeline.
    pub inherit_control: bool,
    /// When true, apply fork-safety filtering to the copied Surface:
    /// - Strip synthetic user messages (doom loop warnings, compaction metadata)
    /// - Truncate at the last complete turn boundary
    /// - Remove trailing incomplete assistant responses
    pub fork_filter: bool,
    /// When true, strip `reasoning` (thinking/reasoning_content) from all
    /// assistant messages in the copied Surface.
    ///
    /// Set for forks so that the new session does not inherit the prior
    /// model's chain-of-thought -- each fork starts with a clean slate
    /// for reasoning on the new prompt.
    pub strip_reasoning: bool,
    /// The original workspace directory this worktree session was spawned from.
    /// Propagated to the forked session's `Summary::source_workspace_dir`.
    pub source_workspace_dir: Option<String>,
}

impl Default for CopySessionOptions {
    fn default() -> Self {
        Self {
            parent_session_id: None,
            new_model_id: None,
            target_prompt_index: None,
            skip_cwd_transform: false,
            prompt_display_cwd: None,
            session_kind: None,
            fork_context_source: None,
            fork_parent_prompt_id: None,
            inherit_control: true,
            fork_filter: false,
            strip_reasoning: false,
            source_workspace_dir: None,
        }
    }
}

/// Chunk `_meta.promptIndex` on an ACP `UserMessageChunk`, if present.
fn acp_user_chunk_prompt_index(update: &SessionUpdate) -> Option<usize> {
    let SessionUpdate::Acp(n) = update else {
        return None;
    };
    let acp::SessionUpdate::UserMessageChunk(chunk) = &n.update else {
        return None;
    };
    chunk
        .meta
        .as_ref()
        .and_then(|m| m.get("promptIndex"))
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
}

pub(crate) const HOST_TURN_META_KEY: &str = "hostTurn";

pub(crate) fn is_host_turn_chunk(chunk: &acp::ContentChunk) -> bool {
    chunk
        .meta
        .as_ref()
        .and_then(|m| m.get(HOST_TURN_META_KEY))
        .and_then(|v| v.as_bool())
        == Some(true)
}

fn is_host_turn_update(update: &SessionUpdate) -> bool {
    let SessionUpdate::Acp(n) = update else {
        return false;
    };
    let acp::SessionUpdate::UserMessageChunk(chunk) = &n.update else {
        return false;
    };
    is_host_turn_chunk(chunk)
}

fn is_acp_user_message_chunk(update: &SessionUpdate) -> bool {
    matches!(
        update,
        SessionUpdate::Acp(n) if matches!(n.update, acp::SessionUpdate::UserMessageChunk(_))
    )
}

/// Tracks user-message runs for turn counting (updates truncate / filter_rewind).
///
/// Progressive: every user run counts until the first `promptIndex` appears;
/// after that only marked runs count (mid-turn phantoms omit the marker).
/// A change of `promptIndex` (including unmarked ↔ marked) opens a new run —
/// matching replay's split so back-to-back cancelled prompts stay distinct.
struct UserRunTurnTracker {
    seen_marker: bool,
    in_user: bool,
    /// `promptIndex` of the current user run (`None` = unmarked / phantom run).
    current_run_pi: Option<usize>,
}

impl UserRunTurnTracker {
    fn new() -> Self {
        Self {
            seen_marker: false,
            in_user: false,
            current_run_pi: None,
        }
    }

    /// Returns true if this user chunk opens a **counted** turn.
    fn on_user_chunk(&mut self, prompt_index: Option<usize>) -> bool {
        if prompt_index.is_some() {
            self.seen_marker = true;
        }
        let counts = if self.seen_marker {
            prompt_index.is_some()
        } else {
            true
        };
        let new_run = if !self.in_user {
            true
        } else if self.seen_marker || prompt_index.is_some() {
            prompt_index != self.current_run_pi
        } else {
            false
        };
        if new_run {
            self.current_run_pi = prompt_index;
            self.in_user = true;
            counts
        } else {
            self.in_user = true;
            false
        }
    }

    fn on_non_user(&mut self) {
        self.in_user = false;
        self.current_run_pi = None;
    }
}

/// Calculate how many updates to keep for a given target prompt index (0-based, inclusive).
///
/// Progressive: unmarked user runs before the first `_meta.promptIndex` count
/// as turns; after the first marker only marked runs count (phantoms omit it).
pub fn updates_truncate_for_prompt(updates: &[SessionUpdate], target_prompt_index: usize) -> usize {
    let mut user_turn_count = 0;
    let mut tracker = UserRunTurnTracker::new();

    for (i, update) in updates.iter().enumerate() {
        if is_acp_user_message_chunk(update) && !is_host_turn_update(update) {
            if tracker.on_user_chunk(acp_user_chunk_prompt_index(update)) {
                user_turn_count += 1;
                if user_turn_count > target_prompt_index + 1 {
                    return i;
                }
            }
        } else {
            tracker.on_non_user();
        }
    }

    updates.len()
}

#[derive(Debug)]
pub enum AppendUpdateError {
    NotCommitted(io::Error),
    Committed(io::Error),
}

impl AppendUpdateError {
    pub fn into_io_error(self) -> io::Error {
        match self {
            Self::NotCommitted(error) | Self::Committed(error) => error,
        }
    }
}

impl std::fmt::Display for AppendUpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotCommitted(error) | Self::Committed(error) => error.fmt(formatter),
        }
    }
}

/// Storage adapter trait for session persistence
/// Abstracts over different storage backends (JSONL, SQLite, etc.)
#[async_trait]
pub trait StorageAdapter: Send + Sync {
    /// Initialize a new session or load existing one
    /// Returns the Summary (creates if needed, loads if exists)
    async fn init_session(&self, info: &Info, model_id: acp::ModelId) -> io::Result<Summary>;

    /// Repair the denormalized title cache from an already-validated canonical
    /// Timeline fold. Ordinary writers never call this path. Returns false for
    /// a stale/idempotent sequence.
    async fn repair_session_title_projection(
        &self,
        info: &Info,
        event_seq: u64,
        title: String,
        source: chat_state::SessionTitleSource,
    ) -> io::Result<bool>;

    /// Append a session update (ACP update or Grow extension update) and increment counter
    async fn append_update(&self, info: &Info, update: &SessionUpdate) -> io::Result<()>;

    /// Append one update and report whether the replay record was committed before an error.
    async fn append_update_commit_aware(
        &self,
        info: &Info,
        update: &SessionUpdate,
    ) -> Result<(), AppendUpdateError> {
        self.append_update(info, update)
            .await
            .map_err(AppendUpdateError::NotCommitted)
    }

    /// Append one update durably, preserving whether the replay record committed before failure.
    async fn append_update_durable_commit_aware(
        &self,
        _info: &Info,
        _update: &SessionUpdate,
    ) -> Result<(), AppendUpdateError> {
        Err(AppendUpdateError::NotCommitted(io::Error::new(
            io::ErrorKind::Unsupported,
            "durable session update append is unsupported",
        )))
    }

    /// Append one immutable conversation event without rewriting prior facts.
    async fn append_timeline_event(
        &self,
        info: &Info,
        event: &chat_state::TimelineEvent,
    ) -> io::Result<()>;

    /// Append one timeline boundary and sync it before returning.
    async fn append_timeline_event_durable(
        &self,
        _info: &Info,
        _event: &chat_state::TimelineEvent,
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "durable timeline append is unsupported",
        ))
    }

    /// Durably append one event to a short-lived sideband's independent
    /// Timeline. Implementations must preserve contiguous sequence identity.
    async fn append_sideband_event_durable(
        &self,
        _info: &Info,
        _event: &chat_state::SidebandEvent,
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "durable sideband append is unsupported",
        ))
    }

    /// Update the current model and agent name in summary.
    /// `agent_name` is the resolved agent definition name
    /// persisted so session resume doesn't depend on the mutable model catalog.
    /// `None` preserves the corresponding persisted field.
    async fn update_current_model_and_agent(
        &self,
        info: &Info,
        model_id: &acp::ModelId,
        agent_name: Option<&str>,
        reasoning_effort: Option<Option<ReasoningEffort>>,
    ) -> io::Result<()>;

    /// Update the persisted HEAD commit and branch in summary
    async fn update_git_head(
        &self,
        info: &Info,
        commit: Option<String>,
        branch: Option<String>,
    ) -> io::Result<()>;

    async fn write_workflow_run_state(
        &self,
        info: &Info,
        manifest: &crate::session::workflow::store::WorkflowRunManifest,
    ) -> io::Result<()>;

    async fn delete_workflow_run_state(&self, info: &Info, run_id: &str) -> io::Result<()>;

    /// Load all persisted data for a session
    async fn load_session(&self, info: &Info) -> io::Result<PersistedData>;

    /// Load session data WITHOUT updates (for memory efficiency when updates
    /// will be streamed). Implementations also do NOT read rewind points here;
    /// those are deferred and lazily loaded on demand from the path returned by
    /// [`rewind_points_file_path`](StorageAdapter::rewind_points_file_path).
    async fn load_session_without_updates(&self, info: &Info) -> io::Result<PersistedDataLight>;

    /// Loads the summary of the session
    async fn load_summary(&self, info: &Info) -> io::Result<Summary>;

    /// List session summaries, optionally filtered by current working directory.
    /// When `cwd` is `None`, returns summaries for all sessions.
    async fn list_sessions(&self, cwd: Option<&str>) -> io::Result<Vec<Summary>>;

    /// Permanently delete a session's stored data (all files for the
    /// session). Implementations must treat a missing session as success
    /// (idempotent delete).
    async fn delete_session(&self, info: &Info) -> io::Result<()>;

    /// Append a rewind point for session rewind functionality
    async fn append_rewind_point(&self, info: &Info, point: &RewindPoint) -> io::Result<()>;

    /// Load all rewind points for a session
    async fn load_rewind_points(&self, info: &Info) -> io::Result<Vec<RewindPoint>>;

    /// Truncate rewind points from a specific prompt index (inclusive)
    /// Used when rewinding to remove future history
    async fn truncate_rewind_points_from(&self, info: &Info, from_index: usize) -> io::Result<()>;

    /// Merge rewind points at indices `>= target_index` into the point at
    /// `target_index - 1` and drop the folded points, as a read-modify-write on
    /// disk (used after a ConversationOnly rewind). Reading the current on-disk
    /// set makes this authoritative: it never relies on a (possibly partially
    /// loaded) in-memory tracker, so historical points can't be lost.
    async fn merge_rewind_points_from(&self, info: &Info, target_index: usize) -> io::Result<()>;

    /// Copy session data from source to target, transforming session IDs
    /// The `options` parameter allows setting parent session tracking and model overrides.
    async fn copy_session_data(
        &self,
        source_info: &Info,
        target_info: &Info,
        options: CopySessionOptions,
    ) -> io::Result<CopySessionResult>;

    /// Load the current branch's typed user-authored inputs from Timeline.
    async fn load_prompt_records(&self, info: &Info) -> io::Result<Vec<chat_state::PromptRecord>>;

    /// Get the path to the canonical Timeline ledger for bounded background
    /// reads. Returns None when the backend cannot expose a local path.
    fn timeline_file_path(&self, info: &Info) -> Option<std::path::PathBuf>;

    /// Get the path to the updates file for streaming reads.
    /// Returns None if the storage backend doesn't support streaming.
    fn updates_file_path(&self, info: &Info) -> Option<std::path::PathBuf>;

    /// Path to the rewind-points file for lazy/deferred loading, or None if the
    /// backend doesn't persist them to a streamable file. The adapter owns the
    /// on-disk layout, so callers must use this rather than recomputing the path
    /// (it differs for non-default storage modes, e.g. subagent/fork sessions).
    fn rewind_points_file_path(&self, info: &Info) -> Option<std::path::PathBuf>;
}

pub use jsonl::JsonlStorageAdapter;

/// Extracts `method` and raw `params` from an updates.jsonl envelope
/// without parsing the notification payload.
#[derive(serde::Deserialize)]
pub(crate) struct RawLinePeek<'a> {
    pub method: &'a str,
    #[serde(borrow)]
    pub params: &'a serde_json::value::RawValue,
}

/// Peeks at `update.sessionUpdate` tag and `_meta` without full deserialization.
#[derive(serde::Deserialize)]
pub(crate) struct RawParamsPeek<'a> {
    #[serde(borrow, default)]
    pub update: Option<RawUpdatePeek<'a>>,
    #[serde(borrow, default, rename = "_meta")]
    pub meta: Option<&'a serde_json::value::RawValue>,
}

#[derive(serde::Deserialize)]
pub(crate) struct RawUpdatePeek<'a> {
    #[serde(rename = "sessionUpdate")]
    pub session_update: &'a str,
    #[serde(default)]
    pub target_prompt_index: Option<usize>,
    /// Chunk `_meta.promptIndex` when present (owned; not borrowed).
    #[serde(default, rename = "_meta")]
    pub meta: Option<RawChunkMetaPeek>,
}

#[derive(serde::Deserialize)]
pub(crate) struct RawChunkMetaPeek {
    #[serde(default, rename = "promptIndex")]
    pub prompt_index: Option<u64>,
    #[serde(default, rename = "hostTurn")]
    pub host_turn: Option<bool>,
}

/// Role of one item in the rewind timeline, as seen by [`filter_rewind_by`].
enum RewindStep {
    /// Rewind marker: truncate survivors back to `target`'s prompt boundary.
    Rewind { target: usize },
    /// User-message chunk opening (or continuing) a prompt run.
    UserChunk { prompt_index: Option<usize> },
    /// Anything else: kept, but ends the current user run.
    Other,
}

/// Shared rewind dead-branch filter. `classify` maps each item to its
/// [`RewindStep`]; the driver tracks prompt boundaries and, on a marker,
/// truncates survivors back to the target prompt. [`filter_rewind_lines`] and
/// [`filter_rewind_updates`] wrap this over raw JSONL and typed updates so the
/// two paths share one algorithm.
fn filter_rewind_by<T>(items: Vec<T>, classify: impl Fn(&T) -> RewindStep) -> Vec<T> {
    let mut result: Vec<T> = Vec::with_capacity(items.len());
    let mut prompt_starts: Vec<usize> = Vec::new();
    let mut tracker = UserRunTurnTracker::new();

    for item in items {
        match classify(&item) {
            RewindStep::Rewind { target } => {
                // Out-of-range target keeps every survivor: fold to `result.len()`.
                let trunc = prompt_starts.get(target).copied().unwrap_or(result.len());
                result.truncate(trunc);
                prompt_starts.truncate(target);
                tracker.on_non_user();
                continue;
            }
            RewindStep::UserChunk { prompt_index } => {
                if tracker.on_user_chunk(prompt_index) {
                    prompt_starts.push(result.len());
                }
            }
            RewindStep::Other => tracker.on_non_user(),
        }
        result.push(item);
    }
    result
}

/// Classify a raw JSONL line by peeking at its tag and `_meta` without fully
/// deserializing the payload.
fn rewind_step_for_line(line: &str) -> RewindStep {
    let Ok(env) = serde_json::from_str::<RawLinePeek<'_>>(line) else {
        return RewindStep::Other;
    };
    let is_grow = match env.method {
        GROW_SESSION_UPDATE_METHOD => true,
        ACP_SESSION_UPDATE_METHOD => false,
        _ => return RewindStep::Other,
    };

    let Some(u) = serde_json::from_str::<RawParamsPeek<'_>>(env.params.get())
        .ok()
        .and_then(|p| p.update)
    else {
        return RewindStep::Other;
    };

    if is_grow
        && u.session_update == *REWIND_MARKER
        && let Some(target) = u.target_prompt_index
    {
        return RewindStep::Rewind { target };
    }

    let is_host_turn = u.meta.as_ref().and_then(|m| m.host_turn).unwrap_or(false);
    if !is_grow && !is_host_turn && u.session_update == *USER_MESSAGE_CHUNK {
        let prompt_index = u
            .meta
            .as_ref()
            .and_then(|m| m.prompt_index.map(|v| v as usize));
        return RewindStep::UserChunk { prompt_index };
    }

    RewindStep::Other
}

/// Classify a typed `SessionUpdate`.
fn rewind_step_for_update(update: &SessionUpdate) -> RewindStep {
    if let SessionUpdate::Grow(n) = update
        && let crate::extensions::notification::SessionUpdate::RewindMarker {
            target_prompt_index,
            ..
        } = &n.update
    {
        return RewindStep::Rewind {
            target: *target_prompt_index,
        };
    }
    if is_acp_user_message_chunk(update) && !is_host_turn_update(update) {
        return RewindStep::UserChunk {
            prompt_index: acp_user_chunk_prompt_index(update),
        };
    }
    RewindStep::Other
}

/// Filter rewind dead branches from raw JSONL lines.
///
/// Canonical raw-line rewind filter used by the initial and delta replay paths.
/// Skips parsing entirely when no rewind markers are present.
pub(crate) fn filter_rewind_lines(lines: Vec<&str>) -> Vec<&str> {
    if !lines.iter().any(|l| l.contains(&*REWIND_MARKER)) {
        return lines;
    }
    filter_rewind_by(lines, |line| rewind_step_for_line(line))
}

/// Filter rewind dead branches from typed `SessionUpdate` values.
///
/// Typed equivalent of [`filter_rewind_lines`] over the same
/// [`filter_rewind_by`] driver, operating on fully-deserialized updates.
pub fn filter_rewind_updates(updates: Vec<SessionUpdate>) -> Vec<SessionUpdate> {
    let has_rewinds = updates.iter().any(|u| {
        matches!(
            u,
            SessionUpdate::Grow(n) if matches!(
                n.update,
                crate::extensions::notification::SessionUpdate::RewindMarker { .. }
            )
        )
    });
    if !has_rewinds {
        return updates;
    }
    filter_rewind_by(updates, rewind_step_for_update)
}

/// Strip `<fork-context>` and `<resume-context>` XML wrappers from user
/// message chunks so replayed/exported prompts show clean text.
///
/// Only modifies `UserMessageChunk` text content; all other update types
/// pass through unchanged. The tags are injected by the subagent fork/resume
/// logic in `subagent.rs`.
pub fn strip_context_wrappers(update: acp::SessionUpdate) -> acp::SessionUpdate {
    let acp::SessionUpdate::UserMessageChunk(mut chunk) = update else {
        return update;
    };
    if let acp::ContentBlock::Text(ref mut t) = chunk.content {
        for tag in &["fork-context", "resume-context"] {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            if let Some(start) = t.text.find(&open)
                && let Some(rel_end) = t.text[start + open.len()..].find(&close)
            {
                let end = start + open.len() + rel_end;
                let remove_end = end + close.len();
                t.text = format!("{}{}", &t.text[..start], t.text[remove_end..].trim_start());
            }
        }
    }
    acp::SessionUpdate::UserMessageChunk(chunk)
}

// Replay-loader family, all resolving through `replay_updates_path_in_dir` and
// reading through `for_each_replay_update_in_file`. Pick by need:
//   - production, current grow home:   `load_updates_for_replay`
//   - production, streaming (bounded): `stream_replay_updates_at`
//   - tests, explicit grow home:       `load_updates_for_replay_at` (typed reference)

/// Load replay-ready typed ACP updates for a session, or `None` when the
/// session or its `updates.jsonl` is missing.
pub fn load_updates_for_replay(
    session_id: &str,
) -> std::io::Result<Option<Vec<acp::SessionUpdate>>> {
    let Some(session_dir) =
        crate::session::persistence::find_persisted_session_dir_by_id_result(session_id)?
    else {
        return Ok(None);
    };
    let Some(updates_path) = replay_updates_path_in_dir(&session_dir) else {
        return Ok(None);
    };
    Ok(Some(collect_replay_updates(&updates_path)?))
}

/// Like [`load_updates_for_replay`], but resolves the session under a specific
/// grow home. Typed, materialize-all replay reader: collects every update into
/// owned `Vec`s. Production forwards replay through [`stream_replay_updates_at`]
/// to bound peak memory, so this has no production caller and is compiled only
/// for tests: the `testkit_synth_roundtrip` and `session_load_perf` parity
/// references and the in-crate relocation tests.
#[cfg(any(test, feature = "test-support"))]
pub fn load_updates_for_replay_at(
    session_id: &str,
    grow_home: &std::path::Path,
) -> std::io::Result<Option<Vec<acp::SessionUpdate>>> {
    let Some(updates_path) = resolve_replay_updates_path(session_id, grow_home)? else {
        return Ok(None);
    };
    Ok(Some(collect_replay_updates(&updates_path)?))
}

/// The session dir's `updates.jsonl` path if it exists, else `None`. Sole owner
/// of the "does this dir have a replayable updates file" gate.
fn replay_updates_path_in_dir(session_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let updates_path = session_dir.join(UPDATES_FILE);
    updates_path.exists().then_some(updates_path)
}

/// Collect every replay-ready ACP update from `updates_path` into a `Vec`, the
/// materializing counterpart of the streaming [`for_each_replay_update_in_file`].
fn collect_replay_updates(
    updates_path: &std::path::Path,
) -> std::io::Result<Vec<acp::SessionUpdate>> {
    let mut acp_updates: Vec<acp::SessionUpdate> = Vec::new();
    for_each_replay_update_in_file(updates_path, |u| acp_updates.push(u))?;
    Ok(acp_updates)
}

/// Resolve `updates.jsonl` for `session_id` under `grow_home`, or `None` when
/// the session directory or the file is missing. Shared by the typed
/// `load_updates_for_replay_at` and the streaming [`stream_replay_updates_at`].
fn resolve_replay_updates_path(
    session_id: &str,
    grow_home: &std::path::Path,
) -> std::io::Result<Option<std::path::PathBuf>> {
    let sessions_root = grow_home.join("sessions");
    let Some(session_dir) =
        crate::session::persistence::find_persisted_session_dir_by_id_in_root_result(
            session_id,
            &sessions_root,
        )?
    else {
        return Ok(None);
    };
    Ok(replay_updates_path_in_dir(&session_dir))
}

/// Whether a replay stream forwarded any update. Gates the caller's
/// post-replay memory purge: `Empty` means nothing was reclaimable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum ReplayEmission {
    Emitted,
    Empty,
}

/// Invoke `f` once per replay-ready ACP update for a session under `grow_home`,
/// never building the full typed `Vec`. Reads the session's JSONL transcript
/// directly; a non-JSONL backend would need its own bounded replay.
///
/// Forking or resuming replays the inherited transcript. The typed load parsed
/// the whole file and copied it several times, so a large session briefly held
/// several times its size in live heap and a per-user memory cgroup OOM-killed
/// it. Streaming holds one typed update at a time, so peak drops to about the
/// file size.
///
/// `Empty` folds the missing-session, missing-file, and no-ACP-updates cases;
/// the typed `load_updates_for_replay_at` keeps them distinct (`Ok(None)` vs
/// `Ok(Some(vec![]))`) since it returns the parsed contents rather than a purge
/// signal.
///
/// The sink is infallible by design: replay only rehydrates UI scrollback, a
/// best-effort step, so failing to apply one update must neither abort the
/// stream nor surface an error. I/O errors from reading the file still
/// propagate via the `Result`.
pub fn stream_replay_updates_at<F: FnMut(acp::SessionUpdate)>(
    session_id: &str,
    grow_home: &std::path::Path,
    f: F,
) -> std::io::Result<ReplayEmission> {
    let Some(updates_path) = resolve_replay_updates_path(session_id, grow_home)? else {
        return Ok(ReplayEmission::Empty);
    };
    Ok(if for_each_replay_update_in_file(&updates_path, f)? {
        ReplayEmission::Emitted
    } else {
        ReplayEmission::Empty
    })
}

/// Stream durable Grow extension notifications from one already-resolved
/// session directory. This is separate from conversation replay because the
/// pager normally applies only ACP chunks to a child view; reconnect
/// reconstruction additionally needs nested subagent lifecycle records to
/// rebuild descendant routing before pending interactions are replayed.
///
/// The full notification is retained deliberately: its `_meta.eventId` is the
/// source-session dedup identity shared by the persisted record and a live
/// event buffered during an ancestor `session/load`.
pub fn stream_replay_grow_notifications_in_dir<
    F: FnMut(crate::extensions::notification::SessionNotification),
>(
    session_dir: &std::path::Path,
    mut f: F,
) -> std::io::Result<ReplayEmission> {
    let Some(updates_path) = replay_updates_path_in_dir(session_dir) else {
        return Ok(ReplayEmission::Empty);
    };
    let raw_contents = std::fs::read_to_string(updates_path)?;
    let live = filter_rewind_lines(
        raw_contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect(),
    );
    let mut emitted = false;
    for line in live {
        match SessionUpdateEnvelope::from_str(line) {
            Ok(SessionUpdate::Grow(notification)) => {
                emitted = true;
                f(*notification);
            }
            Ok(SessionUpdate::Acp(_)) => {}
            Err(error) => {
                tracing::debug!(?error, "skipping unparseable Grow replay line");
            }
        }
    }
    Ok(if emitted {
        ReplayEmission::Emitted
    } else {
        ReplayEmission::Empty
    })
}

// Rewind can drop earlier lines, so surviving lines are held until the end of
// the file; one `String` plus `&str` slices keeps that minimal. Output matches
// the typed load. Returns whether any ACP update was forwarded.
fn for_each_replay_update_in_file<F: FnMut(acp::SessionUpdate)>(
    updates_path: &std::path::Path,
    mut f: F,
) -> std::io::Result<bool> {
    // Whole-file read is bounded by file size; only the forwarding is streamed.
    let raw_contents = std::fs::read_to_string(updates_path)?;
    let live: Vec<&str> = filter_rewind_lines(
        raw_contents
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect(),
    );
    let mut forwarded = false;
    for line in live {
        match SessionUpdateEnvelope::from_str(line) {
            // Only ACP updates replay.
            Ok(SessionUpdate::Acp(notif)) => {
                forwarded = true;
                f(strip_context_wrappers(notif.update));
            }
            // Grow extensions (rewind markers, compaction signals) are consumed
            // by the filter and intentionally dropped (matching the typed load).
            Ok(SessionUpdate::Grow(_)) => {}
            // Best-effort: an unparseable line (e.g. a partially written trailing
            // line) is skipped rather than aborting replay; the typed load drops
            // it too. Logged for diagnostics.
            Err(e) => tracing::debug!(error = %e, "skipping unparseable replay line"),
        }
    }
    Ok(forwarded)
}

#[doc(hidden)]
pub struct PreparedReplay<'a> {
    /// Rewind-filtered replay lines, each borrowed from the input transcript.
    pub lines: Vec<&'a str>,
    pub(crate) mark_replay: bool,
    /// Highest `eventId` counter across all live (rewind-filtered) lines, used
    /// to re-seed the process-global event counter on resume so post-load live
    /// events keep monotonically increasing ids (see
    /// [`crate::util::event_id::ensure_event_counter_at_least`]). `None` when no
    /// line carried a parseable `eventId`.
    pub(crate) max_event_seq: Option<u64>,
    pub(crate) total_live: usize,
    /// UI cache coverage for canonical subagent lifecycle facts.
    pub(crate) subagent_projections: SubagentProjectionState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SubagentProjectionState {
    pub spawned: std::collections::BTreeSet<String>,
    pub finished: std::collections::BTreeSet<String>,
}

/// Coverage of canonical lifecycle facts already present in the replay cache.
fn collect_subagent_projection_state(filtered: &[&str]) -> SubagentProjectionState {
    use crate::extensions::notification::SessionUpdate as Update;
    let mut state = SubagentProjectionState::default();
    for line in filtered {
        if !line.contains("subagent_spawned") && !line.contains("subagent_finished") {
            continue;
        }
        let Ok(envelope) = serde_json::from_str::<RawLinePeek<'_>>(line) else {
            continue;
        };
        if envelope.method != GROW_SESSION_UPDATE_METHOD {
            continue;
        }
        let Ok(notification) = serde_json::from_str::<SessionNotification>(envelope.params.get())
        else {
            continue;
        };
        match notification.update {
            Update::SubagentSpawned { subagent_id, .. } => {
                state.spawned.insert(subagent_id);
            }
            Update::SubagentFinished { subagent_id, .. } => {
                state.finished.insert(subagent_id);
            }
            _ => {}
        }
    }
    state
}

/// The raw `_meta` object of a canonical persisted envelope, if any, without
/// allocating a `serde_json::Value`.
fn line_meta(line: &str) -> Option<&serde_json::value::RawValue> {
    let env = serde_json::from_str::<RawLinePeek<'_>>(line).ok()?;
    if !matches!(
        env.method,
        ACP_SESSION_UPDATE_METHOD | GROW_SESSION_UPDATE_METHOD
    ) {
        return None;
    }
    serde_json::from_str::<RawParamsPeek<'_>>(env.params.get())
        .ok()?
        .meta
}

/// The `"update":` object key (a protocol key, not an enum discriminant). The
/// structural `params.update` is the FIRST occurrence in a persisted line: the
/// envelope prefix has no `"update":`, and any nested `"update"` (in `_meta` or a
/// tool's `rawInput`/`rawOutput`) is serialized after it, so the first match delimits it.
const UPDATE_KEY: &str = r#""update":"#;

/// Is this persisted line an `available_commands_update`?
///
/// The slash-command catalog is re-advertised in full after every `session/load`,
/// so the historical copies in `updates.jsonl` are redundant on replay and
/// dominate large sessions (~51% of bytes in pathological cases). The lines stay
/// on disk; this only skips forwarding them to the client.
///
/// A cheap [`AVAILABLE_COMMANDS_UPDATE_PREFIX`] substring pre-filter, then a
/// positional confirm that the value at the first [`UPDATE_KEY`] begins with the
/// ACU discriminant. Reads only the prefix (never the huge `availableCommands`
/// array), so it can't be fooled by the discriminant embedded in `_meta` or a
/// tool payload (never the first `"update":`).
pub(crate) fn line_is_available_commands_update(line: &str) -> bool {
    if !line.contains(&*AVAILABLE_COMMANDS_UPDATE_PREFIX) {
        return false;
    }
    line.find(UPDATE_KEY)
        .map(|pos| {
            line[pos + UPDATE_KEY.len()..]
                .trim_start()
                .starts_with(&*AVAILABLE_COMMANDS_UPDATE_PREFIX)
        })
        .unwrap_or(false)
}

// `_meta` protocol field names (not enum discriminants).
/// `_meta` key holding the per-event id used for cursor-based reconnect.
const EVENT_ID_KEY: &str = "eventId";

/// This line's `_meta.eventId`, if any. Cheap peek (no `Value`).
fn line_event_id(line: &str) -> Option<std::borrow::Cow<'_, str>> {
    if !line.contains(EVENT_ID_KEY) {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct EventIdPeek<'a> {
        // `Cow` so an escaped eventId still parses and compares equal
        // (`Option<Cow>` always deserializes owned; `&str` would error).
        #[serde(rename = "eventId", borrow)]
        event_id: Option<std::borrow::Cow<'a, str>>,
    }
    serde_json::from_str::<EventIdPeek<'_>>(line_meta(line)?.get())
        .ok()
        .and_then(|e| e.event_id)
}

/// Does this line's `_meta.eventId` equal `cursor_id`?
fn line_has_event_id(line: &str, cursor_id: &str) -> bool {
    line_event_id(line).as_deref() == Some(cursor_id)
}

/// Rewind-filter, resolve the reconnect cursor, and drop redundant command
/// catalogs. Pure UI replay processing, no agent-state recovery.
///
/// The cursor is resolved before dropping ACUs, because an idle client often
/// reconnects with an ACU's `eventId` as its cursor; resolving against the
/// ACU-inclusive set keeps reconnect incremental instead of a full replay.
///
/// `#[doc(hidden)] pub` (not stable API): production replay uses it, and the
/// session-load memory test drives it to check the peek stays zero-copy.
#[doc(hidden)]
pub fn prepare_replay_lines<'a>(contents: &'a str, cursor: Option<&str>) -> PreparedReplay<'a> {
    let filtered = filter_rewind_lines(contents.lines().filter(|l| !l.trim().is_empty()).collect());

    // Highest `eventId` counter across all live (rewind-filtered) lines, used to
    // re-seed the process-global event counter on resume so post-load live events
    // keep monotonically increasing ids. eventId is "{sessionId}-{counter}" and
    // session ids contain dashes, so the counter is the suffix after the LAST '-'.
    let mut max_event_seq: Option<u64> = None;
    for line in &filtered {
        if line.contains("eventId")
            && let Ok(env) = serde_json::from_str::<RawLinePeek<'_>>(line)
            && matches!(
                env.method,
                ACP_SESSION_UPDATE_METHOD | GROW_SESSION_UPDATE_METHOD
            )
            && let Ok(pp) = serde_json::from_str::<RawParamsPeek<'_>>(env.params.get())
            && let Some(meta_raw) = pp.meta
            && let Ok(meta) = serde_json::from_str::<serde_json::Value>(meta_raw.get())
            && let Some(seq) = meta
                .get("eventId")
                .and_then(|v| v.as_str())
                .and_then(|s| s.rsplit('-').next())
                .and_then(|c| c.parse::<u64>().ok())
        {
            max_event_seq = Some(max_event_seq.map_or(seq, |m| m.max(seq)));
        }
    }

    // Resolve the reconnect cursor against the ACU-inclusive set. `mark_replay`
    // is true for a full historical replay (no cursor, or cursor not found).
    //
    // The cursor is refused when a FORWARDED tail line lacks an `eventId`:
    // such a line cannot be covered by a future cursor and has no client-side
    // dedup, so re-delivering it as live would re-apply it. Full replay is
    // the safe fallback — the client swaps it in wholesale. Id-less lines
    // come from older binaries or any emitter outside the stamping
    // chokepoints (see `ensure_event_id_meta`). ACU lines are exempt: they
    // are dropped below, never forwarded.
    let cursor_pos = cursor
        .and_then(|id| filtered.iter().rposition(|l| line_has_event_id(l, id)))
        .filter(|&pos| {
            let bounded = filtered[pos + 1..]
                .iter()
                .all(|l| line_is_available_commands_update(l) || line_event_id(l).is_some());
            if !bounded {
                tracing::warn!(
                    "replay: post-cursor tail contains eventId-less lines; full replay instead"
                );
            }
            bounded
        });
    let mark_replay = cursor_pos.is_none();
    let start = cursor_pos.map_or(0, |pos| pos + 1);

    // Single pass: drop ACUs (kept on disk), collect the post-cursor tail to
    // forward, and count the full ACU-free live set for the skip log.
    let mut lines: Vec<&str> = Vec::with_capacity(filtered.len().saturating_sub(start));
    let mut total_live = 0usize;
    for (i, &line) in filtered.iter().enumerate() {
        if line_is_available_commands_update(line) {
            continue;
        }
        total_live += 1;
        if i >= start {
            lines.push(line);
        }
    }

    PreparedReplay {
        lines,
        mark_replay,
        max_event_seq,
        total_live,
        subagent_projections: collect_subagent_projection_state(&filtered),
    }
}

/// Blank-strip, drop redundant command catalogs, and rewind-filter a raw
/// `updates.jsonl` segment. Shared by the delta-replay path (which has no
/// reconnect cursor); the initial replay path is [`prepare_replay_lines`], which
/// additionally resolves a cursor (and so must see ACUs) before dropping them.
pub(crate) fn filter_delta_replay_lines(contents: &str) -> Vec<&str> {
    let live: Vec<&str> = contents
        .lines()
        .filter(|l| !l.trim().is_empty() && !line_is_available_commands_update(l))
        .collect();
    filter_rewind_lines(live)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn durable_atomic_write_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("atomic-state.json");
        write_bytes_atomic_durable_async(&path, b"old".to_vec())
            .await
            .unwrap();

        write_bytes_atomic_durable_async(&path, b"new".to_vec())
            .await
            .unwrap();

        assert_eq!(std::fs::read(path).unwrap(), b"new");
    }

    #[cfg(any(
        all(target_os = "linux", target_env = "gnu"),
        target_os = "macos",
        windows
    ))]
    #[test]
    fn no_replace_publication_preserves_an_existing_target() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&target).unwrap();
        std::fs::write(source.join("source-marker"), b"source").unwrap();
        std::fs::write(target.join("target-marker"), b"target").unwrap();

        let error = rename_no_replace(&source, &target).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(source.join("source-marker")).unwrap(),
            b"source"
        );
        assert_eq!(
            std::fs::read(target.join("target-marker")).unwrap(),
            b"target"
        );
    }

    /// Wrap an ACP notification as the envelope stored in updates.jsonl.
    fn acp_envelope(session_update_json: &str) -> String {
        format!(
            r#"{{"timestamp":1,"method":"session/update","params":{{"sessionId":"s","update":{session_update_json}}}}}"#
        )
    }

    /// Wrap a Grow notification as the envelope stored in updates.jsonl.
    fn grow_envelope(session_update_json: &str) -> String {
        format!(
            r#"{{"timestamp":1,"method":"_grow/session/update","params":{{"sessionId":"s","update":{session_update_json}}}}}"#
        )
    }

    fn user_chunk(text: &str, prompt_index: Option<usize>) -> SessionUpdate {
        let mut chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
            text.to_string(),
        )));
        if let Some(pi) = prompt_index {
            chunk = chunk.meta(
                serde_json::json!({ "promptIndex": pi })
                    .as_object()
                    .cloned(),
            );
        }
        SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            acp::SessionId::new("s"),
            acp::SessionUpdate::UserMessageChunk(chunk),
        )))
    }

    fn agent_chunk(text: &str) -> SessionUpdate {
        SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            acp::SessionId::new("s"),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text.to_string()),
            ))),
        )))
    }

    #[test]
    fn updates_truncate_ignores_unmarked_phantoms_when_markers_present() {
        let updates = vec![
            user_chunk("P0", Some(0)),
            agent_chunk("A0"),
            user_chunk("!pwd", None),
            agent_chunk("out"),
            user_chunk("P1", Some(1)),
            agent_chunk("A1"),
            user_chunk("P2", Some(2)),
            agent_chunk("A2"),
        ];
        // Keep through P1 (indices 0,1); cut at start of P2 run.
        let cut = updates_truncate_for_prompt(&updates, 1);
        assert_eq!(cut, 6);
        assert!(matches!(
            &updates[cut],
            SessionUpdate::Acp(n) if matches!(
                &n.update,
                acp::SessionUpdate::UserMessageChunk(c)
                    if matches!(&c.content, acp::ContentBlock::Text(t) if t.text == "P2")
            )
        ));
    }

    #[test]
    fn updates_truncate_splits_consecutive_marked_prompts_without_agent() {
        let updates: Vec<_> = (0..6)
            .map(|i| user_chunk(&format!("P{i}"), Some(i)))
            .collect();
        // Target 2 keeps turns 0 and 1; cut at P2 (index 2).
        assert_eq!(updates_truncate_for_prompt(&updates, 1), 2);
        assert_eq!(updates_truncate_for_prompt(&updates, 2), 3);
        assert_eq!(updates_truncate_for_prompt(&updates, 5), 6);
    }

    /// Mixed stream: unmarked runs before the first promptIndex still count.
    #[test]
    fn updates_truncate_mixed_unmarked_prefix_then_markers() {
        let updates = vec![
            user_chunk("old0", None),
            agent_chunk("A0"),
            user_chunk("old1", None),
            agent_chunk("A1"),
            user_chunk("new2", Some(2)),
            agent_chunk("A2"),
            user_chunk("!pwd", None),
            agent_chunk("out"),
            user_chunk("new3", Some(3)),
            agent_chunk("A3"),
        ];
        // Target 1 keeps old0+old1; cut at new2.
        assert_eq!(updates_truncate_for_prompt(&updates, 1), 4);
        // Target 2 keeps through A2 (and phantom run does not add a turn); cut at new3.
        assert_eq!(updates_truncate_for_prompt(&updates, 2), 8);
        assert_eq!(updates_truncate_for_prompt(&updates, 0), 2);
    }

    #[test]
    fn filter_rewind_mixed_unmarked_prefix_then_markers() {
        let o0 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"old0"}}"#,
        );
        let a0 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A0"}}"#,
        );
        let o1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"old1"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A1"}}"#,
        );
        let n2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"new2"},"_meta":{"promptIndex":2}}"#,
        );
        let a2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A2"}}"#,
        );
        let n3 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"new3"},"_meta":{"promptIndex":3}}"#,
        );
        // Rewind to target 2: keep turns 0,1 (old0, old1); drop new2+.
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":2,"created_at":"2024-01-01"}"#,
        );
        let after = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"after"},"_meta":{"promptIndex":2}}"#,
        );
        let lines = vec![
            o0.as_str(),
            a0.as_str(),
            o1.as_str(),
            a1.as_str(),
            n2.as_str(),
            a2.as_str(),
            n3.as_str(),
            rw.as_str(),
            after.as_str(),
        ];
        let kept = filter_rewind_lines(lines);
        let texts: Vec<&str> = kept
            .iter()
            .filter_map(|l| {
                if l.contains("\"text\":\"old0\"") {
                    Some("old0")
                } else if l.contains("\"text\":\"old1\"") {
                    Some("old1")
                } else if l.contains("\"text\":\"new2\"") {
                    Some("new2")
                } else if l.contains("\"text\":\"new3\"") {
                    Some("new3")
                } else if l.contains("\"text\":\"after\"") {
                    Some("after")
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(texts, vec!["old0", "old1", "after"]);
    }

    #[test]
    fn filter_rewind_ignores_unmarked_phantoms_when_markers_present() {
        let p0 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"P0"},"_meta":{"promptIndex":0}}"#,
        );
        let a0 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A0"}}"#,
        );
        let phantom = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"!pwd"}}"#,
        );
        let p1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"P1"},"_meta":{"promptIndex":1}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A1"}}"#,
        );
        let p2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"P2"},"_meta":{"promptIndex":2}}"#,
        );
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":2,"created_at":"2024-01-01"}"#,
        );
        let after = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"after"},"_meta":{"promptIndex":2}}"#,
        );
        let lines = vec![
            p0.as_str(),
            a0.as_str(),
            phantom.as_str(),
            p1.as_str(),
            a1.as_str(),
            p2.as_str(),
            rw.as_str(),
            after.as_str(),
        ];
        let kept = filter_rewind_lines(lines);
        let texts: Vec<&str> = kept
            .iter()
            .filter_map(|l| {
                if l.contains("\"text\":\"P0\"") {
                    Some("P0")
                } else if l.contains("!pwd") {
                    Some("phantom")
                } else if l.contains("\"text\":\"P1\"") {
                    Some("P1")
                } else if l.contains("\"text\":\"P2\"") {
                    Some("P2")
                } else if l.contains("\"text\":\"after\"") {
                    Some("after")
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(texts, vec!["P0", "phantom", "P1", "after"]);
    }

    // ── filter_rewind_lines tests ────────────────────────────────────────────

    #[test]
    fn filter_rewind_removes_dead_branch() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"first"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp1"}}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"second"}}"#,
        );
        let a2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp2"}}"#,
        );
        // Rewind to prompt 1 — kills u2, a2
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let u3 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"replacement"}}"#,
        );
        let a3 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp3"}}"#,
        );

        let lines = vec![
            u1.as_str(),
            a1.as_str(),
            u2.as_str(),
            a2.as_str(),
            rw.as_str(),
            u3.as_str(),
            a3.as_str(),
        ];
        let result = filter_rewind_lines(lines);

        // u1, a1 survive. u2, a2, rewind marker removed. u3, a3 added.
        assert_eq!(result.len(), 4);
        assert!(result[0].contains("first"));
        assert!(result[1].contains("resp1"));
        assert!(result[2].contains("replacement"));
        assert!(result[3].contains("resp3"));
    }

    #[test]
    fn filter_rewind_ignores_a_malformed_middle_line() {
        let user_message_1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"first"}}"#,
        );
        let agent_message_1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp1"}}"#,
        );
        let user_message_2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"second"}}"#,
        );
        let agent_message_2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp2"}}"#,
        );
        let rewind_to_1 = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let torn = "{ torn, unparseable jsonl line";

        // The malformed line is kept but not counted as a prompt boundary, so
        // the rewind still drops prompt 1.
        let survivors = filter_rewind_lines(vec![
            user_message_1.as_str(),
            agent_message_1.as_str(),
            torn,
            user_message_2.as_str(),
            agent_message_2.as_str(),
            rewind_to_1.as_str(),
        ]);

        pretty_assertions::assert_eq!(
            survivors,
            vec![user_message_1.as_str(), agent_message_1.as_str(), torn]
        );
    }

    #[test]
    fn filter_rewind_to_zero_clears_all() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"only"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp"}}"#,
        );
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":0,"created_at":"2024-01-01"}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"fresh start"}}"#,
        );

        let lines = vec![u1.as_str(), a1.as_str(), rw.as_str(), u2.as_str()];
        let result = filter_rewind_lines(lines);

        assert_eq!(result.len(), 1);
        assert!(result[0].contains("fresh start"));
    }

    #[test]
    fn filter_rewind_double_rewind() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r1"}}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p2"}}"#,
        );
        let a2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r2"}}"#,
        );
        let u3 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p3"}}"#,
        );
        let a3 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r3"}}"#,
        );
        // Rewind to prompt 2 — kills p3/r3
        let rw1 = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":2,"created_at":"2024-01-01"}"#,
        );
        let u4 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p4"}}"#,
        );
        let a4 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r4"}}"#,
        );
        // Rewind to prompt 1 — kills p2/r2/p4/r4
        let rw2 = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let u5 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"final"}}"#,
        );

        let lines = vec![
            u1.as_str(),
            a1.as_str(),
            u2.as_str(),
            a2.as_str(),
            u3.as_str(),
            a3.as_str(),
            rw1.as_str(),
            u4.as_str(),
            a4.as_str(),
            rw2.as_str(),
            u5.as_str(),
        ];
        let result = filter_rewind_lines(lines);

        // Only p1, r1, final survive
        assert_eq!(result.len(), 3);
        assert!(result[0].contains("p1"));
        assert!(result[1].contains("r1"));
        assert!(result[2].contains("final"));
    }

    /// The raw-line filter and the typed filter must truncate an identical
    /// rewind timeline to the same surviving updates, in the same order.
    #[test]
    fn filter_rewind_lines_and_updates_agree() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r1"}}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p2"}}"#,
        );
        let a2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r2"}}"#,
        );
        let rw1 = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":2,"created_at":"2024-01-01"}"#,
        );
        let u3 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p3"}}"#,
        );
        let a3 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r3"}}"#,
        );
        let rw2 = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let u4 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"final"}}"#,
        );

        let lines = vec![
            u1.as_str(),
            a1.as_str(),
            u2.as_str(),
            a2.as_str(),
            rw1.as_str(),
            u3.as_str(),
            a3.as_str(),
            rw2.as_str(),
            u4.as_str(),
        ];

        let ser = |u: &SessionUpdate| serde_json::to_string(u).unwrap();
        let via_lines: Vec<String> = filter_rewind_lines(lines.clone())
            .iter()
            .map(|l| ser(&SessionUpdateEnvelope::from_str(l).unwrap()))
            .collect();
        let typed: Vec<SessionUpdate> = lines
            .iter()
            .map(|l| SessionUpdateEnvelope::from_str(l).unwrap())
            .collect();
        let via_updates: Vec<String> = filter_rewind_updates(typed).iter().map(ser).collect();

        assert_eq!(via_lines, via_updates);
    }

    /// An out-of-range rewind target folds to `result.len()` (the
    /// `unwrap_or(result.len())` branch in `filter_rewind_by`), so truncation is
    /// a no-op and every survivor is kept.
    #[test]
    fn filter_rewind_out_of_range_target_keeps_all() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r1"}}"#,
        );
        // Only prompt index 0 exists; target 5 is out of range.
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":5,"created_at":"2024-01-01"}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p2"}}"#,
        );

        let lines = vec![u1.as_str(), a1.as_str(), rw.as_str(), u2.as_str()];
        let result = filter_rewind_lines(lines);

        // Marker is dropped; the three ACP survivors remain in order.
        assert_eq!(result.len(), 3);
        assert!(result[0].contains("p1"));
        assert!(result[1].contains("r1"));
        assert!(result[2].contains("p2"));
    }

    /// A session with no `updates.jsonl` streams nothing, so the emission gate
    /// reports `Empty` and forwards no updates.
    #[test]
    fn stream_replay_updates_at_missing_session_is_empty() {
        let grow_home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(grow_home.path().join("sessions")).unwrap();

        let mut count = 0usize;
        let emission =
            stream_replay_updates_at("does-not-exist", grow_home.path(), |_| count += 1).unwrap();

        assert_eq!(emission, ReplayEmission::Empty);
        assert_eq!(count, 0);
    }

    /// A resolvable session whose `updates.jsonl` cannot be read surfaces the
    /// error rather than folding to `Empty`, so the caller logs a real fault
    /// instead of mistaking it for an absent transcript. (The path is a
    /// directory, which `read_to_string` rejects.)
    #[test]
    fn stream_replay_updates_at_surfaces_read_errors() {
        let grow_home = tempfile::tempdir().unwrap();
        let session_dir = grow_home.path().join("sessions").join("cwd").join("sess");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join(SUMMARY_FILE), "{}").unwrap();
        std::fs::create_dir(session_dir.join(UPDATES_FILE)).unwrap();

        let result = stream_replay_updates_at("sess", grow_home.path(), |_| {});
        assert!(
            result.is_err(),
            "read fault must surface, not fold to Empty: {result:?}"
        );
    }

    /// End-to-end: the streaming core (`for_each_replay_update_in_file`, what
    /// `stream_replay_updates_at` wraps) applies rewind over a real file and
    /// yields the same survivors as the typed parse-all path.
    #[test]
    fn streaming_replay_applies_rewind_like_the_typed_path() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r1"}}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p2"}}"#,
        );
        // Rewind to prompt 1 drops p2.
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let u3 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"final"}}"#,
        );
        let raw = format!("{u1}\n{a1}\n{u2}\n{rw}\n{u3}\n");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(UPDATES_FILE);
        std::fs::write(&path, &raw).unwrap();

        let mut streamed = Vec::new();
        let forwarded = for_each_replay_update_in_file(&path, |u| streamed.push(u)).unwrap();
        assert!(forwarded);

        // Typed reference: parse all, rewind-filter, map ACP survivors.
        let typed: Vec<SessionUpdate> = raw
            .lines()
            .map(|l| SessionUpdateEnvelope::from_str(l).unwrap())
            .collect();
        let reference: Vec<acp::SessionUpdate> = filter_rewind_updates(typed)
            .into_iter()
            .filter_map(|u| match u {
                SessionUpdate::Acp(notif) => Some(strip_context_wrappers(notif.update)),
                SessionUpdate::Grow(_) => None,
            })
            .collect();

        let ser = |u: &acp::SessionUpdate| serde_json::to_string(u).unwrap();
        assert_eq!(
            streamed.iter().map(ser).collect::<Vec<_>>(),
            reference.iter().map(ser).collect::<Vec<_>>(),
        );
    }

    // ── prepare_replay_lines tests ───────────────────────────────────────────

    /// Envelope with _meta at the params level (where the real agent puts it).
    fn acp_envelope_with_meta(session_update_json: &str, meta_json: &str) -> String {
        format!(
            r#"{{"timestamp":1,"method":"session/update","params":{{"sessionId":"s","update":{session_update_json},"_meta":{meta_json}}}}}"#
        )
    }

    #[test]
    fn prepare_replay_cursor_skips_to_position() {
        let u1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"old"}}"#,
            r#"{"eventId":"ev1"}"#,
        );
        let a1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"old resp"}}"#,
            r#"{"eventId":"ev2"}"#,
        );
        let u2 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"new"}}"#,
            r#"{"eventId":"ev3"}"#,
        );
        let raw = format!("{u1}\n{a1}\n{u2}\n");

        let prepared = prepare_replay_lines(&raw, Some("ev2"));
        // Should skip ev1 and ev2, return only ev3
        assert_eq!(prepared.lines.len(), 1);
        assert!(!prepared.mark_replay);
        assert!(prepared.lines[0].contains("new"));
        assert_eq!(prepared.total_live, 3);
    }

    #[test]
    fn prepare_replay_cursor_not_found_returns_all() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}}"#,
        );
        let raw = format!("{u1}\n");

        let prepared = prepare_replay_lines(&raw, Some("nonexistent"));
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.mark_replay); // fallback to full replay
    }

    /// A resolved cursor is refused when the tail contains an eventId-less
    /// line (older-binary history): the line has no client-side dedup and no
    /// future cursor can cover it, so an incremental tail would re-apply it.
    /// Full replay is the safe fallback.
    #[test]
    fn prepare_replay_cursor_refused_when_tail_has_event_id_less_line() {
        let a1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"seen"}}"#,
            r#"{"eventId":"ev1"}"#,
        );
        // Grow-style line persisted by an older binary: no _meta at all.
        let old_grow = r#"{"timestamp":2,"method":"_grow/session/update","params":{"sessionId":"s","update":{"sessionUpdate":"hook_annotation","message":"trailing"}}}"#;
        let raw = format!("{a1}\n{old_grow}\n");

        let prepared = prepare_replay_lines(&raw, Some("ev1"));
        assert!(
            prepared.mark_replay,
            "an unbounded tail must force a full replay"
        );
        assert_eq!(prepared.lines.len(), 2, "full history is replayed");

        // Same history with the trailing line stamped resolves incrementally.
        let new_grow = r#"{"timestamp":2,"method":"_grow/session/update","params":{"sessionId":"s","update":{"sessionUpdate":"hook_annotation","message":"trailing"},"_meta":{"eventId":"ev2"}}}"#;
        let raw = format!("{a1}\n{new_grow}\n");
        let prepared = prepare_replay_lines(&raw, Some("ev1"));
        assert!(!prepared.mark_replay);
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.lines[0].contains("trailing"));

        // An id-less ACU in the tail is exempt from the refusal — ACUs are
        // dropped before forwarding, so they can never be re-applied.
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let raw = format!("{a1}\n{acu}\n");
        let prepared = prepare_replay_lines(&raw, Some("ev1"));
        assert!(
            !prepared.mark_replay,
            "a trailing id-less ACU must not force a full replay"
        );
        assert!(
            prepared.lines.is_empty(),
            "the ACU is dropped, never forwarded"
        );
    }

    #[test]
    fn prepare_replay_extracts_max_event_seq() {
        // eventId is "{sessionId}-{counter}" and session ids contain dashes, so
        // the counter is the suffix after the LAST '-'. max_event_seq is the
        // highest counter across all live lines — used to re-seed the global
        // event counter on resume so post-load live events stay monotonic and
        // don't get dropped by the client's eventId dedup.
        let a1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a"}}"#,
            r#"{"eventId":"019e-abcd-7","totalTokens":100}"#,
        );
        let a2 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"b"}}"#,
            r#"{"eventId":"019e-abcd-42","totalTokens":250}"#,
        );
        // Out-of-order counter (lower than the max) must not lower the result.
        let a3 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"c"}}"#,
            r#"{"eventId":"019e-abcd-13","totalTokens":250}"#,
        );
        let raw = format!("{a1}\n{a2}\n{a3}\n");

        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(
            prepared.max_event_seq,
            Some(42),
            "max counter across all lines (suffix after last '-')"
        );
    }

    #[test]
    fn prepare_replay_no_event_ids_yields_none_max_seq() {
        // Lines without a parseable numeric eventId suffix (older shell) yield
        // None, so the counter is left untouched on resume.
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a"}}"#,
        );
        let raw = format!("{a1}\n");
        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(prepared.max_event_seq, None);
    }

    // ── available_commands_update skip (T1) + single-pass equivalence ─────────

    #[test]
    fn acu_line_detection_exact_and_no_false_positive() {
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        assert!(line_is_available_commands_update(&acu));

        // A user message that merely mentions the phrase must NOT match.
        let user_mentions = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"what is available_commands_update?"}}"#,
        );
        assert!(!line_is_available_commands_update(&user_mentions));
    }

    /// The anchor must reject the discriminant when it sits inside `_meta` (not
    /// at the `params.update` position) — the real update here is a non-ACU.
    #[test]
    fn acu_anchor_ignores_discriminant_in_meta() {
        let line = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}"#,
            r#"{"sessionUpdate":"available_commands_update"}"#,
        );
        // The exact `"sessionUpdate":"available_commands_update"` substring IS
        // present (in _meta), but it's not anchored to `"update":{`.
        assert!(line.contains(r#""sessionUpdate":"available_commands_update""#));
        assert!(!line_is_available_commands_update(&line));
    }

    /// A NON-ACU line whose `_meta` embeds the FULL unescaped nested anchor
    /// (`{"update":{"sessionUpdate":"available_commands_update",...}}`) passes the
    /// cheap substring pre-filter but must be REJECTED by the positional confirm
    /// (its real `params.update` is a `tool_call`) — so it is never dropped.
    #[test]
    fn acu_confirm_rejects_nested_update_anchor_in_meta() {
        let line = acp_envelope_with_meta(
            r#"{"sessionUpdate":"tool_call","toolCallId":"t","title":"x"}"#,
            r#"{"echo":{"update":{"sessionUpdate":"available_commands_update","availableCommands":[]}}}"#,
        );
        // The discriminant prefix IS present (in _meta) — pre-filter would match...
        assert!(line.contains(&*AVAILABLE_COMMANDS_UPDATE_PREFIX));
        // ...but the structural params.update is a tool_call, so NOT an ACU.
        assert!(!line_is_available_commands_update(&line));

        // And the non-ACU line survives replay (is not dropped).
        let raw = format!("{line}\n");
        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(prepared.lines.len(), 1, "non-ACU line must not be dropped");
        assert!(prepared.lines[0].contains("tool_call"));
    }

    /// Pin the cross-crate assumption behind [`line_is_available_commands_update`]:
    /// the structural `params.update` serializes BEFORE the optional `_meta`. Run a
    /// genuine ACU through the real write path ([`SessionUpdateEnvelope::from_update`])
    /// and assert its first `"update":` precedes any `"_meta":`, and the detector accepts it.
    #[test]
    fn acu_real_write_path_serializes_update_before_meta() {
        let notif = acp::SessionNotification::new(
            acp::SessionId::new("s"),
            acp::SessionUpdate::AvailableCommandsUpdate(acp::AvailableCommandsUpdate::new(vec![])),
        )
        .meta(serde_json::json!({ "eventId": "ev1" }).as_object().cloned());
        let envelope =
            SessionUpdateEnvelope::from_update(&SessionUpdate::Acp(Box::new(notif))).unwrap();
        let line = serde_json::to_string(&envelope).unwrap();

        let update_idx = line
            .find(UPDATE_KEY)
            .expect("serialized ACU line must contain an \"update\" key");
        if let Some(meta_idx) = line.find(r#""_meta":"#) {
            assert!(
                update_idx < meta_idx,
                "params.update must serialize before _meta: {line}"
            );
        }
        assert!(line_is_available_commands_update(&line));
    }

    #[test]
    fn prepare_replay_drops_available_commands_update() {
        let u = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}}"#,
        );
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let a = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"yo"}}"#,
        );
        let raw = format!("{u}\n{acu}\n{a}\n");

        let prepared = prepare_replay_lines(&raw, None);
        // ACU dropped; the two real updates kept in original order.
        assert_eq!(prepared.lines.len(), 2);
        assert_eq!(prepared.total_live, 2);
        assert!(
            prepared
                .lines
                .iter()
                .all(|l| !l.contains("available_commands_update"))
        );
        assert!(prepared.lines[0].contains("hi"));
        assert!(prepared.lines[1].contains("yo"));
        assert!(prepared.mark_replay);
    }

    #[test]
    fn prepare_replay_rewind_truncates_and_drops_acu() {
        let u0 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p0"}}"#,
            r#"{"totalTokens":5}"#,
        );
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let a0 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a0"}}"#,
            r#"{"totalTokens":7}"#,
        );
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":0,"created_at":"2024-01-01"}"#,
        );
        let u1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
            r#"{"totalTokens":9}"#,
        );
        let raw = format!("{u0}\n{acu}\n{a0}\n{rw}\n{u1}\n");

        let prepared = prepare_replay_lines(&raw, None);
        // Rewind to 0 kills u0/a0; ACU dropped; only the new p1 survives.
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.lines[0].contains("p1"));
        assert_eq!(prepared.total_live, 1);
        assert!(prepared.mark_replay);
    }

    /// The single-pass implementation must match an independent reference that
    /// drops ACU then applies the (canonical) rewind filter — for a mixed input.
    #[test]
    fn prepare_replay_single_pass_matches_reference() {
        let lines_src = [
            acp_envelope_with_meta(
                r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p0"}}"#,
                r#"{"totalTokens":3}"#,
            ),
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#),
            acp_envelope_with_meta(
                r#"{"sessionUpdate":"tool_call_update","toolCallId":"t","status":"completed"}"#,
                r#"{"totalTokens":11}"#,
            ),
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#),
            acp_envelope(
                r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a0"}}"#,
            ),
        ];
        let raw = format!("{}\n", lines_src.join("\n"));

        // Reference: filter blanks + ACU, then canonical rewind filter, count.
        let reference: Vec<&str> = filter_rewind_lines(
            raw.lines()
                .filter(|l| !l.trim().is_empty() && !line_is_available_commands_update(l))
                .collect(),
        );

        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(prepared.lines, reference);
        assert_eq!(prepared.total_live, reference.len());
    }

    /// A user prompt whose text contains the literal escaped-JSON ACU
    /// discriminant must NOT be dropped as an `available_commands_update` — the
    /// `"update":{` anchor only matches the real structural discriminant, not the
    /// escaped fragment in content.
    #[test]
    fn acu_drop_ignores_escaped_json_in_content() {
        let line = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"paste: {\"sessionUpdate\":\"available_commands_update\"}"}}"#,
        );
        // The bare phrase appears in the (escaped) content, but it's not at the
        // structural `"update":{"sessionUpdate":...` position, so it's kept.
        assert!(line.contains("available_commands_update"));
        assert!(!line_is_available_commands_update(&line));

        let raw = format!("{line}\n");
        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(prepared.lines.len(), 1, "user prompt must survive replay");
        assert!(prepared.lines[0].contains("available_commands_update"));
    }

    /// An idle client reconnecting with the cursor pointing at the LAST persisted
    /// event — an ACU (the post-load re-advertise) — must resolve the cursor on the
    /// ACU-inclusive set rather than fall back to full replay.
    #[test]
    fn prepare_replay_cursor_on_dropped_acu_resolves() {
        let u = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}}"#,
            r#"{"eventId":"ev1"}"#,
        );
        let a = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"yo"}}"#,
            r#"{"eventId":"ev2"}"#,
        );
        let acu = acp_envelope_with_meta(
            r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#,
            r#"{"eventId":"ev3"}"#,
        );
        let raw = format!("{u}\n{a}\n{acu}\n");

        // Cursor == the ACU's eventId → resolved; nothing after → no replay,
        // and crucially NOT a full replay.
        let prepared = prepare_replay_lines(&raw, Some("ev3"));
        assert!(!prepared.mark_replay, "must not fall back to full replay");
        assert!(prepared.lines.is_empty(), "client is already caught up");

        // Cursor == ev1 → replay ev2, ev3; the ACU (ev3) is dropped from the tail.
        let prepared = prepare_replay_lines(&raw, Some("ev1"));
        assert!(!prepared.mark_replay);
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.lines[0].contains("yo"));
    }

    /// A trailing `rewind_marker` empties the live replay set.
    #[test]
    fn prepare_replay_trailing_rewind_marker_empties() {
        let u0 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p0"}}"#,
            r#"{"totalTokens":5}"#,
        );
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":0,"created_at":"2024-01-01"}"#,
        );
        let raw = format!("{u0}\n{rw}\n");
        let prepared = prepare_replay_lines(&raw, None);
        assert!(prepared.lines.is_empty());
        assert_eq!(prepared.total_live, 0);
    }

    /// An ACU as the final line is dropped.
    #[test]
    fn prepare_replay_trailing_acu_dropped() {
        let u = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}}"#,
            r#"{"totalTokens":7}"#,
        );
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let raw = format!("{u}\n{acu}\n");
        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.lines[0].contains("hi"));
        assert_eq!(prepared.total_live, 1);
    }

    /// Rewind + cursor + ACU together, with explicit expected values.
    #[test]
    fn prepare_replay_rewind_then_cursor_with_acu() {
        let u0 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p0"}}"#,
            r#"{"eventId":"e0","totalTokens":2}"#,
        );
        let a0 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a0"}}"#,
            r#"{"eventId":"e1"}"#,
        );
        let acu0 =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":0,"created_at":"2024-01-01"}"#,
        );
        let u1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
            r#"{"eventId":"e2","totalTokens":9}"#,
        );
        let acu1 =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let a1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a1"}}"#,
            r#"{"eventId":"e3","totalTokens":12}"#,
        );
        let raw = format!("{u0}\n{a0}\n{acu0}\n{rw}\n{u1}\n{acu1}\n{a1}\n");

        // Rewind to 0 kills u0/a0/acu0; surviving live = [u1(e2), acu1, a1(e3)].
        // Cursor on e2 → tail = [acu1, a1]; drop acu1 → lines = [a1].
        let prepared = prepare_replay_lines(&raw, Some("e2"));
        assert!(!prepared.mark_replay);
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.lines[0].contains("a1"));
        assert_eq!(prepared.total_live, 2); // ACU-free survivors: u1, a1
    }

    /// The delta-replay helper (shared with the initial path) drops blanks + ACUs
    /// and applies the canonical rewind filter.
    #[test]
    fn filter_delta_replay_drops_blank_acu_and_rewinds() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
        );
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a1"}}"#,
        );
        // A second prompt that a trailing rewind_marker then discards.
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p2-dead"}}"#,
        );
        let a2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a2-dead"}}"#,
        );
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let raw = format!("{u1}\n\n{acu}\n{a1}\n{u2}\n{a2}\n{rw}\n");

        let live = filter_delta_replay_lines(&raw);
        // Blank + ACU dropped; the rewind to prompt 1 truncates the dead branch
        // (u2/a2) and consumes the marker, leaving only p1/a1.
        assert_eq!(live.len(), 2);
        assert!(
            live.iter()
                .all(|l| !l.contains("available_commands_update"))
        );
        assert!(live[0].contains("p1"));
        assert!(live[1].contains("a1"));
        assert!(live.iter().all(|l| !l.contains("dead")));
        assert!(live.iter().all(|l| !l.contains("rewind_marker")));
    }

    #[test]
    fn prepare_replay_reports_subagent_projection_coverage() {
        let spawn = |id: &str, child: &str| {
            format!(
                r#"{{"method":"_grow/session/update","params":{{"sessionId":"s","update":{{"sessionUpdate":"subagent_spawned","subagent_id":"{id}","parent_session_id":"s","child_session_id":"{child}","subagent_type":"general-purpose","description":"task"}},"_meta":{{"eventId":"s-1"}}}}}}"#
            )
        };
        let finish = |id: &str| {
            format!(
                r#"{{"method":"_grow/session/update","params":{{"sessionId":"s","update":{{"sessionUpdate":"subagent_finished","subagent_id":"{id}","child_session_id":"c{id}","status":"completed","tool_calls":0,"turns":0,"duration_ms":0,"tokens_used":0}},"_meta":{{"eventId":"s-2"}}}}}}"#
            )
        };
        let raw = format!(
            "{}\n{}\n{}\n",
            spawn("a", "ca"),
            finish("a"),
            spawn("b", "cb")
        );
        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(
            prepared.subagent_projections.spawned,
            ["a".to_string(), "b".to_string()].into_iter().collect()
        );
        assert_eq!(
            prepared.subagent_projections.finished,
            ["a".to_string()].into_iter().collect()
        );
    }

    /// Resume idempotency seam: the finish the projection repair emits must
    /// re-pair the spawn on the next resume (emit→serialize→collect),
    /// so a second resume doesn't re-emit. Guards a `SubagentFinished` shape drift.
    #[test]
    fn collect_tracks_spawn_and_finish_projections_independently() {
        use crate::extensions::notification::{SessionNotification, SessionUpdate};

        let spawn = grow_envelope(
            r#"{"sessionUpdate":"subagent_spawned","subagent_id":"sa","parent_session_id":"s","child_session_id":"ca","subagent_type":"general-purpose","description":"task"}"#,
        );
        // Build the finish exactly as the stream reconcile emits it.
        let finish_notification = SessionNotification {
            session_id: acp::SessionId::new("s"),
            update: SessionUpdate::SubagentFinished {
                subagent_id: "sa".into(),
                child_session_id: "ca".into(),
                status: "cancelled".into(),
                error: Some("interrupted by process restart".into()),
                tool_calls: 0,
                turns: 0,
                duration_ms: 0,
                tokens_used: 0,
                output: None,
            },
            meta: None,
        };
        let finish = serde_json::to_string(
            &SessionUpdateEnvelope::from_update(&super::SessionUpdate::Grow(Box::new(
                finish_notification,
            )))
            .unwrap(),
        )
        .unwrap();

        let state = collect_subagent_projection_state(&[spawn.as_str(), finish.as_str()]);
        assert!(state.spawned.contains("sa"));
        assert!(state.finished.contains("sa"));
    }

    #[test]
    fn from_str_unknown_grow_variant_is_rejected() {
        let line = grow_envelope(r#"{"sessionUpdate":"git_branch_update","branch":"main"}"#);
        assert!(SessionUpdateEnvelope::from_str(&line).is_err());
    }

    #[test]
    fn from_str_known_grow_variant_still_works() {
        let line = grow_envelope(r#"{"sessionUpdate":"memory_flush_started"}"#);
        let update = SessionUpdateEnvelope::from_str(&line).unwrap();
        match update {
            SessionUpdate::Grow(notif) => {
                assert_eq!(
                    notif.update,
                    crate::extensions::notification::SessionUpdate::MemoryFlushStarted
                );
            }
            SessionUpdate::Acp(_) => panic!("expected Grow variant"),
        }
    }
}

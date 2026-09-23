//! Durable notification payload artifacts.
//!
//! Timeline owns receipt, ordering and consumption. This module owns only the
//! immutable bytes referenced by those facts; it is not an inbox or a second
//! delivery-state store.

use std::io;
use std::path::Path;

const ARTIFACT_DIRECTORY: &str = "artifacts/notifications";
const ORPHAN_SWEEP_BATCH_SIZE: usize = 256;

/// A delivery error is classified at the commit boundary, never by UI prose.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AgentMessageDeliveryError {
    #[error("{message}")]
    Rejected { code: String, message: String },
    #[error("{message}")]
    Unconfirmed { code: String, message: String },
}
impl AgentMessageDeliveryError {
    pub(crate) fn rejected(code: &str, message: impl Into<String>) -> Self {
        Self::Rejected {
            code: code.into(),
            message: message.into(),
        }
    }
    pub(crate) fn unconfirmed(code: &str, message: impl Into<String>) -> Self {
        Self::Unconfirmed {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Read-only exact-identity confirmation. It never admits or replays delivery.
pub(crate) fn agent_message_receipt(
    receipts: &[chat_state::TimelineEvent],
    owner: &str,
    source: &chat_state::NotificationSource,
    body: &str,
) -> Result<Option<String>, AgentMessageDeliveryError> {
    let Some((source_id, operation, _, _)) = source.agent_message() else {
        return Err(AgentMessageDeliveryError::rejected(
            "invalid_source",
            "Invalid agent message source",
        ));
    };
    for event in receipts {
        let chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
            id,
            owner_session_id,
            source: existing,
            source_version,
            payload_ref,
        }) = &event.kind
        else {
            continue;
        };
        if existing
            .agent_message()
            .is_some_and(|(sender, id, _, _)| sender == source_id && id == operation)
        {
            if owner_session_id != owner
                || existing != source
                || *source_version
                    != (chat_state::NotificationSourceVersion::Ordinal {
                        value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
                    })
                || payload_ref.bytes != body.len() as u64
                || payload_ref.blake3 != blake3::hash(body.as_bytes()).to_hex().as_str()
            {
                return Err(AgentMessageDeliveryError::rejected(
                    "message_conflict",
                    "Message identity conflicts with its durable receipt",
                ));
            }
            return Ok(Some(id.clone()));
        }
    }
    Ok(None)
}

/// Project only a verified receipt; transport metadata never owns its identity.
pub(crate) fn parent_message_notice(
    event: &chat_state::TimelineEvent,
    body: Option<String>,
) -> Option<crate::extensions::notification::UiNotice> {
    use crate::extensions::notification::{
        AgentMessageNotice, UiNotice, UiNoticeCategory, UiNoticeTone,
    };
    let chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
        id,
        source,
        ..
    }) = &event.kind
    else {
        return None;
    };
    let (source_id, message_id, reply_to, interrupt) = source.agent_message()?;
    let details = AgentMessageNotice {
        source_session_id: source_id.to_owned(),
        message_id: message_id.to_owned(),
        interrupt,
        reply_to: reply_to.cloned(),
        message: body,
    };
    let missing = details.message.is_none();
    Some(UiNotice {
        correlation_id: id.clone(),
        category: UiNoticeCategory::Coordination,
        subject: Some(AgentMessageNotice::SUBJECT.into()),
        description: None,
        message: if missing {
            "Message unavailable"
        } else {
            "Received"
        }
        .into(),
        tone: if missing {
            UiNoticeTone::Warning
        } else {
            UiNoticeTone::Info
        },
        details: Some(serde_json::to_string(&details).expect("parent receipt serializes")),
    })
}

pub(crate) fn read_parent_message_notice(
    session: &crate::session::storage::ContainedDirectory,
    event: &chat_state::TimelineEvent,
) -> Option<crate::extensions::notification::UiNotice> {
    let chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
        source,
        payload_ref,
        ..
    }) = &event.kind
    else {
        return None;
    };
    source.agent_message()?;
    let body = match read_payload(session, payload_ref) {
        Ok(body) => Some(body),
        Err(error) => {
            tracing::warn!(%error, receipt_seq = event.seq.get(), "parent message history body unavailable");
            None
        }
    };
    parent_message_notice(event, body)
}

pub(crate) fn write_payload(
    session: &crate::session::storage::ContainedDirectory,
    text: &str,
) -> io::Result<chat_state::NotificationPayloadRef> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() as u64 > chat_state::MAX_NOTIFICATION_PAYLOAD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "notification payload is empty or exceeds its byte limit",
        ));
    }
    let hash = blake3::hash(bytes).to_hex().to_string();
    crate::session::persistence::write_immutable_blob_to_directory(
        session,
        &Path::new(ARTIFACT_DIRECTORY).join(format!("{hash}.txt")),
        bytes,
    )?;
    Ok(chat_state::NotificationPayloadRef {
        blake3: hash,
        bytes: bytes.len() as u64,
    })
}

pub(crate) fn read_payload(
    session: &crate::session::storage::ContainedDirectory,
    payload: &chat_state::NotificationPayloadRef,
) -> io::Result<String> {
    if payload.blake3.len() != 64
        || !payload.blake3.bytes().all(|byte| byte.is_ascii_hexdigit())
        || payload.bytes == 0
        || payload.bytes > chat_state::MAX_NOTIFICATION_PAYLOAD_BYTES
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "notification payload reference is invalid",
        ));
    }
    let directory = session.open_relative(
        Path::new(ARTIFACT_DIRECTORY),
        "notification payload directory",
        false,
    )?;
    let file_name = format!("{}.txt", payload.blake3);
    let bytes = directory.read_bounded(
        std::ffi::OsStr::new(&file_name),
        "notification payload",
        chat_state::MAX_NOTIFICATION_PAYLOAD_BYTES,
    )?;
    if bytes.len() as u64 != payload.bytes
        || blake3::hash(&bytes).to_hex().as_str() != payload.blake3
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "notification payload artifact does not match its Timeline reference",
        ));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(crate) fn remove_payload(
    session: &crate::session::storage::ContainedDirectory,
    payload: &chat_state::NotificationPayloadRef,
) -> io::Result<()> {
    if payload.blake3.len() != 64 || !payload.blake3.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "notification payload reference is invalid",
        ));
    }
    let directory = session.open_relative(
        Path::new(ARTIFACT_DIRECTORY),
        "notification payload directory",
        false,
    )?;
    let file_name = format!("{}.txt", payload.blake3);
    match directory.remove_file(std::ffi::OsStr::new(&file_name), true) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Stream well-formed payload hashes in bounded batches. Unknown files are
/// ignored but never stop the iterator, so they cannot starve later payloads.
pub(crate) fn visit_payload_hash_batches(
    session: &crate::session::storage::ContainedDirectory,
    mut is_cancelled: impl FnMut() -> bool,
    mut visit: impl FnMut(Vec<String>) -> io::Result<()>,
) -> io::Result<()> {
    let directory = match session.open_relative(
        Path::new(ARTIFACT_DIRECTORY),
        "notification payload directory",
        false,
    ) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut batch = Vec::with_capacity(ORPHAN_SWEEP_BATCH_SIZE);
    directory.visit_names(|name| {
        if is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "notification payload enumeration cancelled",
            ));
        }
        if let Some(hash) = name.to_str().and_then(|name| name.strip_suffix(".txt"))
            && hash.len() == 64
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            batch.push(hash.to_owned());
            if batch.len() == ORPHAN_SWEEP_BATCH_SIZE {
                visit(std::mem::take(&mut batch))?;
                batch.reserve(ORPHAN_SWEEP_BATCH_SIZE);
            }
        }
        Ok(())
    })?;
    if !batch.is_empty() {
        if is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "notification payload enumeration cancelled",
            ));
        }
        visit(batch)?;
    }
    Ok(())
}

/// Remove one candidate batch after its caller has excluded hashes retained
/// by the current Timeline projection.
pub(crate) fn remove_payload_hashes(
    session: &crate::session::storage::ContainedDirectory,
    hashes: &[String],
) -> io::Result<usize> {
    let directory = match session.open_relative(
        Path::new(ARTIFACT_DIRECTORY),
        "notification payload directory",
        false,
    ) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut removed = 0usize;
    for hash in hashes {
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "notification payload hash is invalid",
            ));
        }
        let name = format!("{hash}.txt");
        match directory.remove_file(std::ffi::OsStr::new(&name), false) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if removed > 0 {
        directory.sync()?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_round_trip_is_content_addressed() {
        let temp = tempfile::tempdir().unwrap();
        let session = crate::session::storage::ContainedDirectory::open(
            temp.path(),
            Path::new(""),
            "notification test session",
            false,
        )
        .unwrap();
        let first = write_payload(&session, "terminal result").unwrap();
        let second = write_payload(&session, "terminal result").unwrap();
        assert_eq!(first, second);
        assert_eq!(read_payload(&session, &first).unwrap(), "terminal result");
    }

    #[test]
    fn orphan_cleanup_keeps_only_timeline_referenced_payloads() {
        let temp = tempfile::tempdir().unwrap();
        let session = crate::session::storage::ContainedDirectory::open(
            temp.path(),
            Path::new(""),
            "notification test session",
            false,
        )
        .unwrap();
        let retained = write_payload(&session, "pending receipt").unwrap();
        let orphaned = write_payload(&session, "write before failed admission").unwrap();
        let artifact_dir = temp.path().join(ARTIFACT_DIRECTORY);
        std::fs::write(artifact_dir.join("unrelated.file"), b"leave me").unwrap();

        let mut candidates = Vec::new();
        visit_payload_hash_batches(
            &session,
            || false,
            |batch| {
                candidates.extend(batch);
                Ok(())
            },
        )
        .unwrap();
        candidates.retain(|hash| hash != &retained.blake3);
        assert_eq!(remove_payload_hashes(&session, &candidates).unwrap(), 1);
        assert_eq!(
            read_payload(&session, &retained).unwrap(),
            "pending receipt"
        );
        assert!(matches!(
            read_payload(&session, &orphaned),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        ));
        assert_eq!(
            std::fs::read(artifact_dir.join("unrelated.file")).unwrap(),
            b"leave me"
        );
    }

    #[test]
    fn payload_hash_stream_is_bounded_and_unknown_files_do_not_starve_orphans() {
        let temp = tempfile::tempdir().unwrap();
        let session = crate::session::storage::ContainedDirectory::open(
            temp.path(),
            Path::new(""),
            "notification test session",
            false,
        )
        .unwrap();
        let artifact_dir = temp.path().join(ARTIFACT_DIRECTORY);
        std::fs::create_dir_all(&artifact_dir).unwrap();
        let mut expected = std::collections::BTreeSet::new();
        for index in 0..=ORPHAN_SWEEP_BATCH_SIZE {
            std::fs::write(artifact_dir.join(format!("unknown-{index}")), b"keep").unwrap();
            let hash = format!("{index:064x}");
            std::fs::write(artifact_dir.join(format!("{hash}.txt")), b"orphan").unwrap();
            expected.insert(hash);
        }

        let mut batches = Vec::new();
        visit_payload_hash_batches(
            &session,
            || false,
            |batch| {
                assert!(batch.len() <= ORPHAN_SWEEP_BATCH_SIZE);
                batches.push(batch);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(
            batches
                .concat()
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            expected
        );
    }

    #[test]
    fn payload_hash_stream_stops_inside_an_unknown_file_tail_when_cancelled() {
        let temp = tempfile::tempdir().unwrap();
        let session = crate::session::storage::ContainedDirectory::open(
            temp.path(),
            Path::new(""),
            "notification cancellation test session",
            false,
        )
        .unwrap();
        let artifact_dir = temp.path().join(ARTIFACT_DIRECTORY);
        std::fs::create_dir_all(&artifact_dir).unwrap();
        for index in 0..300 {
            std::fs::write(artifact_dir.join(format!("unknown-{index}")), b"keep").unwrap();
        }

        let mut visited = 0usize;
        let error = visit_payload_hash_batches(
            &session,
            || {
                visited += 1;
                visited > 8
            },
            |_| panic!("unknown files must not produce a payload batch"),
        )
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(visited, 9);
    }
}

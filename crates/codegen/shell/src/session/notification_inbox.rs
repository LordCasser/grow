//! Durable notification payload artifacts.
//!
//! Timeline owns receipt, ordering and consumption. This module owns only the
//! immutable bytes referenced by those facts; it is not an inbox or a second
//! delivery-state store.

use std::io;
use std::path::Path;

const ARTIFACT_DIRECTORY: &str = "artifacts/notifications";

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

/// Reclaim well-formed payload artifacts that have no pending Timeline
/// receipt. The Timeline projection is the only liveness authority; unknown
/// files are left untouched so garbage collection never broadens its scope.
pub(crate) fn sweep_orphaned_payloads(
    session: &crate::session::storage::ContainedDirectory,
    retained_hashes: &std::collections::BTreeSet<String>,
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
    for name in directory.list_names()? {
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(hash) = name.strip_suffix(".txt") else {
            continue;
        };
        if hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || retained_hashes.contains(hash)
        {
            continue;
        }
        match directory.remove_file(std::ffi::OsStr::new(name), false) {
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
    fn orphan_sweep_keeps_only_timeline_referenced_payloads() {
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

        let retained_hashes = std::collections::BTreeSet::from([retained.blake3.clone()]);
        assert_eq!(
            sweep_orphaned_payloads(&session, &retained_hashes).unwrap(),
            1
        );
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
}

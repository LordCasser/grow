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
}

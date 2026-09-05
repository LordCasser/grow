//! Immutable provider bodies referenced by the session Timeline. These bytes
//! explain historical requests and never hydrate native continuation state.

use super::storage::ContainedDirectory;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io, path::Path, sync::Arc};

pub(crate) const CHUNK_BYTES: usize = 64 * 1024;

const DIRECTORY: &str = "artifacts/sampling";
const SCOPE: &str = "sampling_evidence";
const LIMIT: u64 = sampler::audit::MAX_RESPONSE_EVIDENCE_BYTES as u64;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    owner: Value,
    kind: String,
    metadata: Value,
    bytes: u64,
    chunks: Vec<BodyChunk>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyChunk {
    blake3: String,
    bytes: u64,
}

fn write_body(directory: &ContainedDirectory, body: &[u8]) -> io::Result<Vec<BodyChunk>> {
    let mut written = std::collections::BTreeSet::new();
    body.chunks(CHUNK_BYTES)
        .map(|bytes| {
            let hash = blake3::hash(bytes).to_hex().to_string();
            if written.insert(hash.clone()) {
                super::persistence::write_immutable_blob_to_directory(
                    directory,
                    &Path::new(DIRECTORY).join(format!("{hash}.bin")),
                    bytes,
                )?;
            }
            Ok(BodyChunk {
                blake3: hash,
                bytes: bytes.len() as u64,
            })
        })
        .collect()
}

pub(crate) fn sink(
    directory: Arc<ContainedDirectory>,
    state: chat_state::ChatStateHandle,
    owner: Value,
    request_id: Option<String>,
) -> sampler::audit::EvidenceSink {
    Arc::new(move |evidence| {
        let directory = directory.clone();
        let state = state.clone();
        let mut owner = owner.clone();
        if evidence.kind != "request" {
            if let Some(owner) = owner.as_object_mut() {
                owner.remove("source_projection");
            }
        }
        let request_id = request_id.clone();
        Box::pin(async move {
            let record = tokio::task::spawn_blocking(move || {
                let bytes = evidence.body.len() as u64;
                if bytes > LIMIT {
                    return Err(io::Error::other("sampling evidence exceeds its limit"));
                }
                let chunks = write_body(&directory, &evidence.body)?;
                Ok::<_, io::Error>(Record {
                    owner,
                    kind: evidence.kind.into(),
                    metadata: evidence.metadata,
                    bytes,
                    chunks,
                })
            })
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
            let retry = if record.kind == "retry" {
                request_id.map(|id| chat_state::RequestEvent::Retrying {
                    id,
                    attempt: record.metadata["attempt"].as_u64().unwrap_or(0) as u32,
                    max_retries: record.metadata["max_retries"].as_u64().unwrap_or(0) as u32,
                    reason: record.metadata["error"]["message"]
                        .as_str()
                        .unwrap_or("retry")
                        .to_owned(),
                })
            } else {
                None
            };
            state
                .record_timeline_event_durably(chat_state::TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: SCOPE.into(),
                        name: record.kind.clone(),
                        turn: None,
                        step: None,
                        data: Some(
                            serde_json::to_value(record).map_err(|error| error.to_string())?,
                        ),
                    },
                ))
                .await
                .map_err(|error| error.to_string())?;
            if let Some(retry) = retry {
                state
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Request(retry))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        })
    })
}

/// Restore/import must reject missing or altered historical evidence even
/// though it is never used to resume a provider's native state.
pub(crate) fn verify(
    directory: &ContainedDirectory,
    timeline: &chat_state::Timeline,
) -> io::Result<()> {
    let mut verified = std::collections::BTreeSet::new();
    for event in timeline.events() {
        let chat_state::TimelineEventKind::Observation(observation) = &event.kind else {
            continue;
        };
        if observation.scope != SCOPE {
            continue;
        }
        let record = decode_record(observation)?;
        for chunk in record.chunks {
            if !verified.insert((chunk.blake3.clone(), chunk.bytes)) {
                continue;
            }
            let artifacts =
                directory.open_relative(Path::new(DIRECTORY), "sampling evidence", false)?;
            let bytes = artifacts.read_bounded(
                std::ffi::OsStr::new(&format!("{}.bin", chunk.blake3)),
                "sampling evidence",
                CHUNK_BYTES as u64,
            )?;
            if bytes.len() as u64 != chunk.bytes
                || blake3::hash(&bytes).to_hex().as_str() != chunk.blake3
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "sampling evidence content mismatch",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn referenced_hashes(
    timeline: &chat_state::Timeline,
) -> io::Result<std::collections::BTreeSet<String>> {
    let mut hashes = std::collections::BTreeSet::new();
    for event in timeline.events() {
        let chat_state::TimelineEventKind::Observation(observation) = &event.kind else {
            continue;
        };
        if observation.scope != SCOPE {
            continue;
        }
        let record = decode_record(observation)?;
        hashes.extend(record.chunks.into_iter().map(|chunk| chunk.blake3));
    }
    Ok(hashes)
}

fn decode_record(observation: &chat_state::ObservationEvent) -> io::Result<Record> {
    let record: Record = serde_json::from_value(observation.data.clone().unwrap_or(Value::Null))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let expected_chunks = record.bytes.div_ceil(CHUNK_BYTES as u64);
    if record.bytes > LIMIT
        || record.chunks.len() as u64 != expected_chunks
        || record.kind != observation.name
        || !matches!(record.kind.as_str(), "request" | "response" | "retry")
        || record.chunks.iter().enumerate().any(|(index, chunk)| {
            let expected_bytes =
                (record.bytes - index as u64 * CHUNK_BYTES as u64).min(CHUNK_BYTES as u64);
            chunk.bytes != expected_bytes
                || chunk.blake3.len() != 64
                || !chunk
                    .blake3
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid sampling evidence reference",
        ));
    }
    Ok(record)
}

#[cfg(test)]
#[test]
fn append_only_request_bodies_reuse_prefix_chunks() {
    let temp = tempfile::tempdir().unwrap();
    let directory =
        ContainedDirectory::open(temp.path(), Path::new(""), "test session", false).unwrap();
    let mut body = vec![b'a'; CHUNK_BYTES];
    body.extend(vec![b'b'; CHUNK_BYTES]);
    body.extend_from_slice(b"first tail");
    let first = write_body(&directory, &body).unwrap();
    body.extend_from_slice(b" and new context");
    let second = write_body(&directory, &body).unwrap();
    assert_eq!(first[0].blake3, second[0].blake3);
    assert_eq!(first[1].blake3, second[1].blake3);
    assert_ne!(first[2].blake3, second[2].blake3);
    assert_eq!(
        std::fs::read_dir(temp.path().join(DIRECTORY))
            .unwrap()
            .count(),
        4
    );
    let reconstructed: Vec<u8> = second
        .iter()
        .flat_map(|chunk| {
            std::fs::read(
                temp.path()
                    .join(DIRECTORY)
                    .join(format!("{}.bin", chunk.blake3)),
            )
            .unwrap()
        })
        .collect();
    assert_eq!(reconstructed, body);
}

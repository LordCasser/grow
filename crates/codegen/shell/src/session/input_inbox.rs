//! Immutable, content-addressed payloads for durable human input admission.
//!
//! Timeline owns admission, routing, consumption, and dismissal. This module
//! owns only the recoverable JSON bytes referenced by `InputEvent::Submitted`.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

const ARTIFACT_DIRECTORY: &str = "artifacts/inputs";
const ORPHAN_SWEEP_BATCH_SIZE: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum InputPayload {
    Prompt {
        prompt_id: String,
        prompt_blocks: Vec<agent_client_protocol::schema::v1::ContentBlock>,
        client_identifier: Option<String>,
        screen_mode: Option<String>,
        verbatim: bool,
        json_schema: Option<serde_json::Value>,
        origin: crate::session::PromptOrigin,
        turn_kind: crate::session::TurnKind,
        queue: Option<StoredQueueEntry>,
    },
}

impl InputPayload {
    /// Exact user-authored payload projection used by UserPromptSubmit Hooks.
    /// Plain text preserves the established wire shape. Structured inputs use
    /// canonical JSON so images, block ordering, verbatim mode, and output
    /// schema cannot differ between the reviewed artifact and later execution.
    pub(crate) fn hook_prompt(&self) -> String {
        let Self::Prompt {
            prompt_blocks,
            verbatim,
            json_schema,
            ..
        } = self;
        if json_schema.is_none()
            && !*verbatim
            && prompt_blocks.iter().all(|block| {
                matches!(
                    block,
                    agent_client_protocol::schema::v1::ContentBlock::Text(_)
                )
            })
        {
            return prompt_blocks
                .iter()
                .filter_map(|block| match block {
                    agent_client_protocol::schema::v1::ContentBlock::Text(text) => {
                        Some(text.text.as_str())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n");
        }
        serde_json::to_string(&serde_json::json!({
            "promptBlocks": prompt_blocks,
            "verbatim": verbatim,
            "jsonSchema": json_schema,
        }))
        .expect("ACP input payload must serialize")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredQueueEntry {
    pub id: String,
    pub version: u64,
    pub owner: Option<String>,
    pub last_editor: Option<String>,
    pub kind: String,
    pub text: String,
    pub combined_texts: Option<Vec<String>>,
}

impl From<&crate::session::prompt_queue::QueueEntryMeta> for StoredQueueEntry {
    fn from(value: &crate::session::prompt_queue::QueueEntryMeta) -> Self {
        Self {
            id: value.id.clone(),
            version: value.version,
            owner: value.owner.clone(),
            last_editor: value.last_editor.clone(),
            kind: value.kind.clone(),
            text: value.text.clone(),
            combined_texts: value.combined_texts.clone(),
        }
    }
}

impl From<StoredQueueEntry> for crate::session::prompt_queue::QueueEntryMeta {
    fn from(value: StoredQueueEntry) -> Self {
        Self {
            id: value.id,
            version: value.version,
            owner: value.owner,
            last_editor: value.last_editor,
            kind: value.kind,
            text: value.text,
            combined_texts: value.combined_texts,
        }
    }
}

pub(crate) fn write_payload(
    session: &crate::session::storage::ContainedDirectory,
    payload: &InputPayload,
) -> io::Result<chat_state::InputPayloadRef> {
    let bytes = serde_json::to_vec(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if bytes.is_empty() || bytes.len() as u64 > chat_state::MAX_INPUT_PAYLOAD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input payload is empty or exceeds its byte limit",
        ));
    }
    let hash = blake3::hash(&bytes).to_hex().to_string();
    crate::session::persistence::write_immutable_blob_to_directory(
        session,
        &Path::new(ARTIFACT_DIRECTORY).join(format!("{hash}.json")),
        &bytes,
    )?;
    Ok(chat_state::InputPayloadRef {
        blake3: hash,
        bytes: bytes.len() as u64,
    })
}

pub(crate) fn read_payload(
    session: &crate::session::storage::ContainedDirectory,
    payload: &chat_state::InputPayloadRef,
) -> io::Result<InputPayload> {
    validate_ref(payload)?;
    let directory = session.open_relative(
        Path::new(ARTIFACT_DIRECTORY),
        "input payload directory",
        false,
    )?;
    let name = format!("{}.json", payload.blake3);
    let bytes = directory.read_bounded(
        std::ffi::OsStr::new(&name),
        "input payload",
        chat_state::MAX_INPUT_PAYLOAD_BYTES,
    )?;
    if bytes.len() as u64 != payload.bytes
        || blake3::hash(&bytes).to_hex().as_str() != payload.blake3
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input payload artifact does not match its Timeline reference",
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(crate) fn validate_submitted_payloads(
    session: &crate::session::storage::ContainedDirectory,
    events: &[chat_state::TimelineEvent],
) -> io::Result<()> {
    for event in events {
        let chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted {
            input_id,
            intent,
            payload_ref,
        }) = &event.kind
        else {
            continue;
        };
        let payload = read_payload(session, payload_ref).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("input {input_id} payload is unavailable or invalid: {error}"),
            )
        })?;
        if !payload_matches_intent(*intent, &payload) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("input {input_id} intent does not match its payload artifact"),
            ));
        }
    }
    Ok(())
}

pub(crate) fn payload_matches_intent(
    intent: chat_state::InputIntent,
    payload: &InputPayload,
) -> bool {
    matches!(
        (intent, payload),
        (
            chat_state::InputIntent::Prompt
                | chat_state::InputIntent::Followup
                | chat_state::InputIntent::Steer,
            InputPayload::Prompt { .. }
        )
    )
}

fn validate_ref(payload: &chat_state::InputPayloadRef) -> io::Result<()> {
    if payload.blake3.len() != 64
        || !payload.blake3.bytes().all(|byte| byte.is_ascii_hexdigit())
        || payload.bytes == 0
        || payload.bytes > chat_state::MAX_INPUT_PAYLOAD_BYTES
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input payload reference is invalid",
        ));
    }
    Ok(())
}

pub(crate) fn visit_payload_hash_batches(
    session: &crate::session::storage::ContainedDirectory,
    mut is_cancelled: impl FnMut() -> bool,
    mut visit: impl FnMut(Vec<String>) -> io::Result<()>,
) -> io::Result<()> {
    let directory = match session.open_relative(
        Path::new(ARTIFACT_DIRECTORY),
        "input payload directory",
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
                "input payload enumeration cancelled",
            ));
        }
        if let Some(hash) = name.to_str().and_then(|name| name.strip_suffix(".json"))
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
    if !batch.is_empty() && !is_cancelled() {
        visit(batch)?;
    }
    Ok(())
}

pub(crate) fn remove_payload_hashes(
    session: &crate::session::storage::ContainedDirectory,
    hashes: &[String],
) -> io::Result<usize> {
    let directory = match session.open_relative(
        Path::new(ARTIFACT_DIRECTORY),
        "input payload directory",
        false,
    ) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut removed = 0;
    for hash in hashes {
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "input payload hash is invalid",
            ));
        }
        match directory.remove_file(std::ffi::OsStr::new(&format!("{hash}.json")), false) {
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

    fn prompt_payload(text: &str) -> InputPayload {
        InputPayload::Prompt {
            prompt_id: "prompt-1".into(),
            prompt_blocks: vec![agent_client_protocol::schema::v1::ContentBlock::Text(
                agent_client_protocol::schema::v1::TextContent::new(text),
            )],
            client_identifier: None,
            screen_mode: None,
            verbatim: false,
            json_schema: None,
            origin: crate::session::PromptOrigin::User,
            turn_kind: crate::session::TurnKind::User,
            queue: None,
        }
    }

    #[test]
    fn hook_prompt_reviews_raw_text_not_display_metadata() {
        let mut text = agent_client_protocol::schema::v1::TextContent::new(
            "expanded instruction with executable detail",
        );
        text.meta = Some(
            [("displayText".to_string(), serde_json::json!("/compact"))]
                .into_iter()
                .collect(),
        );
        let mut payload = prompt_payload("unused");
        let InputPayload::Prompt { prompt_blocks, .. } = &mut payload;
        *prompt_blocks = vec![agent_client_protocol::schema::v1::ContentBlock::Text(text)];

        assert_eq!(
            payload.hook_prompt(),
            "expanded instruction with executable detail"
        );
    }

    #[test]
    fn structured_hook_prompt_binds_images_verbatim_and_schema() {
        let mut payload = prompt_payload("describe this");
        let InputPayload::Prompt {
            prompt_blocks,
            verbatim,
            json_schema,
            ..
        } = &mut payload;
        prompt_blocks.push(agent_client_protocol::schema::v1::ContentBlock::Image(
            agent_client_protocol::schema::v1::ImageContent::new("aW1hZ2U=", "image/png"),
        ));
        *verbatim = true;
        *json_schema = Some(serde_json::json!({
            "type": "object",
            "required": ["caption"]
        }));

        let reviewed: serde_json::Value = serde_json::from_str(&payload.hook_prompt()).unwrap();
        assert_eq!(reviewed["verbatim"], true);
        assert_eq!(reviewed["promptBlocks"][0]["text"], "describe this");
        assert_eq!(reviewed["promptBlocks"][1]["mimeType"], "image/png");
        assert_eq!(reviewed["promptBlocks"][1]["data"], "aW1hZ2U=");
        assert_eq!(reviewed["jsonSchema"]["required"][0], "caption");
    }

    #[test]
    fn payload_round_trip_is_content_addressed() {
        let temp = tempfile::tempdir().unwrap();
        let session = crate::session::storage::ContainedDirectory::open(
            temp.path(),
            Path::new(""),
            "input test session",
            false,
        )
        .unwrap();
        let payload = prompt_payload("status");
        let first = write_payload(&session, &payload).unwrap();
        let second = write_payload(&session, &payload).unwrap();
        assert_eq!(first, second);
        assert!(matches!(
            read_payload(&session, &first).unwrap(),
            InputPayload::Prompt { .. }
        ));
    }

    #[test]
    fn submitted_payload_validation_checks_every_reference_and_intent() {
        let temp = tempfile::tempdir().unwrap();
        let session = crate::session::storage::ContainedDirectory::open(
            temp.path(),
            Path::new(""),
            "input test session",
            false,
        )
        .unwrap();
        let payload_ref = write_payload(&session, &prompt_payload("status")).unwrap();
        let submitted = |input_id: &str, intent, payload_ref| chat_state::TimelineEvent {
            version: chat_state::TIMELINE_SCHEMA_VERSION,
            seq: chat_state::EventSeq::new(1),
            at_ms: 1,
            kind: chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted {
                input_id: input_id.into(),
                intent,
                payload_ref,
            }),
        };

        assert!(
            validate_submitted_payloads(
                &session,
                &[submitted(
                    "prompt",
                    chat_state::InputIntent::Prompt,
                    payload_ref.clone(),
                )],
            )
            .is_ok()
        );
        assert!(matches!(
            validate_submitted_payloads(
                &session,
                &[submitted(
                    "missing",
                    chat_state::InputIntent::Prompt,
                    chat_state::InputPayloadRef {
                        blake3: "0".repeat(64),
                        bytes: 1,
                    },
                )],
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn orphan_sweep_removes_only_well_formed_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let session = crate::session::storage::ContainedDirectory::open(
            temp.path(),
            Path::new(""),
            "input test session",
            false,
        )
        .unwrap();
        let retained = write_payload(&session, &prompt_payload("status")).unwrap();
        let orphan = write_payload(&session, &prompt_payload("help")).unwrap();
        let mut hashes = Vec::new();
        visit_payload_hash_batches(
            &session,
            || false,
            |batch| {
                hashes.extend(batch);
                Ok(())
            },
        )
        .unwrap();
        hashes.retain(|hash| hash != &retained.blake3);
        assert_eq!(remove_payload_hashes(&session, &hashes).unwrap(), 1);
        assert!(read_payload(&session, &retained).is_ok());
        assert!(matches!(
            read_payload(&session, &orphan),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        ));
    }
}

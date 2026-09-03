//! Immutable, content-addressed payloads for durable human input admission.
//!
//! Timeline owns admission, routing, consumption, and dismissal. This module
//! owns only the recoverable JSON bytes referenced by `InputEvent::Submitted`.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::Path;

use agent_client_protocol::schema::v1::{ContentBlock, ImageContent};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

const ARTIFACT_DIRECTORY: &str = "artifacts/inputs";
const IMAGE_DIRECTORY: &str = "artifacts/inputs/images";
const MAX_IMAGE_BLOB_BYTES: u64 = 64 * 1024 * 1024;
const MAX_INPUT_IMAGE_BYTES: u64 = MAX_IMAGE_BLOB_BYTES;
const MAX_INPUT_IMAGES: usize = 16;
const ORPHAN_SWEEP_BATCH_SIZE: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum InputPayload<Block = ContentBlock> {
    Prompt {
        prompt_id: String,
        prompt_blocks: Vec<Block>,
        client_identifier: Option<String>,
        screen_mode: Option<String>,
        verbatim: bool,
        json_schema: Option<serde_json::Value>,
        origin: crate::session::PromptOrigin,
        turn_kind: crate::session::TurnKind,
        queue: Option<StoredQueueEntry>,
    },
}

// Only this codec knows about references. Runtime callers and Hooks continue
// to receive the complete, unchanged ACP blocks (including annotations/_meta).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum StoredBlock {
    Image(ImageReference),
    Inline(ContentBlock),
}

impl<'de> Deserialize<'de> for StoredBlock {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        // Presence selects the private format. A damaged reference must never
        // fall through to ACP's permissive unknown-field handling as text.
        if value.get("input_image").is_some() {
            serde_json::from_value(value)
                .map(Self::Image)
                .map_err(serde::de::Error::custom)
        } else {
            serde_json::from_value(value)
                .map(Self::Inline)
                .map_err(serde::de::Error::custom)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageReference {
    input_image: ImageBlobRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageBlobRef {
    blake3: String,
    bytes: u64,
}

impl<Block> InputPayload<Block> {
    fn try_map_blocks<T>(
        self,
        map: impl FnMut(Block) -> io::Result<T>,
    ) -> io::Result<InputPayload<T>> {
        let Self::Prompt {
            prompt_id,
            prompt_blocks,
            client_identifier,
            screen_mode,
            verbatim,
            json_schema,
            origin,
            turn_kind,
            queue,
        } = self;
        Ok(InputPayload::Prompt {
            prompt_id,
            prompt_blocks: prompt_blocks
                .into_iter()
                .map(map)
                .collect::<io::Result<_>>()?,
            client_identifier,
            screen_mode,
            verbatim,
            json_schema,
            origin,
            turn_kind,
            queue,
        })
    }
}

/// Streaming count/write: reject before growing a JSON buffer beyond its cap.
struct LimitedWriter<W> {
    inner: W,
    remaining: u64,
}

impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "input artifact exceeds its byte limit",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn serialized_size(value: &impl Serialize, limit: u64) -> io::Result<u64> {
    let mut writer = LimitedWriter {
        inner: io::sink(),
        remaining: limit,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    Ok(limit - writer.remaining)
}

fn bounded_json(value: &impl Serialize, limit: u64) -> io::Result<Vec<u8>> {
    let mut writer = LimitedWriter {
        inner: Vec::new(),
        remaining: limit,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    Ok(writer.inner)
}

/// Check all encoded sizes/counts before decoding even the first image. JSON
/// escaping and Base64 are included in the aggregate budget, not just raw bytes.
fn validate_payload_bounds(payload: &InputPayload) -> io::Result<()> {
    let InputPayload::Prompt { prompt_blocks, .. } = payload;
    validate_image_sizes(prompt_blocks.iter().filter_map(|block| match block {
        ContentBlock::Image(image) => Some(image),
        _ => None,
    }))?;
    let mut inline_bytes = 0;
    for block in prompt_blocks {
        if matches!(block, ContentBlock::Image(_)) {
            inline_bytes += serialized_size(block, MAX_IMAGE_BLOB_BYTES + 32)?;
        }
    }
    let total = serialized_size(
        payload,
        MAX_INPUT_IMAGE_BYTES + chat_state::MAX_INPUT_PAYLOAD_BYTES + 512,
    )?;
    if total - inline_bytes > chat_state::MAX_INPUT_PAYLOAD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input manifest exceeds its byte limit",
        ));
    }
    for block in prompt_blocks {
        if let ContentBlock::Image(image) = block {
            validate_image(image)?;
        }
    }
    Ok(())
}

/// The ingress calls this before cloning an incoming image snapshot.
pub(crate) fn validate_image_sizes<'a>(
    images: impl IntoIterator<Item = &'a ImageContent>,
) -> io::Result<()> {
    let mut count = 0;
    let mut total = 0;
    for image in images {
        count += 1;
        if count > MAX_INPUT_IMAGES || image.data.len() as u64 > MAX_IMAGE_BLOB_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "input image count or byte limit exceeded",
            ));
        }
        total += serialized_size(image, MAX_IMAGE_BLOB_BYTES)?;
        if total > MAX_INPUT_IMAGE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "input image count or total encoded byte limit exceeded",
            ));
        }
    }
    Ok(())
}

fn validate_image(image: &ImageContent) -> io::Result<()> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&image.data)
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "input image has invalid Base64",
            )
        })?;
    let (width, height, format) = tools::util::image_validate::validate_image_bytes_unrestricted(
        &raw, false,
    )
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "input image has an invalid header",
        )
    })?;
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height)
            > crate::session::image_normalize::MAX_VISION_TOTAL_PX
        || !tools::util::image_validate::format_structurally_complete(format, &raw)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input image is incomplete or exceeds the decoded pixel limit",
        ));
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("input image {hash} unavailable: {source}")]
struct AttachmentError {
    hash: String,
    source: io::Error,
}

pub(crate) fn is_attachment_error(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|error| error.is::<AttachmentError>())
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
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct HookPrompt<'a> {
            prompt_blocks: &'a [ContentBlock],
            verbatim: bool,
            json_schema: &'a Option<serde_json::Value>,
        }
        serde_json::to_string(&HookPrompt {
            prompt_blocks,
            verbatim: *verbatim,
            json_schema,
        })
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
    validate_payload_bounds(payload)?;
    let stored = payload.clone().try_map_blocks(|block| match block {
        ContentBlock::Image(image) => {
            let bytes = bounded_json(&image, MAX_IMAGE_BLOB_BYTES)?;
            let hash = blake3::hash(&bytes).to_hex().to_string();
            crate::session::persistence::write_immutable_blob_to_directory(
                session,
                &Path::new(IMAGE_DIRECTORY).join(format!("{hash}.json")),
                &bytes,
            )?;
            Ok(StoredBlock::Image(ImageReference {
                input_image: ImageBlobRef {
                    blake3: hash,
                    bytes: bytes.len() as u64,
                },
            }))
        }
        block => Ok(StoredBlock::Inline(block)),
    })?;
    let bytes = bounded_json(&stored, chat_state::MAX_INPUT_PAYLOAD_BYTES)?;
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
    let stored = read_manifest(session, payload)?;
    stored.try_map_blocks(|block| match block {
        StoredBlock::Inline(block) => Ok(block),
        StoredBlock::Image(reference) => {
            let reference = reference.input_image;
            read_image(session, &reference)
                .map(ContentBlock::Image)
                .map_err(|source| {
                    io::Error::new(
                        source.kind(),
                        AttachmentError {
                            hash: reference.blake3,
                            source,
                        },
                    )
                })
        }
    })
}

fn read_image(
    session: &crate::session::storage::ContainedDirectory,
    reference: &ImageBlobRef,
) -> io::Result<ImageContent> {
    let directory =
        session.open_relative(Path::new(IMAGE_DIRECTORY), "input image directory", false)?;
    let bytes = directory.read_bounded(
        std::ffi::OsStr::new(&format!("{}.json", reference.blake3)),
        "input image",
        reference.bytes,
    )?;
    if bytes.len() as u64 != reference.bytes
        || blake3::hash(&bytes).to_hex().as_str() != reference.blake3
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input image does not match its manifest reference",
        ));
    }
    let image = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    validate_image(&image)?;
    Ok(image)
}

fn read_manifest(
    session: &crate::session::storage::ContainedDirectory,
    payload: &chat_state::InputPayloadRef,
) -> io::Result<InputPayload<StoredBlock>> {
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
    decode_manifest(&bytes)
}

fn decode_manifest(bytes: &[u8]) -> io::Result<InputPayload<StoredBlock>> {
    let stored: InputPayload<StoredBlock> = serde_json::from_slice(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let InputPayload::Prompt { prompt_blocks, .. } = &stored;
    let mut total = 0;
    let mut count = 0;
    for block in prompt_blocks {
        let size = match block {
            StoredBlock::Image(reference) => {
                let reference = &reference.input_image;
                if !valid_hash(&reference.blake3)
                    || reference.bytes == 0
                    || reference.bytes > MAX_IMAGE_BLOB_BYTES
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "input image reference is invalid",
                    ));
                }
                reference.bytes
            }
            // Existing inline ACP snapshots were admitted under the original
            // 1 MiB manifest/hash contract. New ingress rules must not turn an
            // old consumed input into a new session-wide startup failure.
            _ => continue,
        };
        total += size;
        count += 1;
        if total > MAX_INPUT_IMAGE_BYTES || count > MAX_INPUT_IMAGES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "input image references exceed the aggregate limit",
            ));
        }
    }
    Ok(stored)
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
        // The manifest is the integrity boundary. Unavailable image bytes do
        // not invalidate the Timeline or the already-consumed conversation.
        let payload = read_manifest(session, payload_ref).map_err(|error| {
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
        let InputPayload::Prompt { prompt_blocks, .. } = payload;
        for block in prompt_blocks {
            if let StoredBlock::Image(reference) = block
                && let Err(error) = read_image(session, &reference.input_image)
            {
                tracing::warn!(%input_id, image_hash = %reference.input_image.blake3, %error,
                    "historical input attachment unavailable; pending input will be invalidated during recovery");
            }
        }
    }
    Ok(())
}

pub(crate) fn payload_matches_intent<Block>(
    intent: chat_state::InputIntent,
    payload: &InputPayload<Block>,
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
    if !valid_hash(&payload.blake3)
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

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Called under input_artifact_gate after obtaining ALL Submitted hashes.
/// A broken manifest stops GC: guessing its edges could erase audit evidence.
pub(crate) fn reconcile_image_blobs(
    session: &crate::session::storage::ContainedDirectory,
    manifests: &BTreeSet<String>,
    mut is_cancelled: impl FnMut() -> bool,
) -> io::Result<usize> {
    let mut retained = BTreeSet::new();
    if !manifests.is_empty() {
        let directory = session.open_relative(
            Path::new(ARTIFACT_DIRECTORY),
            "input manifest directory",
            false,
        )?;
        for hash in manifests {
            if is_cancelled() {
                return Ok(0);
            }
            if !valid_hash(hash) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid retained input hash",
                ));
            }
            let bytes = directory.read_bounded(
                std::ffi::OsStr::new(&format!("{hash}.json")),
                "input manifest",
                chat_state::MAX_INPUT_PAYLOAD_BYTES,
            )?;
            if blake3::hash(&bytes).to_hex().as_str() != hash {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "retained input manifest hash mismatch",
                ));
            }
            let InputPayload::Prompt { prompt_blocks, .. } = decode_manifest(&bytes)?;
            for block in prompt_blocks {
                if let StoredBlock::Image(reference) = block {
                    retained.insert(reference.input_image.blake3);
                }
            }
        }
    }
    let directory =
        match session.open_relative(Path::new(IMAGE_DIRECTORY), "input image directory", false) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
    let mut removed = 0;
    directory.visit_names(|name| {
        if is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "input image GC cancelled",
            ));
        }
        if let Some(hash) = name.to_str().and_then(|name| name.strip_suffix(".json"))
            && valid_hash(hash)
            && !retained.contains(hash)
        {
            match directory.remove_file(&name, false) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })?;
    if removed > 0 {
        directory.sync()?;
    }
    Ok(removed)
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
pub(crate) mod tests {
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

    pub(crate) fn image_payload() -> InputPayload {
        // Incompressible, legal PNG reproduces the >1 MiB manifest incident.
        let mut seed = 0x12345678u32;
        let pixels = (0..700 * 600 * 3)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let image = image::RgbImage::from_raw(700, 600, pixels).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        assert!(png.get_ref().len() > 1_237_672);
        let mut payload = prompt_payload("describe [Image #1]");
        let InputPayload::Prompt {
            prompt_blocks,
            json_schema,
            verbatim,
            queue,
            ..
        } = &mut payload;
        prompt_blocks.push(ContentBlock::Image(
            serde_json::from_value(serde_json::json!({
                "data": base64::engine::general_purpose::STANDARD.encode(png.into_inner()),
                "mimeType": "image/png",
                "uri": "file:///original/snapshot.png",
                "annotations": {"audience": ["user"], "priority": 0.7},
                "_meta": {"grow.dev/imageDisplayNumber": 1, "test": "unchanged"}
            }))
            .unwrap(),
        ));
        prompt_blocks.push(ContentBlock::Text(
            agent_client_protocol::schema::v1::TextContent::new("after image"),
        ));
        *verbatim = true;
        *json_schema = Some(serde_json::json!({"type":"object"}));
        *queue = Some(StoredQueueEntry {
            id: "q".into(),
            version: 3,
            owner: Some("client".into()),
            last_editor: None,
            kind: "prompt".into(),
            text: "display".into(),
            combined_texts: None,
        });
        payload
    }

    fn open_test_session(path: &Path) -> crate::session::storage::ContainedDirectory {
        crate::session::storage::ContainedDirectory::open(path, Path::new(""), "input test", false)
            .unwrap()
    }

    fn image_ref(
        session: &crate::session::storage::ContainedDirectory,
        payload: &chat_state::InputPayloadRef,
    ) -> ImageBlobRef {
        let InputPayload::Prompt { prompt_blocks, .. } = read_manifest(session, payload).unwrap();
        prompt_blocks
            .into_iter()
            .find_map(|block| match block {
                StoredBlock::Image(reference) => Some(reference.input_image),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn large_image_round_trip_preserves_entire_hook_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let session = open_test_session(temp.path());
        let payload = image_payload();
        assert!(
            serde_json::to_vec(&payload).unwrap().len() as u64
                > chat_state::MAX_INPUT_PAYLOAD_BYTES
        );
        let reference = write_payload(&session, &payload).unwrap();
        assert!(reference.bytes < 2048);
        let restored = read_payload(&session, &reference).unwrap();
        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            serde_json::to_value(&restored).unwrap()
        );
        assert_eq!(payload.hook_prompt(), restored.hook_prompt());
        assert_eq!(reference, write_payload(&session, &restored).unwrap());
    }

    #[test]
    fn image_corruption_is_separate_from_manifest_corruption() {
        let temp = tempfile::tempdir().unwrap();
        let session = open_test_session(temp.path());
        let reference = write_payload(&session, &image_payload()).unwrap();
        let image = image_ref(&session, &reference);
        let image_path = temp
            .path()
            .join(IMAGE_DIRECTORY)
            .join(format!("{}.json", image.blake3));
        std::fs::write(&image_path, b"corrupt").unwrap();
        let error = read_payload(&session, &reference).unwrap_err();
        assert!(is_attachment_error(&error));
        assert!(read_manifest(&session, &reference).is_ok());
        assert!(
            write_payload(&session, &image_payload()).is_err(),
            "immutable collision must fail closed"
        );
        std::fs::remove_file(image_path).unwrap();
        assert!(is_attachment_error(
            &read_payload(&session, &reference).unwrap_err()
        ));
        let event = chat_state::TimelineEvent {
            version: chat_state::TIMELINE_SCHEMA_VERSION,
            seq: chat_state::EventSeq::new(1),
            at_ms: 1,
            kind: chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted {
                input_id: "missing-image".into(),
                intent: chat_state::InputIntent::Prompt,
                payload_ref: reference.clone(),
            }),
        };
        assert!(
            validate_submitted_payloads(&session, &[event]).is_ok(),
            "history remains openable"
        );
        std::fs::write(
            temp.path()
                .join(ARTIFACT_DIRECTORY)
                .join(format!("{}.json", reference.blake3)),
            b"{}",
        )
        .unwrap();
        assert!(!is_attachment_error(
            &read_payload(&session, &reference).unwrap_err()
        ));
    }

    #[test]
    fn reference_limits_and_paths_are_checked_before_image_reads() {
        for input_image in [
            serde_json::Value::Null,
            serde_json::json!({"blake3":"broken", "bytes":0}),
        ] {
            assert!(
                serde_json::from_value::<StoredBlock>(serde_json::json!({
                    "input_image": input_image, "type":"text", "text":"do not silently execute"
                }))
                .is_err()
            );
        }
        let payload = prompt_payload("test")
            .try_map_blocks(|_| {
                Ok(StoredBlock::Image(ImageReference {
                    input_image: ImageBlobRef {
                        blake3: "a".repeat(64),
                        bytes: 1,
                    },
                }))
            })
            .unwrap();
        let mut value = serde_json::to_value(&payload).unwrap();
        for hash in ["../outside".to_string(), "G".repeat(64), "A".repeat(64)] {
            value["prompt_blocks"][0]["input_image"]["blake3"] = serde_json::json!(hash);
            assert!(decode_manifest(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        value["prompt_blocks"][0]["input_image"]["blake3"] = serde_json::json!("a".repeat(64));
        for bytes in [0, MAX_IMAGE_BLOB_BYTES + 1, u64::MAX] {
            value["prompt_blocks"][0]["input_image"]["bytes"] = serde_json::json!(bytes);
            assert!(decode_manifest(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        value["prompt_blocks"][0]["input_image"]["bytes"] = serde_json::json!(MAX_IMAGE_BLOB_BYTES);
        let block = value["prompt_blocks"][0].clone();
        value["prompt_blocks"] = serde_json::json!([block.clone(), block]);
        assert!(decode_manifest(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut block = value["prompt_blocks"][0].clone();
        block["input_image"]["bytes"] = serde_json::json!(1);
        value["prompt_blocks"] = serde_json::json!(vec![block; MAX_INPUT_IMAGES + 1]);
        assert!(decode_manifest(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn historical_inline_images_keep_the_manifest_integrity_contract() {
        let temp = tempfile::tempdir().unwrap();
        let session = open_test_session(temp.path());
        let mut payload = prompt_payload("old input");
        let InputPayload::Prompt { prompt_blocks, .. } = &mut payload;
        *prompt_blocks =
            vec![
                ContentBlock::Image(ImageContent::new("historical invalid data", "image/png"));
                MAX_INPUT_IMAGES + 1
            ];
        // The original writer stored this shape without attachment validation.
        let bytes = bounded_json(&payload, chat_state::MAX_INPUT_PAYLOAD_BYTES).unwrap();
        let reference = chat_state::InputPayloadRef {
            blake3: blake3::hash(&bytes).to_hex().to_string(),
            bytes: bytes.len() as u64,
        };
        crate::session::persistence::write_immutable_blob_to_directory(
            &session,
            &Path::new(ARTIFACT_DIRECTORY).join(format!("{}.json", reference.blake3)),
            &bytes,
        )
        .unwrap();
        assert_eq!(
            read_payload(&session, &reference).unwrap().hook_prompt(),
            payload.hook_prompt()
        );
        assert!(
            write_payload(&session, &payload).is_err(),
            "new admissions remain strict"
        );
    }

    #[test]
    fn bounded_writer_and_invalid_image_do_not_publish_a_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let session = open_test_session(temp.path());
        assert!(
            write_payload(
                &session,
                &prompt_payload(&"x".repeat(chat_state::MAX_INPUT_PAYLOAD_BYTES as usize))
            )
            .is_err()
        );
        let payload = prompt_payload("x")
            .try_map_blocks(|_| {
                Ok(ContentBlock::Image(ImageContent::new(
                    "invalid!",
                    "image/png",
                )))
            })
            .unwrap();
        assert!(write_payload(&session, &payload).is_err());
        assert!(!temp.path().join(ARTIFACT_DIRECTORY).exists());
        let mut writer = LimitedWriter {
            inner: Vec::new(),
            remaining: 2,
        };
        assert!(writer.write_all(b"123").is_err());
        assert!(writer.inner.is_empty());
    }

    #[test]
    fn gc_keeps_reachable_images_and_removes_pre_manifest_crash_blobs() {
        let temp = tempfile::tempdir().unwrap();
        let session = open_test_session(temp.path());
        let payload = image_payload();
        let retained = write_payload(&session, &payload).unwrap();
        let mut orphan = payload.clone();
        let InputPayload::Prompt { prompt_blocks, .. } = &mut orphan;
        let ContentBlock::Image(image) = &mut prompt_blocks[1] else {
            panic!()
        };
        image.uri = Some("orphan".into());
        let orphan = write_payload(&session, &orphan).unwrap();
        let orphan_image = image_ref(&session, &orphan);
        remove_payload_hashes(&session, &[orphan.blake3]).unwrap();
        assert_eq!(
            reconcile_image_blobs(&session, &BTreeSet::from([retained.blake3.clone()]), || {
                false
            })
            .unwrap(),
            1
        );
        assert!(
            !temp
                .path()
                .join(IMAGE_DIRECTORY)
                .join(format!("{}.json", orphan_image.blake3))
                .exists()
        );
        assert_eq!(
            read_payload(&session, &retained).unwrap().hook_prompt(),
            payload.hook_prompt()
        );
        assert_eq!(
            reconcile_image_blobs(&session, &BTreeSet::new(), || false).unwrap(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn image_read_rejects_symlink_even_when_target_hash_matches() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let session = open_test_session(temp.path());
        let reference = write_payload(&session, &image_payload()).unwrap();
        let image = image_ref(&session, &reference);
        let path = temp
            .path()
            .join(IMAGE_DIRECTORY)
            .join(format!("{}.json", image.blake3));
        let target = outside.path().join("image.json");
        std::fs::rename(&path, &target).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(is_attachment_error(
            &read_payload(&session, &reference).unwrap_err()
        ));
    }

    #[tokio::test]
    async fn fork_uses_materialized_images_and_trace_export_carries_input_blobs() {
        use crate::session::storage::{JsonlStorageAdapter, StorageAdapter};
        let root = tempfile::tempdir().unwrap();
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let source = crate::session::info::Info {
            id: agent_client_protocol::schema::v1::SessionId::new("input-source"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        storage
            .init_session(&source, crate::session::persistence::default_model_id())
            .await
            .unwrap();
        let directory = storage
            .open_session(&source)
            .unwrap()
            .directory()
            .try_clone()
            .unwrap();
        let payload = image_payload();
        let reference = write_payload(&directory, &payload).unwrap();
        let InputPayload::Prompt { prompt_blocks, .. } = &payload;
        let ContentBlock::Image(image) = &prompt_blocks[1] else {
            panic!()
        };
        let user = sampling_types::ConversationItem::user_with_parts(vec![
            sampling_types::ContentPart::Text {
                text: "image".into(),
            },
            sampling_types::ContentPart::Image {
                url: format!("data:{};base64,{}", image.mime_type, image.data).into(),
            },
        ]);
        let mut timeline = chat_state::Timeline::from_seed(vec![
            sampling_types::ConversationItem::system("system"),
            user,
        ])
        .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Input(
                chat_state::InputEvent::Submitted {
                    input_id: "input-1".into(),
                    intent: chat_state::InputIntent::Prompt,
                    payload_ref: reference.clone(),
                },
            ))
            .unwrap();
        for event in timeline.events() {
            storage
                .append_timeline_event_durable(&source, event)
                .await
                .unwrap();
        }
        let child = crate::session::info::Info {
            id: agent_client_protocol::schema::v1::SessionId::new("input-child"),
            cwd: source.cwd.clone(),
        };
        storage
            .copy_session_data_sync(&source, &child, Default::default())
            .unwrap();
        let child_events = storage.read_timeline_events_sync(&child).unwrap();
        let child_timeline = chat_state::Timeline::from_events(child_events.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(timeline.surface()).unwrap(),
            serde_json::to_value(child_timeline.surface()).unwrap()
        );
        assert!(
            child_timeline.submitted_input_payload_hashes().is_empty(),
            "forks seed Surface, not parent input authority"
        );
        validate_submitted_payloads(
            storage.open_session(&child).unwrap().directory(),
            &child_events,
        )
        .unwrap();

        let trace =
            crate::session::storage::load_session_trace_at(source.id.0.as_ref(), root.path())
                .unwrap()
                .unwrap();
        let exported = tempfile::tempdir().unwrap();
        let exported_dir = open_test_session(exported.path());
        for file in trace.files {
            if file.relative_path.starts_with(ARTIFACT_DIRECTORY) {
                crate::session::persistence::write_immutable_blob_to_directory(
                    &exported_dir,
                    &file.relative_path,
                    &file.bytes,
                )
                .unwrap();
            }
        }
        assert_eq!(
            read_payload(&exported_dir, &reference)
                .unwrap()
                .hook_prompt(),
            payload.hook_prompt()
        );
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

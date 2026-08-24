//! `grow/session/state` reads a session's canonical ledgers and materialized metadata;
//! `grow/session/import` recreates the same causal state on another host.

use std::path::{Path, PathBuf};

use agent_client_protocol as acp;
use serde::Deserialize;
use serde_json::{Value, json};

use super::ExtResult;
use crate::session::persistence::Summary;
use crate::session::storage as st;

const SUMMARY_COLUMN: &str = "summary";
const TIMELINE_COLUMN: &str = "timeline";
const SIDEBANDS_COLUMN: &str = "sidebands";
const BLOBS_COLUMN: &str = "blobs";
const UPDATES_COLUMN: &str = "updates";
const MAX_SESSION_STATE_BYTES: u64 = 128 * 1024 * 1024;

type ImmutableBlobs = std::collections::BTreeMap<String, String>;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StateRequest {
    session_id: String,
}

/// A session id is a UUID (see acp_agent's new_session); requiring that keeps it safe
/// to join into a filesystem path.
fn validate_session_uuid(session_id: &str) -> Result<(), acp::Error> {
    uuid::Uuid::try_parse(session_id)
        .map(|_| ())
        .map_err(|_| acp::Error::invalid_params().data("sessionId must be a UUID"))
}

/// `grow/session/state`: return metadata columns keyed by logical name. Errors when
/// the session isn't found on this host, since it reads a single record whose absence
/// is not an empty result (unlike the collection returned by `grow/session/updates`).
pub async fn handle_state(args: &acp::ExtRequest) -> ExtResult {
    let request: StateRequest = super::parse_params(args)?;
    validate_session_uuid(&request.session_id)?;

    let opened = open_session_by_id(&request.session_id)
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))?
        .ok_or_else(|| acp::Error::invalid_params().data("session not found"))?;
    let state = read_entity_state(&opened)
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
    validate_state_size(&state)
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
    super::to_raw_response(&state)
}

fn read_entity_state(
    opened: &st::jsonl::OpenedSession,
) -> std::io::Result<std::collections::HashMap<String, Value>> {
    let timeline_events = opened.timeline_events()?;
    let timeline = chat_state::Timeline::from_events(timeline_events.clone())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let session_id = opened.summary().info.id.to_string();
    let sidebands = opened.sideband_ledgers(&session_id, &timeline)?;
    let blobs = read_entity_blobs(opened.directory(), &timeline)?;
    let updates = opened.update_envelopes()?;
    Ok(std::collections::HashMap::from([
        (
            SUMMARY_COLUMN.to_string(),
            serde_json::to_value(opened.summary())
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        ),
        (
            TIMELINE_COLUMN.to_string(),
            serde_json::to_value(timeline_events)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        ),
        (
            SIDEBANDS_COLUMN.to_string(),
            serde_json::to_value(sidebands)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        ),
        (
            BLOBS_COLUMN.to_string(),
            serde_json::to_value(blobs)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        ),
        (
            UPDATES_COLUMN.to_string(),
            serde_json::to_value(updates)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        ),
    ]))
}

fn validate_state_size(value: &impl serde::Serialize) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if bytes.len() as u64 > MAX_SESSION_STATE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("session state exceeds {MAX_SESSION_STATE_BYTES} bytes"),
        ));
    }
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportRequest {
    session_id: String,
    cwd: String,
    #[serde(default)]
    state: std::collections::HashMap<String, Value>,
}

/// `grow/session/import`: recreate a session on this host from the exact mirrored
/// summary + update/Timeline/Sideband ledgers + referenced immutable blobs. A
/// repeated import is idempotent only when the complete entity is identical.
pub async fn handle_import(args: &acp::ExtRequest) -> ExtResult {
    let mut request: ImportRequest = super::parse_params(args)?;
    validate_session_uuid(&request.session_id)?;

    let info = crate::session::info::Info {
        id: acp::SessionId::new(request.session_id.clone()),
        cwd: request.cwd.clone(),
    };
    let storage = st::JsonlStorageAdapter::new();
    validate_import_state_columns(&request.state)?;
    let Some(summary_value) = request.state.get_mut(SUMMARY_COLUMN) else {
        return Err(acp::Error::invalid_params().data("session/import requires a summary column"));
    };
    let Some(summary_object) = summary_value.as_object_mut() else {
        return Err(acp::Error::invalid_params().data("session/import summary must be an object"));
    };
    validate_import_summary_format(summary_object)?;
    validate_import_summary_identity(summary_object, &request.session_id)?;
    sanitize_summary_for_host(summary_object, &request.session_id, &request.cwd);
    let Ok(summary) = Summary::deserialize(&*summary_value) else {
        return Err(acp::Error::invalid_params().data("summary column is not a valid summary"));
    };
    summary
        .validate_current_format()
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let summary_bytes = serde_json::to_vec(&summary)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    if summary_bytes.len() as u64 > st::MAX_SESSION_SUMMARY_BYTES {
        return Err(acp::Error::invalid_params().data(format!(
            "session/import summary exceeds {} bytes",
            st::MAX_SESSION_SUMMARY_BYTES
        )));
    }
    *summary_value = serde_json::to_value(summary)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let timeline = validate_timeline_column(&request.state)?;
    let sidebands = validate_sidebands_column(&request.state)?;
    validate_updates_column(&request.state)?;
    validate_blobs_column(&request.state, &timeline)?;
    validate_state_size(&request.state)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    st::validate_sideband_ledgers(&request.session_id, &timeline, &sidebands)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;

    if let Some(existing) = storage
        .open_session_by_id(&request.session_id)
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))?
    {
        if existing.summary().info.cwd != request.cwd {
            return Err(acp::Error::invalid_params().data(format!(
                "session {} already exists under cwd {}",
                request.session_id,
                existing.summary().info.cwd
            )));
        }
        let mut existing_state = read_entity_state(&existing)
            .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
        let existing_summary = existing_state
            .get_mut(SUMMARY_COLUMN)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                acp::Error::internal_error().data("existing session summary is not an object")
            })?;
        sanitize_summary_for_host(
            existing_summary,
            &request.session_id,
            &request.cwd,
        );
        let existing_summary = Summary::deserialize(&*existing_summary).map_err(|error| {
            acp::Error::internal_error().data(format!(
                "existing session summary cannot be normalized: {error}"
            ))
        })?;
        existing_state.insert(
            SUMMARY_COLUMN.to_string(),
            serde_json::to_value(existing_summary)
                .map_err(|error| acp::Error::internal_error().data(error.to_string()))?,
        );
        if existing_state == request.state {
            return super::to_raw_response(&json!({ "imported": false }));
        }
        return Err(acp::Error::invalid_params().data(format!(
            "session {} already exists with different causal state",
            request.session_id
        )));
    }

    let parent = storage
        .ensure_session_parent(&info)
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
    let target_name = info.id.to_string();
    write_import(
        &parent,
        std::ffi::OsStr::new(&target_name),
        &request.state,
    )
    .map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            acp::Error::invalid_params().data(error.to_string())
        } else {
            acp::Error::internal_error().data(error.to_string())
        }
    })?;
    super::to_raw_response(&json!({ "imported": true }))
}

fn validate_import_state_columns(
    state: &std::collections::HashMap<String, Value>,
) -> Result<(), acp::Error> {
    let unknown = state
        .keys()
        .filter(|column| {
            !matches!(
                column.as_str(),
                SUMMARY_COLUMN
                    | TIMELINE_COLUMN
                    | SIDEBANDS_COLUMN
                    | BLOBS_COLUMN
                    | UPDATES_COLUMN
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(acp::Error::invalid_params().data(format!(
            "session/import contains unsupported state columns: {}",
            unknown.join(", ")
        )));
    }
    Ok(())
}

fn validate_updates_column(
    state: &std::collections::HashMap<String, Value>,
) -> Result<&Vec<Value>, acp::Error> {
    let updates = state
        .get(UPDATES_COLUMN)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            acp::Error::invalid_params().data("session/import requires an updates array")
        })?;
    for (index, update) in updates.iter().enumerate() {
        validate_jsonl_entry_size(update, &format!("update {index}"))?;
        crate::session::storage::SessionUpdateEnvelope::from_value(update.clone()).map_err(
            |error| {
                acp::Error::invalid_params()
                    .data(format!("session/import update {index} is invalid: {error}"))
            },
        )?;
    }
    Ok(updates)
}

fn validate_timeline_column(
    state: &std::collections::HashMap<String, Value>,
) -> Result<chat_state::Timeline, acp::Error> {
    let value = state.get(TIMELINE_COLUMN).ok_or_else(|| {
        acp::Error::invalid_params().data("session/import requires a timeline array")
    })?;
    let events = serde_json::from_value::<Vec<chat_state::TimelineEvent>>(value.clone())
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    for (index, event) in events.iter().enumerate() {
        validate_jsonl_entry_size(event, &format!("Timeline event {index}"))?;
    }
    chat_state::Timeline::from_events(events)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))
}

fn validate_sidebands_column(
    state: &std::collections::HashMap<String, Value>,
) -> Result<st::SidebandLedgers, acp::Error> {
    let value = state.get(SIDEBANDS_COLUMN).ok_or_else(|| {
        acp::Error::invalid_params().data("session/import requires a sidebands object")
    })?;
    let ledgers: st::SidebandLedgers = serde_json::from_value(value.clone())
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    for (sideband_id, events) in &ledgers {
        for (index, event) in events.iter().enumerate() {
            validate_jsonl_entry_size(event, &format!("sideband {sideband_id} event {index}"))?;
        }
    }
    Ok(ledgers)
}

fn validate_jsonl_entry_size(
    value: &impl serde::Serialize,
    description: &str,
) -> Result<(), acp::Error> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    if (bytes.len() as u64).saturating_add(1) > st::MAX_JSONL_ENTRY_BYTES {
        return Err(acp::Error::invalid_params().data(format!(
            "session/import {description} exceeds {} bytes",
            st::MAX_JSONL_ENTRY_BYTES
        )));
    }
    Ok(())
}

fn referenced_blob_keys(
    timeline: &chat_state::Timeline,
) -> std::io::Result<std::collections::BTreeSet<String>> {
    let mut keys = std::collections::BTreeSet::new();
    for event in timeline.events() {
        match &event.kind {
            chat_state::TimelineEventKind::Messages(messages) => {
                keys.extend(
                    crate::session::persistence::referenced_prompt_blob_hashes(&messages.items)?
                        .into_iter()
                        .map(|hash| format!("prompt/{hash}")),
                );
            }
            chat_state::TimelineEventKind::SubagentResult(result) => {
                if let Some(reference) = result.output_ref.as_deref()
                    && let Some(hash) = reference.strip_prefix("artifact:subagent-output:blake3:")
                {
                    keys.insert(format!("subagent-output/{hash}"));
                }
            }
            _ => {}
        }
    }
    Ok(keys)
}

fn blob_path(dir: &Path, key: &str) -> Option<PathBuf> {
    let (kind, hash) = key.split_once('/')?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    match kind {
        "prompt" => Some(dir.join("prompts").join(format!("{hash}.txt"))),
        "subagent-output" => Some(
            dir.join("artifacts")
                .join("subagent-output")
                .join(format!("{hash}.json")),
        ),
        _ => None,
    }
}

fn read_entity_blobs(
    session: &st::ContainedDirectory,
    timeline: &chat_state::Timeline,
) -> std::io::Result<ImmutableBlobs> {
    let mut blobs = ImmutableBlobs::new();
    for key in referenced_blob_keys(timeline)? {
        let relative = blob_path(Path::new(""), &key).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid immutable blob key",
            )
        })?;
        let parent = relative.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "immutable blob path has no parent",
            )
        })?;
        let directory = session.open_relative(
            parent,
            "session immutable blob directory",
            false,
        )?;
        let file_name = relative.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "immutable blob path has no file name",
            )
        })?;
        let bytes = directory.read_bounded(
            file_name,
            "session immutable blob",
            crate::session::persistence::MAX_IMMUTABLE_BLOB_BYTES,
        )?;
        let hash = key
            .rsplit_once('/')
            .map(|(_, hash)| hash)
            .unwrap_or_default();
        if blake3::hash(&bytes).to_hex().as_str() != hash {
            let path = session.display_path().join(&relative);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("immutable blob hash mismatch at {}", path.display()),
            ));
        }
        let path = session.display_path().join(&relative);
        let text = String::from_utf8(bytes).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("immutable blob is not UTF-8: {}", path.display()),
            )
        })?;
        blobs.insert(key, text);
    }
    Ok(blobs)
}

fn validate_blobs_column(
    state: &std::collections::HashMap<String, Value>,
    timeline: &chat_state::Timeline,
) -> Result<ImmutableBlobs, acp::Error> {
    let value = state.get(BLOBS_COLUMN).ok_or_else(|| {
        acp::Error::invalid_params().data("session/import requires a blobs object")
    })?;
    let blobs: ImmutableBlobs = serde_json::from_value(value.clone())
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let expected = referenced_blob_keys(timeline)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let actual = blobs
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if actual != expected {
        return Err(acp::Error::invalid_params().data(format!(
            "session/import blob set differs from Timeline references: expected {expected:?}, got {actual:?}"
        )));
    }
    for (key, content) in &blobs {
        let Some(_) = blob_path(Path::new("."), key) else {
            return Err(acp::Error::invalid_params().data(format!("invalid blob key {key}")));
        };
        let hash = key
            .rsplit_once('/')
            .map(|(_, hash)| hash)
            .unwrap_or_default();
        if content.len() as u64 > crate::session::persistence::MAX_IMMUTABLE_BLOB_BYTES {
            return Err(acp::Error::invalid_params().data(format!(
                "blob {key} exceeds {} bytes",
                crate::session::persistence::MAX_IMMUTABLE_BLOB_BYTES
            )));
        }
        if blake3::hash(content.as_bytes()).to_hex().as_str() != hash {
            return Err(acp::Error::invalid_params()
                .data(format!("blob {key} does not match its content hash")));
        }
    }
    Ok(blobs)
}

fn validate_import_summary_format(
    summary: &serde_json::Map<String, Value>,
) -> Result<(), acp::Error> {
    let incoming_version = summary
        .get("session_format_version")
        .and_then(Value::as_u64);
    if incoming_version
        != Some(u64::from(
            crate::session::persistence::SESSION_FORMAT_VERSION,
        ))
    {
        return Err(acp::Error::invalid_params().data(format!(
            "session/import requires session format version {}",
            crate::session::persistence::SESSION_FORMAT_VERSION
        )));
    }
    Ok(())
}

fn validate_import_summary_identity(
    summary: &serde_json::Map<String, Value>,
    session_id: &str,
) -> Result<(), acp::Error> {
    let incoming_id = summary
        .get("info")
        .and_then(Value::as_object)
        .and_then(|info| info.get("id"))
        .and_then(Value::as_str);
    if incoming_id != Some(session_id) {
        return Err(acp::Error::invalid_params().data(
            "session/import cannot remap an immutable Timeline identity; summary.info.id must equal sessionId",
        ));
    }
    Ok(())
}

/// Rewrite a mirrored summary's host-specific fields to describe this host.
fn sanitize_summary_for_host(summary: &mut serde_json::Map<String, Value>, id: &str, cwd: &str) {
    if let Some(info_obj) = summary.get_mut("info").and_then(Value::as_object_mut) {
        info_obj.insert("id".to_string(), Value::String(id.to_string()));
        info_obj.insert("cwd".to_string(), Value::String(cwd.to_string()));
    }
    summary.insert("git_remotes".to_string(), json!([]));
    for field in [
        "prompt_display_cwd",
        "source_workspace_dir",
        "git_root_dir",
        "head_commit",
        "head_branch",
        "worktree_label",
        "request_id",
    ] {
        summary.remove(field);
    }
    set_or_remove(
        summary,
        "grow_home",
        crate::session::persistence::grow_home_string(),
    );
    set_or_remove(
        summary,
        "sandbox_profile",
        sandbox::configured_profile_name().map(String::from),
    );
}

fn set_or_remove(obj: &mut serde_json::Map<String, Value>, key: &str, value: Option<String>) {
    match value {
        Some(v) => {
            obj.insert(key.to_string(), Value::String(v));
        }
        None => {
            obj.remove(key);
        }
    }
}

/// Build an imported current-format session out of namespace, then publish the whole
/// directory with the storage layer's no-replace commit primitive.
fn write_import(
    parent: &st::ContainedDirectory,
    target_name: &std::ffi::OsStr,
    state: &std::collections::HashMap<String, Value>,
) -> std::io::Result<()> {
    st::JsonlStorageAdapter::build_and_publish_session(parent, target_name, |staging| {
        write_import_staging(staging, state)
    })
}

fn write_import_staging(
    staging: &st::ContainedDirectory,
    state: &std::collections::HashMap<String, Value>,
) -> std::io::Result<()> {
    let timeline = state
        .get(TIMELINE_COLUMN)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "session/import requires a timeline array",
            )
        })?;

    staging.write_atomic(
        std::ffi::OsStr::new(st::TIMELINE_FILE),
        &st::to_jsonl_bytes(timeline)?,
        false,
        true,
    )?;
    let sidebands = state
        .get(SIDEBANDS_COLUMN)
        .and_then(Value::as_object)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "session/import requires a sidebands object",
            )
        })?;
    for (sideband_id, events) in sidebands {
        let events = events.as_array().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("sideband {sideband_id} ledger must be an array"),
            )
        })?;
        let sideband = staging.open_relative(
            &Path::new(st::SIDEBANDS_DIR).join(sideband_id),
            "session/import sideband directory",
            true,
        )?;
        sideband.write_atomic(
            std::ffi::OsStr::new(st::TIMELINE_FILE),
            &st::to_jsonl_bytes(events)?,
            false,
            true,
        )?;
    }
    let blobs: ImmutableBlobs =
        serde_json::from_value(state.get(BLOBS_COLUMN).cloned().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "session/import requires a blobs object",
            )
        })?)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    for (key, content) in blobs {
        let path = blob_path(Path::new(""), &key).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("invalid immutable blob key {key}"),
            )
        })?;
        crate::session::persistence::write_immutable_blob_to_directory(
            staging,
            &path,
            content.as_bytes(),
        )?;
    }
    let updates = state
        .get(UPDATES_COLUMN)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "session/import requires an updates array",
            )
        })?;
    if !updates.is_empty() {
        staging.write_atomic(
            std::ffi::OsStr::new(st::UPDATES_FILE),
            &st::to_jsonl_bytes(updates)?,
            false,
            true,
        )?;
    }

    let summary = state.get(SUMMARY_COLUMN).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "session/import requires a summary column",
        )
    })?;
    write_column(staging, st::SUMMARY_FILE, summary)?;
    Ok(())
}

fn write_column(
    directory: &st::ContainedDirectory,
    rel: &str,
    value: &Value,
) -> std::io::Result<()> {
    directory.write_atomic(
        std::ffi::OsStr::new(rel),
        value.to_string().as_bytes(),
        false,
        true,
    )
}

fn open_session_by_id(session_id: &str) -> std::io::Result<Option<st::jsonl::OpenedSession>> {
    let storage = st::JsonlStorageAdapter::new();
    storage.open_session_by_id(session_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::storage::StorageAdapter as _;
    use serde_json::json;

    fn linked_ledgers() -> (
        String,
        Vec<chat_state::TimelineEvent>,
        Vec<chat_state::SidebandEvent>,
    ) {
        let parent_id = uuid::Uuid::now_v7().to_string();
        let sideband_id = uuid::Uuid::now_v7().to_string();
        let mut parent = chat_state::Timeline::default();
        parent
            .record(chat_state::TimelineEventKind::Sideband(
                chat_state::SidebandSpawnEvent {
                    sideband_id: sideband_id.clone(),
                    purpose: chat_state::SidebandPurpose::PermissionJudgment,
                    source_refs: Vec::new(),
                },
            ))
            .unwrap();
        let mut sideband = chat_state::SidebandTimeline::new(sideband_id.clone()).unwrap();
        for kind in [
            chat_state::SidebandEventKind::Request(chat_state::SidebandRequest {
                purpose: chat_state::SidebandPurpose::PermissionJudgment,
                prompt: "judge".into(),
                source_refs: Vec::new(),
                route: chat_state::SidebandRoute {
                    model: "test-model".into(),
                    backend: "responses".into(),
                },
                initiator_ref: format!("t:{parent_id}/sideband:{sideband_id}"),
                executor: "main".into(),
                output_schema: None,
            }),
            chat_state::SidebandEventKind::Attempt(chat_state::SidebandAttempt {
                attempt_no: 1,
                input_refs: Vec::new(),
                assembly_manifest: chat_state::SidebandAssemblyManifest {
                    strategy: "all-sources".into(),
                    strategy_version: 1,
                    source_revision: None,
                    context_surface_ids: Vec::new(),
                    selected_surface_ids: Vec::new(),
                    materialized_input_tokens: 1,
                    max_output_tokens: Some(1),
                },
                feedback: None,
            }),
            chat_state::SidebandEventKind::Result(chat_state::SidebandResult {
                raw_output: "allow".into(),
                structured_output: None,
                usage: chat_state::SidebandUsage::default(),
                finish: "stop".into(),
                source_event_seqs: [0, 1],
                evidence_refs: Vec::new(),
            }),
            chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
                outcome: chat_state::SidebandOutcome::Completed,
                error: None,
            }),
        ] {
            let event = sideband.prepare(kind).unwrap();
            sideband.accept(event).unwrap();
        }
        (
            parent_id,
            parent.events().to_vec(),
            sideband.events().to_vec(),
        )
    }

    #[test]
    fn sanitize_summary_for_host_rewrites_host_fields() {
        let mut summary = json!({
            "info": { "id": "s1", "cwd": "/remote/host/work" },
            "session_format_version": crate::session::persistence::SESSION_FORMAT_VERSION,
            "prompt_display_cwd": "/remote/host/work",
            "source_workspace_dir": "/remote/host",
            "git_root_dir": "/remote/host/repo",
            "git_remotes": ["origin"],
            "head_commit": "deadbeef",
            "head_branch": "feature",
            "worktree_label": "wt",
            "request_id": "req-1",
        })
        .as_object()
        .unwrap()
        .clone();

        sanitize_summary_for_host(&mut summary, "s-new", "/local/work");

        assert_eq!(summary["info"]["id"], json!("s-new"));
        assert_eq!(summary["info"]["cwd"], json!("/local/work"));
        assert_eq!(
            summary["session_format_version"],
            json!(crate::session::persistence::SESSION_FORMAT_VERSION)
        );
        assert_eq!(summary["git_remotes"], json!([]));
        for gone in [
            "prompt_display_cwd",
            "source_workspace_dir",
            "git_root_dir",
            "head_commit",
            "head_branch",
            "worktree_label",
            "request_id",
        ] {
            assert!(!summary.contains_key(gone), "{gone} should be dropped");
        }
    }

    #[test]
    fn import_summary_format_is_a_hard_version_gate() {
        for value in [json!({}), json!({ "session_format_version": 1 })] {
            assert!(
                validate_import_summary_format(value.as_object().unwrap()).is_err(),
                "missing and obsolete summaries must not be relabeled during import"
            );
        }
        let current = json!({
            "session_format_version": crate::session::persistence::SESSION_FORMAT_VERSION
        });
        assert!(validate_import_summary_format(current.as_object().unwrap()).is_ok());
    }

    #[test]
    fn import_state_columns_are_exact() {
        let valid = std::collections::HashMap::from([
            (SUMMARY_COLUMN.to_string(), json!({})),
            (TIMELINE_COLUMN.to_string(), json!([])),
            (SIDEBANDS_COLUMN.to_string(), json!({})),
            (BLOBS_COLUMN.to_string(), json!({})),
            (UPDATES_COLUMN.to_string(), json!([])),
        ]);
        assert!(validate_import_state_columns(&valid).is_ok());

        let mut with_old_sidecar = valid;
        with_old_sidecar.insert("control".into(), json!({}));
        let error = validate_import_state_columns(&with_old_sidecar).unwrap_err();
        assert!(
            error
                .data
                .is_some_and(|data| data.to_string().contains("control"))
        );
    }

    #[test]
    fn sideband_column_is_required_and_parent_linked() {
        let (parent_id, parent_events, sideband_events) = linked_ledgers();
        let parent = chat_state::Timeline::from_events(parent_events.clone()).unwrap();
        let sideband_id = sideband_events[0].sideband_id.clone();
        let state = std::collections::HashMap::from([
            (SUMMARY_COLUMN.to_string(), json!({})),
            (TIMELINE_COLUMN.to_string(), json!(parent_events)),
            (
                SIDEBANDS_COLUMN.to_string(),
                serde_json::to_value(std::collections::BTreeMap::from([(
                    sideband_id.clone(),
                    sideband_events.clone(),
                )]))
                .unwrap(),
            ),
            (BLOBS_COLUMN.to_string(), json!({})),
        ]);
        let ledgers = validate_sidebands_column(&state).unwrap();
        st::validate_sideband_ledgers(&parent_id, &parent, &ledgers).unwrap();

        let missing = std::collections::HashMap::from([
            (SUMMARY_COLUMN.to_string(), json!({})),
            (TIMELINE_COLUMN.to_string(), json!(parent_events)),
        ]);
        assert!(validate_sidebands_column(&missing).is_err());

        let mut tampered = ledgers;
        let request = tampered.get_mut(&sideband_id).unwrap().first_mut().unwrap();
        let chat_state::SidebandEventKind::Request(request) = &mut request.kind else {
            unreachable!()
        };
        request.purpose = chat_state::SidebandPurpose::SessionRecap;
        assert!(st::validate_sideband_ledgers(&parent_id, &parent, &tampered).is_err());
    }

    #[test]
    fn blob_column_exactly_matches_timeline_references() {
        let content = "full oversized request";
        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        let reference = format!(
            "{}{}",
            crate::session::persistence::PROMPT_BLOB_REF_PREFIX,
            hash
        );
        let timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user(format!(
                "read\n{reference}\nthen continue"
            ))])
            .unwrap();
        let mut state = std::collections::HashMap::from([(
            BLOBS_COLUMN.to_string(),
            serde_json::to_value(std::collections::BTreeMap::from([(
                format!("prompt/{hash}"),
                content,
            )]))
            .unwrap(),
        )]);
        assert_eq!(validate_blobs_column(&state, &timeline).unwrap().len(), 1);

        state.insert(BLOBS_COLUMN.to_string(), json!({}));
        assert!(validate_blobs_column(&state, &timeline).is_err());
        state.insert(
            BLOBS_COLUMN.to_string(),
            serde_json::to_value(std::collections::BTreeMap::from([(
                format!("prompt/{hash}"),
                "tampered",
            )]))
            .unwrap(),
        );
        assert!(validate_blobs_column(&state, &timeline).is_err());
    }

    #[test]
    fn import_identity_cannot_be_remapped() {
        let summary = json!({ "info": { "id": "source", "cwd": "/work" } });
        assert!(
            validate_import_summary_identity(summary.as_object().unwrap(), "destination").is_err()
        );
        assert!(validate_import_summary_identity(summary.as_object().unwrap(), "source").is_ok());
    }

    #[test]
    fn write_import_writes_columns_and_updates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("target-session");
        let parent = st::ContainedDirectory::open(
            tmp.path(),
            Path::new(""),
            "session import test parent",
            false,
        )
        .unwrap();

        let mut state = std::collections::HashMap::new();
        state.insert(
            "summary".to_string(),
            json!({ "info": { "id": "s1", "cwd": "/work" } }),
        );
        let (_, parent_events, sideband_events) = linked_ledgers();
        let sideband_id = sideband_events[0].sideband_id.clone();
        state.insert("timeline".to_string(), json!(parent_events));
        state.insert(
            "sidebands".to_string(),
            serde_json::to_value(std::collections::BTreeMap::from([(
                sideband_id.clone(),
                sideband_events,
            )]))
            .unwrap(),
        );
        state.insert("blobs".to_string(), json!({}));
        let updates = vec![
            json!({ "method": "session/update", "params": { "a": 1 } }),
            json!({ "method": "session/update", "params": { "b": 2 } }),
        ];
        state.insert(UPDATES_COLUMN.to_string(), json!(updates));

        write_import(&parent, std::ffi::OsStr::new("target-session"), &state).unwrap();

        assert!(dir.join("summary.json").exists(), "summary.json written");
        assert!(
            dir.join(st::SIDEBANDS_DIR)
                .join(sideband_id)
                .join(st::TIMELINE_FILE)
                .exists(),
            "sideband ledger written"
        );
        let published_summary = std::fs::read_to_string(dir.join(st::SUMMARY_FILE)).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("updates.jsonl"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        assert!(std::fs::read_dir(tmp.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with("import-staging")
        }));

        let error = write_import(
            &parent,
            std::ffi::OsStr::new("target-session"),
            &state,
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read_to_string(dir.join(st::SUMMARY_FILE)).unwrap(),
            published_summary,
            "a duplicate import must not alter the published session"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_import_cannot_publish_through_a_symlinked_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::TempDir::new().unwrap();
        let outside = tmp.path().join("outside");
        let target = tmp.path().join("target-session");
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, &target).unwrap();
        let parent = st::ContainedDirectory::open(
            tmp.path(),
            Path::new(""),
            "session import test parent",
            false,
        )
        .unwrap();
        let state = std::collections::HashMap::from([
            (
                SUMMARY_COLUMN.to_string(),
                json!({ "info": { "id": "s1", "cwd": "/work" } }),
            ),
            (TIMELINE_COLUMN.to_string(), json!([])),
            (SIDEBANDS_COLUMN.to_string(), json!({})),
            (BLOBS_COLUMN.to_string(), json!({})),
            (UPDATES_COLUMN.to_string(), json!([])),
        ]);

        let error = write_import(
            &parent,
            std::ffi::OsStr::new("target-session"),
            &state,
        )
            .expect_err("session import must not traverse a symlinked target");
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    }
}

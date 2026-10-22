//! `grow/session/state` reads a session's canonical ledgers and materialized metadata;
//! `grow/session/import` recreates the same causal state on another host.

use std::path::{Path, PathBuf};

use acp_transport::protocol as acp;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{Value, json};

use super::ExtResult;
use crate::session::persistence::Summary;
use crate::session::storage as st;

const SUMMARY_COLUMN: &str = "summary";
const TIMELINE_COLUMN: &str = "timeline";
const SIDEBANDS_COLUMN: &str = "sidebands";
const BLOBS_COLUMN: &str = "blobs";

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
    let session_id = opened.summary().info.id.to_string();
    let validated = opened.validated_timeline(&session_id)?;
    let timeline = validated.timeline;
    let sidebands = validated.sidebands;
    let blobs = read_entity_blobs(opened.directory(), &timeline)?;
    let records = opened.journal_records(&validated.history)?;
    Ok(std::collections::HashMap::from([
        (
            SUMMARY_COLUMN.to_string(),
            serde_json::to_value(opened.summary())
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        ),
        (
            TIMELINE_COLUMN.to_string(),
            serde_json::to_value(records)
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
    crate::session::history::HistorySnapshot::validate_owner(&timeline, &request.session_id)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let sidebands = validate_sidebands_column(&request.state)?;

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
        sanitize_summary_for_host(existing_summary, &request.session_id, &request.cwd);
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
    write_import(&parent, std::ffi::OsStr::new(&target_name), &request.state).map_err(|error| {
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
                SUMMARY_COLUMN | TIMELINE_COLUMN | SIDEBANDS_COLUMN | BLOBS_COLUMN
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

fn validate_timeline_column(
    state: &std::collections::HashMap<String, Value>,
) -> Result<chat_state::Timeline, acp::Error> {
    let value = state.get(TIMELINE_COLUMN).ok_or_else(|| {
        acp::Error::invalid_params().data("session/import requires a timeline array")
    })?;
    let records = serde_json::from_value::<Vec<String>>(value.clone())
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let journal = st::JsonlStorageAdapter::decode_journal_lines(&records)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let timeline = chat_state::Timeline::from_events(journal.events)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    journal
        .history
        .validate(&timeline)
        .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let runtime = timeline.runtime_lifecycle();
    if runtime.phase != chat_state::session_runtime::RuntimePhase::Running
        && (runtime.checkpoint.is_some() || runtime.operation_id.is_some())
    {
        return Err(acp::Error::invalid_params().data(
            "session/import cannot import an unconsumed runtime checkpoint; restore or resolve the source session first",
        ));
    }
    validate_import_plan_authority(&timeline)?;
    Ok(timeline)
}

fn validate_import_plan_authority(timeline: &chat_state::Timeline) -> Result<(), acp::Error> {
    let control =
        crate::session::control::SessionControlSnapshot::latest_from_timeline(timeline.events())
            .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
    let retained_plan_or_execution = control.as_ref().is_some_and(|control| {
        let behavior = &control.behavior;
        behavior.retained_plan.as_ref().is_some_and(|candidate| {
            matches!(
                candidate.status,
                crate::session::behavior::RetainedPlanStatus::Deferred
                    | crate::session::behavior::RetainedPlanStatus::Reviewing
            )
        }) || behavior.approval_pending
            || behavior.last_plan_handoff.is_some()
            || matches!(
                behavior.state,
                crate::session::behavior::BehaviorState::Plan(
                    crate::session::behavior::PlanPhase::AwaitingApproval
                        | crate::session::behavior::PlanPhase::Amending
                        | crate::session::behavior::PlanPhase::Executing
                )
            )
    });
    let unconsumed_plan_handoff = timeline.pending_notifications().iter().any(|notification| {
        matches!(
            &notification.source,
            chat_state::NotificationSource::PlanHandoff { .. }
        )
    });
    let unresolved_plan_approval = {
        let mut open = std::collections::HashSet::new();
        for event in timeline.events() {
            match &event.kind {
                chat_state::TimelineEventKind::Interaction(
                    chat_state::InteractionEvent::Opened {
                        tool_call_id,
                        attempt_id,
                        kind: chat_state::InteractionKind::PlanApproval,
                    },
                ) => {
                    open.insert((tool_call_id.as_str(), attempt_id.as_str()));
                }
                chat_state::TimelineEventKind::Interaction(
                    chat_state::InteractionEvent::Resolved {
                        tool_call_id,
                        attempt_id,
                        kind: chat_state::InteractionKind::PlanApproval,
                        ..
                    },
                ) => {
                    open.remove(&(tool_call_id.as_str(), attempt_id.as_str()));
                }
                _ => {}
            }
        }
        !open.is_empty()
    };
    if retained_plan_or_execution || unresolved_plan_approval || unconsumed_plan_handoff {
        return Err(acp::Error::invalid_params().data(
            "session/import cannot transfer retained Plan approval or execution authority; create a new Plan submission and approve it on the target owner",
        ));
    }
    Ok(())
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
    let mut keys: std::collections::BTreeSet<_> =
        crate::session::sampling_evidence::referenced_hashes(timeline)?
            .into_iter()
            .map(|hash| format!("sampling/{hash}"))
            .collect();
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
        "sampling" => Some(dir.join("artifacts/sampling").join(format!("{hash}.bin"))),
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
        let directory = session.open_relative(parent, "session immutable blob directory", false)?;
        let file_name = relative.file_name().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "immutable blob path has no file name",
            )
        })?;
        let bytes = directory.read_bounded(
            file_name,
            "session immutable blob",
            if key.starts_with("sampling/") {
                crate::session::sampling_evidence::CHUNK_BYTES as u64
            } else {
                crate::session::persistence::MAX_IMMUTABLE_BLOB_BYTES
            },
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
        let text = if key.starts_with("sampling/") {
            base64::engine::general_purpose::STANDARD.encode(bytes)
        } else {
            String::from_utf8(bytes).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("immutable blob is not UTF-8: {}", path.display()),
                )
            })?
        };
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
        let content = decode_blob(key, content)
            .map_err(|error| acp::Error::invalid_params().data(error.to_string()))?;
        if content.len() as u64 > crate::session::persistence::MAX_IMMUTABLE_BLOB_BYTES {
            return Err(acp::Error::invalid_params().data(format!(
                "blob {key} exceeds {} bytes",
                crate::session::persistence::MAX_IMMUTABLE_BLOB_BYTES
            )));
        }
        if blake3::hash(&content).to_hex().as_str() != hash {
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
        &timeline
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(|line| format!("{line}\n"))
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Timeline record must be an exact string",
                        )
                    })
            })
            .collect::<std::io::Result<String>>()?
            .into_bytes(),
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
            &decode_blob(&key, &content)?,
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

    fn timeline_column(events: &[chat_state::TimelineEvent], wire_updates: &[Value]) -> Value {
        let facts = wire_updates
            .iter()
            .map(|wire_update| {
                let update = serde_json::from_value::<crate::session::storage::SessionUpdate>(
                    wire_update.clone(),
                )
                .expect("test wire envelope must decode as a session update");
                crate::session::history::HistoryFact::from_update(&update)
                    .expect("test update must convert to a history fact")
                    .expect("test update must produce a history fact")
            })
            .collect::<Vec<_>>();
        let bytes = st::JsonlStorageAdapter::encode_journal(events, &facts)
            .expect("test Timeline facts must encode");
        let lines = std::str::from_utf8(&bytes)
            .expect("encoded Timeline journal is UTF-8")
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        json!(lines)
    }

    fn test_history_update(
        session_id: &str,
        reason: &str,
    ) -> crate::session::storage::SessionUpdate {
        crate::session::storage::SessionUpdate::Grow(Box::new(
            crate::extensions::notification::SessionNotification {
                session_id: acp::SessionId::new(session_id),
                update: crate::extensions::notification::SessionUpdate::AutoCompactStarted {
                    tokens_used: 10,
                    context_window: 100,
                    percentage: 10,
                    reason: reason.to_owned(),
                },
                meta: None,
            },
        ))
    }

    fn runtime_test_timeline(resumed: bool, open_turn: bool) -> chat_state::Timeline {
        let mut timeline = chat_state::Timeline::default();
        let turn_id = open_turn.then_some(chat_state::TurnId(98));
        if let Some(turn_id) = turn_id {
            timeline
                .record(chat_state::TimelineEventKind::Turn(
                    chat_state::TurnEvent::Started {
                        prompt_id: "import-prompt".into(),
                        id: turn_id,
                        input_ids: Vec::new(),
                        identity: chat_state::TurnIdentity {
                            origin: "user".into(),
                            turn_kind: "internal".into(),
                            goal_id: None,
                            goal_definition_revision: None,
                            stage_id: None,
                        },
                        model_id: "test-model".into(),
                        input_message_count: 0,
                        prompt_index: 0,
                        prompt_text: "open imported turn".into(),
                        input_kind: chat_state::TurnInputKind::Prompt,
                        redirect_kind: None,
                    },
                ))
                .unwrap();
        }
        timeline
            .record(chat_state::TimelineEventKind::Runtime(
                chat_state::session_runtime::SessionRuntimeEvent::SuspendRequested {
                    operation_id: "import-suspend-op".into(),
                    root_session_id: "session-import-test".into(),
                },
            ))
            .unwrap();
        let settled = timeline
            .record(chat_state::TimelineEventKind::Runtime(
                chat_state::session_runtime::SessionRuntimeEvent::CommandsSettled {
                    operation_id: "import-suspend-op".into(),
                    commands: Vec::new(),
                },
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Runtime(
                chat_state::session_runtime::SessionRuntimeEvent::Checkpoint {
                    operation_id: "import-suspend-op".into(),
                    checkpoint: Box::new(chat_state::session_runtime::RuntimeCheckpoint {
                        frontier: settled.seq,
                        runtime_source_hash: "c".repeat(64),
                        turn: turn_id.map(|turn_id| {
                            chat_state::session_runtime::TurnContinuation {
                                turn_id,
                                prompt_id: "import-prompt".into(),
                                prompt_index: 0,
                                origin: "user".into(),
                                next_step_index: 0,
                                tool_turns_used: 0,
                                tool_calls_used: 0,
                                stop_continuations_used: 0,
                                output_schema: None,
                                resume_point: chat_state::session_runtime::TurnResumePoint::Model,
                                task_output_usage: None,
                                accepted_response: None,
                                structured_output_retries: 0,
                                sampling_attempts_used: 0,
                                sampling_max_attempts: None,
                                sampling_replay_safe: true,
                                sampling_deadline_ms: None,
                                operations: Vec::new(),
                            }
                        }),
                        step: None,
                    }),
                },
            ))
            .unwrap();
        if resumed {
            timeline
                .record(chat_state::TimelineEventKind::Runtime(
                    chat_state::session_runtime::SessionRuntimeEvent::RestorePrepared {
                        operation_id: "import-suspend-op".into(),
                        instance_id: "import-instance".into(),
                    },
                ))
                .unwrap();
            timeline
                .record(chat_state::TimelineEventKind::Runtime(
                    chat_state::session_runtime::SessionRuntimeEvent::Resumed {
                        operation_id: "import-suspend-op".into(),
                        instance_id: "import-instance".into(),
                    },
                ))
                .unwrap();
            timeline
                .record(chat_state::TimelineEventKind::Runtime(
                    chat_state::session_runtime::SessionRuntimeEvent::ExecutionReleased {
                        operation_id: "import-suspend-op".into(),
                        instance_id: "import-instance".into(),
                    },
                ))
                .unwrap();
        }
        timeline
    }

    #[test]
    fn import_rejects_unconsumed_checkpoint_but_accepts_resumed_audit() {
        let suspended = runtime_test_timeline(false, false);
        let suspended_state = std::collections::HashMap::from([(
            TIMELINE_COLUMN.to_string(),
            timeline_column(suspended.events(), &[]),
        )]);
        assert!(validate_timeline_column(&suspended_state).is_err());

        let resumed = runtime_test_timeline(true, true);
        let resumed_state = std::collections::HashMap::from([(
            TIMELINE_COLUMN.to_string(),
            timeline_column(resumed.events(), &[]),
        )]);
        let mut imported = validate_timeline_column(&resumed_state)
            .expect("consumed checkpoint remains valid read-only audit history");
        assert_eq!(
            imported.runtime_lifecycle().phase,
            chat_state::session_runtime::RuntimePhase::Running
        );
        assert!(imported.runtime_lifecycle().checkpoint.is_some());
        assert_eq!(imported.active_turn(), Some(chat_state::TurnId(98)));
        assert!(!imported.recover_interrupted().unwrap().is_empty());
        assert!(imported.active_turn().is_none());
    }

    fn plan_import_timeline(approved: bool, pending_handoff: bool) -> chat_state::Timeline {
        use crate::session::behavior::{
            BehaviorCoordinator, RetainedPlanAction, RetainedPlanDecisionOutcome,
        };

        let mut timeline = chat_state::Timeline::default();
        if approved {
            let mut behavior = BehaviorCoordinator::new();
            behavior.select_behavior(tool_types::BehaviorId::Plan);
            behavior.record_plan_artifact("# source plan");
            assert!(behavior.submit_initial_plan());
            let submitted = behavior.snapshot();
            let submission_id = behavior.plan_submission_id().unwrap();
            assert!(matches!(
                behavior.decide_submitted_plan_if(
                    &submitted,
                    submission_id,
                    "source-approved-decision",
                    RetainedPlanAction::Approve,
                    None,
                ),
                RetainedPlanDecisionOutcome::Applied(
                    crate::session::behavior::RetainedPlanStatus::Approved
                )
            ));
            timeline
                .record(
                    crate::session::control::SessionControlSnapshot::new(
                        1,
                        "grow-build",
                        behavior.snapshot(),
                        None,
                    )
                    .timeline_kind()
                    .unwrap(),
                )
                .unwrap();
        } else {
            timeline
                .record(
                    crate::session::control::SessionControlSnapshot::new(
                        1,
                        "grow-build",
                        crate::session::behavior::BehaviorSnapshot::normal(),
                        None,
                    )
                    .timeline_kind()
                    .unwrap(),
                )
                .unwrap();
        }

        if pending_handoff {
            let body = b"approved plan handoff";
            let owner_session_id = "import-owner-session";
            let source = chat_state::NotificationSource::PlanHandoff {
                artifact_hash: blake3::hash(b"# source plan").to_hex().to_string(),
                artifact_revision: 1,
                handoff: chat_state::PlanHandoffKind::Execute,
            };
            let source_version = chat_state::NotificationSourceVersion::Ordinal { value: 1 };
            timeline
                .record(chat_state::TimelineEventKind::Notification(
                    chat_state::NotificationEvent::Received {
                        id: chat_state::notification_id(owner_session_id, &source, &source_version)
                            .unwrap(),
                        owner_session_id: owner_session_id.into(),
                        source,
                        source_version,
                        payload_ref: chat_state::NotificationPayloadRef {
                            blake3: blake3::hash(body).to_hex().to_string(),
                            bytes: body.len() as u64,
                        },
                    },
                ))
                .unwrap();
        }
        timeline
    }

    fn validate_import_timeline(timeline: &chat_state::Timeline) -> Result<(), acp::Error> {
        let state = std::collections::HashMap::from([(
            TIMELINE_COLUMN.to_string(),
            timeline_column(timeline.events(), &[]),
        )]);
        validate_timeline_column(&state).map(|_| ())
    }

    #[test]
    fn import_rejects_reviewable_or_approved_plan_authority_but_accepts_closed_history() {
        let mut deferred = crate::session::behavior::BehaviorCoordinator::new();
        deferred.select_behavior(tool_types::BehaviorId::Plan);
        deferred.record_plan_artifact("# deferred source");
        assert!(deferred.submit_initial_plan());
        let submitted = deferred.snapshot();
        let submission_id = deferred.plan_submission_id().unwrap();
        assert!(matches!(
            deferred.decide_submitted_plan_if(
                &submitted,
                submission_id,
                "source-defer-decision",
                crate::session::behavior::RetainedPlanAction::Defer,
                None,
            ),
            crate::session::behavior::RetainedPlanDecisionOutcome::Applied(
                crate::session::behavior::RetainedPlanStatus::Deferred
            )
        ));
        assert!(deferred.finish_plan());
        let mut deferred_timeline = chat_state::Timeline::default();
        deferred_timeline
            .record(
                crate::session::control::SessionControlSnapshot::new(
                    1,
                    "grow-build",
                    deferred.snapshot(),
                    None,
                )
                .timeline_kind()
                .unwrap(),
            )
            .unwrap();
        let error = validate_import_timeline(&deferred_timeline).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("new Plan submission and approve")
        );

        let approved = plan_import_timeline(true, false);
        let error = validate_import_timeline(&approved).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("new Plan submission and approve")
        );

        let mut awaiting = crate::session::behavior::BehaviorCoordinator::new();
        awaiting.select_behavior(tool_types::BehaviorId::Plan);
        awaiting.record_plan_artifact("# pending source");
        assert!(awaiting.submit_initial_plan());
        let mut awaiting_timeline = chat_state::Timeline::default();
        awaiting_timeline
            .record(
                crate::session::control::SessionControlSnapshot::new(
                    1,
                    "grow-build",
                    awaiting.snapshot(),
                    None,
                )
                .timeline_kind()
                .unwrap(),
            )
            .unwrap();
        let error = validate_import_timeline(&awaiting_timeline).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("new Plan submission and approve")
        );

        let pending_handoff_without_control = plan_import_timeline(false, true);
        let error = validate_import_timeline(&pending_handoff_without_control).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("new Plan submission and approve")
        );

        let mut unresolved_approval = plan_import_timeline(false, false);
        unresolved_approval
            .record(chat_state::TimelineEventKind::Interaction(
                chat_state::InteractionEvent::Opened {
                    tool_call_id: "plan-call-open".into(),
                    attempt_id: "plan-attempt-open".into(),
                    kind: chat_state::InteractionKind::PlanApproval,
                },
            ))
            .unwrap();
        let error = validate_import_timeline(&unresolved_approval).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("new Plan submission and approve")
        );

        let mut resolved_approval = plan_import_timeline(false, false);
        resolved_approval
            .record(chat_state::TimelineEventKind::Interaction(
                chat_state::InteractionEvent::Opened {
                    tool_call_id: "plan-call-resolved".into(),
                    attempt_id: "plan-attempt-resolved".into(),
                    kind: chat_state::InteractionKind::PlanApproval,
                },
            ))
            .unwrap();
        resolved_approval
            .record(chat_state::TimelineEventKind::Interaction(
                chat_state::InteractionEvent::Resolved {
                    tool_call_id: "plan-call-resolved".into(),
                    attempt_id: "plan-attempt-resolved".into(),
                    kind: chat_state::InteractionKind::PlanApproval,
                    resolution: chat_state::InteractionResolution::Answered,
                },
            ))
            .unwrap();
        validate_import_timeline(&resolved_approval)
            .expect("resolved approval history does not carry a live waiter");

        let mut closed_plan_history = plan_import_timeline(true, false);
        let mut closed_behavior = crate::session::behavior::BehaviorSnapshot::normal();
        closed_behavior.last_plan_submission_id = 1;
        closed_plan_history
            .record(
                crate::session::control::SessionControlSnapshot::new(
                    2,
                    "grow-build",
                    closed_behavior,
                    None,
                )
                .timeline_kind()
                .unwrap(),
            )
            .unwrap();
        assert!(validate_import_timeline(&closed_plan_history).is_ok());

        for (action, expected_status) in [
            (
                crate::session::behavior::RetainedPlanAction::Approve,
                crate::session::behavior::RetainedPlanStatus::Approved,
            ),
            (
                crate::session::behavior::RetainedPlanAction::Abandon,
                crate::session::behavior::RetainedPlanStatus::Abandoned,
            ),
        ] {
            let mut closed = crate::session::behavior::BehaviorCoordinator::new();
            closed.select_behavior(tool_types::BehaviorId::Plan);
            closed.record_plan_artifact("# closed candidate history");
            assert!(closed.submit_initial_plan());
            let submission_id = closed.plan_submission_id().unwrap();
            if action == crate::session::behavior::RetainedPlanAction::Abandon {
                let submitted = closed.snapshot();
                assert!(matches!(
                    closed.decide_submitted_plan_if(
                        &submitted,
                        submission_id,
                        "closed-candidate-defer",
                        crate::session::behavior::RetainedPlanAction::Defer,
                        None,
                    ),
                    crate::session::behavior::RetainedPlanDecisionOutcome::Applied(
                        crate::session::behavior::RetainedPlanStatus::Deferred
                    )
                ));
                assert!(closed.finish_plan());
            }
            let expected = closed.snapshot();
            assert!(matches!(
                closed.decide_submitted_plan_if(
                    &expected,
                    submission_id,
                    "closed-candidate-decision",
                    action,
                    None,
                ),
                crate::session::behavior::RetainedPlanDecisionOutcome::Applied(status)
                    if status == expected_status
            ));
            if action == crate::session::behavior::RetainedPlanAction::Approve {
                assert!(closed.finish_plan());
            }
            let closed_snapshot = closed.snapshot();
            assert_eq!(
                closed_snapshot
                    .retained_plan
                    .as_ref()
                    .map(|candidate| candidate.status),
                Some(expected_status),
                "the regression must preserve actual closed candidate metadata"
            );
            let mut closed_candidate_history = chat_state::Timeline::default();
            closed_candidate_history
                .record(
                    crate::session::control::SessionControlSnapshot::new(
                        1,
                        "grow-build",
                        closed_snapshot,
                        None,
                    )
                    .timeline_kind()
                    .unwrap(),
                )
                .unwrap();
            validate_import_timeline(&closed_candidate_history)
                .expect("closed candidate metadata has no review or execution authority");
        }
    }

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
                budget_policy: chat_state::SidebandBudgetPolicy {
                    max_attempts: 1,
                    max_input_tokens_per_attempt: 1,
                    max_output_tokens_per_attempt: Some(1),
                },
                route: chat_state::SidebandRoute {
                    model: "test-model".into(),
                    backend: sampling_types::ApiBackend::Responses,
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
        let with_old_updates = std::collections::HashMap::from([
            (SUMMARY_COLUMN.to_string(), json!({})),
            (TIMELINE_COLUMN.to_string(), json!([])),
            (SIDEBANDS_COLUMN.to_string(), json!({})),
            (BLOBS_COLUMN.to_string(), json!({})),
            ("updates".to_string(), json!([])),
        ]);
        assert!(validate_import_state_columns(&with_old_updates).is_err());
    }

    #[test]
    fn sideband_column_is_required_and_parent_linked() {
        let (parent_id, parent_events, sideband_events) = linked_ledgers();
        let parent = chat_state::Timeline::from_events(parent_events.clone()).unwrap();
        let sideband_id = sideband_events[0].sideband_id.clone();
        let state = std::collections::HashMap::from([
            (SUMMARY_COLUMN.to_string(), json!({})),
            (
                TIMELINE_COLUMN.to_string(),
                timeline_column(&parent_events, &[]),
            ),
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
            (
                TIMELINE_COLUMN.to_string(),
                timeline_column(&parent_events, &[]),
            ),
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
    fn write_import_writes_unified_timeline_and_derived_history() {
        let tmp = tempfile::TempDir::new().unwrap();
        let relative_parent =
            Path::new("sessions").join(crate::util::grow_home::encode_cwd_dirname("/work"));
        let parent = st::ContainedDirectory::open(
            tmp.path(),
            &relative_parent,
            "session import test parent",
            true,
        )
        .unwrap();

        let mut state = std::collections::HashMap::new();
        let (parent_id, parent_events, sideband_events) = linked_ledgers();
        let info = crate::session::info::Info {
            id: acp::SessionId::new(parent_id.clone()),
            cwd: "/work".into(),
        };
        let summary = Summary::new(&info, crate::session::persistence::default_model_id()).unwrap();
        state.insert(
            SUMMARY_COLUMN.to_string(),
            serde_json::to_value(summary).unwrap(),
        );
        let sideband_id = sideband_events[0].sideband_id.clone();
        let wire_updates = ["first", "second"]
            .into_iter()
            .map(|reason| test_history_update(&parent_id, reason))
            .map(|update| {
                let envelope = st::SessionUpdateEnvelope::from_update(&update).unwrap();
                serde_json::to_value(envelope).unwrap()
            })
            .collect::<Vec<_>>();
        state.insert(
            TIMELINE_COLUMN.to_string(),
            timeline_column(&parent_events, &wire_updates),
        );
        state.insert(
            "sidebands".to_string(),
            serde_json::to_value(std::collections::BTreeMap::from([(
                sideband_id.clone(),
                sideband_events,
            )]))
            .unwrap(),
        );
        state.insert("blobs".to_string(), json!({}));
        let dir = tmp.path().join(&relative_parent).join(&parent_id);
        write_import(&parent, std::ffi::OsStr::new(&parent_id), &state).unwrap();

        assert!(dir.join("summary.json").exists(), "summary.json written");
        assert!(
            dir.join(st::SIDEBANDS_DIR)
                .join(sideband_id)
                .join(st::TIMELINE_FILE)
                .exists(),
            "sideband ledger written"
        );
        let published_summary = std::fs::read_to_string(dir.join(st::SUMMARY_FILE)).unwrap();
        assert!(!dir.join("updates.jsonl").exists());
        let imported = st::JsonlStorageAdapter::with_root(tmp.path().to_path_buf())
            .open_session_by_id(&parent_id)
            .unwrap()
            .expect("imported session is readable");
        let validated = imported.validated_timeline(&parent_id).unwrap();
        let prepared = validated
            .history
            .prepare(&imported.summary().info.id, &validated.timeline, None)
            .unwrap();
        assert_eq!(prepared.updates.len(), 2);
        assert!(
            std::fs::read_dir(tmp.path().join(&relative_parent))
                .unwrap()
                .all(|entry| {
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .ends_with("import-staging")
                })
        );

        let error = write_import(&parent, std::ffi::OsStr::new(&parent_id), &state).unwrap_err();
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
        ]);

        let error = write_import(&parent, std::ffi::OsStr::new("target-session"), &state)
            .expect_err("session import must not traverse a symlinked target");
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    }
}

/// Sampling blobs may contain malformed UTF-8 from a provider. Base64 belongs
/// only to the transport column; the immutable artifact hashes the raw bytes.
fn decode_blob<'a>(key: &str, text: &'a str) -> std::io::Result<std::borrow::Cow<'a, [u8]>> {
    if !key.starts_with("sampling/") {
        return Ok(std::borrow::Cow::Borrowed(text.as_bytes()));
    }
    let limit = crate::session::sampling_evidence::CHUNK_BYTES as u64;
    if text.len() as u64 > limit.div_ceil(3) * 4 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "encoded sampling blob exceeds limit",
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "sampling blob exceeds limit",
        ));
    }
    Ok(std::borrow::Cow::Owned(bytes))
}

#[cfg(test)]
mod sampling_blob_tests {
    use super::*;

    #[test]
    fn binary_sampling_evidence_round_trips_and_is_required() {
        let temp = tempfile::tempdir().unwrap();
        let directory =
            st::ContainedDirectory::open(temp.path(), Path::new(""), "test session", false)
                .unwrap();
        let raw = b"data: malformed \xff\x00\r\n";
        let hash = blake3::hash(raw).to_hex().to_string();
        let relative = Path::new("artifacts/sampling").join(format!("{hash}.bin"));
        crate::session::persistence::write_immutable_blob_to_directory(&directory, &relative, raw)
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        let event = timeline.prepare(chat_state::TimelineEventKind::Observation(chat_state::ObservationEvent {
            scope: "sampling_evidence".into(), name: "response".into(), turn: None, step: None,
            data: Some(json!({"owner": {"request_id": "request"}, "kind": "response", "metadata": {}, "chunks": [{"blake3": hash, "bytes": raw.len()}], "bytes": raw.len()})),
        })).unwrap();
        timeline.accept(event).unwrap();
        timeline.record(chat_state::TimelineEventKind::Observation(chat_state::ObservationEvent {
            scope: "sampling_evidence".into(), name: "recovery_stop".into(), turn: None, step: None,
            data: Some(json!({"owner": {"request_id": "request"}, "kind": "recovery_stop", "metadata": {"decision": "Fatal", "attempt": 1}, "bytes": 0, "chunks": []})),
        })).unwrap();
        crate::session::sampling_evidence::verify(&directory, &timeline).unwrap();
        let blobs = read_entity_blobs(&directory, &timeline).unwrap();
        let key = format!("sampling/{hash}");
        assert_eq!(&*decode_blob(&key, &blobs[&key]).unwrap(), raw);
        let state = std::collections::HashMap::from([(
            BLOBS_COLUMN.into(),
            serde_json::to_value(&blobs).unwrap(),
        )]);
        assert_eq!(validate_blobs_column(&state, &timeline).unwrap(), blobs);
        let missing = std::collections::HashMap::from([(BLOBS_COLUMN.into(), json!({}))]);
        assert!(validate_blobs_column(&missing, &timeline).is_err());
        std::fs::write(temp.path().join(&relative), b"changed").unwrap();
        assert!(crate::session::sampling_evidence::verify(&directory, &timeline).is_err());
        assert!(read_entity_blobs(&directory, &timeline).is_err());
        std::fs::remove_file(temp.path().join(&relative)).unwrap();
        assert!(crate::session::sampling_evidence::verify(&directory, &timeline).is_err());
    }
}

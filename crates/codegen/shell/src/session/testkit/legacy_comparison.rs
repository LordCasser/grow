//! Export a unified-journal fixture into the pre-unification layout for CLI
//! benchmark comparison only. This is test-support tooling, never a production
//! reader or compatibility path.

use crate::session::history::{HistoryContent, HistorySource};
use crate::session::storage::{
    ContainedDirectory, SUMMARY_FILE, SessionUpdate, SessionUpdateEnvelope, TIMELINE_FILE,
    UPDATES_FILE,
};
use acp_transport::protocol as acp;
use std::io;
use std::path::Path;

/// Write the old Timeline + updates.jsonl fixture form into `directory` for
/// comparison with a preserved pre-change CLI binary.
pub fn export_legacy_comparison_fixture(
    info: &crate::session::info::Info,
    directory: &Path,
) -> io::Result<()> {
    let target =
        ContainedDirectory::open(directory, Path::new(""), "legacy comparison fixture", false)?;
    let journal =
        crate::session::storage::JsonlStorageAdapter::read_journal_from_directory(&target)?;
    let timeline = chat_state::Timeline::from_events(journal.events.clone())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let responses = timeline.admitted_response_views();
    let mut updates = Vec::new();

    for source in &journal.history.sources {
        match source {
            HistorySource::Domain { event, .. } => {
                let domain = timeline
                    .events()
                    .iter()
                    .find(|candidate| candidate.seq.get() == *event)
                    .ok_or_else(|| invalid("history source references missing Domain event"))?;
                if domain
                    .messages()
                    .is_some_and(|message| message.response_admission.is_some())
                    && let Some(response) = responses
                        .iter()
                        .find(|response| response.event_seq.get() == *event)
                {
                    let projection =
                        crate::session::response_projection::project_admitted_response_view(
                            &info.id, *response,
                        )
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    updates.push(SessionUpdate::ResponseReplayProjection(Box::new(
                        projection,
                    )));
                }
            }
            HistorySource::Presentation { fact, .. } => match &fact.content {
                HistoryContent::Candidate(key) => {
                    let meta = fact.meta.clone().ok_or_else(|| {
                        invalid("candidate history fact is missing original metadata")
                    })?;
                    if meta
                        .get("samplingRequestId")
                        .and_then(serde_json::Value::as_str)
                        != Some(key.request_id.as_str())
                        || meta
                            .get("samplingAttempt")
                            .and_then(serde_json::Value::as_u64)
                            != Some(u64::from(key.attempt))
                    {
                        return Err(invalid("candidate metadata differs from its history key"));
                    }
                    let mut notification = acp::SessionNotification::new(
                        info.id.clone(),
                        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                            acp::ContentBlock::Text(acp::TextContent::new(String::new())),
                        )),
                    );
                    notification.meta = Some(meta);
                    updates.push(SessionUpdate::Acp(Box::new(notification)));
                }
                HistoryContent::Message { .. } | HistoryContent::Notice { .. } => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "legacy comparison export does not support Message or Notice references",
                    ));
                }
                _ => {
                    if let Some(update) = fact.clone().into_update(&info.id)? {
                        updates.push(update);
                    }
                }
            },
        }
    }

    let envelopes = updates
        .iter()
        .map(SessionUpdateEnvelope::from_update)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let timeline_bytes = crate::session::storage::to_jsonl_bytes(&journal.events)?;
    let updates_bytes = crate::session::storage::to_jsonl_bytes(&envelopes)?;

    let summary_bytes = target.read_bounded(
        std::ffi::OsStr::new(SUMMARY_FILE),
        "session summary",
        crate::session::storage::MAX_SESSION_SUMMARY_BYTES,
    )?;
    let mut summary: crate::session::persistence::Summary = serde_json::from_slice(&summary_bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    summary.validate_current_format()?;
    if summary.info.id != info.id || summary.info.cwd != info.cwd {
        return Err(invalid(
            "fixture summary identity differs from requested session",
        ));
    }
    summary.session_format_version = 6;
    let mut summary_bytes = serde_json::to_vec_pretty(&summary)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    summary_bytes.push(b'\n');
    if summary_bytes.len() as u64 > crate::session::storage::MAX_SESSION_SUMMARY_BYTES {
        return Err(invalid("legacy comparison summary exceeds the byte limit"));
    }

    target.write_atomic(
        std::ffi::OsStr::new(TIMELINE_FILE),
        &timeline_bytes,
        true,
        true,
    )?;
    target.write_atomic(
        std::ffi::OsStr::new(UPDATES_FILE),
        &updates_bytes,
        true,
        true,
    )?;
    target.write_atomic(
        std::ffi::OsStr::new(SUMMARY_FILE),
        &summary_bytes,
        true,
        true,
    )?;
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

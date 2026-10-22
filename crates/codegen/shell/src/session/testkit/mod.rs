//! Session synthesis and in-process e2e harness for the load-perf and fork
//! bench tests.
//!
//! Lives in `shell` (feature `test-support`) rather than
//! `test-support` because synthesis drives the real
//! `JsonlStorageAdapter`; the reverse dependency would be circular.

pub mod e2e;
pub mod legacy_comparison;
pub mod synth;

/// Write a strict typed presentation fixture into the unified Timeline journal.
///
/// This test-support helper rejects transient or unsupported notifications and
/// never writes the retired `updates.jsonl` representation.
#[cfg(feature = "test-support")]
pub fn write_timeline_history_fixture(
    directory: &std::path::Path,
    events: &[chat_state::TimelineEvent],
    updates: &[crate::session::storage::SessionUpdate],
) -> std::io::Result<()> {
    let facts = updates
        .iter()
        .map(|update| {
            crate::session::history::HistoryFact::from_update(update)?.ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "transient update cannot be written as a presentation history fact",
                )
            })
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let bytes = crate::session::storage::JsonlStorageAdapter::encode_journal(events, &facts)?;
    std::fs::write(
        directory.join(crate::session::storage::TIMELINE_FILE),
        bytes,
    )
}

/// Decode a physical Timeline journal fixture through the production checksum
/// codec, then validate its domain events as a typed Timeline.
#[cfg(feature = "test-support")]
pub fn decode_timeline_fixture(contents: &str) -> std::io::Result<chat_state::Timeline> {
    let complete_len = contents.rfind('\n').map_or(0, |position| position + 1);
    let lines = contents[..complete_len]
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let journal = crate::session::storage::JsonlStorageAdapter::decode_journal_lines(&lines)?;
    chat_state::Timeline::from_events(journal.events).map_err(std::io::Error::other)
}

/// Read a physical Timeline fixture with the production journal decoder.
#[cfg(feature = "test-support")]
pub fn read_timeline_fixture(path: &std::path::Path) -> std::io::Result<chat_state::Timeline> {
    decode_timeline_fixture(&std::fs::read_to_string(path)?)
}

/// Append typed domain events to a physical Timeline test fixture while
/// preserving its existing interleaved presentation records and checksums.
#[cfg(feature = "test-support")]
pub fn append_timeline_domain_fixture(
    path: &std::path::Path,
    events: &[chat_state::TimelineEvent],
) -> std::io::Result<()> {
    use crate::session::history::{self, JournalFact, ResponseAnchor};
    use std::io::Write as _;

    let contents = std::fs::read_to_string(path)?;
    if !contents.is_empty() && !contents.ends_with('\n') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "cannot append a Timeline fixture after an incomplete record",
        ));
    }
    let lines = contents.lines().map(str::to_owned).collect::<Vec<_>>();
    let mut journal = crate::session::storage::JsonlStorageAdapter::decode_journal_lines(&lines)?;
    let mut timeline =
        chat_state::Timeline::from_events(journal.events.clone()).map_err(std::io::Error::other)?;
    let mut encoded = Vec::new();

    for event in events {
        let expected_seq = timeline.events().len() as u64;
        if event.seq.get() != expected_seq {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "fixture Timeline domain sequence is not contiguous",
            ));
        }
        timeline
            .accept(event.clone())
            .map_err(std::io::Error::other)?;
        let raw = serde_json::value::to_raw_value(event).map_err(std::io::Error::other)?;
        let response_anchor = event
            .messages()
            .and_then(|message| message.response_admission.as_ref())
            .map(|_| ResponseAnchor::Admission);
        let record = history::JournalRecord {
            seq: journal.state.records,
            fact: JournalFact::Domain {
                event: event.clone(),
                response_anchor,
            },
            digest: blake3::hash(raw.get().as_bytes()),
        };
        journal.state.validate(&record)?;
        encoded.extend(history::encode_domain(
            journal.state.records,
            &raw,
            response_anchor,
        )?);
        journal.state.accept(&record)?;
    }

    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    file.write_all(&encoded)?;
    file.flush()
}

/// Project typed notifications through the physical presentation-fact journal
/// and the production History projector without touching disk.
pub fn project_presentation_history(
    session_id: &acp_transport::protocol::SessionId,
    updates: &[crate::session::storage::SessionUpdate],
) -> std::io::Result<Vec<crate::session::storage::SessionUpdate>> {
    let timeline = chat_state::Timeline::default();
    let mut state = crate::session::history::AppendState::default();
    let mut history = crate::session::history::HistorySnapshot::default();

    for (seq, update) in updates.iter().enumerate() {
        let fact = crate::session::history::HistoryFact::from_update(update)?
            .ok_or_else(|| std::io::Error::other("update has no durable history fact"))?;
        let raw = serde_json::value::to_raw_value(&fact).map_err(std::io::Error::other)?;
        let encoded = crate::session::history::encode_presentation(seq as u64, seq as i64, &raw)?;
        let bytes = encoded
            .strip_suffix(b"\n")
            .ok_or_else(|| std::io::Error::other("encoded Timeline record lacks newline"))?;
        let record = crate::session::history::decode_record(bytes)?;
        state.accept(&record)?;
        history.push(&record);
    }

    history
        .prepare(session_id, &timeline, None)
        .map(|prepared| prepared.updates)
}

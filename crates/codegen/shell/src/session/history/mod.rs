//! The physical Timeline journal and its read-only presentation projection.
//!
//! ChatState keeps its domain sequence. The journal also orders the finite
//! presentation facts that cannot be inferred from model context. No ACP
//! method/params envelope or second response body is stored here.

mod grow_facts;
pub(crate) use grow_facts::GrowHistoryFact;
mod projector;
pub(crate) use projector::{HISTORY_CURSOR_KEY, PreparedHistory};

use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::io;

pub(crate) const RECORD_VERSION: u8 = 1;
pub(crate) const PROJECTOR_VERSION: u8 = 1;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResponseKey {
    pub request_id: String,
    pub attempt: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ResponseAnchor {
    Candidate {
        record: u64,
    },
    /// No candidate was emitted. The committed response itself is the first
    /// available display source; it must not borrow an earlier position.
    Admission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HistoryFact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<acp::Meta>,
    pub content: HistoryContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum HistoryContent {
    Candidate(ResponseKey),
    User(acp::ContentChunk),
    Assistant(acp::ContentChunk),
    Reasoning(acp::ContentChunk),
    ToolCall(acp::ToolCall),
    ToolUpdate(acp::ToolCallUpdate),
    Plan(acp::Plan),
    Mode(acp::CurrentModeUpdate),
    // Keep sparse control payloads from inflating every text/anchor source
    // slot in the history vector. Box is transparent in the storage encoding.
    Grow(Box<GrowHistoryFact>),
    Notice {
        event: u64,
    },
    /// A fork owns this display reference, but acquires no parent admission
    /// or execution identity. Only a seed/ordinary message can be referenced.
    Message {
        event: u64,
        item: usize,
        channel: TextChannel,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chunk_meta: Option<acp::Meta>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TextChannel {
    User,
    Assistant,
    Reasoning,
}

impl HistoryFact {
    pub(crate) fn event_id(&self) -> Option<&str> {
        self.meta.as_ref()?.get("eventId")?.as_str()
    }

    pub(crate) fn from_update(
        update: &crate::session::storage::SessionUpdate,
    ) -> io::Result<Option<Self>> {
        use crate::session::storage::SessionUpdate;
        let (meta, content) = match update {
            SessionUpdate::Acp(notification) => {
                let meta = notification.meta.clone();
                if let Some((request_id, attempt)) =
                    crate::session::persistence::sampling_attempt_key(notification)
                {
                    return Ok(Some(Self {
                        meta,
                        content: HistoryContent::Candidate(ResponseKey {
                            request_id,
                            attempt,
                        }),
                    }));
                }
                let content = match &notification.update {
                    acp::SessionUpdate::UserMessageChunk(value) => {
                        HistoryContent::User(value.clone())
                    }
                    acp::SessionUpdate::AgentMessageChunk(value) => {
                        HistoryContent::Assistant(value.clone())
                    }
                    acp::SessionUpdate::AgentThoughtChunk(value) => {
                        HistoryContent::Reasoning(value.clone())
                    }
                    acp::SessionUpdate::ToolCall(value) => HistoryContent::ToolCall(value.clone()),
                    acp::SessionUpdate::ToolCallUpdate(value) => {
                        HistoryContent::ToolUpdate(value.clone())
                    }
                    acp::SessionUpdate::Plan(value) => HistoryContent::Plan(value.clone()),
                    acp::SessionUpdate::CurrentModeUpdate(value) => {
                        HistoryContent::Mode(value.clone())
                    }
                    acp::SessionUpdate::AvailableCommandsUpdate(_) => return Ok(None),
                    _ => return Err(invalid("unsupported persistent ACP presentation kind")),
                };
                (meta, content)
            }
            SessionUpdate::Grow(notification) => (
                notification
                    .meta
                    .clone()
                    .map(|value| {
                        value
                            .as_object()
                            .cloned()
                            .ok_or_else(|| invalid("Grow metadata must be an object"))
                    })
                    .transpose()?,
                HistoryContent::Grow(Box::new(
                    GrowHistoryFact::try_from(notification.update.clone()).map_err(invalid)?,
                )),
            ),
            #[cfg(any(test, feature = "test-support"))]
            SessionUpdate::ResponseReplayProjection(_) => {
                return Err(invalid("response projections are not journal facts"));
            }
        };
        Ok(Some(Self { meta, content }))
    }

    pub(crate) fn into_update(
        self,
        session_id: &acp::SessionId,
    ) -> io::Result<Option<crate::session::storage::SessionUpdate>> {
        use crate::session::storage::SessionUpdate;
        let update = match self.content {
            HistoryContent::Candidate(_) => return Ok(None),
            HistoryContent::Notice { .. } => {
                return Err(invalid("notice reference requires the Timeline projector"));
            }
            HistoryContent::User(value) => acp::SessionUpdate::UserMessageChunk(value),
            HistoryContent::Assistant(value) => acp::SessionUpdate::AgentMessageChunk(value),
            HistoryContent::Reasoning(value) => acp::SessionUpdate::AgentThoughtChunk(value),
            HistoryContent::ToolCall(value) => acp::SessionUpdate::ToolCall(value),
            HistoryContent::ToolUpdate(value) => acp::SessionUpdate::ToolCallUpdate(value),
            HistoryContent::Plan(value) => acp::SessionUpdate::Plan(value),
            HistoryContent::Mode(value) => acp::SessionUpdate::CurrentModeUpdate(value),
            HistoryContent::Grow(value) => {
                return Ok(Some(SessionUpdate::Grow(Box::new(
                    crate::extensions::notification::SessionNotification {
                        session_id: session_id.clone(),
                        update: (*value).into(),
                        meta: self.meta.map(serde_json::Value::Object),
                    },
                ))));
            }
            HistoryContent::Message { .. } => {
                return Err(invalid("message reference needs a validated Timeline"));
            }
        };
        Ok(Some(SessionUpdate::Acp(Box::new(
            acp::SessionNotification::new(session_id.clone(), update).meta(self.meta),
        ))))
    }
}

/// The checksum covers the exact stored fact bytes plus the fixed version and
/// physical position. It never re-serializes a decoded domain response.
fn checksum(seq: u64, fact: &RawValue) -> blake3::Hash {
    let mut hash = blake3::Hasher::new();
    hash.update(b"grow.timeline.record\0");
    hash.update(&[RECORD_VERSION]);
    hash.update(&seq.to_le_bytes());
    hash.update(fact.get().as_bytes());
    hash.finalize()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record<'a> {
    version: u8,
    seq: u64,
    checksum: String,
    #[serde(borrow)]
    fact: &'a RawValue,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum FactWire<'a> {
    Domain {
        event: &'a RawValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_anchor: Option<ResponseAnchor>,
    },
    Presentation {
        at_ms: i64,
        event: &'a RawValue,
    },
}

// Internally tagged enum deserialization buffers serde::Content, which cannot
// borrow a RawValue. A flat typed header keeps the actual payload on its input
// bytes rather than silently allocating a JSON value tree.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum FactKind {
    Domain,
    Presentation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FactHeader<'a> {
    kind: FactKind,
    #[serde(borrow)]
    event: &'a RawValue,
    #[serde(default)]
    response_anchor: Option<ResponseAnchor>,
    #[serde(default)]
    at_ms: Option<i64>,
}

#[expect(
    clippy::large_enum_variant,
    reason = "A journal fact is a transient decode/append value, not a retained history slot; keep presentation payloads inline to avoid an allocation per record"
)]
#[derive(Debug, Clone)]
pub(crate) enum JournalFact {
    Domain {
        event: chat_state::TimelineEvent,
        response_anchor: Option<ResponseAnchor>,
    },
    Presentation {
        at_ms: i64,
        event: HistoryFact,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct JournalRecord {
    pub seq: u64,
    pub fact: JournalFact,
    pub digest: blake3::Hash,
}

pub(crate) fn decode_record(bytes: &[u8]) -> io::Result<JournalRecord> {
    #[derive(Deserialize)]
    struct Version {
        version: u64,
    }
    let version: Version =
        serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?;
    if version.version != u64::from(RECORD_VERSION) {
        return Err(crate::session::persistence::session_version_mismatch(
            "Timeline storage",
            version.version,
            u64::from(RECORD_VERSION),
        ));
    }
    let record: Record<'_> =
        serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?;
    if checksum(record.seq, record.fact).to_hex().as_str() != record.checksum {
        return Err(invalid("Timeline record checksum mismatch"));
    }
    let fact: FactHeader<'_> =
        serde_json::from_str(record.fact.get()).map_err(|error| invalid(error.to_string()))?;
    let event = fact.event;
    let (fact, digest) = match fact.kind {
        FactKind::Domain => {
            if fact.at_ms.is_some() {
                return Err(invalid("domain fact has presentation timestamp"));
            }
            let response_anchor = fact.response_anchor;
            let version: Version = serde_json::from_str(event.get()).map_err(io::Error::other)?;
            if version.version != u64::from(chat_state::TIMELINE_SCHEMA_VERSION) {
                return Err(crate::session::persistence::session_version_mismatch(
                    "Timeline schema",
                    version.version,
                    u64::from(chat_state::TIMELINE_SCHEMA_VERSION),
                ));
            }
            let domain =
                serde_json::from_str(event.get()).map_err(|error| invalid(error.to_string()))?;
            (
                JournalFact::Domain {
                    event: domain,
                    response_anchor,
                },
                blake3::hash(event.get().as_bytes()),
            )
        }
        FactKind::Presentation => {
            if fact.response_anchor.is_some() {
                return Err(invalid("presentation fact has domain admission anchor"));
            }
            let at_ms = fact
                .at_ms
                .ok_or_else(|| invalid("presentation timestamp is missing"))?;
            if at_ms < 0 {
                return Err(invalid("negative presentation timestamp"));
            }
            let presentation =
                serde_json::from_str(event.get()).map_err(|error| invalid(error.to_string()))?;
            (
                JournalFact::Presentation {
                    at_ms,
                    event: presentation,
                },
                blake3::hash(event.get().as_bytes()),
            )
        }
    };
    Ok(JournalRecord {
        seq: record.seq,
        fact,
        digest,
    })
}

pub(crate) fn encode_domain(
    seq: u64,
    event: &RawValue,
    response_anchor: Option<ResponseAnchor>,
) -> io::Result<Vec<u8>> {
    encode(
        seq,
        &FactWire::Domain {
            event,
            response_anchor,
        },
    )
}

pub(crate) fn encode_presentation(seq: u64, at_ms: i64, event: &RawValue) -> io::Result<Vec<u8>> {
    encode(seq, &FactWire::Presentation { at_ms, event })
}

fn encode(seq: u64, fact: &FactWire<'_>) -> io::Result<Vec<u8>> {
    let fact = serde_json::value::to_raw_value(fact).map_err(|error| invalid(error.to_string()))?;
    let record = Record {
        version: RECORD_VERSION,
        seq,
        checksum: checksum(seq, &fact).to_hex().to_string(),
        fact: &fact,
    };
    let mut bytes = serde_json::to_vec(&record).map_err(|error| invalid(error.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AppendState {
    pub records: u64,
    pub events: u64,
    pub display_records: usize,
    pub anchors: BTreeMap<ResponseKey, u64>,
    pub identities: BTreeMap<String, blake3::Hash>,
    pub last_domain: Option<(u64, blake3::Hash, Option<ResponseAnchor>)>,
    pub receipts: BTreeMap<
        (String, Option<String>, Option<String>),
        crate::session::storage::communication_history::ReceiptSource,
    >,
}

impl AppendState {
    pub(crate) fn index_receipt(&mut self, event: &chat_state::TimelineEvent) -> io::Result<()> {
        if let Some(identity) =
            crate::session::storage::communication_history::receipt_identity(event)?
        {
            self.receipts.entry(identity).or_insert(
                crate::session::storage::communication_history::ReceiptSource::from_event(event)?,
            );
        }
        Ok(())
    }

    fn receipt_for_notice(
        &self,
        notice: &crate::extensions::notification::UiNotice,
    ) -> io::Result<Option<&crate::session::storage::communication_history::ReceiptSource>> {
        let identity = crate::session::storage::communication_history::notice_identity(notice);
        if let Some(source) = self.receipts.get(&identity) {
            return Ok(Some(source));
        }
        let lower = (identity.0.clone(), identity.1.clone(), None);
        if self
            .receipts
            .range(lower..)
            .next()
            .is_some_and(|(key, _)| key.0 == identity.0 && key.1 == identity.1)
        {
            return Err(invalid(
                "notice source does not match its canonical receipt",
            ));
        }
        Ok(None)
    }

    pub(crate) fn bind_presentation(&self, fact: &mut HistoryFact) -> io::Result<()> {
        if let HistoryContent::Grow(value) = &fact.content
            && let GrowHistoryFact::UiNotice(notice) = value.as_ref()
            && let Some(source) = self.receipt_for_notice(notice)?
        {
            source.validate_notice(notice)?;
            fact.content = HistoryContent::Notice {
                event: source.event,
            };
        }
        Ok(())
    }

    pub(crate) fn validate(&self, record: &JournalRecord) -> io::Result<()> {
        if record.seq != self.records {
            return Err(invalid("non-contiguous physical Timeline position"));
        }
        match &record.fact {
            JournalFact::Domain {
                event,
                response_anchor,
            } => {
                if event.seq.get() != self.events {
                    return Err(invalid("non-contiguous domain Timeline sequence"));
                }
                let admission = event
                    .messages()
                    .and_then(|message| message.response_admission.as_ref());
                match (admission, response_anchor) {
                    (Some(admission), Some(ResponseAnchor::Candidate { record })) => {
                        let key = ResponseKey {
                            request_id: admission.identity.request_id.clone(),
                            attempt: admission.identity.attempt,
                        };
                        if self.anchors.get(&key) != Some(record) {
                            return Err(invalid(
                                "response references a missing or different candidate anchor",
                            ));
                        }
                    }
                    (Some(_), Some(ResponseAnchor::Admission)) | (None, None) => {}
                    _ => return Err(invalid("response admission has no closed display anchor")),
                }
            }
            JournalFact::Presentation { event, .. } => {
                if let HistoryContent::Grow(value) = &event.content
                    && let GrowHistoryFact::UiNotice(notice) = value.as_ref()
                    && self.receipt_for_notice(notice)?.is_some()
                {
                    return Err(invalid(
                        "canonical receipt must use its validated history reference",
                    ));
                }
                if let HistoryContent::Notice { event } = &event.content
                    && *event >= self.events
                {
                    return Err(invalid("notice references an absent communication receipt"));
                }
                if let Some(id) = event.event_id() {
                    if id.is_empty() || id.len() > 512 || id.chars().any(char::is_control) {
                        return Err(invalid("invalid presentation identity"));
                    }
                    if self.identities.contains_key(id) {
                        return Err(invalid("duplicate presentation identity"));
                    }
                }
                if let HistoryContent::Candidate(key) = &event.content
                    && (key.request_id.is_empty()
                        || key.attempt == 0
                        || self.anchors.contains_key(key))
                {
                    return Err(invalid("invalid or duplicate candidate anchor"));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn accept(&mut self, record: &JournalRecord) -> io::Result<()> {
        self.validate(record)?;
        match &record.fact {
            JournalFact::Domain {
                event,
                response_anchor,
            } => {
                self.index_receipt(event)?;
                self.last_domain = Some((event.seq.get(), record.digest, *response_anchor));
                self.events += 1;
                self.display_records += usize::from(response_anchor.is_some());
            }
            JournalFact::Presentation { event, .. } => {
                self.display_records +=
                    usize::from(!matches!(event.content, HistoryContent::Candidate(_)));
                if let Some(id) = event.event_id() {
                    self.identities.insert(id.to_owned(), record.digest);
                }
                if let HistoryContent::Candidate(key) = &event.content {
                    self.anchors.insert(key.clone(), record.seq);
                }
            }
        }
        self.records += 1;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) enum HistorySource {
    Domain {
        event: u64,
        response_anchor: Option<ResponseAnchor>,
    },
    Presentation {
        at_ms: i64,
        // Independent presentation payloads are consumed by owned replay.
        // Keep the physical position vector compact as each payload is moved.
        fact: Box<HistoryFact>,
    },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct HistorySnapshot {
    pub sources: Vec<HistorySource>,
    pub bytes: u64,
    pub prefix_hash: String,
    pub notices: BTreeMap<u64, crate::extensions::notification::UiNotice>,
}

impl HistorySnapshot {
    pub(crate) fn load_notices(
        &mut self,
        directory: &crate::session::storage::ContainedDirectory,
        timeline: &chat_state::Timeline,
        session: &str,
    ) -> io::Result<()> {
        Self::validate_owner(timeline, session)?;
        let mut seen = std::collections::BTreeSet::new();
        for event in timeline.events() {
            if let Some(identity) =
                crate::session::storage::communication_history::receipt_identity(event)?
            {
                if !seen.insert(identity) {
                    continue;
                }
                if let Some(notice) =
                    crate::session::storage::communication_history::read_notice(directory, event)?
                {
                    self.notices.insert(event.seq.get(), notice);
                }
            }
        }
        Ok(())
    }
    pub(crate) fn validate_owner(timeline: &chat_state::Timeline, session: &str) -> io::Result<()> {
        for event in timeline.events() {
            if let chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Received {
                    owner_session_id, ..
                },
            ) = &event.kind
                && owner_session_id != session
            {
                return Err(invalid("foreign communication receipt in session history"));
            }
        }
        Ok(())
    }

    pub(crate) fn validate(&self, timeline: &chat_state::Timeline) -> io::Result<()> {
        let mut receipt_sources = BTreeMap::new();
        for event in timeline.events() {
            if let Some(identity) =
                crate::session::storage::communication_history::receipt_identity(event)?
            {
                receipt_sources.entry(identity).or_insert(event.seq.get());
            }
        }
        let mut echoes = BTreeMap::<String, (usize, Vec<serde_json::Value>)>::new();
        for source in &self.sources {
            let HistorySource::Presentation { fact, .. } = source else {
                continue;
            };
            if let Some(meta) = &fact.meta {
                match (
                    meta.get("grow/displayProjectionSource"),
                    meta.get("grow/displayProjectionRevision"),
                ) {
                    (None, None) => {}
                    (Some(source), Some(revision))
                        if source.as_str().is_some_and(|source| !source.is_empty())
                            && revision.as_u64().is_some() => {}
                    _ => return Err(invalid("invalid snapshot source identity or revision")),
                }
            }
            if let HistoryContent::Notice { event } = &fact.content {
                let event = timeline
                    .events()
                    .get(*event as usize)
                    .ok_or_else(|| invalid("notice source is missing"))?;
                let identity =
                    crate::session::storage::communication_history::receipt_identity(event)?
                        .ok_or_else(|| invalid("notice source is not a communication receipt"))?;
                if receipt_sources.get(&identity) != Some(&event.seq.get()) {
                    return Err(invalid(
                        "notice reference does not select its original receipt",
                    ));
                }
            }
            if let HistoryContent::Message {
                event,
                item,
                channel,
                ..
            } = &fact.content
            {
                let message = timeline
                    .events()
                    .get(*event as usize)
                    .filter(|event_value| event_value.seq.get() == *event)
                    .and_then(|event| event.messages())
                    .filter(|message| message.response_admission.is_none())
                    .ok_or_else(|| invalid("invalid inherited history source"))?;
                if !matches!(
                    (channel, message.items.get(*item)),
                    (
                        TextChannel::User,
                        Some(sampling_types::ConversationItem::User(_))
                    ) | (
                        TextChannel::Assistant,
                        Some(sampling_types::ConversationItem::Assistant(_))
                    ) | (
                        TextChannel::Reasoning,
                        Some(sampling_types::ConversationItem::Reasoning(_))
                    )
                ) {
                    return Err(invalid("inherited history item or channel is invalid"));
                }
            }
            let HistoryContent::User(chunk) = &fact.content else {
                continue;
            };
            let Some(meta) = &chunk.meta else { continue };
            let Some(prompt) = meta
                .get("promptIndex")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            let Some(message) = meta.get("messageId").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let echo = echoes
                .entry(message.to_owned())
                .or_insert_with(|| (prompt, Vec::new()));
            if echo.0 != prompt {
                return Err(invalid(
                    "user echo identity carries multiple prompt indices",
                ));
            }
            echo.1
                .push(serde_json::to_value(&chunk.content).map_err(io::Error::other)?);
        }
        let has_committed_echoes = timeline.events().iter().any(|event| {
            matches!(
                &event.kind,
                chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                    display_echo: Some(_),
                    ..
                })
            )
        });
        let selected_echo_events = if has_committed_echoes {
            timeline
                .branch_transcript_with_ids()
                .0
                .into_iter()
                .filter(|id| id.item == 0)
                .map(|id| id.event.get())
                .collect::<std::collections::BTreeSet<_>>()
        } else {
            Default::default()
        };
        for event in timeline.events() {
            if let chat_state::TimelineEventKind::DisplayProjectionExpected(_) = &event.kind {
                return Err(invalid(
                    "digest-only display expectations are not valid unified facts",
                ));
            }
            let chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                display_echo: Some(expected),
                ..
            }) = &event.kind
            else {
                continue;
            };
            if !selected_echo_events.contains(&event.seq.get()) {
                continue;
            }
            let (prompt, blocks) = echoes
                .get(&expected.message_id)
                .ok_or_else(|| invalid("committed user echo is missing"))?;
            let mut value = serde_json::Value::Array(blocks.clone());
            value.sort_all_objects();
            let bytes = serde_json::to_vec(&value).map_err(io::Error::other)?;
            if *prompt != expected.prompt_index
                || blocks.len() != expected.block_count
                || blake3::hash(&bytes).to_hex().as_str() != expected.blake3
            {
                return Err(invalid(
                    "committed user echo differs from its source fingerprint",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn display_record_count(&self) -> usize {
        self.sources
            .iter()
            .filter(|source| match source {
                HistorySource::Domain {
                    response_anchor, ..
                } => response_anchor.is_some(),
                HistorySource::Presentation { fact, .. } => {
                    !matches!(fact.content, HistoryContent::Candidate(_))
                }
            })
            .count()
    }

    fn event_counter_floor(&self) -> u64 {
        let max = self
            .sources
            .iter()
            .filter_map(|source| {
                let HistorySource::Presentation { fact, .. } = source else {
                    return None;
                };
                fact.event_id()?.rsplit_once('-')?.1.parse::<u64>().ok()
            })
            .max();
        // Synthesized receipt IDs end in their domain position. Cover every
        // physical position even when no presentation row exists (noReplay or
        // a hidden branch must not let later live events fall below that UI highwater).
        max.map_or(self.sources.len() as u64, |max| {
            max.saturating_add(1).max(self.sources.len() as u64)
        })
    }

    pub(crate) fn reseed_event_counter(&self) {
        crate::util::event_id::ensure_event_counter_at_least(self.event_counter_floor());
    }
    pub(crate) fn push(&mut self, record: &JournalRecord) {
        self.sources.push(match &record.fact {
            JournalFact::Domain {
                event,
                response_anchor,
            } => HistorySource::Domain {
                event: event.seq.get(),
                response_anchor: *response_anchor,
            },
            JournalFact::Presentation { at_ms, event } => HistorySource::Presentation {
                at_ms: *at_ms,
                fact: Box::new(event.clone()),
            },
        });
    }
}

#[cfg(test)]
mod tests;

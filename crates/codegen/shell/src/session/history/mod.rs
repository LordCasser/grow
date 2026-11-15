//! The physical Timeline journal and its read-only presentation projection.
//!
//! ChatState keeps its domain sequence. The journal also orders the finite
//! presentation facts that cannot be inferred from model context. No ACP
//! method/params envelope or second response body is stored here.

mod grow_facts;
pub(crate) use grow_facts::GrowHistoryFact;

pub(crate) fn indexes_background_task(event: &HistoryFact) -> bool {
    matches!(&event.content, HistoryContent::Grow(value)
        if matches!(value.as_ref(), GrowHistoryFact::TaskBackgrounded { .. } | GrowHistoryFact::TaskCompleted { .. }))
        && event.meta.as_ref().is_none_or(|meta| {
            !meta.contains_key("grow/displayProjectionSource")
                && !meta.contains_key("grow/displayProjectionRevision")
        })
}
mod projector;
pub(crate) use projector::{
    HISTORY_CURSOR_KEY, IndexedPages, PagePosition, PreparedHistory, background_command_settlements,
};
#[cfg(unix)]
pub(crate) use projector::command_settlement_update;
#[cfg(test)]
pub(crate) use projector::{HISTORY_PAGE_BYTES, HISTORY_PAGE_ROWS};
mod seed;
pub(crate) use seed::SeedPresentation;

use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::io;

pub(crate) const RECORD_VERSION: u8 = 1;
pub(crate) const PROJECTOR_VERSION: u8 = 3;

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

pub(crate) fn user_echo_identity(chunk: &acp::ContentChunk) -> Option<(&str, usize)> {
    let meta = chunk.meta.as_ref()?;
    let id = meta.get("messageId")?.as_str()?;
    let prompt = usize::try_from(meta.get("promptIndex")?.as_u64()?).ok()?;
    Some((id, prompt))
}

pub(crate) fn provisional_user_echo(chunk: &acp::ContentChunk) -> bool {
    chunk.meta.as_ref()
        .and_then(|meta| meta.get("grow/consumedInputEcho"))
        .and_then(serde_json::Value::as_bool) == Some(true)
}

pub(crate) fn verify_user_echo(
    expected: &chat_state::UserDisplayEcho,
    blocks: &[(usize, serde_json::Value)],
) -> io::Result<()> {
    let mut value = serde_json::Value::Array(
        blocks.iter().map(|(_, content)| content.clone()).collect(),
    );
    value.sort_all_objects();
    let bytes = serde_json::to_vec(&value).map_err(io::Error::other)?;
    if blocks.len() != expected.block_count
        || blocks.iter().any(|(prompt, _)| *prompt != expected.prompt_index)
        || blake3::hash(&bytes).to_hex().as_str() != expected.blake3
    {
        return Err(invalid("committed user echo differs from its source fingerprint"));
    }
    Ok(())
}

impl HistoryFact {
    pub(crate) fn inherited_item<'a>(
        &self,
        source: &'a chat_state::TimelineEvent,
    ) -> io::Result<&'a sampling_types::ConversationItem> {
        use sampling_types::ConversationItem;
        let HistoryContent::Message { event, item, channel, .. } = &self.content else {
            unreachable!("only inherited history refers to another message");
        };
        let item = source.messages()
            .filter(|message| source.seq.get() == *event && message.response_admission.is_none())
            .and_then(|message| message.items.get(*item))
            .ok_or_else(|| invalid("invalid inherited history source or item"))?;
        match (channel, item) {
            (TextChannel::User, ConversationItem::User(_))
            | (TextChannel::Assistant, ConversationItem::Assistant(_))
            | (TextChannel::Reasoning, ConversationItem::Reasoning(_)) => Ok(item),
            _ => Err(invalid("inherited history channel differs from canonical item")),
        }
    }

    pub(crate) fn validate_projection_identity(&self) -> io::Result<()> {
        if let Some(meta) = &self.meta {
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
        Ok(())
    }

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

pub(crate) fn event_counter_after(id: &str) -> Option<u64> {
    let counter = id.rsplit_once('-')?.1.parse::<u64>().ok()?;
    Some(counter.saturating_add(1))
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

#[cfg(unix)]
pub(crate) fn response_item_tokens(bytes: &[u8]) -> io::Result<Vec<&RawValue>> {
    #[derive(Deserialize)]
    struct Domain<'a> {
        #[serde(borrow)]
        event: Message<'a>,
    }
    #[derive(Deserialize)]
    struct Message<'a> {
        #[serde(borrow)]
        event: Items<'a>,
    }
    #[derive(Deserialize)]
    struct Items<'a> {
        #[serde(borrow)]
        items: Vec<&'a RawValue>,
    }
    let record: Record<'_> = serde_json::from_slice(bytes)?;
    let domain: Domain<'_> = serde_json::from_str(record.fact.get())?;
    Ok(domain.event.event.items)
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
    pub first_domain_at_ms: Option<i64>,
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
            && let std::collections::btree_map::Entry::Vacant(entry) = self.receipts.entry(identity)
        {
            entry.insert(crate::session::storage::communication_history::ReceiptSource::from_event(event)?);
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
        self.bind_indexed_presentation(fact, None)
    }

    pub(crate) fn bind_indexed_presentation(
        &self, fact: &mut HistoryFact,
        index: Option<&crate::session::storage::jsonl::IndexedHistory>,
    ) -> io::Result<()> {
        if let HistoryContent::Grow(value) = &fact.content
            && let GrowHistoryFact::UiNotice(notice) = value.as_ref()
            && let Some(source) = self.find_notice_source(notice, index)?
        {
            source.validate_notice(notice)?;
            fact.content = HistoryContent::Notice {
                event: source.event,
            };
        }
        Ok(())
    }

    fn find_notice_source(
        &self, notice: &crate::extensions::notification::UiNotice,
        index: Option<&crate::session::storage::jsonl::IndexedHistory>,
    ) -> io::Result<Option<crate::session::storage::communication_history::ReceiptSource>> {
        if let Some(index) = index {
            let identity = crate::session::storage::communication_history::notice_identity(notice);
            if let Some(source) = index.notice_source(&identity, false)? {
                return Ok(Some(source));
            }
        }
        if let Some(source) = self.receipt_for_notice(notice)? {
            return Ok(Some(source.clone()));
        }
        match index {
            Some(index) => index.notice_source(
                &crate::session::storage::communication_history::notice_identity(notice), true,
            ),
            None => Ok(None),
        }
    }

    pub(crate) fn validate_indexed_presentation(
        &self, fact: &HistoryFact, index: &crate::session::storage::jsonl::IndexedHistory,
    ) -> io::Result<()> {
        if let HistoryContent::Grow(value) = &fact.content
            && let GrowHistoryFact::UiNotice(notice) = value.as_ref()
            && self.find_notice_source(notice, Some(index))?.is_some()
        {
            return Err(invalid("canonical receipt must use its validated history reference"));
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
                self.first_domain_at_ms.get_or_insert(event.at_ms);
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
    pub indexed: Option<crate::session::storage::jsonl::IndexedHistory>,
    pub positions: Vec<u64>,
    pub(crate) notice_anchors_in_tail: std::collections::BTreeSet<u64>,
    pub(crate) response_branch: Option<chat_state::EventSeq>,
}

impl HistoryFact {
    pub(crate) fn recap_covered_main_turns(&self) -> Option<usize> {
        if let HistoryContent::Grow(value) = &self.content
            && matches!(value.as_ref(), GrowHistoryFact::SessionRecap { .. })
        {
            usize::try_from(self.meta.as_ref()?.get("recapCoveredMainTurns")?.as_u64()?).ok()
        } else {
            None
        }
    }
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
                if let Some(index) = &self.indexed
                    && (event.seq.get() < index.published_domain()
                        || index.notice_source(&identity, false)?.is_some())
                {
                    continue;
                }
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
        if let Some(index) = &self.indexed {
            for source in &self.sources {
                let HistorySource::Presentation { fact, .. } = source else { continue };
                let HistoryContent::Notice { event } = fact.content else { continue };
                if event < index.published_domain() {
                    self.notice_anchors_in_tail.insert(event);
                    if let std::collections::btree_map::Entry::Vacant(entry) = self.notices.entry(event) {
                        let source = timeline.exact_event(chat_state::EventSeq::new(event))?
                            .ok_or_else(|| invalid("notice source is missing"))?;
                        let notice = crate::session::storage::communication_history::read_notice(directory, &source)?
                            .ok_or_else(|| invalid("notice source is not a communication receipt"))?;
                        entry.insert(notice);
                    }
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
        let mut echoes = BTreeMap::<String, Vec<(usize, serde_json::Value)>>::new();
        for source in &self.sources {
            let HistorySource::Presentation { fact, .. } = source else {
                continue;
            };
            fact.validate_projection_identity()?;
            if let HistoryContent::Notice { event } = &fact.content {
                let event = timeline
                    .exact_event(chat_state::EventSeq::new(*event))?
                    .ok_or_else(|| invalid("notice source is missing"))?;
                let identity =
                    crate::session::storage::communication_history::receipt_identity(&event)?
                        .ok_or_else(|| invalid("notice source is not a communication receipt"))?;
                let canonical = match &self.indexed {
                    Some(index) => index.notice_source(&identity, false)?.map(|source| source.event),
                    None => None,
                }.or_else(|| receipt_sources.get(&identity).copied());
                if canonical != Some(event.seq.get()) {
                    return Err(invalid(
                        "notice reference does not select its original receipt",
                    ));
                }
            }
            if let HistoryContent::Message { event, .. } = &fact.content {
                let source = timeline.exact_event(chat_state::EventSeq::new(*event))?
                    .ok_or_else(|| invalid("inherited history source is absent"))?;
                fact.inherited_item(&source)?;
            }
            let HistoryContent::User(chunk) = &fact.content else {
                continue;
            };
            let Some((message, prompt)) = user_echo_identity(chunk) else {
                continue;
            };
            let blocks = echoes.entry(message.to_owned()).or_default();
            if blocks.first().is_some_and(|(old_prompt, _)| *old_prompt != prompt) {
                return Err(invalid("user echo identity carries multiple prompt indices"));
            }
            blocks.push((prompt, serde_json::to_value(&chunk.content).map_err(io::Error::other)?));
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
        let selected_echo_events = if has_committed_echoes && !timeline.has_surface_checkpoint() {
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
            if !timeline.has_surface_checkpoint() && !selected_echo_events.contains(&event.seq.get()) {
                continue;
            }
            let blocks = echoes
                .get(&expected.message_id)
                .ok_or_else(|| invalid("committed user echo is missing"))?;
            verify_user_echo(expected, blocks)?;
        }
        Ok(())
    }

    pub(crate) fn display_record_count(&self) -> usize {
        self.indexed
            .as_ref()
            .map_or(0, |index| index.omitted_display_record_count())
            + self
                .sources
                .iter()
                .enumerate()
                .filter(|(offset, source)| {
                    !self.indexed.as_ref().is_some_and(|index| self.positions.get(*offset).is_some_and(|position| *position < index.records())
                        && matches!(source, HistorySource::Presentation { fact, .. }
                            if matches!(&fact.content, HistoryContent::User(chunk)
                                if user_echo_identity(chunk).is_some() && !provisional_user_echo(chunk))
                                || indexes_background_task(fact)))
                })
                .filter(|(_, source)| match source {
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
                event_counter_after(fact.event_id()?)
            })
            .max();
        // Synthesized receipt IDs end in their domain position. Cover every
        // physical position even when no presentation row exists (noReplay or
        // a hidden branch must not let later live events fall below that UI highwater).
        max.unwrap_or(0).max(self.record_count()).max(
            self.indexed.as_ref().map_or(0, |index| index.event_counter_floor()),
        )
    }

    pub(crate) fn record_count(&self) -> u64 {
        self.positions
            .last()
            .map_or(self.sources.len() as u64, |last| last + 1)
            .max(self.indexed.as_ref().map_or(0, |index| index.records()))
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

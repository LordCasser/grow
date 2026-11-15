use super::tool_history::{DisplayReference, DomainReference, read_domain};
use super::working_index::RecordRef;
use crate::session::history::{
    HistoryContent, HistoryFact, JournalFact, JournalRecord, ResponseAnchor, TextChannel,
    decode_record, response_item_tokens,
};
use acp_transport::protocol as acp;
use chat_state::{EventSeq, ResponseAdmissionIdentity};
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use sampling_types::ConversationItem;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

const CANDIDATES: TableDefinition<(&str, u32), &[u8]> = TableDefinition::new("response_candidate");
const ADMISSIONS: TableDefinition<(&str, u32), &[u8]> = TableDefinition::new("response_admission");
const ADMITTED_CALLS: TableDefinition<(&str, u64, u64), &[u8]> =
    TableDefinition::new("response_admitted_calls");
const ROWS: TableDefinition<(u64, u64, u64), &[u8]> = TableDefinition::new("response_display");
const SELECTED: TableDefinition<(u64, u64), ()> = TableDefinition::new("selected_responses");
const INHERITED_ROWS: u64 = u64::MAX;
const FRAGMENTS: TableDefinition<(u64, u64, u64), &[u8]> =
    TableDefinition::new("response_fragment");
const FRAGMENT_ENCODED_BYTES: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Candidate {
    position: u64,
    at_ms: i64,
    meta: Option<acp::Meta>,
    source: Option<DisplayReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Row {
    pub position: u64,
    pub part: u64,
    pub source: EventSeq,
    pub origin: TextOrigin,
    pub ordinal: usize,
    pub at_ms: i64,
    pub meta: Option<acp::Meta>,
    pub complete: bool,
    pub encoded_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum TextOrigin {
    Response { identity: ResponseAdmissionIdentity, reasoning: bool },
    Inherited { reasoning: bool, chunk_meta: Option<acp::Meta> },
}

pub(super) struct Writer<'tx> {
    candidates: Table<'tx, (&'static str, u32), &'static [u8]>,
    admissions: Table<'tx, (&'static str, u32), &'static [u8]>,
    admitted_calls: Table<'tx, (&'static str, u64, u64), &'static [u8]>,
    rows: Table<'tx, (u64, u64, u64), &'static [u8]>,
    selected: Table<'tx, (u64, u64), ()>,
    fragments: Table<'tx, (u64, u64, u64), &'static [u8]>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl<'tx> Writer<'tx> {
    pub(super) fn open(tx: &'tx WriteTransaction) -> io::Result<Self> {
        Ok(Self {
            candidates: tx.open_table(CANDIDATES).map_err(io::Error::other)?,
            admissions: tx.open_table(ADMISSIONS).map_err(io::Error::other)?,
            admitted_calls: tx.open_table(ADMITTED_CALLS).map_err(io::Error::other)?,
            rows: tx.open_table(ROWS).map_err(io::Error::other)?,
            selected: tx.open_table(SELECTED).map_err(io::Error::other)?,
            fragments: tx.open_table(FRAGMENTS).map_err(io::Error::other)?,
        })
    }

    pub(super) fn select_rewind(
        &mut self,
        previous_branch: Option<EventSeq>,
        rewind: EventSeq,
        timeline: &chat_state::Timeline,
    ) -> io::Result<()> {
        let survivors = timeline.admitted_response_views().into_iter()
            .map(|view| view.event_seq).collect::<std::collections::BTreeSet<_>>();
        let previous = context_partition(previous_branch);
        let branch = context_partition(Some(rewind));
        let mut rows = Vec::new();
        for result in self.rows.range((previous, 0, 0)..=(previous, u64::MAX, u64::MAX))
            .map_err(io::Error::other)?
        {
            let (key, value) = result.map_err(io::Error::other)?;
            let row: Row = serde_json::from_slice(value.value())?;
            if survivors.contains(&row.source) {
                let (_, position, part) = key.value();
                rows.push(((branch, position, part), value.value().to_vec()));
            }
        }
        for (key, value) in rows {
            self.rows.insert(key, value.as_slice()).map_err(io::Error::other)?;
        }
        for seq in survivors {
            self.selected.insert((branch, seq.get()), ()).map_err(io::Error::other)?;
        }
        Ok(())
    }

    pub(super) fn accept(
        &mut self,
        record: &JournalRecord,
        offset: u64,
        json: &[u8],
        context_rebuild: Option<EventSeq>,
    ) -> io::Result<()> {
        if let JournalFact::Domain { event, .. } = &record.fact
            && let Some(message) = event.messages()
            && let Some(admission) = &message.response_admission
        {
            let key = (
                admission.identity.request_id.as_str(),
                admission.identity.attempt,
            );
            let reference = DomainReference {
                seq: event.seq,
                record: RecordRef {
                    offset,
                    len: json.len() as u64,
                    digest: blake3::hash(json),
                },
            };
            let encoded = serde_json::to_vec(&reference)?;
            if let Some(existing) = self.admissions.get(key).map_err(io::Error::other)? {
                let existing: DomainReference = serde_json::from_slice(existing.value())?;
                if existing.seq != reference.seq || existing.record != reference.record {
                    return Err(invalid("response admission identity is not unique"));
                }
            } else {
                self.admissions
                    .insert(key, encoded.as_slice())
                    .map_err(io::Error::other)?;
            }
            for (index, call) in response_calls(message).enumerate() {
                self.admitted_calls
                    .insert(
                        (call.id.as_ref(), event.seq.get(), index as u64),
                        encoded.as_slice(),
                    )
                    .map_err(io::Error::other)?;
            }
        }
        match &record.fact {
            JournalFact::Presentation { at_ms, event } => {
                if let HistoryContent::Candidate(key) = &event.content {
                    let candidate = Candidate {
                        position: record.seq,
                        at_ms: *at_ms,
                        meta: event.meta.clone(),
                        source: Some(DisplayReference {
                            position: record.seq,
                            record: RecordRef {
                                offset,
                                len: json.len() as u64,
                                digest: blake3::hash(json),
                            },
                        }),
                    };
                    let encoded = serde_json::to_vec(&candidate)?;
                    self.candidates
                        .insert((key.request_id.as_str(), key.attempt), encoded.as_slice())
                        .map_err(io::Error::other)?;
                }
            }
            JournalFact::Domain {
                event,
                response_anchor: Some(anchor),
            } => {
                let message = event
                    .messages()
                    .ok_or_else(|| invalid("response anchor has no message"))?;
                let admission = message
                    .response_admission
                    .as_ref()
                    .ok_or_else(|| invalid("response anchor has no admission"))?;
                if admission.quarantined_tool_exchanges != 0 {
                    return Ok(());
                }
                let rows = message
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(ordinal, item)| match item {
                        ConversationItem::Assistant(item) if !item.content.is_empty() => {
                            Some((ordinal, false))
                        }
                        ConversationItem::Reasoning(item) if !item.text.is_empty() => {
                            Some((ordinal, true))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if rows.is_empty() {
                    return Ok(());
                }
                let candidate = match anchor {
                    ResponseAnchor::Admission => Candidate {
                        position: record.seq,
                        at_ms: event.at_ms,
                        meta: None,
                        source: None,
                    },
                    ResponseAnchor::Candidate { record } => {
                        let key = &admission.identity;
                        let candidate = self
                            .candidates
                            .get((key.request_id.as_str(), key.attempt))
                            .map_err(io::Error::other)?
                            .ok_or_else(|| invalid("missing indexed response candidate"))?;
                        let candidate: Candidate = serde_json::from_slice(candidate.value())?;
                        if candidate.position != *record {
                            return Err(invalid(
                                "indexed response candidate differs from admission",
                            ));
                        }
                        candidate
                    }
                };
                let items = response_item_tokens(json)?;
                for (part, (ordinal, reasoning)) in rows.iter().copied().enumerate() {
                    let item = items
                        .get(ordinal)
                        .ok_or_else(|| invalid("response item token is absent"))?;
                    let token = text_token(item, reasoning)?;
                    let (position, part) = (candidate.position, part as u64);
                    let encoded_bytes =
                        self.index_fragments(position, part, offset, json, token)?;
                    let row = Row {
                        position,
                        part,
                        source: event.seq,
                        origin: TextOrigin::Response {
                            identity: admission.identity.clone(),
                            reasoning,
                        },
                        ordinal,
                        at_ms: candidate.at_ms,
                        meta: candidate.meta.clone(),
                        complete: part + 1 == rows.len() as u64,
                        encoded_bytes,
                    };
                    self.rows
                        .insert((context_partition(context_rebuild), position, part), serde_json::to_vec(&row)?.as_slice())
                        .map_err(io::Error::other)?;
                }
            }
            JournalFact::Domain { .. } => {}
        }
        Ok(())
    }

    pub(super) fn inherit(
        &mut self,
        position: u64,
        at_ms: i64,
        fact: &HistoryFact,
        journal: &mut File,
        read_reference: &impl Fn(EventSeq) -> io::Result<Option<RecordRef>>,
    ) -> io::Result<()> {
        let HistoryContent::Message { event, item, channel, chunk_meta } = &fact.content else {
            unreachable!("indexed inherited text must name its source");
        };
        let source = EventSeq::new(*event);
        let reference = read_reference(source)?
            .ok_or_else(|| invalid("missing inherited text source"))?;
        let json = super::working_index::read_record_bytes(journal, reference)?;
        let record = decode_record(&json)?;
        let JournalFact::Domain { event, .. } = &record.fact else {
            return Err(invalid("inherited text reference has no domain record"));
        };
        if record.seq >= position || event.seq != source {
            return Err(invalid("inherited text reference has a different source"));
        }
        fact.inherited_item(event)?;
        let reasoning = *channel == TextChannel::Reasoning;
        let items = response_item_tokens(&json)?;
        let token = text_token(items.get(*item).ok_or_else(|| invalid("inherited text token is absent"))?, reasoning)?;
        let encoded_bytes = self.index_fragments(position, 0, reference.offset, &json, token)?;
        let row = Row {
            position,
            part: 0,
            source,
            origin: TextOrigin::Inherited { reasoning, chunk_meta: chunk_meta.clone() },
            ordinal: *item,
            at_ms,
            meta: fact.meta.clone(),
            complete: true,
            encoded_bytes,
        };
        self.rows.insert((INHERITED_ROWS, position, 0), serde_json::to_vec(&row)?.as_slice())
            .map_err(io::Error::other)?;
        Ok(())
    }

    fn index_fragments(
        &mut self,
        position: u64,
        part: u64,
        offset: u64,
        json: &[u8],
        token: &RawValue,
    ) -> io::Result<u64> {
        let token = token.get();
        let body = token
            .strip_prefix('"')
            .and_then(|token| token.strip_suffix('"'))
            .ok_or_else(|| invalid("response body is not a string token"))?;
        let token_offset = (body.as_ptr() as usize)
            .checked_sub(json.as_ptr() as usize)
            .filter(|start| *start + body.len() <= json.len())
            .ok_or_else(|| invalid("response body lies outside its canonical record"))?;
        for (start, end) in fragment_ranges(body)? {
            let bytes = &body.as_bytes()[start..end];
            let reference = RecordRef {
                offset: offset + (token_offset + start) as u64,
                len: bytes.len() as u64,
                digest: blake3::hash(bytes),
            };
            self.fragments
                .insert(
                    (position, part, end as u64),
                    serde_json::to_vec(&reference)?.as_slice(),
                )
                .map_err(io::Error::other)?;
        }
        Ok(body.len() as u64)
    }
}

fn response_calls(
    message: &chat_state::MessageEvent,
) -> impl Iterator<Item = &sampling_types::ToolCall> {
    message
        .items
        .iter()
        .filter_map(|item| match item {
            ConversationItem::Assistant(assistant) => Some(assistant.tool_calls.as_slice()),
            _ => None,
        })
        .flatten()
}

pub(super) fn admitted_calls_before(
    tx: &ReadTransaction,
    journal: &mut File,
    call_id: &str,
    start: u64,
    end: u64,
    bytes: u64,
    limit: usize,
    rewind: Option<EventSeq>,
) -> io::Result<Vec<sampling_types::ToolCall>> {
    if start >= end {
        return Ok(Vec::new());
    }
    let table = tx.open_table(ADMITTED_CALLS).map_err(io::Error::other)?;
    let mut calls = Vec::new();
    for row in table.range((call_id, start, 0)..(call_id, end, 0)).map_err(io::Error::other)? {
        if calls.len() == limit { break; }
        let (key, value) = row.map_err(io::Error::other)?;
        let (_, seq, index) = key.value();
        if !selected_at(tx, rewind, EventSeq::new(seq))? { continue; }
        let reference: DomainReference = serde_json::from_slice(value.value())?;
        if reference.seq.get() != seq {
            return Err(invalid("admitted Tool call index differs from its response coordinate"));
        }
        let event = read_domain(journal, reference, bytes)?;
        let call = event
            .messages()
            .filter(|message| message.response_admission.is_some())
            .and_then(|message| response_calls(message).nth(index as usize))
            .filter(|call| call.id.as_ref() == call_id)
            .cloned()
            .ok_or_else(|| invalid("admitted Tool call index differs from its response"))?;
        calls.push(call);
    }
    Ok(calls)
}

pub(super) fn candidate_before(
    tx: &ReadTransaction,
    key: &crate::session::history::ResponseKey,
    records: u64,
) -> io::Result<Option<DisplayReference>> {
    let candidates = tx.open_table(CANDIDATES).map_err(io::Error::other)?;
    let Some(row) = candidates
        .get((key.request_id.as_str(), key.attempt))
        .map_err(io::Error::other)?
    else {
        return Ok(None);
    };
    let candidate: Candidate = serde_json::from_slice(row.value())?;
    let source = candidate
        .source
        .filter(|source| source.position == candidate.position)
        .ok_or_else(|| invalid("missing response candidate source reference"))?;
    Ok((source.position < records).then_some(source))
}

/// Resolve the exact admitted response visible before `before` in a pinned
/// journal cut. The admission index may lead the manifest after an interrupted
/// publication, so references outside either cut frontier are treated as not
/// yet visible.
pub(super) fn admission_before(
    tx: &ReadTransaction,
    journal: &mut File,
    identity: &ResponseAdmissionIdentity,
    before: EventSeq,
    domain: EventSeq,
    bytes: u64,
) -> io::Result<Option<chat_state::TimelineEvent>> {
    let table = tx.open_table(ADMISSIONS).map_err(io::Error::other)?;
    let Some(value) = table
        .get((identity.request_id.as_str(), identity.attempt))
        .map_err(io::Error::other)?
    else {
        return Ok(None);
    };
    let reference: DomainReference = serde_json::from_slice(value.value())?;
    if reference.seq >= before || reference.seq >= domain {
        return Ok(None);
    }
    let event = read_domain(journal, reference, bytes)?;
    let actual = event
        .messages()
        .and_then(|message| message.response_admission.as_ref())
        .ok_or_else(|| invalid("response admission reference has no admission"))?;
    if actual.identity != *identity {
        return Err(invalid(
            "response admission reference has a different identity",
        ));
    }
    Ok(Some(event))
}

fn text_token(item: &RawValue, reasoning: bool) -> io::Result<&RawValue> {
    #[derive(Deserialize)]
    struct Text<'a> {
        #[serde(borrow)]
        content: Option<&'a RawValue>,
        #[serde(borrow)]
        text: Option<&'a RawValue>,
    }
    let item: Text<'_> = serde_json::from_str(item.get())?;
    if reasoning { item.text } else { item.content }
        .ok_or_else(|| invalid("canonical response has no text token"))
}

fn fragment_ranges(body: &str) -> io::Result<Vec<(usize, usize)>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < body.len() {
        let step = match body.as_bytes()[i] {
            b'\\' if body.as_bytes().get(i + 1) == Some(&b'u') => {
                let escape = body
                    .get(i + 2..i + 6)
                    .ok_or_else(|| invalid("short JSON Unicode escape"))?;
                let code = u16::from_str_radix(escape, 16).map_err(io::Error::other)?;
                if (0xd800..=0xdbff).contains(&code) {
                    12
                } else {
                    6
                }
            }
            b'\\' => 2,
            _ => body[i..]
                .chars()
                .next()
                .expect("string character")
                .len_utf8(),
        };
        if i - start + step > FRAGMENT_ENCODED_BYTES {
            ranges.push((start, i));
            start = i;
        }
        i += step;
    }
    if i != body.len() {
        return Err(invalid("invalid response fragment boundary"));
    }
    if start != i {
        ranges.push((start, i));
    }
    Ok(ranges)
}

pub(super) fn rows_before(
    tx: &ReadTransaction,
    before: (u64, u64),
    frontier: super::tool_history::Frontier,
    records: u64,
    limit: usize,
) -> io::Result<Vec<Row>> {
    let table = tx.open_table(ROWS).map_err(io::Error::other)?;
    let before = before.min((records, 0));
    let mut rows = Vec::with_capacity(2 * limit);
    for partition in [context_partition(frontier.history_branch()), INHERITED_ROWS] {
        let start = (partition, 0, 0);
        let end = (partition, before.0, before.1);
        let mut visible = 0;
        for entry in table.range(start..end).map_err(io::Error::other)?.rev() {
            let (key, value) = entry.map_err(io::Error::other)?;
            let (_, position, part) = key.value();
            let row: Row = serde_json::from_slice(value.value())?;
            if (row.position, row.part) != (position, part) {
                return Err(invalid("response row differs from its physical key"));
            }
            // An admission in a newer cut can refer to a candidate before
            // this cut. The source event, as well as the anchor, must exist.
            if row.source.get() < frontier.domain {
                rows.push(row);
                visible += 1;
                if visible == limit {
                    break;
                }
            }
        }
    }
    rows.sort_unstable_by_key(|row| std::cmp::Reverse((row.position, row.part)));
    rows.truncate(limit);
    Ok(rows)
}

fn context_partition(rebuild: Option<EventSeq>) -> u64 {
    rebuild.map_or(0, |seq| seq.get() + 1)
}

pub(super) fn selected_at(tx: &ReadTransaction, rewind: Option<EventSeq>, source: EventSeq) -> io::Result<bool> {
    let Some(rewind) = rewind.filter(|rewind| source < *rewind) else { return Ok(true); };
    let table = tx.open_table(SELECTED).map_err(io::Error::other)?;
    Ok(table.get((context_partition(Some(rewind)), source.get())).map_err(io::Error::other)?.is_some())
}

pub(super) fn read_fragment(
    tx: &ReadTransaction,
    journal: &mut File,
    row: &Row,
    end: u64,
    published_bytes: u64,
) -> io::Result<(u64, String)> {
    let table = tx.open_table(FRAGMENTS).map_err(io::Error::other)?;
    let value = table
        .get((row.position, row.part, end))
        .map_err(io::Error::other)?
        .ok_or_else(|| invalid("missing response byte fragment"))?;
    let reference: RecordRef = serde_json::from_slice(value.value())?;
    let start = end
        .checked_sub(reference.len)
        .filter(|_| {
            end <= row.encoded_bytes
                && reference.len as usize <= FRAGMENT_ENCODED_BYTES
                && reference
                    .offset
                    .checked_add(reference.len)
                    .is_some_and(|end| end < published_bytes)
        })
        .ok_or_else(|| invalid("response byte fragment crosses its cut"))?;
    let mut quoted = vec![b'"'; reference.len as usize + 2];
    journal.seek(SeekFrom::Start(reference.offset))?;
    journal.read_exact(&mut quoted[1..reference.len as usize + 1])?;
    if blake3::hash(&quoted[1..reference.len as usize + 1]) != reference.digest {
        return Err(invalid(
            "response byte fragment differs from canonical journal",
        ));
    }
    Ok((start, serde_json::from_slice(&quoted)?))
}

pub(super) fn read_row(
    tx: &ReadTransaction,
    journal: &mut File,
    row: &Row,
    mut end: u64,
    bytes: u64,
    limit: u64,
) -> io::Result<(u64, String)> {
    let start = end;
    let mut parts = Vec::new();
    while end != 0 && (parts.is_empty() || start - end < limit) {
        let (older, text) = read_fragment(tx, journal, row, end, bytes)?;
        if !parts.is_empty() && start - older > limit {
            break;
        }
        parts.push(text);
        end = older;
    }
    parts.reverse();
    Ok((end, parts.concat()))
}

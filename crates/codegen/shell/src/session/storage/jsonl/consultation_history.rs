use super::JsonlStorageAdapter;
use super::compaction_history::validate_sideband_spawn;
use super::tool_history::{DomainReference, Frontier, read_domain};
use crate::coordination::{IncomingInquiryAudit, InquiryEvent};
use crate::session::storage::{ContainedDirectory, SidebandLedgers, validate_consultation_result};
use chat_state::{
    EventSeq, NotificationEvent, NotificationSource, NotificationSourceVersion, TimelineEvent,
    TimelineEventKind,
};
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use std::{fs::File, io};

pub(super) const FACTS: TableDefinition<(&str, &str, u64), &[u8]> =
    TableDefinition::new("incoming_consultation");
const TERMINALS: TableDefinition<(&str, &str), &[u8]> =
    TableDefinition::new("consultation_terminal");
const OPEN: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("consultation_open");
const ANSWERS: TableDefinition<u64, &[u8]> = TableDefinition::new("consultation_answers");
const CUTS: TableDefinition<u64, &[u8]> = TableDefinition::new("consultation_cuts");

pub(super) fn is_incoming(observation: &chat_state::ObservationEvent) -> bool {
    observation.scope == "coordination"
        && observation.name == "inquiry"
        && observation
            .data
            .as_ref()
            .and_then(|data| data.get("kind"))
            .and_then(|value| value.as_str())
            == Some("incoming")
}

pub(super) struct Writer<'tx> {
    facts: Table<'tx, (&'static str, &'static str, u64), &'static [u8]>,
    terminals: Table<'tx, (&'static str, &'static str), &'static [u8]>,
    open: Table<'tx, (&'static str, &'static str), &'static [u8]>,
    answers: Table<'tx, u64, &'static [u8]>,
    cuts: Table<'tx, u64, &'static [u8]>,
}

impl<'tx> Writer<'tx> {
    pub(super) fn open(tx: &'tx WriteTransaction) -> io::Result<Self> {
        Ok(Self {
            facts: tx.open_table(FACTS).map_err(io::Error::other)?,
            terminals: tx.open_table(TERMINALS).map_err(io::Error::other)?,
            open: tx.open_table(OPEN).map_err(io::Error::other)?,
            answers: tx.open_table(ANSWERS).map_err(io::Error::other)?,
            cuts: tx.open_table(CUTS).map_err(io::Error::other)?,
        })
    }

    pub(super) fn accept(
        &mut self,
        event: &TimelineEvent,
        reference: DomainReference,
        session: &str,
        verify_answer: impl FnOnce(
            &IncomingInquiryAudit,
            &chat_state::TimelineRangeRef,
        ) -> io::Result<()>,
        read_event: impl FnOnce(EventSeq) -> io::Result<Option<TimelineEvent>>,
    ) -> io::Result<()> {
        if matches!(&event.kind, TimelineEventKind::Observation(observation) if is_incoming(observation))
            && let Some(InquiryEvent::Incoming { inquiry_id, audit }) =
                InquiryEvent::from_timeline(event).map_err(invalid)?
        {
            let key = (audit.source_session_id.as_str(), inquiry_id.as_str());
            let encoded = serde_json::to_vec(&reference)?;
            self.facts
                .insert((key.0, key.1, event.seq.get()), encoded.as_slice())
                .map_err(io::Error::other)?;
            let terminal_exists = self.terminals.get(key).map_err(io::Error::other)?.is_some();
            if let Some(proof) = audit
                .answer_context
                .as_ref()
                .filter(|proof| proof.target_session_id == session)
            {
                if terminal_exists {
                    return Err(invalid("duplicate canonical consultation answer"));
                }
                verify_answer(&audit, &proof.result_ref)?;
                self.answers
                    .insert(event.seq.get(), encoded.as_slice())
                    .map_err(io::Error::other)?;
            }
            if audit.outcome.is_some() {
                if !terminal_exists {
                    self.terminals
                        .insert(key, encoded.as_slice())
                        .map_err(io::Error::other)?;
                    self.open.remove(key).map_err(io::Error::other)?;
                }
            } else if !terminal_exists {
                self.open
                    .insert(key, encoded.as_slice())
                    .map_err(io::Error::other)?;
            }
        }
        if let Some(seq) = receipt_source(event, session)? {
            let original = read_event(seq)?
                .ok_or_else(|| invalid("consultation receipt has no canonical answer"))?;
            validate_receipt(event, session, &original)?;
            if self
                .answers
                .remove(seq.get())
                .map_err(io::Error::other)?
                .is_none()
            {
                return Err(invalid("consultation receipt has no unsettled answer"));
            }
        }
        Ok(())
    }

    pub(super) fn publish(&mut self, previous: u64, bytes: u64) -> io::Result<()> {
        let mut references: Vec<DomainReference> = Vec::new();
        for row in self.open.iter().map_err(io::Error::other)? {
            let (_, value) = row.map_err(io::Error::other)?;
            references.push(serde_json::from_slice(value.value())?);
        }
        for row in self.answers.iter().map_err(io::Error::other)? {
            let (_, value) = row.map_err(io::Error::other)?;
            references.push(serde_json::from_slice(value.value())?);
        }
        self.cuts
            .retain(|key, _| key == previous)
            .map_err(io::Error::other)?;
        self.cuts
            .insert(bytes, serde_json::to_vec(&references)?.as_slice())
            .map_err(io::Error::other)?;
        Ok(())
    }
}

pub(super) fn validate_answer(
    spawn: Option<DomainReference>,
    journal: &mut File,
    directory: &ContainedDirectory,
    source: DomainReference,
    session: &str,
    bytes: u64,
    audit: &IncomingInquiryAudit,
    result_ref: &chat_state::TimelineRangeRef,
    read_event: &impl Fn(EventSeq) -> io::Result<Option<TimelineEvent>>,
) -> io::Result<()> {
    let reference = spawn
        .ok_or_else(|| invalid("consultation proof does not reference its InfoRequest Sideband"))?;
    let event = read_domain(journal, reference, bytes)?;
    let TimelineEventKind::Sideband(spawn) = event.kind else {
        return Err(invalid(
            "consultation proof does not reference its InfoRequest Sideband",
        ));
    };
    if event.seq >= source.seq
        || spawn.purpose != chat_state::SidebandPurpose::InfoRequest
        || spawn.sideband_id != result_ref.timeline_id
    {
        return Err(invalid(
            "consultation proof does not reference its InfoRequest Sideband",
        ));
    }
    let events = JsonlStorageAdapter::read_sideband_ledger(directory, &spawn.sideband_id)?
        .ok_or_else(|| invalid("consultation result ledger is missing"))?;
    let ledger = chat_state::SidebandTimeline::from_events(events.clone()).map_err(invalid)?;
    validate_sideband_spawn(&ledger, session, &spawn, read_event)?;
    validate_consultation_result(
        audit,
        result_ref,
        &SidebandLedgers::from([(spawn.sideband_id, events)]),
    )?;
    Ok(())
}

pub(super) fn pending(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
) -> io::Result<Vec<TimelineEvent>> {
    let table = tx.open_table(CUTS).map_err(io::Error::other)?;
    let value = table
        .get(cut.bytes)
        .map_err(io::Error::other)?
        .ok_or_else(|| invalid("missing incoming consultation checkpoint"))?;
    let refs: Vec<DomainReference> = serde_json::from_slice(value.value())?;
    refs.into_iter()
        .map(|reference| {
            if reference.seq.get() >= cut.domain {
                return Err(invalid("consultation checkpoint crosses its frontier"));
            }
            let event = read_domain(journal, reference, cut.bytes)?;
            match InquiryEvent::from_timeline(&event).map_err(invalid)? {
                Some(InquiryEvent::Incoming { audit, .. })
                    if audit.outcome.is_none() || audit.answer_context.is_some() =>
                {
                    Ok(event)
                }
                _ => Err(invalid("consultation checkpoint differs from its journal")),
            }
        })
        .collect()
}

pub(super) fn facts_before(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
    sender: &str,
    inquiry: &str,
) -> io::Result<Vec<TimelineEvent>> {
    let table = tx.open_table(FACTS).map_err(io::Error::other)?;
    table.range((sender, inquiry, 0)..(sender, inquiry, cut.domain)).map_err(io::Error::other)?
        .map(|row| {
            let (key, row) = row.map_err(io::Error::other)?;
            let reference: DomainReference = serde_json::from_slice(row.value())?;
            let event = read_domain(journal, reference, cut.bytes)?;
            if event.seq.get() != key.value().2 || !matches!(InquiryEvent::from_timeline(&event).map_err(invalid)?,
                Some(InquiryEvent::Incoming { inquiry_id, audit }) if inquiry_id == inquiry && audit.source_session_id == sender)
            {
                return Err(invalid("incoming inquiry key differs from its journal"));
            }
            Ok(event)
        }).collect()
}

fn receipt_source(event: &TimelineEvent, session: &str) -> io::Result<Option<EventSeq>> {
    let TimelineEventKind::Notification(NotificationEvent::Received {
        source: NotificationSource::InquiryAnswered { .. },
        source_version,
        ..
    }) = &event.kind
    else {
        return Ok(None);
    };
    let seq = match source_version {
        NotificationSourceVersion::Opaque { value } => value
            .split_once(':')
            .filter(|(owner, _)| *owner == session)
            .and_then(|(_, seq)| seq.parse::<u64>().ok())
            .map(EventSeq::new),
        _ => None,
    };
    seq.filter(|seq| *seq < event.seq)
        .map(Some)
        .ok_or_else(|| invalid("consultation receipt is not backed by its canonical answer proof"))
}

fn validate_receipt(
    event: &TimelineEvent,
    session: &str,
    original: &TimelineEvent,
) -> io::Result<()> {
    let valid = if let (
        TimelineEventKind::Notification(NotificationEvent::Received {
            owner_session_id,
            source:
                NotificationSource::InquiryAnswered {
                    source_session_id,
                    inquiry_id,
                    ..
                },
            source_version: NotificationSourceVersion::Opaque { value },
            payload_ref,
            ..
        }),
        Some(InquiryEvent::Incoming {
            inquiry_id: key,
            audit,
        }),
    ) = (
        &event.kind,
        InquiryEvent::from_timeline(original).map_err(invalid)?,
    ) {
        original.seq < event.seq
            && owner_session_id == session
            && value == &format!("{session}:{}", original.seq.get())
            && audit
                .answer_context
                .as_ref()
                .is_some_and(|proof| proof.target_session_id == session)
            && inquiry_id == &key
            && source_session_id == &audit.source_session_id
            && audit.answer_context_body().is_some_and(|body| {
                payload_ref.bytes == body.len() as u64
                    && payload_ref.blake3 == blake3::hash(body.as_bytes()).to_hex().as_str()
            })
    } else {
        false
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(
            "consultation receipt is not backed by its canonical answer proof",
        ))
    }
}

fn invalid(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

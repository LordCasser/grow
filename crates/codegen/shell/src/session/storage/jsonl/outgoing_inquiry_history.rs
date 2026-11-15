//! Original source-side coordination identities at the published Timeline cut.

use super::tool_history::{DomainReference, read_domain};
use crate::coordination::InquiryEvent;
use chat_state::{EventSeq, TimelineEvent};
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use std::{fs::File, io};

const OUTGOING: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("outgoing_inquiries");

pub(super) fn is_outgoing(observation: &chat_state::ObservationEvent) -> bool {
    observation.scope == "coordination"
        && observation.name == "inquiry"
        && matches!(
            observation
                .data
                .as_ref()
                .and_then(|data| data.get("kind"))
                .and_then(serde_json::Value::as_str),
            Some("outgoing_started" | "outgoing_completed")
        )
}

fn inquiry_id(event: &InquiryEvent) -> Option<&str> {
    match event {
        InquiryEvent::OutgoingStarted { inquiry_id, .. } => Some(inquiry_id),
        InquiryEvent::OutgoingCompleted { audit } => Some(&audit.outcome.inquiry_id),
        InquiryEvent::Incoming { .. } => None,
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

pub(super) struct Writer<'a>(Table<'a, (&'static str, u64), &'static [u8]>);

impl<'a> Writer<'a> {
    pub(super) fn open(tx: &'a WriteTransaction) -> io::Result<Self> {
        Ok(Self(tx.open_table(OUTGOING).map_err(io::Error::other)?))
    }

    pub(super) fn accept(
        &mut self,
        event: &TimelineEvent,
        reference: DomainReference,
    ) -> io::Result<()> {
        let inquiry = InquiryEvent::from_timeline(event)
            .map_err(invalid)?
            .ok_or_else(|| invalid("indexed outgoing inquiry is absent"))?;
        let id = inquiry_id(&inquiry).ok_or_else(|| invalid("indexed inquiry is not outgoing"))?;
        self.0
            .insert(
                (id, reference.seq.get()),
                serde_json::to_vec(&reference)?.as_slice(),
            )
            .map_err(io::Error::other)?;
        Ok(())
    }
}

pub(super) fn before(
    tx: &ReadTransaction,
    journal: &mut File,
    id: &str,
    seq: EventSeq,
    bytes: u64,
) -> io::Result<Vec<TimelineEvent>> {
    let table = tx.open_table(OUTGOING).map_err(io::Error::other)?;
    let mut events = Vec::new();
    for row in table
        .range((id, 0)..(id, seq.get()))
        .map_err(io::Error::other)?
    {
        let (key, row) = row.map_err(io::Error::other)?;
        let reference: DomainReference = serde_json::from_slice(row.value())?;
        if reference.seq.get() != key.value().1 {
            return Err(invalid(
                "outgoing inquiry coordinate differs from its index",
            ));
        }
        let event = read_domain(journal, reference, bytes)?;
        let inquiry = InquiryEvent::from_timeline(&event)
            .map_err(invalid)?
            .ok_or_else(|| invalid("indexed outgoing inquiry is absent"))?;
        if inquiry_id(&inquiry) != Some(id) {
            return Err(invalid(
                "outgoing inquiry index differs from the journal identity",
            ));
        }
        events.push(event);
    }
    Ok(events)
}

use super::tool_history::{DisplayReference, DomainReference, Frontier};
use super::working_index;
use crate::session::history::{self, HistoryContent, JournalFact, JournalRecord};
use chat_state::UserDisplayEcho;
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::{fs::File, io};

const BLOCKS: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("user_echo_blocks");
const CONSUMED: TableDefinition<&str, &[u8]> = TableDefinition::new("user_echo_consumed");
const ROWS: TableDefinition<u64, &[u8]> = TableDefinition::new("user_echo_display");

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct Row {
    pub presentation: DisplayReference,
    consume_seq: chat_state::EventSeq,
    consume_offset: u64,
}

pub(super) struct Writer<'a> {
    blocks: Table<'a, (&'static str, u64), &'static [u8]>,
    consumed: Table<'a, &'static str, &'static [u8]>,
    rows: Table<'a, u64, &'static [u8]>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl<'a> Writer<'a> {
    pub(super) fn open(tx: &'a WriteTransaction) -> io::Result<Self> {
        Ok(Self {
            blocks: tx.open_table(BLOCKS).map_err(io::Error::other)?,
            consumed: tx.open_table(CONSUMED).map_err(io::Error::other)?,
            rows: tx.open_table(ROWS).map_err(io::Error::other)?,
        })
    }

    pub(super) fn add_block(&mut self, id: &str, source: DisplayReference) -> io::Result<()> {
        self.blocks
            .insert(
                (id, source.position),
                serde_json::to_vec(&source)?.as_slice(),
            )
            .map_err(io::Error::other)?;
        Ok(())
    }

    pub(super) fn consume(
        &mut self,
        journal: &mut File,
        echo: &UserDisplayEcho,
        source: DomainReference,
        bytes: u64,
    ) -> io::Result<()> {
        let mut blocks = Vec::new();
        let mut provisional = Vec::new();
        for reference in references(&self.blocks, &echo.message_id, u64::MAX)? {
            let record = read_block(journal, reference, bytes, &echo.message_id)?;
            let JournalFact::Presentation { event, .. } = record.fact else {
                unreachable!()
            };
            let HistoryContent::User(chunk) = event.content else {
                unreachable!()
            };
            let (_, prompt) = history::user_echo_identity(&chunk).expect("verified echo block");
            if history::provisional_user_echo(&chunk) {
                provisional.push(reference);
            }
            blocks.push((
                prompt,
                serde_json::to_value(chunk.content).map_err(io::Error::other)?,
            ));
        }
        history::verify_user_echo(echo, &blocks)?;
        if self
            .consumed
            .insert(
                echo.message_id.as_str(),
                serde_json::to_vec(&source)?.as_slice(),
            )
            .map_err(io::Error::other)?
            .is_some()
        {
            return Err(invalid("duplicate consumed user echo"));
        }
        for presentation in provisional {
            let row = Row {
                presentation,
                consume_seq: source.seq,
                consume_offset: source.record.offset,
            };
            self.rows
                .insert(presentation.position, serde_json::to_vec(&row)?.as_slice())
                .map_err(io::Error::other)?;
        }
        Ok(())
    }
}

fn references(
    table: &impl ReadableTable<(&'static str, u64), &'static [u8]>,
    id: &str,
    before: u64,
) -> io::Result<Vec<DisplayReference>> {
    table
        .range((id, 0)..(id, before))
        .map_err(io::Error::other)?
        .map(|row| {
            let (key, value) = row.map_err(io::Error::other)?;
            let reference: DisplayReference = serde_json::from_slice(value.value())?;
            if reference.position != key.value().1 {
                return Err(invalid("user echo block differs from its physical key"));
            }
            Ok(reference)
        })
        .collect()
}

fn read_block(
    journal: &mut File,
    reference: DisplayReference,
    bytes: u64,
    id: &str,
) -> io::Result<JournalRecord> {
    if reference
        .record
        .offset
        .checked_add(reference.record.len)
        .is_none_or(|end| end >= bytes)
    {
        return Err(invalid("user echo block crosses its published frontier"));
    }
    let record = working_index::read_record(journal, reference.record)?;
    if record.seq != reference.position
        || !matches!(&record.fact,
        JournalFact::Presentation { event, .. } if matches!(&event.content,
            HistoryContent::User(chunk) if history::user_echo_identity(chunk).is_some_and(|(key, _)| key == id)))
    {
        return Err(invalid(
            "user echo identity differs from its original block",
        ));
    }
    Ok(record)
}

pub(super) fn blocks_before(
    tx: &ReadTransaction,
    journal: &mut File,
    id: &str,
    records: u64,
    bytes: u64,
) -> io::Result<Vec<JournalRecord>> {
    let blocks = tx.open_table(BLOCKS).map_err(io::Error::other)?;
    references(&blocks, id, records)?
        .into_iter()
        .map(|reference| read_block(journal, reference, bytes, id))
        .collect()
}

pub(super) fn rows_before(
    tx: &ReadTransaction,
    before: u64,
    frontier: Frontier,
    limit: usize,
) -> io::Result<Vec<Row>> {
    let rows = tx.open_table(ROWS).map_err(io::Error::other)?;
    let mut selected = Vec::new();
    for row in rows.range(..before).map_err(io::Error::other)?.rev() {
        let (key, value) = row.map_err(io::Error::other)?;
        let row: Row = serde_json::from_slice(value.value())?;
        if row.presentation.position != key.value() {
            return Err(invalid("indexed user echo differs from its physical key"));
        }
        if row.consume_seq.get() < frontier.domain
            && row.presentation.record.offset < frontier.bytes
        {
            selected.push(row);
            if selected.len() == limit {
                break;
            }
        }
    }
    Ok(selected)
}

pub(super) fn read_row(
    journal: &mut File,
    row: Row,
    frontier: Frontier,
) -> io::Result<JournalRecord> {
    if row.consume_seq.get() >= frontier.domain || row.consume_offset >= frontier.bytes {
        return Err(invalid("user echo consume crosses its published frontier"));
    }
    let record = working_index::read_record(journal, row.presentation.record)?;
    if record.seq != row.presentation.position
        || row
            .presentation
            .record
            .offset
            .checked_add(row.presentation.record.len)
            .is_none_or(|end| end >= frontier.bytes)
        || !matches!(&record.fact, JournalFact::Presentation { event, .. }
            if matches!(&event.content, HistoryContent::User(chunk) if history::provisional_user_echo(chunk)))
    {
        return Err(invalid(
            "indexed provisional echo differs from its original block",
        ));
    }
    Ok(record)
}

//! Derived prompt-catalog ranks for one published tool index.
//!
//! Chunk metadata is captured while the journal suffix is already being
//! published. A query seeks `(generation, ordinal)` and returns at most the
//! requested page; it does not walk every prompt or reopen message bodies.
//! Ordinary appends extend the current generation. Consuming a provisional
//! echo whose source follows the ranked tail extends that generation as well;
//! revealing an older source, or a branch publication that changes
//! visibility, rebuilds the next generation from the stored metadata.

use crate::session::history::{
    HistoryContent, HistoryFact, HistorySourceKey, HistoryTimelineEntry, HistoryTimelinePage,
    JournalFact, PromptPiece, TextChannel, TimelineQuery, catalog_from_pieces, expand_piece,
    page_bounds, page_from_bounds, prompt_piece, prompt_piece_from_text,
};
use redb::{ReadableDatabase, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::{fs::File, io};

const CHUNKS: TableDefinition<u64, &[u8]> = TableDefinition::new("timeline_prompt_chunk");
const RANKS: TableDefinition<(u64, u64), &[u8]> = TableDefinition::new("timeline_prompt_rank");
const SPANS: TableDefinition<u64, &[u8]> = TableDefinition::new("timeline_prompt_span");
/// First ordinal of this message in the generation. A later append does not
/// replace it, so an older span ignores ids ranked past its own count.
const SEEN: TableDefinition<(u64, &str), u64> = TableDefinition::new("timeline_prompt_seen");
const STATE: TableDefinition<(), &[u8]> = TableDefinition::new("timeline_prompt_state");

#[derive(Serialize, Deserialize)]
struct StoredChunk {
    part: u64,
    message_id: Option<String>,
    previews: Vec<String>,
    provisional: bool,
    revealed: bool,
}

#[derive(Serialize, Deserialize)]
struct RankRow {
    record: u64,
    part: u64,
    #[serde(default)]
    display_segment: Option<u32>,
    message_id: Option<String>,
    preview: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Span {
    generation: u64,
    count: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct State {
    generation: u64,
    count: u64,
    branch: Option<u64>,
}

pub(super) struct Writer<'tx> {
    chunks: Table<'tx, u64, &'static [u8]>,
    ranks: Table<'tx, (u64, u64), &'static [u8]>,
    spans: Table<'tx, u64, &'static [u8]>,
    seen: Table<'tx, (u64, &'static str), u64>,
    state_row: Table<'tx, (), &'static [u8]>,
    state: Option<State>,
    fresh: Vec<u64>,
    reorder: bool,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    serde_json::from_slice(bytes).map_err(io::Error::other)
}

impl<'tx> Writer<'tx> {
    pub(super) fn open(tx: &'tx WriteTransaction) -> io::Result<Self> {
        let state_row = tx.open_table(STATE).map_err(io::Error::other)?;
        let state = state_row
            .get(())
            .map_err(io::Error::other)?
            .map(|row| decode(row.value()))
            .transpose()?;
        Ok(Self {
            chunks: tx.open_table(CHUNKS).map_err(io::Error::other)?,
            ranks: tx.open_table(RANKS).map_err(io::Error::other)?,
            spans: tx.open_table(SPANS).map_err(io::Error::other)?,
            seen: tx.open_table(SEEN).map_err(io::Error::other)?,
            state_row,
            state,
            fresh: Vec::new(),
            reorder: false,
        })
    }

    pub(super) fn observe(
        &mut self,
        record: u64,
        event: &HistoryFact,
        journal: &mut File,
        read_reference: &impl Fn(
            chat_state::EventSeq,
        ) -> io::Result<Option<super::working_index::RecordRef>>,
    ) -> io::Result<()> {
        if event
            .meta
            .as_ref()
            .is_some_and(|meta| meta.contains_key("grow/displayProjectionSource"))
        {
            return Ok(());
        }
        match &event.content {
            HistoryContent::User(chunk) => self.note_user(record, chunk),
            HistoryContent::Message {
                channel: TextChannel::User,
                ..
            } => self.note_inherited(record, event, journal, read_reference),
            _ => Ok(()),
        }
    }

    /// `positions` are the presentation record numbers consume just revealed.
    /// They address `CHUNKS` by its own key, so the revealed sources join
    /// `fresh` and extend the ranked tail. Revealing a source that precedes
    /// the last ranked record inserts an earlier ordinal, which only a
    /// rebuild can express.
    pub(super) fn reveal(&mut self, positions: &[u64]) -> io::Result<()> {
        let last_ranked = self.last_ranked_record()?;
        for &record in positions {
            let Some(mut chunk) = self.read_chunk(record)? else {
                continue;
            };
            if !chunk.provisional || chunk.revealed {
                continue;
            }
            chunk.revealed = true;
            self.write_chunk(record, &chunk)?;
            self.fresh.retain(|&fresh| fresh != record);
            let at = self.fresh.partition_point(|&fresh| fresh < record);
            self.fresh.insert(at, record);
            if last_ranked.is_some_and(|last| record < last) {
                self.reorder = true;
            }
        }
        Ok(())
    }

    pub(super) fn finish(
        &mut self,
        tx: &WriteTransaction,
        branch: Option<u64>,
        bytes: u64,
    ) -> io::Result<()> {
        let branch_changed = self
            .state
            .as_ref()
            .is_some_and(|state| state.branch != branch);
        if self.state.is_none() || self.reorder || branch_changed {
            self.rebuild(tx, branch, bytes)
        } else {
            self.append(tx, branch, bytes)
        }
    }

    fn note_user(
        &mut self,
        record: u64,
        chunk: &acp_transport::protocol::ContentChunk,
    ) -> io::Result<()> {
        let Some(piece) = prompt_piece(HistorySourceKey { record, part: 0 }, chunk) else {
            return Ok(());
        };
        self.insert(
            &piece,
            crate::session::history::provisional_user_echo(chunk),
        )
    }

    fn note_inherited(
        &mut self,
        record: u64,
        event: &HistoryFact,
        journal: &mut File,
        read_reference: &impl Fn(
            chat_state::EventSeq,
        ) -> io::Result<Option<super::working_index::RecordRef>>,
    ) -> io::Result<()> {
        let HistoryContent::Message { event: source, .. } = &event.content else {
            unreachable!()
        };
        let reference = read_reference(chat_state::EventSeq::new(*source))?
            .ok_or_else(|| invalid("missing inherited user prompt source"))?;
        let stored = super::working_index::read_record(journal, reference)?;
        let JournalFact::Domain { event: domain, .. } = &stored.fact else {
            return Err(invalid(
                "inherited user prompt source is not a domain record",
            ));
        };
        let text = event.inherited_item(domain)?.text_content();
        let Some(piece) = prompt_piece_from_text(HistorySourceKey { record, part: 0 }, &text)
        else {
            return Ok(());
        };
        self.insert(&piece, false)
    }

    fn insert(&mut self, piece: &PromptPiece, provisional: bool) -> io::Result<()> {
        if self
            .chunks
            .get(piece.source.record)
            .map_err(io::Error::other)?
            .is_some()
        {
            return Err(invalid("duplicate timeline prompt chunk"));
        }
        let stored = StoredChunk {
            part: piece.source.part,
            message_id: piece.message_id.clone(),
            previews: piece.previews.clone(),
            provisional,
            revealed: false,
        };
        self.write_chunk(piece.source.record, &stored)?;
        self.fresh.push(piece.source.record);
        Ok(())
    }

    fn append(&mut self, tx: &WriteTransaction, branch: Option<u64>, bytes: u64) -> io::Result<()> {
        let state = self.state.clone().unwrap_or(State {
            generation: 0,
            count: 0,
            branch,
        });
        let mut ordinal = state.count;
        for record in self.fresh.clone() {
            let Some(chunk) = self.read_chunk(record)? else {
                continue;
            };
            if !visible(&chunk, record, tx, branch)? {
                continue;
            }
            if let Some(id) = chunk.message_id.as_deref()
                && self
                    .seen
                    .get(&(state.generation, id))
                    .map_err(io::Error::other)?
                    .is_some()
            {
                continue;
            }
            let first = ordinal;
            let entries = expand_piece(&piece_from(record, &chunk), ordinal);
            for entry in &entries {
                self.write_rank(state.generation, entry)?;
            }
            ordinal += entries.len() as u64;
            if let Some(id) = chunk.message_id.as_deref() {
                self.seen
                    .insert(&(state.generation, id), first)
                    .map_err(io::Error::other)?;
            }
        }
        self.store(
            bytes,
            State {
                generation: state.generation,
                count: ordinal,
                branch,
            },
        )
    }

    fn rebuild(
        &mut self,
        tx: &WriteTransaction,
        branch: Option<u64>,
        bytes: u64,
    ) -> io::Result<()> {
        let generation = self
            .state
            .as_ref()
            .map(|state| state.generation.saturating_add(1))
            .unwrap_or(0);
        let mut pieces = Vec::new();
        for row in self.chunks.iter().map_err(io::Error::other)? {
            let (key, value) = row.map_err(io::Error::other)?;
            let record = key.value();
            let chunk: StoredChunk = decode(value.value())?;
            if visible(&chunk, record, tx, branch)? {
                pieces.push(piece_from(record, &chunk));
            }
        }
        let entries = catalog_from_pieces(pieces);
        for entry in &entries {
            self.write_rank(generation, entry)?;
            if let Some(id) = entry.message_id.as_deref()
                && self
                    .seen
                    .get(&(generation, id))
                    .map_err(io::Error::other)?
                    .is_none()
            {
                self.seen
                    .insert(&(generation, id), entry.ordinal)
                    .map_err(io::Error::other)?;
            }
        }
        self.store(
            bytes,
            State {
                generation,
                count: entries.len() as u64,
                branch,
            },
        )
    }

    fn last_ranked_record(&self) -> io::Result<Option<u64>> {
        let Some(state) = &self.state else {
            return Ok(None);
        };
        if state.count == 0 {
            return Ok(None);
        }
        let row = self
            .ranks
            .get(&(state.generation, state.count - 1))
            .map_err(io::Error::other)?
            .ok_or_else(|| invalid("history timeline rank is not contiguous"))?;
        Ok(Some(decode::<RankRow>(row.value())?.record))
    }

    fn write_rank(&mut self, generation: u64, entry: &HistoryTimelineEntry) -> io::Result<()> {
        let row = RankRow {
            record: entry.source.record,
            part: entry.source.part,
            display_segment: entry.display_segment,
            message_id: entry.message_id.clone(),
            preview: entry.preview.clone(),
        };
        self.ranks
            .insert(
                &(generation, entry.ordinal),
                serde_json::to_vec(&row)?.as_slice(),
            )
            .map_err(io::Error::other)?;
        Ok(())
    }

    fn store(&mut self, bytes: u64, state: State) -> io::Result<()> {
        let span = Span {
            generation: state.generation,
            count: state.count,
        };
        self.spans
            .insert(bytes, serde_json::to_vec(&span)?.as_slice())
            .map_err(io::Error::other)?;
        self.state_row
            .insert((), serde_json::to_vec(&state)?.as_slice())
            .map_err(io::Error::other)?;
        self.state = Some(state);
        self.fresh.clear();
        self.reorder = false;
        Ok(())
    }

    fn read_chunk(&self, record: u64) -> io::Result<Option<StoredChunk>> {
        self.chunks
            .get(record)
            .map_err(io::Error::other)?
            .map(|row| decode(row.value()))
            .transpose()
    }

    fn write_chunk(&mut self, record: u64, chunk: &StoredChunk) -> io::Result<()> {
        self.chunks
            .insert(record, serde_json::to_vec(chunk)?.as_slice())
            .map_err(io::Error::other)?;
        Ok(())
    }
}

fn piece_from(record: u64, chunk: &StoredChunk) -> PromptPiece {
    PromptPiece {
        source: HistorySourceKey {
            record,
            part: chunk.part,
        },
        message_id: chunk.message_id.clone(),
        previews: chunk.previews.clone(),
    }
}

fn visible(
    chunk: &StoredChunk,
    record: u64,
    tx: &WriteTransaction,
    branch: Option<u64>,
) -> io::Result<bool> {
    if chunk.previews.is_empty() || (chunk.provisional && !chunk.revealed) {
        return Ok(false);
    }
    super::branch_history::visible_at_write(tx, branch, record)
}

pub(super) fn read_page(
    db: &redb::Database,
    bytes: u64,
    query: TimelineQuery,
    limit: usize,
) -> io::Result<HistoryTimelinePage> {
    let tx = db.begin_read().map_err(io::Error::other)?;
    let spans = tx.open_table(SPANS).map_err(io::Error::other)?;
    let span: Span = spans
        .get(bytes)
        .map_err(io::Error::other)?
        .map(|row| decode(row.value()))
        .transpose()?
        .ok_or_else(|| invalid("history timeline is not published at this cut"))?;
    let (start, end) = page_bounds(span.count, query, limit)?;
    let ranks = tx.open_table(RANKS).map_err(io::Error::other)?;
    let mut entries = Vec::with_capacity((end - start) as usize);
    if start < end {
        for row in ranks
            .range((span.generation, start)..(span.generation, end))
            .map_err(io::Error::other)?
        {
            let (key, value) = row.map_err(io::Error::other)?;
            let ordinal = key.value().1;
            if key.value().0 != span.generation || ordinal != start + entries.len() as u64 {
                return Err(invalid("history timeline rank is not contiguous"));
            }
            let rank: RankRow = decode(value.value())?;
            entries.push(HistoryTimelineEntry {
                ordinal,
                source: HistorySourceKey {
                    record: rank.record,
                    part: rank.part,
                },
                message_id: rank.message_id,
                display_segment: rank.display_segment,
                preview: rank.preview,
            });
        }
        if entries.len() as u64 != end - start {
            return Err(invalid("history timeline rank is incomplete"));
        }
    }
    Ok(page_from_bounds(span.count, start, entries))
}

#[cfg(test)]
pub(super) fn read_generation(db: &redb::Database, bytes: u64) -> io::Result<u64> {
    let tx = db.begin_read().map_err(io::Error::other)?;
    let spans = tx.open_table(SPANS).map_err(io::Error::other)?;
    let span: Span = spans
        .get(bytes)
        .map_err(io::Error::other)?
        .map(|row| decode(row.value()))
        .transpose()?
        .ok_or_else(|| invalid("history timeline is not published at this cut"))?;
    Ok(span.generation)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RankBound {
    pub ordinal: u64,
    pub exact: bool,
}

#[cfg(test)]
std::thread_local! {
    static RANK_READS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn note_rank_read() {
    #[cfg(test)]
    RANK_READS.with(|reads| reads.set(reads.get().saturating_add(1)));
}

#[cfg(test)]
pub(super) fn reset_rank_reads() {
    RANK_READS.with(|reads| reads.set(0));
}

#[cfg(test)]
pub(super) fn rank_reads() -> u64 {
    RANK_READS.with(|reads| reads.get())
}

fn published_span(tx: &redb::ReadTransaction, bytes: u64) -> io::Result<Span> {
    let spans = tx.open_table(SPANS).map_err(io::Error::other)?;
    spans
        .get(bytes)
        .map_err(io::Error::other)?
        .map(|row| decode(row.value()))
        .transpose()?
        .ok_or_else(|| invalid("history timeline is not published at this cut"))
}

fn rank_order(record: u64, part: u64, segment: Option<u32>) -> (u64, u64, u32) {
    (record, part, segment.unwrap_or(0))
}

fn read_rank_row(
    ranks: &redb::ReadOnlyTable<(u64, u64), &[u8]>,
    generation: u64,
    ordinal: u64,
) -> io::Result<RankRow> {
    let row = ranks
        .get(&(generation, ordinal))
        .map_err(io::Error::other)?
        .ok_or_else(|| invalid("history timeline rank is not contiguous"))?;
    note_rank_read();
    let rank: RankRow = decode(row.value())?;
    Ok(rank)
}

/// `None` means this cut has no published catalog. A present count can be zero
/// when every stored prompt is still provisional.
pub(super) fn read_count(db: &redb::Database, bytes: u64) -> io::Result<Option<u64>> {
    let tx = db.begin_read().map_err(io::Error::other)?;
    match published_span(&tx, bytes) {
        Ok(span) => Ok(Some(span.count)),
        Err(error)
            if error.kind() == io::ErrorKind::InvalidData
                && error.to_string() == "history timeline is not published at this cut" =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// First published ordinal whose `(record, part, segment)` is not before the
/// probe. `exact` is source identity, not preview text. Each step reads one
/// rank row, so a capture probes O(log count) metadata.
pub(super) fn rank_lower_bound(
    db: &redb::Database,
    bytes: u64,
    source: HistorySourceKey,
    display_segment: Option<u32>,
) -> io::Result<RankBound> {
    let tx = db.begin_read().map_err(io::Error::other)?;
    let span = published_span(&tx, bytes)?;
    let ranks = tx.open_table(RANKS).map_err(io::Error::other)?;
    let probe = rank_order(source.record, source.part, display_segment);
    let mut lo = 0u64;
    let mut hi = span.count;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let rank = read_rank_row(&ranks, span.generation, mid)?;
        if rank_order(rank.record, rank.part, rank.display_segment) < probe {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let exact = if lo < span.count {
        let rank = read_rank_row(&ranks, span.generation, lo)?;
        rank.record == source.record
            && rank.part == source.part
            && rank.display_segment == display_segment
    } else {
        false
    };
    Ok(RankBound { ordinal: lo, exact })
}

pub(super) fn message_seen(db: &redb::Database, bytes: u64, message_id: &str) -> io::Result<bool> {
    let tx = db.begin_read().map_err(io::Error::other)?;
    let span = published_span(&tx, bytes)?;
    let seen = tx.open_table(SEEN).map_err(io::Error::other)?;
    let Some(row) = seen
        .get(&(span.generation, message_id))
        .map_err(io::Error::other)?
    else {
        return Ok(false);
    };
    Ok(row.value() < span.count)
}

/// Rank rows in `[start, end)`. Callers pass a page-sized window; this does
/// not walk the rest of the generation.
pub(super) fn read_rank_window(
    db: &redb::Database,
    bytes: u64,
    start: u64,
    end: u64,
) -> io::Result<Vec<HistoryTimelineEntry>> {
    if start > end {
        return Err(invalid("history timeline rank is not contiguous"));
    }
    let tx = db.begin_read().map_err(io::Error::other)?;
    let span = published_span(&tx, bytes)?;
    if end > span.count {
        return Err(invalid("history timeline rank is incomplete"));
    }
    let ranks = tx.open_table(RANKS).map_err(io::Error::other)?;
    let mut entries = Vec::with_capacity((end - start) as usize);
    if start < end {
        for row in ranks
            .range((span.generation, start)..(span.generation, end))
            .map_err(io::Error::other)?
        {
            let (key, value) = row.map_err(io::Error::other)?;
            let ordinal = key.value().1;
            if key.value().0 != span.generation || ordinal != start + entries.len() as u64 {
                return Err(invalid("history timeline rank is not contiguous"));
            }
            note_rank_read();
            let rank: RankRow = decode(value.value())?;
            entries.push(HistoryTimelineEntry {
                ordinal,
                source: HistorySourceKey {
                    record: rank.record,
                    part: rank.part,
                },
                message_id: rank.message_id,
                display_segment: rank.display_segment,
                preview: rank.preview,
            });
        }
        if entries.len() as u64 != end - start {
            return Err(invalid("history timeline rank is incomplete"));
        }
    }
    Ok(entries)
}

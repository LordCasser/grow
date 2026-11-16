use super::JsonlStorageAdapter;
use super::tool_history::{DomainReference, Frontier, read_domain};
use crate::session::storage::{self, ContainedDirectory};
use chat_state::{
    CompactionEvent, EventSeq, MessageCause, SidebandPurpose, SurfaceId, SurfaceOp, TimelineEvent,
    TimelineEventKind,
};
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::{fs::File, io};

const COMPACTIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("compaction_identity");
const SPAWNS: TableDefinition<&str, &[u8]> = TableDefinition::new("sideband_spawn_identity");
const UNRESOLVED: TableDefinition<&str, &[u8]> = TableDefinition::new("compaction_sidebands");
const CUTS: TableDefinition<u64, &[u8]> = TableDefinition::new("compaction_at_frontier");

#[derive(Default, Serialize, Deserialize)]
struct Cut {
    open: Option<Open>,
    completed_prompt: Option<usize>,
    sidebands: Vec<DomainReference>,
    #[serde(default)]
    legacy_sidebands: Vec<DomainReference>,
}

#[derive(Serialize, Deserialize)]
struct Open {
    id: String,
    start: DomainReference,
    prompt: usize,
    promoted: Option<DomainReference>,
    summary: Option<(DomainReference, String)>,
    replacement: Option<DomainReference>,
}

pub(super) struct Writer<'a> {
    compactions: Table<'a, &'static str, &'static [u8]>,
    spawns: Table<'a, &'static str, &'static [u8]>,
    unresolved: Table<'a, &'static str, &'static [u8]>,
    cuts: Table<'a, u64, &'static [u8]>,
    cut: Cut,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn archives_terminal_sideband(purpose: SidebandPurpose) -> bool {
    matches!(
        purpose,
        SidebandPurpose::SideQuestion
            | SidebandPurpose::PromptSuggestion
            | SidebandPurpose::SessionRecap
            | SidebandPurpose::RangeSummary
            | SidebandPurpose::PermissionJudgment
            | SidebandPurpose::LazinessJudgment
            | SidebandPurpose::MemoryDream
            | SidebandPurpose::MemoryFlush
            | SidebandPurpose::MemoryRewrite
            | SidebandPurpose::ProgressReport
            | SidebandPurpose::InfoRequest
            | SidebandPurpose::ImageDescription
            | SidebandPurpose::ContextRecall
    )
}

impl<'a> Writer<'a> {
    pub(super) fn open(tx: &'a WriteTransaction, bytes: u64) -> io::Result<Self> {
        let cuts = tx.open_table(CUTS).map_err(io::Error::other)?;
        let mut cut: Cut = match cuts.get(bytes).map_err(io::Error::other)? {
            Some(row) => serde_json::from_slice(row.value())?,
            None if bytes == 0 => Cut::default(),
            None => return Err(invalid("missing compaction publication")),
        };
        // Rebuild active references from UNRESOLVED, while retaining terminal
        // legacy recalls without reopening their ledgers at later publications.
        cut.sidebands.clear();
        Ok(Self {
            compactions: tx.open_table(COMPACTIONS).map_err(io::Error::other)?,
            spawns: tx.open_table(SPAWNS).map_err(io::Error::other)?,
            unresolved: tx.open_table(UNRESOLVED).map_err(io::Error::other)?,
            cuts,
            cut,
        })
    }

    pub(super) fn accept(
        &mut self,
        event: &TimelineEvent,
        source: DomainReference,
    ) -> io::Result<Option<OpenTerminal>> {
        match &event.kind {
            TimelineEventKind::Sideband(spawn) => {
                let encoded = serde_json::to_vec(&source)?;
                if self
                    .spawns
                    .insert(spawn.sideband_id.as_str(), encoded.as_slice())
                    .map_err(io::Error::other)?
                    .is_some()
                {
                    return Err(invalid("duplicate indexed Sideband spawn"));
                }
                if spawn.purpose == SidebandPurpose::CompactionSummary
                    || archives_terminal_sideband(spawn.purpose)
                {
                    self.unresolved
                        .insert(spawn.sideband_id.as_str(), encoded.as_slice())
                        .map_err(io::Error::other)?;
                }
            }
            TimelineEventKind::Compaction(CompactionEvent::Started {
                id, prompt_index, ..
            }) => {
                if self.cut.open.is_some()
                    || self
                        .compactions
                        .insert(id.as_str(), serde_json::to_vec(&source)?.as_slice())
                        .map_err(io::Error::other)?
                        .is_some()
                {
                    return Err(invalid("duplicate or overlapping compaction"));
                }
                self.cut.open = Some(Open {
                    id: id.clone(),
                    start: source,
                    prompt: *prompt_index,
                    promoted: None,
                    summary: None,
                    replacement: None,
                });
            }
            TimelineEventKind::Compaction(CompactionEvent::Promoted { id }) => {
                let open = self.open_for(id)?;
                if open.summary.is_some() || open.promoted.replace(source).is_some() {
                    return Err(invalid("invalid indexed compaction promotion"));
                }
            }
            TimelineEventKind::Compaction(CompactionEvent::Summary { id, result_ref, .. }) => {
                let open = self.open_for(id)?;
                if open
                    .summary
                    .replace((source, result_ref.timeline_id.clone()))
                    .is_some()
                {
                    return Err(invalid("duplicate indexed compaction summary"));
                }
            }
            TimelineEventKind::Messages(message) if message.cause == MessageCause::Compaction => {
                let open = self
                    .cut
                    .open
                    .as_mut()
                    .ok_or_else(|| invalid("compaction replacement has no open transaction"))?;
                if open.summary.is_none() || open.replacement.replace(source).is_some() {
                    return Err(invalid("invalid indexed compaction replacement"));
                }
            }
            TimelineEventKind::Compaction(CompactionEvent::Completed { id, .. })
            | TimelineEventKind::Compaction(CompactionEvent::Failed { id, .. }) => {
                let open = self
                    .cut
                    .open
                    .take()
                    .filter(|open| open.id == *id)
                    .ok_or_else(|| invalid("compaction terminal has no open transaction"))?;
                let completed = matches!(
                    event.kind,
                    TimelineEventKind::Compaction(CompactionEvent::Completed { .. })
                );
                if completed != open.replacement.is_some() {
                    return Err(invalid(
                        "compaction terminal does not match its replacement",
                    ));
                }
                if completed {
                    self.cut.completed_prompt = Some(open.prompt);
                }
                return Ok(open.summary.map(|(summary, sideband)| OpenTerminal {
                    summary,
                    sideband,
                    replacement: open.replacement,
                }));
            }
            TimelineEventKind::Messages(message) if message.cause == MessageCause::Rewind => {
                self.cut.completed_prompt = None;
            }
            _ => {}
        }
        Ok(None)
    }

    pub(super) fn spawn_reference(&self, id: &str) -> io::Result<Option<DomainReference>> {
        self.spawns
            .get(id)
            .map_err(io::Error::other)?
            .map(|value| serde_json::from_slice(value.value()).map_err(io::Error::other))
            .transpose()
    }

    fn open_for(&mut self, id: &str) -> io::Result<&mut Open> {
        self.cut
            .open
            .as_mut()
            .filter(|open| open.id == id)
            .ok_or_else(|| invalid("compaction fact has no open transaction"))
    }

    pub(super) fn publish(
        &mut self,
        directory: &ContainedDirectory,
        journal: &mut File,
        session: &str,
        previous: u64,
        bytes: u64,
        terminals: Vec<OpenTerminal>,
        read_event: &impl Fn(EventSeq) -> io::Result<Option<TimelineEvent>>,
    ) -> io::Result<()> {
        for terminal in terminals {
            let entry = self
                .spawns
                .get(terminal.sideband.as_str())
                .map_err(io::Error::other)?
                .ok_or_else(|| invalid("completed compaction has no Sideband spawn"))?;
            let source = serde_json::from_slice(entry.value())?;
            drop(entry);
            let spawn = read_domain(journal, source, bytes)?;
            let summary = read_domain(journal, terminal.summary, bytes)?;
            let TimelineEventKind::Sideband(spawn) = spawn.kind else {
                return Err(invalid("compaction Sideband lookup does not name a spawn"));
            };
            let TimelineEventKind::Compaction(CompactionEvent::Summary {
                result_ref,
                input_ref,
                target,
                summary_chars,
                ..
            }) = summary.kind
            else {
                return Err(invalid(
                    "compaction summary reference does not name a summary",
                ));
            };
            if source.seq >= summary.seq
                || spawn.sideband_id != terminal.sideband
                || spawn.purpose != SidebandPurpose::CompactionSummary
                || !spawn.source_refs.contains(&input_ref)
                || result_ref.timeline_id != terminal.sideband
                || result_ref.first_seq != result_ref.last_seq
            {
                return Err(invalid(
                    "compaction Sideband linkage differs from its summary",
                ));
            }
            let events = JsonlStorageAdapter::read_sideband_ledger(directory, &terminal.sideband)?
                .ok_or_else(|| invalid("compaction Sideband ledger is missing"))?;
            let sideband = chat_state::SidebandTimeline::from_events(events.clone())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            validate_sideband_spawn(&sideband, session, &spawn, read_event)?;
            let ledgers = storage::SidebandLedgers::from([(terminal.sideband.clone(), events)]);
            let result = storage::completed_sideband_result(
                &ledgers,
                &terminal.sideband,
                result_ref.first_seq,
                "compaction/summary",
            )?;
            if result.raw_output.chars().count() != summary_chars {
                return Err(invalid(
                    "compaction character count differs from the Sideband result",
                ));
            }
            if let Some(reference) = terminal.replacement {
                let replacement = read_domain(journal, reference, bytes)?;
                let message = replacement.messages().ok_or_else(|| {
                    invalid("compaction replacement reference does not name a message")
                })?;
                if message.surface
                    != (SurfaceOp::Replace {
                        start: target.start,
                        end: target.end,
                        shadowed: target.shadowed,
                    })
                    || reference.seq <= summary.seq
                {
                    return Err(invalid(
                        "compaction replacement differs from its summary target",
                    ));
                }
                storage::validate_compaction_replacement(
                    &message.items,
                    &result_ref,
                    &result.raw_output,
                )?;
            }
            self.unresolved
                .remove(terminal.sideband.as_str())
                .map_err(io::Error::other)?;
        }
        let mut settled = Vec::new();
        for row in self.unresolved.iter().map_err(io::Error::other)? {
            let (key, value) = row.map_err(io::Error::other)?;
            let source = serde_json::from_slice(value.value())?;
            let id = key.value();
            let event = read_domain(journal, source, bytes)?;
            let TimelineEventKind::Sideband(spawn) = event.kind else {
                return Err(invalid(
                    "unresolved Sideband reference does not name a spawn",
                ));
            };
            if spawn.sideband_id != id {
                return Err(invalid(
                    "unresolved Sideband identity differs from its journal record",
                ));
            }
            if archives_terminal_sideband(spawn.purpose)
                && let Some(events) = JsonlStorageAdapter::read_sideband_ledger(directory, id)?
            {
                let ledger = chat_state::SidebandTimeline::from_events(events)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                if ledger.is_ended() {
                    validate_sideband_spawn(&ledger, session, &spawn, read_event)?;
                    if spawn.purpose == SidebandPurpose::ContextRecall {
                        if ledger.has_recall_materialization_observation() {
                            validate_recall_observation_cut(
                                &ledger, session, source.seq, read_event,
                            )?;
                        } else {
                            self.cut.legacy_sidebands.push(source);
                        }
                    }
                    settled.push(id.to_owned());
                    continue;
                }
            }
            self.cut.sidebands.push(source);
        }
        for id in settled {
            self.unresolved
                .remove(id.as_str())
                .map_err(io::Error::other)?;
        }
        self.cuts
            .retain(|key, _| key == previous)
            .map_err(io::Error::other)?;
        self.cuts
            .insert(bytes, serde_json::to_vec(&self.cut)?.as_slice())
            .map_err(io::Error::other)?;
        Ok(())
    }
}

pub(super) fn validate_sideband_spawn(
    ledger: &chat_state::SidebandTimeline,
    session: &str,
    spawn: &chat_state::SidebandSpawnEvent,
    read_event: &impl Fn(EventSeq) -> io::Result<Option<TimelineEvent>>,
) -> io::Result<()> {
    let mut read_error = None;
    ledger
        .validate_spawn(session, spawn, |id: SurfaceId| match read_event(id.event) {
            Ok(Some(event)) => event.owns_surface_id(id),
            Ok(None) => false,
            Err(error) => {
                read_error = Some(error);
                false
            }
        })
        .map_err(|error| {
            read_error.unwrap_or_else(|| io::Error::new(io::ErrorKind::InvalidData, error))
        })
}

fn validate_recall_observation_cut(
    ledger: &chat_state::SidebandTimeline,
    session: &str,
    spawn_seq: EventSeq,
    read_event: &impl Fn(EventSeq) -> io::Result<Option<TimelineEvent>>,
) -> io::Result<()> {
    let observation = ledger
        .events()
        .iter()
        .find_map(|event| match &event.kind {
            chat_state::SidebandEventKind::RecallMaterializationObserved(observation) => {
                Some(observation)
            }
            _ => None,
        })
        .ok_or_else(|| invalid("ContextRecall ledger has no materialization observation"))?;
    let source = &observation.source_ref;
    if source.timeline_id != session || source.first_seq != 0 || source.last_seq >= spawn_seq.get()
    {
        return Err(invalid(
            "ContextRecall observation is outside its parent spawn cut",
        ));
    }
    if !read_event(EventSeq::new(source.last_seq))?
        .is_some_and(|event| event.seq.get() == source.last_seq)
    {
        return Err(invalid(
            "ContextRecall observation high-water event is missing",
        ));
    }
    Ok(())
}

pub(super) struct OpenTerminal {
    summary: DomainReference,
    sideband: String,
    replacement: Option<DomainReference>,
}

pub(super) struct Restored {
    pub events: Vec<TimelineEvent>,
    pub completed_prompt: Option<usize>,
}

pub(super) fn restore(
    tx: &ReadTransaction,
    journal: &mut File,
    frontier: Frontier,
) -> io::Result<Restored> {
    let cuts = tx.open_table(CUTS).map_err(io::Error::other)?;
    let row = cuts
        .get(frontier.bytes)
        .map_err(io::Error::other)?
        .ok_or_else(|| invalid("missing published compaction cut"))?;
    let Cut {
        open,
        completed_prompt,
        sidebands,
        legacy_sidebands,
    } = serde_json::from_slice(row.value())?;
    let mut references = sidebands;
    references.extend(legacy_sidebands);
    if let Some(open) = open {
        references.push(open.start);
        references.extend(open.promoted);
        references.extend(open.summary.map(|(source, _)| source));
        references.extend(open.replacement);
    }
    let events = references
        .into_iter()
        .map(|reference| {
            if reference.seq.get() >= frontier.domain {
                return Err(invalid("compaction evidence crosses the domain cut"));
            }
            read_domain(journal, reference, frontier.bytes)
        })
        .collect::<io::Result<_>>()?;
    Ok(Restored {
        events,
        completed_prompt,
    })
}

pub(super) fn identity_before(
    tx: &ReadTransaction,
    journal: &mut File,
    id: &str,
    seq: EventSeq,
    frontier: Frontier,
    sideband: bool,
) -> io::Result<bool> {
    let table = tx
        .open_table(if sideband { SPAWNS } else { COMPACTIONS })
        .map_err(io::Error::other)?;
    let Some(row) = table.get(id).map_err(io::Error::other)? else {
        return Ok(false);
    };
    let reference: DomainReference = serde_json::from_slice(row.value())?;
    if reference.seq >= seq || reference.seq.get() >= frontier.domain {
        return Ok(false);
    }
    let event = read_domain(journal, reference, frontier.bytes)?;
    let valid = match event.kind {
        TimelineEventKind::Sideband(spawn) if sideband => spawn.sideband_id == id,
        TimelineEventKind::Compaction(CompactionEvent::Started { id: key, .. }) if !sideband => {
            key == id
        }
        _ => false,
    };
    if !valid {
        return Err(invalid(
            "compaction history identity differs from its journal record",
        ));
    }
    Ok(true)
}

pub(super) fn sideband_spawn(
    tx: &ReadTransaction,
    journal: &mut File,
    id: &str,
    frontier: Frontier,
) -> io::Result<Option<TimelineEvent>> {
    let table = tx.open_table(SPAWNS).map_err(io::Error::other)?;
    let Some(row) = table.get(id).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let reference: DomainReference = serde_json::from_slice(row.value())?;
    if reference.seq.get() >= frontier.domain {
        return Ok(None);
    }
    let event = read_domain(journal, reference, frontier.bytes)?;
    if !matches!(&event.kind, TimelineEventKind::Sideband(spawn) if spawn.sideband_id == id) {
        return Err(invalid("indexed Sideband spawn differs from journal"));
    }
    Ok(Some(event))
}

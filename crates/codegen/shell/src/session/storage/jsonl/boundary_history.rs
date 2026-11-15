use super::tool_history::{DomainReference, Frontier, read_domain};
use super::working_snapshot::RecordClass;
use chat_state::{
    EventSeq, PromptRecord, StepEvent, StepId, TimelineEvent, TimelineEventKind, TurnEvent, TurnId,
};
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::{fs::File, io};

const TURNS: TableDefinition<u64, &[u8]> = TableDefinition::new("turn_starts");
const STEPS: TableDefinition<(u64, u32), &[u8]> = TableDefinition::new("step_starts");
const COORDINATES: TableDefinition<(u64, u64), u64> = TableDefinition::new("prompt_coordinates");
const PROMPTS: TableDefinition<(u64, u64, u64), &[u8]> = TableDefinition::new("user_prompt_starts");
const DEPENDENCIES: TableDefinition<u64, &[u8]> = TableDefinition::new("boundary_dependencies");
const CUTS: TableDefinition<u64, &[u8]> = TableDefinition::new("boundary_cuts");

#[derive(Default, Deserialize, Serialize)]
struct Cut {
    active_turn: Option<DomainReference>,
    active_step: Option<DomainReference>,
    dependencies: u64,
    next_prompt: usize,
    max_prompt: Option<usize>,
    branch: u64,
    keep_turn: bool,
    keep_step: bool,
}

pub(super) struct Writer<'a> {
    cut: Cut,
    cuts: Table<'a, u64, &'static [u8]>,
    turns: Table<'a, u64, &'static [u8]>,
    steps: Table<'a, (u64, u32), &'static [u8]>,
    coordinates: Table<'a, (u64, u64), u64>,
    prompts: Table<'a, (u64, u64, u64), &'static [u8]>,
    dependencies: Table<'a, u64, &'static [u8]>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl<'a> Writer<'a> {
    pub(super) fn open(tx: &'a WriteTransaction, bytes: u64) -> io::Result<Self> {
        let cuts = tx.open_table(CUTS).map_err(io::Error::other)?;
        let cut = match cuts.get(bytes).map_err(io::Error::other)? {
            Some(row) => serde_json::from_slice(row.value())?,
            None if bytes == 0 => Cut::default(),
            None => return Err(invalid("missing Turn/Step publication")),
        };
        Ok(Self {
            cut,
            cuts,
            turns: tx.open_table(TURNS).map_err(io::Error::other)?,
            steps: tx.open_table(STEPS).map_err(io::Error::other)?,
            coordinates: tx.open_table(COORDINATES).map_err(io::Error::other)?,
            prompts: tx.open_table(PROMPTS).map_err(io::Error::other)?,
            dependencies: tx.open_table(DEPENDENCIES).map_err(io::Error::other)?,
        })
    }

    pub(super) fn accept(
        &mut self,
        event: &TimelineEvent,
        reference: DomainReference,
        class: RecordClass,
    ) -> io::Result<()> {
        match &event.kind {
            TimelineEventKind::Turn(TurnEvent::Started {
                id,
                prompt_index,
                identity,
                ..
            }) => {
                let value = serde_json::to_vec(&reference)?;
                if self.cut.active_turn.replace(reference).is_some()
                    || self
                        .turns
                        .insert(id.0, value.as_slice())
                        .map_err(io::Error::other)?
                        .is_some()
                {
                    return Err(invalid("repeated Turn start in publication"));
                }
                let prompt = *prompt_index as u64;
                if self
                    .coordinates
                    .get((self.cut.branch, prompt))
                    .map_err(io::Error::other)?
                    .is_none()
                {
                    self.coordinates
                        .insert((self.cut.branch, prompt), event.seq.get())
                        .map_err(io::Error::other)?;
                }
                if identity.origin == "user" {
                    self.prompts
                        .insert((self.cut.branch, prompt, event.seq.get()), value.as_slice())
                        .map_err(io::Error::other)?;
                }
                self.cut.max_prompt = self.cut.max_prompt.max(Some(*prompt_index));
                while self
                    .coordinates
                    .get((self.cut.branch, self.cut.next_prompt as u64))
                    .map_err(io::Error::other)?
                    .is_some()
                {
                    self.cut.next_prompt = self
                        .cut
                        .next_prompt
                        .checked_add(1)
                        .ok_or_else(|| invalid("prompt coordinate overflow"))?;
                }
            }
            TimelineEventKind::Turn(TurnEvent::Ended { .. }) => {
                let start = self
                    .cut
                    .active_turn
                    .take()
                    .ok_or_else(|| invalid("Turn end has no start"))?;
                if std::mem::take(&mut self.cut.keep_turn) {
                    self.keep_pair(start, reference)?;
                }
            }
            TimelineEventKind::Step(StepEvent::Started { id }) => {
                if self.cut.active_step.replace(reference).is_some()
                    || self
                        .steps
                        .insert(
                            (id.turn.0, id.index),
                            serde_json::to_vec(&reference)?.as_slice(),
                        )
                        .map_err(io::Error::other)?
                        .is_some()
                {
                    return Err(invalid("repeated Step start in publication"));
                }
            }
            TimelineEventKind::Step(StepEvent::Ended { .. }) => {
                let start = self
                    .cut
                    .active_step
                    .take()
                    .ok_or_else(|| invalid("Step end has no start"))?;
                if std::mem::take(&mut self.cut.keep_step) {
                    self.keep_pair(start, reference)?;
                }
            }
            // Retained facts still use original boundary semantics.
            TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { .. })
                if class != RecordClass::Notification =>
            {
                self.keep_active()
            }
            TimelineEventKind::Messages(message)
                if class == RecordClass::Retained
                    && message.cause == chat_state::MessageCause::ToolResult =>
            {
                self.keep_active();
            }
            _ => {}
        }
        Ok(())
    }

    fn keep_active(&mut self) {
        self.cut.keep_turn |= self.cut.active_turn.is_some();
        self.cut.keep_step |= self.cut.active_step.is_some();
    }

    pub(super) fn select_rewind(
        &mut self,
        source: EventSeq,
        timeline: &chat_state::Timeline,
    ) -> io::Result<()> {
        let branch = source.get() + 1;
        self.cut.branch = branch;
        self.cut.next_prompt = timeline.next_prompt_index();
        self.cut.max_prompt = None;
        for (index, user) in timeline.selected_prompt_coordinates() {
            self.cut.max_prompt = Some(index);
            self.coordinates.insert((branch, index as u64), source.get()).map_err(io::Error::other)?;
            let Some(seq) = user else { continue };
            let Some(TimelineEventKind::Turn(TurnEvent::Started { id, .. })) =
                timeline.event(seq).map(|event| &event.kind)
            else { return Err(invalid("selected prompt has no Turn start")) };
            let reference = self.turns.get(id.0).map_err(io::Error::other)?
                .ok_or_else(|| invalid("selected prompt has no indexed Turn"))?;
            self.prompts.insert((branch, index as u64, seq.get()), reference.value()).map_err(io::Error::other)?;
        }
        Ok(())
    }

    fn keep_pair(&mut self, start: DomainReference, end: DomainReference) -> io::Result<()> {
        for reference in [start, end] {
            self.dependencies
                .insert(
                    self.cut.dependencies,
                    serde_json::to_vec(&reference)?.as_slice(),
                )
                .map_err(io::Error::other)?;
            self.cut.dependencies += 1;
        }
        Ok(())
    }

    pub(super) fn publish(&mut self, previous: u64, bytes: u64) -> io::Result<()> {
        self.cuts
            .retain(|key, _| key == previous)
            .map_err(io::Error::other)?;
        self.cuts
            .insert(bytes, serde_json::to_vec(&self.cut)?.as_slice())
            .map_err(io::Error::other)?;
        Ok(())
    }
}

pub(super) struct Restored {
    pub events: Vec<TimelineEvent>,
    pub next_prompt: usize,
    pub max_prompt: Option<usize>,
}

pub(super) fn restore(
    tx: &ReadTransaction,
    journal: &mut File,
    bytes: u64,
    domain: u64,
) -> io::Result<Restored> {
    let cuts = tx.open_table(CUTS).map_err(io::Error::other)?;
    let cut: Cut = serde_json::from_slice(
        cuts.get(bytes)
            .map_err(io::Error::other)?
            .ok_or_else(|| invalid("missing published Turn/Step cut"))?
            .value(),
    )?;
    let table = tx.open_table(DEPENDENCIES).map_err(io::Error::other)?;
    let mut references = Vec::new();
    for key in 0..cut.dependencies {
        let row = table
            .get(key)
            .map_err(io::Error::other)?
            .ok_or_else(|| invalid("missing retained boundary reference"))?;
        references.push(serde_json::from_slice::<DomainReference>(row.value())?);
    }
    references.extend(cut.active_turn);
    references.extend(cut.active_step);
    let events = references
        .into_iter()
        .map(|reference| {
            if reference.seq.get() >= domain {
                return Err(invalid("retained boundary crosses published sequence"));
            }
            let event = read_domain(journal, reference, bytes)?;
            if !matches!(
                event.kind,
                TimelineEventKind::Turn(_) | TimelineEventKind::Step(_)
            ) {
                return Err(invalid("retained boundary differs from journal"));
            }
            Ok(event)
        })
        .collect::<io::Result<_>>()?;
    Ok(Restored {
        events,
        next_prompt: cut.next_prompt,
        max_prompt: cut.max_prompt,
    })
}

pub(super) fn turn_before(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
    id: TurnId,
    seq: EventSeq,
) -> io::Result<bool> {
    let table = tx.open_table(TURNS).map_err(io::Error::other)?;
    let Some(row) = table.get(id.0).map_err(io::Error::other)? else {
        return Ok(false);
    };
    let reference: DomainReference = serde_json::from_slice(row.value())?;
    if reference.seq.get() >= cut.domain.min(seq.get()) {
        return Ok(false);
    }
    if !matches!(read_domain(journal, reference, cut.bytes)?.kind,
        TimelineEventKind::Turn(TurnEvent::Started { id: found, .. }) if found == id)
    {
        return Err(invalid("Turn index identity differs from journal"));
    }
    Ok(true)
}

pub(super) fn step_before(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
    id: StepId,
    seq: EventSeq,
) -> io::Result<bool> {
    let table = tx.open_table(STEPS).map_err(io::Error::other)?;
    let Some(row) = table.get((id.turn.0, id.index)).map_err(io::Error::other)? else {
        return Ok(false);
    };
    let reference: DomainReference = serde_json::from_slice(row.value())?;
    if reference.seq.get() >= cut.domain.min(seq.get()) {
        return Ok(false);
    }
    if !matches!(read_domain(journal, reference, cut.bytes)?.kind,
        TimelineEventKind::Step(StepEvent::Started { id: found }) if found == id)
    {
        return Err(invalid("Step index identity differs from journal"));
    }
    Ok(true)
}

pub(super) fn has_prompt(tx: &ReadTransaction, cut: Frontier, prompt: usize) -> io::Result<bool> {
    let table = tx.open_table(COORDINATES).map_err(io::Error::other)?;
    Ok(table
        .get((cut.rewind.map_or(0, |seq| seq.get() + 1), prompt as u64))
        .map_err(io::Error::other)?
        .is_some_and(|row| row.value() < cut.domain))
}

pub(super) fn prompts_before(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
    limit: usize,
) -> io::Result<Vec<PromptRecord>> {
    let table = tx.open_table(PROMPTS).map_err(io::Error::other)?;
    let mut records = Vec::new();
    let branch = cut.rewind.map_or(0, |seq| seq.get() + 1);
    let mut before = None;
    while records.len() < limit {
        let row = match before {
            Some(prompt) => table
                .range((branch, 0, 0)..(branch, prompt, 0))
                .map_err(io::Error::other)?
                .next_back(),
            None => table.range((branch, 0, 0)..=(branch, u64::MAX, u64::MAX))
                .map_err(io::Error::other)?.next_back(),
        };
        let Some(row) = row else { break };
        let (key, value) = row.map_err(io::Error::other)?;
        let (_, prompt, seq) = key.value();
        before = Some(prompt);
        let value = if seq < cut.domain {
            Some(value)
        } else {
            table
                .range((branch, prompt, 0)..(branch, prompt, cut.domain))
                .map_err(io::Error::other)?
                .next_back()
                .transpose()
                .map_err(io::Error::other)?
                .map(|(_, value)| value)
        };
        let Some(value) = value else { continue };
        let reference: DomainReference = serde_json::from_slice(value.value())?;
        let event = read_domain(journal, reference, cut.bytes)?;
        let TimelineEventKind::Turn(TurnEvent::Started {
            prompt_index,
            prompt_text,
            input_kind,
            identity,
            ..
        }) = event.kind
        else {
            return Err(invalid("prompt index does not refer to a Turn"));
        };
        if prompt_index as u64 != prompt || identity.origin != "user" {
            return Err(invalid("prompt index differs from journal"));
        }
        records.push(PromptRecord {
            prompt_index,
            text: prompt_text,
            input_kind,
        });
    }
    Ok(records)
}

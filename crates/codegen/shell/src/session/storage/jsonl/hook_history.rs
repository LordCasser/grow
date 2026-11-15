use super::tool_history::{DomainReference, read_domain};
use chat_state::{
    EventSeq, HookCause, HookEvent, HookEventType, HookLifecycleProjection, InputEvent, Timeline,
    TimelineEvent, TimelineEventKind, TurnId,
};
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::{fs::File, io};

const OCCURRENCES: TableDefinition<&str, &[u8]> = TableDefinition::new("hook_occurrence");
const PENDING: TableDefinition<&str, &[u8]> = TableDefinition::new("hook_pending");
const RUNS: TableDefinition<&str, &[u8]> = TableDefinition::new("hook_run");
const INPUTS: TableDefinition<&str, &str> = TableDefinition::new("hook_input");
const CUTS: TableDefinition<u64, &[u8]> = TableDefinition::new("hook_at_frontier");

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    pub session_id: Option<String>,
    pub generation: u64,
    pub references: Vec<DomainReference>,
}

#[derive(Serialize, Deserialize)]
struct Occurrence {
    events: Vec<DomainReference>,
    completed: bool,
    // A completed hook can still be needed by a live execution owner.
    input: Option<String>,
    tool: Option<String>,
    turn: Option<TurnId>,
    handlers: bool,
}

#[derive(Serialize, Deserialize)]
struct Run {
    occurrence: String,
    trigger: DomainReference,
}

pub(super) struct Writer<'tx> {
    occurrences: Table<'tx, &'static str, &'static [u8]>,
    pending: Table<'tx, &'static str, &'static [u8]>,
    runs: Table<'tx, &'static str, &'static [u8]>,
    inputs: Table<'tx, &'static str, &'static str>,
    cuts: Table<'tx, u64, &'static [u8]>,
    checkpoint: Checkpoint,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    serde_json::from_slice(bytes).map_err(io::Error::other)
}

impl<'tx> Writer<'tx> {
    pub(super) fn open(tx: &'tx WriteTransaction, previous: u64) -> io::Result<Self> {
        let cuts = tx.open_table(CUTS).map_err(io::Error::other)?;
        let mut checkpoint: Checkpoint = match cuts.get(previous).map_err(io::Error::other)? {
            Some(value) => decode(value.value())?,
            None if previous == 0 => Checkpoint::default(),
            None => return Err(invalid("missing prior Hook checkpoint")),
        };
        checkpoint.references.clear();
        Ok(Self {
            occurrences: tx.open_table(OCCURRENCES).map_err(io::Error::other)?,
            pending: tx.open_table(PENDING).map_err(io::Error::other)?,
            runs: tx.open_table(RUNS).map_err(io::Error::other)?,
            inputs: tx.open_table(INPUTS).map_err(io::Error::other)?,
            cuts,
            checkpoint,
        })
    }

    /// Returns whether a terminal has a visible projection.
    pub(super) fn accept(
        &mut self,
        event: &TimelineEvent,
        reference: DomainReference,
        active_turn: Option<TurnId>,
    ) -> io::Result<bool> {
        if let TimelineEventKind::Input(InputEvent::AdmissionResolved { input_id, .. }) =
            &event.kind
        {
            let occurrence = self
                .inputs
                .get(input_id.as_str())
                .map_err(io::Error::other)?
                .map(|value| value.value().to_owned());
            if let Some(id) = occurrence {
                self.pending.remove(id.as_str()).map_err(io::Error::other)?;
            }
            return Ok(false);
        }
        let TimelineEventKind::Hook(hook) = &event.kind else {
            return Ok(false);
        };
        let id = hook.occurrence_id();
        let mut occurrence = if let HookEvent::Triggered {
            event,
            cause,
            config_generation,
            handlers,
            ..
        } = hook
        {
            if self
                .occurrences
                .get(id)
                .map_err(io::Error::other)?
                .is_some()
            {
                return Err(invalid("duplicate indexed Hook occurrence"));
            }
            self.checkpoint.generation = self.checkpoint.generation.max(*config_generation);
            if let HookCause::Session { session_id } = cause {
                self.checkpoint
                    .session_id
                    .get_or_insert_with(|| session_id.clone());
            }
            for handler in handlers {
                let run = Run {
                    occurrence: id.to_owned(),
                    trigger: reference,
                };
                if self
                    .runs
                    .insert(
                        handler.run_id.as_str(),
                        serde_json::to_vec(&run)?.as_slice(),
                    )
                    .map_err(io::Error::other)?
                    .is_some()
                {
                    return Err(invalid("duplicate indexed Hook run"));
                }
            }
            let input = match cause {
                HookCause::Input { input_id } => {
                    self.inputs
                        .insert(input_id.as_str(), id)
                        .map_err(io::Error::other)?;
                    Some(input_id.clone())
                }
                _ => None,
            };
            let tool = match (event, cause) {
                (HookEventType::PreToolUse, HookCause::Tool { call_id }) => Some(call_id.clone()),
                _ => None,
            };
            let turn = match event {
                HookEventType::Stop | HookEventType::SubagentStop => active_turn,
                _ => None,
            };
            Occurrence {
                events: Vec::new(),
                completed: false,
                input,
                tool,
                turn,
                handlers: !handlers.is_empty(),
            }
        } else {
            let value = self
                .occurrences
                .get(id)
                .map_err(io::Error::other)?
                .ok_or_else(|| invalid("indexed Hook fact has no trigger"))?;
            let occurrence: Occurrence = decode(value.value())?;
            if occurrence.completed {
                return Err(invalid("indexed Hook occurrence is already terminal"));
            }
            occurrence
        };
        occurrence.events.push(reference);
        occurrence.completed = matches!(hook, HookEvent::Completed { .. });
        let encoded = serde_json::to_vec(&occurrence)?;
        self.occurrences
            .insert(id, encoded.as_slice())
            .map_err(io::Error::other)?;
        if !occurrence.completed
            || occurrence.input.is_some()
            || occurrence.tool.is_some()
            || occurrence.turn.is_some()
        {
            self.pending
                .insert(id, encoded.as_slice())
                .map_err(io::Error::other)?;
        } else {
            self.pending.remove(id).map_err(io::Error::other)?;
        }
        Ok(occurrence.completed && occurrence.handlers)
    }

    pub(super) fn publish(
        &mut self,
        previous: u64,
        bytes: u64,
        turn: Option<TurnId>,
        tool_is_open: impl Fn(&str) -> io::Result<bool>,
        input_is_live: impl Fn(&str) -> io::Result<bool>,
    ) -> io::Result<()> {
        let mut settled = Vec::new();
        for row in self.pending.iter().map_err(io::Error::other)? {
            let (id, value) = row.map_err(io::Error::other)?;
            let occurrence: Occurrence = decode(value.value())?;
            let open_tool = occurrence
                .tool
                .as_deref()
                .map(&tool_is_open)
                .transpose()?
                .unwrap_or(false);
            let live_input = occurrence
                .input
                .as_deref()
                .map(&input_is_live)
                .transpose()?
                .unwrap_or(false);
            if !occurrence.completed
                || live_input
                || open_tool
                || occurrence.turn.is_some_and(|owner| turn == Some(owner))
            {
                self.checkpoint.references.extend(occurrence.events);
            } else {
                settled.push(id.value().to_owned());
            }
        }
        for id in settled {
            self.pending.remove(id.as_str()).map_err(io::Error::other)?;
        }
        self.cuts
            .retain(|key, _| key == previous)
            .map_err(io::Error::other)?;
        self.cuts
            .insert(bytes, serde_json::to_vec(&self.checkpoint)?.as_slice())
            .map_err(io::Error::other)?;
        Ok(())
    }
}

pub(super) fn checkpoint(tx: &ReadTransaction, bytes: u64) -> io::Result<Checkpoint> {
    let table = tx.open_table(CUTS).map_err(io::Error::other)?;
    let value = table
        .get(bytes)
        .map_err(io::Error::other)?
        .ok_or_else(|| invalid("missing published Hook checkpoint"))?;
    decode(value.value())
}

pub(super) fn occurrence(
    tx: &ReadTransaction,
    journal: &mut File,
    id: &str,
    bytes: u64,
    before: EventSeq,
) -> io::Result<Option<HookLifecycleProjection>> {
    let table = tx.open_table(OCCURRENCES).map_err(io::Error::other)?;
    let Some(value) = table.get(id).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let entry: Occurrence = decode(value.value())?;
    let events = entry
        .events
        .into_iter()
        .take_while(|reference| reference.seq < before)
        .map(|reference| read_domain(journal, reference, bytes))
        .collect::<io::Result<Vec<_>>>()?;
    if events.is_empty() {
        return Ok(None);
    }
    let projection = Timeline::project_indexed_hook(&events)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if projection.occurrence_id != id {
        return Err(invalid("Hook index occurrence differs from journal"));
    }
    Ok(Some(projection))
}

pub(super) fn run_before(
    tx: &ReadTransaction,
    journal: &mut File,
    run_id: &str,
    bytes: u64,
    before: EventSeq,
) -> io::Result<bool> {
    let table = tx.open_table(RUNS).map_err(io::Error::other)?;
    let Some(value) = table.get(run_id).map_err(io::Error::other)? else {
        return Ok(false);
    };
    let run: Run = decode(value.value())?;
    if run.trigger.seq >= before {
        return Ok(false);
    }
    let trigger = read_domain(journal, run.trigger, bytes)?;
    if !matches!(trigger.kind, TimelineEventKind::Hook(HookEvent::Triggered { occurrence_id, handlers, .. })
        if occurrence_id == run.occurrence && handlers.iter().any(|handler| handler.run_id == run_id))
    {
        return Err(invalid("Hook index run differs from journal"));
    }
    Ok(true)
}

pub(super) fn input_occurrence(
    tx: &ReadTransaction,
    journal: &mut File,
    input_id: &str,
    bytes: u64,
    before: EventSeq,
) -> io::Result<Option<HookLifecycleProjection>> {
    let table = tx.open_table(INPUTS).map_err(io::Error::other)?;
    let Some(id) = table.get(input_id).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let projection = occurrence(tx, journal, id.value(), bytes, before)?;
    if projection.as_ref().is_some_and(
        |hook| !matches!(&hook.cause, HookCause::Input { input_id: id } if id == input_id),
    ) {
        return Err(invalid("Hook index input differs from journal"));
    }
    Ok(projection)
}

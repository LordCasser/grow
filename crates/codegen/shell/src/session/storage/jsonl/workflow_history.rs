use super::tool_history::{DomainReference, Frontier, read_domain};
use chat_state::{EventSeq, TimelineEvent, TimelineEventKind, WorkflowEvent};
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::{fs::File, io};

const SPAWNS: TableDefinition<u64, &str> = TableDefinition::new("workflow_spawns");
const BOUNDARIES: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("workflow_boundaries");
const OPEN: TableDefinition<&str, &[u8]> = TableDefinition::new("workflow_open");
const CUTS: TableDefinition<u64, &[u8]> = TableDefinition::new("workflow_cuts");

#[derive(Clone, Copy, Deserialize, Serialize)]
struct Boundaries {
    spawn: DomainReference,
    latest: DomainReference,
}

pub(super) struct Writer<'a> {
    spawns: Table<'a, u64, &'static str>,
    boundaries: Table<'a, (&'static str, u64), &'static [u8]>,
    open: Table<'a, &'static str, &'static [u8]>,
    cuts: Table<'a, u64, &'static [u8]>,
}

impl<'a> Writer<'a> {
    pub(super) fn open(tx: &'a WriteTransaction) -> io::Result<Self> {
        Ok(Self {
            spawns: tx.open_table(SPAWNS).map_err(io::Error::other)?,
            boundaries: tx.open_table(BOUNDARIES).map_err(io::Error::other)?,
            open: tx.open_table(OPEN).map_err(io::Error::other)?,
            cuts: tx.open_table(CUTS).map_err(io::Error::other)?,
        })
    }

    pub(super) fn accept(
        &mut self,
        event: &TimelineEvent,
        reference: DomainReference,
    ) -> io::Result<()> {
        let TimelineEventKind::Workflow(workflow) = &event.kind else {
            return Ok(());
        };
        let id = workflow.run_id();
        let prior = lookup(&self.boundaries, id, reference.seq.get())?;
        let spawn = match (workflow, prior) {
            (WorkflowEvent::Spawned { .. }, None) => {
                self.spawns
                    .insert(reference.seq.get(), id)
                    .map_err(io::Error::other)?;
                reference
            }
            (WorkflowEvent::Spawned { .. }, Some(_)) => {
                return Err(invalid("duplicate indexed Workflow spawn"));
            }
            (_, Some(prior)) => prior.spawn,
            (_, None) => return Err(invalid("indexed Workflow boundary has no spawn")),
        };
        let encoded = serde_json::to_vec(&Boundaries {
            spawn,
            latest: reference,
        })?;
        self.boundaries
            .insert((id, reference.seq.get()), encoded.as_slice())
            .map_err(io::Error::other)?;
        if matches!(
            workflow,
            WorkflowEvent::Spawned { .. }
                | WorkflowEvent::Resumed { .. }
                | WorkflowEvent::Ended {
                    execution_suspended: true,
                    ..
                }
        ) {
            self.open
                .insert(id, encoded.as_slice())
                .map_err(io::Error::other)?;
        } else {
            self.open.remove(id).map_err(io::Error::other)?;
        }
        Ok(())
    }

    pub(super) fn publish(&mut self, previous: u64, bytes: u64) -> io::Result<()> {
        let open = self
            .open
            .iter()
            .map_err(io::Error::other)?
            .map(|row| {
                let (run_id, refs) = row.map_err(io::Error::other)?;
                Ok((
                    run_id.value().to_owned(),
                    serde_json::from_slice::<Boundaries>(refs.value())?,
                ))
            })
            .collect::<io::Result<Vec<_>>>()?;
        self.cuts
            .retain(|key, _| key == previous)
            .map_err(io::Error::other)?;
        self.cuts
            .insert(bytes, serde_json::to_vec(&open)?.as_slice())
            .map_err(io::Error::other)?;
        Ok(())
    }
}

pub(super) fn open_workflows(
    tx: &ReadTransaction,
    journal: &mut File,
    frontier: Frontier,
) -> io::Result<Vec<(TimelineEvent, TimelineEvent)>> {
    let cuts = tx.open_table(CUTS).map_err(io::Error::other)?;
    let cut = cuts
        .get(frontier.bytes)
        .map_err(io::Error::other)?
        .ok_or_else(|| invalid("missing open Workflow checkpoint"))?;
    serde_json::from_slice::<Vec<(String, Boundaries)>>(cut.value())?
        .into_iter()
        .map(|(id, references)| read_boundaries(journal, &id, references, frontier))
        .collect()
}

pub(super) fn workflow_before(
    tx: &ReadTransaction,
    journal: &mut File,
    frontier: Frontier,
    run_id: &str,
    seq: EventSeq,
) -> io::Result<Option<(TimelineEvent, TimelineEvent)>> {
    let boundaries = tx.open_table(BOUNDARIES).map_err(io::Error::other)?;
    lookup(&boundaries, run_id, frontier.domain.min(seq.get()))?
        .map(|references| read_boundaries(journal, run_id, references, frontier))
        .transpose()
}

pub(super) fn spawns_before(
    tx: &ReadTransaction,
    end: u64,
    limit: usize,
) -> io::Result<Vec<(EventSeq, String)>> {
    let spawns = tx.open_table(SPAWNS).map_err(io::Error::other)?;
    spawns
        .range(..end)
        .map_err(io::Error::other)?
        .rev()
        .take(limit)
        .map(|row| {
            let (seq, run_id) = row.map_err(io::Error::other)?;
            Ok((EventSeq::new(seq.value()), run_id.value().to_owned()))
        })
        .collect()
}

fn lookup(
    table: &impl ReadableTable<(&'static str, u64), &'static [u8]>,
    run_id: &str,
    end: u64,
) -> io::Result<Option<Boundaries>> {
    let last = table
        .range((run_id, 0)..(run_id, end))
        .map_err(io::Error::other)?
        .next_back()
        .transpose()
        .map_err(io::Error::other)?;
    last.map(|(key, value)| {
        let references: Boundaries = serde_json::from_slice(value.value())?;
        if references.latest.seq.get() != key.value().1 {
            return Err(invalid(
                "Workflow boundary coordinate differs from its index",
            ));
        }
        Ok(references)
    })
    .transpose()
}

fn read_boundaries(
    journal: &mut File,
    run_id: &str,
    references: Boundaries,
    frontier: Frontier,
) -> io::Result<(TimelineEvent, TimelineEvent)> {
    if references.latest.seq.get() >= frontier.domain
        || references.spawn.seq > references.latest.seq
    {
        return Err(invalid("Workflow boundary crosses its verified frontier"));
    }
    let spawn = read_domain(journal, references.spawn, frontier.bytes)?;
    let latest = if references.spawn.seq == references.latest.seq {
        spawn.clone()
    } else {
        read_domain(journal, references.latest, frontier.bytes)?
    };
    if !matches!(&spawn.kind, TimelineEventKind::Workflow(WorkflowEvent::Spawned { run_id: id, .. }) if id == run_id)
        || !matches!(&latest.kind, TimelineEventKind::Workflow(event) if event.run_id() == run_id)
    {
        return Err(invalid(
            "Workflow boundary differs from its journal identity",
        ));
    }
    Ok((spawn, latest))
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

//! Disk index for child lifecycle facts outside the resident Timeline.

use super::tool_history::{DomainReference, read_domain};
use chat_state::{
    EventSeq, MessageCause, NotificationEvent, NotificationSource, SubagentEvent, SurfaceOp,
    TimelineEvent, TimelineEventKind, TurnId,
};
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::{fs::File, io};

const SPAWNS: TableDefinition<&str, &[u8]> = TableDefinition::new("subagent_spawn");
const CHILDREN: TableDefinition<&str, &str> = TableDefinition::new("subagent_child");
const OPEN: TableDefinition<&str, &[u8]> = TableDefinition::new("subagent_open");
const PENDING: TableDefinition<&str, ()> = TableDefinition::new("subagent_pending");
const RECEIPTS: TableDefinition<&str, &str> = TableDefinition::new("subagent_receipt");
const RECEIVED: TableDefinition<&str, ()> = TableDefinition::new("subagent_received_receipt");
const RECEIPT_COUNTS: TableDefinition<&str, u64> = TableDefinition::new("subagent_receipt_count");
const PLANS: TableDefinition<&str, &[u8]> = TableDefinition::new("subagent_plan_link");
const EVENTS: TableDefinition<u64, &[u8]> = TableDefinition::new("subagent_event");
const CUTS: TableDefinition<u64, &[u8]> = TableDefinition::new("subagent_at_frontier");
const DELIVERIES: TableDefinition<&str, &[u8]> = TableDefinition::new("subagent_delivery");

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    spawn: DomainReference,
    child_session_id: String,
    surface_completion: bool,
    parent_step_id: Option<String>,
    terminal: Option<Terminal>,
    usage_settled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Delivery {
    reference: DomainReference,
    turn: TurnId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Terminal {
    reference: DomainReference,
    outcome: chat_state::SubagentOutcome,
    tokens_used: u64,
    turns: u32,
    result_ref: Option<chat_state::TimelineRangeRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanLink {
    parent_step_id: String,
    child_session_id: String,
    terminal: bool,
    result_id: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    pub references: Vec<DomainReference>,
}

pub(super) struct Writer<'tx> {
    spawns: Table<'tx, &'static str, &'static [u8]>,
    children: Table<'tx, &'static str, &'static str>,
    open: Table<'tx, &'static str, &'static [u8]>,
    pending: Table<'tx, &'static str, ()>,
    receipts: Table<'tx, &'static str, &'static str>,
    received: Table<'tx, &'static str, ()>,
    receipt_counts: Table<'tx, &'static str, u64>,
    plans: Table<'tx, &'static str, &'static [u8]>,
    events: Table<'tx, u64, &'static [u8]>,
    cuts: Table<'tx, u64, &'static [u8]>,
    deliveries: Table<'tx, &'static str, &'static [u8]>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    serde_json::from_slice(bytes).map_err(io::Error::other)
}

impl<'tx> Writer<'tx> {
    pub(super) fn delivered(&self, id: &str) -> io::Result<bool> {
        Ok(self.deliveries.get(id).map_err(io::Error::other)?.is_some())
    }
    pub(super) fn open(tx: &'tx WriteTransaction) -> io::Result<Self> {
        Ok(Self {
            spawns: tx.open_table(SPAWNS).map_err(io::Error::other)?,
            children: tx.open_table(CHILDREN).map_err(io::Error::other)?,
            open: tx.open_table(OPEN).map_err(io::Error::other)?,
            pending: tx.open_table(PENDING).map_err(io::Error::other)?,
            receipts: tx.open_table(RECEIPTS).map_err(io::Error::other)?,
            received: tx.open_table(RECEIVED).map_err(io::Error::other)?,
            receipt_counts: tx.open_table(RECEIPT_COUNTS).map_err(io::Error::other)?,
            plans: tx.open_table(PLANS).map_err(io::Error::other)?,
            events: tx.open_table(EVENTS).map_err(io::Error::other)?,
            cuts: tx.open_table(CUTS).map_err(io::Error::other)?,
            deliveries: tx.open_table(DELIVERIES).map_err(io::Error::other)?,
        })
    }

    pub(super) fn accept(
        &mut self,
        event: &TimelineEvent,
        reference: DomainReference,
        active_turn: Option<TurnId>,
    ) -> io::Result<()> {
        match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)) => {
                if self
                    .spawns
                    .get(spawn.subagent_id.as_str())
                    .map_err(io::Error::other)?
                    .is_some()
                {
                    return Err(invalid("duplicate indexed Subagent spawn"));
                }
                if self
                    .children
                    .get(spawn.child_session_id.as_str())
                    .map_err(io::Error::other)?
                    .is_some()
                {
                    return Err(invalid("duplicate indexed Subagent child"));
                }
                let entry = Entry {
                    spawn: reference,
                    child_session_id: spawn.child_session_id.clone(),
                    surface_completion: spawn.surface_completion,
                    parent_step_id: spawn.parent_step_id.clone(),
                    terminal: None,
                    usage_settled: false,
                };
                self.spawns
                    .insert(
                        spawn.subagent_id.as_str(),
                        serde_json::to_vec(&entry)?.as_slice(),
                    )
                    .map_err(io::Error::other)?;
                self.children
                    .insert(spawn.child_session_id.as_str(), spawn.subagent_id.as_str())
                    .map_err(io::Error::other)?;
                self.open
                    .insert(
                        spawn.subagent_id.as_str(),
                        serde_json::to_vec(&reference)?.as_slice(),
                    )
                    .map_err(io::Error::other)?;
                self.events
                    .insert(
                        reference.seq.get(),
                        serde_json::to_vec(&reference)?.as_slice(),
                    )
                    .map_err(io::Error::other)?;
            }
            TimelineEventKind::Subagent(SubagentEvent::Ended(terminal)) => {
                let Some(value) = self
                    .spawns
                    .get(terminal.subagent_id.as_str())
                    .map_err(io::Error::other)?
                else {
                    return Err(invalid("indexed Subagent terminal has no spawn"));
                };
                let mut entry: Entry = decode(value.value())?;
                drop(value);
                if entry.terminal.is_some() {
                    return Err(invalid("duplicate indexed Subagent terminal"));
                }
                // The spawn reference is verified when its source is read by the
                // index; the child identity in the terminal is checked against
                // the unique child map here.
                let child = self
                    .children
                    .get(terminal.child_session_id.as_str())
                    .map_err(io::Error::other)?;
                if child.as_ref().map(|value| value.value()) != Some(terminal.subagent_id.as_str())
                {
                    return Err(invalid(
                        "indexed Subagent terminal child differs from spawn",
                    ));
                }
                entry.terminal = Some(Terminal {
                    reference,
                    outcome: terminal.outcome,
                    tokens_used: terminal.tokens_used,
                    turns: terminal.turns,
                    result_ref: terminal.result_ref.clone(),
                });
                self.spawns
                    .insert(
                        terminal.subagent_id.as_str(),
                        serde_json::to_vec(&entry)?.as_slice(),
                    )
                    .map_err(io::Error::other)?;
                self.open
                    .remove(terminal.subagent_id.as_str())
                    .map_err(io::Error::other)?;
                self.pending
                    .insert(terminal.subagent_id.as_str(), ())
                    .map_err(io::Error::other)?;
                self.events
                    .insert(
                        reference.seq.get(),
                        serde_json::to_vec(&reference)?.as_slice(),
                    )
                    .map_err(io::Error::other)?;
            }
            TimelineEventKind::Observation(observation)
                if observation.scope == "session_usage"
                    && observation.name == "subagent_settled" =>
            {
                let Some(id) = observation
                    .data
                    .as_ref()
                    .and_then(|value| value.get("subagent_id"))
                    .and_then(serde_json::Value::as_str)
                else {
                    return Err(invalid("indexed Subagent usage settlement has no identity"));
                };
                let entry = {
                    let value = self.spawns.get(id).map_err(io::Error::other)?;
                    value
                        .map(|value| decode::<Entry>(value.value()))
                        .transpose()?
                };
                if let Some(mut entry) = entry {
                    entry.usage_settled = true;
                    self.spawns
                        .insert(id, serde_json::to_vec(&entry)?.as_slice())
                        .map_err(io::Error::other)?;
                }
            }
            TimelineEventKind::Notification(NotificationEvent::Received {
                id,
                source: NotificationSource::SubagentCompleted { subagent_id, .. },
                ..
            }) => {
                if self
                    .spawns
                    .get(subagent_id.as_str())
                    .map_err(io::Error::other)?
                    .is_none()
                {
                    // The notification inbox owns receipt validity. A child
                    // lifecycle only tracks delivery obligations for its spawns.
                    return Ok(());
                }
                self.received
                    .insert(subagent_id.as_str(), ())
                    .map_err(io::Error::other)?;
                if self
                    .deliveries
                    .get(subagent_id.as_str())
                    .map_err(io::Error::other)?
                    .is_some()
                {
                    return Ok(());
                }
                if self
                    .receipts
                    .insert(id.as_str(), subagent_id.as_str())
                    .map_err(io::Error::other)?
                    .is_some()
                {
                    return Err(invalid("duplicate indexed Subagent completion receipt"));
                }
                let count = self
                    .receipt_counts
                    .get(subagent_id.as_str())
                    .map_err(io::Error::other)?
                    .map_or(0, |value| value.value());
                self.receipt_counts
                    .insert(subagent_id.as_str(), count + 1)
                    .map_err(io::Error::other)?;
            }
            TimelineEventKind::Messages(messages)
                if active_turn.is_some()
                    && messages.cause == MessageCause::ToolResult
                    && matches!(messages.surface, SurfaceOp::Append) =>
            {
                let turn = active_turn.expect("guarded active turn");
                for item in &messages.items {
                    let sampling_types::conversation::ConversationItem::ToolResult(result) = item
                    else {
                        continue;
                    };
                    let Some(id) = result.delivered_subagent_id.as_deref() else {
                        continue;
                    };
                    if self.deliveries.get(id).map_err(io::Error::other)?.is_none() {
                        self.deliveries
                            .insert(
                                id,
                                serde_json::to_vec(&Delivery { reference, turn })?.as_slice(),
                            )
                            .map_err(io::Error::other)?;
                        let mut receipt_ids = Vec::new();
                        for row in self.receipts.iter().map_err(io::Error::other)? {
                            let (receipt_id, subagent_id) = row.map_err(io::Error::other)?;
                            if subagent_id.value() == id {
                                receipt_ids.push(receipt_id.value().to_owned());
                            }
                        }
                        for receipt_id in receipt_ids {
                            self.receipts
                                .remove(receipt_id.as_str())
                                .map_err(io::Error::other)?;
                            let count = self
                                .receipt_counts
                                .get(id)
                                .map_err(io::Error::other)?
                                .map_or(0, |value| value.value());
                            if count <= 1 {
                                self.receipt_counts.remove(id).map_err(io::Error::other)?;
                            } else {
                                self.receipt_counts
                                    .insert(id, count - 1)
                                    .map_err(io::Error::other)?;
                            }
                        }
                    }
                }
            }
            TimelineEventKind::Notification(NotificationEvent::Consumed {
                notification_ids,
                ..
            })
            | TimelineEventKind::Notification(NotificationEvent::Dismissed {
                notification_ids,
                ..
            }) => {
                for id in notification_ids {
                    if let Some(value) = self
                        .receipts
                        .remove(id.as_str())
                        .map_err(io::Error::other)?
                    {
                        let subagent_id = value.value().to_owned();
                        let count = self
                            .receipt_counts
                            .get(subagent_id.as_str())
                            .map_err(io::Error::other)?
                            .map_or(0, |value| value.value());
                        if count <= 1 {
                            self.receipt_counts
                                .remove(subagent_id.as_str())
                                .map_err(io::Error::other)?;
                        } else {
                            self.receipt_counts
                                .insert(subagent_id.as_str(), count - 1)
                                .map_err(io::Error::other)?;
                        }
                    }
                }
            }
            TimelineEventKind::Planning(planning) => {
                let plan = &planning.plan;
                let mut links = std::collections::BTreeSet::new();
                for link in &plan.links {
                    links.insert(link.id.as_str());
                    let value = PlanLink {
                        parent_step_id: link.parent_step_id.clone(),
                        child_session_id: link.child_session_id.clone(),
                        terminal: link.terminal,
                        result_id: link.result_id.clone(),
                    };
                    self.plans
                        .insert(link.id.as_str(), serde_json::to_vec(&value)?.as_slice())
                        .map_err(io::Error::other)?;
                }
                self.plans
                    .retain(|id, _| links.contains(id))
                    .map_err(io::Error::other)?;
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn publish(&mut self, previous: u64, bytes: u64) -> io::Result<()> {
        let mut checkpoint = Checkpoint::default();
        for row in self.open.iter().map_err(io::Error::other)? {
            let (_, value) = row.map_err(io::Error::other)?;
            checkpoint.references.push(decode(value.value())?);
        }
        let mut settled = Vec::new();
        for row in self.pending.iter().map_err(io::Error::other)? {
            let (id, _) = row.map_err(io::Error::other)?;
            let value = self
                .spawns
                .get(id.value())
                .map_err(io::Error::other)?
                .ok_or_else(|| invalid("pending Subagent has no indexed spawn"))?;
            let entry: Entry = decode(value.value())?;
            let terminal = entry
                .terminal
                .as_ref()
                .ok_or_else(|| invalid("pending Subagent has no terminal"))?;
            let pending_receipts = self
                .receipt_counts
                .get(id.value())
                .map_err(io::Error::other)?
                .is_some_and(|value| value.value() > 0);
            let notification_required = entry.surface_completion
                && terminal.outcome != chat_state::SubagentOutcome::Cancelled;
            let notification_pending = notification_required
                && self
                    .deliveries
                    .get(id.value())
                    .map_err(io::Error::other)?
                    .is_none()
                && (pending_receipts
                    || self
                        .received
                        .get(id.value())
                        .map_err(io::Error::other)?
                        .is_none());
            let usage_pending =
                (terminal.tokens_used > 0 || terminal.turns > 0) && !entry.usage_settled;
            let planning_pending = match (
                &entry.parent_step_id,
                self.plans.get(id.value()).map_err(io::Error::other)?,
            ) {
                (None, _) => false,
                (Some(_), Some(link)) => {
                    let link: PlanLink = decode(link.value())?;
                    !link.terminal
                        || link.parent_step_id != *entry.parent_step_id.as_ref().expect("matched")
                        || link.child_session_id != entry.child_session_id
                        || link.result_id.as_deref()
                            != terminal
                                .result_ref
                                .as_ref()
                                .map(serde_json::to_string)
                                .transpose()?
                                .as_deref()
                }
                (Some(_), None) => true,
            };
            if notification_pending || usage_pending || planning_pending {
                checkpoint.references.push(entry.spawn);
                checkpoint.references.push(terminal.reference);
            } else {
                settled.push(id.value().to_owned());
            }
        }
        checkpoint
            .references
            .sort_unstable_by_key(|reference| reference.seq);
        self.cuts
            .retain(|key, _| key == previous)
            .map_err(io::Error::other)?;
        self.cuts
            .insert(bytes, serde_json::to_vec(&checkpoint)?.as_slice())
            .map_err(io::Error::other)?;
        for id in settled {
            // The facts remain in SPAWNS/EVENTS for point lookups and explicit
            // history reads; only the working Timeline drops them.
            self.pending.remove(id.as_str()).map_err(io::Error::other)?;
        }
        Ok(())
    }
}

pub(super) fn checkpoint(tx: &ReadTransaction, bytes: u64) -> io::Result<Checkpoint> {
    let table = tx.open_table(CUTS).map_err(io::Error::other)?;
    let value = table
        .get(bytes)
        .map_err(io::Error::other)?
        .ok_or_else(|| invalid("missing published Subagent checkpoint"))?;
    decode(value.value())
}

pub(super) fn read_open_events(
    journal: &mut File,
    checkpoint: &Checkpoint,
    bytes: u64,
    domain: u64,
) -> io::Result<Vec<TimelineEvent>> {
    checkpoint
        .references
        .iter()
        .map(|reference| {
            if reference.seq.get() >= domain {
                return Err(invalid(
                    "Subagent checkpoint reference crosses its frontier",
                ));
            }
            let event = read_domain(journal, *reference, bytes)?;
            if !matches!(
                event.kind,
                TimelineEventKind::Subagent(SubagentEvent::Spawned(_) | SubagentEvent::Ended(_))
            ) {
                return Err(invalid(
                    "Subagent checkpoint reference differs from lifecycle",
                ));
            }
            Ok(event)
        })
        .collect()
}

fn spawn_entry(
    tx: &ReadTransaction,
    journal: &mut File,
    subagent_id: &str,
    bytes: u64,
    domain: u64,
    before: EventSeq,
) -> io::Result<Option<(EventSeq, chat_state::SubagentSpawnEvent)>> {
    let table = tx.open_table(SPAWNS).map_err(io::Error::other)?;
    let Some(value) = table.get(subagent_id).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let entry: Entry = decode(value.value())?;
    if entry.spawn.seq.get() >= domain.min(before.get()) {
        return Ok(None);
    }
    let event = read_domain(journal, entry.spawn, bytes)?;
    match event.kind {
        TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))
            if spawn.subagent_id == subagent_id
                && spawn.child_session_id == entry.child_session_id =>
        {
            Ok(Some((event.seq, *spawn)))
        }
        _ => Err(invalid(
            "indexed Subagent spawn identity differs from journal",
        )),
    }
}

pub(super) fn spawn_before(
    tx: &ReadTransaction,
    journal: &mut File,
    subagent_id: &str,
    bytes: u64,
    domain: u64,
    before: EventSeq,
) -> io::Result<Option<(EventSeq, chat_state::SubagentSpawnEvent)>> {
    spawn_entry(tx, journal, subagent_id, bytes, domain, before)
}

pub(super) fn spawn_for_child_before(
    tx: &ReadTransaction,
    journal: &mut File,
    child_session_id: &str,
    bytes: u64,
    domain: u64,
    before: EventSeq,
) -> io::Result<Option<(EventSeq, chat_state::SubagentSpawnEvent)>> {
    let table = tx.open_table(CHILDREN).map_err(io::Error::other)?;
    let Some(subagent_id) = table.get(child_session_id).map_err(io::Error::other)? else {
        return Ok(None);
    };
    spawn_entry(tx, journal, subagent_id.value(), bytes, domain, before)
}

pub(super) fn terminal_before(
    tx: &ReadTransaction,
    journal: &mut File,
    subagent_id: &str,
    bytes: u64,
    domain: u64,
    before: EventSeq,
) -> io::Result<Option<(EventSeq, chat_state::SubagentTerminalEvent)>> {
    let table = tx.open_table(SPAWNS).map_err(io::Error::other)?;
    let Some(value) = table.get(subagent_id).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let entry: Entry = decode(value.value())?;
    let Some(terminal) = entry.terminal else {
        return Ok(None);
    };
    if terminal.reference.seq.get() >= domain.min(before.get()) {
        return Ok(None);
    }
    let event = read_domain(journal, terminal.reference, bytes)?;
    match event.kind {
        TimelineEventKind::Subagent(SubagentEvent::Ended(value))
            if value.subagent_id == subagent_id
                && value.child_session_id == entry.child_session_id =>
        {
            Ok(Some((event.seq, value)))
        }
        _ => Err(invalid(
            "indexed Subagent terminal identity differs from journal",
        )),
    }
}

pub(super) fn delivery_before(
    tx: &ReadTransaction,
    journal: &mut File,
    subagent_id: &str,
    frontier: super::tool_history::Frontier,
    before: EventSeq,
) -> io::Result<Option<TurnId>> {
    let table = tx.open_table(DELIVERIES).map_err(io::Error::other)?;
    let Some(value) = table.get(subagent_id).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let delivery: Delivery = decode(value.value())?;
    if delivery.reference.seq.get() >= frontier.domain.min(before.get()) {
        return Ok(None);
    }
    let event = read_domain(journal, delivery.reference, frontier.bytes)?;
    let valid_source = matches!(&event.kind,
        TimelineEventKind::Messages(messages)
            if messages.cause == MessageCause::ToolResult
                && matches!(messages.surface, SurfaceOp::Append)
                && messages.items.iter().any(|item| matches!(
                    item,
                    sampling_types::conversation::ConversationItem::ToolResult(result)
                        if result.delivered_subagent_id.as_deref() == Some(subagent_id)
                ))
    );
    if !valid_source {
        return Err(invalid(
            "indexed Subagent delivery differs from ToolResult source",
        ));
    }
    if !super::boundary_history::turn_before(tx, journal, frontier, delivery.turn, event.seq)? {
        return Err(invalid(
            "indexed Subagent delivery Turn differs from journal",
        ));
    }
    Ok(Some(delivery.turn))
}

pub(super) fn all_events_before(
    tx: &ReadTransaction,
    journal: &mut File,
    bytes: u64,
    domain: u64,
) -> io::Result<Vec<TimelineEvent>> {
    let table = tx.open_table(EVENTS).map_err(io::Error::other)?;
    table
        .range(0..domain)
        .map_err(io::Error::other)?
        .map(|row| {
            let (seq, value) = row.map_err(io::Error::other)?;
            let reference: DomainReference = decode(value.value())?;
            if reference.seq.get() != seq.value() {
                return Err(invalid("Subagent event index sequence differs"));
            }
            let event = read_domain(journal, reference, bytes)?;
            if !matches!(event.kind, TimelineEventKind::Subagent(_)) {
                return Err(invalid("Subagent event index points to another event kind"));
            }
            Ok(event)
        })
        .collect()
}

pub(super) fn events_for_child_before(
    tx: &ReadTransaction,
    journal: &mut File,
    child_session_id: &str,
    bytes: u64,
    domain: u64,
    before: EventSeq,
) -> io::Result<Vec<TimelineEvent>> {
    let children = tx.open_table(CHILDREN).map_err(io::Error::other)?;
    let Some(owner) = children.get(child_session_id).map_err(io::Error::other)? else {
        return Ok(Vec::new());
    };
    let spawns = tx.open_table(SPAWNS).map_err(io::Error::other)?;
    let stored = spawns
        .get(owner.value())
        .map_err(io::Error::other)?
        .ok_or_else(|| invalid("indexed child has no spawn reference"))?;
    let entry: Entry = decode(stored.value())?;
    let frontier = domain.min(before.get());
    if entry.spawn.seq.get() >= frontier {
        return Ok(Vec::new());
    }
    let start = read_domain(journal, entry.spawn, bytes)?;
    if !matches!(&start.kind, TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))
        if spawn.child_session_id == child_session_id && spawn.subagent_id == owner.value())
    {
        return Err(invalid("indexed child differs from its original spawn"));
    }
    let mut events = vec![start];
    if let Some(terminal) = entry
        .terminal
        .filter(|terminal| terminal.reference.seq.get() < frontier)
    {
        let event = read_domain(journal, terminal.reference, bytes)?;
        if !matches!(&event.kind, TimelineEventKind::Subagent(SubagentEvent::Ended(end))
            if end.child_session_id == child_session_id && end.subagent_id == owner.value())
        {
            return Err(invalid("indexed child differs from its original terminal"));
        }
        events.push(event);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::storage::jsonl::working_index::RecordRef;
    use redb::{Database, ReadableDatabase, ReadableTableMetadata};

    #[test]
    fn settled_child_history_does_not_grow_the_working_checkpoint() {
        const CLOSED_CHILDREN: u64 = 20_000;
        let directory = tempfile::tempdir().unwrap();
        let database = Database::create(directory.path().join("children.redb")).unwrap();
        let tx = database.begin_write().unwrap();
        let mut writer = Writer::open(&tx).unwrap();
        let digest = blake3::hash(b"synthetic record");

        for child in 0..CLOSED_CHILDREN {
            let id = format!("child-{child}");
            let child_session_id = format!("session-{child}");
            let spawn = DomainReference {
                seq: EventSeq::new(child * 2),
                record: RecordRef {
                    offset: child * 2,
                    len: 1,
                    digest,
                },
            };
            let terminal = Terminal {
                reference: DomainReference {
                    seq: EventSeq::new(child * 2 + 1),
                    record: RecordRef {
                        offset: child * 2 + 1,
                        len: 1,
                        digest,
                    },
                },
                outcome: chat_state::SubagentOutcome::Completed,
                tokens_used: 0,
                turns: 0,
                result_ref: None,
            };
            let entry = Entry {
                spawn,
                child_session_id,
                surface_completion: false,
                parent_step_id: None,
                terminal: Some(terminal),
                usage_settled: false,
            };
            writer
                .spawns
                .insert(id.as_str(), serde_json::to_vec(&entry).unwrap().as_slice())
                .unwrap();
            writer.pending.insert(id.as_str(), ()).unwrap();
            writer
                .events
                .insert(
                    spawn.seq.get(),
                    serde_json::to_vec(&spawn).unwrap().as_slice(),
                )
                .unwrap();
            writer
                .events
                .insert(
                    entry.terminal.as_ref().unwrap().reference.seq.get(),
                    serde_json::to_vec(&entry.terminal.as_ref().unwrap().reference)
                        .unwrap()
                        .as_slice(),
                )
                .unwrap();
        }

        let active = DomainReference {
            seq: EventSeq::new(CLOSED_CHILDREN * 2),
            record: RecordRef {
                offset: CLOSED_CHILDREN * 2,
                len: 1,
                digest,
            },
        };
        writer
            .open
            .insert("active", serde_json::to_vec(&active).unwrap().as_slice())
            .unwrap();
        writer.publish(0, CLOSED_CHILDREN * 2 + 1).unwrap();
        drop(writer);
        tx.commit().unwrap();

        let tx = database.begin_read().unwrap();
        let checkpoint = checkpoint(&tx, CLOSED_CHILDREN * 2 + 1).unwrap();
        assert_eq!(checkpoint.references.len(), 1);
        assert_eq!(checkpoint.references[0].seq, active.seq);
        assert_eq!(checkpoint.references[0].record, active.record);
        assert_eq!(tx.open_table(PENDING).unwrap().len().unwrap(), 0);
        assert_eq!(
            tx.open_table(SPAWNS).unwrap().len().unwrap(),
            CLOSED_CHILDREN
        );
        assert_eq!(
            tx.open_table(EVENTS).unwrap().len().unwrap(),
            CLOSED_CHILDREN * 2
        );
    }

    #[test]
    fn delivered_tool_result_settles_child_and_suppresses_late_receipt() {
        use crate::session::storage::jsonl::working_index::RecordRef;

        let directory = tempfile::tempdir().unwrap();
        let database = Database::create(directory.path().join("delivery.redb")).unwrap();
        let tx = database.begin_write().unwrap();
        let mut writer = Writer::open(&tx).unwrap();
        let digest = blake3::hash(b"delivery");
        let reference = |seq| DomainReference {
            seq: EventSeq::new(seq),
            record: RecordRef {
                offset: seq,
                len: 1,
                digest,
            },
        };
        let entry = Entry {
            spawn: reference(1),
            child_session_id: "child-session".into(),
            surface_completion: true,
            parent_step_id: None,
            terminal: Some(Terminal {
                reference: reference(2),
                outcome: chat_state::SubagentOutcome::Completed,
                tokens_used: 0,
                turns: 0,
                result_ref: None,
            }),
            usage_settled: false,
        };
        writer
            .spawns
            .insert("child", serde_json::to_vec(&entry).unwrap().as_slice())
            .unwrap();
        writer.pending.insert("child", ()).unwrap();

        let delivery = TimelineEvent {
            version: 1,
            seq: EventSeq::new(3),
            at_ms: 0,
            kind: TimelineEventKind::Messages(chat_state::MessageEvent {
                cause: MessageCause::ToolResult,
                items: vec![sampling_types::conversation::ConversationItem::ToolResult(
                    sampling_types::conversation::ToolResultItem {
                        tool_call_id: "get-result".into(),
                        content: "done".into(),
                        images: Vec::new(),
                        delivered_subagent_id: Some("child".into()),
                    },
                )],
                surface: SurfaceOp::Append,
                response_admission: None,
            }),
        };
        writer
            .accept(&delivery, reference(3), Some(TurnId(7)))
            .unwrap();
        let receipt = TimelineEvent {
            version: 1,
            seq: EventSeq::new(4),
            at_ms: 0,
            kind: TimelineEventKind::Notification(NotificationEvent::Received {
                id: "late".into(),
                owner_session_id: "parent".into(),
                source: NotificationSource::SubagentCompleted {
                    subagent_id: "child".into(),
                    owner: chat_state::NotificationOwner::Session,
                },
                source_version: chat_state::NotificationSourceVersion::Ordinal { value: 1 },
                payload_ref: chat_state::NotificationPayloadRef {
                    blake3: "x".into(),
                    bytes: 0,
                },
            }),
        };
        writer
            .accept(&receipt, reference(4), Some(TurnId(7)))
            .unwrap();
        let indexed_delivery: Delivery = decode(
            writer
                .deliveries
                .get("child")
                .unwrap()
                .expect("ToolResult delivery is indexed independently")
                .value(),
        )
        .unwrap();
        assert_eq!(indexed_delivery.reference.seq, EventSeq::new(3));
        assert_eq!(indexed_delivery.turn, TurnId(7));
        writer.publish(0, 10).unwrap();
        assert_eq!(
            writer
                .receipt_counts
                .get("child")
                .unwrap()
                .map(|value| value.value()),
            None
        );
        drop(writer);
        tx.commit().unwrap();
        assert!(
            checkpoint(&database.begin_read().unwrap(), 10)
                .unwrap()
                .references
                .is_empty()
        );
    }
}

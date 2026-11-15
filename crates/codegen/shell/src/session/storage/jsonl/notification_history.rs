use super::tool_history::{DisplayReference, DomainReference, Frontier, read_domain};
use crate::session::history::{HistoryContent, JournalFact};
use crate::session::storage::communication_history::{self, ReceiptSource};
use chat_state::{
    EventSeq, NotificationEvent, NotificationSource, NotificationTaskKind, TimelineEvent,
    TimelineEventKind, TurnId, timeline::MAX_PENDING_MONITOR_PROGRESS_PER_TASK,
};
use redb::{ReadTransaction, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use std::{fs::File, io};

const RECEIPTS: TableDefinition<&str, &[u8]> = TableDefinition::new("task_notification_receipt");
const SETTLEMENTS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("task_notification_settlement");
const CONSUMED_MESSAGES: TableDefinition<(&str, u64), &[u8]> =
    TableDefinition::new("consumed_agent_messages");
const TASKS: TableDefinition<(&str, bool), &[u8]> =
    TableDefinition::new("task_notification_terminal");
const OPEN: TableDefinition<&str, &[u8]> = TableDefinition::new("task_notification_open");
const PROGRESS: TableDefinition<(&str, u64), &str> =
    TableDefinition::new("task_notification_progress");
const CHECKPOINTS: TableDefinition<u64, &[u8]> =
    TableDefinition::new("task_notification_checkpoint");
const MESSAGES: TableDefinition<(&str, &str, bool, u64), &[u8]> =
    TableDefinition::new("agent_message_receipts");
const PAYLOADS: TableDefinition<&str, &[u8]> = TableDefinition::new("agent_message_payloads");
pub(super) const NOTICE_ANCHORS: TableDefinition<(u64, u64), &[u8]> =
    TableDefinition::new("agent_message_notice_anchors");
pub(super) const NOTICE_SOURCES: TableDefinition<(&str, &str, &str, u64), &[u8]> =
    TableDefinition::new("communication_notice_sources");

#[derive(Serialize, Deserialize)]
struct Open {
    source: DomainReference,
    progress_task: Option<String>,
    waiting_task: Option<String>,
    child: Option<String>,
}

pub(super) struct Writer<'tx> {
    receipts: Table<'tx, &'static str, &'static [u8]>,
    settlements: Table<'tx, &'static str, &'static [u8]>,
    consumed_messages: Table<'tx, (&'static str, u64), &'static [u8]>,
    tasks: Table<'tx, (&'static str, bool), &'static [u8]>,
    open: Table<'tx, &'static str, &'static [u8]>,
    progress: Table<'tx, (&'static str, u64), &'static str>,
    cuts: Table<'tx, u64, &'static [u8]>,
    messages: Table<'tx, (&'static str, &'static str, bool, u64), &'static [u8]>,
    payloads: Table<'tx, &'static str, &'static [u8]>,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn has_notice_anchor(
    tx: &ReadTransaction, journal: &mut File, cut: Frontier, receipt: u64, records: u64,
) -> io::Result<bool> {
    let table = tx.open_table(NOTICE_ANCHORS).map_err(io::Error::other)?;
    let Some((key, row)) = table.range((receipt, 0)..(receipt, records)).map_err(io::Error::other)?
        .next().transpose().map_err(io::Error::other)?
    else { return Ok(false) };
    let reference: DisplayReference = serde_json::from_slice(row.value())?;
    if reference.position != key.value().1
        || reference.record.offset.checked_add(reference.record.len).is_none_or(|end| end >= cut.bytes)
    {
        return Err(invalid("agent message notice anchor crosses its frontier"));
    }
    let record = super::working_index::read_record(journal, reference.record)?;
    if record.seq != reference.position
        || !matches!(record.fact, JournalFact::Presentation { event, .. }
            if matches!(event.content, HistoryContent::Notice { event: source } if source == receipt))
    {
        return Err(invalid("agent message notice anchor differs from its journal"));
    }
    Ok(true)
}

pub(super) fn notice_source(
    table: &impl ReadableTable<(&'static str, &'static str, &'static str, u64), &'static [u8]>,
    journal: &mut File,
    bytes: u64,
    before: u64,
    identity: &(String, Option<String>, Option<String>),
    reject_conflict: bool,
) -> io::Result<Option<ReceiptSource>> {
    let (correlation, subject, sender) = identity;
    let subject = serde_json::to_string(subject)?;
    let sender = serde_json::to_string(sender)?;
    let key = (correlation.as_str(), subject.as_str(), sender.as_str());
    if let Some((found, row)) = table
        .range((key.0, key.1, key.2, 0)..(key.0, key.1, key.2, before))
        .map_err(io::Error::other)?
        .next()
        .transpose()
        .map_err(io::Error::other)?
    {
        let reference: DomainReference = serde_json::from_slice(row.value())?;
        let original = read_domain(journal, reference, bytes)?;
        if original.seq.get() != found.value().3 || communication_history::receipt_identity(&original)?.as_ref() != Some(identity) {
            return Err(invalid("notice source differs from its journal identity"));
        }
        return Ok(Some(ReceiptSource::from_event(&original)?));
    }
    if reject_conflict {
        for row in table.range((key.0, key.1, "", 0)..).map_err(io::Error::other)? {
            let (found, _) = row.map_err(io::Error::other)?;
            let (correlation, subject, _, seq) = found.value();
            if (correlation, subject) != (key.0, key.1) {
                break;
            }
            if seq < before {
                return Err(invalid("notice source does not match its canonical receipt"));
            }
        }
    }
    Ok(None)
}
impl<'tx> Writer<'tx> {
    pub(super) fn open(tx: &'tx WriteTransaction) -> io::Result<Self> {
        Ok(Self {
            receipts: tx.open_table(RECEIPTS).map_err(io::Error::other)?,
            settlements: tx.open_table(SETTLEMENTS).map_err(io::Error::other)?,
            consumed_messages: tx.open_table(CONSUMED_MESSAGES).map_err(io::Error::other)?,
            tasks: tx.open_table(TASKS).map_err(io::Error::other)?,
            open: tx.open_table(OPEN).map_err(io::Error::other)?,
            progress: tx.open_table(PROGRESS).map_err(io::Error::other)?,
            cuts: tx.open_table(CHECKPOINTS).map_err(io::Error::other)?,
            messages: tx.open_table(MESSAGES).map_err(io::Error::other)?,
            payloads: tx.open_table(PAYLOADS).map_err(io::Error::other)?,
        })
    }

    pub(super) fn accept(
        &mut self,
        event: &TimelineEvent,
        reference: DomainReference,
        session: &str,
        child_already_delivered: impl FnOnce(&str) -> io::Result<bool>,
    ) -> io::Result<()> {
        if let TimelineEventKind::Notification(NotificationEvent::Consumed {
            input: Some(sampling_types::ConversationItem::AgentMessage(batch)),
            ..
        }) = &event.kind
        {
            let encoded = serde_json::to_vec(&reference)?;
            for message in &batch.messages {
                self.consumed_messages
                    .insert(
                        (message.message_id.as_str(), event.seq.get()),
                        encoded.as_slice(),
                    )
                    .map_err(io::Error::other)?;
            }
        }
        match &event.kind {
            TimelineEventKind::Notification(NotificationEvent::Received {
                id,
                source,
                source_version,
                owner_session_id,
                payload_ref,
            }) => {
                if owner_session_id != session
                    || chat_state::notification_id(session, source, source_version)
                        .ok()
                        .as_deref()
                        != Some(id)
                {
                    return Err(invalid(
                        "task notification differs from its session or identity",
                    ));
                }
                let encoded = serde_json::to_vec(&reference)?;
                if self
                    .receipts
                    .insert(id.as_str(), encoded.as_slice())
                    .map_err(io::Error::other)?
                    .is_some()
                {
                    return Err(invalid("duplicate task notification identity"));
                }
                if let Some((sender, operation, _, _)) = source.agent_message() {
                    let guidance = matches!(source, NotificationSource::PlanGuidance { .. });
                    self.messages
                        .insert(
                            (sender, operation, guidance, reference.seq.get()),
                            encoded.as_slice(),
                        )
                        .map_err(io::Error::other)?;
                    if self
                        .payloads
                        .get(payload_ref.blake3.as_str())
                        .map_err(io::Error::other)?
                        .is_none()
                    {
                        self.payloads
                            .insert(payload_ref.blake3.as_str(), encoded.as_slice())
                            .map_err(io::Error::other)?;
                    }
                }
                let mut pending = Open {
                    source: reference,
                    progress_task: None,
                    waiting_task: None,
                    child: None,
                };
                match source {
                    NotificationSource::MonitorProgress { task_id, .. } => {
                        if self
                            .tasks
                            .get((task_id.as_str(), true))
                            .map_err(io::Error::other)?
                            .is_some()
                        {
                            return Ok(());
                        }
                        self.progress
                            .insert((task_id.as_str(), event.seq.get()), id.as_str())
                            .map_err(io::Error::other)?;
                        pending.progress_task = Some(task_id.clone());
                        let older = self
                            .progress
                            .range((task_id.as_str(), 0)..=(task_id.as_str(), event.seq.get()))
                            .map_err(io::Error::other)?
                            .rev()
                            .nth(MAX_PENDING_MONITOR_PROGRESS_PER_TASK)
                            .transpose()
                            .map_err(io::Error::other)?
                            .map(|(_, old)| old.value().to_owned());
                        if let Some(old) = older {
                            self.retire(&old)?;
                        }
                    }
                    NotificationSource::TaskStillRunning { task_id, .. } => {
                        if self
                            .tasks
                            .get((task_id.as_str(), false))
                            .map_err(io::Error::other)?
                            .is_some()
                        {
                            return Ok(());
                        }
                        pending.waiting_task = Some(task_id.clone());
                    }
                    NotificationSource::TaskCompleted {
                        task_id, task_kind, ..
                    } => {
                        let monitor = *task_kind == NotificationTaskKind::Monitor;
                        for kind in [false, true].into_iter().take(if monitor { 2 } else { 1 }) {
                            let key = (task_id.as_str(), kind);
                            if self.tasks.get(key).map_err(io::Error::other)?.is_none() {
                                self.tasks
                                    .insert(key, encoded.as_slice())
                                    .map_err(io::Error::other)?;
                            }
                        }
                        let mut old = Vec::new();
                        for row in self.open.iter().map_err(io::Error::other)? {
                            let (id, value) = row.map_err(io::Error::other)?;
                            let value: Open = serde_json::from_slice(value.value())?;
                            if value.waiting_task.as_ref() == Some(task_id)
                                || (monitor && value.progress_task.as_ref() == Some(task_id))
                            {
                                old.push(id.value().to_owned());
                            }
                        }
                        for old in old {
                            self.retire(&old)?;
                        }
                    }
                    NotificationSource::SubagentCompleted { subagent_id, .. } => {
                        if child_already_delivered(subagent_id)? {
                            return Ok(());
                        }
                        pending.child = Some(subagent_id.clone());
                    }
                    NotificationSource::PlanHandoff { .. }
                    | NotificationSource::InquiryAnswered { .. }
                    | NotificationSource::WorkflowHandoff { .. }
                    | NotificationSource::ParentMessage { .. }
                    | NotificationSource::AgentReply { .. }
                    | NotificationSource::PlanGuidance { .. } => {}
                }
                self.open
                    .insert(id.as_str(), serde_json::to_vec(&pending)?.as_slice())
                    .map_err(io::Error::other)?;
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
                    self.retire(id)?;
                    let encoded = serde_json::to_vec(&reference)?;
                    if self
                        .settlements
                        .insert(id.as_str(), encoded.as_slice())
                        .map_err(io::Error::other)?
                        .is_some()
                    {
                        return Err(invalid("duplicate notification settlement"));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn retire(&mut self, id: &str) -> io::Result<()> {
        let Some(row) = self.open.remove(id).map_err(io::Error::other)? else {
            return Ok(());
        };
        let value: Open = serde_json::from_slice(row.value())?;
        if let Some(task) = value.progress_task {
            self.progress
                .remove((task.as_str(), value.source.seq.get()))
                .map_err(io::Error::other)?;
        }
        Ok(())
    }

    pub(super) fn finish_child(&mut self, child: &str) -> io::Result<()> {
        let mut settled = Vec::new();
        for row in self.open.iter().map_err(io::Error::other)? {
            let (id, value) = row.map_err(io::Error::other)?;
            if serde_json::from_slice::<Open>(value.value())?
                .child
                .as_deref()
                == Some(child)
            {
                settled.push(id.value().to_owned());
            }
        }
        for id in settled {
            self.retire(&id)?;
        }
        Ok(())
    }

    pub(super) fn publish(&mut self, previous: u64, bytes: u64) -> io::Result<()> {
        let mut references = Vec::new();
        for row in self.open.iter().map_err(io::Error::other)? {
            let (_, value) = row.map_err(io::Error::other)?;
            references.push(serde_json::from_slice::<Open>(value.value())?.source);
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

pub(super) fn pending(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
) -> io::Result<Vec<TimelineEvent>> {
    let table = tx.open_table(CHECKPOINTS).map_err(io::Error::other)?;
    let row = table
        .get(cut.bytes)
        .map_err(io::Error::other)?
        .ok_or_else(|| invalid("missing task notification checkpoint"))?;
    let mut refs: Vec<DomainReference> = serde_json::from_slice(row.value())?;
    refs.sort_unstable_by_key(|reference| reference.seq);
    refs.into_iter().map(|reference| {
        read_before(journal, reference, cut, EventSeq::new(cut.domain))?
            .filter(|event| matches!(&event.kind, TimelineEventKind::Notification(NotificationEvent::Received { .. })))
            .ok_or_else(|| invalid("task inbox checkpoint differs from its journal"))
    }).collect()
}

pub(super) fn receipt(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
    id: &str,
    before: EventSeq,
) -> io::Result<Option<TimelineEvent>> {
    let table = tx.open_table(RECEIPTS).map_err(io::Error::other)?;
    let Some(row) = table.get(id).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let event = read_before(journal, serde_json::from_slice(row.value())?, cut, before)?;
    if let Some(event) = &event
        && !matches!(&event.kind, TimelineEventKind::Notification(NotificationEvent::Received { id: original, .. }) if id == original)
    {
        return Err(invalid("task receipt differs from its journal identity"));
    }
    Ok(event)
}

pub(super) fn message_receipts(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
    identity: Option<(&str, &str)>,
) -> io::Result<Vec<TimelineEvent>> {
    let table = tx.open_table(MESSAGES).map_err(io::Error::other)?;
    let rows = match identity {
        Some((sender, operation)) => {
            table.range((sender, operation, false, 0)..=(sender, operation, true, u64::MAX))
        }
        None => table.range::<(&str, &str, bool, u64)>(..),
    }
    .map_err(io::Error::other)?;
    let mut receipts = Vec::new();
    for row in rows {
        let (key, value) = row.map_err(io::Error::other)?;
        let (sender, operation, guidance, seq) = key.value();
        if seq >= cut.domain {
            continue;
        }
        let reference: DomainReference = serde_json::from_slice(value.value())?;
        let event = read_domain(journal, reference, cut.bytes)?;
        if event.seq.get() != seq
            || !matches!(&event.kind,
            TimelineEventKind::Notification(NotificationEvent::Received { source, .. })
            if guidance == matches!(source, NotificationSource::PlanGuidance { .. })
                && source.agent_message().is_some_and(|(s, o, _, _)| (s, o) == (sender, operation)))
        {
            return Err(invalid(
                "agent message receipt differs from its journal identity",
            ));
        }
        receipts.push(event);
    }
    receipts.sort_unstable_by_key(|event| event.seq);
    Ok(receipts)
}

pub(super) fn payload_hashes(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
    candidates: Option<&[String]>,
    cancelled: &dyn Fn() -> bool,
) -> io::Result<std::collections::BTreeSet<String>> {
    let table = tx.open_table(PAYLOADS).map_err(io::Error::other)?;
    let mut hashes = std::collections::BTreeSet::new();
    let mut check = |hash: &str, value: &[u8]| -> io::Result<()> {
        if cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "notification payload lookup cancelled",
            ));
        }
        let reference = serde_json::from_slice(value)?;
        let Some(event) = read_before(journal, reference, cut, EventSeq::new(cut.domain))? else {
            return Ok(());
        };
        if !matches!(&event.kind, TimelineEventKind::Notification(NotificationEvent::Received { source, payload_ref, .. })
            if source.agent_message().is_some() && payload_ref.blake3 == hash)
        {
            return Err(invalid(
                "notification payload root differs from its journal identity",
            ));
        }
        hashes.insert(hash.to_owned());
        Ok(())
    };
    if let Some(candidates) = candidates {
        for hash in candidates {
            if let Some(value) = table.get(hash.as_str()).map_err(io::Error::other)? {
                check(hash, value.value())?;
            }
        }
    } else {
        for row in table.iter().map_err(io::Error::other)? {
            let (key, value) = row.map_err(io::Error::other)?;
            check(key.value(), value.value())?;
        }
    }
    Ok(hashes)
}

pub(super) fn agent_message_was_consumed(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
    id: &str,
    before: EventSeq,
) -> io::Result<bool> {
    let table = tx.open_table(CONSUMED_MESSAGES).map_err(io::Error::other)?;
    let end = cut.domain.min(before.get());
    let last = table
        .range((id, 0)..(id, end))
        .map_err(io::Error::other)?
        .next_back()
        .transpose()
        .map_err(io::Error::other)?;
    let Some((key, value)) = last else {
        return Ok(false);
    };
    let reference: DomainReference = serde_json::from_slice(value.value())?;
    if reference.seq.get() != key.value().1 {
        return Err(invalid(
            "AgentMessage consumption differs from its indexed position",
        ));
    }
    let event = read_domain(journal, reference, cut.bytes)?;
    if !matches!(event.kind, TimelineEventKind::Notification(NotificationEvent::Consumed {
        input: Some(sampling_types::ConversationItem::AgentMessage(batch)), ..
    }) if batch.messages.iter().any(|message| message.message_id == id))
    {
        return Err(invalid(
            "AgentMessage consumption differs from its journal identity",
        ));
    }
    Ok(true)
}

pub(super) fn settlement(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
    id: &str,
    before: EventSeq,
) -> io::Result<Option<Option<TurnId>>> {
    let table = tx.open_table(SETTLEMENTS).map_err(io::Error::other)?;
    let Some(row) = table.get(id).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let Some(event) = read_before(journal, serde_json::from_slice(row.value())?, cut, before)?
    else {
        return Ok(None);
    };
    match event.kind {
        TimelineEventKind::Notification(NotificationEvent::Consumed {
            notification_ids,
            turn,
            ..
        }) if notification_ids.iter().any(|value| value == id) => Ok(Some(Some(turn))),
        TimelineEventKind::Notification(NotificationEvent::Dismissed {
            notification_ids, ..
        }) if notification_ids.iter().any(|value| value == id) => Ok(Some(None)),
        _ => Err(invalid(
            "notification settlement differs from its journal identity",
        )),
    }
}

pub(super) fn completed_task(
    tx: &ReadTransaction,
    journal: &mut File,
    cut: Frontier,
    id: &str,
    monitor: bool,
    before: EventSeq,
) -> io::Result<bool> {
    let table = tx.open_table(TASKS).map_err(io::Error::other)?;
    let Some(row) = table.get((id, monitor)).map_err(io::Error::other)? else {
        return Ok(false);
    };
    let Some(event) = read_before(journal, serde_json::from_slice(row.value())?, cut, before)?
    else {
        return Ok(false);
    };
    if matches!(&event.kind, TimelineEventKind::Notification(NotificationEvent::Received {
        source: NotificationSource::TaskCompleted { task_id, task_kind, .. }, ..
    }) if task_id == id && (!monitor || *task_kind == NotificationTaskKind::Monitor))
    {
        Ok(true)
    } else {
        Err(invalid("task completion differs from its journal identity"))
    }
}

fn read_before(
    journal: &mut File,
    reference: DomainReference,
    cut: Frontier,
    before: EventSeq,
) -> io::Result<Option<TimelineEvent>> {
    if reference.seq.get() >= before.get().min(cut.domain) {
        return Ok(None);
    }
    read_domain(journal, reference, cut.bytes).map(Some)
}

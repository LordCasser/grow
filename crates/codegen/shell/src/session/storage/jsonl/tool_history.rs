//! Exact execution lifecycle and display lookup for a published journal cut.

use super::tool_fragments;
use super::working_index::{self, RecordRef};
use super::{JsonlStorageAdapter, LedgerFileStamp};
use super::{
    boundary_history, hook_history, input_history, notification_history, response_history,
    subagent_history, workflow_history,
};
use crate::session::history::{HistoryContent, JournalFact, JournalRecord};
use crate::session::storage::{ContainedDirectory, TIMELINE_FILE};
use acp_transport::protocol as acp;
use chat_state::actor::state::{UsageCheckpoint, usage_history_key};
use chat_state::session_runtime::{RuntimeLifecycle, RuntimeWaits};
use chat_state::timeline::{
    ActiveTurnProgress, ExecutionHistory, RequestHistoryState, SurfaceCheckpoint, ToolHistoryState,
};
use chat_state::{
    EventSeq, RequestEvent, StepEvent, StepId, TimelineEvent, TimelineEventKind, ToolEvent,
    TurnEvent, TurnId,
};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};
use std::{ffi::OsStr, fs::File, io, sync::Arc};
use uuid::Uuid;

const TOOL: TableDefinition<&str, &[u8]> = TableDefinition::new("tool");
const OPEN: TableDefinition<&str, &[u8]> = TableDefinition::new("open_tool");
const REQUEST: TableDefinition<&str, &[u8]> = TableDefinition::new("request");
const OPEN_REQUEST: TableDefinition<&str, &[u8]> = TableDefinition::new("open_request");
const OPEN_CUT: TableDefinition<u64, &[u8]> = TableDefinition::new("open_at_frontier");
const DISPLAY: TableDefinition<u64, &[u8]> = TableDefinition::new("display");
const DISPLAY_IDS: TableDefinition<&str, &[u8]> = TableDefinition::new("display_id");
const SUBAGENT_DISPLAY: TableDefinition<u64, &[u8]> = TableDefinition::new("subagent_display");
const TOOL_DISPLAY: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("tool_display");
const HEAD: TableDefinition<(), &[u8]> = TableDefinition::new("frontier");
const USAGE: TableDefinition<&str, &[u8]> = TableDefinition::new("usage_identity");
const USAGE_CUT: TableDefinition<u64, &[u8]> = TableDefinition::new("usage_at_frontier");
const SIDEBAND: TableDefinition<&str, ()> = TableDefinition::new("sideband_id");
const USER_IMAGE_PATHS: TableDefinition<&str, &[u8]> = TableDefinition::new("user_image_paths");
const INTERACTIONS: TableDefinition<(&str, &str), &[u8]> =
    TableDefinition::new("interaction_identity");
const OPEN_INTERACTIONS: TableDefinition<(&str, &str), &[u8]> =
    TableDefinition::new("interaction_open");
const EXECUTION_CUT: TableDefinition<u64, &[u8]> = TableDefinition::new("execution_at_frontier");
const CONTROL_RECEIPTS: TableDefinition<(&str, &str), &[u8]> =
    TableDefinition::new("control_receipts");
const CONTROL_UI: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("control_ui");
const CONTROL_UI_CUT: TableDefinition<u64, &[u8]> = TableDefinition::new("control_ui_at_frontier");
const OPEN_TASKS: TableDefinition<&str, &[u8]> = TableDefinition::new("open_background_tasks");
const TASK_CUT: TableDefinition<u64, &[u8]> = TableDefinition::new("background_tasks_at_frontier");
const TASK_TERMINALS: TableDefinition<(u64, u64), &[u8]> =
    TableDefinition::new("background_task_terminal_rows");
const TASK_FINISHES: TableDefinition<(&str, u64), ()> =
    TableDefinition::new("background_task_finished_rows");

#[derive(Serialize, Deserialize)]
struct ControlReceipt {
    token: (u64, u64),
    target: crate::extensions::notification::ControlTarget,
    source: DomainReference,
}

#[derive(Serialize, Deserialize)]
struct ControlPresentation {
    token: (u64, u64),
    target: crate::extensions::notification::ControlTarget,
    phase: crate::extensions::notification::ControlPhase,
    source: DisplayReference,
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct ExecutionCut {
    domain: u64,
    pub lifecycle: RuntimeLifecycle,
    pub waits: RuntimeWaits,
    pub surface: SurfaceCheckpoint,
    pub progress: ActiveTurnProgress,
    pub image_projection: Option<DomainReference>,
    planning: Option<DomainReference>,
    title: Option<DomainReference>,
    model: Option<DomainReference>,
    checkpoint_model: Option<DomainReference>,
    signals: Option<DomainReference>,
    announcement: Option<DomainReference>,
    control: Option<DomainReference>,
    control_receipts: Vec<DomainReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(super) struct Frontier {
    pub bytes: u64,
    pub domain: u64,
    pub latest_tool: Option<EventSeq>,
    #[serde(default)]
    pub latest_request: Option<EventSeq>,
    #[serde(default)]
    pub last_request: Option<(EventSeq, i64)>,
    pub active_turn_had_tool: bool,
    #[serde(default)]
    pub started_turns: u32,
    #[serde(default)]
    pub started_tools: u32,
    #[serde(default)]
    pub recap_covered_main_turns: Option<usize>,
    #[serde(default)]
    pub context_rebuild: Option<EventSeq>,
    pub rewind: Option<EventSeq>,
    pub display_branch: Option<u64>,
    #[serde(default)]
    pub child_seed: Option<EventSeq>,
    #[serde(alias = "notices")]
    pub display_rows: u64,
    #[serde(default)]
    pub inherited_text_rows: u64,
    #[serde(default)]
    pub fallback_notice_rows: u64,
    #[serde(default)]
    pub has_child_presentation: bool,
    pub event_counter_floor: u64,
    turn: Option<TurnId>,
    step: Option<StepId>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct DomainReference {
    pub seq: EventSeq,
    pub record: RecordRef,
}

impl Frontier {
    pub(super) fn history_branch(self) -> Option<EventSeq> {
        self.context_rebuild.max(self.rewind)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct DisplayReference {
    pub position: u64,
    pub record: RecordRef,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct TaskTerminalRow {
    pub position: u64,
    pub part: u64,
    pub start: DisplayReference,
    pub settlement: DomainReference,
    pub command: usize,
    pub complete: bool,
    pub task_id: String,
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct ToolDisplay {
    pub start: Option<DisplayReference>,
    // One canonical update reference per replaced field, regardless of how
    // long the tool streamed before the requested display position.
    fields: [Option<DisplayReference>; 6],
}

impl ToolDisplay {
    pub fn changes(&self) -> Vec<DisplayReference> {
        let mut changes = self.fields.iter().copied().flatten().collect::<Vec<_>>();
        changes.sort_unstable_by_key(|reference| reference.position);
        changes.dedup_by_key(|reference| reference.position);
        changes
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Entry {
    start: DomainReference,
    terminal: Option<DomainReference>,
}

#[derive(Clone)]
pub(super) struct Index {
    db: Arc<Database>,
    directory: Arc<ContainedDirectory>,
    journal: LedgerFileStamp,
    file: Arc<File>,
}

impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolHistory").finish_non_exhaustive()
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn is_subagent_display(content: &HistoryContent) -> bool {
    let HistoryContent::Grow(fact) = content else {
        return false;
    };
    match fact.as_ref() {
        crate::session::history::GrowHistoryFact::SubagentSpawned { .. }
        | crate::session::history::GrowHistoryFact::SubagentProgress { .. }
        | crate::session::history::GrowHistoryFact::SubagentFinished { .. } => true,
        _ => false,
    }
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    serde_json::from_slice(bytes).map_err(io::Error::other)
}

pub(super) fn name(generation: Uuid) -> String {
    format!("working-tools-{generation}.redb")
}

impl Index {
    pub(super) fn visible_range(
        &self,
        frontier: Frontier,
        anchor: u64,
    ) -> io::Result<std::ops::RangeInclusive<u64>> {
        let Some(publication) = frontier.display_branch else {
            return Ok(0..=anchor);
        };
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        super::branch_history::visible_range(&tx, publication, anchor)
    }

    pub(super) fn visible_range_after(
        &self,
        frontier: Frontier,
        anchor: u64,
        last: u64,
    ) -> io::Result<std::ops::RangeInclusive<u64>> {
        let Some(publication) = frontier.display_branch else {
            return Ok(anchor..=last);
        };
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        super::branch_history::visible_range_after(&tx, publication, anchor, last)
    }

    pub(super) fn pending_consultations(
        &self,
        frontier: Frontier,
    ) -> io::Result<Vec<TimelineEvent>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        super::consultation_history::pending(&tx, &mut self.journal()?, frontier)
    }

    pub(super) fn sideband_spawn(
        &self,
        frontier: Frontier,
        id: &str,
    ) -> io::Result<Option<TimelineEvent>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        super::compaction_history::sideband_spawn(&tx, &mut self.journal()?, id, frontier)
    }

    pub(super) fn open(
        directory: Arc<ContainedDirectory>,
        generation: Uuid,
        journal: LedgerFileStamp,
        create: bool,
    ) -> io::Result<Self> {
        let name = name(generation);
        let file = if create {
            directory.open_read_write_create(OsStr::new(&name))?
        } else {
            directory.open_read_write_existing(OsStr::new(&name), "Tool index")?
        };
        let mut builder = Database::builder();
        builder.set_cache_size(8 * 1024 * 1024);
        builder.set_repair_callback(|progress| {
            if progress.progress() == 0.0 {
                tracing::warn!("working index requires full allocator repair");
            }
        });
        let db = builder
            .create_file(file.try_clone()?)
            .map_err(io::Error::other)?;
        Ok(Self {
            db: Arc::new(db),
            directory,
            journal,
            file: Arc::new(file),
        })
    }

    /// Sync the journal first. The database may lead the manifest if the
    /// process stops between these two publications. Retrying the same cut
    /// reuses the committed transaction without changing its identity.
    pub(super) fn advance_checked(
        &self,
        file: &mut File,
        previous: Frontier,
        bytes: u64,
        domain: u64,
        session: &str,
        read_reference: impl Fn(EventSeq) -> io::Result<Option<RecordRef>>,
        check_cancelled: impl Fn() -> io::Result<()>,
    ) -> io::Result<Frontier> {
        let mut tx = self.db.begin_write().map_err(io::Error::other)?;
        tx.set_quick_repair(true);
        let mut frontier = {
            let head = tx.open_table(HEAD).map_err(io::Error::other)?;
            head.get(())
                .map_err(io::Error::other)?
                .map(|value| decode::<Frontier>(value.value()))
                .transpose()?
                .unwrap_or_default()
        };
        if frontier.bytes == bytes
            && frontier.domain == domain
            && tx
                .open_table(OPEN_CUT)
                .map_err(io::Error::other)?
                .get(bytes)
                .map_err(io::Error::other)?
                .is_some()
        {
            JsonlStorageAdapter::sync_file_durable(&self.file)?;
            return Ok(frontier);
        }
        tool_fragments::initialize(&tx)?;
        if frontier.bytes < previous.bytes
            || frontier.domain < previous.domain
            || frontier.bytes > bytes
        {
            return Err(invalid(
                "Tool index frontier differs from journal publication",
            ));
        }
        let mut journal = self.journal()?;
        let domain_journal = std::cell::RefCell::new(self.journal()?);
        let read_event = |seq| {
            let Some(record) = read_reference(seq)? else {
                return Ok(None);
            };
            read_domain(
                &mut domain_journal.borrow_mut(),
                DomainReference { seq, record },
                bytes,
            )
            .map(Some)
        };
        {
            let mut tools = tx.open_table(TOOL).map_err(io::Error::other)?;
            let mut open = tx.open_table(OPEN).map_err(io::Error::other)?;
            let mut requests = tx.open_table(REQUEST).map_err(io::Error::other)?;
            let mut open_requests = tx.open_table(OPEN_REQUEST).map_err(io::Error::other)?;
            let mut interactions = tx.open_table(INTERACTIONS).map_err(io::Error::other)?;
            let mut open_interactions =
                tx.open_table(OPEN_INTERACTIONS).map_err(io::Error::other)?;
            let mut hooks = hook_history::Writer::open(&tx, frontier.bytes)?;
            let mut inputs = input_history::Writer::open(&tx)?;
            let mut echoes = super::user_echo_history::Writer::open(&tx)?;
            let mut prompt_catalog = super::history_timeline::Writer::open(&tx)?;
            let mut consumed_echoes = Vec::new();
            let mut subagents = subagent_history::Writer::open(&tx)?;
            let mut notifications = notification_history::Writer::open(&tx)?;
            let mut outgoing_inquiries = super::outgoing_inquiry_history::Writer::open(&tx)?;
            let mut consultations = super::consultation_history::Writer::open(&tx)?;
            let mut responses = response_history::Writer::open(&tx)?;
            let mut boundaries = boundary_history::Writer::open(&tx, frontier.bytes)?;
            let mut workflows = workflow_history::Writer::open(&tx)?;
            let mut compaction = super::compaction_history::Writer::open(&tx, frontier.bytes)?;
            let mut compaction_terminals = Vec::new();
            let mut notices = tx.open_table(DISPLAY).map_err(io::Error::other)?;
            let mut subagent_display = tx.open_table(SUBAGENT_DISPLAY).map_err(io::Error::other)?;
            let mut notice_anchors = tx
                .open_table(notification_history::NOTICE_ANCHORS)
                .map_err(io::Error::other)?;
            let mut notice_sources = tx
                .open_table(notification_history::NOTICE_SOURCES)
                .map_err(io::Error::other)?;
            let mut notice_ids = tx.open_table(DISPLAY_IDS).map_err(io::Error::other)?;
            let mut tool_display = tx.open_table(TOOL_DISPLAY).map_err(io::Error::other)?;
            let mut background_tasks = tx.open_table(OPEN_TASKS).map_err(io::Error::other)?;
            let mut task_terminals = tx.open_table(TASK_TERMINALS).map_err(io::Error::other)?;
            let mut task_finishes = tx.open_table(TASK_FINISHES).map_err(io::Error::other)?;
            let mut usage_ids = tx.open_table(USAGE).map_err(io::Error::other)?;
            let mut usage_cuts = tx.open_table(USAGE_CUT).map_err(io::Error::other)?;
            let mut sidebands = tx.open_table(SIDEBAND).map_err(io::Error::other)?;
            let mut user_image_paths = tx.open_table(USER_IMAGE_PATHS).map_err(io::Error::other)?;
            let mut runtime_cuts = tx.open_table(EXECUTION_CUT).map_err(io::Error::other)?;
            let mut control_receipts = tx.open_table(CONTROL_RECEIPTS).map_err(io::Error::other)?;
            let mut control_ui = tx.open_table(CONTROL_UI).map_err(io::Error::other)?;
            let mut runtime = match runtime_cuts.get(frontier.bytes).map_err(io::Error::other)? {
                Some(row) => decode::<ExecutionCut>(row.value())?,
                None if frontier.bytes == 0 => ExecutionCut::default(),
                None => return Err(invalid("missing Runtime publication")),
            };
            if runtime.domain != frontier.domain {
                return Err(invalid("Runtime checkpoint differs from journal frontier"));
            }
            let prior_usage = usage_cuts
                .get(frontier.bytes)
                .map_err(io::Error::other)?
                .map(|value| decode::<UsageCheckpoint>(value.value()))
                .transpose()?;
            let mut usage = match prior_usage {
                Some(checkpoint)
                    if checkpoint.frontier() == Some(EventSeq::new(frontier.domain)) =>
                {
                    checkpoint
                }
                None if frontier.bytes == 0 => UsageCheckpoint::default(),
                _ => {
                    return Err(invalid(
                        "accounting checkpoint differs from journal frontier",
                    ));
                }
            };
            let mut usage_journal = self.journal()?;
            JsonlStorageAdapter::scan_journal_bytes(
                file,
                frontier.bytes..bytes,
                &self.directory.display_path().join(TIMELINE_FILE),
                &mut blake3::Hasher::new(),
                None,
                None,
                |record, offset, json| {
                    check_cancelled()?;
                    if let JournalFact::Presentation { event, .. } = &record.fact {
                        prompt_catalog.observe(record.seq, event, &mut journal, &read_reference)?;
                    }
                    responses.accept(&record, offset, json, frontier.history_branch())?;
                    let reference = RecordRef {
                        offset,
                        len: json.len() as u64,
                        digest: blake3::hash(json),
                    };
                    let class = super::working_snapshot::classify(&record);
                    if class == super::working_snapshot::RecordClass::BranchMarker {
                        let (selected, history) = super::branch_history::read_transformation(
                            &self.directory,
                            offset + json.len() as u64 + 1,
                            frontier.domain,
                            true,
                            &check_cancelled,
                        )?;
                        super::branch_history::publish_display(
                            &tx,
                            &self.directory,
                            session,
                            &selected,
                            history,
                            record.seq,
                        )?;
                        frontier.display_branch = Some(record.seq);
                    }
                    if let JournalFact::Presentation { event, .. } = &record.fact {
                        event.validate_projection_identity()?;
                        if let Some(id) = event.event_id() {
                            let display = DisplayReference {
                                position: record.seq,
                                record: reference,
                            };
                            if notice_ids
                                .insert(id, serde_json::to_vec(&display)?.as_slice())
                                .map_err(io::Error::other)?
                                .is_some()
                            {
                                return Err(invalid("duplicate indexed display identity"));
                            }
                            frontier.event_counter_floor = frontier
                                .event_counter_floor
                                .max(crate::session::history::event_counter_after(id).unwrap_or(0));
                        }
                    }
                    if let JournalFact::Presentation { at_ms, event } = &record.fact
                        && class == super::working_snapshot::RecordClass::InheritedText
                    {
                        responses.inherit(
                            record.seq,
                            *at_ms,
                            event,
                            &mut journal,
                            &read_reference,
                        )?;
                        frontier.inherited_text_rows += 1;
                    }
                    if let JournalFact::Presentation { event, .. } = &record.fact
                        && let HistoryContent::Notice { event: seq } = &event.content
                    {
                        let source = read_event(EventSeq::new(*seq))?
                            .filter(|event| *seq < frontier.domain && event.seq.get() == *seq)
                            .ok_or_else(|| invalid("missing communication notice source"))?;
                        let identity =
                            crate::session::storage::communication_history::receipt_identity(
                                &source,
                            )?
                            .ok_or_else(|| invalid("history notice names a different source"))?;
                        let original = notification_history::notice_source(
                            &notice_sources,
                            &mut journal,
                            bytes,
                            frontier.domain,
                            &identity,
                            false,
                        )?;
                        if original.is_none_or(|original| original.event != *seq) {
                            return Err(invalid(
                                "notice reference does not select its original receipt",
                            ));
                        }
                        let display = DisplayReference {
                            position: record.seq,
                            record: reference,
                        };
                        notice_anchors
                            .insert((*seq, record.seq), serde_json::to_vec(&display)?.as_slice())
                            .map_err(io::Error::other)?;
                    }
                    if matches!(
                        class,
                        super::working_snapshot::RecordClass::Display
                            | super::working_snapshot::RecordClass::UserEcho
                            | super::working_snapshot::RecordClass::ResponseCandidate
                            | super::working_snapshot::RecordClass::BackgroundTask
                            | super::working_snapshot::RecordClass::CommunicationNotice
                    ) {
                        let reference = DisplayReference {
                            position: record.seq,
                            record: reference,
                        };
                        let JournalFact::Presentation { at_ms, event } = record.fact else {
                            unreachable!()
                        };
                        let encoded = serde_json::to_vec(&reference)?;
                        if let Some(coverage) = event.recap_covered_main_turns() {
                            frontier.recap_covered_main_turns = Some(coverage);
                        }
                        if class == super::working_snapshot::RecordClass::BackgroundTask
                            && let HistoryContent::Grow(content) = &event.content
                        {
                            match content.as_ref() {
                                crate::session::history::GrowHistoryFact::TaskBackgrounded {
                                    task_id,
                                    ..
                                } => {
                                    background_tasks
                                        .insert(task_id.as_str(), encoded.as_slice())
                                        .map_err(io::Error::other)?;
                                }
                                crate::session::history::GrowHistoryFact::TaskCompleted {
                                    task_snapshot,
                                } => {
                                    task_finishes
                                        .insert((task_snapshot.task_id.as_str(), record.seq), ())
                                        .map_err(io::Error::other)?;
                                    background_tasks
                                        .remove(task_snapshot.task_id.as_str())
                                        .map_err(io::Error::other)?;
                                }
                                _ => unreachable!(),
                            }
                        }
                        if let HistoryContent::User(chunk) = &event.content
                            && let Some((id, _)) =
                                crate::session::history::user_echo_identity(chunk)
                        {
                            echoes.add_block(id, reference)?;
                        }
                        if let HistoryContent::Grow(content) = &event.content
                            && let crate::session::history::GrowHistoryFact::ControlStateUpdate(
                                update,
                            ) = content.as_ref()
                        {
                            use crate::extensions::notification::ControlPhase;
                            if let Some(intent) = &update.intent
                                && matches!(
                                    update.phase,
                                    ControlPhase::Applied
                                        | ControlPhase::Rejected
                                        | ControlPhase::Superseded
                                )
                            {
                                let target = update.desired.as_ref().unwrap_or(&update.current);
                                if target.domain() != update.domain {
                                    return Err(invalid(
                                        "persisted control terminal target does not match its domain",
                                    ));
                                }
                                let key =
                                    (control_domain_key(update.domain), intent.client_id.as_str());
                                let token = (intent.generation, intent.sequence);
                                let authoritative = control_receipts
                                    .get(key)
                                    .map_err(io::Error::other)?
                                    .map(|value| decode::<ControlReceipt>(value.value()))
                                    .transpose()?;
                                if authoritative.as_ref().is_some_and(|old| {
                                    old.token == token
                                        && (&old.target != target
                                            || update.phase != ControlPhase::Applied)
                                }) {
                                    return Err(invalid(
                                        "Control projection conflicts with its durable receipt",
                                    ));
                                }
                                let prior = control_ui
                                    .get(key)
                                    .map_err(io::Error::other)?
                                    .map(|value| decode::<ControlPresentation>(value.value()))
                                    .transpose()?;
                                if prior.as_ref().is_some_and(|old| {
                                    old.token == token
                                        && (&old.target != target || old.phase != update.phase)
                                }) {
                                    return Err(invalid("conflicting terminal Control projection"));
                                }
                                if prior.is_none_or(|old| old.token < token) {
                                    let row = ControlPresentation {
                                        token,
                                        target: target.clone(),
                                        phase: update.phase,
                                        source: reference,
                                    };
                                    control_ui
                                        .insert(key, serde_json::to_vec(&row)?.as_slice())
                                        .map_err(io::Error::other)?;
                                }
                            }
                        }
                        if let Some(call) = terminal_display_call(
                            &tool_display,
                            &mut journal,
                            &event.content,
                            reference,
                            |position| {
                                super::branch_history::visible_at_write(
                                    &tx,
                                    frontier.display_branch,
                                    position,
                                )
                            },
                        )? {
                            tool_fragments::publish(&tx, reference.position, at_ms, &call)?;
                        }
                        let visible = match &event.content {
                            HistoryContent::Candidate(_) => false,
                            HistoryContent::User(chunk) => {
                                !crate::session::history::provisional_user_echo(chunk)
                            }
                            content => index_tool_display(&mut tool_display, content, reference)?,
                        };
                        if visible {
                            notices
                                .insert(frontier.display_rows, encoded.as_slice())
                                .map_err(io::Error::other)?;
                            if is_subagent_display(&event.content) {
                                subagent_display
                                    .insert(record.seq, encoded.as_slice())
                                    .map_err(io::Error::other)?;
                                frontier.has_child_presentation = true;
                            }
                            frontier.display_rows += 1;
                        }
                        return Ok(true);
                    }
                    let JournalFact::Domain { event, .. } = record.fact else {
                        return Ok(true);
                    };
                    if event.seq.get() != frontier.domain {
                        return Err(invalid("Tool index domain frontier is not contiguous"));
                    }
                    crate::session::sampling_evidence::validate_reference(&event)?;
                    frontier.domain += 1;
                    let reference = DomainReference {
                        seq: event.seq,
                        record: reference,
                    };
                    if let Some(identity) =
                        crate::session::storage::communication_history::receipt_identity(&event)?
                    {
                        let original = notification_history::notice_source(
                            &notice_sources,
                            &mut journal,
                            bytes,
                            event.seq.get(),
                            &identity,
                            false,
                        )?;
                        let (id, subject, sender) = identity;
                        let subject = serde_json::to_string(&subject)?;
                        let sender = serde_json::to_string(&sender)?;
                        notice_sources
                            .insert(
                                (
                                    id.as_str(),
                                    subject.as_str(),
                                    sender.as_str(),
                                    reference.seq.get(),
                                ),
                                serde_json::to_vec(&reference)?.as_slice(),
                            )
                            .map_err(io::Error::other)?;
                        if original.is_none() {
                            let display = DisplayReference {
                                position: record.seq,
                                record: reference.record,
                            };
                            notices
                                .insert(
                                    frontier.display_rows,
                                    serde_json::to_vec(&display)?.as_slice(),
                                )
                                .map_err(io::Error::other)?;
                            frontier.display_rows += 1;
                            frontier.fallback_notice_rows += 1;
                        }
                    }
                    for path in chat_state::Timeline::user_image_paths(std::slice::from_ref(&event))
                    {
                        if user_image_paths
                            .get(path.as_str())
                            .map_err(io::Error::other)?
                            .is_none()
                        {
                            user_image_paths
                                .insert(path.as_str(), serde_json::to_vec(&reference)?.as_slice())
                                .map_err(io::Error::other)?;
                        }
                    }
                    if matches!(&event.kind, TimelineEventKind::SubagentSeed(_)) {
                        frontier.child_seed = Some(event.seq);
                    }
                    if event.messages().is_some_and(|message| {
                        message.cause == chat_state::MessageCause::ContextRebuild
                    }) {
                        frontier.context_rebuild = Some(event.seq);
                    }
                    match class {
                        super::working_snapshot::RecordClass::OutgoingInquiry => {
                            outgoing_inquiries.accept(&event, reference)?;
                        }
                        super::working_snapshot::RecordClass::Control => {
                            let snapshot = crate::session::control::SessionControlSnapshot::latest_from_timeline(
                                std::slice::from_ref(&event),
                            )?.expect("Control snapshot matches its class");
                            runtime.control = Some(reference);
                            if let Some(receipt) = snapshot.applied_control {
                                publish_control_receipt(
                                    &mut control_receipts,
                                    &control_ui,
                                    receipt,
                                    reference,
                                )?;
                            }
                        }
                        super::working_snapshot::RecordClass::Planning => {
                            runtime.planning = Some(reference);
                        }
                        super::working_snapshot::RecordClass::Title => {
                            runtime.title = Some(reference);
                        }
                        super::working_snapshot::RecordClass::Model => {
                            use crate::session::persistence::{
                                durable_model_control_receipts, latest_model_selection,
                            };

                            let previous = runtime
                                .model
                                .map(|previous| read_domain(&mut journal, previous, bytes))
                                .transpose()?;
                            let chain: Vec<_> = previous
                                .into_iter()
                                .chain(std::iter::once(event.clone()))
                                .collect();
                            latest_model_selection(&chain)?;
                            runtime.model = Some(reference);
                            for receipt in
                                durable_model_control_receipts(std::slice::from_ref(&event))?
                            {
                                publish_control_receipt(
                                    &mut control_receipts,
                                    &control_ui,
                                    receipt,
                                    reference,
                                )?;
                            }
                        }
                        super::working_snapshot::RecordClass::Signals => {
                            crate::session::signals::SessionSignals::latest_from_timeline(
                                std::slice::from_ref(&event),
                            )?;
                            runtime.signals = Some(reference);
                        }
                        super::working_snapshot::RecordClass::Announcement => {
                            crate::session::announcement_state::AnnouncementState::latest_from_timeline(std::slice::from_ref(&event))?;
                            runtime.announcement = Some(reference);
                        }
                        _ => {}
                    }
                    boundaries.accept(&event, reference, class)?;
                    workflows.accept(&event, reference)?;
                    if let Some(terminal) = compaction.accept(&event, reference)? {
                        compaction_terminals.push(terminal);
                    }
                    consultations.accept(
                        &event,
                        reference,
                        session,
                        |audit, result| {
                            super::consultation_history::validate_answer(
                                compaction.spawn_reference(&result.timeline_id)?,
                                &mut journal,
                                &self.directory,
                                reference,
                                session,
                                bytes,
                                audit,
                                result,
                                &read_event,
                            )
                        },
                        &read_event,
                    )?;
                    if let TimelineEventKind::ImageProjection(projection) = &event.kind {
                        let projected = super::branch_history::project_image(
                            &self.directory,
                            session,
                            reference,
                            projection,
                            &check_cancelled,
                        )?;
                        runtime.surface.accept_projected_image_surface(&projected);
                        runtime.image_projection = Some(reference);
                    } else if event
                        .messages()
                        .is_some_and(|message| message.cause == chat_state::MessageCause::Rewind)
                    {
                        let (rewound, history) = super::branch_history::read_transformation(
                            &self.directory,
                            offset + json.len() as u64 + 1,
                            frontier.domain,
                            true,
                            &check_cancelled,
                        )?;
                        boundaries.select_rewind(event.seq, &rewound)?;
                        responses.select_rewind(frontier.history_branch(), event.seq, &rewound)?;
                        runtime.surface.accept_rewound_surface(&rewound);
                        frontier.rewind = Some(event.seq);
                        super::branch_history::publish_display(
                            &tx,
                            &self.directory,
                            session,
                            &rewound,
                            history,
                            record.seq,
                        )?;
                        frontier.display_branch = Some(record.seq);
                    } else {
                        runtime
                            .surface
                            .accept(&event, frontier.turn.is_some(), frontier.step.is_some())
                            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    }
                    runtime.progress.accept(&event, frontier.turn);
                    runtime.waits.apply(&runtime.lifecycle, &event.kind);
                    if let TimelineEventKind::Runtime(transition) = &event.kind {
                        runtime.lifecycle = runtime
                            .lifecycle
                            .transition(
                                transition,
                                event.seq,
                                frontier.turn,
                                frontier.step,
                                !open_requests.is_empty().map_err(io::Error::other)?,
                            )
                            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                        if matches!(
                            transition,
                            chat_state::session_runtime::SessionRuntimeEvent::Checkpoint { .. }
                        ) {
                            runtime.checkpoint_model = runtime.model;
                        } else if runtime.lifecycle.checkpoint.is_none() {
                            runtime.checkpoint_model = None;
                        }
                    }
                    let mut settled_tasks = Vec::new();
                    for (command, settlement) in
                        crate::session::history::background_command_settlements(&event).enumerate()
                    {
                        let Some(start) = background_tasks
                            .get(settlement.command_id.as_str())
                            .map_err(io::Error::other)?
                            .map(|value| decode::<DisplayReference>(value.value()))
                            .transpose()?
                        else {
                            continue;
                        };
                        let JournalFact::Presentation {
                            event: presentation,
                            ..
                        } = working_index::read_record(&mut journal, start.record)?.fact
                        else {
                            return Err(invalid("background start must be a presentation fact"));
                        };
                        if !matches!(&presentation.content, HistoryContent::Grow(value)
                            if matches!(value.as_ref(), crate::session::history::GrowHistoryFact::TaskBackgrounded { tool_call_id, .. }
                                if tool_call_id == &settlement.call_id))
                        {
                            continue;
                        }
                        settled_tasks.push((start, command, settlement.command_id.clone()));
                        background_tasks
                            .remove(settlement.command_id.as_str())
                            .map_err(io::Error::other)?;
                    }
                    let parts = settled_tasks.len();
                    for (part, (start, command, task_id)) in settled_tasks.into_iter().enumerate() {
                        let row = TaskTerminalRow {
                            position: record.seq,
                            part: part as u64,
                            start,
                            settlement: reference,
                            command,
                            complete: part + 1 == parts,
                            task_id,
                        };
                        task_terminals
                            .insert((record.seq, row.part), serde_json::to_vec(&row)?.as_slice())
                            .map_err(io::Error::other)?;
                    }
                    if let TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                        display_echo: Some(echo),
                        ..
                    }) = &event.kind
                    {
                        consumed_echoes.push((echo.clone(), reference));
                    }
                    inputs.accept(&event.kind, reference)?;
                    if hooks.accept(&event, reference, frontier.turn)? {
                        let display = DisplayReference {
                            position: record.seq,
                            record: reference.record,
                        };
                        notices
                            .insert(
                                frontier.display_rows,
                                serde_json::to_vec(&display)?.as_slice(),
                            )
                            .map_err(io::Error::other)?;
                        frontier.display_rows += 1;
                    }
                    if let TimelineEventKind::Sideband(spawn) = &event.kind {
                        sidebands
                            .insert(spawn.sideband_id.as_str(), ())
                            .map_err(io::Error::other)?;
                    }
                    usage
                        .publish_fact(
                            &event,
                            |key| {
                                read_usage_fact(
                                    &usage_ids,
                                    &mut usage_journal,
                                    key,
                                    bytes,
                                    event.seq.get(),
                                )
                            },
                            |id| super::usage_history::get_write(&tx, bytes, id),
                            |id| Ok(sidebands.get(id).map_err(io::Error::other)?.is_some()),
                        )
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    for (id, segment) in usage.take_changed_segments() {
                        super::usage_history::publish(&tx, bytes, id, &segment)?;
                    }
                    subagents.accept(&event, reference, frontier.turn)?;
                    notifications
                        .accept(&event, reference, session, |id| subagents.delivered(id))?;
                    if frontier.turn.is_some()
                        && let Some(message) = event.messages()
                        && message.cause == chat_state::MessageCause::ToolResult
                        && matches!(message.surface, chat_state::SurfaceOp::Append)
                    {
                        for item in &message.items {
                            if let sampling_types::ConversationItem::ToolResult(result) = item
                                && let Some(id) = result.delivered_subagent_id.as_deref()
                            {
                                notifications.finish_child(id)?;
                            }
                        }
                    }
                    if let TimelineEventKind::Observation(observation) = &event.kind
                        && let Some(key) = usage_history_key(observation)
                        && usage_ids
                            .get(key.as_str())
                            .map_err(io::Error::other)?
                            .is_none()
                    {
                        usage_ids
                            .insert(key.as_str(), serde_json::to_vec(&reference)?.as_slice())
                            .map_err(io::Error::other)?;
                    }
                    match &event.kind {
                        TimelineEventKind::Turn(TurnEvent::Started { id, .. }) => {
                            frontier.started_turns = frontier.started_turns.saturating_add(1);
                            frontier.turn = Some(*id);
                            frontier.active_turn_had_tool = false;
                        }
                        TimelineEventKind::Turn(TurnEvent::Ended { .. }) => {
                            require_tools_closed(&open)?;
                            require_requests_closed(&open_requests)?;
                            frontier.turn = None;
                            frontier.active_turn_had_tool = false;
                        }
                        TimelineEventKind::Step(StepEvent::Started { id }) => {
                            frontier.step = Some(*id)
                        }
                        TimelineEventKind::Step(StepEvent::Ended { .. }) => {
                            require_tools_closed(&open)?;
                            require_requests_closed(&open_requests)?;
                            frontier.step = None;
                        }
                        TimelineEventKind::Interaction(chat_state::InteractionEvent::Opened {
                            tool_call_id,
                            attempt_id,
                            ..
                        }) => {
                            let key = (tool_call_id.as_str(), attempt_id.as_str());
                            let encoded = serde_json::to_vec(&reference)?;
                            if interactions
                                .insert(key, encoded.as_slice())
                                .map_err(io::Error::other)?
                                .is_some()
                            {
                                return Err(invalid("duplicate indexed interaction attempt"));
                            }
                            open_interactions
                                .insert(key, encoded.as_slice())
                                .map_err(io::Error::other)?;
                        }
                        TimelineEventKind::Interaction(
                            chat_state::InteractionEvent::Resolved {
                                tool_call_id,
                                attempt_id,
                                kind,
                                ..
                            },
                        ) => {
                            let key = (tool_call_id.as_str(), attempt_id.as_str());
                            let started = open_interactions
                                .remove(key)
                                .map_err(io::Error::other)?
                                .ok_or_else(|| {
                                    invalid("interaction resolution has no indexed start")
                                })?;
                            let started =
                                read_domain(&mut journal, decode(started.value())?, bytes)?;
                            if !matches!(&started.kind, TimelineEventKind::Interaction(chat_state::InteractionEvent::Opened { kind: original, .. }) if original == kind)
                            {
                                return Err(invalid(
                                    "indexed interaction resolution differs from its start",
                                ));
                            }
                        }
                        TimelineEventKind::Tool(tool) => {
                            if matches!(tool, ToolEvent::Started { .. }) {
                                frontier.started_tools = frontier.started_tools.saturating_add(1);
                            }
                            frontier.latest_tool = Some(event.seq);
                            frontier.active_turn_had_tool = true;
                            index_tool(
                                &mut tools,
                                &mut open,
                                &mut journal,
                                frontier,
                                tool,
                                reference,
                                bytes,
                            )?;
                        }
                        TimelineEventKind::Request(request) => {
                            if event.at_ms < 0 {
                                return Err(invalid(
                                    "invalid Request timestamp in indexed journal",
                                ));
                            }
                            index_request(
                                &mut requests,
                                &mut open_requests,
                                frontier,
                                request,
                                reference,
                            )?;
                            frontier.last_request = Some((event.seq, event.at_ms));
                            if matches!(request, RequestEvent::Started { .. }) {
                                frontier.latest_request = Some(event.seq);
                            }
                        }
                        _ => {}
                    }
                    Ok(true)
                },
            )?;
            // Bulk copy writes domain records before the presentation facts.
            for (echo, reference) in consumed_echoes {
                let revealed = echoes.consume(&mut journal, &echo, reference, bytes)?;
                prompt_catalog.reveal(&revealed)?;
            }
            let mut cuts = tx.open_table(OPEN_CUT).map_err(io::Error::other)?;
            cuts.retain(|key, _| key == previous.bytes)
                .map_err(io::Error::other)?;
            let mut open_at_cut = open_references(&open)?;
            let task_refs = background_tasks
                .iter()
                .map_err(io::Error::other)?
                .map(|row| {
                    row.map_err(io::Error::other)
                        .and_then(|(_, value)| decode::<DisplayReference>(value.value()))
                })
                .collect::<io::Result<Vec<_>>>()?;
            let mut task_cuts = tx.open_table(TASK_CUT).map_err(io::Error::other)?;
            task_cuts
                .retain(|key, _| key == previous.bytes)
                .map_err(io::Error::other)?;
            task_cuts
                .insert(bytes, serde_json::to_vec(&task_refs)?.as_slice())
                .map_err(io::Error::other)?;
            open_at_cut.extend(open_references(&open_requests)?);
            for row in open_interactions.iter().map_err(io::Error::other)? {
                let (_, value) = row.map_err(io::Error::other)?;
                open_at_cut.push(decode(value.value())?);
            }
            hooks.publish(
                previous.bytes,
                bytes,
                frontier.turn,
                |id| Ok(open.get(id).map_err(io::Error::other)?.is_some()),
                |id| inputs.contains_live(id),
            )?;
            inputs.publish(previous.bytes, bytes)?;
            subagents.publish(previous.bytes, bytes)?;
            notifications.publish(previous.bytes, bytes)?;
            consultations.publish(previous.bytes, bytes)?;
            boundaries.publish(previous.bytes, bytes)?;
            workflows.publish(previous.bytes, bytes)?;
            compaction.publish(
                &self.directory,
                &mut journal,
                session,
                previous.bytes,
                bytes,
                compaction_terminals,
                &read_event,
            )?;
            runtime.control_receipts.clear();
            for row in control_receipts.iter().map_err(io::Error::other)? {
                let (_, value) = row.map_err(io::Error::other)?;
                runtime
                    .control_receipts
                    .push(decode::<ControlReceipt>(value.value())?.source);
            }
            let mut presentations = Vec::new();
            for row in control_ui.iter().map_err(io::Error::other)? {
                let (_, value) = row.map_err(io::Error::other)?;
                presentations.push(decode::<ControlPresentation>(value.value())?.source);
            }
            presentations.sort_unstable_by_key(|reference| reference.position);
            let mut control_ui_cut = tx.open_table(CONTROL_UI_CUT).map_err(io::Error::other)?;
            control_ui_cut
                .retain(|key, _| key == previous.bytes)
                .map_err(io::Error::other)?;
            control_ui_cut
                .insert(bytes, serde_json::to_vec(&presentations)?.as_slice())
                .map_err(io::Error::other)?;
            runtime.domain = domain;
            runtime_cuts
                .retain(|key, _| key == previous.bytes)
                .map_err(io::Error::other)?;
            runtime_cuts
                .insert(bytes, serde_json::to_vec(&runtime)?.as_slice())
                .map_err(io::Error::other)?;
            cuts.insert(bytes, serde_json::to_vec(&open_at_cut)?.as_slice())
                .map_err(io::Error::other)?;
            usage.set_frontier(EventSeq::new(domain));
            usage_cuts
                .retain(|key, _| key == previous.bytes)
                .map_err(io::Error::other)?;
            usage_cuts
                .insert(bytes, serde_json::to_vec(&usage)?.as_slice())
                .map_err(io::Error::other)?;
            prompt_catalog.finish(&tx, frontier.display_branch, bytes)?;
        }
        if frontier.domain != domain {
            return Err(invalid("Tool index did not cover the domain frontier"));
        }
        frontier.bytes = bytes;
        tx.open_table(HEAD)
            .map_err(io::Error::other)?
            .insert((), serde_json::to_vec(&frontier)?.as_slice())
            .map_err(io::Error::other)?;
        tx.commit().map_err(io::Error::other)?;
        JsonlStorageAdapter::sync_file_durable(&self.file)?;
        Ok(frontier)
    }

    pub(super) fn control_presentations(
        &self,
        frontier: Frontier,
    ) -> io::Result<Vec<DisplayReference>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let table = tx.open_table(CONTROL_UI_CUT).map_err(io::Error::other)?;
        let row = table
            .get(frontier.bytes)
            .map_err(io::Error::other)?
            .ok_or_else(|| invalid("missing published Control presentation receipts"))?;
        decode(row.value())
    }

    pub(super) fn background_task_starts(
        &self,
        frontier: Frontier,
    ) -> io::Result<Vec<DisplayReference>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let cuts = tx.open_table(TASK_CUT).map_err(io::Error::other)?;
        let row = cuts
            .get(frontier.bytes)
            .map_err(io::Error::other)?
            .ok_or_else(|| invalid("missing background task checkpoint"))?;
        decode(row.value())
    }

    pub(super) fn task_terminals_before(
        &self,
        before: (u64, u64),
        limit: usize,
        cut: u64,
    ) -> io::Result<Vec<(TaskTerminalRow, bool)>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let rows = tx.open_table(TASK_TERMINALS).map_err(io::Error::other)?;
        let finishes = tx.open_table(TASK_FINISHES).map_err(io::Error::other)?;
        let mut result = Vec::with_capacity(limit);
        for entry in rows
            .range(..before)
            .map_err(io::Error::other)?
            .rev()
            .take(limit)
        {
            let (_, value) = entry.map_err(io::Error::other)?;
            let row: TaskTerminalRow = decode(value.value())?;
            let superseded = finishes
                .range((row.task_id.as_str(), row.position)..(row.task_id.as_str(), cut))
                .map_err(io::Error::other)?
                .next()
                .transpose()
                .map_err(io::Error::other)?
                .is_some();
            result.push((row, superseded));
        }
        Ok(result)
    }

    pub(super) fn task_terminals_after(
        &self,
        after: (u64, u64),
        limit: usize,
        cut: u64,
    ) -> io::Result<Vec<(TaskTerminalRow, bool)>> {
        use std::ops::Bound::Included;
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let rows = tx.open_table(TASK_TERMINALS).map_err(io::Error::other)?;
        let finishes = tx.open_table(TASK_FINISHES).map_err(io::Error::other)?;
        let mut result = Vec::with_capacity(limit);
        for entry in rows
            .range((Included(after), Included((cut, 0))))
            .map_err(io::Error::other)?
            .take(limit)
        {
            let (_, value) = entry.map_err(io::Error::other)?;
            let row: TaskTerminalRow = decode(value.value())?;
            if row.position >= cut {
                break;
            }
            let superseded = finishes
                .range((row.task_id.as_str(), row.position)..(row.task_id.as_str(), cut))
                .map_err(io::Error::other)?
                .next()
                .transpose()
                .map_err(io::Error::other)?
                .is_some();
            result.push((row, superseded));
        }
        Ok(result)
    }

    pub(super) fn display_at(&self, ordinal: u64) -> io::Result<DisplayReference> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let notices = tx.open_table(DISPLAY).map_err(io::Error::other)?;
        decode(
            notices
                .get(ordinal)
                .map_err(io::Error::other)?
                .ok_or_else(|| invalid("missing indexed notice position"))?
                .value(),
        )
    }

    pub(super) fn subagent_display_before(&self, before: u64) -> io::Result<Vec<DisplayReference>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let rows = tx.open_table(SUBAGENT_DISPLAY).map_err(io::Error::other)?;
        rows.range(..before)
            .map_err(io::Error::other)?
            .map(|entry| {
                let (_, value) = entry.map_err(io::Error::other)?;
                decode(value.value())
            })
            .collect()
    }

    pub(super) fn notice_source(
        &self,
        frontier: Frontier,
        identity: &(String, Option<String>, Option<String>),
        reject_conflict: bool,
    ) -> io::Result<Option<crate::session::storage::communication_history::ReceiptSource>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let table = tx
            .open_table(notification_history::NOTICE_SOURCES)
            .map_err(io::Error::other)?;
        notification_history::notice_source(
            &table,
            &mut self.journal()?,
            frontier.bytes,
            frontier.domain,
            identity,
            reject_conflict,
        )
    }

    pub(super) fn has_notice_anchor(
        &self,
        frontier: Frontier,
        journal: &mut File,
        seq: u64,
        records: u64,
    ) -> io::Result<bool> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        notification_history::has_notice_anchor(&tx, journal, frontier, seq, records)
    }

    pub(super) fn user_echo_rows_before(
        &self,
        before: u64,
        frontier: Frontier,
        limit: usize,
    ) -> io::Result<Vec<super::user_echo_history::Row>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        super::user_echo_history::rows_before(&tx, before, frontier, limit)
    }

    pub(super) fn user_echo_rows_after(
        &self,
        after: u64,
        frontier: Frontier,
        limit: usize,
    ) -> io::Result<Vec<super::user_echo_history::Row>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        super::user_echo_history::rows_after(&tx, after, frontier, limit)
    }

    pub(super) fn timeline_page(
        &self,
        bytes: u64,
        query: crate::session::history::TimelineQuery,
        limit: usize,
    ) -> io::Result<crate::session::history::HistoryTimelinePage> {
        super::history_timeline::read_page(&self.db, bytes, query, limit)
    }

    /// Tests prove a consecutive consume extends the published generation
    /// instead of rebuilding ranks.
    #[cfg(test)]
    pub(crate) fn timeline_generation(&self, bytes: u64) -> io::Result<u64> {
        super::history_timeline::read_generation(&self.db, bytes)
    }

    pub(super) fn timeline_base_count(&self, bytes: u64) -> io::Result<Option<u64>> {
        super::history_timeline::read_count(&self.db, bytes)
    }

    pub(super) fn timeline_rank_bound(
        &self,
        bytes: u64,
        source: crate::session::history::HistorySourceKey,
        display_segment: Option<u32>,
    ) -> io::Result<super::history_timeline::RankBound> {
        super::history_timeline::rank_lower_bound(&self.db, bytes, source, display_segment)
    }

    pub(super) fn timeline_message_seen(&self, bytes: u64, message_id: &str) -> io::Result<bool> {
        super::history_timeline::message_seen(&self.db, bytes, message_id)
    }

    pub(super) fn timeline_rank_window(
        &self,
        bytes: u64,
        start: u64,
        end: u64,
    ) -> io::Result<Vec<crate::session::history::HistoryTimelineEntry>> {
        super::history_timeline::read_rank_window(&self.db, bytes, start, end)
    }

    pub(super) fn response_rows_after(
        &self,
        after: (u64, u64),
        frontier: Frontier,
        records: u64,
        limit: usize,
    ) -> io::Result<Vec<response_history::Row>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        response_history::rows_after(&tx, after, frontier, records, limit)
    }

    pub(super) fn response_fragment_after(
        &self,
        journal: &mut File,
        row: &response_history::Row,
        start: u64,
        frontier: Frontier,
        limit: u64,
    ) -> io::Result<(u64, String)> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        response_history::read_row_after(&tx, journal, row, start, frontier.bytes, limit)
    }

    pub(super) fn tool_fragment_after(
        &self,
        position: u64,
        start: u64,
    ) -> io::Result<
        Option<(
            Option<u64>,
            i64,
            crate::extensions::notification::ToolOutputFragment,
        )>,
    > {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        tool_fragments::read_after(&tx, position, start)
    }

    pub(super) fn user_echo_blocks(
        &self,
        id: &str,
        records: u64,
        bytes: u64,
    ) -> io::Result<Vec<JournalRecord>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        super::user_echo_history::blocks_before(&tx, &mut self.journal()?, id, records, bytes)
    }

    pub(super) fn response_candidate(
        &self,
        key: &crate::session::history::ResponseKey,
        records: u64,
    ) -> io::Result<Option<DisplayReference>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        response_history::candidate_before(&tx, key, records)
    }

    pub(super) fn response_rows_before(
        &self,
        before: (u64, u64),
        frontier: Frontier,
        records: u64,
        limit: usize,
    ) -> io::Result<Vec<response_history::Row>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        response_history::rows_before(&tx, before, frontier, records, limit)
    }

    pub(super) fn response_fragment(
        &self,
        journal: &mut File,
        row: &response_history::Row,
        end: u64,
        frontier: Frontier,
        limit: u64,
    ) -> io::Result<(u64, String)> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        response_history::read_row(&tx, journal, row, end, frontier.bytes, limit)
    }

    pub(super) fn display_by_id(&self, id: &str) -> io::Result<Option<DisplayReference>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let notices = tx.open_table(DISPLAY_IDS).map_err(io::Error::other)?;
        notices
            .get(id)
            .map_err(io::Error::other)?
            .map(|value| decode(value.value()))
            .transpose()
    }

    pub(super) fn tool_display_before(&self, id: &str, position: u64) -> io::Result<ToolDisplay> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let table = tx.open_table(TOOL_DISPLAY).map_err(io::Error::other)?;
        tool_display_before(&table, id, position)
    }

    pub(super) fn tool_fragment(
        &self,
        position: u64,
        end_offset: Option<u64>,
    ) -> io::Result<
        Option<(
            Option<u64>,
            i64,
            crate::extensions::notification::ToolOutputFragment,
        )>,
    > {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        tool_fragments::read(&tx, position, end_offset)
    }

    pub(super) fn display_rank(&self, position: u64, count: u64) -> io::Result<usize> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let notices = tx.open_table(DISPLAY).map_err(io::Error::other)?;
        let (mut low, mut high) = (0, count);
        while low < high {
            let mid = low + (high - low) / 2;
            let reference: DisplayReference = decode(
                notices
                    .get(mid)
                    .map_err(io::Error::other)?
                    .ok_or_else(|| invalid("missing indexed notice position"))?
                    .value(),
            )?;
            if reference.position < position {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        Ok(low as usize)
    }

    pub(super) fn stamp(&self) -> io::Result<LedgerFileStamp> {
        Ok(LedgerFileStamp::from_metadata(&self.file.metadata()?))
    }

    pub(super) fn open_execution(&self, frontier: Frontier) -> io::Result<Vec<TimelineEvent>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let cuts = tx.open_table(OPEN_CUT).map_err(io::Error::other)?;
        let refs: Vec<DomainReference> = decode(
            cuts.get(frontier.bytes)
                .map_err(io::Error::other)?
                .ok_or_else(|| invalid("missing open execution checkpoint"))?
                .value(),
        )?;
        let mut journal = self.journal()?;
        refs.into_iter()
            .map(|reference| {
                let event = read_domain(&mut journal, reference, frontier.bytes)?;
                if !matches!(
                    &event.kind,
                    TimelineEventKind::Tool(ToolEvent::Started { .. })
                        | TimelineEventKind::Request(RequestEvent::Started { .. })
                        | TimelineEventKind::Interaction(
                            chat_state::InteractionEvent::Opened { .. }
                        )
                ) {
                    return Err(invalid("open execution reference does not name a start"));
                }
                Ok(event)
            })
            .collect()
    }

    pub(super) fn boundaries(&self, frontier: Frontier) -> io::Result<boundary_history::Restored> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        boundary_history::restore(&tx, &mut self.journal()?, frontier.bytes, frontier.domain)
    }

    pub(super) fn open_workflows(
        &self,
        frontier: Frontier,
    ) -> io::Result<Vec<(TimelineEvent, TimelineEvent)>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        workflow_history::open_workflows(&tx, &mut self.journal()?, frontier)
    }

    pub(super) fn compaction(
        &self,
        frontier: Frontier,
    ) -> io::Result<super::compaction_history::Restored> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        super::compaction_history::restore(&tx, &mut self.journal()?, frontier)
    }

    pub(super) fn execution_checkpoint(&self, frontier: Frontier) -> io::Result<ExecutionCut> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let cuts = tx.open_table(EXECUTION_CUT).map_err(io::Error::other)?;
        let row = cuts
            .get(frontier.bytes)
            .map_err(io::Error::other)?
            .ok_or_else(|| invalid("missing published Runtime checkpoint"))?;
        let runtime: ExecutionCut = decode(row.value())?;
        if runtime.domain != frontier.domain {
            return Err(invalid("Runtime checkpoint differs from journal frontier"));
        }
        Ok(runtime)
    }

    pub(super) fn recovery_snapshots(
        &self,
        execution: &ExecutionCut,
        frontier: Frontier,
    ) -> io::Result<Vec<TimelineEvent>> {
        use crate::session::{announcement_state, signals};

        let mut journal = self.journal()?;
        let mut control = execution.control_receipts.clone();
        control.extend(execution.control);
        control.extend(execution.model);
        control.extend(execution.checkpoint_model);
        control.sort_unstable_by_key(|reference| reference.seq);
        control.dedup_by_key(|reference| reference.seq);
        use super::working_snapshot::RecordClass;

        let mut events = [
            (execution.planning, RecordClass::Planning),
            (execution.title, RecordClass::Title),
            (execution.signals, RecordClass::Signals),
            (execution.announcement, RecordClass::Announcement),
        ]
        .into_iter()
        .filter_map(|(reference, class)| reference.map(|reference| (reference, class)))
        .map(|(reference, class)| {
            if reference.seq.get() >= frontier.domain {
                return Err(invalid("recovery projection crosses its domain frontier"));
            }
            let event = read_domain(&mut journal, reference, frontier.bytes)?;
            let valid = match (&event.kind, class) {
                (TimelineEventKind::Planning(_), RecordClass::Planning)
                | (TimelineEventKind::SessionTitle(_), RecordClass::Title) => true,
                (TimelineEventKind::Observation(event), RecordClass::Signals) => {
                    event.scope == signals::TIMELINE_SIGNALS_SCOPE
                        && event.name == signals::TIMELINE_SIGNALS_NAME
                }
                (TimelineEventKind::Observation(event), RecordClass::Announcement) => {
                    event.scope == announcement_state::TIMELINE_ANNOUNCEMENT_SCOPE
                        && event.name == announcement_state::TIMELINE_ANNOUNCEMENT_NAME
                }
                _ => false,
            };
            if !valid {
                return Err(invalid("recovery projection differs from the journal"));
            }
            Ok(event)
        })
        .collect::<io::Result<Vec<_>>>()?;
        for reference in control {
            if reference.seq.get() >= frontier.domain {
                return Err(invalid("Control projection crosses its domain frontier"));
            }
            let event = read_domain(&mut journal, reference, frontier.bytes)?;
            if !matches!(&event.kind, TimelineEventKind::Control(_))
                && !crate::session::persistence::is_model_change(&event)
            {
                return Err(invalid("Control projection differs from the journal"));
            }
            events.push(event);
        }
        Ok(events)
    }

    pub(super) fn open_subagents(&self, frontier: Frontier) -> io::Result<Vec<TimelineEvent>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let checkpoint = subagent_history::checkpoint(&tx, frontier.bytes)?;
        subagent_history::read_open_events(
            &mut self.journal()?,
            &checkpoint,
            frontier.bytes,
            frontier.domain,
        )
    }

    pub(super) fn pending_notifications(
        &self,
        frontier: Frontier,
    ) -> io::Result<Vec<TimelineEvent>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        notification_history::pending(&tx, &mut self.journal()?, frontier)
    }

    pub(super) fn subagent_lifecycle_events(
        &self,
        frontier: Frontier,
    ) -> io::Result<Vec<TimelineEvent>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        subagent_history::all_events_before(
            &tx,
            &mut self.journal()?,
            frontier.bytes,
            frontier.domain,
        )
    }

    pub(super) fn open_hooks(
        &self,
        frontier: Frontier,
    ) -> io::Result<(hook_history::Checkpoint, Vec<TimelineEvent>)> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let checkpoint = hook_history::checkpoint(&tx, frontier.bytes)?;
        let mut journal = self.journal()?;
        let events = checkpoint
            .references
            .iter()
            .map(|reference| {
                let event = read_domain(&mut journal, *reference, frontier.bytes)?;
                if reference.seq.get() >= frontier.domain
                    || !matches!(event.kind, TimelineEventKind::Hook(_))
                {
                    return Err(invalid("invalid open Hook reference"));
                }
                Ok(event)
            })
            .collect::<io::Result<_>>()?;
        Ok((checkpoint, events))
    }

    pub(super) fn open_inputs(
        &self,
        frontier: Frontier,
    ) -> io::Result<Vec<chat_state::InputCheckpointEntry>> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        input_history::open_inputs(
            &tx,
            &mut self.journal()?,
            frontier.bytes,
            EventSeq::new(frontier.domain),
        )
    }

    pub(super) fn cut(&self, history: super::working_snapshot::IndexedHistory) -> io::Result<Cut> {
        let frontier = history.execution_frontier();
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let head = tx.open_table(HEAD).map_err(io::Error::other)?;
        let published: Frontier = decode(
            head.get(())
                .map_err(io::Error::other)?
                .ok_or_else(|| invalid("missing Tool index frontier"))?
                .value(),
        )?;
        if published.bytes < frontier.bytes
            || published.domain < frontier.domain
            || (published.bytes == frontier.bytes && published != frontier)
        {
            return Err(invalid("Tool index does not cover the published cut"));
        }
        Ok(Cut {
            index: self.clone(),
            frontier,
            history,
        })
    }

    pub(super) fn usage_checkpoint(&self, frontier: Frontier) -> io::Result<UsageCheckpoint> {
        let tx = self.db.begin_read().map_err(io::Error::other)?;
        let cuts = tx.open_table(USAGE_CUT).map_err(io::Error::other)?;
        let value = cuts
            .get(frontier.bytes)
            .map_err(io::Error::other)?
            .ok_or_else(|| invalid("missing accounting checkpoint"))?;
        let checkpoint: UsageCheckpoint = decode(value.value())?;
        if checkpoint.frontier() != Some(EventSeq::new(frontier.domain)) {
            return Err(invalid(
                "accounting checkpoint differs from published frontier",
            ));
        }
        Ok(checkpoint)
    }

    fn journal(&self) -> io::Result<File> {
        let file = self
            .directory
            .open_regular(OsStr::new(TIMELINE_FILE), "Timeline")?;
        if !self
            .journal
            .same_file(&LedgerFileStamp::from_metadata(&file.metadata()?))
        {
            return Err(invalid("indexed Tool journal was replaced"));
        }
        Ok(file)
    }
}

fn publish_control_receipt(
    receipts: &mut redb::Table<'_, (&str, &str), &[u8]>,
    ui: &redb::Table<'_, (&str, &str), &[u8]>,
    receipt: crate::session::control::DurableControlReceipt,
    source: DomainReference,
) -> io::Result<()> {
    let key = (
        control_domain_key(receipt.domain),
        receipt.intent.client_id.as_str(),
    );
    let token = (receipt.intent.generation, receipt.intent.sequence);
    let presentation = ui
        .get(key)
        .map_err(io::Error::other)?
        .map(|value| decode::<ControlPresentation>(value.value()))
        .transpose()?;
    if presentation.as_ref().is_some_and(|old| {
        old.token == token
            && (old.target != receipt.target
                || old.phase != crate::extensions::notification::ControlPhase::Applied)
    }) {
        return Err(invalid(
            "Control projection conflicts with its durable receipt",
        ));
    }
    let prior = receipts
        .get(key)
        .map_err(io::Error::other)?
        .map(|value| decode::<ControlReceipt>(value.value()))
        .transpose()?;
    if prior
        .as_ref()
        .is_some_and(|old| old.token == token && old.target != receipt.target)
    {
        return Err(invalid("conflicting terminal Control intent"));
    }
    if prior.is_none_or(|old| old.token < token) {
        let row = ControlReceipt {
            token,
            target: receipt.target,
            source,
        };
        receipts
            .insert(key, serde_json::to_vec(&row)?.as_slice())
            .map_err(io::Error::other)?;
    }
    Ok(())
}

fn control_domain_key(domain: crate::extensions::notification::ControlDomain) -> &'static str {
    use crate::extensions::notification::ControlDomain;
    match domain {
        ControlDomain::Agent => "agent",
        ControlDomain::Behavior => "behavior",
        ControlDomain::Sampling => "sampling",
    }
}

fn require_tools_closed(open: &redb::Table<'_, &str, &[u8]>) -> io::Result<()> {
    if open.is_empty().map_err(io::Error::other)? {
        Ok(())
    } else {
        Err(invalid("execution boundary ended with open Tools"))
    }
}

fn require_requests_closed(open: &redb::Table<'_, &str, &[u8]>) -> io::Result<()> {
    if !open.is_empty().map_err(io::Error::other)? {
        return Err(invalid("execution boundary ended with open Requests"));
    }
    Ok(())
}

fn open_references(open: &redb::Table<'_, &str, &[u8]>) -> io::Result<Vec<DomainReference>> {
    open.iter()
        .map_err(io::Error::other)?
        .map(|row| {
            let (_, value) = row.map_err(io::Error::other)?;
            decode(value.value())
        })
        .collect()
}

fn tool_display_before(
    table: &impl ReadableTable<(&'static str, u64), &'static [u8]>,
    id: &str,
    position: u64,
) -> io::Result<ToolDisplay> {
    let mut range = table
        .range((id, 0)..(id, position))
        .map_err(io::Error::other)?;
    let Some(entry) = range.next_back() else {
        return Ok(ToolDisplay::default());
    };
    decode(entry.map_err(io::Error::other)?.1.value())
}

fn terminal_display_call(
    table: &impl ReadableTable<(&'static str, u64), &'static [u8]>,
    journal: &mut File,
    content: &HistoryContent,
    reference: DisplayReference,
    visible: impl Fn(u64) -> io::Result<bool>,
) -> io::Result<Option<acp::ToolCall>> {
    match content {
        HistoryContent::ToolCall(call)
            if matches!(
                call.status,
                acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed
            ) =>
        {
            Ok(Some(call.clone()))
        }
        HistoryContent::ToolUpdate(update)
            if matches!(
                update.fields.status,
                Some(acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed)
            ) =>
        {
            let id = update.tool_call_id.0.as_ref();
            let state = tool_display_before(table, id, reference.position)?;
            let Some(start) = state.start else {
                return Ok(None);
            };
            if !visible(start.position)? {
                return Ok(None);
            }
            let mut call = read_tool_start(journal, start)?;
            if call.tool_call_id.0.as_ref() != id {
                return Err(invalid("Tool display start refers to a different call"));
            }
            for change in state.changes() {
                if change.position <= start.position || change.position >= reference.position {
                    return Err(invalid("Tool display field crosses its terminal row"));
                }
                if !visible(change.position)? {
                    continue;
                }
                let HistoryContent::ToolUpdate(update) = read_display_content(journal, change)?
                else {
                    return Err(invalid("Tool display field refers to a different record"));
                };
                if update.tool_call_id.0.as_ref() != id {
                    return Err(invalid("Tool display field refers to a different call"));
                }
                call.update(update.fields);
            }
            call.update(update.fields.clone());
            Ok(Some(call))
        }
        _ => Ok(None),
    }
}

fn read_tool_start(journal: &mut File, reference: DisplayReference) -> io::Result<acp::ToolCall> {
    let HistoryContent::ToolCall(call) = read_display_content(journal, reference)? else {
        return Err(invalid("Tool display start refers to a different record"));
    };
    Ok(call)
}

fn read_display_content(
    journal: &mut File,
    reference: DisplayReference,
) -> io::Result<HistoryContent> {
    let record = working_index::read_record(journal, reference.record)?;
    if record.seq != reference.position {
        return Err(invalid(
            "Tool display reference differs from its journal position",
        ));
    }
    let JournalFact::Presentation { event, .. } = record.fact else {
        return Err(invalid(
            "Tool display reference is not a presentation record",
        ));
    };
    Ok(event.content)
}

fn index_tool_display(
    table: &mut redb::Table<'_, (&str, u64), &[u8]>,
    content: &HistoryContent,
    reference: DisplayReference,
) -> io::Result<bool> {
    let (id, state, terminal) = match content {
        HistoryContent::ToolCall(call) => {
            let terminal = matches!(
                call.status,
                acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed
            );
            let state = if terminal {
                ToolDisplay::default()
            } else {
                ToolDisplay {
                    start: Some(reference),
                    ..ToolDisplay::default()
                }
            };
            (call.tool_call_id.0.as_ref(), state, terminal)
        }
        HistoryContent::ToolUpdate(update) => {
            let id = update.tool_call_id.0.as_ref();
            match update.fields.status {
                Some(acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed) => {
                    (id, ToolDisplay::default(), true)
                }
                _ => {
                    let mut state = tool_display_before(table, id, reference.position)?;
                    if state.start.is_none() {
                        return Ok(false);
                    }
                    let present = [
                        update.fields.kind.is_some(),
                        update.fields.title.is_some(),
                        update.fields.content.is_some(),
                        update.fields.locations.is_some(),
                        update.fields.raw_input.is_some(),
                        update.fields.raw_output.is_some(),
                    ];
                    for (slot, changed) in state.fields.iter_mut().zip(present) {
                        if changed {
                            *slot = Some(reference);
                        }
                    }
                    (id, state, false)
                }
            }
        }
        _ => return Ok(true),
    };
    let encoded = serde_json::to_vec(&state)?;
    table
        .insert((id, reference.position), encoded.as_slice())
        .map_err(io::Error::other)?;
    Ok(terminal)
}

fn index_tool(
    tools: &mut redb::Table<'_, &str, &[u8]>,
    open: &mut redb::Table<'_, &str, &[u8]>,
    journal: &mut File,
    frontier: Frontier,
    tool: &ToolEvent,
    reference: DomainReference,
    bytes: u64,
) -> io::Result<()> {
    let (id, entry) = match tool {
        ToolEvent::Started {
            call_id,
            turn,
            step,
            ..
        } => {
            if frontier.turn != Some(*turn)
                || frontier.step != Some(*step)
                || tools
                    .get(call_id.as_str())
                    .map_err(io::Error::other)?
                    .is_some()
            {
                return Err(invalid("invalid Tool start in indexed journal"));
            }
            open.insert(call_id.as_str(), serde_json::to_vec(&reference)?.as_slice())
                .map_err(io::Error::other)?;
            (
                call_id,
                Entry {
                    start: reference,
                    terminal: None,
                },
            )
        }
        ToolEvent::Completed { call_id, name, .. } => {
            let start: DomainReference = open
                .remove(call_id.as_str())
                .map_err(io::Error::other)?
                .map(|value| decode(value.value()))
                .transpose()?
                .ok_or_else(|| invalid("Tool terminal has no open start"))?;
            let event = read_domain(journal, start, bytes)?;
            if !matches!(event.kind, TimelineEventKind::Tool(ToolEvent::Started {
                call_id: id, name: tool, ..
            }) if id == *call_id && tool == *name)
            {
                return Err(invalid("Tool terminal disagrees with start"));
            }
            (
                call_id,
                Entry {
                    start,
                    terminal: Some(reference),
                },
            )
        }
    };
    tools
        .insert(id.as_str(), serde_json::to_vec(&entry)?.as_slice())
        .map_err(io::Error::other)?;
    Ok(())
}

fn index_request(
    requests: &mut redb::Table<'_, &str, &[u8]>,
    open: &mut redb::Table<'_, &str, &[u8]>,
    frontier: Frontier,
    request: &RequestEvent,
    reference: DomainReference,
) -> io::Result<()> {
    let (id, entry) = match request {
        RequestEvent::Started { id, turn, step, .. } => {
            if frontier.turn != Some(*turn)
                || frontier.step != Some(*step)
                || requests
                    .get(id.as_str())
                    .map_err(io::Error::other)?
                    .is_some()
            {
                return Err(invalid("invalid Request start in indexed journal"));
            }
            open.insert(id.as_str(), serde_json::to_vec(&reference)?.as_slice())
                .map_err(io::Error::other)?;
            (
                id,
                Entry {
                    start: reference,
                    terminal: None,
                },
            )
        }
        RequestEvent::Retrying { id, .. } => {
            if open.get(id.as_str()).map_err(io::Error::other)?.is_none() {
                return Err(invalid("Request retry has no open start"));
            }
            return Ok(());
        }
        RequestEvent::Completed { id, .. }
        | RequestEvent::Failed { id, .. }
        | RequestEvent::Cancelled { id, .. } => {
            let start = open
                .remove(id.as_str())
                .map_err(io::Error::other)?
                .map(|value| decode(value.value()))
                .transpose()?
                .ok_or_else(|| invalid("Request terminal has no open start"))?;
            (
                id,
                Entry {
                    start,
                    terminal: Some(reference),
                },
            )
        }
    };
    requests
        .insert(id.as_str(), serde_json::to_vec(&entry)?.as_slice())
        .map_err(io::Error::other)?;
    Ok(())
}

#[derive(Debug)]
pub(super) struct Cut {
    index: Index,
    frontier: Frontier,
    history: super::working_snapshot::IndexedHistory,
}

pub(super) fn read_domain(
    journal: &mut File,
    reference: DomainReference,
    bytes: u64,
) -> io::Result<TimelineEvent> {
    if reference
        .record
        .offset
        .checked_add(reference.record.len)
        .is_none_or(|end| end >= bytes)
    {
        return Err(invalid("execution index reference crosses its frontier"));
    }
    match working_index::read_record(journal, reference.record)? {
        JournalRecord {
            fact: JournalFact::Domain { event, .. },
            ..
        } if event.seq == reference.seq => Ok(event),
        _ => Err(invalid(
            "execution index reference does not name its domain event",
        )),
    }
}

fn read_usage_fact(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    journal: &mut File,
    key: &str,
    bytes: u64,
    domain: u64,
) -> io::Result<Option<chat_state::ObservationEvent>> {
    let Some(value) = table.get(key).map_err(io::Error::other)? else {
        return Ok(None);
    };
    let reference: DomainReference = decode(value.value())?;
    if reference.seq.get() >= domain {
        return Ok(None);
    }
    let event = read_domain(journal, reference, bytes)?;
    let TimelineEventKind::Observation(observation) = event.kind else {
        return Err(invalid("usage index does not name an observation"));
    };
    if usage_history_key(&observation).as_deref() != Some(key) {
        return Err(invalid("usage index identity differs from journal"));
    }
    Ok(Some(observation))
}

impl ExecutionHistory for Cut {
    fn event(&self, seq: EventSeq) -> io::Result<Option<TimelineEvent>> {
        if seq.get() >= self.frontier.domain {
            return Ok(None);
        }
        Ok(self.history.domain_events(seq.get()..seq.get() + 1)?.pop())
    }

    fn events_before(&self, end: EventSeq) -> io::Result<Vec<TimelineEvent>> {
        if end.get() > self.frontier.domain {
            return Err(invalid("historical range crosses its published frontier"));
        }
        self.history.domain_events(0..end.get())
    }

    fn response_admission_before(
        &self,
        identity: &chat_state::ResponseAdmissionIdentity,
        seq: EventSeq,
    ) -> io::Result<Option<TimelineEvent>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        response_history::admission_before(
            &tx,
            &mut self.index.journal()?,
            identity,
            seq,
            EventSeq::new(self.frontier.domain),
            self.frontier.bytes,
        )
    }

    fn response_selected(&self, source: EventSeq) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        response_history::selected_at(&tx, self.frontier.rewind, source)
    }

    fn turn_before(&self, id: TurnId, seq: EventSeq) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        boundary_history::turn_before(&tx, &mut self.index.journal()?, self.frontier, id, seq)
    }

    fn admitted_calls_before(
        &self,
        call_id: &str,
        seq: EventSeq,
        limit: usize,
    ) -> io::Result<Vec<sampling_types::ToolCall>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        response_history::admitted_calls_before(
            &tx,
            &mut self.index.journal()?,
            call_id,
            self.frontier
                .context_rebuild
                .map_or(0, |rebuild| rebuild.get() + 1),
            self.frontier.domain.min(seq.get()),
            self.frontier.bytes,
            limit,
            self.frontier.rewind,
        )
    }

    fn step_before(&self, id: StepId, seq: EventSeq) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        boundary_history::step_before(&tx, &mut self.index.journal()?, self.frontier, id, seq)
    }

    fn workflow_before(
        &self,
        run_id: &str,
        seq: EventSeq,
    ) -> io::Result<Option<(TimelineEvent, TimelineEvent)>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        workflow_history::workflow_before(
            &tx,
            &mut self.index.journal()?,
            self.frontier,
            run_id,
            seq,
        )
    }

    fn workflow_spawns_before(
        &self,
        seq: EventSeq,
        limit: usize,
    ) -> io::Result<Vec<(EventSeq, String)>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        workflow_history::spawns_before(&tx, self.frontier.domain.min(seq.get()), limit)
    }

    fn outgoing_inquiry_before(
        &self,
        inquiry_id: &str,
        seq: EventSeq,
    ) -> io::Result<Vec<TimelineEvent>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        super::outgoing_inquiry_history::before(
            &tx,
            &mut self.index.journal()?,
            inquiry_id,
            seq.min(EventSeq::new(self.frontier.domain)),
            self.frontier.bytes,
        )
    }

    fn incoming_inquiry_events(
        &self,
        sender: &str,
        inquiry: &str,
    ) -> io::Result<Vec<TimelineEvent>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        super::consultation_history::facts_before(
            &tx,
            &mut self.index.journal()?,
            self.frontier,
            sender,
            inquiry,
        )
    }

    fn user_image_paths(
        &self,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<std::collections::BTreeSet<String>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        let table = tx.open_table(USER_IMAGE_PATHS).map_err(io::Error::other)?;
        let mut journal = self.index.journal()?;
        let mut paths = std::collections::BTreeSet::new();
        for row in table.iter().map_err(io::Error::other)? {
            if cancelled() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "user image ownership read cancelled",
                ));
            }
            let (path, row) = row.map_err(io::Error::other)?;
            let reference: DomainReference = decode(row.value())?;
            if reference.seq.get() < self.frontier.domain {
                let event = read_domain(&mut journal, reference, self.frontier.bytes)?;
                if !chat_state::Timeline::user_image_paths(std::slice::from_ref(&event))
                    .contains(path.value())
                {
                    return Err(invalid(
                        "indexed image ownership differs from its journal record",
                    ));
                }
                paths.insert(path.value().to_owned());
            }
        }
        Ok(paths)
    }

    fn prompts_before(&self, limit: usize) -> io::Result<Vec<chat_state::PromptRecord>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        boundary_history::prompts_before(&tx, &mut self.index.journal()?, self.frontier, limit)
    }

    fn has_prompt(&self, index: usize) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        boundary_history::has_prompt(&tx, self.frontier, index)
    }

    fn input_before(
        &self,
        id: &str,
        seq: EventSeq,
    ) -> io::Result<Option<(TimelineEvent, chat_state::InputLifecycle)>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        input_history::input_before(
            &tx,
            &mut self.index.journal()?,
            id,
            self.frontier.bytes,
            seq.min(EventSeq::new(self.frontier.domain)),
        )
    }

    fn input_payload_hashes(
        &self,
        candidates: Option<&[String]>,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<std::collections::BTreeSet<String>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        input_history::payload_hashes(
            &tx,
            &mut self.index.journal()?,
            self.frontier.bytes,
            EventSeq::new(self.frontier.domain),
            candidates,
            cancelled,
        )
    }
    fn hook_before(
        &self,
        id: &str,
        seq: EventSeq,
    ) -> io::Result<Option<chat_state::HookLifecycleProjection>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        let mut journal = self.index.journal()?;
        hook_history::occurrence(
            &tx,
            &mut journal,
            id,
            self.frontier.bytes,
            seq.min(EventSeq::new(self.frontier.domain)),
        )
    }

    fn hook_run_before(&self, id: &str, seq: EventSeq) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        let mut journal = self.index.journal()?;
        hook_history::run_before(
            &tx,
            &mut journal,
            id,
            self.frontier.bytes,
            seq.min(EventSeq::new(self.frontier.domain)),
        )
    }

    fn input_hook_before(
        &self,
        id: &str,
        seq: EventSeq,
    ) -> io::Result<Option<chat_state::HookLifecycleProjection>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        let mut journal = self.index.journal()?;
        hook_history::input_occurrence(
            &tx,
            &mut journal,
            id,
            self.frontier.bytes,
            seq.min(EventSeq::new(self.frontier.domain)),
        )
    }

    fn tool_before(&self, call_id: &str, seq: EventSeq) -> io::Result<Option<ToolHistoryState>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        let tools = tx.open_table(TOOL).map_err(io::Error::other)?;
        let Some(value) = tools.get(call_id).map_err(io::Error::other)? else {
            return Ok(None);
        };
        let entry: Entry = decode(value.value())?;
        let seq = seq.get().min(self.frontier.domain);
        if entry.start.seq.get() >= seq {
            return Ok(None);
        }
        let mut journal = self.index.journal()?;
        let start = read_domain(&mut journal, entry.start, self.frontier.bytes)?;
        let TimelineEventKind::Tool(ToolEvent::Started {
            call_id: id, name, ..
        }) = start.kind
        else {
            return Err(invalid("Tool index start is not a Tool start"));
        };
        if id != call_id {
            return Err(invalid("Tool index start identity differs from journal"));
        }
        if let Some(terminal) = entry.terminal.filter(|terminal| terminal.seq.get() < seq) {
            let terminal = read_domain(&mut journal, terminal, self.frontier.bytes)?;
            if !matches!(terminal.kind, TimelineEventKind::Tool(ToolEvent::Completed {
                call_id: id, name: terminal_name, ..
            }) if id == call_id && terminal_name == name)
            {
                return Err(invalid("Tool index terminal identity differs from journal"));
            }
            return Ok(Some(ToolHistoryState::Closed));
        }
        Ok(Some(ToolHistoryState::Open))
    }

    fn compaction_before(&self, id: &str, seq: EventSeq) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        super::compaction_history::identity_before(
            &tx,
            &mut self.index.journal()?,
            id,
            seq,
            self.frontier,
            false,
        )
    }

    fn sideband_before(&self, id: &str, seq: EventSeq) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        super::compaction_history::identity_before(
            &tx,
            &mut self.index.journal()?,
            id,
            seq,
            self.frontier,
            true,
        )
    }

    fn interaction_before(&self, call: &str, attempt: &str, seq: EventSeq) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        let table = tx.open_table(INTERACTIONS).map_err(io::Error::other)?;
        let Some(row) = table.get((call, attempt)).map_err(io::Error::other)? else {
            return Ok(false);
        };
        let reference: DomainReference = decode(row.value())?;
        if reference.seq >= seq || reference.seq.get() >= self.frontier.domain {
            return Ok(false);
        }
        let event = read_domain(&mut self.index.journal()?, reference, self.frontier.bytes)?;
        if !matches!(&event.kind, TimelineEventKind::Interaction(chat_state::InteractionEvent::Opened {
            tool_call_id, attempt_id, ..
        }) if tool_call_id == call && attempt_id == attempt)
        {
            return Err(invalid(
                "interaction identity differs from its journal record",
            ));
        }
        Ok(true)
    }

    fn request_before(&self, id: &str, seq: EventSeq) -> io::Result<Option<RequestHistoryState>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        let requests = tx.open_table(REQUEST).map_err(io::Error::other)?;
        let Some(value) = requests.get(id).map_err(io::Error::other)? else {
            return Ok(None);
        };
        let entry: Entry = decode(value.value())?;
        let seq = seq.get().min(self.frontier.domain);
        if entry.start.seq.get() >= seq {
            return Ok(None);
        }
        let mut journal = self.index.journal()?;
        let start = read_domain(&mut journal, entry.start, self.frontier.bytes)?;
        let TimelineEventKind::Request(RequestEvent::Started {
            id: original, turn, ..
        }) = start.kind
        else {
            return Err(invalid("Request index start is not a Request start"));
        };
        if original != id {
            return Err(invalid("Request index start identity differs from journal"));
        }
        let mut state = RequestHistoryState {
            turn,
            completed_attempt: None,
            provider_terminal: false,
        };
        if let Some(terminal) = entry.terminal.filter(|terminal| terminal.seq.get() < seq) {
            let event = read_domain(&mut journal, terminal, self.frontier.bytes)?;
            match event.kind {
                TimelineEventKind::Request(RequestEvent::Completed {
                    id: original,
                    attempt,
                    provider_terminal,
                    ..
                }) if original == id => {
                    state.completed_attempt = Some(attempt);
                    state.provider_terminal = attempt > 0 && provider_terminal.is_some();
                }
                TimelineEventKind::Request(
                    RequestEvent::Failed { id: original, .. }
                    | RequestEvent::Cancelled { id: original, .. },
                ) if original == id => {}
                _ => {
                    return Err(invalid(
                        "Request index terminal identity differs from journal",
                    ));
                }
            }
        }
        Ok(Some(state))
    }

    fn usage_fact(&self, key: &str) -> io::Result<Option<chat_state::ObservationEvent>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        let table = tx.open_table(USAGE).map_err(io::Error::other)?;
        let mut journal = self.index.journal()?;
        read_usage_fact(
            &table,
            &mut journal,
            key,
            self.frontier.bytes,
            self.frontier.domain,
        )
    }

    fn usage_segment(
        &self,
        id: chat_state::UsageSegmentId,
    ) -> io::Result<Option<chat_state::UsageSegment>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        super::usage_history::get(&tx, self.frontier.bytes, id)
    }

    fn usage_segments_page(
        &self,
        before: Option<chat_state::UsageSegmentId>,
        byte_limit: usize,
    ) -> io::Result<chat_state::UsageSegmentPage> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        super::usage_history::page(&tx, self.frontier.bytes, before, byte_limit)
    }

    fn subagent_spawn_before(
        &self,
        subagent_id: &str,
        seq: EventSeq,
    ) -> io::Result<Option<(EventSeq, chat_state::SubagentSpawnEvent)>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        super::subagent_history::spawn_before(
            &tx,
            &mut self.index.journal()?,
            subagent_id,
            self.frontier.bytes,
            self.frontier.domain,
            seq,
        )
    }

    fn subagent_spawn_for_child_before(
        &self,
        child_session_id: &str,
        seq: EventSeq,
    ) -> io::Result<Option<(EventSeq, chat_state::SubagentSpawnEvent)>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        super::subagent_history::spawn_for_child_before(
            &tx,
            &mut self.index.journal()?,
            child_session_id,
            self.frontier.bytes,
            self.frontier.domain,
            seq,
        )
    }

    fn subagent_events_for_child(
        &self,
        child_session_id: &str,
        seq: EventSeq,
    ) -> io::Result<Vec<TimelineEvent>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        subagent_history::events_for_child_before(
            &tx,
            &mut self.index.journal()?,
            child_session_id,
            self.frontier.bytes,
            self.frontier.domain,
            seq,
        )
    }

    fn subagent_events(&self) -> io::Result<Vec<TimelineEvent>> {
        self.index.subagent_lifecycle_events(self.frontier)
    }

    fn subagent_delivery_before(&self, id: &str, seq: EventSeq) -> io::Result<Option<TurnId>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        subagent_history::delivery_before(&tx, &mut self.index.journal()?, id, self.frontier, seq)
    }

    fn notification_before(&self, id: &str, seq: EventSeq) -> io::Result<Option<TimelineEvent>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        notification_history::receipt(&tx, &mut self.index.journal()?, self.frontier, id, seq)
    }

    fn agent_message_receipts(
        &self,
        identity: Option<(&str, &str)>,
    ) -> io::Result<Vec<TimelineEvent>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        notification_history::message_receipts(
            &tx,
            &mut self.index.journal()?,
            self.frontier,
            identity,
        )
    }

    fn notification_payload_hashes(
        &self,
        candidates: Option<&[String]>,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<std::collections::BTreeSet<String>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        notification_history::payload_hashes(
            &tx,
            &mut self.index.journal()?,
            self.frontier,
            candidates,
            cancelled,
        )
    }

    fn notification_consumer(&self, id: &str, seq: EventSeq) -> io::Result<Option<TurnId>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        Ok(notification_history::settlement(
            &tx,
            &mut self.index.journal()?,
            self.frontier,
            id,
            seq,
        )?
        .flatten())
    }

    fn agent_message_was_consumed(&self, id: &str, seq: EventSeq) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        notification_history::agent_message_was_consumed(
            &tx,
            &mut self.index.journal()?,
            self.frontier,
            id,
            seq,
        )
    }

    fn notification_settled(&self, id: &str, seq: EventSeq) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        Ok(notification_history::settlement(
            &tx,
            &mut self.index.journal()?,
            self.frontier,
            id,
            seq,
        )?
        .is_some())
    }

    fn task_completion_before(&self, id: &str, monitor: bool, seq: EventSeq) -> io::Result<bool> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        notification_history::completed_task(
            &tx,
            &mut self.index.journal()?,
            self.frontier,
            id,
            monitor,
            seq,
        )
    }

    fn subagent_terminal_before(
        &self,
        subagent_id: &str,
        seq: EventSeq,
    ) -> io::Result<Option<(EventSeq, chat_state::SubagentTerminalEvent)>> {
        let tx = self.index.db.begin_read().map_err(io::Error::other)?;
        super::subagent_history::terminal_before(
            &tx,
            &mut self.index.journal()?,
            subagent_id,
            self.frontier.bytes,
            self.frontier.domain,
            seq,
        )
    }
}

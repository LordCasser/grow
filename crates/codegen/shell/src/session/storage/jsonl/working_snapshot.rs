//! Read plans for a verified journal prefix. Independent notices and diagnostic
//! observations stay out of executable replay and remain addressable on disk.

#[cfg(unix)]
use super::ledger::JournalData;
use super::working_index::{self, RecordRef};
use super::{JsonlStorageAdapter, LedgerFileStamp};
#[cfg(unix)]
use super::{LedgerPrefix, OpenedSession, ValidatedTimeline};
use crate::session::history::{self, HistoryContent, HistorySnapshot, JournalFact, JournalRecord};
use crate::session::storage::{ContainedDirectory, SessionUpdate, TIMELINE_FILE};
use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::File;
use std::io;
#[cfg(unix)]
use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;

#[cfg(unix)]
pub(crate) struct IndexedResponseRow(super::response_history::Row);
#[cfg(not(unix))]
pub(crate) enum IndexedResponseRow {}

#[cfg(unix)]
pub(crate) struct IndexedUserEchoRow(super::user_echo_history::Row);
#[cfg(not(unix))]
pub(crate) enum IndexedUserEchoRow {}

#[cfg(unix)]
pub(crate) struct IndexedTaskTerminal(super::tool_history::TaskTerminalRow, bool);
#[cfg(not(unix))]
pub(crate) enum IndexedTaskTerminal {}

impl IndexedTaskTerminal {
    pub(crate) fn task_id(&self) -> &str {
        #[cfg(unix)]
        return &self.0.task_id;
        #[cfg(not(unix))]
        match *self {}
    }
    pub(crate) fn superseded(&self) -> bool {
        #[cfg(unix)]
        return self.1;
        #[cfg(not(unix))]
        match *self {}
    }
    pub(crate) fn position(&self) -> (u64, u64) {
        #[cfg(unix)]
        return (self.0.position, self.0.part);
        #[cfg(not(unix))]
        match *self {}
    }
    pub(crate) fn start(&self) -> u64 {
        #[cfg(unix)]
        return self.0.start.position;
        #[cfg(not(unix))]
        match *self {}
    }
}

impl IndexedUserEchoRow {
    pub(crate) fn position(&self) -> (u64, u64) {
        #[cfg(unix)]
        return (self.0.presentation.position, 0);
        #[cfg(not(unix))]
        match *self {}
    }
}

const VERSION: u8 = 51;
const MANIFEST: &str = "working-history.json";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
#[cfg(unix)]
const MAX_UNPUBLISHED_BYTES: u64 = 1024 * 1024;
#[cfg(unix)]
const MAX_UNPUBLISHED_RECORDS: u64 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    version: u8,
    session: String,
    journal: LedgerFileStamp,
    index: LedgerFileStamp,
    suffix: LedgerFileStamp,
    retained: LedgerFileStamp,
    generation: uuid::Uuid,
    base_bytes: u64,
    base_records: u64,
    base_domain_events: u64,
    bytes: u64,
    records: u64,
    domain_events: u64,
    first_domain_at_ms: Option<i64>,
    last_domain: Option<(u64, [u8; 32], Option<history::ResponseAnchor>)>,
    event_counter_floor: u64,
    hash: String,
    retained_records: u64,
    #[cfg(unix)]
    tool_frontier: super::tool_history::Frontier,
    #[cfg(unix)]
    tool_stamp: LedgerFileStamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecordClass {
    Retained,
    Display,
    #[cfg(unix)]
    BranchMarker,
    #[cfg(unix)]
    UserEcho,
    #[cfg(unix)]
    ResponseCandidate,
    #[cfg(unix)]
    InheritedText,
    Diagnostic,
    #[cfg(unix)]
    SamplingEvidence,
    #[cfg(unix)]
    Tool,
    #[cfg(unix)]
    Request,
    #[cfg(unix)]
    Usage,
    #[cfg(unix)]
    Hook,
    #[cfg(unix)]
    Subagent,
    #[cfg(unix)]
    Input,
    #[cfg(unix)]
    Boundary,
    #[cfg(unix)]
    Workflow,
    #[cfg(unix)]
    Runtime,
    #[cfg(unix)]
    Surface,
    #[cfg(unix)]
    Planning,
    #[cfg(unix)]
    Title,
    #[cfg(unix)]
    Model,
    #[cfg(unix)]
    Signals,
    #[cfg(unix)]
    Announcement,
    #[cfg(unix)]
    Interaction,
    #[cfg(unix)]
    Control,
    #[cfg(unix)]
    Compaction,
    #[cfg(unix)]
    Sideband,
    #[cfg(unix)]
    Notification,
    #[cfg(unix)]
    CommunicationNotice,
    #[cfg(unix)]
    BackgroundTask,
    #[cfg(unix)]
    OutgoingInquiry,
    #[cfg(unix)]
    IncomingInquiry,
}

pub(super) struct IndexedRecord {
    pub reference: RecordRef,
    class: RecordClass,
    domain: bool,
}

pub(super) struct LegacyMigration {
    #[cfg(unix)]
    prefix: LedgerPrefix,
    #[cfg(unix)]
    records: Vec<IndexedRecord>,
    #[cfg(unix)]
    event_counter_floor: u64,
    #[cfg(unix)]
    previous_generation: Option<uuid::Uuid>,
}

#[cfg(unix)]
impl LegacyMigration {
    pub(super) fn new(
        directory: &ContainedDirectory,
        prefix: &LedgerPrefix,
        records: Vec<IndexedRecord>,
    ) -> io::Result<Self> {
        Ok(Self {
            prefix: LedgerPrefix {
                state: history::AppendState {
                    events: prefix.state.events,
                    records: prefix.state.records,
                    first_domain_at_ms: prefix.state.first_domain_at_ms,
                    last_domain: prefix.state.last_domain,
                    ..Default::default()
                },
                len: prefix.len,
                stamp: prefix.stamp.clone(),
                hash: prefix.hash,
                hasher: prefix.hasher.clone(),
                indexed: None,
                has_full_hash: true,
                unpublished_records: Vec::new(),
                unpublished_domain: Vec::new(),
            },
            records,
            event_counter_floor: prefix
                .state
                .identities
                .keys()
                .filter_map(|id| history::event_counter_after(id))
                .max()
                .unwrap_or(0),
            previous_generation: read_manifest_header(directory)?
                .map(|manifest| manifest.generation),
        })
    }

    pub(super) fn publish_for_writer(
        self,
        opened: &OpenedSession,
        state: &super::TimelinePrefix,
        writer: &super::WeakWriterLeases,
    ) -> io::Result<()> {
        let directory = opened.directory();
        let name = OsStr::new(TIMELINE_FILE);
        let path = directory.display_path().join(name);
        let mut file = directory.open_regular(name, "Timeline journal")?;
        let check_owner = || {
            if writer.strong_count() == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "legacy writer retired",
                ));
            }
            Ok(())
        };
        let mut candidate = build(
            opened.directory_handle(),
            opened.summary().info.id.0.as_ref(),
            &mut file,
            &self.prefix,
            &self.records,
            check_owner,
            true,
        )?;
        Arc::make_mut(&mut candidate.index.manifest).event_counter_floor = self.event_counter_floor;
        let Some(owner) = writer.upgrade() else {
            return Ok(());
        };
        let _lock = JsonlStorageAdapter::lock_append_contained(directory, name, &path)?;
        let info = &opened.summary().info;
        let key = format!("{}\0{}", info.id, info.cwd);
        if !owner
            .lock()
            .map_err(|_| io::Error::other("session writer lease cache poisoned"))?
            .contains_key(&key)
        {
            return Ok(());
        }
        let mut current = state
            .lock()
            .map_err(|_| io::Error::other("Timeline prefix state poisoned"))?;
        let Some(current) = current.as_mut() else {
            return Ok(());
        };
        if current.indexed.is_some()
            || current.len < self.prefix.len
            || !current.stamp.same_file(&self.prefix.stamp)
            || (current.len == self.prefix.len && current.hash != self.prefix.hash)
            || read_manifest_header(directory)?.map(|manifest| manifest.generation)
                != self.previous_generation
        {
            return Ok(());
        }
        let mut active = directory.open_regular(name, "Timeline journal")?;
        JsonlStorageAdapter::check_timeline_prefix_stamp(&active, &current.stamp)?;
        // Attach only the work committed since the validated legacy read.
        // The next ordinary append can advance this publication.
        if let Some(index) = candidate.publish()? {
            attach_to_validated_prefix(
                opened.directory_handle(),
                &mut active,
                current,
                Some(index),
            )?;
        }
        Ok(())
    }
}

impl IndexedRecord {
    pub(super) fn new(record: &JournalRecord, reference: RecordRef) -> Self {
        let class = classify(record);
        Self {
            reference,
            class,
            domain: matches!(record.fact, JournalFact::Domain { .. }),
        }
    }

    pub(super) fn needs_legacy_fold(&self) -> bool {
        !matches!(self.class, RecordClass::Diagnostic | RecordClass::Display)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedHistory {
    directory: Arc<ContainedDirectory>,
    manifest: Arc<Manifest>,
    #[cfg(unix)]
    tools: super::tool_history::Index,
}

pub(super) fn classify(record: &JournalRecord) -> RecordClass {
    #[cfg(unix)]
    if let JournalFact::Domain { event, .. } = &record.fact {
        match &event.kind {
            chat_state::TimelineEventKind::Turn(_) | chat_state::TimelineEventKind::Step(_) => {
                return RecordClass::Boundary;
            }
            chat_state::TimelineEventKind::Runtime(_) => return RecordClass::Runtime,
            chat_state::TimelineEventKind::ImageProjection(_) => return RecordClass::Surface,
            chat_state::TimelineEventKind::Workflow(_) => return RecordClass::Workflow,
            chat_state::TimelineEventKind::Tool(_) => return RecordClass::Tool,
            chat_state::TimelineEventKind::Compaction(_) => return RecordClass::Compaction,
            chat_state::TimelineEventKind::Sideband(spawn)
                if spawn.purpose == chat_state::SidebandPurpose::CompactionSummary
                    || super::compaction_history::archives_terminal_sideband(spawn.purpose) =>
            {
                return RecordClass::Sideband;
            }
            chat_state::TimelineEventKind::Messages(message)
                if message.cause == chat_state::MessageCause::Compaction =>
            {
                return RecordClass::Compaction;
            }

            chat_state::TimelineEventKind::Messages(_) => {
                return RecordClass::Surface;
            }
            chat_state::TimelineEventKind::Control(_) => return RecordClass::Control,
            chat_state::TimelineEventKind::Planning(_) => return RecordClass::Planning,
            chat_state::TimelineEventKind::SessionTitle(_) => return RecordClass::Title,
            _ if crate::session::persistence::is_model_change(event) => return RecordClass::Model,
            chat_state::TimelineEventKind::Interaction(_) => return RecordClass::Interaction,
            chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Received { .. },
            ) => return RecordClass::Notification,
            chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Consumed { input, .. },
            ) if input.as_ref().is_none_or(|item| matches!(item,
                sampling_types::ConversationItem::AgentMessage(_))
                || matches!(item, sampling_types::ConversationItem::User(user) if user.synthetic_reason.is_some())) =>
                return RecordClass::Notification,
            chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Dismissed { .. },
            ) => return RecordClass::Notification,
            chat_state::TimelineEventKind::Request(_) => return RecordClass::Request,
            chat_state::TimelineEventKind::Hook(_) => return RecordClass::Hook,
            chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(_) | chat_state::SubagentEvent::Ended(_),
            ) => return RecordClass::Subagent,
            chat_state::TimelineEventKind::Input(_) => return RecordClass::Input,
            chat_state::TimelineEventKind::Observation(observation)
                if super::outgoing_inquiry_history::is_outgoing(observation) =>
            {
                return RecordClass::OutgoingInquiry;
            }
            chat_state::TimelineEventKind::Observation(observation)
                if super::consultation_history::is_incoming(observation) =>
            {
                return RecordClass::IncomingInquiry;
            }
            chat_state::TimelineEventKind::Observation(observation)
                if observation.scope == crate::session::signals::TIMELINE_SIGNALS_SCOPE
                    && observation.name == crate::session::signals::TIMELINE_SIGNALS_NAME =>
            {
                return RecordClass::Signals;
            }
            chat_state::TimelineEventKind::Observation(observation)
                if observation.scope == crate::session::announcement_state::TIMELINE_ANNOUNCEMENT_SCOPE
                    && observation.name == crate::session::announcement_state::TIMELINE_ANNOUNCEMENT_NAME =>
            {
                return RecordClass::Announcement;
            }
            chat_state::TimelineEventKind::Observation(observation)
                if event.at_ms >= 0 && chat_state::actor::state::is_usage_fact(observation) =>
            {
                return RecordClass::Usage;
            }
            chat_state::TimelineEventKind::Observation(observation)
                if event.at_ms >= 0
                    && observation.scope == crate::session::sampling_evidence::SCOPE =>
            {
                return RecordClass::SamplingEvidence;
            }
            _ => {}
        }
    }
    if let JournalFact::Domain { event, .. } = &record.fact
        && chat_state::Timeline::is_diagnostic(event)
        && event.at_ms >= 0
    {
        return RecordClass::Diagnostic;
    }
    let JournalFact::Presentation { event, .. } = &record.fact else {
        return RecordClass::Retained;
    };
    #[cfg(unix)]
    if matches!(&event.content, HistoryContent::Grow(value)
        if matches!(value.as_ref(), history::GrowHistoryFact::RewindMarker { .. }))
    {
        return RecordClass::BranchMarker;
    }
    #[cfg(unix)]
    if matches!(event.content, HistoryContent::Notice { .. }) {
        return RecordClass::CommunicationNotice;
    }
    #[cfg(unix)]
    if matches!(event.content, HistoryContent::Candidate(_)) {
        return RecordClass::ResponseCandidate;
    }
    #[cfg(unix)]
    if matches!(
        event.content,
        HistoryContent::Message {
            channel: history::TextChannel::Assistant | history::TextChannel::Reasoning,
            ..
        }
    ) {
        return RecordClass::InheritedText;
    }
    #[cfg(unix)]
    if history::indexes_background_task(event) {
        return RecordClass::BackgroundTask;
    }
    #[cfg(unix)]
    if matches!(
        event.content,
        HistoryContent::ToolCall(_) | HistoryContent::ToolUpdate(_)
    ) && event.meta.as_ref().is_none_or(|meta| {
        !meta.contains_key("grow/displayProjectionSource")
            && !meta.contains_key("grow/displayProjectionRevision")
    }) {
        return RecordClass::Display;
    }
    #[cfg(unix)]
    if matches!(&event.content, HistoryContent::User(chunk)
        if history::user_echo_identity(chunk).is_some())
        && event.meta.as_ref().is_none_or(|meta| {
            !meta.contains_key("grow/displayProjectionSource")
                && !meta.contains_key("grow/displayProjectionRevision")
        })
    {
        return RecordClass::UserEcho;
    }
    #[cfg(unix)]
    if matches!(&event.content, HistoryContent::Grow(value)
        if matches!(value.as_ref(),
            history::GrowHistoryFact::TurnCompleted { .. } |
            history::GrowHistoryFact::ControlStateUpdate(_)))
        && event.meta.as_ref().is_none_or(|meta| {
            !meta.contains_key("grow/displayProjectionSource")
                && !meta.contains_key("grow/displayProjectionRevision")
        })
    {
        return RecordClass::Display;
    }
    #[cfg(unix)]
    if matches!(&event.content, HistoryContent::Grow(value)
        if matches!(value.as_ref(), history::GrowHistoryFact::WorkflowUpdated { .. }
            | history::GrowHistoryFact::GoalUpdated { .. }
            | history::GrowHistoryFact::SessionRecap { .. } | history::GrowHistoryFact::SessionRecapUnavailable))
    {
        return RecordClass::Display;
    }
    #[cfg(unix)]
    if matches!(&event.content, HistoryContent::Grow(value)
        if matches!(value.as_ref(), history::GrowHistoryFact::SubagentSpawned { .. }
            | history::GrowHistoryFact::SubagentProgress { .. }
            | history::GrowHistoryFact::SubagentFinished { .. }))
    {
        return RecordClass::Display;
    }
    #[cfg(unix)]
    if matches!(&event.content, HistoryContent::Grow(value)
        if matches!(value.as_ref(),
            history::GrowHistoryFact::RetryState(
                crate::extensions::notification::RetryState::Exhausted { .. }
                    | crate::extensions::notification::RetryState::Failed { .. }
            )
            | history::GrowHistoryFact::AutoCompactCompleted { .. }
            | history::GrowHistoryFact::AutoCompactFailed { .. }
            | history::GrowHistoryFact::AutoCompactCancelled { .. }
            | history::GrowHistoryFact::MemoryFlushCompleted { .. }
            | history::GrowHistoryFact::MemoryDreamCompleted { .. }
            | history::GrowHistoryFact::MemorySessionSaved { .. }))
        && event.meta.as_ref().is_none_or(|meta| {
            !meta.contains_key("grow/displayProjectionSource")
                && !meta.contains_key("grow/displayProjectionRevision")
        })
    {
        return RecordClass::Display;
    }
    if event.meta.as_ref().is_none_or(|meta| {
        meta.keys()
            .all(|key| matches!(key.as_str(), "eventId" | "agentTimestampMs"))
    }) && matches!(&event.content, HistoryContent::Grow(value)
            if matches!(value.as_ref(), history::GrowHistoryFact::UiNotice(_)))
    {
        RecordClass::Display
    } else {
        RecordClass::Retained
    }
}

pub(super) fn retains_reference(record: &JournalRecord) -> bool {
    match classify(record) {
        RecordClass::Diagnostic => false,
        #[cfg(unix)]
        RecordClass::Tool
        | RecordClass::Request
        | RecordClass::Usage
        | RecordClass::Hook
        | RecordClass::Input
        | RecordClass::SamplingEvidence => false,
        #[cfg(unix)]
        RecordClass::Subagent
        | RecordClass::Boundary
        | RecordClass::Runtime
        | RecordClass::Surface
        | RecordClass::UserEcho
        | RecordClass::Planning
        | RecordClass::Title
        | RecordClass::Model
        | RecordClass::ResponseCandidate
        | RecordClass::InheritedText
        | RecordClass::BranchMarker
        | RecordClass::Signals
        | RecordClass::Announcement
        | RecordClass::Interaction
        | RecordClass::Control
        | RecordClass::Compaction
        | RecordClass::Sideband => false,
        #[cfg(unix)]
        RecordClass::Notification
        | RecordClass::CommunicationNotice
        | RecordClass::BackgroundTask
        | RecordClass::Workflow
        | RecordClass::IncomingInquiry
        | RecordClass::OutgoingInquiry => false,
        RecordClass::Display => false,
        RecordClass::Retained => true,
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

impl IndexedHistory {
    #[cfg(unix)]
    pub(super) fn acknowledge_torn_tail_removal(
        &self,
        file: &File,
        complete_len: u64,
        stamp: &LedgerFileStamp,
    ) -> io::Result<Option<Self>> {
        if complete_len != self.manifest.bytes {
            return Ok(None);
        }
        JsonlStorageAdapter::sync_file_durable(file)?;
        let mut manifest = (*self.manifest).clone();
        manifest.journal = stamp.clone();
        if !write_manifest(&self.directory, &manifest)? {
            return Err(invalid("cannot refresh the repaired Timeline stamp"));
        }
        Ok(Some(Self {
            manifest: Arc::new(manifest),
            ..self.clone()
        }))
    }

    pub(crate) fn visible_range_after(
        &self,
        anchor: u64,
        last: u64,
    ) -> io::Result<std::ops::RangeInclusive<u64>> {
        #[cfg(unix)]
        return self
            .tools
            .visible_range_after(self.manifest.tool_frontier, anchor, last);
        #[cfg(not(unix))]
        Ok(anchor..=last)
    }

    pub(crate) fn visible_range(&self, anchor: u64) -> io::Result<std::ops::RangeInclusive<u64>> {
        #[cfg(unix)]
        return self
            .tools
            .visible_range(self.manifest.tool_frontier, anchor);
        #[cfg(not(unix))]
        Ok(0..=anchor)
    }

    pub(crate) fn has_child_presentation(&self) -> bool {
        #[cfg(unix)]
        return self.manifest.tool_frontier.has_child_presentation;
        #[cfg(not(unix))]
        false
    }

    pub(crate) fn subagent_lifecycle_events(&self) -> io::Result<Vec<chat_state::TimelineEvent>> {
        #[cfg(unix)]
        return self
            .tools
            .subagent_lifecycle_events(self.manifest.tool_frontier);
        #[cfg(not(unix))]
        Ok(Vec::new())
    }

    pub(crate) fn notice_source(
        &self,
        identity: &(String, Option<String>, Option<String>),
        reject_conflict: bool,
    ) -> io::Result<Option<crate::session::storage::communication_history::ReceiptSource>> {
        #[cfg(unix)]
        return self
            .tools
            .notice_source(self.manifest.tool_frontier, identity, reject_conflict);
        #[cfg(not(unix))]
        {
            let _ = (identity, reject_conflict);
            Ok(None)
        }
    }

    pub(crate) fn sideband_spawn(&self, id: &str) -> io::Result<Option<chat_state::TimelineEvent>> {
        #[cfg(unix)]
        return self.tools.sideband_spawn(self.manifest.tool_frontier, id);
        #[cfg(not(unix))]
        {
            let _ = id;
            Ok(None)
        }
    }

    pub(crate) fn recap_covered_main_turns(&self) -> Option<usize> {
        #[cfg(unix)]
        return self.manifest.tool_frontier.recap_covered_main_turns;
        #[cfg(not(unix))]
        None
    }

    #[cfg(unix)]
    pub(super) fn execution_frontier(&self) -> super::tool_history::Frontier {
        self.manifest.tool_frontier
    }

    #[cfg(unix)]
    pub(super) fn domain_events(
        &self,
        range: std::ops::Range<u64>,
    ) -> io::Result<Vec<chat_state::TimelineEvent>> {
        read_domain_range_at(
            &self.directory,
            &self.manifest,
            range,
            Some(self.manifest.bytes),
        )?
        .ok_or_else(|| invalid("published domain index is unavailable"))
    }

    pub(crate) fn published_domain(&self) -> u64 {
        self.manifest.domain_events
    }

    pub(crate) fn records(&self) -> u64 {
        self.manifest.records
    }

    pub(crate) fn omitted_count(&self) -> usize {
        #[cfg(unix)]
        {
            self.manifest.tool_frontier.display_rows as usize
        }
        #[cfg(not(unix))]
        {
            0
        }
    }

    pub(crate) fn omitted_display_record_count(&self) -> usize {
        #[cfg(unix)]
        return (self.manifest.tool_frontier.display_rows
            - self.manifest.tool_frontier.fallback_notice_rows
            + self.manifest.tool_frontier.inherited_text_rows) as usize;
        #[cfg(not(unix))]
        0
    }

    pub(crate) fn event_counter_floor(&self) -> u64 {
        self.manifest.event_counter_floor
    }

    pub(super) fn identity(&self, id: &str) -> io::Result<Option<blake3::Hash>> {
        #[cfg(unix)]
        {
            let Some(reference) = self.tools.display_by_id(id)? else {
                return Ok(None);
            };
            if reference.position >= self.manifest.records {
                return Ok(None);
            }
            let mut reader = self.page_reader()?;
            let record = reader.read_published_record(reference)?;
            let JournalFact::Presentation { event, .. } = record.fact else {
                return Err(invalid(
                    "display identity refers to a non-presentation fact",
                ));
            };
            if event.event_id() != Some(id) {
                return Err(invalid("display identity differs from the journal"));
            }
            Ok(Some(record.digest))
        }
        #[cfg(not(unix))]
        {
            let _ = id;
            Ok(None)
        }
    }

    pub(crate) fn control_presentations(&self) -> io::Result<Vec<history::HistoryFact>> {
        #[cfg(unix)]
        {
            let mut reader = self.page_reader()?;
            self.tools.control_presentations(self.manifest.tool_frontier)?
                .into_iter()
                .map(|reference| {
                    let record = reader.read_display(reference)?;
                    let JournalFact::Presentation { event, .. } = record.fact else {
                        return Err(invalid("control receipt does not name presentation"));
                    };
                    if !matches!(&event.content, HistoryContent::Grow(value)
                        if matches!(value.as_ref(), history::GrowHistoryFact::ControlStateUpdate(_))) {
                        return Err(invalid("control receipt differs from its presentation"));
                    }
                    Ok(event)
                })
                .collect()
        }
        #[cfg(not(unix))]
        Ok(Vec::new())
    }

    pub(super) fn response_candidate(
        &self,
        key: &history::ResponseKey,
    ) -> io::Result<Option<JournalRecord>> {
        #[cfg(unix)]
        {
            let Some(reference) = self.tools.response_candidate(key, self.manifest.records)? else {
                return Ok(None);
            };
            let record = self.page_reader()?.read_display(reference)?;
            if !matches!(&record.fact, JournalFact::Presentation { event, .. }
                if matches!(&event.content, HistoryContent::Candidate(original) if original == key))
            {
                return Err(invalid("response candidate key differs from its source"));
            }
            Ok(Some(record))
        }
        #[cfg(not(unix))]
        {
            let _ = key;
            Ok(None)
        }
    }

    /// Number of indexed-only display rows before a physical position.
    pub(crate) fn omitted_before(&self, position: u64) -> io::Result<usize> {
        #[cfg(unix)]
        return self
            .tools
            .display_rank(position, self.manifest.tool_frontier.display_rows);
        #[cfg(not(unix))]
        {
            let _ = position;
            Ok(0)
        }
    }

    #[cfg(unix)]
    pub(crate) fn display_position(&self, rank: usize) -> io::Result<u64> {
        Ok(self.tools.display_at(rank as u64)?.position)
    }

    #[cfg(not(unix))]
    pub(crate) fn display_position(&self, _: usize) -> io::Result<u64> {
        Err(invalid("indexed history is unsupported"))
    }

    pub(crate) fn page_reader(&self) -> io::Result<IndexedHistoryReader<'_>> {
        let journal = self
            .directory
            .open_regular(OsStr::new(TIMELINE_FILE), "Timeline")?;
        let stamp = LedgerFileStamp::from_metadata(&journal.metadata()?);
        if !self.manifest.journal.same_file(&stamp) || stamp.len < self.manifest.bytes {
            return Err(invalid("indexed Timeline source was replaced"));
        }
        Ok(IndexedHistoryReader {
            history: self,
            journal,
        })
    }

    pub(crate) fn timeline_newest(&self, limit: usize) -> io::Result<history::HistoryTimelinePage> {
        self.timeline(history::TimelineQuery::Newest, limit)
    }

    pub(crate) fn timeline_before(
        &self,
        before: u64,
        limit: usize,
    ) -> io::Result<history::HistoryTimelinePage> {
        self.timeline(history::TimelineQuery::Before(before), limit)
    }

    pub(crate) fn timeline_around_ordinal(
        &self,
        ordinal: u64,
        limit: usize,
    ) -> io::Result<history::HistoryTimelinePage> {
        self.timeline(history::TimelineQuery::Around(ordinal), limit)
    }

    /// Read a span frozen at `bytes`. The open history uses its own frontier;
    /// tests use an older frontier to prove a later append did not rewrite it.
    pub(super) fn timeline_at(
        &self,
        bytes: u64,
        query: history::TimelineQuery,
        limit: usize,
    ) -> io::Result<history::HistoryTimelinePage> {
        #[cfg(unix)]
        return self.tools.timeline_page(bytes, query, limit);
        #[cfg(not(unix))]
        {
            let _ = (bytes, query, limit);
            Err(invalid("indexed history is unsupported"))
        }
    }

    fn timeline(
        &self,
        query: history::TimelineQuery,
        limit: usize,
    ) -> io::Result<history::HistoryTimelinePage> {
        #[cfg(unix)]
        return self.timeline_at(self.manifest.tool_frontier.bytes, query, limit);
        #[cfg(not(unix))]
        {
            let _ = (query, limit);
            Err(invalid("indexed history is unsupported"))
        }
    }

    /// Tests prove a consecutive consume extends the published generation
    /// instead of rebuilding ranks.
    #[cfg(all(test, unix))]
    pub(crate) fn timeline_generation(&self, bytes: u64) -> io::Result<u64> {
        self.tools.timeline_generation(bytes)
    }

    /// Published prompt count at the open frontier. `None` means this cut has
    /// no catalog span yet. The count is not the unpublished suffix.
    #[cfg(unix)]
    pub(crate) fn timeline_base_count(&self) -> io::Result<Option<u64>> {
        self.tools
            .timeline_base_count(self.manifest.tool_frontier.bytes)
    }

    /// Insertion rank of one source in the published catalog. `exact` means
    /// that source and display segment are already ranked.
    #[cfg(unix)]
    pub(crate) fn timeline_rank_bound(
        &self,
        source: history::HistorySourceKey,
        display_segment: Option<u32>,
    ) -> io::Result<(u64, bool)> {
        let bound = self.tools.timeline_rank_bound(
            self.manifest.tool_frontier.bytes,
            source,
            display_segment,
        )?;
        Ok((bound.ordinal, bound.exact))
    }

    #[cfg(unix)]
    pub(crate) fn timeline_message_seen(&self, message_id: &str) -> io::Result<bool> {
        self.tools
            .timeline_message_seen(self.manifest.tool_frontier.bytes, message_id)
    }

    /// One bounded rank range. `end` is exclusive and must already be inside
    /// the published count; this does not scan the generation.
    #[cfg(unix)]
    pub(crate) fn timeline_rank_window(
        &self,
        start: u64,
        end: u64,
    ) -> io::Result<Vec<history::HistoryTimelineEntry>> {
        self.tools
            .timeline_rank_window(self.manifest.tool_frontier.bytes, start, end)
    }

    #[cfg(all(test, unix))]
    pub(crate) fn reset_timeline_rank_reads() {
        super::history_timeline::reset_rank_reads();
    }

    #[cfg(all(test, unix))]
    pub(crate) fn timeline_rank_reads() -> u64 {
        super::history_timeline::rank_reads()
    }
}

fn open_index_files(
    directory: &ContainedDirectory,
    manifest: &Manifest,
) -> io::Result<(File, File)> {
    let journal = directory.open_regular(OsStr::new(TIMELINE_FILE), "Timeline")?;
    let index = directory.open_regular(
        OsStr::new(&index_name(manifest.generation)),
        "Timeline index",
    )?;
    Ok((journal, index))
}

/// Resolve an explicit domain range by physical seek.
pub(super) fn read_domain_range(
    directory: &ContainedDirectory,
    range: std::ops::Range<u64>,
) -> io::Result<Option<Vec<chat_state::TimelineEvent>>> {
    let Some(manifest) = read_manifest(directory)? else {
        return Ok(None);
    };
    read_domain_range_at(directory, &manifest, range, None)
}

#[cfg(unix)]
pub(super) fn read_child_result(
    opened: &OpenedSession,
    result: u64,
) -> io::Result<Option<(chat_state::TimelineEvent, chat_state::TimelineEvent)>> {
    let directory = opened.directory();
    let Some(manifest) = read_manifest(directory)? else {
        return Ok(None);
    };
    let Some(seed) = manifest
        .tool_frontier
        .child_seed
        .map(chat_state::EventSeq::get)
    else {
        return Ok(None);
    };
    if manifest.session != opened.summary().info.id.0.as_ref() || seed >= result {
        return Ok(None);
    }
    let get = |seq: u64, bytes| -> io::Result<_> {
        let end = seq
            .checked_add(1)
            .ok_or_else(|| invalid("invalid child result position"))?;
        Ok(read_domain_range_at(directory, &manifest, seq..end, bytes)?
            .and_then(|mut events| events.pop()))
    };
    let Some(seed) = get(seed, Some(manifest.bytes))? else {
        return Ok(None);
    };
    if result < manifest.domain_events {
        return Ok(get(result, Some(manifest.bytes))?.map(|result| (seed, result)));
    }
    let mut file = directory.open_regular(OsStr::new(TIMELINE_FILE), "Timeline")?;
    let file_len = file.metadata()?.len();
    let complete_len = JsonlStorageAdapter::timeline_complete_len(&mut file, file_len)?;
    if complete_len.saturating_sub(manifest.bytes) >= MAX_UNPUBLISHED_BYTES {
        return Ok(None);
    }
    Ok(get(result, Some(complete_len))?.map(|result| (seed, result)))
}

#[cfg(unix)]
fn read_indexed_domain(
    directory: &ContainedDirectory,
    manifest: &Manifest,
    seq: chat_state::EventSeq,
) -> io::Result<Option<chat_state::TimelineEvent>> {
    if seq.get() >= manifest.domain_events {
        return Ok(None);
    }
    let range = seq.get()..seq.get() + 1;
    Ok(
        read_domain_range_at(directory, manifest, range, Some(manifest.bytes))?
            .ok_or_else(|| invalid("compaction source index is unavailable"))?
            .pop(),
    )
}

#[cfg(unix)]
fn read_indexed_domain_reference(
    directory: &ContainedDirectory,
    manifest: &Manifest,
    seq: chat_state::EventSeq,
) -> io::Result<Option<RecordRef>> {
    let seq = seq.get();
    if seq >= manifest.domain_events {
        return Ok(None);
    }
    let (journal, mut index) = open_index_files(directory, manifest)?;
    check_index_sources(manifest, &journal, &index)?;
    let reference = if seq < manifest.base_domain_events {
        let position = working_index::domain_position(&mut index, manifest.base_records, seq)?;
        if position >= manifest.base_records {
            return Err(invalid("invalid base domain position"));
        }
        working_index::read_ref(&mut index, position)?
    } else {
        let mut suffix = directory.open_regular(
            OsStr::new(&suffix_index_name(manifest.generation)),
            "Timeline domain suffix",
        )?;
        let stamp = LedgerFileStamp::from_metadata(&suffix.metadata()?);
        if !manifest.suffix.same_file(&stamp) || stamp.len < manifest.suffix.len {
            return Err(invalid("Timeline domain suffix index was replaced"));
        }
        working_index::read_ref(&mut suffix, seq - manifest.base_domain_events)?
    };
    if reference.offset + reference.len >= manifest.bytes {
        return Err(invalid("domain reference crosses published frontier"));
    }
    Ok(Some(reference))
}

fn read_domain_range_at(
    directory: &ContainedDirectory,
    manifest: &Manifest,
    range: std::ops::Range<u64>,
    cut: Option<u64>,
) -> io::Result<Option<Vec<chat_state::TimelineEvent>>> {
    let (mut journal, mut index) = match open_index_files(directory, manifest) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if check_index_sources(manifest, &journal, &index).is_err() {
        return Ok(None);
    }
    let mut events = Vec::with_capacity((range.end - range.start) as usize);
    for seq in range.start..range.end.min(manifest.base_domain_events) {
        let position = working_index::domain_position(&mut index, manifest.base_records, seq)?;
        let record = read_at(&mut journal, &mut index, position, manifest.base_bytes)?;
        let JournalFact::Domain { event, .. } = record.fact else {
            return Err(invalid(
                "indexed domain position refers to a presentation fact",
            ));
        };
        if event.seq.get() != seq {
            return Err(invalid("indexed domain sequence differs from journal"));
        }
        events.push(event);
    }
    if range.end.min(manifest.domain_events) > manifest.base_domain_events {
        let mut suffix = directory.open_regular(
            OsStr::new(&suffix_index_name(manifest.generation)),
            "Timeline domain suffix",
        )?;
        let stamp = LedgerFileStamp::from_metadata(&suffix.metadata()?);
        if !manifest.suffix.same_file(&stamp) || stamp.len < manifest.suffix.len {
            return Err(invalid("Timeline domain suffix index was replaced"));
        }
        for seq in
            range.start.max(manifest.base_domain_events)..range.end.min(manifest.domain_events)
        {
            let reference =
                working_index::read_ref(&mut suffix, seq - manifest.base_domain_events)?;
            if reference.offset < manifest.base_bytes
                || reference
                    .offset
                    .checked_add(reference.len)
                    .is_none_or(|end| end >= manifest.bytes)
            {
                return Err(invalid("indexed domain suffix crosses its frontier"));
            }
            let record = working_index::read_record(&mut journal, reference)?;
            let JournalFact::Domain { event, .. } = record.fact else {
                return Err(invalid("indexed domain suffix refers to presentation"));
            };
            if event.seq.get() != seq {
                return Err(invalid(
                    "indexed domain suffix sequence differs from journal",
                ));
            }
            events.push(event);
        }
    }
    if range.end > manifest.domain_events {
        // The index covers the published cut. Stop at the actor's captured
        // domain position so a concurrent later commit cannot enter the read.
        let complete_len = match cut {
            Some(bytes) => bytes,
            None => {
                let file_len = journal.metadata()?.len();
                JsonlStorageAdapter::timeline_complete_len(&mut journal, file_len)?
            }
        };
        let mut expected_physical = manifest.records;
        let mut expected_domain = manifest.domain_events;
        JsonlStorageAdapter::scan_journal_bytes(
            &mut journal,
            manifest.bytes..complete_len,
            &directory.display_path().join(TIMELINE_FILE),
            &mut blake3::Hasher::new(),
            None,
            None,
            |record, _, _| {
                if record.seq != expected_physical {
                    return Err(invalid("non-contiguous physical history suffix"));
                }
                expected_physical += 1;
                if let JournalFact::Domain { event, .. } = record.fact {
                    if event.seq.get() != expected_domain {
                        return Err(invalid("non-contiguous domain history suffix"));
                    }
                    expected_domain += 1;
                    if range.contains(&event.seq.get()) {
                        events.push(event);
                    }
                }
                Ok(expected_domain < range.end)
            },
        )?;
        if events.len() as u64 != range.end - range.start {
            return Err(invalid("indexed domain range is incomplete"));
        }
    }
    Ok(Some(events))
}

fn check_index_sources(manifest: &Manifest, journal: &File, index: &File) -> io::Result<()> {
    JsonlStorageAdapter::check_timeline_prefix_stamp(index, &manifest.index)?;
    if working_index::index_len(manifest.base_records)
        .and_then(|len| len.checked_add(manifest.base_domain_events.checked_mul(8)?))
        != Some(manifest.index.len)
    {
        return Err(invalid("incorrect Timeline index length"));
    }
    let stamp = LedgerFileStamp::from_metadata(&journal.metadata()?);
    if !manifest.journal.same_file(&stamp) || stamp.len < manifest.bytes {
        return Err(invalid("indexed Timeline source was replaced"));
    }
    Ok(())
}

impl IndexedHistory {
    #[cfg(unix)]
    pub(super) fn should_advance(&self, prefix: &LedgerPrefix) -> bool {
        prefix.len.saturating_sub(self.manifest.bytes) >= MAX_UNPUBLISHED_BYTES
            || prefix.state.records.saturating_sub(self.manifest.records) >= MAX_UNPUBLISHED_RECORDS
    }

    #[cfg(unix)]
    pub(super) fn advance(
        &self,
        file: &mut File,
        prefix: &LedgerPrefix,
    ) -> io::Result<Option<Self>> {
        if prefix.len == self.manifest.bytes {
            return Ok(None);
        }
        let mut manifest = (*self.manifest).clone();
        let published = manifest
            .domain_events
            .checked_sub(manifest.base_domain_events)
            .ok_or_else(|| invalid("invalid domain suffix frontier"))?;
        if prefix.state.events.checked_sub(manifest.domain_events)
            != Some(prefix.unpublished_domain.len() as u64)
        {
            return Err(invalid("incomplete domain suffix publication"));
        }
        if !prefix.unpublished_domain.is_empty() {
            manifest.suffix = append_index_refs(
                &self.directory,
                &suffix_index_name(manifest.generation),
                &manifest.suffix,
                published,
                &prefix.unpublished_domain,
            )?;
        }
        if !prefix.unpublished_records.is_empty() {
            manifest.retained = append_index_refs(
                &self.directory,
                &retained_index_name(manifest.generation),
                &manifest.retained,
                manifest.retained_records,
                &prefix.unpublished_records,
            )?;
            manifest.retained_records += prefix.unpublished_records.len() as u64;
        }
        manifest.journal = prefix.stamp.clone();
        manifest.bytes = prefix.len;
        manifest.records = prefix.state.records;
        manifest.domain_events = prefix.state.events;
        manifest.first_domain_at_ms = prefix.state.first_domain_at_ms;
        manifest.last_domain = prefix
            .state
            .last_domain
            .map(|(seq, digest, anchor)| (seq, *digest.as_bytes(), anchor));
        manifest.hash = prefix.hash.to_hex().to_string();
        manifest.tool_frontier = self.tools.advance_checked(
            file,
            manifest.tool_frontier,
            prefix.len,
            prefix.state.events,
            &manifest.session,
            |seq| read_indexed_domain_reference(&self.directory, &manifest, seq),
            || Ok(()),
        )?;
        manifest.tool_stamp = self.tools.stamp()?;
        manifest.event_counter_floor = manifest
            .event_counter_floor
            .max(manifest.tool_frontier.event_counter_floor);
        #[cfg(test)]
        publication_crash_probe("advance-before");
        if !write_manifest(&self.directory, &manifest)? {
            return Ok(None);
        }
        #[cfg(test)]
        publication_crash_probe("advance-after");
        Ok(Some(Self {
            directory: self.directory.clone(),
            manifest: Arc::new(manifest),
            tools: self.tools.clone(),
        }))
    }
}

#[cfg(unix)]
fn append_index_refs(
    directory: &ContainedDirectory,
    name: &str,
    published: &LedgerFileStamp,
    position: u64,
    references: &[RecordRef],
) -> io::Result<LedgerFileStamp> {
    let mut file = directory.open_read_write_create(OsStr::new(name))?;
    let stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
    if !published.same_file(&stamp) || stamp.len < published.len {
        return Err(invalid("Timeline index was replaced"));
    }
    if stamp.len > published.len {
        file.set_len(published.len)?;
    }
    for (offset, reference) in references.iter().copied().enumerate() {
        working_index::write_ref(&mut file, position + offset as u64, reference)?;
    }
    JsonlStorageAdapter::sync_file_durable(&file)?;
    Ok(LedgerFileStamp::from_metadata(&file.metadata()?))
}

fn index_name(generation: uuid::Uuid) -> String {
    format!("timeline-{generation}.index")
}

fn suffix_index_name(generation: uuid::Uuid) -> String {
    format!("timeline-{generation}.suffix")
}

fn retained_index_name(generation: uuid::Uuid) -> String {
    format!("timeline-{generation}.retained")
}

pub(crate) struct IndexedHistoryReader<'a> {
    history: &'a IndexedHistory,
    journal: File,
}

impl IndexedHistoryReader<'_> {
    pub(crate) fn timeline_newest(&self, limit: usize) -> io::Result<history::HistoryTimelinePage> {
        self.history.timeline_newest(limit)
    }

    pub(crate) fn timeline_before(
        &self,
        before: u64,
        limit: usize,
    ) -> io::Result<history::HistoryTimelinePage> {
        self.history.timeline_before(before, limit)
    }

    pub(crate) fn timeline_around_ordinal(
        &self,
        ordinal: u64,
        limit: usize,
    ) -> io::Result<history::HistoryTimelinePage> {
        self.history.timeline_around_ordinal(ordinal, limit)
    }

    pub(crate) fn read_subagent_lifecycle(
        &mut self,
        session: &acp::SessionId,
        history: &HistorySnapshot,
    ) -> io::Result<Vec<crate::extensions::notification::SessionNotification>> {
        #[cfg(unix)]
        {
            let mut updates = Vec::new();
            for reference in self
                .history
                .tools
                .subagent_display_before(self.history.records())?
            {
                let Some(SessionUpdate::Grow(notification)) =
                    self.project_display(reference, session, history)?
                else {
                    return Err(invalid("indexed child lifecycle row has no Grow update"));
                };
                if !self
                    .history
                    .visible_range(reference.position)?
                    .contains(&reference.position)
                {
                    continue;
                }
                let parent = match &notification.update {
                    crate::extensions::notification::SessionUpdate::SubagentSpawned {
                        parent_session_id,
                        ..
                    }
                    | crate::extensions::notification::SessionUpdate::SubagentProgress {
                        parent_session_id,
                        ..
                    } => Some(parent_session_id.as_str()),
                    crate::extensions::notification::SessionUpdate::SubagentFinished { .. } => None,
                    _ => return Err(invalid("child lifecycle reference names another update")),
                };
                if parent.is_some_and(|parent| parent != session.0.as_ref()) {
                    return Err(invalid("child lifecycle reference names another parent"));
                }
                updates.push(*notification);
            }
            Ok(updates)
        }
        #[cfg(not(unix))]
        {
            let _ = (session, history);
            Ok(Vec::new())
        }
    }

    pub(crate) fn user_echo_rows_before(
        &self,
        before: (u64, u64),
        limit: usize,
    ) -> io::Result<Vec<IndexedUserEchoRow>> {
        #[cfg(unix)]
        {
            let record = before.0.saturating_add(u64::from(before.1 > 0));
            Ok(self
                .history
                .tools
                .user_echo_rows_before(record, self.history.manifest.tool_frontier, limit)?
                .into_iter()
                .map(IndexedUserEchoRow)
                .collect())
        }
        #[cfg(not(unix))]
        {
            let _ = (before, limit);
            Ok(Vec::new())
        }
    }

    pub(crate) fn user_echo_rows_after(
        &self,
        after: (u64, u64),
        limit: usize,
    ) -> io::Result<Vec<IndexedUserEchoRow>> {
        #[cfg(unix)]
        return Ok(self
            .history
            .tools
            .user_echo_rows_after(
                after.0.saturating_add(u64::from(after.1 > 0)),
                self.history.manifest.tool_frontier,
                limit,
            )?
            .into_iter()
            .map(IndexedUserEchoRow)
            .collect());
        #[cfg(not(unix))]
        {
            let _ = (after, limit);
            Ok(Vec::new())
        }
    }

    pub(crate) fn read_user_echo_row(
        &mut self,
        row: IndexedUserEchoRow,
        session: &acp::SessionId,
        history: &HistorySnapshot,
    ) -> io::Result<SessionUpdate> {
        #[cfg(unix)]
        {
            let record = super::user_echo_history::read_row(
                &mut self.journal,
                row.0,
                self.history.manifest.tool_frontier,
            )?;
            let JournalFact::Presentation { at_ms, event } = record.fact else {
                unreachable!()
            };
            let mut update = event.into_update(session)?.expect("verified user echo row");
            history.stamp_cursor(
                session,
                history.record_count(),
                Some(at_ms),
                record.seq,
                0,
                true,
                &mut update,
            )?;
            Ok(update)
        }
        #[cfg(not(unix))]
        {
            let _ = (session, history);
            match row {}
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn response_rows_before(
        &self,
        _: (u64, u64),
        _: usize,
        _: Option<chat_state::EventSeq>,
    ) -> io::Result<Vec<IndexedResponseRow>> {
        Ok(Vec::new())
    }

    #[cfg(not(unix))]
    pub(crate) fn read_response_row(
        &mut self,
        row: IndexedResponseRow,
        _: Option<usize>,
        _: &acp::SessionId,
        _: &HistorySnapshot,
        _: u64,
    ) -> io::Result<(Option<usize>, SessionUpdate)> {
        match row {}
    }

    #[cfg(unix)]
    pub(crate) fn response_rows_before(
        &self,
        before: (u64, u64),
        limit: usize,
        branch: Option<chat_state::EventSeq>,
    ) -> io::Result<Vec<IndexedResponseRow>> {
        let mut frontier = self.history.manifest.tool_frontier;
        frontier.context_rebuild = branch;
        Ok(self
            .history
            .tools
            .response_rows_before(before, frontier, self.history.records(), limit)?
            .into_iter()
            .map(IndexedResponseRow)
            .collect())
    }

    pub(crate) fn response_rows_after(
        &self,
        after: (u64, u64),
        limit: usize,
        branch: Option<chat_state::EventSeq>,
    ) -> io::Result<Vec<IndexedResponseRow>> {
        #[cfg(unix)]
        {
            let mut frontier = self.history.manifest.tool_frontier;
            frontier.context_rebuild = branch;
            Ok(self
                .history
                .tools
                .response_rows_after(after, frontier, self.history.records(), limit)?
                .into_iter()
                .map(IndexedResponseRow)
                .collect())
        }
        #[cfg(not(unix))]
        {
            let _ = (after, limit, branch);
            Ok(Vec::new())
        }
    }

    pub(crate) fn read_response_row_after(
        &mut self,
        row: IndexedResponseRow,
        offset: Option<usize>,
        session: &acp::SessionId,
        history: &HistorySnapshot,
        budget: u64,
    ) -> io::Result<(Option<usize>, SessionUpdate)> {
        #[cfg(unix)]
        {
            let IndexedResponseRow(row) = row;
            let start = offset.unwrap_or(0) as u64;
            let (end, text) = self.history.tools.response_fragment_after(
                &mut self.journal,
                &row,
                start,
                self.history.manifest.tool_frontier,
                budget,
            )?;
            let newer = (end < row.encoded_bytes).then_some(end as usize);
            Ok((
                newer,
                Self::response_update(row, start, end, text, session, history)?,
            ))
        }
        #[cfg(not(unix))]
        {
            let _ = (offset, session, history, budget);
            match row {}
        }
    }

    #[cfg(unix)]
    pub(crate) fn read_response_row(
        &mut self,
        row: IndexedResponseRow,
        offset: Option<usize>,
        session: &acp::SessionId,
        history: &HistorySnapshot,
        budget: u64,
    ) -> io::Result<(Option<usize>, SessionUpdate)> {
        let IndexedResponseRow(row) = row;
        let end = offset.map_or(row.encoded_bytes, |offset| offset as u64);
        let (start, text) = self.history.tools.response_fragment(
            &mut self.journal,
            &row,
            end,
            self.history.manifest.tool_frontier,
            budget,
        )?;
        Ok((
            (start != 0).then_some(start as usize),
            Self::response_update(row, start, end, text, session, history)?,
        ))
    }

    #[cfg(unix)]
    fn response_update(
        row: super::response_history::Row,
        start: u64,
        end: u64,
        text: String,
        session: &acp::SessionId,
        history: &HistorySnapshot,
    ) -> io::Result<SessionUpdate> {
        use super::response_history::TextOrigin;
        let mut meta = row.meta.unwrap_or_default();
        let (reasoning, chunk_meta) = match row.origin {
            TextOrigin::Response {
                identity,
                reasoning,
            } => {
                meta.remove("eventId");
                meta.insert("responseProjection".into(), true.into());
                meta.insert("samplingRequestId".into(), identity.request_id.into());
                meta.insert("samplingAttempt".into(), identity.attempt.into());
                (reasoning, None)
            }
            TextOrigin::Inherited {
                reasoning,
                chunk_meta,
            } => (reasoning, chunk_meta),
        };
        let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text)))
            .meta(chunk_meta);
        let update = if reasoning {
            acp::SessionUpdate::AgentThoughtChunk(chunk)
        } else {
            acp::SessionUpdate::AgentMessageChunk(chunk)
        };
        meta.insert("grow/historyMessageEvent".into(), row.source.get().into());
        meta.insert("grow/historyMessageItem".into(), row.ordinal.into());
        meta.insert("grow/historyResponseEncodedStartByte".into(), start.into());
        meta.insert("grow/historyResponseEncodedEndByte".into(), end.into());
        meta.insert(
            "grow/historyResponseTotalBytes".into(),
            row.encoded_bytes.into(),
        );
        let mut update = SessionUpdate::Acp(Box::new(
            acp::SessionNotification::new(session.clone(), update).meta(Some(meta)),
        ));
        history.stamp_cursor(
            session,
            history.record_count(),
            Some(row.at_ms),
            row.position,
            row.part as usize,
            row.complete && end == row.encoded_bytes,
            &mut update,
        )?;
        Ok(update)
    }

    pub(crate) fn task_terminals_after(
        &self,
        after: (u64, u64),
        limit: usize,
        cut: u64,
    ) -> io::Result<Vec<IndexedTaskTerminal>> {
        #[cfg(unix)]
        {
            if after >= (self.history.records(), 0) {
                return Ok(Vec::new());
            }
            Ok(self
                .history
                .tools
                .task_terminals_after(after, limit, cut.min(self.history.records()))?
                .into_iter()
                .map(|(row, superseded)| IndexedTaskTerminal(row, superseded))
                .collect())
        }
        #[cfg(not(unix))]
        {
            let _ = (after, limit, cut);
            Ok(Vec::new())
        }
    }

    pub(crate) fn task_terminals_before(
        &self,
        before: (u64, u64),
        limit: usize,
        cut: u64,
    ) -> io::Result<Vec<IndexedTaskTerminal>> {
        #[cfg(unix)]
        return Ok(self
            .history
            .tools
            .task_terminals_before(before.min((self.history.records(), 0)), limit, cut)?
            .into_iter()
            .map(|(row, superseded)| IndexedTaskTerminal(row, superseded))
            .collect());
        #[cfg(not(unix))]
        {
            let _ = (before, limit, cut);
            Ok(Vec::new())
        }
    }

    pub(crate) fn read_task_terminal(
        &mut self,
        row: IndexedTaskTerminal,
        session: &acp::SessionId,
        history: &HistorySnapshot,
    ) -> io::Result<SessionUpdate> {
        #[cfg(unix)]
        {
            let row = row.0;
            let start = self.read_display(row.start)?;
            let JournalFact::Presentation { at_ms, event } = start.fact else {
                return Err(invalid("invalid indexed background task start"));
            };
            let Some(SessionUpdate::Grow(start)) = event.into_update(session)? else {
                return Err(invalid("missing indexed background task start"));
            };
            let event = super::tool_history::read_domain(
                &mut self.journal,
                row.settlement,
                self.history.manifest.bytes,
            )?;
            let settlement = history::background_command_settlements(&event)
                .nth(row.command)
                .ok_or_else(|| invalid("missing indexed Runtime command settlement"))?;
            let crate::extensions::notification::SessionUpdate::TaskBackgrounded {
                task_id,
                tool_call_id,
                ..
            } = &start.update
            else {
                return Err(invalid("background task index does not name a start"));
            };
            if task_id != &settlement.command_id || tool_call_id != &settlement.call_id {
                return Err(invalid(
                    "Runtime command settlement differs from indexed task start",
                ));
            }
            let mut update = history::command_settlement_update(
                session,
                &start.update,
                at_ms,
                settlement,
                event.at_ms,
            );
            history.stamp_cursor(
                session,
                history.record_count(),
                Some(event.at_ms),
                row.position,
                row.part as usize,
                row.complete,
                &mut update,
            )?;
            Ok(update)
        }
        #[cfg(not(unix))]
        {
            let _ = (session, history);
            match row {}
        }
    }

    #[cfg(unix)]
    pub(crate) fn read_display_page(
        &mut self,
        rank: usize,
        session: &acp::SessionId,
        history: &HistorySnapshot,
        end_offset: Option<usize>,
    ) -> io::Result<Option<(Option<usize>, SessionUpdate)>> {
        let reference = self.history.tools.display_at(rank as u64)?;
        if let Some((older, at_ms, fragment)) = self
            .history
            .tools
            .tool_fragment(reference.position, end_offset.map(|value| value as u64))?
        {
            let mut update = SessionUpdate::Grow(Box::new(
                crate::extensions::notification::SessionNotification {
                    session_id: session.clone(),
                    update: crate::extensions::notification::SessionUpdate::ToolOutputFragment(
                        fragment,
                    ),
                    meta: None,
                },
            ));
            history.stamp_cursor(
                session,
                history.record_count(),
                Some(at_ms),
                reference.position,
                0,
                true,
                &mut update,
            )?;
            return Ok(Some((older.map(|value| value as usize), update)));
        }
        if end_offset.is_some() {
            return Err(invalid("Tool byte cursor names an ordinary display row"));
        }
        Ok(self
            .project_display(reference, session, history)?
            .map(|update| (None, update)))
    }

    #[cfg(not(unix))]
    pub(crate) fn read_display_page(
        &mut self,
        _rank: usize,
        _session: &acp::SessionId,
        _history: &HistorySnapshot,
        _end_offset: Option<usize>,
    ) -> io::Result<Option<(Option<usize>, SessionUpdate)>> {
        Err(invalid("indexed history is unsupported"))
    }

    pub(crate) fn read_display_page_after(
        &mut self,
        rank: usize,
        session: &acp::SessionId,
        history: &HistorySnapshot,
        start_offset: Option<usize>,
    ) -> io::Result<Option<(Option<usize>, SessionUpdate)>> {
        #[cfg(unix)]
        {
            let reference = self.history.tools.display_at(rank as u64)?;
            if let Some((newer, at_ms, fragment)) = self
                .history
                .tools
                .tool_fragment_after(reference.position, start_offset.unwrap_or(0) as u64)?
            {
                let mut update = SessionUpdate::Grow(Box::new(
                    crate::extensions::notification::SessionNotification {
                        session_id: session.clone(),
                        update: crate::extensions::notification::SessionUpdate::ToolOutputFragment(
                            fragment,
                        ),
                        meta: None,
                    },
                ));
                history.stamp_cursor(
                    session,
                    history.record_count(),
                    Some(at_ms),
                    reference.position,
                    0,
                    true,
                    &mut update,
                )?;
                return Ok(Some((newer.map(|offset| offset as usize), update)));
            }
            // Byte zero only selects the source itself. A nonzero cursor on a
            // row without an indexed Tool payload is a mismatched coordinate.
            if start_offset.is_some_and(|offset| offset != 0) {
                return Err(invalid("Tool byte cursor names an ordinary display row"));
            }
            Ok(self
                .project_display(reference, session, history)?
                .map(|update| (None, update)))
        }
        #[cfg(not(unix))]
        {
            let _ = (rank, session, history, start_offset);
            Err(invalid("indexed history is unsupported"))
        }
    }

    #[cfg(unix)]
    fn project_display(
        &mut self,
        reference: super::tool_history::DisplayReference,
        session: &acp::SessionId,
        history: &HistorySnapshot,
    ) -> io::Result<Option<SessionUpdate>> {
        let pos = reference.position;
        let record = self.read_display(reference)?;
        let (at_ms, mut update) = match record.fact {
            JournalFact::Presentation { at_ms, event } => {
                let update = if let HistoryContent::Notice { event: seq } = &event.content {
                    let original = read_indexed_domain(
                        &self.history.directory,
                        &self.history.manifest,
                        chat_state::EventSeq::new(*seq),
                    )?
                    .ok_or_else(|| invalid("indexed notice source is absent"))?;
                    self.project_notice(session, &original, event.meta)?
                } else {
                    event
                        .into_update(session)?
                        .expect("indexed display record has a row")
                };
                (at_ms, update)
            }
            JournalFact::Domain { event, .. }
                if crate::session::storage::communication_history::receipt_identity(&event)?
                    .is_some() =>
            {
                let seq = event.seq.get();
                if history.notice_anchors_in_tail.contains(&seq)
                    || self.history.tools.has_notice_anchor(
                        self.history.manifest.tool_frontier,
                        &mut self.journal,
                        seq,
                        self.history.records(),
                    )?
                {
                    return Ok(None);
                }
                let mut meta = acp::Meta::new();
                meta.insert(
                    "eventId".into(),
                    format!("{}-receipt-{seq}", session.0).into(),
                );
                meta.insert("grow/historyTimestampMs".into(), event.at_ms.into());
                (
                    event.at_ms,
                    self.project_notice(session, &event, Some(meta))?,
                )
            }
            JournalFact::Domain { event, .. } => {
                use crate::extensions::notification::SessionNotification;
                use chat_state::{HookEvent, TimelineEventKind};
                let TimelineEventKind::Hook(HookEvent::Completed { occurrence_id, .. }) =
                    event.kind
                else {
                    return Err(invalid("indexed Hook display has no terminal"));
                };
                let cut = self.history.tools.cut(self.history.clone())?;
                let projection = chat_state::timeline::ExecutionHistory::hook_before(
                    &cut,
                    &occurrence_id,
                    chat_state::EventSeq::new(event.seq.get() + 1),
                )?
                .ok_or_else(|| invalid("indexed Hook display has no occurrence"))?;
                let name = crate::session::actor::timeline_hook_event_name(projection.event);
                let update = crate::session::actor::SessionActor::project_hook_execution(
                    name,
                    None,
                    None,
                    &projection,
                    true,
                )
                .ok_or_else(|| invalid("indexed Hook occurrence has no display"))?;
                (
                    event.at_ms,
                    SessionUpdate::Grow(Box::new(SessionNotification {
                        session_id: session.clone(),
                        update,
                        meta: None,
                    })),
                )
            }
        };
        if let SessionUpdate::Acp(notification) = &mut update
            && let acp::SessionUpdate::ToolCallUpdate(change) = &mut notification.update
            && let Some(mut call) = self.tool_before(&change.tool_call_id, pos)?
        {
            call.update(std::mem::take(&mut change.fields));
            notification.update = acp::SessionUpdate::ToolCall(call);
        }
        history.stamp_cursor(
            session,
            history.record_count(),
            Some(at_ms),
            pos,
            0,
            true,
            &mut update,
        )?;
        Ok(Some(update))
    }

    #[cfg(unix)]
    fn project_notice(
        &self,
        session: &acp::SessionId,
        event: &chat_state::TimelineEvent,
        meta: Option<acp::Meta>,
    ) -> io::Result<SessionUpdate> {
        let notice = crate::session::storage::communication_history::read_notice(
            &self.history.directory,
            event,
        )?
        .ok_or_else(|| invalid("indexed notice source has no presentation"))?;
        Ok(SessionUpdate::Grow(Box::new(
            crate::extensions::notification::SessionNotification {
                session_id: session.clone(),
                update: crate::extensions::notification::SessionUpdate::UiNotice(notice),
                meta: meta.map(serde_json::Value::Object),
            },
        )))
    }

    #[cfg(unix)]
    fn read_display(
        &mut self,
        reference: super::tool_history::DisplayReference,
    ) -> io::Result<JournalRecord> {
        let record = self.read_published_record(reference)?;
        if !(matches!(
            classify(&record),
            RecordClass::Display
                | RecordClass::UserEcho
                | RecordClass::ResponseCandidate
                | RecordClass::BackgroundTask
                | RecordClass::CommunicationNotice
        ) || matches!(&record.fact,
                JournalFact::Domain { event, .. } if matches!(&event.kind,
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed { .. }))
                        || crate::session::storage::communication_history::receipt_identity(event)?.is_some()))
        {
            return Err(invalid("indexed record has no display projection"));
        }
        Ok(record)
    }

    #[cfg(unix)]
    fn read_published_record(
        &mut self,
        reference: super::tool_history::DisplayReference,
    ) -> io::Result<JournalRecord> {
        let manifest = &self.history.manifest;
        if reference.position >= manifest.records
            || reference
                .record
                .offset
                .checked_add(reference.record.len)
                .is_none_or(|end| end >= manifest.bytes)
        {
            return Err(invalid("display index crosses its published cut"));
        }
        let record = working_index::read_record(&mut self.journal, reference.record)?;
        if record.seq != reference.position {
            return Err(invalid("display reference differs from journal"));
        }
        Ok(record)
    }

    #[cfg(unix)]
    pub(crate) fn tool_before(
        &mut self,
        id: &acp::ToolCallId,
        position: u64,
    ) -> io::Result<Option<acp::ToolCall>> {
        let state = self
            .history
            .tools
            .tool_display_before(id.0.as_ref(), position.min(self.history.manifest.records))?;
        let Some(start) = state.start else {
            return Ok(None);
        };
        if start.position >= position {
            return Err(invalid("Tool display start crosses the page position"));
        }
        if !self
            .history
            .visible_range(start.position)?
            .contains(&start.position)
        {
            return Ok(None);
        }
        let JournalFact::Presentation { event, .. } = self.read_display(start)?.fact else {
            unreachable!("verified display record");
        };
        let HistoryContent::ToolCall(mut call) = event.content else {
            return Err(invalid("Tool display start refers to a different record"));
        };
        if call.tool_call_id != *id {
            return Err(invalid("Tool display start refers to a different call"));
        }
        for reference in state.changes() {
            if reference.position >= position || reference.position <= start.position {
                return Err(invalid("Tool display field crosses the page position"));
            }
            if !self
                .history
                .visible_range(reference.position)?
                .contains(&reference.position)
            {
                continue;
            }
            let JournalFact::Presentation { event, .. } = self.read_display(reference)?.fact else {
                unreachable!("verified display record");
            };
            let HistoryContent::ToolUpdate(update) = event.content else {
                return Err(invalid("Tool display field refers to a different record"));
            };
            if update.tool_call_id != *id
                || matches!(
                    update.fields.status,
                    Some(acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed)
                )
            {
                return Err(invalid("Tool display field refers to a different call"));
            }
            call.update(update.fields);
        }
        Ok(Some(call))
    }

    #[cfg(not(unix))]
    pub(crate) fn tool_before(
        &mut self,
        _id: &acp::ToolCallId,
        _position: u64,
    ) -> io::Result<Option<acp::ToolCall>> {
        Err(invalid("indexed history is unsupported"))
    }
}

#[cfg(unix)]
impl IndexedResponseRow {
    pub(crate) fn position(&self) -> (u64, u64) {
        (self.0.position, self.0.part)
    }

    pub(crate) fn encoded_bytes(&self) -> u64 {
        self.0.encoded_bytes
    }
}

#[cfg(not(unix))]
impl IndexedResponseRow {
    pub(crate) fn position(&self) -> (u64, u64) {
        match *self {}
    }
    pub(crate) fn encoded_bytes(&self) -> u64 {
        match *self {}
    }
}

impl LedgerFileStamp {
    pub(super) fn same_file(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.device == other.device && self.inode == other.inode
        }
        #[cfg(not(unix))]
        {
            self == other
        }
    }
}

fn read_at(journal: &mut File, index: &mut File, pos: u64, end: u64) -> io::Result<JournalRecord> {
    let reference = working_index::read_ref(index, pos)?;
    if reference
        .offset
        .checked_add(reference.len)
        .is_none_or(|end_of_record| end_of_record >= end)
    {
        return Err(invalid("indexed record crosses published frontier"));
    }
    let record = working_index::read_record(journal, reference)?;
    if record.seq != pos {
        return Err(invalid("indexed record sequence differs from journal"));
    }
    Ok(record)
}

#[cfg(unix)]
pub(super) fn publish(
    directory: Arc<ContainedDirectory>,
    session: &str,
    file: &mut File,
    prefix: &LedgerPrefix,
    references: &[IndexedRecord],
) -> io::Result<Option<IndexedHistory>> {
    let candidate = build(
        directory,
        session,
        file,
        prefix,
        references,
        || Ok(()),
        false,
    )?;
    candidate.publish()
}

#[cfg(unix)]
struct Publication {
    index: IndexedHistory,
    published: bool,
}

#[cfg(unix)]
impl Publication {
    fn publish(mut self) -> io::Result<Option<IndexedHistory>> {
        let directory = &self.index.directory;
        let previous = read_manifest_header(directory)?;
        #[cfg(test)]
        publication_crash_probe("before");
        // An atomic write can report a directory sync failure after rename.
        // Keep this generation available to a manifest that already names it.
        self.published = true;
        if !write_manifest(directory, &self.index.manifest)? {
            self.published = false;
            return Ok(None);
        }
        #[cfg(test)]
        publication_crash_probe("after");
        if let Some(previous) = previous {
            remove_generation(directory, previous.generation);
        }
        Ok(Some(self.index.clone()))
    }
}

#[cfg(unix)]
impl Drop for Publication {
    fn drop(&mut self) {
        if !self.published {
            remove_generation(&self.index.directory, self.index.manifest.generation);
        }
    }
}

#[cfg(unix)]
fn remove_generation(directory: &ContainedDirectory, generation: uuid::Uuid) {
    for name in [
        index_name(generation),
        suffix_index_name(generation),
        retained_index_name(generation),
        super::tool_history::name(generation),
    ] {
        let _ = directory.remove_file(OsStr::new(&name), false);
    }
}

#[cfg(unix)]
fn build(
    directory: Arc<ContainedDirectory>,
    session: &str,
    file: &mut File,
    prefix: &LedgerPrefix,
    references: &[IndexedRecord],
    check_cancelled: impl Fn() -> io::Result<()>,
    allow_append: bool,
) -> io::Result<Publication> {
    let generation = uuid::Uuid::new_v4();
    let result = build_generation(
        &directory,
        session,
        file,
        prefix,
        references,
        generation,
        check_cancelled,
        allow_append,
    );
    match result {
        Ok((manifest, tools)) => Ok(Publication {
            index: IndexedHistory {
                directory,
                manifest: Arc::new(manifest),
                tools,
            },
            published: false,
        }),
        Err(error) => {
            remove_generation(&directory, generation);
            Err(error)
        }
    }
}

#[cfg(unix)]
fn build_generation(
    directory: &Arc<ContainedDirectory>,
    session: &str,
    file: &mut File,
    prefix: &LedgerPrefix,
    references: &[IndexedRecord],
    generation: uuid::Uuid,
    check_cancelled: impl Fn() -> io::Result<()>,
    allow_append: bool,
) -> io::Result<(Manifest, super::tool_history::Index)> {
    let name = index_name(generation);
    let mut index = directory.open_read_write_create(OsStr::new(&name))?;
    let suffix = directory.open_read_write_create(OsStr::new(&suffix_index_name(generation)))?;
    let mut retained =
        directory.open_read_write_create(OsStr::new(&retained_index_name(generation)))?;
    let mut retained_records = 0;
    let mut domains = Vec::new();
    for (seq, record) in references.iter().enumerate() {
        check_cancelled()?;
        let seq = seq as u64;
        working_index::write_ref(&mut index, seq, record.reference)?;
        if record.domain {
            domains.push(seq);
        }
        match record.class {
            RecordClass::Retained => {
                working_index::write_ref(&mut retained, retained_records, record.reference)?;
                retained_records += 1;
            }
            RecordClass::Display
            | RecordClass::Diagnostic
            | RecordClass::SamplingEvidence
            | RecordClass::Tool
            | RecordClass::Request
            | RecordClass::Hook
            | RecordClass::Input
            | RecordClass::Usage => {}
            #[cfg(unix)]
            RecordClass::Subagent
            | RecordClass::Boundary
            | RecordClass::Runtime
            | RecordClass::Surface
            | RecordClass::UserEcho
            | RecordClass::Planning
            | RecordClass::Title
            | RecordClass::Model
            | RecordClass::ResponseCandidate
            | RecordClass::InheritedText
            | RecordClass::BranchMarker
            | RecordClass::Signals
            | RecordClass::Announcement
            | RecordClass::Interaction
            | RecordClass::Control
            | RecordClass::Compaction
            | RecordClass::Sideband => {}
            #[cfg(unix)]
            RecordClass::Notification
            | RecordClass::CommunicationNotice
            | RecordClass::BackgroundTask
            | RecordClass::Workflow
            | RecordClass::IncomingInquiry
            | RecordClass::OutgoingInquiry => {}
        }
    }
    working_index::write_positions(&mut index, &domains)?;
    index.flush()?;
    JsonlStorageAdapter::sync_file_durable(&index)?;
    JsonlStorageAdapter::sync_file_durable(&suffix)?;
    JsonlStorageAdapter::sync_file_durable(&retained)?;
    JsonlStorageAdapter::sync_file_durable(file)?;
    let journal = LedgerFileStamp::from_metadata(&file.metadata()?);
    if journal != prefix.stamp
        && !(allow_append && journal.same_file(&prefix.stamp) && journal.len > prefix.len)
    {
        return Err(invalid("Timeline changed during index publication"));
    }
    let journal = &prefix.stamp;
    let tools =
        super::tool_history::Index::open(directory.clone(), generation, journal.clone(), true)?;
    let mut manifest = Manifest {
        version: VERSION,
        session: session.to_owned(),
        journal: journal.clone(),
        index: LedgerFileStamp::from_metadata(&index.metadata()?),
        suffix: LedgerFileStamp::from_metadata(&suffix.metadata()?),
        retained: LedgerFileStamp::from_metadata(&retained.metadata()?),
        generation,
        base_bytes: prefix.len,
        base_records: prefix.state.records,
        base_domain_events: prefix.state.events,
        bytes: prefix.len,
        records: prefix.state.records,
        domain_events: prefix.state.events,
        first_domain_at_ms: prefix.state.first_domain_at_ms,
        last_domain: prefix
            .state
            .last_domain
            .map(|(seq, digest, anchor)| (seq, *digest.as_bytes(), anchor)),
        event_counter_floor: prefix
            .state
            .identities
            .keys()
            .filter_map(|id| history::event_counter_after(id))
            .max()
            .unwrap_or(0),
        hash: prefix.hash.to_hex().to_string(),
        retained_records,
        tool_frontier: Default::default(),
        tool_stamp: tools.stamp()?,
    };
    manifest.tool_frontier = tools.advance_checked(
        file,
        Default::default(),
        prefix.len,
        prefix.state.events,
        session,
        |seq| read_indexed_domain_reference(directory, &manifest, seq),
        check_cancelled,
    )?;
    manifest.tool_stamp = tools.stamp()?;
    Ok((manifest, tools))
}

#[cfg(unix)]
fn write_manifest(directory: &ContainedDirectory, manifest: &Manifest) -> io::Result<bool> {
    let payload = serde_json::to_vec(manifest).map_err(io::Error::other)?;
    if payload.len() as u64 > MAX_MANIFEST_BYTES {
        return Ok(false);
    }
    let mut encoded = Vec::with_capacity(32 + payload.len());
    encoded.extend_from_slice(blake3::hash(&payload).as_bytes());
    encoded.extend_from_slice(&payload);
    directory.write_atomic(OsStr::new(MANIFEST), &encoded, true, true)?;
    Ok(true)
}

pub(super) fn is_rewind_marker(source: &history::HistorySource) -> bool {
    matches!(source, history::HistorySource::Presentation { fact, .. }
        if matches!(&fact.content, HistoryContent::Grow(value)
            if matches!(value.as_ref(), history::GrowHistoryFact::RewindMarker { .. })))
}

#[cfg(unix)]
pub(super) fn load(
    opened: &OpenedSession,
    file: &mut File,
    complete_len: u64,
    path: &Path,
    cached: Option<IndexedHistory>,
) -> io::Result<Option<(ValidatedTimeline, LedgerPrefix)>> {
    let directory = opened.directory_handle();
    let Some(manifest) = read_manifest(&directory)? else {
        return Ok(None);
    };
    if manifest.session != opened.summary().info.id.0.as_ref() || manifest.bytes > complete_len {
        return Ok(None);
    }
    let stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
    if stamp.len == manifest.journal.len && stamp != manifest.journal {
        return Ok(None);
    }
    let disk_index = match directory.open_regular(
        OsStr::new(&index_name(manifest.generation)),
        "Timeline index",
    ) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if check_index_sources(&manifest, file, &disk_index).is_err() {
        return Ok(None);
    }
    let index = {
        let _timer = crate::instrumentation_timer!("session.load.working_index_open");
        open_history(directory, manifest, cached)?
    };
    let retained = match index.directory.open_regular(
        OsStr::new(&retained_index_name(index.manifest.generation)),
        "Timeline retained index",
    ) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let stamp = LedgerFileStamp::from_metadata(&retained.metadata()?);
    let published = &index.manifest.retained;
    if !published.same_file(&stamp)
        || stamp.len < published.len
        || (stamp.len == published.len && stamp != *published)
    {
        return Ok(None);
    }
    hydrate(opened, file, complete_len, path, index, retained)
}

#[cfg(unix)]
fn open_history(
    directory: Arc<ContainedDirectory>,
    manifest: Manifest,
    cached: Option<IndexedHistory>,
) -> io::Result<IndexedHistory> {
    let named = directory.open_regular(
        OsStr::new(&super::tool_history::name(manifest.generation)),
        "Tool index",
    )?;
    if !manifest
        .tool_stamp
        .same_file(&LedgerFileStamp::from_metadata(&named.metadata()?))
        || manifest.tool_frontier.bytes != manifest.bytes
        || manifest.tool_frontier.domain != manifest.domain_events
        || manifest.tool_frontier.fallback_notice_rows > manifest.tool_frontier.display_rows
    {
        return Err(invalid("Tool index differs from published frontier"));
    }
    let tools = match cached {
        Some(index) if index.manifest.generation == manifest.generation => index.tools,
        _ => super::tool_history::Index::open(
            directory.clone(),
            manifest.generation,
            manifest.journal.clone(),
            false,
        )?,
    };
    if !manifest.tool_stamp.same_file(&tools.stamp()?) {
        return Err(invalid("Tool index was replaced during open"));
    }
    Ok(IndexedHistory {
        directory,
        manifest: Arc::new(manifest),
        tools,
    })
}

/// Retain indexing when a caller requires the dense projection or opens the
/// append path directly. The caller already verified the full journal; collect
/// references only for the part that has not been published.
#[cfg(unix)]
pub(super) fn attach_to_validated_prefix(
    directory: Arc<ContainedDirectory>,
    file: &mut File,
    prefix: &mut LedgerPrefix,
    cached: Option<IndexedHistory>,
) -> io::Result<()> {
    let Some(manifest) = read_manifest(&directory)? else {
        return Ok(());
    };
    if manifest.bytes > prefix.len
        || (manifest.journal.len == prefix.stamp.len && manifest.journal != prefix.stamp)
    {
        return Ok(());
    }
    let index = match directory.open_regular(
        OsStr::new(&index_name(manifest.generation)),
        "Timeline index",
    ) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if check_index_sources(&manifest, file, &index).is_err() {
        return Ok(());
    }
    let mut identities = std::collections::BTreeMap::new();
    JsonlStorageAdapter::scan_journal_bytes(
        file,
        manifest.bytes..prefix.len,
        &directory.display_path().join(TIMELINE_FILE),
        &mut blake3::Hasher::new(),
        None,
        None,
        |record, offset, json| {
            let reference = RecordRef {
                offset,
                len: json.len() as u64,
                digest: blake3::hash(json),
            };
            if matches!(&record.fact, JournalFact::Domain { .. }) {
                prefix.unpublished_domain.push(reference);
            }
            if retains_reference(&record) {
                prefix.unpublished_records.push(reference);
            }
            if let JournalFact::Presentation { event, .. } = &record.fact
                && let Some(id) = event.event_id()
            {
                identities.insert(id.to_owned(), record.digest);
            }
            Ok(true)
        },
    )?;
    JsonlStorageAdapter::check_timeline_prefix_stamp(file, &prefix.stamp)?;
    prefix
        .state
        .anchors
        .retain(|_, position| *position >= manifest.records);
    prefix
        .state
        .receipts
        .retain(|_, source| source.event >= manifest.domain_events);
    prefix.state.identities = identities;
    prefix.indexed = Some(open_history(directory, manifest, cached)?);
    Ok(())
}

fn read_manifest(directory: &ContainedDirectory) -> io::Result<Option<Manifest>> {
    Ok(read_manifest_header(directory)?.filter(|manifest| manifest.version == VERSION))
}

fn read_manifest_header(directory: &ContainedDirectory) -> io::Result<Option<Manifest>> {
    let manifest = match directory.read_bounded(
        OsStr::new(MANIFEST),
        "working history index",
        MAX_MANIFEST_BYTES + 32,
    ) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        // Earlier generations stored cumulative references inside the
        // manifest. They are rebuildable and may exceed the new header size.
        Err(error) if error.kind() == io::ErrorKind::InvalidData => return Ok(None),
        Err(error) => return Err(error),
    };
    let Some((digest, payload)) = manifest.split_at_checked(32) else {
        return Ok(None);
    };
    if blake3::hash(payload).as_bytes() != digest {
        return Ok(None);
    }
    let Ok(manifest) = serde_json::from_slice::<Manifest>(payload) else {
        return Ok(None);
    };
    Ok(Some(manifest))
}

#[cfg(unix)]
pub(super) fn migrate(
    storage: &JsonlStorageAdapter,
    opened: &OpenedSession,
    cancel: &tokio_util::sync::CancellationToken,
) -> io::Result<()> {
    let check_cancelled = || {
        if cancel.is_cancelled() {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "working history migration cancelled",
            ))
        } else {
            Ok(())
        }
    };
    check_cancelled()?;
    let info = &opened.summary().info;
    let directory = opened.directory();
    if read_manifest(directory)?.is_some() {
        return Ok(());
    }
    let name = OsStr::new(TIMELINE_FILE);
    let path = directory.display_path().join(name);
    let mut file = directory.open_regular(name, "Timeline journal")?;
    let stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
    let bytes = JsonlStorageAdapter::timeline_complete_len(&mut file, stamp.len)?;
    let mut references = Vec::new();
    let (events, history, prefix) = JsonlStorageAdapter::read_timeline_prefix_with_index(
        &mut file,
        bytes,
        &path,
        Some(|record: &JournalRecord, reference| {
            check_cancelled()?;
            let indexed = IndexedRecord::new(record, reference);
            let retain = indexed.needs_legacy_fold();
            references.push(indexed);
            Ok(retain)
        }),
    )?;
    JsonlStorageAdapter::check_timeline_prefix_stamp(&file, &stamp)?;
    let timeline = chat_state::Timeline::from_indexed_prefix(
        events,
        chat_state::EventSeq::new(prefix.state.events),
        prefix.state.first_domain_at_ms,
    )
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    opened.validate_timeline_evidence(info.id.0.as_ref(), timeline, history)?;
    check_cancelled()?;
    let candidate = build(
        opened.directory_handle(),
        info.id.0.as_ref(),
        &mut file,
        &prefix,
        &references,
        check_cancelled,
        false,
    )?;
    check_cancelled()?;
    if !storage.try_acquire_writer_lease(info)? {
        return Ok(());
    }
    let result = (|| {
        let current = storage.open_session_for_observation(info)?;
        let current_dir = current.directory();
        if !directory.is_same_entity(current_dir)? || read_manifest(current_dir)?.is_some() {
            return Ok(());
        }
        let _lock = JsonlStorageAdapter::lock_append_contained(current_dir, name, &path)?;
        let current_file = current_dir.open_regular(name, "Timeline journal")?;
        if LedgerFileStamp::from_metadata(&current_file.metadata()?) == stamp {
            candidate.publish()?;
        }
        Ok(())
    })();
    storage.release_writer_lease(&format!("{}\0{}", info.id, info.cwd))?;
    result
}

#[cfg(unix)]
fn hydrate(
    opened: &OpenedSession,
    file: &mut File,
    complete_len: u64,
    path: &Path,
    index: IndexedHistory,
    mut retained: File,
) -> io::Result<Option<(ValidatedTimeline, LedgerPrefix)>> {
    let source = &index.manifest;
    let mut timer = crate::instrumentation_timer!("session.load.working_index_hydrate");
    timer.with_field("journal_bytes", complete_len);
    timer.with_field("tail_bytes", complete_len - source.bytes);
    timer.with_field("retained_records", source.retained_records);
    let mut data = JournalData {
        events: Vec::new(),
        state: history::AppendState::default(),
        history: HistorySnapshot::default(),
    };
    let initial_stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
    let mut expected_retained_offset = 0;
    let mut retained_bytes = 0;
    for position in 0..source.retained_records {
        let reference = working_index::read_ref(&mut retained, position)?;
        let end = reference
            .offset
            .checked_add(reference.len)
            .filter(|end| *end < source.bytes)
            .ok_or_else(|| invalid("working record crosses published frontier"))?;
        if reference.offset < expected_retained_offset {
            return Err(invalid("working record crosses published frontier"));
        }
        expected_retained_offset = end + 1;
        retained_bytes += reference.len;
        accept_retained(
            &mut data,
            working_index::read_record(file, reference)?,
            source.records,
            source.domain_events,
        )?;
    }
    timer.with_field("retained_bytes", retained_bytes);
    let background_starts = index.tools.background_task_starts(source.tool_frontier)?;
    timer.with_field("unresolved_background_tasks", background_starts.len());
    for start in background_starts {
        let record = working_index::read_record(file, start.record)?;
        if record.seq != start.position
            || start.position >= source.records
            || !matches!(&record.fact, JournalFact::Presentation { event, .. }
                if matches!(&event.content, HistoryContent::Grow(value)
                    if matches!(value.as_ref(), history::GrowHistoryFact::TaskBackgrounded { .. })))
        {
            return Err(invalid(
                "background task checkpoint differs from the journal",
            ));
        }
        data.history.positions.push(record.seq);
        data.history.push(&record);
    }
    data.state.display_records += index.omitted_display_record_count();
    data.state.records = source.records;
    data.state.events = source.domain_events;
    data.state.first_domain_at_ms = source.first_domain_at_ms;
    data.state.identities.clear();
    data.state.last_domain = source
        .last_domain
        .map(|(seq, digest, anchor)| (seq, blake3::Hash::from_bytes(digest), anchor));
    let has_full_hash = source.bytes == 0;
    let mut hasher = if has_full_hash {
        blake3::Hasher::new()
    } else {
        let mut seeded = blake3::Hasher::new_derive_key("Grow indexed Timeline tail");
        seeded.update(source.hash.as_bytes());
        seeded
    };
    let mut tail = JournalData {
        events: Vec::new(),
        history: HistorySnapshot::default(),
        state: history::AppendState::default(),
    };
    let mut unpublished_records = Vec::new();
    let mut unpublished_domain = Vec::new();
    let mut published_candidates = Vec::new();
    JsonlStorageAdapter::scan_journal_bytes(
        file,
        source.bytes..complete_len,
        path,
        &mut hasher,
        None,
        None,
        |record, offset, json| {
            match &record.fact {
                JournalFact::Domain {
                    event,
                    response_anchor: Some(history::ResponseAnchor::Candidate { record: position }),
                } if *position < source.records => {
                    let admission = event
                        .messages()
                        .and_then(|message| message.response_admission.as_ref())
                        .ok_or_else(|| invalid("candidate anchor has no response admission"))?;
                    let key = history::ResponseKey {
                        request_id: admission.identity.request_id.clone(),
                        attempt: admission.identity.attempt,
                    };
                    if !data.state.anchors.contains_key(&key) {
                        let candidate = index
                            .response_candidate(&key)?
                            .ok_or_else(|| invalid("missing published response candidate"))?;
                        data.state.anchors.insert(key, candidate.seq);
                        published_candidates.push(candidate);
                    }
                }
                JournalFact::Presentation { event, .. } => {
                    if let HistoryContent::Candidate(key) = &event.content
                        && index.response_candidate(key)?.is_some()
                    {
                        return Err(invalid("duplicate response candidate in Timeline tail"));
                    }
                }
                _ => {}
            }
            if let JournalFact::Presentation { event, .. } = &record.fact {
                data.state.validate_indexed_presentation(event, &index)?;
            }
            data.state.accept(&record)?;
            let reference = RecordRef {
                offset,
                len: json.len() as u64,
                digest: blake3::hash(json),
            };
            if matches!(&record.fact, JournalFact::Domain { .. }) {
                unpublished_domain.push(reference);
            }
            if let JournalFact::Presentation { event, .. } = &record.fact
                && let Some(id) = event.event_id()
                && index.identity(id)?.is_some()
            {
                return Err(invalid("duplicate presentation identity in Timeline tail"));
            }
            match classify(&record) {
                RecordClass::Diagnostic => {
                    if let JournalFact::Domain { event, .. } = record.fact {
                        tail.events.push(event);
                    }
                }
                #[cfg(unix)]
                RecordClass::Subagent => {
                    if let JournalFact::Domain { event, .. } = record.fact {
                        tail.events.push(event);
                    }
                }
                _ => {
                    if retains_reference(&record) {
                        unpublished_records.push(reference);
                    }
                    collect(&mut tail, record);
                }
            }
            Ok(true)
        },
    )?;
    data.events
        .extend(index.tools.open_execution(source.tool_frontier)?);
    let boundaries = index.tools.boundaries(source.tool_frontier)?;
    let workflows = index.tools.open_workflows(source.tool_frontier)?;
    data.events.extend(boundaries.events);
    let compaction = index.tools.compaction(source.tool_frontier)?;
    data.events.extend(compaction.events);
    data.events
        .extend(index.tools.pending_consultations(source.tool_frontier)?);
    let execution = index.tools.execution_checkpoint(source.tool_frontier)?;
    if let Some(reference) = execution.image_projection {
        if reference.seq.get() >= source.domain_events
            || !matches!(
                super::tool_history::read_domain(file, reference, source.bytes)?.kind,
                chat_state::TimelineEventKind::ImageProjection(_)
            )
        {
            return Err(invalid(
                "working Surface image projection differs from the journal",
            ));
        }
    }
    if let Some(rewind) = source.tool_frontier.rewind {
        let event = read_indexed_domain(&index.directory, source, rewind)?
            .ok_or_else(|| invalid("working Surface rewind is missing from the Timeline"))?;
        if !event
            .messages()
            .is_some_and(|message| message.cause == chat_state::MessageCause::Rewind)
        {
            return Err(invalid("working Surface rewind differs from the Timeline"));
        }
    }
    let recovery = index
        .tools
        .recovery_snapshots(&execution, source.tool_frontier)?;
    {
        use crate::session::persistence::{is_model_change, latest_model_selection};

        let model_chain: Vec<_> = recovery
            .iter()
            .rfind(|event| is_model_change(event))
            .into_iter()
            .chain(tail.events.iter().filter(|event| is_model_change(event)))
            .cloned()
            .collect();
        latest_model_selection(&model_chain)?;
    }
    data.events.extend(recovery);
    if tail.events.iter().any(|event| {
        matches!(
            &event.kind,
            chat_state::TimelineEventKind::ImageProjection(_)
                | chat_state::TimelineEventKind::Messages(chat_state::MessageEvent {
                    cause: chat_state::MessageCause::Rewind,
                    ..
                })
        )
    }) {
        return Ok(None);
    }
    data.events
        .extend(index.tools.open_subagents(source.tool_frontier)?);
    let (hooks, hook_events) = index.tools.open_hooks(source.tool_frontier)?;
    let notices = index.tools.pending_notifications(source.tool_frontier)?;
    let inputs = index.tools.open_inputs(source.tool_frontier)?;
    for input in &inputs {
        let input = &input.lifecycle;
        crate::session::input_inbox::validate_submitted_payload(
            opened.directory(),
            &input.input_id,
            input.intent,
            &input.payload_ref,
        )?;
    }
    data.events.extend(hook_events);
    data.events.sort_unstable_by_key(|event| event.seq);
    let mut timeline = chat_state::Timeline::from_working_prefix(
        data.events,
        chat_state::EventSeq::new(source.domain_events),
        source.first_domain_at_ms,
        Some(Arc::new(index.tools.cut(index.clone())?)),
        source
            .tool_frontier
            .latest_tool
            .max(source.tool_frontier.latest_request),
        source.tool_frontier.active_turn_had_tool,
        Some(execution.surface),
    )
    .and_then(|mut base| {
        debug_assert_eq!(base.history_branch(), source.tool_frontier.history_branch());
        base.restore_open_workflows(workflows)?;
        base.restore_hook_checkpoint(hooks.session_id, hooks.generation);
        base.restore_input_checkpoint(inputs)?;
        base.restore_notification_checkpoint(source.session.clone(), notices)?;
        base.restore_prompt_coordinates(boundaries.next_prompt, boundaries.max_prompt);
        base.restore_compaction_prompt_coordinate(compaction.completed_prompt);
        base.restore_runtime_checkpoint(execution.lifecycle, execution.waits, execution.progress);
        base.with_replayed_suffix(tail.events)
    })
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    timeline.restore_last_request(source.tool_frontier.last_request);
    timeline.set_usage_checkpoint(index.tools.usage_checkpoint(source.tool_frontier)?);
    if timeline.archived_active_turn_had_tool()
        && timeline
            .runtime_lifecycle()
            .checkpoint
            .as_ref()
            .and_then(|checkpoint| checkpoint.turn.as_ref())
            .is_some_and(|turn| {
                timeline.active_turn() == Some(turn.turn_id)
                    && turn.resume_point == chat_state::session_runtime::TurnResumePoint::Input
            })
    {
        return Err(invalid(
            "input preparation contains a previous tool preflight",
        ));
    }
    data.history.sources.append(&mut tail.history.sources);
    data.history.positions.append(&mut tail.history.positions);
    // A threshold publication can land between a provisional echo and
    // its consume. Pull only the prefix blocks named by a suffix proof.
    let mut published_echoes = std::collections::BTreeSet::new();
    for event in timeline
        .events()
        .iter()
        .filter(|event| event.seq.get() >= source.domain_events)
    {
        let chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
            display_echo: Some(echo),
            ..
        }) = &event.kind
        else {
            continue;
        };
        if !published_echoes.insert(echo.message_id.as_str()) {
            continue;
        }
        for record in
            index
                .tools
                .user_echo_blocks(&echo.message_id, source.records, source.bytes)?
        {
            data.history.positions.push(record.seq);
            data.history.push(&record);
        }
    }
    let has_published_candidates = !published_candidates.is_empty();
    for record in published_candidates {
        data.history.positions.push(record.seq);
        data.history.push(&record);
    }
    if !published_echoes.is_empty()
        || has_published_candidates
        || !data.history.positions.is_sorted()
    {
        let mut rows = std::mem::take(&mut data.history.positions)
            .into_iter()
            .zip(std::mem::take(&mut data.history.sources))
            .collect::<Vec<_>>();
        rows.sort_unstable_by_key(|(position, _)| *position);
        (data.history.positions, data.history.sources) = rows.into_iter().unzip();
    }
    if data.history.sources.iter().any(is_rewind_marker) {
        return Ok(None);
    }
    let stamp = LedgerFileStamp::from_metadata(&file.metadata()?);
    if stamp != initial_stamp {
        return Err(invalid("Timeline changed while reading indexed cut"));
    }
    data.history.bytes = complete_len;
    data.history.prefix_hash = hasher.finalize().to_hex().to_string();
    data.history.response_branch = timeline.history_branch();
    data.history.indexed = Some(index.clone());
    let validated = opened.validate_timeline_evidence(
        opened.summary().info.id.0.as_ref(),
        timeline,
        data.history,
    )?;
    Ok(Some((
        validated,
        LedgerPrefix {
            len: complete_len,
            hash: hasher.finalize(),
            stamp,
            hasher,
            state: data.state,
            indexed: Some(index),
            has_full_hash,
            unpublished_records,
            unpublished_domain,
        },
    )))
}

#[cfg(unix)]
fn accept_retained(
    data: &mut JournalData,
    record: JournalRecord,
    records: u64,
    domain_events: u64,
) -> io::Result<()> {
    let seq = record.seq;
    if seq >= records || seq < data.state.records {
        return Err(invalid("invalid retained record position"));
    }
    data.state.records = seq;
    if let JournalFact::Domain { event, .. } = &record.fact {
        if event.seq.get() < data.state.events || event.seq.get() >= domain_events {
            return Err(invalid("invalid retained domain sequence"));
        }
        data.state.events = event.seq.get();
    } else if let JournalFact::Presentation { event, .. } = &record.fact
        && let HistoryContent::Notice { event } = &event.content
        && *event >= data.state.events
    {
        return Err(invalid("retained receipt is missing its domain source"));
    }
    data.state.accept(&record)?;
    collect(data, record);
    Ok(())
}

#[cfg(unix)]
fn collect(data: &mut JournalData, record: JournalRecord) {
    data.history.positions.push(record.seq);
    data.history.push(&record);
    if let JournalFact::Domain { event, .. } = record.fact {
        data.events.push(event);
    }
}

#[cfg(all(test, unix))]
const CRASH_STAGE_ENV: &str = "GROW_TEST_WORKING_HISTORY_CRASH_STAGE";
#[cfg(all(test, unix))]
const CRASH_HOME_ENV: &str = "GROW_TEST_WORKING_HISTORY_CRASH_HOME";

#[cfg(all(test, unix))]
fn publication_crash_probe(stage: &str) {
    if std::env::var_os(CRASH_HOME_ENV).is_some()
        && std::env::var(CRASH_STAGE_ENV).as_deref() == Ok(stage)
    {
        std::process::exit(73);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::session::{info::Info, persistence::default_model_id, storage::StorageAdapter};

    include!("working_snapshot/large_publication_test.rs");
    include!("working_snapshot/timeline_catalog_test.rs");
    include!("working_snapshot/timeline_suffix_test.rs");
    mod branch_publication {
        use super::*;
        include!("working_snapshot/branch_publication_test.rs");
    }

    const TEST: &str = "session::storage::jsonl::working_snapshot::tests::process_exit_around_index_publication_recovers_exact_tail";
    const ADVANCE_TEST: &str = "session::storage::jsonl::working_snapshot::tests::process_exit_around_frontier_advance_preserves_domain_sequence";

    #[test]
    fn process_exit_around_frontier_advance_preserves_domain_sequence() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let fixture = |path: &Path| {
            let storage = JsonlStorageAdapter::with_root(path.to_path_buf());
            let info = Info {
                id: acp::SessionId::new("advance-crash"),
                cwd: path.to_string_lossy().into_owned(),
            };
            (storage, info)
        };
        if let Some(root) = std::env::var_os(CRASH_HOME_ENV) {
            runtime.block_on(async {
                let (storage, info) = fixture(Path::new(&root));
                storage
                    .init_session(&info, default_model_id())
                    .await
                    .unwrap();
                let mut timeline = chat_state::Timeline::default();
                let seed = timeline
                    .append(
                        sampling_types::ConversationItem::system("fixed"),
                        chat_state::MessageCause::Seed,
                    )
                    .unwrap();
                storage
                    .append_timeline_event_durable(&info, &seed)
                    .await
                    .unwrap();
                let request = timeline
                    .record(chat_state::TimelineEventKind::Runtime(
                        chat_state::session_runtime::SessionRuntimeEvent::SuspendRequested {
                            operation_id: "suspend".into(),
                            root_session_id: info.id.to_string(),
                        },
                    ))
                    .unwrap();
                storage
                    .append_timeline_event_durable(&info, &request)
                    .await
                    .unwrap();
                let mut large = diagnostic();
                let chat_state::TimelineEventKind::Observation(observation) = &mut large else {
                    unreachable!()
                };
                observation.data = Some(serde_json::Value::String(
                    "x".repeat(MAX_UNPUBLISHED_BYTES as usize),
                ));
                let archived = timeline.record(large).unwrap();
                storage
                    .append_timeline_event(&info, &archived)
                    .await
                    .unwrap();
            });
            panic!("incremental publication crash probe was not reached");
        }
        for stage in ["advance-before", "advance-after"] {
            let root = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", ADVANCE_TEST, "--nocapture"])
                .env(CRASH_HOME_ENV, root.path())
                .env(CRASH_STAGE_ENV, stage)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(73), "{stage}");
            runtime.block_on(async {
                let (storage, info) = fixture(root.path());
                let directory = storage.open_session(&info).unwrap().directory_handle();
                let prior = read_manifest(&directory).unwrap().unwrap();
                assert_eq!(prior.base_bytes, 0);
                assert_eq!(prior.bytes != 0, stage == "advance-after");
                assert_eq!(prior.retained_records, 0);
                let mut journal = std::fs::OpenOptions::new()
                    .append(true)
                    .open(directory.display_path().join(TIMELINE_FILE))
                    .unwrap();
                journal.write_all(b"{\"torn\":").unwrap();
                journal.sync_all().unwrap();
                let loaded = storage
                    .load_session_for_write_with_indexed_history(&info)
                    .await
                    .unwrap();
                assert_eq!(
                    loaded.history.indexed.as_ref().unwrap().manifest.generation,
                    prior.generation,
                    "first resume must use the normal writer's publication"
                );
                assert_eq!(
                    loaded.timeline.events().len(),
                    if stage == "advance-after" { 0 } else { 3 }
                );
                let exact = read_domain_range(&directory, 0..3).unwrap().unwrap();
                assert_eq!(
                    exact
                        .iter()
                        .map(|event| event.seq.get())
                        .collect::<Vec<_>>(),
                    [0, 1, 2]
                );
                assert_eq!(loaded.timeline.next_seq().get(), 3);
                assert_eq!(
                    loaded.timeline.runtime_lifecycle().phase,
                    chat_state::session_runtime::RuntimePhase::Suspending
                );
                storage
                    .append_timeline_event_durable(&info, &exact[2])
                    .await
                    .unwrap();
                let verified = storage.read_timeline_events_sync(&info).unwrap();
                assert_eq!(verified.len(), 3);
                assert_eq!(verified[2].seq.get(), 2);
                assert!(chat_state::Timeline::is_diagnostic(&verified[2]));
            });
        }
    }

    const TOOL_CRASH_TEST: &str = "session::storage::jsonl::working_snapshot::tests::process_exit_during_tool_checkpoint_preserves_open_state_and_identity";

    fn tool_start(call_id: &str) -> chat_state::TimelineEventKind {
        use chat_state::{StepId, TimelineEventKind, ToolEvent, TurnId};
        let turn = TurnId(1);
        TimelineEventKind::Tool(ToolEvent::Started {
            call_id: call_id.into(),
            turn,
            step: StepId { turn, index: 0 },
            name: "example".into(),
            input: None,
        })
    }

    #[test]
    fn process_exit_during_tool_checkpoint_preserves_open_state_and_identity() {
        use chat_state::{
            RequestEvent, StepEvent, StepId, TimelineEventKind, ToolEvent, TurnEvent, TurnId,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let fixture = |path: &Path| {
            (
                JsonlStorageAdapter::with_root(path.to_path_buf()),
                Info {
                    id: acp::SessionId::new("tool-crash"),
                    cwd: path.to_string_lossy().into_owned(),
                },
            )
        };
        if let Some(root) = std::env::var_os(CRASH_HOME_ENV) {
            runtime.block_on(async {
                let (storage, info) = fixture(Path::new(&root));
                storage.init_session(&info, default_model_id()).await.unwrap();
                let turn = TurnId(1);
                let step = StepId { turn, index: 0 };
                let mut timeline = chat_state::Timeline::default();
                for kind in [
                    TimelineEventKind::Turn(TurnEvent::Started {
                        id: turn,
                        prompt_id: "fixture".into(),
                        input_ids: vec![],
                        identity: chat_state::TurnIdentity {
                            origin: "user".into(), turn_kind: "internal".into(),
                            goal_id: None, goal_definition_revision: None, stage_id: None,
                        },
                        model_id: "mock".into(),
                        input_message_count: 0,
                        prompt_index: 0,
                        prompt_text: "fixture".into(),
                        input_kind: chat_state::TurnInputKind::Prompt,
                        redirect_kind: None,
                    }),
                    TimelineEventKind::Step(StepEvent::Started { id: step }),
                    tool_start("closed"),
                    tool_start("open"),
                    TimelineEventKind::Request(RequestEvent::Started {
                        id: "closed-request".into(), turn, step, model_id: "mock".into(),
                        input_message_count: 0, tool_count: 0,
                    }),
                    TimelineEventKind::Request(RequestEvent::Failed {
                        id: "closed-request".into(), duration_ms: 1, error_kind: "fixture".into(),
                        message: "terminal".into(), retryable: false,
                    }),
                    TimelineEventKind::Observation(chat_state::ObservationEvent {
                        scope: "sampling_usage".into(), name: "attempt_settled".into(), turn: None,
                        step: None, data: Some(serde_json::json!({
                            "attempt_key": "crash-attempt", "model_id": "mock", "captured_prompt_index": 0,
                            "usage": sampling_types::TokenUsage { prompt_tokens: 25, ..Default::default() },
                        })),
                    }),
                    TimelineEventKind::Tool(ToolEvent::Completed {
                        call_id: "closed".into(),
                        name: "example".into(),
                        outcome: "success".into(),
                        duration_ms: 1,
                        details: Some(serde_json::json!("x".repeat(MAX_UNPUBLISHED_BYTES as usize))),
                    }),
                ] {
                    let event = timeline.record(kind).unwrap();
                    storage.append_timeline_event(&info, &event).await.unwrap();
                }
            });
            panic!("Tool publication crash probe was not reached");
        }
        for stage in ["advance-before", "advance-after"] {
            let root = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TOOL_CRASH_TEST, "--nocapture"])
                .env(CRASH_HOME_ENV, root.path())
                .env(CRASH_STAGE_ENV, stage)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(73), "{stage}");
            runtime.block_on(async {
                let (storage, info) = fixture(root.path());
                let directory = storage.open_session(&info).unwrap().directory_handle();
                let before = read_manifest(&directory).unwrap().unwrap();
                assert_eq!(before.tool_frontier.domain == 8, stage == "advance-after");
                let mut file = directory
                    .open_read_write_existing(OsStr::new(TIMELINE_FILE), "journal")
                    .unwrap();
                use std::io::Seek;
                file.seek(std::io::SeekFrom::End(0)).unwrap();
                file.write_all(b"{torn").unwrap();
                file.sync_all().unwrap();

                let opened = storage.open_session(&info).unwrap();
                let loaded = storage.load_recovery_writer(&opened).await.unwrap();
                assert_eq!(loaded.execution_start_counts(), (1, 2));
                let timeline = loaded.timeline;
                assert_eq!(timeline.next_seq().get(), 8);
                assert_eq!(
                    chat_state::actor::state::project_timeline_usage(&timeline)
                        .unwrap()
                        .totals
                        .input_tokens,
                    25
                );
                assert_eq!(timeline.open_tool_call_ids().collect::<Vec<_>>(), ["open"]);
                assert!(matches!(
                    timeline.prepare(tool_start("closed")),
                    Err(chat_state::TimelineError::ToolAlreadyOpen(_))
                ));
                assert!(timeline.prepare(tool_start("fresh")).is_ok());
                assert!(
                    timeline
                        .prepare(TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                            occurrence_id: "late-tool-hook".into(),
                            event: chat_state::HookEventType::PostToolUse,
                            gate: chat_state::HookEventType::PostToolUse.gate(),
                            cause: chat_state::HookCause::Tool {
                                call_id: "closed".into()
                            },
                            config_generation: 0,
                            handlers: Vec::new(),
                        }))
                        .is_ok()
                );
                assert_eq!(
                    timeline.event(chat_state::EventSeq::new(2)).is_none(),
                    stage == "advance-after"
                );
                let exact = read_domain_range(&directory, 2..8).unwrap().unwrap();
                assert!(matches!(
                    timeline.prepare(exact[2].kind.clone()),
                    Err(chat_state::TimelineError::RequestAlreadyOpen(_))
                ));
                storage
                    .append_timeline_event_durable(&info, exact.last().unwrap())
                    .await
                    .unwrap();
                assert_eq!(storage.read_timeline_events_sync(&info).unwrap().len(), 8);
                if stage == "advance-after" {
                    let index = loaded.history.indexed.as_ref().unwrap();
                    let mut suffix = directory
                        .open_regular(
                            OsStr::new(&suffix_index_name(index.manifest.generation)),
                            "domain suffix",
                        )
                        .unwrap();
                    let reference =
                        working_index::read_ref(&mut suffix, 2 - index.manifest.base_domain_events)
                            .unwrap();
                    file.seek(io::SeekFrom::Start(reference.offset)).unwrap();
                    file.write_all(b"X").unwrap();
                    assert!(matches!(
                        timeline.prepare(tool_start("closed")),
                        Err(chat_state::TimelineError::ToolHistory(_))
                    ));
                }
            });
        }
    }

    fn diagnostic() -> chat_state::TimelineEventKind {
        chat_state::TimelineEventKind::Observation(chat_state::ObservationEvent {
            scope: "shell".into(),
            name: "artifact".into(),
            turn: None,
            step: None,
            data: Some(serde_json::Value::String("data".repeat(1024))),
        })
    }

    #[tokio::test]
    async fn indexed_recap_keeps_frozen_coverage_without_retaining_older_bodies() {
        use crate::extensions::notification::{SessionNotification, SessionUpdate as GrowUpdate};
        use crate::session::storage::SessionUpdate;

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let recap = |update, coverage: Option<usize>| {
            SessionUpdate::Grow(Box::new(SessionNotification {
                session_id: info.id.clone(),
                update,
                meta: coverage.map(|turns| serde_json::json!({"recapCoveredMainTurns": turns})),
            }))
        };
        let success = |n, coverage| {
            recap(
                GrowUpdate::SessionRecap {
                    summary: format!("recap-{n}"),
                    auto: false,
                    away_period_id: None,
                },
                coverage,
            )
        };
        for n in 1..=128 {
            storage
                .append_update(&info, &success(n, Some(n)))
                .await
                .unwrap();
        }
        let mut timeline = chat_state::Timeline::default();
        let event = timeline.record(observation("recap-publication")).unwrap();
        storage.append_timeline_event(&info, &event).await.unwrap();
        storage
            .append_update(&info, &success(129, None))
            .await
            .unwrap();
        storage
            .append_update(&info, &recap(GrowUpdate::SessionRecapUnavailable, None))
            .await
            .unwrap();
        drop(storage);

        let (storage, _) = fixture(root.path());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let index = loaded.history.indexed.as_ref().unwrap();
        assert_eq!(index.recap_covered_main_turns(), Some(128));
        assert_eq!(index.manifest.retained_records, 0);
        let prepared = loaded
            .history
            .prepare_indexed(&info.id, &loaded.timeline)
            .unwrap();
        let pages = loaded.history.indexed_pages(prepared).unwrap().unwrap();
        let (_, rows) = pages
            .page_before(&info.id, crate::session::history::PagePosition::newest())
            .unwrap();
        let summaries: Vec<_> = rows
            .iter()
            .filter_map(|row| match row {
                SessionUpdate::Grow(notice) => match &notice.update {
                    GrowUpdate::SessionRecap { summary, .. } => Some(summary.as_str()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert!(summaries.ends_with(&["recap-127", "recap-128", "recap-129"]));
    }

    #[tokio::test]
    async fn published_nonpage_presentation_retries_preserve_exact_identity() {
        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let retained_update = |text: &str| {
            SessionUpdate::Acp(Box::new(
                acp::SessionNotification::new(
                    info.id.clone(),
                    acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                        acp::ContentBlock::Text(acp::TextContent::new(text)),
                    )),
                )
                .meta(Some(acp::Meta::from_iter([(
                    "eventId".into(),
                    "retained-example".into(),
                )]))),
            ))
        };
        let update = retained_update("original");
        storage
            .append_update_durable_commit_aware(&info, &update)
            .await
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        let publish = timeline
            .record(observation("publish-all-presentation-identities"))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &publish)
            .await
            .unwrap();
        let directory = storage.open_session(&info).unwrap().directory_handle();
        let path = directory.display_path().join(TIMELINE_FILE);
        let bytes = std::fs::metadata(&path).unwrap().len();
        assert_eq!(read_manifest(&directory).unwrap().unwrap().bytes, bytes);
        storage
            .append_update_durable_commit_aware(&info, &update)
            .await
            .unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), bytes);
        drop(storage);
        let storage = JsonlStorageAdapter::with_root(root.path().to_owned());
        storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        storage
            .append_update_durable_commit_aware(&info, &update)
            .await
            .unwrap();
        assert!(
            storage
                .append_update_durable_commit_aware(&info, &retained_update("changed"))
                .await
                .is_err()
        );
        assert_eq!(std::fs::metadata(&path).unwrap().len(), bytes);
    }

    #[tokio::test]
    async fn consultation_checkpoints_drop_settled_history_and_restore_an_uninjected_answer() {
        use crate::coordination::{
            IncomingInquiryAudit, InquiryAnswerContextProof, InquiryDirection, InquiryEvent,
            InquiryOutcome,
        };
        use chat_state::{
            NotificationEvent, NotificationSource, NotificationSourceVersion,
            SidebandAssemblyManifest, SidebandAttempt, SidebandBudgetPolicy, SidebandEnd,
            SidebandEventKind, SidebandOutcome, SidebandPurpose, SidebandRequest, SidebandResult,
            SidebandRoute, SidebandSpawnEvent, SidebandTimeline, TimelineRangeRef,
        };

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut dense = chat_state::Timeline::default();
        let owner = info.id.to_string();
        let turn = TurnId(1);
        let turn_start = dense.record(turn_start(turn, "current", 0, 0)).unwrap();
        storage
            .append_timeline_event(&info, &turn_start)
            .await
            .unwrap();
        let mut last = None;
        for n in 0..8 {
            let inquiry_id = format!("consult-{n}");
            let sideband_id = uuid::Uuid::from_u128(n as u128 + 1).to_string();
            let sideband = |seq, kind| chat_state::SidebandEvent {
                sideband_id: sideband_id.clone(),
                seq,
                at_ms: 1,
                kind,
                version: chat_state::SIDEBAND_SCHEMA_VERSION,
            };
            let mut audit = IncomingInquiryAudit {
                direction: InquiryDirection::ParentToChild,
                source_peer_id: "peer".into(),
                source_session_id: "sender".into(),
                source_cwd: "/work".into(),
                delegated_subagent_task_name: None,
                question: format!("question {n}"),
                approval: None,
                outcome: None,
                answer_context: None,
            };
            let started = dense
                .record(
                    InquiryEvent::Incoming {
                        inquiry_id: inquiry_id.clone(),
                        audit: audit.clone(),
                    }
                    .timeline_kind(),
                )
                .unwrap();
            storage
                .append_timeline_event(&info, &started)
                .await
                .unwrap();
            let input_ref = TimelineRangeRef {
                timeline_id: owner.clone(),
                first_seq: 0,
                last_seq: 0,
            };
            let spawn = dense
                .record(TimelineEventKind::Sideband(SidebandSpawnEvent {
                    sideband_id: sideband_id.clone(),
                    purpose: SidebandPurpose::InfoRequest,
                    source_refs: vec![input_ref.clone()],
                }))
                .unwrap();
            storage.append_timeline_event(&info, &spawn).await.unwrap();
            let answer = format!("answer {n}");
            let events = vec![
                sideband(
                    0,
                    SidebandEventKind::Request(SidebandRequest {
                        purpose: SidebandPurpose::InfoRequest,
                        prompt: crate::coordination::inquiry_prompt(
                            "sender",
                            "/work",
                            &audit.question,
                        ),
                        source_refs: vec![input_ref.clone()],
                        budget_policy: SidebandBudgetPolicy {
                            max_attempts: 1,
                            max_input_tokens_per_attempt: 8,
                            max_output_tokens_per_attempt: None,
                        },
                        route: SidebandRoute {
                            model: "mock/mock".into(),
                            backend: sampling_types::ApiBackend::Messages,
                        },
                        initiator_ref: format!("t:{owner}/sideband:{sideband_id}"),
                        executor: "test".into(),
                        output_schema: None,
                    }),
                ),
                sideband(
                    1,
                    SidebandEventKind::Attempt(SidebandAttempt {
                        attempt_no: 1,
                        input_refs: vec![input_ref.clone()],
                        assembly_manifest: SidebandAssemblyManifest {
                            strategy: "consultation".into(),
                            strategy_version: 1,
                            source_revision: Some(0),
                            context_surface_ids: Vec::new(),
                            selected_surface_ids: Vec::new(),
                            materialized_input_tokens: 1,
                            max_output_tokens: None,
                        },
                        feedback: None,
                    }),
                ),
                sideband(
                    2,
                    SidebandEventKind::Result(SidebandResult {
                        raw_output: answer.clone(),
                        structured_output: None,
                        usage: Default::default(),
                        finish: "end_turn".into(),
                        source_event_seqs: [0, 1],
                        evidence_refs: vec![input_ref],
                    }),
                ),
                sideband(
                    3,
                    SidebandEventKind::End(SidebandEnd {
                        outcome: SidebandOutcome::Completed,
                        error: None,
                    }),
                ),
            ];
            SidebandTimeline::from_events(events.clone()).unwrap();
            for event in &events {
                storage
                    .append_sideband_event_durable(&info, event)
                    .await
                    .unwrap();
            }
            audit.outcome = Some(InquiryOutcome::answered(&inquiry_id, answer));
            audit.answer_context = Some(InquiryAnswerContextProof {
                target_session_id: owner.clone(),
                result_ref: TimelineRangeRef {
                    timeline_id: sideband_id.clone(),
                    first_seq: 2,
                    last_seq: 2,
                },
            });
            let terminal = dense
                .record(
                    InquiryEvent::Incoming {
                        inquiry_id: inquiry_id.clone(),
                        audit: audit.clone(),
                    }
                    .timeline_kind(),
                )
                .unwrap();
            storage
                .append_timeline_event(&info, &terminal)
                .await
                .unwrap();
            let body = audit.answer_context_body().unwrap();
            let source = NotificationSource::InquiryAnswered {
                source_session_id: "sender".into(),
                inquiry_id: inquiry_id.clone(),
                continue_if_idle: false,
            };
            let source_version = NotificationSourceVersion::Opaque {
                value: format!("{owner}:{}", terminal.seq.get()),
            };
            let id = chat_state::notification_id(&owner, &source, &source_version).unwrap();
            let payload_ref = chat_state::NotificationPayloadRef {
                bytes: body.len() as u64,
                blake3: blake3::hash(body.as_bytes()).to_hex().to_string(),
            };
            let receipt = NotificationEvent::Received {
                id: id.clone(),
                owner_session_id: owner.clone(),
                source,
                source_version,
                payload_ref,
            };
            if n == 7 {
                last = Some((inquiry_id, terminal, receipt));
                break;
            }
            let received = dense
                .record(TimelineEventKind::Notification(receipt))
                .unwrap();
            let consumed = dense
                .record(TimelineEventKind::Notification(
                    NotificationEvent::Consumed {
                        notification_ids: vec![id.clone()],
                        turn,
                        input: Some(sampling_types::ConversationItem::received_agent_message(
                            sampling_types::AgentMessage {
                                receipt_id: id,
                                source_session_id: "sender".into(),
                                target_session_id: owner.clone(),
                                message_id: inquiry_id,
                                reply_to: None,
                                message: body.into(),
                            },
                        )),
                    },
                ))
                .unwrap();
            storage
                .append_timeline_event(&info, &received)
                .await
                .unwrap();
            storage
                .append_timeline_event(&info, &consumed)
                .await
                .unwrap();
        }
        let (inquiry, terminal, receipt) = last.unwrap();
        let publish = dense.record(observation("consultation-cut")).unwrap();
        storage
            .append_timeline_event_durable(&info, &publish)
            .await
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_owned());
        let restored = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            restored
                .timeline
                .events()
                .iter()
                .filter(|event| matches!(
                    InquiryEvent::from_timeline(event).unwrap(),
                    Some(InquiryEvent::Incoming { .. })
                ))
                .map(|event| event.seq)
                .collect::<Vec<_>>(),
            [terminal.seq]
        );
        assert_eq!(
            restored
                .timeline
                .incoming_inquiry_events("sender", "consult-0")
                .unwrap()
                .len(),
            2
        );
        let received = dense
            .record(TimelineEventKind::Notification(receipt))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &received)
            .await
            .unwrap();
        drop(restored);
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_owned());
        let restored = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let pending = restored.timeline.pending_notifications();
        assert_eq!(pending.len(), 1);
        assert!(
            matches!(&pending[0].source, NotificationSource::InquiryAnswered { inquiry_id, .. } if inquiry_id == &inquiry)
        );
    }

    #[tokio::test]
    async fn indexed_agent_messages_keep_receipts_roots_and_anchors_across_publication() {
        use crate::extensions::notification::{SessionNotification, SessionUpdate as GrowUpdate};
        use chat_state::{NotificationEvent, NotificationSource, NotificationSourceVersion};

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let directory = storage.open_session(&info).unwrap().directory_handle();
        let mut dense = chat_state::Timeline::default();
        let turn = TurnId(1);
        dense.record(turn_start(turn, "current", 0, 0)).unwrap();
        let version = NotificationSourceVersion::Ordinal {
            value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
        };
        let mut messages = Vec::new();
        let mut anchors = Vec::new();
        for n in 0..128 {
            let body = format!("message {n}");
            let source = NotificationSource::ParentMessage {
                parent_session_id: "parent".into(),
                message_id: n.to_string(),
                interrupt: false,
            };
            let payload_ref =
                crate::session::notification_inbox::write_payload(&directory, &body).unwrap();
            let owner = info.id.to_string();
            let id = chat_state::notification_id(&owner, &source, &version).unwrap();
            let received = dense
                .record(TimelineEventKind::Notification(
                    NotificationEvent::Received {
                        id: id.clone(),
                        owner_session_id: owner,
                        source: source.clone(),
                        source_version: version.clone(),
                        payload_ref: payload_ref.clone(),
                    },
                ))
                .unwrap();
            dense
                .record(TimelineEventKind::Notification(
                    NotificationEvent::Consumed {
                        notification_ids: vec![id],
                        turn,
                        input: None,
                    },
                ))
                .unwrap();
            let n = if n % 2 == 0 || n == 127 {
                Some(n)
            } else {
                None
            };
            if let Some(n) = n {
                let notice = crate::session::notification_inbox::parent_message_notice(
                    &received,
                    Some(body),
                )
                .unwrap();
                anchors.push((
                    received.seq,
                    n,
                    SessionUpdate::Grow(Box::new(SessionNotification {
                        session_id: info.id.clone(),
                        update: GrowUpdate::UiNotice(notice),
                        meta: Some(serde_json::json!({"eventId": format!("anchored-{n}")})),
                    })),
                ));
            }
            messages.push((received, source, payload_ref));
        }
        let publication = dense.record(observation("publish-agent-messages")).unwrap();
        for event in dense.events() {
            storage.append_timeline_event(&info, event).await.unwrap();
            if let Some((_, _, anchor)) = anchors
                .iter()
                .find(|(seq, n, _)| *seq == event.seq && *n != 127)
            {
                storage
                    .append_update_durable_commit_aware(&info, anchor)
                    .await
                    .unwrap();
            }
        }
        let cut = read_manifest(&directory).unwrap().unwrap();
        assert!(cut.domain_events > publication.seq.get());
        assert_eq!(cut.retained_records, 0);
        // This anchor refers back across the checkpoint. Both live append and
        // suffix recovery must verify the original attribution.
        storage
            .append_update_durable_commit_aware(&info, &anchors.last().unwrap().2)
            .await
            .unwrap();
        let dense_snapshot = storage
            .open_session(&info)
            .unwrap()
            .validated_timeline(info.id.0.as_ref())
            .unwrap();
        let expected = dense_snapshot
            .history
            .prepare(&info.id, &dense_snapshot.timeline, None)
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_owned());
        let restored = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert!(restored.timeline.pending_notifications().is_empty());
        assert!(
            restored
                .timeline
                .events()
                .iter()
                .all(|event| !matches!(event.kind, TimelineEventKind::Notification(_)))
        );
        let (first, first_source, first_payload) = &messages[0];
        let exact = restored
            .timeline
            .exact_agent_message_receipts(Some(("parent", "0")))
            .unwrap();
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].seq, first.seq);
        assert_eq!(
            restored
                .timeline
                .exact_notification_consumer(first_source, &version)
                .unwrap(),
            Some(turn)
        );
        let index = restored.history.indexed.as_ref().unwrap();
        let history = index.tools.cut(index.clone()).unwrap();
        let roots = chat_state::timeline::ExecutionHistory::notification_payload_hashes(
            &history,
            Some(&[first_payload.blake3.clone(), "unused".into()]),
            &|| false,
        )
        .unwrap();
        assert_eq!(
            roots,
            std::collections::BTreeSet::from([first_payload.blake3.clone()])
        );
        let prepared = restored
            .history
            .prepare_indexed(&info.id, &restored.timeline)
            .unwrap();
        let pages = restored.history.indexed_pages(prepared).unwrap().unwrap();
        let mut before = history::PagePosition::newest();
        let mut captured = Vec::new();
        loop {
            let (older, rows) = pages.page_before(&info.id, before).unwrap();
            captured.splice(..0, rows);
            let Some(older) = older else { break };
            assert_ne!(older, before);
            before = older;
        }
        let notices = |rows: Vec<SessionUpdate>| {
            rows.into_iter()
                .filter_map(|row| match row {
                    SessionUpdate::Grow(value)
                        if matches!(value.update, GrowUpdate::UiNotice(_)) =>
                    {
                        Some((
                            value.meta.as_ref().unwrap()["eventId"].clone(),
                            serde_json::to_value(value.update).unwrap(),
                        ))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(notices(captured), notices(expected.updates));
    }

    #[tokio::test]
    async fn indexed_terminal_notices_stay_out_of_cold_execution() {
        use crate::extensions::notification::{
            RetryState, SessionNotification, SessionUpdate as GrowUpdate,
        };

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        for ordinal in 0..132 {
            let payload = format!("terminal-{ordinal}");
            let mut meta = serde_json::json!({"eventId": payload});
            let update = match ordinal % 4 {
                0 => GrowUpdate::RetryState(RetryState::Failed {
                    error_type: "server".into(),
                    message: payload,
                }),
                1 => GrowUpdate::AutoCompactFailed { error: payload },
                2 => GrowUpdate::MemoryDreamCompleted {
                    result: payload,
                    path: None,
                },
                _ => {
                    let mut goal = crate::session::goal_notification::build_goal_cleared();
                    if let GrowUpdate::GoalUpdated { objective, .. } = &mut goal {
                        *objective = payload;
                    }
                    meta["grow/displayProjectionSource"] = "goal:cleared".into();
                    meta["grow/displayProjectionRevision"] = ordinal.into();
                    goal
                }
            };
            let update = SessionUpdate::Grow(Box::new(SessionNotification {
                session_id: info.id.clone(),
                update,
                meta: Some(meta),
            }));
            storage.append_update(&info, &update).await.unwrap();
        }
        let publication = chat_state::Timeline::default()
            .record(observation("terminal-notices-publication"))
            .unwrap();
        storage
            .append_timeline_event(&info, &publication)
            .await
            .unwrap();
        drop(storage);

        let (storage, _) = fixture(root.path());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .manifest
                .retained_records,
            0
        );
        assert!(loaded.history.sources.is_empty());
        let prepared = loaded
            .history
            .prepare_indexed(&info.id, &loaded.timeline)
            .unwrap();
        let pages = loaded.history.indexed_pages(prepared).unwrap().unwrap();
        let (older, recent) = pages
            .page_before(&info.id, crate::session::history::PagePosition::newest())
            .unwrap();
        let (end, older) = pages.page_before(&info.id, older.unwrap()).unwrap();
        assert!(end.is_none());
        let notices: Vec<_> = older
            .into_iter()
            .chain(recent)
            .map(|row| {
                let SessionUpdate::Grow(row) = row else {
                    panic!("expected a terminal notice")
                };
                let body = match row.update {
                    GrowUpdate::RetryState(RetryState::Failed { message, .. }) => message,
                    GrowUpdate::AutoCompactFailed { error } => error,
                    GrowUpdate::MemoryDreamCompleted { result, .. } => result,
                    GrowUpdate::GoalUpdated { objective, .. } => objective,
                    _ => panic!("unexpected Grow update"),
                };
                assert_eq!(row.meta.unwrap()["eventId"], body);
                body
            })
            .collect();
        assert_eq!(
            notices,
            (0..132)
                .map(|ordinal| format!("terminal-{ordinal}"))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn outgoing_inquiries_keep_exact_history_and_merge_a_tail_completion() {
        use crate::coordination::{InquiryAudit, InquiryEvent, InquiryOutcome};

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let start = |id: String| {
            InquiryEvent::OutgoingStarted {
                question: format!("question-{id}"),
                target_session_id: "target".into(),
                inquiry_id: id,
            }
            .timeline_kind()
        };
        let finish = |id: String| {
            InquiryEvent::OutgoingCompleted {
                audit: InquiryAudit {
                    question: format!("question-{id}"),
                    target_session_id: "target".into(),
                    outcome: InquiryOutcome::answered(id, "answer".into()),
                },
            }
            .timeline_kind()
        };
        let mut dense = chat_state::Timeline::default();
        for ordinal in 0..128 {
            let id = format!("closed-{ordinal}");
            for kind in [start(id.clone()), finish(id)] {
                let event = dense.record(kind).unwrap();
                storage.append_timeline_event(&info, &event).await.unwrap();
            }
        }
        let event = dense.record(start("tail".into())).unwrap();
        storage.append_timeline_event(&info, &event).await.unwrap();
        let event = dense.record(observation("inquiry-publication")).unwrap();
        storage.append_timeline_event(&info, &event).await.unwrap();
        let event = dense.record(finish("tail".into())).unwrap();
        storage
            .append_timeline_event_durable(&info, &event)
            .await
            .unwrap();
        drop(storage);

        let (storage, _) = fixture(root.path());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .manifest
                .retained_records,
            0
        );
        assert_eq!(loaded.timeline.events().len(), 1);
        for id in ["closed-0", "closed-127", "tail", "absent"] {
            let actual = loaded.timeline.outgoing_inquiry_events(id).unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(dense.outgoing_inquiry_events(id).unwrap()).unwrap(),
            );
        }
        let tail = loaded.timeline.outgoing_inquiry_events("tail").unwrap();
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].seq.get(), 256);
        assert_eq!(tail[1].seq.get(), 258);
        let mut conflicting = dense.outgoing_inquiry_events("closed-0").unwrap()[1].clone();
        let mut fact = InquiryEvent::from_timeline(&conflicting).unwrap().unwrap();
        if let InquiryEvent::OutgoingCompleted { audit } = &mut fact {
            audit.question = "conflicting retry".into();
        }
        conflicting.kind = fact.timeline_kind();
        let conflicting = dense.record(conflicting.kind).unwrap();
        storage
            .append_timeline_event_durable(&info, &conflicting)
            .await
            .unwrap();
        let publication = dense
            .record(observation("publish-duplicate-inquiry-terminal"))
            .unwrap();
        storage
            .append_timeline_event(&info, &publication)
            .await
            .unwrap();
        drop(loaded);
        drop(storage);
        let (storage, _) = fixture(root.path());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let lookup = loaded.timeline.outgoing_inquiry_events("closed-0").unwrap();
        assert_eq!(lookup.len(), 3);
        assert_eq!(lookup[2].seq, conflicting.seq);
        assert!(matches!(
            crate::coordination::CoordinationHandle::resolve_durable_inquiry("closed-0", &lookup),
            Ok(Some(InquiryEvent::OutgoingCompleted { audit }))
                if audit.question == "question-closed-0"
        ));
    }

    #[tokio::test]
    async fn image_projection_publishes_exact_surface_and_keeps_original_evidence() {
        use chat_state::{ImageProjectionEvent, ImageShadow, ImageShadowSource};
        use sampling_types::conversation::{
            UNSUPPORTED_IMAGE_REPLACEMENT, conversation_image_groups,
        };
        use sampling_types::{ContentPart, ConversationItem, ModelImageInputKey};

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut dense =
            chat_state::Timeline::from_seed(vec![ConversationItem::user_with_parts(vec![
                ContentPart::Image {
                    url: "data:image/png;base64,original".into(),
                    description: None,
                },
            ])])
            .unwrap();
        let original = dense.events()[0].clone();
        storage
            .append_timeline_event(&info, &original)
            .await
            .unwrap();
        let group = conversation_image_groups(dense.surface()).remove(0);
        let projection = dense
            .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: ModelImageInputKey::new("mock", "messages", "endpoint"),
                source_revision: dense.surface_revision(),
                shadows: vec![ImageShadow {
                    source: dense.surface_ids()[0],
                    fingerprint: group.fingerprint,
                    image_count: 1,
                    replacement: UNSUPPORTED_IMAGE_REPLACEMENT.into(),
                    provenance: ImageShadowSource::UnsupportedModel,
                }],
                tool_calls: vec![],
            }))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &projection)
            .await
            .unwrap();
        let tail = dense
            .append(ConversationItem::user("fixed tail"), MessageCause::User)
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &tail)
            .await
            .unwrap();
        drop(storage);

        let (storage, _) = fixture(root.path());
        let recovered = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert!(recovered.timeline.has_surface_checkpoint());
        assert_eq!(
            recovered
                .timeline
                .events()
                .iter()
                .map(|event| event.seq)
                .collect::<Vec<_>>(),
            [tail.seq]
        );
        assert_eq!(recovered.timeline.surface_ids(), dense.surface_ids());
        assert_eq!(
            recovered.timeline.surface_revision(),
            dense.surface_revision()
        );
        assert_eq!(
            serde_json::to_value(recovered.timeline.surface()).unwrap(),
            serde_json::to_value(dense.surface()).unwrap()
        );
        let source = recovered
            .timeline
            .exact_event(original.seq)
            .unwrap()
            .unwrap();
        assert_eq!(
            conversation_image_groups(&source.messages().unwrap().items).len(),
            1
        );
        assert!(conversation_image_groups(recovered.timeline.surface()).is_empty());
        let manifest = &recovered.history.indexed.as_ref().unwrap().manifest;
        assert_eq!(manifest.retained_records, 0);
        assert_eq!(manifest.domain_events, projection.seq.get() + 1);
    }

    #[tokio::test]
    async fn indexed_image_ownership_reads_only_referenced_domain_records() {
        use chat_state::{EventSeq, MessageCause, TimelineEventKind};
        use sampling_types::ConversationItem;

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline =
            chat_state::Timeline::from_seed(vec![ConversationItem::system("fixed")]).unwrap();
        storage
            .append_timeline_event(&info, &timeline.events()[0])
            .await
            .unwrap();
        let archived = "assets/images-published/image-1.png";
        let suffix = "assets/images-suffix/image-1.png";
        let user =
            |path| ConversationItem::user(format!("<image_files>\n1. {path}\n</image_files>"));
        let original = timeline.append(user(archived), MessageCause::User).unwrap();
        storage
            .append_timeline_event(&info, &original)
            .await
            .unwrap();
        let publication = timeline.record(observation("asset-publication")).unwrap();
        storage
            .append_timeline_event(&info, &publication)
            .await
            .unwrap();
        let after = timeline.append(user(suffix), MessageCause::User).unwrap();
        storage
            .append_timeline_event_durable(&info, &after)
            .await
            .unwrap();
        drop(storage);

        let (storage, _) = fixture(root.path());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let index = loaded.history.indexed.as_ref().unwrap();
        let cut = index.tools.cut(index.clone()).unwrap();
        use chat_state::timeline::ExecutionHistory;
        assert_eq!(
            cut.user_image_paths(&|| false).unwrap(),
            [archived.to_owned()].into()
        );
        assert_eq!(
            chat_state::Timeline::user_image_paths(loaded.timeline.events()),
            [suffix.to_owned()].into()
        );
        assert_eq!(
            cut.user_image_paths(&|| true).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        let root_event = index.domain_events(1..2).unwrap().pop().unwrap();
        assert_eq!(root_event.seq, EventSeq::new(1));
        assert!(matches!(root_event.kind, TimelineEventKind::Messages(_)));
        let directory = storage.bound_session_directory(&info).unwrap();
        let mut file = directory
            .open_read_write_existing(OsStr::new(TIMELINE_FILE), "test journal")
            .unwrap();
        use std::io::Seek;
        use std::io::Write;
        let mut refs = directory
            .open_regular(
                OsStr::new(&suffix_index_name(index.manifest.generation)),
                "test suffix",
            )
            .unwrap();
        let record =
            working_index::read_ref(&mut refs, 1 - index.manifest.base_domain_events).unwrap();
        file.seek(io::SeekFrom::Start(record.offset)).unwrap();
        file.write_all(b"X").unwrap();
        assert!(cut.user_image_paths(&|| false).is_err());
    }

    #[tokio::test]
    async fn request_cut_preserves_exact_admission_and_suffix_settlement() {
        use chat_state::{RequestEvent, TimelineError, TimelineEventKind as Kind};

        let root = tempfile::tempdir().unwrap();
        let info = Info {
            id: acp::SessionId::new("request-cut"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut dense = super::super::tests::response_projection_production_timeline_with_items(
            "closed",
            3,
            vec![sampling_types::ConversationItem::assistant("fixed")],
            0,
        );
        let open = match dense.events()[2].kind.clone() {
            Kind::Request(RequestEvent::Started {
                turn,
                step,
                model_id,
                ..
            }) => Kind::Request(RequestEvent::Started {
                id: "open".into(),
                turn,
                step,
                model_id,
                input_message_count: 1,
                tool_count: 0,
            }),
            _ => unreachable!(),
        };
        dense.record(open.clone()).unwrap();
        dense
            .record(Kind::Observation(chat_state::ObservationEvent {
                scope: "shell".into(),
                name: "publish".into(),
                turn: None,
                step: None,
                data: Some(serde_json::json!(
                    "x".repeat(MAX_UNPUBLISHED_BYTES as usize)
                )),
            }))
            .unwrap();
        for event in dense.events() {
            storage.append_timeline_event(&info, event).await.unwrap();
        }
        let directory = storage.open_session(&info).unwrap().directory_handle();
        let cut = read_manifest(&directory).unwrap().unwrap();
        assert_eq!(cut.domain_events, dense.next_seq().get());
        let terminal = dense
            .record(Kind::Request(RequestEvent::Failed {
                id: "open".into(),
                duration_ms: 2,
                error_kind: "fixture".into(),
                message: "request failed".into(),
                retryable: false,
            }))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &terminal)
            .await
            .unwrap();
        assert_eq!(read_manifest(&directory).unwrap().unwrap().bytes, cut.bytes);
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let restored = loaded.timeline;
        assert_eq!(restored.next_seq(), dense.next_seq());
        assert_eq!(restored.surface_ids(), dense.surface_ids());
        assert_eq!(
            serde_json::to_vec(restored.surface()).unwrap(),
            serde_json::to_vec(dense.surface()).unwrap()
        );
        assert!(restored.event(chat_state::EventSeq::new(2)).is_none());
        assert!(restored.event(chat_state::EventSeq::new(3)).is_none());
        assert_eq!(restored.open_request_ids().count(), 0);
        let duplicate = dense.events()[2].kind.clone();
        assert!(matches!(
            dense.prepare(duplicate.clone()),
            Err(TimelineError::RequestAlreadyOpen(_))
        ));
        assert!(matches!(
            restored.prepare(duplicate),
            Err(TimelineError::RequestAlreadyOpen(_))
        ));
        assert!(matches!(
            restored.prepare(open),
            Err(TimelineError::RequestAlreadyOpen(_))
        ));
        storage
            .append_timeline_event_durable(&info, &terminal)
            .await
            .unwrap();
        assert_eq!(
            storage.read_timeline_events_sync(&info).unwrap().len() as u64,
            dense.next_seq().get()
        );

        let mut suffix = directory
            .open_regular(
                OsStr::new(&suffix_index_name(cut.generation)),
                "domain suffix",
            )
            .unwrap();
        let closed_terminal =
            working_index::read_ref(&mut suffix, 3 - cut.base_domain_events).unwrap();
        let mut journal = directory
            .open_read_write_existing(OsStr::new(TIMELINE_FILE), "Timeline journal")
            .unwrap();
        use std::io::{Seek, SeekFrom};
        journal
            .seek(SeekFrom::Start(closed_terminal.offset))
            .unwrap();
        journal.write_all(b"X").unwrap();
        assert!(matches!(
            restored.prepare(dense.events()[2].kind.clone()),
            Err(TimelineError::RequestHistory(_))
        ));
    }

    #[tokio::test]
    async fn accounting_checkpoint_folds_only_suffix_and_checks_exact_retry() {
        use chat_state::actor::state::project_timeline_usage;
        use chat_state::{ObservationEvent, TimelineEventKind as Kind};

        let root = tempfile::tempdir().unwrap();
        let info = Info {
            id: acp::SessionId::new("usage-cut"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let settle = |tokens| {
            Kind::Observation(ObservationEvent {
                scope: "sampling_usage".into(),
                name: "attempt_settled".into(),
                turn: None,
                step: None,
                data: Some(serde_json::json!({
                    "attempt_key": "exact-attempt", "model_id": "mock", "captured_prompt_index": 0,
                    "usage": sampling_types::TokenUsage { prompt_tokens: tokens, ..Default::default() },
                    "cost_usd_ticks": 7, "api_duration_ms": 13,
                })),
            })
        };
        let mut dense = chat_state::Timeline::default();
        for kind in [
            settle(25),
            Kind::Observation(ObservationEvent {
                scope: "shell".into(),
                name: "publish accounting".into(),
                turn: None,
                step: None,
                data: Some(serde_json::json!(
                    "x".repeat(MAX_UNPUBLISHED_BYTES as usize)
                )),
            }),
            Kind::Observation(ObservationEvent {
                scope: "session_usage".into(),
                name: "resume_started".into(),
                turn: None,
                step: None,
                data: None,
            }),
            settle(25),
        ] {
            let event = dense.record(kind).unwrap();
            storage
                .append_timeline_event_durable(&info, &event)
                .await
                .unwrap();
        }
        drop(storage);
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert!(
            loaded
                .timeline
                .event(chat_state::EventSeq::new(0))
                .is_none()
        );
        assert_eq!(
            project_timeline_usage(&loaded.timeline).unwrap(),
            project_timeline_usage(&dense).unwrap()
        );
        let mut restored = loaded.timeline;
        restored.record(settle(26)).unwrap();
        assert!(matches!(
            project_timeline_usage(&restored),
            Err(chat_state::TimelineWriteError::AttemptUsageConflict)
        ));
    }

    #[tokio::test]
    async fn published_closed_requests_and_usage_do_not_expand_resident_replay() {
        use chat_state::{ObservationEvent, RequestEvent, TimelineEventKind as Kind};

        let mut resident_counts = Vec::new();
        for count in [1, 128] {
            let root = tempfile::tempdir().unwrap();
            let info = Info {
                id: acp::SessionId::new("closed-request-and-usage-scale"),
                cwd: root.path().to_string_lossy().into_owned(),
            };
            let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
            storage
                .init_session(&info, default_model_id())
                .await
                .unwrap();
            let mut dense = super::super::tests::response_projection_production_timeline_with_items(
                "initial",
                1,
                vec![sampling_types::ConversationItem::assistant("fixed")],
                0,
            );
            let turn = dense.active_turn().unwrap();
            let step = dense.active_step().unwrap();
            for n in 0..count {
                let id = format!("closed-{n}");
                for kind in [
                    Kind::Request(RequestEvent::Started {
                        id: id.clone(),
                        turn,
                        step,
                        model_id: "model".into(),
                        input_message_count: 1,
                        tool_count: 0,
                    }),
                    Kind::Request(RequestEvent::Failed {
                        id: id.clone(),
                        duration_ms: 2,
                        error_kind: "fixture".into(),
                        message: "x".repeat(16 * 1024),
                        retryable: false,
                    }),
                    Kind::Observation(ObservationEvent {
                        scope: "sampling_usage".into(),
                        name: "attempt_settled".into(),
                        turn: Some(turn),
                        step: Some(step),
                        data: Some(serde_json::json!({
                            "attempt_key": id, "model_id": "model", "captured_prompt_index": 0,
                            "usage": sampling_types::TokenUsage {
                                prompt_tokens: 25, ..Default::default()
                            },
                        })),
                    }),
                ] {
                    dense.record(kind).unwrap();
                }
            }
            let mut filler = diagnostic();
            if let Kind::Observation(observation) = &mut filler {
                observation.data = Some(serde_json::json!(
                    "x".repeat(MAX_UNPUBLISHED_BYTES as usize)
                ));
            }
            dense.record(filler).unwrap();
            for event in dense.events() {
                storage.append_timeline_event(&info, event).await.unwrap();
            }
            drop(storage);

            let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
            let restored = storage
                .load_session_for_write_with_indexed_history(&info)
                .await
                .unwrap()
                .timeline;
            assert_eq!(restored.next_seq(), dense.next_seq());
            assert_eq!(restored.active_turn(), Some(turn));
            assert_eq!(restored.active_step(), Some(step));
            assert_eq!(restored.surface_ids(), dense.surface_ids());
            assert_eq!(restored.open_request_ids().count(), 0);
            let usage = chat_state::actor::state::project_timeline_usage(&restored).unwrap();
            assert_eq!(usage.totals.input_tokens, count * 25);
            assert_eq!(
                usage,
                chat_state::actor::state::project_timeline_usage(&dense).unwrap()
            );
            resident_counts.push(restored.events().len());
        }
        assert_eq!(resident_counts, [2, 2]);
    }

    #[tokio::test]
    async fn published_closed_tools_inside_an_active_step_do_not_grow_recovery() {
        let mut work = Vec::new();
        for count in [1, 128] {
            let root = tempfile::tempdir().unwrap();
            let (storage, info) = fixture(root.path());
            storage
                .init_session(&info, default_model_id())
                .await
                .unwrap();
            let mut dense = chat_state::Timeline::default();
            dense
                .append(
                    sampling_types::ConversationItem::system("fixed"),
                    MessageCause::Seed,
                )
                .unwrap();
            let turn = TurnId(1);
            let step = StepId { turn, index: 0 };
            dense.record(turn_start(turn, "current", 1, 0)).unwrap();
            dense
                .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
                .unwrap();
            for n in 0..count {
                let id = format!("closed-{n}");
                dense.record(tool_start(&id)).unwrap();
                dense
                    .record(TimelineEventKind::Tool(ToolEvent::Completed {
                        call_id: id,
                        name: "example".into(),
                        outcome: "completed".into(),
                        duration_ms: 1,
                        details: None,
                    }))
                    .unwrap();
            }
            dense.record(tool_start("still-open")).unwrap();
            dense
                .record(observation("active-tool-publication"))
                .unwrap();
            for event in dense.events() {
                storage
                    .append_timeline_event_durable(&info, event)
                    .await
                    .unwrap();
            }
            let directory = storage.open_session(&info).unwrap().directory_handle();
            let cut = read_manifest(&directory).unwrap().unwrap();
            drop(storage);
            let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
            let restored = storage
                .load_session_for_write_with_indexed_history(&info)
                .await
                .unwrap()
                .timeline;
            assert_eq!(restored.active_turn(), Some(turn));
            assert_eq!(restored.active_step(), Some(step));
            assert_eq!(restored.surface_ids(), dense.surface_ids());
            assert_eq!(
                restored.open_tool_call_ids().collect::<Vec<_>>(),
                ["still-open"]
            );
            assert_eq!(restored.next_seq(), dense.next_seq());
            assert!(matches!(
                restored.prepare(tool_start("closed-0")),
                Err(chat_state::TimelineError::ToolAlreadyOpen(_))
            ));
            work.push((cut.retained_records, restored.events().len()));
        }
        assert_eq!(work, [(0, 3), (0, 3)]);
    }

    #[tokio::test]
    async fn closed_turns_do_not_expand_the_executable_checkpoint() {
        let mut counts = Vec::new();
        for closed_turns in [1, 128] {
            let root = tempfile::tempdir().unwrap();
            let (storage, info) = fixture(root.path());
            storage
                .init_session(&info, default_model_id())
                .await
                .unwrap();
            let mut dense = chat_state::Timeline::default();
            dense
                .append(
                    sampling_types::ConversationItem::system("fixed active context"),
                    MessageCause::Seed,
                )
                .unwrap();
            let rebuild = dense
                .replace_all(
                    vec![
                        sampling_types::ConversationItem::system("fixed active context"),
                        sampling_types::ConversationItem::project_instructions(
                            "rebuilt startup context",
                        ),
                    ],
                    MessageCause::ContextRebuild,
                )
                .unwrap();
            for n in 1..=closed_turns {
                let id = TurnId(n);
                let step = StepId { turn: id, index: 0 };
                for kind in [
                    turn_start(id, "internal", 1, 0),
                    TimelineEventKind::Step(StepEvent::Started { id: step }),
                    TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                        occurrence_id: format!("stop-{n}"),
                        event: chat_state::HookEventType::Stop,
                        gate: chat_state::HookEventType::Stop.gate(),
                        cause: chat_state::HookCause::Turn { turn: id },
                        config_generation: 1,
                        handlers: Vec::new(),
                    }),
                    TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                        occurrence_id: format!("stop-{n}"),
                        decision: chat_state::HookAggregateDecision::Stop {
                            decision: chat_state::HookStopDecision::AllowStop,
                        },
                    }),
                    TimelineEventKind::Step(StepEvent::Ended {
                        id: step,
                        outcome: "completed".into(),
                        duration_ms: 1,
                    }),
                    TimelineEventKind::Turn(TurnEvent::Ended {
                        id,
                        outcome: "completed".into(),
                        duration_ms: 2,
                        tool_count: 0,
                        terminal: chat_state::TurnTerminal {
                            source: chat_state::TurnTerminalSource::Host,
                            stop_reason: "end_turn".into(),
                            completion_kind: "completed".into(),
                        },
                        cancellation_category: None,
                        details: None,
                    }),
                ] {
                    dense.record(kind).unwrap();
                }
            }
            dense.record(observation("force-publication")).unwrap();
            for event in dense.events() {
                storage.append_timeline_event(&info, event).await.unwrap();
            }
            let directory = storage.open_session(&info).unwrap().directory_handle();
            let cut = read_manifest(&directory).unwrap().unwrap();
            assert_eq!(
                cut.bytes,
                directory
                    .open_regular(OsStr::new(TIMELINE_FILE), "journal")
                    .unwrap()
                    .metadata()
                    .unwrap()
                    .len()
            );
            drop(storage);

            let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
            let restored = storage
                .load_session_for_write_with_indexed_history(&info)
                .await
                .unwrap()
                .timeline;
            assert_eq!(restored.history_branch(), Some(rebuild.seq));
            assert_eq!(restored.active_turn(), None);
            assert_eq!(restored.active_step(), None);
            assert_eq!(
                serde_json::to_value(restored.surface()).unwrap(),
                serde_json::to_value(dense.surface()).unwrap(),
            );
            assert_eq!(restored.surface_ids(), dense.surface_ids());
            assert_eq!(restored.next_seq(), dense.next_seq());
            assert_eq!(restored.next_prompt_index(), dense.next_prompt_index());
            assert!(matches!(
                restored.prepare(turn_start(TurnId(1), "duplicate", 1, 0)),
                Err(chat_state::TimelineError::TurnAlreadySeen(TurnId(1)))
            ));
            let exact = read_domain_range(&directory, 2..8).unwrap().unwrap();
            assert!(matches!(
                &exact[0].kind,
                TimelineEventKind::Turn(TurnEvent::Started { id: TurnId(1), .. })
            ));
            assert!(matches!(
                &exact[5].kind,
                TimelineEventKind::Turn(TurnEvent::Ended { id: TurnId(1), .. })
            ));
            assert!(restored.hook_projection("stop-1").is_none());
            assert!(matches!(&exact[2].kind, TimelineEventKind::Hook(
                chat_state::HookEvent::Triggered { occurrence_id, .. }
            ) if occurrence_id == "stop-1"));
            counts.push((cut.retained_records, restored.events().len()));
        }
        assert_eq!(counts, [(0, 0), (0, 0)]);
    }

    #[tokio::test]
    async fn published_background_settlement_pages_without_resident_runtime_history() {
        use crate::extensions::notification::{SessionNotification, SessionUpdate as GrowUpdate};
        use chat_state::session_runtime::{
            CommandSettlementStatus, RuntimeCheckpoint, RuntimeCommandSettlement,
            SessionRuntimeEvent,
        };

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut dense = chat_state::Timeline::default();
        let start = SessionUpdate::Grow(Box::new(SessionNotification {
            session_id: info.id.clone(),
            update: GrowUpdate::TaskBackgrounded {
                task_id: "task".into(),
                tool_call_id: "call".into(),
                command: "fixture".into(),
                cwd: info.cwd.clone(),
                output_file: "fixture.log".into(),
                description: Some("background fixture".into()),
                monitor_description: None,
            },
            meta: None,
        }));
        storage.append_update(&info, &start).await.unwrap();
        let operation_id = "background-exit".to_string();
        let root_session_id = info.id.to_string();
        let mut append = vec![
            SessionRuntimeEvent::SuspendRequested {
                operation_id: operation_id.clone(),
                root_session_id,
            },
            SessionRuntimeEvent::CommandsSettled {
                operation_id: operation_id.clone(),
                commands: vec![RuntimeCommandSettlement {
                    command_id: "task".into(),
                    call_id: "call".into(),
                    outcome: CommandSettlementStatus::Interrupted,
                    output_file: "fixture.log".into(),
                    exit_code: None,
                    signal: None,
                    output: "private output".into(),
                    is_backgrounded: true,
                    owner: chat_state::NotificationOwner::Session,
                    task_kind: chat_state::NotificationTaskKind::Task,
                    start_time: None,
                    end_time: None,
                }],
            },
            SessionRuntimeEvent::Checkpoint {
                operation_id: operation_id.clone(),
                checkpoint: Box::new(RuntimeCheckpoint {
                    frontier: EventSeq::new(1),
                    runtime_source_hash: "a".repeat(64),
                    turn: None,
                    step: None,
                }),
            },
        ]
        .into_iter()
        .map(TimelineEventKind::Runtime)
        .collect::<Vec<_>>();
        append.push(observation("background-settlement-publication"));
        for kind in append {
            let event = dense.record(kind).unwrap();
            storage
                .append_timeline_event_durable(&info, &event)
                .await
                .unwrap();
        }
        drop(storage);
        let storage = JsonlStorageAdapter::with_root(root.path().into());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            loaded.timeline.runtime_lifecycle(),
            dense.runtime_lifecycle()
        );
        assert!(loaded.timeline.events().is_empty());
        assert!(loaded.history.sources.is_empty());
        let prepared = loaded
            .history
            .prepare_indexed(&info.id, &loaded.timeline)
            .unwrap();
        let pages = loaded.history.indexed_pages(prepared).unwrap().unwrap();
        let (older, rows) = pages
            .page_before(&info.id, history::PagePosition::newest())
            .unwrap();
        assert!(older.is_none());
        assert_eq!(rows.len(), 2);
        assert!(
            matches!(&rows[0], SessionUpdate::Grow(notification) if matches!(&notification.update,
            GrowUpdate::TaskBackgrounded { task_id, .. } if task_id == "task"))
        );
        assert!(
            matches!(&rows[1], SessionUpdate::Grow(notification) if matches!(&notification.update,
            GrowUpdate::TaskCompleted { task_snapshot } if task_snapshot.task_id == "task" && task_snapshot.output.is_empty()))
        );
    }

    #[tokio::test]
    async fn task_receipts_restore_pending_state_and_exact_closed_consumers() {
        use chat_state::{
            NotificationDismissReason, NotificationEvent, NotificationOwner,
            NotificationPayloadRef, NotificationSource, NotificationSourceVersion,
            NotificationTaskKind,
        };
        use sampling_types::{ConversationItem, conversation::SyntheticReason};

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut dense = chat_state::Timeline::default();
        let task = |n| NotificationSource::TaskCompleted {
            task_id: format!("task-{n}"),
            task_kind: NotificationTaskKind::Task,
            owner: NotificationOwner::Session,
        };
        let version = NotificationSourceVersion::Ordinal { value: 1 };
        let receipt = |source: NotificationSource, version: NotificationSourceVersion| {
            let owner = info.id.to_string();
            let id = chat_state::notification_id(&owner, &source, &version).unwrap();
            NotificationEvent::Received {
                id,
                owner_session_id: owner,
                source,
                source_version: version,
                payload_ref: NotificationPayloadRef {
                    bytes: 1,
                    blake3: blake3::hash(b"x").to_hex().to_string(),
                },
            }
        };
        let turn = TurnId(1);
        dense.record(turn_start(turn, "current", 0, 0)).unwrap();
        for n in 0..64 {
            let received = receipt(task(n), version.clone());
            let NotificationEvent::Received { id, .. } = &received else {
                unreachable!()
            };
            let id = id.clone();
            dense
                .record(TimelineEventKind::Notification(received))
                .unwrap();
            let mut input = ConversationItem::user("previous completion");
            input.set_prompt_index(0);
            let ConversationItem::User(user) = &mut input else {
                unreachable!()
            };
            user.synthetic_reason = Some(SyntheticReason::TaskCompleted);
            dense
                .record(TimelineEventKind::Notification(
                    NotificationEvent::Consumed {
                        notification_ids: vec![id],
                        turn,
                        input: Some(input),
                    },
                ))
                .unwrap();
        }
        let dismissed = receipt(
            NotificationSource::TaskCompleted {
                task_id: "retired-goal-task".into(),
                task_kind: NotificationTaskKind::Task,
                owner: NotificationOwner::Goal {
                    goal_id: "goal".into(),
                    definition_revision: 1,
                },
            },
            version.clone(),
        );
        let NotificationEvent::Received { id, .. } = &dismissed else {
            unreachable!()
        };
        let id = id.clone();
        dense
            .record(TimelineEventKind::Notification(dismissed))
            .unwrap();
        dense
            .record(TimelineEventKind::Notification(
                NotificationEvent::Dismissed {
                    notification_ids: vec![id],
                    reason: NotificationDismissReason::GoalOwnedAutostart {
                        goal_id: "goal".into(),
                        definition_revision: 1,
                    },
                },
            ))
            .unwrap();
        let no_context = receipt(task(65), version.clone());
        let NotificationEvent::Received { id, .. } = &no_context else {
            unreachable!()
        };
        let id = id.clone();
        dense
            .record(TimelineEventKind::Notification(no_context))
            .unwrap();
        dense
            .record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![id],
                    turn,
                    input: None,
                },
            ))
            .unwrap();
        let handoff = NotificationSource::WorkflowHandoff {
            run_id: "wf_finished".into(),
            handoff: chat_state::WorkflowTurnHandoff::Completion,
        };
        let handoff_version = NotificationSourceVersion::Opaque {
            value: "wf-finished-1".into(),
        };
        let received = receipt(handoff.clone(), handoff_version.clone());
        let NotificationEvent::Received { id, .. } = &received else {
            unreachable!()
        };
        let id = id.clone();
        dense
            .record(TimelineEventKind::Notification(received))
            .unwrap();
        dense
            .record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![id],
                    turn,
                    input: None,
                },
            ))
            .unwrap();
        let message = NotificationSource::ParentMessage {
            parent_session_id: "parent".into(),
            message_id: "closed-message".into(),
            interrupt: false,
        };
        let message_version = NotificationSourceVersion::Ordinal {
            value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
        };
        let received = receipt(message.clone(), message_version.clone());
        let NotificationEvent::Received { id, .. } = &received else {
            unreachable!()
        };
        let id = id.clone();
        dense
            .record(TimelineEventKind::Notification(received))
            .unwrap();
        dense
            .record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![id.clone()],
                    turn,
                    input: Some(ConversationItem::received_agent_message(
                        sampling_types::AgentMessage {
                            receipt_id: id,
                            source_session_id: "parent".into(),
                            target_session_id: info.id.to_string(),
                            message_id: "closed-message".into(),
                            reply_to: None,
                            message: "x".into(),
                        },
                    )),
                },
            ))
            .unwrap();
        dense
            .record(TimelineEventKind::Notification(receipt(
                task(64),
                version.clone(),
            )))
            .unwrap();
        dense.record(observation("receipt-publication")).unwrap();
        for event in dense.events() {
            storage
                .append_timeline_event_durable(&info, event)
                .await
                .unwrap();
        }
        drop(storage);
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let mut restored = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap()
            .timeline;
        assert_eq!(
            restored.pending_notifications(),
            dense.pending_notifications()
        );
        assert_eq!(restored.surface_ids(), dense.surface_ids());
        assert_eq!(restored.surface().len(), 65);
        assert!(restored.events().iter().all(|event| !matches!(
            event.kind,
            TimelineEventKind::Notification(NotificationEvent::Consumed { .. })
        )));
        assert_eq!(
            restored
                .exact_notification_consumer(&task(0), &version)
                .unwrap(),
            Some(turn)
        );
        assert_eq!(
            restored
                .exact_notification_consumer(&task(64), &version)
                .unwrap(),
            None
        );
        assert_eq!(
            restored
                .exact_notification_consumer(&task(65), &version)
                .unwrap(),
            Some(turn)
        );
        assert_eq!(
            restored
                .exact_notification_consumer(&handoff, &handoff_version)
                .unwrap(),
            Some(turn)
        );
        assert_eq!(
            restored
                .exact_notification_consumer(&message, &message_version)
                .unwrap(),
            Some(turn)
        );
        assert_eq!(
            restored.consumed_agent_messages(&["closed-message".into(), "missing".into()]),
            std::collections::BTreeSet::from(["closed-message".into()])
        );
        assert!(
            restored
                .prepare(TimelineEventKind::Notification(receipt(
                    handoff,
                    handoff_version
                )))
                .is_err()
        );
        assert!(
            restored
                .prepare(TimelineEventKind::Notification(receipt(task(0), version)))
                .is_err()
        );
        let late = receipt(
            NotificationSource::TaskStillRunning {
                task_id: "task-0".into(),
                task_kind: NotificationTaskKind::Task,
                owner: NotificationOwner::Session,
            },
            NotificationSourceVersion::Opaque {
                value: "late".into(),
            },
        );
        let pending = restored.pending_notifications();
        restored
            .record(TimelineEventKind::Notification(late))
            .unwrap();
        assert_eq!(restored.pending_notifications(), pending);
    }

    #[tokio::test]
    async fn closed_step_progress_survives_a_suspending_cut() {
        use chat_state::session_runtime::SessionRuntimeEvent;
        use chat_state::{
            HookAggregateDecision, HookCause, HookEvent, HookEventType, HookFailurePolicy,
            HookHandlerKind, HookHandlerPlan, HookHandlerPlanAction, HookHandlerProvenance,
            HookRunControl, HookRunOutcome, HookStopDecision,
        };

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        let turn = TurnId(1);
        let step = StepId { turn, index: 0 };
        let stop = "stop".to_owned();
        let run = "stop-run".to_owned();
        let reason = "continue".to_owned();
        for kind in [
            turn_start(turn, "current", 0, 0),
            TimelineEventKind::Hook(HookEvent::Triggered {
                occurrence_id: stop.clone(),
                event: HookEventType::Stop,
                gate: HookEventType::Stop.gate(),
                cause: HookCause::Turn { turn },
                config_generation: 1,
                handlers: vec![HookHandlerPlan {
                    index: 0,
                    run_id: run.clone(),
                    name: "continue".into(),
                    provenance: HookHandlerProvenance::ProjectFile,
                    kind: HookHandlerKind::Command,
                    failure_policy: HookFailurePolicy::Allow,
                    action: HookHandlerPlanAction::Execute,
                }],
            }),
            TimelineEventKind::Hook(HookEvent::RunStarted {
                occurrence_id: stop.clone(),
                run_id: run.clone(),
                handler_index: 0,
            }),
            TimelineEventKind::Hook(HookEvent::RunFinished {
                occurrence_id: stop.clone(),
                run_id: run,
                handler_index: 0,
                elapsed_ms: 1,
                outcome: HookRunOutcome::Success,
                control: HookRunControl::StopKeepWorking {
                    reason: Some(reason.clone()),
                    additional_context: None,
                },
            }),
            TimelineEventKind::Hook(HookEvent::Completed {
                occurrence_id: stop,
                decision: HookAggregateDecision::Stop {
                    decision: HookStopDecision::KeepWorking {
                        reasons: vec![reason],
                        additional_context: Vec::new(),
                    },
                },
            }),
            TimelineEventKind::Step(StepEvent::Started { id: step }),
            TimelineEventKind::Step(StepEvent::Ended {
                id: step,
                outcome: "completed".into(),
                duration_ms: 1,
            }),
            TimelineEventKind::Runtime(SessionRuntimeEvent::SuspendRequested {
                operation_id: "suspend".into(),
                root_session_id: info.id.to_string(),
            }),
            observation("force-publication"),
        ] {
            let event = timeline.record(kind).unwrap();
            storage
                .append_timeline_event_durable(&info, &event)
                .await
                .unwrap();
        }
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert!(loaded.timeline.has_surface_checkpoint());
        assert_eq!(loaded.timeline.active_turn(), Some(turn));
        assert_eq!(loaded.timeline.active_step(), None);
        assert!(
            !loaded
                .timeline
                .events()
                .iter()
                .any(|event| matches!(event.kind, TimelineEventKind::Step(_)))
        );
        assert!(matches!(
            loaded.timeline.prepare(TimelineEventKind::Step(StepEvent::Started { id: step })),
            Err(chat_state::TimelineError::StepAlreadySeen(id)) if id == step
        ));
        assert_eq!(
            loaded
                .timeline
                .suspension_turn_evidence(Some(turn), None)
                .unwrap()
                .committed_stop_continuations,
            1
        );
    }

    #[tokio::test]
    async fn indexed_hook_and_input_cut_restores_only_unsettled_admission() {
        use chat_state::{
            HookAggregateDecision, HookCause, HookEvent, HookEventType, HookGateDecision,
            InputAdmissionDecision, InputDismissReason, InputEvent, InputIntent, InputRoute,
            TimelineError, TimelineEventKind as Kind,
        };
        let root = tempfile::tempdir().unwrap();
        let info = Info {
            id: acp::SessionId::new("hook-input-cut"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut dense = chat_state::Timeline::default();
        dense
            .append(
                sampling_types::ConversationItem::system("fixed"),
                chat_state::MessageCause::Seed,
            )
            .unwrap();
        let directory = storage.open_session(&info).unwrap().directory_handle();
        let payload_ref = crate::session::input_inbox::write_payload(
            &directory,
            &crate::session::input_inbox::tests::text_payload("fixture", "unchanged"),
        )
        .unwrap();
        let submit = |id: &str| {
            Kind::Input(InputEvent::Submitted {
                input_id: id.into(),
                intent: InputIntent::Prompt,
                payload_ref: payload_ref.clone(),
            })
        };
        let trigger = |id: &str| {
            Kind::Hook(HookEvent::Triggered {
                occurrence_id: format!("hook-{id}"),
                event: HookEventType::UserPromptSubmit,
                gate: HookEventType::UserPromptSubmit.gate(),
                cause: HookCause::Input {
                    input_id: id.into(),
                },
                config_generation: 17,
                handlers: Vec::new(),
            })
        };
        let complete = |id: &str| {
            Kind::Hook(HookEvent::Completed {
                occurrence_id: format!("hook-{id}"),
                decision: HookAggregateDecision::Prompt {
                    decision: HookGateDecision::Allow,
                },
            })
        };
        let admit = |id: &str| {
            Kind::Input(InputEvent::AdmissionResolved {
                input_id: id.into(),
                decision: InputAdmissionDecision::Allow,
                route: Some(InputRoute::Fifo),
                supersedes: Vec::new(),
            })
        };
        for index in 0..128 {
            let id = format!("closed-{index}");
            for event in [
                submit(&id),
                trigger(&id),
                complete(&id),
                admit(&id),
                Kind::Input(InputEvent::Dismissed {
                    input_ids: vec![id.clone()],
                    reason: InputDismissReason::UserRemoved,
                }),
            ] {
                dense.record(event).unwrap();
            }
        }
        let pending = "pending";
        for event in [submit(pending), trigger(pending), complete(pending)] {
            dense.record(event).unwrap();
        }
        let publication = dense
            .record(Kind::Observation(chat_state::ObservationEvent {
                scope: "shell".into(),
                name: "publish".into(),
                turn: None,
                step: None,
                data: Some(serde_json::json!(
                    "x".repeat(MAX_UNPUBLISHED_BYTES as usize)
                )),
            }))
            .unwrap();
        for event in dense
            .events()
            .iter()
            .take_while(|event| event.seq < publication.seq)
        {
            storage.append_timeline_event(&info, event).await.unwrap();
        }
        storage
            .append_timeline_event_durable(&info, &publication)
            .await
            .unwrap();
        let admission = dense.record(admit(pending)).unwrap();
        storage
            .append_timeline_event_durable(&info, &admission)
            .await
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let timeline = loaded.timeline;
        assert_eq!(timeline.next_seq(), dense.next_seq());
        assert_eq!(timeline.surface_ids(), dense.surface_ids());
        assert_eq!(timeline.hook_config_generation(), 17);
        assert_eq!(timeline.events().len(), 3);
        assert_eq!(
            timeline.pending_allowed_inputs(),
            dense.pending_allowed_inputs()
        );
        let cut = loaded.history.indexed.as_ref().unwrap();
        let archived = cut.tools.cut(cut.clone()).unwrap();
        let roots = chat_state::timeline::ExecutionHistory::input_payload_hashes(
            &archived,
            Some(&[payload_ref.blake3.clone(), "missing".into()]),
            &|| false,
        )
        .unwrap();
        assert_eq!(roots, [payload_ref.blake3.clone()].into_iter().collect());
        assert!(timeline.input_state("closed-0").is_none());
        assert!(timeline.hook_projection("hook-closed-0").is_none());
        assert!(
            matches!(timeline.prepare(submit("closed-0")), Err(TimelineError::DuplicateInput(id)) if id == "closed-0")
        );
    }

    #[tokio::test]
    async fn published_controls_restore_effective_context_and_terminal_highwaters() {
        use super::super::{AppendDurability, ledger::AppendFact};
        use crate::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };
        use crate::session::{
            ControlIntent, behavior::BehaviorSnapshot, control::SessionControlSnapshot,
        };
        use chat_state::{ControlContextActivation, ControlContextLayer};

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        let seed = timeline
            .append(
                sampling_types::ConversationItem::system("head"),
                chat_state::MessageCause::Seed,
            )
            .unwrap();
        storage.append_timeline_event(&info, &seed).await.unwrap();
        let target = ControlTarget::Agent {
            agent_name: "grow".into(),
        };
        let mut first = None;
        for revision in 1..=128 {
            let intent = ControlIntent {
                client_id: "pager".into(),
                generation: 1,
                sequence: revision,
            };
            let snapshot =
                SessionControlSnapshot::new(revision, "grow", BehaviorSnapshot::normal(), None)
                    .with_applied_control(
                        ControlDomain::Agent,
                        target.clone(),
                        Some(intent.clone()),
                    );
            let kind = if revision == 1 {
                snapshot.timeline_kind_with_model_context(
                    ControlContextLayer::AgentRole,
                    ControlContextActivation::Transition,
                    "fixed role".to_string(),
                )
            } else {
                snapshot.timeline_kind()
            }
            .unwrap();
            let event = timeline.record(kind).unwrap();
            first.get_or_insert(event.seq);
            storage.append_timeline_event(&info, &event).await.unwrap();
            let update = history::HistoryFact {
                meta: None,
                content: HistoryContent::Grow(Box::new(
                    history::GrowHistoryFact::ControlStateUpdate(ControlStateUpdate {
                        epoch: "test-epoch".into(),
                        domain: ControlDomain::Agent,
                        revision,
                        intent: Some(intent),
                        snapshot: false,
                        receipt_only: false,
                        phase: ControlPhase::Applied,
                        current: target.clone(),
                        desired: None,
                        message: None,
                    }),
                )),
            };
            storage
                .append_journal_fact(
                    &info,
                    AppendFact::Presentation(update),
                    AppendDurability::Buffered,
                )
                .await
                .unwrap();
        }
        let publish = timeline.record(observation("controls-published")).unwrap();
        storage
            .append_timeline_event_durable(&info, &publish)
            .await
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let index = loaded.history.indexed.as_ref().unwrap();
        assert_eq!(index.manifest.retained_records, 0);
        assert_eq!(loaded.timeline.events().len(), 1);
        assert_eq!(loaded.control_snapshot.unwrap().control_revision, 128);
        assert!(
            loaded
                .timeline
                .exact_event(first.unwrap())
                .unwrap()
                .is_some()
        );
        assert_eq!(loaded.timeline.surface_ids(), timeline.surface_ids());
        let active = loaded.timeline.active_control_contexts();
        assert_eq!(
            active[&ControlContextLayer::AgentRole].surface_id.event,
            first.unwrap()
        );
        let receipts =
            SessionControlSnapshot::durable_receipts_from_timeline(loaded.timeline.events())
                .unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].intent.sequence, 128);
        let ui = index.control_presentations().unwrap();
        assert_eq!(ui.len(), 1);
        assert!(matches!(&ui[0].content, HistoryContent::Grow(fact)
            if matches!(fact.as_ref(), history::GrowHistoryFact::ControlStateUpdate(update)
                if update.intent.as_ref().is_some_and(|intent| intent.sequence == 128))));
    }

    #[tokio::test]
    async fn published_turn_completion_presentation_is_paged() {
        use super::super::{AppendDurability, ledger::AppendFact};
        use crate::extensions::notification::{PromptUsage, PromptUsageModel};
        use history::GrowHistoryFact;

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        // A completion whose frozen usage snapshot (full total 150 = 100 in +
        // 50 out, per-model rows, cache known + unknown buckets, incomplete
        // with unknown cost) must survive every later page read byte for byte.
        // `closed-1` sits outside the initial latest page, so the snapshot is
        // only served from an older DISPLAY page.
        let frozen_usage = PromptUsage {
            totals: PromptUsageModel {
                input_tokens: 100,
                output_tokens: 50,
                total_tokens: 150,
                cached_read_tokens: 20,
                cache_creation_tokens: 10,
                cache_read_known_input_tokens: 15,
                cache_read_unknown_calls: 1,
                cache_write_unknown_calls: 2,
                reasoning_tokens: 5,
                model_calls: 3,
                api_duration_ms: 2500,
                cost_usd_ticks: None,
                cost_is_partial: false,
                cost_missing_calls: 0,
            },
            model_usage: [
                (
                    "model-a",
                    PromptUsageModel {
                        input_tokens: 60,
                        output_tokens: 40,
                        total_tokens: 100,
                        cached_read_tokens: 20,
                        cache_creation_tokens: 0,
                        cache_read_known_input_tokens: 15,
                        cache_read_unknown_calls: 1,
                        cache_write_unknown_calls: 0,
                        reasoning_tokens: 5,
                        model_calls: 2,
                        api_duration_ms: 2000,
                        cost_usd_ticks: None,
                        cost_is_partial: false,
                        cost_missing_calls: 0,
                    },
                ),
                (
                    "model-b",
                    PromptUsageModel {
                        input_tokens: 40,
                        output_tokens: 10,
                        total_tokens: 50,
                        cached_read_tokens: 0,
                        cache_creation_tokens: 10,
                        cache_read_known_input_tokens: 0,
                        cache_read_unknown_calls: 0,
                        cache_write_unknown_calls: 2,
                        reasoning_tokens: 0,
                        model_calls: 1,
                        api_duration_ms: 500,
                        cost_usd_ticks: None,
                        cost_is_partial: false,
                        cost_missing_calls: 0,
                    },
                ),
            ]
            .into_iter()
            .map(|(model, usage)| (model.to_string(), usage))
            .collect(),
            num_turns: 1,
            usage_is_incomplete: true,
        };
        for index in 0..(history::HISTORY_PAGE_ROWS + 2) {
            let fact = history::HistoryFact {
                meta: None,
                content: HistoryContent::Grow(Box::new(GrowHistoryFact::TurnCompleted {
                    prompt_id: format!("closed-{index}"),
                    identity: None,
                    stop_reason: "end_turn".into(),
                    agent_result: None,
                    usage: (index == 1).then(|| frozen_usage.clone()),
                })),
            };
            storage
                .append_journal_fact(
                    &info,
                    AppendFact::Presentation(fact),
                    AppendDurability::Buffered,
                )
                .await
                .unwrap();
        }
        let publish = chat_state::Timeline::default()
            .record(observation("turn-updates-published"))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &publish)
            .await
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert!(loaded.history.sources.is_empty());
        let prepared = loaded
            .history
            .prepare_indexed(&info.id, &loaded.timeline)
            .unwrap();
        let pages = loaded.history.indexed_pages(prepared).unwrap().unwrap();
        // Persisted timeline usage snapshot before further reads; the
        // presentation completions live in the DISPLAY index, so this guards
        // aggregate stability, not the frozen per-turn totals below.
        let timeline_totals =
            chat_state::actor::state::project_timeline_usage_totals(&loaded.timeline).unwrap();
        let (before, recent) = pages
            .page_before(&info.id, history::PagePosition::newest())
            .unwrap();
        assert_eq!(recent.len(), history::HISTORY_PAGE_ROWS);
        assert!(serde_json::to_vec(&recent).unwrap().len() <= history::HISTORY_PAGE_BYTES);
        let (before, older) = pages.page_before(&info.id, before.unwrap()).unwrap();
        assert!(before.is_none());
        assert_eq!(older.len(), 2);
        assert!(serde_json::to_vec(&older).unwrap().len() <= history::HISTORY_PAGE_BYTES);
        // The ids assertion below pins `closed-1` to this older page, so the
        // frozen snapshot is served from outside the initial latest page.
        let older_usage = find_turn_usage(older.iter(), "closed-1")
            .expect("older DISPLAY page serves the frozen completion usage");
        assert_eq!(older_usage, frozen_usage);
        assert_eq!(
            serde_json::to_vec(&older_usage).unwrap(),
            serde_json::to_vec(&frozen_usage).unwrap()
        );
        let ids = older
            .into_iter()
            .chain(recent)
            .map(|update| match update {
                SessionUpdate::Grow(update) => match update.update {
                    crate::extensions::notification::SessionUpdate::TurnCompleted {
                        prompt_id,
                        ..
                    } => prompt_id,
                    _ => panic!("expected a Turn completion"),
                },
                _ => panic!("expected a Grow update"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            (0..(history::HISTORY_PAGE_ROWS + 2))
                .map(|index| format!("closed-{index}"))
                .collect::<Vec<_>>()
        );

        fn find_turn_usage<'a>(
            updates: impl IntoIterator<Item = &'a SessionUpdate>,
            prompt: &str,
        ) -> Option<PromptUsage> {
            for update in updates {
                let SessionUpdate::Grow(update) = update else {
                    panic!("expected a Grow update");
                };
                let crate::extensions::notification::SessionUpdate::TurnCompleted {
                    prompt_id,
                    usage,
                    ..
                } = &update.update
                else {
                    panic!("expected a Turn completion");
                };
                if prompt_id == prompt {
                    return usage.clone();
                }
            }
            None
        }

        // Same source, opposite direction: the forward walk serves the same
        // frozen snapshot exactly once.
        let mut forward_usage = None;
        let mut after = history::PagePosition::oldest();
        loop {
            let window = pages.read_after(&info.id, after).unwrap();
            assert!(window.rows.len() <= history::HISTORY_PAGE_ROWS);
            assert!(serde_json::to_vec(&window.rows).unwrap().len() <= history::HISTORY_PAGE_BYTES);
            if let Some(usage) =
                find_turn_usage(window.rows.iter().map(|row| &row.update), "closed-1")
            {
                assert!(
                    forward_usage.is_none(),
                    "closed-1 served twice in the forward walk"
                );
                assert_eq!(usage, frozen_usage);
                forward_usage = Some(usage);
            }
            match window.newer_position {
                Some(next) => {
                    assert_ne!(next, after);
                    after = next;
                }
                None => break,
            }
        }
        assert_eq!(forward_usage, Some(frozen_usage.clone()));

        // A fresh cold load over the same journal must reproduce the frozen
        // snapshot and the same persisted timeline usage totals.
        drop(pages);
        drop(loaded);
        drop(storage);
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            chat_state::actor::state::project_timeline_usage_totals(&loaded.timeline).unwrap(),
            timeline_totals,
            "persisted timeline usage totals changed across the cold reload"
        );
        let prepared = loaded
            .history
            .prepare_indexed(&info.id, &loaded.timeline)
            .unwrap();
        let pages = loaded.history.indexed_pages(prepared).unwrap().unwrap();
        let (cold_before, cold_recent) = pages
            .page_before(&info.id, history::PagePosition::newest())
            .unwrap();
        assert_eq!(cold_recent.len(), history::HISTORY_PAGE_ROWS);
        let (cold_before, cold_older) = pages.page_before(&info.id, cold_before.unwrap()).unwrap();
        assert!(cold_before.is_none());
        let cold_usage = find_turn_usage(cold_older.iter(), "closed-1")
            .expect("cold reload still serves closed-1 from the older page");
        assert_eq!(cold_usage, frozen_usage);
        assert_eq!(
            serde_json::to_vec(&cold_usage).unwrap(),
            serde_json::to_vec(&frozen_usage).unwrap()
        );

        // ── Late session-only bill ──
        // The frozen 150 above is a preconstructed fixture, not a generated
        // bill. A settlement recorded after every completion must raise the
        // lifetime projection by exactly its own amounts while every frozen
        // TurnCompleted presentation keeps its original snapshot and the
        // completion population stays untouched. Usage facts live in their
        // own record class, so they never enter the DISPLAY pages below.
        drop(pages);
        let mut late_timeline = loaded.timeline.clone();
        // Append through this same writer owner; the indexed Timeline retains
        // its database handle until both loaded views are dropped below.
        let late_settlement = serde_json::json!({
            "attempt_key": "late-session-only",
            "model_id": "model-late",
            "captured_prompt_index": 0,
            "prompt_owner": null,
            "usage": {
                "prompt_tokens": 30,
                "completion_tokens": 0,
                "total_tokens": 30,
                "reasoning_tokens": 0,
                "cached_prompt_tokens": 0,
                "cache_read_known": true,
                "cache_creation_prompt_tokens": 0,
                "cache_write_known": true,
            },
            "cost_usd_ticks": null,
            "api_duration_ms": null,
        });
        let late = late_timeline
            .record(chat_state::TimelineEventKind::Observation(
                chat_state::ObservationEvent {
                    scope: "sampling_usage".into(),
                    name: "attempt_settled".into(),
                    turn: None,
                    step: None,
                    data: Some(late_settlement),
                },
            ))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &late)
            .await
            .unwrap();
        drop(late_timeline);
        drop(loaded);
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let late_totals =
            chat_state::actor::state::project_timeline_usage_totals(&loaded.timeline).unwrap();
        // The lifetime projection rose by exactly the settlement: 30 input
        // tokens (both cache buckets known, so known-input tracks them), one
        // main-loop round, and one call that reported no cost.
        assert_eq!(
            late_totals.totals.input_tokens,
            timeline_totals.totals.input_tokens + 30
        );
        assert_eq!(
            late_totals.totals.cache_read_known_input_tokens,
            timeline_totals.totals.cache_read_known_input_tokens + 30
        );
        assert_eq!(
            late_totals.totals.total_tokens(),
            timeline_totals.totals.total_tokens() + 30
        );
        assert_eq!(
            late_totals.totals.output_tokens,
            timeline_totals.totals.output_tokens
        );
        assert_eq!(
            late_totals.totals.cached_read_tokens,
            timeline_totals.totals.cached_read_tokens
        );
        assert_eq!(
            late_totals.totals.cache_creation_tokens,
            timeline_totals.totals.cache_creation_tokens
        );
        assert_eq!(
            late_totals.totals.cache_read_unknown_calls,
            timeline_totals.totals.cache_read_unknown_calls
        );
        assert_eq!(
            late_totals.totals.cache_write_unknown_calls,
            timeline_totals.totals.cache_write_unknown_calls
        );
        assert_eq!(
            late_totals.totals.reasoning_tokens,
            timeline_totals.totals.reasoning_tokens
        );
        assert_eq!(
            late_totals.totals.model_calls,
            timeline_totals.totals.model_calls + 1
        );
        assert_eq!(
            late_totals.totals.api_duration_ms,
            timeline_totals.totals.api_duration_ms
        );
        assert_eq!(
            late_totals.totals.cost_usd_ticks,
            timeline_totals.totals.cost_usd_ticks
        );
        assert_eq!(
            late_totals.totals.cost_missing_calls,
            timeline_totals.totals.cost_missing_calls + 1
        );
        assert_eq!(
            late_totals.main_loop_model_calls,
            timeline_totals.main_loop_model_calls + 1
        );
        assert!(!late_totals.incomplete);
        assert_eq!(
            late_totals.by_model.len(),
            timeline_totals.by_model.len() + 1
        );
        let late_row = late_totals
            .by_model
            .get("model-late")
            .expect("the settlement lands on its own model row");
        assert_eq!(late_row.input_tokens, 30);
        assert_eq!(late_row.cache_read_known_input_tokens, 30);
        assert_eq!(late_row.model_calls, 1);
        assert_eq!(late_row.total_tokens(), 30);

        // Every frozen presentation is unchanged: same completion population,
        // same ascending ids, `closed-1` still byte-for-byte its original 150
        // snapshot, and every other completion still carries no usage.
        let prepared = loaded
            .history
            .prepare_indexed(&info.id, &loaded.timeline)
            .unwrap();
        let pages = loaded.history.indexed_pages(prepared).unwrap().unwrap();
        let mut completions = Vec::new();
        let mut cursor = Some(history::PagePosition::newest());
        while let Some(before) = cursor {
            let (older, page) = pages.page_before(&info.id, before).unwrap();
            assert!(page.len() <= history::HISTORY_PAGE_ROWS);
            assert!(serde_json::to_vec(&page).unwrap().len() <= history::HISTORY_PAGE_BYTES);
            let mut page_completions = Vec::new();
            for update in page {
                let SessionUpdate::Grow(update) = update else {
                    panic!("expected a Grow update");
                };
                let crate::extensions::notification::SessionUpdate::TurnCompleted {
                    prompt_id,
                    usage,
                    ..
                } = update.update
                else {
                    panic!("expected a Turn completion");
                };
                page_completions.push((prompt_id, usage));
            }
            // Before walks newest windows first; each window is chronological.
            page_completions.append(&mut completions);
            completions = page_completions;
            cursor = older;
        }
        assert_eq!(completions.len(), history::HISTORY_PAGE_ROWS + 2);
        for (index, (prompt_id, usage)) in completions.iter().enumerate() {
            assert_eq!(prompt_id, &format!("closed-{index}"));
            if index == 1 {
                assert_eq!(usage.as_ref().unwrap(), &frozen_usage);
                assert_eq!(
                    serde_json::to_vec(usage).unwrap(),
                    serde_json::to_vec(&frozen_usage).unwrap()
                );
            } else {
                assert!(usage.is_none());
            }
        }
    }

    #[tokio::test]
    async fn closed_interactions_stay_indexed_and_open_attempts_can_resolve() {
        use chat_state::{InteractionEvent, InteractionKind, InteractionResolution};

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        let open = |attempt: &str| {
            TimelineEventKind::Interaction(InteractionEvent::Opened {
                tool_call_id: "call".into(),
                attempt_id: attempt.into(),
                kind: InteractionKind::Question,
            })
        };
        let resolve = |attempt: &str, resolution| {
            TimelineEventKind::Interaction(InteractionEvent::Resolved {
                tool_call_id: "call".into(),
                attempt_id: attempt.into(),
                kind: InteractionKind::Question,
                resolution,
            })
        };
        for index in 0..128 {
            let attempt = format!("attempt-{index}");
            for kind in [
                open(&attempt),
                resolve(&attempt, InteractionResolution::Answered),
            ] {
                let event = timeline.record(kind).unwrap();
                storage.append_timeline_event(&info, &event).await.unwrap();
            }
        }
        for kind in [open("pending"), observation("interaction-published")] {
            let event = timeline.record(kind).unwrap();
            storage
                .append_timeline_event_durable(&info, &event)
                .await
                .unwrap();
        }
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .manifest
                .retained_records,
            0
        );
        assert_eq!(loaded.timeline.events().len(), 1);
        assert!(
            loaded
                .timeline
                .exact_event(chat_state::EventSeq::new(1))
                .unwrap()
                .is_some()
        );
        assert!(matches!(
            loaded.timeline.prepare(open("attempt-0")),
            Err(chat_state::TimelineError::InvalidInteraction)
        ));
        assert!(
            loaded
                .timeline
                .prepare(resolve("pending", InteractionResolution::Suspended))
                .is_ok()
        );
        let event = timeline
            .record(resolve("pending", InteractionResolution::Suspended))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &event)
            .await
            .unwrap();
        let publish = timeline
            .record(observation("interaction-resolved"))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &publish)
            .await
            .unwrap();
        drop(loaded);
        drop(storage);
        let loaded = JsonlStorageAdapter::with_root(root.path().to_path_buf())
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert!(matches!(
            loaded
                .timeline
                .prepare(resolve("pending", InteractionResolution::Answered)),
            Err(chat_state::TimelineError::InvalidInteraction)
        ));
        let index = loaded.history.indexed.as_ref().unwrap();
        let checkpoint = index
            .tools
            .execution_checkpoint(index.manifest.tool_frontier)
            .unwrap();
        assert_eq!(
            checkpoint.waits.as_map().get("call"),
            Some(&(InteractionKind::Question, "pending".into()))
        );
    }

    #[tokio::test]
    async fn published_recovery_observations_restore_the_latest_snapshots() {
        use crate::session::{announcement_state::AnnouncementState, signals::SessionSignals};

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        let snapshots = |revision| {
            let signals = SessionSignals {
                turn_count: revision,
                ..Default::default()
            };
            let announcements = AnnouncementState {
                announced_skill_names: [format!("skill-{revision}")].into_iter().collect(),
                ..Default::default()
            };
            [
                signals.timeline_kind().unwrap(),
                announcements.timeline_kind().unwrap(),
            ]
        };

        for revision in 1..=128 {
            for kind in snapshots(revision) {
                let event = timeline.record(kind).unwrap();
                storage.append_timeline_event(&info, &event).await.unwrap();
            }
        }
        let publish = timeline
            .record(observation("projections-published"))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &publish)
            .await
            .unwrap();
        for kind in snapshots(129) {
            let event = timeline.record(kind).unwrap();
            storage.append_timeline_event(&info, &event).await.unwrap();
        }
        drop(storage);

        let loaded = JsonlStorageAdapter::with_root(root.path().to_path_buf())
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .manifest
                .retained_records,
            0
        );
        assert_eq!(loaded.timeline.events().len(), 4);
        assert_eq!(loaded.signals.unwrap().turn_count, 129);
        assert!(
            loaded
                .announcement_state
                .unwrap()
                .announced_skill_names
                .contains("skill-129")
        );
        assert!(
            loaded
                .timeline
                .exact_event(chat_state::EventSeq::new(0))
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn published_title_restores_current_revision_and_exact_history() {
        use chat_state::{SessionTitleEvent, SessionTitleSource, TimelineError};

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut dense = chat_state::Timeline::default();
        let mut first = None;
        for index in 0..128 {
            let event = dense
                .record(TimelineEventKind::SessionTitle(SessionTitleEvent {
                    title: format!("title {index}"),
                    source: SessionTitleSource::User,
                }))
                .unwrap();
            first.get_or_insert(event.clone());
            storage.append_timeline_event(&info, &event).await.unwrap();
        }
        let publish = dense.record(observation("title-published")).unwrap();
        storage
            .append_timeline_event_durable(&info, &publish)
            .await
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .manifest
                .retained_records,
            0
        );
        assert_eq!(loaded.timeline.events().len(), 1);
        assert_eq!(loaded.timeline.session_title(), dense.session_title());
        let first = first.unwrap();
        let exact = loaded.timeline.exact_event(first.seq).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(exact.as_ref()).unwrap(),
            serde_json::to_value(first).unwrap()
        );
        assert!(matches!(
            loaded
                .timeline
                .prepare(TimelineEventKind::SessionTitle(SessionTitleEvent {
                    title: "generated suggestion".into(),
                    source: SessionTitleSource::Generated {
                        sideband_id: "018f0000-0000-7000-8000-000000000001".into(),
                        result_seq: 1,
                    },
                })),
            Err(TimelineError::GeneratedTitleAfterUserTitle)
        ));
    }

    #[tokio::test]
    async fn published_model_changes_restore_latest_route_and_receipt() {
        use crate::agent::models::ModelId;
        use crate::session::persistence::{
            current_model_selection, durable_model_control_receipts, latest_model_selection,
            model_change_event, model_selection_at_frontier,
        };
        use chat_state::session_runtime::{RuntimeCheckpoint, SessionRuntimeEvent};

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut dense = chat_state::Timeline::default();
        let route = sampling_types::ModelImageInputKey::new("wire", "responses", "endpoint");
        let mut checkpoint_frontier = None;
        for index in 1..=128 {
            let event = dense
                .record(model_change_event(
                    &ModelId::new(format!("mock/{}", index - 1)),
                    &ModelId::new(format!("mock/{index}")),
                    None,
                    None,
                    "wire",
                    "wire",
                    &route,
                    &route,
                    "user_selection",
                    Some(&crate::session::ControlIntent {
                        client_id: "pager".into(),
                        generation: 1,
                        sequence: index,
                    }),
                ))
                .unwrap();
            storage.append_timeline_event(&info, &event).await.unwrap();
            if index == 64 {
                let operation_id = "model-checkpoint".to_owned();
                let requested = dense
                    .record(TimelineEventKind::Runtime(
                        SessionRuntimeEvent::SuspendRequested {
                            operation_id: operation_id.clone(),
                            root_session_id: info.id.to_string(),
                        },
                    ))
                    .unwrap();
                storage
                    .append_timeline_event(&info, &requested)
                    .await
                    .unwrap();
                let settled = dense
                    .record(TimelineEventKind::Runtime(
                        SessionRuntimeEvent::CommandsSettled {
                            operation_id: operation_id.clone(),
                            commands: Vec::new(),
                        },
                    ))
                    .unwrap();
                storage
                    .append_timeline_event(&info, &settled)
                    .await
                    .unwrap();
                checkpoint_frontier = Some(settled.seq);
                let checkpoint = dense
                    .record(TimelineEventKind::Runtime(
                        SessionRuntimeEvent::Checkpoint {
                            operation_id,
                            checkpoint: Box::new(RuntimeCheckpoint {
                                frontier: settled.seq,
                                runtime_source_hash: "a".repeat(64),
                                turn: None,
                                step: None,
                            }),
                        },
                    ))
                    .unwrap();
                storage
                    .append_timeline_event(&info, &checkpoint)
                    .await
                    .unwrap();
            }
        }
        let publish = dense.record(observation("model-published")).unwrap();
        storage
            .append_timeline_event_durable(&info, &publish)
            .await
            .unwrap();
        let tail = dense
            .record(model_change_event(
                &ModelId::new("mock/128"),
                &ModelId::new("mock/current"),
                None,
                None,
                "wire",
                "wire",
                &route,
                &route,
                "user_selection",
                None,
            ))
            .unwrap();
        storage.append_timeline_event(&info, &tail).await.unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .manifest
                .retained_records,
            0
        );
        assert_eq!(loaded.timeline.events().len(), 3);
        assert_eq!(
            current_model_selection(loaded.timeline.events()).unwrap(),
            latest_model_selection(dense.events()).unwrap()
        );
        let frontier = checkpoint_frontier.unwrap();
        let checkpoint_model = model_selection_at_frontier(loaded.timeline.events(), frontier)
            .unwrap()
            .unwrap();
        assert_eq!(checkpoint_model.0, ModelId::new("mock/64"));
        let receipts = durable_model_control_receipts(loaded.timeline.events()).unwrap();
        assert_eq!(
            receipts
                .iter()
                .map(|receipt| receipt.intent.sequence)
                .collect::<Vec<_>>(),
            [64, 128]
        );
        assert!(
            loaded
                .timeline
                .exact_event(chat_state::EventSeq::new(0))
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn published_auxiliary_sidebands_keep_only_unfinished_work() {
        use chat_state::{
            SidebandAttempt, SidebandBudgetPolicy, SidebandEnd, SidebandEventKind, SidebandOutcome,
            SidebandPurpose, SidebandRequest, SidebandRoute, SidebandSpawnEvent, SidebandTimeline,
        };

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let directory = storage.open_session(&info).unwrap().directory_handle();
        let mut dense = chat_state::Timeline::default();
        let mut first = None;
        let mut unfinished = String::new();
        for index in 0..=16 {
            let sideband_id = uuid::Uuid::now_v7().to_string();
            let purpose = [
                SidebandPurpose::SideQuestion,
                SidebandPurpose::PermissionJudgment,
                SidebandPurpose::MemoryFlush,
                SidebandPurpose::LazinessJudgment,
            ][index % 4];
            let spawn = SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose,
                source_refs: Vec::new(),
            };
            let event = dense
                .record(TimelineEventKind::Sideband(spawn.clone()))
                .unwrap();
            first.get_or_insert((event.seq, spawn));
            storage
                .append_timeline_event_durable(&info, &event)
                .await
                .unwrap();
            let mut ledger = SidebandTimeline::new(sideband_id.clone()).unwrap();
            let request = ledger
                .prepare(SidebandEventKind::Request(SidebandRequest {
                    purpose,
                    prompt: "side question".into(),
                    source_refs: Vec::new(),
                    budget_policy: SidebandBudgetPolicy {
                        max_attempts: 1,
                        max_input_tokens_per_attempt: 16,
                        max_output_tokens_per_attempt: None,
                    },
                    route: SidebandRoute {
                        model: "mock".into(),
                        backend: sampling_types::ApiBackend::Responses,
                    },
                    initiator_ref: format!("t:{}/sideband:{sideband_id}", info.id),
                    executor: "main".into(),
                    output_schema: None,
                }))
                .unwrap();
            storage
                .append_sideband_event_durable(&info, &request)
                .await
                .unwrap();
            ledger.accept(request).unwrap();
            if index == 16 {
                unfinished = sideband_id;
                continue;
            }
            let terminal = ledger
                .prepare(SidebandEventKind::End(SidebandEnd {
                    outcome: SidebandOutcome::Cancelled,
                    error: Some("cancelled".into()),
                }))
                .unwrap();
            storage
                .append_sideband_event_durable(&info, &terminal)
                .await
                .unwrap();
        }

        let source_ref = chat_state::TimelineRangeRef {
            timeline_id: info.id.to_string(),
            first_seq: 0,
            last_seq: dense.last_seq().unwrap().get(),
        };
        let mut sealed_context_recall_ids = Vec::new();
        let mut legacy_context_recall_id = String::new();
        for (sealed, outcome) in [
            (true, SidebandOutcome::Completed),
            (false, SidebandOutcome::Completed),
            (true, SidebandOutcome::Cancelled),
        ] {
            let sideband_id = uuid::Uuid::now_v7().to_string();
            let spawn = SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose: SidebandPurpose::ContextRecall,
                source_refs: vec![source_ref.clone()],
            };
            let spawn_event = dense.record(TimelineEventKind::Sideband(spawn)).unwrap();
            storage
                .append_timeline_event_durable(&info, &spawn_event)
                .await
                .unwrap();
            let mut ledger = SidebandTimeline::new(sideband_id.clone()).unwrap();
            let request = ledger
                .prepare(SidebandEventKind::Request(SidebandRequest {
                    purpose: SidebandPurpose::ContextRecall,
                    prompt: "recall from the frozen branch".into(),
                    source_refs: vec![source_ref.clone()],
                    budget_policy: SidebandBudgetPolicy {
                        max_attempts: 1,
                        max_input_tokens_per_attempt: 16,
                        max_output_tokens_per_attempt: Some(8),
                    },
                    route: SidebandRoute {
                        model: "mock".into(),
                        backend: sampling_types::ApiBackend::Responses,
                    },
                    initiator_ref: format!("t:{}/sideband:{sideband_id}", info.id),
                    executor: "main".into(),
                    output_schema: None,
                }))
                .unwrap();
            storage
                .append_sideband_event_durable(&info, &request)
                .await
                .unwrap();
            ledger.accept(request).unwrap();
            let attempt = ledger
                .prepare(SidebandEventKind::Attempt(SidebandAttempt {
                    attempt_no: 1,
                    input_refs: vec![source_ref.clone()],
                    assembly_manifest: chat_state::SidebandAssemblyManifest {
                        strategy: "hybrid-causal-units".into(),
                        strategy_version: 1,
                        source_revision: Some(0),
                        context_surface_ids: Vec::new(),
                        selected_surface_ids: Vec::new(),
                        materialized_input_tokens: 8,
                        max_output_tokens: Some(8),
                    },
                    feedback: None,
                }))
                .unwrap();
            storage
                .append_sideband_event_durable(&info, &attempt)
                .await
                .unwrap();
            ledger.accept(attempt).unwrap();
            let result = ledger
                .prepare(SidebandEventKind::Result(chat_state::SidebandResult {
                    raw_output: "recall result".into(),
                    structured_output: None,
                    usage: chat_state::SidebandUsage::default(),
                    finish: "stop".into(),
                    source_event_seqs: [0, 1],
                    evidence_refs: Vec::new(),
                }))
                .unwrap();
            storage
                .append_sideband_event_durable(&info, &result)
                .await
                .unwrap();
            ledger.accept(result).unwrap();
            if sealed {
                let observation = ledger
                    .prepare_recall_materialization_observation(source_ref.clone(), 0)
                    .unwrap();
                let observation = ledger.prepare(observation).unwrap();
                storage
                    .append_sideband_event_durable(&info, &observation)
                    .await
                    .unwrap();
                ledger.accept(observation).unwrap();
                sealed_context_recall_ids.push(sideband_id.clone());
            } else {
                legacy_context_recall_id = sideband_id.clone();
            }
            let terminal = ledger
                .prepare(SidebandEventKind::End(SidebandEnd {
                    outcome,
                    error: (outcome == SidebandOutcome::Cancelled)
                        .then(|| "cancelled after materialization observation".into()),
                }))
                .unwrap();
            storage
                .append_sideband_event_durable(&info, &terminal)
                .await
                .unwrap();
            ledger.accept(terminal).unwrap();
        }
        let publish = dense.record(observation("sidebands-published")).unwrap();
        storage
            .append_timeline_event_durable(&info, &publish)
            .await
            .unwrap();
        let archived_id = first.as_ref().unwrap().1.sideband_id.clone();
        let audit_suffix = dense
            .record(TimelineEventKind::Observation(
                chat_state::ObservationEvent {
                    scope: "sampling_usage".into(),
                    name: "aux_attempt_started".into(),
                    turn: None,
                    step: None,
                    data: Some(serde_json::json!({
                        "key": {"sideband_id": archived_id, "attempt_no": 1},
                        "model_id": "mock",
                        "captured_prompt_index": 0
                    })),
                },
            ))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &audit_suffix)
            .await
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .manifest
                .retained_records,
            0
        );
        assert_eq!(
            loaded
                .timeline
                .events()
                .iter()
                .filter(|event| matches!(&event.kind, TimelineEventKind::Sideband(_)))
                .count(),
            2
        );
        let retained_sideband_ids = loaded
            .timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                TimelineEventKind::Sideband(spawn) => Some(spawn.sideband_id.as_str()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert!(retained_sideband_ids.contains(legacy_context_recall_id.as_str()));
        assert!(retained_sideband_ids.contains(unfinished.as_str()));
        for sealed_id in &sealed_context_recall_ids {
            assert!(!retained_sideband_ids.contains(sealed_id.as_str()));
        }
        assert_eq!(
            chat_state::actor::state::project_timeline_usage(&loaded.timeline).unwrap(),
            chat_state::actor::state::project_timeline_usage(&dense).unwrap(),
        );
        let (first_seq, first_spawn) = first.unwrap();
        assert!(loaded.timeline.exact_event(first_seq).unwrap().is_some());
        assert!(matches!(
            loaded
                .timeline
                .prepare(TimelineEventKind::Sideband(first_spawn)),
            Err(chat_state::TimelineError::DuplicateSidebandSpawn(_))
        ));
        let ledger = JsonlStorageAdapter::read_sideband_ledger(&directory, &unfinished)
            .unwrap()
            .unwrap();
        assert!(SidebandTimeline::from_events(ledger).unwrap().is_ended());
        assert_eq!(
            storage
                .read_sideband_ledgers_sync(&info, &dense)
                .unwrap()
                .len(),
            20
        );
        let followup = loaded
            .timeline
            .prepare(observation("sidebands-published-again"))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &followup)
            .await
            .unwrap();
        drop(loaded);
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let republished = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let republished_sideband_ids = republished
            .timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                TimelineEventKind::Sideband(spawn) => Some(spawn.sideband_id.as_str()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(republished_sideband_ids.len(), 1);
        assert!(republished_sideband_ids.contains(legacy_context_recall_id.as_str()));
        for sealed_id in &sealed_context_recall_ids {
            assert!(!republished_sideband_ids.contains(sealed_id.as_str()));
        }
    }

    #[tokio::test]
    async fn published_planning_restores_only_the_latest_revision() {
        use chat_state::timeline::PlanningEvent;
        use tool_types::planning::{
            LocalPlan, PlanPriority, PlanStep, PlanStepStatus, PlanUpdateMode,
        };

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        let mut plan = LocalPlan::new(info.id.to_string(), "work");
        let mut previous = None;
        let mut first = None;
        for index in 0..128 {
            plan.update_steps(
                PlanUpdateMode::Merge,
                vec![PlanStep {
                    id: "step".into(),
                    content: format!("revision {index}"),
                    priority: PlanPriority::Normal,
                    status: PlanStepStatus::Pending,
                    requirements: None,
                    removed: false,
                }],
            )
            .unwrap();
            let event = timeline
                .record(TimelineEventKind::Planning(PlanningEvent {
                    previous,
                    plan: Box::new(plan.clone()),
                }))
                .unwrap();
            first.get_or_insert(event.seq);
            previous = Some(event.seq);
            storage.append_timeline_event(&info, &event).await.unwrap();
        }
        let publish = timeline.record(observation("planning-published")).unwrap();
        storage
            .append_timeline_event_durable(&info, &publish)
            .await
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(
            loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .manifest
                .retained_records,
            0
        );
        assert_eq!(loaded.timeline.events().len(), 1);
        assert_eq!(
            loaded.timeline.planning_snapshot().unwrap().seq,
            previous.unwrap()
        );
        assert_eq!(loaded.timeline.planning_snapshot().unwrap().plan, plan);
        assert!(
            loaded
                .timeline
                .exact_event(first.unwrap())
                .unwrap()
                .is_some()
        );
        assert!(
            loaded
                .timeline
                .prepare(TimelineEventKind::Planning(PlanningEvent {
                    previous: first,
                    plan: Box::new(plan.clone()),
                }))
                .is_err()
        );

        let event = timeline
            .record(TimelineEventKind::Planning(PlanningEvent {
                previous,
                plan: Box::new(plan.clone()),
            }))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &event)
            .await
            .unwrap();
        drop(loaded);
        drop(storage);
        let loaded = JsonlStorageAdapter::with_root(root.path().to_path_buf())
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(loaded.timeline.planning_snapshot().unwrap().seq, event.seq);
        assert_eq!(loaded.timeline.planning_snapshot().unwrap().plan, plan);
    }

    #[tokio::test]
    async fn indexed_consumed_echo_survives_a_split_publication() {
        use chat_state::{
            HookAggregateDecision, HookCause, HookEvent, HookEventType, HookGateDecision,
            InputAdmissionDecision, InputEvent, InputIntent, InputRoute, UserDisplayEcho,
        };

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let directory = storage.open_session(&info).unwrap().directory_handle();
        let payload_ref = crate::session::input_inbox::write_payload(
            &directory,
            &crate::session::input_inbox::tests::text_payload("prompt", "hello"),
        )
        .unwrap();
        let mut dense = chat_state::Timeline::default();
        let turn = TurnId(1);
        let mut start = turn_start(turn, "hello", 0, 0);
        let TimelineEventKind::Turn(TurnEvent::Started {
            input_ids,
            identity,
            ..
        }) = &mut start
        else {
            unreachable!()
        };
        input_ids.push("input".into());
        identity.turn_kind = "user".into();
        for kind in [
            TimelineEventKind::Input(InputEvent::Submitted {
                input_id: "input".into(),
                intent: InputIntent::Prompt,
                payload_ref,
            }),
            TimelineEventKind::Hook(HookEvent::Triggered {
                occurrence_id: "hook".into(),
                event: HookEventType::UserPromptSubmit,
                gate: HookEventType::UserPromptSubmit.gate(),
                cause: HookCause::Input {
                    input_id: "input".into(),
                },
                config_generation: 1,
                handlers: Vec::new(),
            }),
            TimelineEventKind::Hook(HookEvent::Completed {
                occurrence_id: "hook".into(),
                decision: HookAggregateDecision::Prompt {
                    decision: HookGateDecision::Allow,
                },
            }),
            TimelineEventKind::Input(InputEvent::AdmissionResolved {
                input_id: "input".into(),
                decision: InputAdmissionDecision::Allow,
                route: Some(InputRoute::Fifo),
                supersedes: Vec::new(),
            }),
            start,
        ] {
            let event = dense.record(kind).unwrap();
            storage
                .append_timeline_event_durable(&info, &event)
                .await
                .unwrap();
        }
        let block = acp::ContentBlock::Text(acp::TextContent::new("hello"));
        let update = SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            info.id.clone(),
            acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(block.clone()).meta(Some(
                serde_json::json!({"messageId": "echo", "promptIndex": 0, "grow/consumedInputEcho": true})
                    .as_object().unwrap().clone(),
            ))),
        )));
        storage.append_update(&info, &update).await.unwrap();
        let published = dense.record(observation("before-consume")).unwrap();
        storage
            .append_timeline_event_durable(&info, &published)
            .await
            .unwrap();
        let before = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let before_page = before
            .history
            .indexed_pages(
                before
                    .history
                    .prepare_indexed(&info.id, &before.timeline)
                    .unwrap(),
            )
            .unwrap()
            .unwrap();
        assert!(before_page.is_empty().unwrap());
        drop(before_page);
        drop(before);

        let mut item = sampling_types::ConversationItem::user("hello");
        item.set_prompt_index(0);
        let mut canonical = serde_json::json!([block]);
        canonical.sort_all_objects();
        let consumed = dense
            .record(TimelineEventKind::Input(InputEvent::Consumed {
                input_ids: vec!["input".into()],
                turn,
                item,
                skill_loads: Vec::new(),
                display_echo: Some(UserDisplayEcho {
                    message_id: "echo".into(),
                    prompt_index: 0,
                    block_count: 1,
                    blake3: blake3::hash(&serde_json::to_vec(&canonical).unwrap())
                        .to_hex()
                        .to_string(),
                }),
            }))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &consumed)
            .await
            .unwrap();
        for publish in [false, true] {
            if publish {
                let event = dense.record(observation("after-consume")).unwrap();
                storage
                    .append_timeline_event_durable(&info, &event)
                    .await
                    .unwrap();
            }
            let cold = storage
                .load_session_for_write_with_indexed_history(&info)
                .await
                .unwrap();
            assert_eq!(cold.timeline.surface_ids(), dense.surface_ids());
            assert_eq!(cold.timeline.event(consumed.seq).is_none(), publish);
            assert!(cold.timeline.exact_event(consumed.seq).unwrap().is_some());
            let page = cold
                .history
                .indexed_pages(
                    cold.history
                        .prepare_indexed(&info.id, &cold.timeline)
                        .unwrap(),
                )
                .unwrap()
                .unwrap();
            let (_, updates) = page
                .page_before(&info.id, history::PagePosition::newest())
                .unwrap();
            assert_eq!(updates.len(), 1);
            let SessionUpdate::Acp(row) = &updates[0] else {
                panic!("expected user echo")
            };
            assert!(
                matches!(&row.update, acp::SessionUpdate::UserMessageChunk(chunk) if chunk.content == block)
            );
        }
    }

    #[tokio::test]
    async fn unpublished_startup_rebuild_uses_the_working_cut() {
        use chat_state::{MessageCause, Timeline};
        use sampling_types::ConversationItem;
        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline = Timeline::from_seed(vec![ConversationItem::system("system")]).unwrap();
        storage
            .append_timeline_event_durable(&info, &timeline.events()[0])
            .await
            .unwrap();
        let publication = timeline
            .record(observation("published diagnostic"))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &publication)
            .await
            .unwrap();
        let rebuild = timeline
            .replace_all(
                vec![
                    ConversationItem::system("system"),
                    ConversationItem::project_instructions("new project context"),
                ],
                MessageCause::ContextRebuild,
            )
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &rebuild)
            .await
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let index = loaded.history.indexed.as_ref().unwrap();
        assert_eq!(index.published_domain(), rebuild.seq.get());
        assert_eq!(loaded.timeline.history_branch(), Some(rebuild.seq));
        assert_eq!(loaded.history.response_branch, Some(rebuild.seq));
        assert_eq!(loaded.timeline.surface_ids(), timeline.surface_ids());
        assert_eq!(loaded.timeline.events().len(), 1);
        assert_eq!(loaded.timeline.events()[0].seq, rebuild.seq);
    }

    #[tokio::test]
    async fn published_response_candidate_binds_a_suffix_admission() {
        use super::super::{AppendDurability, ledger::AppendFact};

        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let response = super::super::tests::response_projection_production_timeline_with_items(
            "split-response",
            3,
            vec![sampling_types::ConversationItem::assistant(
                "canonical response",
            )],
            0,
        );
        let mut timeline = chat_state::Timeline::default();
        let rebuild = timeline
            .replace_all(
                vec![sampling_types::ConversationItem::system("startup context")],
                MessageCause::ContextRebuild,
            )
            .unwrap();
        for event in response.events() {
            timeline.record(event.kind.clone()).unwrap();
        }
        let (admission, predecessors) = timeline.events().split_last().unwrap();
        let key = history::ResponseKey {
            request_id: "split-response".into(),
            attempt: 3,
        };
        let candidate = history::HistoryFact {
            meta: serde_json::json!({"eventId": "original-candidate"})
                .as_object()
                .cloned(),
            content: HistoryContent::Candidate(key.clone()),
        };
        // A future candidate is legal presentation. Its eventual admission
        // determines the branch, while the physical anchor stays here.
        assert!(
            storage
                .append_journal_fact(
                    &info,
                    AppendFact::Presentation(candidate.clone()),
                    AppendDurability::Buffered,
                )
                .await
                .unwrap()
        );
        for event in predecessors {
            storage.append_timeline_event(&info, event).await.unwrap();
        }
        let notice = history::HistoryFact {
            meta: None,
            content: HistoryContent::ToolCall(
                acp::ToolCall::new("after-candidate", "after candidate")
                    .status(acp::ToolCallStatus::Completed),
            ),
        };
        storage
            .append_journal_fact(
                &info,
                AppendFact::Presentation(notice),
                AppendDurability::Buffered,
            )
            .await
            .unwrap();
        let publish = history::HistoryFact {
            meta: None,
            content: HistoryContent::User(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new("x".repeat(MAX_UNPUBLISHED_BYTES as usize)),
            ))),
        };
        storage
            .append_journal_fact(
                &info,
                AppendFact::Presentation(publish),
                AppendDurability::Durable,
            )
            .await
            .unwrap();
        let admission = admission.clone();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(loaded.timeline.history_branch(), Some(rebuild.seq));
        assert!(
            loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .response_candidate(&key)
                .unwrap()
                .is_some()
        );
        assert!(!loaded.history.sources.iter().any(|source| matches!(
            source, history::HistorySource::Presentation { fact, .. } if matches!(fact.content, HistoryContent::Candidate(_))
        )));
        assert!(
            !storage
                .append_journal_fact(
                    &info,
                    AppendFact::Presentation(candidate),
                    AppendDurability::Buffered,
                )
                .await
                .unwrap()
        );
        storage
            .append_timeline_event_durable(&info, &admission)
            .await
            .unwrap();
        drop(loaded);
        drop(storage);

        for publish in [false, true] {
            let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
            if publish {
                let event = timeline.record(observation("response-published")).unwrap();
                storage
                    .append_timeline_event_durable(&info, &event)
                    .await
                    .unwrap();
            }
            let loaded = storage
                .load_session_for_write_with_indexed_history(&info)
                .await
                .unwrap();
            let prepared = loaded
                .history
                .prepare_indexed(&info.id, &loaded.timeline)
                .unwrap();
            let pages = loaded.history.indexed_pages(prepared).unwrap().unwrap();
            let (_, updates) = pages
                .page_before(&info.id, history::PagePosition::newest())
                .unwrap();
            let serialized = serde_json::to_string(&updates).unwrap();
            assert_eq!(serialized.matches("canonical response").count(), 1);
            assert!(
                serialized.find("canonical response").unwrap()
                    < serialized.find("after candidate").unwrap()
            );
        }
    }

    #[tokio::test]
    async fn indexed_response_page_reads_canonical_body_and_rejects_corruption() {
        use std::io::Seek as _;

        let root = tempfile::tempdir().unwrap();
        let info = Info {
            id: acp::SessionId::new("indexed-response"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline = super::super::tests::response_projection_production_timeline_with_items(
            "indexed-request",
            3,
            vec![
                sampling_types::ConversationItem::Reasoning(
                    sampling_types::synthesized_reasoning_item("original reasoning"),
                ),
                sampling_types::ConversationItem::assistant("original response"),
            ],
            0,
        );
        let response_seq = timeline.last_seq().unwrap().get();
        let filler = timeline
            .record(chat_state::TimelineEventKind::Observation(
                chat_state::ObservationEvent {
                    scope: "shell".into(),
                    name: "response publication".into(),
                    turn: None,
                    step: None,
                    data: Some(serde_json::json!(
                        "x".repeat(MAX_UNPUBLISHED_BYTES as usize)
                    )),
                },
            ))
            .unwrap();
        for event in &timeline.events()[..timeline.events().len() - 1] {
            storage.append_timeline_event(&info, event).await.unwrap();
        }
        storage
            .append_timeline_event_durable(&info, &filler)
            .await
            .unwrap();
        drop(storage);
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let history = loaded.history;
        let prepared = history.prepare_indexed(&info.id, &loaded.timeline).unwrap();
        let encoded = serde_json::to_string(&prepared.updates).unwrap();
        assert!(!encoded.contains("original reasoning"));
        assert!(!encoded.contains("original response"));
        let pages = history.indexed_pages(prepared).unwrap().unwrap();
        let (_, page) = pages
            .page_before(&info.id, history::PagePosition::newest())
            .unwrap();
        let encoded = serde_json::to_string(&page).unwrap();
        assert!(encoded.contains("original reasoning"));
        assert!(encoded.contains("original response"));

        let index = history.indexed.unwrap();
        let mut suffix = index
            .directory
            .open_regular(
                OsStr::new(&suffix_index_name(index.manifest.generation)),
                "domain suffix",
            )
            .unwrap();
        let reference = working_index::read_ref(&mut suffix, response_seq).unwrap();
        let mut file = index
            .directory
            .open_read_write_existing(OsStr::new(TIMELINE_FILE), "Timeline journal")
            .unwrap();
        use std::io::Read as _;
        file.seek(io::SeekFrom::Start(reference.offset)).unwrap();
        let mut record = vec![0; reference.len as usize];
        file.read_exact(&mut record).unwrap();
        let body = b"original response";
        let start = record
            .windows(body.len())
            .position(|bytes| bytes == body)
            .unwrap();
        file.seek(io::SeekFrom::Start(reference.offset + start as u64))
            .unwrap();
        file.write_all(b"X").unwrap();
        assert!(
            pages
                .page_before(&info.id, history::PagePosition::newest())
                .is_err()
        );
    }

    #[tokio::test]
    async fn archived_inherited_text_pages_after_surface_replacement() {
        use super::super::AppendDurability;
        use super::super::ledger::AppendFact;
        use chat_state::{MessageCause, Timeline};
        use sampling_types::ConversationItem;
        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let large = "好\u{0001}\"".repeat(800_000);
        let text = std::iter::once(large.clone())
            .chain((1..129).map(|item| format!("inherited message {item}")))
            .collect::<Vec<_>>();
        let mut timeline = Timeline::from_seed(
            text.iter()
                .cloned()
                .map(ConversationItem::assistant)
                .collect(),
        )
        .unwrap();
        for event in timeline.events() {
            storage.append_timeline_event(&info, event).await.unwrap();
        }
        for (item, id) in timeline.surface_ids().iter().enumerate() {
            let fact = history::HistoryFact {
                meta: serde_json::json!({"eventId": format!("inherited-{item}")})
                    .as_object()
                    .cloned(),
                content: HistoryContent::Message {
                    event: id.event.get(),
                    item: id.item as usize,
                    channel: history::TextChannel::Assistant,
                    chunk_meta: serde_json::json!({"inherited": true}).as_object().cloned(),
                },
            };
            storage
                .append_journal_fact(
                    &info,
                    AppendFact::Presentation(fact),
                    AppendDurability::Buffered,
                )
                .await
                .unwrap();
        }
        let replacement = timeline
            .replace_all(
                vec![ConversationItem::user(
                    "x".repeat(MAX_UNPUBLISHED_BYTES as usize),
                )],
                MessageCause::ContextRebuild,
            )
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &replacement)
            .await
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(loaded.timeline.surface_ids(), timeline.surface_ids());
        assert_eq!(
            serde_json::to_value(loaded.timeline.surface()).unwrap(),
            serde_json::to_value(timeline.surface()).unwrap(),
        );
        assert!(loaded.history.sources.is_empty());
        assert_eq!(loaded.history.display_record_count(), text.len());
        let prepared = loaded
            .history
            .prepare_indexed(&info.id, &loaded.timeline)
            .unwrap();
        let pages = loaded.history.indexed_pages(prepared).unwrap().unwrap();
        let mut cursor = Some(history::PagePosition::newest());
        let mut chunks = Vec::new();
        while let Some(before) = cursor {
            let (older, updates) = pages.page_before(&info.id, before).unwrap();
            assert!(serde_json::to_vec(&updates).unwrap().len() <= history::HISTORY_PAGE_BYTES);
            for update in updates.into_iter().rev() {
                let SessionUpdate::Acp(notification) = update else {
                    panic!("expected inherited text")
                };
                let meta = notification.meta.unwrap();
                assert!(!meta.contains_key("samplingRequestId"));
                assert!(!meta.contains_key("responseProjection"));
                let seq = meta["grow/historyMessageEvent"].as_u64().unwrap() as usize;
                assert_eq!(meta["eventId"], format!("inherited-{seq}"));
                let acp::SessionUpdate::AgentMessageChunk(chunk) = notification.update else {
                    panic!("expected assistant chunk")
                };
                assert_eq!(chunk.meta.unwrap()["inherited"], true);
                let acp::ContentBlock::Text(body) = chunk.content else {
                    panic!("expected text")
                };
                chunks.push((seq, body.text));
            }
            assert_ne!(older, cursor);
            cursor = older;
        }
        assert!(chunks.len() > text.len());
        chunks.reverse();
        let mut reconstructed = vec![String::new(); text.len()];
        assert!(chunks.is_sorted_by_key(|(seq, _)| *seq));
        for (seq, body) in chunks {
            reconstructed[seq].push_str(&body);
        }
        assert_eq!(reconstructed, text);

        let mut forward = Vec::new();
        let mut after = history::PagePosition::oldest();
        let mut next_start = std::collections::BTreeMap::<(u64, u64), u64>::new();
        loop {
            let window = pages.read_after(&info.id, after).unwrap();
            assert!(window.rows.len() <= history::HISTORY_PAGE_ROWS);
            assert!(serde_json::to_vec(&window.rows).unwrap().len() <= history::HISTORY_PAGE_BYTES);
            if window.rows.is_empty() {
                assert!(window.range.is_none());
                break;
            }
            for row in &window.rows {
                let history::HistoryEntity::Message {
                    event_seq,
                    item_ordinal,
                } = row.entity.clone().unwrap()
                else {
                    panic!("inherited text keeps its message identity");
                };
                let fragment = row.fragment.expect("indexed text uses an encoded fragment");
                assert_eq!(
                    fragment.coordinate,
                    history::HistoryFragmentCoordinate::EncodedResponseBytes
                );
                let expected = next_start
                    .get(&(event_seq, item_ordinal))
                    .copied()
                    .unwrap_or(0);
                assert_eq!(
                    fragment.start, expected,
                    "adjacent encoded fragments must not overlap"
                );
                next_start.insert((event_seq, item_ordinal), fragment.end_exclusive);
                let SessionUpdate::Acp(notification) = &row.update else {
                    panic!("expected inherited text")
                };
                let meta = notification.meta.as_ref().unwrap();
                assert!(!meta.contains_key("samplingRequestId"));
                assert_eq!(meta["grow/historyMessageEvent"], event_seq);
                assert_eq!(meta["grow/historyMessageItem"], item_ordinal);
                let acp::SessionUpdate::AgentMessageChunk(chunk) = &notification.update else {
                    panic!("expected assistant chunk")
                };
                let acp::ContentBlock::Text(body) = &chunk.content else {
                    panic!("expected text")
                };
                forward.push((event_seq, item_ordinal, body.text.clone()));
            }
            match window.newer_position {
                Some(next) => {
                    assert_ne!(next, after);
                    after = next;
                }
                None => break,
            }
        }
        let mut backward = Vec::new();
        let mut before = history::PagePosition::newest();
        loop {
            let window = pages.read_before(&info.id, before).unwrap();
            if window.rows.is_empty() {
                break;
            }
            for row in window.rows.iter().rev() {
                let SessionUpdate::Acp(notification) = &row.update else {
                    panic!("expected inherited text")
                };
                let meta = notification.meta.as_ref().unwrap();
                let event = meta["grow/historyMessageEvent"].as_u64().unwrap();
                let item = meta["grow/historyMessageItem"].as_u64().unwrap();
                let acp::SessionUpdate::AgentMessageChunk(chunk) = &notification.update else {
                    panic!("expected assistant chunk")
                };
                let acp::ContentBlock::Text(body) = &chunk.content else {
                    panic!("expected text")
                };
                backward.push((event, item, body.text.clone()));
            }
            match window.older_position {
                Some(older) => {
                    assert_ne!(older, before);
                    before = older;
                }
                None => break,
            }
        }
        backward.reverse();
        assert!(forward.len() > text.len());
        // Envelope and page budgets differ per direction, so fragment cuts are
        // allowed to differ; only the per-message reassembly must agree.
        let reassemble = |chunks: Vec<(u64, u64, String)>| {
            let mut messages = std::collections::BTreeMap::<(u64, u64), String>::new();
            for (event, item, body) in chunks {
                messages.entry((event, item)).or_default().push_str(&body);
            }
            messages
        };
        for (direction, messages) in [
            ("forward", reassemble(forward)),
            ("backward", reassemble(backward)),
        ] {
            let reassembled: Vec<String> = messages.into_values().collect();
            assert_eq!(
                reassembled.len(),
                text.len(),
                "{direction} reassembly covers {} canonical messages, expected {}",
                reassembled.len(),
                text.len()
            );
            for (idx, (got, want)) in reassembled.iter().zip(text.iter()).enumerate() {
                assert_eq!(
                    got.len(),
                    want.len(),
                    "{direction} message {idx} reassembled to {} bytes, expected {}",
                    got.len(),
                    want.len()
                );
                if got != want {
                    let diff = got
                        .bytes()
                        .zip(want.bytes())
                        .position(|(a, b)| a != b)
                        .unwrap_or(got.len());
                    panic!("{direction} message {idx} diverges from source at byte {diff}");
                }
            }
        }
    }

    #[tokio::test]
    async fn indexed_response_continues_at_utf8_byte_boundaries() {
        let root = tempfile::tempdir().unwrap();
        let info = Info {
            id: acp::SessionId::new("large-indexed-response"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let original = "好\u{0001}\"".repeat(800_000);
        let timeline = super::super::tests::response_projection_production_timeline_with_items(
            "large-request",
            1,
            vec![sampling_types::ConversationItem::assistant(
                original.clone(),
            )],
            0,
        );
        for event in timeline.events() {
            storage.append_timeline_event(&info, event).await.unwrap();
        }
        drop(storage);
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let history = loaded.history;
        let prepared = history.prepare_indexed(&info.id, &loaded.timeline).unwrap();
        let pages = history.indexed_pages(prepared).unwrap().unwrap();
        let mut end = Some(history::PagePosition::newest());
        let mut parts = Vec::new();
        while let Some(before) = end {
            let (next, updates) = pages.page_before(&info.id, before).unwrap();
            assert!(
                serde_json::to_vec(&updates).unwrap().len()
                    <= crate::session::history::HISTORY_PAGE_BYTES
            );
            for update in updates.into_iter().rev() {
                let SessionUpdate::Acp(notification) = update else {
                    panic!("expected response")
                };
                let acp::SessionUpdate::AgentMessageChunk(chunk) = notification.update else {
                    panic!("expected assistant chunk")
                };
                let acp::ContentBlock::Text(text) = chunk.content else {
                    panic!("expected text")
                };
                parts.push(text.text);
            }
            assert_ne!(next, end);
            end = next;
        }
        assert!(parts.len() > 1);
        parts.reverse();
        assert_eq!(parts.concat(), original);

        let mut after = history::PagePosition::oldest();
        let mut forward = String::new();
        let mut covered = std::collections::BTreeMap::<history::HistorySourceKey, u64>::new();
        loop {
            let window = pages.read_after(&info.id, after).unwrap();
            if window.rows.is_empty() {
                break;
            }
            for row in &window.rows {
                let history::HistoryEntity::Message { .. } = row.entity.clone().unwrap() else {
                    panic!("canonical response keeps message identity");
                };
                let fragment = row
                    .fragment
                    .expect("indexed response bytes stay on their own coordinate");
                assert_eq!(
                    fragment.coordinate,
                    history::HistoryFragmentCoordinate::EncodedResponseBytes
                );
                assert_eq!(
                    fragment.start,
                    covered.get(&row.source).copied().unwrap_or(0)
                );
                covered.insert(row.source, fragment.end_exclusive);
                let SessionUpdate::Acp(notification) = &row.update else {
                    panic!("expected response")
                };
                assert!(
                    notification
                        .meta
                        .as_ref()
                        .unwrap()
                        .contains_key("samplingRequestId")
                );
                let acp::SessionUpdate::AgentMessageChunk(chunk) = &notification.update else {
                    panic!("expected assistant chunk")
                };
                let acp::ContentBlock::Text(text) = &chunk.content else {
                    panic!("expected text")
                };
                forward.push_str(&text.text);
            }
            match window.newer_position {
                Some(next) => after = next,
                None => break,
            }
        }
        assert_eq!(forward, original);
    }

    #[tokio::test]
    async fn paged_tool_terminal_recovers_fields_from_before_the_page() {
        use super::super::AppendDurability;
        use super::super::ledger::AppendFact;

        let root = tempfile::tempdir().unwrap();
        let info = Info {
            id: acp::SessionId::new("paged-tool"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let tool = acp::ToolCallId::new("old-tool");
        let mut facts = vec![
            HistoryContent::ToolCall(acp::ToolCall::new(tool.clone(), "original")),
            HistoryContent::ToolUpdate(acp::ToolCallUpdate::new(
                tool.clone(),
                acp::ToolCallUpdateFields::new()
                    .status(acp::ToolCallStatus::InProgress)
                    .title("updated before the page".to_owned()),
            )),
        ];
        facts.extend((0..160).map(|n| {
            HistoryContent::ToolCall(
                acp::ToolCall::new(format!("tool-{n}"), "other")
                    .status(acp::ToolCallStatus::Completed),
            )
        }));
        facts.push(HistoryContent::ToolUpdate(acp::ToolCallUpdate::new(
            tool.clone(),
            acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed),
        )));
        for content in facts {
            storage
                .append_journal_fact(
                    &info,
                    AppendFact::Presentation(history::HistoryFact {
                        meta: None,
                        content,
                    }),
                    AppendDurability::Buffered,
                )
                .await
                .unwrap();
        }
        let filler = chat_state::Timeline::default()
            .record(chat_state::TimelineEventKind::Observation(
                chat_state::ObservationEvent {
                    scope: "shell".into(),
                    name: "page publication".into(),
                    turn: None,
                    step: None,
                    data: Some(serde_json::json!(
                        "x".repeat(MAX_UNPUBLISHED_BYTES as usize)
                    )),
                },
            ))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &filler)
            .await
            .unwrap();
        drop(storage);
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert!(loaded.history.sources.is_empty());
        let prepared = loaded
            .history
            .prepare(&info.id, &loaded.timeline, None)
            .unwrap();
        let pages = loaded.history.indexed_pages(prepared).unwrap().unwrap();
        let (start, updates) = pages
            .page_before(&info.id, history::PagePosition::newest())
            .unwrap();
        assert!(start.is_some());
        let Some(SessionUpdate::Acp(notification)) = updates.last() else {
            panic!("expected Tool completion");
        };
        let acp::SessionUpdate::ToolCall(call) = &notification.update else {
            panic!("page completion must carry its canonical display base");
        };
        assert_eq!(call.tool_call_id, tool);
        assert_eq!(call.title, "updated before the page");
        assert_eq!(call.status, acp::ToolCallStatus::Completed);
    }

    #[tokio::test]
    async fn indexed_tool_output_continues_by_bounded_page_cursor() {
        use super::super::AppendDurability;
        use super::super::ledger::AppendFact;

        let root = tempfile::tempdir().unwrap();
        let info = Info {
            id: acp::SessionId::new("paged-large-tool-output"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();

        let tool = acp::ToolCallId::new("large-tool");
        let output = format!("large-output-界:{}", "x".repeat(1_400_000));
        let facts = [
            HistoryContent::ToolCall(acp::ToolCall::new(tool.clone(), "Large output tool")),
            HistoryContent::ToolUpdate(acp::ToolCallUpdate::new(
                tool.clone(),
                acp::ToolCallUpdateFields::new()
                    .status(acp::ToolCallStatus::InProgress)
                    .title("Large output tool".to_owned()),
            )),
            HistoryContent::ToolUpdate(acp::ToolCallUpdate::new(
                tool.clone(),
                acp::ToolCallUpdateFields::new()
                    .status(acp::ToolCallStatus::Completed)
                    .raw_output(Some(serde_json::json!({"text": output}))),
            )),
        ];
        for content in facts {
            storage
                .append_journal_fact(
                    &info,
                    AppendFact::Presentation(history::HistoryFact {
                        meta: None,
                        content,
                    }),
                    AppendDurability::Buffered,
                )
                .await
                .unwrap();
        }
        let filler = chat_state::Timeline::default()
            .record(chat_state::TimelineEventKind::Observation(
                chat_state::ObservationEvent {
                    scope: "shell".into(),
                    name: "page publication".into(),
                    turn: None,
                    step: None,
                    data: Some(serde_json::json!(
                        "x".repeat(MAX_UNPUBLISHED_BYTES as usize)
                    )),
                },
            ))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &filler)
            .await
            .unwrap();
        drop(storage);

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let history = loaded.history;
        let prepared = history.prepare(&info.id, &loaded.timeline, None).unwrap();
        let pages = history.indexed_pages(prepared).unwrap().unwrap();
        let mut before = Some(history::PagePosition::newest());
        let mut fragments = Vec::new();
        let mut digest = None;
        let mut total_bytes = None;
        let mut fragment_pages = 0;
        while let Some(position) = before {
            let (older, updates) = pages.page_before(&info.id, position).unwrap();
            assert!(serde_json::to_vec(&updates).unwrap().len() <= history::HISTORY_PAGE_BYTES);
            for update in updates {
                if let SessionUpdate::Grow(notification) = update {
                    let encoded_len = serde_json::to_vec(&*notification).unwrap().len();
                    if let crate::extensions::notification::SessionUpdate::ToolOutputFragment(
                        fragment,
                    ) = notification.update
                    {
                        assert!(
                            encoded_len <= 1024 * 1024,
                            "Tool byte page exceeded the per-fragment response budget",
                        );
                        assert_eq!(fragment.tool_call_id, tool);
                        assert_eq!(fragment.title, "Large output tool");
                        assert_eq!(fragment.status, acp::ToolCallStatus::Completed);
                        if let Some(digest) = &digest {
                            assert_eq!(&fragment.digest, digest);
                        } else {
                            digest = Some(fragment.digest);
                        }
                        if let Some(total_bytes) = total_bytes {
                            assert_eq!(fragment.total_bytes, total_bytes);
                        } else {
                            total_bytes = Some(fragment.total_bytes);
                        }
                        fragments.push(fragment.json);
                        fragment_pages += 1;
                    }
                }
            }
            assert_ne!(older, before);
            before = older;
        }
        assert!(fragment_pages > 1);
        fragments.reverse();
        let json = fragments.concat();
        let digest = digest.expect("before pages published a Tool digest");
        let total_bytes = total_bytes.expect("before pages published a Tool length");
        assert_eq!(json.len() as u64, total_bytes);
        assert_eq!(blake3::hash(json.as_bytes()).to_hex().as_str(), digest);
        let body: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(body["rawOutput"]["text"], output);
        let mut after = history::PagePosition::oldest();
        let mut forward = String::new();
        let mut saw_terminal = false;
        let mut next_byte = 0u64;
        loop {
            let window = pages.read_after(&info.id, after).unwrap();
            assert!(serde_json::to_vec(&window.rows).unwrap().len() <= history::HISTORY_PAGE_BYTES);
            if window.rows.is_empty() {
                break;
            }
            for row in &window.rows {
                match &row.update {
                    SessionUpdate::Grow(notification) => {
                        let crate::extensions::notification::SessionUpdate::ToolOutputFragment(
                            fragment,
                        ) = &notification.update
                        else {
                            continue;
                        };
                        let history::HistoryEntity::Tool { tool_call_id } =
                            row.entity.clone().unwrap()
                        else {
                            panic!("tool fragment identity");
                        };
                        assert_eq!(tool_call_id.as_str(), tool.0.as_ref());
                        assert_eq!(fragment.tool_call_id, tool);
                        assert_eq!(fragment.title, "Large output tool");
                        assert_eq!(fragment.status, acp::ToolCallStatus::Completed);
                        assert_eq!(fragment.digest, digest);
                        assert_eq!(fragment.total_bytes, total_bytes);
                        let envelope = row.fragment.expect("tool bytes use their own coordinate");
                        assert_eq!(
                            envelope.coordinate,
                            history::HistoryFragmentCoordinate::ToolPayloadJsonBytes
                        );
                        assert_eq!(envelope.start, fragment.byte_start);
                        assert_eq!(envelope.start, next_byte);
                        assert_eq!(
                            envelope.end_exclusive,
                            fragment.byte_start + fragment.json.len() as u64
                        );
                        next_byte = envelope.end_exclusive;
                        forward.push_str(&fragment.json);
                        saw_terminal = true;
                    }
                    SessionUpdate::Acp(notification) => {
                        if let acp::SessionUpdate::ToolCall(call) = &notification.update {
                            assert_eq!(call.tool_call_id, tool);
                            assert_eq!(call.status, acp::ToolCallStatus::Completed);
                            assert_eq!(call.title, "Large output tool");
                        }
                    }
                    _ => {}
                }
            }
            match window.newer_position {
                Some(next) => after = next,
                None => break,
            }
        }
        assert!(saw_terminal);
        assert_eq!(next_byte, total_bytes);
        assert_eq!(forward, json);
    }

    #[tokio::test]
    async fn background_migration_leaves_an_active_writer_alone() {
        let root = tempfile::tempdir().unwrap();
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let info = Info {
            id: acp::SessionId::new("legacy-background-migration"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        let seed = timeline
            .append(
                sampling_types::ConversationItem::system("fixed"),
                chat_state::MessageCause::Seed,
            )
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &seed)
            .await
            .unwrap();
        let directory = storage.open_session(&info).unwrap().directory_handle();
        let manifest = read_manifest(&directory).unwrap().unwrap();
        directory.remove_file(OsStr::new(MANIFEST), false).unwrap();
        remove_generation(&directory, manifest.generation);
        let journal = directory.display_path().join(TIMELINE_FILE);
        let canonical = std::fs::read(&journal).unwrap();
        let maintenance = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let cancel = tokio_util::sync::CancellationToken::new();
        maintenance.migrate_working_history(&cancel).unwrap();
        assert!(read_manifest(&directory).unwrap().is_none());
        drop(storage);
        maintenance.migrate_working_history(&cancel).unwrap();
        assert_eq!(read_manifest(&directory).unwrap().unwrap().domain_events, 1);
        assert_eq!(std::fs::read(&journal).unwrap(), canonical);
        let loaded = maintenance
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(loaded.timeline.next_seq(), timeline.next_seq());
        assert_eq!(
            serde_json::to_vec(loaded.timeline.surface()).unwrap(),
            serde_json::to_vec(timeline.surface()).unwrap(),
        );
    }

    #[tokio::test]
    async fn legacy_resume_installs_index_after_returning_execution_state() {
        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let directory = storage.open_session(&info).unwrap().directory_handle();
        let obsolete = read_manifest(&directory).unwrap().unwrap();
        directory.remove_file(OsStr::new(MANIFEST), false).unwrap();
        remove_generation(&directory, obsolete.generation);
        let mut timeline = chat_state::Timeline::default();
        let seed = timeline
            .append(
                sampling_types::ConversationItem::system("legacy"),
                MessageCause::Seed,
            )
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &seed)
            .await
            .unwrap();
        let loaded = storage
            .resume_session_with_indexed_history(&info)
            .await
            .unwrap();
        assert!(loaded.history.indexed.is_none());
        assert_eq!(loaded.timeline.surface_ids(), timeline.surface_ids());

        let tail = timeline.record(diagnostic()).unwrap();
        storage
            .append_timeline_event_durable(&info, &tail)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let state = storage.timeline_prefix_state(&info).unwrap();
            while !state
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|prefix| prefix.indexed.is_some())
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        drop(storage);
        let storage = JsonlStorageAdapter::with_root(root.path().into());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !storage.try_acquire_writer_lease(&info).unwrap() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let loaded = storage
            .resume_session_with_indexed_history(&info)
            .await
            .unwrap();
        assert!(loaded.history.indexed.is_some());
        assert_eq!(loaded.timeline.next_seq(), timeline.next_seq());
        assert_eq!(loaded.timeline.surface_ids(), timeline.surface_ids());
    }

    #[tokio::test]
    async fn published_sampling_body_is_verified_by_full_history_reads() {
        let root = tempfile::tempdir().unwrap();
        let info = Info {
            id: acp::SessionId::new("sampling-audit"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let directory = storage.open_session(&info).unwrap().directory_handle();
        let mut timeline = chat_state::Timeline::default();
        let body = b"historical provider bytes";
        let hash = blake3::hash(body).to_hex().to_string();
        let artifact = format!("artifacts/sampling/{hash}.bin");
        crate::session::persistence::write_immutable_blob_to_directory(
            &directory,
            Path::new(&artifact),
            body,
        )
        .unwrap();
        let audit = timeline
            .record(chat_state::TimelineEventKind::Observation(
                chat_state::ObservationEvent {
                    scope: crate::session::sampling_evidence::SCOPE.into(),
                    name: "response".into(),
                    turn: None,
                    step: None,
                    data: Some(serde_json::json!({
                        "owner": {"request_id": "old"}, "kind": "response", "metadata": {},
                        "bytes": body.len(), "chunks": [{"blake3": hash, "bytes": body.len()}],
                    })),
                },
            ))
            .unwrap();
        storage
            .append_timeline_event_durable(&info, &audit)
            .await
            .unwrap();
        let mut filler = diagnostic();
        if let chat_state::TimelineEventKind::Observation(observation) = &mut filler {
            observation.data = Some(serde_json::json!(
                "x".repeat(MAX_UNPUBLISHED_BYTES as usize)
            ));
        }
        let event = timeline.record(filler).unwrap();
        storage
            .append_timeline_event_durable(&info, &event)
            .await
            .unwrap();
        assert_eq!(read_manifest(&directory).unwrap().unwrap().domain_events, 2);
        drop(storage);
        std::fs::remove_file(directory.display_path().join(&artifact)).unwrap();

        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(loaded.timeline.next_seq(), timeline.next_seq());
        assert!(loaded.timeline.events().is_empty());
        assert_eq!(
            read_domain_range(&directory, 0..1).unwrap().unwrap()[0].seq,
            audit.seq
        );
        assert!(storage.load_session_without_updates(&info).await.is_err());
    }

    #[tokio::test]
    async fn indexed_writer_revalidates_a_rewritten_journal_of_the_same_length() {
        use std::io::{Seek, SeekFrom};

        let root = tempfile::tempdir().unwrap();
        let info = Info {
            id: acp::SessionId::new("rewritten-diagnostic"),
            cwd: root.path().to_string_lossy().into_owned(),
        };
        let storage = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        let mut timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::system(
                "fixed",
            )])
            .unwrap();
        timeline.record(diagnostic()).unwrap();
        let bytes = JsonlStorageAdapter::encode_journal(timeline.events(), &[]).unwrap();
        let directory = storage.open_session(&info).unwrap().directory_handle();
        directory
            .write_atomic(OsStr::new(TIMELINE_FILE), &bytes, true, true)
            .unwrap();
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert!(loaded.timeline.events().is_empty());
        assert_eq!(loaded.timeline.surface_len(), 1);
        drop(storage);

        let mut journal = directory
            .open_read_write_create(OsStr::new(TIMELINE_FILE))
            .unwrap();
        let payload = bytes
            .windows(8)
            .position(|part| part == b"datadata")
            .unwrap();
        journal.seek(SeekFrom::Start(payload as u64)).unwrap();
        journal.write_all(b"X").unwrap();
        assert_eq!(journal.metadata().unwrap().len(), bytes.len() as u64);

        let fresh = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let error = fresh
            .load_session_for_write_with_indexed_history(&info)
            .await
            .err()
            .expect("rewritten journal is invalid");
        assert!(
            error
                .to_string()
                .contains("Timeline record checksum mismatch"),
            "{error}"
        );
    }

    #[test]
    fn process_exit_around_index_publication_recovers_exact_tail() {
        if let Some(root) = std::env::var_os(CRASH_HOME_ENV) {
            run_phase(Path::new(&root), None);
            panic!("publication crash probe was not reached");
        }
        for stage in ["before", "after"] {
            let root = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CRASH_HOME_ENV, root.path())
                .env(CRASH_STAGE_ENV, stage)
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(73), "{stage}");
            run_phase(root.path(), Some(stage));
        }
    }

    fn run_phase(root: &Path, crashed_at: Option<&str>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let storage = JsonlStorageAdapter::with_root(root.to_path_buf());
            let info = Info {
                id: acp::SessionId::new("indexed-crash"),
                cwd: root.to_string_lossy().into_owned(),
            };
            let mut timeline = chat_state::Timeline::default();
            timeline
                .append(
                    sampling_types::ConversationItem::system("fixed"),
                    chat_state::MessageCause::Seed,
                )
                .unwrap();
            let facts = (0..300)
                .map(|n| history::HistoryFact {
                    meta: Some(acp::Meta::from_iter([
                        (
                            "eventId".into(),
                            format!("indexed-crash-{}", n + 1_000).into(),
                        ),
                        ("agentTimestampMs".into(), 123.into()),
                    ])),
                    content: HistoryContent::Grow(Box::new(history::GrowHistoryFact::UiNotice(
                        crate::extensions::notification::UiNotice {
                            correlation_id: format!("notice-{n}"),
                            category: crate::extensions::notification::UiNoticeCategory::Command,
                            subject: None,
                            description: None,
                            message: format!("history {n}"),
                            tone: crate::extensions::notification::UiNoticeTone::Info,
                            details: None,
                        },
                    ))),
                })
                .collect::<Vec<_>>();
            if crashed_at.is_none() {
                let parent = storage.ensure_session_parent(&info).unwrap();
                storage.ensure_writer_lease(&info).unwrap();
                storage
                    .build_publish_and_cache(&info, &parent, |staging| {
                        let summary = super::super::Summary::new(&info, default_model_id())?;
                        staging.write_atomic(
                            OsStr::new(crate::session::storage::SUMMARY_FILE),
                            &crate::session::storage::serialize_summary(&summary)?,
                            true,
                            false,
                        )?;
                        staging.write_atomic(
                            OsStr::new(TIMELINE_FILE),
                            &JsonlStorageAdapter::encode_journal(timeline.events(), &facts)?,
                            true,
                            false,
                        )
                    })
                    .unwrap();
                storage
                    .load_session_for_write_with_indexed_history(&info)
                    .await
                    .unwrap();
                return;
            }
            let directory = storage.open_session(&info).unwrap().directory_handle();
            let prior = read_manifest(&directory).unwrap();
            assert_eq!(prior.is_some(), crashed_at == Some("after"));
            let event = timeline
                .record(chat_state::TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: "test".into(),
                        name: "durable tail".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .unwrap();
            let raw = serde_json::value::to_raw_value(&event).unwrap();
            let committed = history::encode_domain(301, &raw, None).unwrap();
            {
                let mut journal = std::fs::OpenOptions::new()
                    .append(true)
                    .open(directory.display_path().join(TIMELINE_FILE))
                    .unwrap();
                journal.write_all(&committed).unwrap();
                journal.write_all(b"{\"torn\":").unwrap();
                journal.sync_all().unwrap();
            }
            let loaded = storage
                .load_session_for_write_with_indexed_history(&info)
                .await
                .unwrap();
            assert_eq!(loaded.timeline.next_seq(), chat_state::EventSeq::new(2));
            assert_eq!(loaded.history.record_count(), 302);
            assert_eq!(loaded.history.display_record_count(), 300);
            let index = loaded.history.indexed.as_ref().unwrap();
            if let Some(prior) = prior {
                assert_eq!(index.manifest.generation, prior.generation);
            }
            assert_eq!(
                loaded
                    .history
                    .indexed
                    .as_ref()
                    .unwrap()
                    .event_counter_floor(),
                1_300
            );
            let pages = loaded
                .history
                .indexed_pages(
                    loaded
                        .history
                        .prepare(&info.id, &loaded.timeline, None)
                        .unwrap(),
                )
                .unwrap()
                .unwrap();
            let (older, newest) = pages
                .page_before(&info.id, history::PagePosition::newest())
                .unwrap();
            assert!(older.is_some());
            assert_eq!(newest.len(), 128);
            assert!(
                serde_json::to_string(&newest.last())
                    .unwrap()
                    .contains("history 299")
            );
            let exact = facts[3].clone();
            let mut conflict = facts[4].clone();
            conflict.meta = exact.meta.clone();
            assert!(
                !storage
                    .append_journal_fact(
                        &info,
                        super::super::ledger::AppendFact::Presentation(exact),
                        super::super::AppendDurability::Durable,
                    )
                    .await
                    .unwrap()
            );
            assert_eq!(
                storage
                    .append_journal_fact(
                        &info,
                        super::super::ledger::AppendFact::Presentation(conflict),
                        super::super::AppendDurability::Durable,
                    )
                    .await
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
            let next = timeline.record(event.kind.clone()).unwrap();
            storage
                .append_timeline_event_durable(&info, &next)
                .await
                .unwrap();
            let verified = storage.read_timeline_events_sync(&info).unwrap();
            assert_eq!(verified.len(), 3);
            assert_eq!(verified.last().unwrap().seq, next.seq);

            // The newest page should not read the oldest notice. Even a
            // replacement with a valid journal checksum must fail when that
            // page is later selected.
            let mut reader = index.page_reader().unwrap();
            let first = index.tools.display_at(0).unwrap().record;
            let JournalFact::Presentation { at_ms, .. } =
                working_index::read_record(&mut reader.journal, first)
                    .unwrap()
                    .fact
            else {
                unreachable!();
            };
            let mut altered = facts[0].clone();
            let HistoryContent::Grow(update) = &mut altered.content else {
                unreachable!();
            };
            let history::GrowHistoryFact::UiNotice(notice) = update.as_mut() else {
                unreachable!();
            };
            notice.message = "history X".into();
            let raw = serde_json::value::to_raw_value(&altered).unwrap();
            let replacement = history::encode_presentation(1, at_ms, &raw).unwrap();
            assert_eq!(replacement.len() as u64, first.len + 1);
            {
                use std::io::{Seek as _, SeekFrom};
                let mut journal = directory
                    .open_read_write_create(OsStr::new(TIMELINE_FILE))
                    .unwrap();
                journal.seek(SeekFrom::Start(first.offset)).unwrap();
                journal.write_all(&replacement).unwrap();
            }
            let (older, newest) = pages
                .page_before(&info.id, history::PagePosition::newest())
                .unwrap();
            assert_eq!(newest.len(), 128);
            let (oldest, middle) = pages.page_before(&info.id, older.unwrap()).unwrap();
            assert_eq!(middle.len(), 128);
            assert_eq!(
                pages
                    .page_before(&info.id, oldest.unwrap())
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
        });
    }
}

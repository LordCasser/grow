//! Read-only, reconciled conversation sources and verified delegation links.
//!
//! A session is the smallest read unit. A tree capture pins each selected
//! display ledger and validates Timeline before callers materialize bodies.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use chat_state::{
    MessageCause, SubagentEvent, SubagentSeedEvent, Timeline, TimelineEventKind, TurnEvent,
};

use super::jsonl::{JsonlStorageAdapter, OpenedSession};
use super::{
    CommittedJsonlLines, ReconciledReplayLine, SessionUpdate, SessionUpdateEnvelope, TIMELINE_FILE,
    UPDATES_FILE, filter_rewind_lines, reconcile_raw_replay_lines, strip_context_wrappers,
};

mod activity;

const MAX_NODES: usize = 512;
const MAX_DEPTH: usize = 32;
const MAX_TIMELINE_EVENTS: usize = 250_000;
const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug)]
pub struct TranscriptEvent {
    pub update: SessionUpdate,
    /// Original persisted write time. Synthetic projection rows have no time.
    pub timestamp_ms: Option<u64>,
    pub simulated_source: bool,
}

#[derive(Debug)]
pub struct TranscriptSession {
    pub session_id: String,
    pub title: Option<String>,
    pub last_timeline_seq: Option<u64>,
    pub source_bytes: u64,
    pub events: Vec<TranscriptEvent>,
    /// Captured Timeline intervals that should remain at historical speed.
    /// They protect known model / tool / hook execution and conservative
    /// tool spans that may include permission waits.
    pub activity: Vec<std::ops::Range<u64>>,
    /// A reopened process wrote terminal facts at recovery time. These gaps
    /// are candidates for estimated skipping, never confirmed historical idle.
    pub recovery_gaps: Vec<std::ops::Range<u64>>,
    /// Operations with no captured terminal remain potentially active until
    /// the global tree frontier, including while a sibling emits updates.
    pub open_activity: Vec<u64>,
    /// An observed background task lacked enough timestamp evidence to bound
    /// its activity; a replay must not assert ordinary IDLE in this tree.
    pub unknown_activity: bool,
}

#[derive(Debug, Clone)]
pub struct TranscriptNode {
    pub session_id: String,
    pub title: Option<String>,
    pub parent_id: Option<String>,
    pub depth: usize,
    pub children: Vec<String>,
    /// The Timeline frontier used to discover this node's children.
    pub last_timeline_seq: Option<u64>,
    /// Validated lifecycle-owner spawn time, independent of the UI parent.
    pub spawn_at_ms: Option<u64>,
}

struct Facts {
    opened: OpenedSession,
    updates: Option<CapturedLedger>,
    timeline: Timeline,
    control: Option<crate::session::control::SessionControlSnapshot>,
    title: Option<String>,
    summary_parent: Option<String>,
    session_kind: Option<String>,
    seed: Option<SubagentSeedEvent>,
    last_seq: Option<u64>,
    sideband_activity: Vec<std::ops::Range<u64>>,
    selected_spawns: BTreeSet<String>,
}

#[derive(Default)]
struct Budget {
    bytes: u64,
    events: usize,
}

impl Budget {
    fn add_bytes(&mut self, bytes: u64) -> io::Result<()> {
        self.bytes = self.bytes.checked_add(bytes).ok_or_else(limit_error)?;
        if self.bytes > MAX_SOURCE_BYTES {
            return Err(limit_error());
        }
        Ok(())
    }

    fn add_events(&mut self, count: usize) -> io::Result<()> {
        self.events = self.events.checked_add(count).ok_or_else(limit_error)?;
        if self.events > MAX_TIMELINE_EVENTS {
            return Err(limit_error());
        }
        Ok(())
    }
}

fn limit_error() -> io::Error {
    io::Error::new(
        ErrorKind::InvalidData,
        "transcript source exceeds read budget",
    )
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message.into())
}

/// Pin both identity and byte frontier before parsing any ledger.
struct CapturedLedger {
    file: std::fs::File,
    path: PathBuf,
    len: u64,
}

impl CapturedLedger {
    fn open(
        directory: &super::ContainedDirectory,
        name: &str,
        budget: &mut Budget,
    ) -> io::Result<Self> {
        let file = directory.open_regular(OsStr::new(name), "transcript source")?;
        let len = file.metadata()?.len();
        budget.add_bytes(len)?;
        Ok(Self {
            file,
            path: directory.display_path().join(name),
            len,
        })
    }

    fn lines(&self) -> io::Result<CommittedJsonlLines> {
        CommittedJsonlLines::from_open_file_bounded(
            self.file.try_clone()?,
            self.path.clone(),
            "transcript source",
            self.len,
        )
    }

    fn versioned<T: serde::de::DeserializeOwned>(
        &self,
        version: u64,
        budget: &mut Budget,
    ) -> io::Result<Vec<T>> {
        self.lines()?
            .enumerate()
            .map(|(index, line)| {
                budget.add_events(1)?;
                let line = line?;
                let value: serde_json::Value = serde_json::from_slice(&line).map_err(|error| {
                    invalid(format!("{}:{}: {error}", self.path.display(), index + 1))
                })?;
                if value.get("version").and_then(serde_json::Value::as_u64) != Some(version) {
                    return Err(invalid(format!(
                        "{}:{}: invalid ledger schema version",
                        self.path.display(),
                        index + 1
                    )));
                }
                serde_json::from_value(value).map_err(|error| {
                    invalid(format!("{}:{}: {error}", self.path.display(), index + 1))
                })
            })
            .collect()
    }
}

fn load_facts(
    session_id: &str,
    grow_home: &Path,
    facts: &mut BTreeMap<String, Facts>,
    budget: &mut Budget,
    display: bool,
) -> io::Result<()> {
    if facts.contains_key(session_id) {
        return Ok(());
    }
    if facts.len() >= MAX_NODES {
        return Err(limit_error());
    }
    let storage = JsonlStorageAdapter::with_root(grow_home.to_path_buf());
    let opened = storage
        .open_session_by_id_shared_read(session_id)?
        .ok_or_else(|| invalid(format!("required session '{session_id}' is missing")))?;
    if opened.summary().info.id.to_string() != session_id {
        return Err(invalid(format!("session identity mismatch: {session_id}")));
    }
    // Updates are published after their authority. Capture their frontier
    // first; a later Timeline frontier can reconcile a missing display row.
    let updates = if display {
        match CapturedLedger::open(opened.directory(), UPDATES_FILE, budget) {
            Ok(ledger) => Some(ledger),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let ledger = CapturedLedger::open(opened.directory(), TIMELINE_FILE, budget)?;
    let timeline = Timeline::from_events(
        ledger.versioned(u64::from(chat_state::TIMELINE_SCHEMA_VERSION), budget)?,
    )
    .map_err(|error| invalid(format!("session '{session_id}': {error}")))?;
    let control =
        crate::session::control::SessionControlSnapshot::latest_from_timeline(timeline.events())
            .map_err(|error| invalid(format!("session '{session_id}' control: {error}")))?;
    let sideband_activity = verify_references(&opened, session_id, &timeline, budget)?;
    let selected_spawns = selected_spawns(timeline.events());
    let seed = timeline
        .events()
        .iter()
        .find_map(|event| match &event.kind {
            TimelineEventKind::SubagentSeed(seed) => Some(seed.clone()),
            _ => None,
        });
    let last_seq = timeline.events().last().map(|event| event.seq.get());
    let summary = opened.summary();
    let title = match timeline.session_title() {
        Some((seq, title)) => {
            if summary
                .title_event_seq
                .is_some_and(|projected| projected >= seq.get())
                && (summary.title_event_seq != Some(seq.get())
                    || summary.title.as_deref() != Some(title.title.as_str())
                    || summary.title_source.as_ref() != Some(&title.source))
            {
                return Err(invalid(format!(
                    "session '{session_id}' title conflicts with Timeline"
                )));
            }
            Some(title.title.clone())
        }
        None => {
            if summary.title.is_some()
                || summary.title_source.is_some()
                || summary.title_event_seq.is_some()
            {
                return Err(invalid(format!(
                    "session '{session_id}' title has no Timeline authority"
                )));
            }
            None
        }
    };
    facts.insert(
        session_id.to_owned(),
        Facts {
            title,
            summary_parent: opened.summary().parent_session_id.clone(),
            session_kind: opened.summary().session_kind.clone(),
            seed,
            last_seq,
            sideband_activity,
            selected_spawns,
            timeline,
            control,
            opened,
            updates,
        },
    );
    Ok(())
}

fn verify_references(
    opened: &OpenedSession,
    session_id: &str,
    timeline: &Timeline,
    budget: &mut Budget,
) -> io::Result<Vec<std::ops::Range<u64>>> {
    // Immutable references remain hash-verified; reserve their actual file
    // sizes before verification so small ledgers cannot evade the tree budget.
    let mut prompt_hashes = BTreeSet::new();
    for event in timeline.events() {
        if let TimelineEventKind::Messages(messages) = &event.kind {
            prompt_hashes.extend(crate::session::persistence::referenced_prompt_blob_hashes(
                &messages.items,
            )?);
        }
    }
    if !prompt_hashes.is_empty() {
        let directory =
            opened
                .directory()
                .open_relative(Path::new("prompts"), "prompt blobs", false)?;
        for hash in prompt_hashes {
            let file = directory.open_regular(OsStr::new(&format!("{hash}.txt")), "prompt blob")?;
            budget.add_bytes(file.metadata()?.len())?;
        }
    }
    let hashes = crate::session::sampling_evidence::referenced_hashes(timeline)?;
    if !hashes.is_empty() {
        let directory = opened.directory().open_relative(
            Path::new("artifacts/sampling"),
            "sampling evidence",
            false,
        )?;
        for hash in hashes {
            let file =
                directory.open_regular(OsStr::new(&format!("{hash}.bin")), "sampling evidence")?;
            budget.add_bytes(file.metadata()?.len())?;
        }
    }
    crate::session::persistence::verify_timeline_prompt_blobs_from_directory(
        opened.directory(),
        timeline,
    )?;
    let mut sidebands = super::SidebandLedgers::new();
    for id in timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Sideband(spawn) => Some(&spawn.sideband_id),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
    {
        chat_state::validate_sideband_id(id).map_err(|error| invalid(error.to_string()))?;
        let directory = match opened.directory().open_relative(
            &Path::new(super::SIDEBANDS_DIR).join(id),
            "sideband",
            false,
        ) {
            Ok(directory) => directory,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let ledger = match CapturedLedger::open(&directory, TIMELINE_FILE, budget) {
            Ok(ledger) => ledger,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let events = ledger.versioned(u64::from(chat_state::SIDEBAND_SCHEMA_VERSION), budget)?;
        if !events.is_empty() {
            sidebands.insert(id.clone(), events);
        }
    }
    super::validate_sideband_ledgers(session_id, timeline, &sidebands)?;
    Ok(activity::merged(
        sidebands
            .values()
            .flat_map(|events| activity::sideband_intervals(events))
            .collect(),
    ))
}

fn verify_seed(
    child_id: &str,
    grow_home: &Path,
    facts: &mut BTreeMap<String, Facts>,
    budget: &mut Budget,
) -> io::Result<()> {
    let child_fact = facts.get(child_id).expect("child facts loaded");
    let seed = child_fact.seed.clone();
    let Some(seed) = seed else {
        if child_fact
            .session_kind
            .as_deref()
            .is_some_and(|kind| kind.starts_with("subagent"))
        {
            return Err(invalid(format!(
                "subagent session '{child_id}' is missing its owner seed"
            )));
        }
        return Ok(());
    };
    load_facts(&seed.parent_timeline_id, grow_home, facts, budget, false)?;
    let owner = facts.get(&seed.parent_timeline_id).expect("loaded owner");
    let (spawn_seq, spawn) = owner
        .timeline
        .events()
        .iter()
        .find_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))
                if event.seq.get() == seed.parent_spawn_seq =>
            {
                Some((event.seq, spawn))
            }
            _ => None,
        })
        .ok_or_else(|| invalid(format!("missing owner spawn for child '{child_id}'")))?;
    if spawn.child_session_id != child_id {
        return Err(invalid(format!(
            "spawn identity mismatch for child '{child_id}'"
        )));
    }
    let child = facts.get(child_id).expect("loaded child");
    if child.summary_parent.as_deref() != Some(seed.parent_timeline_id.as_str())
        || !child
            .session_kind
            .as_deref()
            .is_some_and(|kind| kind.starts_with("subagent"))
    {
        return Err(invalid(format!(
            "child '{child_id}' summary disagrees with its lifecycle owner"
        )));
    }
    child
        .timeline
        .validate_subagent_seed_link(&seed.parent_timeline_id, spawn_seq, spawn)
        .map_err(|error| invalid(error.to_string()))?;
    if let Some(terminal) = owner
        .timeline
        .events()
        .iter()
        .find_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Ended(end))
                if end.subagent_id == spawn.subagent_id =>
            {
                Some(end)
            }
            _ => None,
        })
        && terminal.result_ref.is_some()
    {
        child
            .timeline
            .validate_subagent_result_link(&seed.parent_timeline_id, spawn_seq, spawn, terminal)
            .map_err(|error| invalid(error.to_string()))?;
    }
    Ok(())
}

/// A rewind selects a branch, but does not delete old lifecycle facts. Keep
/// only children attached to turns that survived the branch selection. The
/// actual display row is still filtered independently by updates.jsonl.
fn selected_spawns(events: &[chat_state::TimelineEvent]) -> BTreeSet<String> {
    let mut selected = BTreeMap::<String, Option<usize>>::new();
    let mut current_prompt = None;
    for event in events {
        match &event.kind {
            TimelineEventKind::Turn(TurnEvent::Started { prompt_index, .. }) => {
                current_prompt = Some(*prompt_index);
            }
            TimelineEventKind::Messages(message) if message.cause != MessageCause::Rewind => {
                if let Some(index) = message
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        sampling_types::ConversationItem::User(user) => user.prompt_index,
                        _ => None,
                    })
                    .last()
                {
                    current_prompt = Some(index);
                }
            }
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)) => {
                selected.insert(spawn.child_session_id.clone(), current_prompt);
            }
            TimelineEventKind::Messages(message) if message.cause == MessageCause::Rewind => {
                let next = message
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        sampling_types::ConversationItem::User(user) => user.prompt_index,
                        _ => None,
                    })
                    .max()
                    .map_or(0, |index| index.saturating_add(1));
                selected.retain(|_, owner| owner.is_some_and(|index| index < next));
                current_prompt = next.checked_sub(1);
            }
            _ => {}
        }
    }
    selected.into_keys().collect()
}

/// Discover only descendants reachable through validated direct delegation.
/// Root may itself be a child; its lifecycle owner is read only as authority.
pub fn discover_tree(session_id: &str) -> io::Result<Vec<TranscriptNode>> {
    discover_tree_at(session_id, &crate::util::grow_home::grow_home())
}

pub fn discover_tree_at(session_id: &str, grow_home: &Path) -> io::Result<Vec<TranscriptNode>> {
    Ok(capture_tree_at(session_id, grow_home)?.nodes)
}

/// One capture owns both the directory topology and every node's display
/// frontier. A caller cannot accidentally rediscover a newer session body.
pub struct TranscriptSnapshot {
    pub nodes: Vec<TranscriptNode>,
    facts: BTreeMap<String, Facts>,
    budget: Budget,
}

impl TranscriptSnapshot {
    pub fn read_session(&mut self, session_id: &str) -> io::Result<TranscriptSession> {
        if !self.nodes.iter().any(|node| node.session_id == session_id) {
            return Err(invalid("session is outside transcript capture"));
        }
        project_session(session_id, &self.facts, &mut self.budget)
    }
}

pub fn capture_tree_at(session_id: &str, grow_home: &Path) -> io::Result<TranscriptSnapshot> {
    let mut facts = BTreeMap::new();
    let mut budget = Budget::default();
    load_facts(session_id, grow_home, &mut facts, &mut budget, true)?;
    verify_seed(session_id, grow_home, &mut facts, &mut budget)?;
    let mut nodes = Vec::new();
    let mut visited = BTreeSet::new();
    visit(
        session_id,
        None,
        0,
        grow_home,
        &mut facts,
        &mut budget,
        &mut visited,
        &mut nodes,
    )?;
    Ok(TranscriptSnapshot {
        nodes,
        facts,
        budget,
    })
}

#[allow(clippy::too_many_arguments)]
fn visit(
    session_id: &str,
    parent_id: Option<&str>,
    depth: usize,
    grow_home: &Path,
    facts: &mut BTreeMap<String, Facts>,
    budget: &mut Budget,
    visited: &mut BTreeSet<String>,
    nodes: &mut Vec<TranscriptNode>,
) -> io::Result<()> {
    if depth > MAX_DEPTH || nodes.len() >= MAX_NODES {
        return Err(limit_error());
    }
    if !visited.insert(session_id.to_owned()) {
        return Err(invalid(format!("duplicate or cyclic child '{session_id}'")));
    }
    let current = facts.get(session_id).expect("visited fact was loaded");
    let mut owners = vec![session_id.to_owned()];
    if let Some(seed) = &current.seed
        && seed.parent_timeline_id != session_id
    {
        owners.push(seed.parent_timeline_id.clone());
    }
    let title = current.title.clone();
    let last_timeline_seq = current.last_seq;
    let seed = current.seed.clone();
    for owner in &owners {
        load_facts(owner, grow_home, facts, budget, false)?;
    }
    let mut children = BTreeMap::new();
    for owner_id in &owners {
        let owner = facts.get(owner_id).expect("owner loaded");
        for event in owner.timeline.events() {
            let TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)) = &event.kind else {
                continue;
            };
            if spawn.security_parent_session_id == session_id
                && owner.selected_spawns.contains(&spawn.child_session_id)
            {
                if children
                    .insert(spawn.child_session_id.clone(), owner_id.clone())
                    .is_some()
                {
                    return Err(invalid(format!(
                        "child '{}' is spawned more than once",
                        spawn.child_session_id
                    )));
                }
            }
        }
    }
    let child_ids = children.keys().cloned().collect::<Vec<_>>();
    let spawn_at_ms = seed.as_ref().and_then(|seed| {
        facts.get(&seed.parent_timeline_id).and_then(|owner| {
            owner.timeline.events().iter().find_map(|event| {
                (event.seq.get() == seed.parent_spawn_seq)
                    .then(|| u64::try_from(event.at_ms).ok())
                    .flatten()
            })
        })
    });
    nodes.push(TranscriptNode {
        session_id: session_id.to_owned(),
        title,
        parent_id: parent_id.map(str::to_owned),
        depth,
        children: child_ids.clone(),
        last_timeline_seq,
        spawn_at_ms,
    });
    for child_id in child_ids {
        let owner_id = &children[&child_id];
        load_facts(&child_id, grow_home, facts, budget, true)?;
        let child_seed = facts.get(&child_id).and_then(|item| item.seed.as_ref());
        if child_seed.is_none_or(|seed| seed.parent_timeline_id != *owner_id) {
            return Err(invalid(format!(
                "child '{child_id}' has no matching owner seed"
            )));
        }
        verify_seed(&child_id, grow_home, facts, budget)?;
        visit(
            &child_id,
            Some(session_id),
            depth + 1,
            grow_home,
            facts,
            budget,
            visited,
            nodes,
        )?;
    }
    Ok(())
}

/// Read a single verified session's display sequence. No writer repair occurs.
pub fn read_session(session_id: &str) -> io::Result<TranscriptSession> {
    read_session_at(session_id, &crate::util::grow_home::grow_home())
}

pub fn read_session_at(session_id: &str, grow_home: &Path) -> io::Result<TranscriptSession> {
    let mut facts = BTreeMap::new();
    let mut budget = Budget::default();
    load_facts(session_id, grow_home, &mut facts, &mut budget, true)?;
    verify_seed(session_id, grow_home, &mut facts, &mut budget)?;
    project_session(session_id, &facts, &mut budget)
}

fn project_session(
    session_id: &str,
    all: &BTreeMap<String, Facts>,
    budget: &mut Budget,
) -> io::Result<TranscriptSession> {
    let facts = all.get(session_id).expect("session was captured");
    let opened = &facts.opened;
    let session_id = opened.summary().info.id.to_string();
    let lines = match &facts.updates {
        Some(ledger) => {
            let mut lines = Vec::new();
            for (index, line) in ledger.lines()?.enumerate() {
                budget.add_events(1)?;
                let line = String::from_utf8(line?).map_err(|error| invalid(error.to_string()))?;
                if line.trim().is_empty() {
                    continue;
                }
                let envelope: SessionUpdateEnvelope =
                    serde_json::from_str(&line).map_err(|error| {
                        invalid(format!(
                            "session '{session_id}' update {}: {error}",
                            index + 1
                        ))
                    })?;
                // Projection caches use snake_case; public notifications use
                // camelCase. Validate identity even on rows removed by rewind
                // or admission reconciliation.
                let identity = envelope
                    .params
                    .get("sessionId")
                    .or_else(|| envelope.params.get("session_id"))
                    .and_then(serde_json::Value::as_str);
                if identity != Some(session_id.as_str()) {
                    return Err(invalid(format!(
                        "session '{session_id}' update {} has a different or missing session identity",
                        index + 1
                    )));
                }
                lines.push(line);
            }
            lines
        }
        None => {
            let only_identity_facts = facts.timeline.events().iter().all(|event| {
                matches!(
                    event.kind,
                    TimelineEventKind::SessionTitle(_) | TimelineEventKind::SubagentSeed(_)
                )
            });
            if !only_identity_facts {
                return Err(invalid(format!(
                    "session '{session_id}' has Timeline facts but no committed updates ledger"
                )));
            }
            Vec::new()
        }
    };
    if lines.is_empty()
        && facts.timeline.events().iter().any(|event| {
            !matches!(
                event.kind,
                TimelineEventKind::SessionTitle(_) | TimelineEventKind::SubagentSeed(_)
            )
        })
    {
        return Err(invalid(format!(
            "session '{session_id}' has Timeline facts but no committed display updates; retry"
        )));
    }
    let filtered = filter_rewind_lines(lines.iter().map(String::as_str).collect());
    // Reconciliation replaces a payload-free preview anchor with canonical
    // text. Preserve that anchor's recorded timing and prompt ownership, not
    // the newly serialized envelope's current timestamp.
    let mut anchors = BTreeMap::new();
    for line in &filtered {
        let envelope: SessionUpdateEnvelope =
            serde_json::from_str(line).map_err(|error| invalid(error.to_string()))?;
        if envelope.method != super::ACP_SESSION_UPDATE_METHOD {
            continue;
        }
        let Some(meta) = envelope
            .params
            .get("_meta")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        if let (Some(request), Some(attempt)) = (
            meta.get("samplingRequestId")
                .and_then(serde_json::Value::as_str),
            meta.get("samplingAttempt")
                .and_then(serde_json::Value::as_u64),
        ) {
            let timing = [
                "promptId",
                "turnStartMs",
                "streamStartMs",
                "agentTimestampMs",
            ]
            .into_iter()
            .filter_map(|key| meta.get(key).map(|value| (key.to_owned(), value.clone())))
            .collect::<serde_json::Map<_, _>>();
            anchors.entry((request.to_owned(), attempt)).or_insert((
                timing,
                (envelope.timestamp != 0).then(|| envelope.timestamp.saturating_mul(1000)),
            ));
        }
    }
    let reconciled =
        reconcile_raw_replay_lines(&opened.summary().info.id, &facts.timeline, filtered)?;
    let timeline_times = facts
        .timeline
        .events()
        .iter()
        .filter_map(|event| {
            u64::try_from(event.at_ms)
                .ok()
                .map(|at| (event.seq.get(), at))
        })
        .collect::<BTreeMap<_, _>>();
    let mut events = Vec::new();
    for line in reconciled.lines {
        let synthetic = matches!(line, ReconciledReplayLine::Owned(_));
        let envelope: SessionUpdateEnvelope = serde_json::from_str(line.as_str())
            .map_err(|error| invalid(format!("invalid committed transcript update: {error}")))?;
        let timestamp_ms = (!synthetic && envelope.timestamp != 0)
            .then(|| envelope.timestamp.saturating_mul(1000));
        match envelope
            .into_update()
            .map_err(|error| invalid(format!("invalid transcript update: {error}")))?
        {
            SessionUpdate::ResponseReplayProjection(projection) => {
                let projection_time = timeline_times.get(&projection.timeline_event).copied();
                let anchor =
                    anchors.get(&(projection.request_id.clone(), u64::from(projection.attempt)));
                let notifications = projection.into_notifications();
                budget.add_events(notifications.len())?;
                for mut notification in notifications {
                    if let Some((timing, _)) = anchor {
                        notification
                            .meta
                            .get_or_insert_with(Default::default)
                            .extend(timing.clone());
                    }
                    notification.update = strip_context_wrappers(notification.update);
                    events.push(TranscriptEvent {
                        update: SessionUpdate::Acp(Box::new(notification)),
                        timestamp_ms: anchor
                            .and_then(|(_, timestamp)| *timestamp)
                            .or(projection_time)
                            .or(timestamp_ms.filter(|_| !synthetic)),
                        simulated_source: synthetic,
                    });
                }
            }
            SessionUpdate::Acp(mut notification) => {
                notification.update = strip_context_wrappers(notification.update);
                events.push(TranscriptEvent {
                    update: SessionUpdate::Acp(notification),
                    timestamp_ms,
                    simulated_source: false,
                });
            }
            update @ SessionUpdate::Grow(_) => events.push(TranscriptEvent {
                update,
                timestamp_ms,
                simulated_source: false,
            }),
        }
    }
    restore_observational_facts(&session_id, facts, all, &mut events, budget)?;
    restore_control_snapshot(facts, &mut events, budget)?;
    let mut activity = activity::from_timeline(facts.timeline.events());
    let background = activity::background_task_activity(&events);
    activity.protected.extend(background.protected);
    activity.open_starts.extend(background.open_starts);
    activity.unknown_activity |= background.unknown_activity;
    activity
        .protected
        .extend(facts.sideband_activity.iter().cloned());
    activity.protected = activity::merged(activity.protected);
    Ok(TranscriptSession {
        session_id: opened.summary().info.id.to_string(),
        title: facts.title.clone(),
        last_timeline_seq: facts.last_seq,
        source_bytes: budget.bytes,
        events,
        activity: activity.protected,
        recovery_gaps: activity.recovery_gaps,
        open_activity: activity.open_starts,
        unknown_activity: activity.unknown_activity,
    })
}

/// Restore only the captured endpoint when the display cache disagrees with
/// its authority. It is not evidence for an earlier position in the stream.
fn restore_control_snapshot(
    facts: &Facts,
    events: &mut Vec<TranscriptEvent>,
    budget: &mut Budget,
) -> io::Result<()> {
    use crate::extensions::notification::{
        SessionNotification, SessionUpdate as GrowUpdate, UiNotice, UiNoticeCategory, UiNoticeTone,
    };
    use crate::session::behavior::{BehaviorState, PlanPhase};
    use acp_transport::protocol as acp;
    let Some(control) = &facts.control else {
        return Ok(());
    };
    let mode = control.behavior.behavior().as_id();
    let phase = match &control.behavior.state {
        BehaviorState::Plan(phase) => Some(match phase {
            PlanPhase::Drafting => "drafting",
            PlanPhase::AwaitingApproval => "awaiting_approval",
            PlanPhase::Executing => "executing",
            PlanPhase::Amending => "amending",
        }),
        _ => None,
    };
    let expected_goal = control.goal.as_ref().map_or_else(
        crate::session::goal_notification::build_goal_cleared,
        |goal| {
            crate::session::goal_notification::build_goal_updated(
                goal,
                goal.tokens_used,
                goal.elapsed_ms,
            )
        },
    );
    let cached_mode = events.iter().rev().find_map(|event| match &event.update {
        SessionUpdate::Acp(notification) => match &notification.update {
            acp::SessionUpdate::CurrentModeUpdate(update) => Some(update),
            _ => None,
        },
        _ => None,
    });
    let mode_matches = cached_mode.is_some_and(|update| {
        update.current_mode_id.0.as_ref() == mode
            && update
                .meta
                .as_ref()
                .and_then(|meta| meta.get("grow/planPhase"))
                .and_then(serde_json::Value::as_str)
                == phase
    });
    let cached_goal = events.iter().rev().find_map(|event| match &event.update {
        SessionUpdate::Grow(notification)
            if matches!(notification.update, GrowUpdate::GoalUpdated { .. }) =>
        {
            Some(&notification.update)
        }
        _ => None,
    });
    let goal_matches = cached_goal.map_or(control.goal.is_none(), |goal| *goal == expected_goal);
    if mode_matches && goal_matches {
        return Ok(());
    }

    budget.add_events(3)?;
    let session_id = facts.opened.summary().info.id.clone();
    let mode_update = acp::CurrentModeUpdate::new(acp::SessionModeId::new(mode)).meta(
        serde_json::json!({"grow/planPhase": phase})
            .as_object()
            .cloned(),
    );
    events.push(TranscriptEvent {
        update: SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            session_id.clone(),
            acp::SessionUpdate::CurrentModeUpdate(mode_update),
        ))),
        timestamp_ms: None,
        simulated_source: true,
    });
    let summary = format!(
        "State at captured snapshot · Behavior: {mode} · Goal: {}",
        match &expected_goal {
            GrowUpdate::GoalUpdated { status, .. } if status != "cleared" => status.as_str(),
            _ => "none",
        }
    );
    for update in [
        expected_goal,
        GrowUpdate::UiNotice(UiNotice {
            correlation_id: format!("transcript-control:{}", control.control_revision),
            category: UiNoticeCategory::Lifecycle,
            subject: Some("Captured state".into()),
            description: None,
            message: summary,
            tone: UiNoticeTone::Info,
            details: None,
        }),
    ] {
        events.push(TranscriptEvent {
            update: SessionUpdate::Grow(Box::new(SessionNotification {
                session_id: session_id.clone(),
                update,
                meta: Some(serde_json::json!({"transcriptSnapshot": true})),
            })),
            timestamp_ms: None,
            simulated_source: true,
        });
    }
    Ok(())
}

/// Reconnect also reconstructs these passive projections. No actor, runtime
/// recovery or hook dispatch is instantiated. Missing cross-ledger ordering
/// is left explicitly estimated rather than sorted by wall clock.
fn restore_observational_facts(
    session_id: &str,
    facts: &Facts,
    all: &BTreeMap<String, Facts>,
    events: &mut Vec<TranscriptEvent>,
    budget: &mut Budget,
) -> io::Result<()> {
    use crate::extensions::notification::{SessionNotification, SessionUpdate as GrowUpdate};
    let mut spawned = BTreeSet::new();
    let mut finished = BTreeSet::new();
    let mut hooks = BTreeSet::new();
    let mut receipts = BTreeSet::new();
    for event in events.iter() {
        if let SessionUpdate::Grow(notification) = &event.update {
            match &notification.update {
                GrowUpdate::SubagentSpawned {
                    child_session_id, ..
                } => {
                    spawned.insert(child_session_id.clone());
                }
                GrowUpdate::SubagentFinished {
                    child_session_id, ..
                } => {
                    finished.insert(child_session_id.clone());
                }
                GrowUpdate::HookExecution { occurrence_id, .. } => {
                    hooks.insert(occurrence_id.clone());
                }
                GrowUpdate::UiNotice(notice)
                    if notice.subject.as_deref()
                        == Some(crate::extensions::notification::AgentMessageNotice::SUBJECT) =>
                {
                    receipts.insert(notice.correlation_id.clone());
                }
                _ => {}
            }
        }
    }
    let restored = |update| TranscriptEvent {
        update: SessionUpdate::Grow(Box::new(SessionNotification {
            session_id: facts.opened.summary().info.id.clone(),
            update,
            meta: None,
        })),
        timestamp_ms: None,
        simulated_source: true,
    };
    let mut owners = vec![facts];
    if let Some(seed) = &facts.seed
        && seed.parent_timeline_id != session_id
        && let Some(owner) = all.get(&seed.parent_timeline_id)
    {
        owners.push(owner);
    }
    for owner in owners {
        for event in owner.timeline.events() {
            let TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn)) = &event.kind else {
                continue;
            };
            if spawn.security_parent_session_id != session_id
                || !owner.selected_spawns.contains(&spawn.child_session_id)
            {
                continue;
            }
            if spawned.insert(spawn.child_session_id.clone()) {
                budget.add_events(1)?;
                let update = restored(crate::agent::subagent::spawn_from_fact(
                    session_id, spawn, None,
                ));
                // A surviving terminal is an exact dependency anchor for its
                // missing spawn; unrelated messages keep their recorded order.
                let position = events.iter().position(|event| matches!(&event.update,
                    SessionUpdate::Grow(n) if matches!(&n.update, GrowUpdate::SubagentFinished { child_session_id, .. } if *child_session_id == spawn.child_session_id)))
                    .unwrap_or(events.len());
                events.insert(position, update);
            }
            if !finished.contains(&spawn.child_session_id)
                && let Some(end) =
                    owner
                        .timeline
                        .events()
                        .iter()
                        .find_map(|event| match &event.kind {
                            TimelineEventKind::Subagent(SubagentEvent::Ended(end))
                                if end.subagent_id == spawn.subagent_id =>
                            {
                                Some(end)
                            }
                            _ => None,
                        })
            {
                budget.add_events(1)?;
                events.push(restored(crate::agent::subagent::finish_from_terminal(
                    end, None,
                )));
                finished.insert(spawn.child_session_id.clone());
            }
        }
    }
    for receipt in facts.timeline.parent_message_receipts() {
        let TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
            id,
            owner_session_id,
            payload_ref,
            ..
        }) = &receipt.kind
        else {
            continue;
        };
        if owner_session_id != session_id {
            return Err(invalid(format!(
                "session '{session_id}' has a foreign message receipt"
            )));
        }
        if !receipts.insert(id.clone()) {
            continue;
        }
        // The existing reader bounds and verifies this immutable artifact.
        // Reserve its actual bytes before materializing; a missing or invalid
        // body retains the same explicit 'Message unavailable' receipt as live
        // reconnect, never another delivery or a guessed body.
        let payload_bytes = facts
            .opened
            .directory()
            .open_relative(
                Path::new("artifacts/notifications"),
                "notification payload directory",
                false,
            )
            .and_then(|directory| {
                directory.open_regular(
                    OsStr::new(&format!("{}.txt", payload_ref.blake3)),
                    "notification payload",
                )
            })
            .and_then(|file| file.metadata())
            .map(|metadata| metadata.len())
            .unwrap_or(payload_ref.bytes);
        budget.add_bytes(payload_bytes)?;
        if let Some(notice) = crate::session::notification_inbox::read_parent_message_notice(
            facts.opened.directory(),
            &receipt,
        ) {
            budget.add_events(1)?;
            events.push(restored(GrowUpdate::UiNotice(notice)));
        }
    }
    for projection in facts.timeline.completed_hook_projections() {
        if !hooks.insert(projection.occurrence_id.clone()) {
            continue;
        }
        let name =
            serde_json::to_value(projection.event).map_err(|error| invalid(error.to_string()))?;
        if let Some(update) = crate::session::actor::SessionActor::project_hook_execution(
            name.as_str().expect("HookEventType serializes as string"),
            None,
            None,
            &projection,
            true,
        ) {
            budget.add_events(1)?;
            events.push(restored(update));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::persistence::Summary;
    use crate::util::grow_home::encode_cwd_dirname;
    use agent_client_protocol::schema::v1::SessionId;
    use sampling_types::ModelImageInputKey;

    mod adversarial_tests {
        include!("transcript/adversarial_tests.rs");
    }

    fn write_session(root: &Path, id: &str, cwd: &str, timeline: &Timeline, child: bool) {
        let dir = root.join("sessions").join(encode_cwd_dirname(cwd)).join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let info = crate::session::info::Info {
            id: SessionId::new(id.to_owned()),
            cwd: cwd.to_owned(),
        };
        let mut summary = Summary::new(&info, crate::agent::models::ModelId::new("model")).unwrap();
        if child {
            summary.parent_session_id = Some("root".into());
            summary.session_kind = Some("subagent".into());
        }
        std::fs::write(
            dir.join(super::super::SUMMARY_FILE),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        let lines = timeline
            .events()
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>();
        std::fs::write(
            dir.join(TIMELINE_FILE),
            if lines.is_empty() {
                String::new()
            } else {
                lines.join("\n") + "\n"
            },
        )
        .unwrap();
    }

    fn spawn(child_id: &str, security_parent: &str) -> chat_state::SubagentSpawnEvent {
        chat_state::SubagentSpawnEvent {
            subagent_id: format!("agent-{child_id}"),
            child_session_id: child_id.into(),
            security_parent_session_id: security_parent.into(),
            subagent_type: "explore".into(),
            description: "inspect".into(),
            prompt: "inspect".into(),
            context_source: chat_state::SubagentContextSource::New,
            source_ref: None,
            context_normalized: false,
            resumed_from: None,
            parent_prompt_id: None,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            workflow_run_id: None,
            goal_id: None,
            goal_definition_revision: None,
            surface_completion: true,
            child_cwd: "/project".into(),
            worktree_path: None,
            effective_model_id: "model".into(),
            model_transport_key: ModelImageInputKey::new("model", "responses", "test"),
            reasoning_effort: None,
        }
    }

    fn child_seed(
        owner: &str,
        seq: u64,
        spawn: &chat_state::SubagentSpawnEvent,
    ) -> chat_state::SubagentSeedEvent {
        chat_state::SubagentSeedEvent {
            parent_timeline_id: owner.into(),
            parent_spawn_seq: seq,
            subagent_id: spawn.subagent_id.clone(),
            security_parent_session_id: spawn.security_parent_session_id.clone(),
            context_source: spawn.context_source,
            source_ref: spawn.source_ref.clone(),
            normalized: spawn.context_normalized,
        }
    }

    #[test]
    fn rewind_excludes_spawned_children_from_the_discovered_branch() {
        use chat_state::{EventSeq, MessageEvent, SurfaceOp, TimelineEvent};
        let mut first_prompt = sampling_types::ConversationItem::user("first");
        first_prompt.set_prompt_index(0);
        let mut second_prompt = sampling_types::ConversationItem::user("second");
        second_prompt.set_prompt_index(1);
        let message = |cause, items| {
            TimelineEventKind::Messages(MessageEvent {
                cause,
                items,
                surface: SurfaceOp::Append,
                response_admission: None,
            })
        };
        let kinds = vec![
            message(MessageCause::User, vec![first_prompt.clone()]),
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn("kept", "root"))),
            message(MessageCause::User, vec![second_prompt]),
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn("rewound", "root"))),
        ];
        let mut events = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| TimelineEvent {
                version: chat_state::TIMELINE_SCHEMA_VERSION,
                seq: EventSeq::new(index as u64 + 1),
                at_ms: index as i64,
                kind,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            selected_spawns(&events),
            BTreeSet::from(["kept".to_owned(), "rewound".to_owned()])
        );
        events.push(TimelineEvent {
            version: chat_state::TIMELINE_SCHEMA_VERSION,
            seq: EventSeq::new(5),
            at_ms: 5,
            kind: message(MessageCause::Rewind, vec![first_prompt]),
        });
        assert_eq!(
            selected_spawns(&events),
            BTreeSet::from(["kept".to_owned()])
        );
    }

    #[test]
    fn discovers_direct_and_nested_children_in_tree_order() {
        let home = tempfile::tempdir().unwrap();
        let mut root = Timeline::default();
        let first = spawn("child", "root");
        root.record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
            first.clone(),
        )))
        .unwrap();
        let first_seq = root.events().last().unwrap().seq.get();
        let grandchild = spawn("grandchild", "child");
        root.record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
            grandchild.clone(),
        )))
        .unwrap();
        let grandchild_seq = root.events().last().unwrap().seq.get();
        let mut child = Timeline::default();
        child
            .record(TimelineEventKind::SubagentSeed(child_seed(
                "root", first_seq, &first,
            )))
            .unwrap();
        let mut grandchild_timeline = Timeline::default();
        grandchild_timeline
            .record(TimelineEventKind::SubagentSeed(child_seed(
                "root",
                grandchild_seq,
                &grandchild,
            )))
            .unwrap();
        write_session(home.path(), "root", "/project", &root, false);
        write_session(home.path(), "child", "/project", &child, true);
        write_session(
            home.path(),
            "grandchild",
            "/project",
            &grandchild_timeline,
            true,
        );

        let tree = discover_tree_at("root", home.path()).unwrap();
        assert_eq!(
            tree.iter()
                .map(|node| node.session_id.as_str())
                .collect::<Vec<_>>(),
            ["root", "child", "grandchild"]
        );
        assert_eq!(tree[0].children, ["child"]);
        assert_eq!(tree[1].parent_id.as_deref(), Some("root"));
        assert_eq!(tree[1].children, ["grandchild"]);
        assert_eq!(tree[2].parent_id.as_deref(), Some("child"));
        let child_tree = discover_tree_at("child", home.path()).unwrap();
        assert_eq!(
            child_tree
                .iter()
                .map(|node| node.session_id.as_str())
                .collect::<Vec<_>>(),
            ["child", "grandchild"]
        );
        assert_eq!(child_tree[0].parent_id, None);
    }

    #[test]
    fn missing_or_corrupt_child_fails_closed() {
        let home = tempfile::tempdir().unwrap();
        let mut root = Timeline::default();
        let child = spawn("child", "root");
        root.record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
            child.clone(),
        )))
        .unwrap();
        let child_seq = root.events().last().unwrap().seq.get();
        write_session(home.path(), "root", "/project", &root, false);
        assert!(discover_tree_at("root", home.path()).is_err());

        write_session(home.path(), "child", "/project", &Timeline::default(), true);
        assert!(
            read_session_at("child", home.path())
                .unwrap_err()
                .to_string()
                .contains("missing its owner seed")
        );

        let mut child_timeline = Timeline::default();
        child_timeline
            .record(TimelineEventKind::SubagentSeed(child_seed(
                "root", child_seq, &child,
            )))
            .unwrap();
        write_session(home.path(), "child", "/project", &child_timeline, true);
        std::fs::write(
            home.path()
                .join("sessions")
                .join(encode_cwd_dirname("/project"))
                .join("child")
                .join(TIMELINE_FILE),
            b"{bad json}\n",
        )
        .unwrap();
        assert!(discover_tree_at("root", home.path()).is_err());
    }

    #[test]
    fn missing_updates_cannot_turn_recorded_lifecycle_into_empty_transcript() {
        let home = tempfile::tempdir().unwrap();
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn(
                "child", "root",
            ))))
            .unwrap();
        write_session(home.path(), "root", "/project", &timeline, false);
        let error = read_session_at("root", home.path()).unwrap_err();
        assert!(error.to_string().contains("no committed updates ledger"));
    }

    #[test]
    fn oversized_source_is_rejected_before_materialization() {
        let home = tempfile::tempdir().unwrap();
        write_session(home.path(), "root", "/project", &Timeline::default(), false);
        let updates_path = home
            .path()
            .join("sessions")
            .join(encode_cwd_dirname("/project"))
            .join("root")
            .join(UPDATES_FILE);
        let file = std::fs::File::create(updates_path).unwrap();
        file.set_len(MAX_SOURCE_BYTES + 1).unwrap();
        let error = read_session_at("root", home.path()).unwrap_err();
        assert!(error.to_string().contains("read budget"));
    }

    #[test]
    fn read_session_does_not_repair_or_modify_source_files() {
        let home = tempfile::tempdir().unwrap();
        let timeline = Timeline::default();
        write_session(home.path(), "root", "/project", &timeline, false);
        let dir = home
            .path()
            .join("sessions")
            .join(encode_cwd_dirname("/project"))
            .join("root");
        let timeline_path = dir.join(TIMELINE_FILE);
        let before = std::fs::read(&timeline_path).unwrap();
        let result = read_session_at("root", home.path()).unwrap();
        assert!(result.events.is_empty());
        assert_eq!(std::fs::read(timeline_path).unwrap(), before);
        assert!(!dir.join(UPDATES_FILE).exists());
    }
}

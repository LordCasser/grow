//! Short-lived derived-computation timelines.
//!
//! A sideband owns one request transaction and an independent sequence space.
//! Parent sessions retain only a [`SidebandSpawnEvent`]; request attempts and
//! results never enter the model-facing Surface.

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use sampling_types::ConversationItem;
use serde::{Deserialize, Serialize};

pub const SIDEBAND_SCHEMA_VERSION: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SidebandPurpose {
    CompactionSummary,
    RangeSummary,
    PermissionJudgment,
    SessionTitle,
    SessionRecap,
    SideQuestion,
    PromptSuggestion,
    LazinessJudgment,
    MemoryDream,
    MemoryFlush,
    MemoryRewrite,
    ImageDescription,
    InfoRequest,
    ProgressReport,
    ContextRecall,
}

impl SidebandPurpose {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompactionSummary => "compaction-summary",
            Self::RangeSummary => "range-summary",
            Self::PermissionJudgment => "permission-judgment",
            Self::SessionTitle => "session-title",
            Self::SessionRecap => "session-recap",
            Self::SideQuestion => "side-question",
            Self::PromptSuggestion => "prompt-suggestion",
            Self::LazinessJudgment => "laziness-judgment",
            Self::MemoryDream => "memory-dream",
            Self::MemoryFlush => "memory-flush",
            Self::MemoryRewrite => "memory-rewrite",
            Self::ImageDescription => "image-description",
            Self::InfoRequest => "info-request",
            Self::ProgressReport => "progress-report",
            Self::ContextRecall => "context-recall",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineRangeRef {
    pub timeline_id: String,
    pub first_seq: u64,
    pub last_seq: u64,
}

#[derive(Debug, Clone)]
pub struct TimelineMaterialization {
    pub input_ref: TimelineRangeRef,
    pub surface_revision: u64,
    pub surface: Vec<ConversationItem>,
    pub surface_ids: Vec<crate::SurfaceId>,
}

#[derive(Debug, Clone)]
pub struct RecallMaterialization {
    pub source_ref: TimelineRangeRef,
    pub surface_revision: u64,
    /// Current model-visible coordinates that explain why recall is needed.
    pub need_surface_ids: Vec<crate::SurfaceId>,
    /// Uncompressed transcript for the selected rewind branch.
    pub transcript: Vec<ConversationItem>,
    pub transcript_ids: Vec<crate::SurfaceId>,
    /// Original leaves shadowed by completed compactions on this branch.
    pub unloaded_surface_ids: Vec<crate::SurfaceId>,
}

impl TimelineRangeRef {
    pub fn validate(&self) -> Result<(), SidebandError> {
        if self.timeline_id.trim().is_empty() {
            return Err(SidebandError::EmptyTimelineId);
        }
        if self.first_seq > self.last_seq {
            return Err(SidebandError::ReversedInputRef {
                first: self.first_seq,
                last: self.last_seq,
            });
        }
        Ok(())
    }
}

/// The only sideband fact retained on the initiating session Timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandSpawnEvent {
    pub sideband_id: String,
    pub purpose: SidebandPurpose,
    pub source_refs: Vec<TimelineRangeRef>,
}

impl SidebandSpawnEvent {
    pub fn validate(&self) -> Result<(), SidebandError> {
        validate_sideband_id(&self.sideband_id)?;
        for source_ref in &self.source_refs {
            source_ref.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandRoute {
    pub model: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandRequest {
    pub purpose: SidebandPurpose,
    /// Purpose-owned instruction. Referenced Timeline content is deliberately
    /// absent and is materialized only when assembling the provider request.
    pub prompt: String,
    /// Frozen upper bound on every Timeline range that any attempt may read.
    pub source_refs: Vec<TimelineRangeRef>,
    pub route: SidebandRoute,
    pub initiator_ref: String,
    pub executor: String,
    pub output_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandAssemblyManifest {
    /// Purpose-owned deterministic assembly strategy.
    pub strategy: String,
    pub strategy_version: u32,
    pub source_revision: Option<u64>,
    pub context_surface_ids: Vec<crate::SurfaceId>,
    /// Stable item coordinates selected inside `input_refs`, when the
    /// materializer chooses less than a whole event.
    pub selected_surface_ids: Vec<crate::SurfaceId>,
    pub materialized_input_tokens: u64,
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandAttempt {
    pub attempt_no: u32,
    /// Exact source subset materialized for this provider request.
    pub input_refs: Vec<TimelineRangeRef>,
    pub assembly_manifest: SidebandAssemblyManifest,
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandResult {
    pub raw_output: String,
    pub structured_output: Option<serde_json::Value>,
    pub usage: SidebandUsage,
    pub finish: String,
    pub source_event_seqs: [u64; 2],
    /// Evidence must be a subset of the successful attempt's input refs.
    pub evidence_refs: Vec<TimelineRangeRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebandOutcome {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandEnd {
    pub outcome: SidebandOutcome,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "event",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SidebandEventKind {
    Request(SidebandRequest),
    Attempt(SidebandAttempt),
    Result(SidebandResult),
    End(SidebandEnd),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandEvent {
    pub version: u8,
    pub sideband_id: String,
    pub seq: u64,
    pub at_ms: i64,
    pub kind: SidebandEventKind,
}

#[derive(Debug, Clone, Default)]
pub struct SidebandTimeline {
    sideband_id: String,
    events: Vec<SidebandEvent>,
    last_attempt: Option<u64>,
    result_seq: Option<u64>,
    ended: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SidebandError {
    #[error("sideband id is not a canonical UUID")]
    InvalidId,
    #[error("Timeline reference has an empty timeline id")]
    EmptyTimelineId,
    #[error("Timeline reference range is reversed ({first} > {last})")]
    ReversedInputRef { first: u64, last: u64 },
    #[error("Timeline reference ends at future seq {last}; spawn is seq {spawn}")]
    FutureInputRef { last: u64, spawn: u64 },
    #[error("unsupported sideband schema version {actual}; expected {expected}")]
    UnsupportedVersion { expected: u8, actual: u8 },
    #[error("sideband event belongs to {actual}, expected {expected}")]
    IdentityMismatch { expected: String, actual: String },
    #[error("sideband event seq {actual} is not the expected contiguous seq {expected}")]
    NonContiguousSeq { expected: u64, actual: u64 },
    #[error("sideband event timestamp must be non-negative")]
    InvalidTimestamp,
    #[error("sideband input ref belongs to Timeline {actual}, expected {expected}")]
    ForeignInputTimeline { expected: String, actual: String },
    #[error("sideband request does not match its parent spawn fact")]
    ParentRequestMismatch,
    #[error("sideband assembly manifest does not match its frozen parent materialization")]
    ParentMaterializationMismatch,
    #[error("sideband request must be seq 0 and unique")]
    InvalidRequestBoundary,
    #[error("sideband request route and executor must be non-empty")]
    IncompleteRequest,
    #[error("sideband attempt input is not covered by the request source refs")]
    InputOutsideSource,
    #[error("sideband attempt assembly manifest is invalid")]
    InvalidAssemblyManifest,
    #[error("sideband attempt selected Surface ids are not canonical or covered by input refs")]
    InvalidSurfaceSelection,
    #[error("sideband attempt requires an open request")]
    AttemptWithoutRequest,
    #[error("sideband attempt number {actual} does not follow {expected}")]
    NonContiguousAttempt { expected: u32, actual: u32 },
    #[error("sideband attempt count exceeds u32 capacity")]
    AttemptOverflow,
    #[error("sideband result requires at least one attempt")]
    ResultWithoutAttempt,
    #[error("sideband result source refs do not identify request 0 and latest attempt {attempt}")]
    InvalidResultSources { attempt: u64 },
    #[error("sideband result is empty")]
    EmptyResult,
    #[error("sideband result evidence is not covered by the successful attempt input refs")]
    EvidenceOutsideInput,
    #[error("sideband already has a result")]
    DuplicateResult,
    #[error("sideband already ended")]
    AlreadyEnded,
    #[error("completed sideband must have one result and no error")]
    InvalidCompletedEnd,
    #[error("failed or cancelled sideband must carry a non-empty error")]
    MissingTerminalError,
}

impl SidebandTimeline {
    pub fn new(sideband_id: String) -> Result<Self, SidebandError> {
        validate_sideband_id(&sideband_id)?;
        Ok(Self {
            sideband_id,
            ..Self::default()
        })
    }

    pub fn from_events(events: Vec<SidebandEvent>) -> Result<Self, SidebandError> {
        let id = events
            .first()
            .map(|event| event.sideband_id.clone())
            .ok_or(SidebandError::InvalidRequestBoundary)?;
        let mut timeline = Self::new(id)?;
        for event in events {
            timeline.accept(event)?;
        }
        Ok(timeline)
    }

    pub fn events(&self) -> &[SidebandEvent] {
        &self.events
    }

    pub fn sideband_id(&self) -> &str {
        &self.sideband_id
    }

    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// Validate this independent ledger against the one immutable fact held by
    /// its initiating Timeline. This is the trust boundary used by resume,
    /// import, and Trajectory; none of those consumers may infer parentage from
    /// directory layout alone.
    pub fn validate_parent(
        &self,
        parent_timeline_id: &str,
        parent: &crate::Timeline,
        spawn_seq: u64,
        spawn: &SidebandSpawnEvent,
    ) -> Result<(), SidebandError> {
        if let Some(source_ref) = spawn
            .source_refs
            .iter()
            .find(|source_ref| source_ref.timeline_id != parent_timeline_id)
        {
            return Err(SidebandError::ForeignInputTimeline {
                expected: parent_timeline_id.to_owned(),
                actual: source_ref.timeline_id.clone(),
            });
        }
        let request = self
            .events
            .first()
            .and_then(|event| match &event.kind {
                SidebandEventKind::Request(request) => Some(request),
                _ => None,
            })
            .ok_or(SidebandError::InvalidRequestBoundary)?;
        if self.sideband_id != spawn.sideband_id
            || request.purpose != spawn.purpose
            || request.source_refs != spawn.source_refs
            || request.initiator_ref
                != format!("t:{parent_timeline_id}/sideband:{}", spawn.sideband_id)
            || !usize::try_from(spawn_seq)
                .ok()
                .and_then(|index| parent.events().get(index))
                .is_some_and(|event| {
                    event.seq.get() == spawn_seq
                        && matches!(
                            &event.kind,
                            crate::TimelineEventKind::Sideband(parent_spawn)
                                if parent_spawn == spawn
                        )
                })
        {
            return Err(SidebandError::ParentRequestMismatch);
        }
        for attempt in self.events.iter().filter_map(|event| match &event.kind {
            SidebandEventKind::Attempt(attempt) => Some(attempt),
            _ => None,
        }) {
            if attempt
                .assembly_manifest
                .context_surface_ids
                .iter()
                .chain(&attempt.assembly_manifest.selected_surface_ids)
                .any(|id| !surface_id_exists(parent, *id))
            {
                return Err(SidebandError::InvalidSurfaceSelection);
            }
        }
        if request.purpose == SidebandPurpose::ContextRecall {
            self.validate_recall_materialization(parent, request)?;
        }
        Ok(())
    }

    fn validate_recall_materialization(
        &self,
        parent: &crate::Timeline,
        request: &SidebandRequest,
    ) -> Result<(), SidebandError> {
        let high_water = request
            .source_refs
            .iter()
            .map(|source_ref| source_ref.last_seq)
            .max()
            .ok_or(SidebandError::ParentMaterializationMismatch)?;
        let high_water = usize::try_from(high_water)
            .map_err(|_| SidebandError::ParentMaterializationMismatch)?;
        let prefix = parent
            .events()
            .get(..=high_water)
            .ok_or(SidebandError::ParentMaterializationMismatch)?;
        let frozen = crate::Timeline::from_events(prefix.to_vec())
            .map_err(|_| SidebandError::ParentMaterializationMismatch)?;
        let branch_ids = frozen
            .branch_transcript_with_ids()
            .0
            .into_iter()
            .collect::<BTreeSet<_>>();
        let unloaded = frozen
            .completed_compaction_unloaded_branch_ids()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let readable = branch_ids
            .intersection(&unloaded)
            .copied()
            .collect::<BTreeSet<_>>();

        for manifest in self.events.iter().filter_map(|event| match &event.kind {
            SidebandEventKind::Attempt(attempt) => Some(&attempt.assembly_manifest),
            _ => None,
        }) {
            if manifest.source_revision != Some(frozen.surface_revision())
                || manifest.context_surface_ids != frozen.surface_ids()
                || manifest
                    .selected_surface_ids
                    .iter()
                    .any(|id| !readable.contains(id))
            {
                return Err(SidebandError::ParentMaterializationMismatch);
            }
        }
        Ok(())
    }

    pub fn prepare(&self, kind: SidebandEventKind) -> Result<SidebandEvent, SidebandError> {
        let event = SidebandEvent {
            version: SIDEBAND_SCHEMA_VERSION,
            sideband_id: self.sideband_id.clone(),
            seq: self.events.len() as u64,
            at_ms: wall_time_ms(),
            kind,
        };
        let mut candidate = self.clone();
        candidate.accept(event.clone())?;
        Ok(event)
    }

    pub fn accept(&mut self, event: SidebandEvent) -> Result<(), SidebandError> {
        self.validate_header(&event)?;
        if self.ended {
            return Err(SidebandError::AlreadyEnded);
        }
        match &event.kind {
            SidebandEventKind::Request(request) => {
                if !self.events.is_empty() {
                    return Err(SidebandError::InvalidRequestBoundary);
                }
                if request.route.model.trim().is_empty()
                    || request.route.backend.trim().is_empty()
                    || request.initiator_ref.trim().is_empty()
                    || request.executor.trim().is_empty()
                {
                    return Err(SidebandError::IncompleteRequest);
                }
                for source_ref in &request.source_refs {
                    source_ref.validate()?;
                }
            }
            SidebandEventKind::Attempt(attempt) => {
                let Some(SidebandEventKind::Request(request)) =
                    self.events.first().map(|event| &event.kind)
                else {
                    return Err(SidebandError::AttemptWithoutRequest);
                };
                if self.result_seq.is_some() {
                    return Err(SidebandError::AttemptWithoutRequest);
                }
                let expected = self
                    .last_attempt
                    .map_or(1, |seq| self.attempt_no_at(seq).saturating_add(1));
                if attempt.attempt_no != expected {
                    return Err(SidebandError::NonContiguousAttempt {
                        expected,
                        actual: attempt.attempt_no,
                    });
                }
                for input_ref in &attempt.input_refs {
                    input_ref.validate()?;
                    if !request
                        .source_refs
                        .iter()
                        .any(|source_ref| range_covers(source_ref, input_ref))
                    {
                        return Err(SidebandError::InputOutsideSource);
                    }
                }
                let manifest = &attempt.assembly_manifest;
                if manifest.strategy.trim().is_empty()
                    || manifest.strategy_version == 0
                    || manifest.max_output_tokens == Some(0)
                {
                    return Err(SidebandError::InvalidAssemblyManifest);
                }
                if !surface_ids_are_unique(&manifest.context_surface_ids)
                    || !surface_ids_are_unique(&manifest.selected_surface_ids)
                    || manifest.context_surface_ids.iter().any(|id| {
                        !request.source_refs.iter().any(|source_ref| {
                            source_ref.first_seq <= id.event.get()
                                && id.event.get() <= source_ref.last_seq
                        })
                    })
                    || manifest.selected_surface_ids.iter().any(|id| {
                        !attempt.input_refs.iter().any(|input_ref| {
                            input_ref.first_seq <= id.event.get()
                                && id.event.get() <= input_ref.last_seq
                        })
                    })
                {
                    return Err(SidebandError::InvalidSurfaceSelection);
                }
                self.last_attempt = Some(event.seq);
            }
            SidebandEventKind::Result(result) => {
                let Some(attempt) = self.last_attempt else {
                    return Err(SidebandError::ResultWithoutAttempt);
                };
                if self.result_seq.is_some() {
                    return Err(SidebandError::DuplicateResult);
                }
                if result.source_event_seqs != [0, attempt] {
                    return Err(SidebandError::InvalidResultSources { attempt });
                }
                if result.raw_output.trim().is_empty() || result.finish.trim().is_empty() {
                    return Err(SidebandError::EmptyResult);
                }
                let SidebandEventKind::Attempt(successful_attempt) =
                    &self.events[attempt as usize].kind
                else {
                    unreachable!("last_attempt always identifies an attempt event")
                };
                for evidence_ref in &result.evidence_refs {
                    evidence_ref.validate()?;
                    if !successful_attempt
                        .input_refs
                        .iter()
                        .any(|input_ref| range_covers(input_ref, evidence_ref))
                    {
                        return Err(SidebandError::EvidenceOutsideInput);
                    }
                }
                self.result_seq = Some(event.seq);
            }
            SidebandEventKind::End(end) => match end.outcome {
                SidebandOutcome::Completed => {
                    if self.result_seq.is_none() || end.error.is_some() {
                        return Err(SidebandError::InvalidCompletedEnd);
                    }
                    self.ended = true;
                }
                SidebandOutcome::Failed | SidebandOutcome::Cancelled => {
                    if end
                        .error
                        .as_deref()
                        .is_none_or(|error| error.trim().is_empty())
                    {
                        return Err(SidebandError::MissingTerminalError);
                    }
                    self.ended = true;
                }
            },
        }
        self.events.push(event);
        Ok(())
    }

    fn validate_header(&self, event: &SidebandEvent) -> Result<(), SidebandError> {
        if event.version != SIDEBAND_SCHEMA_VERSION {
            return Err(SidebandError::UnsupportedVersion {
                expected: SIDEBAND_SCHEMA_VERSION,
                actual: event.version,
            });
        }
        if event.sideband_id != self.sideband_id {
            return Err(SidebandError::IdentityMismatch {
                expected: self.sideband_id.clone(),
                actual: event.sideband_id.clone(),
            });
        }
        let expected = self.events.len() as u64;
        if event.seq != expected {
            return Err(SidebandError::NonContiguousSeq {
                expected,
                actual: event.seq,
            });
        }
        if event.at_ms < 0 {
            return Err(SidebandError::InvalidTimestamp);
        }
        Ok(())
    }

    fn attempt_no_at(&self, seq: u64) -> u32 {
        match &self.events[seq as usize].kind {
            SidebandEventKind::Attempt(attempt) => attempt.attempt_no,
            _ => unreachable!("last_attempt always identifies an attempt event"),
        }
    }
}

fn range_covers(outer: &TimelineRangeRef, inner: &TimelineRangeRef) -> bool {
    outer.timeline_id == inner.timeline_id
        && outer.first_seq <= inner.first_seq
        && inner.last_seq <= outer.last_seq
}

fn surface_ids_are_unique(ids: &[crate::SurfaceId]) -> bool {
    ids.iter().copied().collect::<BTreeSet<_>>().len() == ids.len()
}

fn surface_id_exists(parent: &crate::Timeline, id: crate::SurfaceId) -> bool {
    usize::try_from(id.event.get())
        .ok()
        .and_then(|index| parent.events().get(index))
        .is_some_and(|event| {
            event.seq == id.event
                && matches!(
                    &event.kind,
                    crate::TimelineEventKind::Messages(messages)
                        if usize::try_from(id.item)
                            .ok()
                            .is_some_and(|item| item < messages.items.len())
                )
        })
}

pub fn validate_sideband_id(id: &str) -> Result<(), SidebandError> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| SidebandError::InvalidId)?;
    if parsed.to_string() != id {
        return Err(SidebandError::InvalidId);
    }
    Ok(())
}

fn wall_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> SidebandRequest {
        SidebandRequest {
            purpose: SidebandPurpose::PermissionJudgment,
            prompt: "classify one tool call".into(),
            source_refs: vec![TimelineRangeRef {
                timeline_id: "parent".into(),
                first_seq: 0,
                last_seq: 4,
            }],
            route: SidebandRoute {
                model: "test-model".into(),
                backend: "responses".into(),
            },
            initiator_ref: "t:parent/5".into(),
            executor: "main".into(),
            output_schema: Some(serde_json::json!({"type": "object"})),
        }
    }

    #[test]
    fn lifecycle_is_strict_and_contiguous() {
        let id = uuid::Uuid::now_v7().to_string();
        let mut timeline = SidebandTimeline::new(id).unwrap();
        for kind in [
            SidebandEventKind::Request(request()),
            SidebandEventKind::Attempt(SidebandAttempt {
                attempt_no: 1,
                input_refs: request().source_refs,
                assembly_manifest: SidebandAssemblyManifest {
                    strategy: "all-sources".into(),
                    strategy_version: 1,
                    source_revision: None,
                    context_surface_ids: Vec::new(),
                    selected_surface_ids: Vec::new(),
                    materialized_input_tokens: 32,
                    max_output_tokens: Some(16),
                },
                feedback: None,
            }),
            SidebandEventKind::Result(SidebandResult {
                raw_output: r#"{"decision":"allow","reason":"safe"}"#.into(),
                structured_output: Some(serde_json::json!({"decision": "allow", "reason": "safe"})),
                usage: SidebandUsage::default(),
                finish: "stop".into(),
                source_event_seqs: [0, 1],
                evidence_refs: Vec::new(),
            }),
            SidebandEventKind::End(SidebandEnd {
                outcome: SidebandOutcome::Completed,
                error: None,
            }),
        ] {
            let event = timeline.prepare(kind).unwrap();
            timeline.accept(event).unwrap();
        }
        assert_eq!(timeline.events().len(), 4);

        let encoded = timeline
            .events()
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let decoded = encoded
            .iter()
            .map(|line| serde_json::from_str::<SidebandEvent>(line))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let replayed = SidebandTimeline::from_events(decoded).unwrap();
        assert_eq!(replayed.events().len(), 4);
    }

    #[test]
    fn result_must_reference_latest_attempt() {
        let id = uuid::Uuid::now_v7().to_string();
        let mut timeline = SidebandTimeline::new(id).unwrap();
        for kind in [
            SidebandEventKind::Request(request()),
            SidebandEventKind::Attempt(SidebandAttempt {
                attempt_no: 1,
                input_refs: request().source_refs,
                assembly_manifest: SidebandAssemblyManifest {
                    strategy: "all-sources".into(),
                    strategy_version: 1,
                    source_revision: None,
                    context_surface_ids: Vec::new(),
                    selected_surface_ids: Vec::new(),
                    materialized_input_tokens: 32,
                    max_output_tokens: Some(16),
                },
                feedback: None,
            }),
        ] {
            let event = timeline.prepare(kind).unwrap();
            timeline.accept(event).unwrap();
        }
        let error = timeline
            .prepare(SidebandEventKind::Result(SidebandResult {
                raw_output: "{}".into(),
                structured_output: Some(serde_json::json!({})),
                usage: SidebandUsage::default(),
                finish: "stop".into(),
                source_event_seqs: [0, 0],
                evidence_refs: Vec::new(),
            }))
            .unwrap_err();
        assert!(matches!(
            error,
            SidebandError::InvalidResultSources { attempt: 1 }
        ));
    }

    #[test]
    fn unknown_wire_fields_are_rejected() {
        let id = uuid::Uuid::now_v7().to_string();
        let json = serde_json::json!({
            "version": SIDEBAND_SCHEMA_VERSION,
            "sideband_id": id,
            "seq": 0,
            "at_ms": 1,
            "kind": {
                "type": "request",
                "event": request(),
            },
            "legacy": true
        });
        assert!(serde_json::from_value::<SidebandEvent>(json).is_err());
    }

    #[test]
    fn old_schema_is_not_replayed_by_the_current_reader() {
        let id = uuid::Uuid::now_v7().to_string();
        let timeline = SidebandTimeline::new(id).unwrap();
        let mut event = timeline
            .prepare(SidebandEventKind::Request(request()))
            .unwrap();
        event.version = 2;

        assert!(matches!(
            SidebandTimeline::from_events(vec![event]),
            Err(SidebandError::UnsupportedVersion {
                expected: SIDEBAND_SCHEMA_VERSION,
                actual: 2,
            })
        ));
    }

    #[test]
    fn attempt_input_and_result_evidence_are_bounded_by_frozen_refs() {
        let id = uuid::Uuid::now_v7().to_string();
        let mut timeline = SidebandTimeline::new(id).unwrap();
        let request = timeline
            .prepare(SidebandEventKind::Request(request()))
            .unwrap();
        timeline.accept(request).unwrap();

        let error = timeline
            .prepare(SidebandEventKind::Attempt(SidebandAttempt {
                attempt_no: 1,
                input_refs: vec![TimelineRangeRef {
                    timeline_id: "parent".into(),
                    first_seq: 4,
                    last_seq: 5,
                }],
                assembly_manifest: SidebandAssemblyManifest {
                    strategy: "selected".into(),
                    strategy_version: 1,
                    source_revision: Some(3),
                    context_surface_ids: Vec::new(),
                    selected_surface_ids: Vec::new(),
                    materialized_input_tokens: 8,
                    max_output_tokens: Some(8),
                },
                feedback: None,
            }))
            .unwrap_err();
        assert!(matches!(error, SidebandError::InputOutsideSource));

        let error = timeline
            .prepare(SidebandEventKind::Attempt(SidebandAttempt {
                attempt_no: 1,
                input_refs: vec![TimelineRangeRef {
                    timeline_id: "parent".into(),
                    first_seq: 1,
                    last_seq: 2,
                }],
                assembly_manifest: SidebandAssemblyManifest {
                    strategy: "selected".into(),
                    strategy_version: 1,
                    source_revision: Some(3),
                    context_surface_ids: Vec::new(),
                    selected_surface_ids: vec![crate::SurfaceId {
                        event: serde_json::from_value(serde_json::json!(3)).unwrap(),
                        item: 0,
                    }],
                    materialized_input_tokens: 8,
                    max_output_tokens: Some(8),
                },
                feedback: None,
            }))
            .unwrap_err();
        assert!(matches!(error, SidebandError::InvalidSurfaceSelection));

        let attempt = timeline
            .prepare(SidebandEventKind::Attempt(SidebandAttempt {
                attempt_no: 1,
                input_refs: vec![TimelineRangeRef {
                    timeline_id: "parent".into(),
                    first_seq: 1,
                    last_seq: 2,
                }],
                assembly_manifest: SidebandAssemblyManifest {
                    strategy: "selected".into(),
                    strategy_version: 1,
                    source_revision: Some(3),
                    context_surface_ids: Vec::new(),
                    selected_surface_ids: vec![crate::SurfaceId {
                        event: serde_json::from_value(serde_json::json!(2)).unwrap(),
                        item: 0,
                    }],
                    materialized_input_tokens: 8,
                    max_output_tokens: Some(8),
                },
                feedback: None,
            }))
            .unwrap();
        timeline.accept(attempt).unwrap();

        let error = timeline
            .prepare(SidebandEventKind::Result(SidebandResult {
                raw_output: "result".into(),
                structured_output: None,
                usage: SidebandUsage::default(),
                finish: "stop".into(),
                source_event_seqs: [0, 1],
                evidence_refs: vec![TimelineRangeRef {
                    timeline_id: "parent".into(),
                    first_seq: 3,
                    last_seq: 3,
                }],
            }))
            .unwrap_err();
        assert!(matches!(error, SidebandError::EvidenceOutsideInput));
    }

    #[test]
    fn parent_validation_rejects_recall_selection_that_is_not_unloaded() {
        let mut parent = crate::Timeline::from_seed(vec![ConversationItem::user("live")]).unwrap();
        let live_id = parent.surface_ids()[0];
        let source_ref = TimelineRangeRef {
            timeline_id: "parent".into(),
            first_seq: 0,
            last_seq: 0,
        };
        let sideband_id = uuid::Uuid::now_v7().to_string();
        let spawn = SidebandSpawnEvent {
            sideband_id: sideband_id.clone(),
            purpose: SidebandPurpose::ContextRecall,
            source_refs: vec![source_ref.clone()],
        };
        let spawn_event = parent
            .record(crate::TimelineEventKind::Sideband(spawn.clone()))
            .unwrap();
        let mut sideband = SidebandTimeline::new(sideband_id.clone()).unwrap();
        for kind in [
            SidebandEventKind::Request(SidebandRequest {
                purpose: SidebandPurpose::ContextRecall,
                prompt: "recall a decision".into(),
                source_refs: vec![source_ref.clone()],
                route: SidebandRoute {
                    model: "test-model".into(),
                    backend: "responses".into(),
                },
                initiator_ref: format!("t:parent/sideband:{sideband_id}"),
                executor: "main".into(),
                output_schema: None,
            }),
            SidebandEventKind::Attempt(SidebandAttempt {
                attempt_no: 1,
                input_refs: vec![source_ref],
                assembly_manifest: SidebandAssemblyManifest {
                    strategy: "lexical-neighborhood".into(),
                    strategy_version: 1,
                    source_revision: Some(1),
                    context_surface_ids: vec![live_id],
                    selected_surface_ids: vec![live_id],
                    materialized_input_tokens: 8,
                    max_output_tokens: Some(8),
                },
                feedback: None,
            }),
        ] {
            let event = sideband.prepare(kind).unwrap();
            sideband.accept(event).unwrap();
        }

        assert!(matches!(
            sideband.validate_parent("parent", &parent, spawn_event.seq.get(), &spawn),
            Err(SidebandError::ParentMaterializationMismatch)
        ));
    }
}

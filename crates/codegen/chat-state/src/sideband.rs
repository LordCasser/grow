//! Short-lived derived-computation timelines.
//!
//! A sideband owns one request transaction and an independent sequence space.
//! Parent sessions retain only a [`SidebandSpawnEvent`]; request attempts and
//! results never enter the model-facing Surface.

use std::time::{SystemTime, UNIX_EPOCH};

use sampling_types::ConversationItem;
use serde::{Deserialize, Serialize};

pub const SIDEBAND_SCHEMA_VERSION: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SidebandPurpose {
    CompactionSummary,
    PermissionJudgment,
    SessionTitle,
    SessionRecap,
    PromptSuggestion,
    LazinessJudgment,
    MemoryDream,
    MemoryFlush,
    MemoryRewrite,
    ImageDescription,
    InfoRequest,
    ContextRecall,
}

impl SidebandPurpose {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompactionSummary => "compaction-summary",
            Self::PermissionJudgment => "permission-judgment",
            Self::SessionTitle => "session-title",
            Self::SessionRecap => "session-recap",
            Self::PromptSuggestion => "prompt-suggestion",
            Self::LazinessJudgment => "laziness-judgment",
            Self::MemoryDream => "memory-dream",
            Self::MemoryFlush => "memory-flush",
            Self::MemoryRewrite => "memory-rewrite",
            Self::ImageDescription => "image-description",
            Self::InfoRequest => "info-request",
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandSpawnEvent {
    pub sideband_id: String,
    pub purpose: SidebandPurpose,
    pub input_refs: Vec<TimelineRangeRef>,
}

impl SidebandSpawnEvent {
    pub fn validate(&self) -> Result<(), SidebandError> {
        validate_sideband_id(&self.sideband_id)?;
        for input_ref in &self.input_refs {
            input_ref.validate()?;
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
    pub input_refs: Vec<TimelineRangeRef>,
    pub route: SidebandRoute,
    pub initiator_ref: String,
    pub executor: String,
    pub output_schema: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidebandAttempt {
    pub attempt_no: u32,
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
    #[error("sideband request must be seq 0 and unique")]
    InvalidRequestBoundary,
    #[error("sideband request route and executor must be non-empty")]
    IncompleteRequest,
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

    /// Validate this independent ledger against the one immutable fact held by
    /// its initiating Timeline. This is the trust boundary used by resume,
    /// import, and Trajectory; none of those consumers may infer parentage from
    /// directory layout alone.
    pub fn validate_parent(
        &self,
        parent_timeline_id: &str,
        spawn_seq: u64,
        spawn: &SidebandSpawnEvent,
    ) -> Result<(), SidebandError> {
        if let Some(input_ref) = spawn
            .input_refs
            .iter()
            .find(|input_ref| input_ref.timeline_id != parent_timeline_id)
        {
            return Err(SidebandError::ForeignInputTimeline {
                expected: parent_timeline_id.to_owned(),
                actual: input_ref.timeline_id.clone(),
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
            || request.input_refs != spawn.input_refs
            || request.initiator_ref != format!("t:{parent_timeline_id}/{spawn_seq}")
        {
            return Err(SidebandError::ParentRequestMismatch);
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
                for input_ref in &request.input_refs {
                    input_ref.validate()?;
                }
            }
            SidebandEventKind::Attempt(attempt) => {
                if !matches!(
                    self.events.first().map(|event| &event.kind),
                    Some(SidebandEventKind::Request(_))
                ) || self.result_seq.is_some()
                {
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
            input_refs: vec![TimelineRangeRef {
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
                feedback: None,
            }),
            SidebandEventKind::Result(SidebandResult {
                raw_output: r#"{"decision":"allow","reason":"safe"}"#.into(),
                structured_output: Some(serde_json::json!({"decision": "allow", "reason": "safe"})),
                usage: SidebandUsage::default(),
                finish: "stop".into(),
                source_event_seqs: [0, 1],
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
}

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

pub use tools::implementations::grow_build::coordination::{
    CoordinationError, CoordinationErrorCode,
};

pub const MAX_QUESTION_BYTES: usize = 16 * 1024;
pub const MAX_QUEUED_INQUIRIES: usize = 32;
pub const APPROVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2 * 60);
pub const INQUIRY_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5 * 60);
pub const TERMINAL_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InquiryPhase {
    Discovering,
    Receiving,
    AwaitingApproval,
    Queued,
    Running,
    Reconnecting,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InquiryStatus {
    Answered,
    Rejected,
    Cancelled,
    Unavailable,
    TimedOut,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InquiryOutcome {
    pub inquiry_id: String,
    pub status: InquiryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<CoordinationError>,
}

impl InquiryOutcome {
    pub fn answered(inquiry_id: impl Into<String>, answer: String) -> Self {
        Self {
            inquiry_id: inquiry_id.into(),
            status: InquiryStatus::Answered,
            answer: Some(answer),
            error: None,
        }
    }

    pub fn terminal(
        inquiry_id: impl Into<String>,
        status: InquiryStatus,
        error: impl Into<CoordinationError>,
    ) -> Self {
        debug_assert_ne!(status, InquiryStatus::Answered);
        let mut error = error.into();
        if error.code == CoordinationErrorCode::Failed {
            error.code = match status {
                InquiryStatus::Rejected => CoordinationErrorCode::PermissionDenied,
                InquiryStatus::Unavailable => CoordinationErrorCode::NotFound,
                InquiryStatus::TimedOut => CoordinationErrorCode::TimedOut,
                InquiryStatus::Cancelled => CoordinationErrorCode::Cancelled,
                _ => CoordinationErrorCode::Failed,
            };
        }
        Self {
            inquiry_id: inquiry_id.into(),
            status,
            answer: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InquiryState {
    pub inquiry_id: String,
    pub phase: InquiryPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<InquiryOutcome>,
}

/// Source completion fact carried by the canonical Timeline observation.
/// UiNotice details are a disposable display projection of this fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InquiryAudit {
    pub target_session_id: String,
    pub question: String,
    pub outcome: InquiryOutcome,
}

/// Shell-owned inquiry facts in Timeline's existing Observation family.
/// Recovery reads these typed payloads, never UiNotice prose or replay files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InquiryEvent {
    OutgoingStarted {
        inquiry_id: String,
        target_session_id: String,
        question: String,
    },
    OutgoingCompleted {
        audit: InquiryAudit,
    },
    Incoming {
        inquiry_id: String,
        audit: IncomingInquiryAudit,
    },
}

impl InquiryEvent {
    pub(crate) fn timeline_kind(&self) -> chat_state::TimelineEventKind {
        chat_state::TimelineEventKind::Observation(chat_state::ObservationEvent {
            scope: "coordination".into(),
            name: "inquiry".into(),
            turn: None,
            step: None,
            data: Some(serde_json::to_value(self).expect("inquiry fact serializes")),
        })
    }

    pub(crate) fn from_timeline(event: &chat_state::TimelineEvent) -> Result<Option<Self>, String> {
        let chat_state::TimelineEventKind::Observation(observation) = &event.kind else {
            return Ok(None);
        };
        if observation.scope != "coordination" || observation.name != "inquiry" {
            return Ok(None);
        }
        let value = observation
            .data
            .clone()
            .ok_or("inquiry fact has no payload")?;
        let fact: Self = serde_json::from_value(value).map_err(|error| error.to_string())?;
        if let Self::Incoming { inquiry_id, audit } = &fact {
            if audit.source_peer_id.is_empty()
                || audit.source_session_id.is_empty()
                || audit
                    .outcome
                    .as_ref()
                    .is_some_and(|outcome| &outcome.inquiry_id != inquiry_id)
            {
                return Err("inquiry fact has inconsistent source or outcome identity".into());
            }
        }
        Ok(Some(fact))
    }

    pub(crate) fn notice(&self) -> crate::extensions::notification::UiNotice {
        use crate::extensions::notification::{UiNotice, UiNoticeCategory, UiNoticeTone};
        match self {
            Self::Incoming { inquiry_id, audit } => audit.notice(inquiry_id),
            Self::OutgoingStarted {
                inquiry_id,
                target_session_id,
                question,
            } => UiNotice {
                correlation_id: inquiry_id.clone(),
                category: UiNoticeCategory::Coordination,
                subject: Some("outgoing inquiry".into()),
                description: Some(
                    "Attempting to send a question to another local Grow session".into(),
                ),
                message: format!("Asking session {target_session_id}"),
                tone: UiNoticeTone::Info,
                details: Some(format!(
                    "Inquiry ID: {inquiry_id}\nTarget session: {target_session_id}\n\nQuestion:\n{question}"
                )),
            },
            Self::OutgoingCompleted { audit } => {
                let status =
                    serde_json::to_value(audit.outcome.status).expect("inquiry status serializes");
                UiNotice {
                    correlation_id: audit.outcome.inquiry_id.clone(),
                    category: UiNoticeCategory::Coordination,
                    subject: Some("outgoing inquiry completed".into()),
                    description: Some("Local coordination inquiry terminal state".into()),
                    message: format!(
                        "Inquiry to session {} finished: {}",
                        audit.target_session_id,
                        status.as_str().unwrap()
                    ),
                    tone: match audit.outcome.status {
                        InquiryStatus::Answered => UiNoticeTone::Success,
                        InquiryStatus::Failed => UiNoticeTone::Error,
                        _ => UiNoticeTone::Warning,
                    },
                    details: Some(serde_json::to_string(audit).expect("inquiry audit serializes")),
                }
            }
        }
    }
}

/// Receiving-side Timeline facts, also projected into UiNotice details. Identity is
/// scoped exactly like the runtime's dedup key, never inferred from UI prose.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingInquiryAudit {
    pub source_peer_id: String,
    pub source_session_id: String,
    pub source_cwd: String,
    pub question: String,
    pub approval: Option<String>,
    pub outcome: Option<InquiryOutcome>,
}

impl IncomingInquiryAudit {
    pub(crate) fn received(inquiry: &InboundInquiry) -> Self {
        Self {
            source_peer_id: inquiry.source_peer_id.clone(),
            source_session_id: inquiry.source_session_id.clone(),
            source_cwd: inquiry.source_cwd.clone(),
            question: inquiry.question.clone(),
            approval: None,
            outcome: None,
        }
    }

    pub fn from_notice(notice: &crate::extensions::notification::UiNotice) -> Option<Self> {
        use crate::extensions::notification::UiNoticeCategory;
        if notice.category != UiNoticeCategory::Coordination
            || !matches!(
                notice.subject.as_deref(),
                Some("incoming inquiry" | "inquiry approval" | "inquiry completed")
            )
        {
            return None;
        }
        let audit: Self = serde_json::from_str(notice.details.as_deref()?).ok()?;
        if audit.source_peer_id.is_empty()
            || audit.source_session_id.is_empty()
            || audit
                .outcome
                .as_ref()
                .is_some_and(|outcome| outcome.inquiry_id != notice.correlation_id)
            || (notice.subject.as_deref() == Some("inquiry completed")) != audit.outcome.is_some()
        {
            return None;
        }
        Some(audit)
    }

    pub fn display_details(&self) -> String {
        let mut details = format!(
            "Source session: {}\nSource workspace: {}\n\nQuestion:\n{}",
            self.source_session_id, self.source_cwd, self.question,
        );
        if let Some(approval) = &self.approval {
            details.push_str(&format!("\n\nDecision: {approval}"));
        }
        if let Some(outcome) = &self.outcome {
            let status = serde_json::to_value(outcome.status).expect("inquiry status serializes");
            details.push_str(&format!("\n\nStatus: {}", status.as_str().unwrap()));
            if let Some(answer) = &outcome.answer {
                details.push_str(&format!("\n\nAnswer:\n{answer}"));
            }
            if let Some(error) = &outcome.error {
                details.push_str(&format!("\n\nError: {error}"));
            }
        }
        details
    }

    pub(crate) fn notice(&self, inquiry_id: &str) -> crate::extensions::notification::UiNotice {
        use crate::extensions::notification::{UiNotice, UiNoticeCategory, UiNoticeTone};
        let (subject, label, tone) = match self.outcome.as_ref().map(|outcome| outcome.status) {
            Some(status) => (
                "inquiry completed",
                match status {
                    InquiryStatus::Answered => "Answered session",
                    InquiryStatus::Rejected => "Rejected inquiry from session",
                    InquiryStatus::Cancelled => "Cancelled answer to session",
                    InquiryStatus::Unavailable => "Unable to answer session",
                    InquiryStatus::TimedOut => "Timed out answering session",
                    InquiryStatus::Failed => "Failed to answer session",
                },
                match status {
                    InquiryStatus::Answered => UiNoticeTone::Success,
                    InquiryStatus::Failed => UiNoticeTone::Error,
                    _ => UiNoticeTone::Warning,
                },
            ),
            None => (
                if self.approval.is_some() {
                    "inquiry approval"
                } else {
                    "incoming inquiry"
                },
                "Answering session",
                match self.approval.as_deref() {
                    Some(approval) if approval.starts_with("approved") => UiNoticeTone::Success,
                    Some(_) => UiNoticeTone::Warning,
                    None => UiNoticeTone::Info,
                },
            ),
        };
        UiNotice {
            correlation_id: inquiry_id.to_owned(),
            category: UiNoticeCategory::Coordination,
            subject: Some(subject.into()),
            description: Some("Local coordination inquiry".into()),
            message: format!("{label} {}", self.source_session_id),
            tone,
            details: Some(serde_json::to_string(self).expect("incoming inquiry audit serializes")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InquiryCancellationReason {
    None = 0,
    Explicit = 1,
    SourceUnavailable = 2,
    TimedOut = 3,
}

#[derive(Debug, Clone)]
pub struct InquiryCancellation {
    token: CancellationToken,
    reason: Arc<AtomicU8>,
}

impl InquiryCancellation {
    pub(crate) fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            reason: Arc::new(AtomicU8::new(InquiryCancellationReason::None as u8)),
        }
    }

    pub(crate) fn cancel(&self, reason: InquiryCancellationReason) {
        let _ = self.reason.compare_exchange(
            InquiryCancellationReason::None as u8,
            reason as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.token.cancel();
    }

    pub async fn cancelled(&self) {
        self.token.cancelled().await;
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    pub fn reason(&self) -> InquiryCancellationReason {
        match self.reason.load(Ordering::Acquire) {
            1 => InquiryCancellationReason::Explicit,
            2 => InquiryCancellationReason::SourceUnavailable,
            3 => InquiryCancellationReason::TimedOut,
            _ => InquiryCancellationReason::None,
        }
    }

    pub fn outcome(&self, inquiry_id: &str) -> InquiryOutcome {
        match self.reason() {
            InquiryCancellationReason::TimedOut => InquiryOutcome::terminal(
                inquiry_id,
                InquiryStatus::TimedOut,
                "coordination inquiry timed out",
            ),
            InquiryCancellationReason::SourceUnavailable => InquiryOutcome::terminal(
                inquiry_id,
                InquiryStatus::Cancelled,
                CoordinationError::new(
                    CoordinationErrorCode::SourceUnavailable,
                    "source session is no longer available",
                ),
            ),
            InquiryCancellationReason::Explicit | InquiryCancellationReason::None => {
                InquiryOutcome::terminal(
                    inquiry_id,
                    InquiryStatus::Cancelled,
                    "coordination inquiry was cancelled",
                )
            }
        }
    }
}

#[derive(Debug)]
pub struct InboundInquiry {
    pub inquiry_id: String,
    pub source_peer_id: String,
    pub source_session_id: String,
    pub source_cwd: String,
    pub target_session_id: String,
    pub question: String,
    pub cancellation: InquiryCancellation,
    pub progress: watch::Sender<InquiryPhase>,
    pub respond_to: oneshot::Sender<InquiryOutcome>,
}

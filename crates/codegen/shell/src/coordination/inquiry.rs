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

/// Structured details on the existing durable source UiNotice, not a second
/// job ledger. This supports querying completed inquiries after a reconnect
/// or process restart without replaying a model request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InquiryAudit {
    pub target_session_id: String,
    pub question: String,
    pub outcome: InquiryOutcome,
}

/// Receiving-side facts stored in the existing UiNotice details. Identity is
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
                UiNoticeTone::Info,
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

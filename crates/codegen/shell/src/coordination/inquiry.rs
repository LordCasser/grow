use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;

pub const MAX_QUESTION_BYTES: usize = 16 * 1024;
pub const MAX_QUEUED_INQUIRIES: usize = 32;
pub const APPROVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2 * 60);
pub const INQUIRY_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5 * 60);
pub const TERMINAL_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InquiryPhase {
    Discovering,
    AwaitingApproval,
    Queued,
    Running,
    Reconnecting,
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
    pub error: Option<String>,
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
        error: impl Into<String>,
    ) -> Self {
        debug_assert_ne!(status, InquiryStatus::Answered);
        Self {
            inquiry_id: inquiry_id.into(),
            status,
            answer: None,
            error: Some(error.into()),
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
                "source session is no longer available",
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

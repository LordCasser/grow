mod inquiry;
mod manifest;
mod protocol;
mod runtime;
pub(crate) use inquiry::InquiryAudit;

pub use inquiry::{
    APPROVAL_TIMEOUT, CoordinationError, CoordinationErrorCode, INQUIRY_DEADLINE, InboundInquiry,
    IncomingInquiryAudit, InquiryCancellation, InquiryCancellationReason, InquiryOutcome,
    InquiryPhase, InquiryState, InquiryStatus, MAX_QUESTION_BYTES, MAX_QUEUED_INQUIRIES,
};
pub use manifest::{DiscoveredSession, LocalSessionSnapshot, SubagentStats};
pub(crate) use manifest::{HEARTBEAT_INTERVAL, canonical_cwd};
pub use runtime::{CoordinationHandle, CoordinationRuntime, CoordinationStartError};

mod inquiry;
mod manifest;
mod protocol;
mod runtime;

pub use inquiry::{
    APPROVAL_TIMEOUT, INQUIRY_DEADLINE, InboundInquiry, InquiryCancellation,
    InquiryCancellationReason, InquiryOutcome, InquiryPhase, InquiryStatus, MAX_QUESTION_BYTES,
    MAX_QUEUED_INQUIRIES,
};
pub use manifest::{DiscoveredSession, LocalSessionSnapshot, SubagentStats};
pub(crate) use manifest::{HEARTBEAT_INTERVAL, canonical_cwd};
pub use runtime::{CoordinationHandle, CoordinationRuntime, CoordinationStartError};

mod inquiry;
mod manifest;
mod protocol;
mod runtime;
pub use inquiry::{InquiryAudit, InquiryEvent};
pub(crate) use inquiry::{InquiryAuthority, inquiry_prompt};

pub use inquiry::{
    APPROVAL_TIMEOUT, CoordinationError, CoordinationErrorCode, INQUIRY_DEADLINE, InboundInquiry,
    IncomingInquiryAudit, InquiryAnswerContextProof, InquiryCancellation,
    InquiryCancellationReason, InquiryDirection, InquiryOutcome, InquiryPhase, InquiryState,
    InquiryStatus, MAX_QUESTION_BYTES, MAX_QUEUED_INQUIRIES,
};
pub use manifest::{DiscoveredSession, LocalSessionSnapshot, SubagentStats};
pub(crate) use manifest::{HEARTBEAT_INTERVAL, canonical_cwd};
pub use runtime::{CoordinationHandle, CoordinationRuntime, CoordinationStartError};

/// Peer success must fit the exact response shape written by the wire owner.
pub(crate) fn validate_peer_inquiry_outcome(
    outcome: &InquiryOutcome,
) -> Result<(), crate::local_ipc::frame::FrameError> {
    crate::local_ipc::frame::encode_json_payload(&protocol::Response::Inquiry {
        outcome: outcome.clone(),
    })
    .map(|_| ())
}

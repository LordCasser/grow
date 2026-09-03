//! Layer-2 stream transforms: turn raw HTTP chunk streams into
//! [`SamplingEvent`](crate::events::SamplingEvent) streams.
//!
//! Each backend has its own transform because the raw chunk types
//! differ; backend dispatch happens in M4's
//! [`actor::request_task`](crate::actor::request_task), which knows
//! the API backend from `SamplerConfig.api_backend` and calls the
//! matching `SamplingClient::conversation_stream*` method before
//! handing the result to the corresponding transform here.

pub mod chat_completions;
pub mod collect;
pub mod messages;
pub mod responses;

pub use chat_completions::stream_chat_completions;
pub use collect::collect_response;
pub use messages::stream_messages;
pub use responses::stream_responses;

/// A malformed response fails this request without entering the transient
/// HTTP retry path. Only attach usage known to describe the terminal response.
fn protocol_failure(
    request_id: &crate::types::RequestId,
    message: impl std::fmt::Display,
    usage: Option<sampling_types::TokenUsage>,
) -> crate::events::SamplingEvent {
    let error = sampling_types::SamplingError::Serialization(serde::de::Error::custom(message));
    let mut error = crate::events::SamplingErrorInfo::from(&error);
    error.usage = usage;
    crate::events::SamplingEvent::Failed {
        request_id: request_id.clone(),
        error,
    }
}

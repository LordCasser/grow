pub mod conversation;
pub mod error;
pub mod types;

pub use self::conversation::*;
pub use self::error::{ResponseModelMetadata, Result, SamplingError};
pub use self::types::*;
pub use sampler::ApiBackend;

// Re-export async-openai Responses API types under `rs` namespace
pub use async_openai::types::responses as rs;

// ---------------------------------------------------------------------------
// sampler re-exports
// ---------------------------------------------------------------------------
//
// The actual streaming / retry / HTTP-client logic lives in the
// `sampler` crate. We re-export the public surface here so
// `crate::sampling::{SamplerHandle, SamplerConfig, ...}` paths keep the shell's
// transport surface in one module. The shell-side client implementation and
// composite config are gone; sampler owns both.
pub use sampler::{
    InferenceLatencyStats, OriginClientInfo, RequestId, SamplerActor, SamplerConfig, SamplerHandle,
    SamplingChannel, SamplingClient, SamplingErrorInfo, SamplingErrorKind, SamplingEvent,
};

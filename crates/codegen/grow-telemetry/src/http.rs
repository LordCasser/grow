//! Origin/client identification used by the telemetry engine.
//!
//! [`OriginClientInfo`] is owned by `grow-sampler` (so `SamplerConfig`
//! can use it without depending on shell). Re-exported here so the telemetry
//! engine can label events without depending on shell or sampler internals
//! beyond the type itself.

pub use grow_sampler::OriginClientInfo;

/// Construct an [`OriginClientInfo`] from `GROW_CLIENT_NAME` /
/// `GROW_CLIENT_VERSION` env vars. Returns `None` when `GROW_CLIENT_NAME`
/// is unset. Free function (not an inherent method) because the type lives
/// in another crate.
pub fn origin_client_info_from_env() -> Option<OriginClientInfo> {
    std::env::var("GROW_CLIENT_NAME")
        .ok()
        .map(|product| OriginClientInfo {
            product,
            version: std::env::var("GROW_CLIENT_VERSION").ok(),
        })
}

mod manifest;
mod protocol;
mod runtime;

pub use manifest::{DiscoveredSession, LocalSessionSnapshot, SubagentStats};
pub(crate) use manifest::{HEARTBEAT_INTERVAL, canonical_cwd};
pub use runtime::{CoordinationRuntime, CoordinationStartError};

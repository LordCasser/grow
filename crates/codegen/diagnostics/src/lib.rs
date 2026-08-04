//! Local diagnostics and structured logging for Grow.

mod appender;
pub mod context;
pub mod debug_log;
pub mod enums;
pub mod events;
pub mod hooks_log;
pub mod id;
pub mod instrumentation;
pub mod memory_events;
pub mod memory_log;
pub mod prompt_timing;
pub mod sampling_log;
pub mod session_ctx;
pub mod session_metrics;
pub mod tls;
pub mod unified_log;

pub use events::DiagnosticEvent;
pub use session_ctx::{DiagnosticCtx, emit_event, log_event, log_session_event, with_session_ctx};

//! Session lifecycle event structs.
//!
//! Re-exported from `grow-diagnostics`.
//! The structs themselves live in the diagnostics crate; this module preserves
//! the existing import path so nothing else in shell needs to change.

pub(crate) use grow_diagnostics::session_metrics::{
    DoomLoopRecovery, SessionStarted, Turn, TurnCompletedLifecycle,
};

//! Session-specific integration for the standalone `memory` crate.
//!
//! The engine is consumed directly through `memory::*`; only lifecycle hooks
//! that depend on shell session state live here.

pub mod hooks;

#![allow(
    unused_imports,
    unused_variables,
    unused_mut,
    unreachable_code,
    dead_code
)]
pub(crate) use ::diagnostics::unified_log;
pub use tracing_macros::{teprintln, timed, tprintln};
pub mod active_sessions;
pub mod agent;
pub mod auth;
pub mod builtin;
pub mod bundle;
pub mod cli_models;
pub mod config;
pub mod coordination;
pub use shell_base::cpu_profile;
pub use shell_base::env;
pub mod extensions;
pub mod heap_profile;
pub use grow_http as http;
pub mod inspect;
pub mod instrumentation;
pub mod leader;
pub mod local_ipc;
pub mod mcp_doctor;
pub mod plugin;
pub mod remote;
pub mod sampling;
pub mod session;
pub mod terminal;
#[cfg(test)]
pub(crate) mod test_support;
pub mod tools;
pub mod trace_classifier;
pub mod util;

//! Process-local transport primitives shared by Grow IPC protocols.

pub mod frame;
#[cfg(windows)]
pub(crate) mod security;
pub mod transport;

//! Remote storage client for the backend.

pub mod client;

pub(crate) use client::DEFAULT_CONTEXT_WINDOW;
pub use client::{BackendError, fetch_bundle};

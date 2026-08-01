//! Remote storage client for the backend.

pub mod client;

pub(crate) use client::DEFAULT_CONTEXT_WINDOW;
pub use client::{BackendError, FetchedBundle, fetch_bundle, fetch_subagent_bundle};

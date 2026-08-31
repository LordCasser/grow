//! Remote storage client for the backend.

pub mod client;

pub use client::BackendError;
pub(crate) use client::{BundleServiceCredential, DEFAULT_CONTEXT_WINDOW, fetch_bundle};

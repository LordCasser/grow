//! BYOK credential seam shared by HTTP clients without importing shell types.

pub mod auth_provider;
#[cfg(feature = "middleware")]
pub mod header_middleware;
pub mod visibility;

pub use auth_provider::{AuthCredentialProvider, CredentialSnapshot, StaticAuthCredentialProvider};
#[cfg(feature = "middleware")]
pub use header_middleware::AuthHeaderMiddleware;
pub use visibility::HttpAuth;

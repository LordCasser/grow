//! BYOK credential helpers.
//!
//! Grow deliberately owns no OAuth/OIDC login, refresh token, or account
//! session. Model credentials come from model/provider configuration, the
//! environment, or an explicitly configured command helper.

mod auth_provider;
mod token_output;

pub use auth_provider::{AuthProviderConfig, AuthProviderRef};
pub(crate) use auth_provider::{
    PROVIDER_TIMEOUT_CEILING_SECS, PROVIDER_TOKEN_EXPIRY_SKEW_SECS, ProviderRefreshOutcome,
};

#[cfg(test)]
pub(crate) use auth_provider::{test_backdate_provider_mint, test_counting_provider};

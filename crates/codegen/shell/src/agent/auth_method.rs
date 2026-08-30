//! ACP representation of Grow's BYOK-only credential boundary.
//!
//! Grow advertises one non-interactive method. The method selects credentials
//! already supplied by the user through provider config, environment variables,
//! or a local key helper; it never starts a login flow.

use acp_transport::protocol as acp;

use crate::agent::config::ModelEntry;

pub(crate) type SharedAuthMethodId = std::sync::Arc<arc_swap::ArcSwapOption<acp::AuthMethodId>>;

pub(crate) fn new_shared_auth_method_id(initial: Option<acp::AuthMethodId>) -> SharedAuthMethodId {
    std::sync::Arc::new(arc_swap::ArcSwapOption::new(
        initial.map(std::sync::Arc::new),
    ))
}

pub const GROW_API_KEY_ENV_VAR: &str = "GROW_API_KEY";

pub fn read_provider_api_key_env() -> Result<String, std::env::VarError> {
    std::env::var(GROW_API_KEY_ENV_VAR)
}

pub fn has_provider_api_key_env() -> bool {
    read_provider_api_key_env().is_ok()
}

/// A configured model is enough to advertise BYOK: local endpoints may be
/// intentionally keyless and still must not be redirected to a login UI.
pub fn should_advertise_provider_api_key<'a, I>(models: I) -> bool
where
    I: IntoIterator<Item = &'a ModelEntry>,
{
    models.into_iter().next().is_some()
}

pub struct BuiltAuthMethods {
    pub methods: Vec<acp::AuthMethod>,
    pub default_auth_method_id: acp::AuthMethodId,
}

pub fn build_auth_methods() -> BuiltAuthMethods {
    BuiltAuthMethods {
        methods: vec![provider_api_key_auth_method()],
        default_auth_method_id: acp::AuthMethodId::new(PROVIDER_API_KEY_METHOD_ID),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethodKind {
    ProviderApiKey,
    Unknown,
}

impl AuthMethodKind {
    pub fn from_id(id: &acp::AuthMethodId) -> Self {
        match id.0.as_ref() {
            PROVIDER_API_KEY_METHOD_ID => Self::ProviderApiKey,
            _ => Self::Unknown,
        }
    }

    pub fn is_api_key(self) -> bool {
        matches!(self, Self::ProviderApiKey)
    }

    pub fn auth_error_message(self) -> &'static str {
        AUTH_ERROR_API_KEY
    }
}

/// Per-model status used for diagnostics and fail-closed config reloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelByok {
    Byok,
    NotByok,
    Unknown,
}

impl ModelByok {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Byok => "byok",
            Self::NotByok => "keyless",
            Self::Unknown => "unknown",
        }
    }
}

pub const AUTH_ERROR_API_KEY: &str = "Authentication failed. Set GROW_API_KEY or configure api_key/env_key/auth_provider in ~/.grow/config.toml.";

pub const PREFERRED_API_KEY_UNAVAILABLE: &str = "no BYOK provider is configured (set GROW_API_KEY or model api_key/env_key/auth_provider in config.toml).";

pub const PROVIDER_API_KEY_METHOD_ID: &str = "provider.api_key";

pub fn provider_api_key_auth_method() -> acp::AuthMethod {
    acp::AuthMethod::Agent(
        acp::AuthMethodAgent::new(
            acp::AuthMethodId::new(PROVIDER_API_KEY_METHOD_ID),
            "BYOK provider configuration".to_string(),
        )
        .description(Some(format!(
            "{GROW_API_KEY_ENV_VAR} or api_key/env_key/auth_provider in config.toml"
        ))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_exactly_one_non_interactive_method() {
        let built = build_auth_methods();
        assert_eq!(built.methods.len(), 1);
        assert_eq!(
            built.default_auth_method_id.0.as_ref(),
            PROVIDER_API_KEY_METHOD_ID
        );
        assert!(AuthMethodKind::from_id(&built.default_auth_method_id).is_api_key());
    }
}

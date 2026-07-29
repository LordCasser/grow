use super::model::TEAM_PRINCIPAL_TYPE;
use serde::{Deserialize, Serialize};
fn default_oidc_scopes() -> Vec<String> {
    vec![
        "openid".into(),
        "profile".into(),
        "email".into(),
        "offline_access".into(),
    ]
}
/// Conservative OAuth scopes used only for an explicitly configured legacy
/// service auth block. LLM providers normally declare their scopes under
/// `[auth_provider.<name>]`.
fn default_oauth2_scopes() -> Vec<String> {
    vec![
        "openid".into(),
        "profile".into(),
        "email".into(),
        "offline_access".into(),
    ]
}
fn default_team_oauth2_scopes() -> Vec<String> {
    default_oauth2_scopes()
}
/// Pin automatic auth to one method (`[auth] preferred_method` in config.toml).
///
/// When set, only that method is used for automatic selection; if it is
/// unavailable, auth fails (no silent fallthrough to the other method).
/// Unset keeps today's multi-method fallthrough (session preferred when both
/// exist). Config-toml only — not remote settings, settings UI, or env.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferredAuthMethod {
    /// `GROW_API_KEY` / locally stored API key / per-model BYOK (`provider.api_key`).
    ApiKey,
    /// OIDC / OAuth2 session (`cached_token`, interactive `service.example.com` / `oidc`,
    /// including devbox-minted OIDC).
    Oidc,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServiceAuthConfig {
    pub service_ws_origin: String,
    pub service_ws_url: String,
    pub token_header: String,
    /// OIDC config for customer-provided IdPs. See [`OidcAuthConfig`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc: Option<OidcAuthConfig>,
    /// OAuth2 provider config. When set, preferred over the legacy relay flow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth2: Option<OAuth2ProviderConfig>,
    /// External auth provider command (stdout = token, stderr = user UX, exit 0 = success).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_provider_command: Option<String>,
    /// Login button label (env: `GROW_AUTH_PROVIDER_LABEL`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_provider_label: Option<String>,
    /// Token TTL in seconds for external auth providers that output bare
    /// tokens without `expires_in`. Synthesizes `expires_at` so proactive
    /// refresh works. Env: `GROW_AUTH_TOKEN_TTL`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token_ttl: Option<u64>,
    /// Admin kill switch: when `Some(true)`, the `provider.api_key` auth method is
    /// neither advertised nor accepted, so `GROW_API_KEY`/per-model credentials
    /// can't bypass the deployment's IdP login. Env: `GROW_DISABLE_API_KEY_AUTH`.
    /// Parity with common force-login-method admin knobs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_api_key_auth: Option<bool>,
    /// Restrict login to a specific team — the login token's team principal must
    /// equal this. Put in `requirements.toml` to enforce as non-overridable policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_login_team_uuid: Option<ForceLoginTeam>,
    /// Pin automatic auth to `api_key` or `oidc`. When set and the chosen
    /// method is unavailable, auth fails (no fallthrough). Unset keeps
    /// multi-method fallthrough. Config.toml only (`[auth] preferred_method`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_method: Option<PreferredAuthMethod>,
}
/// Team login restriction. TOML string or array; an empty array fails closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ForceLoginTeam {
    /// The only allowed team.
    Single(String),
    /// Allowed teams; empty = fail closed.
    AnyOf(Vec<String>),
}
/// Customer OIDC Identity Provider configuration (`[auth.oidc]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcAuthConfig {
    pub issuer: String,
    pub client_id: String,
    #[serde(default = "default_oidc_scopes")]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,
}
/// OAuth2 provider configuration (`GROW_OAUTH2_ISSUER` / `GROW_OAUTH2_CLIENT_ID`).
///
/// Uses the standard OAuth 2.1 Auth Code + PKCE flow via [`OidcAuthConfig`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuth2ProviderConfig {
    pub issuer: String,
    pub client_id: String,
    #[serde(default = "default_oauth2_scopes")]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
    /// Client-supplied referrer for OAuth usage-attribution analytics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referrer: Option<String>,
}
/// Grow has no built-in OAuth issuer or browser-app origin. Provider login uses
/// the issuer declared in config and the normal redirect flow.
pub fn allowed_accounts_app_origins() -> Vec<String> {
    Vec::new()
}
/// Build a CORS layer that accepts requests from the accounts-app deployments
/// listed in [`allowed_accounts_app_origins`] for the given HTTP method.
///
/// Callers can chain additional configuration (e.g. `.allow_headers(...)` or
/// `.allow_private_network(true)`) onto the returned layer.
pub fn accounts_app_cors_layer(method: axum::http::Method) -> tower_http::cors::CorsLayer {
    tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::list(
            allowed_accounts_app_origins()
                .iter()
                .filter_map(|origin| match origin.parse() {
                    Ok(value) => Some(value),
                    Err(_) => {
                        tracing::warn!(origin, "skipping malformed accounts-app CORS origin");
                        None
                    }
                }),
        ))
        .allow_methods([method])
}
const DEFAULT_OAUTH2_REFERRER: &str = "grow";
/// Stable neutral issuer used by auth tests and examples.
pub const TEST_OAUTH2_ISSUER: &str = "https://login.example.com";
/// Historical service-auth scope. It is deliberately local and cannot match
/// credentials created by an upstream vendor client.
pub const LEGACY_AUTH_SCOPE: &str = "grow::service";
impl ServiceAuthConfig {
    /// Whether `provider.api_key` auth is disabled. Pinning a team
    /// (`force_login_team_uuid`) implies this — team membership can't be verified
    /// from a bare API key, so it must go through IdP login. The
    /// `GROW_DISABLE_API_KEY_AUTH` env lockdown is sticky: because the env value
    /// seeds `default()` (the merge base), a lower-trust user `config.toml` could
    /// otherwise set `disable_api_key_auth = false` and override it — so the env
    /// is OR-ed in here and cannot be turned back off by a user layer. Trusted
    /// `requirements.toml` already wins over `config.toml` via layer precedence.
    pub fn api_key_auth_disabled(&self) -> bool {
        self.disable_api_key_auth == Some(true)
            || self.force_login_team_uuid.is_some()
            || env_lockdown_forced()
    }
    /// When `preferred_method = api_key`, automatic OIDC paths (devbox mint,
    /// interactive browser login, external auth provider) must not run — the
    /// pin is fail-closed. Explicit `grow login --devbox` / `--api-key` bypass
    /// this by not consulting automatic flow helpers.
    pub fn blocks_automatic_oidc(&self) -> bool {
        matches!(self.preferred_method, Some(PreferredAuthMethod::ApiKey))
    }
    /// The auth.json scope key for this config.
    pub fn auth_scope(&self) -> String {
        if let Some(ref oidc) = self.oidc {
            format!("{}::{}", oidc.issuer.trim_end_matches('/'), oidc.client_id)
        } else if let Some(ref oauth2) = self.oauth2 {
            oauth2.auth_scope()
        } else {
            LEGACY_AUTH_SCOPE.to_owned()
        }
    }
}
impl OAuth2ProviderConfig {
    pub fn is_team_principal(&self) -> bool {
        self.principal_type.as_deref() == Some(TEAM_PRINCIPAL_TYPE)
    }
    pub fn from_env() -> Option<Self> {
        let issuer = std::env::var("GROW_OAUTH2_ISSUER").ok()?;
        let client_id = std::env::var("GROW_OAUTH2_CLIENT_ID").ok()?;
        let principal_type = std::env::var("GROW_OAUTH2_PRINCIPAL_TYPE").ok();
        let principal_id = std::env::var("GROW_OAUTH2_PRINCIPAL_ID").ok();
        let default_scopes = match principal_type.as_deref() {
            Some(TEAM_PRINCIPAL_TYPE) => default_team_oauth2_scopes(),
            _ => default_oauth2_scopes(),
        };
        Some(Self {
            issuer,
            client_id,
            scopes: std::env::var("GROW_OAUTH2_SCOPES")
                .map(|s| s.split(',').map(|s| s.trim().to_owned()).collect())
                .unwrap_or(default_scopes),
            principal_type,
            principal_id,
            referrer: Some(
                std::env::var("GROW_OAUTH2_REFERRER")
                    .unwrap_or_else(|_| DEFAULT_OAUTH2_REFERRER.to_owned()),
            ),
        })
    }
    /// Convert to [`OidcAuthConfig`] to reuse the OIDC login flow.
    pub fn as_oidc(&self) -> OidcAuthConfig {
        OidcAuthConfig {
            issuer: self.issuer.clone(),
            client_id: self.client_id.clone(),
            scopes: self.scopes.clone(),
            audience: None,
        }
    }
    pub fn base_auth_scope(&self) -> String {
        format!("{}::{}", self.issuer.trim_end_matches('/'), self.client_id)
    }
    pub fn auth_scope(&self) -> String {
        self.base_auth_scope()
    }
}
impl Default for ServiceAuthConfig {
    fn default() -> Self {
        let oidc = OidcAuthConfig::from_env();
        let oauth2 = oidc
            .is_none()
            .then(OAuth2ProviderConfig::from_env)
            .flatten();
        Self {
            service_ws_origin: std::env::var("GROW_WS_ORIGIN").unwrap_or_default(),
            service_ws_url: std::env::var("GROW_WS_URL").unwrap_or_default(),
            token_header: "grow-cli".to_owned(),
            oidc,
            oauth2,
            auth_provider_command: std::env::var("GROW_AUTH_PROVIDER_COMMAND").ok(),
            auth_provider_label: std::env::var("GROW_AUTH_PROVIDER_LABEL").ok(),
            auth_token_ttl: std::env::var("GROW_AUTH_TOKEN_TTL")
                .ok()
                .and_then(|v| v.parse().ok()),
            disable_api_key_auth: std::env::var("GROW_DISABLE_API_KEY_AUTH")
                .ok()
                .map(|v| env_flag_enabled(&v)),
            force_login_team_uuid: None,
            preferred_method: None,
        }
    }
}
/// Parse a boolean env-var value for grow's on/off flags. A bare presence
/// enables the flag, but the common falsy spellings (`0`, `false`, `off`,
/// `no`, empty) count as disabled — so e.g. `GROW_DISABLE_API_KEY_AUTH=false`
/// does NOT turn the kill switch on.
fn env_flag_enabled(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "off" | "no"
    )
}
/// True when the admin has set `GROW_DISABLE_API_KEY_AUTH` to a truthy value in
/// the process environment. Read live (call-time) and OR-ed into
/// `api_key_auth_disabled()` so the env lockdown is non-overridable by a
/// user-layer `config.toml`.
fn env_lockdown_forced() -> bool {
    std::env::var("GROW_DISABLE_API_KEY_AUTH")
        .ok()
        .is_some_and(|v| env_flag_enabled(&v))
}
impl OidcAuthConfig {
    pub fn from_env() -> Option<Self> {
        let issuer = std::env::var("GROW_OIDC_ISSUER").ok()?;
        let client_id = std::env::var("GROW_OIDC_CLIENT_ID").ok()?;
        Some(Self {
            issuer,
            client_id,
            scopes: std::env::var("GROW_OIDC_SCOPES")
                .map(|s| s.split(',').map(|s| s.trim().to_owned()).collect())
                .unwrap_or_else(|_| default_oidc_scopes()),
            audience: std::env::var("GROW_OIDC_AUDIENCE").ok(),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn team_auth_scope_is_base_scope() {
        let cfg = OAuth2ProviderConfig {
            issuer: "https://login.example.com".into(),
            client_id: "client-123".into(),
            scopes: default_team_oauth2_scopes(),
            principal_type: Some("Team".into()),
            principal_id: Some("team-abc".into()),
            referrer: Some("grow-build".into()),
        };
        assert_eq!(cfg.auth_scope(), "https://login.example.com::client-123");
    }
    #[test]
    fn env_flag_enabled_treats_falsy_spellings_as_off() {
        for off in ["", " ", "0", "false", "FALSE", "off", "No", "  false  "] {
            assert!(!env_flag_enabled(off), "{off:?} should be off");
        }
        for on in ["1", "true", "yes", "on", "enabled"] {
            assert!(env_flag_enabled(on), "{on:?} should be on");
        }
    }
    #[test]
    fn personal_auth_scope_is_base_scope() {
        let cfg = OAuth2ProviderConfig {
            issuer: "https://login.example.com".into(),
            client_id: "client-123".into(),
            scopes: default_oauth2_scopes(),
            principal_type: None,
            principal_id: None,
            referrer: Some("grow-build".into()),
        };
        assert_eq!(cfg.auth_scope(), "https://login.example.com::client-123");
    }
    #[test]
    fn no_oauth_browser_origin_is_built_in() {
        assert!(allowed_accounts_app_origins().is_empty());
    }
    #[test]
    fn default_oauth2_scopes_are_provider_neutral() {
        let scopes = default_oauth2_scopes();
        let scopes: Vec<&str> = scopes.iter().map(String::as_str).collect();
        assert_eq!(scopes, ["openid", "profile", "email", "offline_access",]);
    }
    #[test]
    fn preferred_method_deserializes_from_toml() {
        let cfg: ServiceAuthConfig = toml::from_str(
            r#"
            preferred_method = "api_key"
            "#,
        )
        .expect("parse");
        assert_eq!(cfg.preferred_method, Some(PreferredAuthMethod::ApiKey));
        let cfg: ServiceAuthConfig = toml::from_str(
            r#"
            preferred_method = "oidc"
            "#,
        )
        .expect("parse");
        assert_eq!(cfg.preferred_method, Some(PreferredAuthMethod::Oidc));
        let cfg: ServiceAuthConfig = toml::from_str("").expect("parse empty");
        assert_eq!(cfg.preferred_method, None);
    }
}

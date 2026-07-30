use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const TOKEN_TTL: Duration = Duration::days(30);
const DEFAULT_EARLY_INVALIDATION_SECS: u64 = 300; // 5 minutes

/// auth.json scope key for plain API key auth (`grow login --api-key`).
pub const API_KEY_SCOPE: &str = "grow::api_key";

const BLOCKED_REASON_NO_LOGS: &str = "BLOCKED_REASON_NO_LOGS";
const BLOCKED_REASON_NO_LOGS_MODERATED: &str = "BLOCKED_REASON_NO_LOGS_MODERATED";

/// Token provenance (debugging/auth.json only -- no code branches on this).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// OIDC or OAuth2 interactive login via customer IdP
    #[serde(alias = "oidc")]
    Oidc,
    /// External auth provider binary
    External,
    /// Plain API key (e.g. from an embedding client or `grow login --api-key`)
    ApiKey,
}

/// Wire value of `principal_type` for team OAuth principals (capitalized by
/// the auth service). Single source for every comparison site.
pub(crate) const TEAM_PRINCIPAL_TYPE: &str = "Team";

#[derive(Clone, Serialize, Deserialize)]
pub struct ProviderAuth {
    pub key: String,
    pub auth_mode: AuthMode,
    pub create_time: DateTime<Utc>,
    pub user_id: String,
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_image_asset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub team_blocked_reasons: Vec<String>,
    /// Refresh token (OIDC/OAuth2 or external provider).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,

    /// Server-provided expiration (from OIDC `expires_in`).
    /// When present, takes precedence over the hardcoded `TOKEN_TTL`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,

    /// Issuer URL that issued this token. For OIDC credentials it drives
    /// refresh via discovery; for external-provider credentials it is the
    /// provider's `issuer` claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc_issuer: Option<String>,

    /// OIDC client_id used to obtain this token (needed for refresh).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
}

impl std::fmt::Debug for ProviderAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderAuth")
            .field("key", &token_suffix(&self.key))
            .field("auth_mode", &self.auth_mode)
            .field("user_id", &self.user_id)
            .field("expires_at", &self.expires_at)
            .field(
                "refresh_token",
                &self.refresh_token.as_deref().map(token_suffix),
            )
            .finish_non_exhaustive()
    }
}

impl ProviderAuth {
    /// Seconds since this credential was minted. Negative when the local
    /// clock stepped back past `create_time` (NTP correction, VM restore, or
    /// a sibling machine's clock via an adopted auth.json) — `create_time`
    /// is always stamped from the minting machine's local clock.
    pub(crate) fn mint_age_seconds(&self) -> i64 {
        Utc::now()
            .signed_duration_since(self.create_time)
            .num_seconds()
    }

    /// `true` for an explicitly configured refreshable service credential.
    /// Endpoint selection is independent and must also be configured.
    pub fn is_service_auth(&self) -> bool {
        matches!(self.auth_mode, AuthMode::Oidc | AuthMode::External)
    }

    /// `true` when this auth can access explicitly configured managed services.
    pub fn is_managed_mcp_eligible(&self) -> bool {
        self.is_service_auth()
    }

    /// Whether this credential can access `supported_in_api: false` models.
    ///
    /// OIDC sessions always qualify; external-provider credentials qualify when explicitly configured.
    /// Plain API keys never do.
    pub fn is_session_auth(&self) -> bool {
        match self.auth_mode {
            AuthMode::Oidc => true,
            AuthMode::External => self.is_service_auth(),
            AuthMode::ApiKey => false,
        }
    }

    pub fn is_team_principal(&self) -> bool {
        self.principal_type.as_deref() == Some(TEAM_PRINCIPAL_TYPE) && self.team_id.is_some()
    }

    /// `true` when the team has Zero Data Retention (ZDR) enabled.
    pub fn is_zdr_team(&self) -> bool {
        self.team_blocked_reasons
            .iter()
            .any(|r| r == BLOCKED_REASON_NO_LOGS || r == BLOCKED_REASON_NO_LOGS_MODERATED)
    }

    /// Carry `/user`-derived fields from a previous auth so refresh rebuilds don't drop them.
    pub(crate) fn carry_user_profile_from(&mut self, prev: &ProviderAuth) {
        self.user_id = prev.user_id.clone();
        self.email = prev.email.clone();
        self.principal_type = prev.principal_type.clone();
        self.principal_id = prev.principal_id.clone();
        self.team_id = prev.team_id.clone();
        self.team_name = prev.team_name.clone();
        self.team_role = prev.team_role.clone();
        self.organization_id = prev.organization_id.clone();
        self.organization_name = prev.organization_name.clone();
        self.organization_role = prev.organization_role.clone();
        self.user_blocked_reason = prev.user_blocked_reason.clone();
        self.team_blocked_reasons = prev.team_blocked_reasons.clone();
    }
}

impl Default for ProviderAuth {
    fn default() -> Self {
        Self {
            key: String::new(),
            auth_mode: AuthMode::Oidc,
            create_time: Utc::now(),
            user_id: String::new(),
            email: None,
            first_name: None,
            last_name: None,
            profile_image_asset_id: None,
            principal_type: None,
            principal_id: None,
            team_id: None,
            team_name: None,
            team_role: None,
            organization_id: None,
            organization_name: None,
            organization_role: None,
            user_blocked_reason: None,
            team_blocked_reasons: vec![],
            refresh_token: None,
            expires_at: None,
            oidc_issuer: None,
            oidc_client_id: None,
        }
    }
}

#[cfg(test)]
impl ProviderAuth {
    /// Returns a `ProviderAuth` with sensible defaults for tests. Override fields
    /// with struct update syntax:
    /// ```ignore
    /// ProviderAuth { key: "my-key".into(), ..ProviderAuth::test_default() }
    /// ```
    pub fn test_default() -> Self {
        Self {
            key: "test-key".into(),
            user_id: "test-user".into(),
            ..Default::default()
        }
    }
}

pub(crate) type AuthStore = BTreeMap<String, ProviderAuth>;

/// User information from the cli-chat-proxy `GET /v1/user` endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserInfo {
    pub(crate) user_id: String,
    #[serde(default)]
    pub(super) email: Option<String>,
    #[serde(default)]
    pub(super) first_name: Option<String>,
    #[serde(default)]
    pub(super) last_name: Option<String>,
    #[serde(default)]
    pub(super) profile_image_asset_id: Option<String>,
    #[serde(default)]
    pub(super) principal_type: Option<String>,
    #[serde(default)]
    pub(super) principal_id: Option<String>,
    #[serde(default)]
    pub(super) team_id: Option<String>,
    #[serde(default)]
    pub(super) team_name: Option<String>,
    #[serde(default)]
    pub(super) team_role: Option<String>,
    #[serde(default)]
    pub(super) organization_id: Option<String>,
    #[serde(default)]
    pub(super) organization_name: Option<String>,
    #[serde(default)]
    pub(super) organization_role: Option<String>,
    #[serde(default)]
    pub(super) user_blocked_reason: Option<String>,
    #[serde(default)]
    pub(super) team_blocked_reasons: Option<Vec<String>>,
}

/// Last 12 chars of a token string, safe for diagnostic logging.
/// Uses the tail because JWT access tokens all share the same base64
/// header prefix (`eyJ0eXAiOiJh…`); the tail (signature bytes) is
/// unique per token and makes `key_changed` / `is_stale_snapshot`
/// diagnostics meaningful.
pub(crate) fn token_suffix(t: &str) -> &str {
    let len = t.len();
    if len > 12 { &t[len - 12..] } else { t }
}

/// Look up auth from the store by its exact scope key.
pub fn lookup_auth(map: &AuthStore, scope: &str) -> Option<ProviderAuth> {
    map.get(scope).cloned()
}

/// Early-invalidation buffer. Override with `GROW_AUTH_EARLY_INVALIDATION_SECS`
/// for testing (e.g. `=5` to shrink the buffer to 5 seconds).
pub(super) fn early_invalidation() -> Duration {
    std::env::var("GROW_AUTH_EARLY_INVALIDATION_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(|s| Duration::seconds(s as i64))
        .unwrap_or_else(|| Duration::seconds(DEFAULT_EARLY_INVALIDATION_SECS as i64))
}

pub(crate) fn is_expired(auth: &ProviderAuth) -> bool {
    is_expired_with_buffer(auth, early_invalidation())
}

/// Like [`is_expired`] but with an explicit pre-expiry buffer. Pass
/// `Duration::zero()` for actual (hard) expiry — the instant the token would
/// really be rejected on the wire, with no early-invalidation margin.
pub(crate) fn is_expired_with_buffer(auth: &ProviderAuth, buffer: Duration) -> bool {
    if let Some(expires_at) = auth.expires_at {
        Utc::now() >= (expires_at - buffer)
    } else {
        let age = Utc::now().signed_duration_since(auth.create_time);
        age >= (TOKEN_TTL - buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_auth(mode: AuthMode) -> ProviderAuth {
        ProviderAuth {
            key: "k".into(),
            auth_mode: mode,
            create_time: Utc::now(),
            user_id: "u".into(),
            email: None,
            first_name: None,
            last_name: None,
            profile_image_asset_id: None,
            principal_type: None,
            principal_id: None,
            team_id: None,
            team_name: None,
            team_role: None,
            organization_id: None,
            organization_name: None,
            organization_role: None,
            user_blocked_reason: None,
            team_blocked_reasons: vec![],
            refresh_token: None,
            expires_at: None,
            oidc_issuer: None,
            oidc_client_id: None,
        }
    }

    #[test]
    fn is_service_auth_matrix() {
        use crate::auth::TEST_OAUTH2_ISSUER;
        let with_issuer = |mode: AuthMode, issuer: Option<&str>| ProviderAuth {
            oidc_issuer: issuer.map(str::to_owned),
            ..make_auth(mode)
        };

        // OIDC and explicitly configured external auth qualify regardless of issuer.
        assert!(with_issuer(AuthMode::Oidc, Some(TEST_OAUTH2_ISSUER)).is_service_auth());
        assert!(with_issuer(AuthMode::External, Some(TEST_OAUTH2_ISSUER)).is_service_auth());
        assert!(with_issuer(AuthMode::Oidc, None).is_service_auth());
        assert!(with_issuer(AuthMode::External, None).is_service_auth());
        assert!(with_issuer(AuthMode::Oidc, Some("https://idp.acme.example")).is_service_auth());
        assert!(
            with_issuer(AuthMode::External, Some("https://idp.acme.example")).is_service_auth()
        );

        // API keys stay outside configured service auth.
        assert!(!with_issuer(AuthMode::ApiKey, Some(TEST_OAUTH2_ISSUER)).is_service_auth());
    }

    #[test]
    fn is_session_auth_accepts_configured_external_provider() {
        use crate::auth::TEST_OAUTH2_ISSUER;
        let with_issuer = |mode: AuthMode, issuer: Option<&str>| ProviderAuth {
            oidc_issuer: issuer.map(str::to_owned),
            ..make_auth(mode)
        };

        // Session logins qualify regardless of issuer (incl. enterprise OIDC).
        assert!(with_issuer(AuthMode::Oidc, None).is_session_auth());
        assert!(with_issuer(AuthMode::Oidc, Some("https://idp.acme.example")).is_session_auth());

        // Explicit external providers qualify regardless of issuer metadata.
        assert!(with_issuer(AuthMode::External, Some(TEST_OAUTH2_ISSUER)).is_session_auth());
        assert!(with_issuer(AuthMode::External, None).is_session_auth());
        assert!(
            with_issuer(AuthMode::External, Some("https://idp.acme.example")).is_session_auth()
        );

        // Plain API keys never do.
        assert!(!with_issuer(AuthMode::ApiKey, Some(TEST_OAUTH2_ISSUER)).is_session_auth());
    }

    #[test]
    fn lookup_auth_returns_oidc_token() {
        let mut map = AuthStore::new();
        map.insert("scope".into(), make_auth(AuthMode::Oidc));
        assert!(lookup_auth(&map, "scope").is_some());
    }

    #[test]
    fn lookup_auth_returns_api_key_token() {
        let mut map = AuthStore::new();
        map.insert("scope".into(), make_auth(AuthMode::ApiKey));
        assert!(lookup_auth(&map, "scope").is_some());
    }
}

//! Credential dependency-inversion seam for outbound BYOK HTTP requests.

use reqwest::RequestBuilder;

use crate::visibility::HttpAuth;

/// Snapshot of the API key that will be sent on the wire.
#[derive(Clone, Debug, Default)]
pub struct CredentialSnapshot {
    /// API key. `None` when the provider is intentionally keyless.
    pub token: Option<String>,
}

/// Source of truth for outbound auth on data-collector requests.
///
/// Supertrait of `HttpAuth` so a single provider supplies both the raw BYOK
/// value used by middleware and endpoint-specific header construction.
pub trait AuthCredentialProvider: HttpAuth + Send + Sync + 'static {
    /// Return the current credential snapshot. The token must mirror the
    /// bearer that `HttpAuth::apply` would send on the wire.
    fn snapshot(&self) -> CredentialSnapshot;
}

/// Static credential provider used by callers that already resolved BYOK.
///
/// `apply()` delegates to the underlying `HttpAuth::apply()`.
/// `bearer` is the wire bearer the inner `HttpAuth` will send in the
/// `Authorization` header. Stored alongside the inner so `snapshot().token`
/// returns the same prefix that goes out on the wire (used by
/// 401-attribution diagnostics). `None` when no bearer is configured.
pub struct StaticAuthCredentialProvider {
    inner: Box<dyn HttpAuth>,
    bearer: Option<String>,
}

impl StaticAuthCredentialProvider {
    /// Wrap `inner` so callers see it as an `AuthCredentialProvider`. Pass
    /// the bearer token that `inner.apply()` will send in the `Authorization`
    /// header so `snapshot().token` reflects the wire bearer truthfully.
    pub fn new(inner: Box<dyn HttpAuth>, bearer: Option<String>) -> Self {
        Self { inner, bearer }
    }
}

impl std::fmt::Debug for StaticAuthCredentialProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticAuthCredentialProvider")
            .field("has_bearer", &self.bearer.is_some())
            .finish()
    }
}

impl HttpAuth for StaticAuthCredentialProvider {
    fn apply(&self, builder: RequestBuilder, base_url: &str) -> RequestBuilder {
        self.inner.apply(builder, base_url)
    }
}

impl AuthCredentialProvider for StaticAuthCredentialProvider {
    fn snapshot(&self) -> CredentialSnapshot {
        CredentialSnapshot {
            token: self.bearer.clone(),
            ..Default::default()
        }
    }
}

//! HTTP client for backend CRUD operations.
use crate::auth::{ProviderAuth, ServiceAuthConfig};
use indexmap::IndexMap;
use prod_mc_cli_chat_proxy_types::SubagentBundle;
use serde::Deserialize;
use std::time::Duration;
fn add_cli_chat_proxy_headers_blocking(
    builder: reqwest::blocking::RequestBuilder,
    auth: &ProviderAuth,
    alpha_test_key: Option<&str>,
    url: &str,
) -> reqwest::blocking::RequestBuilder {
    let mut builder = builder
        .header("Authorization", format!("Bearer {}", &auth.key))
        .header(
            "X-Grow-Token-Auth",
            ServiceAuthConfig::default().token_header,
        )
        .header("x-userid", &auth.user_id)
        .header("x-grow-client-version", grow_version::VERSION);
    if let Some(email) = &auth.email {
        builder = builder.header("x-email", email);
    }
    let _ = (alpha_test_key, url);
    builder
        .header(
            "x-grow-client-identifier",
            crate::http::process_client_identifier(),
        )
        .header(
            crate::http::CLIENT_MODE_HEADER,
            crate::http::process_client_mode(),
        )
}
async fn parse_json_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, BackendError> {
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).map_err(BackendError::from)
}
async fn add_bundle_fetch_headers(
    builder: reqwest::RequestBuilder,
    auth_manager: Option<&std::sync::Arc<crate::auth::AuthManager>>,
    deployment_key: Option<&str>,
    alpha_test_key: Option<&str>,
    url: &str,
) -> reqwest::RequestBuilder {
    let resolved_auth = match auth_manager {
        Some(am) => am.auth().await.ok(),
        None => None,
    };
    let mut credentials = crate::util::provider_auth_credentials::ProviderAuthCredentials::new(
        resolved_auth.as_ref().map(|auth| auth.key.clone()),
    );
    credentials.deployment_key = deployment_key.map(str::to_owned);
    credentials.alpha_test_key = alpha_test_key.map(str::to_owned);
    let mut builder = credentials
        .apply(builder, url)
        .header("x-grow-client-version", grow_version::VERSION);
    if deployment_key.is_none()
        && let Some(auth) = &resolved_auth
    {
        builder = builder.header("x-userid", &auth.user_id);
        if let Some(email) = &auth.email {
            builder = builder.header("x-email", email);
        }
    }
    builder = builder
        .header(
            "x-grow-client-identifier",
            crate::http::process_client_identifier(),
        )
        .header(
            crate::http::CLIENT_MODE_HEADER,
            crate::http::process_client_mode(),
        );
    builder
}
/// Fetch the bundled subagent cache payload from cli-chat-proxy `GET /v1/subagents/bundle`.
///
/// Uses the shell's standard proxy-backed auth model: deployment key auth takes
/// precedence when configured; otherwise user-session token auth is used.
pub async fn fetch_subagent_bundle(
    cli_chat_proxy_base_url: &str,
    auth_manager: Option<&std::sync::Arc<crate::auth::AuthManager>>,
    deployment_key: Option<&str>,
    alpha_test_key: Option<&str>,
) -> Result<SubagentBundle, BackendError> {
    let url = format!("{}/subagents/bundle", cli_chat_proxy_base_url);
    let response = add_bundle_fetch_headers(
        crate::http::shared_client()
            .get(&url)
            .timeout(std::time::Duration::from_secs(10)),
        auth_manager,
        deployment_key,
        alpha_test_key,
        &url,
    )
    .await
    .send()
    .await?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(BackendError::RequestFailed { status, body });
    }
    let bundle: SubagentBundle = parse_json_response(response).await?;
    tracing::debug!(
        version = %bundle.version,
        personas = bundle.personas.len(),
        roles = bundle.roles.len(),
        agents = bundle.agents.len(),
        "Fetched subagent bundle from cli-chat-proxy"
    );
    Ok(bundle)
}
/// The result of fetching a bundle: either raw tar.gz bytes from the new
/// archive endpoint, or a parsed JSON bundle from the legacy endpoint.
#[derive(Debug)]
pub enum FetchedBundle {
    Archive(Vec<u8>),
    Legacy(SubagentBundle),
}
/// Fetch a bundle, trying the archive endpoint first and falling back to
/// legacy JSON on any non-success HTTP status.
pub async fn fetch_bundle(
    cli_chat_proxy_base_url: &str,
    auth_manager: Option<&std::sync::Arc<crate::auth::AuthManager>>,
    deployment_key: Option<&str>,
    alpha_test_key: Option<&str>,
) -> Result<FetchedBundle, BackendError> {
    fetch_bundle_inner(
        cli_chat_proxy_base_url,
        auth_manager,
        deployment_key,
        alpha_test_key,
    )
    .await
}
async fn fetch_bundle_inner(
    cli_chat_proxy_base_url: &str,
    auth_manager: Option<&std::sync::Arc<crate::auth::AuthManager>>,
    deployment_key: Option<&str>,
    alpha_test_key: Option<&str>,
) -> Result<FetchedBundle, BackendError> {
    let archive_url = format!("{}/bundle/archive", cli_chat_proxy_base_url);
    let raw_client = crate::http::shared_client();
    let client: reqwest_middleware::ClientWithMiddleware = if let Some(am) = auth_manager {
        let provider: std::sync::Arc<dyn grow_auth::AuthCredentialProvider> = std::sync::Arc::new(
            crate::auth::credential_provider::ShellAuthCredentialProvider::new(
                am.clone(),
                deployment_key.map(str::to_owned),
                alpha_test_key.map(str::to_owned),
            ),
        );
        crate::http::with_auth_retry(raw_client, provider)
    } else {
        reqwest_middleware::ClientBuilder::new(raw_client).build()
    };
    let mut request = client
        .get(&archive_url)
        .timeout(std::time::Duration::from_secs(30))
        .header("x-grow-client-version", grow_version::VERSION)
        .header(
            crate::http::CLIENT_MODE_HEADER,
            crate::http::process_client_mode(),
        );
    if deployment_key.is_none()
        && let Some(am) = auth_manager
        && let Some(auth) = am.current()
    {
        request = request.header("x-userid", &auth.user_id);
        if let Some(ref email) = auth.email {
            request = request.header("x-email", email);
        }
    }
    let archive_response = request.send().await.map_err(|e| match e {
        reqwest_middleware::Error::Reqwest(e) => BackendError::Network(e),
        reqwest_middleware::Error::Middleware(e) => BackendError::Auth(e.to_string()),
    })?;
    if archive_response.status().is_success() {
        let bytes = archive_response.bytes().await?;
        return Ok(FetchedBundle::Archive(bytes.to_vec()));
    }
    if archive_response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let body = archive_response.text().await.unwrap_or_default();
        return Err(BackendError::RequestFailed { status: 401, body });
    }
    tracing::debug!(
        status = %archive_response.status(),
        "archive endpoint unavailable, falling back to legacy JSON"
    );
    let bundle = fetch_subagent_bundle(
        cli_chat_proxy_base_url,
        auth_manager,
        deployment_key,
        alpha_test_key,
    )
    .await?;
    Ok(FetchedBundle::Legacy(bundle))
}
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Request failed: {status} - {body}")]
    RequestFailed { status: u16, body: String },
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Auth error: {0}")]
    Auth(String),
}
#[derive(Deserialize)]
struct LoginConfigResponse {
    /// Tri-state: `Some` forces a transport; `None`/absent → client default.
    #[serde(default)]
    device_flow: Option<bool>,
}
/// Fetch `grow_build_login_device_flow` from cli-chat-proxy `GET /v1/login-config`.
///
/// Best-effort: any error or unset flag returns `None` so the caller keeps the
/// loopback default. Caps at 1.5s with no retries since it's on the login path.
pub async fn fetch_login_device_flow(cli_chat_proxy_base_url: &str) -> Option<bool> {
    let client = crate::http::shared_client();
    let url = format!("{}/login-config", cli_chat_proxy_base_url);
    let response = client
        .get(&url)
        .timeout(std::time::Duration::from_millis(1500))
        .send()
        .await;
    let resp = match response {
        Ok(resp) if resp.status().is_success() => resp,
        Ok(resp) => {
            tracing::debug!(status = resp.status().as_u16(), "login-config fetch failed");
            return None;
        }
        Err(e) => {
            tracing::debug!("login-config fetch error: {e}");
            return None;
        }
    };
    match resp.json::<LoginConfigResponse>().await {
        Ok(cfg) => {
            tracing::debug!(device_flow = ?cfg.device_flow, "Fetched remote login-config");
            cfg.device_flow
        }
        Err(e) => {
            tracing::debug!("Failed to parse login-config response: {e}");
            None
        }
    }
}
/// Default context window (256k) when the remote endpoint doesn't provide one.
pub(crate) const DEFAULT_CONTEXT_WINDOW: u64 = 256_000;
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        extract::{Path, State},
        http::{HeaderMap, StatusCode},
        routing::get,
    };
    use std::sync::{Arc, Mutex};
    #[test]
    fn login_config_response_parses_tristate() {
        let parse = |s: &str| {
            serde_json::from_str::<LoginConfigResponse>(s)
                .unwrap()
                .device_flow
        };
        assert_eq!(parse(r#"{"device_flow": true}"#), Some(true));
        assert_eq!(parse(r#"{"device_flow": false}"#), Some(false));
        assert_eq!(parse(r#"{"device_flow": null}"#), None);
        assert_eq!(parse("{}"), None, "absent flag must parse as unset");
    }
    fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    }
    #[derive(Debug, Default, Clone)]
    struct LoginConfigHeaders {
        authorization: Option<String>,
        user_id: Option<String>,
        email: Option<String>,
    }
    #[derive(Clone)]
    struct LoginConfigServerState {
        status_code: StatusCode,
        body: String,
        seen: Arc<Mutex<Vec<LoginConfigHeaders>>>,
    }
    /// Mock cli-chat-proxy serving `GET /v1/login-config` with a fixed status +
    /// raw body, recording the request headers it saw.
    async fn start_login_config_server(
        status_code: StatusCode,
        body: String,
    ) -> (
        String,
        Arc<Mutex<Vec<LoginConfigHeaders>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let state = LoginConfigServerState {
            status_code,
            body,
            seen: seen.clone(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let app = Router::new()
            .route(
                "/v1/login-config",
                get(
                    |State(state): State<LoginConfigServerState>, headers: HeaderMap| async move {
                        state.seen.lock().unwrap().push(LoginConfigHeaders {
                            authorization: header_str(&headers, "authorization"),
                            user_id: header_str(&headers, "x-userid"),
                            email: header_str(&headers, "x-email"),
                        });
                        (state.status_code, state.body)
                    },
                ),
            )
            .with_state(state);
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("{base}/v1"), seen, handle)
    }
    #[tokio::test]
    async fn fetch_login_device_flow_parses_2xx_bodies() {
        for (body, expected) in [
            (r#"{"device_flow": true}"#, Some(true)),
            (r#"{"device_flow": false}"#, Some(false)),
            (r#"{"device_flow": null}"#, None),
            (r#"{}"#, None),
            (r#"{"other": 1}"#, None),
        ] {
            let (base, _seen, server) =
                start_login_config_server(StatusCode::OK, body.to_string()).await;
            let got = fetch_login_device_flow(&base).await;
            server.abort();
            assert_eq!(got, expected, "body {body:?}");
        }
    }
    #[tokio::test]
    async fn fetch_login_device_flow_errors_return_none() {
        for (status, body) in [
            (StatusCode::NOT_FOUND, r#"{"device_flow": true}"#),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"device_flow": true}"#,
            ),
            (StatusCode::OK, "not json"),
        ] {
            let (base, _seen, server) = start_login_config_server(status, body.to_string()).await;
            let got = fetch_login_device_flow(&base).await;
            server.abort();
            assert_eq!(got, None, "status {status}, body {body:?}");
        }
    }
    #[tokio::test]
    async fn fetch_login_device_flow_sends_only_unauthenticated_headers() {
        let (base, seen, server) =
            start_login_config_server(StatusCode::OK, r#"{"device_flow": true}"#.to_string()).await;
        let got = fetch_login_device_flow(&base).await;
        server.abort();
        assert_eq!(got, Some(true));
        let seen = seen.lock().unwrap();
        let h = seen
            .last()
            .expect("server should have received one request");
        assert_eq!(h.authorization, None, "must not send Authorization");
        assert_eq!(h.user_id, None, "must not send x-userid");
        assert_eq!(h.email, None, "must not send x-email");
    }
    #[derive(Debug, Default, Clone)]
    struct SeenHeaders {
        authorization: Option<String>,
        token_auth: Option<String>,
        user_id: Option<String>,
        email: Option<String>,
        alpha_test_key: Option<String>,
        client_version: Option<String>,
    }
    #[derive(Clone)]
    struct BundleServerState {
        body: serde_json::Value,
        status_code: StatusCode,
        seen_headers: Arc<Mutex<Vec<SeenHeaders>>>,
    }
    async fn start_bundle_server(
        status_code: StatusCode,
        body: serde_json::Value,
    ) -> (
        String,
        Arc<Mutex<Vec<SeenHeaders>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let seen_headers = Arc::new(Mutex::new(Vec::new()));
        let state = BundleServerState {
            body,
            status_code,
            seen_headers: seen_headers.clone(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let app = Router::new()
            .route(
                "/v1/subagents/bundle",
                get(
                    |State(state): State<BundleServerState>, headers: HeaderMap| async move {
                        state.seen_headers.lock().unwrap().push(SeenHeaders {
                            authorization: headers
                                .get("authorization")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned),
                            token_auth: headers
                                .get("x-grow-token-auth")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned),
                            user_id: headers
                                .get("x-userid")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned),
                            email: headers
                                .get("x-email")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned),
                            alpha_test_key: {
                                let _ = &headers;
                                None
                            },
                            client_version: headers
                                .get("x-grow-client-version")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned),
                        });
                        (state.status_code, axum::Json(state.body))
                    },
                ),
            )
            .route(
                "/forward/{tail}",
                get(
                    |Path(_tail): Path<String>,
                     State(state): State<BundleServerState>,
                     headers: HeaderMap| async move {
                        state.seen_headers.lock().unwrap().push(SeenHeaders {
                            authorization: headers
                                .get("authorization")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned),
                            token_auth: headers
                                .get("x-grow-token-auth")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned),
                            user_id: headers
                                .get("x-userid")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned),
                            email: headers
                                .get("x-email")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned),
                            alpha_test_key: {
                                let _ = &headers;
                                None
                            },
                            client_version: headers
                                .get("x-grow-client-version")
                                .and_then(|v| v.to_str().ok())
                                .map(str::to_owned),
                        });
                        (state.status_code, axum::Json(state.body))
                    },
                ),
            )
            .with_state(state);
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("{base}/v1"), seen_headers, handle)
    }
    fn test_auth() -> ProviderAuth {
        ProviderAuth {
            key: "token".to_string(),
            auth_mode: crate::auth::AuthMode::Oidc,
            create_time: chrono::Utc::now(),
            user_id: "user-1".to_string(),
            email: Some("test@example.com".to_string()),
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
            expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
            oidc_issuer: None,
            oidc_client_id: None,
        }
    }
    fn test_auth_manager() -> Arc<crate::auth::AuthManager> {
        let dir = tempfile::tempdir().unwrap();
        let mgr =
            crate::auth::AuthManager::new(dir.path(), crate::auth::ServiceAuthConfig::default());
        mgr.hot_swap(test_auth());
        std::mem::forget(dir);
        Arc::new(mgr)
    }
    #[tokio::test(flavor = "current_thread")]
    async fn fetch_subagent_bundle_success() {
        let body = serde_json::json!({
            "version": "bundle-v1",
            "personas": {"researcher": "persona"},
            "roles": {"reviewer": "role"},
            "agents": {"default": "agent"}
        });
        let (proxy_base_url, seen_headers, server) =
            start_bundle_server(axum::http::StatusCode::OK, body).await;
        let am = test_auth_manager();
        let bundle = fetch_subagent_bundle(&proxy_base_url, Some(&am), None, None)
            .await
            .unwrap();
        assert_eq!(bundle.version, "bundle-v1");
        assert_eq!(
            bundle.personas.get("researcher"),
            Some(&"persona".to_string())
        );
        assert_eq!(bundle.roles.get("reviewer"), Some(&"role".to_string()));
        assert_eq!(bundle.agents.get("default"), Some(&"agent".to_string()));
        let headers = seen_headers.lock().unwrap();
        let headers = headers.last().unwrap();
        assert_eq!(headers.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(headers.token_auth.as_deref(), Some("grow-cli"));
        assert_eq!(headers.user_id.as_deref(), Some("user-1"));
        assert_eq!(headers.email.as_deref(), Some("test@example.com"));
        assert_eq!(headers.alpha_test_key, None);
        assert!(headers.client_version.is_some());
        server.abort();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn fetch_subagent_bundle_uses_deployment_key_without_user_headers() {
        let body = serde_json::json!({
            "version": "bundle-v1",
            "personas": {},
            "roles": {},
            "agents": {}
        });
        let (proxy_base_url, seen_headers, server) =
            start_bundle_server(axum::http::StatusCode::OK, body).await;
        let am = test_auth_manager();
        let bundle = fetch_subagent_bundle(&proxy_base_url, Some(&am), Some("deploy-key"), None)
            .await
            .unwrap();
        assert_eq!(bundle.version, "bundle-v1");
        let headers = seen_headers.lock().unwrap();
        let headers = headers.last().unwrap();
        assert_eq!(headers.authorization.as_deref(), Some("Bearer deploy-key"));
        assert_eq!(headers.token_auth, None);
        assert_eq!(headers.user_id, None);
        assert_eq!(headers.email, None);
        server.abort();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn fetch_subagent_bundle_http_failure() {
        let (proxy_base_url, _seen_headers, server) = start_bundle_server(
            axum::http::StatusCode::UNAUTHORIZED,
            serde_json::json!({"error": "unauthorized"}),
        )
        .await;
        let am = test_auth_manager();
        let error = fetch_subagent_bundle(&proxy_base_url, Some(&am), None, None)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            BackendError::RequestFailed { status: 401, .. }
        ));
        server.abort();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn fetch_subagent_bundle_parse_failure() {
        let (proxy_base_url, _seen_headers, server) = start_bundle_server(
            axum::http::StatusCode::OK,
            serde_json::json!({"version": 42}),
        )
        .await;
        let am = test_auth_manager();
        let error = fetch_subagent_bundle(&proxy_base_url, Some(&am), None, None)
            .await
            .unwrap_err();
        assert!(matches!(error, BackendError::Serialization(_)));
        server.abort();
    }
    /// REGRESSION: `grow setup` must send the deployment key to
    /// the proxy, never the inference endpoint.
    #[test]
    #[serial_test::serial]
    fn deployment_config_url_uses_cli_chat_proxy_when_not_overridden() {
        use crate::agent::config::EndpointsConfig;
        for k in [
            "GROW_CLI_CHAT_PROXY_BASE_URL",
            "GROW_MANAGED_CONFIG_URL",
            "GROW_INFERENCE_BASE_URL",
        ] {
            unsafe { std::env::remove_var(k) };
        }
        unsafe { std::env::set_var("GROW_DEPLOYMENT_KEY", "provider-token-ENTERPRISE") };
        let managed: toml::Value = toml::from_str(
            r#"[endpoints]
            deployment_key = "provider-token-ENTERPRISE"
            inference_base_url = "https://inference.acme-corp.example/provider/v1""#,
        )
        .unwrap();
        let url = EndpointsConfig::from_config_value(&managed).resolve_managed_config_url();
        assert_eq!(url, None);
        let pinned: toml::Value = toml::from_str(
            r#"[endpoints]
            inference_base_url = "https://inference.acme-corp.example/provider/v1"
            cli_chat_proxy_base_url = "https://proxy.acme-corp.example/v1""#,
        )
        .unwrap();
        assert_eq!(
            EndpointsConfig::from_config_value(&pinned)
                .resolve_managed_config_url()
                .as_deref(),
            Some("https://proxy.acme-corp.example/v1/deployment/config")
        );
        unsafe { std::env::remove_var("GROW_DEPLOYMENT_KEY") };
    }
    #[derive(Clone)]
    struct DualBundleServerState {
        archive_status: StatusCode,
        archive_bytes: Vec<u8>,
        legacy_status: StatusCode,
        legacy_body: serde_json::Value,
    }
    async fn start_dual_bundle_server(
        state: DualBundleServerState,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let app = Router::new()
            .route(
                "/v1/bundle/archive",
                get(|State(state): State<DualBundleServerState>| async move {
                    (state.archive_status, state.archive_bytes)
                }),
            )
            .route(
                "/v1/subagents/bundle",
                get(|State(state): State<DualBundleServerState>| async move {
                    (state.legacy_status, axum::Json(state.legacy_body))
                }),
            )
            .with_state(state);
        let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("{base}/v1"), handle)
    }
    #[tokio::test(flavor = "current_thread")]
    async fn fetch_bundle_returns_archive_on_success() {
        let archive_bytes = b"fake-tar-gz-bytes".to_vec();
        let (proxy_base_url, server) = start_dual_bundle_server(DualBundleServerState {
            archive_status: StatusCode::OK,
            archive_bytes: archive_bytes.clone(),
            legacy_status: StatusCode::OK,
            legacy_body: serde_json::json!({
                "version": "v1", "personas": {}, "roles": {}, "agents": {}
            }),
        })
        .await;
        let am = test_auth_manager();
        let result = fetch_bundle(&proxy_base_url, Some(&am), None, None)
            .await
            .unwrap();
        match result {
            FetchedBundle::Archive(bytes) => assert_eq!(bytes, archive_bytes),
            FetchedBundle::Legacy(_) => panic!("expected Archive variant"),
        }
        server.abort();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn fetch_bundle_falls_back_on_archive_404() {
        let (proxy_base_url, server) = start_dual_bundle_server(DualBundleServerState {
            archive_status: StatusCode::NOT_FOUND,
            archive_bytes: Vec::new(),
            legacy_status: StatusCode::OK,
            legacy_body: serde_json::json!({
                "version": "v1",
                "personas": {"r": "p"},
                "roles": {},
                "agents": {}
            }),
        })
        .await;
        let am = test_auth_manager();
        let result = fetch_bundle(&proxy_base_url, Some(&am), None, None)
            .await
            .unwrap();
        match result {
            FetchedBundle::Legacy(bundle) => {
                assert_eq!(bundle.version, "v1");
                assert_eq!(bundle.personas.get("r"), Some(&"p".to_string()));
            }
            FetchedBundle::Archive(_) => panic!("expected Legacy variant"),
        }
        server.abort();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn fetch_bundle_falls_back_on_archive_503() {
        let (proxy_base_url, server) = start_dual_bundle_server(DualBundleServerState {
            archive_status: StatusCode::SERVICE_UNAVAILABLE,
            archive_bytes: Vec::new(),
            legacy_status: StatusCode::OK,
            legacy_body: serde_json::json!({
                "version": "v1", "personas": {}, "roles": {}, "agents": {}
            }),
        })
        .await;
        let am = test_auth_manager();
        let result = fetch_bundle(&proxy_base_url, Some(&am), None, None)
            .await
            .unwrap();
        match &result {
            FetchedBundle::Legacy(bundle) => assert_eq!(bundle.version, "v1"),
            FetchedBundle::Archive(_) => panic!("expected Legacy variant"),
        }
        server.abort();
    }
    #[tokio::test(flavor = "current_thread")]
    async fn fetch_bundle_propagates_legacy_error_after_fallback() {
        let (proxy_base_url, server) = start_dual_bundle_server(DualBundleServerState {
            archive_status: StatusCode::NOT_FOUND,
            archive_bytes: Vec::new(),
            legacy_status: StatusCode::UNAUTHORIZED,
            legacy_body: serde_json::json!({"error": "unauthorized"}),
        })
        .await;
        let am = test_auth_manager();
        let error = fetch_bundle(&proxy_base_url, Some(&am), None, None)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            BackendError::RequestFailed { status: 401, .. }
        ));
        server.abort();
    }
}

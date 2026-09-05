//! Embedding provider abstraction for memory vector search.
//!
//! Defines the `EmbeddingProvider` trait and an API-based implementation
//! that calls an OpenAI-compatible embeddings API endpoint.
//!
//! Embeddings are cached in the sqlite-vec `chunks_vec` table — the vec0
//! virtual table IS the cache. No separate cache needed.

use async_trait::async_trait;
use std::sync::Arc;
use std::sync::OnceLock;

/// Maximum retry attempts for transient API errors (429, 5xx).
const MAX_RETRIES: usize = 3;
/// Initial backoff delay in milliseconds (doubles on each retry: 1s, 2s, 4s).
const INITIAL_BACKOFF_MS: u64 = 1000;
/// Process-owned service endpoint whose live credential may be reused for
/// embeddings. The endpoint is resolved once when the capability is minted;
/// later environment changes cannot retarget an existing capability.
const LIVE_SERVICE_BASE_URL_ENV: &str = "GROW_CLI_CHAT_PROXY_BASE_URL";

/// Trait for generating text embeddings.
///
/// Implementations must be `Send + Sync` so they can be used in `Send`
/// futures (e.g., inside `tokio::spawn`). The `embed_batch` method is
/// async to support API-based providers.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a batch of texts, returning one vector per input text.
    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>>;

    /// The model name used for embeddings.
    fn model_name(&self) -> &str;

    /// The dimensionality of the embedding vectors.
    fn dimensions(&self) -> usize;
}

/// API-based embedding provider using an OpenAI-compatible embeddings endpoint.
pub struct ApiEmbeddingProvider {
    request_url: reqwest::Url,
    model: String,
    dimensions: usize,
    client: reqwest_middleware::ClientWithMiddleware,
    max_batch_size: usize,
}

impl ApiEmbeddingProvider {
    fn new(
        request_url: reqwest::Url,
        model: String,
        dimensions: usize,
        client: reqwest_middleware::ClientWithMiddleware,
    ) -> Self {
        Self {
            request_url,
            model,
            dimensions,
            client,
            max_batch_size: 32,
        }
    }

    fn from_endpoint(
        config: &config_types::MemoryEmbeddingConfig,
        request_url: reqwest::Url,
        client: reqwest_middleware::ClientWithMiddleware,
    ) -> Option<Self> {
        let model = config.model.clone().filter(|m| !m.is_empty())?;
        Some(Self::new(request_url, model, config.dimensions, client))
    }

    #[cfg(test)]
    fn request_url_for_test(&self) -> &reqwest::Url {
        &self.request_url
    }
}

/// Endpoint and credential are one capability: neither can be copied out or
/// replaced independently. Every live/static/background embedding request is
/// constructed from this type.
#[derive(Clone)]
pub struct EmbeddingEndpoint {
    base_url: reqwest::Url,
    request_url: reqwest::Url,
    credential: EmbeddingCredential,
}

#[derive(Clone)]
enum EmbeddingCredential {
    Auth(Arc<dyn auth::AuthCredentialProvider>),
    DynamicApiKey(tools::types::SharedApiKeyProvider),
    StaticApiKey(Arc<str>),
}

impl std::fmt::Debug for EmbeddingEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match &self.credential {
            EmbeddingCredential::Auth(_) => "auth",
            EmbeddingCredential::DynamicApiKey(_) => "dynamic_api_key",
            EmbeddingCredential::StaticApiKey(_) => "static_api_key",
        };
        f.debug_struct("EmbeddingEndpoint")
            .field("base_url", &self.base_url)
            .field("credential_kind", &kind)
            .finish()
    }
}

impl EmbeddingEndpoint {
    /// Credential-free cache identity for the exact endpoint and vector model.
    pub fn cache_identity(&self, config: &config_types::MemoryEmbeddingConfig) -> Option<String> {
        let model = config.model.as_deref().filter(|model| !model.is_empty())?;
        Some(serde_json::json!([self.request_url.as_str(), model, config.dimensions]).to_string())
    }

    /// Bind a static API key to exactly one endpoint.
    pub fn from_static(endpoint: &str, api_key: String) -> Option<Self> {
        if api_key.trim().is_empty() {
            return None;
        }
        Self::new(endpoint, EmbeddingCredential::StaticApiKey(api_key.into()))
    }

    /// Bind live provider credentials to the exact process-owned service
    /// endpoint. Callers cannot supply their own trust predicate: the endpoint
    /// authority and credential are resolved into this opaque value together.
    pub fn from_live(
        endpoint: &str,
        auth_credentials: Option<Arc<dyn auth::AuthCredentialProvider>>,
        api_key_provider: Option<tools::types::SharedApiKeyProvider>,
    ) -> Option<Self> {
        let trusted_endpoint = std::env::var(LIVE_SERVICE_BASE_URL_ENV).ok()?;
        Self::bind_live(
            endpoint,
            &trusted_endpoint,
            auth_credentials,
            api_key_provider,
        )
    }

    fn bind_live(
        endpoint: &str,
        trusted_endpoint: &str,
        auth_credentials: Option<Arc<dyn auth::AuthCredentialProvider>>,
        api_key_provider: Option<tools::types::SharedApiKeyProvider>,
    ) -> Option<Self> {
        if !live_endpoint_matches(endpoint, trusted_endpoint) {
            if auth_credentials.is_some() || api_key_provider.is_some() {
                tracing::info!(
                    target: diagnostics::memory_log::TARGET,
                    endpoint,
                    "memory embeddings: live credentials withheld for non-first-party endpoint"
                );
            }
            return None;
        }
        let credential = auth_credentials
            .map(EmbeddingCredential::Auth)
            .or_else(|| api_key_provider.map(EmbeddingCredential::DynamicApiKey))?;
        Self::new(endpoint, credential)
    }

    #[cfg(test)]
    pub(crate) fn from_live_for_trusted_endpoint(
        endpoint: &str,
        trusted_endpoint: &str,
        auth_credentials: Option<Arc<dyn auth::AuthCredentialProvider>>,
        api_key_provider: Option<tools::types::SharedApiKeyProvider>,
    ) -> Option<Self> {
        Self::bind_live(
            endpoint,
            trusted_endpoint,
            auth_credentials,
            api_key_provider,
        )
    }

    fn new(endpoint: &str, credential: EmbeddingCredential) -> Option<Self> {
        let base_url = parse_base_url(endpoint)?;
        let request_url = embeddings_url(&base_url)?;
        Some(Self {
            base_url,
            request_url,
            credential,
        })
    }

    /// Exact URL match after standards-based URL parsing/normalization. This is
    /// intentionally stricter than authority-only matching: path changes select
    /// a different service boundary.
    #[cfg(test)]
    fn is_exact_endpoint(&self, candidate: &str) -> bool {
        parse_base_url(candidate).is_some_and(|candidate| candidate == self.base_url)
    }

    /// Resolve a dynamic key, if necessary, then build the only provider that
    /// can issue requests for this endpoint capability.
    pub async fn make_provider(
        &self,
        config: &config_types::MemoryEmbeddingConfig,
    ) -> Option<ApiEmbeddingProvider> {
        if config.model.as_ref().is_none_or(|model| model.is_empty()) {
            return None;
        }
        let client = match &self.credential {
            EmbeddingCredential::Auth(credentials) => build_middleware_client(credentials.clone()),
            EmbeddingCredential::DynamicApiKey(provider) => {
                let api_key = provider.current_api_key_async().await?;
                build_static_middleware_client(api_key)
            }
            EmbeddingCredential::StaticApiKey(api_key) => {
                build_static_middleware_client(api_key.to_string())
            }
        };
        ApiEmbeddingProvider::from_endpoint(config, self.request_url.clone(), client)
    }
}

fn parse_base_url(endpoint: &str) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(endpoint).ok()?;
    let host = url.host_str()?;
    let secure_transport =
        url.scheme() == "https" || (url.scheme() == "http" && is_loopback_host(host));
    if !secure_transport
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    Some(url)
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn live_endpoint_matches(candidate: &str, trusted_endpoint: &str) -> bool {
    let Some(candidate) = parse_base_url(candidate) else {
        return false;
    };
    let Some(trusted_endpoint) = parse_base_url(trusted_endpoint) else {
        return false;
    };
    candidate.scheme() == "https"
        && candidate
            .host_str()
            .is_some_and(|host| !is_loopback_host(host))
        && candidate == trusted_endpoint
}

fn embeddings_url(base_url: &reqwest::Url) -> Option<reqwest::Url> {
    let mut request_url = base_url.clone();
    {
        let mut segments = request_url.path_segments_mut().ok()?;
        segments.pop_if_empty();
        segments.push("embeddings");
    }
    Some(request_url)
}

fn embedding_http_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(grow_http::process_user_agent_string())
                .build()
                .expect("failed to build embedding HTTP client")
        })
        .clone()
}

fn build_middleware_client(
    credentials: Arc<dyn auth::AuthCredentialProvider>,
) -> reqwest_middleware::ClientWithMiddleware {
    grow_http::with_auth_header(embedding_http_client(), credentials)
}

fn build_static_middleware_client(api_key: String) -> reqwest_middleware::ClientWithMiddleware {
    let provider: Arc<dyn auth::AuthCredentialProvider> = Arc::new(
        auth::StaticAuthCredentialProvider::new(Box::new(NoopHttpAuth), Some(api_key)),
    );
    build_middleware_client(provider)
}

struct NoopHttpAuth;

impl auth::HttpAuth for NoopHttpAuth {
    fn apply(&self, builder: reqwest::RequestBuilder, _base_url: &str) -> reqwest::RequestBuilder {
        builder
    }
}

#[async_trait]
impl EmbeddingProvider for ApiEmbeddingProvider {
    #[tracing::instrument(name = "memory.embed_batch", skip_all, fields(batch_size = texts.len()))]
    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let mut all_embeddings = Vec::with_capacity(texts.len());

        // Process in batches to respect API payload limits
        for batch in texts.chunks(self.max_batch_size) {
            let input: Vec<&str> = batch.to_vec();
            let body_json = serde_json::json!({
                "model": self.model,
                "input": input,
                "dimensions": self.dimensions,
            });
            let body = serde_json::to_vec(&body_json)?;

            // Retry with exponential backoff on transient errors (429, 5xx)
            let mut last_err = String::new();
            let mut success = false;
            for attempt in 0..MAX_RETRIES {
                if attempt > 0 {
                    let delay = INITIAL_BACKOFF_MS * 2u64.pow(attempt as u32 - 1);
                    tracing::warn!(
                        attempt,
                        delay_ms = delay,
                        "retrying embedding API call after transient error"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }

                let response = self
                    .client
                    .post(self.request_url.clone())
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(body.clone())
                    .header("X-Grow-Token-Auth", "grow-cli")
                    .header("x-grow-client-version", version::VERSION)
                    .send()
                    .await;
                let response = match response {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = format!("request failed: {e}");
                        continue;
                    }
                };

                let status = response.status();
                if status.is_success() {
                    let body: serde_json::Value = response.json().await?;
                    let data = body
                        .get("data")
                        .and_then(|d| d.as_array())
                        .ok_or("embedding response missing 'data' array")?;

                    for item in data {
                        let embedding: Vec<f32> = item
                            .get("embedding")
                            .and_then(|e| e.as_array())
                            .ok_or("embedding item missing 'embedding' array")?
                            .iter()
                            .filter_map(|v| v.as_f64().map(|f| f as f32))
                            .collect();
                        all_embeddings.push(embedding);
                    }
                    success = true;
                    break;
                }

                // Retry on 429 (rate limit) or 5xx (server error)
                if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                    last_err = format!("HTTP {status}");
                    continue;
                }

                // Non-retryable error (4xx other than 429)
                return Err(format!("embedding API error {status}").into());
            }

            if !success {
                return Err(format!(
                    "embedding API failed after {MAX_RETRIES} attempts: {last_err}"
                )
                .into());
            }
        }

        Ok(all_embeddings)
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

/// A mock embedding provider for testing that returns deterministic vectors.
/// Uses blake3 hash of text → float values for reproducible results.
#[cfg(any(test, feature = "test-support"))]
pub struct MockEmbeddingProvider {
    pub dimensions: usize,
}

#[cfg(any(test, feature = "test-support"))]
#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        Ok(texts
            .iter()
            .map(|text| {
                let hash = blake3::hash(text.as_bytes());
                let bytes = hash.as_bytes();
                (0..self.dimensions)
                    .map(|i| bytes[i % 32] as f32 / 255.0)
                    .collect()
            })
            .collect())
    }

    fn model_name(&self) -> &str {
        "mock-embedding"
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn serve_one(response: String) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 16 * 1024];
            let bytes = stream.read(&mut request).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request[..bytes]).into_owned()
        });
        (format!("http://{address}"), task)
    }

    fn embedding_response(status: &str, body: &str, extra_headers: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
            body.len()
        )
    }

    fn config() -> config_types::MemoryEmbeddingConfig {
        config_types::MemoryEmbeddingConfig {
            model: Some("test-embedding-model".to_string()),
            dimensions: 2,
            ..Default::default()
        }
    }

    #[ctor::ctor]
    fn install_rustls_provider() {
        diagnostics::tls::install_ring_provider_once();
    }

    #[tokio::test]
    async fn static_key_is_bound_to_the_exact_request_url() {
        let body = r#"{"data":[{"embedding":[0.25,0.75]}]}"#;
        let (server_url, request) = serve_one(embedding_response("200 OK", body, "")).await;
        let endpoint = format!("{server_url}/v1");
        let binding = EmbeddingEndpoint::from_static(&endpoint, "static-secret".into()).unwrap();
        let provider = binding.make_provider(&config()).await.unwrap();
        assert_eq!(
            provider.request_url_for_test().as_str(),
            format!("{endpoint}/embeddings")
        );
        assert_eq!(provider.embed_batch(&["hello"]).await.unwrap().len(), 1);
        let request = request.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("post /v1/embeddings "));
        assert!(request.contains("authorization: bearer static-secret"));
        assert!(request.contains("x-grow-token-auth: grow-cli"));
    }

    #[test]
    fn cache_identity_binds_endpoint_model_and_dimensions_without_credentials() {
        let endpoint =
            EmbeddingEndpoint::from_static("https://one.example/v1", "secret-one".into()).unwrap();
        let rotated =
            EmbeddingEndpoint::from_static("https://one.example/v1/", "secret-two".into()).unwrap();
        let other =
            EmbeddingEndpoint::from_static("https://two.example/v1", "secret-one".into()).unwrap();
        let mut config = config_types::MemoryEmbeddingConfig {
            model: Some("model-a".into()),
            dimensions: 4,
            ..Default::default()
        };
        let identity = endpoint.cache_identity(&config).unwrap();
        assert!(!identity.contains("secret"));
        assert_eq!(Some(identity.clone()), rotated.cache_identity(&config));
        assert_ne!(Some(identity.clone()), other.cache_identity(&config));
        config.model = Some("model-b".into());
        assert_ne!(Some(identity.clone()), endpoint.cache_identity(&config));
        config.model = Some("model-a".into());
        config.dimensions = 8;
        assert_ne!(Some(identity), endpoint.cache_identity(&config));
    }

    #[test]
    fn endpoint_matching_is_exact_and_invalid_urls_fail_closed() {
        let endpoint =
            EmbeddingEndpoint::from_static("https://API.example.com/v1", "static-secret".into())
                .unwrap();
        assert!(endpoint.is_exact_endpoint("https://api.example.com/v1"));
        assert!(!endpoint.is_exact_endpoint("https://api.example.com/v1/"));
        assert!(!endpoint.is_exact_endpoint("https://api.example.com/v2"));
        assert!(!endpoint.is_exact_endpoint("https://other.example.com/v1"));
        assert!(!endpoint.is_exact_endpoint("https://api.example.com/v1?q=1"));
        assert!(EmbeddingEndpoint::from_static("not-a-url", "secret".into()).is_none());
        assert!(EmbeddingEndpoint::from_static("file:///tmp/socket", "secret".into()).is_none());
        assert!(
            EmbeddingEndpoint::from_static("http://api.example.com/v1", "secret".into()).is_none()
        );
        assert!(EmbeddingEndpoint::from_static("http://localhost/v1", "secret".into()).is_some());
        assert!(
            EmbeddingEndpoint::from_static("https://api.example.com/v1", String::new()).is_none()
        );
        assert!(
            EmbeddingEndpoint::from_static("https://api.example.com/v1", "  ".into()).is_none()
        );
    }

    #[tokio::test]
    async fn background_provider_resolves_live_key_from_the_same_endpoint_capability() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct LiveKey(Arc<AtomicUsize>);
        impl tools::types::ApiKeyProvider for LiveKey {
            fn current_api_key(&self) -> Option<String> {
                panic!("embedding capability must resolve live keys asynchronously")
            }

            fn current_api_key_async(
                &self,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + '_>>
            {
                let calls = self.0.clone();
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Some("live-secret".to_string())
                })
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let key: tools::types::SharedApiKeyProvider = Arc::new(LiveKey(calls.clone()));
        let binding = EmbeddingEndpoint::from_live_for_trusted_endpoint(
            "https://service.example/v1",
            "https://service.example/v1",
            None,
            Some(key),
        )
        .unwrap();

        // Background reindex receives a clone of this same inseparable
        // endpoint+credential capability; it cannot supply or replace either
        // component independently.
        let background_binding = binding.clone();
        let provider = background_binding.make_provider(&config()).await.unwrap();
        assert_eq!(
            provider.request_url_for_test().as_str(),
            "https://service.example/v1/embeddings"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn untrusted_or_credentialless_live_endpoint_is_none() {
        struct Key;
        impl tools::types::ApiKeyProvider for Key {
            fn current_api_key(&self) -> Option<String> {
                Some("must-not-resolve".to_string())
            }
        }
        let key = || Arc::new(Key) as tools::types::SharedApiKeyProvider;
        assert!(
            EmbeddingEndpoint::from_live_for_trusted_endpoint(
                "https://api.example.com/v1",
                "https://other.example.com/v1",
                None,
                Some(key())
            )
            .is_none()
        );
        assert!(
            EmbeddingEndpoint::from_live_for_trusted_endpoint(
                "https://api.example.com/v1",
                "https://api.example.com/v1",
                None,
                None,
            )
            .is_none()
        );
    }

    #[test]
    fn live_endpoint_authority_requires_one_exact_non_loopback_https_url() {
        struct Key;
        impl tools::types::ApiKeyProvider for Key {
            fn current_api_key(&self) -> Option<String> {
                Some("secret".to_string())
            }
        }
        let key = || Arc::new(Key) as tools::types::SharedApiKeyProvider;
        let trusted = "https://API.example.com/v1";

        assert!(
            EmbeddingEndpoint::from_live_for_trusted_endpoint(
                "https://api.example.com/v1",
                trusted,
                None,
                Some(key()),
            )
            .is_some()
        );
        for candidate in [
            "https://api.example.com/v1/",
            "https://api.example.com/v1/child",
            "https://api.example.com/v2",
            "https://other.example.com/v1",
            "http://api.example.com/v1",
            "https://localhost/v1",
        ] {
            assert!(
                EmbeddingEndpoint::from_live_for_trusted_endpoint(
                    candidate,
                    trusted,
                    None,
                    Some(key()),
                )
                .is_none(),
                "unexpected live authority for {candidate}",
            );
        }
    }

    #[tokio::test]
    async fn embedding_client_never_follows_redirects() {
        let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let location = format!("http://{}/stolen", target.local_addr().unwrap());
        let (source_url, redirect) = serve_one(embedding_response(
            "302 Found",
            "",
            &format!("Location: {location}\r\n"),
        ))
        .await;
        let endpoint = format!("{source_url}/v1");
        let binding = EmbeddingEndpoint::from_static(&endpoint, "redirect-secret".into()).unwrap();
        let provider = binding.make_provider(&config()).await.unwrap();
        let error = provider
            .embed_batch(&["do not redirect"])
            .await
            .unwrap_err();
        assert!(error.to_string().contains("302"));
        let request = redirect.await.unwrap().to_ascii_lowercase();
        assert!(request.contains("authorization: bearer redirect-secret"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), target.accept())
                .await
                .is_err(),
            "credential-bearing client must not request the redirect target"
        );
    }

    #[tokio::test]
    async fn embedding_errors_do_not_retain_untrusted_response_bodies() {
        let secret = "server-secret-that-must-not-enter-errors";
        let (server_url, request) =
            serve_one(embedding_response("403 Forbidden", secret, "")).await;
        let endpoint = format!("{server_url}/v1");
        let binding = EmbeddingEndpoint::from_static(&endpoint, "request-secret".into()).unwrap();
        let provider = binding.make_provider(&config()).await.unwrap();

        let error = provider
            .embed_batch(&["request"])
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("403"));
        assert!(!error.contains("server-secret"));
        request.await.unwrap();
    }

    #[tokio::test]
    async fn test_mock_embedding_deterministic() {
        let provider = MockEmbeddingProvider { dimensions: 4 };
        let r1 = provider.embed_batch(&["hello"]).await.unwrap();
        let r2 = provider.embed_batch(&["hello"]).await.unwrap();
        assert_eq!(r1, r2);
    }

    #[tokio::test]
    async fn test_mock_embedding_different_texts() {
        let provider = MockEmbeddingProvider { dimensions: 4 };
        let results = provider.embed_batch(&["hello", "world"]).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_ne!(results[0], results[1]);
    }

    #[tokio::test]
    async fn test_mock_embedding_empty_input() {
        let provider = MockEmbeddingProvider { dimensions: 4 };
        let results = provider.embed_batch(&[]).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_mock_embedding_correct_dimensions() {
        let provider = MockEmbeddingProvider { dimensions: 128 };
        let results = provider.embed_batch(&["test"]).await.unwrap();
        assert_eq!(results[0].len(), 128);
    }
}

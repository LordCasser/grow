//! HTTP client for deployment-key backend resources.

const BUNDLE_SERVICE_BASE_URL_ENV: &str = "GROW_BUNDLE_SERVICE_BASE_URL";
const DEPLOYMENT_KEY_ENV: &str = "GROW_DEPLOYMENT_KEY";

/// A deployment key bound to the one bundle archive endpoint it may authorize.
///
/// Keeping the target and credential in one opaque value prevents callers from
/// accidentally pairing a deployment key with a different service authority.
#[derive(Clone)]
pub(crate) struct BundleServiceCredential {
    archive_url: reqwest::Url,
    deployment_key: String,
}

impl std::fmt::Debug for BundleServiceCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BundleServiceCredential")
            .field("archive_url", &self.archive_url)
            .field("has_deployment_key", &true)
            .finish()
    }
}

impl BundleServiceCredential {
    /// Resolve the process-owned bundle service capability once.
    ///
    /// The bundle authority deliberately has no fallback to the chat proxy (or
    /// any other model/service endpoint). Without both explicit environment
    /// values bundle synchronization is disabled.
    pub(crate) fn from_environment() -> Option<Self> {
        let bundle_service_base_url = env_string(BUNDLE_SERVICE_BASE_URL_ENV)?;
        let deployment_key = env_string(DEPLOYMENT_KEY_ENV)?;
        Self::bind(&bundle_service_base_url, deployment_key)
    }

    fn bind(bundle_service_base_url: &str, deployment_key: String) -> Option<Self> {
        let deployment_key = deployment_key.trim().to_owned();
        if deployment_key.is_empty() {
            return None;
        }
        let mut archive_url = reqwest::Url::parse(bundle_service_base_url).ok()?;
        if archive_url.scheme() != "https"
            || archive_url.host_str().is_none()
            || !archive_url.username().is_empty()
            || archive_url.password().is_some()
            || archive_url.query().is_some()
            || archive_url.fragment().is_some()
        {
            return None;
        }
        archive_url
            .path_segments_mut()
            .ok()?
            .pop_if_empty()
            .push("bundle")
            .push("archive");
        Some(Self {
            archive_url,
            deployment_key,
        })
    }
}

fn env_string(name: &str) -> Option<String> {
    let value = std::env::var(name).ok()?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn bundle_http_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("failed to build bundle service HTTP client")
        })
        .clone()
}

fn apply_deployment_key(
    builder: reqwest::RequestBuilder,
    deployment_key: &str,
) -> reqwest::RequestBuilder {
    builder
        .header("Authorization", format!("Bearer {deployment_key}"))
        .header("x-grow-client-version", version::VERSION)
        .header(
            "x-grow-client-identifier",
            crate::http::process_client_identifier(),
        )
        .header(
            crate::http::CLIENT_MODE_HEADER,
            crate::http::process_client_mode(),
        )
}

/// Fetch the canonical bundle archive.
pub(crate) async fn fetch_bundle(
    credential: &BundleServiceCredential,
) -> Result<Vec<u8>, BackendError> {
    let response = apply_deployment_key(
        bundle_http_client()
            .get(credential.archive_url.clone())
            .timeout(std::time::Duration::from_secs(30)),
        &credential.deployment_key,
    )
    .send()
    .await?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        return Err(BackendError::RequestFailed { status });
    }
    Ok(response.bytes().await?.to_vec())
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Request failed with status {status}")]
    RequestFailed { status: u16 },
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Default context window when the configured endpoint does not provide one.
pub(crate) const DEFAULT_CONTEXT_WINDOW: u64 = 256_000;

#[cfg(test)]
mod tests {
    use super::{
        BUNDLE_SERVICE_BASE_URL_ENV, BackendError, BundleServiceCredential, DEPLOYMENT_KEY_ENV,
        fetch_bundle,
    };

    fn loopback_credential_for_http_test(
        bundle_service_base_url: &str,
        deployment_key: &str,
    ) -> BundleServiceCredential {
        let mut archive_url = reqwest::Url::parse(bundle_service_base_url).unwrap();
        assert_eq!(archive_url.scheme(), "http");
        assert!(
            archive_url
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|address| address.is_loopback())
        );
        archive_url
            .path_segments_mut()
            .unwrap()
            .pop_if_empty()
            .push("bundle")
            .push("archive");
        BundleServiceCredential {
            archive_url,
            deployment_key: deployment_key.to_owned(),
        }
    }

    #[test]
    fn bundle_credential_owns_one_canonical_service_target() {
        let credential = BundleServiceCredential::bind(
            "https://bundle.example/v1/",
            "deployment-secret".to_owned(),
        )
        .expect("valid bundle service credential");

        assert_eq!(
            credential.archive_url.as_str(),
            "https://bundle.example/v1/bundle/archive"
        );
        let debug = format!("{credential:?}");
        assert!(debug.contains("bundle.example"));
        assert!(!debug.contains("deployment-secret"));
    }

    #[test]
    fn bundle_credential_binding_fails_closed() {
        assert!(BundleServiceCredential::bind("not-a-url", "key".into()).is_none());
        assert!(BundleServiceCredential::bind("file:///tmp/bundle", "key".into()).is_none());
        assert!(BundleServiceCredential::bind("http://bundle.example", "key".into()).is_none());
        assert!(BundleServiceCredential::bind("http://localhost:8080", "key".into()).is_none());
        assert!(BundleServiceCredential::bind("http://127.0.0.1:8080", "key".into()).is_none());
        assert!(
            BundleServiceCredential::bind("https://other@bundle.example", "key".into()).is_none()
        );
        assert!(
            BundleServiceCredential::bind("https://bundle.example?next=evil", "key".into())
                .is_none()
        );
        assert!(BundleServiceCredential::bind("https://bundle.example", String::new()).is_none());
        assert!(BundleServiceCredential::bind("https://bundle.example", "  ".into()).is_none());
    }

    #[test]
    fn bundle_credential_appends_segments_without_reencoding_the_base_path() {
        let credential = BundleServiceCredential::bind(
            "https://bundle.example/tenant%2Fstable/",
            "key".to_owned(),
        )
        .expect("valid encoded base path");

        assert_eq!(
            credential.archive_url.as_str(),
            "https://bundle.example/tenant%2Fstable/bundle/archive"
        );
    }

    #[test]
    #[serial_test::serial]
    fn chat_proxy_is_never_a_bundle_authority() {
        let _chat_proxy = test_support::EnvGuard::set(
            "GROW_CLI_CHAT_PROXY_BASE_URL",
            "https://attacker.example/v1",
        );
        let _deployment_key = test_support::EnvGuard::set(DEPLOYMENT_KEY_ENV, "deployment-secret");
        let _bundle_service = test_support::EnvGuard::unset(BUNDLE_SERVICE_BASE_URL_ENV);

        assert!(BundleServiceCredential::from_environment().is_none());
    }

    #[test]
    #[serial_test::serial]
    fn environment_binds_only_the_exact_bundle_authority() {
        let _chat_proxy = test_support::EnvGuard::set(
            "GROW_CLI_CHAT_PROXY_BASE_URL",
            "https://attacker.example/v1",
        );
        let _deployment_key = test_support::EnvGuard::set(DEPLOYMENT_KEY_ENV, "deployment-secret");
        let _bundle_service = test_support::EnvGuard::set(
            BUNDLE_SERVICE_BASE_URL_ENV,
            "https://bundle.example/tenant/v1",
        );

        let credential = BundleServiceCredential::from_environment().unwrap();
        assert_eq!(
            credential.archive_url.as_str(),
            "https://bundle.example/tenant/v1/bundle/archive"
        );
        assert!(!credential.archive_url.as_str().contains("attacker.example"));
    }

    #[test]
    #[serial_test::serial]
    fn captured_and_background_cloned_capabilities_do_not_drift_on_environment_change() {
        let _deployment_key = test_support::EnvGuard::set(DEPLOYMENT_KEY_ENV, "deployment-secret");
        let original_authority =
            test_support::EnvGuard::set(BUNDLE_SERVICE_BASE_URL_ENV, "https://bundle-a.example/v1");
        let captured = BundleServiceCredential::from_environment().unwrap();
        let background = captured.clone();

        let refreshed_authority =
            test_support::EnvGuard::set(BUNDLE_SERVICE_BASE_URL_ENV, "https://bundle-b.example/v2");
        let refreshed = BundleServiceCredential::from_environment().unwrap();

        assert_eq!(
            captured.archive_url.as_str(),
            "https://bundle-a.example/v1/bundle/archive"
        );
        assert_eq!(background.archive_url, captured.archive_url);
        assert_eq!(
            refreshed.archive_url.as_str(),
            "https://bundle-b.example/v2/bundle/archive"
        );

        drop(refreshed_authority);
        drop(original_authority);
    }

    #[tokio::test]
    async fn bundle_fetch_never_forwards_the_credential_across_a_redirect() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let bytes = first.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..bytes]).to_ascii_lowercase();
            assert!(request.contains("authorization: bearer deployment-secret"));
            first
                .write_all(
                    format!(
                        "HTTP/1.1 302 Found\r\nLocation: http://{address}/credential-leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            drop(first);
            tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept())
                .await
                .is_err()
        });
        let credential = loopback_credential_for_http_test(
            &format!("http://{address}/api"),
            "deployment-secret",
        );

        let error = fetch_bundle(&credential).await.unwrap_err();
        assert!(matches!(error, BackendError::RequestFailed { status: 302 }));
        assert!(
            server.await.unwrap(),
            "redirect target must not be requested"
        );
    }

    #[tokio::test]
    async fn bundle_fetch_does_not_retain_an_untrusted_error_body() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            let body = "server-secret-that-must-not-enter-errors";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let credential =
            loopback_credential_for_http_test(&format!("http://{address}"), "deployment-secret");

        let error = fetch_bundle(&credential).await.unwrap_err();
        let rendered = error.to_string();
        assert!(matches!(error, BackendError::RequestFailed { status: 403 }));
        assert!(!rendered.contains("server-secret"));
        server.await.unwrap();
    }
}

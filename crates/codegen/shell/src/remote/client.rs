//! HTTP client for deployment-key backend resources.

async fn parse_json_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, BackendError> {
    let bytes = response.bytes().await?;
    serde_json::from_slice(&bytes).map_err(BackendError::from)
}

fn apply_deployment_key(
    builder: reqwest::RequestBuilder,
    deployment_key: Option<&str>,
) -> reqwest::RequestBuilder {
    let builder = match deployment_key.filter(|key| !key.is_empty()) {
        Some(key) => builder.header("Authorization", format!("Bearer {key}")),
        None => builder,
    };
    builder
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
pub async fn fetch_bundle(
    cli_chat_proxy_base_url: &str,
    deployment_key: Option<&str>,
) -> Result<Vec<u8>, BackendError> {
    let archive_url = format!("{cli_chat_proxy_base_url}/bundle/archive");
    let response = apply_deployment_key(
        crate::http::shared_client()
            .get(&archive_url)
            .timeout(std::time::Duration::from_secs(30)),
        deployment_key,
    )
    .send()
    .await?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(BackendError::RequestFailed { status, body });
    }
    Ok(response.bytes().await?.to_vec())
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Request failed: {status} - {body}")]
    RequestFailed { status: u16, body: String },
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Default context window when the configured endpoint does not provide one.
pub(crate) const DEFAULT_CONTEXT_WINDOW: u64 = 256_000;

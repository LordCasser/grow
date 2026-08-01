//! HTTP client for deployment-key backend resources.

use prod_mc_cli_chat_proxy_types::SubagentBundle;

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

/// Fetch the legacy JSON bundle using only an explicit deployment key.
pub async fn fetch_subagent_bundle(
    cli_chat_proxy_base_url: &str,
    deployment_key: Option<&str>,
) -> Result<SubagentBundle, BackendError> {
    let url = format!("{cli_chat_proxy_base_url}/subagents/bundle");
    let response = apply_deployment_key(
        crate::http::shared_client()
            .get(&url)
            .timeout(std::time::Duration::from_secs(10)),
        deployment_key,
    )
    .send()
    .await?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(BackendError::RequestFailed { status, body });
    }
    parse_json_response(response).await
}

#[derive(Debug)]
pub enum FetchedBundle {
    Archive(Vec<u8>),
    Legacy(SubagentBundle),
}

/// Fetch a bundle archive, falling back to the legacy JSON endpoint.
pub async fn fetch_bundle(
    cli_chat_proxy_base_url: &str,
    deployment_key: Option<&str>,
) -> Result<FetchedBundle, BackendError> {
    let archive_url = format!("{cli_chat_proxy_base_url}/bundle/archive");
    let response = apply_deployment_key(
        crate::http::shared_client()
            .get(&archive_url)
            .timeout(std::time::Duration::from_secs(30)),
        deployment_key,
    )
    .send()
    .await?;
    if response.status().is_success() {
        return Ok(FetchedBundle::Archive(response.bytes().await?.to_vec()));
    }
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let body = response.text().await.unwrap_or_default();
        return Err(BackendError::RequestFailed { status: 401, body });
    }
    tracing::debug!(
        status = %response.status(),
        "bundle archive unavailable, falling back to legacy JSON"
    );
    fetch_subagent_bundle(cli_chat_proxy_base_url, deployment_key)
        .await
        .map(FetchedBundle::Legacy)
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

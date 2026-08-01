//! Managed MCP configuration resolution via cli-chat-proxy.
//!
//! This module treats returned transport headers as opaque deployment-managed
//! configuration. Grow does not own an interactive authorization or token
//! refresh lifecycle for these servers.
//!
//! Config-file/plugin merge layering (which reads shell's config system) lives
//! in shell's `session::managed_mcp`, which re-exports everything here.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use agent_client_protocol as acp;

/// Agent-level cache for managed MCP configs.
///
/// Explicit tri-state prevents concurrent double-fetches:
/// only the first caller transitions `NotFetched → Fetching`;
/// subsequent callers see `Fetching` and wait for the in-flight fetch.
pub enum ManagedMcpCache {
    NotFetched,
    /// Fetch in progress — callers should wait on `fetch_notify`.
    Fetching,
    /// May be empty if no managed servers are configured for this user.
    Ready(Vec<ManagedMcpConfig>),
}

/// Agent-level cache for managed MCP gateway tool catalogs.
pub enum GatewayToolCatalogCache {
    NotFetched,
    /// Fetch in progress for the recorded gateway tool epoch.
    Fetching(u64),
    /// May be empty if the user has no gateway-exposed tools.
    Ready(GatewayToolCatalog),
}

enum FetchDecision<T> {
    Ready(T),
    Wait(tokio::sync::futures::OwnedNotified),
    Fetch,
}

trait ThreeStateFetchCache {
    type Value: Clone;

    fn claim_fetch(&mut self, notify: Arc<tokio::sync::Notify>) -> FetchDecision<Self::Value>;
}

impl ThreeStateFetchCache for ManagedMcpCache {
    type Value = Vec<ManagedMcpConfig>;

    fn claim_fetch(&mut self, notify: Arc<tokio::sync::Notify>) -> FetchDecision<Self::Value> {
        match self {
            Self::Ready(configs) => FetchDecision::Ready(configs.clone()),
            Self::Fetching => FetchDecision::Wait(notify.notified_owned()),
            Self::NotFetched => {
                *self = Self::Fetching;
                FetchDecision::Fetch
            }
        }
    }
}

pub struct ManagedMcpState {
    pub cache: ManagedMcpCache,
    pub fetch_notify: Arc<tokio::sync::Notify>,
    pub gateway_tools_active: bool,
    pub gateway_tool_epoch: u64,
    pub gateway_tool_cache: GatewayToolCatalogCache,
    pub gateway_tool_fetch_notify: Arc<tokio::sync::Notify>,
    /// Retained across gateway disable/cache invalidation so the on-disk
    /// MCP descriptor mirror can remove stale gateway connector directories when
    /// the current catalog is empty or absent.
    pub gateway_tool_connectors_seen: HashSet<String>,
}

impl Default for ManagedMcpState {
    fn default() -> Self {
        Self {
            cache: ManagedMcpCache::NotFetched,
            fetch_notify: Arc::new(tokio::sync::Notify::new()),
            gateway_tools_active: false,
            gateway_tool_epoch: 0,
            gateway_tool_cache: GatewayToolCatalogCache::NotFetched,
            gateway_tool_fetch_notify: Arc::new(tokio::sync::Notify::new()),
            gateway_tool_connectors_seen: HashSet::new(),
        }
    }
}

async fn wait_for_fetch_slot<T>(
    handle: &ManagedMcpStateHandle,
    claim: impl Fn(&mut ManagedMcpState) -> FetchDecision<T>,
) -> Option<T> {
    loop {
        let decision = {
            let mut state = handle.lock().await;
            claim(&mut state)
        };
        match decision {
            FetchDecision::Ready(value) => return Some(value),
            FetchDecision::Wait(notified) => notified.await,
            FetchDecision::Fetch => return None,
        }
    }
}

async fn get_authenticated_json<T: serde::de::DeserializeOwned>(
    url: &str,
    deployment_key: &str,
    unavailable_message: &'static str,
    fetch_failed_message: &'static str,
    parse_error_message: &'static str,
) -> Result<T, ManagedMcpFetchError> {
    let resp = match grow_http::shared_client()
        .get(url)
        .timeout(std::time::Duration::from_secs(10))
        .header("Authorization", format!("Bearer {}", deployment_key))
        .header("X-Grow-Token-Auth", "grow-cli")
        .header("x-grow-client-version", grow_version::VERSION)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            let status = r.status();
            tracing::warn!(status = %status, "{}", unavailable_message);
            return Err(ManagedMcpFetchError::Status {
                status,
                message: format!("HTTP {status}"),
            });
        }
        Err(e) => {
            tracing::warn!(error = %e, "{}", fetch_failed_message);
            return Err(e.into());
        }
    };

    match resp.json::<T>().await {
        Ok(value) => Ok(value),
        Err(e) => {
            tracing::debug!(error = %e, "{}", parse_error_message);
            Err(e.into())
        }
    }
}

impl ManagedMcpState {
    /// Store fetched configs and wake concurrent callers waiting on this fetch.
    pub fn complete_fetch(&mut self, configs: Vec<ManagedMcpConfig>) {
        self.cache = ManagedMcpCache::Ready(configs);
        self.fetch_notify.notify_waiters();
    }

    /// Record a failed fetch: roll the cache back to `NotFetched` so the next
    /// caller retries, and wake concurrent waiters so they don't hang on a
    /// fetch that will never complete.
    ///
    /// Deliberately NOT `Ready(vec![])`: the agent cache has no TTL, so
    /// committing a transient failure (expired token at leader startup, proxy
    /// blip) as an empty catalog would erase managed connectors for the whole
    /// process lifetime — days, for a leader.
    pub fn fail_fetch(&mut self) {
        self.cache = ManagedMcpCache::NotFetched;
        self.fetch_notify.notify_waiters();
    }

    pub fn enable_gateway_tools(&mut self) -> u64 {
        if !self.gateway_tools_active {
            self.gateway_tool_epoch = self.gateway_tool_epoch.wrapping_add(1);
        }
        self.gateway_tools_active = true;
        self.gateway_tool_epoch
    }

    pub fn start_gateway_tool_fetch(&mut self) -> Option<u64> {
        if !self.gateway_tools_active {
            return None;
        }
        self.gateway_tool_cache = GatewayToolCatalogCache::Fetching(self.gateway_tool_epoch);
        Some(self.gateway_tool_epoch)
    }

    pub fn complete_gateway_tool_fetch(&mut self, epoch: u64, catalog: GatewayToolCatalog) -> bool {
        if !self.gateway_tools_active || self.gateway_tool_epoch != epoch {
            self.gateway_tool_fetch_notify.notify_waiters();
            return false;
        }
        self.gateway_tool_connectors_seen
            .extend(catalog.tools.iter().map(|tool| tool.connector_id.clone()));
        self.gateway_tool_cache = GatewayToolCatalogCache::Ready(catalog);
        self.gateway_tool_fetch_notify.notify_waiters();
        true
    }

    pub fn fail_gateway_tool_fetch(&mut self, epoch: u64) {
        if self.gateway_tools_active
            && self.gateway_tool_epoch == epoch
            && matches!(self.gateway_tool_cache, GatewayToolCatalogCache::Fetching(fetch_epoch) if fetch_epoch == epoch)
        {
            self.gateway_tool_cache = GatewayToolCatalogCache::NotFetched;
        }
        self.gateway_tool_fetch_notify.notify_waiters();
    }

    pub fn disable_gateway_tools(&mut self) {
        self.gateway_tools_active = false;
        self.gateway_tool_epoch = self.gateway_tool_epoch.wrapping_add(1);
        self.gateway_tool_cache = GatewayToolCatalogCache::NotFetched;
        self.gateway_tool_fetch_notify.notify_waiters();
    }
}

pub type ManagedMcpStateHandle = Arc<tokio::sync::Mutex<ManagedMcpState>>;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ManagedMcpConfig {
    /// Human-readable connector name (e.g. "Slack", "Linear").
    #[serde(default)]
    pub name: String,
    pub endpoint: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub scope_id: Option<String>,
    #[serde(default)]
    pub scope_name: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct McpConfigsResponse {
    mcp_servers: Vec<ManagedMcpConfig>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GatewayToolCallRequest {
    pub call_id: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GatewayToolCallResponse {
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GatewayToolCatalog {
    #[serde(default)]
    pub tools: Vec<GatewayTool>,
    #[serde(default)]
    pub total_tools: u32,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct GatewayTool {
    pub connector_id: String,
    pub connector_name: String,
    pub tool_id: String,
    pub tool_name: String,
    pub call_id: String,
    pub description: String,
    pub json_schema: serde_json::Value,
}

impl GatewayTool {
    pub fn qualified_name(&self) -> String {
        format!("{}__{}", self.connector_id, self.tool_id)
    }
}

/// Why a managed-MCP config fetch failed. Distinguishes "fetch failed" from
/// the legitimate "fetched, zero connectors configured" (`Ok(vec![])`) so the
/// agent cache never commits a transient failure as a permanent empty catalog.
#[derive(Debug, thiserror::Error)]
pub enum ManagedMcpFetchError {
    #[error("HTTP {status}: {message}")]
    Status {
        status: reqwest::StatusCode,
        message: String,
    },
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("no deployment key available")]
    MissingDeploymentKey,
}

/// Fetch managed MCP configs from cli-chat-proxy (`GET /v1/mcp/configs`).
///
/// `Ok(vec![])` means the server answered and the user genuinely has no
/// managed connectors. `Err(_)` means we don't know (HTTP error, transport
/// failure, parse error) — callers must NOT cache the result as empty.
pub async fn fetch_managed_configs(
    proxy_base_url: &str,
    deployment_key: &str,
) -> Result<Vec<ManagedMcpConfig>, ManagedMcpFetchError> {
    let url = format!("{}/mcp/configs", proxy_base_url);

    let response: McpConfigsResponse = get_authenticated_json(
        &url,
        deployment_key,
        "Managed MCP configs unavailable",
        "Managed MCP configs fetch failed",
        "Managed MCP configs parse error",
    )
    .await?;
    tracing::info!(
        count = response.mcp_servers.len(),
        "Fetched managed MCP configs"
    );
    Ok(response.mcp_servers)
}

// Above the server-side tool-call budget so the client is not the first
// hop to abort a slow tool call.
const GATEWAY_TOOL_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(75);

pub async fn call_gateway_tool(
    proxy_base_url: &str,
    deployment_key: &str,
    call_id: &str,
    arguments: serde_json::Value,
) -> Result<GatewayToolCallResponse, ManagedMcpFetchError> {
    let url = format!("{}/mcp/tools/call", proxy_base_url);
    let arguments = if arguments.is_null() {
        serde_json::json!({})
    } else {
        arguments
    };
    let request = GatewayToolCallRequest {
        call_id: call_id.to_owned(),
        arguments,
    };

    let resp = match grow_http::shared_client()
        .post(&url)
        .timeout(GATEWAY_TOOL_CALL_TIMEOUT)
        .header("Authorization", format!("Bearer {}", deployment_key))
        .header("X-Grow-Token-Auth", "grow-cli")
        .header("x-grow-client-version", grow_version::VERSION)
        .json(&request)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            let status = r.status();
            let message = gateway_error_message(status, r).await;
            tracing::warn!(
                call_id = %call_id,
                "Managed MCP gateway tool call unavailable: HTTP {status}"
            );
            return Err(ManagedMcpFetchError::Status { status, message });
        }
        Err(e) => {
            tracing::warn!(
                call_id = %call_id,
                "Managed MCP gateway tool call failed: {}",
                e
            );
            return Err(e.into());
        }
    };

    match resp.json::<GatewayToolCallResponse>().await {
        Ok(response) => Ok(response),
        Err(e) => {
            tracing::debug!(
                call_id = %call_id,
                "Managed MCP gateway tool call parse error: {}",
                e
            );
            Err(e.into())
        }
    }
}

async fn gateway_error_message(status: reqwest::StatusCode, response: reqwest::Response) -> String {
    let fallback = format!("HTTP {status}");
    let Ok(body) = response.text().await else {
        return fallback;
    };
    if body.trim().is_empty() {
        return fallback;
    }
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(value) => value
            .get("error")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or(fallback),
        Err(_) => fallback,
    }
}

/// Fetch the managed MCP gateway tool catalog from cli-chat-proxy
/// (`GET /v1/mcp/tools/list`).
///
/// `Ok(catalog)` means the server answered and the catalog contents are
/// authoritative for this fetch, even when empty. `Err(_)` means freshness is
/// unknown and callers must leave any cache retryable rather than committing an
/// empty catalog.
pub async fn fetch_gateway_tool_catalog(
    proxy_base_url: &str,
    deployment_key: &str,
) -> Result<GatewayToolCatalog, ManagedMcpFetchError> {
    let url = format!("{}/mcp/tools/list", proxy_base_url);

    let catalog: GatewayToolCatalog = get_authenticated_json(
        &url,
        deployment_key,
        "Managed MCP gateway tools unavailable",
        "Managed MCP gateway tools fetch failed",
        "Managed MCP gateway tools parse error",
    )
    .await?;
    tracing::info!(
        count = catalog.tools.len(),
        total_tools = catalog.total_tools,
        "Fetched managed MCP gateway tool catalog"
    );
    Ok(catalog)
}

/// Invalidate all managed MCP caches so the next caller refetches both legacy
/// managed configs and gateway tools.
pub async fn invalidate_cache(handle: &ManagedMcpStateHandle) {
    let mut state = handle.lock().await;
    state.cache = ManagedMcpCache::NotFetched;
    state.gateway_tool_cache = GatewayToolCatalogCache::NotFetched;
}

/// Invalidate only the gateway tool catalog so the next gateway-aware caller
/// refetches `/v1/mcp/tools/list`.
pub async fn invalidate_gateway_tool_cache(handle: &ManagedMcpStateHandle) {
    let mut state = handle.lock().await;
    state.gateway_tool_cache = GatewayToolCatalogCache::NotFetched;
}

/// Fetch-or-wait: returns cached configs if ready, otherwise fetches once
/// and wakes any concurrent waiters. Callers provide credentials; this
/// function owns the tri-state lifecycle.
///
/// Only a successful fetch (including a genuine zero-connector response) is
/// committed to the cache. A failed fetch — or a missing auth token — rolls
/// back to `NotFetched` and returns `vec![]` for this caller only, so the
/// next caller retries instead of inheriting a poisoned empty catalog.
/// Woken waiters re-enter the loop, observe `NotFetched`, and become the
/// next fetcher (bounded: each caller performs at most one fetch).
pub async fn get_or_fetch(
    handle: &ManagedMcpStateHandle,
    proxy_url: &str,
    deployment_key: Option<&str>,
) -> Vec<ManagedMcpConfig> {
    let fetch = wait_for_fetch_slot(handle, |state| {
        state.cache.claim_fetch(state.fetch_notify.clone())
    })
    .await;
    if let Some(configs) = fetch {
        return configs;
    }

    let result = match deployment_key {
        Some(key) => fetch_managed_configs(proxy_url, key).await,
        None => Err(ManagedMcpFetchError::MissingDeploymentKey),
    };

    match result {
        Ok(configs) => {
            handle.lock().await.complete_fetch(configs.clone());
            configs
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Managed MCP fetch failed; leaving cache unpopulated for retry"
            );
            handle.lock().await.fail_fetch();
            vec![]
        }
    }
}

/// Fetch-or-wait for the managed MCP gateway tool catalog.
///
/// Returns `Some(catalog)` for either a cached catalog or a successful fresh
/// fetch, including a genuine empty catalog. Returns `None` when gateway tools
/// are disabled by the caller, auth is unavailable, or the fetch failed. Failed
/// fetches roll back to `NotFetched`, so a later caller can retry.
pub async fn get_or_fetch_gateway_tool_catalog(
    handle: &ManagedMcpStateHandle,
    proxy_url: &str,
    deployment_key: Option<&str>,
) -> Option<GatewayToolCatalog> {
    let fetch_epoch = loop {
        let maybe_notify = {
            let mut state = handle.lock().await;
            if !state.gateway_tools_active {
                return None;
            }
            match &state.gateway_tool_cache {
                GatewayToolCatalogCache::Ready(catalog) => return Some(catalog.clone()),
                GatewayToolCatalogCache::Fetching(_) => {
                    Some(state.gateway_tool_fetch_notify.clone().notified_owned())
                }
                GatewayToolCatalogCache::NotFetched => {
                    let epoch = state.start_gateway_tool_fetch()?;
                    break epoch;
                }
            }
        };

        if let Some(notified) = maybe_notify {
            notified.await;
            continue;
        }
    };

    let result = match deployment_key {
        Some(key) => fetch_gateway_tool_catalog(proxy_url, key).await,
        None => Err(ManagedMcpFetchError::MissingDeploymentKey),
    };

    match result {
        Ok(catalog) => {
            let committed = handle
                .lock()
                .await
                .complete_gateway_tool_fetch(fetch_epoch, catalog.clone());
            committed.then_some(catalog)
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Managed MCP gateway tool fetch failed; leaving cache unpopulated for retry"
            );
            handle.lock().await.fail_gateway_tool_fetch(fetch_epoch);
            None
        }
    }
}

/// Namespace prefix for managed MCP servers.
///
/// Servers with names starting with this prefix are managed by service.example.com —
/// their transport headers are supplied by deployment-managed configuration.
/// Servers without this prefix are user-managed (local keychain, config.toml headers, etc.).
///
/// Examples:
///   `grow_managed_linear`  → managed by service.example.com
///   `grow_managed_slack`   → managed by service.example.com
///   `my_company_api`   → user-managed (local)
///
/// Single source of truth lives in `grow-workspace` (which matches policy
/// `serverName`s against it); re-exported here so the two never drift.
pub use grow_workspace::permission::resolution::MANAGED_MCP_PREFIX;

/// `"Linear"` -> `"grow_managed_linear"`. Shares normalization and the
/// `MANAGED_MCP_NAME_MAX_CHARS` cap with policy matching (`mcp_name_matches`) so
/// the runtime name and a policy `serverName` never drift.
pub fn to_managed_name(display_name: &str) -> String {
    use grow_workspace::permission::resolution::{
        MANAGED_MCP_NAME_MAX_CHARS, normalize_managed_name,
    };
    let raw = format!(
        "{MANAGED_MCP_PREFIX}{}",
        normalize_managed_name(display_name)
    );
    grow_shell_base::util::truncate(&raw, MANAGED_MCP_NAME_MAX_CHARS).to_string()
}

/// Returns `true` if this server should use server-side managed credentials.
///
/// Both conditions must hold:
/// 1. Server name starts with `MANAGED_MCP_PREFIX` ("grow_managed_")
/// 2. Server URL matches a managed config endpoint
///
/// This prevents false injection if a user accidentally names a server `grow_managed_*`
/// but it's not actually in the catalog, and prevents injecting into servers
/// that happen to share a URL but aren't opted in to managed auth.
pub fn should_inject_managed_auth(
    server_name: &str,
    server_url: &str,
    managed_by_url: &HashMap<String, &ManagedMcpConfig>,
) -> bool {
    server_name.starts_with(MANAGED_MCP_PREFIX)
        && managed_by_url.contains_key(&normalize_url(server_url))
}

pub fn normalize_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

/// Key for managed config lookup: (normalized_url, scope, scope_id).
type ManagedConfigKey = (String, Option<String>, Option<String>);

/// Inject deployment-managed headers into `grow_managed_`-prefixed MCP servers.
///
/// Matches on endpoint URL + scope from `X-Connector-Scope` headers.
/// Falls back to URL-only match when no scope headers are present (backward compat).
/// Existing headers are preserved; managed headers are appended.
/// Non-prefixed servers are left untouched.
pub fn inject_managed_headers(servers: &mut [acp::McpServer], managed: &[ManagedMcpConfig]) {
    tracing::debug!(
        servers = servers.len(),
        managed = managed.len(),
        "Injecting managed MCP credentials"
    );
    if managed.is_empty() {
        return;
    }

    let managed_by_key: HashMap<ManagedConfigKey, &ManagedMcpConfig> = managed
        .iter()
        .map(|m| {
            let key = (
                normalize_url(&m.endpoint),
                m.scope.clone(),
                m.scope_id.clone(),
            );
            (key, m)
        })
        .collect();

    // URL-only fallback for backward compat
    let managed_by_url: HashMap<String, &ManagedMcpConfig> = managed
        .iter()
        .map(|m| (normalize_url(&m.endpoint), m))
        .collect();

    let mut injected = 0usize;
    let mut skipped_no_prefix = 0usize;
    let mut skipped_no_match = 0usize;
    let mut skipped_no_headers = 0usize;

    for server in servers.iter_mut() {
        let (name, url, headers) = match server {
            acp::McpServer::Http(acp::McpServerHttp {
                name, url, headers, ..
            })
            | acp::McpServer::Sse(acp::McpServerSse {
                name, url, headers, ..
            }) => (name.as_str(), url.as_str(), headers),
            _ => continue,
        };

        let normalized_url = normalize_url(url);

        if !name.starts_with(MANAGED_MCP_PREFIX) {
            if managed_by_url.contains_key(&normalized_url) {
                skipped_no_prefix += 1;
                tracing::debug!(
                    server_name = %name,
                    server_url = %url,
                    "Skipping managed injection: URL matches but name lacks '{}' prefix",
                    MANAGED_MCP_PREFIX,
                );
            }
            continue;
        }

        let scope = headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("x-connector-scope"))
            .map(|h| h.value.clone());
        let scope_id = headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("x-connector-scope-id"))
            .map(|h| h.value.clone());

        let config = match (&scope, &scope_id) {
            (Some(s), Some(id)) => {
                let key = (normalized_url.clone(), Some(s.clone()), Some(id.clone()));
                managed_by_key.get(&key).copied()
            }
            _ => managed_by_url.get(&normalized_url).copied(),
        };

        let Some(config) = config else {
            skipped_no_match += 1;
            tracing::debug!(
                server_name = %name,
                server_url = %url,
                scope = ?scope,
                scope_id = ?scope_id,
                "Skipping managed injection: no matching managed config"
            );
            continue;
        };

        if config.headers.is_empty() {
            skipped_no_headers += 1;
            tracing::debug!(
                server_name = %name,
                server_url = %url,
                "Skipping managed injection: managed config matched but has no headers",
            );
            continue;
        }

        let managed_keys: std::collections::HashSet<&str> =
            config.headers.keys().map(|k| k.as_str()).collect();
        headers.retain(|h| {
            !managed_keys.contains(h.name.as_str())
                && !h.name.eq_ignore_ascii_case("x-connector-scope")
                && !h.name.eq_ignore_ascii_case("x-connector-scope-id")
        });
        headers.extend(
            config
                .headers
                .iter()
                .map(|(k, v)| acp::HttpHeader::new(k.clone(), v.clone())),
        );

        injected += 1;
    }

    if injected > 0 {
        tracing::info!(count = injected, "Injected managed MCP credentials");
    }
    if skipped_no_prefix > 0 {
        tracing::info!(
            count = skipped_no_prefix,
            "Skipped servers with matching URLs but missing '{}' prefix",
            MANAGED_MCP_PREFIX,
        );
    }
    if skipped_no_match > 0 {
        tracing::info!(
            count = skipped_no_match,
            "Skipped servers with '{}' prefix but no matching managed config (URL+scope)",
            MANAGED_MCP_PREFIX,
        );
    }
    if skipped_no_headers > 0 {
        tracing::info!(
            count = skipped_no_headers,
            "Skipped servers with matching managed config but empty headers",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_managed(name: &str, endpoint: &str, scope: &str) -> ManagedMcpConfig {
        ManagedMcpConfig {
            name: name.to_string(),
            endpoint: endpoint.to_string(),
            headers: HashMap::from([("Authorization".into(), "Bearer tok".into())]),
            scope: Some(scope.to_string()),
            scope_id: Some(format!("{scope}-id-123")),
            scope_name: None,
        }
    }

    /// A failed fetch (here: no auth token) must NOT be committed to the
    /// cache as `Ready([])`. The cache has no TTL, so caching a transient
    /// failure as an empty catalog would erase managed connectors for the
    /// process lifetime — days, for a leader. It must roll back to
    /// `NotFetched` so the next caller retries.
    #[tokio::test]
    async fn failed_fetch_is_not_cached_as_ready_empty() {
        let handle = ManagedMcpStateHandle::default();
        let configs = get_or_fetch(&handle, "http://127.0.0.1:0", None).await;
        assert!(configs.is_empty());
        assert!(
            matches!(handle.lock().await.cache, ManagedMcpCache::NotFetched),
            "failed fetch must roll back to NotFetched, not poison the cache as Ready([])"
        );
    }

    #[test]
    fn gateway_tool_catalog_deserializes() {
        let catalog: GatewayToolCatalog = serde_json::from_str(
            r#"{
            "tools": [
                {
                    "connector_id": "gmail",
                    "connector_name": "Gmail",
                    "tool_id": "search",
                    "tool_name": "Search Gmail",
                    "call_id": "gmail_search",
                    "description": "Search email by query",
                    "json_schema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" }
                        },
                        "required": ["query"]
                    }
                }
            ],
            "total_tools": 1
        }"#,
        )
        .unwrap();

        assert_eq!(1, catalog.total_tools);
        let without_total_tools: GatewayToolCatalog = serde_json::from_str(
            r#"{
            "tools": []
        }"#,
        )
        .unwrap();
        assert_eq!(0, without_total_tools.total_tools);
        assert_eq!("gmail_search", catalog.tools[0].call_id);
        assert_eq!("gmail__search", catalog.tools[0].qualified_name());
        assert_eq!("gmail", catalog.tools[0].connector_id);
        assert_eq!("Gmail", catalog.tools[0].connector_name);
        assert_eq!("search", catalog.tools[0].tool_id);
        assert_eq!("Search Gmail", catalog.tools[0].tool_name);
        assert_eq!(
            Some("string"),
            catalog.tools[0]
                .json_schema
                .pointer("/properties/query/type")
                .and_then(|v| v.as_str())
        );
    }

    #[tokio::test]
    async fn gateway_tool_call_error_preserves_proxy_message() {
        use axum::Router;
        use axum::routing::post;
        use tokio::net::TcpListener;

        let app = Router::new().route(
            "/mcp/tools/call",
            post(|| async {
                (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "code": "Client specified an invalid argument",
                        "error": "Invalid arguments for google_calendar_availability: missing field `calendars`"
                    })),
                )
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let err = call_gateway_tool(
            &format!("http://{addr}"),
            "token",
            "google_calendar_availability",
            serde_json::json!({}),
        )
        .await
        .unwrap_err();

        match err {
            ManagedMcpFetchError::Status { status, message } => {
                assert_eq!(reqwest::StatusCode::BAD_REQUEST, status);
                assert_eq!(
                    "Invalid arguments for google_calendar_availability: missing field `calendars`",
                    message
                );
            }
            other => panic!("expected status error, got {other:?}"),
        }
    }

    #[test]
    fn disable_gateway_tools_clears_cached_catalog() {
        let mut state = ManagedMcpState::default();
        state.enable_gateway_tools();
        let epoch = state.start_gateway_tool_fetch().unwrap();
        assert!(state.complete_gateway_tool_fetch(
            epoch,
            GatewayToolCatalog {
                tools: vec![],
                total_tools: 0,
            }
        ));
        assert!(state.gateway_tools_active);
        assert!(matches!(
            state.gateway_tool_cache,
            GatewayToolCatalogCache::Ready(_)
        ));

        state.disable_gateway_tools();
        assert!(!state.gateway_tools_active);
        assert!(matches!(
            state.gateway_tool_cache,
            GatewayToolCatalogCache::NotFetched
        ));
    }

    #[test]
    fn stale_gateway_tool_fetch_success_does_not_commit_after_disable() {
        let mut state = ManagedMcpState::default();
        state.enable_gateway_tools();
        let epoch = state.start_gateway_tool_fetch().unwrap();
        state.disable_gateway_tools();

        let committed = state.complete_gateway_tool_fetch(
            epoch,
            GatewayToolCatalog {
                tools: vec![],
                total_tools: 0,
            },
        );

        assert!(!committed);
        assert!(!state.gateway_tools_active);
        assert!(matches!(
            state.gateway_tool_cache,
            GatewayToolCatalogCache::NotFetched
        ));
    }

    #[tokio::test]
    async fn gateway_tool_waiter_woken_by_disable_does_not_reenable() {
        let handle = ManagedMcpStateHandle::default();
        {
            let mut state = handle.lock().await;
            state.enable_gateway_tools();
            state.start_gateway_tool_fetch().unwrap();
        }
        let waiter_handle = handle.clone();
        let waiter = tokio::spawn(async move {
            get_or_fetch_gateway_tool_catalog(&waiter_handle, "http://127.0.0.1:0", Some("token"))
                .await
        });

        tokio::task::yield_now().await;
        handle.lock().await.disable_gateway_tools();
        let catalog = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter must wake after disable")
            .expect("waiter task should not panic");
        assert!(catalog.is_none());
        let state = handle.lock().await;
        assert!(!state.gateway_tools_active);
        assert!(matches!(
            state.gateway_tool_cache,
            GatewayToolCatalogCache::NotFetched
        ));
    }

    #[tokio::test]
    async fn failed_gateway_tool_fetch_is_not_cached_as_ready_empty() {
        let handle = ManagedMcpStateHandle::default();
        let catalog = get_or_fetch_gateway_tool_catalog(&handle, "http://127.0.0.1:0", None).await;
        assert!(catalog.is_none());
        assert!(
            matches!(
                handle.lock().await.gateway_tool_cache,
                GatewayToolCatalogCache::NotFetched
            ),
            "failed gateway tool fetch must roll back to NotFetched, not poison the cache as Ready(empty)"
        );
    }

    #[test]
    fn failed_gateway_tool_fetch_does_not_clear_ready_catalog_from_same_epoch() {
        let mut state = ManagedMcpState::default();
        state.enable_gateway_tools();
        let epoch = state.start_gateway_tool_fetch().unwrap();
        assert!(state.complete_gateway_tool_fetch(
            epoch,
            GatewayToolCatalog {
                tools: vec![],
                total_tools: 0,
            },
        ));

        state.fail_gateway_tool_fetch(epoch);
        assert!(matches!(
            state.gateway_tool_cache,
            GatewayToolCatalogCache::Ready(_)
        ));
    }

    #[tokio::test]
    async fn successful_gateway_tool_fetch_is_cached_ready() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let app_calls = calls.clone();
        let app = axum::Router::new().route(
            "/mcp/tools/list",
            axum::routing::get(move || {
                app_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async {
                    axum::Json(serde_json::json!({
                        "tools": [
                            {
                                "connector_id": "gmail",
                                "connector_name": "Gmail",
                                "tool_id": "search",
                                "tool_name": "Search Gmail",
                                "call_id": "gmail_search",
                                "description": "Search email by query",
                                "json_schema": { "type": "object" }
                            }
                        ],
                        "total_tools": 1
                    }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let handle = ManagedMcpStateHandle::default();
        handle.lock().await.enable_gateway_tools();
        let catalog = get_or_fetch_gateway_tool_catalog(&handle, &base_url, Some("token"))
            .await
            .expect("gateway catalog fetch should succeed");
        assert_eq!("gmail__search", catalog.tools[0].qualified_name());
        assert!(matches!(
            handle.lock().await.gateway_tool_cache,
            GatewayToolCatalogCache::Ready(_)
        ));

        let cached =
            get_or_fetch_gateway_tool_catalog(&handle, "http://127.0.0.1:0", Some("token"))
                .await
                .expect("second call should use cached catalog");
        assert_eq!("gmail_search", cached.tools[0].call_id);
        assert_eq!(1, calls.load(std::sync::atomic::Ordering::SeqCst));
        server.abort();
    }

    #[tokio::test]
    async fn gateway_tool_fetch_waiter_survives_notify_before_await() {
        let handle = ManagedMcpStateHandle::default();
        let (epoch, registered) = {
            let mut state = handle.lock().await;
            state.enable_gateway_tools();
            let epoch = state.start_gateway_tool_fetch().unwrap();
            (
                epoch,
                state.gateway_tool_fetch_notify.clone().notified_owned(),
            )
        };
        handle.lock().await.fail_gateway_tool_fetch(epoch);
        tokio::time::timeout(std::time::Duration::from_secs(1), registered)
            .await
            .expect("registered gateway catalog waiter must observe notify_waiters");
    }

    /// Concurrent callers must not hang when the in-flight fetch fails:
    /// `fail_fetch` wakes waiters, which re-enter the loop and retry (or
    /// fail) themselves instead of waiting forever on `fetch_notify`.
    #[tokio::test]
    async fn concurrent_callers_do_not_hang_on_failed_fetch() {
        let handle = ManagedMcpStateHandle::default();
        let h2 = handle.clone();
        let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(
                get_or_fetch(&handle, "http://127.0.0.1:0", None),
                get_or_fetch(&h2, "http://127.0.0.1:0", None),
            )
        })
        .await
        .expect("concurrent get_or_fetch must not hang on failure");
        assert!(a.is_empty() && b.is_empty());
        assert!(matches!(
            handle.lock().await.cache,
            ManagedMcpCache::NotFetched
        ));
    }
}

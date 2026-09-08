//! HTTP hook handler runner.
//!
//! Executes hooks by POSTing the event envelope JSON to a URL endpoint.
//! Supports the same blocking (deny/allow) response format as command hooks.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use url::Url;

use crate::config::HookSpec;
use crate::event::HookEventEnvelope;
use crate::result::{HookDecision, HttpInfo, StopHookOutcome};

use super::{
    GateKind, HookRunOutput, HookRunnerResult, RunContext, StopHookJson, stop_json_to_outcome,
};

const RESPONSE_PREVIEW_MAX: usize = 200;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// CWE-918: `true` if `ip` is in a private, link-local, or cloud metadata range
/// that must be blocked to prevent SSRF. Loopback (`127.x`/`::1`) is allowed for
/// local development servers.
fn is_blocked_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            if octets[0] == 127 {
                return false; // loopback, allowed for local dev
            }
            if octets[0] == 10 {
                return true; // RFC 1918: 10.0.0.0/8
            }
            if octets[0] == 172 && (16..=31).contains(&octets[1]) {
                return true; // RFC 1918: 172.16.0.0/12
            }
            if octets[0] == 192 && octets[1] == 168 {
                return true; // RFC 1918: 192.168.0.0/16
            }
            if octets[0] == 169 && octets[1] == 254 {
                return true; // RFC 3927: 169.254.0.0/16 (link-local, cloud metadata)
            }
            if octets[0] == 100 && (64..=127).contains(&octets[1]) {
                return true; // RFC 6598: 100.64.0.0/10 (CGNAT)
            }
            if v4.is_unspecified() {
                return true; // 0.0.0.0
            }
            false
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                return false; // ::1, allowed for local dev
            }
            if v6.is_unspecified() {
                return true; // ::
            }
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_blocked_ip(&IpAddr::V4(v4));
            }
            let segments = v6.segments();
            if segments[0] & 0xffc0 == 0xfe80 {
                return true; // fe80::/10 link-local
            }
            if segments[0] & 0xfe00 == 0xfc00 {
                return true; // fc00::/7 unique local (ULA)
            }
            false
        }
    }
}

/// Resolve once and return only addresses admitted by the hook IP policy.
async fn validate_hook_url(url: &str) -> Result<(String, Vec<SocketAddr>), String> {
    let parsed = Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!(
            "only https:// URLs are allowed for HTTP hooks, got {}://",
            parsed.scheme()
        ));
    }
    let host = parsed.host().ok_or_else(|| "URL has no host".to_string())?;
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<SocketAddr> = match host {
        url::Host::Ipv4(ip) => vec![SocketAddr::new(IpAddr::V4(ip), port)],
        url::Host::Ipv6(ip) => vec![SocketAddr::new(IpAddr::V6(ip), port)],
        url::Host::Domain(domain) => tokio::net::lookup_host((domain, port))
            .await
            .map_err(|e| format!("DNS resolution failed for {domain}: {e}"))?
            .collect(),
    };
    if addrs.is_empty() {
        return Err(format!("DNS resolved no addresses for {host}"));
    }
    for addr in &addrs {
        if is_blocked_ip(&addr.ip()) {
            return Err(format!(
                "URL host {host} resolves to blocked private/internal IP: {}",
                addr.ip()
            ));
        }
    }
    Ok((host.to_string(), addrs))
}

fn build_hook_client(timeout_ms: u64, host: &str, addrs: &[SocketAddr]) -> reqwest::Client {
    reqwest::Client::builder()
        // Proxy-side resolution would bypass the address admission above.
        .no_proxy()
        .resolve_to_addrs(host, addrs)
        .timeout(Duration::from_millis(timeout_ms))
        // `validate_hook_url` only vets the initial URL, not redirect targets.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        // A default fallback would follow redirects and drop the timeout,
        // reopening the SSRF path; build only fails on a TLS init fault.
        .expect("hook HTTP client config is valid")
}

/// Read decision bodies without retaining unbounded endpoint output.
async fn read_response_body(
    mut response: reqwest::Response,
    log_url: &str,
) -> Result<String, HookRunnerResult> {
    let mut body = Vec::new();
    loop {
        let chunk = response.chunk().await.map_err(|error| {
            if error.is_timeout() {
                HookRunnerResult::TimedOut
            } else {
                HookRunnerResult::Failed(format!(
                    "failed to read response body for {}: {}",
                    log_url,
                    error.without_url()
                ))
            }
        })?;
        let Some(chunk) = chunk else { break };
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err(HookRunnerResult::Failed(format!(
                "HTTP hook response exceeds {MAX_RESPONSE_BYTES} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// POST the serialized `HookEventEnvelope` to `spec.url` and parse the response
/// per gate mode (blocking parses a decision JSON; observe treats any 2xx as
/// success).
pub async fn run_http_hook(
    spec: &HookSpec,
    envelope: &HookEventEnvelope,
    _ctx: &RunContext<'_>,
    mode: GateKind,
) -> HookRunOutput {
    run_http_hook_with_validation(spec, envelope, mode, validate_hook_url).await
}

async fn run_http_hook_with_validation(
    spec: &HookSpec,
    envelope: &HookEventEnvelope,
    mode: GateKind,
    validate: impl AsyncFn(&str) -> Result<(String, Vec<SocketAddr>), String>,
) -> HookRunOutput {
    let start = Instant::now();
    let mut timeout_info = None;
    let execute = async {
        let Some(ref raw_url) = spec.url else {
            return (
                HookRunnerResult::Failed("http hook has no 'url' field".into()),
                start.elapsed(),
                None,
            );
        };

        // Re-expand the URL here (in addition to the load-time pass) because plugin
        // vars (e.g. `${GROW_PLUGIN_ROOT}/check`) only land in `extra_env` after
        // the plugin adapter runs. Unset refs are preserved so `validate_hook_url`
        // rejects them rather than smuggling a literal `${VAR}` past validation.
        let expanded_url = crate::env_expand::expand_env_vars_with_extra(raw_url, &spec.extra_env);
        let url: &str = &expanded_url;
        // Prefer the pre-expansion source for logs so resolved `env` secrets don't
        // reach `~/.grow/logs`; threaded into the reqwest error format below so
        // reqwest's default `Display` (which appends the URL) can't bypass it.
        let log_url: &str = spec.url_raw.as_deref().unwrap_or(url);

        let make_info = |status: Option<u16>, preview: Option<String>| -> HttpInfo {
            HttpInfo {
                url: url.to_owned(),
                raw_url: spec.url_raw.clone(),
                status,
                response_preview: preview,
            }
        };

        timeout_info = Some(make_info(None, None));
        let validation = validate(url).await;
        let (host, addrs) = match validation {
            Ok(validated) => validated,
            Err(reason) => {
                tracing::warn!(
                    hook_name = %spec.name,
                    url = %log_url,
                    %reason,
                    "SSRF protection: blocked HTTP hook URL"
                );
                return (
                    HookRunnerResult::Failed(format!("blocked by SSRF protection: {reason}")),
                    start.elapsed(),
                    Some(make_info(None, None)),
                );
            }
        };

        let body = match serde_json::to_string(envelope) {
            Ok(j) => j,
            Err(e) => {
                return (
                    HookRunnerResult::Failed(format!("failed to serialize envelope: {e}")),
                    start.elapsed(),
                    Some(make_info(None, None)),
                );
            }
        };

        let client = build_hook_client(spec.timeout_ms, &host, &addrs);

        let response = match client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let elapsed = start.elapsed();
                // SECURITY: `reqwest::Error::Display` appends the request URL, which
                // may embed an `env`-map secret and leak into `Failed.error` and
                // pager scrollback. `e.without_url()` strips it so we substitute
                // `log_url` (the raw source form).
                if e.is_timeout() {
                    return (
                        HookRunnerResult::TimedOut,
                        elapsed,
                        Some(make_info(None, None)),
                    );
                }
                let error = format!("HTTP request failed for {}: {}", log_url, e.without_url());
                return (
                    HookRunnerResult::Failed(error),
                    elapsed,
                    Some(make_info(None, None)),
                );
            }
        };

        let status = response.status();
        let status_code = status.as_u16();
        timeout_info = Some(make_info(Some(status_code), None));
        let elapsed = start.elapsed();

        tracing::debug!(
            hook_name = %spec.name,
            url = %log_url,
            status = status_code,
            elapsed_ms = elapsed.as_millis() as u64,
            "http hook completed"
        );

        if mode == GateKind::Observe {
            let http_info = Some(make_info(Some(status_code), None));
            if status.is_success() {
                return (HookRunnerResult::Success, elapsed, http_info);
            }
            return (
                HookRunnerResult::Failed(format!("HTTP status {}", status)),
                elapsed,
                http_info,
            );
        }

        let response_text = match read_response_body(response, log_url).await {
            Ok(text) => text,
            Err(result) => {
                return (
                    result,
                    start.elapsed(),
                    Some(make_info(Some(status_code), None)),
                );
            }
        };
        let elapsed = start.elapsed();

        let response_preview = if response_text.trim().is_empty() {
            None
        } else {
            Some(truncate_preview(&response_text))
        };

        let http_info = Some(make_info(Some(status_code), response_preview.clone()));

        let result = match mode {
            GateKind::Prompt | GateKind::Tool => {
                parse_http_blocking_result(&response_text, status, &spec.name)
            }
            GateKind::Stop => parse_http_stop_result(&response_text, status, &spec.name),
            GateKind::Observe => HookRunnerResult::Success,
        };
        (result, elapsed, http_info)
    };
    match tokio::time::timeout(Duration::from_millis(spec.timeout_ms), execute).await {
        Ok(result) => result,
        Err(_) => (HookRunnerResult::TimedOut, start.elapsed(), timeout_info),
    }
}

/// HTTP analogue of `command::parse_stop_result`: a 2xx JSON body is parsed for
/// the decision; a 2xx empty/non-JSON body allows the stop; a non-2xx response
/// is a failure (callers fail open).
fn parse_http_stop_result(
    response_text: &str,
    status: reqwest::StatusCode,
    hook_name: &str,
) -> HookRunnerResult {
    if !status.is_success() {
        return HookRunnerResult::Failed(format!("HTTP status {status}"));
    }
    let trimmed = response_text.trim();
    if trimmed.is_empty() {
        return HookRunnerResult::Stop(StopHookOutcome::default());
    }
    match super::parse_hook_json::<StopHookJson>(trimmed) {
        Ok(json) => match stop_json_to_outcome(json, hook_name) {
            Ok(outcome) => HookRunnerResult::Stop(outcome),
            Err(err) => HookRunnerResult::Failed(err),
        },
        Err(e) if super::is_structured_output_error(trimmed, &e) => HookRunnerResult::Failed(
            format!("stop hook '{hook_name}' returned invalid decision JSON: {e}"),
        ),
        Err(e) => {
            tracing::warn!(
                hook_name = %hook_name,
                error = %e,
                "could not parse HTTP stop hook response JSON, treating as allow-stop"
            );
            HookRunnerResult::Stop(StopHookOutcome::default())
        }
    }
}

/// Parse an HTTP blocking hook response, the analogue of
/// `command::parse_blocking_result`.
fn parse_http_blocking_result(
    response_text: &str,
    status: reqwest::StatusCode,
    hook_name: &str,
) -> HookRunnerResult {
    if response_text.trim().is_empty() {
        if status.is_success() {
            return HookRunnerResult::Decision(HookDecision::Allow);
        }
        return HookRunnerResult::Failed(format!("HTTP status {} with empty body", status));
    }

    match super::parse_hook_json::<super::GateHookJson>(response_text) {
        Ok(output) => match super::gate_json_to_decision(output, hook_name) {
            Ok(HookDecision::Allow) if !status.is_success() => {
                HookRunnerResult::Failed(format!("HTTP status {status}"))
            }
            Ok(decision) => HookRunnerResult::Decision(decision),
            Err(err) => HookRunnerResult::Failed(err),
        },
        Err(e) if super::is_structured_output_error(response_text, &e) => HookRunnerResult::Failed(
            format!("hook '{hook_name}' returned invalid decision JSON: {e}"),
        ),
        Err(e) => {
            if status.is_success() {
                tracing::warn!(
                    hook_name = %hook_name,
                    error = %e,
                    "could not parse HTTP hook response JSON, treating as allow"
                );
                HookRunnerResult::Decision(HookDecision::Allow)
            } else {
                HookRunnerResult::Failed(format!(
                    "HTTP status {} and failed to parse response: {e}",
                    status
                ))
            }
        }
    }
}

/// Truncate a response body for preview, cutting on a UTF-8 char boundary so
/// multi-byte characters never panic.
fn truncate_preview(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= RESPONSE_PREVIEW_MAX {
        trimmed.to_string()
    } else {
        let boundary = trimmed
            .char_indices()
            .take_while(|&(i, _)| i <= RESPONSE_PREVIEW_MAX)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        let mut preview = trimmed[..boundary].to_string();
        preview.push_str("...");
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    /// reqwest is built with `rustls-no-provider` (see the vendoring notes on
    /// the workspace's rustls setup): production installs the ring provider at
    /// CLI startup, but tests bypass startup, so install it once here.
    #[ctor::ctor]
    fn install_rustls_provider() {
        diagnostics::tls::install_ring_provider_once();
    }

    #[test]
    fn http_allow_cannot_hide_failure_status() {
        for code in [302, 403, 500] {
            let status = StatusCode::from_u16(code).unwrap();
            assert!(matches!(
                parse_http_blocking_result(r#"{"decision":"allow"}"#, status, "gate"),
                HookRunnerResult::Failed(_)
            ));
            assert!(matches!(
                parse_http_blocking_result(r#"{"decision":"deny"}"#, status, "gate"),
                HookRunnerResult::Decision(HookDecision::Deny { .. })
            ));
        }
    }

    #[test]
    fn structured_errors_are_not_http_log_noise() {
        for body in [
            r#"{"decision":false}"#,
            r#"{"decision":"block","typo":1}"#,
            "{\"decision\":",
            "[]",
        ] {
            assert!(
                matches!(
                    parse_http_blocking_result(body, StatusCode::OK, "gate"),
                    HookRunnerResult::Failed(_)
                ),
                "{body}"
            );
            assert!(
                matches!(
                    parse_http_stop_result(body, StatusCode::OK, "stop"),
                    HookRunnerResult::Failed(_)
                ),
                "{body}"
            );
        }
    }

    #[test]
    fn http_allow_json() {
        let result =
            parse_http_blocking_result(r#"{"decision":"allow"}"#, StatusCode::OK, "test-hook");
        assert!(matches!(
            result,
            HookRunnerResult::Decision(HookDecision::Allow)
        ));
    }

    #[test]
    fn http_deny_json_with_reason() {
        let result = parse_http_blocking_result(
            r#"{"decision":"deny","reason":"dangerous command"}"#,
            StatusCode::OK,
            "test-hook",
        );
        match result {
            HookRunnerResult::Decision(HookDecision::Deny { reason, hook_name }) => {
                assert_eq!(reason, "dangerous command");
                assert_eq!(hook_name, "test-hook");
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn http_deny_json_without_reason() {
        let result =
            parse_http_blocking_result(r#"{"decision":"deny"}"#, StatusCode::OK, "my-hook");
        match result {
            HookRunnerResult::Decision(HookDecision::Deny { reason, .. }) => {
                assert!(
                    reason.contains("my-hook"),
                    "reason should mention hook name"
                );
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn http_unknown_decision_fails() {
        let result =
            parse_http_blocking_result(r#"{"decision":"maybe"}"#, StatusCode::OK, "test-hook");
        match result {
            HookRunnerResult::Failed(msg) => {
                assert!(msg.contains("maybe"));
                assert!(msg.contains("test-hook"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// The JSON vocabulary itself is covered by the shared `stop_json_to_outcome`
    /// tests; this covers only the HTTP status/body handling.
    #[test]
    fn http_stop_status_and_body_handling() {
        match parse_http_stop_result(
            r#"{"decision":"block","reason":"tests failing"}"#,
            StatusCode::OK,
            "s",
        ) {
            HookRunnerResult::Stop(o) => {
                assert_eq!(o.block_reason.as_deref(), Some("tests failing"));
            }
            other => panic!("expected Stop, got {other:?}"),
        }
        match parse_http_stop_result("", StatusCode::OK, "s") {
            HookRunnerResult::Stop(o) => assert!(o.is_empty()),
            other => panic!("expected Stop, got {other:?}"),
        }
        match parse_http_stop_result("not json", StatusCode::OK, "s") {
            HookRunnerResult::Stop(o) => assert!(o.is_empty()),
            other => panic!("expected Stop, got {other:?}"),
        }
        assert!(matches!(
            parse_http_stop_result(r#"{"decision":"deny"}"#, StatusCode::OK, "s"),
            HookRunnerResult::Failed(_)
        ));
        assert!(matches!(
            parse_http_stop_result(
                r#"{"decision":"block"}"#,
                StatusCode::INTERNAL_SERVER_ERROR,
                "s"
            ),
            HookRunnerResult::Failed(_)
        ));
    }

    #[test]
    fn http_empty_body_success_allows() {
        for body in ["", "   \n  "] {
            let result = parse_http_blocking_result(body, StatusCode::OK, "test-hook");
            assert!(matches!(
                result,
                HookRunnerResult::Decision(HookDecision::Allow)
            ));
        }
    }

    #[test]
    fn http_empty_body_error_status_fails() {
        let result = parse_http_blocking_result("", StatusCode::INTERNAL_SERVER_ERROR, "test-hook");
        match result {
            HookRunnerResult::Failed(msg) => {
                assert!(msg.contains("500"));
                assert!(msg.contains("empty body"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn http_plain_text_allows_but_truncated_json_fails() {
        assert!(matches!(
            parse_http_blocking_result("not json at all", StatusCode::OK, "test-hook"),
            HookRunnerResult::Decision(HookDecision::Allow)
        ));
        assert!(matches!(
            parse_http_blocking_result(r#"{"decision":"deny""#, StatusCode::OK, "test-hook"),
            HookRunnerResult::Failed(_)
        ));
    }

    #[test]
    fn http_invalid_json_error_status_fails() {
        let result =
            parse_http_blocking_result("not json", StatusCode::INTERNAL_SERVER_ERROR, "test-hook");
        match result {
            HookRunnerResult::Failed(msg) => {
                assert!(msg.contains("500"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn http_deny_with_non_success_status() {
        let result = parse_http_blocking_result(
            r#"{"decision":"deny","reason":"forbidden"}"#,
            StatusCode::FORBIDDEN,
            "test-hook",
        );
        match result {
            HookRunnerResult::Decision(HookDecision::Deny { reason, .. }) => {
                assert_eq!(reason, "forbidden");
            }
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn ssrf_blocks_private_and_special_ranges() {
        for ip in [
            "10.0.0.1", // RFC 1918 10.0.0.0/8
            "10.255.255.255",
            "172.16.0.1", // RFC 1918 172.16.0.0/12
            "172.31.255.255",
            "192.168.0.1", // RFC 1918 192.168.0.0/16
            "192.168.255.255",
            "169.254.0.1", // link-local / metadata
            "169.254.169.254",
            "100.64.0.1", // CGNAT
            "100.127.255.255",
            "0.0.0.0", // unspecified
            "::",
        ] {
            assert!(is_blocked_ip(&ip.parse().unwrap()), "{ip} must be blocked");
        }
        // Just outside the blocked RFC 1918 / CGNAT ranges.
        for ip in ["172.15.0.1", "172.32.0.1", "100.63.0.1"] {
            assert!(!is_blocked_ip(&ip.parse().unwrap()), "{ip} must be allowed");
        }
    }

    #[test]
    fn ssrf_allows_loopback_and_public() {
        for ip in ["127.0.0.1", "::1", "1.1.1.1", "8.8.8.8"] {
            assert!(!is_blocked_ip(&ip.parse().unwrap()), "{ip} must be allowed");
        }
    }

    #[test]
    fn ssrf_blocks_ipv6_ranges() {
        for ip in [
            "fe80::1", // link-local
            "fc00::1", // unique local (ULA)
            "fd00::1",
            "::ffff:10.0.0.1", // IPv4-mapped private
            "::ffff:192.168.1.1",
        ] {
            assert!(
                is_blocked_ip(&ip.parse::<IpAddr>().unwrap()),
                "{ip} must be blocked"
            );
        }
    }

    #[tokio::test]
    async fn ssrf_rejects_non_https_schemes() {
        for url in ["http://example.com/hook", "ftp://example.com/hook"] {
            let err = validate_hook_url(url).await.expect_err("must reject");
            assert!(err.contains("https://"));
        }
    }

    #[tokio::test]
    async fn ssrf_rejects_blocked_ip_literals() {
        for url in [
            "https://10.0.0.1/hook",
            "https://169.254.169.254/latest/meta-data/",
        ] {
            let err = validate_hook_url(url).await.expect_err("must reject");
            assert!(err.contains("blocked"));
        }
    }

    #[tokio::test]
    async fn ssrf_allows_https_public_ip() {
        let result = validate_hook_url("https://1.1.1.1/hook").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn ssrf_rejects_invalid_url() {
        let result = validate_hook_url("not a url").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid URL"));
    }

    /// A host that never resolves fails validation (covers the DNS branch that
    /// the literal-IP tests skip). `.invalid` is reserved to never resolve.
    #[tokio::test]
    async fn ssrf_rejects_unresolvable_host() {
        let err = validate_hook_url("https://nonexistent.invalid/hook")
            .await
            .expect_err("unresolvable host must fail validation");
        assert!(err.contains("DNS resolution failed"), "got: {err}");
    }

    use crate::config::HookSpec;
    use crate::event::{HookEventEnvelope, HookEventName, HookPayload};
    use crate::test_support::with_env_var;

    #[tokio::test]
    async fn validation_and_body_share_total_timeout() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let raw = format!("http://{addr}/hook");
        let spec = HookSpec {
            name: "test-ssrf-post-expand".into(),
            event: HookEventName::PreToolUse,
            handler_type: crate::config::HandlerType::Http,
            configured_matcher: None,
            matcher: None,
            enabled: true,
            command: None,
            command_raw: None,
            url: Some(raw.to_string()),
            url_raw: Some(raw.to_string()),
            timeout_ms: 200,
            on_failure: crate::config::OnFailure::Allow,
            source_dir: std::env::temp_dir(),
            extra_env: Default::default(),
            layer: crate::config::HookProvenance::File,
        };

        let envelope = HookEventEnvelope {
            hook_event_name: HookEventName::PreToolUse,
            session_id: "test".into(),
            cwd: "/tmp".into(),
            workspace_root: "/tmp".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            transcript_path: None,
            client_identifier: None,
            prompt_id: None,
            permission_mode: None,
            payload: HookPayload::PreToolUse {
                tool_name: "test".into(),
                tool_use_id: "id-1".into(),
                tool_input: serde_json::json!({}),
                tool_input_truncated: false,
                subagent_type: None,
            },
        };
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(120)).await;
            let _ = socket.write_all(b"{}").await;
        });
        let (result, elapsed, info) =
            run_http_hook_with_validation(&spec, &envelope, GateKind::Tool, async |_: &str| {
                tokio::time::sleep(Duration::from_millis(120)).await;
                Ok(("127.0.0.1".into(), vec![addr]))
            })
            .await;
        server.await.unwrap();
        assert!(
            matches!(result, HookRunnerResult::TimedOut),
            "each phase must not get a new budget: {result:?}"
        );
        assert!(elapsed >= Duration::from_millis(200));
        let info = info.unwrap();
        assert_eq!(info.status, Some(200));
        assert_eq!(info.url, raw);
        assert!(info.response_preview.is_none());
    }

    /// SSRF validation in `run_http_hook` must operate on the post-expansion URL,
    /// and `HttpInfo` must carry the resolved form while `raw_url` mirrors the
    /// source.
    #[tokio::test]
    async fn run_http_hook_uses_post_expansion_url_for_ssrf() {
        let mut extra_env = std::collections::HashMap::new();
        extra_env.insert("INTERNAL_HOST_SSRF".to_string(), "10.0.0.1".to_string());

        let raw = "https://${INTERNAL_HOST_SSRF}/hook";
        let spec = HookSpec {
            name: "test-ssrf-post-expand".into(),
            event: HookEventName::PreToolUse,
            handler_type: crate::config::HandlerType::Http,
            configured_matcher: None,
            matcher: None,
            enabled: true,
            command: None,
            command_raw: None,
            url: Some(raw.to_string()),
            url_raw: Some(raw.to_string()),
            timeout_ms: 1000,
            on_failure: crate::config::OnFailure::Allow,
            source_dir: std::env::temp_dir(),
            extra_env,
            layer: crate::config::HookProvenance::File,
        };

        let envelope = HookEventEnvelope {
            hook_event_name: HookEventName::PreToolUse,
            session_id: "test".into(),
            cwd: "/tmp".into(),
            workspace_root: "/tmp".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            transcript_path: None,
            client_identifier: None,
            prompt_id: None,
            permission_mode: None,
            payload: HookPayload::PreToolUse {
                tool_name: "test".into(),
                tool_use_id: "id-1".into(),
                tool_input: serde_json::json!({}),
                tool_input_truncated: false,
                subagent_type: None,
            },
        };
        let ctx = crate::runner::RunContext {
            session_id: "test",
            workspace_root: "/tmp",
            process_scope: None,
        };
        let (result, _, info) = run_http_hook(&spec, &envelope, &ctx, GateKind::Tool).await;

        match result {
            crate::runner::HookRunnerResult::Failed(reason) => {
                assert!(
                    reason.contains("blocked") || reason.contains("SSRF"),
                    "expected SSRF block message, got: {reason}"
                );
            }
            other => panic!("expected SSRF Failed, got {other:?}"),
        }

        let info = info.expect("HttpInfo should be present for SSRF block path");
        assert_eq!(
            info.url, "https://10.0.0.1/hook",
            "HttpInfo.url must reflect the post-expansion URL (the actual target SSRF blocked)"
        );
        assert_eq!(
            info.raw_url.as_deref(),
            Some("https://${INTERNAL_HOST_SSRF}/hook"),
            "HttpInfo.raw_url must mirror HookSpec::url_raw"
        );
    }

    /// `reqwest::Error::Display` appends the request URL, so a `${TOKEN}` secret
    /// would leak into `Failed.error` and pager scrollback. Assert the secret
    /// never appears in the error from a guaranteed-dead host (TEST-NET-1).
    #[tokio::test]
    async fn run_http_hook_scrubs_url_from_reqwest_error() {
        // TEST-NET-1 (RFC 5737) is not RFC1918, so SSRF validation lets it
        // through, but no connection succeeds: reqwest returns a connection
        // error whose default Display would include the URL.
        let secret = "ghp_VERY_REAL_SECRET_TOKEN_42";
        let mut extra_env = std::collections::HashMap::new();
        extra_env.insert("RUNTIME_HOST".to_string(), "192.0.2.1".to_string());
        extra_env.insert("MY_TOKEN".to_string(), secret.to_string());

        let raw = "https://${RUNTIME_HOST}/check?token=${MY_TOKEN}";
        let spec = HookSpec {
            name: "test-scrub-reqwest-error".into(),
            event: HookEventName::PreToolUse,
            handler_type: crate::config::HandlerType::Http,
            configured_matcher: None,
            matcher: None,
            enabled: true,
            command: None,
            command_raw: None,
            url: Some(raw.to_string()),
            url_raw: Some(raw.to_string()),
            // Short but long enough to attempt the connection, so we exercise
            // the Err(e) branch of `send().await` rather than a timeout.
            timeout_ms: 500,
            on_failure: crate::config::OnFailure::Allow,
            source_dir: std::env::temp_dir(),
            extra_env,
            layer: crate::config::HookProvenance::File,
        };
        let envelope = HookEventEnvelope {
            hook_event_name: HookEventName::PreToolUse,
            session_id: "test".into(),
            cwd: "/tmp".into(),
            workspace_root: "/tmp".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            transcript_path: None,
            client_identifier: None,
            prompt_id: None,
            permission_mode: None,
            payload: HookPayload::PreToolUse {
                tool_name: "test".into(),
                tool_use_id: "id-1".into(),
                tool_input: serde_json::json!({}),
                tool_input_truncated: false,
                subagent_type: None,
            },
        };
        let ctx = crate::runner::RunContext {
            session_id: "test",
            workspace_root: "/tmp",
            process_scope: None,
        };

        let (result, _, info) = run_http_hook(&spec, &envelope, &ctx, GateKind::Tool).await;

        // Either a timeout or a connection error is fine; both previously risked
        // embedding the raw URL via `format!("...{e}")`.
        let error_text = match result {
            crate::runner::HookRunnerResult::Failed(reason) => Some(reason),
            crate::runner::HookRunnerResult::TimedOut => None,
            other => panic!("expected Failed or TimedOut, got {other:?}"),
        };

        if let Some(error_text) = error_text.as_deref() {
            assert!(
                !error_text.contains(secret),
                "secret leaked into error text: {error_text}"
            );

            // The connection-error branch must reference the raw URL form (so users
            // see which hook failed), never the resolved secret-bearing form.
            assert!(
                error_text.contains("${RUNTIME_HOST}") || error_text.contains("${MY_TOKEN}"),
                "expected error to reference the raw URL form, got: {error_text}"
            );
        }

        // HttpInfo.url stays post-expansion for SSRF debugging; consumers prefer
        // raw_url for display (see the HttpInfo rustdoc).
        let info = info.expect("HttpInfo should be present for connection failures too");
        assert_eq!(
            info.url,
            "https://192.0.2.1/check?token=ghp_VERY_REAL_SECRET_TOKEN_42"
        );
        assert_eq!(info.raw_url.as_deref(), Some(raw));
    }

    /// The hook client must not follow redirects: `validate_hook_url` only vets
    /// the initial URL, so a followed 3xx would reach an unvalidated target.
    #[tokio::test]
    async fn decision_body_limit_rejects_unfinished_oversized_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for size in [0, MAX_RESPONSE_BYTES, MAX_RESPONSE_BYTES + 1] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (release, released) = tokio::sync::oneshot::channel();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                socket.read(&mut request).await.unwrap();
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                    .await
                    .unwrap();
                if size > 0 {
                    socket
                        .write_all(format!("{size:x}\r\n").as_bytes())
                        .await
                        .unwrap();
                    socket.write_all(&vec![b'x'; size]).await.unwrap();
                    socket.write_all(b"\r\n").await.unwrap();
                }
                if size <= MAX_RESPONSE_BYTES {
                    socket.write_all(b"0\r\n\r\n").await.unwrap();
                }
                let _ = released.await;
            });
            let response = build_hook_client(5000, "127.0.0.1", &[addr])
                .get(format!("http://{addr}/hook"))
                .send()
                .await
                .unwrap();
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                read_response_body(response, "test-hook"),
            )
            .await;
            let _ = release.send(());
            server.await.unwrap();
            let result = result.expect("oversized body must fail before EOF or request timeout");
            if size > MAX_RESPONSE_BYTES {
                assert!(
                    matches!(result, Err(HookRunnerResult::Failed(reason)) if reason.contains("exceeds"))
                );
            } else {
                assert_eq!(result.unwrap(), "x".repeat(size));
            }
        }
    }

    #[tokio::test]
    async fn hook_client_pins_addresses_and_preserves_host() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            let count = socket.read(&mut bytes).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            String::from_utf8_lossy(&bytes[..count]).to_ascii_lowercase()
        });
        let response = build_hook_client(1000, "pinned.invalid", &[addr])
            .get(format!("http://pinned.invalid:{}/hook", addr.port()))
            .send()
            .await;
        if response.is_err() {
            server.abort();
        }
        assert!(response.unwrap().status().is_success());
        assert!(
            server
                .await
                .unwrap()
                .contains(&format!("host: pinned.invalid:{}", addr.port()))
        );
    }

    #[tokio::test]
    async fn ipv6_literals_are_classified_without_dns() {
        assert!(
            validate_hook_url("https://[fd00::1]/hook")
                .await
                .unwrap_err()
                .contains("blocked")
        );
        let (_, addrs) = validate_hook_url("https://[::1]:8443/hook").await.unwrap();
        assert_eq!(addrs, vec!["[::1]:8443".parse::<SocketAddr>().unwrap()]);
    }

    #[tokio::test]
    async fn hook_client_does_not_follow_redirects() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let server_requests = Arc::clone(&requests);

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                server_requests.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                let response = "HTTP/1.1 302 Found\r\n\
                     Location: http://169.254.169.254/latest/meta-data/\r\n\
                     Content-Length: 0\r\n\r\n";
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
            }
        });

        let client = build_hook_client(5000, "127.0.0.1", &[addr]);
        let resp = client
            .post(format!("http://{addr}/hook"))
            .body("{}")
            .send()
            .await
            .expect("request should succeed without following the redirect");

        assert_eq!(
            resp.status().as_u16(),
            302,
            "redirect must be surfaced, not followed"
        );
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "client must not issue a second request to the redirect target"
        );
    }

    /// An unresolved `${VAR}` ref is preserved verbatim, so `validate_hook_url`
    /// rejects it rather than smuggling the literal placeholder past validation.
    #[tokio::test]
    async fn url_unresolved_var_fails_validation() {
        let key = "GROW_HOOKS_HTTP_TEST_UNRESOLVED";
        // `with_env_var`'s closure is synchronous (it uses `catch_unwind`), so
        // run the async `validate_hook_url` outside it to avoid nesting runtimes.
        let expanded = with_env_var(key, None, || {
            let extra = std::collections::HashMap::new();
            crate::env_expand::expand_env_vars_with_extra(
                &format!("https://${{{key}}}/check"),
                &extra,
            )
        });
        assert!(expanded.contains(&format!("${{{key}}}")));
        // Url::parse rejects the literal `${` (`{` is not a valid URL character).
        let result = validate_hook_url(&expanded).await;
        assert!(result.is_err(), "expected invalid URL error, got Ok");
    }
}

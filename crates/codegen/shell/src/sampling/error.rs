//! Sampling error types.
//!
//! The canonical error types now live in `sampling_types::error`.
//! This module re-exports them and adds `map_sampling_err_to_acp` which
//! depends on `agent_client_protocol::schema::v1::Error` (a shell dependency).

// Re-export everything from the standalone crate.
pub use sampling_types::error::*;

use acp_transport::protocol as acp;

/// ACP error code for rate-limited requests (HTTP 429).
/// Uses the JSON-RPC implementation-defined server error range (-32000 to -32099).
///
/// Contract: set only for actual HTTP 429 responses from the sampling client.
/// Clients derive user-facing text via [`format_rate_limited_user_message`].
/// The ACP path is unchanged: `prompt_complete_fields` still reports the
/// stop reason with no detail.
pub const RATE_LIMITED_ERROR_CODE: i32 = -32003;

pub const RATE_LIMITED_USER_MESSAGE: &str =
    "The selected provider rate-limited the request. Review its quota or try again later.";

/// User-facing text for an ACP -32003 rate-limit error.
///
/// Non-empty provider detail is preserved after removing the sampler's display
/// prefix. Empty responses use a provider-neutral fallback. Callers still run
/// their normal redaction and length cap before display.
pub fn format_rate_limited_user_message(server_detail: Option<&str>) -> String {
    if let Some(detail) = server_detail.map(str::trim).filter(|s| !s.is_empty()) {
        return strip_sampling_api_error_prefix(detail).to_string();
    }
    RATE_LIMITED_USER_MESSAGE.to_string()
}

/// Drop `SamplingError::Api`'s Display prefix so users see the IC body, not
/// `API error (status 429 Too Many Requests): …`.
fn strip_sampling_api_error_prefix(detail: &str) -> &str {
    const PREFIX: &str = "API error (status ";
    const SEP: &str = "): ";
    if let Some(rest) = detail.strip_prefix(PREFIX)
        && let Some(idx) = rest.find(SEP)
    {
        return rest[idx + SEP.len()..].trim();
    }
    detail.trim()
}

/// IC sometimes reuses consumer free-tier upsell copy on 429s ("upgrade to a Grow
/// subscription" / grow.com/supergrok). That is wrong for BYOK traffic:
/// higher limits come from credits and spend-based rate-limit tiers, not a
/// personal SuperGrok plan.
fn pushes_consumer_subscription_upsell(detail: &str) -> bool {
    let d = detail.to_ascii_lowercase();
    d.contains("grow.com/supergrok") || d.contains("upgrade to a grow subscription")
}

/// User-facing copy for capacity/overload failures (stream `overloaded_error`,
/// HTTP 529, proxy-wrapped 5xx). See [`SamplingError::is_overloaded`].
pub const OVERLOADED_USER_MESSAGE: &str = "Model is temporarily overloaded. Try again in a moment.";

/// Map a `SamplingError` to an ACP `Error` for client-facing responses.
/// This stays in shell because it depends on `agent_client_protocol::schema::v1::Error`.
pub fn map_sampling_err_to_acp(err: SamplingError) -> acp::Error {
    use reqwest::StatusCode;
    // Capacity/overload gets the same short copy on every surface. Message
    // only, `data` deliberately unset: `Display` appends JSON-encoded `data`,
    // and this string is meant for direct display.
    if err.is_overloaded() {
        return acp::Error::new(
            acp::ErrorCode::InternalError.into(),
            OVERLOADED_USER_MESSAGE,
        );
    }
    match err {
        SamplingError::Auth { message, .. } => acp::Error::auth_required().data(message),
        SamplingError::InvalidConfiguration(msg) => acp::Error::invalid_params().data(msg),
        SamplingError::Http(e) => {
            acp::Error::internal_error().data(format!("http client init failed: {e}"))
        }
        SamplingError::Serialization(_) => acp::Error::invalid_params().data(err.to_string()),
        SamplingError::Api {
            status, message, ..
        } => match status {
            StatusCode::UNAUTHORIZED => acp::Error::auth_required().data(message),
            // 403 Forbidden is NOT an auth error — the request was
            // authenticated, but the action is not permitted (content-safety
            // blocks, ZDR-gated operations, remote-settings-blocked users).
            // Surfacing the proxy's message via internal_error keeps the
            // explanation visible to the user without triggering the client's
            // credential-required handling on -32000.
            StatusCode::FORBIDDEN => acp::Error::internal_error().data(message),
            StatusCode::BAD_REQUEST => acp::Error::invalid_params().data(message),
            StatusCode::NOT_FOUND => acp::Error::resource_not_found(None).data(message),
            StatusCode::PAYLOAD_TOO_LARGE => acp::Error::invalid_params().data(message),
            StatusCode::TOO_MANY_REQUESTS => {
                acp::Error::new(RATE_LIMITED_ERROR_CODE, "Rate limited".to_string()).data(message)
            }
            // Preserve the HTTP status in data so the classifier folds capacity
            // errors (503/529) into `rate_limit`.
            _ => acp::Error::internal_error()
                .data(error_data_with_status(message, Some(status.as_u16()))),
        },
        SamplingError::EventStreamError(message) => acp::Error::internal_error().data(message),
        SamplingError::EmptyResponse { context } => acp::Error::internal_error().data(format!(
            "empty response from model ({}): model={}, had_reasoning={}, finish_reason={}",
            context.reason,
            context.model,
            context.had_reasoning,
            context.finish_reason_str(),
        )),
        SamplingError::IdleTimeout { elapsed_secs } => acp::Error::internal_error().data(format!(
            "No response from model for {elapsed_secs}s — the model may be stuck"
        )),
        // Recovery consumes these inside the sampler's retry loop; a stray
        // terminal one still renders its labels.
        SamplingError::DoomLoopDetected { .. } => {
            acp::Error::internal_error().data(err.to_string())
        }
    }
}

pub fn error_data_with_status(message: String, http_status: Option<u16>) -> serde_json::Value {
    match http_status {
        Some(sc) => serde_json::json!({ "message": message, "http_status": sc }),
        None => serde_json::Value::String(message),
    }
}

/// Typed terminal-failure payload. Every caller records the stable error kind;
/// consumers never infer semantics from message text or a legacy shape.
pub fn terminal_error_data(
    message: String,
    http_status: Option<u16>,
    kind: &str,
) -> serde_json::Value {
    let mut data = serde_json::json!({ "message": message, "error_kind": kind });
    if let Some(sc) = http_status {
        data["http_status"] = serde_json::json!(sc);
    }
    data
}

/// Whether a terminal error is the explicit input-context exhaustion state.
pub fn context_window_exceeded_for_turn_error(err: &acp::Error) -> bool {
    err.data
        .as_ref()
        .and_then(|d| d.get("error_kind"))
        .and_then(|v| v.as_str())
        .is_some_and(|kind| kind == ::hooks::event::StopFailureKind::ContextWindowExceeded.as_str())
}

fn error_message_from_data(data: &serde_json::Value) -> serde_json::Value {
    data.get("message").cloned().unwrap_or_else(|| data.clone())
}

pub fn error_detail_from_data(data: &serde_json::Value) -> Option<String> {
    if let Some(m) = data.get("message").and_then(|v| v.as_str()) {
        return Some(m.to_owned());
    }
    if let Some(s) = data.as_str() {
        return Some(s.to_owned());
    }
    data.get("detail")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

pub fn http_status_from_error(err: &acp::Error) -> Option<u16> {
    err.data
        .as_ref()?
        .get("http_status")?
        .as_u64()
        .map(|s| s as u16)
}

const PROMPT_USAGE_DATA_KEY: &str = "promptUsage";

pub fn attach_prompt_usage(
    err: acp::Error,
    usage: Option<crate::extensions::notification::PromptUsage>,
) -> acp::Error {
    let Some(usage) = usage else {
        return err;
    };
    let Ok(usage_val) = serde_json::to_value(&usage) else {
        tracing::warn!(
            "attach_prompt_usage: failed to serialize PromptUsage; leaving error unchanged"
        );
        return err;
    };
    let mut map = match err.data.clone() {
        Some(serde_json::Value::Object(map)) => map,
        Some(serde_json::Value::String(message)) => {
            let mut m = serde_json::Map::new();
            m.insert("message".into(), serde_json::Value::String(message));
            m
        }
        Some(other) => {
            let mut m = serde_json::Map::new();
            m.insert("message".into(), other);
            m
        }
        None => {
            let mut m = serde_json::Map::new();
            m.insert(
                "message".into(),
                serde_json::Value::String(err.message.clone()),
            );
            m
        }
    };
    map.insert(PROMPT_USAGE_DATA_KEY.into(), usage_val);
    err.data(serde_json::Value::Object(map))
}

pub fn prompt_usage_from_error(
    err: &acp::Error,
) -> Option<crate::extensions::notification::PromptUsage> {
    let data = err.data.as_ref()?;
    let raw = data.get(PROMPT_USAGE_DATA_KEY)?;
    serde_json::from_value(raw.clone()).ok()
}

/// Derive `(stopReason, agentResult)` JSON values for the `prompt_complete`
/// notification from a prompt result. Rate-limit errors produce
/// `("rate_limit", null)` so the client shows its own upgrade message;
/// other errors produce `("error", <detail>)`.
pub fn prompt_complete_fields(
    result: &std::result::Result<acp::StopReason, acp::Error>,
) -> (serde_json::Value, serde_json::Value) {
    match result {
        Ok(reason) => (serde_json::json!(*reason), serde_json::Value::Null),
        Err(err) => {
            let is_rate_limit = i32::from(err.code) == RATE_LIMITED_ERROR_CODE;
            let stop = if is_rate_limit { "rate_limit" } else { "error" };
            let result = if is_rate_limit {
                serde_json::Value::Null
            } else {
                err.data
                    .as_ref()
                    .map(error_message_from_data)
                    .unwrap_or_else(|| serde_json::Value::String(err.message.clone()))
            };
            (serde_json::json!(stop), result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn attach_prompt_usage_preserves_error_kind_and_round_trips() {
        let mut ledger = chat_state::UsageLedger::default();
        ledger.record_main_loop_call(
            "m",
            &sampling_types::TokenUsage {
                prompt_tokens: 3,
                completion_tokens: 1,
                total_tokens: 4,
                reasoning_tokens: 0,
                cached_prompt_tokens: 0,
                cache_creation_prompt_tokens: 0,
            },
            None,
            Some(10),
        );
        let usage = crate::extensions::notification::PromptUsage::from(&ledger);
        let err = attach_prompt_usage(
            acp::Error::internal_error().data(terminal_error_data(
                "context remains over window".into(),
                None,
                ::hooks::event::StopFailureKind::ContextWindowExceeded.as_str(),
            )),
            Some(usage.clone()),
        );
        assert!(context_window_exceeded_for_turn_error(&err));
        let back = prompt_usage_from_error(&err).expect("usage attached");
        assert_eq!(back.totals.input_tokens, 3);
        assert_eq!(back.num_turns, 1);
    }

    #[test]
    fn attach_prompt_usage_keeps_string_message_readable() {
        let usage = crate::extensions::notification::PromptUsage {
            totals: Default::default(),
            model_usage: Default::default(),
            num_turns: 1,
            usage_is_incomplete: false,
        };
        let free = "provider:request-quota-exhausted";
        let err = attach_prompt_usage(
            acp::Error::new(RATE_LIMITED_ERROR_CODE, "Rate limited").data(free),
            Some(usage),
        );
        let msg = err
            .data
            .as_ref()
            .and_then(|d| {
                d.as_str()
                    .or_else(|| d.get("message").and_then(|m| m.as_str()))
            })
            .unwrap_or("");
        assert!(msg.contains("provider:request-quota-exhausted"));
        assert!(prompt_usage_from_error(&err).is_some());
        assert!(!err.data.as_ref().unwrap().is_string());
    }

    #[test]
    fn error_detail_from_data_reads_message_field() {
        let data = error_data_with_status("upstream unavailable".into(), Some(503));
        assert_eq!(
            error_detail_from_data(&data).as_deref(),
            Some("upstream unavailable")
        );
    }
    #[test]
    fn rate_limited_empty_detail_uses_provider_neutral_fallback() {
        assert_eq!(
            format_rate_limited_user_message(None),
            RATE_LIMITED_USER_MESSAGE
        );
        assert_eq!(
            format_rate_limited_user_message(Some("   ")),
            RATE_LIMITED_USER_MESSAGE
        );
    }

    #[test]
    fn rate_limited_nonempty_detail_is_preserved() {
        assert_eq!(
            format_rate_limited_user_message(Some("slow down")),
            "slow down"
        );
    }

    #[test]
    fn rate_limited_detail_drops_sampling_display_prefix() {
        let body = "The provider is temporarily at capacity.";
        let wire = format!("API error (status 429 Too Many Requests): {body}");
        assert_eq!(format_rate_limited_user_message(Some(&wire)), body);
    }
    #[test]
    fn overload_maps_to_display_message_without_data() {
        let err = SamplingError::from_stream_error("overloaded_error", "Overloaded");
        let acp_err = map_sampling_err_to_acp(err);
        assert_eq!(acp_err.code, acp::ErrorCode::InternalError);
        assert_eq!(acp_err.message, OVERLOADED_USER_MESSAGE);
        // Display appends JSON-encoded `data`; direct-display copy must not
        // carry any.
        assert_eq!(acp_err.data, None);

        let err_529 = SamplingError::Api {
            status: StatusCode::from_u16(529).expect("valid status"),
            message: "capacity".into(),
            model_metadata: None,
            retry_after_secs: None,
            should_retry: None,
        };
        let acp_529 = map_sampling_err_to_acp(err_529);
        assert_eq!(acp_529.message, OVERLOADED_USER_MESSAGE);
        assert_eq!(acp_529.data, None);
    }

    #[test]
    fn rate_limit_error_uses_dedicated_code() {
        let err = SamplingError::Api {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Rate limit exceeded".into(),
            model_metadata: None,
            retry_after_secs: None,
            should_retry: None,
        };
        let acp_err = map_sampling_err_to_acp(err);
        assert_eq!(acp_err.code, acp::ErrorCode::from(RATE_LIMITED_ERROR_CODE));
        assert_eq!(acp_err.message, "Rate limited");
        assert_eq!(
            acp_err.data,
            Some(serde_json::Value::String("Rate limit exceeded".into()))
        );
    }

    #[test]
    fn rate_limit_mapping_is_stable_with_retry_after() {
        let err = SamplingError::Api {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Rate limit exceeded".into(),
            model_metadata: None,
            retry_after_secs: Some(60),
            should_retry: None,
        };
        assert_eq!(err.retry_after(), Some(60));
        let acp_err = map_sampling_err_to_acp(err);
        assert_eq!(acp_err.code, acp::ErrorCode::from(RATE_LIMITED_ERROR_CODE));
        assert_eq!(acp_err.message, "Rate limited");
    }

    #[test]
    fn rate_limit_code_differs_from_internal_error() {
        let rate_err = SamplingError::Api {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "limited".into(),
            model_metadata: None,
            retry_after_secs: None,
            should_retry: None,
        };
        let server_err = SamplingError::Api {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "oops".into(),
            model_metadata: None,
            retry_after_secs: None,
            should_retry: None,
        };
        let rate_acp = map_sampling_err_to_acp(rate_err);
        let server_acp = map_sampling_err_to_acp(server_err);

        assert_eq!(rate_acp.code, acp::ErrorCode::from(RATE_LIMITED_ERROR_CODE));
        assert_ne!(rate_acp.code, server_acp.code);
        assert_eq!(server_acp.code, acp::Error::internal_error().code);
    }

    #[test]
    fn service_unavailable_retains_http_status_for_classification() {
        let err = SamplingError::Api {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "at capacity".into(),
            model_metadata: None,
            retry_after_secs: None,
            should_retry: None,
        };
        let acp_err = map_sampling_err_to_acp(err);
        assert_eq!(acp_err.code, acp::Error::internal_error().code);
        assert_eq!(http_status_from_error(&acp_err), Some(503));
    }

    #[test]
    fn auth_errors_map_to_auth_required() {
        let err = SamplingError::Api {
            status: StatusCode::UNAUTHORIZED,
            message: "bad token".into(),
            model_metadata: None,
            retry_after_secs: None,
            should_retry: None,
        };
        let acp_err = map_sampling_err_to_acp(err);
        assert_eq!(acp_err.code, acp::Error::auth_required().code);
    }

    /// Regression test: 403 Forbidden must NOT map to auth_required.
    /// The cli-chat-proxy returns 403 for policy denials that are unrelated to
    /// the caller's credentials (content-safety blocks like
    /// SAFETY_CHECK_TYPE_DATA_LEAKAGE, ZDR-gated operations, remote settings
    /// blocks). Mapping these to auth_required causes embedding clients to
    /// tear down the session even though replacing the key cannot help.
    #[test]
    fn forbidden_does_not_map_to_auth_required() {
        let err = SamplingError::Api {
            status: StatusCode::FORBIDDEN,
            message:
                "Content violates usage guidelines. Failed check: SAFETY_CHECK_TYPE_DATA_LEAKAGE"
                    .into(),
            model_metadata: None,
            retry_after_secs: None,
            should_retry: None,
        };
        let acp_err = map_sampling_err_to_acp(err);
        assert_ne!(
            acp_err.code,
            acp::Error::auth_required().code,
            "403 Forbidden must not be surfaced as auth_required"
        );
        assert_eq!(
            acp_err.data,
            Some(serde_json::Value::String(
                "Content violates usage guidelines. Failed check: SAFETY_CHECK_TYPE_DATA_LEAKAGE"
                    .into()
            ))
        );
    }

    #[test]
    fn prompt_complete_fields_ok_passes_through_stop_reason() {
        let result: std::result::Result<acp::StopReason, acp::Error> = Ok(acp::StopReason::EndTurn);
        let (stop, agent_result) = prompt_complete_fields(&result);
        assert_eq!(stop, serde_json::json!("end_turn"));
        assert_eq!(agent_result, serde_json::Value::Null);
    }

    #[test]
    fn prompt_complete_fields_rate_limit_omits_detail() {
        let err = acp::Error::new(RATE_LIMITED_ERROR_CODE, "Rate limited".to_string())
            .data("Rate limit exceeded");
        let (stop, agent_result) = prompt_complete_fields(&Err(err));
        assert_eq!(stop, serde_json::json!("rate_limit"));
        assert_eq!(agent_result, serde_json::Value::Null);
    }

    #[test]
    fn prompt_complete_fields_generic_error_includes_detail() {
        let err = acp::Error::internal_error().data("connection reset");
        let (stop, agent_result) = prompt_complete_fields(&Err(err));
        assert_eq!(stop, serde_json::json!("error"));
        assert_eq!(
            agent_result,
            serde_json::Value::String("connection reset".into())
        );
    }

    #[test]
    fn prompt_complete_fields_error_without_data_falls_back_to_message() {
        let err = acp::Error::new(-32000, "something broke".to_string());
        assert!(err.data.is_none());
        let (stop, agent_result) = prompt_complete_fields(&Err(err));
        assert_eq!(stop, serde_json::json!("error"));
        assert_eq!(
            agent_result,
            serde_json::Value::String("something broke".into())
        );
    }

    #[test]
    fn http_status_from_error_extracts_status() {
        let err = acp::Error::internal_error()
            .data(error_data_with_status("bad token".into(), Some(401)));
        assert_eq!(http_status_from_error(&err), Some(401));
    }

    /// The typed context-window kind survives the terminal ACP boundary.
    #[test]
    fn terminal_error_distinguishes_context_window_exhaustion() {
        let err = acp::Error::internal_error().data(terminal_error_data(
            "context remains over window".into(),
            None,
            ::hooks::event::StopFailureKind::ContextWindowExceeded.as_str(),
        ));
        assert!(context_window_exceeded_for_turn_error(&err));
        assert!(!context_window_exceeded_for_turn_error(
            &acp::Error::internal_error()
        ));
    }

    #[test]
    fn prompt_complete_fields_extracts_message_from_status_data() {
        let err = acp::Error::internal_error()
            .data(error_data_with_status("model not found".into(), Some(404)));
        let (stop, agent_result) = prompt_complete_fields(&Err(err));
        assert_eq!(stop, serde_json::json!("error"));
        assert_eq!(
            agent_result,
            serde_json::Value::String("model not found".into())
        );
    }
}

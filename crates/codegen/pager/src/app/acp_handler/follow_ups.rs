use super::*;

/// Max follow-up chips kept from a single (server-controlled) notification.
pub(super) const MAX_FOLLOW_UPS: usize = 6;

/// Max chars kept per (server-controlled) suggestion label.
pub(super) const MAX_FOLLOW_UP_LABEL: usize = 256;

/// Max `response_id` length accepted; longer ids are rejected (not truncated —
/// truncation could collide ids) so an oversized server id can't bloat the
/// retained `follow_up_seen` ring.
pub(super) const MAX_RESPONSE_ID_LEN: usize = 128;

/// Canonical `grow/follow_ups` payload. Session and turn identities are
/// mandatory so notifications never depend on the active view or arrival
/// ordering for routing.
#[derive(serde::Deserialize)]
pub(super) struct FollowUpsParams {
    #[serde(rename = "sessionId")]
    session_id: acp::SessionId,
    response_id: String,
    suggestions: Vec<FollowUpSuggestionParam>,
    #[serde(rename = "promptId")]
    prompt_id: String,
    /// Reserved replay marker carrier. Absent in v1 (the shell never sets
    /// it); honored from day one so future replay producers need no pager
    /// change. Parsed loosely as a JSON value to read the `"grow/replayed"`
    /// metadata key without coupling this consumer to the rest of the envelope.
    #[serde(default, rename = "_meta")]
    meta: Option<serde_json::Value>,
}

/// A single `grow/follow_ups` suggestion. Only the human-facing `label` is
/// consumed; additional wire-shape properties are
/// ignored.
#[derive(serde::Deserialize)]
pub(super) struct FollowUpSuggestionParam {
    #[serde(default)]
    label: String,
}

/// Sanitize a server-supplied suggestion label for safe chip rendering and
/// submission: drop control + bidi/format characters
/// ([`is_unsafe_display_char`](crate::render::line_utils::is_unsafe_display_char)),
/// bound the length, and trim surrounding whitespace.
pub(super) fn sanitize_suggestion(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .filter(|c| !crate::render::line_utils::is_unsafe_display_char(*c))
        .take(MAX_FOLLOW_UP_LABEL)
        .collect();
    cleaned.trim().to_owned()
}

/// Handle `grow/follow_ups` — render follow-up suggestion chips for the
/// latest assistant response.
///
/// Newest-response-wins keying lives in [`AgentView::apply_follow_ups`]. The
/// reserved `_meta["grow/replayed"] == true` marker suppresses rendering (it
/// is absent today and treated as optional). Server-controlled count and label
/// length are bounded and labels sanitized at ingestion. Malformed payloads
/// are ignored (no chip, no panic).
pub(super) fn handle_follow_ups(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    let Ok(params) = serde_json::from_str::<FollowUpsParams>(notif.params.get()) else {
        return false;
    };
    if params
        .meta
        .as_ref()
        .and_then(|m| m.get("grow/replayed"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return false;
    }
    // The response id is the newest-wins key (and is retained in the seen
    // ring); drop a notification with a missing or oversized one.
    if params.response_id.is_empty() || params.response_id.len() > MAX_RESPONSE_ID_LEN {
        return false;
    }
    // Bound count + length at ingestion (a stored cap, not just render-time).
    let suggestions: Vec<String> = params
        .suggestions
        .into_iter()
        .take(MAX_FOLLOW_UPS)
        .filter_map(|s| {
            let label = sanitize_suggestion(&s.label);
            (!label.is_empty()).then_some(label)
        })
        .collect();

    let Some(matched) = find_session_match(app, &params.session_id) else {
        return false;
    };
    if matches!(matched, SessionMatch::Child(_)) {
        return false;
    }
    let id = matched.agent_id();
    let is_active = is_matched_agent_active(app, id);
    let Some(agent) = app.agents.get_mut(&id) else {
        return false;
    };
    agent.apply_follow_ups(params.response_id, &params.prompt_id, suggestions) && is_active
}

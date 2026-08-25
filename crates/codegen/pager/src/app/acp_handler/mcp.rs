use super::*;

pub(super) fn handle_mcp_init_progress(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Payload {
        total: u32,
        connected: u32,
        session_id: String,
    }
    let Ok(payload) = serde_json::from_str::<Payload>(notif.params.get()) else {
        return false;
    };
    let Some((is_active, agent)) = mcp_target_agent(app, &payload.session_id) else {
        return false;
    };
    agent
        .session
        .update_mcp_init_progress(payload.total, payload.connected);
    is_active
}

/// Handle completion of the session-wide MCP initialization phase.
///
/// Routing rules (verified against every shell emit site):
///
/// `sessionId` is mandatory; malformed, ownerless, and child-session payloads
/// are rejected instead of being guessed from the active view.
pub(super) fn handle_mcp_initialized(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Payload {
        session_id: String,
    }
    let Ok(payload) = serde_json::from_str::<Payload>(notif.params.get()) else {
        return false;
    };
    let session_id = acp::SessionId::new(payload.session_id);
    let target: Option<(bool, AgentId)> = match find_session_match(app, &session_id) {
        // Subagent (child) sessions don't own the top-level MCP
        // modal / connecting indicator — drop them.
        Some(SessionMatch::Child(_)) => None,
        Some(matched) => {
            let id = matched.agent_id();
            Some((is_matched_agent_active(app, id), id))
        }
        None => None,
    };
    let Some((is_active, id)) = target else {
        return false;
    };

    app.agents
        .get_mut(&id)
        .is_some_and(|agent| agent.session.clear_mcp_init_progress())
        && is_active
}

/// Per-agent coalescing test for [`Effect::FetchMcpsList`].
/// An earlier approach used `matches!(e, FetchMcpsList { .. })`
/// which collapsed across agents — a pending fetch on agent A would
/// drop the push for agent B. Now we key on `agent_id` so each
/// agent's refetch is independently debounced.
pub(super) fn agent_has_pending_mcps_fetch(app: &AppView, agent_id: AgentId) -> bool {
    app.pending_effects.iter().any(|e| {
        matches!(
            e,
            Effect::FetchMcpsList { agent_id: a, .. } if *a == agent_id
        )
    })
}

/// Handle `grow/mcp/server_status`.
///
/// Routes by the notification's `sessionId` via
/// [`find_session_match`] — the matched agent's extensions modal is
/// patched in-place via [`crate::views::mcps_modal::patch_server_row`]. Catalog
/// deltas that omit a complete tool inventory schedule one owner-scoped
/// `mcp/list` refresh through the same canonical notification.
///
/// No-ops when:
/// - the `sessionId` does not match any known agent (drop),
/// - the matched agent has no extensions modal open (cheap path —
///   the next `/mcps` open will pull fresh data anyway),
/// - the modal's `mcps_data` is not yet `Loaded` (Loading / Error
///   states would produce incoherent patches; the in-flight fetch
///   will land a consistent snapshot shortly),
/// - the named server is not present in the cached `servers` vec
///   ([`patch_server_row`] silently returns).
///
/// Re-uses the shell's canonical wire types
/// ([`shell::extensions::mcp::McpServerStatusPayload`] +
/// [`shell::extensions::mcp::McpServerStatus`]) instead of
/// re-declaring a parallel pager enum. Later variants (e.g.
/// `RestartSucceeded` / `RestartFailed`) ride through automatically
/// without a pager code change.
///
/// `status` is **not** `serde(default)`; a malformed
/// payload falls into the `tracing::warn!` arm rather than silently
/// re-painting the row red.
///
/// Returns `true` (request redraw) only when the row mutation
/// happened AND the matched agent is the currently active view.
pub(super) fn handle_mcp_server_status(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    use crate::views::extensions_modal::TabDataState;
    use crate::views::mcps_modal::{McpServerDisplayStatus, McpToolDetail, patch_server_row};
    use shell::extensions::mcp::{McpServerStatus, McpServerStatusPayload, McpServerStatusReason};

    let Ok(payload) = serde_json::from_str::<McpServerStatusPayload>(notif.params.get()) else {
        tracing::warn!(
            "Failed to parse grow/mcp/server_status: {}",
            &notif.params.get()
                [..crate::render::line_utils::floor_char_boundary(notif.params.get(), 100)]
        );
        return false;
    };

    let session_id = acp::SessionId::new(payload.session_id);
    let Some(matched) = find_session_match(app, &session_id) else {
        return false;
    };
    let id = matched.agent_id();
    let is_active = is_matched_agent_active(app, id);
    let refresh_catalog = payload.tools.is_none()
        && matches!(
            payload.reason,
            McpServerStatusReason::ConfigAdded
                | McpServerStatusReason::ConfigRemoved
                | McpServerStatusReason::ConfigChanged
        );
    let modal_open = app
        .agents
        .get(&id)
        .is_some_and(|agent| agent.extensions_modal.is_some());
    let mut redraw = false;
    if refresh_catalog
        && modal_open
        && !agent_has_pending_mcps_fetch(app, id)
        && let Some(session_id) = app
            .agents
            .get(&id)
            .and_then(|agent| agent.session.session_id.clone())
    {
        app.pending_effects.push(Effect::FetchMcpsList {
            agent_id: id,
            session_id,
            cache: true,
        });
        redraw = is_active;
    }
    let Some(agent) = app.agents.get_mut(&id) else {
        return redraw;
    };
    // Cheap path: modal closed. Drop the push — the next `/mcps`
    // open will fetch a fresh full list.
    let Some(modal) = agent.extensions_modal.as_mut() else {
        return redraw;
    };
    // Cheap path: list still loading / errored. Patching would
    // produce incoherent state; the in-flight fetch will land
    // a consistent snapshot momentarily.
    let TabDataState::Loaded(ref mut servers) = modal.mcps_data else {
        return redraw;
    };
    let display_status = match payload.status {
        McpServerStatus::Ready => McpServerDisplayStatus::Ready,
        McpServerStatus::Initializing => McpServerDisplayStatus::Initializing,
        McpServerStatus::Unavailable => McpServerDisplayStatus::Unavailable,
    };
    let new_tools = payload.tools.map(|entries| {
        entries
            .into_iter()
            .map(|tool| McpToolDetail {
                name: tool.name,
                display_name: tool.display_name,
                description: tool.description,
                enabled: tool.enabled,
            })
            .collect::<Vec<_>>()
    });
    let mutated = patch_server_row(servers, &payload.name, display_status, new_tools);
    redraw || (mutated && is_active)
}

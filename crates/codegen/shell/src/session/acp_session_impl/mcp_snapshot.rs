//! MCP snapshot concern for `SessionActor`: server-snapshot refresh and
//! reminder scheduling, templated-prefix handshake waits, and tool
//! re-registration on a rebuilt bridge.

use super::*;

pub(super) const MCP_INIT_CANCELLED_CONFIG_CHANGED: &str = "config_changed";

impl McpReminderMode {
    pub(super) fn from_env() -> Self {
        match std::env::var("MCP_REMINDER_MODE")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "full" => Self::Full,
            _ => Self::Delta,
        }
    }
}

pub(super) async fn refresh_mcp_snapshot_and_schedule_reminder_with(
    tool_bridge: Arc<tools::bridge::ToolBridge>,
    mcp_state: Arc<TokioMutex<McpState>>,
    tool_metadata_snapshot: Arc<std::sync::Mutex<crate::session::tool_index::ToolMetadataSnapshot>>,
    mcp_reminder_dirty: Arc<std::sync::atomic::AtomicBool>,
    mcp_initialized: bool,
) {
    use crate::session::tool_index::{
        ServerMetadata, ToolMetadata, extract_parameter_names, split_qualified_name,
    };

    let all_defs = tool_bridge.tool_definitions().await;
    let mut seen_tools = std::collections::HashSet::new();
    let mcp_tools: Vec<ToolMetadata> = all_defs
        .iter()
        .filter(|d| d.function.name.contains("__"))
        .filter(|d| seen_tools.insert(d.function.name.clone()))
        .map(|d| {
            let (server, tool) = split_qualified_name(&d.function.name);
            ToolMetadata {
                qualified_name: d.function.name.clone(),
                server_name: server.to_string(),
                tool_name: tool.to_string(),
                description: d.function.description.clone().unwrap_or_default(),
                parameters: extract_parameter_names(&d.function.parameters),
                input_schema: d.function.parameters.clone(),
            }
        })
        .collect();
    let parent_qualified_tools = mcp_tools
        .iter()
        .map(|tool| tool.qualified_name.clone())
        .collect();
    mcp_state
        .lock()
        .await
        .publish_eligibility(parent_qualified_tools);

    let servers_with_tools: std::collections::HashSet<&str> =
        mcp_tools.iter().map(|t| t.server_name.as_str()).collect();

    let server_metadata: Vec<ServerMetadata> = {
        let mcp_state = mcp_state.lock().await;
        let mut metadata = Vec::new();
        for (name, client) in mcp_state.all_clients() {
            if servers_with_tools.contains(name.as_str()) {
                metadata.push(ServerMetadata {
                    name: name.clone(),
                    description: client.server_instructions().await,
                });
            }
        }
        metadata
    };

    let mut snapshot = tool_metadata_snapshot.lock().unwrap();
    snapshot.tools = mcp_tools;
    snapshot.servers = server_metadata;
    snapshot.mcp_initialized = mcp_initialized;
    drop(snapshot);

    mcp_reminder_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    tracing::debug!("MCP snapshot updated, reminder marked dirty");

}

impl SessionActor {
    /// Re-register MCP tools onto a freshly-built `ToolBridge` after a
    /// zero-turn harness rebuild.
    ///
    /// Snapshots the live MCP `Client` connections from `mcp_state` and
    /// (eventually) re-walks each client's `list_tools` to mirror its
    /// tool registrations onto the new bridge. Best-effort: per-server
    /// failures are logged but do not abort the rebuild.
    ///
    /// Re-register MCP tools from existing clients onto the rebuilt bridge.
    ///
    /// Iterates over all connected MCP clients, calls `list_tools` on each
    /// to obtain tool registrations, and registers them on the new bridge.
    /// Errors on individual servers are logged but don't abort the process.
    /// After re-registration, refreshes the tool metadata snapshot so
    /// `search_tool` returns accurate results.
    pub(super) async fn re_register_mcp_tools_on_rebuilt_bridge(&self) {
        // Snapshot server names + client Arcs to avoid holding the lock
        // across async list_tools calls.
        let clients: Vec<(
            String,
            std::sync::Arc<crate::session::mcp_servers::McpClient>,
        )> = {
            let st = self.mcp_state.lock().await;
            st.all_clients()
                .map(|(name, client)| (name.clone(), std::sync::Arc::clone(client)))
                .collect()
        };

        if clients.is_empty() {
            self.refresh_mcp_snapshot_and_schedule_reminder().await;
            return;
        }

        tracing::info!(
            session_id = %self.session_info.id.0,
            server_count = clients.len(),
            "re_register_mcp_tools_on_rebuilt_bridge: re-registering MCP tools from existing clients"
        );

        let mcp_state_arc = std::sync::Arc::clone(&self.mcp_state);
        let mut all_ui_tools: std::collections::HashMap<
            String,
            Vec<crate::extensions::mcp::McpToolEntry>,
        > = std::collections::HashMap::new();

        for (server_name, client) in &clients {
            let registrations = match client
                .get_tool_registrations(std::sync::Arc::clone(&mcp_state_arc))
                .await
            {
                Ok(regs) => regs,
                Err(e) => {
                    tracing::warn!(
                        session_id = %self.session_info.id.0,
                        server = %server_name,
                        error = %e,
                        "re_register_mcp_tools_on_rebuilt_bridge: failed to list tools, skipping server"
                    );
                    continue;
                }
            };

            let tool_count = registrations.len();
            let mut mcp_state = self.mcp_state.lock().await;

            for reg in registrations {
                self.register_mcp_tool(server_name, reg, &mut mcp_state, &mut all_ui_tools)
                    .await;
            }
            drop(mcp_state);

            tracing::info!(
                session_id = %self.session_info.id.0,
                server = %server_name,
                tool_count,
                "re_register_mcp_tools_on_rebuilt_bridge: re-registered tools"
            );
        }

        // Refresh the snapshot so search_tool returns accurate results
        // against the newly-registered tools.
        self.refresh_mcp_snapshot_and_schedule_reminder().await;
        self.emit_mcp_catalog_updates(all_ui_tools);
    }
}

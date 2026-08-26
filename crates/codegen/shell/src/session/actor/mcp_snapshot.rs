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
    /// Restore the last observed MCP catalog onto a replacement Agent bridge
    /// without a remote `tools/list` call. Reconstructed tools dispatch
    /// through the live shared `McpState`, so the step-control/cancellation
    /// gate contains only bounded local work. In-flight handshakes and later
    /// list-change pushes still update whichever bridge is live.
    pub(super) async fn restore_mcp_tools_from_snapshot(&self) {
        let tools = self
            .mcp
            .tool_metadata_snapshot
            .lock()
            .unwrap()
            .tools
            .clone();
        if tools.is_empty() {
            self.refresh_mcp_snapshot_and_schedule_reminder().await;
            return;
        }
        let mut all_ui_tools: std::collections::HashMap<
            String,
            Vec<crate::extensions::mcp::McpToolEntry>,
        > = std::collections::HashMap::new();

        for tool in tools {
            let meta = self
                .mcp_state
                .lock()
                .await
                .mcp_tool_meta
                .get(&tool.qualified_name)
                .cloned();
            let Some(registration) = crate::session::mcp_servers::McpTool::new(
                tool.tool_name,
                tool.description,
                tool.server_name.clone(),
                std::sync::Arc::clone(&self.mcp_state),
                tool.input_schema,
                meta,
            )
            .into_registration() else {
                continue;
            };
            let mut mcp_state = self.mcp_state.lock().await;
            self.register_mcp_tool(
                &tool.server_name,
                registration,
                &mut mcp_state,
                &mut all_ui_tools,
            )
            .await;
        }
        self.refresh_mcp_snapshot_and_schedule_reminder().await;
        self.emit_mcp_catalog_updates(all_ui_tools);
    }
}

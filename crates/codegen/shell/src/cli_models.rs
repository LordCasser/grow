//! Data API for `grow models`; clients own presentation.

use acp_transport::{AcpAgentTx, acp_send};
use agent_client_protocol as acp;
use anyhow::Result;

/// Read the session's configured model catalog over ACP.
pub async fn list_models(
    acp_tx: &AcpAgentTx,
    client_type: &str,
    client_version: &str,
) -> Result<acp::SessionModelState> {
    let init_resp: acp::InitializeResponse = acp_send(
        acp::InitializeRequest::new(acp::ProtocolVersion::V1)
            .client_capabilities(
                acp::ClientCapabilities::new()
                    .fs(acp::FileSystemCapabilities::new())
                    .terminal(false),
            )
            .meta(
                serde_json::json!({
                    "clientType": client_type,
                    "clientVersion": client_version,
                })
                .as_object()
                .cloned(),
            ),
        acp_tx,
    )
    .await?;

    let model_state = init_resp
        .meta
        .and_then(|m| m.get("modelState").cloned())
        .ok_or_else(|| anyhow::anyhow!("InitializeResponse missing modelState"))?;
    serde_json::from_value(model_state)
        .map_err(|e| anyhow::anyhow!("Failed to parse modelState: {e}"))
}

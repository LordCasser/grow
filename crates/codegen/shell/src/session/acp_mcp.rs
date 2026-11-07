//! In-process SDK MCP servers over the ACP reverse channel (`grow/mcp/sdk_call`).
//!
//! The official `agent-sdk` lets a host define in-process tools (`@tool` /
//! `create_sdk_mcp_server`). When `transport="acp"`, the SDK registers them in
//! `session/new` `_meta["grow/mcp/servers"] = [{ "name", "serverId" }]` and the agent
//! invokes their tools by sending each MCP JSON-RPC message back to the client as a
//! reverse `grow/mcp/sdk_call` request — handled here by [`GatewayAcpInvoker`].
//!
//! NOTE: the *reverse* route (agent -> client, `grow/mcp/sdk_call`) invokes a tool that
//! lives in the SDK's process. It is the zero-IPC mirror of the *forward* route (client
//! -> agent, `grow/mcp/call` in `extensions::mcp`), which invokes a tool on a server the
//! AGENT is connected to. They use distinct method strings and sit on opposite request
//! handlers, so they never collide.

use std::time::Duration;

use ::mcp::servers::AcpServerEntry;
use acp_transport::AcpAgentGatewaySender;
use acp_transport::protocol as acp;
use mcp::acp_transport::AcpReverseInvoker;
use mcp::wire;

/// Parse `_meta["grow/mcp/servers"]` into [`AcpServerEntry`] registrations. Each entry
/// is deserialized directly into the canonical type (so the `serverId` wire field is
/// serde-checked, not hand-read); entries missing `name`/`serverId` are skipped with a
/// warning. Two successfully parsed entries sharing a `name` are rejected: server names
/// are the tool namespace, so a duplicate within one client array must not silently
/// shadow (exact `McpServerName` equality; no normalization). Absent meta yields none.
pub fn parse_acp_mcp_servers(meta: Option<&acp::Meta>) -> Result<Vec<AcpServerEntry>, String> {
    let Some(array) = meta
        .and_then(|m| m.get(wire::MCP_SERVERS))
        .and_then(|v| v.as_array())
    else {
        return Ok(Vec::new());
    };
    let mut seen = std::collections::HashSet::new();
    let mut servers = Vec::new();
    for entry in array {
        let server: AcpServerEntry = match serde_json::from_value(entry.clone()) {
            Ok(server) => server,
            Err(err) => {
                tracing::warn!(entry = %entry, %err, "ignoring malformed grow/mcp/servers entry");
                continue;
            }
        };
        if !seen.insert(server.name.clone()) {
            return Err(format!(
                "duplicate grow/mcp/servers entry: two entries declare the same name {}",
                server.name
            ));
        }
        servers.push(server);
    }
    Ok(servers)
}

/// Reverse-RPC invoker for in-process SDK MCP servers.
///
/// Each [`invoke`](AcpReverseInvoker::invoke) sends one `grow/mcp/sdk_call` reverse request
/// straight through the gateway. `AcpAgentGatewaySender::send` returns a `Send` future
/// (unlike the `?Send` `acp::Client::ext_method` trait method), so the rmcp transport's
/// `Send` invoker bound is satisfied without a forwarding task. Calls are independent and may
/// run concurrently — the gateway serializes them onto the session's message channel.
///
/// The invoker is bound to the registration it was created for: the session id
/// scopes the reverse call to that session, and `leader_client_id` (when the
/// session was created through a leader) names the accepted connection that
/// declared the SDK servers. The leader routes the request to that owner ahead
/// of the session's current driver; it never selects an owner from the
/// `serverId`.
pub struct GatewayAcpInvoker {
    gateway: AcpAgentGatewaySender,
    session_id: acp::SessionId,
    leader_client_id: Option<u64>,
}

impl GatewayAcpInvoker {
    pub fn new(
        gateway: AcpAgentGatewaySender,
        session_id: acp::SessionId,
        leader_client_id: Option<u64>,
    ) -> Self {
        Self {
            gateway,
            session_id,
            leader_client_id,
        }
    }
}

/// Reverse `grow/mcp/sdk_call` params. Declares the on-wire field names once (mirrors
/// the forward side's typed `McpCallRequest`) so the `serverId` literal isn't hand-spelled.
/// `sessionId` and the leader owner describe the registration; the SDK's own
/// JSON-RPC `message` (including its id) is passed through untouched.
#[derive(serde::Serialize)]
struct SdkCallParams<'a> {
    #[serde(rename = "serverId")]
    server_id: &'a str,
    message: serde_json::Value,
    #[serde(rename = "sessionId")]
    session_id: &'a str,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    meta: Option<SdkCallMeta>,
}

#[derive(serde::Serialize)]
struct SdkCallMeta {
    #[serde(rename = "grow/leaderClientId")]
    leader_client_id: u64,
}

#[async_trait::async_trait]
impl AcpReverseInvoker for GatewayAcpInvoker {
    async fn invoke(
        &self,
        server_id: &str,
        message: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        let params = serde_json::value::to_raw_value(&SdkCallParams {
            server_id,
            message,
            session_id: self.session_id.0.as_ref(),
            meta: self
                .leader_client_id
                .map(|leader_client_id| SdkCallMeta { leader_client_id }),
        })
        .map_err(|err| err.to_string())?;
        let request = acp::ExtRequest::new(wire::MCP_SDK_CALL, params.into());
        // Bound the round trip so a missing or hung client fails this reverse call at
        // the configured per-server tool timeout rather than stalling the tool loop.
        let response = tokio::time::timeout(timeout, self.gateway.send(request))
            .await
            .map_err(|_| {
                format!(
                    "{} to server {server_id} timed out after {}ms",
                    wire::MCP_SDK_CALL,
                    timeout.as_millis()
                )
            })?
            .map_err(|err| err.to_string())?;
        serde_json::from_str(response.0.get()).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Client end of a real ACP gateway: records each reverse `_grow/mcp/sdk_call`
    /// request and answers it with a canned JSON-RPC response.
    struct RecordingSdkClient {
        requests: Rc<RefCell<Vec<acp::ExtRequest>>>,
        response: serde_json::Value,
    }

    #[async_trait::async_trait(?Send)]
    impl acp_transport::AcpClientHandler for RecordingSdkClient {
        async fn request_permission(
            &self,
            _: acp::RequestPermissionRequest,
        ) -> acp::Result<acp::RequestPermissionResponse> {
            Err(acp::Error::internal_error().data("unexpected permission request in SDK fixture"))
        }

        async fn session_notification(&self, _: acp::SessionNotification) -> acp::Result<()> {
            Ok(())
        }

        async fn ext_method(&self, args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
            self.requests.borrow_mut().push(args);
            let raw = serde_json::value::to_raw_value(&self.response)
                .expect("canned response serializes");
            Ok(acp::ExtResponse::new(raw.into()))
        }
    }

    /// The real producer bound to one registration, exactly as the session actor
    /// builds it. `None` models standalone ACP (no leader owner).
    fn sdk_invoker(
        gateway: AcpAgentGatewaySender,
        leader_client_id: Option<u64>,
    ) -> GatewayAcpInvoker {
        GatewayAcpInvoker::new(
            gateway,
            acp::SessionId::new("sess-sdk-owner"),
            leader_client_id,
        )
    }

    /// The wire params of a real reverse request must carry the registration
    /// session and leader owner, while the SDK's own JSON-RPC message (including
    /// its id) and the parsed response stay untouched.
    #[tokio::test(flavor = "current_thread")]
    async fn producer_carries_registration_session_owner_and_keeps_message() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let requests = Rc::new(RefCell::new(Vec::new()));
                let canned = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 41,
                    "result": { "content": [{ "type": "text", "text": "pong" }] },
                });
                let (gateway, receiver) =
                    acp_transport::acp_gateway::<acp::AgentSide, _>(RecordingSdkClient {
                        requests: requests.clone(),
                        response: canned.clone(),
                    });
                tokio::task::spawn_local(receiver.run());

                let message = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 41,
                    "method": "tools/call",
                    "params": { "name": "ping", "arguments": {} },
                });
                let invoker = sdk_invoker(gateway, Some(7));
                let response = invoker
                    .invoke("srv-registered", message.clone(), Duration::from_secs(5))
                    .await
                    .expect("reverse round trip succeeds");

                assert_eq!(response, canned, "SDK response must be returned verbatim");
                let requests = requests.borrow();
                assert_eq!(requests.len(), 1, "exactly one reverse request");
                assert_eq!(&*requests[0].method, wire::MCP_SDK_CALL);
                let params: serde_json::Value =
                    serde_json::from_str(requests[0].params.get()).expect("params are JSON");
                assert_eq!(params["serverId"], "srv-registered");
                assert_eq!(
                    params["message"], message,
                    "SDK message value must be unchanged"
                );
                assert_eq!(
                    params["sessionId"], "sess-sdk-owner",
                    "reverse call must carry the registering session"
                );
                assert_eq!(
                    params["_meta"]["grow/leaderClientId"], 7,
                    "reverse call must carry the leader-assigned registration owner"
                );
            })
            .await;
    }

    /// Standalone ACP has no leader owner: the reverse call still names the
    /// registering session but must not invent a `_meta` target, and the SDK
    /// message/response stay unchanged.
    #[tokio::test(flavor = "current_thread")]
    async fn standalone_producer_omits_leader_meta_and_keeps_message() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let requests = Rc::new(RefCell::new(Vec::new()));
                let canned = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 42,
                    "result": { "tools": [] },
                });
                let (gateway, receiver) =
                    acp_transport::acp_gateway::<acp::AgentSide, _>(RecordingSdkClient {
                        requests: requests.clone(),
                        response: canned.clone(),
                    });
                tokio::task::spawn_local(receiver.run());

                let message = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 42,
                    "method": "tools/list",
                    "params": {},
                });
                let response = sdk_invoker(gateway, None)
                    .invoke("srv-standalone", message.clone(), Duration::from_secs(5))
                    .await
                    .expect("standalone reverse round trip succeeds");

                assert_eq!(response, canned, "SDK response must be returned verbatim");
                let requests = requests.borrow();
                assert_eq!(requests.len(), 1, "exactly one reverse request");
                let params: serde_json::Value =
                    serde_json::from_str(requests[0].params.get()).expect("params are JSON");
                assert_eq!(params["serverId"], "srv-standalone");
                assert_eq!(params["sessionId"], "sess-sdk-owner");
                assert_eq!(
                    params["message"], message,
                    "SDK message value must be unchanged"
                );
                assert!(
                    params.get("_meta").is_none(),
                    "standalone registration must not invent a leader owner"
                );
            })
            .await;
    }

    #[test]
    fn parses_valid_entries_and_skips_malformed() {
        let meta = serde_json::json!({
            "grow/mcp/servers": [
                { "name": "harness-tools", "serverId": "srv_0" },
                { "name": "missing-id" },
                { "serverId": "no_name" },
            ]
        });
        let servers = parse_acp_mcp_servers(meta.as_object()).expect("valid meta parses");
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "harness-tools");
        assert_eq!(servers[0].server_id, "srv_0");
    }

    #[test]
    fn duplicate_parsed_names_are_rejected() {
        let meta = serde_json::json!({
            "grow/mcp/servers": [
                { "name": "tools", "serverId": "srv_0" },
                { "name": "tools", "serverId": "srv_1" },
            ]
        });
        let err =
            parse_acp_mcp_servers(meta.as_object()).expect_err("duplicate parsed names must error");
        assert!(err.contains("duplicate"), "unexpected error: {err}");
        assert!(err.contains("tools"), "unexpected error: {err}");
    }

    #[test]
    fn malformed_duplicate_entries_are_skipped_not_rejected() {
        // Malformed entries keep the warning-and-skip policy: two entries that fail
        // to parse (or a malformed repeat) never form a duplicate identity.
        let meta = serde_json::json!({
            "grow/mcp/servers": [
                { "name": "tools", "serverId": "srv_0" },
                { "name": "tools" },
            ]
        });
        let servers = parse_acp_mcp_servers(meta.as_object())
            .expect("malformed repeat must be skipped, not rejected");
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].server_id, "srv_0");
    }

    #[test]
    fn absent_meta_yields_none() {
        assert!(
            parse_acp_mcp_servers(None)
                .expect("absent meta is Ok")
                .is_empty()
        );
        assert!(
            parse_acp_mcp_servers(serde_json::json!({}).as_object())
                .expect("empty meta is Ok")
                .is_empty()
        );
    }
}

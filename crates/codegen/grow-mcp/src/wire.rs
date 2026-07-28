//! Single source of truth for the `grow/mcp/*` ACP wire strings.
//!
//! These method/`_meta` keys are part of the cross-language MCP-over-ACP
//! protocol the SDK speaks (mirrors the SDK's `_mcp_wire.py` / `mcpWire.ts`).
//! Reference these constants instead of re-typing the literals so the agent and
//! SDK can't drift apart.

/// Forward tool-invocation method (client -> agent): `grow/mcp/call`.
///
/// The pager/client asks the agent to invoke an MCP tool on a server the agent is
/// connected to, outside the LLM loop. See `extensions::mcp::handle_call`.
pub const MCP_CALL: &str = "grow/mcp/call";

/// Reverse zero-IPC tool-invocation method (agent -> client): `grow/mcp/sdk_call`.
///
/// The agent invokes a tool that lives in the SDK's in-process MCP server by sending
/// the MCP JSON-RPC message back to the client over the ACP reverse channel. Distinct
/// from [`MCP_CALL`] so the two disjoint schemas don't share a method string for
/// metrics/tracing. See the agent-side ACP invoker that handles this method.
pub const MCP_SDK_CALL: &str = "grow/mcp/sdk_call";

/// `session/new` `_meta` key listing in-process SDK MCP servers: `grow/mcp/servers`.
pub const MCP_SERVERS: &str = "grow/mcp/servers";

/// `initialize` `_meta` capability flag advertising in-process SDK MCP support
/// (enables the SDK's `transport="acp"`): `grow/mcp/sdk`.
pub const MCP_SDK: &str = "grow/mcp/sdk";

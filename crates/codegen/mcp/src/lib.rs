//! MCP integration crate.
//!
//! Two responsibilities:
//!
//! 1. **Owns `rmcp` 2.2 integration.** Consumers reach `rmcp` model types
//!    through this namespace (`mcp::rmcp::*`).
//!
//! 2. **Owns MCP-specific integration code**:
//!    - [`servers`] -- MCP transport layer (rmcp's `StreamableHttpClientTransport`
//!      and `TokioChildProcess`) plus client lifecycle, tool invocation, error
//!      classification, and managed-MCP refresh.
//!    - [`mcp_http_client`] -- backoff wrapper around the HTTP client handed to
//!      rmcp's streamable-HTTP transport (works around rmcp's zero-backoff SSE
//!      reconnect loop).

pub use rmcp;

pub mod acp_transport;
pub mod liveness;
pub mod mcp_http_client;
pub mod servers;
pub mod wire;

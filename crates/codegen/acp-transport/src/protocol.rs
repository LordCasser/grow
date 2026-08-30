//! Stable ACP v1 surface used by Grow.
//!
//! The upstream SDK 2.x deliberately keeps protocol schema types under
//! `schema::v1`.  Grow imports them through this module so SDK connection
//! mechanics do not leak through every crate.

pub use agent_client_protocol::schema::v1::*;
pub use agent_client_protocol::schema::{
    IntoMaybeUndefined, IntoOption, MaybeUndefined, ProtocolVersion,
};
pub use agent_client_protocol::{
    ByteStreams, ConnectionTo, JsonRpcNotification, JsonRpcRequest, RequestCancellation, Responder,
    SentRequest, TransportFrame,
};

/// Marker for the agent's view of a v1 connection.
#[derive(Debug, Clone, Copy)]
pub struct AgentSide;

/// Marker for the client's view of a v1 connection.
#[derive(Debug, Clone, Copy)]
pub struct ClientSide;

mod channel;
mod common;
mod connection;
mod gateway;
mod handler;
mod line_reader;
mod message;
pub mod protocol;
mod stdin_reader;

pub use self::{
    channel::{AcpAgentChannel, AcpChannel, AcpClientChannel, acp_channels, acp_send},
    common::{
        AcpAgentRx, AcpAgentTx, AcpChannelFailure, AcpClientRx, AcpClientTx, AcpResult, AcpRxo,
        AcpTxo, acp_channel_failure, acp_internal_error,
    },
    connection::{AgentSideConnection, ClientSideConnection, connect_agent_v1, connect_client_v1},
    gateway::{
        AcpAgentGatewayReceiver, AcpAgentGatewaySender, AcpClientGatewayReceiver,
        AcpClientGatewaySender, AcpGatewayReceiver, AcpGatewaySender, acp_gateway,
    },
    handler::{AcpAgentHandler, AcpClientHandler},
    message::{
        AcpAgentMessage, AcpAgentMessageBox, AcpAgentMessageGeneric, AcpArgs, AcpArgsBox,
        AcpClientMessage, AcpClientMessageBox, AcpClientMessageGeneric, AcpMethod, AcpRequest,
        AcpSide, Boxed, StorageMarker, Unboxed,
    },
};

pub use self::line_reader::LineBufferedRead;
pub use self::stdin_reader::spawn_stdin_line_reader;

#[doc(hidden)]
pub use self::common::compact_json;

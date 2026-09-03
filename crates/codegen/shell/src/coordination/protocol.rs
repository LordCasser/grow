use serde::{Deserialize, Serialize};

use super::inquiry::{InquiryOutcome, InquiryPhase};
use super::manifest::PeerDescription;

// Grow's private IPC version, unrelated to ACP's stable v1 wire schema.
pub(crate) const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClientHello {
    pub protocol_version: u32,
    pub peer_id: String,
    pub incarnation: String,
    pub bearer_token: String,
    pub source_session_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerHello {
    pub protocol_version: u32,
    pub peer_id: String,
    pub incarnation: String,
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Request {
    Ping,
    Describe,
    Ask {
        inquiry_id: String,
        target_session_id: String,
        question: String,
    },
    Cancel {
        inquiry_id: String,
        target_session_id: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Response {
    Pong,
    Description {
        peer: PeerDescription,
    },
    Progress {
        inquiry_id: String,
        phase: InquiryPhase,
    },
    Inquiry {
        outcome: InquiryOutcome,
    },
    Cancellation {
        inquiry_id: String,
        accepted: bool,
    },
    Error {
        code: String,
        message: String,
    },
}

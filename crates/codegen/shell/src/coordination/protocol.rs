use serde::{Deserialize, Serialize};

use super::manifest::PeerDescription;

pub(crate) const PROTOCOL_VERSION: u32 = 1;

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
    Description { peer: PeerDescription },
    Error { code: String, message: String },
}

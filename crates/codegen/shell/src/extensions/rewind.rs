//! `grow/rewind/*` extension handlers.
//!
//! - `rewind/execute`: rewind a session to a target prompt index, optionally
//!   forcing past in-flight prompts and choosing a `RewindMode`.
//! - `rewind/points`: list the prompt indices that can be rewound to.
//!
//! Local mode dispatches [`handle`]. In gateway-bridge mode the agent's
//! routing hook calls [`handle_bridge`], which composes the server's
//! conversation rewind with the local file half so the pager-facing ACP
//! response is identical either way.
use super::{ExtResult, parse_params, to_raw_response};
use crate::agent::MvpAgent;
use crate::session::{RewindMode, RewindRequest, SessionCommand};
use acp_transport::protocol as acp;
use serde::Deserialize;
use tokio::sync::oneshot;
#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    tracing::info!("handling rewind request: {}", args.method);
    match args.method.as_ref() {
        "grow/rewind/execute" => handle_execute(agent, args).await,
        "grow/rewind/points" => handle_points(agent, args).await,
        _ => Err(acp::Error::method_not_found()),
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RewindSessionRequest {
    session_id: String,
    #[serde(default)]
    target_prompt_index: Option<usize>,
    #[serde(default)]
    target_response_id: Option<String>,
    force: bool,
    mode: RewindMode,
}
impl RewindSessionRequest {
    fn prompt_index_for_local(&self) -> Result<usize, acp::Error> {
        if let Some(idx) = self.target_prompt_index {
            return Ok(idx);
        }
        if response_id_from_req(self).is_some() {
            return Err(
                acp::Error::invalid_params()
                    .data(
                        "targetResponseId rewind requires a chat/bridge session (use targetPromptIndex for local)",
                    ),
            );
        }
        Err(acp::Error::invalid_params().data("targetPromptIndex or targetResponseId is required"))
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RewindPointsRequest {
    session_id: String,
}
/// Resolve a primary session through the same reconnect/load barrier as other
/// user-facing session controls. A bare roster lookup can race the replayed
/// `session/load` and report a false `session not found`.
async fn lookup_session(
    agent: &MvpAgent,
    session_id: String,
) -> Result<crate::session::handle::SessionHandle, acp::Error> {
    agent
        .session_handle_waiting_for_load(&acp::SessionId::new(session_id))
        .await
        .ok_or_else(|| acp::Error::resource_not_found(Some("session not found".into())))
}
async fn handle_execute(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let request: RewindSessionRequest = parse_params(args)?;
    let target_prompt_index = request.prompt_index_for_local()?;
    let handle = lookup_session(agent, request.session_id).await?;
    let (tx, rx) = oneshot::channel();
    handle
        .cmd_tx
        .send(SessionCommand::Rewind {
            request: RewindRequest {
                target_prompt_index,
                force: request.force,
                mode: request.mode,
            },
            respond_to: tx,
        })
        .map_err(|_| acp::Error::internal_error().data("failed to send rewind command"))?;
    let result = rx
        .await
        .map_err(|_| acp::Error::internal_error().data("session failed to respond"))?
        .map_err(|e| acp::Error::internal_error().data(format!("Rewind failed: {:?}", e)))?;
    to_raw_response(&result)
}
async fn handle_points(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let request: RewindPointsRequest = parse_params(args)?;
    let handle = lookup_session(agent, request.session_id).await?;
    let (tx, rx) = oneshot::channel();
    handle
        .cmd_tx
        .send(SessionCommand::GetRewindPoints { respond_to: tx })
        .map_err(|_| acp::Error::internal_error().data("failed to send command"))?;
    let result = rx
        .await
        .map_err(|_| acp::Error::internal_error().data("session failed to respond"))?;
    to_raw_response(&result)
}
fn response_id_from_req(req: &RewindSessionRequest) -> Option<&str> {
    req.target_response_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

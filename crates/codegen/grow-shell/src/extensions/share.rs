//! `grow/share_session` extension handler.
//!
//! Builds the local session snapshot needed by a future share service. Grow
//! does not ship a network share backend.

use agent_client_protocol as acp;

use super::{ExtResult, parse_params};
use crate::agent::MvpAgent;
use crate::session::export::ExportedSession;
use crate::session::info::Info as SessionInfo;
use crate::session::persistence::list_summaries;
use crate::session::share::ShareSessionRequest;

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(_agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "grow/share_session" => {
            tracing::info!("handling share session request");
            handle_share_session(args).await
        }
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn handle_share_session(args: &acp::ExtRequest) -> ExtResult {
    let request: ShareSessionRequest = parse_params(args)?;

    // Find session info by searching through summaries
    let summaries = list_summaries(None).await.map_err(|e| {
        acp::Error::internal_error().data(format!("Failed to list sessions: {}", e))
    })?;

    let summary = summaries
        .iter()
        .find(|s| s.info.id.0.as_ref() == request.session_id.as_str())
        .ok_or_else(|| acp::Error::resource_not_found(Some("Session not found".into())))?;

    let info = SessionInfo {
        id: acp::SessionId::new(request.session_id.clone()),
        cwd: summary.info.cwd.clone(),
    };

    // Load and export session
    let exported = ExportedSession::from_local_session(&info)
        .await
        .map_err(|e| acp::Error::internal_error().data(format!("Failed to load session: {}", e)))?;

    // Check for empty session
    if exported.messages.is_empty() {
        return Err(acp::Error::invalid_params().data("No messages to share yet"));
    }

    tracing::info!(
        session_id = %request.session_id,
        message_count = exported.messages.len(),
        "local share snapshot prepared"
    );
    Err(acp::Error::invalid_params().data("Share service is not configured"))
}

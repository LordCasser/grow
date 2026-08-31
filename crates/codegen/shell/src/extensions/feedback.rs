//! `grow/btw` and local `grow/review/*` extension handlers.

use std::sync::Arc;

use acp_transport::protocol as acp;
use tokio::sync::oneshot;

use super::{ExtResult, parse_params};
use crate::agent::MvpAgent;
use crate::session::{
    CommentDeleteRequest, CommentDeleteResponse, CommentRequest, CommentResponse, SessionCommand,
    SideQuestionError,
};

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "grow/btw" => handle_btw(agent, args).await,
        method if method.starts_with("grow/review") => handle_review(args).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn handle_btw(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BtwRequest {
        session_id: String,
        question: String,
    }

    let req: BtwRequest = parse_params(args)?;
    let sid: acp::SessionId = req.session_id.clone().into();
    let session = agent.sessions.borrow().get(&sid).cloned().ok_or_else(|| {
        acp::Error::invalid_params().data(format!("session not found: {}", req.session_id))
    })?;
    let (tx, rx) = oneshot::channel();
    let _ = session.cmd_tx.send(SessionCommand::SideQuestion {
        question: req.question,
        respond_to: tx,
    });
    match rx
        .await
        .map_err(|_| acp::Error::internal_error().data("session failed to respond"))?
    {
        Ok(answer) => super::to_ext_response(Ok(serde_json::json!({ "answer": answer }))),
        // Model errors take the canonical mapping: overload gets its short
        // display copy there, rate limits keep the typed code + upgrade
        // copy, auth failures surface as auth_required.
        Err(SideQuestionError::Sampling(e)) => {
            Err(crate::sampling::error::map_sampling_err_to_acp(e))
        }
        // Non-model failures are already readable sentences. Set `message`
        // and leave `data` unset — `Display` appends JSON-encoded `data`,
        // and `internal_error().data(e)` rendered as `Internal error: "…"`,
        // which made capacity failures look like client bugs in the TUI.
        Err(e) => Err(acp::Error::new(
            acp::ErrorCode::InternalError.into(),
            e.to_string(),
        )),
    }
}

async fn handle_review(args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "grow/review/comment" => {
            let request: CommentRequest = parse_params(args)?;
            let comment_id = uuid::Uuid::now_v7().to_string();
            tracing::info!(
                comment_id = %comment_id,
                session_id = %request.session_id,
                prompt_index = request.prompt_index,
                path = %request.citation.path,
                start_line = request.citation.start_line,
                end_line = request.citation.end_line,
                "review comment recorded locally"
            );
            raw_response(CommentResponse {
                comment_id,
                recorded: true,
            })
        }
        "grow/review/comment/delete" => {
            let request: CommentDeleteRequest = parse_params(args)?;
            tracing::info!(
                comment_id = %request.comment_id,
                session_id = %request.session_id,
                "review comment deletion recorded locally"
            );
            raw_response(CommentDeleteResponse {
                comment_id: request.comment_id,
                deleted: true,
            })
        }
        _ => Err(acp::Error::method_not_found()),
    }
}

fn raw_response(value: impl serde::Serialize) -> ExtResult {
    serde_json::to_value(value)
        .and_then(|value| serde_json::value::to_raw_value(&value))
        .map(|value| acp::ExtResponse::new(Arc::from(value)))
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))
}

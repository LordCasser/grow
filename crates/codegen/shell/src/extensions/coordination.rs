use crate::agent::MvpAgent;
use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};

use super::{ExtResult, parse_params, to_raw_response};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListRequest {
    source_session_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse {
    sessions: Vec<crate::coordination::DiscoveredSession>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AskRequest {
    inquiry_id: String,
    source_session_id: String,
    target_session_id: String,
    question: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelRequest {
    inquiry_id: String,
    source_session_id: String,
    target_session_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CancelResponse {
    inquiry_id: String,
    cancelled: bool,
}

pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "grow/coordination/list" => {
            let request: ListRequest = parse_params(args)?;
            let sessions = agent
                .list_coordination_sessions(&request.source_session_id)
                .await
                .map_err(coordination_error)?;
            to_raw_response(&ListResponse { sessions })
        }
        "grow/coordination/ask" => {
            let request: AskRequest = parse_params(args)?;
            agent
                .validate_coordination_source(&request.source_session_id)
                .map_err(coordination_error)?;
            let inquiry_id = request.inquiry_id.clone();
            let outcome = agent
                .ask_coordination_session(
                    request.inquiry_id,
                    request.source_session_id,
                    request.target_session_id,
                    request.question,
                    None,
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .unwrap_or_else(|error| {
                    crate::coordination::InquiryOutcome::terminal(
                        inquiry_id,
                        crate::coordination::InquiryStatus::Failed,
                        error,
                    )
                });
            to_raw_response(&outcome)
        }
        "grow/coordination/cancel" => {
            let request: CancelRequest = parse_params(args)?;
            let cancelled = agent
                .cancel_coordination_session(
                    &request.inquiry_id,
                    &request.source_session_id,
                    &request.target_session_id,
                )
                .await
                .map_err(coordination_error)?;
            to_raw_response(&CancelResponse {
                inquiry_id: request.inquiry_id,
                cancelled,
            })
        }
        _ => Err(acp::Error::method_not_found()),
    }
}

fn coordination_error(error: String) -> acp::Error {
    acp::Error::invalid_params().data(error)
}

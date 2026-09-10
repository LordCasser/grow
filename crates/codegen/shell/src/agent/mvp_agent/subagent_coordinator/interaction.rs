use crate::coordination::*;
use crate::session::{SessionCommand, SessionHandle};
use tools::implementations::grow_build::task::interaction::{
    AgentInteraction, AgentInteractionOutput, AgentInteractionRequest,
};

pub(super) async fn run(
    request: AgentInteractionRequest,
    source: SessionHandle,
    target: SessionHandle,
) {
    let result = match &request.action {
        AgentInteraction::Ask { question } => ask(&request, &source, &target, question).await,
        AgentInteraction::Send { message, interrupt } => {
            let (respond_to, response) = tokio::sync::oneshot::channel();
            if target
                .cmd_tx
                .send(SessionCommand::ReceiveParentMessage {
                    source_session_id: request.source_session_id.clone(),
                    message_id: request.id.clone(),
                    message: message.clone(),
                    interrupt: *interrupt,
                    respond_to,
                })
                .is_err()
            {
                Err("Child session closed".into())
            } else {
                tokio::select! {
                    biased;
                    _ = request.cancellation.cancelled() => Err("Message cancelled; receipt may already be durable".into()),
                    result = tokio::time::timeout(std::time::Duration::from_secs(30), response) => {
                        match result {
                            Ok(Ok(Ok(_))) => Ok(AgentInteractionOutput { id: request.id.clone(), status: "received".into(), answer: None, error: None }),
                            Ok(Ok(Err(error))) => Err(error),
                            _ => Err("Message receipt acknowledgement unavailable; delivery is unknown".into()),
                        }
                    }
                }
            }
        }
    };
    let _ = request.respond_to.send(result);
}

async fn ask(
    request: &AgentInteractionRequest,
    source: &SessionHandle,
    target: &SessionHandle,
    question: &str,
) -> Result<AgentInteractionOutput, String> {
    let record = super::super::coordination::record_coordination_inquiry;
    let id = format!("{}:{}", request.source_session_id, request.id);
    let target_id = target.info.id.to_string();
    record(
        source,
        InquiryEvent::OutgoingStarted {
            inquiry_id: id.clone(),
            target_session_id: target_id.clone(),
            question: question.into(),
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    let cancellation = InquiryCancellation::new();
    let (progress, _phases) = tokio::sync::watch::channel(InquiryPhase::Receiving);
    let (respond_to, response) = tokio::sync::oneshot::channel();
    let inquiry = InboundInquiry {
        authority: InquiryAuthority::Delegation,
        inquiry_id: id.clone(),
        source_peer_id: "local-delegation".into(),
        source_session_id: source.info.id.to_string(),
        source_cwd: source.info.cwd.clone(),
        target_session_id: target_id.clone(),
        question: question.into(),
        cancellation: cancellation.clone(),
        progress,
        respond_to,
    };
    let outcome = if target
        .cmd_tx
        .send(SessionCommand::RunCoordinationInquiry { inquiry })
        .is_err()
    {
        InquiryOutcome::terminal(&id, InquiryStatus::Unavailable, "Target agent closed")
    } else {
        tokio::select! {
            biased;
            _ = request.cancellation.cancelled() => {
                cancellation.cancel(InquiryCancellationReason::Explicit);
                cancellation.outcome(&id)
            }
            result = tokio::time::timeout(INQUIRY_DEADLINE, response) => match result {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(_)) => InquiryOutcome::terminal(&id, InquiryStatus::Unavailable, "Target agent closed before answering"),
                Err(_) => {
                    cancellation.cancel(InquiryCancellationReason::TimedOut);
                    cancellation.outcome(&id)
                }
            }
        }
    };
    record(
        source,
        InquiryEvent::OutgoingCompleted {
            audit: InquiryAudit {
                target_session_id: target_id,
                question: question.into(),
                outcome: outcome.clone(),
            },
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(AgentInteractionOutput {
        id,
        status: serde_json::to_value(outcome.status)
            .unwrap()
            .as_str()
            .unwrap()
            .into(),
        answer: outcome.answer,
        error: outcome.error.map(|error| error.to_string()),
    })
}

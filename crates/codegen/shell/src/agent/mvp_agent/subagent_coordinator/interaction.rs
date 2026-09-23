use crate::coordination::*;
use crate::session::{SessionCommand, SessionHandle};
use tools::implementations::grow_build::task::interaction::{
    AgentInteraction, AgentInteractionOutput, AgentInteractionRequest,
};

pub(super) async fn run(
    request: AgentInteractionRequest,
    source: SessionHandle,
    target: Option<SessionHandle>,
    target_id: String,
    subagent_task_name: String,
) {
    let result = match &request.action {
        AgentInteraction::Ask { question } => match &target {
            Some(target) => ask(&request, &source, target, question, &subagent_task_name).await,
            None => Err("Target agent is no longer running".into()),
        },
        AgentInteraction::Send {
            message,
            interrupt,
            reply_to,
        } => Ok(send(
            &request,
            &source,
            target.as_ref(),
            &target_id,
            message,
            *interrupt,
            reply_to.as_ref(),
        )
        .await),
    };
    let result = result.map(|mut output| {
        output.subagent_task_name = Some(subagent_task_name);
        output.target_session_id = Some(target_id);
        output
    });
    let _ = request.respond_to.send(result);
}

fn delivery_output(
    id: &str,
    result: Result<String, crate::session::notification_inbox::AgentMessageDeliveryError>,
) -> AgentInteractionOutput {
    use crate::session::notification_inbox::AgentMessageDeliveryError;
    match result {
        Ok(receipt) => AgentInteractionOutput::message_received(id, receipt),
        Err(AgentMessageDeliveryError::Rejected { code, message }) => {
            AgentInteractionOutput::message_rejected(id, code, message)
        }
        Err(AgentMessageDeliveryError::Unconfirmed { code, message }) => {
            AgentInteractionOutput::message_unconfirmed(id, code, message)
        }
    }
}

async fn send(
    request: &AgentInteractionRequest,
    source: &SessionHandle,
    target: Option<&SessionHandle>,
    target_id: &str,
    message: &str,
    interrupt: bool,
    reply_to: Option<&sampling_types::AgentMessageRef>,
) -> AgentInteractionOutput {
    if let Some(reference) = reply_to {
        if interrupt || reference.source_session_id != target_id {
            return AgentInteractionOutput::message_rejected(
                &request.id,
                "invalid_reply",
                "A reply cannot interrupt or change its recipient",
            );
        }
        let Some(receipts) = source.chat_state_handle.parent_message_receipts().await else {
            return AgentInteractionOutput::message_unconfirmed(
                &request.id,
                "reply_history_unavailable",
                "Reply authorization could not be verified",
            );
        };
        if !authorizes_reply(&receipts, &request.source_session_id, reference) {
            return AgentInteractionOutput::message_rejected(
                &request.id,
                "reply_not_received",
                "Replies require a message actually received by this session",
            );
        }
    }
    let notification_source = match reply_to {
        Some(reference) => chat_state::NotificationSource::AgentReply {
            source_session_id: request.source_session_id.clone(),
            message_id: request.id.clone(),
            reply_to: reference.clone(),
        },
        None => chat_state::NotificationSource::ParentMessage {
            parent_session_id: request.source_session_id.clone(),
            message_id: request.id.clone(),
            interrupt,
        },
    };
    let Some(target) = target else {
        return confirm_closed_target(request, target_id, notification_source, message.to_owned())
            .await;
    };
    if request.receipt_only || request.cancellation.is_cancelled() {
        let receipts = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            target.chat_state_handle.parent_message_receipts(),
        )
        .await;
        match receipts {
            Ok(Some(receipts)) => match crate::session::notification_inbox::agent_message_receipt(
                &receipts,
                target_id,
                &notification_source,
                message,
            ) {
                Ok(Some(id)) => return AgentInteractionOutput::message_received(&request.id, id),
                Err(error) => return delivery_output(&request.id, Err(error)),
                Ok(None) => {}
            },
            _ => {
                return AgentInteractionOutput::message_unconfirmed(
                    &request.id,
                    "receipt_unavailable",
                    "Receipt history is unavailable",
                );
            }
        }
        return AgentInteractionOutput::message_rejected(
            &request.id,
            if request.receipt_only {
                "target_inactive"
            } else {
                "cancelled_before_dispatch"
            },
            "No existing receipt; this route cannot admit a new message",
        );
    }
    let (respond_to, mut response) = tokio::sync::oneshot::channel();
    if target
        .cmd_tx
        .send(SessionCommand::ReceiveAgentMessage {
            source_session_id: request.source_session_id.clone(),
            message_id: request.id.clone(),
            message: message.to_owned(),
            interrupt,
            reply_to: reply_to.cloned(),
            respond_to,
        })
        .is_err()
    {
        return confirm_closed_target(request, target_id, notification_source, message.to_owned())
            .await;
    }
    let result = wait_for_message_ack(&mut response, &request.cancellation).await;
    if let Some(result) = result {
        return delivery_output(&request.id, result);
    }
    // One bounded read can recover an ACK lost after commit without waking the target.
    if let Ok(Some(receipts)) = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        target.chat_state_handle.parent_message_receipts(),
    )
    .await
    {
        match crate::session::notification_inbox::agent_message_receipt(
            &receipts,
            target_id,
            &notification_source,
            message,
        ) {
            Ok(Some(id)) => return AgentInteractionOutput::message_received(&request.id, id),
            Err(error) => return delivery_output(&request.id, Err(error)),
            Ok(None) => {}
        }
    }
    AgentInteractionOutput::message_unconfirmed(
        &request.id,
        "acknowledgement_unavailable",
        "Receipt could not be confirmed; the target may have accepted the message",
    )
}

async fn wait_for_message_ack(
    response: &mut tokio::sync::oneshot::Receiver<
        Result<String, crate::session::notification_inbox::AgentMessageDeliveryError>,
    >,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Option<Result<String, crate::session::notification_inbox::AgentMessageDeliveryError>> {
    // A ready durable ACK wins a simultaneous cancellation. Neither path resends.
    tokio::select! {
        biased;
        result = response => result.ok(),
        _ = cancellation.cancelled() => None,
        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => None,
    }
}

fn authorizes_reply(
    receipts: &[chat_state::TimelineEvent],
    source: &str,
    reference: &sampling_types::AgentMessageRef,
) -> bool {
    receipts.iter().any(|event| matches!(&event.kind,
        chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received { owner_session_id, source: original, .. })
        if owner_session_id == source && original.agent_message().is_some_and(|(sender, operation, _, _)| sender == reference.source_session_id && operation == reference.message_id)
    ))
}

async fn confirm_closed_target(
    request: &AgentInteractionRequest,
    target_id: &str,
    source: chat_state::NotificationSource,
    body: String,
) -> AgentInteractionOutput {
    let target = target_id.to_owned();
    let sender = request.source_session_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        use crate::session::notification_inbox::AgentMessageDeliveryError as Error;
        let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
        let opened = storage.open_session_by_id(&target).map_err(|error| Error::unconfirmed("receipt_unavailable", error.to_string()))?
            .ok_or_else(|| Error::rejected("target_unavailable", "Target session is unavailable"))?;
        let validated = opened.validated_timeline(&target).map_err(|error| Error::unconfirmed("receipt_unavailable", error.to_string()))?;
        // Initial messages retain direct delegation authority even for a read-only ACK lookup.
        if matches!(source, chat_state::NotificationSource::ParentMessage { .. }) && !validated.timeline.events().iter().any(|event| matches!(
            &event.kind, chat_state::TimelineEventKind::SubagentSeed(seed) if seed.security_parent_session_id == sender && seed.subagent_id == target
        )) {
            return Err(Error::rejected("not_direct_child", "Target is not an owned direct child"));
        }
        crate::session::notification_inbox::agent_message_receipt(&validated.timeline.parent_message_receipts(), &target, &source, &body)?
            .ok_or_else(|| Error::rejected("target_inactive", "Target is inactive and has no receipt for this operation"))
    }).await;
    match result {
        Ok(result) => delivery_output(&request.id, result),
        Err(error) => AgentInteractionOutput::message_unconfirmed(
            &request.id,
            "receipt_unavailable",
            error.to_string(),
        ),
    }
}

async fn ask(
    request: &AgentInteractionRequest,
    source: &SessionHandle,
    target: &SessionHandle,
    question: &str,
    subagent_task_name: &str,
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
    let direction = if request.target_child_id.is_some() {
        InquiryDirection::ParentToChild
    } else {
        InquiryDirection::ChildToParent
    };
    let inquiry = InboundInquiry {
        authority: InquiryAuthority::Delegation,
        direction,
        inquiry_id: id.clone(),
        source_peer_id: "local-delegation".into(),
        source_session_id: source.info.id.to_string(),
        source_cwd: source.info.cwd.clone(),
        delegated_subagent_task_name: Some(subagent_task_name.to_owned()),
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
        receipt_id: None,
        subagent_task_name: None,
        target_session_id: None,
        answer: outcome.answer,
        error: outcome.error.map(|error| {
            tools::implementations::grow_build::task::interaction::AgentInteractionError::new(
                "inquiry_failed",
                error.to_string(),
            )
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn durable_ack_wins_ready_cancellation_but_absence_stays_unknown() {
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        tx.send(Ok("durable-receipt".to_owned())).unwrap();
        assert_eq!(
            wait_for_message_ack(&mut rx, &cancellation)
                .await
                .unwrap()
                .unwrap(),
            "durable-receipt"
        );
        let (_tx, mut rx) = tokio::sync::oneshot::channel();
        assert!(wait_for_message_ack(&mut rx, &cancellation).await.is_none());
    }

    #[test]
    fn reply_authority_requires_own_receipt_and_exact_original_sender() {
        let source = chat_state::NotificationSource::ParentMessage {
            parent_session_id: "parent".into(),
            message_id: "m1".into(),
            interrupt: false,
        };
        let version = chat_state::NotificationSourceVersion::Ordinal {
            value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
        };
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Received {
                    id: chat_state::notification_id("child", &source, &version).unwrap(),
                    owner_session_id: "child".into(),
                    source,
                    source_version: version,
                    payload_ref: chat_state::NotificationPayloadRef {
                        blake3: blake3::hash(b"opinion").to_hex().to_string(),
                        bytes: 7,
                    },
                },
            ))
            .unwrap();
        let original = sampling_types::AgentMessageRef {
            source_session_id: "parent".into(),
            message_id: "m1".into(),
        };
        assert!(authorizes_reply(timeline.events(), "child", &original));
        assert!(!authorizes_reply(timeline.events(), "sibling", &original));
        assert!(!authorizes_reply(
            timeline.events(),
            "child",
            &sampling_types::AgentMessageRef {
                source_session_id: "peer".into(),
                ..original.clone()
            }
        ));
        assert!(!authorizes_reply(
            timeline.events(),
            "child",
            &sampling_types::AgentMessageRef {
                message_id: "guessed".into(),
                ..original
            }
        ));
        assert!(!authorizes_reply(
            &[],
            "child",
            &sampling_types::AgentMessageRef {
                source_session_id: "parent".into(),
                message_id: "m1".into()
            }
        ));
    }
}

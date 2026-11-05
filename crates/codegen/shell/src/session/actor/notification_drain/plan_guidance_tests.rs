use super::*;
use crate::extensions::notification::{AgentMessageNotice, UiNotice, UiNoticeTone};
use crate::session::storage::{JsonlStorageAdapter, SessionUpdate, StorageAdapter as _};
use chat_state::{NotificationSource, NotificationSourceVersion};
use sampling_types::ConversationItem;

const BODY: &str = "The parent TODO changed. Pause work that depends on the previous objective, inspect the latest parent TODO with todo_read, and adjust only affected work. Ask the parent with ask_parent if the changed objective is unclear. Preserve valid completed work and acknowledge this exact change.";

fn gateway_notices(
    gateway: &mut mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) -> Vec<crate::extensions::notification::SessionNotification> {
    let mut notices = Vec::new();
    while let Ok(message) = gateway.try_recv() {
        if let acp_transport::AcpClientMessage::ExtNotification(args) = message
            && args.request.method.as_ref() == "grow/session_notification"
            && let Ok(notification) = serde_json::from_str::<
                crate::extensions::notification::SessionNotification,
            >(args.request.params.get())
            && matches!(notification.update, GrowSessionUpdate::UiNotice(_))
        {
            notices.push(notification);
        }
    }
    notices
}

fn history_notices(updates: &[SessionUpdate]) -> Vec<&UiNotice> {
    updates
        .iter()
        .filter_map(|update| match update {
            SessionUpdate::Grow(notification) => match &notification.update {
                GrowSessionUpdate::UiNotice(notice)
                    if AgentMessageNotice::from_notice(notice).is_some() =>
                {
                    Some(notice)
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn marker(session: &acp::SessionId, name: &str) -> SessionUpdate {
    SessionUpdate::Acp(Box::new(
        acp::SessionNotification::new(
            session.clone(),
            acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(name),
            ))),
        )
        .meta(Some(
            serde_json::json!({"eventId": name})
                .as_object()
                .unwrap()
                .clone(),
        )),
    ))
}

#[tokio::test(flavor = "current_thread")]
async fn plan_guidance_is_hidden_live_and_cold_but_consumed_once_as_model_input() {
    tokio::task::LocalSet::new()
        .run_until(check_live_and_cold_guidance_presentation())
        .await;
}

async fn check_live_and_cold_guidance_presentation() {
    let root = tempfile::tempdir().unwrap();
    let info = crate::session::info::Info {
        id: acp::SessionId::new("test-actor"),
        cwd: std::env::temp_dir().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(root.path().to_path_buf()));
    storage
        .init_session(&info, crate::session::persistence::default_model_id())
        .await
        .unwrap();
    let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel::<PersistenceMsg>();
    let writer = storage.clone();
    let writer_info = info.clone();
    let persistence_task = tokio::task::spawn_local(async move {
        while let Some(message) = persistence_rx.recv().await {
            match message {
                PersistenceMsg::TimelineDurablyAndAck { event, respond_to } => {
                    let result = writer
                        .append_timeline_event_durable(&writer_info, &event)
                        .await;
                    let _ = respond_to.send(result);
                }
                PersistenceMsg::Update(update) => {
                    writer
                        .append_update_durable_commit_aware(&writer_info, &update)
                        .await
                        .unwrap();
                }
                PersistenceMsg::Flush => {}
                _ => panic!("unexpected persistence operation in notification fixture"),
            }
        }
    });
    let (gateway_tx, mut gateway) = mpsc::unbounded_channel();
    let (mut actor, _events) =
        crate::session::actor::tests::support::create_test_actor_ex_with_physical_projection(
            0,
            256_000,
            85,
            gateway_tx,
            persistence_tx,
        )
        .await;
    // The fixture's seed and every live receipt use the real JSONL ACK path.
    assert!(
        !actor
            .chat_state_handle
            .timeline_events()
            .await
            .unwrap()
            .is_empty()
    );
    actor.session_directory = storage.open_session(&info).unwrap().directory_handle();
    actor.session_dir = actor.session_directory.display_path().to_path_buf();
    let actor = Arc::new(actor);
    crate::session::actor::tests::support::begin_test_active_causal_turn(&actor).await;
    let turn = actor.events.current_turn().unwrap();
    storage
        .append_update_durable_commit_aware(&info, &marker(&info.id, "before"))
        .await
        .unwrap();
    let version = NotificationSourceVersion::Ordinal {
        value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
    };
    let guidance = NotificationSource::PlanGuidance {
        parent_session_id: "parent".into(),
        message_id: "change-1".into(),
    };
    // Exercise the current receipt owner, without inventing the later planning constructor.
    let hidden_id = actor
        .receive_notification_detailed(guidance.clone(), version.clone(), BODY.into(), None)
        .await
        .unwrap();
    assert!(gateway_notices(&mut gateway).is_empty());
    assert_eq!(
        actor
            .chat_state_handle
            .parent_message_receipts()
            .await
            .unwrap()
            .len(),
        1
    );
    let pending = actor
        .chat_state_handle
        .pending_notifications()
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, hidden_id);
    let payload = pending[0].payload_ref.clone();
    assert_eq!(
        crate::session::notification_inbox::read_payload(&actor.session_directory, &payload)
            .unwrap(),
        BODY
    );
    assert_eq!(
        actor
            .chat_state_handle
            .consumed_notification_turn(guidance.clone(), version.clone())
            .await,
        Some(None)
    );
    assert!(
        !actor
            .chat_state_handle
            .get_conversation()
            .await
            .iter()
            .any(|item| { matches!(item, ConversationItem::AgentMessage(_)) })
    );

    let ordinary_id = actor
        .receive_notification_detailed(
            NotificationSource::ParentMessage {
                parent_session_id: "parent".into(),
                message_id: "change-1".into(),
                interrupt: true,
            },
            version.clone(),
            BODY.into(),
            None,
        )
        .await
        .unwrap();
    assert_ne!(ordinary_id, hidden_id);
    let ordinary_live = gateway_notices(&mut gateway);
    assert_eq!(ordinary_live.len(), 1);
    assert_eq!(ordinary_live[0].meta.as_ref().unwrap()["transient"], true);
    let GrowSessionUpdate::UiNotice(notice) = &ordinary_live[0].update else {
        panic!("expected ordinary parent notice")
    };
    assert_eq!(notice.correlation_id, ordinary_id);
    assert_eq!(
        AgentMessageNotice::from_notice(notice)
            .unwrap()
            .message
            .as_deref(),
        Some(BODY)
    );
    let mut cached = ordinary_live[0].clone();
    cached.meta = Some(serde_json::json!({"eventId": "ordinary-notice"}));
    let cached = SessionUpdate::Grow(Box::new(cached));
    for _ in 0..2 {
        storage
            .append_update_durable_commit_aware(&info, &cached)
            .await
            .unwrap();
    }
    storage
        .append_update_durable_commit_aware(&info, &marker(&info.id, "between"))
        .await
        .unwrap();
    let reply_id = actor
        .receive_notification_detailed(
            NotificationSource::AgentReply {
                source_session_id: "child".into(),
                message_id: "reply-1".into(),
                reply_to: sampling_types::AgentMessageRef {
                    source_session_id: info.id.0.to_string(),
                    message_id: "original".into(),
                },
            },
            version.clone(),
            BODY.into(),
            None,
        )
        .await
        .unwrap();
    let replies = gateway_notices(&mut gateway);
    assert_eq!(replies.len(), 1);
    let GrowSessionUpdate::UiNotice(notice) = &replies[0].update else {
        panic!("expected ordinary reply notice")
    };
    assert_eq!(notice.correlation_id, reply_id);
    assert_eq!(
        AgentMessageNotice::from_notice(notice)
            .unwrap()
            .message
            .as_deref(),
        Some(BODY)
    );

    assert!(actor.drain_active_notifications().await.unwrap());
    assert!(!actor.drain_active_notifications().await.unwrap());
    assert_eq!(
        actor
            .chat_state_handle
            .consumed_notification_turn(guidance.clone(), version.clone())
            .await,
        Some(Some(turn))
    );
    let request = actor
        .chat_state_handle
        .build_request(info.id.0.as_ref(), vec![], None, None, None)
        .await
        .unwrap();
    let messages = request
        .items
        .iter()
        .filter_map(|item| match item {
            ConversationItem::AgentMessage(batch) => Some(batch),
            _ => None,
        })
        .flat_map(|batch| &batch.messages)
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.receipt_id.as_str())
            .collect::<Vec<_>>(),
        [hidden_id.as_str(), ordinary_id.as_str(), reply_id.as_str()]
    );
    assert!(
        messages
            .iter()
            .all(|message| message.message.as_ref() == BODY)
    );
    assert!(
        actor
            .chat_state_handle
            .pending_notifications()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        actor
            .chat_state_handle
            .retained_notification_payload_hashes()
            .await
            .unwrap()
            .contains(&payload.blake3)
    );
    actor
        .reconcile_notification_payloads(&tokio_util::sync::CancellationToken::new())
        .await;
    assert_eq!(
        crate::session::notification_inbox::read_payload(&actor.session_directory, &payload)
            .unwrap(),
        BODY
    );
    storage
        .append_update_durable_commit_aware(&info, &marker(&info.id, "after"))
        .await
        .unwrap();
    let live_events = actor.chat_state_handle.timeline_events().await.unwrap();
    assert_eq!(
        serde_json::to_value(storage.read_timeline_events_sync(&info).unwrap()).unwrap(),
        serde_json::to_value(&live_events).unwrap()
    );
    persistence_task.abort();
    assert!(persistence_task.await.unwrap_err().is_cancelled());

    let opened = storage.open_session(&info).unwrap();
    let cold = opened.validated_timeline(info.id.0.as_ref()).unwrap();
    assert_eq!(cold.timeline.parent_message_receipts().len(), 3);
    assert!(cold.timeline.pending_notifications().is_empty());
    assert_eq!(
        cold.timeline
            .consumed_notification_turn(&guidance, &version),
        Some(turn)
    );
    assert!(
        cold.timeline
            .retained_notification_payload_hashes()
            .contains(&payload.blake3)
    );
    assert_eq!(
        cold.timeline
            .surface()
            .iter()
            .filter_map(|item| match item {
                ConversationItem::AgentMessage(batch) => Some(batch),
                _ => None,
            })
            .flat_map(|batch| &batch.messages)
            .filter(|message| message.receipt_id == hidden_id && message.message.as_ref() == BODY)
            .count(),
        1
    );
    let ledger = actor
        .session_dir
        .join(crate::session::storage::TIMELINE_FILE);
    let before = std::fs::read(&ledger).unwrap();
    let mut emitted = Vec::new();
    assert_eq!(
        crate::session::storage::stream_session_history_at(
            info.id.0.as_ref(),
            root.path(),
            |update| emitted.push(update)
        )
        .unwrap(),
        crate::session::storage::ReplayEmission::Emitted
    );
    let notices = history_notices(&emitted);
    assert_eq!(
        notices
            .iter()
            .map(|notice| notice.correlation_id.as_str())
            .collect::<Vec<_>>(),
        [ordinary_id.as_str(), reply_id.as_str()]
    );
    assert!(notices.iter().all(|notice| {
        AgentMessageNotice::from_notice(notice)
            .unwrap()
            .message
            .as_deref()
            == Some(BODY)
    }));
    assert_eq!(
        emitted.len(),
        5,
        "three physical markers and two ordinary receipts"
    );
    for index in [0, 2, 4] {
        assert!(
            matches!(&emitted[index], SessionUpdate::Acp(notification) if matches!(&notification.update, acp::SessionUpdate::UserMessageChunk(_)))
        );
    }
    let transcript =
        crate::session::storage::transcript::read_session_at(info.id.0.as_ref(), root.path())
            .unwrap();
    let offline = transcript
        .events
        .into_iter()
        .map(|event| event.update)
        .collect::<Vec<_>>();
    assert_eq!(history_notices(&offline), notices);
    assert_eq!(
        std::fs::read(&ledger).unwrap(),
        before,
        "replay must not write facts or consume messages"
    );

    // All three sources share these bytes. Missing history must warn only for ordinary messages.
    crate::session::notification_inbox::remove_payload(&actor.session_directory, &payload).unwrap();
    let hidden_receipt = live_events.iter().find(|event| matches!(&event.kind, chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received { id, .. }) if id == &hidden_id)).unwrap();
    assert!(
        crate::session::notification_inbox::read_parent_message_notice(
            &actor.session_directory,
            hidden_receipt
        )
        .is_none()
    );
    let mut missing = Vec::new();
    assert_eq!(
        crate::session::storage::stream_session_history_at(
            info.id.0.as_ref(),
            root.path(),
            |update| missing.push(update)
        )
        .unwrap(),
        crate::session::storage::ReplayEmission::Emitted
    );
    let warnings = history_notices(&missing);
    assert_eq!(
        warnings
            .iter()
            .map(|notice| notice.correlation_id.as_str())
            .collect::<Vec<_>>(),
        [ordinary_id.as_str(), reply_id.as_str()]
    );
    assert!(warnings.iter().all(|notice| {
        notice.message == "Message unavailable"
            && notice.tone == UiNoticeTone::Warning
            && AgentMessageNotice::from_notice(notice)
                .unwrap()
                .message
                .is_none()
    }));
    assert_eq!(std::fs::read(&ledger).unwrap(), before);
}

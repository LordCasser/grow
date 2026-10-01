use super::*;
use crate::extensions::notification::{UiNotice, UiNoticeCategory, UiNoticeTone};
use crate::session::storage::{RawReconciliation, ReconciledReplayLine};
use acp_transport::protocol as acp;
use std::collections::HashMap;

fn session_id() -> acp::SessionId {
    acp::SessionId::new("history-test")
}

fn receipt(timeline: &mut chat_state::Timeline, id: &str) -> chat_state::TimelineEvent {
    let source = chat_state::NotificationSource::ParentMessage {
        parent_session_id: "parent".into(),
        message_id: id.into(),
        interrupt: false,
    };
    let source_version = chat_state::NotificationSourceVersion::Ordinal { value: 2 };
    let id = chat_state::notification_id("history-test", &source, &source_version).unwrap();
    timeline
        .record(chat_state::TimelineEventKind::Notification(
            chat_state::NotificationEvent::Received {
                id: id.clone(),
                owner_session_id: "history-test".into(),
                source,
                source_version,
                payload_ref: chat_state::NotificationPayloadRef {
                    blake3: blake3::hash(b"body").to_hex().to_string(),
                    bytes: 4,
                },
            },
        ))
        .unwrap()
}

fn marker(timeline: &mut chat_state::Timeline, input_id: &str) -> chat_state::TimelineEvent {
    timeline
        .record(chat_state::TimelineEventKind::Input(
            chat_state::InputEvent::Submitted {
                input_id: input_id.into(),
                intent: chat_state::InputIntent::Prompt,
                payload_ref: chat_state::InputPayloadRef {
                    blake3: blake3::hash(input_id.as_bytes()).to_hex().to_string(),
                    bytes: input_id.len() as u64,
                },
            },
        ))
        .unwrap()
}

fn event_id(event: &chat_state::TimelineEvent) -> &str {
    let chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
        id,
        ..
    }) = &event.kind
    else {
        panic!("expected receipt")
    };
    id
}

fn notice(id: &str, message: Option<&str>) -> UiNotice {
    UiNotice {
        correlation_id: id.into(),
        category: UiNoticeCategory::Coordination,
        subject: Some(AgentMessageNotice::SUBJECT.into()),
        description: None,
        message: "Agent reply received".into(),
        tone: UiNoticeTone::Info,
        details: Some(
            serde_json::to_string(&AgentMessageNotice {
                source_session_id: "parent".into(),
                message_id: id.into(),
                interrupt: false,
                reply_to: None,
                message: message.map(str::to_owned),
            })
            .unwrap(),
        ),
    }
}

fn anchor(seq: u64) -> String {
    serde_json::json!({"method": crate::session::response_projection::RESPONSE_REPLAY_PROJECTION_METHOD, "params": {"timeline_event": seq}}).to_string()
}

fn user_message_update(session_id: &str, prompt_id: &str, event_id: &str, text: &str) -> String {
    serde_json::json!({
        "timestamp": 1_700_000_000,
        "method": "session/update",
        "params": {
            "sessionId": session_id,
            "update": {
                "sessionUpdate": "user_message_chunk",
                "content": {"type": "text", "text": text}
            },
            "_meta": {"eventId": event_id, "promptId": prompt_id}
        }
    })
    .to_string()
}

fn restore_rows<'a>(
    timeline: &chat_state::Timeline,
    rows: Vec<&'a str>,
    notices: &HashMap<String, UiNotice>,
) -> RawReconciliation<'a> {
    let history = RawReconciliation {
        lines: rows
            .into_iter()
            .map(ReconciledReplayLine::Borrowed)
            .collect(),
        changed: false,
    };
    restore(&session_id(), timeline, history, |event| {
        let chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
            id,
            ..
        }) = &event.kind
        else {
            return Ok(None);
        };
        Ok(notices.get(id).cloned())
    })
    .unwrap()
}

fn incoming_inquiry(
    timeline: &mut chat_state::Timeline,
    inquiry_id: &str,
    approval: Option<&str>,
    outcome: Option<crate::coordination::InquiryOutcome>,
) -> chat_state::TimelineEvent {
    incoming_inquiry_from_peer(timeline, inquiry_id, "parent-peer", approval, outcome)
}

fn incoming_inquiry_from_peer(
    timeline: &mut chat_state::Timeline,
    inquiry_id: &str,
    source_peer_id: &str,
    approval: Option<&str>,
    outcome: Option<crate::coordination::InquiryOutcome>,
) -> chat_state::TimelineEvent {
    let audit = crate::coordination::IncomingInquiryAudit {
        direction: crate::coordination::InquiryDirection::ParentToChild,
        source_peer_id: source_peer_id.into(),
        source_session_id: format!("session-{source_peer_id}"),
        source_cwd: "/parent/workspace".into(),
        delegated_subagent_task_name: None,
        question: "What changed?".into(),
        approval: approval.map(str::to_owned),
        outcome,
    };
    timeline
        .record(
            crate::coordination::InquiryEvent::Incoming {
                inquiry_id: inquiry_id.into(),
                audit,
            }
            .timeline_kind(),
        )
        .unwrap()
}

fn notice_row(notice: UiNotice, timeline_event: u64) -> String {
    serde_json::to_string(&SessionUpdateEnvelope {
        timestamp: 0,
        method: GROW_SESSION_UPDATE_METHOD.into(),
        params: serde_json::to_value(SessionNotification {
            session_id: session_id(),
            update: GrowUpdate::UiNotice(notice),
            meta: Some(serde_json::json!({"timelineEvent": timeline_event})),
        })
        .unwrap(),
    })
    .unwrap()
}

#[test]
fn inserts_timeline_only_receipts_between_physical_anchors_in_receipt_order() {
    let mut timeline = chat_state::Timeline::default();
    let before = receipt(&mut timeline, "before");
    let first_anchor = marker(&mut timeline, "input-1");
    let middle_1 = receipt(&mut timeline, "middle-1");
    let middle_2 = receipt(&mut timeline, "middle-2");
    let second_anchor = marker(&mut timeline, "input-2");
    let after = receipt(&mut timeline, "after");
    let before_row = anchor(first_anchor.seq.get());
    let after_row = anchor(second_anchor.seq.get());
    let notices = [
        (&before, "before"),
        (&middle_1, "middle-1"),
        (&middle_2, "middle-2"),
        (&after, "after"),
    ]
    .into_iter()
    .map(|(event, label)| {
        (
            event_id(event).to_owned(),
            notice(event_id(event), Some(label)),
        )
    })
    .collect();
    let history = restore_rows(&timeline, vec![&before_row, &after_row], &notices);
    let ids = history
        .lines
        .iter()
        .filter_map(|line| {
            let env: SessionUpdateEnvelope = serde_json::from_str(line.as_str()).ok()?;
            let notification: SessionNotification = serde_json::from_value(env.params).ok()?;
            let GrowUpdate::UiNotice(n) = notification.update else {
                return None;
            };
            Some(
                AgentMessageNotice::from_notice(&n)
                    .unwrap()
                    .message
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, ["before", "middle-1", "middle-2", "after"]);
    assert!(history.changed);
}

#[test]
fn preserves_existing_notice_position_deduplicates_and_keeps_original_timestamp_without_event_id() {
    let mut timeline = chat_state::Timeline::default();
    let existing_event = receipt(&mut timeline, "already");
    let new_event = receipt(&mut timeline, "new");
    let existing = serde_json::to_string(&SessionUpdateEnvelope {
        timestamp: 0,
        method: GROW_SESSION_UPDATE_METHOD.into(),
        params: serde_json::to_value(SessionNotification {
            session_id: session_id(),
            update: GrowUpdate::UiNotice(notice(event_id(&existing_event), Some("old"))),
            meta: None,
        })
        .unwrap(),
    })
    .unwrap();
    let anchor = anchor(existing_event.seq.get());
    let notices = [
        (
            event_id(&existing_event).to_owned(),
            notice(event_id(&existing_event), Some("replacement")),
        ),
        (
            event_id(&new_event).to_owned(),
            notice(event_id(&new_event), Some("body")),
        ),
    ]
    .into_iter()
    .collect();
    let history = restore_rows(&timeline, vec![&anchor, &existing], &notices);
    assert_eq!(history.lines.len(), 3);
    assert_eq!(history.lines[1].as_str(), existing);
    let inserted = &history.lines[2];
    let env: SessionUpdateEnvelope = serde_json::from_str(inserted.as_str()).unwrap();
    assert_eq!(
        env.params["_meta"]["agentTimestampMs"],
        new_event.at_ms as u64
    );
    assert!(
        !env.params["_meta"]
            .as_object()
            .unwrap()
            .contains_key("eventId")
    );
    assert_eq!(
        history
            .lines
            .iter()
            .filter(|line| line.as_str().contains(event_id(&existing_event)))
            .count(),
        1
    );
}

#[test]
fn missing_payload_and_unanchored_fallback_are_explicit_and_bounded() {
    let mut timeline = chat_state::Timeline::default();
    let event = receipt(&mut timeline, "missing");
    let notices = [(event_id(&event).to_owned(), notice(event_id(&event), None))]
        .into_iter()
        .collect();
    let history = restore_rows(&timeline, vec![], &notices);
    let env: SessionUpdateEnvelope = serde_json::from_str(history.lines[0].as_str()).unwrap();
    let notification: SessionNotification = serde_json::from_value(env.params).unwrap();
    let GrowUpdate::UiNotice(n) = notification.update else {
        panic!("expected notice")
    };
    assert!(
        AgentMessageNotice::from_notice(&n)
            .unwrap()
            .display_details("missing")
            .contains("Message unavailable")
    );
    assert_eq!(
        notification.meta.as_ref().unwrap()["historyOrderEstimated"],
        true
    );
    assert_eq!(
        notification.meta.unwrap()["agentTimestampMs"],
        event.at_ms as u64
    );
}

#[test]
fn synthesized_history_forces_full_cursor_replay() {
    let mut timeline = chat_state::Timeline::default();
    receipt(&mut timeline, "reply");
    let raw = serde_json::json!({"method":"acp/session/update", "params":{"sessionId":"history-test", "update":{"sessionUpdate":"agent_message_chunk", "content":{"type":"text", "text":"physical"}}, "_meta":{"eventId":"seen"}}}).to_string();
    let mut calls = 0;
    let prepared = crate::session::storage::prepare_reconciled_replay_lines(
        &session_id(),
        &timeline,
        &raw,
        Some("seen"),
        |event| {
            calls += 1;
            let chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Received { id, .. },
            ) = &event.kind
            else {
                return Ok(None);
            };
            Ok(Some(notice(id, Some("reply body"))))
        },
    )
    .unwrap();
    assert!(prepared.mark_replay);
    assert_eq!(calls, 1);
    assert_eq!(prepared.lines.len(), 2);
    assert!(
        prepared
            .lines
            .iter()
            .any(|line| line.as_str().contains("reply body"))
    );
}

#[test]
fn restores_only_missing_incoming_inquiry_phases_around_existing_anchor_rows() {
    use crate::coordination::{InquiryEvent, InquiryOutcome};

    let mut timeline = chat_state::Timeline::default();
    let received = incoming_inquiry(&mut timeline, "inquiry-1", None, None);
    let before_approval = marker(&mut timeline, "before-approval");
    let approval = incoming_inquiry(&mut timeline, "inquiry-1", Some("approved"), None);
    let before_completion = marker(&mut timeline, "before-completion");
    let completed = incoming_inquiry(
        &mut timeline,
        "inquiry-1",
        None,
        Some(InquiryOutcome::answered("inquiry-1", "Answer".into())),
    );

    let received_fact = InquiryEvent::from_timeline(&received).unwrap().unwrap();
    let approval_fact = InquiryEvent::from_timeline(&approval).unwrap().unwrap();
    let completed_fact = InquiryEvent::from_timeline(&completed).unwrap().unwrap();
    let existing_approval = notice_row(approval_fact.notice(), approval.seq.get());
    let rows = vec![
        anchor(before_approval.seq.get()),
        existing_approval.clone(),
        anchor(before_completion.seq.get()),
    ];
    let history = RawReconciliation {
        lines: rows
            .iter()
            .map(|line| ReconciledReplayLine::Borrowed(line.as_str()))
            .collect(),
        changed: false,
    };
    let restored = restore(&session_id(), &timeline, history, |event| {
        Ok(InquiryEvent::from_timeline(event)
            .ok()
            .flatten()
            .map(|fact| fact.notice()))
    })
    .unwrap();

    let notices = restored
        .lines
        .iter()
        .filter_map(|line| {
            let envelope: SessionUpdateEnvelope = serde_json::from_str(line.as_str()).ok()?;
            if envelope.method != GROW_SESSION_UPDATE_METHOD {
                return None;
            }
            let notification: SessionNotification = serde_json::from_value(envelope.params).ok()?;
            let GrowUpdate::UiNotice(notice) = notification.update else {
                return None;
            };
            Some((
                notice.subject.unwrap(),
                notification.meta.unwrap()["timelineEvent"]
                    .as_u64()
                    .unwrap(),
            ))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        notices,
        [
            ("incoming inquiry".into(), received.seq.get()),
            ("inquiry approval".into(), approval.seq.get()),
            ("inquiry completed".into(), completed.seq.get()),
        ]
    );
    assert_eq!(restored.lines.len(), 5);
    assert_eq!(restored.lines[2].as_str(), existing_approval);
    assert!(restored.changed);
}

#[test]
fn incoming_phase_identity_includes_source_peer_when_inquiry_ids_collide() {
    use crate::coordination::InquiryEvent;

    let mut timeline = chat_state::Timeline::default();
    let cached_peer = incoming_inquiry_from_peer(&mut timeline, "shared-id", "peer-a", None, None);
    let between = marker(&mut timeline, "between-peers");
    let missing_peer = incoming_inquiry_from_peer(&mut timeline, "shared-id", "peer-b", None, None);
    let after = marker(&mut timeline, "after-peers");

    let cached_fact = InquiryEvent::from_timeline(&cached_peer).unwrap().unwrap();
    let cached_notice = cached_fact.notice();
    let cached_row = notice_row(cached_notice.clone(), cached_peer.seq.get());
    let between_row = anchor(between.seq.get());
    let after_row = anchor(after.seq.get());
    let rows = [
        cached_row.as_str(),
        between_row.as_str(),
        after_row.as_str(),
    ];
    let history = RawReconciliation {
        lines: rows
            .iter()
            .map(|line| ReconciledReplayLine::Borrowed(line))
            .collect(),
        changed: false,
    };

    let restored = restore(&session_id(), &timeline, history, |event| {
        Ok(InquiryEvent::from_timeline(event)
            .ok()
            .flatten()
            .map(|fact| fact.notice()))
    })
    .unwrap();
    assert_eq!(restored.lines.len(), 4);
    assert_eq!(restored.lines[0].as_str(), cached_row);

    let notices = restored
        .lines
        .iter()
        .enumerate()
        .filter_map(|(position, line)| {
            let envelope: SessionUpdateEnvelope = serde_json::from_str(line.as_str()).ok()?;
            if envelope.method != GROW_SESSION_UPDATE_METHOD {
                return None;
            }
            let notification: SessionNotification = serde_json::from_value(envelope.params).ok()?;
            let GrowUpdate::UiNotice(notice) = notification.update else {
                return None;
            };
            let audit = crate::coordination::IncomingInquiryAudit::from_notice(&notice).unwrap();
            Some((
                position,
                audit.source_peer_id,
                notification.meta.unwrap()["timelineEvent"]
                    .as_u64()
                    .unwrap(),
            ))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        notices,
        [
            (0, "peer-a".into(), cached_peer.seq.get()),
            (2, "peer-b".into(), missing_peer.seq.get()),
        ]
    );
}

#[test]
fn matched_cached_notice_anchors_missing_inquiry_phases_without_other_anchors() {
    use crate::coordination::{InquiryEvent, InquiryOutcome};

    let mut timeline = chat_state::Timeline::default();
    let received = incoming_inquiry(&mut timeline, "unanchored-inquiry", None, None);
    let approval = incoming_inquiry(&mut timeline, "unanchored-inquiry", Some("approved"), None);
    let completed = incoming_inquiry(
        &mut timeline,
        "unanchored-inquiry",
        None,
        Some(InquiryOutcome::answered(
            "unanchored-inquiry",
            "Done".into(),
        )),
    );
    let approval_fact = InquiryEvent::from_timeline(&approval).unwrap().unwrap();
    // The cache metadata is deliberately unrelated to its Timeline phase.
    let cached_approval = notice_row(approval_fact.notice(), 999_999);
    let history = RawReconciliation {
        lines: vec![ReconciledReplayLine::Borrowed(&cached_approval)],
        changed: false,
    };

    let restored = restore(&session_id(), &timeline, history, |event| {
        Ok(InquiryEvent::from_timeline(event)
            .ok()
            .flatten()
            .map(|fact| fact.notice()))
    })
    .unwrap();

    let notices = restored
        .lines
        .iter()
        .enumerate()
        .filter_map(|(position, line)| {
            let envelope: SessionUpdateEnvelope = serde_json::from_str(line.as_str()).ok()?;
            let notification: SessionNotification = serde_json::from_value(envelope.params).ok()?;
            let GrowUpdate::UiNotice(notice) = notification.update else {
                return None;
            };
            Some((
                position,
                notice.subject.unwrap(),
                notification.meta.unwrap(),
            ))
        })
        .collect::<Vec<_>>();
    assert_eq!(notices.len(), 3);
    assert_eq!(notices[0].0, 0);
    assert_eq!(notices[0].1, "incoming inquiry");
    assert_eq!(notices[0].2["timelineEvent"], received.seq.get());
    assert_eq!(notices[1].0, 1);
    assert_eq!(notices[1].1, "inquiry approval");
    assert_eq!(notices[1].2["timelineEvent"], 999_999);
    assert_eq!(notices[2].0, 2);
    assert_eq!(notices[2].1, "inquiry completed");
    assert_eq!(notices[2].2["timelineEvent"], completed.seq.get());
}

#[tokio::test]
async fn stream_session_history_uses_the_same_planner_without_rewriting_source_caches() {
    use crate::session::persistence::default_model_id;
    use crate::session::storage::jsonl::JsonlStorageAdapter;
    use tempfile::TempDir;

    let home = TempDir::new().unwrap();
    let info = crate::session::info::Info {
        id: session_id(),
        cwd: "/isolated".into(),
    };
    let storage = JsonlStorageAdapter::with_root(home.path().to_path_buf());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let opened = storage.open_session(&info).unwrap();
    let body = "restored from isolated fixture";
    let payload_ref =
        crate::session::notification_inbox::write_payload(opened.directory(), body).unwrap();
    let mut timeline = chat_state::Timeline::default();
    let source = chat_state::NotificationSource::ParentMessage {
        parent_session_id: "parent".into(),
        message_id: "reply".into(),
        interrupt: false,
    };
    let source_version = chat_state::NotificationSourceVersion::Ordinal {
        value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
    };
    let id = chat_state::notification_id(info.id.0.as_ref(), &source, &source_version).unwrap();
    let event = timeline
        .record(chat_state::TimelineEventKind::Notification(
            chat_state::NotificationEvent::Received {
                id: id.clone(),
                owner_session_id: info.id.0.to_string(),
                source,
                source_version,
                payload_ref,
            },
        ))
        .unwrap();
    storage
        .append_timeline_event_durable(&info, &event)
        .await
        .unwrap();

    let updates_path = opened
        .directory()
        .display_path()
        .join(crate::session::storage::UPDATES_FILE);
    let physical_updates = [
        user_message_update(
            info.id.0.as_ref(),
            "before",
            "event-before",
            "before receipt",
        ),
        user_message_update(info.id.0.as_ref(), "after", "event-after", "after receipt"),
    ]
    .join("\n");
    std::fs::write(&updates_path, format!("{physical_updates}\n")).unwrap();
    let updates_before = std::fs::read(&updates_path).unwrap();
    let timeline_path = opened
        .directory()
        .display_path()
        .join(crate::session::storage::TIMELINE_FILE);
    let timeline_before = std::fs::read(&timeline_path).unwrap();
    let mut emitted = Vec::new();
    assert_eq!(
        crate::session::storage::stream_session_history_at(
            info.id.0.as_ref(),
            home.path(),
            |update| emitted.push(update)
        )
        .unwrap(),
        crate::session::storage::ReplayEmission::Emitted
    );
    assert_eq!(emitted.len(), 3);
    assert!(matches!(&emitted[0], SessionUpdate::Acp(_)));
    assert!(matches!(&emitted[1], SessionUpdate::Acp(_)));
    let SessionUpdate::Grow(notification) = &emitted[2] else {
        panic!("expected reconstructed Grow notice")
    };
    let GrowUpdate::UiNotice(ui_notice) = &notification.update else {
        panic!("expected reconstructed UI notice")
    };
    assert_eq!(ui_notice.correlation_id, id);
    assert_eq!(
        AgentMessageNotice::from_notice(ui_notice)
            .unwrap()
            .message
            .as_deref(),
        Some(body)
    );
    let source_time = event.at_ms as u64;
    assert_eq!(
        notification.meta.as_ref().unwrap()["timelineEvent"],
        event.seq.get()
    );
    assert_eq!(
        notification.meta.as_ref().unwrap()["agentTimestampMs"],
        source_time
    );
    assert!(
        !notification
            .meta
            .as_ref()
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("eventId")
    );

    let transcript =
        crate::session::storage::transcript::read_session_at(info.id.0.as_ref(), home.path())
            .unwrap();
    let transcript_receipt = transcript
        .events
        .iter()
        .position(|entry| matches!(&entry.update, SessionUpdate::Grow(n) if matches!(&n.update, GrowUpdate::UiNotice(notice) if notice.correlation_id == id)))
        .unwrap();
    assert_eq!(transcript_receipt, 2);
    let SessionUpdate::Grow(transcript_notification) =
        &transcript.events[transcript_receipt].update
    else {
        unreachable!()
    };
    assert_eq!(
        transcript_notification.meta.as_ref().unwrap()["agentTimestampMs"],
        source_time
    );
    assert_eq!(transcript.events[transcript_receipt].timestamp_ms, None);
    assert_eq!(std::fs::read(&updates_path).unwrap(), updates_before);
    assert_eq!(std::fs::read(&timeline_path).unwrap(), timeline_before);
}

#[tokio::test]
async fn stream_and_offline_history_restore_incoming_phases_without_delivery_side_effects() {
    use crate::coordination::InquiryOutcome;
    use crate::session::persistence::default_model_id;
    use crate::session::storage::jsonl::JsonlStorageAdapter;
    use tempfile::TempDir;

    let home = TempDir::new().unwrap();
    let info = crate::session::info::Info {
        id: session_id(),
        cwd: "/isolated".into(),
    };
    let storage = JsonlStorageAdapter::with_root(home.path().to_path_buf());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let opened = storage.open_session(&info).unwrap();
    let mut timeline = chat_state::Timeline::default();
    let received = incoming_inquiry(&mut timeline, "inquiry-restore", None, None);
    let before_approval = marker(&mut timeline, "input-before-approval");
    let approval = incoming_inquiry(&mut timeline, "inquiry-restore", Some("approved"), None);
    let before_completion = marker(&mut timeline, "input-before-completion");
    let completed = incoming_inquiry(
        &mut timeline,
        "inquiry-restore",
        None,
        Some(InquiryOutcome::answered(
            "inquiry-restore",
            "The change is ready.".into(),
        )),
    );
    for event in [
        &received,
        &before_approval,
        &approval,
        &before_completion,
        &completed,
    ] {
        storage
            .append_timeline_event_durable(&info, event)
            .await
            .unwrap();
    }

    let updates_path = opened
        .directory()
        .display_path()
        .join(crate::session::storage::UPDATES_FILE);
    let physical_updates = [
        user_message_update(
            info.id.0.as_ref(),
            "before-approval",
            "event-before-approval",
            "before approval",
        ),
        user_message_update(
            info.id.0.as_ref(),
            "before-completion",
            "event-before-completion",
            "before completion",
        ),
    ]
    .join("\n");
    std::fs::write(&updates_path, format!("{physical_updates}\n")).unwrap();
    let updates_before = std::fs::read(&updates_path).unwrap();
    let timeline_path = opened
        .directory()
        .display_path()
        .join(crate::session::storage::TIMELINE_FILE);
    let timeline_before = std::fs::read(&timeline_path).unwrap();

    let mut emitted = Vec::new();
    assert_eq!(
        crate::session::storage::stream_session_history_at(
            info.id.0.as_ref(),
            home.path(),
            |update| emitted.push(update)
        )
        .unwrap(),
        crate::session::storage::ReplayEmission::Emitted
    );
    assert_eq!(emitted.len(), 5);
    assert_eq!(
        emitted
            .iter()
            .filter(|update| matches!(update, SessionUpdate::Acp(_)))
            .count(),
        2
    );
    let emitted_phases = emitted
        .iter()
        .filter_map(|update| {
            let SessionUpdate::Grow(notification) = update else {
                return None;
            };
            let GrowUpdate::UiNotice(notice) = &notification.update else {
                return None;
            };
            Some((
                notice.subject.as_deref().unwrap().to_owned(),
                notification.meta.as_ref().unwrap()["timelineEvent"]
                    .as_u64()
                    .unwrap(),
            ))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        emitted_phases,
        [
            ("incoming inquiry".into(), received.seq.get()),
            ("inquiry approval".into(), approval.seq.get()),
            ("inquiry completed".into(), completed.seq.get()),
        ]
    );

    let transcript =
        crate::session::storage::transcript::read_session_at(info.id.0.as_ref(), home.path())
            .unwrap();
    let offline_phases = transcript
        .events
        .iter()
        .filter_map(|entry| {
            let SessionUpdate::Grow(notification) = &entry.update else {
                return None;
            };
            let GrowUpdate::UiNotice(notice) = &notification.update else {
                return None;
            };
            Some((
                notice.subject.as_deref().unwrap().to_owned(),
                notification.meta.as_ref().unwrap()["timelineEvent"]
                    .as_u64()
                    .unwrap(),
            ))
        })
        .collect::<Vec<_>>();
    assert_eq!(offline_phases, emitted_phases);
    assert_eq!(std::fs::read(&updates_path).unwrap(), updates_before);
    assert_eq!(std::fs::read(&timeline_path).unwrap(), timeline_before);
    assert_eq!(timeline.events().len(), 5);
    assert!(timeline.events().iter().all(|event| {
        !matches!(
            event.kind,
            chat_state::TimelineEventKind::Notification(_)
                | chat_state::TimelineEventKind::Request(_)
                | chat_state::TimelineEventKind::Hook(_)
        )
    }));
}

#[test]
#[ignore = "read-only check against an explicitly selected source session"]
fn source_session_receipts_replay_near_their_reply_anchors_without_writes() {
    use crate::session::storage::jsonl::JsonlStorageAdapter;
    use chat_state::{NotificationEvent, NotificationSource, TimelineEventKind, ToolEvent};

    let session_id = std::env::var("GROW_SOURCE_SESSION_ID")
        .expect("set GROW_SOURCE_SESSION_ID to the session under inspection");
    let grow_home = std::path::PathBuf::from(
        std::env::var_os("GROW_HOME").expect("set GROW_HOME to the Grow storage root"),
    );
    let storage = JsonlStorageAdapter::with_root(grow_home.clone());
    let opened = storage
        .open_session_by_id_shared_read(&session_id)
        .unwrap()
        .expect("source session must exist under GROW_HOME");
    let session_dir = opened.directory().display_path();
    let updates_path = session_dir.join(crate::session::storage::UPDATES_FILE);
    let timeline_path = session_dir.join(crate::session::storage::TIMELINE_FILE);
    let updates_before = std::fs::read(&updates_path).unwrap();
    let timeline_before = std::fs::read(&timeline_path).unwrap();
    let timeline = opened.validated_timeline(&session_id).unwrap().timeline;

    let mut replies = Vec::new();
    let mut tool_starts = HashMap::new();
    let mut tool_ends = HashMap::new();
    for event in timeline.events() {
        match &event.kind {
            TimelineEventKind::Notification(NotificationEvent::Received {
                id,
                source: NotificationSource::AgentReply { reply_to, .. },
                ..
            }) => replies.push((event.seq.get(), id.clone(), reply_to.message_id.clone())),
            TimelineEventKind::Tool(ToolEvent::Started { call_id, .. }) => {
                tool_starts.insert(call_id.as_str(), event.seq.get());
            }
            TimelineEventKind::Tool(ToolEvent::Completed { call_id, .. }) => {
                tool_ends.insert(call_id.as_str(), event.seq.get());
            }
            _ => {}
        }
    }

    let ordered_lines =
        super::super::with_reconciled_replay_lines(&session_id, &grow_home, |lines| {
            Ok(lines
                .iter()
                .map(|line| line.as_str().to_owned())
                .collect::<Vec<_>>())
        })
        .unwrap()
        .expect("source session must resolve");

    let mut receipt_positions = HashMap::new();
    let mut tool_call_positions = HashMap::new();
    let mut anchors = Vec::<(usize, u64)>::new();
    let mut response_positions = Vec::new();
    for (index, line) in ordered_lines.iter().enumerate() {
        let Ok(envelope) = serde_json::from_str::<SessionUpdateEnvelope>(line) else {
            continue;
        };
        if envelope.method == crate::session::response_projection::RESPONSE_REPLAY_PROJECTION_METHOD
        {
            if let Some(seq) = envelope
                .params
                .get("timeline_event")
                .and_then(|v| v.as_u64())
            {
                anchors.push((index, seq));
                response_positions.push(index);
            }
            continue;
        }
        if envelope.method == GROW_SESSION_UPDATE_METHOD {
            let Ok(notification) = serde_json::from_value::<SessionNotification>(envelope.params)
            else {
                continue;
            };
            let GrowUpdate::UiNotice(notice) = notification.update else {
                continue;
            };
            if let Some(seq) = notification
                .meta
                .as_ref()
                .and_then(|meta| meta.get("timelineEvent"))
                .and_then(serde_json::Value::as_u64)
            {
                receipt_positions.insert(notice.correlation_id, (index, seq));
            }
            continue;
        }
        if envelope.method != crate::session::storage::ACP_SESSION_UPDATE_METHOD {
            continue;
        }
        let Some(update) = envelope.params.get("update") else {
            continue;
        };
        let kind = update.get("sessionUpdate").and_then(|v| v.as_str());
        let Some(call_id) = update.get("toolCallId").and_then(|v| v.as_str()) else {
            continue;
        };
        let seq = match kind {
            Some("tool_call") => tool_starts.get(call_id).copied(),
            Some("tool_call_update")
                if matches!(
                    update.get("status").and_then(|v| v.as_str()),
                    Some("completed" | "failed")
                ) =>
            {
                tool_ends
                    .get(call_id)
                    .or_else(|| tool_starts.get(call_id))
                    .copied()
            }
            Some("tool_call_update") => tool_starts.get(call_id).copied(),
            _ => None,
        };
        if let Some(seq) = seq {
            anchors.push((index, seq));
            if kind == Some("tool_call") {
                tool_call_positions.insert(call_id.to_owned(), index);
            }
        }
    }

    assert_eq!(
        replies.len(),
        12,
        "the inspected source should contain 12 AgentReply receipts"
    );
    assert_eq!(
        receipt_positions.len(),
        replies.len(),
        "each durable reply must have one reconstructed notice"
    );
    let tail_response_position = *response_positions
        .last()
        .expect("source history should end with an admitted response");
    let mut positions = Vec::with_capacity(replies.len());
    for (receipt_seq, receipt_id, reply_call_id) in &replies {
        let (receipt_position, recorded_seq) = receipt_positions
            .get(receipt_id)
            .expect("receipt projection must retain its Timeline identity");
        assert_eq!(recorded_seq, receipt_seq);
        let call_position = *tool_call_positions
            .get(reply_call_id)
            .expect("reply_to must match its persisted ToolCall");
        assert!(
            receipt_position > &call_position,
            "reply notice should follow its source ToolCall"
        );
        let next_anchor_position = anchors
            .iter()
            .filter(|(_, seq)| seq > receipt_seq)
            .min_by_key(|(_, seq)| *seq)
            .map(|(position, _)| *position)
            .expect("each receipt should precede a later recorded history anchor");
        assert!(receipt_position < &next_anchor_position);
        assert!(
            next_anchor_position - receipt_position <= replies.len(),
            "receipt should be placed at the next source anchor, not at the transcript tail"
        );
        assert!(
            receipt_position < &tail_response_position,
            "historical replies must remain before the tail response"
        );
        positions.push(*receipt_position);
    }
    positions.sort_unstable();
    let min_position = *positions.first().unwrap();
    let max_position = *positions.last().unwrap();
    eprintln!(
        "source_session={} receipts={} history_rows={} receipt_pos_min={} receipt_pos_max={} tail_response_pos={}",
        session_id,
        replies.len(),
        ordered_lines.len(),
        min_position,
        max_position,
        tail_response_position,
    );

    let updates_after = std::fs::read(&updates_path).unwrap();
    let timeline_after = std::fs::read(&timeline_path).unwrap();
    assert!(updates_after.len() >= updates_before.len());
    assert!(updates_after.starts_with(&updates_before));
    assert!(timeline_after.len() >= timeline_before.len());
    assert!(timeline_after.starts_with(&timeline_before));
    eprintln!(
        "source_session={} appended_during_read={} updates_bytes_before={} updates_bytes_after={} timeline_bytes_before={} timeline_bytes_after={}",
        session_id,
        updates_after.len() > updates_before.len() || timeline_after.len() > timeline_before.len(),
        updates_before.len(),
        updates_after.len(),
        timeline_before.len(),
        timeline_after.len(),
    );
}

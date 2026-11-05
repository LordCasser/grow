use super::*;
use chat_state::{NotificationEvent, NotificationSource, NotificationSourceVersion, Timeline};

const BODY: &str = "The parent TODO was removed or cancelled. This delegation is terminated.";

fn receive(
    timeline: &mut Timeline,
    source: NotificationSource,
    payload_ref: &chat_state::NotificationPayloadRef,
) -> TimelineEvent {
    let source_version = NotificationSourceVersion::Ordinal {
        value: chat_state::PARENT_MESSAGE_SOURCE_VERSION,
    };
    let id = chat_state::notification_id("history-test", &source, &source_version).unwrap();
    timeline
        .record(TimelineEventKind::Notification(
            NotificationEvent::Received {
                id,
                owner_session_id: "history-test".into(),
                source,
                source_version,
                payload_ref: payload_ref.clone(),
            },
        ))
        .unwrap()
}

fn sources() -> [NotificationSource; 3] {
    [
        NotificationSource::PlanGuidance {
            parent_session_id: "parent".into(),
            message_id: "change-1".into(),
        },
        NotificationSource::ParentMessage {
            parent_session_id: "parent".into(),
            message_id: "change-1".into(),
            interrupt: true,
        },
        NotificationSource::AgentReply {
            source_session_id: "child".into(),
            message_id: "reply-1".into(),
            reply_to: sampling_types::AgentMessageRef {
                source_session_id: "history-test".into(),
                message_id: "original".into(),
            },
        },
    ]
}

#[test]
fn plan_guidance_is_not_a_display_receipt_even_when_its_payload_is_missing() {
    let root = tempfile::tempdir().unwrap();
    let directory = ContainedDirectory::open(
        root.path(),
        std::path::Path::new(""),
        "plan guidance history test",
        false,
    )
    .unwrap();
    let payload = chat_state::NotificationPayloadRef {
        blake3: blake3::hash(BODY.as_bytes()).to_hex().to_string(),
        bytes: BODY.len() as u64,
    };
    let mut timeline = Timeline::default();
    let [guidance, ordinary, reply] =
        sources().map(|source| receive(&mut timeline, source, &payload));
    assert!(receipt_identity(&guidance).unwrap().is_none());
    assert_eq!(
        ReceiptSource::from_event(&guidance).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert!(read_notice(&directory, &guidance).unwrap().is_none());
    assert!(
        crate::session::notification_inbox::parent_message_notice(&guidance, Some(BODY.into()))
            .is_none()
    );
    let mut append = crate::session::history::AppendState::default();
    for event in timeline.events() {
        append.index_receipt(event).unwrap();
    }
    assert_eq!(append.receipts.len(), 2);
    for event in [&ordinary, &reply] {
        let identity = receipt_identity(event).unwrap().unwrap();
        assert_eq!(append.receipts[&identity].event, event.seq.get());
        let notice = read_notice(&directory, event).unwrap().unwrap();
        assert_eq!(notice.message, "Message unavailable");
        assert_eq!(
            notice.tone,
            crate::extensions::notification::UiNoticeTone::Warning
        );
        assert!(
            AgentMessageNotice::from_notice(&notice)
                .unwrap()
                .message
                .is_none()
        );
    }
    let mut snapshot = crate::session::history::HistorySnapshot::default();
    snapshot
        .load_notices(&directory, &timeline, "history-test")
        .unwrap();
    assert_eq!(snapshot.notices.len(), 2);
    assert!(!snapshot.notices.contains_key(&guidance.seq.get()));
    snapshot
        .sources
        .push(crate::session::history::HistorySource::Presentation {
            at_ms: 1,
            fact: Box::new(crate::session::history::HistoryFact {
                meta: None,
                content: crate::session::history::HistoryContent::Notice {
                    event: guidance.seq.get(),
                },
            }),
        });
    assert_eq!(
        snapshot.validate(&timeline).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        timeline.parent_message_receipts().len(),
        3,
        "display indexing cannot alter internal receipts"
    );
}

fn row(notice: UiNotice) -> String {
    serde_json::to_string(&SessionUpdateEnvelope {
        timestamp: 0,
        method: GROW_SESSION_UPDATE_METHOD.into(),
        params: serde_json::to_value(SessionNotification {
            session_id: acp::SessionId::new("history-test"),
            update: GrowUpdate::UiNotice(notice),
            meta: None,
        })
        .unwrap(),
    })
    .unwrap()
}

#[test]
fn plan_guidance_restore_keeps_ordinary_receipt_order_and_cached_duplicate_suppression() {
    let root = tempfile::tempdir().unwrap();
    let directory = ContainedDirectory::open(
        root.path(),
        std::path::Path::new(""),
        "plan guidance restore test",
        false,
    )
    .unwrap();
    let payload = crate::session::notification_inbox::write_payload(&directory, BODY).unwrap();
    let mut timeline = Timeline::default();
    let [guidance, ordinary, reply] =
        sources().map(|source| receive(&mut timeline, source, &payload));
    let timeline = Timeline::from_events(
        serde_json::from_slice(&serde_json::to_vec(timeline.events()).unwrap()).unwrap(),
    )
    .unwrap();
    let notice = read_notice(&directory, &ordinary).unwrap().unwrap();
    let cached = row(notice.clone());
    let history = RawReconciliation {
        lines: vec![
            ReconciledReplayLine::Borrowed(&cached),
            ReconciledReplayLine::Borrowed(&cached),
        ],
        changed: false,
    };
    let mut read_sequences = Vec::new();
    let restored = restore(
        &acp::SessionId::new("history-test"),
        &timeline,
        history,
        |event| {
            assert_ne!(
                event.seq, guidance.seq,
                "hidden receipts must never request notice payloads"
            );
            read_sequences.push(event.seq.get());
            read_notice(&directory, event)
        },
    )
    .unwrap();
    assert_eq!(read_sequences, [reply.seq.get()]);
    assert!(restored.changed);
    assert_eq!(restored.lines.len(), 2);
    assert_eq!(restored.lines[0].as_str(), cached);
    let notices = restored
        .lines
        .iter()
        .map(|line| {
            let envelope: SessionUpdateEnvelope = serde_json::from_str(line.as_str()).unwrap();
            let notification: SessionNotification =
                serde_json::from_value(envelope.params).unwrap();
            let GrowUpdate::UiNotice(notice) = notification.update else {
                panic!("expected ordinary receipt notice")
            };
            notice
        })
        .collect::<Vec<_>>();
    assert_eq!(notices[0], notice);
    assert_eq!(
        notices[1],
        read_notice(&directory, &reply).unwrap().unwrap()
    );
    assert!(notices.iter().all(|notice| {
        AgentMessageNotice::from_notice(notice)
            .unwrap()
            .message
            .as_deref()
            == Some(BODY)
    }));
    assert_eq!(timeline.parent_message_receipts().len(), 3);
    assert_eq!(
        timeline.pending_notifications().len(),
        3,
        "read-only restoration must not consume model input"
    );
    assert!(
        timeline
            .retained_notification_payload_hashes()
            .contains(&payload.blake3)
    );
}

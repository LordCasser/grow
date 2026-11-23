use super::{assert_inquiry_wire_pairs, inquiry};
use crate::coordination::{
    IncomingInquiryAudit, InquiryAuthority, InquiryCancellation, InquiryCancellationReason,
    InquiryDirection, InquiryEvent, InquiryOutcome, InquiryStatus,
};
use crate::session::actor::SessionActor;
use crate::session::actor::tests::support::{build_actor, replace_test_surface};
use crate::session::persistence::PersistenceMsg;
use crate::session::storage::{SidebandLedgers, validate_sideband_ledgers};
use chat_state::{
    NotificationEvent, NotificationSource, SidebandEventKind, SidebandOutcome, SidebandPurpose,
    Timeline, TimelineEvent, TimelineEventKind,
};
use sampling_types::{ApiBackend, ConversationItem, ConversationRequest, ToolCall};
use std::cell::RefCell;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;
use test_support::MockInferenceServer;
use tokio::sync::oneshot;

const CONTEXT_WINDOW: u64 = 1_024;

async fn budget_fixture(
    server: &MockInferenceServer,
    mut hold_request: Option<oneshot::Sender<oneshot::Sender<()>>>,
) -> (Arc<SessionActor>, Rc<RefCell<SidebandLedgers>>) {
    let (mut actor, _gateway) = build_actor().await;
    actor
        .notifications
        .gateway_enabled
        .store(false, Ordering::Release);
    let ledgers = Rc::new(RefCell::new(SidebandLedgers::new()));
    let captured = Rc::clone(&ledgers);
    let (persistence, mut updates) = tokio::sync::mpsc::unbounded_channel();
    let bridge = std::mem::replace(
        &mut Arc::get_mut(&mut actor)
            .unwrap()
            .notifications
            .persistence_tx,
        persistence,
    );
    tokio::task::spawn_local(async move {
        while let Some(message) = updates.recv().await {
            if let PersistenceMsg::SidebandDurablyAndAck { event, .. } = &message {
                captured
                    .borrow_mut()
                    .entry(event.sideband_id.clone())
                    .or_default()
                    .push(event.clone());
                if matches!(&event.kind, SidebandEventKind::Request(request) if request.purpose == SidebandPurpose::InfoRequest)
                    && let Some(held) = hold_request.take()
                {
                    let (release, released) = oneshot::channel();
                    if held.send(release).is_ok() {
                        let _ = released.await;
                    }
                }
            }
            if bridge.send(message).is_err() {
                break;
            }
        }
    });

    let mut route = actor.chat_state_handle.get_sampling_config().await.unwrap();
    route.base_url = server.url();
    route.api_backend = ApiBackend::ChatCompletions;
    route.context_window = NonZeroU64::new(CONTEXT_WINDOW).unwrap();
    actor.chat_state_handle.replace_sampling_route(route);
    (actor, ledgers)
}

async fn enqueue(
    actor: &Arc<SessionActor>,
    question: &str,
) -> (InquiryCancellation, oneshot::Receiver<InquiryOutcome>) {
    let (mut inbound, response) = inquiry(1);
    inbound.authority = InquiryAuthority::Delegation;
    inbound.direction = InquiryDirection::ChildToParent;
    inbound.delegated_subagent_task_name = Some("Inspect the reader".into());
    inbound.target_session_id = actor.session_info.id.to_string();
    inbound.question = question.into();
    let cancellation = inbound.cancellation.clone();
    actor.enqueue_coordination_inquiry(inbound).await;
    (cancellation, response)
}

async fn ask(actor: &Arc<SessionActor>, question: &str) -> InquiryOutcome {
    let (_, response) = enqueue(actor, question).await;
    tokio::time::timeout(Duration::from_secs(5), response)
        .await
        .unwrap()
        .unwrap()
}

fn assert_terminal(
    events: &[TimelineEvent],
    outcome: &InquiryOutcome,
    question: &str,
    expected_receipts: usize,
) -> IncomingInquiryAudit {
    let terminals = events
        .iter()
        .filter_map(|event| match InquiryEvent::from_timeline(event).unwrap() {
            Some(InquiryEvent::Incoming { inquiry_id, audit })
                if inquiry_id == outcome.inquiry_id && audit.outcome.is_some() =>
            {
                Some(audit)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [terminal] = terminals.as_slice() else {
        panic!("expected one canonical inquiry terminal: {terminals:?}");
    };
    assert_eq!(terminal.question, question);
    assert_eq!(terminal.outcome.as_ref(), Some(outcome));
    let receipts = events
        .iter()
        .filter(|event| {
            matches!(
                &event.kind,
                TimelineEventKind::Notification(NotificationEvent::Received {
                    source: NotificationSource::InquiryAnswered { inquiry_id, .. }, ..
                }) if inquiry_id == &outcome.inquiry_id
            )
        })
        .count();
    assert_eq!(receipts, expected_receipts);
    terminal.clone()
}

#[tokio::test(flavor = "current_thread")]
async fn long_frozen_history_keeps_recent_tool_evidence_and_delivers_answer() {
    let test = async {
        let server = MockInferenceServer::start().await.unwrap();
        server.set_response("The reader returns an owned record.");
        let (actor, ledgers) = budget_fixture(&server, None).await;
        replace_test_surface(
            &actor.chat_state_handle,
            vec![
                ConversationItem::user("Inspect the reader."),
                ConversationItem::assistant("Old unrelated evidence. ".repeat(500)),
                ConversationItem::assistant("Recent evidence from the reader inspection."),
                ConversationItem::assistant_tool_calls(vec![ToolCall {
                    id: "reader-inspection".into(),
                    name: "read_file".into(),
                    arguments: r#"{"path":"src/reader.rs"}"#.into(),
                }]),
                ConversationItem::tool_result(
                    "reader-inspection",
                    "The return type is an owned record.",
                ),
            ],
        )
        .await;
        let session_id = actor.session_info.id.to_string();
        let before = actor
            .chat_state_handle
            .materialize_timeline(session_id.clone())
            .await
            .unwrap();
        let question = "What did the reader inspection establish?";
        let outcome = ask(&actor, question).await;
        assert_eq!(outcome.status, InquiryStatus::Answered, "{outcome:?}");
        assert_eq!(
            outcome.answer.as_deref(),
            Some("The reader returns an owned record.")
        );

        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let body = requests[0].body.as_ref().unwrap();
        assert!(
            body.get("tools")
                .is_none_or(|tools| tools.as_array().is_some_and(Vec::is_empty))
        );
        assert!(body.get("tool_choice").is_none());
        let wire = body.to_string();
        assert!(wire.contains("test system prompt"));
        assert!(wire.contains(question));
        assert!(wire.contains("Recent evidence from the reader inspection."));
        assert!(wire.contains("src/reader.rs"));
        assert!(wire.contains("The return type is an owned record."));
        assert!(!wire.contains("Old unrelated evidence."));
        assert_inquiry_wire_pairs(body, &["reader-inspection"]);

        let after = actor
            .chat_state_handle
            .materialize_timeline(session_id.clone())
            .await
            .unwrap();
        assert_eq!(after.surface_revision, before.surface_revision);
        assert_eq!(after.surface_ids, before.surface_ids);
        assert_eq!(
            serde_json::to_value(after.surface).unwrap(),
            serde_json::to_value(before.surface).unwrap()
        );

        let events = actor.chat_state_handle.timeline_events().await.unwrap();
        let terminal = assert_terminal(&events, &outcome, question, 1);
        let proof = terminal.answer_context.unwrap();
        let ledgers = ledgers.borrow();
        assert_eq!(ledgers.len(), 1);
        let [request, attempt, result, end] = ledgers[&proof.result_ref.timeline_id].as_slice()
        else {
            panic!("expected one request, attempt, result and terminal");
        };
        let SidebandEventKind::Request(request) = &request.kind else {
            panic!("missing request");
        };
        assert_eq!(request.purpose, SidebandPurpose::InfoRequest);
        assert_eq!(
            request.budget_policy.max_input_tokens_per_attempt,
            CONTEXT_WINDOW
        );
        let SidebandEventKind::Attempt(attempt) = &attempt.kind else {
            panic!("missing attempt");
        };
        let manifest = &attempt.assembly_manifest;
        assert_eq!(manifest.source_revision, Some(before.surface_revision));
        assert_eq!(
            manifest.context_surface_ids,
            [
                before.surface_ids[0],
                before.surface_ids[3],
                before.surface_ids[4],
                before.surface_ids[5]
            ]
        );
        assert!(manifest.materialized_input_tokens <= CONTEXT_WINDOW);
        assert!(
            matches!(&result.kind, SidebandEventKind::Result(result) if Some(result.raw_output.as_str()) == outcome.answer.as_deref())
        );
        assert!(
            matches!(&end.kind, SidebandEventKind::End(end) if end.outcome == SidebandOutcome::Completed)
        );
        validate_sideband_ledgers(
            &session_id,
            &Timeline::from_events(events).unwrap(),
            &ledgers,
        )
        .unwrap();
    };
    tokio::task::LocalSet::new().run_until(test).await;
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_question_settles_failed_without_provider_or_answer_receipt() {
    let test = async {
        let server = MockInferenceServer::start().await.unwrap();
        let (actor, ledgers) = budget_fixture(&server, None).await;
        let question = "Which interface should the delegated reader use? ".repeat(140);
        assert!(question.len() <= crate::coordination::MAX_QUESTION_BYTES);
        assert!(
            chat_state::estimate_request_input_tokens(&ConversationRequest {
                items: vec![ConversationItem::user(question.clone())],
                ..Default::default()
            }) > CONTEXT_WINDOW
        );
        let session_id = actor.session_info.id.to_string();
        let before = actor
            .chat_state_handle
            .materialize_timeline(session_id.clone())
            .await
            .unwrap();
        let outcome = ask(&actor, &question).await;
        assert_eq!(outcome.status, InquiryStatus::Failed, "{outcome:?}");
        assert!(outcome.answer.is_none());
        assert_eq!(server.request_count(), 0);

        let events = actor.chat_state_handle.timeline_events().await.unwrap();
        let terminal = assert_terminal(&events, &outcome, &question, 0);
        assert!(terminal.answer_context.is_none());
        let ledgers = ledgers.borrow();
        assert_eq!(ledgers.len(), 1);
        let [request, end] = ledgers.values().next().unwrap().as_slice() else {
            panic!("locally oversized mandatory input must have no attempt or result");
        };
        let SidebandEventKind::Request(request) = &request.kind else {
            panic!("missing request");
        };
        assert_eq!(request.purpose, SidebandPurpose::InfoRequest);
        let SidebandEventKind::End(end) = &end.kind else {
            panic!("missing failed terminal");
        };
        assert_eq!(end.outcome, SidebandOutcome::Failed);
        let detail = end.error.as_deref().unwrap();
        let mut items = before.surface;
        items.push(ConversationItem::user(request.prompt.clone()));
        let estimated = chat_state::estimate_request_input_tokens(&ConversationRequest {
            items,
            ..Default::default()
        });
        assert!(detail.contains(&estimated.to_string()), "{detail}");
        assert!(detail.contains(&CONTEXT_WINDOW.to_string()), "{detail}");
        validate_sideband_ledgers(
            &session_id,
            &Timeline::from_events(events).unwrap(),
            &ledgers,
        )
        .unwrap();
    };
    tokio::task::LocalSet::new().run_until(test).await;
}

#[tokio::test(flavor = "current_thread")]
async fn inquiry_cancelled_while_request_ack_is_held_reports_cancellation() {
    let test = async {
        let server = MockInferenceServer::start().await.unwrap();
        let (hold_request, request_held) = oneshot::channel();
        let (actor, ledgers) = budget_fixture(&server, Some(hold_request)).await;
        let question = "Which interface should the delegated reader use? ".repeat(140);
        assert!(question.len() <= crate::coordination::MAX_QUESTION_BYTES);
        let (cancellation, mut response) = enqueue(&actor, &question).await;
        let release = tokio::time::timeout(Duration::from_secs(5), request_held)
            .await
            .unwrap()
            .unwrap();

        assert!(cancellation.cancel(InquiryCancellationReason::Explicit));
        assert!(matches!(
            response.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        release.send(()).unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(5), response)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(outcome.status, InquiryStatus::Cancelled, "{outcome:?}");
        assert_eq!(outcome, cancellation.outcome(&outcome.inquiry_id));
        assert_eq!(server.request_count(), 0);

        let events = actor.chat_state_handle.timeline_events().await.unwrap();
        assert!(
            assert_terminal(&events, &outcome, &question, 0)
                .answer_context
                .is_none()
        );
        let ledgers = ledgers.borrow();
        assert_eq!(ledgers.len(), 1);
        let [request, end] = ledgers.values().next().unwrap().as_slice() else {
            panic!("cancelled oversized inquiry must have no attempt or result");
        };
        assert!(
            matches!(&request.kind, SidebandEventKind::Request(request) if request.purpose == SidebandPurpose::InfoRequest)
        );
        assert!(
            matches!(&end.kind, SidebandEventKind::End(end) if end.outcome == SidebandOutcome::Failed
            && end.error.as_deref().is_some_and(|detail| detail.contains("question input budget exceeded")))
        );
        validate_sideband_ledgers(
            &actor.session_info.id.to_string(),
            &Timeline::from_events(events).unwrap(),
            &ledgers,
        )
        .unwrap();
    };
    tokio::task::LocalSet::new().run_until(test).await;
}

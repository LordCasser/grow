//! Raw UTF-8 context size and the real Peer JSON frame are distinct boundaries.
use super::*;

#[test]
fn peer_escaped_answer_must_fit_actual_wire_before_success_admission() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use crate::coordination::*;
            let answer = "\"\\".repeat(300_000);
            assert!(answer.len() < chat_state::MAX_NOTIFICATION_PAYLOAD_BYTES as usize);
            assert!(matches!(crate::coordination::validate_peer_inquiry_outcome(&InquiryOutcome::answered("escaped", answer.clone())),
                Err(crate::local_ipc::frame::FrameError::TooLarge { .. })));
            for peer in [true, false] {
                let server = MockInferenceServer::start().await.unwrap();
                let (actor, staged, records) = cancellation_tests::actor_at_result_ack(&server).await;
                server.enqueue_response("/v1/messages", messages_turn(&[text_block(&answer)], END_TURN));
                let (mut inquiry, response) = live_consultation(&actor, "escaped", "Return the exact escaped evidence.");
                if peer { inquiry.authority = InquiryAuthority::Peer; inquiry.direction = InquiryDirection::Peer; }
                actor.enqueue_coordination_inquiry(inquiry).await;
                tokio::time::timeout(std::time::Duration::from_secs(10), staged).await.unwrap().unwrap().send(()).unwrap();
                let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), response).await.unwrap().unwrap();
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let pending = actor.chat_state_handle.pending_notifications().await.unwrap();
                if peer {
                    assert_eq!(outcome.status, InquiryStatus::Failed, "Peer must reject an impossible wire success");
                    assert!(outcome.error.unwrap().to_string().contains("IPC frame"));
                    assert!(pending.is_empty());
                    assert!(!events.iter().filter_map(|event| InquiryEvent::from_timeline(event).unwrap()).any(|event|
                        matches!(event, InquiryEvent::Incoming { audit, .. } if audit.answer_context.is_some())));
                } else {
                    assert_eq!(outcome.status, InquiryStatus::Answered);
                    assert_eq!(pending.len(), 1);
                    assert!(pending[0].payload_ref.bytes <= chat_state::MAX_NOTIFICATION_PAYLOAD_BYTES);
                    // Keep genuine request/result binding, changing only the route
                    // proves cold validation checks the Peer wire constraint too.
                    let mut imported = events;
                    for event in &mut imported {
                        if let Some(InquiryEvent::Incoming { inquiry_id, mut audit }) = InquiryEvent::from_timeline(event).unwrap() {
                            audit.direction = InquiryDirection::Peer;
                            event.kind = InquiryEvent::Incoming { inquiry_id, audit }.timeline_kind();
                        }
                    }
                    let sideband = records.lock().unwrap().clone();
                    let ledgers = std::collections::BTreeMap::from([(sideband[0].sideband_id.clone(), sideband)]);
                    let error = crate::session::storage::validate_sideband_ledgers(&actor.session_info.id.to_string(),
                        &chat_state::Timeline::from_events(imported).unwrap(), &ledgers).unwrap_err();
                    assert!(error.to_string().contains("IPC frame"), "{error}");
                }
                let sideband = records.lock().unwrap();
                assert!(matches!(&sideband.last().unwrap().kind,
                    chat_state::SidebandEventKind::End(end) if end.outcome == chat_state::SidebandOutcome::Completed));
                assert!(sideband.iter().any(|event| matches!(&event.kind,
                    chat_state::SidebandEventKind::Result(result) if result.raw_output == answer)));
                assert_eq!(server.requests().len(), 1);
            }
        }));
    });
}

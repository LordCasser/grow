use super::support::*;
use super::*;
use crate::extensions::notification::{SamplingAttemptState, SessionUpdate as GrowUpdate};
use crate::session::replay_events::SessionNotification;
use sampler::{SamplingChannel, SamplingEvent};

#[tokio::test(flavor = "current_thread")]
async fn sampling_candidate_is_scoped_and_dropped_until_durable_admission() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (gateway, _) = tokio::sync::mpsc::unbounded_channel();
            let (persistence, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
            let (mut actor, mut rx) =
                create_test_actor_ex(0, 256_000, 85, gateway, persistence).await;
            actor.tool_context.sampling_output_delivery = sampler::OutputDelivery::Retractable;
            let actor = std::sync::Arc::new(actor);
            let request_id = sampler::RequestId::from("candidate");
            actor
                .handle_sampling_event(SamplingEvent::AttemptStarted {
                    request_id: request_id.clone(),
                    attempt: 1,
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::ResponseStarted {
                    request_id: request_id.clone(),
                    message_id: "remote".into(),
                    model: "test".into(),
                    input_tokens: 1,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::ChannelToken {
                    request_id: request_id.clone(),
                    channel: SamplingChannel::Text,
                    text: "provisional".into(),
                    chunk_index: 1,
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::ChannelToken {
                    request_id: request_id.clone(),
                    channel: SamplingChannel::Reasoning,
                    text: "thinking".into(),
                    chunk_index: 2,
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::ToolCallDelta {
                    request_id: request_id.clone(),
                    tool_index: 0,
                    id: Some("call".into()),
                    name: Some("tool".into()),
                    arguments_delta: Some("{}".into()),
                })
                .await;
            actor
                .handle_sampling_event(SamplingEvent::ReasoningCompleted {
                    request_id: request_id.clone(),
                    signature: "signature".into(),
                })
                .await;
            let response = sampling_types::ConversationResponse {
                items: vec![sampling_types::ConversationItem::assistant("provisional")],
                stop_reason: Some(sampling_types::StopReason::Stop),
                usage: None,
                cost_usd_ticks: None,
                message_chunks_emitted: 1,
                doom_loop_signals: vec![],
                stop_message: None,
                message_id: None,
                provider_terminal: None,
                native_continuation: None,
            };
            actor
                .handle_sampling_event(SamplingEvent::Completed {
                    request_id,
                    response: Box::new(response),
                    metrics: Default::default(),
                })
                .await;
            assert_eq!(
                actor.sampling_preview.lock().as_ref(),
                Some(&("candidate".into(), 1))
            );
            let mut preview_count = 0;
            while let Ok(event) = rx.try_recv() {
                if let SessionEvent::Notification(notification) = event {
                    let meta = match notification {
                        SessionNotification::Acp(notification) => {
                            notification.meta.map(serde_json::Value::Object)
                        }
                        SessionNotification::Grow(notification) => {
                            assert!(!matches!(
                                notification.update,
                                GrowUpdate::SamplingAttempt {
                                    state: SamplingAttemptState::Accepted,
                                    ..
                                }
                            ));
                            if matches!(notification.update, GrowUpdate::SamplingAttempt { .. }) {
                                continue;
                            }
                            notification.meta
                        }
                    };
                    let meta = meta.expect("preview metadata");
                    assert_eq!(meta["samplingRequestId"], "candidate");
                    assert_eq!(meta["samplingAttempt"], 1);
                    preview_count += 1;
                }
            }
            assert_eq!(
                preview_count, 5,
                "text, reasoning, tool, signature and response-start are scoped"
            );
            // An admission error/drop closes the candidate, never accepts it.
            drop(updates::SamplingPreviewGuard(&actor));
            let Some(SessionEvent::Notification(SessionNotification::Grow(discard))) =
                rx.recv().await
            else {
                panic!("discard boundary")
            };
            assert!(matches!(
                discard.update,
                GrowUpdate::SamplingAttempt {
                    state: SamplingAttemptState::Discarded,
                    ..
                }
            ));
            assert!(actor.sampling_preview.lock().is_none());
        })
        .await;
}

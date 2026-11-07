//! F1: restore an already-admitted Steer whose target Turn survives in a
//! suspended/restoring checkpoint back into the actor's interjection buffer,
//! keeping its original identity and content. Non-preserved targets keep the
//! existing deterministic reroute-to-FIFO path.

use super::*;
use crate::session::actor::interjection::PendingInterjection;

/// Admit a text+image Steer bound to the actor's active durable turn through
/// the real durable admission path (one Submitted, one Hook, one Allowed).
/// Returns (input_id, target_turn); the durable turn id is uuid-minted.
async fn admit_preservable_steer(
    actor: &SessionActor,
    prompt_id: &str,
) -> (String, chat_state::TurnId) {
    support::begin_test_active_causal_turn(actor).await;
    let target_turn = actor
        .events
        .current_turn()
        .expect("test actor has an active durable turn");
    let mut payload = crate::session::input_inbox::tests::image_payload();
    let crate::session::input_inbox::InputPayload::Prompt {
        prompt_id: payload_prompt_id,
        requested_route,
        ..
    } = &mut payload;
    *payload_prompt_id = prompt_id.into();
    *requested_route = crate::session::input_inbox::InputRequestRoute::Steer {
        expected_turn_id: "fixture-active".into(),
    };
    let input_id = actor
        .admit_human_input(
            chat_state::InputIntent::Steer,
            payload,
            Some(prompt_id.into()),
            chat_state::InputRoute::Steer { target_turn },
            Vec::new(),
        )
        .await
        .expect("steer admitted against the active turn")
        .input_id;
    (input_id, target_turn)
}

#[tokio::test(flavor = "current_thread")]
async fn preserved_checkpoint_turn_restores_steer_into_interjection_buffer() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway) = support::build_actor().await;
            let (input_id, target_turn) = admit_preservable_steer(&actor, "steer-prompt").await;

            // Durable suspension with a checkpoint that preserves the target
            // Turn, exactly like the runtime suspension fixture.
            actor
                .state
                .lock()
                .await
                .runtime_suspend
                .store(true, std::sync::atomic::Ordering::Release);
            let operation_id = "steer-restore-op";
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                    chat_state::session_runtime::SessionRuntimeEvent::SuspendRequested {
                        operation_id: operation_id.into(),
                        root_session_id: actor.session_id_string(),
                    },
                ))
                .await
                .unwrap();
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                    chat_state::session_runtime::SessionRuntimeEvent::CommandsSettled {
                        operation_id: operation_id.into(),
                        commands: vec![],
                    },
                ))
                .await
                .unwrap();
            actor
                .chat_state_handle
                .checkpoint_runtime(
                    operation_id.into(),
                    actor.runtime_source_hash().await.unwrap(),
                    Some(chat_state::session_runtime::TurnContinuation {
                        turn_id: target_turn,
                        prompt_id: "fixture-prompt".into(),
                        prompt_index: 0,
                        origin: serde_json::to_string(&crate::session::PromptOrigin::User).unwrap(),
                        next_step_index: 0,
                        tool_turns_used: 0,
                        tool_calls_used: 0,
                        stop_continuations_used: 0,
                        output_schema: None,
                        resume_point: chat_state::session_runtime::TurnResumePoint::Model,
                        task_output_usage: None,
                        accepted_response: None,
                        structured_output_retries: 0,
                        sampling_attempts_used: 0,
                        sampling_max_attempts: None,
                        sampling_replay_safe: true,
                        sampling_deadline_ms: None,
                        operations: vec![],
                    }),
                    Some(chat_state::StepId {
                        turn: target_turn,
                        index: 0,
                    }),
                    tokio::time::Instant::now() + std::time::Duration::from_secs(5),
                )
                .await
                .unwrap();

            let before = actor.chat_state_handle.timeline_events().await.unwrap();
            actor.restore_pending_human_inputs().await.unwrap();

            // Rebuilt buffer entry keeps full identity and content.
            let entries: Vec<PendingInterjection<acp::ImageContent>> =
                actor.pending_interjections.drain_all();
            assert_eq!(entries.len(), 1, "one steer must return to the buffer");
            let entry = entries.into_iter().next().unwrap();
            assert_eq!(entry.text, "describe [Image #1]\n\nafter image");
            assert_eq!(
                entry.attachments.len(),
                1,
                "image attachment survives restoration"
            );
            let requeue = entry.requeue.expect("user steer keeps its requeue payload");
            assert_eq!(requeue.input_ids, vec![input_id.clone()]);
            assert_eq!(requeue.prompt_id, "steer-prompt");
            assert_eq!(requeue.origin, crate::session::PromptOrigin::User);
            assert_eq!(requeue.turn_kind, crate::session::TurnKind::User);
            assert!(requeue.verbatim, "verbatim survives restoration");
            assert!(requeue.json_schema.is_some(), "json_schema survives");
            assert!(requeue.client_identifier.is_none() && requeue.screen_mode.is_none());

            // No FIFO row and no new admission facts.
            assert!(
                actor.state.lock().await.pending_inputs.is_empty(),
                "preserved steer must not enter the FIFO"
            );
            let after = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(
                after.len(),
                before.len(),
                "reconstruction records nothing new"
            );
            assert!(!after.iter().any(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::Input(chat_state::InputEvent::Rerouted {
                    input_ids,
                    ..
                }) if input_ids == &vec![input_id.clone()]
            )));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn closed_interrupted_turn_still_reroutes_the_pending_steer_once() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway) = support::build_actor().await;
            let (input_id, _target_turn) = admit_preservable_steer(&actor, "steer-prompt").await;

            // Non-checkpoint process interruption: recovery durably closes the
            // target Turn, so reconstruction follows the existing reroute path.
            actor
                .chat_state_handle
                .recover_interrupted_durably()
                .await
                .unwrap();
            actor.restore_pending_human_inputs().await.unwrap();

            assert!(actor.pending_interjections.is_empty());
            let state = actor.state.lock().await;
            assert_eq!(state.pending_inputs.len(), 1);
            assert_eq!(
                state.pending_inputs.front().unwrap().input_ids,
                vec![input_id.clone()]
            );
            drop(state);
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Rerouted { input_ids, route }
                        ) if input_ids == &vec![input_id.clone()]
                            && route == &chat_state::InputRoute::Fifo
                    ))
                    .count(),
                1,
                "the closed target reroutes exactly once into the FIFO"
            );
        })
        .await;
}

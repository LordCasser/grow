//! Durable human-input admission and recovery helpers.

use super::*;

pub(super) struct AdmittedInput {
    pub input_id: String,
    pub replayed: bool,
}

pub(super) enum InputAdmissionAttempt {
    New {
        input_id: String,
        decision: chat_state::InputAdmissionDecision,
    },
    Existing {
        input_id: String,
        state: ExistingInputState,
    },
}

pub(super) enum ExistingInputState {
    Submitted,
    Allowed,
    Blocked(chat_state::InputBlockReason),
    Consumed,
    Handled,
    Dismissed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::input_inbox::{self, InputPayload};

    struct JsonlTimelinePersistence {
        storage: crate::session::storage::jsonl::JsonlStorageAdapter,
        info: crate::session::info::Info,
    }

    impl chat_state::TimelinePersistence for JsonlTimelinePersistence {
        fn persist_timeline_event_and_ack(
            &mut self,
            event: &chat_state::TimelineEvent,
        ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
            use crate::session::storage::StorageAdapter as _;

            let storage = self.storage.clone();
            let info = self.info.clone();
            let event = event.clone();
            let (respond_to, receiver) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let _ = respond_to.send(storage.append_timeline_event_durable(&info, &event).await);
            });
            receiver
        }

        fn flush(&mut self) {}
    }

    async fn admit_image(actor: &SessionActor) -> String {
        actor
            .admit_human_input(
                chat_state::InputIntent::Prompt,
                input_inbox::tests::image_payload(),
                Some("prompt-1".into()),
                chat_state::InputRoute::Fifo,
                vec![],
            )
            .await
            .unwrap()
            .input_id
    }

    #[tokio::test(flavor = "current_thread")]
    async fn physical_writer_replacement_replays_stable_input_identity_once() {
        const ROOT_ENV: &str = "GROW_TEST_STABLE_INPUT_REPLACEMENT_ROOT";
        const READY_ENV: &str = "GROW_TEST_STABLE_INPUT_REPLACEMENT_READY";
        const TEST: &str = "session::actor::input_admission::tests::physical_writer_replacement_replays_stable_input_identity_once";
        const PROMPT_ID: &str = "stable-writer-replacement-prompt";

        fn fixture_info() -> crate::session::info::Info {
            crate::session::info::Info {
                id: acp::SessionId::new("test-actor"),
                cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            }
        }

        if let Ok(root) = std::env::var(ROOT_ENV) {
            use crate::session::storage::StorageAdapter as _;
            use chat_state::{HookEvent, HookEventType, Timeline, TimelineEventKind};

            let root = std::path::PathBuf::from(root);
            let ready = std::path::PathBuf::from(std::env::var(READY_ENV).unwrap());
            let info = fixture_info();
            let storage = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root);
            storage
                .init_session(&info, crate::session::persistence::default_model_id())
                .await
                .unwrap();
            let opened = storage.open_session(&info).unwrap();
            let payload = input_inbox::tests::text_payload(PROMPT_ID, "durable submitted input");
            let payload_ref = input_inbox::write_payload(opened.directory(), &payload).unwrap();
            let input_id = input_inbox::input_id_for_prompt(info.id.0.as_ref(), PROMPT_ID);
            let occurrence_id = format!("hook-{input_id}");
            let mut timeline = Timeline::default();
            let submitted = timeline
                .record(TimelineEventKind::Input(
                    chat_state::InputEvent::Submitted {
                        input_id: input_id.clone(),
                        intent: chat_state::InputIntent::Prompt,
                        payload_ref,
                    },
                ))
                .unwrap();
            storage
                .append_timeline_event_durable(&info, &submitted)
                .await
                .unwrap();
            let triggered = timeline
                .record(TimelineEventKind::Hook(HookEvent::Triggered {
                    occurrence_id: occurrence_id.clone(),
                    event: HookEventType::UserPromptSubmit,
                    gate: chat_state::HookGateKind::Prompt,
                    cause: chat_state::HookCause::Input {
                        input_id: input_id.clone(),
                    },
                    config_generation: 1,
                    handlers: Vec::new(),
                }))
                .unwrap();
            storage
                .append_timeline_event_durable(&info, &triggered)
                .await
                .unwrap();
            let completed = timeline
                .record(TimelineEventKind::Hook(HookEvent::Completed {
                    occurrence_id,
                    decision: chat_state::HookAggregateDecision::Prompt {
                        decision: chat_state::HookGateDecision::Allow,
                    },
                }))
                .unwrap();
            storage
                .append_timeline_event_durable(&info, &completed)
                .await
                .unwrap();
            std::fs::write(ready, b"submitted and Hook facts durable")
                .expect("signal durable fixture readiness");
            loop {
                std::thread::park_timeout(std::time::Duration::from_secs(60));
            }
        }

        struct KillChildOnDrop(std::process::Child);
        impl Drop for KillChildOnDrop {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let temp = tempfile::TempDir::new().expect("isolated writer root");
        let root = temp.path().join("session-storage");
        let ready = temp.path().join("child-ready");
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env(ROOT_ENV, &root)
            .env(READY_ENV, &ready)
            .spawn()
            .unwrap();
        let mut child = KillChildOnDrop(child);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !ready.is_file() && std::time::Instant::now() < deadline {
            if let Some(status) = child.0.try_wait().unwrap() {
                panic!("fixture writer exited before readiness: {status}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(ready.is_file(), "child did not publish durable input facts");
        assert_eq!(
            std::fs::read(&ready).unwrap(),
            b"submitted and Hook facts durable"
        );

        let info = fixture_info();
        let observer = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.clone());
        assert!(
            observer.claim_recovery_writer(&info).is_err(),
            "the physical writer process must exclude its replacement"
        );
        child.0.kill().unwrap();
        let _ = child.0.wait().unwrap();

        let replacement = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root);
        replacement
            .claim_recovery_writer(&info)
            .expect("process exit releases the OS writer lease");
        let loaded = replacement
            .load_session_for_write_without_updates(&info)
            .await
            .expect("replacement loads the committed session");
        let original_input_id = input_inbox::input_id_for_prompt(info.id.0.as_ref(), PROMPT_ID);
        let original_lifecycle = loaded
            .timeline
            .input_state(&original_input_id)
            .expect("replacement recovers Submitted state before lifecycle repair");
        assert!(matches!(
            &original_lifecycle.state,
            chat_state::InputState::Submitted
        ));
        let original_payload_ref = loaded
            .timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted {
                    input_id,
                    payload_ref,
                    ..
                }) if input_id == &original_input_id => Some(payload_ref.clone()),
                _ => None,
            })
            .expect("durable Submitted payload reference");
        let original_payload = input_inbox::read_payload(
            replacement.open_session(&info).unwrap().directory(),
            &original_payload_ref,
        )
        .expect("replacement can read the complete original payload");
        assert_eq!(
            serde_json::to_value(&original_payload).unwrap(),
            serde_json::to_value(input_inbox::tests::text_payload(
                PROMPT_ID,
                "durable submitted input"
            ))
            .unwrap()
        );

        let (chat_event_tx, _chat_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let chat_state = chat_state::ChatStateActor::spawn_from_timeline(
            loaded.timeline.events().to_vec(),
            sampling_types::SamplingConfig {
                base_url: "http://localhost".into(),
                model: "test".into(),
                output_limit: None,
                temperature: None,
                top_p: None,
                api_backend: Default::default(),
                extra_headers: Default::default(),
                query_params: Default::default(),
                env_http_headers: Default::default(),
                context_window: std::num::NonZeroU64::new(256_000).unwrap(),
                reasoning_effort: None,
                stream_tool_calls: None,
            },
            Box::new(JsonlTimelinePersistence {
                storage: replacement.clone(),
                info: info.clone(),
            }),
            chat_event_tx,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("replacement ChatState actor");
        let replaced_events = chat_state.timeline_events().await.unwrap();
        assert!(matches!(
            replaced_events.iter().find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Input(
                    chat_state::InputEvent::AdmissionResolved {
                        input_id,
                        decision:
                            chat_state::InputAdmissionDecision::Block {
                                reason: chat_state::InputBlockReason::ProcessInterrupted,
                            },
                        ..
                    },
                ) if input_id == &original_input_id => Some(()),
                _ => None,
            }),
            Some(())
        ));

        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut actor, _gateway) =
                    crate::session::actor::tests::support::build_actor().await;
                let directory = replacement.open_session(&info).unwrap().directory_handle();
                let directory_path = directory.display_path().to_path_buf();
                let actor = std::sync::Arc::get_mut(&mut actor)
                    .expect("test actor has one owner before injection");
                actor.session_dir = directory_path;
                actor.session_directory = directory;
                actor.events = crate::session::events::EventTracker::new(chat_state.clone());
                actor.chat_state_handle = chat_state.clone();

                let before_retry = chat_state.timeline_events().await.unwrap();
                let attempt = actor
                    .run_human_input_admission_hook(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload(PROMPT_ID, "durable submitted input"),
                        Some(PROMPT_ID.into()),
                    )
                    .await
                    .expect("stable retry reconciles against reloaded identity");
                assert!(matches!(
                    attempt,
                    InputAdmissionAttempt::Existing {
                        input_id,
                        state: ExistingInputState::Blocked(
                            chat_state::InputBlockReason::ProcessInterrupted
                        ),
                    } if input_id == original_input_id
                ));
                let retry_error = match actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload(PROMPT_ID, "durable submitted input"),
                        Some(PROMPT_ID.into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                {
                    Err(error) => error,
                    Ok(_) => panic!("interrupted stable input cannot be re-admitted"),
                };
                assert_eq!(retry_error, "input admission was interrupted");
                let after_retry = chat_state.timeline_events().await.unwrap();
                assert_eq!(after_retry.len(), before_retry.len());
                assert_eq!(
                    after_retry
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { input_id, .. }
                            ) if input_id == &original_input_id
                        ))
                        .count(),
                    1,
                    "replacement retry keeps one durable Submitted fact"
                );
                assert_eq!(
                    after_retry
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Triggered {
                                    event: chat_state::HookEventType::UserPromptSubmit,
                                    cause: chat_state::HookCause::Input { input_id },
                                    ..
                                }
                            ) if input_id == &original_input_id
                        ))
                        .count(),
                    1,
                    "replacement retry does not run the Hook again"
                );
                assert!(!after_retry.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. })
                        | chat_state::TimelineEventKind::Request(_)
                        | chat_state::TimelineEventKind::Tool(_)
                )));
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn input_identity_replay_reuses_admission_and_rejects_payload_conflicts() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let first = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("stable-prompt", "same request"),
                        Some("stable-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("first request admitted");
                assert!(!first.replayed);
                let after_first = actor.chat_state_handle.timeline_events().await.unwrap();
                let replay = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("stable-prompt", "same request"),
                        Some("stable-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("exact retry reuses admission");
                assert!(replay.replayed);
                assert_eq!(replay.input_id, first.input_id);
                let after_replay = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(after_replay.len(), after_first.len());

                let submitted_payload = after_first
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Submitted {
                                input_id,
                                payload_ref,
                                ..
                            },
                        ) if input_id == &first.input_id => Some(
                            input_inbox::read_payload(&actor.session_directory, payload_ref)
                                .unwrap(),
                        ),
                        _ => None,
                    })
                    .expect("submitted payload snapshot");
                assert_eq!(
                    serde_json::to_value(submitted_payload).unwrap(),
                    serde_json::to_value(input_inbox::tests::text_payload(
                        "stable-prompt",
                        "same request"
                    ))
                    .unwrap(),
                    "replay must retain the original immutable payload snapshot"
                );

                let conflict = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("stable-prompt", "changed request"),
                        Some("stable-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await;
                assert!(conflict.is_err());
                assert_eq!(
                    actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .len(),
                    after_first.len(),
                    "a conflicting payload must fail before Hook or admission"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interrupted_input_admission_recovers_blocked_without_repeating_hook_or_turn() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let prompt_id = "interrupted-admission-prompt";
                let payload = input_inbox::tests::text_payload(prompt_id, "preserve this payload");
                let attempt = actor
                    .run_human_input_admission_hook(
                        chat_state::InputIntent::Prompt,
                        payload.clone(),
                        Some(prompt_id.into()),
                    )
                    .await
                    .expect("submission and Hook facts are durable");
                let input_id = match attempt {
                    InputAdmissionAttempt::New {
                        input_id,
                        decision: chat_state::InputAdmissionDecision::Allow,
                    } => input_id,
                    _ => panic!("fixture Hook should leave a new Allow decision unresolved"),
                };

                let after_hook = actor.chat_state_handle.timeline_events().await.unwrap();
                let submitted_payload = after_hook
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Submitted {
                                input_id: submitted_id,
                                payload_ref,
                                ..
                            },
                        ) if submitted_id == &input_id => Some(
                            input_inbox::read_payload(&actor.session_directory, payload_ref)
                                .expect("original payload remains readable"),
                        ),
                        _ => None,
                    })
                    .expect("Submitted evidence exists");
                assert_eq!(
                    serde_json::to_value(&submitted_payload).unwrap(),
                    serde_json::to_value(&payload).unwrap()
                );
                assert_eq!(
                    after_hook
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { input_id: id, .. }
                            ) if id == &input_id
                        ))
                        .count(),
                    1
                );
                assert_eq!(
                    after_hook
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Triggered {
                                    event: chat_state::HookEventType::UserPromptSubmit,
                                    cause: chat_state::HookCause::Input { input_id: id },
                                    ..
                                }
                            ) if id == &input_id
                        ))
                        .count(),
                    1
                );
                let hook_occurrence_id = after_hook
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                            occurrence_id,
                            event: chat_state::HookEventType::UserPromptSubmit,
                            cause: chat_state::HookCause::Input { input_id: id },
                            ..
                        }) if id == &input_id => Some(occurrence_id),
                        _ => None,
                    })
                    .expect("input Hook occurrence");
                assert_eq!(
                    after_hook
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Completed { occurrence_id, .. }
                            ) if occurrence_id == hook_occurrence_id
                        ))
                        .count(),
                    1,
                    "fixture stops after the Hook has durably completed"
                );
                assert!(!after_hook.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(
                        chat_state::InputEvent::AdmissionResolved { input_id: id, .. }
                    ) if id == &input_id
                )));

                let unresolved_retry = match actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        payload.clone(),
                        Some(prompt_id.into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                {
                    Err(error) => error,
                    Ok(_) => panic!("Submitted without admission cannot be retried as accepted"),
                };
                assert!(unresolved_retry.contains("remains unresolved"));
                assert_eq!(
                    actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .len(),
                    after_hook.len(),
                    "an unresolved retry must not repeat Submitted or Hook facts"
                );

                let recovery = actor
                    .chat_state_handle
                    .recover_interrupted_durably()
                    .await
                    .expect("real durable interrupted-work recovery");
                assert!(recovery.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(
                        chat_state::InputEvent::AdmissionResolved {
                            input_id: id,
                            decision: chat_state::InputAdmissionDecision::Block {
                                reason: chat_state::InputBlockReason::ProcessInterrupted
                            },
                            ..
                        }
                    ) if id == &input_id
                )));

                let recovered_retry = match actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        payload.clone(),
                        Some(prompt_id.into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                {
                    Err(error) => error,
                    Ok(_) => panic!("recovered input must remain durably blocked"),
                };
                assert_eq!(recovered_retry, "input admission was interrupted");
                actor.restore_pending_human_inputs().await.unwrap();
                assert!(
                    actor.state.lock().await.pending_inputs.is_empty(),
                    "ProcessInterrupted inputs must not be restored into the FIFO"
                );

                let final_events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    final_events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { input_id: id, .. }
                            ) if id == &input_id
                        ))
                        .count(),
                    1,
                    "recovery and retry preserve one immutable submission"
                );
                assert_eq!(
                    final_events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(
                                chat_state::HookEvent::Triggered {
                                    event: chat_state::HookEventType::UserPromptSubmit,
                                    cause: chat_state::HookCause::Input { input_id: id },
                                    ..
                                }
                            ) if id == &input_id
                        ))
                        .count(),
                    1,
                    "retry must not run a second Hook"
                );
                assert_eq!(
                    final_events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::AdmissionResolved {
                                    input_id: id,
                                    decision: chat_state::InputAdmissionDecision::Block {
                                        reason: chat_state::InputBlockReason::ProcessInterrupted
                                    },
                                    ..
                                }
                            ) if id == &input_id
                        ))
                        .count(),
                    1,
                    "interruption recovery resolves admission exactly once"
                );
                assert!(!final_events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. })
                        | chat_state::TimelineEventKind::Request(_)
                        | chat_state::TimelineEventKind::Tool(_)
                )));
                let recovered_payload_ref = final_events
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Submitted {
                                input_id: id,
                                payload_ref,
                                ..
                            },
                        ) if id == &input_id => Some(payload_ref),
                        _ => None,
                    })
                    .expect("original submission still anchors its payload");
                assert_eq!(
                    serde_json::to_value(
                        input_inbox::read_payload(&actor.session_directory, recovered_payload_ref)
                            .unwrap()
                    )
                    .unwrap(),
                    serde_json::to_value(payload).unwrap(),
                    "interruption recovery must retain the original payload artifact"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn input_identity_rejects_fifo_steer_and_steer_target_conflicts() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let mut fifo = input_inbox::tests::text_payload("route-prompt", "same text");
                let InputPayload::Prompt {
                    requested_route, ..
                } = &mut fifo;
                *requested_route = input_inbox::InputRequestRoute::Fifo;
                actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        fifo,
                        Some("route-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .expect("initial FIFO input admitted");

                let mut steer = input_inbox::tests::text_payload("route-prompt", "same text");
                let InputPayload::Prompt {
                    requested_route, ..
                } = &mut steer;
                *requested_route = input_inbox::InputRequestRoute::Steer {
                    expected_turn_id: "1".into(),
                };
                assert!(
                    actor
                        .admit_human_input(
                            chat_state::InputIntent::Steer,
                            steer,
                            Some("route-prompt".into()),
                            chat_state::InputRoute::Steer {
                                target_turn: chat_state::TurnId(1),
                            },
                            Vec::new(),
                        )
                        .await
                        .is_err(),
                    "the same identity cannot change from FIFO to steer"
                );

                let (steer_actor, _gateway) =
                    crate::session::actor::tests::support::build_actor().await;
                crate::session::actor::tests::support::begin_test_active_causal_turn(&steer_actor)
                    .await;
                let target_turn = steer_actor
                    .events
                    .current_turn()
                    .expect("test actor has an active durable turn");
                let mut original = input_inbox::tests::text_payload("steer-prompt", "same text");
                let InputPayload::Prompt {
                    requested_route, ..
                } = &mut original;
                *requested_route = input_inbox::InputRequestRoute::Steer {
                    expected_turn_id: "1".into(),
                };
                steer_actor
                    .admit_human_input(
                        chat_state::InputIntent::Steer,
                        original,
                        Some("steer-prompt".into()),
                        chat_state::InputRoute::Steer { target_turn },
                        Vec::new(),
                    )
                    .await
                    .expect("initial steer input admitted");

                let mut changed_target =
                    input_inbox::tests::text_payload("steer-prompt", "same text");
                let InputPayload::Prompt {
                    requested_route, ..
                } = &mut changed_target;
                *requested_route = input_inbox::InputRequestRoute::Steer {
                    expected_turn_id: "2".into(),
                };
                assert!(
                    steer_actor
                        .admit_human_input(
                            chat_state::InputIntent::Steer,
                            changed_target,
                            Some("steer-prompt".into()),
                            chat_state::InputRoute::Steer {
                                target_turn: chat_state::TurnId(2),
                            },
                            Vec::new(),
                        )
                        .await
                        .is_err(),
                    "the same identity cannot change its original steer target"
                );

                for actor in [&actor, &steer_actor] {
                    let events = actor.chat_state_handle.timeline_events().await.unwrap();
                    assert_eq!(
                        events
                            .iter()
                            .filter(|event| matches!(
                                &event.kind,
                                chat_state::TimelineEventKind::Hook(
                                    chat_state::HookEvent::Triggered {
                                        event: chat_state::HookEventType::UserPromptSubmit,
                                        ..
                                    }
                                )
                            ))
                            .count(),
                        1,
                        "route conflicts must be rejected before a second Hook"
                    );
                }
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn same_text_with_a_new_identity_is_admitted_as_a_new_input() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let first = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("first-prompt", "same text"),
                        Some("first-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .unwrap();
                let second = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::text_payload("second-prompt", "same text"),
                        Some("second-prompt".into()),
                        chat_state::InputRoute::Fifo,
                        Vec::new(),
                    )
                    .await
                    .unwrap();

                assert_ne!(first.input_id, second.input_id);
                assert!(!first.replayed && !second.replayed);
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { .. }
                            )
                        ))
                        .count(),
                    2
                );
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                                event: chat_state::HookEventType::UserPromptSubmit,
                                ..
                            })
                        ))
                        .count(),
                    2
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shared_storage_failure_rejects_queued_input_before_hook_and_keeps_observation_readable()
     {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (mut actor, _gateway) =
                    crate::session::actor::tests::support::build_actor().await;
                let shared_gate =
                    std::sync::Arc::new(crate::session::storage_admission::StorageAdmission::new(
                        actor.session_dir.clone(),
                    ));
                let manager_clone = shared_gate.clone();
                manager_clone.record_failure(
                    actor.session_info.id.0.as_ref(),
                    "failed-writer",
                    None,
                    Some(std::io::ErrorKind::StorageFull),
                    "disk full",
                );
                std::sync::Arc::get_mut(&mut actor)
                    .expect("test actor has one owner")
                    .tool_context
                    .storage_admission = Some(shared_gate);

                let diagnostic_session_id = actor.runtime_observation.read().session_id.clone();
                let (respond_to, response) = tokio::sync::oneshot::channel();
                actor
                    .queue_input(
                        vec![acp::ContentBlock::Text(acp::TextContent::new(
                            "blocked input",
                        ))],
                        "blocked-prompt".into(),
                        crate::session::PromptOrigin::User,
                        crate::session::TurnKind::User,
                        None,
                        None,
                        false,
                        None,
                        respond_to,
                        None,
                    )
                    .await;

                assert!(response.await.unwrap().is_err());
                assert_eq!(
                    actor.runtime_observation.read().session_id,
                    diagnostic_session_id,
                    "read-only runtime diagnostics remain available during storage failure"
                );
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert!(!events.iter().any(|event| matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted { .. })
                        | chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                            event: chat_state::HookEventType::UserPromptSubmit,
                            ..
                        })
                )));
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_input_identity_replays_share_one_submission_and_hook() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let first = actor.admit_human_input(
                    chat_state::InputIntent::Prompt,
                    input_inbox::tests::text_payload("parallel-prompt", "same request"),
                    Some("parallel-prompt".into()),
                    chat_state::InputRoute::Fifo,
                    Vec::new(),
                );
                let second = actor.admit_human_input(
                    chat_state::InputIntent::Prompt,
                    input_inbox::tests::text_payload("parallel-prompt", "same request"),
                    Some("parallel-prompt".into()),
                    chat_state::InputRoute::Fifo,
                    Vec::new(),
                );
                let (first, second) = tokio::join!(first, second);
                let first = first.expect("first admission");
                let second = second.expect("concurrent retry");
                assert_eq!(first.input_id, second.input_id);
                assert_ne!(first.replayed, second.replayed);
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Input(
                                chat_state::InputEvent::Submitted { .. }
                            )
                        ))
                        .count(),
                    1
                );
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| matches!(
                            &event.kind,
                            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                                event: chat_state::HookEventType::UserPromptSubmit,
                                ..
                            })
                        ))
                        .count(),
                    1
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn large_input_recovery_reuses_admission_and_snapshot() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let input_id = admit_image(&actor).await;
                let before = actor.chat_state_handle.timeline_events().await.unwrap();
                actor.restore_pending_human_inputs().await.unwrap();
                let state = actor.state.lock().await;
                let restored = state.pending_inputs.front().unwrap();
                assert_eq!(restored.input_ids, vec![input_id]);
                let InputPayload::Prompt { prompt_blocks, .. } =
                    input_inbox::tests::image_payload();
                assert_eq!(
                    serde_json::to_value(&restored.prompt_blocks).unwrap(),
                    serde_json::to_value(prompt_blocks).unwrap()
                );
                drop(state);
                assert_eq!(
                    before.len(),
                    actor
                        .chat_state_handle
                        .timeline_events()
                        .await
                        .unwrap()
                        .len(),
                    "recovery must not repeat Hooks or admission"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_input_attachment_is_invalidated_without_blocking_other_inputs() {
        tokio::task::LocalSet::new().run_until(async {
            let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
            let missing_id = admit_image(&actor).await;
            let image_dir = actor.session_dir.join("artifacts/inputs/images");
            for path in std::fs::read_dir(&image_dir).unwrap() {
                std::fs::remove_file(path.unwrap().path()).unwrap();
            }
            let mut text = input_inbox::tests::image_payload();
            let InputPayload::Prompt { prompt_blocks, prompt_id, .. } = &mut text;
            prompt_blocks.retain(|block| matches!(block, acp::ContentBlock::Text(_)));
            *prompt_id = "healthy".into();
            let healthy_id = actor.admit_human_input(chat_state::InputIntent::Prompt, text, Some("healthy".into()), chat_state::InputRoute::Fifo, vec![]).await.unwrap().input_id;
            actor.restore_pending_human_inputs().await.unwrap();
            let state = actor.state.lock().await;
            assert_eq!(state.pending_inputs.len(), 1);
            assert_eq!(state.pending_inputs.front().unwrap().input_ids, vec![healthy_id]);
            drop(state);
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(events.iter().any(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Input(chat_state::InputEvent::Dismissed { input_ids, reason: chat_state::InputDismissReason::Invalidated }) if input_ids == &vec![missing_id.clone()]
            )));
            assert_eq!(actor.chat_state_handle.submitted_input_payload_hashes().await.unwrap().len(), 2, "invalidated snapshots remain audit roots");
        }).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_failure_retry_has_one_submission_and_gc_keeps_consumed_images() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway) = crate::session::actor::tests::support::build_actor().await;
                let artifact_path = actor.session_dir.join("artifacts");
                std::fs::write(&artifact_path, b"fault injection").unwrap();
                let failure = actor
                    .admit_human_input(
                        chat_state::InputIntent::Prompt,
                        input_inbox::tests::image_payload(),
                        Some("prompt-1".into()),
                        chat_state::InputRoute::Fifo,
                        vec![],
                    )
                    .await;
                assert!(failure.is_err());
                assert!(
                    actor
                        .chat_state_handle
                        .submitted_input_payload_hashes()
                        .await
                        .unwrap()
                        .is_empty()
                );
                std::fs::remove_file(artifact_path).unwrap();
                let input_id = admit_image(&actor).await;
                let turn = chat_state::TurnId(1);
                actor
                    .chat_state_handle
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Turn(
                        chat_state::TurnEvent::Started {
                            id: turn,
                            input_ids: vec![input_id.clone()],
                            identity: crate::session::PromptOrigin::User
                                .turn_identity(crate::session::TurnKind::User),
                            model_id: "test".into(),
                            input_message_count: 0,
                            prompt_index: 0,
                            prompt_text: "image".into(),
                            input_kind: chat_state::TurnInputKind::Prompt,
                            redirect_kind: None,
                        },
                    ))
                    .await
                    .unwrap();
                actor
                    .consume_fifo_inputs(
                        vec![input_id],
                        turn,
                        sampling_types::ConversationItem::user("consumed image"),
                    )
                    .await
                    .unwrap();
                assert!(
                    actor
                        .chat_state_handle
                        .get_pending_allowed_inputs()
                        .await
                        .unwrap()
                        .is_empty()
                );
                actor
                    .reconcile_input_payloads(&tokio_util::sync::CancellationToken::new())
                    .await;
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let refs = events
                    .iter()
                    .filter_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Input(
                            chat_state::InputEvent::Submitted { payload_ref, .. },
                        ) => Some(payload_ref),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(refs.len(), 1);
                assert_eq!(
                    input_inbox::read_payload(&actor.session_directory, refs[0])
                        .unwrap()
                        .hook_prompt(),
                    input_inbox::tests::image_payload().hook_prompt()
                );
            })
            .await;
    }
}

impl SessionActor {
    pub(super) async fn admit_human_input(
        &self,
        intent: chat_state::InputIntent,
        payload: crate::session::input_inbox::InputPayload,
        prompt_id: Option<String>,
        route: chat_state::InputRoute,
        supersedes: Vec<String>,
    ) -> Result<AdmittedInput, String> {
        let _admission_guard = self.input_admission_gate.lock().await;
        let attempt = self
            .run_human_input_admission_hook(intent, payload, prompt_id.clone())
            .await?;
        let (input_id, decision) = match attempt {
            InputAdmissionAttempt::New { input_id, decision } => (input_id, decision),
            InputAdmissionAttempt::Existing { input_id, state } => {
                return match state {
                    ExistingInputState::Submitted => Err(format!(
                        "input admission remains unresolved for prompt {prompt_id:?} (input {input_id})"
                    )),
                    ExistingInputState::Allowed => Ok(AdmittedInput {
                        input_id,
                        replayed: true,
                    }),
                    ExistingInputState::Blocked(reason) => Err(input_block_reason_message(reason)),
                    ExistingInputState::Consumed => Err(format!(
                        "input was already consumed for prompt {prompt_id:?} (input {input_id})"
                    )),
                    ExistingInputState::Handled => Err(format!(
                        "input was already handled for prompt {prompt_id:?} (input {input_id})"
                    )),
                    ExistingInputState::Dismissed => Err(format!(
                        "input was already dismissed for prompt {prompt_id:?} (input {input_id})"
                    )),
                };
            }
        };
        let (route, supersedes) = if matches!(decision, chat_state::InputAdmissionDecision::Allow) {
            (Some(route), supersedes)
        } else {
            (None, Vec::new())
        };
        let admitted = self
            .resolve_human_input_admission(input_id, decision, route, supersedes)
            .await?;
        Ok(AdmittedInput {
            replayed: false,
            ..admitted
        })
    }

    /// Publish a HumanIntent and run its one UserPromptSubmit Hook occurrence,
    /// but leave the final admission decision to the caller. Direct steering
    /// uses this split phase so target liveness is checked after a potentially
    /// slow Hook without losing the submitted input fact.
    pub(super) async fn run_human_input_admission_hook(
        &self,
        intent: chat_state::InputIntent,
        payload: crate::session::input_inbox::InputPayload,
        prompt_id: Option<String>,
    ) -> Result<InputAdmissionAttempt, String> {
        let payload_prompt_id = match &payload {
            crate::session::input_inbox::InputPayload::Prompt { prompt_id, .. } => prompt_id,
        };
        if payload_prompt_id.is_empty() {
            return Err("input prompt identity is empty".into());
        }
        let requested_route = match &payload {
            crate::session::input_inbox::InputPayload::Prompt {
                requested_route, ..
            } => requested_route,
        };
        let route_matches_intent = matches!(
            (intent, requested_route),
            (
                chat_state::InputIntent::Prompt | chat_state::InputIntent::Followup,
                crate::session::input_inbox::InputRequestRoute::Fifo
            ) | (
                chat_state::InputIntent::Steer,
                crate::session::input_inbox::InputRequestRoute::Steer { .. }
            )
        );
        if !route_matches_intent {
            return Err("input route does not match its immutable request intent".into());
        }
        if prompt_id
            .as_deref()
            .is_some_and(|id| id != payload_prompt_id)
        {
            return Err("input prompt identity does not match its immutable payload".into());
        }
        let prompt_id = payload_prompt_id.clone();
        let input_id = crate::session::input_inbox::input_id_for_prompt(
            self.session_info.id.0.as_ref(),
            &prompt_id,
        );
        let compare_payload = payload.clone();
        let artifact_guard = self.input_artifact_gate.lock().await;
        let directory = self
            .session_directory
            .try_clone()
            .map_err(|error| format!("input artifact directory unavailable: {error}"))?;
        let (payload_ref, hook_prompt) = tokio::task::spawn_blocking(move || {
            let reference = crate::session::input_inbox::write_payload(&directory, &payload)?;
            // Serialization/decoded-image budgets have now been checked. The
            // Hook reviews this exact snapshot, never a mutable source path.
            Ok::<_, std::io::Error>((reference, payload.hook_prompt()))
        })
        .await
        .map_err(|error| format!("input artifact writer failed: {error}"))?
        .map_err(|error| format!("input artifact write failed: {error}"))?;

        let events = self
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or_else(|| "Timeline unavailable while reconciling input identity".to_string())?;
        if let Some(existing_id) = self
            .find_existing_prompt_input(&events, &prompt_id, intent, &compare_payload)
            .await?
        {
            drop(artifact_guard);
            return Ok(InputAdmissionAttempt::Existing {
                state: existing_input_state(&events, &existing_id)?,
                input_id: existing_id,
            });
        }

        let submitted = self
            .chat_state_handle
            .submit_input_durably(input_id.clone(), intent, payload_ref.clone())
            .await
            .map_err(|error| format!("input submission was not durable: {error}"))?;
        if !submitted.created {
            drop(artifact_guard);
            let events = self
                .chat_state_handle
                .timeline_events()
                .await
                .ok_or_else(|| {
                    "Timeline unavailable while reconciling input identity".to_string()
                })?;
            return Ok(InputAdmissionAttempt::Existing {
                state: existing_input_state(&events, &input_id)?,
                input_id,
            });
        }
        // The blob is now reachable from Timeline, so maintenance can no longer
        // classify it as an orphan. Do not serialize slow external Hook runs on
        // the artifact-maintenance gate.
        drop(artifact_guard);

        let envelope = self.make_hook_envelope(
            ::hooks::event::HookEventName::UserPromptSubmit,
            Some(prompt_id),
            ::hooks::event::HookPayload::UserPromptSubmit {
                prompt: Some(hook_prompt),
            },
        );
        let aggregate = self
            .dispatch_prompt_hook(
                chat_state::HookCause::Input {
                    input_id: input_id.clone(),
                },
                envelope,
                ::hooks::event::GateKind::Prompt,
            )
            .await
            .map_err(|error| format!("input hook lifecycle was not durable: {error}"))?;
        let decision = match aggregate {
            HookAggregate::Prompt { decision, .. } => decision,
            _ => unreachable!("Prompt gate returned a non-Prompt aggregate"),
        };
        let decision = match decision {
            ::hooks::result::HookDecision::Allow => chat_state::InputAdmissionDecision::Allow,
            ::hooks::result::HookDecision::Deny { reason, .. } => {
                chat_state::InputAdmissionDecision::Block {
                    reason: chat_state::InputBlockReason::Hook {
                        reason: reason.clone(),
                    },
                }
            }
        };
        Ok(InputAdmissionAttempt::New { input_id, decision })
    }

    async fn find_existing_prompt_input(
        &self,
        events: &[chat_state::TimelineEvent],
        prompt_id: &str,
        intent: chat_state::InputIntent,
        requested: &crate::session::input_inbox::InputPayload,
    ) -> Result<Option<String>, String> {
        let mut matched = None;
        for event in events {
            let chat_state::TimelineEventKind::Input(chat_state::InputEvent::Submitted {
                input_id,
                intent: existing_intent,
                payload_ref,
            }) = &event.kind
            else {
                continue;
            };
            let existing_prompt_id =
                crate::session::input_inbox::read_prompt_id(&self.session_directory, payload_ref)
                    .map_err(|error| {
                    format!("input identity evidence is unavailable for {input_id}: {error}")
                })?;
            if existing_prompt_id != prompt_id {
                continue;
            }
            if matched.is_some() {
                return Err(format!(
                    "input identity evidence is ambiguous for prompt {prompt_id}"
                ));
            }
            if existing_intent != &intent {
                return Err(format!(
                    "input identity conflict for prompt {prompt_id}: intent differs"
                ));
            }
            let existing_payload =
                crate::session::input_inbox::read_payload(&self.session_directory, payload_ref)
                    .map_err(|error| {
                        format!("input payload evidence is unavailable for {input_id}: {error}")
                    })?;
            let existing_value = serde_json::to_value(existing_payload)
                .map_err(|error| format!("existing input payload cannot be compared: {error}"))?;
            let requested_value = serde_json::to_value(requested)
                .map_err(|error| format!("requested input payload cannot be compared: {error}"))?;
            if existing_value != requested_value {
                return Err(format!(
                    "input identity conflict for prompt {prompt_id}: immutable payload or route differs"
                ));
            }
            matched = Some(input_id.clone());
        }
        Ok(matched)
    }

    pub(super) async fn resolve_human_input_admission(
        &self,
        input_id: String,
        decision: chat_state::InputAdmissionDecision,
        route: Option<chat_state::InputRoute>,
        supersedes: Vec<String>,
    ) -> Result<AdmittedInput, String> {
        self.record_input_event(chat_state::InputEvent::AdmissionResolved {
            input_id: input_id.clone(),
            decision: decision.clone(),
            route,
            supersedes,
        })
        .await?;
        if let chat_state::InputAdmissionDecision::Block { reason } = decision {
            return Err(match reason {
                chat_state::InputBlockReason::Hook { reason } => reason,
                chat_state::InputBlockReason::StaleSteerTarget => {
                    "the target turn is no longer running".to_string()
                }
                chat_state::InputBlockReason::ProcessInterrupted => {
                    "input admission was interrupted".to_string()
                }
            });
        }
        Ok(AdmittedInput {
            input_id,
            replayed: false,
        })
    }

    pub(super) async fn reroute_input(
        &self,
        input_id: String,
        route: chat_state::InputRoute,
    ) -> Result<(), String> {
        self.reroute_input_ids(vec![input_id], route).await
    }

    pub(super) async fn reroute_input_ids(
        &self,
        input_ids: Vec<String>,
        route: chat_state::InputRoute,
    ) -> Result<(), String> {
        self.record_input_event(chat_state::InputEvent::Rerouted { input_ids, route })
            .await
    }

    pub(super) async fn consume_steer_inputs(
        &self,
        input_ids: Vec<String>,
        turn: chat_state::TurnId,
        item: sampling_types::ConversationItem,
    ) -> Result<(), String> {
        self.record_input_event(chat_state::InputEvent::Consumed {
            input_ids,
            turn,
            item,
        })
        .await
    }

    pub(super) async fn consume_fifo_inputs(
        &self,
        input_ids: Vec<String>,
        turn: chat_state::TurnId,
        item: sampling_types::ConversationItem,
    ) -> Result<(), String> {
        self.record_input_event(chat_state::InputEvent::Consumed {
            input_ids,
            turn,
            item,
        })
        .await
    }

    pub(super) async fn complete_unmodeled_fifo_inputs(
        &self,
        input_ids: Vec<String>,
        turn: chat_state::TurnId,
    ) -> Result<(), String> {
        self.record_input_event(chat_state::InputEvent::Handled { input_ids, turn })
            .await
    }

    pub(super) async fn dismiss_input_ids(
        &self,
        input_ids: impl IntoIterator<Item = String>,
        reason: chat_state::InputDismissReason,
    ) -> Result<(), String> {
        let input_ids = input_ids.into_iter().collect::<Vec<_>>();
        if input_ids.is_empty() {
            return Ok(());
        }
        self.record_input_event(chat_state::InputEvent::Dismissed { input_ids, reason })
            .await
    }

    async fn record_input_event(&self, event: chat_state::InputEvent) -> Result<(), String> {
        self.chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Input(event))
            .await
            .map(|_| ())
            .map_err(|error| format!("input lifecycle was not durable: {error}"))
    }

    pub(super) async fn restore_pending_human_inputs(&self) -> Result<(), String> {
        let pending = self
            .chat_state_handle
            .get_pending_allowed_inputs()
            .await
            .ok_or_else(|| "Timeline unavailable while restoring pending inputs".to_string())?;
        let directory = self
            .session_directory
            .try_clone()
            .map_err(|error| format!("input artifact directory unavailable: {error}"))?;
        for pending in pending {
            let directory = directory
                .try_clone()
                .map_err(|error| format!("input artifact directory unavailable: {error}"))?;
            let payload_ref = pending.payload_ref.clone();
            let payload = tokio::task::spawn_blocking(move || {
                crate::session::input_inbox::read_payload(&directory, &payload_ref)
            })
            .await
            .map_err(|error| format!("input artifact reader failed: {error}"))?;
            let payload = match payload {
                Ok(payload) => payload,
                Err(error) if crate::session::input_inbox::is_attachment_error(&error) => {
                    // Keep Submitted and its references as audit evidence. An
                    // allowed input whose snapshot cannot be restored must not
                    // become a text-only turn or prevent unrelated history use.
                    self.dismiss_input_ids(
                        [pending.input_id.clone()],
                        chat_state::InputDismissReason::Invalidated,
                    )
                    .await?;
                    self.persist_ui_notice(crate::extensions::notification::UiNotice {
                        correlation_id: pending.input_id,
                        category: crate::extensions::notification::UiNoticeCategory::Lifecycle,
                        subject: Some("input attachment".into()),
                        description: None,
                        message: "A pending input was stopped because an image attachment is unavailable. Reattach the image and submit a new prompt.".into(),
                        tone: crate::extensions::notification::UiNoticeTone::Error,
                        details: Some(error.to_string()),
                    }).await?;
                    continue;
                }
                Err(error) => return Err(format!("input payload is missing or corrupt: {error}")),
            };
            if !crate::session::input_inbox::payload_matches_intent(pending.intent, &payload) {
                return Err("input intent does not match its payload artifact".into());
            }
            match (pending.intent, payload) {
                (
                    chat_state::InputIntent::Prompt
                    | chat_state::InputIntent::Followup
                    | chat_state::InputIntent::Steer,
                    crate::session::input_inbox::InputPayload::Prompt {
                        prompt_id,
                        requested_route: _,
                        prompt_blocks,
                        client_identifier,
                        screen_mode,
                        verbatim,
                        json_schema,
                        origin,
                        turn_kind,
                        queue,
                    },
                ) => {
                    if origin != crate::session::PromptOrigin::User
                        || turn_kind != crate::session::TurnKind::User
                    {
                        return Err("pending human input payload has a synthetic identity".into());
                    }
                    if matches!(pending.route, chat_state::InputRoute::Steer { .. }) {
                        self.reroute_input(pending.input_id.clone(), chat_state::InputRoute::Fifo)
                            .await?;
                    } else if !matches!(pending.route, chat_state::InputRoute::Fifo) {
                        return Err("pending prompt input has an incompatible route".into());
                    }
                    let queue_meta = queue.map(Into::into).unwrap_or_else(|| {
                        crate::session::prompt_queue::QueueEntryMeta {
                            id: prompt_id.clone(),
                            version: 0,
                            owner: client_identifier.clone(),
                            last_editor: None,
                            kind: "prompt".into(),
                            text: Self::queue_text_from_blocks(&prompt_blocks),
                            combined_texts: None,
                        }
                    });
                    let (respond_to, _response) = tokio::sync::oneshot::channel();
                    self.state.lock().await.pending_inputs.push_back(InputItem {
                        input_ids: vec![pending.input_id],
                        prompt_id,
                        turn_kind,
                        prompt_blocks,
                        client_identifier,
                        screen_mode,
                        verbatim,
                        json_schema,
                        origin,
                        host_command: None,
                        notification_ids: Vec::new(),
                        respond_to,
                        persist_ack: None,
                        queue_meta: Some(queue_meta),
                    });
                }
            }
        }
        Ok(())
    }

    pub(super) async fn reconcile_input_payloads(
        &self,
        shutdown: &tokio_util::sync::CancellationToken,
    ) {
        let directory = match self.session_directory.try_clone() {
            Ok(directory) => directory,
            Err(error) => {
                tracing::warn!(%error, "input payload reconciliation directory unavailable");
                return;
            }
        };
        let (batch_tx, mut batch_rx) = tokio::sync::mpsc::channel::<Vec<String>>(1);
        let producer_shutdown = shutdown.clone();
        let producer = tokio::task::spawn_blocking(move || {
            crate::session::input_inbox::visit_payload_hash_batches(
                &directory,
                || batch_tx.is_closed() || producer_shutdown.is_cancelled(),
                |batch| {
                    batch_tx.blocking_send(batch).map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "input payload reconciler stopped",
                        )
                    })
                },
            )
        });
        while let Some(mut hashes) = tokio::select! {
            biased;
            _ = shutdown.cancelled() => None,
            hashes = batch_rx.recv() => hashes,
        } {
            let _guard = self.input_artifact_gate.lock().await;
            let Some(retained) = self
                .chat_state_handle
                .submitted_input_payload_hashes()
                .await
            else {
                break;
            };
            // Submitted payloads remain part of the immutable audit record even
            // after the input is blocked, consumed, or dismissed. Only blobs with
            // no Timeline reference are orphans.
            hashes.retain(|hash| !retained.contains(hash));
            if hashes.is_empty() {
                continue;
            }
            let Ok(directory) = self.session_directory.try_clone() else {
                break;
            };
            match tokio::task::spawn_blocking(move || {
                crate::session::input_inbox::remove_payload_hashes(&directory, &hashes)
            })
            .await
            {
                Ok(Ok(_)) => {}
                error => {
                    tracing::warn!(?error, "input manifest reconciliation failed");
                    break;
                }
            }
        }
        drop(batch_rx);
        let _ = producer.await;
        if shutdown.is_cancelled() {
            return;
        }
        // Includes crashes after image writes but before the manifest exists.
        // Hold the same gate across the root snapshot and the attachment sweep.
        let _guard = self.input_artifact_gate.lock().await;
        let Some(retained) = self
            .chat_state_handle
            .submitted_input_payload_hashes()
            .await
        else {
            return;
        };
        let Ok(directory) = self.session_directory.try_clone() else {
            return;
        };
        let shutdown = shutdown.clone();
        match tokio::task::spawn_blocking(move || {
            crate::session::input_inbox::reconcile_image_blobs(&directory, &retained, || {
                shutdown.is_cancelled()
            })
        })
        .await
        {
            Ok(Ok(_)) => {}
            error => tracing::warn!(?error, "input image reconciliation failed"),
        }
    }

    pub(super) async fn reconcile_user_image_assets(
        &self,
        shutdown: &tokio_util::sync::CancellationToken,
    ) {
        if shutdown.is_cancelled() {
            return;
        }
        let _guard = self.input_artifact_gate.lock().await;
        if shutdown.is_cancelled() {
            return;
        }
        let Ok(directory) = self.session_directory.try_clone() else {
            return;
        };
        match tokio::task::spawn_blocking(move || {
            crate::session::image_describe::reconcile_user_image_assets(&directory)
        })
        .await
        {
            Ok(Ok(_)) => {}
            error => tracing::warn!(?error, "user image asset reconciliation failed"),
        }
    }
}

fn existing_input_state(
    events: &[chat_state::TimelineEvent],
    input_id: &str,
) -> Result<ExistingInputState, String> {
    let mut found = false;
    let mut state = ExistingInputState::Submitted;
    for event in events {
        let chat_state::TimelineEventKind::Input(input) = &event.kind else {
            continue;
        };
        match input {
            chat_state::InputEvent::Submitted {
                input_id: submitted_id,
                ..
            } if submitted_id == input_id => {
                if found {
                    return Err(format!("duplicate Submitted evidence for input {input_id}"));
                }
                found = true;
            }
            chat_state::InputEvent::AdmissionResolved {
                input_id: resolved_id,
                decision,
                ..
            } if resolved_id == input_id => {
                state = match decision {
                    chat_state::InputAdmissionDecision::Allow => ExistingInputState::Allowed,
                    chat_state::InputAdmissionDecision::Block { reason } => {
                        ExistingInputState::Blocked(reason.clone())
                    }
                };
            }
            chat_state::InputEvent::Consumed { input_ids, .. }
                if input_ids.iter().any(|id| id == input_id) =>
            {
                state = ExistingInputState::Consumed;
            }
            chat_state::InputEvent::Handled { input_ids, .. }
                if input_ids.iter().any(|id| id == input_id) =>
            {
                state = ExistingInputState::Handled;
            }
            chat_state::InputEvent::Dismissed { input_ids, .. }
                if input_ids.iter().any(|id| id == input_id) =>
            {
                state = ExistingInputState::Dismissed;
            }
            _ => {}
        }
    }
    found
        .then_some(state)
        .ok_or_else(|| format!("Submitted evidence is missing for input {input_id}"))
}

pub(super) fn input_block_reason_message(reason: chat_state::InputBlockReason) -> String {
    match reason {
        chat_state::InputBlockReason::Hook { reason } => reason,
        chat_state::InputBlockReason::StaleSteerTarget => {
            "the target turn is no longer running".into()
        }
        chat_state::InputBlockReason::ProcessInterrupted => {
            "input admission was interrupted".into()
        }
    }
}

use super::*;
use test_support::MockInferenceServer;

#[test]
fn same_model_window_reduction_rejects_known_pressure_before_provider_dispatch() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, mut notifications) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 200_000).await;

            let mut route = actor
                .chat_state_handle
                .get_sampling_config()
                .await
                .expect("sampling route exists");
            actor.compaction.previous_model.set(Some(
                crate::session::compaction_config::PreviousModelInfo {
                    model_slug: route.model.clone(),
                    context_window: 200_000,
                },
            ));
            actor
                .chat_state_handle
                .record_provider_context_anchor(100_001);
            route.context_window = std::num::NonZeroU64::new(100_000).unwrap();
            actor.chat_state_handle.update_sampling_config(route);

            run_user_turn(&actor, "same-model-window-shrink")
                .await
                .expect_err("known oversized request must fail before provider admission");

            assert_eq!(server.messages_request_count(), 0);
            let (updates, _) = drain_session_updates(&mut notifications);
            assert_eq!(
                updates
                    .iter()
                    .filter(|kind| *kind == "auto_compact_started")
                    .count(),
                1,
                "same-model window reduction still enters ordinary pressure recovery: {updates:?}"
            );
            assert_eq!(
                updates
                    .iter()
                    .filter(|kind| *kind == "auto_compact_failed")
                    .count(),
                1,
                "the uncompressible fixture settles before provider admission: {updates:?}"
            );
        }));
    });
}

#[test]
fn task_output_budget_disables_compaction_but_known_pressure_still_blocks_provider() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, mut notifications) =
                actor_with_sampler_cw_and_task_budget(&server, 100_000).await;
            actor
                .chat_state_handle
                .record_provider_context_anchor(100_001);

            run_user_turn(&actor, "budgeted-known-pressure")
                .await
                .expect_err("known oversized request must fail before provider admission");

            assert_eq!(server.messages_request_count(), 0);
            let (updates, _) = drain_session_updates(&mut notifications);
            assert!(
                !updates.iter().any(|kind| kind == "auto_compact_started"),
                "a budgeted task must not dispatch automatic compaction: {updates:?}"
            );
        }));
    });
}

#[test]
fn explicit_output_limit_blocks_ordinary_request_before_provider_dispatch() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, _) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            let mut route = actor
                .chat_state_handle
                .get_sampling_config()
                .await
                .expect("sampling route exists");
            route.output_limit = Some(25_000);
            actor.chat_state_handle.update_sampling_config(route);
            actor
                .chat_state_handle
                .record_provider_context_anchor(80_000);
            run_user_turn(&actor, "explicit-output-overflow")
                .await
                .expect_err("input plus explicit output limit exceeds the window");

            assert_eq!(server.messages_request_count(), 0);
        }));
    });
}

#[test]
fn task_output_clamp_is_the_output_reserve_used_for_admission() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, _) = actor_with_sampler_cw_and_task_budget(&server, 100_000).await;
            let mut route = actor
                .chat_state_handle
                .get_sampling_config()
                .await
                .expect("sampling route exists");
            route.output_limit = Some(25_000);
            actor.chat_state_handle.update_sampling_config(route);
            actor
                .chat_state_handle
                .record_provider_context_anchor(80_000);

            run_user_turn(&actor, "task-output-clamp-fits")
                .await
                .expect("the task clamp leaves the request within the window");

            let bodies = server.request_bodies();
            assert_eq!(bodies.len(), 1);
            assert_eq!(bodies[0]["max_tokens"], 1_000);
        }));
    });
}

#[test]
fn context_admission_accepts_exact_fit_and_does_not_guess_unset_output() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            let (actor, _) =
                actor_with_sampler_cw(&server, sampling_types::ApiBackend::Messages, 100_000).await;
            actor
                .chat_state_handle
                .record_provider_context_anchor(99_999);

            actor
                .ensure_model_context_fits(Some(1))
                .await
                .expect("input plus output equal to the window fits");
            actor
                .ensure_model_context_fits(Some(0))
                .await
                .expect("an explicit zero output reserve fits below the window");
            actor
                .ensure_model_context_fits(None)
                .await
                .expect("an unset output limit adds no guessed reserve");
            actor
                .ensure_model_context_fits(Some(2))
                .await
                .expect_err("one token beyond the window must fail");
            actor
                .ensure_model_context_fits(Some(100_000))
                .await
                .expect_err("an output reserve equal to the window cannot fit positive input");
            actor
                .ensure_model_context_fits(Some(u32::MAX))
                .await
                .expect_err("an output reserve larger than the window cannot fit positive input");
        }));
    });
}

/// Build the same real sampler fixture while installing the child output
/// budget before the actor is shared behind `Arc`.
async fn actor_with_sampler_cw_and_task_budget(
    server: &MockInferenceServer,
    context_window: u64,
) -> (
    std::sync::Arc<SessionActor>,
    tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let (gateway_tx, gateway_rx) = tokio::sync::mpsc::unbounded_channel();
    let (persistence_tx, mut persistence_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_local(async move {
        while let Some(message) = persistence_rx.recv().await {
            if let PersistenceMsg::SidebandDurablyAndAck { respond_to, .. } = message {
                let _ = respond_to.send(Ok(()));
            }
        }
    });

    let (mut actor, planning_rx) =
        create_test_actor_with_planning_receiver(0, context_window, 85, gateway_tx, persistence_tx)
            .await;
    actor.tool_context.task_output_token_budget = Some(
        crate::tools::tool_context::TaskOutputTokenBudget::limited(1_000),
    );

    if let Some(mut route) = actor.chat_state_handle.get_sampling_config().await {
        route.base_url = server.url();
        route.api_backend = sampling_types::ApiBackend::Messages;
        actor.chat_state_handle.replace_sampling_route(route);
    }
    let sampler_config = sampler::SamplerConfig {
        api_key: Some("test-key".to_string()),
        base_url: server.url(),
        model: "test-model".to_string(),
        output_limit: None,
        temperature: None,
        top_p: None,
        api_backend: sampling_types::ApiBackend::Messages,
        auth_scheme: Default::default(),
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window,
        force_http1: false,
        max_retries: Some(0),
        stream_tool_calls: false,
        idle_timeout_secs: Some(60),
        reasoning_effort: None,
        origin_client: None,
        bearer_resolver: None,
        compactions_remaining: None,
        compaction_at_tokens: None,
        doom_loop_recovery: None,
    };
    let (sampler_event_tx, mut sampler_event_rx) = tokio::sync::mpsc::unbounded_channel();
    actor.sampler_handle = sampler::SamplerActor::spawn(
        sampler_config,
        sampler::RetryPolicy::default(),
        sampler_event_tx,
    );
    let actor = std::sync::Arc::new(actor);
    start_test_planning_driver(&actor, planning_rx);
    let drainer = actor.clone();
    tokio::task::spawn_local(async move {
        while let Some(event) = sampler_event_rx.recv().await {
            drainer.handle_sampling_event(event).await;
        }
    });
    (actor, gateway_rx)
}

#[path = "pressure_admission_tests/output_precompaction_tests.rs"]
mod output_precompaction_tests;

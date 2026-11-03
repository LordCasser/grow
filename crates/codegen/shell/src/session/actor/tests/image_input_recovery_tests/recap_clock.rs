use super::*;
use crate::session::actor::spawn::TimelineBootstrap;

fn existing_clock(events: Vec<chat_state::TimelineEvent>) -> i64 {
    TimelineBootstrap::Existing {
        timeline: Box::new(chat_state::Timeline::from_events(events).unwrap()),
        history: Default::default(),
    }
    .last_api_request_at()
}

#[test]
fn cold_load_recovers_primary_api_clock_without_counting_later_sideband_work() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_response("A completed primary request.");
            let (actor, _gateway, _) = actor_with_sampler_and_persistence(&server, None).await;
            let before = chrono::Utc::now().timestamp_millis();
            admit_inline_query(
                &actor,
                "recap-clock-source",
                "A real primary request".into(),
            )
            .await;
            let primary_events = actor.chat_state_handle.timeline_events().await.unwrap();
            let restored = existing_clock(primary_events.clone());
            assert!(restored >= before && restored <= chrono::Utc::now().timestamp_millis());
            assert!(
                restored > 0,
                "cold load must not reset the recap gate to no-evidence"
            );

            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            assert!(
                !actor
                    .handle_side_question("A later auxiliary request")
                    .await
                    .unwrap()
                    .is_empty()
            );
            let after_sideband = actor.chat_state_handle.timeline_events().await.unwrap();
            assert!(after_sideband.last().unwrap().at_ms > restored);
            assert_eq!(
                existing_clock(after_sideband),
                restored,
                "Sideband activity must not postpone main API idle"
            );
            assert_eq!(
                TimelineBootstrap::Seeded {
                    timeline: Box::new(chat_state::Timeline::from_events(primary_events).unwrap()),
                    history: Default::default(),
                }
                .last_api_request_at(),
                0,
                "a new task does not inherit its parent's API clock"
            );
            assert_eq!(
                TimelineBootstrap::Fresh {
                    session_rules: None
                }
                .last_api_request_at(),
                0
            );
            let history_only = chat_state::Timeline::from_seed(vec![ConversationItem::user(
                "history without primary API evidence",
            )])
            .unwrap();
            assert_eq!(
                TimelineBootstrap::Existing {
                    timeline: Box::new(history_only),
                    history: Default::default()
                }
                .last_api_request_at(),
                0
            );
        }));
    });
}

//! Regressions for keeping context pressure in effective-request coordinates.

use super::*;

async fn effective_request_tokens(handle: &crate::handle::ChatStateHandle) -> u64 {
    let request = handle
        .build_request("context-pressure", vec![], None, None, None)
        .await
        .expect("request projection succeeds");
    crate::estimate_request_input_tokens(&request)
}

fn goal_pressure_items() -> (Vec<ConversationItem>, sampling_types::GoalDirectiveTag) {
    let active = sampling_types::GoalDirectiveTag {
        goal_id: "pressure-goal".into(),
        definition_revision: 3,
    };
    let prior = sampling_types::GoalDirectiveTag {
        goal_id: "pressure-goal".into(),
        definition_revision: 2,
    };
    let other = sampling_types::GoalDirectiveTag {
        goal_id: "other-goal".into(),
        definition_revision: 1,
    };
    let directive = |text: String, tag| {
        ConversationItem::goal_directive(text, sampling_types::SyntheticReason::SystemReminder, tag)
    };
    (
        vec![
            directive("old goal directive ".repeat(8_000), prior),
            ConversationItem::assistant("first response"),
            directive("other goal directive ".repeat(8_000), other),
            ConversationItem::assistant("second response"),
            directive("current goal directive ".repeat(8_000), active.clone()),
        ],
        active,
    )
}

fn projected_goal_tokens(
    items: &[ConversationItem],
    active: &sampling_types::GoalDirectiveTag,
) -> u64 {
    let projected =
        sampling_types::project_conversation_for_goal_scope(items.to_vec(), Some(active));
    crate::estimate_conversation_tokens(&projected)
}

async fn assert_goal_route_rebase(
    h: &mut TestHarness,
    active: &sampling_types::GoalDirectiveTag,
    expected_overhead: u64,
    publishes_reset: bool,
) {
    let surface = h.handle.get_conversation().await;
    let expected_request = projected_goal_tokens(&surface, active);
    let pressure = h.handle.get_projected_tokens().await;
    assert_eq!(
        pressure,
        expected_request + expected_overhead,
        "route invalidation keeps the latest request's Goal projection"
    );
    assert_eq!(
        h.drain_events()
            .into_iter()
            .filter_map(|event| match event {
                ChatStateEvent::ContextPressureUpdated { projected_tokens } => {
                    Some(projected_tokens)
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        if publishes_reset {
            vec![pressure]
        } else {
            vec![]
        },
        "route reset publishes its baseline; an unchanged image invalidation stays silent"
    );

    let request = h
        .handle
        .build_request(
            "goal-route-pressure",
            vec![],
            None,
            Some(active.clone()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        crate::estimate_request_input_tokens(&request),
        expected_request,
        "the rebuilt request uses the same portable Goal projection"
    );
    assert_eq!(
        h.handle.get_projected_tokens().await,
        expected_request + expected_overhead,
        "rebuilding does not change the rebased pressure"
    );
    assert!(
        h.drain_events()
            .into_iter()
            .all(|event| !matches!(event, ChatStateEvent::ContextPressureUpdated { .. })),
        "rebuilding the same projection is idempotent"
    );
}

#[tokio::test]
async fn sampling_route_replacement_retains_goal_scope_for_pressure_rebase() {
    let (items, active) = goal_pressure_items();
    let mut h = TestHarness::with_conversation(items);
    let initial = h
        .handle
        .build_request(
            "goal-route-pressure",
            vec![],
            None,
            Some(active.clone()),
            None,
        )
        .await
        .unwrap();
    let expected = crate::estimate_request_input_tokens(&initial);
    assert!(expected < crate::estimate_conversation_tokens(&h.handle.get_conversation().await));
    h.handle.record_provider_context_anchor(expected + 2_000);
    assert_eq!(h.handle.get_projected_tokens().await, expected + 2_000);
    h.drain_events();

    h.handle.replace_sampling_route(SamplingConfig {
        model: "replacement-goal-route".into(),
        ..test_config()
    });
    assert_goal_route_rebase(&mut h, &active, 0, true).await;
}

#[tokio::test]
async fn image_route_invalidation_retains_goal_scope_and_same_route_calibration() {
    let (items, active) = goal_pressure_items();
    let mut h = TestHarness::with_conversation(items);
    let initial = h
        .handle
        .build_request(
            "goal-route-pressure",
            vec![],
            None,
            Some(active.clone()),
            None,
        )
        .await
        .unwrap();
    let expected = crate::estimate_request_input_tokens(&initial);
    assert!(expected < crate::estimate_conversation_tokens(&h.handle.get_conversation().await));
    h.handle.record_provider_context_anchor(expected + 2_000);
    assert_eq!(h.handle.get_projected_tokens().await, expected + 2_000);
    h.drain_events();

    h.handle.update_sampling_config(SamplingConfig {
        model: "different-image-route".into(),
        ..test_config()
    });
    assert_goal_route_rebase(&mut h, &active, 2_000, false).await;
}

#[tokio::test]
async fn successful_request_replaces_cached_goal_scope() {
    let (items, active) = goal_pressure_items();
    let h = TestHarness::with_conversation(items);
    h.handle
        .build_request("goal-scope-transition", vec![], None, Some(active), None)
        .await
        .unwrap();

    let no_goal = h
        .handle
        .build_request("goal-scope-transition", vec![], None, None, None)
        .await
        .unwrap();
    let expected_no_goal = crate::estimate_request_input_tokens(&no_goal);
    assert_eq!(h.handle.get_projected_tokens().await, expected_no_goal);

    h.handle.replace_sampling_route(SamplingConfig {
        model: "route-after-goal-ended".into(),
        ..test_config()
    });
    let surface = h.handle.get_conversation().await;
    assert_eq!(
        h.handle.get_projected_tokens().await,
        crate::estimate_conversation_tokens(&sampling_types::project_conversation_for_goal_scope(
            surface.clone(),
            None,
        )),
        "None shadows every old directive after the scope is replaced"
    );

    let new_goal = sampling_types::GoalDirectiveTag {
        goal_id: "new-pressure-goal".into(),
        definition_revision: 1,
    };
    h.handle
        .push_response_durably(vec![ConversationItem::assistant("resume")], None)
        .await
        .unwrap();
    h.handle
        .push_user_message_durably(ConversationItem::goal_directive(
            "new current directive ".repeat(8_000),
            sampling_types::SyntheticReason::SystemReminder,
            new_goal.clone(),
        ))
        .await
        .unwrap();
    let new_request = h
        .handle
        .build_request(
            "goal-scope-transition",
            vec![],
            None,
            Some(new_goal.clone()),
            None,
        )
        .await
        .unwrap();
    let expected_new_goal = crate::estimate_request_input_tokens(&new_request);
    assert_eq!(h.handle.get_projected_tokens().await, expected_new_goal);

    h.handle.update_sampling_config(SamplingConfig {
        model: "image-route-after-new-goal".into(),
        ..test_config()
    });
    let surface = h.handle.get_conversation().await;
    assert_eq!(
        h.handle.get_projected_tokens().await,
        projected_goal_tokens(&surface, &new_goal),
        "a later invalidation uses the new successful request's scope"
    );
}

#[tokio::test]
async fn compaction_of_omitted_reasoning_preserves_calibrated_pressure_idempotently() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("s".repeat(128)),
        ConversationItem::user("u".repeat(800)),
        ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
            "r".repeat(80_000),
        )),
        ConversationItem::assistant("a".repeat(400)),
    ]);

    let before_effective = effective_request_tokens(&h.handle).await;
    let provider_anchor = before_effective + 2_000;
    h.handle.record_provider_context_anchor(provider_anchor);
    assert_eq!(h.handle.get_projected_tokens().await, provider_anchor);

    let raw_before = crate::estimate_conversation_tokens(&h.handle.get_conversation().await);
    commit_compaction_range(&h.handle, vec![ConversationItem::user_meta("summary")]).await;
    let retained_surface = h.handle.get_conversation().await;
    let raw_after = crate::estimate_conversation_tokens(&retained_surface);
    assert!(raw_before - raw_after > provider_anchor);

    // Full replacement leaves only portable system/summary content, so its
    // current request estimate is directly available without rebuilding.
    let retained_effective = crate::estimate_conversation_tokens(&retained_surface);
    let expected = retained_effective + (provider_anchor - before_effective);
    assert!(
        retained_effective > 0,
        "the compacted request retains content"
    );
    assert_eq!(h.handle.get_projected_tokens().await, expected);

    assert_eq!(
        effective_request_tokens(&h.handle).await,
        retained_effective
    );
    assert_eq!(h.handle.get_projected_tokens().await, expected);
    for _ in 0..2 {
        assert_eq!(
            effective_request_tokens(&h.handle).await,
            retained_effective
        );
        assert_eq!(h.handle.get_projected_tokens().await, expected);
    }
}

#[tokio::test]
async fn partial_compaction_keeps_omitted_reasoning_out_of_pressure() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("system"),
        ConversationItem::user("old question".repeat(100)),
        ConversationItem::assistant("old answer".repeat(100)),
        ConversationItem::user("recent question"),
        ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
            "r".repeat(40_000),
        )),
        ConversationItem::assistant("recent answer"),
    ]);
    let before = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    let before_effective = effective_request_tokens(&h.handle).await;
    let provider_anchor = before_effective + 8_000;
    h.handle.record_provider_context_anchor(provider_anchor);

    let target = crate::SurfaceRange {
        start: before.surface_ids[1],
        end: before.surface_ids[2],
        shadowed: before.surface_ids[1..=2].to_vec(),
    };
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                mode: crate::CompactionMode::Foreground,
                id: "partial-pressure".into(),
                source_items: before.surface.len(),
                prompt_index: 0,
            },
        ))
        .await
        .unwrap();
    let (spawn, summary) = compaction_summary_facts("partial-pressure", target.clone());
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Sideband(spawn))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(summary))
        .await
        .unwrap();
    h.handle
        .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Completed {
                id: "partial-pressure".into(),
                source_items: before.surface.len(),
                result_items: 1,
                duration_ms: 1,
            },
        ))
        .await
        .unwrap();

    let raw_surface = h.handle.get_conversation().await;
    assert!(
        raw_surface
            .iter()
            .any(|item| matches!(item, ConversationItem::Reasoning(_)))
    );
    let retained_effective = effective_request_tokens(&h.handle).await;
    let expected = retained_effective + (provider_anchor - before_effective);
    assert!(retained_effective > 0);
    assert_eq!(h.handle.get_projected_tokens().await, expected);
    assert_eq!(
        effective_request_tokens(&h.handle).await,
        retained_effective
    );
    assert_eq!(h.handle.get_projected_tokens().await, expected);
}

#[tokio::test]
async fn pruning_retains_provider_calibration_without_counting_raw_reasoning() {
    let h = TestHarness::new();
    use sampling_types::ToolCall;

    let mut conversation = vec![ConversationItem::system("system")];
    for index in 0..12 {
        conversation.push(ConversationItem::user(format!("question-{index}")));
        conversation.push(ConversationItem::Reasoning(
            sampling_types::synthesized_reasoning_item("r".repeat(8_000)),
        ));
        conversation.push(ConversationItem::assistant_tool_calls(vec![ToolCall {
            id: format!("call-{index}").into(),
            name: "test_tool".into(),
            arguments: "{}".into(),
        }]));
        conversation.push(ConversationItem::tool_result(
            format!("call-{index}"),
            "x".repeat(200_000),
        ));
        conversation.push(ConversationItem::assistant("answer"));
    }
    replace_test_surface(&h.handle, conversation.clone()).await;
    for index in 0..12 {
        record_prompt(&h.handle, format!("question-{index}")).await;
    }

    let before_effective = effective_request_tokens(&h.handle).await;
    let provider_anchor = before_effective + 10_000;
    h.handle.record_provider_context_anchor(provider_anchor);
    let plan = compaction::plan_tool_result_pruning(
        &conversation,
        &crate::actor::state::EstimatedItemTokenCounter,
        100,
        1_000,
    );
    h.handle
        .prune_tool_results(plan)
        .await
        .expect("canonical pre-prune succeeds");

    let after_surface = h.handle.get_conversation().await;
    assert!(
        after_surface
            .iter()
            .any(|item| matches!(item, ConversationItem::Reasoning(_)))
    );
    let after_effective = effective_request_tokens(&h.handle).await;
    assert!(
        after_effective < before_effective,
        "pruning changes the request"
    );
    let expected = after_effective + (provider_anchor - before_effective);
    assert_eq!(h.handle.get_projected_tokens().await, expected);
    assert_eq!(effective_request_tokens(&h.handle).await, after_effective);
    assert_eq!(h.handle.get_projected_tokens().await, expected);
}

#[tokio::test]
async fn changing_image_route_restores_pressure_before_rebuilding_image_budget() {
    fn image(index: usize) -> ConversationItem {
        let mut item = ConversationItem::user(format!("image {index}"));
        for part in 0..2 {
            item.add_image(format!(
                "data:image/png;base64,{}{}",
                "A".repeat(6 * 1024 * 1024),
                part
            ));
        }
        item
    }

    let mut h = TestHarness::with_conversation((0..4).map(image).collect());
    let first = h
        .handle
        .build_request("image-route-pressure", vec![], None, None, None)
        .await
        .unwrap();
    let before = crate::estimate_request_input_tokens(&first);
    assert!(
        !first.source_projection.as_ref().unwrap()["image_budget"]["evicted_parts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(h.handle.get_projected_tokens().await, before);
    h.drain_events();

    h.handle.update_sampling_config(SamplingConfig {
        model: "different-image-route".into(),
        ..test_config()
    });
    let restored = crate::estimate_conversation_tokens(&h.handle.get_conversation().await);
    assert_eq!(h.handle.get_projected_tokens().await, restored);
    assert!(
        restored > before,
        "clearing image choices restores image pressure"
    );
    assert_eq!(
        h.drain_events()
            .into_iter()
            .filter_map(|event| match event {
                ChatStateEvent::ContextPressureUpdated { projected_tokens } => {
                    Some(projected_tokens)
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        [restored],
        "the invalidated projection publishes its changed pressure before request build"
    );

    let rebuilt = h
        .handle
        .build_request("image-route-pressure", vec![], None, None, None)
        .await
        .unwrap();
    assert!(
        !rebuilt.source_projection.as_ref().unwrap()["image_budget"]["newly_evicted_parts"]
            .is_null(),
        "the new route performs a fresh image-budget pass"
    );
    assert_eq!(
        h.handle.get_projected_tokens().await,
        crate::estimate_request_input_tokens(&rebuilt)
    );
}

#[tokio::test]
async fn native_reset_publishes_changed_effective_pressure_before_request_rebuild() {
    use sampling_types::ApiBackend;

    let config = SamplingConfig {
        api_backend: ApiBackend::Responses,
        ..test_config()
    };
    let mut h = TestHarness::with_config(vec![ConversationItem::user("q1")], config);
    h.handle
        .build_request("native-reset-pressure", vec![], None, None, None)
        .await
        .unwrap();
    h.handle
        .push_response_durably(
            vec![ConversationItem::assistant("a".repeat(4_000))],
            Some(native_fragment_for(&ApiBackend::Responses)),
        )
        .await
        .unwrap();
    h.handle
        .push_user_message_durably(ConversationItem::user("q2"))
        .await
        .unwrap();
    let native = h
        .handle
        .build_request("native-reset-pressure", vec![], None, None, None)
        .await
        .unwrap();
    assert!(native.has_native_continuation());
    let before = h.handle.get_projected_tokens().await;
    h.drain_events();

    assert!(h.handle.reset_continuation().await);
    let reset = h.handle.get_projected_tokens().await;
    assert_ne!(
        reset, before,
        "reset replaces the native fragment projection"
    );
    assert_eq!(
        h.drain_events()
            .into_iter()
            .filter_map(|event| match event {
                ChatStateEvent::ContextPressureUpdated { projected_tokens } => {
                    Some(projected_tokens)
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        [reset],
        "the explicit native reset publishes its changed projection immediately"
    );
}

#[tokio::test]
async fn learned_portable_reasoning_publishes_changed_pressure_immediately() {
    use sampling_types::ApiBackend;

    let backend = ApiBackend::ChatCompletions;
    let config = SamplingConfig {
        api_backend: backend.clone(),
        ..test_config()
    };
    let mut h = TestHarness::with_config(
        vec![
            ConversationItem::user("question"),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "r".repeat(40_000),
            )),
            ConversationItem::assistant("answer"),
        ],
        config,
    );
    h.handle
        .build_request("portable-reasoning-pressure", vec![], None, None, None)
        .await
        .unwrap();
    let before = h.handle.get_projected_tokens().await;
    h.drain_events();

    assert_eq!(
        h.handle.enable_portable_reasoning(backend).await,
        Some(true)
    );
    let learned = h.handle.get_projected_tokens().await;
    assert!(learned > before);
    assert_eq!(
        h.drain_events()
            .into_iter()
            .filter_map(|event| match event {
                ChatStateEvent::ContextPressureUpdated { projected_tokens } => {
                    Some(projected_tokens)
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        [learned],
        "learning portable reasoning publishes its changed pressure immediately"
    );
}

#[tokio::test]
async fn rejected_control_does_not_change_goal_scope_or_pressure() {
    let tag = sampling_types::GoalDirectiveTag {
        goal_id: "goal-scope-reject".into(),
        definition_revision: 1,
    };
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("system"),
        ConversationItem::goal_directive(
            "active scope ".repeat(2_000),
            sampling_types::SyntheticReason::SystemReminder,
            tag.clone(),
        ),
    ]);
    h.handle
        .set_request_goal_scope(Some(tag.clone()))
        .await
        .unwrap();
    let control = crate::ControlEvent {
        revision: 1,
        snapshot: serde_json::json!({}),
        retired_context_layers: vec![],
        model_contexts: vec![],
    };
    h.handle
        .record_control_event_durably(control.clone(), Some(tag))
        .await
        .unwrap();
    let before = h.handle.get_projected_tokens().await;
    let event_count = h.handle.timeline_events().await.unwrap().len();
    let result = h.handle.record_control_event_durably(control, None).await;
    assert!(matches!(result, Err(crate::TimelineWriteError::Invalid(_))));
    assert_eq!(h.handle.get_projected_tokens().await, before);
    assert_eq!(h.handle.timeline_events().await.unwrap().len(), event_count);
}

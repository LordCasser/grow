//! Composed growth and compaction transitions after a low provider anchor.
use super::*;

async fn effective_request_tokens(handle: &crate::handle::ChatStateHandle) -> u64 {
    let request = handle
        .build_request("composed-low-anchor", vec![], None, None, None)
        .await
        .expect("request projection succeeds");
    crate::estimate_request_input_tokens(&request)
}

fn scaled_pressure(current: u64, before: u64, after: u64) -> u64 {
    assert!(
        current < before,
        "the provider observation remains below the local estimate"
    );
    assert!(
        after < before,
        "each compaction must shrink the effective request"
    );
    (u128::from(current) * u128::from(after)).div_ceil(u128::from(before)) as u64
}

async fn compact_and_advance_oracle(
    handle: &crate::handle::ChatStateHandle,
    current_pressure: u64,
    previous_estimate: u64,
    summary: &str,
) -> (u64, u64) {
    commit_compaction_range(
        handle,
        vec![ConversationItem::user_meta(summary.to_owned())],
    )
    .await;
    let estimate = effective_request_tokens(handle).await;
    let pressure = scaled_pressure(current_pressure, previous_estimate, estimate);
    assert_eq!(handle.get_projected_tokens().await, pressure);
    (estimate, pressure)
}

#[tokio::test]
async fn low_anchor_uses_rolling_reference_through_compaction_growth_and_rebuilds() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("system instructions"),
        ConversationItem::user("initial history ".repeat(5_000)),
    ]);
    let usage_before = h.handle.try_get_session_usage().await.unwrap().totals;
    let initial = h
        .handle
        .materialize_timeline("composed-low-anchor".into())
        .await
        .unwrap();
    let stable_system_id = initial.surface_ids[0];

    let mut reference_estimate = effective_request_tokens(&h.handle).await;
    let mut expected_pressure = reference_estimate / 5;
    h.handle.record_provider_context_anchor(expected_pressure);
    assert_eq!(h.handle.get_projected_tokens().await, expected_pressure);

    h.handle
        .push_user_message_durably(ConversationItem::user("first append ".repeat(300)))
        .await
        .unwrap();
    let after_first_append = h
        .handle
        .materialize_timeline("composed-low-anchor".into())
        .await
        .unwrap();
    assert_eq!(
        &after_first_append.surface_ids[..initial.surface_ids.len()],
        initial.surface_ids.as_slice(),
        "an append preserves every prior Surface identity"
    );
    let estimate = effective_request_tokens(&h.handle).await;
    expected_pressure += estimate - reference_estimate;
    assert_eq!(h.handle.get_projected_tokens().await, expected_pressure);
    reference_estimate = estimate;

    (reference_estimate, expected_pressure) = compact_and_advance_oracle(
        &h.handle,
        expected_pressure,
        reference_estimate,
        &"first summary ".repeat(300),
    )
    .await;
    let after_first_compaction = h
        .handle
        .materialize_timeline("composed-low-anchor".into())
        .await
        .unwrap();
    assert_eq!(after_first_compaction.surface_ids[0], stable_system_id);
    assert_eq!(after_first_compaction.surface_ids.len(), 2);
    assert!(
        !initial
            .surface_ids
            .contains(&after_first_compaction.surface_ids[1]),
        "the committed summary has a new exact Surface identity"
    );

    for _ in 0..3 {
        assert_eq!(
            effective_request_tokens(&h.handle).await,
            reference_estimate
        );
        assert_eq!(h.handle.get_projected_tokens().await, expected_pressure);
    }

    let before_second_append = h
        .handle
        .materialize_timeline("composed-low-anchor".into())
        .await
        .unwrap();
    h.handle
        .push_user_message_durably(ConversationItem::user("regrowth ".repeat(250)))
        .await
        .unwrap();
    let after_second_append = h
        .handle
        .materialize_timeline("composed-low-anchor".into())
        .await
        .unwrap();
    assert_eq!(
        &after_second_append.surface_ids[..before_second_append.surface_ids.len()],
        before_second_append.surface_ids.as_slice(),
        "regrowth preserves the summary and system identities"
    );
    let estimate = effective_request_tokens(&h.handle).await;
    expected_pressure += estimate - reference_estimate;
    assert_eq!(h.handle.get_projected_tokens().await, expected_pressure);
    reference_estimate = estimate;

    (reference_estimate, expected_pressure) = compact_and_advance_oracle(
        &h.handle,
        expected_pressure,
        reference_estimate,
        &"second summary ".repeat(180),
    )
    .await;
    let after_second_compaction = h
        .handle
        .materialize_timeline("composed-low-anchor".into())
        .await
        .unwrap();
    assert_eq!(after_second_compaction.surface_ids[0], stable_system_id);
    assert_eq!(after_second_compaction.surface_ids.len(), 2);
    assert_ne!(
        after_second_compaction.surface_ids[1], after_first_compaction.surface_ids[1],
        "each committed summary receives a distinct exact identity"
    );

    for _ in 0..3 {
        assert_eq!(
            effective_request_tokens(&h.handle).await,
            reference_estimate
        );
        assert_eq!(h.handle.get_projected_tokens().await, expected_pressure);
    }

    let before_rejected_compaction = h
        .handle
        .materialize_timeline("composed-low-anchor".into())
        .await
        .unwrap();
    let stale_range = crate::SurfaceRange {
        start: before_rejected_compaction.surface_ids[1],
        end: before_rejected_compaction.surface_ids[1],
        shadowed: vec![before_rejected_compaction.surface_ids[1]],
    };
    assert!(
        h.handle
            .replace_compaction_range(
                stale_range,
                vec![ConversationItem::user_meta("rejected summary")],
            )
            .await
            .is_err(),
        "a replacement without an active compaction is rejected"
    );
    let after_rejected_compaction = h
        .handle
        .materialize_timeline("composed-low-anchor".into())
        .await
        .unwrap();
    assert_eq!(
        after_rejected_compaction.surface_ids, before_rejected_compaction.surface_ids,
        "a rejected durable compaction preserves exact Surface identities"
    );
    assert_eq!(
        after_rejected_compaction.surface_revision, before_rejected_compaction.surface_revision,
        "a rejected durable compaction does not advance the Surface revision"
    );
    assert_eq!(
        effective_request_tokens(&h.handle).await,
        reference_estimate
    );
    assert_eq!(h.handle.get_projected_tokens().await, expected_pressure);
    assert_eq!(
        h.handle.try_get_session_usage().await.unwrap().totals,
        usage_before,
        "a rejected compaction does not change lifetime usage"
    );
    for _ in 0..2 {
        assert_eq!(
            effective_request_tokens(&h.handle).await,
            reference_estimate
        );
        assert_eq!(h.handle.get_projected_tokens().await, expected_pressure);
    }

    h.handle
        .push_user_message_durably(ConversationItem::user(
            "growth after second shrink ".repeat(40),
        ))
        .await
        .unwrap();
    let after_later_growth = h
        .handle
        .materialize_timeline("composed-low-anchor".into())
        .await
        .unwrap();
    assert_eq!(
        &after_later_growth.surface_ids[..after_rejected_compaction.surface_ids.len()],
        after_rejected_compaction.surface_ids.as_slice(),
        "a later valid append retains the exact pre-failure identities"
    );
    let final_estimate = effective_request_tokens(&h.handle).await;
    expected_pressure += final_estimate - reference_estimate;
    assert_eq!(
        h.handle.get_projected_tokens().await,
        expected_pressure,
        "growth after repeated shrink adds its full local request cost"
    );
    assert_eq!(
        h.handle.try_get_session_usage().await.unwrap().totals,
        usage_before,
        "provider context calibration does not change lifetime usage"
    );
}

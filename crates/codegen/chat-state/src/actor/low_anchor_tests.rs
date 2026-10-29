//! Provider context measurements below the local heuristic remain anchors.
use super::*;

#[path = "composed_low_anchor_tests.rs"]
mod composed_low_anchor_tests;

#[tokio::test]
async fn low_context_anchor_survives_growth_compaction_and_repeated_projection() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("system"),
        ConversationItem::user("q".repeat(40_000)),
        ConversationItem::assistant("answer"),
    ]);
    let request = h
        .handle
        .build_request("low-anchor", vec![], None, None, None)
        .await
        .unwrap();
    let local = crate::estimate_request_input_tokens(&request);
    let measured = local / 4;
    let usage_before = h.handle.try_get_session_usage().await.unwrap().totals;
    h.handle.record_provider_context_anchor(measured);
    assert_eq!(
        h.handle.get_projected_tokens().await,
        measured,
        "complete low measurement is authoritative"
    );
    h.handle
        .push_user_message_durably(ConversationItem::user("next".repeat(400)))
        .await
        .unwrap();
    let grown = crate::estimate_request_input_tokens(
        &h.handle
            .build_request("low-anchor", vec![], None, None, None)
            .await
            .unwrap(),
    );
    assert_eq!(
        h.handle.get_projected_tokens().await,
        measured + grown - local
    );
    commit_compaction_range(
        &h.handle,
        vec![ConversationItem::user_meta(
            "summary retained after shrinking ".repeat(100),
        )],
    )
    .await;
    let retained = crate::estimate_conversation_tokens(&h.handle.get_conversation().await);
    let measured_after_growth = measured + grown - local;
    let expected = (u128::from(measured_after_growth) * u128::from(retained))
        .div_ceil(u128::from(grown)) as u64;
    assert!(retained > 0 && expected > 0);
    assert_eq!(
        h.handle.get_projected_tokens().await,
        expected,
        "shrinking must not saturate a negative offset to fake zero"
    );
    for _ in 0..2 {
        h.handle
            .build_request("low-anchor", vec![], None, None, None)
            .await
            .unwrap();
        assert_eq!(h.handle.get_projected_tokens().await, expected);
    }
    h.handle
        .push_user_message_durably(ConversationItem::user("n".repeat(400)))
        .await
        .unwrap();
    assert_eq!(
        h.handle.get_projected_tokens().await,
        expected + 100,
        "growth after shrinking still adds its full local cost"
    );
    let retained = crate::estimate_conversation_tokens(&h.handle.get_conversation().await);
    assert_eq!(
        h.handle.try_get_session_usage().await.unwrap().totals,
        usage_before
    );
    h.handle.replace_sampling_route(SamplingConfig {
        model: "new-route".into(),
        ..test_config()
    });
    assert_eq!(
        h.handle.get_projected_tokens().await,
        retained,
        "a new route discards the old anchor"
    );
}

#[tokio::test]
async fn low_context_anchor_zero_and_maximum_growth_do_not_wrap_or_freeze() {
    let h = TestHarness::with_conversation(vec![ConversationItem::user("q".repeat(400))]);
    h.handle.record_provider_context_anchor(0);
    assert_eq!(h.handle.get_projected_tokens().await, 0);
    h.handle
        .push_user_message_durably(ConversationItem::user("n".repeat(400)))
        .await
        .unwrap();
    assert_eq!(
        h.handle.get_projected_tokens().await,
        100,
        "new content is still counted after a measured zero"
    );
    h.handle.record_provider_context_anchor(u64::MAX);
    h.handle
        .push_user_message_durably(ConversationItem::user("n".repeat(400)))
        .await
        .unwrap();
    assert_eq!(h.handle.get_projected_tokens().await, u64::MAX);
}

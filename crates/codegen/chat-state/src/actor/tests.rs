//! Tests for ChatStateActor.

use std::num::NonZeroU64;
use std::time::Duration;

use sampling_types::{ConversationItem, SamplingConfig};
use tokio::sync::mpsc;

use crate::actor::ChatStateActor;
use crate::events::ChatStateEvent;
use crate::persistence::{MockPersistenceReceiver, MockTimelinePersistence, PersistenceRecord};

fn persisted_messages(record: &PersistenceRecord) -> Option<&crate::MessageEvent> {
    match record {
        PersistenceRecord::Timeline(event) => event.messages(),
        PersistenceRecord::Flush => None,
    }
}

/// Helper to build a `SamplingConfig` for tests.
fn test_config() -> SamplingConfig {
    test_config_with_window(128_000)
}

fn test_config_with_window(context_window: u64) -> SamplingConfig {
    SamplingConfig {
        base_url: "https://api.example.com".to_string(),
        model: "test-model".to_string(),
        output_limit: None,
        temperature: None,
        top_p: None,
        api_backend: Default::default(),
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: NonZeroU64::new(context_window)
            .expect("test context_window must be non-zero"),
        reasoning_effort: None,
        stream_tool_calls: None,
    }
}

fn marked_user(text: impl Into<String>, prompt_index: usize) -> ConversationItem {
    let mut item = ConversationItem::user(text);
    item.set_prompt_index(prompt_index);
    item
}

fn compaction_summary_facts(
    id: &str,
    target: crate::SurfaceRange,
) -> (crate::SidebandSpawnEvent, crate::CompactionEvent) {
    let sideband_id = uuid::Uuid::now_v7().to_string();
    let input_ref = crate::TimelineRangeRef {
        timeline_id: "test-timeline".into(),
        first_seq: 0,
        last_seq: 0,
    };
    let spawn = crate::SidebandSpawnEvent {
        sideband_id: sideband_id.clone(),
        purpose: crate::SidebandPurpose::CompactionSummary,
        source_refs: vec![input_ref.clone()],
    };
    let summary = crate::CompactionEvent::Summary {
        id: id.into(),
        input_ref,
        result_ref: crate::TimelineRangeRef {
            timeline_id: sideband_id,
            first_seq: 2,
            last_seq: 2,
        },
        target,
        source_tokens: 100,
        summary_chars: 7,
    };
    (spawn, summary)
}

async fn record_compaction_summary(
    handle: &crate::handle::ChatStateHandle,
    id: &str,
) -> crate::SurfaceRange {
    let materialized = handle
        .materialize_timeline("test-timeline".into())
        .await
        .expect("test Timeline must materialize");
    let compactable_start = usize::from(matches!(
        materialized.surface.first(),
        Some(ConversationItem::System(_))
    ));
    assert!(
        compactable_start < materialized.surface_ids.len(),
        "compaction requires body items after the stable System head"
    );
    let shadowed = materialized.surface_ids[compactable_start..].to_vec();
    let target = crate::SurfaceRange {
        start: *shadowed.first().expect("compactable Surface body"),
        end: *shadowed.last().expect("compactable Surface body"),
        shadowed,
    };
    let (spawn, summary) = compaction_summary_facts(id, target.clone());
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Sideband(spawn))
        .await
        .unwrap();
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(summary))
        .await
        .unwrap();
    target
}

async fn record_prompt(handle: &crate::handle::ChatStateHandle, text: impl Into<String>) {
    static NEXT_TURN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = crate::TurnId(NEXT_TURN.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let prompt_index = handle.get_prompt_index().await;
    handle.record_timeline_event(crate::TimelineEventKind::Turn(crate::TurnEvent::Started {
        id,
        identity: crate::TurnIdentity {
            origin: "user".into(),
            turn_kind: "user".into(),
            goal_id: None,
            stage_id: None,
        },
        model_id: "test-model".into(),
        input_message_count: handle.get_conversation_len().await,
        prompt_index,
        prompt_text: text.into(),
        input_kind: crate::TurnInputKind::Prompt,
        redirect_kind: None,
    }));
    handle.record_timeline_event(crate::TimelineEventKind::Turn(crate::TurnEvent::Ended {
        id,
        outcome: "completed".into(),
        duration_ms: 1,
        tool_count: 0,
        terminal: crate::TurnTerminal {
            stop_reason: "end_turn".into(),
            completion_kind: "completed".into(),
        },
        cancellation_category: None,
        details: None,
    }));
    let _ = handle.get_prompt_index().await;
}

/// Test harness that spawns an actor and keeps the event + persistence channels.
struct TestHarness {
    handle: crate::handle::ChatStateHandle,
    event_rx: mpsc::UnboundedReceiver<ChatStateEvent>,
    persistence_rx: MockPersistenceReceiver,
    bootstrap_records_to_skip: usize,
    _cancellation_token: tokio_util::sync::CancellationToken,
}

impl TestHarness {
    fn new() -> Self {
        Self::with_config(vec![], test_config())
    }

    fn with_conversation(items: Vec<ConversationItem>) -> Self {
        Self::with_config(items, test_config())
    }

    fn with_context_window(window: u64) -> Self {
        Self::with_config(vec![], test_config_with_window(window))
    }

    fn with_config(items: Vec<ConversationItem>, config: SamplingConfig) -> Self {
        let (mock, persistence_rx) = MockTimelinePersistence::new();
        Self::with_persistence(items, config, mock, persistence_rx)
    }

    fn with_manual_timeline_ack(items: Vec<ConversationItem>) -> Self {
        Self::with_manual_timeline_ack_after(items, 0)
    }

    fn with_manual_timeline_ack_after(
        items: Vec<ConversationItem>,
        automatic_live_acks: usize,
    ) -> Self {
        let config = test_config();
        let bootstrap_events = crate::actor::state::ChatState::new(items.clone(), config.clone())
            .timeline
            .events()
            .len();
        let (mock, persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack_after(
            bootstrap_events + automatic_live_acks,
        );
        Self::with_persistence(items, config, mock, persistence_rx)
    }

    fn with_persistence(
        items: Vec<ConversationItem>,
        config: SamplingConfig,
        mock: MockTimelinePersistence,
        persistence_rx: MockPersistenceReceiver,
    ) -> Self {
        let bootstrap_records_to_skip =
            crate::actor::state::ChatState::new(items.clone(), config.clone())
                .timeline
                .events()
                .len();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let token = tokio_util::sync::CancellationToken::new();
        let handle = ChatStateActor::spawn(items, config, Box::new(mock), event_tx, token.clone());
        Self {
            handle,
            event_rx,
            persistence_rx,
            bootstrap_records_to_skip,
            _cancellation_token: token,
        }
    }

    /// Drain the next event from the event channel (with timeout).
    async fn next_event(&mut self) -> ChatStateEvent {
        tokio::time::timeout(Duration::from_secs(1), self.event_rx.recv())
            .await
            .expect("timed out waiting for event")
            .expect("event channel closed")
    }

    /// Drain all pending events.
    fn drain_events(&mut self) -> Vec<ChatStateEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }

    /// Drain all pending persistence records.
    fn drain_persistence(&mut self) -> Vec<PersistenceRecord> {
        self.persistence_rx
            .drain()
            .into_iter()
            .filter(|record| {
                if self.bootstrap_records_to_skip > 0
                    && matches!(record, PersistenceRecord::Timeline(_))
                {
                    self.bootstrap_records_to_skip -= 1;
                    false
                } else {
                    true
                }
            })
            .collect()
    }
}

async fn fail_once_then_ack_exact_retry(persistence_rx: &mut MockPersistenceReceiver) {
    persistence_rx
        .next_timeline_ack()
        .await
        .expect("first Timeline acknowledgement")
        .send(Err(std::io::Error::other("simulated disk failure")))
        .unwrap();
    persistence_rx
        .next_timeline_ack()
        .await
        .expect("exact Timeline retry acknowledgement")
        .send(Ok(()))
        .unwrap();
}

async fn replace_test_surface(
    handle: &crate::handle::ChatStateHandle,
    items: Vec<ConversationItem>,
) {
    let (_, source_revision) = handle
        .get_conversation_with_revision()
        .await
        .expect("actor must provide Surface revision");
    handle
        .replace_context_durably(items, source_revision)
        .await
        .unwrap();
}

async fn seed_test_system(handle: &crate::handle::ChatStateHandle, content: impl Into<String>) {
    assert_eq!(handle.get_conversation_len().await, 0);
    replace_test_surface(handle, vec![ConversationItem::system(content)]).await;
}

async fn commit_compaction_range(
    handle: &crate::handle::ChatStateHandle,
    items: Vec<ConversationItem>,
) {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = format!(
        "test-compaction-{}",
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let source_items = handle.get_conversation_len().await;
    let (_, source_surface_revision) = handle
        .get_conversation_with_revision()
        .await
        .expect("actor must provide compaction source");
    let prompt_index = handle.get_prompt_index().await;
    let result_items = items.len();
    handle.record_timeline_event(crate::TimelineEventKind::Compaction(
        crate::CompactionEvent::Started {
            id: id.clone(),
            source_items,
            prompt_index,
        },
    ));
    let target = record_compaction_summary(handle, &id).await;
    handle
        .replace_compaction_range(target, items, source_surface_revision)
        .await
        .unwrap();
    handle.record_timeline_event(crate::TimelineEventKind::Compaction(
        crate::CompactionEvent::Completed {
            id,
            source_items,
            result_items,
            duration_ms: 1,
        },
    ));
    let _ = handle.get_conversation_len().await;
}

#[tokio::test]
async fn compaction_rejects_a_stale_surface_without_hiding_late_messages() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("system"),
        ConversationItem::user("body"),
    ]);
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                id: "stale-compaction".into(),
                source_items: 2,
                prompt_index: 0,
            },
        ))
        .await
        .unwrap();
    let (_, source_revision) = h
        .handle
        .get_conversation_with_revision()
        .await
        .expect("actor must provide compaction source");

    let target = record_compaction_summary(&h.handle, "stale-compaction").await;

    h.handle
        .push_user_message_durably(ConversationItem::user("arrived during compaction"))
        .await
        .unwrap();
    let error = h
        .handle
        .replace_compaction_range(
            target,
            vec![ConversationItem::user("stale summary")],
            source_revision,
        )
        .await
        .expect_err("stale compaction must fail closed");
    assert!(matches!(
        error,
        crate::TimelineWriteError::SurfaceChanged { .. }
    ));

    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Failed {
                id: "stale-compaction".into(),
                duration_ms: 1,
                error: "surface changed".into(),
            },
        ))
        .await
        .unwrap();
    let surface = h.handle.get_conversation().await;
    assert_eq!(surface.len(), 3);
    assert_eq!(surface[2].text_content(), "arrived during compaction");
}

// ============================================================================
// Lifecycle tests
// ============================================================================

#[tokio::test]
async fn actor_spawns_and_shuts_down_via_cancellation() {
    let (mock, _rx) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let token = tokio_util::sync::CancellationToken::new();
    let _handle = ChatStateActor::spawn(
        vec![],
        test_config(),
        Box::new(mock),
        event_tx,
        token.clone(),
    );
    token.cancel();
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test]
async fn partial_compaction_preserves_unselected_surface_identity() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("system"),
        ConversationItem::project_instructions("rules"),
        ConversationItem::user("old task"),
        ConversationItem::assistant("old answer"),
        ConversationItem::user("recent task"),
        ConversationItem::assistant("recent answer"),
    ]);
    let before = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    let target = crate::SurfaceRange {
        start: before.surface_ids[2],
        end: before.surface_ids[3],
        shadowed: before.surface_ids[2..=3].to_vec(),
    };

    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                id: "partial-compaction".into(),
                source_items: before.surface.len(),
                prompt_index: 0,
            },
        ))
        .await
        .unwrap();
    let (spawn, summary) = compaction_summary_facts("partial-compaction", target.clone());
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Sideband(spawn))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(summary))
        .await
        .unwrap();
    h.handle
        .replace_compaction_range(
            target,
            vec![ConversationItem::user_meta("summary")],
            before.surface_revision,
        )
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Completed {
                id: "partial-compaction".into(),
                source_items: before.surface.len(),
                result_items: 1,
                duration_ms: 1,
            },
        ))
        .await
        .unwrap();

    let after = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    assert_eq!(
        after
            .surface
            .iter()
            .map(ConversationItem::text_content)
            .collect::<Vec<_>>(),
        vec!["system", "rules", "summary", "recent task", "recent answer"]
    );
    assert_eq!(&after.surface_ids[..2], &before.surface_ids[..2]);
    assert_eq!(&after.surface_ids[3..], &before.surface_ids[4..]);
    assert_ne!(after.surface_ids[2], before.surface_ids[2]);

    let branch = h
        .handle
        .materialize_branch_transcript("test-timeline".into())
        .await
        .unwrap();
    assert_eq!(branch.source_ref, after.input_ref);
    assert_eq!(
        branch
            .transcript
            .iter()
            .map(ConversationItem::text_content)
            .collect::<Vec<_>>(),
        vec![
            "system",
            "rules",
            "old task",
            "old answer",
            "recent task",
            "recent answer"
        ]
    );
    assert_eq!(branch.transcript.len(), branch.transcript_ids.len());
    assert_eq!(&branch.transcript_ids[..4], &before.surface_ids[..4]);
    assert_eq!(
        branch.unloaded_surface_ids,
        before.surface_ids[2..=3].to_vec()
    );
    assert_eq!(branch.need_surface_ids, after.surface_ids);
}

#[tokio::test]
async fn actor_shuts_down_when_all_handles_dropped() {
    let (mock, _rx) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let token = tokio_util::sync::CancellationToken::new();
    let handle = ChatStateActor::spawn(vec![], test_config(), Box::new(mock), event_tx, token);
    drop(handle);
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test]
async fn restored_actor_replays_surface_and_continues_event_sequence() {
    let mut timeline = crate::Timeline::from_seed(vec![
        ConversationItem::user("old question"),
        ConversationItem::assistant("old answer"),
    ])
    .unwrap();
    timeline
        .record(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                id: "compact".into(),
                source_items: 2,
                prompt_index: 0,
            },
        ))
        .unwrap();
    let target = crate::SurfaceRange {
        start: *timeline.surface_ids().first().unwrap(),
        end: *timeline.surface_ids().last().unwrap(),
        shadowed: timeline.surface_ids().to_vec(),
    };
    let (spawn, summary) = compaction_summary_facts("compact", target.clone());
    timeline
        .record(crate::TimelineEventKind::Sideband(spawn))
        .unwrap();
    timeline
        .record(crate::TimelineEventKind::Compaction(summary))
        .unwrap();
    timeline
        .replace_compaction_range(target, vec![ConversationItem::user("summary")])
        .unwrap();
    timeline
        .record(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Completed {
                id: "compact".into(),
                source_items: 2,
                result_items: 1,
                duration_ms: 1,
            },
        ))
        .unwrap();
    let expected_seq = timeline.next_seq();
    let (mock, mut persistence_rx) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let token = tokio_util::sync::CancellationToken::new();
    let handle = ChatStateActor::spawn_from_timeline(
        timeline.events().to_vec(),
        test_config(),
        Box::new(mock),
        event_tx,
        token,
    )
    .await
    .unwrap();

    assert_eq!(handle.get_conversation().await[0].text_content(), "summary");
    assert!(
        persistence_rx.drain().is_empty(),
        "replay must not rewrite facts"
    );
    handle.push_assistant_response(ConversationItem::assistant("continued"));
    let _ = handle.get_conversation().await;
    let records = persistence_rx.drain();
    let PersistenceRecord::Timeline(event) = &records[0] else {
        panic!("expected timeline append, got {records:?}");
    };
    assert_eq!(event.seq, expected_seq);
}

#[tokio::test]
async fn restored_actor_durably_repairs_dangling_tool_surface_before_launch() {
    let timeline = crate::Timeline::from_seed(vec![ConversationItem::Assistant(
        sampling_types::AssistantItem {
            content: "".into(),
            tool_calls: vec![sampling_types::ToolCall {
                id: "dangling".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            }],
            model_id: Some("model".into()),
            model_fingerprint: None,
            reasoning_effort: None,
        },
    )])
    .unwrap();
    let (mock, mut persistence_rx) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn_from_timeline(
        timeline.events().to_vec(),
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();

    let surface = handle.get_conversation().await;
    assert_eq!(surface.len(), 2);
    assert!(matches!(surface[1], ConversationItem::ToolResult(_)));
    let recovery = persistence_rx
        .drain()
        .into_iter()
        .filter(|record| matches!(record, PersistenceRecord::Timeline(_)))
        .count();
    assert_eq!(recovery, 2, "recovery intent and replacement must commit");
}

// ============================================================================
// Mutation tests
// ============================================================================

#[tokio::test]
async fn push_user_message_appends_and_persists() {
    let mut h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("hello"));

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 1);

    let records = h.drain_persistence();
    assert!(matches!(
        records.as_slice(),
        [PersistenceRecord::Timeline(_)]
    ));
}

#[tokio::test]
async fn push_user_message_durably_waits_for_timeline_commit() {
    let mut h = TestHarness::new();
    let ack = h
        .handle
        .push_user_message_durably(ConversationItem::user("hello"))
        .await;

    assert!(ack.is_ok());

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 1);

    let records = h.drain_persistence();
    assert!(matches!(
        records.as_slice(),
        [PersistenceRecord::Timeline(_)]
    ));
}
#[tokio::test]
async fn push_assistant_response_appends_and_persists() {
    let mut h = TestHarness::new();
    h.handle
        .push_assistant_response(ConversationItem::assistant("hi"));

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 1);

    let records = h.drain_persistence();
    assert!(matches!(
        records.as_slice(),
        [PersistenceRecord::Timeline(_)]
    ));
}

#[tokio::test]
async fn push_tool_result_appends_and_persists() {
    let mut h = TestHarness::new();
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-1", "result"));

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 1);

    let records = h.drain_persistence();
    assert!(matches!(
        records.as_slice(),
        [PersistenceRecord::Timeline(_)]
    ));
}

#[tokio::test]
async fn provider_context_anchor_emits_event() {
    let mut h = TestHarness::new();
    h.handle.record_provider_context_anchor(1000);
    let event = h.next_event().await;
    assert!(matches!(
        event,
        ChatStateEvent::ContextPressureUpdated {
            projected_tokens: 1000
        }
    ));

    let tokens = h.handle.get_projected_tokens().await;
    assert_eq!(tokens, 1000);
}

#[tokio::test]
async fn provider_anchor_below_final_request_estimate_is_ignored() {
    use sampling_types::ToolSpec;

    let h = TestHarness::with_conversation(vec![ConversationItem::user("hello")]);
    h.handle
        .build_request(
            "test-timeline",
            vec![ToolSpec {
                name: "large".into(),
                description: Some("x".repeat(4_000)),
                parameters: serde_json::json!({"type": "object"}),
            }],
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let projected = h.handle.get_projected_tokens().await;
    assert!(projected > 1_000);

    h.handle.record_provider_context_anchor(10);
    assert_eq!(h.handle.get_projected_tokens().await, projected);
}

#[tokio::test]
async fn record_last_turn_usage_round_trip() {
    use sampling_types::TokenUsage;

    let h = TestHarness::new();

    // Initial state: nothing stashed.
    assert!(h.handle.get_last_turn_usage().await.is_none());

    let usage = TokenUsage {
        prompt_tokens: 1234,
        completion_tokens: 56,
        total_tokens: 1290,
        reasoning_tokens: 0,
        cached_prompt_tokens: 800,
        cache_creation_prompt_tokens: 0,
    };
    h.handle.record_last_turn_usage(usage.clone());

    let got = h.handle.get_last_turn_usage().await.expect("usage stashed");
    assert_eq!(got.prompt_tokens, 1234);
    assert_eq!(got.completion_tokens, 56);
    assert_eq!(got.cached_prompt_tokens, 800);

    // Subsequent record overwrites.
    let next = TokenUsage {
        prompt_tokens: 9999,
        completion_tokens: 1,
        total_tokens: 10000,
        reasoning_tokens: 0,
        cached_prompt_tokens: 0,
        cache_creation_prompt_tokens: 0,
    };
    h.handle.record_last_turn_usage(next);
    let got2 = h
        .handle
        .get_last_turn_usage()
        .await
        .expect("overwritten usage");
    assert_eq!(got2.prompt_tokens, 9999);
    assert_eq!(got2.cached_prompt_tokens, 0);
}

#[tokio::test]
async fn prompt_usage_ledger_via_handle_resets_and_clears() {
    use sampling_types::TokenUsage;

    let call = TokenUsage {
        prompt_tokens: 10,
        completion_tokens: 2,
        total_tokens: 12,
        reasoning_tokens: 0,
        cached_prompt_tokens: 0,
        cache_creation_prompt_tokens: 0,
    };

    let h = TestHarness::new();
    h.handle
        .record_model_call_usage(Some("m".into()), call.clone(), None, None);
    h.handle
        .record_model_call_usage(None, call.clone(), None, None);
    assert_eq!(
        h.handle
            .try_get_prompt_usage()
            .await
            .unwrap()
            .expect("prompt ledger")
            .totals
            .model_calls,
        2
    );
    h.handle.push_user_message(marked_user("prompt", 0));
    record_prompt(&h.handle, "prompt").await;
    assert!(
        h.handle
            .try_get_prompt_usage()
            .await
            .ok()
            .flatten()
            .is_none()
    );
    assert_eq!(
        h.handle
            .try_get_session_usage()
            .await
            .expect("actor alive")
            .totals
            .model_calls,
        2
    );

    h.handle
        .record_model_call_usage(Some("m".into()), call.clone(), None, None);
    h.handle
        .record_model_call_usage(Some("m".into()), call, None, None);
    h.handle.rewind_durably(0).await.unwrap();
    assert!(
        h.handle
            .try_get_prompt_usage()
            .await
            .ok()
            .flatten()
            .is_none()
    );
}

#[tokio::test]
async fn projected_tokens_track_tool_result_growth() {
    let h = TestHarness::new();
    h.handle.record_provider_context_anchor(100_000);

    // Push a tool result with 4000 chars → ~1000 estimated tokens
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-1", "x".repeat(4000)));

    assert_eq!(h.handle.get_projected_tokens().await, 101_000);
}

#[tokio::test]
async fn provider_anchor_replaces_local_projection() {
    let h = TestHarness::new();
    h.handle.record_provider_context_anchor(100_000);
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-1", "x".repeat(4000)));

    assert_eq!(h.handle.get_projected_tokens().await, 101_000);

    // The next provider total already includes the tool result.
    h.handle.record_provider_context_anchor(105_000);
    assert_eq!(h.handle.get_projected_tokens().await, 105_000);
}

#[tokio::test]
async fn projected_tokens_track_synthetic_user_message_growth() {
    let h = TestHarness::new();
    h.handle.record_provider_context_anchor(100_000);

    // Simulate a 4MB background-task completion notification pushed as a
    // synthetic User item between turns.
    h.handle.push_user_message(ConversationItem::user(
        "Background task completed:\n".to_string() + &"x".repeat(4_000_000),
    ));

    let projected = h.handle.get_projected_tokens().await;
    // 100K model-reported + ~1M tokens for the 4MB reminder.
    assert!(
        (1_099_000..=1_101_000).contains(&projected),
        "expected ~1.1M projected tokens, got {projected}",
    );
}

/// Regression: a normal user prompt pushed at turn start must increment
/// current context pressure. The next provider anchor already includes it.
#[tokio::test]
async fn projected_tokens_track_real_user_message_then_accept_provider_anchor() {
    let h = TestHarness::new();
    h.handle.record_provider_context_anchor(100_000);

    // Real user turn — 4000 chars / 4 = ~1000 tokens.
    h.handle
        .push_user_message(ConversationItem::user("u".repeat(4000)));

    let pre = h.handle.get_projected_tokens().await;
    assert!(
        (101_000..=101_010).contains(&pre),
        "expected ~101K after user push, got {pre}",
    );

    h.handle.record_provider_context_anchor(103_000);
    assert_eq!(h.handle.get_projected_tokens().await, 103_000);
}

/// Response facts are estimated first, then replaced by the provider anchor.
#[tokio::test]
async fn provider_anchor_replaces_response_item_estimates() {
    let h = TestHarness::new();
    h.handle.record_provider_context_anchor(100_000);
    h.handle
        .push_assistant_response(ConversationItem::assistant("a".repeat(4000)));
    h.handle
        .push_assistant_response(reasoning_sibling("reasoning-1", None));

    assert!(h.handle.get_projected_tokens().await > 100_000);
    h.handle.record_provider_context_anchor(103_000);
    assert_eq!(
        h.handle.get_projected_tokens().await,
        103_000,
        "provider anchor must replace response-item estimates"
    );
}

#[tokio::test]
async fn response_items_without_provider_usage_remain_estimated() {
    let h = TestHarness::new();
    h.handle.record_provider_context_anchor(100_000);
    h.handle
        .push_assistant_response(ConversationItem::assistant("a".repeat(4000)));

    assert_eq!(h.handle.get_projected_tokens().await, 101_000);
}

#[tokio::test]
async fn rewind_applies_signed_surface_delta() {
    let mut h = TestHarness::new();
    h.handle.record_provider_context_anchor(100_000);
    h.handle.push_user_message(marked_user("q1", 0));
    record_prompt(&h.handle, "q1").await;
    h.handle
        .push_assistant_response(ConversationItem::assistant("a1"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-1", "x".repeat(4000)));

    assert_eq!(h.handle.get_projected_tokens().await, 101_000);

    // Rewind removes Surface content but preserves provider overhead.
    h.drain_events();
    let surface_before =
        crate::estimate_conversation_tokens(&h.handle.get_conversation().await);
    h.handle.rewind_durably(0).await.unwrap();
    let surface_after =
        crate::estimate_conversation_tokens(&h.handle.get_conversation().await);
    assert_eq!(
        h.handle.get_projected_tokens().await,
        101_000u64.saturating_sub(surface_before - surface_after)
    );
}

#[tokio::test]
async fn timeline_turn_start_emits_prompt_projection_event() {
    let mut h = TestHarness::new();
    record_prompt(&h.handle, "prompt").await;
    let event = h.next_event().await;
    assert!(matches!(
        event,
        ChatStateEvent::PromptIndexChanged { new_index: 1 }
    ));

    let idx = h.handle.get_prompt_index().await;
    assert_eq!(idx, 1);
}

#[tokio::test]
async fn replace_conversation_persists_and_emits_reset() {
    let mut h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("a"));
    h.handle.push_user_message(ConversationItem::user("b"));

    // Drain the two Timeline message events.
    let _ = h.handle.get_conversation().await; // sync point
    h.drain_persistence();

    let new_items = vec![ConversationItem::user("rebuilt")];
    replace_test_surface(&h.handle, new_items).await;

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 1);

    let records = h.drain_persistence();
    assert!(matches!(
        records.as_slice(),
        [PersistenceRecord::Timeline(_)]
    ));
}

#[tokio::test]
async fn image_projection_preserves_canonical_images_and_is_scoped_to_model_route() {
    use sampling_types::conversation::{
        ContentPart, SyntheticReason, UserItem, conversation_image_groups,
    };

    let user = ConversationItem::User(UserItem {
        content: vec![
            ContentPart::Text {
                text: "inspect these".into(),
            },
            ContentPart::Image {
                url: "data:image/png;base64,user".into(),
            },
        ],
        synthetic_reason: Some(SyntheticReason::Interjection),
        permission_evidence: None,
        prompt_index: Some(7),
        ..Default::default()
    });
    let tool_result = ConversationItem::tool_result_with_images(
        "call_7",
        "Read image file",
        vec![
            ContentPart::Text {
                text: "keep-me".into(),
            },
            ContentPart::Image {
                url: "data:image/png;base64,tool-a".into(),
            },
            ContentPart::Image {
                url: "data:image/png;base64,tool-b".into(),
            },
        ],
    );
    let mut h = TestHarness::new();
    h.handle.begin_turn_capture();
    h.handle.push_user_message(user);
    h.handle.push_tool_result(tool_result);
    let materialized = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    let groups = conversation_image_groups(&materialized.surface);
    let sideband_id = "00000000-0000-0000-0000-000000000007".to_owned();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Sideband(
            crate::SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose: crate::SidebandPurpose::ImageDescription,
                source_refs: vec![materialized.input_ref.clone()],
            },
        ))
        .await
        .unwrap();
    let shadows = groups
        .iter()
        .map(|group| crate::ImageShadow {
            source: materialized.surface_ids[group.item_index],
            fingerprint: group.fingerprint.clone(),
            image_count: group.image_count(),
            replacement: if group.item_index == 0 {
                "converted user image".to_owned()
            } else {
                "image description unavailable".to_owned()
            },
            provenance: if group.item_index == 0 {
                crate::ImageShadowSource::Description {
                    result_ref: crate::TimelineRangeRef {
                        timeline_id: sideband_id.clone(),
                        first_seq: 2,
                        last_seq: 2,
                    },
                }
            } else {
                crate::ImageShadowSource::Unavailable
            },
        })
        .collect();

    let report = h
        .handle
        .record_image_projection_and_ack(crate::ImageProjectionEvent {
            runtime: sampling_types::model_image_input_key(&test_config()),
            source_revision: materialized.surface_revision,
            shadows,
        })
        .await
        .unwrap();
    assert_eq!(report.described_images, 1);
    assert_eq!(report.unavailable_images, 2);

    let conversation = h.handle.get_conversation().await;
    let ConversationItem::User(user) = &conversation[0] else {
        panic!("expected user item");
    };
    assert_eq!(user.synthetic_reason, Some(SyntheticReason::Interjection));
    assert_eq!(user.prompt_index, Some(7));
    assert!(
        user
            .content
            .iter()
            .any(|part| matches!(part, ContentPart::Image { .. }))
    );

    let ConversationItem::ToolResult(result) = &conversation[1] else {
        panic!("expected tool result");
    };
    assert_eq!(result.tool_call_id, "call_7");
    assert_eq!(result.content.as_ref(), "Read image file");
    assert!(matches!(
        result.images.as_slice(),
        [ContentPart::Text { text }, ContentPart::Image { .. }, ContentPart::Image { .. }]
            if text.as_ref() == "keep-me"
    ));
    let capture = h.handle.take_turn_messages().await.unwrap();
    assert_eq!(capture.messages.len(), conversation.len());
    assert_eq!(conversation_image_groups(&capture.messages).len(), 2);

    let projected = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
    assert!(conversation_image_groups(&projected.items).is_empty());
    assert!(projected.items[0].text_content().contains("converted user image"));
    assert!(projected.items[1]
        .text_content()
        .contains("image description unavailable"));

    let mut other_route = test_config();
    other_route.model = "vision-model".to_owned();
    h.handle.update_sampling_config(other_route);
    let restored = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
    assert_eq!(conversation_image_groups(&restored.items).len(), 2);
    assert!(
        h.drain_persistence()
            .iter()
            .any(|record| matches!(
                record,
                PersistenceRecord::Timeline(crate::TimelineEvent {
                    kind: crate::TimelineEventKind::ImageProjection(_),
                    ..
                })
            ))
    );
}

#[tokio::test]
async fn image_projection_retries_an_uncertain_persistence_failure() {
    use sampling_types::conversation::{ContentPart, UserItem, conversation_image_groups};

    let user = ConversationItem::User(UserItem {
        content: vec![ContentPart::Image {
            url: "data:image/png;base64,original".into(),
        }],
        ..Default::default()
    });
    let mut h = TestHarness::with_manual_timeline_ack(vec![user.clone()]);
    let materialized = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    let groups = conversation_image_groups(&materialized.surface);
    let projection = crate::ImageProjectionEvent {
        runtime: sampling_types::model_image_input_key(&test_config()),
        source_revision: materialized.surface_revision,
        shadows: vec![crate::ImageShadow {
            source: materialized.surface_ids[groups[0].item_index],
            fingerprint: groups[0].fingerprint.clone(),
            image_count: groups[0].image_count(),
            replacement: "image description unavailable".to_owned(),
            provenance: crate::ImageShadowSource::Unavailable,
        }],
    };
    let handle = h.handle.clone();
    let projection_future = async move { handle.record_image_projection_and_ack(projection).await };
    let retry = fail_once_then_ack_exact_retry(&mut h.persistence_rx);
    let (report, ()) = tokio::join!(projection_future, retry);
    assert_eq!(report.unwrap().unavailable_images, 1);
    assert_eq!(
        serde_json::to_vec(&h.handle.get_conversation().await).unwrap(),
        serde_json::to_vec(&vec![user]).unwrap(),
    );
    let materialized = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    assert_eq!(materialized.active_image_projections.len(), 1);
}

#[tokio::test]
async fn durable_rewind_retries_an_uncertain_persistence_failure() {
    let original = marked_user("original", 0);
    let mut h = TestHarness::with_manual_timeline_ack_after(vec![], 3);
    h.handle.push_user_message(original.clone());
    record_prompt(&h.handle, "original").await;
    let handle = h.handle.clone();
    let mut replace = std::pin::pin!(async move { handle.rewind_durably(0).await });
    let retry = fail_once_then_ack_exact_retry(&mut h.persistence_rx);
    let (result, ()) = tokio::join!(&mut replace, retry);
    result.unwrap();
}

#[tokio::test]
async fn durable_user_message_retries_an_uncertain_persistence_failure() {
    let original = ConversationItem::system("system");
    let mut h = TestHarness::with_manual_timeline_ack(vec![original.clone()]);
    let handle = h.handle.clone();
    let push = async move {
        handle
            .push_user_message_durably(ConversationItem::user("not committed"))
            .await
    };
    let retry = fail_once_then_ack_exact_retry(&mut h.persistence_rx);
    let (result, ()) = tokio::join!(push, retry);
    result.unwrap();
    assert_eq!(h.handle.get_conversation().await.len(), 2);
}

#[tokio::test]
async fn lost_timeline_ack_retries_the_exact_event_once() {
    let mut h = TestHarness::with_manual_timeline_ack(vec![]);
    let handle = h.handle.clone();
    let push = async move {
        handle
            .push_user_message_durably(ConversationItem::user("committed once"))
            .await
    };
    let acknowledge = async {
        let first = h
            .persistence_rx
            .next_timeline_ack()
            .await
            .expect("first Timeline acknowledgement");
        drop(first);
        h.persistence_rx
            .next_timeline_ack()
            .await
            .expect("retried Timeline acknowledgement")
            .send(Ok(()))
            .unwrap();
    };
    let (result, ()) = tokio::join!(push, acknowledge);
    result.unwrap();

    let events = h
        .drain_persistence()
        .into_iter()
        .filter_map(|record| match record {
            crate::PersistenceRecord::Timeline(event) => Some(event),
            crate::PersistenceRecord::Flush => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 2);
    assert_eq!(
        serde_json::to_value(&events[0]).unwrap(),
        serde_json::to_value(&events[1]).unwrap(),
    );
    assert_eq!(h.handle.get_conversation().await.len(), 1);
}

#[tokio::test]
async fn bootstrap_persists_strictly_one_event_at_a_time() {
    let seed = vec![
        ConversationItem::system("system"),
        ConversationItem::user("user"),
    ];
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        seed,
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );

    let first = persistence_rx
        .next_timeline_ack()
        .await
        .expect("first bootstrap acknowledgement");
    let records = persistence_rx.drain();
    assert_eq!(records.len(), 1);
    let PersistenceRecord::Timeline(first_event) = &records[0] else {
        panic!("expected first bootstrap event");
    };
    assert_eq!(first_event.seq.get(), 0);
    first
        .send(Err(std::io::Error::other("committed state unknown")))
        .unwrap();

    let retry = persistence_rx
        .next_timeline_ack()
        .await
        .expect("exact bootstrap retry acknowledgement");
    let records = persistence_rx.drain();
    assert_eq!(records.len(), 1);
    let PersistenceRecord::Timeline(retried_event) = &records[0] else {
        panic!("expected retried bootstrap event");
    };
    assert_eq!(
        serde_json::to_value(retried_event).unwrap(),
        serde_json::to_value(first_event).unwrap()
    );
    retry.send(Ok(())).unwrap();

    let second = persistence_rx
        .next_timeline_ack()
        .await
        .expect("second bootstrap acknowledgement");
    let records = persistence_rx.drain();
    assert_eq!(records.len(), 1);
    let PersistenceRecord::Timeline(second_event) = &records[0] else {
        panic!("expected second bootstrap event");
    };
    assert_eq!(second_event.seq.get(), 1);
    second.send(Ok(())).unwrap();
    assert_eq!(handle.get_conversation().await.len(), 2);
}

#[tokio::test]
async fn dropping_last_handle_cancels_an_unacknowledged_pending_event() {
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        Vec::new(),
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    handle.push_assistant_response(ConversationItem::assistant("pending"));
    let acknowledgement = persistence_rx
        .next_timeline_ack()
        .await
        .expect("pending acknowledgement");

    drop(handle);
    let closed = tokio::time::timeout(std::time::Duration::from_secs(1), event_rx.recv())
        .await
        .expect("actor must stop after its last handle is dropped");
    assert!(closed.is_none());
    drop(acknowledgement);
}

#[tokio::test]
async fn permanent_persistence_failure_poison_closes_the_actor_mailbox() {
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        Vec::new(),
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    handle.push_assistant_response(ConversationItem::assistant("first"));
    handle.push_assistant_response(ConversationItem::assistant("must not reuse seq"));
    persistence_rx
        .next_timeline_ack()
        .await
        .expect("first pending acknowledgement")
        .send(Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "ledger identity conflict",
        )))
        .unwrap();

    let closed = tokio::time::timeout(std::time::Duration::from_secs(1), event_rx.recv())
        .await
        .expect("poisoned actor must close its event channel");
    assert!(closed.is_none());
    let second_ack = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        persistence_rx.next_timeline_ack(),
    )
    .await;
    assert!(
        !matches!(second_ack, Ok(Some(_))),
        "queued commands must not persist after writer poison"
    );
    drop(handle);
}

#[tokio::test]
async fn buffered_append_recovers_from_io_failure_without_breaking_sequence() {
    let mut h = TestHarness::with_manual_timeline_ack(vec![]);
    h.handle
        .push_assistant_response(ConversationItem::assistant("first"));
    let first_sync = {
        let handle = h.handle.clone();
        async move { handle.get_conversation().await }
    };
    let recover_first = async {
        h.persistence_rx
            .next_timeline_ack()
            .await
            .expect("first buffered attempt")
            .send(Err(std::io::Error::other("simulated ENOSPC")))
            .unwrap();
        h.persistence_rx
            .next_timeline_ack()
            .await
            .expect("exact buffered retry")
            .send(Ok(()))
            .unwrap();
    };
    let (surface, ()) = tokio::join!(first_sync, recover_first);
    assert_eq!(surface.len(), 1);

    h.handle
        .push_assistant_response(ConversationItem::assistant("second"));
    let second_sync = {
        let handle = h.handle.clone();
        async move { handle.get_conversation().await }
    };
    let commit_second = async {
        h.persistence_rx
            .next_timeline_ack()
            .await
            .expect("next buffered append")
            .send(Ok(()))
            .unwrap();
    };
    let (surface, ()) = tokio::join!(second_sync, commit_second);
    assert_eq!(surface.len(), 2);

    let events = h
        .drain_persistence()
        .into_iter()
        .filter_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].seq.get(), 0);
    assert_eq!(events[1].seq.get(), 0);
    assert_eq!(events[2].seq.get(), 1);
    assert_eq!(
        serde_json::to_vec(&events[0]).unwrap(),
        serde_json::to_vec(&events[1]).unwrap(),
        "the failed append must retry the identical immutable fact"
    );
}

#[tokio::test]
async fn conditional_tool_result_rejects_stale_recall_and_closes_the_call() {
    use sampling_types::ToolCall;

    let h = TestHarness::new();
    h.handle
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![
            ToolCall {
                id: "recall".into(),
                name: "context_recall".into(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "sibling".into(),
                name: "read_file".into(),
                arguments: "{}".into(),
            },
        ]));
    let frozen_revision = h.handle.get_surface_revision().await.unwrap();

    h.handle.push_tool_result(ConversationItem::tool_result(
        "sibling",
        "the competing result",
    ));
    let _ = h.handle.get_conversation().await;

    let outcome = h
        .handle
        .push_tool_result_conditionally(
            ConversationItem::tool_result("recall", "stale secret evidence"),
            ConversationItem::tool_result("recall", "recall rejected; retry"),
            frozen_revision,
            128_000,
            2_048,
        )
        .await
        .unwrap();
    assert_eq!(
        outcome,
        crate::ConditionalToolResultOutcome::RejectedSurfaceChanged
    );

    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();
    let recall_results = request
        .items
        .iter()
        .filter_map(|item| match item {
            ConversationItem::ToolResult(result) if result.tool_call_id == "recall" => {
                Some(result.content.as_ref())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(recall_results, ["recall rejected; retry"]);
}

#[tokio::test]
async fn conditional_tool_result_rechecks_headroom_at_commit() {
    use sampling_types::ToolCall;

    let h = TestHarness::with_context_window(10_000);
    h.handle
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![ToolCall {
            id: "recall".into(),
            name: "context_recall".into(),
            arguments: "{}".into(),
        }]));
    let frozen_revision = h.handle.get_surface_revision().await.unwrap();

    // Provider accounting can advance without changing Surface revision while
    // recall synthesis is in flight.
    h.handle.record_provider_context_anchor(7_900);
    assert_eq!(h.handle.get_projected_tokens().await, 7_900);
    let outcome = h
        .handle
        .push_tool_result_conditionally(
            ConversationItem::tool_result("recall", "x".repeat(1_000)),
            ConversationItem::tool_result("recall", "recall rejected; retry"),
            frozen_revision,
            7_952,
            2_048,
        )
        .await
        .unwrap();
    assert_eq!(
        outcome,
        crate::ConditionalToolResultOutcome::RejectedHeadroom
    );

    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();
    let result = request
        .items
        .iter()
        .find_map(|item| match item {
            ConversationItem::ToolResult(result) => Some(result),
            _ => None,
        })
        .expect("rejection must close the tool call");
    assert_eq!(result.content.as_ref(), "recall rejected; retry");
}

#[tokio::test]
async fn compaction_preserves_provider_overhead_when_surface_estimate_is_unchanged() {
    let h = TestHarness::new();
    // ~1k estimated tokens; provider reports 51k → 50k overhead.
    h.handle
        .push_user_message(ConversationItem::user("x".repeat(4000)));
    h.handle.record_provider_context_anchor(51_000);

    let compacted = vec![ConversationItem::user("summary ".repeat(500))];
    commit_compaction_range(&h.handle, compacted).await;

    assert_eq!(h.handle.get_projected_tokens().await, 51_000);
}

#[tokio::test]
async fn compaction_applies_signed_surface_delta_to_provider_anchor() {
    let h = TestHarness::new();
    h.handle
        .push_user_message(ConversationItem::user("x".repeat(160_000)));
    let estimate_at_response =
        crate::estimate_conversation_tokens(&h.handle.get_conversation().await);
    assert_eq!(estimate_at_response, 40_000);
    let provider_total = 87_000;
    h.handle.record_provider_context_anchor(provider_total);

    let compacted = vec![ConversationItem::user("z".repeat(12_000))];
    let compacted_estimate = crate::estimate_conversation_tokens(&compacted);
    assert_eq!(compacted_estimate, 3_000);
    commit_compaction_range(&h.handle, compacted).await;

    let expected = provider_total - (estimate_at_response - compacted_estimate);
    assert_eq!(expected, 50_000);
    assert_eq!(h.handle.get_projected_tokens().await, expected);
}

#[tokio::test]
async fn compaction_delta_includes_post_response_surface_growth_once() {
    let h = TestHarness::new();
    h.handle
        .push_user_message(ConversationItem::user("y".repeat(4000)));
    h.handle.record_provider_context_anchor(11_000);
    h.handle
        .push_tool_result(ConversationItem::tool_result("c1", "z".repeat(100_000)));

    let before = h.handle.get_conversation().await;
    let surface_before = crate::estimate_conversation_tokens(&before);
    assert_eq!(h.handle.get_projected_tokens().await, 36_000);

    let compacted = vec![ConversationItem::user("s".repeat(2_000))];
    let surface_after = crate::estimate_conversation_tokens(&compacted);
    commit_compaction_range(&h.handle, compacted).await;

    let expected = 36_000 - (surface_before - surface_after);
    assert_eq!(h.handle.get_projected_tokens().await, expected);
}

#[tokio::test]
async fn pruning_projects_signed_surface_delta_from_provider_anchor() {
    let h = TestHarness::new();

    // A large retained conversation gives the model-free pre-prune planner
    // enough oversized tool results to reduce the Surface materially.
    let mut conv = Vec::new();
    for i in 0..12 {
        conv.push(ConversationItem::user(format!("q{i}")));
        conv.push(ConversationItem::tool_result(
            format!("call-{i}"),
            "x".repeat(200_000),
        ));
    }
    replace_test_surface(&h.handle, conv.clone()).await;
    for index in 0..12 {
        record_prompt(&h.handle, format!("q{index}")).await;
    }

    let estimate_at_response = crate::estimate_conversation_tokens(&conv);
    let provider_total = estimate_at_response + estimate_at_response / 2;
    h.handle.record_provider_context_anchor(provider_total);

    let plan = compaction::plan_tool_result_pruning(
        &conv,
        &crate::actor::state::EstimatedItemTokenCounter,
        100,
        1_000,
    );
    h.handle
        .prune_tool_results(plan)
        .await
        .expect("canonical pre-prune succeeds");
    let pruned_estimate = crate::estimate_conversation_tokens(&h.handle.get_conversation().await);
    assert!(
        pruned_estimate + 20_000 < estimate_at_response,
        "test setup must actually prune ({pruned_estimate} vs {estimate_at_response})"
    );
    let expected = provider_total.saturating_sub(estimate_at_response - pruned_estimate);
    assert_eq!(
        h.handle.get_projected_tokens().await,
        expected,
        "pruning must subtract only its signed Surface delta from the provider anchor"
    );
}

#[tokio::test]
async fn compaction_without_provider_anchor_tracks_signed_surface_growth() {
    let h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("hello"));

    let compacted = vec![ConversationItem::user("w".repeat(8000))];
    commit_compaction_range(&h.handle, compacted).await;

    let total = h.handle.get_projected_tokens().await;
    assert_eq!(
        total, 2_000,
        "fresh-session projection must follow Surface growth"
    );
}

#[tokio::test]
async fn non_compaction_replace_preserves_provider_overhead() {
    let h = TestHarness::new();
    h.handle
        .push_user_message(ConversationItem::user("x".repeat(4000)));
    h.handle.record_provider_context_anchor(51_000);

    replace_test_surface(&h.handle, vec![ConversationItem::user("q".repeat(4000))]).await;

    let total = h.handle.get_projected_tokens().await;
    assert_eq!(
        total, 51_000,
        "equal-sized replacement must preserve provider overhead"
    );
}

#[tokio::test]
async fn flush_calls_persistence_flush() {
    let mut h = TestHarness::new();
    h.handle.flush();

    // Use a query as sync point to ensure flush was processed
    let _ = h.handle.get_prompt_index().await;

    let records = h.drain_persistence();
    assert_eq!(records.len(), 1);
    assert!(matches!(&records[0], PersistenceRecord::Flush));
}

#[tokio::test]
async fn snapshot_projects_timeline_and_token_accounting() {
    let mut h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("msg"));
    h.handle.record_provider_context_anchor(500);
    record_prompt(&h.handle, "msg").await;

    // Drain events from the mutations above
    let _ = h.handle.get_conversation().await;
    h.drain_events();

    let snapshot = h.handle.snapshot().await.unwrap();
    assert_eq!(snapshot.prompt_index, 1);
    assert_eq!(snapshot.projected_tokens, 500);
    assert_eq!(snapshot.conversation.len(), 1);

    assert_eq!(
        snapshot
            .prompt_records
            .iter()
            .map(|record| (record.prompt_index, record.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "msg")]
    );
    assert_eq!(snapshot.last_compaction_prompt_index, None);
}

// ============================================================================
// Query tests
// ============================================================================

#[tokio::test]
async fn get_conversation_returns_current_state() {
    let h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("msg1"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("resp1"));

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 2);
}

#[tokio::test]
async fn get_projected_tokens_returns_zero_initially() {
    let h = TestHarness::new();
    let tokens = h.handle.get_projected_tokens().await;
    assert_eq!(tokens, 0);
}

#[tokio::test]
async fn empty_conversation_queries_return_defaults() {
    let h = TestHarness::new();
    assert!(h.handle.get_conversation().await.is_empty());
    assert_eq!(h.handle.get_prompt_index().await, 0);
    assert_eq!(h.handle.get_projected_tokens().await, 0);
    assert!(h.handle.get_agent_edited_paths().await.is_empty());
}

#[tokio::test]
async fn check_auto_compact_returns_none_when_under_threshold() {
    let h = TestHarness::with_context_window(10000);
    h.handle.record_provider_context_anchor(100);
    // Sync point
    let _ = h.handle.get_projected_tokens().await;

    let trigger = h.handle.check_auto_compact_needed(85).await;
    assert!(trigger.is_none());
}

#[tokio::test]
async fn check_auto_compact_triggers_at_threshold() {
    let h = TestHarness::with_context_window(10000);
    h.handle.record_provider_context_anchor(8600);
    let _ = h.handle.get_projected_tokens().await;

    let trigger = h.handle.check_auto_compact_needed(85).await;
    assert!(trigger.is_some());
    let t = trigger.unwrap();
    assert_eq!(t.projected_tokens, 8600);
    assert_eq!(t.context_window.get(), 10000);
    assert_eq!(t.utilization_percent, 86);
}

// ============================================================================
// Edge-case / integration tests
// ============================================================================

#[tokio::test]
async fn record_agent_edited_path_deduplicates() {
    let h = TestHarness::new();
    h.handle.record_agent_edited_path("src/main.rs".to_string());
    h.handle.record_agent_edited_path("src/main.rs".to_string());
    h.handle.record_agent_edited_path("src/lib.rs".to_string());

    let paths = h.handle.get_agent_edited_paths().await;
    assert_eq!(paths.len(), 2);
}

#[tokio::test]
async fn prompt_records_are_projected_from_timeline_turns() {
    let h = TestHarness::new();
    record_prompt(&h.handle, "first").await;
    record_prompt(&h.handle, "second").await;
    record_prompt(&h.handle, "third").await;

    let snap = h.handle.snapshot().await.unwrap();
    assert_eq!(
        snap.prompt_records
            .iter()
            .map(|record| (record.prompt_index, record.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "first"), (1, "second"), (2, "third")]
    );
}

#[tokio::test]
async fn update_sampling_config_is_queryable() {
    let h = TestHarness::new();
    let new_config = SamplingConfig {
        base_url: "https://new.example.com".to_string(),
        model: "grow-3".to_string(),
        output_limit: Some(4096),
        temperature: Some(0.5),
        top_p: None,
        api_backend: Default::default(),
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: NonZeroU64::new(200_000).unwrap(),
        reasoning_effort: None,
        stream_tool_calls: None,
    };
    h.handle.update_sampling_config(new_config.clone());

    let config = h.handle.get_sampling_config().await.unwrap();
    assert_eq!(config.model, "grow-3");
    assert_eq!(config.context_window, NonZeroU64::new(200_000).unwrap());
}

#[tokio::test]
async fn notification_meta_reflects_timing() {
    let h = TestHarness::new();
    h.handle.record_stream_start(1000);
    h.handle.record_turn_start(2000);

    let meta = h.handle.get_notification_meta().await.unwrap();
    assert_eq!(meta.stream_start_ms, Some(1000));
    assert_eq!(meta.turn_start_ms, Some(2000));
}

#[tokio::test]
async fn handle_clone_is_cheap_and_works() {
    let h = TestHarness::new();
    let handle2 = h.handle.clone();
    handle2.push_user_message(ConversationItem::user("from clone"));

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 1);
}

#[tokio::test]
async fn multiple_push_and_query_interleave() {
    let h = TestHarness::new();
    for i in 0..10 {
        h.handle
            .push_user_message(ConversationItem::user(format!("msg-{i}")));
    }
    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 10);
}

#[tokio::test]
async fn completed_compaction_is_reflected_in_snapshot() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("system"),
        ConversationItem::user("body"),
    ]);
    for text in ["one", "two", "three"] {
        record_prompt(&h.handle, text).await;
    }
    commit_compaction_range(&h.handle, vec![ConversationItem::user("compacted")]).await;
    let snap = h.handle.snapshot().await.unwrap();
    assert_eq!(snap.last_compaction_prompt_index, Some(3));
}

#[tokio::test]
async fn truncate_removes_items_after_target_prompt_index() {
    let mut h = TestHarness::new();

    // Build 3 turns: system + 3x (user + assistant)
    seed_test_system(&h.handle, "sys").await;
    h.handle.push_user_message(marked_user("q1", 0));
    record_prompt(&h.handle, "q1").await;
    h.handle
        .push_assistant_response(ConversationItem::assistant("a1"));

    h.handle.push_user_message(marked_user("q2", 1));
    record_prompt(&h.handle, "q2").await;
    h.handle
        .push_assistant_response(ConversationItem::assistant("a2"));

    h.handle.push_user_message(marked_user("q3", 2));
    record_prompt(&h.handle, "q3").await;
    h.handle
        .push_assistant_response(ConversationItem::assistant("a3"));

    // Sync + drain
    let _ = h.handle.get_prompt_index().await;
    h.drain_events();
    h.drain_persistence();

    // Truncate to prompt_index 1 → keep sys, q1, a1 (stop before q2)
    h.handle.rewind_durably(1).await.unwrap();

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 3); // sys + q1 + a1
    let idx = h.handle.get_prompt_index().await;
    assert_eq!(idx, 1);

    let snap = h.handle.snapshot().await.unwrap();
    assert_eq!(
        snap.prompt_records
            .iter()
            .map(|record| (record.prompt_index, record.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "q1")]
    );

    // Verify persistence was called
    let records = h.drain_persistence();
    assert!(
        records
            .iter()
            .filter_map(persisted_messages)
            .any(|event| event.cause == crate::MessageCause::Rewind)
    );
}

#[tokio::test]
async fn truncate_to_zero_keeps_only_system() {
    let mut h = TestHarness::new();

    seed_test_system(&h.handle, "sys").await;
    h.handle.push_user_message(marked_user("q1", 0));
    record_prompt(&h.handle, "q1").await;
    h.handle
        .push_assistant_response(ConversationItem::assistant("a1"));

    let _ = h.handle.get_prompt_index().await;
    h.drain_events();

    h.handle.rewind_durably(0).await.unwrap();

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 1); // just "sys"
    assert!(matches!(&conv[0], ConversationItem::System(_)));
    assert_eq!(h.handle.get_prompt_index().await, 0);
}

#[tokio::test]
async fn rewind_rejects_non_earlier_targets() {
    let mut h = TestHarness::new();
    record_prompt(&h.handle, "q1").await;

    let _ = h.handle.get_prompt_index().await;
    h.drain_events();
    h.drain_persistence();

    assert!(matches!(
        h.handle.rewind_durably(1).await,
        Err(crate::TimelineWriteError::InvalidRewindTarget {
            target: 1,
            current: 1
        })
    ));
    assert!(matches!(
        h.handle.rewind_durably(5).await,
        Err(crate::TimelineWriteError::InvalidRewindTarget {
            target: 5,
            current: 1
        })
    ));

    let idx = h.handle.get_prompt_index().await;
    assert_eq!(idx, 1);

    let events = h.drain_events();
    assert!(events.is_empty());
}

// ============================================================================
// Read-model snapshot tests
// ============================================================================

#[tokio::test]
async fn snapshot_combines_timeline_projection_and_runtime_metadata() {
    let h = TestHarness::new();

    // Set up state with non-default values for EVERY field
    h.handle.push_user_message(marked_user("q1", 0));
    h.handle
        .push_assistant_response(ConversationItem::assistant("a1"));
    h.handle.record_provider_context_anchor(999);
    record_prompt(&h.handle, "query 1").await;
    h.handle.record_agent_edited_path("src/foo.rs".to_string());
    h.handle.record_agent_edited_path("src/bar.rs".to_string());
    h.handle.record_stream_start(12345);
    h.handle.record_turn_start(12340);
    let snapshot = h.handle.snapshot().await.unwrap();
    assert_eq!(snapshot.conversation.len(), 2);
    assert_eq!(snapshot.projected_tokens, 999);
    assert_eq!(snapshot.prompt_index, 1);
    assert_eq!(
        snapshot
            .prompt_records
            .iter()
            .map(|record| (record.prompt_index, record.text.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "query 1")]
    );
    assert_eq!(snapshot.agent_edited_paths.len(), 2);
    assert_eq!(snapshot.stream_start_ms, Some(12345));
    assert_eq!(snapshot.turn_start_ms, Some(12340));
}

#[tokio::test]
async fn auto_compact_does_not_trigger_below_threshold() {
    let h = TestHarness::with_context_window(10000);
    h.handle.record_provider_context_anchor(8400);
    let _ = h.handle.get_projected_tokens().await;

    let trigger = h.handle.check_auto_compact_needed(85).await;
    assert!(trigger.is_none());
}

#[tokio::test]
async fn with_initial_conversation_preserves_items() {
    let items = vec![
        ConversationItem::system("sys prompt"),
        ConversationItem::user("initial msg"),
    ];
    let h = TestHarness::with_conversation(items);
    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 2);
}

// ============================================================================
// BuildConversationRequest tests
// ============================================================================

#[tokio::test]
async fn build_request_includes_all_messages() {
    let h = TestHarness::new();
    seed_test_system(&h.handle, "sys").await;
    h.handle.push_user_message(ConversationItem::user("hello"));
    // Sync point
    let _ = h.handle.get_conversation().await;

    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();
    assert_eq!(request.items.len(), 2);
}

#[tokio::test]
async fn build_request_with_empty_conversation() {
    let h = TestHarness::new();
    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();
    assert!(request.items.is_empty());
}

#[tokio::test]
async fn build_request_preserves_system_message() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("You are a coding assistant."),
        ConversationItem::user("hi"),
    ]);
    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();
    assert_eq!(request.items.len(), 2);
    if let ConversationItem::System(ref sys) = request.items[0] {
        assert_eq!(sys.content.as_ref(), "You are a coding assistant.");
    } else {
        panic!("expected System item");
    }
}

#[tokio::test]
async fn build_request_injects_memory_reminder() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("You are helpful."),
        ConversationItem::user("hi"),
    ]);
    let request = h
        .handle
        .build_request(
            "test-timeline",
            vec![],
            Some("Remember: user prefers Rust".to_string()),
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(request.items[0].text_content(), "You are helpful.");
    assert_eq!(request.items.len(), 3);
    assert!(matches!(
        request.items.last(),
        Some(ConversationItem::User(user))
            if user.synthetic_reason
                == Some(sampling_types::SyntheticReason::MemoryContext)
                && request.items.last().unwrap().text_content()
                    == "Remember: user prefers Rust"
    ));
}

#[tokio::test]
async fn build_request_injects_memory_when_no_system() {
    let h = TestHarness::with_conversation(vec![ConversationItem::user("hi")]);
    let request = h
        .handle
        .build_request(
            "test-timeline",
            vec![],
            Some("Remember this".to_string()),
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(request.items.len(), 2);
    assert_eq!(request.items[0].text_content(), "hi");
    assert!(matches!(
        request.items.last(),
        Some(ConversationItem::User(user))
            if user.synthetic_reason
                == Some(sampling_types::SyntheticReason::MemoryContext)
    ));
}

#[tokio::test]
async fn build_request_repairs_dangling_tool_calls() {
    use sampling_types::ToolCall;

    // Repair happens in ChatState::new() (called by with_conversation → spawn),
    // not in build_request. Both handler and clone-level repair are no-ops here.
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("do something"),
        ConversationItem::assistant_tool_calls(vec![ToolCall {
            id: "call_1".into(),
            name: "my_tool".to_string(),
            arguments: "{}".into(),
        }]),
        // No ToolResult for call_1 — repaired by ChatState::new() before any command.
    ]);

    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    // Synthetic ToolResult present (inserted at construction, not at request time).
    assert_eq!(request.items.len(), 4);
    assert!(matches!(&request.items[3], ConversationItem::ToolResult(_)));
}

#[tokio::test]
async fn build_request_with_tool_definitions() {
    use sampling_types::ToolSpec;

    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("hello"),
    ]);

    let tools = vec![ToolSpec {
        name: "read_file".to_string(),
        description: Some("Read a file".to_string()),
        parameters: serde_json::json!({"type": "object"}),
    }];

    let request = h.handle.build_request("test-timeline", tools, None, None, None).await.unwrap();

    assert_eq!(request.tools.len(), 1);
    assert_eq!(request.tools[0].name, "read_file");
}

#[tokio::test]
async fn request_projection_tracks_tool_schema_delta_from_provider_anchor() {
    use sampling_types::ToolSpec;

    let h = TestHarness::with_conversation(vec![ConversationItem::user("hello")]);
    let first = h
        .handle
        .build_request(
            "test-timeline",
            vec![ToolSpec {
                name: "read".into(),
                description: Some("read a file".into()),
                parameters: serde_json::json!({"type": "object"}),
            }],
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        h.handle.get_projected_tokens().await,
        crate::estimate_request_input_tokens(&first)
    );

    h.handle.record_provider_context_anchor(100_000);
    let second = h
        .handle
        .build_request(
            "test-timeline",
            vec![ToolSpec {
                name: "read".into(),
                description: Some("read a file and return every matching line".into()),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}}
                }),
            }],
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let expected_delta = crate::estimate_request_input_tokens(&second)
        - crate::estimate_request_input_tokens(&first);
    assert_eq!(
        h.handle.get_projected_tokens().await,
        100_000 + expected_delta
    );
    let repeated = h
        .handle
        .build_request(
            "test-timeline",
            second.tools.clone(),
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        crate::estimate_request_input_tokens(&repeated),
        crate::estimate_request_input_tokens(&second)
    );
    assert_eq!(
        h.handle.get_projected_tokens().await,
        100_000 + expected_delta,
        "rebuilding an identical envelope must not double-count its adjustment"
    );
}

#[tokio::test]
async fn final_request_projection_accounts_for_goal_shadows_and_json_schema() {
    use sampling_types::{GoalDirectiveTag, JsonOutputFormat, SyntheticReason};

    let old = GoalDirectiveTag {
        goal_id: "goal".into(),
        definition_revision: 1,
    };
    let current = GoalDirectiveTag {
        goal_id: "goal".into(),
        definition_revision: 2,
    };
    let h = TestHarness::with_conversation(vec![
        ConversationItem::goal_directive(
            "obsolete objective ".repeat(1_000),
            SyntheticReason::AutoContinue,
            old,
        ),
        ConversationItem::goal_directive(
            "current objective",
            SyntheticReason::AutoContinue,
            current.clone(),
        ),
    ]);
    let request = h
        .handle
        .build_request(
            "test-timeline",
            vec![],
            None,
            Some(current),
            Some(JsonOutputFormat::JsonSchema(serde_json::json!({
                "type": "object",
                "properties": {"answer": {"type": "string"}}
            }))),
        )
        .await
        .unwrap();

    assert_eq!(
        request.items[0].text_content(),
        sampling_types::SUPERSEDED_GOAL_DIRECTIVE
    );
    assert_eq!(
        h.handle.get_projected_tokens().await,
        crate::estimate_request_input_tokens(&request)
    );
}

#[tokio::test]
async fn build_request_uses_sampling_config() {
    let config = SamplingConfig {
        base_url: "https://api.example.com".to_string(),
        model: "grow-3".to_string(),
        output_limit: Some(8192),
        temperature: Some(0.7),
        top_p: Some(0.9),
        api_backend: Default::default(),
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: NonZeroU64::new(128_000).unwrap(),
        reasoning_effort: None,
        stream_tool_calls: None,
    };
    let h = TestHarness::with_config(vec![ConversationItem::user("hi")], config);

    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    assert_eq!(request.model, Some("grow-3".to_string()));
    assert_eq!(request.temperature, Some(0.7));
    assert_eq!(request.max_output_tokens, Some(8192));
    assert_eq!(request.top_p, Some(0.9));
}

#[tokio::test]
async fn build_request_without_memory_does_not_mutate_actor_state() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("hi"),
    ]);

    let _ = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    // Actor's own conversation should be unchanged
    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 2);
    if let ConversationItem::System(ref sys) = conv[0] {
        assert_eq!(sys.content.as_ref(), "sys");
    }
}

#[tokio::test]
async fn build_request_can_persist_memory_into_actor_state() {
    let mut h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("hi"),
    ]);

    let request = h
        .handle
        .build_request(
            "test-timeline",
            vec![],
            Some("<memory-context>\nRemember this\n</memory-context>".to_string()),
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(request.items[0].text_content(), "sys");
    assert!(matches!(
        request.items.last(),
        Some(ConversationItem::User(user))
            if user.synthetic_reason
                == Some(sampling_types::SyntheticReason::MemoryContext)
    ));

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv[0].text_content(), "sys");
    assert!(conv.last().unwrap().text_content().contains("Remember this"));

    let records = h.drain_persistence();
    assert!(records.iter().filter_map(persisted_messages).any(|event| {
        event.cause == crate::MessageCause::MemoryContext
            && matches!(
                event.items.first(),
                Some(ConversationItem::User(user))
                    if user.synthetic_reason
                        == Some(sampling_types::SyntheticReason::MemoryContext)
            )
    }));
}

#[tokio::test]
async fn persistent_memory_context_retries_an_uncertain_commit() {
    let original = vec![ConversationItem::system("system")];
    let mut h = TestHarness::with_manual_timeline_ack(original.clone());
    let handle = h.handle.clone();
    let build = async move {
        handle
            .build_request(
                "test-timeline",
                vec![],
                Some("remember".to_owned()),
                None,
                None,
            )
            .await
    };
    let retry = fail_once_then_ack_exact_retry(&mut h.persistence_rx);
    let (result, ()) = tokio::join!(build, retry);
    result.unwrap();
    assert_ne!(
        serde_json::to_vec(&h.handle.get_conversation().await).unwrap(),
        serde_json::to_vec(&original).unwrap(),
    );
}

#[tokio::test]
async fn build_request_with_multiple_tool_calls_and_results() {
    use sampling_types::ToolCall;

    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("do things"),
        ConversationItem::assistant_tool_calls(vec![
            ToolCall {
                id: "call_1".into(),
                name: "tool_a".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_2".into(),
                name: "tool_b".to_string(),
                arguments: "{}".into(),
            },
        ]),
        ConversationItem::tool_result("call_1", "result a"),
        ConversationItem::tool_result("call_2", "result b"),
        ConversationItem::assistant("Done!"),
    ]);

    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    // All items should pass through because no repair or projection is needed.
    assert_eq!(request.items.len(), 6);
}

// ============================================================================
// Parallel tool calls with mixed accept/reject
// ============================================================================

/// Simulates the exact sequence that `shell`'s `execute_tool_calls`
/// produces when the model emits 3 parallel tool calls and:
///   - Tool #1 (read_file):       user **accepts** → executed successfully
///   - Tool #2 (edit_file):       user **rejects** → handle_tool_not_executed
///   - Tool #3 (run_terminal_cmd): **skipped** due to earlier rejection
///
/// In the shell, `execute_tool_calls` iterates sequentially. When tool #2 is
/// rejected, `final_result` is set to `PermissionReject`, causing tool #3 to
/// be skipped with a synthetic cancellation message pushed as a ToolResult.
///
/// The conversation should end up as:
///   [0] System
///   [1] User
///   [2] Assistant (3 tool calls)
///   [3] ToolResult for call_1 (success)
///   [4] ToolResult for call_2 (rejection reason)
///   [5] ToolResult for call_3 (cancellation due to earlier rejection)
#[tokio::test]
async fn parallel_tool_calls_accept_first_reject_second_skip_third() {
    use sampling_types::ToolCall;

    let h = TestHarness::new();

    // ── Turn setup ──────────────────────────────────────────────────────
    // System prompt
    seed_test_system(&h.handle, "You are a helpful coding assistant.").await;

    // User asks the model to do something complex
    h.handle.push_user_message(ConversationItem::user(
        "Read main.rs, fix the typo, then run the tests",
    ));

    record_prompt(&h.handle, "tool turn").await;

    // ── Model response: 3 parallel tool calls ───────────────────────────
    // The model's single assistant message contains all 3 tool calls.
    // In the real code, this is built from the streaming response and pushed
    // via `push_assistant_response`.
    let assistant_with_tools = ConversationItem::Assistant(sampling_types::AssistantItem {
        content: "I'll read the file, fix it, and run tests.".into(),
        tool_calls: vec![
            ToolCall {
                id: "call_1".into(),
                name: "read_file".to_string(),
                arguments: r#"{"target_file":"src/main.rs"}"#.into(),
            },
            ToolCall {
                id: "call_2".into(),
                name: "edit_file".to_string(),
                arguments: r#"{"target_file":"src/main.rs","new_string":"fixed"}"#.into(),
            },
            ToolCall {
                id: "call_3".into(),
                name: "run_terminal_cmd".to_string(),
                arguments: r#"{"command":"cargo test"}"#.into(),
            },
        ],
        model_id: Some("grow-3".to_string()),
        model_fingerprint: None,
        reasoning_effort: None,
    });
    h.handle.push_assistant_response(assistant_with_tools);

    // ── Tool execution results (simulating execute_tool_calls) ──────────

    // Tool #1: read_file — user accepted, tool executed successfully
    h.handle.push_tool_result(ConversationItem::tool_result(
        "call_1",
        "fn main() {\n    println!(\"hello wrold\");\n}",
    ));

    // Tool #2: edit_file — user rejected via permission prompt
    // In the shell, `handle_tool_not_executed` pushes a ToolResult with the
    // rejection reason and returns ToolLoop::PermissionReject.
    h.handle.push_tool_result(ConversationItem::tool_result(
        "call_2",
        "User rejected: permission denied for tool `edit_file`",
    ));

    // Tool #3: run_terminal_cmd — skipped because tool #2 was rejected.
    // In `execute_tool_calls`, when `final_result` is set, remaining tools
    // get a synthetic cancellation message.
    h.handle.push_tool_result(ConversationItem::tool_result(
        "call_3",
        "Tool execution cancelled due to earlier permission rejection for tool `run_terminal_cmd`",
    ));

    // ── Verify the conversation state ───────────────────────────────────
    let conv = h.handle.get_conversation().await;

    // Expected: System + User + Assistant(3 calls) + 3 ToolResults = 6 items
    assert_eq!(
        conv.len(),
        6,
        "expected 6 conversation items, got {}",
        conv.len()
    );

    // [0] System
    assert!(
        matches!(&conv[0], ConversationItem::System(s) if s.content.as_ref() == "You are a helpful coding assistant."),
        "item[0] should be the system prompt"
    );

    // [1] User
    assert!(
        matches!(&conv[1], ConversationItem::User(_)),
        "item[1] should be the user message"
    );

    // [2] Assistant with 3 tool calls
    match &conv[2] {
        ConversationItem::Assistant(a) => {
            assert_eq!(a.tool_calls.len(), 3, "assistant should have 3 tool calls");
            assert_eq!(a.tool_calls[0].id.as_ref(), "call_1");
            assert_eq!(a.tool_calls[0].name, "read_file");
            assert_eq!(a.tool_calls[1].id.as_ref(), "call_2");
            assert_eq!(a.tool_calls[1].name, "edit_file");
            assert_eq!(a.tool_calls[2].id.as_ref(), "call_3");
            assert_eq!(a.tool_calls[2].name, "run_terminal_cmd");
            assert_eq!(
                a.content.as_ref(),
                "I'll read the file, fix it, and run tests."
            );
        }
        other => panic!("item[2] should be Assistant, got {:?}", other),
    }

    // [3] ToolResult for call_1 — success
    match &conv[3] {
        ConversationItem::ToolResult(tr) => {
            assert_eq!(tr.tool_call_id, "call_1");
            assert!(
                tr.content.contains("hello wrold"),
                "tool_result for call_1 should contain the file content"
            );
        }
        other => panic!("item[3] should be ToolResult, got {:?}", other),
    }

    // [4] ToolResult for call_2 — rejected
    match &conv[4] {
        ConversationItem::ToolResult(tr) => {
            assert_eq!(tr.tool_call_id, "call_2");
            assert!(
                tr.content.contains("rejected") || tr.content.contains("denied"),
                "tool_result for call_2 should indicate rejection, got: {}",
                tr.content
            );
        }
        other => panic!("item[4] should be ToolResult, got {:?}", other),
    }

    // [5] ToolResult for call_3 — cancelled due to earlier rejection
    match &conv[5] {
        ConversationItem::ToolResult(tr) => {
            assert_eq!(tr.tool_call_id, "call_3");
            assert!(
                tr.content.contains("cancelled")
                    && tr.content.contains("earlier permission rejection"),
                "tool_result for call_3 should indicate cancellation due to earlier rejection, got: {}",
                tr.content
            );
        }
        other => panic!("item[5] should be ToolResult, got {:?}", other),
    }
}

/// After parallel tool calls with rejection, verify that `build_request`
/// sees no dangling tool calls (every call has a matching ToolResult).
/// This is important because dangling calls trigger synthetic repair which
/// would corrupt the rejection messages.
#[tokio::test]
async fn parallel_tool_calls_with_rejection_has_no_dangling_calls() {
    use sampling_types::ToolCall;

    let h = TestHarness::new();

    seed_test_system(&h.handle, "sys").await;
    h.handle
        .push_user_message(ConversationItem::user("do things"));

    // Assistant with 3 parallel tool calls
    h.handle
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![
            ToolCall {
                id: "call_1".into(),
                name: "read_file".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_2".into(),
                name: "edit_file".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_3".into(),
                name: "run_terminal_cmd".to_string(),
                arguments: "{}".into(),
            },
        ]));

    // All 3 get ToolResults (accept, reject, skip)
    h.handle
        .push_tool_result(ConversationItem::tool_result("call_1", "file contents"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call_2", "rejected by user"));
    h.handle.push_tool_result(ConversationItem::tool_result(
        "call_3",
        "Tool execution cancelled due to earlier permission rejection for tool `run_terminal_cmd`",
    ));

    // Build request — should NOT add any synthetic ToolResults
    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    // 2 (sys+user) + 1 (assistant) + 3 (tool results) = 6
    assert_eq!(
        request.items.len(),
        6,
        "build_request should not insert synthetic tool results when all calls have results"
    );

    // Verify the tool results are preserved with their original content
    let tool_results: Vec<_> = request
        .items
        .iter()
        .filter_map(|item| {
            if let ConversationItem::ToolResult(tr) = item {
                Some(tr)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(tool_results.len(), 3);
    assert_eq!(tool_results[0].tool_call_id, "call_1");
    assert_eq!(tool_results[0].content.as_ref(), "file contents");
    assert_eq!(tool_results[1].tool_call_id, "call_2");
    assert_eq!(tool_results[1].content.as_ref(), "rejected by user");
    assert_eq!(tool_results[2].tool_call_id, "call_3");
    assert!(tool_results[2].content.contains("cancelled"));
}

/// Verify that persistence records all 5 pushes (assistant + 3 tool results)
/// correctly for the parallel tool call scenario.
#[tokio::test]
async fn parallel_tool_calls_with_rejection_persists_all_items() {
    use sampling_types::ToolCall;

    let mut h = TestHarness::new();

    seed_test_system(&h.handle, "sys").await;
    h.handle
        .push_user_message(ConversationItem::user("do things"));
    h.handle
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![
            ToolCall {
                id: "call_1".into(),
                name: "read_file".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_2".into(),
                name: "edit_file".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_3".into(),
                name: "run_terminal_cmd".to_string(),
                arguments: "{}".into(),
            },
        ]));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call_1", "success"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call_2", "rejected"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call_3", "cancelled"));

    // Sync point
    let _ = h.handle.get_conversation().await;

    // All 6 items should have been persisted as Timeline message events.
    let records = h.drain_persistence();
    let message_count = records
        .iter()
        .filter_map(persisted_messages)
        .map(|event| event.items.len())
        .sum::<usize>();

    assert_eq!(
        message_count, 6,
        "expected 6 persisted messages (sys + user + assistant + 3 tool results), got {}",
        message_count
    );
}

// ============================================================================
// Race condition: cancellation mid-tool-execution → dangling calls on reload
// ============================================================================

/// Simulates the race condition where:
///   1. Model emits 3 parallel tool calls (single assistant message)
///   2. Tool #1 executes and its result is persisted
///   3. User cancels (Ctrl+C) or app crashes BEFORE tool #2/#3 results are pushed
///   4. On session reload, Timeline has the assistant (3 calls) + only 1 result
///
/// `ChatState::new` now repairs dangling tool calls eagerly at initialization,
/// so the actor's in-memory conversation is clean from the start — not just the
/// clone produced by `build_request`.
#[tokio::test]
async fn dangling_tool_calls_after_crash_are_repaired_on_load() {
    use sampling_types::ToolCall;

    // Simulate what the Timeline Surface looks like after a crash:
    // The assistant message (with 3 tool calls) was persisted, and only
    // tool #1's result was persisted before the process died.
    let crashed_conversation = vec![
        ConversationItem::system("You are a helpful assistant."),
        ConversationItem::user("Read, edit, and test"),
        ConversationItem::Assistant(sampling_types::AssistantItem {
            content: std::sync::Arc::<str>::from(""),
            tool_calls: vec![
                ToolCall {
                    id: "call_1".into(),
                    name: "read_file".to_string(),
                    arguments: r#"{"target_file":"src/main.rs"}"#.into(),
                },
                ToolCall {
                    id: "call_2".into(),
                    name: "edit_file".to_string(),
                    arguments: r#"{"target_file":"src/main.rs","new_string":"fixed"}"#.into(),
                },
                ToolCall {
                    id: "call_3".into(),
                    name: "run_terminal_cmd".to_string(),
                    arguments: r#"{"command":"cargo test"}"#.into(),
                },
            ],
            model_id: Some("grow-3".to_string()),
            model_fingerprint: None,
            reasoning_effort: None,
        }),
        // Only call_1 got persisted before the crash
        ConversationItem::tool_result("call_1", "fn main() { ... }"),
        // call_2 and call_3 are MISSING — this is the dangling state
    ];

    // "Reload" the session by creating an actor with the crashed conversation.
    // ChatState::new repairs dangling tool calls eagerly.
    let h = TestHarness::with_conversation(crashed_conversation);

    // The actor's conversation should already be repaired (6 items, not 4)
    let conv = h.handle.get_conversation().await;
    assert_eq!(
        conv.len(),
        6,
        "actor should repair dangling calls on load: expected 6 items (sys + user + assistant + 3 results), got {}",
        conv.len()
    );

    // Verify the tool results
    let tool_results: Vec<_> = conv
        .iter()
        .filter_map(|item| {
            if let ConversationItem::ToolResult(tr) = item {
                Some(tr)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        tool_results.len(),
        3,
        "should have 3 tool results (1 real + 2 synthetic)"
    );

    // call_1: real result (persisted before crash)
    assert_eq!(tool_results[0].tool_call_id, "call_1");
    assert!(
        tool_results[0].content.contains("fn main"),
        "call_1 should have the original result"
    );

    // call_2: synthetic repair
    assert_eq!(tool_results[1].tool_call_id, "call_2");
    assert!(
        tool_results[1].content.contains("cancelled")
            || tool_results[1].content.contains("not executed"),
        "call_2 should have a synthetic cancellation result, got: {}",
        tool_results[1].content
    );

    // call_3: synthetic repair
    assert_eq!(tool_results[2].tool_call_id, "call_3");
    assert!(
        tool_results[2].content.contains("cancelled")
            || tool_results[2].content.contains("not executed"),
        "call_3 should have a synthetic cancellation result, got: {}",
        tool_results[2].content
    );

    // build_request should also see 6 items (no double-repair)
    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();
    assert_eq!(
        request.items.len(),
        6,
        "build_request should not double-repair already-fixed conversation"
    );
}

/// Verify that both the actor state AND the build_request output are
/// consistent after repair — the synthetic results appear in both.
#[tokio::test]
async fn dangling_tool_calls_repair_is_consistent_between_state_and_request() {
    use sampling_types::ToolCall;

    let crashed_conversation = vec![
        ConversationItem::system("sys"),
        ConversationItem::user("do stuff"),
        ConversationItem::assistant_tool_calls(vec![
            ToolCall {
                id: "call_1".into(),
                name: "read_file".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_2".into(),
                name: "edit_file".to_string(),
                arguments: "{}".into(),
            },
        ]),
        // Only call_1 has a result — call_2 is dangling
        ConversationItem::tool_result("call_1", "ok"),
    ];

    let h = TestHarness::with_conversation(crashed_conversation);

    // Actor state should have 5 items (repaired on load)
    let conv = h.handle.get_conversation().await;
    assert_eq!(
        conv.len(),
        5,
        "actor state should be repaired on load: sys + user + assistant + 2 results"
    );

    // build_request should match (no extra synthetic results)
    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();
    assert_eq!(
        request.items.len(),
        5,
        "build_request should match actor state length"
    );
}

/// Worst case: crash happens right after the assistant message is persisted
/// but BEFORE any tool results. All 3 tool calls are dangling.
/// ChatState::new should repair all 3 eagerly.
#[tokio::test]
async fn all_tool_calls_dangling_after_crash() {
    use sampling_types::ToolCall;

    let crashed_conversation = vec![
        ConversationItem::system("sys"),
        ConversationItem::user("do everything"),
        ConversationItem::assistant_tool_calls(vec![
            ToolCall {
                id: "call_1".into(),
                name: "read_file".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_2".into(),
                name: "edit_file".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_3".into(),
                name: "run_terminal_cmd".to_string(),
                arguments: "{}".into(),
            },
        ]),
        // NO tool results at all — complete crash right after assistant was persisted
    ];

    let h = TestHarness::with_conversation(crashed_conversation);

    // Actor state should be repaired: sys + user + assistant + 3 synthetic results = 6
    let conv = h.handle.get_conversation().await;
    assert_eq!(
        conv.len(),
        6,
        "all 3 dangling calls should be repaired on load"
    );

    let tool_results: Vec<_> = conv
        .iter()
        .filter_map(|item| {
            if let ConversationItem::ToolResult(tr) = item {
                Some(tr)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(tool_results.len(), 3);

    // All should be synthetic cancellation messages
    for (i, tr) in tool_results.iter().enumerate() {
        assert_eq!(tr.tool_call_id, format!("call_{}", i + 1));
        assert!(
            tr.content.contains("cancelled") || tr.content.contains("not executed"),
            "call_{} should have synthetic cancellation, got: {}",
            i + 1,
            tr.content
        );
    }

    // build_request should also see 6 items — no double-repair
    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();
    assert_eq!(request.items.len(), 6);
}

// ============================================================================
// Live-session cancellation: user cancels mid-tool-execution (no restart)
// ============================================================================

/// Simulates an in-session abort where:
///   1. Model emits 3 parallel tool calls → assistant pushed to conversation
///   2. User immediately cancels (Ctrl+C) → tokio task aborted
///   3. Zero tool results pushed (abort happened before execute_tool_calls)
///   4. TUI stays alive, user types a new prompt
///
/// This is different from the reload scenario: `ChatState::new` doesn't run
/// again because the actor is still alive. The fix is that `push_user_message`
/// now calls `repair_dangling_tool_calls` before appending the new user
/// message, so the conversation is cleaned up in-place.
#[tokio::test]
async fn live_cancel_before_any_tool_execution_repairs_on_next_user_message() {
    use sampling_types::ToolCall;

    let h = TestHarness::new();

    // ── Turn 1: normal conversation ─────────────────────────────────────
    seed_test_system(&h.handle, "You are a helpful assistant.").await;
    h.handle.push_user_message(ConversationItem::user("Hello"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("Hi! How can I help?"));

    // ── Turn 2: model wants 3 tool calls, user cancels immediately ──────
    h.handle
        .push_user_message(ConversationItem::user("Read, edit, and test everything"));

    // Model streams its response → assistant with 3 tool calls is pushed
    h.handle
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![
            ToolCall {
                id: "call_1".into(),
                name: "read_file".to_string(),
                arguments: r#"{"target_file":"src/main.rs"}"#.into(),
            },
            ToolCall {
                id: "call_2".into(),
                name: "edit_file".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_3".into(),
                name: "run_terminal_cmd".to_string(),
                arguments: r#"{"command":"cargo test"}"#.into(),
            },
        ]));

    // *** USER CANCELS HERE (Ctrl+C) ***
    // The tokio task is aborted. execute_tool_calls never ran.
    // Zero ToolResult items pushed. The conversation has dangling calls.

    // get_conversation() and snapshot() are pure reads — they do NOT repair.
    // The dangling calls are visible in the raw state until the next write boundary.
    let conv_before = h.handle.get_conversation().await;
    // sys + user("Hello") + assistant("Hi!") + user("Read...") + assistant(3 calls) = 5
    assert_eq!(
        conv_before.len(),
        5,
        "get_conversation() should be a pure read (no repair), got {} items",
        conv_before.len()
    );

    let snap = h.handle.snapshot().await.unwrap();
    assert_eq!(
        snap.conversation.len(),
        5,
        "snapshot() should be a pure read (no repair)"
    );

    // ── Turn 3: push_user_message() is the write boundary; repairs here.
    h.handle
        .push_user_message(ConversationItem::user("Actually, just read the file"));

    let conv = h.handle.get_conversation().await;

    // sys + user + assistant + user + assistant(3 calls) + 3 repairs + user = 9
    assert_eq!(conv.len(), 9);

    // Verify the synthetic repairs are in the right place
    for (idx, expected_call_id) in [(5, "call_1"), (6, "call_2"), (7, "call_3")] {
        match &conv[idx] {
            ConversationItem::ToolResult(tr) => {
                assert_eq!(tr.tool_call_id, expected_call_id);
                assert!(
                    tr.content.contains("cancelled") || tr.content.contains("not executed"),
                    "item[{}] should be synthetic cancellation for {}, got: {}",
                    idx,
                    expected_call_id,
                    tr.content
                );
            }
            other => panic!(
                "item[{}] should be synthetic ToolResult for {}, got {:?}",
                idx, expected_call_id, other
            ),
        }
    }

    // New user message is at the end
    assert!(
        matches!(&conv[8], ConversationItem::User(_)),
        "item[8] should be the new user message"
    );

    // build_request should work cleanly — no dangling calls, no double-repair
    let request = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();
    assert_eq!(request.items.len(), 9);
}

/// Partial cancellation: tool #1 result was pushed, then user cancelled.
/// Tools #2 and #3 are dangling. Next user message should repair only those.
#[tokio::test]
async fn live_cancel_after_partial_tool_results_repairs_remaining() {
    use sampling_types::ToolCall;

    let h = TestHarness::new();

    seed_test_system(&h.handle, "sys").await;
    h.handle
        .push_user_message(ConversationItem::user("do everything"));

    // Model returns 3 parallel tool calls
    h.handle
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![
            ToolCall {
                id: "call_1".into(),
                name: "read_file".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_2".into(),
                name: "edit_file".to_string(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "call_3".into(),
                name: "run_terminal_cmd".to_string(),
                arguments: "{}".into(),
            },
        ]));

    // Tool #1 executed and result was pushed before abort
    h.handle.push_tool_result(ConversationItem::tool_result(
        "call_1",
        "file contents here",
    ));

    // *** USER CANCELS HERE — tool #2 and #3 never executed ***

    // User types a new prompt
    h.handle.push_user_message(ConversationItem::user(
        "never mind, just help me with something else",
    ));

    let conv = h.handle.get_conversation().await;

    // sys + user + assistant(3 calls) + result(call_1) + repair(call_2) + repair(call_3) + user(new) = 7
    assert_eq!(
        conv.len(),
        7,
        "expected 7 items after partial repair, got {}",
        conv.len()
    );

    // call_1 should still have the real result
    match &conv[3] {
        ConversationItem::ToolResult(tr) => {
            assert_eq!(tr.tool_call_id, "call_1");
            assert_eq!(tr.content.as_ref(), "file contents here");
        }
        other => panic!(
            "item[3] should be real ToolResult for call_1, got {:?}",
            other
        ),
    }

    // call_2 and call_3 should be synthetic repairs
    for (idx, expected_call_id) in [(4, "call_2"), (5, "call_3")] {
        match &conv[idx] {
            ConversationItem::ToolResult(tr) => {
                assert_eq!(tr.tool_call_id, expected_call_id);
                assert!(
                    tr.content.contains("cancelled") || tr.content.contains("not executed"),
                    "item[{}] should be synthetic for {}, got: {}",
                    idx,
                    expected_call_id,
                    tr.content
                );
            }
            other => panic!("item[{}] should be ToolResult, got {:?}", idx, other),
        }
    }

    // New user message at the end
    assert!(matches!(&conv[6], ConversationItem::User(_)));
}

// Turn message capture tests
// ============================================================================

#[tokio::test]
async fn turn_capture_collects_all_message_types() {
    let h = TestHarness::new();

    h.handle.begin_turn_capture();
    h.handle.push_user_message(ConversationItem::user("hello"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("hi"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-1", "result"));

    let capture = h
        .handle
        .take_turn_messages()
        .await
        .expect("capture was active");

    assert_eq!(capture.messages.len(), 3);
    assert!(matches!(&capture.messages[0], ConversationItem::User(_)));
    assert!(matches!(
        &capture.messages[1],
        ConversationItem::Assistant(_)
    ));
    assert!(matches!(
        &capture.messages[2],
        ConversationItem::ToolResult(_)
    ));
    assert!(!capture.compaction_occurred);
}

#[tokio::test]
async fn turn_capture_survives_compaction_and_flags_it() {
    let h = TestHarness::new();

    h.handle.begin_turn_capture();
    h.handle.push_user_message(ConversationItem::user("q1"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("a1"));

    commit_compaction_range(&h.handle, vec![ConversationItem::user("compacted")]).await;

    h.handle.push_user_message(marked_user("q2", 1));

    let capture = h
        .handle
        .take_turn_messages()
        .await
        .expect("capture was active");

    assert_eq!(capture.messages.len(), 3);
    assert!(matches!(&capture.messages[0], ConversationItem::User(_)));
    assert!(matches!(
        &capture.messages[1],
        ConversationItem::Assistant(_)
    ));
    assert!(matches!(&capture.messages[2], ConversationItem::User(_)));
    assert!(capture.compaction_occurred);
}

#[tokio::test]
async fn replace_conversation_without_compaction_does_not_set_flag() {
    let h = TestHarness::new();

    h.handle.begin_turn_capture();
    h.handle.push_user_message(ConversationItem::user("q1"));

    replace_test_surface(&h.handle, vec![ConversationItem::user("new state")]).await;

    let capture = h
        .handle
        .take_turn_messages()
        .await
        .expect("capture was active");

    assert!(!capture.compaction_occurred);
}

#[tokio::test]
async fn take_without_begin_returns_none() {
    let h = TestHarness::new();

    h.handle.push_user_message(ConversationItem::user("hello"));

    let result = h.handle.take_turn_messages().await;
    assert!(result.is_none());
}

#[tokio::test]
async fn take_twice_returns_none_second_time() {
    let h = TestHarness::new();

    h.handle.begin_turn_capture();
    h.handle.push_user_message(ConversationItem::user("hello"));

    let first = h.handle.take_turn_messages().await;
    assert!(first.is_some());

    let second = h.handle.take_turn_messages().await;
    assert!(second.is_none());
}

#[tokio::test]
async fn begin_capture_clears_previous_buffer() {
    let h = TestHarness::new();

    h.handle.begin_turn_capture();
    h.handle.push_user_message(ConversationItem::user("old"));

    h.handle.begin_turn_capture();
    h.handle.push_user_message(ConversationItem::user("new"));

    let capture = h
        .handle
        .take_turn_messages()
        .await
        .expect("capture was active");

    assert_eq!(capture.messages.len(), 1);
    assert!(matches!(&capture.messages[0], ConversationItem::User(_)));
}

#[tokio::test]
async fn truncate_clears_turn_capture() {
    let h = TestHarness::new();

    h.handle.push_user_message(marked_user("q1", 0));
    record_prompt(&h.handle, "q1").await;
    h.handle
        .push_assistant_response(ConversationItem::assistant("a1"));

    h.handle.begin_turn_capture();
    h.handle.push_user_message(marked_user("q2", 1));
    record_prompt(&h.handle, "q2").await;

    h.handle.rewind_durably(1).await.unwrap();

    let result = h.handle.take_turn_messages().await;
    assert!(result.is_none());
}

#[tokio::test]
async fn turn_capture_survives_integrity_repair_prefix_shrink() {
    use sampling_types::ToolCall;
    let h = TestHarness::new();

    // Build a prefix (before the capture starts) holding three removable
    // duplicate ToolResults — one per tool call. `dedup_duplicate_tool_results`
    // keeps the last result per id and drops the earlier one, shrinking the
    // prefix by three items when integrity repair later runs.
    let call = |id: &'static str| ToolCall {
        id: id.into(),
        name: "t".into(),
        arguments: "{}".into(),
    };
    h.handle
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![
            call("call-1"),
            call("call-2"),
            call("call-3"),
        ]));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-1", "dup-1"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-1", "real-1"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-2", "dup-2"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-2", "real-2"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-3", "dup-3"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("call-3", "real-3"));

    // Capture starts after the 7-item prefix: turn_start_offset == 7.
    h.handle.begin_turn_capture();

    // First turn item lands while the prefix duplicates are still present.
    h.handle
        .push_assistant_response(ConversationItem::assistant("turn-1"));

    // Integrity repair removes the three prefix duplicates, shrinking the
    // conversation to len 5 while the un-rebased offset stays at 7 (offset 7 >
    // len 5). Without the fix the later take_turn_messages slice is out of range,
    // panics the actor, and the query comes back as None.
    h.handle.repair_dangling_after_harness_halt("test-halt");

    // Second turn item lands after the rebase — it must still be captured.
    h.handle
        .push_assistant_response(ConversationItem::assistant("turn-2"));

    let capture = h
        .handle
        .take_turn_messages()
        .await
        .expect("capture survived the in-place integrity repair (no actor panic)");

    // turn-1 via the pre-repair snapshot, turn-2 via the rebased offset; none of
    // the deduped prefix items leak in.
    assert_eq!(capture.messages.len(), 2);
    assert!(matches!(
        &capture.messages[0],
        ConversationItem::Assistant(a) if a.content.as_ref() == "turn-1" && a.tool_calls.is_empty()
    ));
    assert!(matches!(
        &capture.messages[1],
        ConversationItem::Assistant(a) if a.content.as_ref() == "turn-2" && a.tool_calls.is_empty()
    ));
}

#[tokio::test]
async fn integrity_repair_does_not_flag_compaction() {
    let h = TestHarness::new();

    h.handle.begin_turn_capture();
    h.handle.push_user_message(ConversationItem::user("q1"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("a1"));

    // An in-place integrity repair goes through `snapshot_turn_slice` like
    // compaction does, but it is NOT compaction — the flag must stay unset.
    h.handle.repair_dangling_after_harness_halt("test-halt");

    let capture = h
        .handle
        .take_turn_messages()
        .await
        .expect("capture was active");

    assert!(!capture.compaction_occurred);
    assert_eq!(capture.messages.len(), 2);
}

#[tokio::test]
async fn turn_capture_records_persisted_memory_context_append() {
    // Retrieved memory is a durable typed append, never a synthetic System
    // prepend or a rewrite of captured turn messages.
    let h = TestHarness::with_conversation(vec![ConversationItem::user("hi")]);

    h.handle.begin_turn_capture();
    h.handle.push_user_message(ConversationItem::user("turn-q"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("turn-a"));

    let request = h
        .handle
        .build_request(
            "test-timeline",
            vec![],
            Some("Remember this".to_string()),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(request.items[0].text_content(), "hi");
    assert!(matches!(
        request.items.last(),
        Some(ConversationItem::User(user))
            if user.synthetic_reason
                == Some(sampling_types::SyntheticReason::MemoryContext)
    ));

    let capture = h
        .handle
        .take_turn_messages()
        .await
        .expect("capture was active");

    assert_eq!(capture.messages.len(), 3);
    assert!(matches!(&capture.messages[0], ConversationItem::User(_)));
    assert!(matches!(
        &capture.messages[1],
        ConversationItem::Assistant(a) if a.content.as_ref() == "turn-a"
    ));
    assert!(matches!(
        &capture.messages[2],
        ConversationItem::User(user)
            if user.synthetic_reason
                == Some(sampling_types::SyntheticReason::MemoryContext)
    ));
}

// ============================================================================
// Narrow targeted query tests
// ============================================================================

#[tokio::test]
async fn get_conversation_len_empty() {
    let h = TestHarness::new();
    assert_eq!(h.handle.get_conversation_len().await, 0);
}

#[tokio::test]
async fn get_conversation_len_matches_full_conversation() {
    let h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("a"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("b"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("c1", "r"));

    // Use len query as sync point and verify it matches the full vec
    let len = h.handle.get_conversation_len().await;
    let full = h.handle.get_conversation().await;
    assert_eq!(len, full.len());
    assert_eq!(len, 3);
}

#[tokio::test]
async fn has_dangling_tool_calls_reflects_unanswered_calls() {
    use sampling_types::ToolCall;
    let h = TestHarness::new();
    assert!(!h.handle.has_dangling_tool_calls().await);

    // `push_assistant_response` does not run the repair, so the unanswered call
    // stays dangling (mirrors a turn parked mid-tool / on a permission prompt).
    h.handle
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![ToolCall {
            id: "call_1".into(),
            name: "my_tool".to_string(),
            arguments: "{}".into(),
        }]));
    assert!(h.handle.has_dangling_tool_calls().await);

    // Answering the call clears the dangling state.
    h.handle
        .push_tool_result(ConversationItem::tool_result("call_1", "ok"));
    assert!(!h.handle.has_dangling_tool_calls().await);
}

#[tokio::test]
async fn get_last_assistant_text_empty_conversation() {
    let h = TestHarness::new();
    assert!(h.handle.get_last_assistant_text().await.is_none());
}

#[tokio::test]
async fn get_last_assistant_text_returns_last_nonempty() {
    let h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("q1"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("first answer"));
    h.handle.push_user_message(ConversationItem::user("q2"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("second answer"));

    let text = h.handle.get_last_assistant_text().await;
    assert_eq!(text.as_deref(), Some("second answer"));
}

#[tokio::test]
async fn get_last_assistant_text_skips_whitespace_only() {
    let h = TestHarness::new();
    h.handle
        .push_assistant_response(ConversationItem::assistant("real answer"));
    // Push a whitespace-only assistant message after the real one
    h.handle
        .push_assistant_response(ConversationItem::assistant("   \n  "));

    let text = h.handle.get_last_assistant_text().await;
    // Must skip the whitespace-only entry and return the previous one
    assert_eq!(text.as_deref(), Some("real answer"));
}

#[tokio::test]
async fn get_last_assistant_text_in_turn_stops_at_boundary() {
    let h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("q1"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("previous turn answer"));
    h.handle.push_user_message(ConversationItem::user("q2"));

    assert!(h.handle.get_last_assistant_text_in_turn().await.is_none());
    assert_eq!(
        h.handle.get_last_assistant_text().await.as_deref(),
        Some("previous turn answer"),
        "the unbounded sibling still sees prior turns"
    );
}

#[tokio::test]
async fn get_last_assistant_text_in_turn_walks_past_synthetic_injections() {
    let h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("q"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("turn answer"));
    h.handle
        .push_user_message(ConversationItem::stop_hook_feedback("keep working"));

    assert_eq!(
        h.handle.get_last_assistant_text_in_turn().await.as_deref(),
        Some("turn answer"),
        "synthetic mid-turn items must not act as turn boundaries"
    );

    // A turn-starting synthetic item (auto-wake) IS a boundary.
    h.handle
        .push_user_message(ConversationItem::task_completed("task done"));
    assert!(h.handle.get_last_assistant_text_in_turn().await.is_none());
}

#[tokio::test]
async fn get_last_assistant_text_no_assistant_messages() {
    let h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("hi"));
    assert!(h.handle.get_last_assistant_text().await.is_none());
}

#[tokio::test]
async fn get_first_user_text_empty_conversation() {
    let h = TestHarness::new();
    assert!(h.handle.get_first_user_text().await.is_none());
}

#[tokio::test]
async fn get_first_user_text_returns_first_user_text() {
    let h = TestHarness::new();
    seed_test_system(&h.handle, "sys").await;
    h.handle
        .push_user_message(ConversationItem::user("hello world"));
    h.handle.push_user_message(ConversationItem::user("second"));

    let text = h.handle.get_first_user_text().await;
    assert_eq!(text.as_deref(), Some("hello world"));
}

#[tokio::test]
async fn get_first_user_text_no_user_messages() {
    let h = TestHarness::new();
    seed_test_system(&h.handle, "sys only").await;

    assert!(h.handle.get_first_user_text().await.is_none());
}

#[tokio::test]
async fn get_conversation_item_at_out_of_bounds() {
    let h = TestHarness::new();
    assert!(h.handle.get_conversation_item_at(0).await.is_none());
    assert!(h.handle.get_conversation_item_at(99).await.is_none());
}

#[tokio::test]
async fn get_conversation_item_at_returns_correct_item() {
    let h = TestHarness::new();
    seed_test_system(&h.handle, "sys").await;
    h.handle.push_user_message(ConversationItem::user("hello"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("hi"));

    // Sync point
    let _ = h.handle.get_conversation_len().await;

    let item0 = h.handle.get_conversation_item_at(0).await.unwrap();
    assert!(matches!(item0, ConversationItem::System(_)));

    let item1 = h.handle.get_conversation_item_at(1).await.unwrap();
    assert!(matches!(item1, ConversationItem::User(_)));

    let item2 = h.handle.get_conversation_item_at(2).await.unwrap();
    assert!(matches!(item2, ConversationItem::Assistant(_)));

    assert!(h.handle.get_conversation_item_at(3).await.is_none());
}

#[tokio::test]
async fn get_conversation_item_at_does_not_mutate_state() {
    let h = TestHarness::new();
    seed_test_system(&h.handle, "sys").await;
    h.handle.push_user_message(ConversationItem::user("q"));

    // Fetching item[1] should not change what get_conversation returns
    let _ = h.handle.get_conversation_item_at(1).await;

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 2);
}

// ── Multimodal regression tests for get_first_user_text() ────────────────────

/// Confirms that `get_first_user_text()` returns `None` when the first content
/// part of the first user message is an image (not text). This preserves the
/// original call-site semantics used by the memory-search path.
#[tokio::test]
async fn get_first_user_text_image_first_returns_none() {
    use sampling_types::{ContentPart, UserItem};

    let h = TestHarness::new();
    // First message: image-only user message (no text part)
    h.handle.push_user_message(ConversationItem::User(UserItem {
        content: vec![ContentPart::Image {
            url: "data:image/png;base64,abc".into(),
        }],
        synthetic_reason: None,
        permission_evidence: None,
        ..Default::default()
    }));

    // Must return None — first content part is not Text.
    assert!(h.handle.get_first_user_text().await.is_none());
}

/// Confirms that when the first user message has an image first and text second,
/// `get_first_user_text()` still returns `None` (first-part-is-text semantics).
#[tokio::test]
async fn get_first_user_text_image_then_text_returns_none() {
    use sampling_types::{ContentPart, UserItem};

    let h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::User(UserItem {
        content: vec![
            ContentPart::Image {
                url: "data:image/png;base64,abc".into(),
            },
            ContentPart::Text {
                text: "describe this image".into(),
            },
        ],
        synthetic_reason: None,
        permission_evidence: None,
        ..Default::default()
    }));

    // Still None — first part is an image, so we do not fall through to the text part.
    assert!(h.handle.get_first_user_text().await.is_none());
}

/// Confirms the happy path for text-first multimodal messages.
#[tokio::test]
async fn get_first_user_text_text_then_image_returns_text() {
    use sampling_types::{ContentPart, UserItem};

    let h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::User(UserItem {
        content: vec![
            ContentPart::Text {
                text: "look at this".into(),
            },
            ContentPart::Image {
                url: "data:image/png;base64,abc".into(),
            },
        ],
        synthetic_reason: None,
        permission_evidence: None,
        ..Default::default()
    }));

    let text = h.handle.get_first_user_text().await;
    assert_eq!(text.as_deref(), Some("look at this"));
}

// ── Tests for GetLastUserQueryText, GetConversationCounts, GetSystemMessage ───

#[tokio::test]
async fn get_last_user_query_text_empty_conversation() {
    let h = TestHarness::new();
    assert!(h.handle.get_last_user_query_text().await.is_none());
}

#[tokio::test]
async fn get_last_user_query_text_returns_last() {
    let h = TestHarness::new();
    h.handle
        .push_user_message(ConversationItem::user("first question"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("answer"));
    h.handle
        .push_user_message(ConversationItem::user("second question"));

    let text = h.handle.get_last_user_query_text().await;
    assert_eq!(text.as_deref(), Some("second question"));
}

#[tokio::test]
async fn get_conversation_counts_empty() {
    let h = TestHarness::new();
    let counts = h.handle.get_conversation_counts().await;
    assert_eq!(counts.total, 0);
    assert_eq!(counts.user, 0);
    assert_eq!(counts.assistant, 0);
    assert_eq!(counts.tool_result, 0);
}

#[tokio::test]
async fn get_conversation_counts_mixed() {
    let h = TestHarness::new();
    seed_test_system(&h.handle, "sys").await;
    h.handle.push_user_message(ConversationItem::user("q1"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("a1"));
    h.handle
        .push_tool_result(ConversationItem::tool_result("c1", "r1"));
    h.handle.push_user_message(ConversationItem::user("q2"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("a2"));

    let counts = h.handle.get_conversation_counts().await;
    assert_eq!(counts.total, 6);
    assert_eq!(counts.user, 2);
    assert_eq!(counts.assistant, 2);
    assert_eq!(counts.tool_result, 1);
}

#[tokio::test]
async fn get_system_message_none_when_absent() {
    let h = TestHarness::new();
    h.handle.push_user_message(ConversationItem::user("no sys"));
    assert!(h.handle.get_system_message().await.is_none());
}

#[tokio::test]
async fn get_system_message_returns_first_system() {
    let h = TestHarness::new();
    seed_test_system(&h.handle, "You are helpful.").await;
    h.handle.push_user_message(ConversationItem::user("hi"));

    let sys = h.handle.get_system_message().await.unwrap();
    assert!(matches!(sys, ConversationItem::System(s) if s.content.as_ref() == "You are helpful."));
}

// ============================================================================
// Stable System seed regression tests.
// ============================================================================

#[tokio::test]
async fn fresh_timeline_can_be_seeded_with_a_system_message() {
    let h = TestHarness::new(); // spawns with vec![]

    // At this point the actor has no system message, mirroring the bug.
    assert!(h.handle.get_system_message().await.is_none());

    let system_prompt = "You are a helpful subagent.".to_string();
    let conversation = vec![ConversationItem::system(system_prompt.clone())];
    replace_test_surface(&h.handle, conversation).await;

    // After the sync the system message must be available for compaction.
    let sys = h.handle.get_system_message().await.unwrap();
    assert!(
        matches!(&sys, ConversationItem::System(s) if s.content.as_ref() == system_prompt),
        "expected system prompt after replace_conversation, got {sys:?}"
    );
}

#[tokio::test]
async fn an_existing_timeline_rejects_system_head_replacement() {
    let parent_prompt = "You are the parent agent.".to_string();
    let child_prompt = "You are the child subagent.".to_string();

    // Simulate a forked subagent: actor starts with the parent's conversation
    // which already contains the parent's system message.
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system(parent_prompt.clone()),
        ConversationItem::user("hello"),
        ConversationItem::assistant("hi"),
    ]);

    // Verify the parent's system message is present initially.
    let sys = h.handle.get_system_message().await.unwrap();
    assert!(matches!(&sys, ConversationItem::System(s) if s.content.as_ref() == parent_prompt));

    let mut conversation = vec![
        ConversationItem::system(parent_prompt.clone()),
        ConversationItem::user("hello"),
        ConversationItem::assistant("hi"),
    ];
    if let Some(ConversationItem::System(sys)) = conversation.first_mut() {
        sys.content = child_prompt.clone().into();
    }
    let (_, source_revision) = h
        .handle
        .get_conversation_with_revision()
        .await
        .expect("actor must provide Surface revision");
    assert!(
        h.handle
            .replace_context_durably(conversation, source_revision)
            .await
            .is_err(),
        "a child head must be chosen before Timeline creation"
    );

    let sys = h.handle.get_system_message().await.unwrap();
    assert!(
        matches!(&sys, ConversationItem::System(s) if s.content.as_ref() == parent_prompt),
        "the persisted System head must remain unchanged, got {sys:?}"
    );

    // The rest of the conversation must be preserved.
    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 3);
}

#[tokio::test]
async fn get_last_model_metadata_returns_both_fields() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("hi"),
        ConversationItem::Assistant(sampling_types::AssistantItem {
            content: "hello".into(),
            tool_calls: vec![],
            model_id: Some("grow-4.5".into()),
            model_fingerprint: Some("fp_abc123".into()),
            reasoning_effort: None,
        }),
    ]);
    let meta = h.handle.get_last_model_metadata().await;
    assert_eq!(meta.resolved_model_id.as_deref(), Some("grow-4.5"));
    assert_eq!(meta.model_fingerprint.as_deref(), Some("fp_abc123"));
}

#[tokio::test]
async fn get_last_model_metadata_returns_default_when_no_assistant() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("hi"),
    ]);
    let meta = h.handle.get_last_model_metadata().await;
    assert!(meta.resolved_model_id.is_none());
    assert!(meta.model_fingerprint.is_none());
}

/// Reproduce: after compaction replaces the conversation, `get_sampling_config`
/// must still return the original model/context_window/api_backend. The
/// `SamplingConfig` lives in a separate field — `replace_conversation` must
/// not touch it.
#[tokio::test]
async fn sampling_config_survives_compaction_replacement() {
    use sampling_types::ApiBackend;

    let config = SamplingConfig {
        base_url: "https://api.example.com".to_string(),
        model: "grow-build".to_string(),
        output_limit: None,
        temperature: Some(0.7),
        top_p: Some(0.95),
        api_backend: ApiBackend::Responses,
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: NonZeroU64::new(500_000).unwrap(),
        reasoning_effort: None,
        stream_tool_calls: None,
    };

    let h = TestHarness::with_config(
        vec![
            ConversationItem::system("You are a helpful assistant."),
            ConversationItem::user("fix the bug"),
            ConversationItem::Assistant(sampling_types::AssistantItem {
                content: "I'll fix it.".into(),
                tool_calls: vec![],
                model_id: Some("grow-4.5".into()),
                model_fingerprint: Some("fp_abc123".into()),
                reasoning_effort: None,
            }),
        ],
        config,
    );

    // Pre-compaction: everything correct.
    let pre = h.handle.get_sampling_config().await.unwrap();
    assert_eq!(pre.model, "grow-build");
    assert_eq!(pre.context_window.get(), 500_000);
    assert_eq!(pre.api_backend, ApiBackend::Responses);

    let pre_meta = h.handle.get_last_model_metadata().await;
    assert_eq!(pre_meta.resolved_model_id.as_deref(), Some("grow-4.5"));
    assert_eq!(pre_meta.model_fingerprint.as_deref(), Some("fp_abc123"));

    // Simulate compaction: shadow only the body with a user summary. The
    // stable System head remains outside the compaction range.
    commit_compaction_range(
        &h.handle,
        vec![ConversationItem::user(
            "Compaction summary: user asked to fix a bug...",
        )],
    )
    .await;

    // Post-compaction: SamplingConfig MUST be preserved.
    let post = h.handle.get_sampling_config().await.unwrap();
    assert_eq!(
        post.model, "grow-build",
        "BUG: model changed after compaction"
    );
    assert_eq!(
        post.context_window.get(),
        500_000,
        "BUG: context_window dropped to default after compaction"
    );
    assert_eq!(
        post.api_backend,
        ApiBackend::Responses,
        "BUG: api_backend switched to ChatCompletions after compaction"
    );

    // Post-compaction: model metadata is LOST (no AssistantItem in compacted history).
    // This is the visible symptom -- fingerprint/hash disappears from /session-info.
    let post_meta = h.handle.get_last_model_metadata().await;
    assert!(
        post_meta.resolved_model_id.is_none(),
        "resolved_model_id should be None after compaction (no AssistantItem)"
    );
    assert!(
        post_meta.model_fingerprint.is_none(),
        "model_fingerprint should be None after compaction (no AssistantItem)"
    );
}

/// After compaction, the `build_session_info` display path uses
/// `get_sampling_config().model` as the source-of-truth model slug.
/// If that model slug is e.g. "grow-build" and not in the ModelState
/// catalog with a display name, the pager shows the raw slug. This
/// test verifies the pager's `current_model_name()` behavior when the
/// model ID doesn't match any catalog entry.
#[tokio::test]
async fn model_metadata_lost_after_compaction_then_recovered_on_next_turn() {
    let config = SamplingConfig {
        base_url: "https://api.example.com".to_string(),
        model: "grow-build".to_string(),
        output_limit: None,
        temperature: Some(0.7),
        top_p: Some(0.95),
        api_backend: Default::default(),
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: NonZeroU64::new(500_000).unwrap(),
        reasoning_effort: None,
        stream_tool_calls: None,
    };

    let h = TestHarness::with_config(
        vec![
            ConversationItem::system("sys"),
            ConversationItem::user("task"),
            ConversationItem::Assistant(sampling_types::AssistantItem {
                content: "done".into(),
                tool_calls: vec![],
                model_id: Some("grow-4.5".into()),
                model_fingerprint: Some("fp_acd3142484d3ad6f".into()),
                reasoning_effort: None,
            }),
        ],
        config,
    );

    // Before compaction: metadata present.
    let meta = h.handle.get_last_model_metadata().await;
    assert_eq!(meta.resolved_model_id.as_deref(), Some("grow-4.5"));
    assert_eq!(
        meta.model_fingerprint.as_deref(),
        Some("fp_acd3142484d3ad6f")
    );

    // Compaction replaces conversation — no AssistantItem in compacted history.
    commit_compaction_range(&h.handle, vec![ConversationItem::user("compacted summary")]).await;

    // Metadata gone.
    let meta = h.handle.get_last_model_metadata().await;
    assert!(meta.resolved_model_id.is_none());
    assert!(meta.model_fingerprint.is_none());

    // Simulate next turn: model responds and metadata is restored.
    h.handle
        .push_user_message(ConversationItem::user("next task"));
    h.handle
        .push_assistant_response(ConversationItem::Assistant(sampling_types::AssistantItem {
            content: "working on it".into(),
            tool_calls: vec![],
            model_id: Some("grow-4.5".into()),
            model_fingerprint: Some("fp_acd3142484d3ad6f".into()),
            reasoning_effort: None,
        }));

    // Metadata recovered.
    let meta = h.handle.get_last_model_metadata().await;
    assert_eq!(meta.resolved_model_id.as_deref(), Some("grow-4.5"));
    assert_eq!(
        meta.model_fingerprint.as_deref(),
        Some("fp_acd3142484d3ad6f")
    );
}

/// Verify that a context_window downgrade via `update_sampling_config`
/// causes `check_auto_compact_needed` to fire when token usage already
/// exceeds the new (smaller) window.
///
/// This exercises the actor's arithmetic: if anything (model switch,
/// session resume, etc.) shrinks the context window below accumulated
/// token usage, auto-compact must trigger.
///
/// Note: `handle_model_metadata_update` in acp_session.rs now blocks
/// response-header downgrades (only upgrades accepted), so this path
/// is mainly reachable via model switches. The actor itself still
/// accepts any value via `update_sampling_config` — the guard lives
/// in the session layer.
#[tokio::test]
async fn context_window_downgrade_triggers_auto_compact() {
    use sampling_types::ApiBackend;

    // Initial config: 500k context, Responses backend (matches grow-4.5)
    let config = SamplingConfig {
        base_url: "https://api.example.com/v1".to_string(),
        model: "grow-4.5".to_string(),
        output_limit: None,
        temperature: Some(0.7),
        top_p: Some(0.95),
        api_backend: ApiBackend::Responses,
        extra_headers: Default::default(),
        query_params: Default::default(),
        env_http_headers: Default::default(),
        context_window: NonZeroU64::new(500_000).unwrap(),
        reasoning_effort: None,
        stream_tool_calls: None,
    };

    let h = TestHarness::with_config(vec![], config);

    // Simulate 217k tokens of conversation (matching turn 587's total_tokens)
    h.handle.record_provider_context_anchor(217_000);

    // Pre-downgrade: 217k / 500k = 43% — well under auto-compact threshold
    let pre = h.handle.get_sampling_config().await.unwrap();
    assert_eq!(pre.context_window.get(), 500_000);

    let trigger = h.handle.check_auto_compact_needed(85).await;
    assert!(
        trigger.is_none(),
        "should NOT trigger auto-compact at 43% (217k/500k)"
    );

    // Simulate a context_window downgrade (e.g. model switch, response
    // header from cli-chat-proxy, or stale prefetched model list).
    let mut downgraded = pre.clone();
    downgraded.context_window = NonZeroU64::new(128_000).unwrap();
    h.handle.update_sampling_config(downgraded);

    // Post-downgrade: 217k / 128k = 170% — massively over capacity
    let post = h.handle.get_sampling_config().await.unwrap();
    assert_eq!(
        post.context_window.get(),
        128_000,
        "context_window should be overwritten by update_sampling_config"
    );
    assert_eq!(post.model, "grow-4.5", "model slug must not change");
    assert_eq!(
        post.api_backend,
        ApiBackend::Responses,
        "api_backend must not change"
    );

    // Now auto-compact sees the 128k window and fires
    let trigger = h.handle.check_auto_compact_needed(85).await;
    assert!(
        trigger.is_some(),
        "MUST trigger auto-compact at 170% (217k/128k) — this is the bug!"
    );
    let info = trigger.unwrap();
    assert_eq!(info.context_window, NonZeroU64::new(128_000).unwrap());
    assert_eq!(info.projected_tokens, 217_000);
    // utilization_percent is u8 so it caps at 255, but we just need >85
    assert!(
        info.utilization_percent > 85,
        "utilization should be well above threshold but got {}%",
        info.utilization_percent
    );
}

// ============================================================================
// KV Cache Prefix Stability Tests
//
// These test `build_conversation_request()` output prefix stability through
// the full pipeline -- memory injection, image projection, snapshot
// restore. Prefix stability within a compaction epoch is the invariant that
// keeps the inference engine's prefix / KV cache hitting. The sibling-Reasoning refactor
// deleted the placeholder/splice machinery these tests previously had to work
// around.
//
// These target the refactored sibling-Reasoning shape:
//   - No `__RAW_OUTPUT_PLACEHOLDER__` sentinels
//   - No `extract_raw_input_items()` / `splice_raw_input_items()`
//   - Reasoning lives as `ConversationItem::Reasoning(rs::ReasoningItem)`
//     siblings; the From<&ConversationRequest> for rs::CreateResponse impl
//     emits them inline in `input` order.
// ============================================================================

/// Serialize a ConversationRequest using only the public
/// `From<&ConversationRequest> for rs::CreateResponse` trait impl.
///
/// After the sibling-Reasoning refactor there is no placeholder/splice dance: the `Vec<rs::InputItem>`
/// produced by the From impl is directly the wire shape (modulo
/// `patch_reasoning_text_types` which only stamps a `type` field on nested
/// reasoning content blocks and does not reorder items).
fn serialize_via_public_api(req: &sampling_types::ConversationRequest) -> serde_json::Value {
    use sampling_types::rs;
    let create_response: rs::CreateResponse = req.into();
    let mut body = serde_json::to_value(&create_response).unwrap();
    sampling_types::patch_reasoning_text_types(&mut body);
    // Sanity guard: the placeholder string from the pre-refactor design
    // must never appear in the serialized output. If a future change
    // re-introduces a stringly-typed splice, this catches it.
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(
        !body_str.contains("__RAW_OUTPUT_PLACEHOLDER_"),
        "placeholder sentinel must never appear in serialized output \
         post-sibling-Reasoning refactor"
    );
    body
}

/// Assert that request N's serialized input is a byte-stable prefix of
/// request N+1's.
fn assert_prefix_stable_pair(
    base: &sampling_types::ConversationRequest,
    extended: &sampling_types::ConversationRequest,
    label: &str,
) {
    let base_body = serialize_via_public_api(base);
    let ext_body = serialize_via_public_api(extended);

    let base_input = base_body["input"].as_array().unwrap();
    let ext_input = ext_body["input"].as_array().unwrap();

    assert!(
        ext_input.len() >= base_input.len(),
        "{label}: extended request has fewer input items ({}) than base ({})",
        ext_input.len(),
        base_input.len(),
    );
    assert_eq!(
        &ext_input[..base_input.len()],
        base_input.as_slice(),
        "{label}: prefix broken. Base has {} items, extended has {}. \
         First divergence at index {}",
        base_input.len(),
        ext_input.len(),
        base_input
            .iter()
            .zip(ext_input.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(base_input.len()),
    );
}

/// Build a sibling Reasoning item with the given id and (optionally)
/// encrypted_content. Replaces the pre-refactor "AssistantItem.raw_output"
/// helper.
fn reasoning_sibling(id: &str, encrypted: Option<&str>) -> ConversationItem {
    use sampling_types::rs;
    ConversationItem::Reasoning(rs::ReasoningItem {
        id: Some(id.to_string()),
        summary: vec![rs::SummaryPart::SummaryText(rs::SummaryTextContent {
            text: format!("thinking for {id}"),
        })],
        content: None,
        encrypted_content: encrypted.map(str::to_owned),
        status: None,
    })
}

/// Basic multi-turn prefix stability through build_request().
/// Each turn adds a user message + assistant response; the serialized
/// input from turn N must be a prefix of turn N+1.
#[tokio::test]
async fn prefix_stable_across_user_assistant_turns() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("You are a helpful assistant."),
        ConversationItem::user("Hello"),
    ]);

    let req1 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    h.handle
        .push_assistant_response(ConversationItem::assistant("Hi there!"));
    h.handle
        .push_user_message(ConversationItem::user("How are you?"));

    let req2 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    assert_prefix_stable_pair(&req1, &req2, "turn 1 -> turn 2");

    h.handle
        .push_assistant_response(ConversationItem::assistant("I'm well!"));
    h.handle.push_user_message(ConversationItem::user("Great"));

    let req3 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    assert_prefix_stable_pair(&req2, &req3, "turn 2 -> turn 3");
}

/// Prefix stability when memory reminders are injected.
/// Once a memory reminder is established, subsequent requests with the
/// SAME reminder must produce stable prefixes.
#[tokio::test]
async fn prefix_stable_with_consistent_memory_injection() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("You are helpful."),
        ConversationItem::user("Hello"),
    ]);

    let req1 = h
        .handle
        .build_request(
            "test-timeline",
            vec![],
            Some("Remember: user likes Rust".to_string()),
            None,
            None,
        )
        .await
        .unwrap();

    h.handle
        .push_assistant_response(ConversationItem::assistant("Hi!"));
    h.handle
        .push_user_message(ConversationItem::user("Tell me more"));

    let req2 = h
        .handle
        .build_request(
            "test-timeline",
            vec![],
            Some("Remember: user likes Rust".to_string()),
            None,
            None,
        )
        .await
        .unwrap();

    assert_prefix_stable_pair(&req1, &req2, "memory-injected turn 1 -> turn 2");
}

/// Prefix stability with Reasoning siblings (encrypted reasoning) through
/// the full build_request pipeline. This is the structural equivalent of the
/// earlier `prefix_stable_with_raw_output_through_build_request` test --
/// it exercises the exact code path that caused a prefix-instability incident,
/// but on the post-refactor data model where reasoning rides as a typed sibling
/// rather than an `AssistantItem.raw_output` blob.
#[tokio::test]
async fn prefix_stable_with_reasoning_siblings_through_build_request() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("u1"),
    ]);

    let req1 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    // Push Reasoning sibling + Assistant (the new ordering produced by
    // `response_to_conversation_items`: Reasoning before Assistant).
    h.handle
        .push_tool_result(reasoning_sibling("r_abc", Some("enc1")));
    h.handle
        .push_assistant_response(ConversationItem::assistant("response 1"));
    h.handle.push_user_message(ConversationItem::user("u2"));

    let req2 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    assert_prefix_stable_pair(&req1, &req2, "Reasoning sibling turn 1 -> turn 2");

    // Push another turn's Reasoning + Assistant
    h.handle
        .push_tool_result(reasoning_sibling("r_def", Some("enc2")));
    h.handle
        .push_assistant_response(ConversationItem::assistant("response 2"));
    h.handle.push_user_message(ConversationItem::user("u3"));

    let req3 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    assert_prefix_stable_pair(
        &req2,
        &req3,
        "Reasoning sibling turn 2 -> turn 3 (cross-turn prefix-stability regression)",
    );
}

/// Tool definitions are in the `tools` field, not the `input` array.
/// Changing the tool set between requests must not affect the input prefix.
#[tokio::test]
async fn prefix_stable_after_tool_schema_change() {
    use sampling_types::ToolSpec;

    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("hello"),
    ]);

    let tools_v1 = vec![ToolSpec {
        name: "read_file".to_string(),
        description: Some("Read a file".to_string()),
        parameters: serde_json::json!({"type": "object"}),
    }];

    let req1 = h.handle.build_request("test-timeline", tools_v1, None, None, None).await.unwrap();

    h.handle
        .push_assistant_response(ConversationItem::assistant("read it"));
    h.handle
        .push_user_message(ConversationItem::user("now edit"));

    let tools_v2 = vec![
        ToolSpec {
            name: "read_file".to_string(),
            description: Some("Read a file".to_string()),
            parameters: serde_json::json!({"type": "object"}),
        },
        ToolSpec {
            name: "edit_file".to_string(),
            description: Some("Edit a file".to_string()),
            parameters: serde_json::json!({"type": "object"}),
        },
    ];

    let req2 = h.handle.build_request("test-timeline", tools_v2, None, None, None).await.unwrap();

    assert_prefix_stable_pair(&req1, &req2, "tool schema v1 -> v2");
}

/// Model switch between turns: model is in request metadata, not input
/// items. Input prefix must remain stable.
#[tokio::test]
async fn prefix_stable_after_model_switch() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("hello"),
    ]);

    let req1 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    h.handle
        .push_assistant_response(ConversationItem::assistant("hi"));
    h.handle
        .push_user_message(ConversationItem::user("continue"));

    let new_config = SamplingConfig {
        model: "grow-3-mini".to_string(),
        ..test_config()
    };
    h.handle.update_sampling_config(new_config);

    let req2 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    assert_prefix_stable_pair(&req1, &req2, "model switch");
}

#[tokio::test]
async fn prompt_cache_key_tracks_timeline_model_and_rewind_lineage() {
    let h = TestHarness::new();
    seed_test_system(&h.handle, "system").await;
    h.handle.push_user_message(marked_user("q1", 0));
    record_prompt(&h.handle, "q1").await;

    let first = h
        .handle
        .build_request("timeline-a", vec![], None, None, None)
        .await
        .unwrap();
    let first_key = first.prompt_cache_key.expect("normal requests need a key");

    h.handle
        .push_assistant_response(ConversationItem::assistant("a1"));
    h.handle.push_user_message(marked_user("q2", 1));
    record_prompt(&h.handle, "q2").await;
    let appended = h
        .handle
        .build_request("timeline-a", vec![], None, None, None)
        .await
        .unwrap();
    assert_eq!(
        appended.prompt_cache_key.as_deref(),
        Some(first_key.as_str()),
        "ordinary appends stay on one cache lineage"
    );

    h.handle.update_sampling_config(SamplingConfig {
        model: "other-model".into(),
        ..test_config()
    });
    let other_model = h
        .handle
        .build_request("timeline-a", vec![], None, None, None)
        .await
        .unwrap();
    assert_ne!(other_model.prompt_cache_key.as_deref(), Some(first_key.as_str()));

    h.handle.update_sampling_config(test_config());
    h.handle.rewind_durably(1).await.unwrap();
    let rewound = h
        .handle
        .build_request("timeline-a", vec![], None, None, None)
        .await
        .unwrap();
    assert_ne!(rewound.prompt_cache_key.as_deref(), Some(first_key.as_str()));

    let fork = h
        .handle
        .build_request("timeline-b", vec![], None, None, None)
        .await
        .unwrap();
    assert_ne!(fork.prompt_cache_key, rewound.prompt_cache_key);
}

/// Synthetic user messages (doom loop warning, auto-continue) are
/// appended -- not inserted before existing items -- so the prefix
/// must remain stable.
#[tokio::test]
async fn prefix_stable_with_synthetic_user_messages() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::user("hello"),
    ]);

    let req1 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    h.handle
        .push_assistant_response(ConversationItem::assistant("hi"));

    h.handle
        .push_user_message(ConversationItem::system_reminder("stop looping"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("ok"));
    h.handle
        .push_user_message(ConversationItem::auto_continue("keep going"));

    let req2 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    assert_prefix_stable_pair(&req1, &req2, "with synthetic user messages");
}

/// Prefix stability after size-gated image eviction. Once the serialized body
/// nears the 50 MB ceiling, old user turns' images are replaced with text
/// placeholders on the request clone -- text items before the evicted region
/// must stay prefix-stable in their relative ordering. The image here is sized
/// past `IMAGE_COMPACT_TRIGGER_BYTES` so the eviction actually fires.
#[tokio::test]
async fn prefix_stable_after_image_pruning() {
    use sampling_types::ContentPart;

    // Large enough that the serialized body crosses the compaction trigger.
    let big_image_url = format!(
        "data:image/png;base64,{}",
        "A".repeat(crate::actor::request_builder::IMAGE_COMPACT_TRIGGER_BYTES)
    );

    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::User(sampling_types::UserItem {
            content: vec![
                ContentPart::Text {
                    text: "look at this image".into(),
                },
                ContentPart::Image {
                    url: big_image_url.into(),
                },
            ],
            synthetic_reason: None,
            permission_evidence: None,
            ..Default::default()
        }),
        ConversationItem::assistant("I see it"),
        ConversationItem::user("u2"),
    ]);

    let req1 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    h.handle
        .push_assistant_response(ConversationItem::assistant("a2"));
    h.handle
        .push_user_message(ConversationItem::User(sampling_types::UserItem {
            content: vec![
                ContentPart::Text {
                    text: "new image".into(),
                },
                ContentPart::Image {
                    url: "data:image/png;base64,newImageData".into(),
                },
            ],
            synthetic_reason: None,
            permission_evidence: None,
            ..Default::default()
        }));

    let req2 = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    // Image stripping mutates the old user turn's content, so full
    // byte-level prefix stability cannot hold at that item. We verify:
    //   1. System prompt preserved
    //   2. Items grew
    //   3. Text items appear in the same relative order
    let body1 = serialize_via_public_api(&req1);
    let body2 = serialize_via_public_api(&req2);

    let input1 = body1["input"].as_array().unwrap();
    let input2 = body2["input"].as_array().unwrap();

    assert_eq!(
        input1[0], input2[0],
        "system prompt must be preserved after image pruning"
    );
    assert!(
        input2.len() > input1.len(),
        "extended request must have more items"
    );

    let extract_text_items = |input: &[serde_json::Value]| -> Vec<String> {
        input
            .iter()
            .filter_map(|v| {
                let role = v.get("role").and_then(|r| r.as_str())?;
                let content = v.get("content").and_then(|c| c.as_str())?;
                Some(format!("{role}:{content}"))
            })
            .collect()
    };
    let texts1 = extract_text_items(input1);
    let texts2 = extract_text_items(input2);
    let mut idx2 = 0;
    for t1 in &texts1 {
        while idx2 < texts2.len() && &texts2[idx2] != t1 {
            idx2 += 1;
        }
        assert!(
            idx2 < texts2.len(),
            "text item {t1:?} from req1 must appear in req2 in the same order"
        );
        idx2 += 1;
    }
}

/// Regression for the image cache-miss bug: with normal small images (well
/// under the 50 MB ceiling), an old user turn's image is preserved across
/// turns instead of being rewritten to a placeholder. Rewriting old images on
/// every turn busted the KV-cache prefix (the over-aggressive earlier
/// behavior this size-gate replaces).
#[tokio::test]
async fn build_request_preserves_small_old_images() {
    use sampling_types::{ContentPart, UserItem};

    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("sys"),
        ConversationItem::User(UserItem {
            content: vec![
                ContentPart::Text {
                    text: "look at this".into(),
                },
                ContentPart::Image {
                    url: "data:image/png;base64,iVBORw0KGgo=".into(),
                },
            ],
            synthetic_reason: None,
            permission_evidence: None,
            ..Default::default()
        }),
        ConversationItem::assistant("I see it"),
        ConversationItem::user("follow up question"),
    ]);

    let req = h.handle.build_request("test-timeline", vec![], None, None, None).await.unwrap();

    // The old user turn's image must survive (small payload, far under 50 MB),
    // so the KV-cache prefix stays byte-stable instead of being rewritten.
    let image_retained = req.items.iter().any(|item| {
        matches!(item, ConversationItem::User(u)
            if u.content.iter().any(|p| matches!(p, ContentPart::Image { .. })))
    });
    assert!(
        image_retained,
        "a small old image must be preserved, not stripped to a placeholder"
    );
}

// ============================================================================
// Out-of-band history repair (grow/session/repair)
// ============================================================================

/// Bricked-session shape: an orphaned tool result survives load (the eager
/// repairs only fix dangling calls) and 400s on every request. The
/// out-of-band `RepairHistory` command must strip it and persist the fix.
#[tokio::test]
async fn repair_history_command_strips_orphan_and_persists() {
    use sampling_types::ToolCall;

    let corrupted = vec![
        ConversationItem::system("sys"),
        ConversationItem::user("prompt"),
        // ← assistant owning call_LOST is missing (skipped corrupt line)
        ConversationItem::tool_result("call_LOST", "orphaned"),
        ConversationItem::assistant_tool_calls(vec![ToolCall {
            id: "call_OK".into(),
            name: "read_file".to_string(),
            arguments: "{}".into(),
        }]),
        ConversationItem::tool_result("call_OK", "fine"),
    ];
    let mut h = TestHarness::with_conversation(corrupted);

    // Load-time repairs leave the orphan in place (the bug under test).
    assert_eq!(h.handle.get_conversation().await.len(), 5);
    h.drain_persistence();

    // Dry run: reports the orphan, mutates nothing, persists nothing.
    let report = h.handle.repair_history(true, None).await.unwrap().unwrap();
    assert!(report.changed());
    assert_eq!(report.stripped_tool_result_ids, vec!["call_LOST"]);
    assert_eq!(h.handle.get_conversation().await.len(), 5);
    assert!(h.drain_persistence().is_empty(), "dry run must not persist");

    // Real repair: orphan stripped, fix persisted as one Timeline replacement.
    let report = h.handle.repair_history(false, None).await.unwrap().unwrap();
    assert!(report.changed());
    assert_eq!(report.stripped_tool_result_ids, vec!["call_LOST"]);

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 4);
    assert!(!conv.iter().any(|i| matches!(
        i,
        ConversationItem::ToolResult(tr) if tr.tool_call_id == "call_LOST"
    )));

    let records = h.drain_persistence();
    let replaced = records
        .iter()
        .filter_map(persisted_messages)
        .find(|event| event.cause == crate::MessageCause::IntegrityRepair)
        .expect("repair must persist one Timeline replacement");
    assert_eq!(replaced.items.len(), 4);

    // Idempotent: a second repair reports no changes and persists nothing.
    let report = h.handle.repair_history(false, None).await.unwrap().unwrap();
    assert!(!report.changed());
    assert!(h.drain_persistence().is_empty());
}

/// A repair command processed while the shared turn-active flag is set must
/// be refused without mutating or persisting anything (the in-actor check
/// closes the race with a turn starting after the caller's own check).
#[tokio::test]
async fn repair_history_command_refused_while_turn_active() {
    let corrupted = vec![
        ConversationItem::user("prompt"),
        ConversationItem::tool_result("call_ORPHAN", "orphaned"),
    ];
    let mut h = TestHarness::with_conversation(corrupted);
    h.drain_persistence();

    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let result = h
        .handle
        .repair_history(false, Some(flag.clone()))
        .await
        .unwrap();
    assert!(matches!(
        result,
        Err(crate::commands::RepairHistoryError::TurnActive)
    ));
    // Nothing was mutated or persisted.
    assert_eq!(h.handle.get_conversation().await.len(), 2);
    assert!(h.drain_persistence().is_empty());

    // Once the turn ends, the same call succeeds.
    flag.store(false, std::sync::atomic::Ordering::SeqCst);
    let report = h
        .handle
        .repair_history(false, Some(flag))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.stripped_tool_result_ids, vec!["call_ORPHAN"]);
}

#[tokio::test]
async fn repair_history_retries_an_uncertain_timeline_commit() {
    let corrupted = vec![
        ConversationItem::user("prompt"),
        ConversationItem::tool_result("call_ORPHAN", "orphaned"),
    ];
    let mut h = TestHarness::with_manual_timeline_ack(corrupted.clone());
    let handle = h.handle.clone();
    let repair = async move { handle.repair_history(false, None).await.unwrap() };
    let retry = fail_once_then_ack_exact_retry(&mut h.persistence_rx);
    let (result, ()) = tokio::join!(repair, retry);
    assert!(result.is_ok());
    assert_ne!(
        serde_json::to_vec(&h.handle.get_conversation().await).unwrap(),
        serde_json::to_vec(&corrupted).unwrap()
    );
    assert_eq!(
        h.drain_persistence()
            .iter()
            .filter_map(persisted_messages)
            .count(),
        2,
        "the exact durable event is retried after an uncertain failure"
    );
}

// ============================================================================
// Tool-result pruning (PruneToolResults)
// ============================================================================

/// Full happy path: a planned oversized tool result is replaced by
/// head + marker + tail, structural fields survive, projected pressure drops,
/// one Timeline replacement is persisted, and no UI
/// event is published (the pager renders streamed wire events and must not
/// be disturbed by pruning stored state).
#[tokio::test]
async fn prune_tool_results_trims_content_preserves_structure_and_persists() {
    use crate::actor::state::EstimatedItemTokenCounter;
    use compaction::plan_tool_result_pruning;

    let content = format!("{}{}{}", "H".repeat(400), "M".repeat(400), "T".repeat(400));
    let conv = vec![
        ConversationItem::user("hello"),
        ConversationItem::tool_result_with_images(
            "call-1",
            content.as_str(),
            vec![sampling_types::ContentPart::Text {
                text: "keep-me".into(),
            }],
        ),
    ];
    let mut h = TestHarness::with_conversation(conv.clone());
    h.drain_events();

    // 1200 bytes ≈ 300 tokens; a 50-token budget prunes it to 200 bytes.
    let plan = plan_tool_result_pruning(&conv, &EstimatedItemTokenCounter, 50, 20);
    assert_eq!(
        plan.items.len(),
        1,
        "only the oversized tool result is selected"
    );
    assert_eq!(plan.items[0].index, 1);

    let before = h.handle.get_projected_tokens().await;
    let report = h
        .handle
        .prune_tool_results(plan)
        .await
        .expect("prune succeeds");
    assert_eq!(report.pruned_count, 1);
    assert_eq!(report.tokens_before, before);
    assert!(
        report.tokens_after < report.tokens_before,
        "pruning must reduce projected context pressure"
    );

    let conversation = h.handle.get_conversation().await;
    assert_eq!(conversation.len(), 2, "conversation length is preserved");
    let ConversationItem::ToolResult(tr) = &conversation[1] else {
        panic!("expected tool result at index 1");
    };
    assert_eq!(
        tr.tool_call_id, "call-1",
        "structural fields survive pruning"
    );
    assert!(matches!(
        tr.images.as_slice(),
        [sampling_types::ContentPart::Text { text }] if text.as_ref() == "keep-me"
    ));
    // Head (100 bytes) + full marker (39 bytes, fits the 50-byte room) +
    // tail (50 bytes): head+tail pruning, not a prefix clip.
    assert_eq!(
        tr.content.as_ref(),
        format!(
            "{}{}{}",
            "H".repeat(100),
            super::mutations::PRUNE_MARKER,
            "T".repeat(50)
        )
        .as_str(),
        "content must be head + marker + tail"
    );

    assert_eq!(h.handle.get_projected_tokens().await, report.tokens_after);

    // Persisted as exactly one Timeline replacement.
    let records = h.drain_persistence();
    let events = records
        .iter()
        .filter_map(persisted_messages)
        .collect::<Vec<_>>();
    assert_eq!(
        events.len(),
        1,
        "expected one Timeline replacement: {records:?}"
    );
    assert_eq!(events[0].cause, crate::MessageCause::ToolResultPrune);
    let persisted = events[0]
        .items
        .iter()
        .find_map(|item| match item {
            ConversationItem::ToolResult(result) => Some(result),
            _ => None,
        })
        .expect("expected tool result in replacement event");
    assert!(persisted.content.contains(super::mutations::PRUNE_MARKER));

    // Pruning must not disturb the rendered UI: no events at all.
    assert!(
        h.drain_events().is_empty(),
        "prune must not publish UI events"
    );
}

/// Replaying the same plan never re-prunes: already-marker'd or already
/// in-budget content is skipped, and the no-op run persists nothing.
#[tokio::test]
async fn prune_tool_results_is_idempotent() {
    use crate::actor::state::EstimatedItemTokenCounter;
    use compaction::plan_tool_result_pruning;

    let conv = vec![ConversationItem::tool_result("call-1", "x".repeat(4000))];
    let mut h = TestHarness::with_conversation(conv.clone());
    let plan = plan_tool_result_pruning(&conv, &EstimatedItemTokenCounter, 50, 100);

    let first = h
        .handle
        .prune_tool_results(plan.clone())
        .await
        .expect("first prune succeeds");
    assert_eq!(first.pruned_count, 1);
    let content_after_first = match &h.handle.get_conversation().await[0] {
        ConversationItem::ToolResult(tr) => tr.content.clone(),
        other => panic!("expected tool result, got {other:?}"),
    };
    h.drain_persistence();

    let second = h
        .handle
        .prune_tool_results(plan)
        .await
        .expect("second prune succeeds");
    assert_eq!(second.pruned_count, 0, "repeat plan must not re-prune");
    assert_eq!(second.tokens_before, first.tokens_after);
    assert_eq!(second.tokens_after, first.tokens_after);
    let content_after_second = match &h.handle.get_conversation().await[0] {
        ConversationItem::ToolResult(tr) => tr.content.clone(),
        other => panic!("expected tool result, got {other:?}"),
    };
    assert_eq!(
        content_after_second, content_after_first,
        "replayed plan must not change content"
    );
    assert!(
        h.drain_persistence().is_empty(),
        "a no-op replay must not persist"
    );
}

/// The re-estimate is clamped to the pre-prune total: pruning must never
/// appear to increase usage, even when the provider-reported total is far
/// below the static byte estimate.
#[tokio::test]
async fn prune_tool_results_clamps_signed_delta_at_zero() {
    use crate::actor::state::EstimatedItemTokenCounter;
    use compaction::plan_tool_result_pruning;

    let conv = vec![ConversationItem::tool_result("call-1", "x".repeat(4000))];
    let mut h = TestHarness::with_conversation(conv.clone());
    h.handle.record_provider_context_anchor(10);
    // Sync point: the query is ordered after the fire-and-forget anchor.
    assert_eq!(h.handle.get_projected_tokens().await, 10);
    h.drain_events();

    let plan = plan_tool_result_pruning(&conv, &EstimatedItemTokenCounter, 50, 100);
    let report = h
        .handle
        .prune_tool_results(plan)
        .await
        .expect("prune succeeds");
    assert_eq!(report.pruned_count, 1, "content is still pruned");
    assert_eq!(report.tokens_before, 10);
    assert_eq!(report.tokens_after, 0, "signed deltas clamp at zero");
    assert_eq!(h.handle.get_projected_tokens().await, 0);
    assert!(h.drain_events().is_empty());
}

#[tokio::test]
async fn prune_tool_results_retries_an_uncertain_timeline_commit() {
    use compaction::{PruneItem, PrunePlan};

    let original = vec![ConversationItem::tool_result("call-1", "x".repeat(4000))];
    let mut h = TestHarness::with_manual_timeline_ack(original.clone());
    let handle = h.handle.clone();
    let prune = async move {
        handle
            .prune_tool_results(PrunePlan {
                items: vec![PruneItem {
                    index: 0,
                    tokens_before: 1000,
                    budget_tokens: 50,
                    estimated_savings: 950,
                }],
            })
            .await
    };
    let retry = fail_once_then_ack_exact_retry(&mut h.persistence_rx);
    let (result, ()) = tokio::join!(prune, retry);
    assert!(result.is_ok());
    assert_ne!(
        serde_json::to_vec(&h.handle.get_conversation().await).unwrap(),
        serde_json::to_vec(&original).unwrap(),
    );
}

/// A prune command and a concurrent `PushToolResult` serialize inside the
/// actor: whichever lands first, the other is not lost and both effects are
/// durable.
#[tokio::test]
async fn prune_tool_results_interleaves_with_push_without_losing_messages() {
    use compaction::{PruneItem, PrunePlan};

    let mut h = TestHarness::with_conversation(vec![
        ConversationItem::user("hello"),
        ConversationItem::tool_result("call-1", "H".repeat(4000).as_str()),
    ]);
    let plan = PrunePlan {
        items: vec![PruneItem {
            index: 1,
            tokens_before: 1000,
            budget_tokens: 50,
            estimated_savings: 950,
        }],
    };

    let prune_handle = h.handle.clone();
    let push_handle = h.handle.clone();
    let prune = async move { prune_handle.prune_tool_results(plan).await.unwrap() };
    let push = async move {
        tokio::task::yield_now().await;
        push_handle.push_tool_result(ConversationItem::tool_result("call-2", "fresh-result"));
    };
    let (report, ()) = tokio::join!(prune, push);

    assert_eq!(report.pruned_count, 1);
    let conversation = h.handle.get_conversation().await;
    assert_eq!(conversation.len(), 3, "concurrent push must not be lost");
    let ConversationItem::ToolResult(pruned) = &conversation[1] else {
        panic!("expected pruned tool result at index 1");
    };
    assert!(pruned.content.contains(super::mutations::PRUNE_MARKER));
    let ConversationItem::ToolResult(fresh) = &conversation[2] else {
        panic!("expected pushed tool result at index 2");
    };
    assert_eq!(fresh.tool_call_id, "call-2");
    assert_eq!(fresh.content.as_ref(), "fresh-result");

    // Both effects are represented by Timeline events (their order may vary).
    let records = h.drain_persistence();
    assert!(records.iter().filter_map(persisted_messages).any(|event| {
        event.items.iter().any(
            |item| matches!(item, ConversationItem::ToolResult(tr) if tr.tool_call_id == "call-2"),
        )
    }));
    assert!(records.iter().filter_map(persisted_messages).any(|event| {
        event.cause == crate::MessageCause::ToolResultPrune
            && event.items.iter().any(|item| {
                matches!(item, ConversationItem::ToolResult(tr)
                    if tr.content.contains(super::mutations::PRUNE_MARKER))
            })
    }));
}

/// Defensive plan entries are skipped with diagnostics instead of panicking:
/// out-of-bounds indices, indices onto non-tool items, stale `tokens_before`
/// (the actual content decides), and duplicate entries (only the first
/// prunes).
#[tokio::test]
async fn prune_tool_results_defensively_skips_bad_plan_entries() {
    use compaction::{PruneItem, PrunePlan};

    let h = TestHarness::with_conversation(vec![
        ConversationItem::user("not a tool result"),
        ConversationItem::tool_result("call-1", "x".repeat(4000).as_str()),
    ]);
    let plan = PrunePlan {
        items: vec![
            // Out of bounds: skipped, never a panic.
            PruneItem {
                index: 99,
                tokens_before: 500,
                budget_tokens: 50,
                estimated_savings: 450,
            },
            // A non-tool item: forbidden to prune, skipped.
            PruneItem {
                index: 0,
                tokens_before: 500,
                budget_tokens: 50,
                estimated_savings: 450,
            },
            // Stale token count: actual content (1000) decides, still prunes.
            PruneItem {
                index: 1,
                tokens_before: u32::MAX,
                budget_tokens: 50,
                estimated_savings: 950,
            },
            // Duplicate of the real target: marker already present, skipped.
            PruneItem {
                index: 1,
                tokens_before: 1000,
                budget_tokens: 50,
                estimated_savings: 950,
            },
        ],
    };

    let report = h
        .handle
        .prune_tool_results(plan)
        .await
        .expect("prune succeeds");
    assert_eq!(report.pruned_count, 1, "only the first valid entry prunes");
    assert!(report.tokens_after < report.tokens_before);

    let conversation = h.handle.get_conversation().await;
    assert_eq!(conversation.len(), 2);
    let ConversationItem::User(user) = &conversation[0] else {
        panic!("user item must be untouched");
    };
    assert!(matches!(
        user.content.as_slice(),
        [sampling_types::ContentPart::Text { text }] if text.as_ref() == "not a tool result"
    ));
    let ConversationItem::ToolResult(tr) = &conversation[1] else {
        panic!("expected tool result at index 1");
    };
    assert!(tr.content.contains(super::mutations::PRUNE_MARKER));
    assert_eq!(
        tr.content.matches(super::mutations::PRUNE_MARKER).count(),
        1,
        "marker must appear exactly once after a duplicate plan entry"
    );
}

/// A `budget_tokens == 0` plan entry is clamped to 1 token: the content is
/// trimmed to head + clipped marker + tail, never silently emptied.
#[tokio::test]
async fn prune_tool_results_zero_budget_clamps_instead_of_emptying() {
    use compaction::{PruneItem, PrunePlan};

    // 300 'H' + 300 'T' = 600 bytes. Clamped budget 1 token → 4 bytes:
    // head 2 + marker clipped to its 1-byte room ("\n") + tail 1.
    let content = format!("{}{}", "H".repeat(300), "T".repeat(300));
    let h = TestHarness::with_conversation(vec![ConversationItem::tool_result("call-1", content)]);
    let plan = PrunePlan {
        items: vec![PruneItem {
            index: 0,
            tokens_before: 150,
            budget_tokens: 0,
            estimated_savings: 150,
        }],
    };

    let report = h
        .handle
        .prune_tool_results(plan)
        .await
        .expect("prune succeeds");
    assert_eq!(report.pruned_count, 1);
    let conversation = h.handle.get_conversation().await;
    let ConversationItem::ToolResult(tr) = &conversation[0] else {
        panic!("expected tool result");
    };
    assert!(
        !tr.content.is_empty(),
        "a zero budget must clamp, not empty the content"
    );
    assert_eq!(tr.content.as_ref(), "HH\nT", "head + clipped marker + tail");
    assert!(report.tokens_after < report.tokens_before);
}

/// Error paths: pruning an empty conversation fails without persisting; an
/// empty plan on a non-empty conversation is a zero-prune no-op; a dead actor
/// surfaces `ActorUnavailable` instead of a silent skip.
#[tokio::test]
async fn prune_tool_results_error_paths() {
    use compaction::{PruneItem, PrunePlan};

    let plan = PrunePlan {
        items: vec![PruneItem {
            index: 0,
            tokens_before: 10,
            budget_tokens: 5,
            estimated_savings: 5,
        }],
    };

    let mut h = TestHarness::new();
    let err = h
        .handle
        .prune_tool_results(plan.clone())
        .await
        .expect_err("empty conversation must be an error");
    assert!(matches!(
        err,
        crate::commands::PruneError::EmptyConversation
    ));
    assert!(
        h.drain_persistence().is_empty(),
        "the empty-conversation error must not persist"
    );
    assert!(h.drain_events().is_empty());

    // An empty plan on a non-empty conversation prunes nothing.
    h.handle.push_user_message(ConversationItem::user("hello"));
    // Sync point: the query is ordered after the fire-and-forget push, so its
    // Message persistence record is in the channel before we drain.
    assert_eq!(h.handle.get_conversation_len().await, 1);
    h.drain_persistence();
    let report = h
        .handle
        .prune_tool_results(compaction::PrunePlan::default())
        .await
        .expect("empty plan succeeds");
    assert_eq!(report.pruned_count, 0);
    assert_eq!(report.tokens_before, report.tokens_after);
    assert!(h.drain_persistence().is_empty());

    // A dead actor (no receiver) reports delivery failure.
    let noop = crate::handle::ChatStateHandle::noop();
    let err = noop
        .prune_tool_results(plan)
        .await
        .expect_err("dead actor must be an error");
    assert!(matches!(err, crate::commands::PruneError::ActorUnavailable));
}

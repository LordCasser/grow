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

async fn seed_completed_request(h: &mut TestHarness, id: &str) {
    let turn = crate::TurnId(700);
    let step = crate::StepId { turn, index: 0 };
    let identity = crate::TurnIdentity {
        origin: "user".into(),
        turn_kind: "internal".into(),
        goal_id: None,
        goal_definition_revision: None,
        stage_id: None,
    };
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Turn(crate::TurnEvent::Started {
            prompt_id: "fixture-prompt".into(),
            id: turn,
            input_ids: Vec::new(),
            identity,
            model_id: "test-model".into(),
            input_message_count: 0,
            prompt_index: 0,
            prompt_text: "test".into(),
            input_kind: crate::TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Step(crate::StepEvent::Started {
            id: step,
        }))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Request(
            crate::RequestEvent::Started {
                id: id.into(),
                turn,
                step,
                model_id: "test-model".into(),
                input_message_count: 0,
                tool_count: 0,
            },
        ))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Request(
            crate::RequestEvent::Completed {
                id: id.into(),
                duration_ms: 1,
                time_to_first_token_ms: None,
                usage: crate::RequestUsage::default(),
                response_message_count: 1,
                attempt: 1,
                provider_terminal: None,
            },
        ))
        .await
        .unwrap();
}

async fn record_prompt(handle: &crate::handle::ChatStateHandle, text: impl Into<String>) {
    static NEXT_TURN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = crate::TurnId(NEXT_TURN.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let prompt_index = handle.get_prompt_index().await;
    handle.record_timeline_event(crate::TimelineEventKind::Turn(crate::TurnEvent::Started {
        id,
        prompt_id: format!("test-prompt-{}", id.0),
        input_ids: Vec::new(),
        identity: crate::TurnIdentity {
            origin: "user".into(),
            turn_kind: "internal".into(),
            goal_id: None,
            goal_definition_revision: None,
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
            source: crate::TurnTerminalSource::Host,
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
    assert_eq!(
        items.len(),
        1,
        "compaction installs one CompactionMeta item"
    );
    let replacement = vec![ConversationItem::user_meta(items[0].text_content())];
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = format!(
        "test-compaction-{}",
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let source_items = handle.get_conversation_len().await;
    let prompt_index = handle.get_prompt_index().await;
    let result_items = replacement.len();
    handle.record_timeline_event(crate::TimelineEventKind::Compaction(
        crate::CompactionEvent::Started {
            mode: crate::CompactionMode::Foreground,
            id: id.clone(),
            source_items,
            prompt_index,
        },
    ));
    let target = record_compaction_summary(handle, &id).await;
    handle
        .replace_compaction_range(target, replacement)
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
async fn compaction_accepts_unrelated_appends_without_hiding_late_messages() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("system"),
        ConversationItem::user("body"),
    ]);
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                mode: crate::CompactionMode::Foreground,
                id: "stale-compaction".into(),
                source_items: 2,
                prompt_index: 0,
            },
        ))
        .await
        .unwrap();
    let target = record_compaction_summary(&h.handle, "stale-compaction").await;

    h.handle
        .push_user_message_durably(ConversationItem::user("arrived during compaction"))
        .await
        .unwrap();
    h.handle
        .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
        .await
        .expect("unrelated appends do not change the frozen target");

    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Completed {
                id: "stale-compaction".into(),
                duration_ms: 1,
                source_items: 2,
                result_items: 3,
            },
        ))
        .await
        .unwrap();
    let surface = h.handle.get_conversation().await;
    assert_eq!(surface.len(), 3);
    assert_eq!(surface[0].text_content(), "system");
    assert_eq!(surface[1].text_content(), "summary");
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
                mode: crate::CompactionMode::Foreground,
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
        .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
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
    assert_eq!(
        serde_json::to_value(&branch.surface).unwrap(),
        serde_json::to_value(&after.surface).unwrap()
    );
    assert_eq!(branch.surface_ids, after.surface_ids);
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
                mode: crate::CompactionMode::Foreground,
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
        .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
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
            response_messages: Vec::new(),
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

    let committed = ack.unwrap();

    let conv = h.handle.get_conversation().await;
    assert_eq!(conv.len(), 1);

    let records = h.drain_persistence();
    assert!(
        matches!(records.as_slice(), [PersistenceRecord::Timeline(event)] if serde_json::to_value(event).unwrap() == serde_json::to_value(&committed).unwrap())
    );
    h.handle
        .push_user_message_durably(ConversationItem::user("later"))
        .await
        .unwrap();
    let snapshot = h
        .handle
        .materialize_timeline("session".into())
        .await
        .unwrap();
    assert_ne!(snapshot.input_ref.last_seq, committed.seq.get());
    assert_eq!(snapshot.surface_ids[0].event, committed.seq);
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
async fn host_completion_result_waits_for_durable_ack_and_reports_failure() {
    for fail in [false, true] {
        let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let handle = ChatStateActor::spawn(
            Vec::new(),
            test_config(),
            Box::new(mock),
            event_tx,
            tokio_util::sync::CancellationToken::new(),
        );
        let write = {
            let handle = handle.clone();
            tokio::spawn(async move {
                handle
                    .push_tool_result_durably(ConversationItem::tool_result("finish", "accepted"))
                    .await
            })
        };
        let ack = persistence_rx.next_timeline_ack().await.unwrap();
        assert!(
            !write.is_finished(),
            "completion cannot be exposed before commit acknowledgement"
        );
        ack.send(if fail {
            Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "disk full",
            ))
        } else {
            Ok(())
        })
        .unwrap();
        assert_eq!(write.await.unwrap().is_err(), fail);
        if !fail {
            let history = handle.get_conversation().await;
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].text_content(), "accepted");
            assert!(handle.get_projected_tokens().await > 0);
        }
    }
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

fn malformed_response() -> Vec<ConversationItem> {
    vec![ConversationItem::assistant_tool_calls(vec![
        sampling_types::ToolCall {
            id: "healthy".into(),
            name: "run".into(),
            arguments: "{}".into(),
        },
        sampling_types::ToolCall {
            id: "".into(),
            name: "".into(),
            arguments: "{\"limit\":50}".into(),
        },
    ])]
}

#[tokio::test]
async fn response_repair_retains_raw_fact_and_allows_the_next_request() {
    let mut h = TestHarness::new();
    h.handle.begin_turn_capture();
    let raw = malformed_response();
    let native = native_fragment_for(&test_config().api_backend);
    assert_eq!(
        h.handle
            .push_response_durably(raw.clone(), Some(native))
            .await
            .unwrap(),
        1
    );
    let repaired_request = h
        .handle
        .build_request("after-quarantine", vec![], None, None, None)
        .await
        .unwrap();
    assert!(
        !repaired_request.has_native_continuation(),
        "quarantined responses must never install provider continuation"
    );
    let records = h.drain_persistence();
    assert_eq!(records.len(), 2);
    let source = persisted_messages(&records[0]).unwrap();
    assert_eq!(source.cause, crate::MessageCause::Assistant);
    assert_eq!(
        serde_json::to_value(&source.items).unwrap(),
        serde_json::to_value(raw).unwrap()
    );
    let repair = persisted_messages(&records[1]).unwrap();
    assert_eq!(repair.cause, crate::MessageCause::IntegrityRepair);
    assert!(matches!(repair.surface, crate::SurfaceOp::Replace { .. }));
    let capture = h.handle.take_turn_messages().await.unwrap();
    assert!(!capture.compaction_occurred);
    assert_eq!(capture.messages.len(), 1);
    assert!(matches!(&capture.messages[0], ConversationItem::User(_)));

    let events = h.handle.timeline_events().await.unwrap();
    let mut replayed = crate::Timeline::from_events(events).unwrap();
    assert!(replayed.recover_surface_integrity().unwrap().is_empty());
    assert_eq!(
        serde_json::to_value(replayed.branch_transcript()).unwrap(),
        serde_json::to_value(replayed.surface()).unwrap()
    );
    assert!(
        replayed
            .completed_compaction_unloaded_branch_ids()
            .is_empty()
    );
    assert_eq!(
        h.handle
            .push_response_durably(vec![ConversationItem::assistant("next response")], None,)
            .await
            .unwrap(),
        0
    );
    let request = h
        .handle
        .build_request("test", vec![], None, None, None)
        .await
        .unwrap();
    assert!(
        request.items.iter().all(
            |item| !matches!(item, ConversationItem::Assistant(a) if !a.tool_calls.is_empty())
        )
    );
    assert!(!h.handle.is_closed());
}

#[tokio::test]
async fn response_admission_reconciles_after_caller_reply_is_lost() {
    let mut h = TestHarness::with_manual_timeline_ack_after(vec![], 4);
    seed_completed_request(&mut h, "req-reconcile").await;
    let items = vec![ConversationItem::assistant("answer")];
    let identity = crate::ResponseAdmissionIdentity {
        request_id: "req-reconcile".into(),
        attempt: 1,
    };
    let task = tokio::spawn({
        let handle = h.handle.clone();
        let items = items.clone();
        let identity = identity.clone();
        async move {
            handle
                .push_response_durably_with_identity(items, identity, None)
                .await
        }
    });
    let raw_ack = h.persistence_rx.next_timeline_ack().await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    raw_ack.send(Ok(())).unwrap();
    let conversation = h.handle.get_conversation().await;
    assert_eq!(
        serde_json::to_value(&conversation[conversation.len() - items.len()..]).unwrap(),
        serde_json::to_value(&items).unwrap()
    );
    let admitted = h.handle.admitted_response(identity.clone()).await.unwrap();
    assert_eq!(admitted.identity, identity);
    assert_eq!(
        serde_json::to_value(&admitted.items).unwrap(),
        serde_json::to_value(&items).unwrap()
    );

    assert_eq!(
        h.handle
            .push_response_durably_with_identity(items.clone(), identity, None)
            .await
            .unwrap(),
        0
    );
    let records = h.drain_persistence();
    assert_eq!(
        records
            .iter()
            .filter(|record| persisted_messages(record).is_some())
            .count(),
        1
    );
}

#[tokio::test]
async fn response_admission_conflict_preserves_timeline_and_surface() {
    let mut h = TestHarness::with_manual_timeline_ack_after(vec![], 4);
    seed_completed_request(&mut h, "req-conflict").await;
    let identity = crate::ResponseAdmissionIdentity {
        request_id: "req-conflict".into(),
        attempt: 1,
    };
    let first = tokio::spawn({
        let handle = h.handle.clone();
        let identity = identity.clone();
        async move {
            handle
                .push_response_durably_with_identity(
                    vec![ConversationItem::assistant("first")],
                    identity,
                    None,
                )
                .await
        }
    });
    h.persistence_rx
        .next_timeline_ack()
        .await
        .unwrap()
        .send(Ok(()))
        .unwrap();
    first.await.unwrap().unwrap();
    let before = h.handle.timeline_events().await.unwrap();
    let before_surface = h.handle.get_conversation().await;
    assert!(matches!(
        h.handle
            .push_response_durably_with_identity(
                vec![ConversationItem::assistant("different")],
                identity,
                None,
            )
            .await,
        Err(crate::TimelineWriteError::ResponseAdmissionConflict)
    ));
    assert_eq!(
        serde_json::to_value(h.handle.get_conversation().await).unwrap(),
        serde_json::to_value(before_surface).unwrap()
    );
    assert_eq!(
        serde_json::to_value(h.handle.timeline_events().await.unwrap()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
}

#[tokio::test]
async fn malformed_response_repair_completes_before_reconciled_confirmation() {
    let mut h = TestHarness::with_manual_timeline_ack_after(vec![], 4);
    seed_completed_request(&mut h, "req-malformed").await;
    let items = malformed_response();
    let identity = crate::ResponseAdmissionIdentity {
        request_id: "req-malformed".into(),
        attempt: 1,
    };
    let task = tokio::spawn({
        let handle = h.handle.clone();
        let items = items.clone();
        let identity = identity.clone();
        async move {
            handle
                .push_response_durably_with_identity(items, identity, None)
                .await
        }
    });
    let raw_ack = h.persistence_rx.next_timeline_ack().await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    raw_ack.send(Ok(())).unwrap();
    let repair_ack = h.persistence_rx.next_timeline_ack().await.unwrap();
    let retry = {
        let handle = h.handle.clone();
        let items = items.clone();
        let identity = identity.clone();
        tokio::spawn(async move {
            handle
                .push_response_durably_with_identity(items, identity, None)
                .await
        })
    };
    tokio::task::yield_now().await;
    assert!(!retry.is_finished());
    repair_ack.send(Ok(())).unwrap();
    assert_eq!(retry.await.unwrap().unwrap(), 1);
    let records = h.drain_persistence();
    assert_eq!(
        records
            .iter()
            .filter(|r| persisted_messages(r).is_some())
            .count(),
        2
    );
    assert_eq!(
        records
            .iter()
            .filter_map(persisted_messages)
            .filter(|message| message.cause == crate::MessageCause::Assistant)
            .count(),
        1
    );
}

#[tokio::test]
async fn repaired_history_stays_non_executable_across_all_backend_switches() {
    use sampling_types::ApiBackend;
    let backends = [
        ApiBackend::ChatCompletions,
        ApiBackend::Responses,
        ApiBackend::Messages,
    ];
    for source in &backends {
        for target in &backends {
            if source == target {
                continue;
            }
            let h = TestHarness::new();
            h.handle.replace_sampling_route(SamplingConfig {
                api_backend: source.clone(),
                ..test_config()
            });
            h.handle
                .push_response_durably(malformed_response(), None)
                .await
                .unwrap();
            h.handle.replace_sampling_route(SamplingConfig {
                api_backend: target.clone(),
                ..test_config()
            });
            let request = h
                .handle
                .build_request("switch-after-repair", vec![], None, None, None)
                .await
                .unwrap();
            let wire = match target {
                ApiBackend::ChatCompletions => {
                    serde_json::to_value(sampling_types::ChatCompletionRequest::from(request))
                        .unwrap()
                }
                ApiBackend::Responses => {
                    serde_json::to_value(sampling_types::rs::CreateResponse::from(&request))
                        .unwrap()
                }
                ApiBackend::Messages => {
                    serde_json::to_value(sampling_types::build_messages_request(&request)).unwrap()
                }
            };
            fn assert_no_executable_tools(value: &serde_json::Value) {
                match value {
                    serde_json::Value::Object(map) => {
                        assert!(
                            map.get("tool_calls")
                                .is_none_or(|calls| calls.as_array().is_some_and(Vec::is_empty))
                        );
                        assert!(!map.contains_key("tool_call_id"));
                        assert!(!matches!(
                            map.get("type").and_then(serde_json::Value::as_str),
                            Some(
                                "function_call"
                                    | "function_call_output"
                                    | "tool_use"
                                    | "tool_result"
                            )
                        ));
                        for child in map.values() {
                            assert_no_executable_tools(child);
                        }
                    }
                    serde_json::Value::Array(values) => {
                        for child in values {
                            assert_no_executable_tools(child);
                        }
                    }
                    _ => {}
                }
            }
            assert_no_executable_tools(&wire);
            assert!(wire.to_string().contains("untrusted historical evidence"));
        }
    }
}

#[tokio::test]
async fn response_repair_waits_for_both_durable_acknowledgements() {
    let mut h = TestHarness::with_manual_timeline_ack(vec![]);
    let handle = h.handle.clone();
    let task = tokio::spawn(async move {
        handle
            .push_response_durably(malformed_response(), None)
            .await
    });
    let raw_ack = h.persistence_rx.next_timeline_ack().await.unwrap();
    assert!(!task.is_finished());
    raw_ack.send(Ok(())).unwrap();
    let repair_ack = h.persistence_rx.next_timeline_ack().await.unwrap();
    assert!(!task.is_finished());
    repair_ack
        .send(Err(std::io::Error::other("transient repair write failure")))
        .unwrap();
    let retry = h.persistence_rx.next_timeline_ack().await.unwrap();
    assert!(!task.is_finished());
    retry.send(Ok(())).unwrap();
    assert_eq!(task.await.unwrap().unwrap(), 1);
    let records = h.drain_persistence();
    assert_eq!(records.len(), 3);
    assert_eq!(
        serde_json::to_value(persisted_messages(&records[1]).unwrap()).unwrap(),
        serde_json::to_value(persisted_messages(&records[2]).unwrap()).unwrap()
    );
}

#[tokio::test]
async fn failed_response_repair_can_recover_from_only_the_durable_raw_prefix() {
    let mut h = TestHarness::with_manual_timeline_ack(vec![]);
    let handle = h.handle.clone();
    let task = tokio::spawn(async move {
        handle
            .push_response_durably(malformed_response(), None)
            .await
    });
    h.persistence_rx
        .next_timeline_ack()
        .await
        .unwrap()
        .send(Ok(()))
        .unwrap();
    h.persistence_rx
        .next_timeline_ack()
        .await
        .unwrap()
        .send(Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "repair writer failed",
        )))
        .unwrap();
    assert!(
        task.await.unwrap().is_err(),
        "failed repair must not authorize dispatch"
    );
    let records = h.drain_persistence();
    let PersistenceRecord::Timeline(raw) = &records[0] else {
        panic!("missing raw response")
    };
    let mut replayed = crate::Timeline::from_events(vec![raw.clone()]).unwrap();
    assert_eq!(replayed.recover_surface_integrity().unwrap().len(), 2);
    assert!(replayed.recover_surface_integrity().unwrap().is_empty());
    assert!(matches!(replayed.surface(), [ConversationItem::User(_)]));
    assert_eq!(
        serde_json::to_value(&replayed.events()[0]).unwrap(),
        serde_json::to_value(raw).unwrap()
    );
}

#[tokio::test]
async fn response_admission_does_not_close_healthy_pending_calls() {
    let h = TestHarness::new();
    let items = vec![ConversationItem::assistant_tool_calls(vec![
        sampling_types::ToolCall {
            id: "call".into(),
            name: "not_registered".into(),
            arguments: "{invalid".into(),
        },
    ])];
    assert_eq!(
        h.handle
            .push_response_durably(items.clone(), None)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        serde_json::to_value(h.handle.get_conversation().await).unwrap(),
        serde_json::to_value(items).unwrap()
    );
    assert_eq!(h.handle.timeline_events().await.unwrap().len(), 1);
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
        cache_read_known: true,
        cache_creation_prompt_tokens: 0,
        cache_write_known: true,
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
        cache_read_known: true,
        cache_creation_prompt_tokens: 0,
        cache_write_known: true,
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
async fn session_usage_events_replace_totals_and_include_late_children() {
    let mut h = TestHarness::new();
    let call = sampling_types::TokenUsage {
        prompt_tokens: 100,
        completion_tokens: 20,
        cached_prompt_tokens: 75,
        cache_read_known: true,
        ..Default::default()
    };
    h.handle
        .record_model_call_usage(Some("parent".into()), call, None, None);
    let ChatStateEvent::SessionUsageUpdated { usage } = h.next_event().await else {
        panic!("usage mutation must publish its cumulative snapshot");
    };
    assert_eq!(usage.totals.total_tokens(), 120);
    assert_eq!(usage.totals.cached_read_tokens, 75);
    assert!(!usage.incomplete);

    assert!(matches!(
        h.handle
            .record_subagent_usage(
                "child-1".into(),
                vec![(
                    "child".into(),
                    crate::usage::UsageTotals {
                        input_tokens: 900,
                        output_tokens: 80,
                        cached_read_tokens: 425,
                        model_calls: 1,
                        ..Default::default()
                    }
                )],
                false,
                false
            )
            .await,
        Ok(true)
    ));
    let ChatStateEvent::SessionUsageUpdated { usage } = h.next_event().await else {
        panic!("late child usage must publish even without prompt attribution");
    };
    assert_eq!(usage.totals.total_tokens(), 1100);
    assert_eq!(usage.totals.cached_read_tokens, 500);
    assert_eq!(usage.totals.input_tokens, 1000);
    assert_eq!(usage.by_model.len(), 2);
    assert_eq!(
        usage.by_agent[&crate::UsageAgent::Owner].total_tokens(),
        120
    );
    assert_eq!(
        usage.by_agent[&crate::UsageAgent::Subagent("child-1".into())].total_tokens(),
        980
    );
    assert_eq!(
        h.handle
            .try_get_prompt_usage()
            .await
            .unwrap()
            .unwrap()
            .totals
            .total_tokens(),
        120
    );

    assert!(h.handle.mark_usage_incomplete(false, true).await);
    let ChatStateEvent::SessionUsageUpdated { usage } = h.next_event().await else {
        panic!("incomplete session usage must be visible");
    };
    assert!(usage.incomplete);
    assert_eq!(usage.totals.total_tokens(), 1100);
    assert!(h.handle.mark_usage_incomplete(false, true).await);
    assert!(
        h.drain_events().is_empty(),
        "unchanged incomplete state must not emit again"
    );
    let usage_facts = h
        .drain_persistence()
        .into_iter()
        .filter(|record| matches!(record, PersistenceRecord::Timeline(_)))
        .count();
    assert_eq!(
        usage_facts, 2,
        "child settlement and incomplete are durable"
    );
    let fresh = TestHarness::new();
    assert_eq!(
        fresh
            .handle
            .try_get_session_usage()
            .await
            .unwrap()
            .totals
            .total_tokens(),
        0
    );
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
        cache_read_known: true,
        cache_creation_prompt_tokens: 0,
        cache_write_known: true,
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
async fn auxiliary_attempt_usage_is_durable_idempotent_and_separate_from_rounds() {
    let h = TestHarness::new();
    record_prompt(&h.handle, "auxiliary").await;
    let sideband_id = uuid::Uuid::now_v7().to_string();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Sideband(
            crate::SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose: crate::SidebandPurpose::PermissionJudgment,
                source_refs: Vec::new(),
            },
        ))
        .await
        .unwrap();
    let usage = sampling_types::TokenUsage {
        prompt_tokens: 80,
        completion_tokens: 9,
        total_tokens: 89,
        reasoning_tokens: 3,
        cached_prompt_tokens: 20,
        cache_read_known: true,
        ..Default::default()
    };
    assert!(
        h.handle
            .begin_auxiliary_attempt_usage(sideband_id.clone(), 1, "aux-model".into(), Some(0))
            .await
            .unwrap()
    );
    assert!(
        !h.handle
            .begin_auxiliary_attempt_usage(sideband_id.clone(), 1, "aux-model".into(), Some(0))
            .await
            .unwrap()
    );
    assert!(matches!(
        h.handle
            .begin_auxiliary_attempt_usage(sideband_id.clone(), 1, "other".into(), Some(0))
            .await,
        Err(crate::TimelineWriteError::AttemptUsageConflict)
    ));
    assert!(
        h.handle
            .settle_auxiliary_attempt_usage(
                sideband_id.clone(),
                1,
                Some(usage.clone()),
                Some(11),
                Some(7)
            )
            .await
            .unwrap()
    );
    assert!(
        !h.handle
            .settle_auxiliary_attempt_usage(sideband_id.clone(), 1, Some(usage), Some(11), Some(7))
            .await
            .unwrap()
    );
    assert!(matches!(
        h.handle
            .settle_auxiliary_attempt_usage(sideband_id.clone(), 1, None, None, None)
            .await,
        Err(crate::TimelineWriteError::AttemptUsageConflict)
    ));
    let session = h.handle.try_get_session_usage().await.unwrap();
    let prompt = h.handle.try_get_prompt_usage().await.unwrap().unwrap();
    assert_eq!(session.totals.total_tokens(), 89);
    assert_eq!(session.totals.model_calls, 1);
    assert_eq!(session.main_loop_model_calls, 0);
    assert_eq!(
        session.by_agent[&crate::UsageAgent::Owner].total_tokens(),
        89
    );
    assert_eq!(prompt.totals.total_tokens(), 89);

    let events = h.handle.timeline_events().await.unwrap();
    let (persistence, _) = MockTimelinePersistence::new();
    let (event_tx, _) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        events,
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let recovered = restored.try_get_session_usage().await.unwrap();
    assert_eq!(recovered.totals, session.totals);
    assert_eq!(recovered.main_loop_model_calls, 0);

    let mut timeline =
        crate::Timeline::from_events(h.handle.timeline_events().await.unwrap()).unwrap();
    let settlement = timeline
        .events()
        .iter()
        .find_map(|event| match &event.kind {
            crate::TimelineEventKind::Observation(observation)
                if observation.name == "aux_attempt_settled" =>
            {
                Some(observation.clone())
            }
            _ => None,
        })
        .unwrap();
    timeline
        .record(crate::TimelineEventKind::Observation(settlement.clone()))
        .unwrap();
    let (persistence, _) = MockTimelinePersistence::new();
    let (event_tx, _) = mpsc::unbounded_channel();
    let duplicate = ChatStateActor::spawn_from_timeline(
        timeline.events().to_vec(),
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        duplicate.try_get_session_usage().await.unwrap().totals,
        session.totals
    );
    let mut conflict = settlement;
    conflict.data.as_mut().unwrap()["api_duration_ms"] = serde_json::json!(99);
    timeline
        .record(crate::TimelineEventKind::Observation(conflict))
        .unwrap();
    assert!(matches!(
        rejected_usage_restore(timeline.events().to_vec()).await,
        crate::TimelineWriteError::AttemptUsageConflict
    ));
}

#[tokio::test]
async fn auxiliary_retries_bill_once_each_without_reassigning_a_late_prompt() {
    let h = TestHarness::new();
    record_prompt(&h.handle, "origin").await;
    let sideband_id = uuid::Uuid::now_v7().to_string();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Sideband(
            crate::SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose: crate::SidebandPurpose::PermissionJudgment,
                source_refs: Vec::new(),
            },
        ))
        .await
        .unwrap();
    h.handle
        .begin_auxiliary_attempt_usage(sideband_id.clone(), 1, "aux-model".into(), Some(0))
        .await
        .unwrap();
    record_prompt(&h.handle, "next").await;
    h.handle
        .settle_auxiliary_attempt_usage(
            sideband_id.clone(),
            1,
            Some(sampling_types::TokenUsage {
                prompt_tokens: 8,
                completion_tokens: 2,
                total_tokens: 10,
                ..Default::default()
            }),
            None,
            None,
        )
        .await
        .unwrap();
    assert!(h.handle.try_get_prompt_usage().await.unwrap().is_none());
    h.handle
        .begin_auxiliary_attempt_usage(sideband_id.clone(), 2, "aux-model".into(), Some(1))
        .await
        .unwrap();
    h.handle
        .settle_auxiliary_attempt_usage(
            sideband_id,
            2,
            Some(sampling_types::TokenUsage {
                prompt_tokens: 4,
                completion_tokens: 1,
                total_tokens: 5,
                ..Default::default()
            }),
            None,
            None,
        )
        .await
        .unwrap();
    let session = h.handle.try_get_session_usage().await.unwrap();
    assert_eq!(session.totals.total_tokens(), 15);
    assert_eq!(session.totals.model_calls, 2);
    assert_eq!(session.main_loop_model_calls, 0);
    assert_eq!(
        h.handle
            .try_get_prompt_usage()
            .await
            .unwrap()
            .unwrap()
            .totals
            .total_tokens(),
        5
    );
}

#[tokio::test]
async fn open_auxiliary_attempt_marks_origin_segment_incomplete_on_restore() {
    let h = TestHarness::new();
    let sideband_id = uuid::Uuid::now_v7().to_string();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Sideband(
            crate::SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose: crate::SidebandPurpose::PermissionJudgment,
                source_refs: Vec::new(),
            },
        ))
        .await
        .unwrap();
    h.handle
        .begin_auxiliary_attempt_usage(sideband_id.clone(), 1, "aux-model".into(), None)
        .await
        .unwrap();
    h.handle.begin_usage_resume_segment().await.unwrap();
    let events = h.handle.timeline_events().await.unwrap();
    let (persistence, _) = MockTimelinePersistence::new();
    let (event_tx, _) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        events,
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let recovered = restored.try_get_session_usage().await.unwrap();
    assert!(recovered.incomplete);
    assert!(recovered.segments[0].incomplete);
    assert!(!recovered.segments[1].incomplete);
    assert_eq!(recovered.totals.model_calls, 0);
}

#[tokio::test]
async fn model_attempt_usage_settlement_is_durable_and_idempotent() {
    let mut h = TestHarness::new();
    record_prompt(&h.handle, "settle").await;
    h.drain_events();
    let usage = sampling_types::TokenUsage {
        prompt_tokens: 100,
        completion_tokens: 20,
        total_tokens: 120,
        ..Default::default()
    };

    assert!(matches!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-1".into(),
                0,
                "model-a".into(),
                Some(usage.clone()),
                Some(7),
                None
            )
            .await,
        Ok(true)
    ));
    let prompt = h.handle.try_get_prompt_usage().await.unwrap().unwrap();
    let session = h.handle.try_get_session_usage().await.unwrap();
    assert_eq!(prompt.totals.model_calls, 1);
    assert_eq!(session.totals.model_calls, 1);
    assert_eq!(
        h.handle
            .get_last_turn_usage()
            .await
            .unwrap()
            .completion_tokens,
        20
    );

    assert!(matches!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-1".into(),
                0,
                "model-a".into(),
                Some(usage.clone()),
                Some(7),
                None
            )
            .await,
        Ok(false)
    ));
    assert_eq!(
        h.handle
            .try_get_session_usage()
            .await
            .unwrap()
            .totals
            .model_calls,
        1
    );
    assert!(matches!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-1".into(),
                0,
                "model-b".into(),
                Some(usage),
                Some(7),
                None
            )
            .await,
        Err(crate::TimelineWriteError::AttemptUsageConflict)
    ));
    assert!(h
        .handle
        .timeline_events()
        .await
        .unwrap()
        .iter()
        .any(|event| matches!(
            &event.kind,
            crate::TimelineEventKind::Observation(observation)
                if observation.scope == "sampling_usage" && observation.name == "attempt_settled"
        )));
}

#[tokio::test]
async fn model_attempt_usage_settlement_preserves_api_duration_and_matches_it() {
    let mut h = TestHarness::new();
    record_prompt(&h.handle, "duration").await;
    h.drain_events();
    let usage = sampling_types::TokenUsage {
        prompt_tokens: 3,
        completion_tokens: 2,
        total_tokens: 5,
        ..Default::default()
    };

    assert!(matches!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-duration".into(),
                0,
                "model-a".into(),
                Some(usage.clone()),
                None,
                Some(42),
            )
            .await,
        Ok(true)
    ));
    assert_eq!(
        h.handle
            .try_get_session_usage()
            .await
            .unwrap()
            .totals
            .api_duration_ms,
        42
    );
    let event = h
        .handle
        .timeline_events()
        .await
        .unwrap()
        .into_iter()
        .find(|event| {
            matches!(
                &event.kind,
                crate::TimelineEventKind::Observation(observation)
                    if observation.scope == "sampling_usage"
                        && observation.name == "attempt_settled"
            )
        })
        .expect("settlement observation");
    let crate::TimelineEventKind::Observation(observation) = event.kind else {
        unreachable!()
    };
    assert_eq!(observation.data.unwrap()["api_duration_ms"], 42);

    assert!(matches!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-duration".into(),
                0,
                "model-a".into(),
                Some(usage.clone()),
                None,
                Some(42),
            )
            .await,
        Ok(false)
    ));
    assert!(matches!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-duration".into(),
                0,
                "model-a".into(),
                Some(usage),
                None,
                Some(43),
            )
            .await,
        Err(crate::TimelineWriteError::AttemptUsageConflict)
    ));
}

#[tokio::test]
async fn unknown_model_attempt_usage_marks_current_ledgers_incomplete() {
    let mut h = TestHarness::new();
    record_prompt(&h.handle, "unknown").await;
    h.drain_events();
    assert!(matches!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-unknown".into(),
                0,
                "model-a".into(),
                None,
                None,
                None,
            )
            .await,
        Ok(true)
    ));
    assert!(
        h.handle
            .try_get_prompt_usage()
            .await
            .unwrap()
            .unwrap()
            .incomplete
    );
    assert!(h.handle.try_get_session_usage().await.unwrap().incomplete);
}

#[tokio::test]
async fn known_cache_samples_survive_unknown_attempt_and_cold_restore() {
    let h = TestHarness::new();
    let first = sampling_types::TokenUsage {
        prompt_tokens: 100,
        total_tokens: 100,
        cached_prompt_tokens: 80,
        cache_read_known: true,
        ..Default::default()
    };
    assert!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-a".into(),
                0,
                "model-a".into(),
                Some(first.clone()),
                None,
                None,
            )
            .await
            .unwrap()
    );
    assert!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-interrupted".into(),
                0,
                "model-a".into(),
                None,
                None,
                None,
            )
            .await
            .unwrap()
    );
    assert!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-interrupted-2".into(),
                0,
                "model-a".into(),
                None,
                None,
                None,
            )
            .await
            .unwrap()
    );
    let second = sampling_types::TokenUsage {
        prompt_tokens: 900,
        total_tokens: 900,
        cached_prompt_tokens: 810,
        cache_read_known: true,
        ..Default::default()
    };
    assert!(
        h.handle
            .settle_model_attempt_usage(
                "attempt-b".into(),
                0,
                "model-a".into(),
                Some(second),
                None,
                None,
            )
            .await
            .unwrap()
    );
    assert!(
        !h.handle
            .settle_model_attempt_usage(
                "attempt-a".into(),
                0,
                "model-a".into(),
                Some(first),
                None,
                None,
            )
            .await
            .unwrap()
    );

    let current = h.handle.try_get_session_usage().await.unwrap();
    assert!(current.incomplete);
    assert_eq!(current.totals.input_tokens, 1_000);
    assert_eq!(current.totals.cached_read_tokens, 890);
    assert_eq!(current.totals.cache_read_known_input_tokens, 1_000);
    assert_eq!(current.totals.model_calls, 2);

    let events = h.handle.timeline_events().await.unwrap();
    let (persistence, _records) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        events,
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let lifetime = restored.try_get_session_usage().await.unwrap();
    assert_eq!(lifetime.totals, current.totals);
    assert!(lifetime.incomplete);
    assert!(
        !restored
            .settle_model_attempt_usage(
                "attempt-interrupted".into(),
                0,
                "model-a".into(),
                None,
                None,
                None,
            )
            .await
            .unwrap()
    );
    assert_eq!(
        restored.try_get_session_usage().await.unwrap().totals,
        current.totals
    );
}

#[tokio::test]
async fn model_attempt_usage_waits_for_persistence_ack_before_folding() {
    let mut h = TestHarness::with_manual_timeline_ack(vec![]);
    let handle = h.handle.clone();
    let mut settle = tokio::spawn(async move {
        handle
            .settle_model_attempt_usage(
                "attempt-ack".into(),
                0,
                "model-a".into(),
                Some(sampling_types::TokenUsage {
                    prompt_tokens: 1,
                    completion_tokens: 2,
                    total_tokens: 3,
                    ..Default::default()
                }),
                None,
                None,
            )
            .await
    });
    let ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("settlement persistence request");
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut settle)
            .await
            .is_err()
    );
    ack.send(Ok(())).expect("actor still waiting for ACK");
    assert!(matches!(settle.await.unwrap(), Ok(true)));
    assert_eq!(
        h.handle
            .try_get_session_usage()
            .await
            .unwrap()
            .totals
            .model_calls,
        1
    );
}

#[tokio::test]
async fn model_attempt_usage_retries_exact_event_and_restores_dedup_index() {
    let mut h = TestHarness::with_manual_timeline_ack(vec![]);
    let usage = sampling_types::TokenUsage {
        prompt_tokens: 11,
        completion_tokens: 7,
        total_tokens: 18,
        ..Default::default()
    };
    let handle = h.handle.clone();
    let settle_usage = usage.clone();
    let settle = async move {
        handle
            .settle_model_attempt_usage(
                "attempt-retry".into(),
                0,
                "model-a".into(),
                Some(settle_usage),
                Some(9),
                Some(17),
            )
            .await
    };
    let retry = fail_once_then_ack_exact_retry(&mut h.persistence_rx);
    let (result, ()) = tokio::join!(settle, retry);
    assert!(matches!(result, Ok(true)));

    let session = h.handle.try_get_session_usage().await.unwrap();
    assert_eq!(session.totals.model_calls, 1);
    assert_eq!(session.totals.api_duration_ms, 17);

    let records = h.drain_persistence();
    let persisted = records
        .iter()
        .filter_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(persisted.len(), 2, "failed write must retry the same event");
    assert_eq!(
        serde_json::to_value(persisted[0]).unwrap(),
        serde_json::to_value(persisted[1]).unwrap(),
    );

    let timeline = h.handle.timeline_events().await.unwrap();
    let (mock, _persistence_rx) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        timeline,
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(matches!(
        restored
            .settle_model_attempt_usage(
                "attempt-retry".into(),
                0,
                "model-a".into(),
                Some(usage.clone()),
                Some(9),
                Some(17),
            )
            .await,
        Ok(false)
    ));
    let mut availability_conflict = usage;
    availability_conflict.cache_read_known = true;
    assert!(matches!(
        restored
            .settle_model_attempt_usage(
                "attempt-retry".into(),
                0,
                "model-a".into(),
                Some(availability_conflict),
                Some(9),
                Some(17),
            )
            .await,
        Err(crate::TimelineWriteError::AttemptUsageConflict)
    ));
    assert_eq!(
        restored
            .try_get_session_usage()
            .await
            .unwrap()
            .totals
            .model_calls,
        1,
        "restored settlement must rebuild lifetime usage exactly once"
    );
}

#[tokio::test]
async fn restored_model_attempt_usage_is_deduplicated_and_late_prompt_is_session_only() {
    let mut h = TestHarness::new();
    record_prompt(&h.handle, "first").await;
    h.drain_events();
    let usage = sampling_types::TokenUsage {
        prompt_tokens: 10,
        completion_tokens: 4,
        total_tokens: 14,
        ..Default::default()
    };
    h.handle
        .settle_model_attempt_usage(
            "attempt-restore".into(),
            0,
            "model-a".into(),
            Some(usage.clone()),
            None,
            None,
        )
        .await
        .unwrap();
    let events = h.handle.timeline_events().await.unwrap();

    let (persistence, _records) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let token = tokio_util::sync::CancellationToken::new();
    let restored = ChatStateActor::spawn_from_timeline(
        events,
        test_config(),
        Box::new(persistence),
        event_tx,
        token,
    )
    .await
    .unwrap();
    assert!(matches!(
        restored
            .settle_model_attempt_usage(
                "attempt-restore".into(),
                0,
                "model-a".into(),
                Some(usage),
                None,
                None
            )
            .await,
        Ok(false)
    ));

    record_prompt(&restored, "second").await;
    assert!(matches!(
        restored
            .settle_model_attempt_usage(
                "attempt-late".into(),
                0,
                "model-a".into(),
                Some(sampling_types::TokenUsage {
                    prompt_tokens: 5,
                    completion_tokens: 2,
                    total_tokens: 7,
                    ..Default::default()
                }),
                None,
                None,
            )
            .await,
        Ok(true)
    ));
    assert!(restored.try_get_prompt_usage().await.unwrap().is_none());
    // The historical fact is restored into the lifetime ledger and the late
    // new attempt is added exactly once.
    assert_eq!(
        restored
            .try_get_session_usage()
            .await
            .unwrap()
            .totals
            .model_calls,
        2
    );
}

#[tokio::test]
async fn lifetime_usage_restores_children_incomplete_and_resume_segments() {
    let h = TestHarness::new();
    let first = sampling_types::TokenUsage {
        prompt_tokens: 10,
        completion_tokens: 2,
        total_tokens: 12,
        cached_prompt_tokens: 8,
        cache_read_known: true,
        ..Default::default()
    };
    h.handle
        .settle_model_attempt_usage(
            "attempt-initial".into(),
            0,
            "model-a".into(),
            Some(first),
            Some(5),
            Some(7),
        )
        .await
        .unwrap();
    let child_bill = vec![(
        "model-b".into(),
        crate::UsageTotals {
            input_tokens: 20,
            output_tokens: 3,
            cached_read_tokens: 4,
            cache_read_known_input_tokens: 20,
            cache_write_unknown_calls: 1,
            model_calls: 1,
            cost_usd_ticks: Some(6),
            ..Default::default()
        },
    )];
    assert!(matches!(
        h.handle
            .record_subagent_usage("child-1".into(), child_bill.clone(), false, false)
            .await,
        Ok(true)
    ));
    h.handle.begin_usage_resume_segment().await.unwrap();
    h.handle
        .settle_model_attempt_usage(
            "attempt-resume".into(),
            0,
            "model-a".into(),
            Some(sampling_types::TokenUsage {
                prompt_tokens: 5,
                completion_tokens: 1,
                total_tokens: 6,
                ..Default::default()
            }),
            Some(2),
            Some(3),
        )
        .await
        .unwrap();
    assert!(h.handle.mark_usage_incomplete(false, true).await);

    let events = h.handle.timeline_events().await.unwrap();
    let (persistence, _records) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        events,
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let usage = restored.try_get_session_usage().await.unwrap();
    assert_eq!(usage.totals.input_tokens, 35);
    assert_eq!(usage.totals.output_tokens, 6);
    assert_eq!(usage.totals.cached_read_tokens, 12);
    assert_eq!(usage.totals.cache_read_known_input_tokens, 30);
    assert_eq!(usage.totals.cache_read_unknown_calls, 1);
    assert_eq!(usage.totals.cache_write_unknown_calls, 3);
    assert_eq!(usage.segments[0].totals.cache_read_known_input_tokens, 30);
    assert_eq!(usage.segments[1].totals.cache_read_unknown_calls, 1);
    assert_eq!(usage.main_loop_model_calls, 2);
    assert_eq!(usage.by_model.len(), 2);
    assert_eq!(
        usage.by_agent[&crate::UsageAgent::Subagent("child-1".into())].total_tokens(),
        23
    );
    assert!(usage.incomplete);
    assert_eq!(usage.segments.len(), 2);
    assert_eq!(usage.segments[0].totals.total_tokens(), 35);
    assert_eq!(usage.segments[1].totals.total_tokens(), 6);
    assert!(usage.segments[1].incomplete);
    assert_eq!(
        usage
            .segments
            .iter()
            .map(|segment| segment.totals.total_tokens())
            .sum::<u64>(),
        usage.totals.total_tokens()
    );

    assert!(matches!(
        restored
            .record_subagent_usage("child-1".into(), child_bill.clone(), false, false)
            .await,
        Ok(false)
    ));
    assert!(matches!(
        restored
            .record_subagent_usage("child-1".into(), Vec::new(), false, true)
            .await,
        Err(crate::TimelineWriteError::SubagentUsageConflict)
    ));
}

async fn rejected_usage_restore(events: Vec<crate::TimelineEvent>) -> crate::TimelineWriteError {
    let (persistence, mut records) = MockTimelinePersistence::new();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let result = ChatStateActor::spawn_from_timeline(
        events,
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await;
    let Err(error) = result else {
        panic!("invalid usage must reject actor restoration");
    };
    assert!(
        records.drain().is_empty(),
        "failed restore must not persist recovery events"
    );
    assert!(
        event_rx.try_recv().is_err(),
        "failed restore must not publish actor updates"
    );
    error
}

#[tokio::test]
async fn restored_usage_checks_duplicate_payloads_across_resume_segments() {
    let h = TestHarness::new();
    h.handle
        .settle_model_attempt_usage(
            "attempt-1".into(),
            0,
            "model-a".into(),
            Some(sampling_types::TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
                ..Default::default()
            }),
            Some(5),
            Some(7),
        )
        .await
        .unwrap();
    h.handle
        .record_subagent_usage(
            "child-1".into(),
            vec![(
                "model-b".into(),
                crate::UsageTotals {
                    input_tokens: 20,
                    output_tokens: 3,
                    model_calls: 1,
                    ..Default::default()
                },
            )],
            false,
            false,
        )
        .await
        .unwrap();
    let original = h.handle.timeline_events().await.unwrap();
    let settlements: Vec<_> = original
        .iter()
        .filter_map(|event| match &event.kind {
            crate::TimelineEventKind::Observation(o) if o.name.ends_with("_settled") => {
                Some(o.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(settlements.len(), 2);
    let mut timeline = crate::Timeline::from_events(original).unwrap();
    timeline
        .record(crate::TimelineEventKind::Observation(
            crate::ObservationEvent {
                scope: "session_usage".into(),
                name: "resume_started".into(),
                turn: None,
                step: None,
                data: None,
            },
        ))
        .unwrap();
    for settlement in &settlements {
        timeline
            .record(crate::TimelineEventKind::Observation(settlement.clone()))
            .unwrap();
    }
    let (persistence, _records) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        timeline.events().to_vec(),
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let usage = restored.try_get_session_usage().await.unwrap();
    assert_eq!(usage.totals.total_tokens(), 35);
    assert_eq!(usage.segments.len(), 2);
    assert_eq!(usage.segments[0].totals.total_tokens(), 35);
    assert_eq!(usage.segments[1].totals.total_tokens(), 0);
    assert!(!usage.incomplete);

    for mut settlement in settlements {
        let is_attempt = settlement.name == "attempt_settled";
        let data = settlement.data.as_mut().unwrap();
        if is_attempt {
            data["api_duration_ms"] = serde_json::json!(99);
        } else {
            data["incomplete"] = serde_json::json!(true);
        }
        let mut conflict = timeline.clone();
        conflict
            .record(crate::TimelineEventKind::Observation(settlement))
            .unwrap();
        let error = rejected_usage_restore(conflict.events().to_vec()).await;
        assert!(matches!(
            (is_attempt, error),
            (true, crate::TimelineWriteError::AttemptUsageConflict)
                | (false, crate::TimelineWriteError::SubagentUsageConflict)
        ));
    }
}

#[tokio::test]
async fn restored_usage_rejects_malformed_known_facts() {
    let attempt = serde_json::json!({
        "attempt_key": "a", "model_id": "m", "captured_prompt_index": 0,
        "usage": null, "cost_usd_ticks": null, "api_duration_ms": null,
    });
    let child = serde_json::json!({"subagent_id": "c", "by_model": [], "incomplete": true});
    for (scope, name, valid) in [
        ("sampling_usage", "attempt_settled", attempt),
        ("session_usage", "subagent_settled", child),
    ] {
        let mut unknown_field = valid;
        unknown_field["unexpected"] = serde_json::json!(true);
        for data in [
            None,
            Some(serde_json::Value::Null),
            Some(serde_json::json!({})),
            Some(serde_json::json!(17)),
            Some(unknown_field),
        ] {
            let mut timeline = crate::Timeline::default();
            let event = timeline
                .record(crate::TimelineEventKind::Observation(
                    crate::ObservationEvent {
                        scope: scope.into(),
                        name: name.into(),
                        turn: None,
                        step: None,
                        data,
                    },
                ))
                .unwrap();
            let error = rejected_usage_restore(timeline.events().to_vec()).await;
            assert!(
                matches!(error, crate::TimelineWriteError::InvalidUsageObservation { seq, .. } if seq == event.seq.get())
            );
        }
    }
    for name in ["incomplete", "resume_started"] {
        let mut timeline = crate::Timeline::default();
        timeline
            .record(crate::TimelineEventKind::Observation(
                crate::ObservationEvent {
                    scope: "session_usage".into(),
                    name: name.into(),
                    turn: None,
                    step: None,
                    data: Some(serde_json::json!({"unexpected": true})),
                },
            ))
            .unwrap();
        assert!(matches!(
            rejected_usage_restore(timeline.events().to_vec()).await,
            crate::TimelineWriteError::InvalidUsageObservation { .. }
        ));
    }
}

#[tokio::test]
async fn restored_usage_ignores_unrelated_diagnostic_observations() {
    let mut timeline = crate::Timeline::default();
    for (scope, name) in [
        ("diagnostic", "attempt_settled"),
        ("session_usage", "future_diagnostic"),
    ] {
        timeline
            .record(crate::TimelineEventKind::Observation(
                crate::ObservationEvent {
                    scope: scope.into(),
                    name: name.into(),
                    turn: None,
                    step: None,
                    data: Some(serde_json::json!({"anything": 17})),
                },
            ))
            .unwrap();
    }
    let (persistence, _records) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        timeline.events().to_vec(),
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let usage = restored.try_get_session_usage().await.unwrap();
    assert_eq!(usage.totals.total_tokens(), 0);
    assert!(!usage.incomplete);
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
    let surface_before = crate::estimate_conversation_tokens(&h.handle.get_conversation().await);
    h.handle.rewind_durably(0).await.unwrap();
    let surface_after = crate::estimate_conversation_tokens(&h.handle.get_conversation().await);
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
async fn image_projection_pairs_descriptions_and_selects_images_per_request() {
    use sampling_types::conversation::{
        ContentPart, ToolCall, UserItem, conversation_image_groups,
    };

    let user = ConversationItem::User(UserItem {
        content: vec![
            ContentPart::Text {
                text: "inspect these".into(),
            },
            ContentPart::Image {
                description: None,
                url: "data:image/png;base64,user".into(),
            },
        ],
        synthetic_reason: None,
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
                description: None,
                url: "data:image/png;base64,tool-a".into(),
            },
            ContentPart::Image {
                description: None,
                url: "data:image/png;base64,tool-b".into(),
            },
        ],
    );
    let mut h = TestHarness::new();
    h.handle.begin_turn_capture();
    h.handle.push_user_message(user);
    h.handle
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![ToolCall {
            id: "call_7".into(),
            name: "read_file".to_owned(),
            arguments: r#"{"target_file":"/secret/original.png"}"#.into(),
        }]));
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
                "converted tool images".to_owned()
            },
            provenance: crate::ImageShadowSource::Description {
                result_ref: crate::TimelineRangeRef {
                    timeline_id: sideband_id.clone(),
                    first_seq: 2,
                    last_seq: 2,
                },
            },
        })
        .collect();
    let tool_group = groups
        .iter()
        .find_map(|group| group.tool_call.as_ref())
        .expect("tool image group must retain typed call provenance");
    let tool_calls = vec![crate::ImageToolCallShadow {
        source: materialized.surface_ids[tool_group.item_index],
        tool_call_ids: vec![tool_group.tool_call_id.clone()],
        carrier_sources: Vec::new(),
    }];

    let report = h
        .handle
        .record_image_projection_and_ack(crate::ImageProjectionEvent {
            trigger_runtime: sampling_types::model_image_input_key(&test_config()),
            source_revision: materialized.surface_revision,
            shadows,
            tool_calls,
        })
        .await
        .unwrap();
    assert_eq!(report.described_images, 3);

    let conversation = h.handle.get_conversation().await;
    assert!(
        sampling_types::conversation::item_image_description(&conversation[0])
            .unwrap()
            .contains("converted user image")
    );
    let ConversationItem::User(user) = &conversation[0] else {
        panic!("expected user item");
    };
    assert_eq!(user.synthetic_reason, None);
    assert_eq!(user.prompt_index, Some(7));
    assert!(user.content.iter().any(|part| matches!(
        part,
        ContentPart::Image {
            description: Some(_),
            ..
        }
    )));
    let ConversationItem::Assistant(assistant) = &conversation[1] else {
        panic!("expected assistant item");
    };
    assert_eq!(assistant.tool_calls[0].id.as_ref(), "call_7");
    assert!(
        !assistant.tool_calls[0]
            .arguments
            .contains("/secret/original.png")
    );
    let ConversationItem::ToolResult(result) = &conversation[2] else {
        panic!("expected tool result");
    };
    assert_eq!(result.tool_call_id, "call_7");
    assert_eq!(result.images.len(), 3);
    assert!(matches!(&result.images[0], ContentPart::Text { text } if text.as_ref() == "keep-me"));
    assert_eq!(
        sampling_types::conversation::item_image_description(&conversation[2]),
        Some("converted tool images")
    );
    assert!(result.content.contains("Read image file"));
    let capture = h.handle.take_turn_messages().await.unwrap();
    assert_eq!(capture.messages.len(), conversation.len());
    assert_eq!(conversation_image_groups(&capture.messages).len(), 2);

    let projected = h
        .handle
        .build_request_for_image_mode("test-timeline", vec![], None, None, None, true)
        .await
        .unwrap();
    assert!(conversation_image_groups(&projected.items).is_empty());
    assert!(
        projected.items[0]
            .text_content()
            .contains("converted user image")
    );
    assert!(
        projected.items[2]
            .text_content()
            .contains("converted tool images")
    );
    let ConversationItem::Assistant(assistant) = &projected.items[1] else {
        panic!("expected projected assistant item");
    };
    assert!(
        !assistant.tool_calls[0]
            .arguments
            .contains("/secret/original.png")
    );

    let mut other_route = test_config();
    other_route.model = "vision-model".to_owned();
    h.handle.replace_sampling_route(other_route);
    let after_model_change = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
    assert_eq!(
        conversation_image_groups(&after_model_change.items).len(),
        2
    );
    assert!(
        sampling_types::conversation::item_image_description(&after_model_change.items[0])
            .unwrap()
            .contains("converted user image")
    );
    let records = h.drain_persistence();
    assert!(records.iter().any(|record| matches!(
        record,
        PersistenceRecord::Timeline(crate::TimelineEvent {
            kind: crate::TimelineEventKind::ImageProjection(_),
            ..
        })
    )));
    assert!(records.iter().any(|record| matches!(
        record,
        PersistenceRecord::Timeline(crate::TimelineEvent {
            kind: crate::TimelineEventKind::Messages(crate::MessageEvent { items, .. }),
            ..
        }) if !conversation_image_groups(items).is_empty()
    )));
}

#[tokio::test]
async fn image_removal_projection_replaces_user_images_and_replays_identically() {
    use sampling_types::conversation::{
        ContentPart, UNSUPPORTED_IMAGE_REPLACEMENT, UserItem, conversation_image_groups,
    };

    let user = ConversationItem::User(UserItem {
        content: vec![
            ContentPart::Text {
                text: "inspect these".into(),
            },
            ContentPart::Image {
                description: None,
                url: "data:image/png;base64,first".into(),
            },
            ContentPart::Image {
                description: None,
                url: "data:image/png;base64,second".into(),
            },
        ],
        synthetic_reason: None,
        permission_evidence: None,
        goal_directive: None,
        cwd_generation: None,
        prior_turn_interrupt: None,
        prompt_index: Some(5),
    });
    let mut h = TestHarness::new();
    h.handle.begin_turn_capture();
    h.handle.push_user_message(user);
    let materialized = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    let group = conversation_image_groups(&materialized.surface).remove(0);
    assert_eq!(group.image_count(), 2);
    let source = materialized.surface_ids[group.item_index];

    let report = h
        .handle
        .record_image_projection_and_ack(crate::ImageProjectionEvent {
            trigger_runtime: sampling_types::model_image_input_key(&test_config()),
            source_revision: materialized.surface_revision,
            shadows: vec![crate::ImageShadow {
                source,
                fingerprint: group.fingerprint.clone(),
                image_count: group.image_count(),
                replacement: UNSUPPORTED_IMAGE_REPLACEMENT.to_owned(),
                provenance: crate::ImageShadowSource::UnsupportedModel,
            }],
            tool_calls: vec![],
        })
        .await
        .unwrap();
    assert_eq!(report.removed_images, 2);
    assert_eq!(report.described_images, 0);
    assert_eq!(report.total_images(), 2);

    let accepted = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    assert!(conversation_image_groups(&accepted.surface).is_empty());
    assert_ne!(
        accepted.surface_ids[0], source,
        "a removal projection must advance the Surface identity"
    );
    let ConversationItem::User(projected) = &accepted.surface[0] else {
        panic!("expected user item");
    };
    assert_eq!(projected.prompt_index, Some(5));
    assert!(matches!(
        projected.content.as_slice(),
        [ContentPart::Text { text: heading }, ContentPart::Text { text: replacement }]
            if heading.as_ref() == "inspect these"
                && replacement.as_ref() == UNSUPPORTED_IMAGE_REPLACEMENT
    ));

    let events = h
        .drain_persistence()
        .into_iter()
        .filter_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .collect::<Vec<_>>();
    assert!(
        events.iter().any(|event| matches!(
            &event.kind,
            crate::TimelineEventKind::Messages(messages)
                if !conversation_image_groups(&messages.items).is_empty()
        )),
        "the raw image-bearing message must stay immutable evidence"
    );
    let replayed = crate::Timeline::from_events(events).unwrap();
    assert_eq!(
        serde_json::to_value(replayed.surface()).unwrap(),
        serde_json::to_value(&accepted.surface).unwrap()
    );
    assert_eq!(replayed.surface_ids(), accepted.surface_ids);
}

#[tokio::test]
async fn mixed_description_and_removal_projection_splits_report_counts() {
    use sampling_types::conversation::{
        ContentPart, UNSUPPORTED_IMAGE_REPLACEMENT, UserItem, conversation_image_groups,
    };

    let described = ConversationItem::User(UserItem {
        content: vec![
            ContentPart::Text {
                text: "described group".into(),
            },
            ContentPart::Image {
                description: None,
                url: "data:image/png;base64,described".into(),
            },
        ],
        ..Default::default()
    });
    let removed = ConversationItem::User(UserItem {
        content: vec![
            ContentPart::Text {
                text: "removed group".into(),
            },
            ContentPart::Image {
                description: None,
                url: "data:image/png;base64,removed-a".into(),
            },
            ContentPart::Image {
                description: None,
                url: "data:image/png;base64,removed-b".into(),
            },
        ],
        ..Default::default()
    });
    let h = TestHarness::new();
    h.handle.begin_turn_capture();
    h.handle.push_user_message(described);
    h.handle.push_user_message(removed);
    let materialized = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    let groups = conversation_image_groups(&materialized.surface);
    assert_eq!(groups.len(), 2);
    let shadows = groups
        .iter()
        .map(|group| crate::ImageShadow {
            source: materialized.surface_ids[group.item_index],
            fingerprint: group.fingerprint.clone(),
            image_count: group.image_count(),
            replacement: if group.item_index == 0 {
                "described in order".to_owned()
            } else {
                UNSUPPORTED_IMAGE_REPLACEMENT.to_owned()
            },
            provenance: if group.item_index == 0 {
                crate::ImageShadowSource::LocalOcr {
                    engine: "tesseract".into(),
                }
            } else {
                crate::ImageShadowSource::UnsupportedModel
            },
        })
        .collect::<Vec<_>>();

    let report = h
        .handle
        .record_image_projection_and_ack(crate::ImageProjectionEvent {
            trigger_runtime: sampling_types::model_image_input_key(&test_config()),
            source_revision: materialized.surface_revision,
            shadows,
            tool_calls: vec![],
        })
        .await
        .unwrap();
    assert_eq!(report.described_images, 1);
    assert_eq!(report.removed_images, 2);
    assert_eq!(report.total_images(), 3);

    let conversation = h.handle.get_conversation().await;
    assert_eq!(
        sampling_types::conversation::item_image_description(&conversation[0]),
        Some("described in order")
    );
    assert_eq!(conversation_image_groups(&conversation[..1]).len(), 1);
    assert!(conversation_image_groups(&conversation[1..]).is_empty());
    let ConversationItem::User(removed_item) = &conversation[1] else {
        panic!("expected user item");
    };
    assert!(matches!(
        removed_item.content.as_slice(),
        [ContentPart::Text { text: heading }, ContentPart::Text { text: replacement }]
            if heading.as_ref() == "removed group"
                && replacement.as_ref() == UNSUPPORTED_IMAGE_REPLACEMENT
    ));
    let materialized_after = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    assert_ne!(
        materialized_after.surface_ids[0],
        materialized.surface_ids[0]
    );
    assert_ne!(
        materialized_after.surface_ids[1],
        materialized.surface_ids[1]
    );
    assert_eq!(
        materialized_after.surface_revision,
        materialized.surface_revision + 1
    );
}

#[tokio::test]
async fn image_projection_retries_an_uncertain_persistence_failure() {
    use sampling_types::conversation::{ContentPart, UserItem, conversation_image_groups};

    let user = ConversationItem::User(UserItem {
        content: vec![ContentPart::Image {
            description: None,
            url: "data:image/png;base64,original".into(),
        }],
        ..Default::default()
    });
    let mut h = TestHarness::with_manual_timeline_ack_after(vec![user.clone()], 1);
    let materialized = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    let groups = conversation_image_groups(&materialized.surface);
    let source = materialized.surface_ids[groups[0].item_index];
    let sideband_id = uuid::Uuid::now_v7().to_string();
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
    let projection = crate::ImageProjectionEvent {
        trigger_runtime: sampling_types::model_image_input_key(&test_config()),
        source_revision: materialized.surface_revision,
        shadows: vec![crate::ImageShadow {
            source,
            fingerprint: groups[0].fingerprint.clone(),
            image_count: groups[0].image_count(),
            replacement: "durable description".to_owned(),
            provenance: crate::ImageShadowSource::Description {
                result_ref: crate::TimelineRangeRef {
                    timeline_id: sideband_id,
                    first_seq: 1,
                    last_seq: 1,
                },
            },
        }],
        tool_calls: vec![],
    };
    let handle = h.handle.clone();
    let projection_future = async move { handle.record_image_projection_and_ack(projection).await };
    let retry = fail_once_then_ack_exact_retry(&mut h.persistence_rx);
    let (report, ()) = tokio::join!(projection_future, retry);
    assert_eq!(report.unwrap().described_images, 1);
    assert_eq!(
        conversation_image_groups(&h.handle.get_conversation().await).len(),
        1
    );
    let materialized = h
        .handle
        .materialize_timeline("test-timeline".into())
        .await
        .unwrap();
    assert_eq!(conversation_image_groups(&materialized.surface).len(), 1);
    assert_eq!(
        sampling_types::conversation::item_image_description(&materialized.surface[0]),
        Some("durable description")
    );
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
async fn storage_full_failure_poison_closes_the_actor_without_retrying_forever() {
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        Vec::new(),
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    let write = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .record_timeline_event_durably(crate::TimelineEventKind::Observation(
                    crate::ObservationEvent {
                        scope: "test".into(),
                        name: "disk_full".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .await
        })
    };
    persistence_rx
        .next_timeline_ack()
        .await
        .expect("pending storage acknowledgement")
        .send(Err(std::io::Error::new(
            std::io::ErrorKind::StorageFull,
            "simulated full disk",
        )))
        .unwrap();

    let error = tokio::time::timeout(Duration::from_secs(1), write)
        .await
        .expect("storage-full write must return promptly")
        .expect("write task")
        .expect_err("storage-full write must fail");
    assert!(matches!(
        error,
        crate::TimelineWriteError::Persistence(error)
            if error.kind() == std::io::ErrorKind::StorageFull
    ));
    let pending = handle
        .pending_timeline_commit()
        .expect("permanent persistence failure keeps the rejected event identity");
    assert_eq!(pending.event.seq.get(), 0);
    assert_eq!(pending.error_kind, Some(std::io::ErrorKind::StorageFull));
    assert_eq!(
        pending.error_message.as_deref(),
        Some("simulated full disk")
    );
    assert!(!pending.acknowledgement_lost);
    assert!(matches!(
        &pending.event.kind,
        crate::TimelineEventKind::Observation(event) if event.name == "disk_full"
    ));
    let closed = tokio::time::timeout(Duration::from_secs(1), event_rx.recv())
        .await
        .expect("poisoned actor must close its event channel");
    assert!(closed.is_none());
    let retry = tokio::time::timeout(
        Duration::from_millis(100),
        persistence_rx.next_timeline_ack(),
    )
    .await;
    assert!(!matches!(retry, Ok(Some(_))), "storage full must not retry");
}

#[tokio::test(start_paused = true)]
async fn unacknowledged_write_times_out_with_shared_exact_pending_identity() {
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        Vec::new(),
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    let observer = handle.clone();
    let write = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .record_timeline_event_durably(crate::TimelineEventKind::Observation(
                    crate::ObservationEvent {
                        scope: "test".into(),
                        name: "ack_deadline".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .await
        })
    };
    let acknowledgement = persistence_rx
        .next_timeline_ack()
        .await
        .expect("writer sends the pending append");
    let pending_before = observer
        .pending_timeline_commit()
        .expect("shared handle exposes the pending commit identity");
    assert_eq!(pending_before.event.seq.get(), 0);
    assert!(!pending_before.acknowledgement_lost);
    assert!(matches!(
        &pending_before.event.kind,
        crate::TimelineEventKind::Observation(event) if event.name == "ack_deadline"
    ));
    let at_ms = pending_before.event.at_ms;

    tokio::time::advance(Duration::from_secs(30)).await;
    let error = write
        .await
        .expect("write task")
        .expect_err("ACK deadline must expire");
    assert!(matches!(error, crate::TimelineWriteError::DeadlineExceeded));
    tokio::time::timeout(Duration::from_secs(1), async {
        while !handle.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the ambiguous writer epoch closes its mailbox");

    let pending_after = observer
        .pending_timeline_commit()
        .expect("ambiguous event identity remains available to observers");
    assert_eq!(pending_after.event.seq.get(), 0);
    assert_eq!(pending_after.event.at_ms, at_ms);
    assert!(pending_after.acknowledgement_lost);
    assert!(matches!(
        &pending_after.event.kind,
        crate::TimelineEventKind::Observation(event) if event.name == "ack_deadline"
    ));
    assert!(
        acknowledgement.send(Ok(())).is_err(),
        "late ACK has no live writer"
    );

    let later = handle
        .record_timeline_event_durably(crate::TimelineEventKind::Observation(
            crate::ObservationEvent {
                scope: "test".into(),
                name: "must_not_reuse_sequence".into(),
                turn: None,
                step: None,
                data: None,
            },
        ))
        .await;
    assert!(matches!(
        later,
        Err(crate::TimelineWriteError::AcknowledgementLost)
    ));
    assert_eq!(handle.pending_timeline_commit().unwrap().event.seq.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn subagent_usage_ack_loss_retries_the_same_fact_and_deadline_stops_dispatch() {
    let mut h = TestHarness::with_manual_timeline_ack(vec![]);
    let usage = vec![(
        "child-model".to_owned(),
        crate::usage::UsageTotals {
            input_tokens: 120,
            output_tokens: 30,
            model_calls: 1,
            ..Default::default()
        },
    )];
    let record_usage = {
        let handle = h.handle.clone();
        let usage = usage.clone();
        tokio::spawn(async move {
            handle
                .record_subagent_usage("child-ack-lost".into(), usage, false, false)
                .await
        })
    };
    let first_ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("first child-usage append");
    let first_event = h
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("the first child-usage event is sent before its ACK");
    drop(first_ack);

    let retry_ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("a lost ACK retries the pending usage fact");
    let retry_event = h
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("the retry is dispatched");
    assert_eq!(retry_event.seq, first_event.seq);
    assert_eq!(
        serde_json::to_value(&retry_event).unwrap(),
        serde_json::to_value(&first_event).unwrap(),
        "the retry retains the immutable event identity and payload"
    );
    retry_ack.send(Ok(())).expect("the retry ACK is pending");
    assert!(record_usage.await.unwrap().unwrap());
    assert_eq!(
        h.handle
            .try_get_session_usage()
            .await
            .unwrap()
            .totals
            .total_tokens(),
        150,
        "the committed usage fact is projected once"
    );
    assert_eq!(
        h.handle
            .timeline_events()
            .await
            .unwrap()
            .iter()
            .filter(|event| matches!(
                &event.kind,
                crate::TimelineEventKind::Observation(observation)
                    if observation.name == "subagent_settled"
            ))
            .count(),
        1,
        "the canonical ledger contains one settlement"
    );

    let unresolved_usage = vec![(
        "child-model".to_owned(),
        crate::usage::UsageTotals {
            input_tokens: 4,
            output_tokens: 2,
            model_calls: 1,
            ..Default::default()
        },
    )];
    let unresolved = {
        let handle = h.handle.clone();
        tokio::spawn(async move {
            handle
                .record_subagent_usage("child-no-ack".into(), unresolved_usage, false, false)
                .await
        })
    };
    let unresolved_first = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("second usage append");
    let unresolved_event = h
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("second usage event is dispatched");
    drop(unresolved_first);
    let unresolved_retry = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("second usage append is retried exactly");
    let unresolved_retry_event = h
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("second retry is dispatched");
    assert_eq!(
        serde_json::to_value(&unresolved_retry_event).unwrap(),
        serde_json::to_value(&unresolved_event).unwrap()
    );

    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(matches!(
        unresolved.await.unwrap(),
        Err(crate::TimelineWriteError::DeadlineExceeded)
    ));
    assert!(
        unresolved_retry.send(Ok(())).is_err(),
        "late ACK is rejected"
    );
    let pending = h
        .handle
        .pending_timeline_commit()
        .expect("unconfirmed usage remains owned after deadline");
    assert_eq!(pending.event.seq, unresolved_event.seq);
    assert!(pending.acknowledgement_lost);
    assert_eq!(
        serde_json::to_value(&pending.event).unwrap(),
        serde_json::to_value(&unresolved_event).unwrap()
    );
    assert!(
        h.drain_persistence().is_empty(),
        "deadline expiry must not dispatch another mutation retry"
    );
}

async fn commit_finalization_fact_after_lost_ack(
    h: &mut TestHarness,
    kind: crate::TimelineEventKind,
) -> crate::TimelineEvent {
    let write = {
        let handle = h.handle.clone();
        tokio::spawn(async move { handle.record_timeline_event_durably(kind).await })
    };
    let first_ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("first finalization fact append");
    let first_event = h
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("first finalization fact is dispatched");
    drop(first_ack);

    let retry_ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("a lost ACK retries the same fact");
    let retry_event = h
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("retry finalization fact is dispatched");
    assert_eq!(
        serde_json::to_vec(&retry_event).unwrap(),
        serde_json::to_vec(&first_event).unwrap(),
        "exact retry preserves event sequence and payload"
    );
    retry_ack.send(Ok(())).expect("retry ACK is pending");
    let committed = write.await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_vec(&committed).unwrap(),
        serde_json::to_vec(&first_event).unwrap()
    );
    first_event
}

#[tokio::test]
async fn child_result_and_parent_ended_retries_keep_their_canonical_event_once() {
    let mut child = TestHarness::with_manual_timeline_ack(vec![]);
    commit_finalization_fact_after_lost_ack(
        &mut child,
        crate::TimelineEventKind::SubagentSeed(crate::SubagentSeedEvent {
            parent_timeline_id: "parent-session".into(),
            parent_spawn_seq: 0,
            subagent_id: "child-result-retry".into(),
            security_parent_session_id: "parent-session".into(),
            context_source: crate::SubagentContextSource::New,
            source_ref: None,
            normalized: false,
        }),
    )
    .await;
    let child_result = commit_finalization_fact_after_lost_ack(
        &mut child,
        crate::TimelineEventKind::SubagentResult(crate::SubagentResultEvent {
            subagent_id: "child-result-retry".into(),
            outcome: crate::SubagentOutcome::Completed,
            duration_ms: 19,
            tool_calls: 1,
            turns: 1,
            tokens_used: 7,
            error: None,
            output_ref: Some(format!(
                "artifact:subagent-output:blake3:{}",
                "b".repeat(64)
            )),
            planning_summary: None,
        }),
    )
    .await;
    assert_eq!(
        child
            .handle
            .timeline_events()
            .await
            .unwrap()
            .iter()
            .filter(|event| {
                serde_json::to_vec(*event).unwrap() == serde_json::to_vec(&child_result).unwrap()
            })
            .count(),
        1,
        "the child ledger has one canonical result"
    );

    let mut parent = TestHarness::with_manual_timeline_ack(vec![]);
    let spawn = commit_finalization_fact_after_lost_ack(
        &mut parent,
        crate::TimelineEventKind::Subagent(crate::SubagentEvent::Spawned(
            crate::SubagentSpawnEvent {
                subagent_id: "parent-ended-retry".into(),
                child_session_id: "child-session".into(),
                origin_tool_call_id: None,
                security_parent_session_id: "parent-session".into(),
                parent_step_id: None,
                subagent_type: "task".into(),
                description: "finalization test".into(),
                prompt: "complete the task".into(),
                execution_recipe: serde_json::json!({}),
                context_source: crate::SubagentContextSource::New,
                source_ref: None,
                context_normalized: false,
                resumed_from: None,
                parent_prompt_id: None,
                capability_mode: None,
                permission_mode: None,
                effective_permission_mode: None,
                workflow_run_id: None,
                goal_id: None,
                goal_definition_revision: None,
                surface_completion: true,
                child_cwd: "/tmp".into(),
                worktree_path: None,
                effective_model_id: "test-model".into(),
                model_transport_key: sampling_types::model_image_input_key(&test_config()),
                reasoning_effort: None,
            },
        )),
    )
    .await;
    let parent_terminal = commit_finalization_fact_after_lost_ack(
        &mut parent,
        crate::TimelineEventKind::Subagent(crate::SubagentEvent::Ended(
            crate::SubagentTerminalEvent {
                subagent_id: "parent-ended-retry".into(),
                child_session_id: "child-session".into(),
                outcome: crate::SubagentOutcome::Completed,
                duration_ms: 19,
                tool_calls: 1,
                turns: 1,
                tokens_used: 7,
                error: None,
                result_ref: Some(crate::TimelineRangeRef {
                    timeline_id: "child-session".into(),
                    first_seq: 1,
                    last_seq: 1,
                }),
                snapshot_ref: None,
            },
        )),
    )
    .await;
    assert_eq!(
        parent_terminal.seq.get(),
        spawn.seq.get() + 1,
        "parent Ended follows its one acknowledged Spawned fact"
    );
    assert_eq!(
        parent
            .handle
            .timeline_events()
            .await
            .unwrap()
            .iter()
            .filter(|event| {
                serde_json::to_vec(*event).unwrap() == serde_json::to_vec(&parent_terminal).unwrap()
            })
            .count(),
        1,
        "the parent ledger has one canonical Ended fact"
    );
}

#[tokio::test(start_paused = true)]
async fn explicit_timeline_deadline_keeps_exact_pending_event_and_poison_state() {
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        Vec::new(),
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    let observer = handle.clone();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    let write = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .record_timeline_event_durably_before(
                    crate::TimelineEventKind::Observation(crate::ObservationEvent {
                        scope: "test".into(),
                        name: "explicit_deadline".into(),
                        turn: None,
                        step: None,
                        data: None,
                    }),
                    deadline,
                )
                .await
        })
    };
    let ack = persistence_rx
        .next_timeline_ack()
        .await
        .expect("append starts before its deadline");
    let pending_before = observer
        .pending_timeline_commit()
        .expect("exact pending event is visible during persistence");

    tokio::time::advance(Duration::from_secs(4)).await;
    assert!(matches!(
        write.await.expect("write task"),
        Err(crate::TimelineWriteError::DeadlineExceeded)
    ));
    let pending_after = observer
        .pending_timeline_commit()
        .expect("deadline retains pending event identity");
    assert_eq!(
        serde_json::to_vec(&pending_after.event).unwrap(),
        serde_json::to_vec(&pending_before.event).unwrap()
    );
    assert!(pending_after.acknowledgement_lost);
    assert!(handle.is_closed(), "deadline poisons the writer epoch");
    assert!(
        ack.send(Ok(())).is_err(),
        "late acknowledgement is rejected"
    );
}

#[tokio::test(start_paused = true)]
async fn already_elapsed_timeline_deadline_does_not_dispatch_append() {
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        Vec::new(),
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    let deadline = tokio::time::Instant::now() - Duration::from_secs(1);

    let result = handle
        .record_timeline_event_durably_before(
            crate::TimelineEventKind::Observation(crate::ObservationEvent {
                scope: "test".into(),
                name: "expired_before_append".into(),
                turn: None,
                step: None,
                data: None,
            }),
            deadline,
        )
        .await;

    assert!(matches!(
        result,
        Err(crate::TimelineWriteError::DeadlineExceeded)
    ));
    let pending = handle
        .pending_timeline_commit()
        .expect("prepared event identity survives pre-append expiry");
    assert_eq!(pending.event.seq.get(), 0);
    assert!(!pending.acknowledgement_lost);
    assert!(
        persistence_rx.drain().is_empty(),
        "no append was dispatched"
    );
    assert!(
        handle.is_closed(),
        "pre-append deadline poisons writer epoch"
    );
}

#[tokio::test(start_paused = true)]
async fn already_elapsed_receipt_deadline_reaches_chat_state_before_append() {
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        Vec::new(),
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    let deadline = tokio::time::Instant::now() - Duration::from_secs(1);
    let result = handle
        .receive_notification_durably_before(
            "session-1".into(),
            crate::NotificationSource::SubagentCompleted {
                subagent_id: "child-1".into(),
                owner: crate::NotificationOwner::Session,
            },
            crate::NotificationSourceVersion::Ordinal { value: 1 },
            crate::NotificationPayloadRef {
                blake3: "00".repeat(32),
                bytes: 1,
            },
            Some(deadline),
        )
        .await;

    assert!(matches!(
        result,
        Err(crate::TimelineWriteError::DeadlineExceeded)
    ));
    let pending = handle
        .pending_timeline_commit()
        .expect("receipt event identity remains pending");
    assert!(matches!(
        pending.event.kind,
        crate::TimelineEventKind::Notification(_)
    ));
    assert!(!pending.acknowledgement_lost);
    assert!(
        persistence_rx.drain().is_empty(),
        "expired receipt was never appended"
    );
}

#[tokio::test(start_paused = true)]
async fn already_elapsed_usage_deadline_reaches_chat_state_before_append() {
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        Vec::new(),
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    let deadline = tokio::time::Instant::now() - Duration::from_secs(1);
    let result = handle
        .record_subagent_usage_before("child-1".into(), Vec::new(), false, true, Some(deadline))
        .await;

    assert!(matches!(
        result,
        Err(crate::TimelineWriteError::DeadlineExceeded)
    ));
    let pending = handle
        .pending_timeline_commit()
        .expect("usage event identity remains pending");
    assert!(matches!(
        &pending.event.kind,
        crate::TimelineEventKind::Observation(event)
            if event.scope == "session_usage" && event.name == "subagent_settled"
    ));
    assert!(!pending.acknowledgement_lost);
    assert!(
        persistence_rx.drain().is_empty(),
        "expired usage was never appended"
    );
}

#[tokio::test(start_paused = true)]
async fn repeated_transient_writes_keep_the_same_event_until_the_ack_deadline() {
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        Vec::new(),
        test_config(),
        Box::new(mock),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    );
    let caller_deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    let write = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .record_timeline_event_durably_before(
                    crate::TimelineEventKind::Observation(crate::ObservationEvent {
                        scope: "test".into(),
                        name: "retry_until_deadline".into(),
                        turn: None,
                        step: None,
                        data: None,
                    }),
                    caller_deadline,
                )
                .await
        })
    };
    let deadline = caller_deadline;
    let mut exact_event = None;
    let mut attempts = 0;
    loop {
        let ack = tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            ack = persistence_rx.next_timeline_ack() => {
                let Some(ack) = ack else { break };
                ack
            }
        };
        let event = persistence_rx
            .drain()
            .into_iter()
            .find_map(|record| match record {
                PersistenceRecord::Timeline(event) => Some(event),
                PersistenceRecord::Flush => None,
            })
            .expect("timeline retry record");
        if let Some(first) = &exact_event {
            assert_eq!(
                serde_json::to_vec(&event).unwrap(),
                serde_json::to_vec(first).unwrap(),
                "retry {attempts} must retain seq, timestamp, and event kind"
            );
        } else {
            exact_event = Some(event);
        }
        attempts += 1;
        ack.send(Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "transient storage timeout",
        )))
        .unwrap();
    }
    assert!(
        attempts > 2,
        "transient retries must continue until the deadline"
    );
    let error = write
        .await
        .expect("write task")
        .expect_err("retry deadline must expire");
    assert!(matches!(error, crate::TimelineWriteError::DeadlineExceeded));
    let pending = handle
        .pending_timeline_commit()
        .expect("pending event retained");
    let first = exact_event.expect("at least one attempted event");
    assert_eq!(
        serde_json::to_vec(&pending.event).unwrap(),
        serde_json::to_vec(&first).unwrap()
    );
    assert_eq!(pending.error_kind, Some(std::io::ErrorKind::TimedOut));
    assert!(!pending.acknowledgement_lost);
}

#[tokio::test]
async fn transient_persistence_retry_is_cancelled_by_actor_shutdown() {
    let (mock, mut persistence_rx) = MockTimelinePersistence::new_with_manual_timeline_ack();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let cancellation = tokio_util::sync::CancellationToken::new();
    let handle = ChatStateActor::spawn(
        Vec::new(),
        test_config(),
        Box::new(mock),
        event_tx,
        cancellation.clone(),
    );
    let write = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .record_timeline_event_durably(crate::TimelineEventKind::Observation(
                    crate::ObservationEvent {
                        scope: "test".into(),
                        name: "transient_retry".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .await
        })
    };
    persistence_rx
        .next_timeline_ack()
        .await
        .expect("pending acknowledgement")
        .send(Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "simulated transient failure",
        )))
        .unwrap();

    cancellation.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), write)
        .await
        .expect("cancellation must interrupt retry backoff")
        .expect("write task");
    assert!(matches!(result, Err(crate::TimelineWriteError::Cancelled)));
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
            .send(Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "simulated transient storage timeout",
            )))
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

    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
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

    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
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
async fn replace_sampling_route_is_queryable() {
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
    h.handle.replace_sampling_route(new_config.clone());

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

    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
    assert_eq!(request.items.len(), 2);
}

#[tokio::test]
async fn build_request_with_empty_conversation() {
    let h = TestHarness::new();
    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
    assert!(request.items.is_empty());
}

#[tokio::test]
async fn build_request_preserves_system_message() {
    let h = TestHarness::with_conversation(vec![
        ConversationItem::system("You are a coding assistant."),
        ConversationItem::user("hi"),
    ]);
    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
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

    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

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

    let request = h
        .handle
        .build_request("test-timeline", tools, None, None, None)
        .await
        .unwrap();

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
        .build_request("test-timeline", second.tools.clone(), None, None, None)
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

    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

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

    let _ = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

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
    assert!(
        conv.last()
            .unwrap()
            .text_content()
            .contains("Remember this")
    );

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

    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

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
        response_messages: Vec::new(),
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
    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

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
            response_messages: Vec::new(),
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
    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
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
    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
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
    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
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
    let request = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();
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
            description: None,
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
                description: None,
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
                description: None,
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
        .push_assistant_response(ConversationItem::assistant_tool_calls(vec![
            sampling_types::ToolCall {
                id: "c1".into(),
                name: "read".into(),
                arguments: "{}".into(),
            },
        ]));
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
async fn resumed_multi_context_control_accepts_durable_initial_context() {
    let mut timeline =
        crate::Timeline::from_seed(vec![ConversationItem::system("system")]).unwrap();
    timeline
        .record(crate::TimelineEventKind::Control(crate::ControlEvent {
            revision: 1,
            snapshot: serde_json::json!({"behavior": "normal"}),
            retired_context_layers: vec![],
            model_contexts: [
                crate::ControlContextLayer::AgentRole,
                crate::ControlContextLayer::Behavior,
            ]
            .into_iter()
            .map(|layer| crate::ControlContext {
                layer,
                activation: crate::ControlContextActivation::Transition,
                item: ConversationItem::system_reminder(format!("{layer:?}")),
            })
            .collect(),
        }))
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
    assert!(persistence_rx.drain().is_empty());
    let (mut context, revision) = handle.get_conversation_with_revision().await.unwrap();
    context.insert(
        1,
        ConversationItem::project_instructions("project instructions"),
    );
    handle
        .replace_context_durably(context.clone(), revision)
        .await
        .unwrap();
    let records = persistence_rx.drain();
    assert_eq!(records.len(), 1);
    let PersistenceRecord::Timeline(event) = &records[0] else {
        panic!("expected durable context");
    };
    assert_eq!(event.seq, timeline.next_seq());
    timeline.accept(event.clone()).unwrap();
    let replay = crate::Timeline::from_events(timeline.events().to_vec()).unwrap();
    assert_eq!(
        serde_json::to_value(replay.surface()).unwrap(),
        serde_json::to_value(&context).unwrap()
    );
    assert_eq!(
        serde_json::to_value(handle.get_conversation().await).unwrap(),
        serde_json::to_value(context).unwrap()
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
            response_messages: Vec::new(),
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
                response_messages: Vec::new(),
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
                response_messages: Vec::new(),
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
            response_messages: Vec::new(),
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
/// Note: `handle_model_metadata_update` in the session actor now blocks
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
// These target the provider-neutral durable reasoning shape:
//   - No `__RAW_OUTPUT_PLACEHOLDER__` sentinels
//   - No `extract_raw_input_items()` / `splice_raw_input_items()`
//   - Visible reasoning lives as a durable sibling but is not emitted by the
//     portable projector; same-route native fragments are memory-only.
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

/// Build a durable, visible reasoning sibling. Native encrypted continuation is
/// intentionally kept out of Timeline fixtures.
fn reasoning_sibling(id: &str, _encrypted: Option<&str>) -> ConversationItem {
    ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(format!(
        "thinking for {id}"
    )))
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

    let req1 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

    h.handle
        .push_assistant_response(ConversationItem::assistant("Hi there!"));
    h.handle
        .push_user_message(ConversationItem::user("How are you?"));

    let req2 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

    assert_prefix_stable_pair(&req1, &req2, "turn 1 -> turn 2");

    h.handle
        .push_assistant_response(ConversationItem::assistant("I'm well!"));
    h.handle.push_user_message(ConversationItem::user("Great"));

    let req3 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

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

    let req1 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

    // Push Reasoning sibling + Assistant (the new ordering produced by
    // `response_to_conversation_items`: Reasoning before Assistant).
    h.handle
        .push_tool_result(reasoning_sibling("r_abc", Some("enc1")));
    h.handle
        .push_assistant_response(ConversationItem::assistant("response 1"));
    h.handle.push_user_message(ConversationItem::user("u2"));

    let req2 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

    assert_prefix_stable_pair(&req1, &req2, "Reasoning sibling turn 1 -> turn 2");

    // Push another turn's Reasoning + Assistant
    h.handle
        .push_tool_result(reasoning_sibling("r_def", Some("enc2")));
    h.handle
        .push_assistant_response(ConversationItem::assistant("response 2"));
    h.handle.push_user_message(ConversationItem::user("u3"));

    let req3 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

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

    let req1 = h
        .handle
        .build_request("test-timeline", tools_v1, None, None, None)
        .await
        .unwrap();

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

    let req2 = h
        .handle
        .build_request("test-timeline", tools_v2, None, None, None)
        .await
        .unwrap();

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

    let req1 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

    h.handle
        .push_assistant_response(ConversationItem::assistant("hi"));
    h.handle
        .push_user_message(ConversationItem::user("continue"));

    let new_config = SamplingConfig {
        model: "grow-3-mini".to_string(),
        ..test_config()
    };
    h.handle.replace_sampling_route(new_config);

    let req2 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

    assert_prefix_stable_pair(&req1, &req2, "model switch");
}

fn native_fragment_for(
    backend: &sampling_types::ApiBackend,
) -> sampling_types::NativeContinuationFragment {
    use sampling_types::{NativeContinuationFragment, messages, rs};

    match backend {
        sampling_types::ApiBackend::ChatCompletions => NativeContinuationFragment::ChatCompletions(
            sampling_types::ChatRequestMessage::assistant(
                "answer",
                Some("native_secret".to_owned()),
            ),
        ),
        sampling_types::ApiBackend::Responses => NativeContinuationFragment::Responses(vec![
            rs::InputItem::Item(rs::Item::Reasoning(rs::ReasoningItem {
                id: Some("reasoning_native_id".to_owned()),
                summary: Vec::new(),
                content: None,
                encrypted_content: Some("native_secret".to_owned()),
                status: None,
            })),
            rs::InputItem::EasyMessage(rs::EasyInputMessage {
                r#type: rs::MessageType::Message,
                role: rs::Role::Assistant,
                content: rs::EasyInputContent::Text("answer".to_owned()),
                phase: None,
            }),
        ]),
        sampling_types::ApiBackend::Messages => NativeContinuationFragment::Messages(vec![
            messages::ContentBlock::Thinking {
                thinking: "visible thought".to_owned(),
                signature: "native_secret".to_owned(),
            },
            messages::ContentBlock::Text {
                text: "answer".to_owned(),
                cache_control: None,
            },
        ]),
    }
}

fn wire_request_json(
    request: &sampling_types::ConversationRequest,
    backend: &sampling_types::ApiBackend,
) -> serde_json::Value {
    match backend {
        sampling_types::ApiBackend::ChatCompletions => {
            serde_json::to_value(sampling_types::ChatCompletionRequest::from(request.clone()))
                .unwrap()
        }
        sampling_types::ApiBackend::Responses => {
            serde_json::to_value(sampling_types::rs::CreateResponse::from(request)).unwrap()
        }
        sampling_types::ApiBackend::Messages => {
            serde_json::to_value(sampling_types::build_messages_request(request)).unwrap()
        }
    }
}

#[tokio::test]
async fn response_without_native_keeps_tool_results_in_the_portable_prefix() {
    use sampling_types::{ApiBackend, ContentPart, ToolCall};
    for backend in [
        ApiBackend::ChatCompletions,
        ApiBackend::Responses,
        ApiBackend::Messages,
    ] {
        let h = TestHarness::with_config(
            vec![
                ConversationItem::system("sys"),
                ConversationItem::user("inspect"),
            ],
            SamplingConfig {
                api_backend: backend.clone(),
                ..test_config()
            },
        );
        h.handle
            .build_request("unsigned", vec![], None, None, None)
            .await
            .unwrap();
        h.handle
            .push_response_durably(
                vec![
                    ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                        "unsigned thought",
                    )),
                    ConversationItem::assistant_tool_calls(vec![
                        ToolCall {
                            id: "call_a".into(),
                            name: "read_file".into(),
                            arguments: r#"{"path":"a.png"}"#.into(),
                        },
                        ToolCall {
                            id: "call_b".into(),
                            name: "read_file".into(),
                            arguments: r#"{"path":"b.txt"}"#.into(),
                        },
                    ]),
                ],
                None,
            )
            .await
            .unwrap();
        h.handle
            .push_tool_result(ConversationItem::tool_result("call_b", "plain result"));
        h.handle
            .push_tool_result(ConversationItem::tool_result_with_images(
                "call_a",
                "image result",
                vec![ContentPart::Image {
                    description: None,
                    url: "data:image/png;base64,aW1hZ2U=".into(),
                }],
            ));
        let request = h
            .handle
            .build_request("unsigned", vec![], None, None, None)
            .await
            .unwrap();
        assert!(!request.has_native_continuation());
        let wire = wire_request_json(&request, &backend).to_string();
        for id in ["call_a", "call_b"] {
            assert_eq!(
                wire.matches(id).count(),
                2,
                "{backend:?} needs one call and one result: {wire}"
            );
        }
        assert!(!wire.contains("unsigned thought"));
        assert!(!wire.contains("Historical tool exchange"));
        assert!(wire.contains("aW1hZ2U="));
        assert_eq!(
            request.source_projection.as_ref().unwrap()["portable_prefix_len"],
            request.items.len()
        );
        let mut fully_portable = request.clone();
        fully_portable
            .native_continuation
            .as_mut()
            .unwrap()
            .portable_prefix_len = request.items.len();
        assert_eq!(
            crate::estimate_request_input_tokens(&request),
            crate::estimate_request_input_tokens(&fully_portable)
        );
        assert!(
            crate::estimate_request_input_tokens(&request)
                >= token_estimation::IMAGE_TOKEN_ESTIMATE
        );
        let history = h.handle.get_conversation().await;
        assert_eq!(
            history
                .iter()
                .filter(|item| matches!(item, ConversationItem::ToolResult(_)))
                .count(),
            2
        );
        assert!(
            history
                .iter()
                .any(|item| item.text_content() == "unsigned thought")
        );
    }
}

/// Same-route native state is useful only inside one live sampling epoch. Every
/// one of the six cross-endpoint directions must rebuild portable history and
/// must not resurrect the source state after A -> B -> A.
#[tokio::test]
async fn endpoint_switch_matrix_strips_native_reasoning_and_diagnostics() {
    use sampling_types::{ApiBackend, AssistantItem, ToolCall};

    let backends = [
        ApiBackend::ChatCompletions,
        ApiBackend::Responses,
        ApiBackend::Messages,
    ];
    for source in &backends {
        for target in &backends {
            if source == target {
                continue;
            }
            let source_config = SamplingConfig {
                model: "source-model".to_owned(),
                api_backend: source.clone(),
                ..test_config()
            };
            let h = TestHarness::with_config(
                vec![
                    ConversationItem::system("sys"),
                    ConversationItem::user("q1"),
                    ConversationItem::assistant_tool_calls(vec![ToolCall {
                        id: "historical-call".into(),
                        name: "read_file".into(),
                        arguments: r#"{"path":"README.md"}"#.into(),
                    }]),
                    ConversationItem::tool_result("historical-call", "historical result"),
                ],
                source_config.clone(),
            );
            let _ = h
                .handle
                .build_request("switch-matrix", vec![], None, None, None)
                .await
                .unwrap();
            h.handle
                .push_response_durably(
                    vec![
                        ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                            "visible thought",
                        )),
                        ConversationItem::Assistant(AssistantItem {
                            content: "answer".into(),
                            tool_calls: Vec::new(),
                            model_id: Some("source-model".to_owned()),
                            model_fingerprint: Some("source-fingerprint".to_owned()),
                            reasoning_effort: Some(sampling_types::ReasoningEffort::High),
                            response_messages: Vec::new(),
                        }),
                    ],
                    Some(native_fragment_for(source)),
                )
                .await
                .unwrap();
            h.handle
                .push_user_message_durably(ConversationItem::user("q2"))
                .await
                .unwrap();

            let same_route = h
                .handle
                .build_request("switch-matrix", vec![], None, None, None)
                .await
                .unwrap();
            assert!(same_route.has_native_continuation());
            assert!(
                wire_request_json(&same_route, source)
                    .to_string()
                    .contains("native_secret")
            );
            let source_key = same_route.prompt_cache_key.clone().unwrap();

            h.handle.replace_sampling_route(SamplingConfig {
                model: "target-model".to_owned(),
                api_backend: target.clone(),
                ..test_config()
            });
            let switched = h
                .handle
                .build_request("switch-matrix", vec![], None, None, None)
                .await
                .unwrap();
            assert!(!switched.has_native_continuation());
            let switched_wire = wire_request_json(&switched, target).to_string();
            assert_eq!(switched_wire.matches("historical-call").count(), 2);
            assert!(switched_wire.contains("historical result"));
            assert!(!switched_wire.contains("Historical tool exchange"));
            for forbidden in [
                "native_secret",
                "reasoning_native_id",
                "visible thought",
                "source-model",
                "source-fingerprint",
                "model_id",
            ] {
                assert!(
                    !switched_wire.contains(forbidden),
                    "{source:?} -> {target:?} leaked {forbidden}: {switched_wire}"
                );
            }

            h.handle.replace_sampling_route(source_config);
            let switched_back = h
                .handle
                .build_request("switch-matrix", vec![], None, None, None)
                .await
                .unwrap();
            assert!(!switched_back.has_native_continuation());
            assert!(
                !wire_request_json(&switched_back, source)
                    .to_string()
                    .contains("native_secret")
            );
            assert_ne!(
                switched_back.prompt_cache_key.as_deref(),
                Some(source_key.as_str())
            );
        }
    }
}

#[tokio::test]
async fn sampling_config_updates_preserve_same_route_native_continuation() {
    let backend = sampling_types::ApiBackend::Responses;
    let h = TestHarness::with_config(
        vec![
            ConversationItem::system("sys"),
            ConversationItem::user("q1"),
        ],
        SamplingConfig {
            api_backend: backend.clone(),
            ..test_config()
        },
    );
    let _ = h
        .handle
        .build_request("limit-update", vec![], None, None, None)
        .await
        .unwrap();
    h.handle
        .push_response_durably(
            vec![ConversationItem::assistant("answer")],
            Some(native_fragment_for(&backend)),
        )
        .await
        .unwrap();
    h.handle
        .push_user_message_durably(ConversationItem::user("q2"))
        .await
        .unwrap();

    let before = h
        .handle
        .build_request("limit-update", vec![], None, None, None)
        .await
        .unwrap();
    assert!(before.has_native_continuation());
    let cache_key = before.prompt_cache_key.clone();

    let mut updated = h.handle.get_sampling_config().await.unwrap();
    updated.context_window = NonZeroU64::new(256_000).unwrap();
    updated.output_limit = Some(8192);
    updated.reasoning_effort = Some(sampling_types::ReasoningEffort::High);
    h.handle.update_sampling_config(updated);
    let after = h
        .handle
        .build_request("limit-update", vec![], None, None, None)
        .await
        .unwrap();
    let config = h.handle.get_sampling_config().await.unwrap();

    assert!(after.has_native_continuation());
    assert_eq!(after.prompt_cache_key, cache_key);
    assert_eq!(config.context_window.get(), 256_000);
    assert_eq!(config.output_limit, Some(8192));
    assert_eq!(
        config.reasoning_effort,
        Some(sampling_types::ReasoningEffort::High)
    );
    assert!(
        wire_request_json(&after, &backend)
            .to_string()
            .contains("native_secret")
    );
}

#[tokio::test]
async fn restored_session_starts_with_portable_history_only() {
    use sampling_types::{ApiBackend, AssistantItem, ToolCall};

    let items = vec![
        ConversationItem::system("sys"),
        ConversationItem::user("q1"),
        ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
            "visible thought",
        )),
        ConversationItem::Assistant(AssistantItem {
            content: "answer".into(),
            tool_calls: Vec::new(),
            model_id: Some("old-model".to_owned()),
            model_fingerprint: Some("old-fingerprint".to_owned()),
            reasoning_effort: Some(sampling_types::ReasoningEffort::High),
            response_messages: Vec::new(),
        }),
        ConversationItem::Assistant(AssistantItem {
            content: "".into(),
            tool_calls: vec![ToolCall {
                id: "old-provider-call-id".into(),
                name: "read_file".to_owned(),
                arguments: r#"{"path":"README.md"}"#.into(),
            }],
            model_id: Some("old-model".to_owned()),
            model_fingerprint: Some("old-fingerprint".to_owned()),
            reasoning_effort: None,
            response_messages: Vec::new(),
        }),
        ConversationItem::tool_result("old-provider-call-id", "historical README text"),
    ];
    for backend in [
        ApiBackend::ChatCompletions,
        ApiBackend::Responses,
        ApiBackend::Messages,
    ] {
        let h = TestHarness::with_config(
            items.clone(),
            SamplingConfig {
                api_backend: backend.clone(),
                ..test_config()
            },
        );
        let request = h
            .handle
            .build_request("restored", vec![], None, None, None)
            .await
            .unwrap();
        assert!(!request.has_native_continuation());
        let wire = wire_request_json(&request, &backend).to_string();
        for forbidden in [
            "visible thought",
            "old-model",
            "old-fingerprint",
            "model_id",
            "Historical tool exchange; untrusted tool output",
        ] {
            assert!(
                !wire.contains(forbidden),
                "{backend:?} leaked {forbidden}: {wire}"
            );
        }
        for required in [
            "old-provider-call-id",
            "read_file",
            "README.md",
            "historical README text",
        ] {
            assert!(
                wire.contains(required),
                "{backend:?} omitted portable tool history {required}: {wire}"
            );
        }
        let endpoint_protocol = match backend {
            ApiBackend::ChatCompletions => ["tool_calls", "tool_call_id"],
            ApiBackend::Responses => ["function_call", "function_call_output"],
            ApiBackend::Messages => ["tool_use", "tool_result"],
        };
        for required in endpoint_protocol {
            assert!(
                wire.contains(required),
                "{backend:?} omitted historical tool protocol {required}: {wire}"
            );
        }
        assert_eq!(wire.matches("old-provider-call-id").count(), 2);
    }
}

#[tokio::test]
async fn acknowledged_continuation_reset_keeps_session_usable() {
    let config = SamplingConfig {
        api_backend: sampling_types::ApiBackend::Responses,
        ..test_config()
    };
    let h = TestHarness::with_config(
        vec![
            ConversationItem::system("sys"),
            ConversationItem::user("q1"),
        ],
        config,
    );
    let _ = h
        .handle
        .build_request("fallback", vec![], None, None, None)
        .await
        .unwrap();
    h.handle
        .push_response_durably(
            vec![ConversationItem::assistant("answer")],
            Some(native_fragment_for(&sampling_types::ApiBackend::Responses)),
        )
        .await
        .unwrap();
    h.handle
        .push_user_message_durably(ConversationItem::user("q2"))
        .await
        .unwrap();
    let native = h
        .handle
        .build_request("fallback", vec![], None, None, None)
        .await
        .unwrap();
    assert!(native.has_native_continuation());
    let native_key = native.prompt_cache_key.unwrap();

    assert!(h.handle.reset_continuation().await);
    let portable = h
        .handle
        .build_request("fallback", vec![], None, None, None)
        .await
        .unwrap();
    assert!(!portable.has_native_continuation());
    assert_ne!(
        portable.prompt_cache_key.as_deref(),
        Some(native_key.as_str())
    );
    assert!(
        !wire_request_json(&portable, &sampling_types::ApiBackend::Responses)
            .to_string()
            .contains("native_secret")
    );
    assert!(!h.handle.is_closed());
}

#[tokio::test]
async fn reasoning_replay_is_route_local_and_acknowledged() {
    use sampling_types::{ApiBackend, ToolCall};

    for backend in [ApiBackend::Responses, ApiBackend::ChatCompletions] {
        let reasoning_text = "reasoning required after queued route switch";
        let responses_config = SamplingConfig {
            api_backend: backend.clone(),
            model: "responses-a".into(),
            ..test_config()
        };
        let h = TestHarness::with_config(
            vec![
                ConversationItem::system("sys"),
                ConversationItem::user("inspect"),
                ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                    reasoning_text,
                )),
                ConversationItem::assistant_tool_calls(vec![ToolCall {
                    id: "queued-switch-call".into(),
                    name: "read_file".into(),
                    arguments: r#"{"path":"a"}"#.into(),
                }]),
                ConversationItem::tool_result("queued-switch-call", "done"),
            ],
            responses_config.clone(),
        );

        let before = h
            .handle
            .build_request("reasoning-replay", vec![], None, None, None)
            .await
            .unwrap();
        assert!(
            !wire_request_json(&before, &backend)
                .to_string()
                .contains(reasoning_text)
        );

        let wrong_backend = if backend == ApiBackend::Responses {
            ApiBackend::ChatCompletions
        } else {
            ApiBackend::Responses
        };
        assert_eq!(
            h.handle.enable_portable_reasoning(wrong_backend).await,
            Some(false)
        );
        assert_eq!(
            h.handle.enable_portable_reasoning(backend.clone()).await,
            Some(true)
        );
        assert_eq!(
            h.handle.enable_portable_reasoning(backend.clone()).await,
            Some(false),
            "the compatibility state may change only once per route"
        );
        let enabled = h
            .handle
            .build_request("reasoning-replay", vec![], None, None, None)
            .await
            .unwrap();
        let enabled_wire = wire_request_json(&enabled, &backend);
        if backend == ApiBackend::Responses {
            let reasoning = enabled_wire["input"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["type"] == "reasoning")
                .unwrap();
            assert_eq!(reasoning["content"][0]["type"], "reasoning_text");
            assert_eq!(reasoning["content"][0]["text"], reasoning_text);
        } else {
            assert_eq!(
                enabled_wire["messages"][2]["reasoning_content"],
                reasoning_text
            );
        }
        assert_eq!(
            enabled.source_projection.as_ref().unwrap()["portable_reasoning_backend"],
            serde_json::to_value(&backend).unwrap()
        );
        assert!(
            crate::estimate_request_input_tokens(&enabled)
                > crate::estimate_request_input_tokens(&before)
        );
        assert_eq!(
            serde_json::to_value(&enabled.items).unwrap(),
            serde_json::to_value(&before.items).unwrap()
        );
        h.handle.update_sampling_config(responses_config.clone());

        assert!(h.handle.reset_continuation().await);
        let after_reset = h
            .handle
            .build_request("reasoning-replay", vec![], None, None, None)
            .await
            .unwrap();
        assert!(
            wire_request_json(&after_reset, &backend)
                .to_string()
                .contains(reasoning_text),
            "same-route native reset must preserve the learned compatibility"
        );

        h.handle.replace_sampling_route(SamplingConfig {
            model: "responses-b".into(),
            ..responses_config
        });
        let replaced = h
            .handle
            .build_request("reasoning-replay", vec![], None, None, None)
            .await
            .unwrap();
        assert!(
            !wire_request_json(&replaced, &backend)
                .to_string()
                .contains(reasoning_text),
            "a real route replacement must not leak learned compatibility"
        );
    }
}

#[tokio::test]
async fn chat_reasoning_replay_requires_an_assistant_but_not_invented_reasoning() {
    use sampling_types::ApiBackend;
    for backend in [
        ApiBackend::ChatCompletions,
        ApiBackend::Responses,
        ApiBackend::Messages,
    ] {
        let h = TestHarness::with_config(
            vec![ConversationItem::user("question")],
            SamplingConfig {
                api_backend: backend.clone(),
                ..test_config()
            },
        );
        assert_eq!(
            h.handle.enable_portable_reasoning(backend.clone()).await,
            Some(false)
        );
        h.handle
            .push_assistant_response(ConversationItem::assistant("plain history"));
        assert!(h.handle.reset_continuation().await);
        assert_eq!(
            h.handle.enable_portable_reasoning(backend.clone()).await,
            Some(backend == ApiBackend::ChatCompletions)
        );
        if backend == ApiBackend::ChatCompletions {
            let request = h
                .handle
                .build_request("plain-history", vec![], None, None, None)
                .await
                .unwrap();
            let wire = wire_request_json(&request, &backend);
            assert_eq!(wire["messages"][1]["reasoning_content"], "");
        }
    }
}

#[tokio::test]
async fn responses_reasoning_replay_requires_a_complete_portable_tool_exchange() {
    let h = TestHarness::with_config(
        vec![
            ConversationItem::user("question"),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "standalone final-answer reasoning",
            )),
            ConversationItem::assistant("answer"),
        ],
        SamplingConfig {
            api_backend: sampling_types::ApiBackend::Responses,
            ..test_config()
        },
    );

    assert_eq!(
        h.handle
            .enable_portable_reasoning(sampling_types::ApiBackend::Responses)
            .await,
        Some(false)
    );
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

    h.handle.replace_sampling_route(SamplingConfig {
        model: "other-model".into(),
        ..test_config()
    });
    let other_model = h
        .handle
        .build_request("timeline-a", vec![], None, None, None)
        .await
        .unwrap();
    assert_ne!(
        other_model.prompt_cache_key.as_deref(),
        Some(first_key.as_str())
    );

    h.handle.replace_sampling_route(test_config());
    h.handle.rewind_durably(1).await.unwrap();
    let rewound = h
        .handle
        .build_request("timeline-a", vec![], None, None, None)
        .await
        .unwrap();
    assert_ne!(
        rewound.prompt_cache_key.as_deref(),
        Some(first_key.as_str())
    );

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

    let req1 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

    h.handle
        .push_assistant_response(ConversationItem::assistant("hi"));

    h.handle
        .push_user_message(ConversationItem::system_reminder("stop looping"));
    h.handle
        .push_assistant_response(ConversationItem::assistant("ok"));
    h.handle
        .push_user_message(ConversationItem::auto_continue("keep going"));

    let req2 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

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
                    description: None,
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

    let req1 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

    h.handle
        .push_assistant_response(ConversationItem::assistant("a2"));
    h.handle
        .push_user_message(ConversationItem::User(sampling_types::UserItem {
            content: vec![
                ContentPart::Text {
                    text: "new image".into(),
                },
                ContentPart::Image {
                    description: None,
                    url: "data:image/png;base64,newImageData".into(),
                },
            ],
            synthetic_reason: None,
            permission_evidence: None,
            ..Default::default()
        }));

    let req2 = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

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
                    description: None,
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

    let req = h
        .handle
        .build_request("test-timeline", vec![], None, None, None)
        .await
        .unwrap();

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

#[tokio::test]
async fn image_budget_selection_survives_append_until_next_high_water_mark() {
    use sampling_types::ContentPart;

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
    fn selected(request: &sampling_types::ConversationRequest, index: usize) -> bool {
        matches!(
            &request.items[index],
            ConversationItem::User(user)
                if user.content.iter().any(|part| matches!(part, ContentPart::Image { .. }))
        )
    }

    let h = TestHarness::with_conversation((0..4).map(image).collect());
    let first = h
        .handle
        .build_request("image-selection", vec![], None, None, None)
        .await
        .unwrap();
    assert!(!selected(&first, 0));
    assert!(!selected(&first, 1));
    assert!(selected(&first, 2));
    assert!(selected(&first, 3));
    assert_eq!(
        first.source_projection.as_ref().unwrap()["image_budget"]["evicted_parts"],
        serde_json::json!([[0, 1], [0, 2], [1, 1], [1, 2]])
    );

    let repeated = h
        .handle
        .build_request("image-selection", vec![], None, None, None)
        .await
        .unwrap();
    assert_eq!(
        repeated.source_projection.as_ref().unwrap()["image_budget"]["evicted_parts"],
        first.source_projection.as_ref().unwrap()["image_budget"]["evicted_parts"]
    );
    assert_eq!(
        repeated.source_projection.as_ref().unwrap()["image_budget"]["newly_evicted_parts"],
        serde_json::Value::Null
    );

    h.handle.push_user_message(image(4));
    let appended = h
        .handle
        .build_request("image-selection", vec![], None, None, None)
        .await
        .unwrap();
    assert!(
        selected(&appended, 2),
        "old selection must retain the third image"
    );
    assert_eq!(
        appended.source_projection.as_ref().unwrap()["image_budget"]["evicted_parts"],
        serde_json::json!([[0, 1], [0, 2], [1, 1], [1, 2]])
    );
    assert_eq!(first.prompt_cache_key, appended.prompt_cache_key);
    assert_eq!(
        first.source_projection.as_ref().unwrap()["continuation_epoch"],
        appended.source_projection.as_ref().unwrap()["continuation_epoch"]
    );

    // A new actor starts from immutable source evidence, not this actor's
    // request-only selection. At five images its first budget pass differs.
    let (persistence, _records) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let cold = ChatStateActor::spawn_from_timeline(
        h.handle.timeline_events().await.unwrap(),
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let cold_request = cold
        .build_request("image-selection", vec![], None, None, None)
        .await
        .unwrap();
    assert!(!selected(&cold_request, 2));
    assert!(selected(&cold_request, 4));

    h.handle
        .push_user_message(ConversationItem::user("text-only append"));
    let text_append = h
        .handle
        .build_request("image-selection", vec![], None, None, None)
        .await
        .unwrap();
    assert!(selected(&text_append, 2));

    h.handle.push_user_message(image(5));
    let next_boundary = h
        .handle
        .build_request("image-selection", vec![], None, None, None)
        .await
        .unwrap();
    assert!(!selected(&next_boundary, 2));
    assert!(!selected(&next_boundary, 3));
    assert!(selected(&next_boundary, 4));
    assert!(selected(&next_boundary, 6));
    h.handle
        .push_response_durably(vec![ConversationItem::assistant("ok")], None)
        .await
        .unwrap();
    let after_native_reset = h
        .handle
        .build_request("image-selection", vec![], None, None, None)
        .await
        .unwrap();
    assert_eq!(
        after_native_reset.source_projection.as_ref().unwrap()["image_budget"]["evicted_parts"],
        next_boundary.source_projection.as_ref().unwrap()["image_budget"]["evicted_parts"]
    );
    assert!(selected(&after_native_reset, 4));
    assert!(h.handle.get_conversation().await.iter().any(|item| matches!(item,
        ConversationItem::User(user) if user.content.iter().any(|part| matches!(part, ContentPart::Image { .. }))
    )));
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

/// An under-reported provider anchor is ignored, then pruning applies the
/// signed Surface delta while retaining the bounded replacement marker.
#[tokio::test]
async fn prune_tool_results_clamps_after_underreported_anchor_is_ignored() {
    use crate::actor::state::EstimatedItemTokenCounter;
    use compaction::plan_tool_result_pruning;

    let conv = vec![ConversationItem::tool_result("call-1", "x".repeat(4000))];
    let mut h = TestHarness::with_conversation(conv.clone());
    h.handle.record_provider_context_anchor(10);
    // Sync point: the query is ordered after the fire-and-forget anchor.
    assert_eq!(h.handle.get_projected_tokens().await, 1000);
    h.drain_events();

    let plan = plan_tool_result_pruning(&conv, &EstimatedItemTokenCounter, 50, 100);
    let report = h
        .handle
        .prune_tool_results(plan)
        .await
        .expect("prune succeeds");
    assert_eq!(report.pruned_count, 1, "content is still pruned");
    assert_eq!(report.tokens_before, 1000);
    assert!(report.tokens_after < report.tokens_before);
    assert_eq!(h.handle.get_projected_tokens().await, report.tokens_after);
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

#[tokio::test]
async fn durable_agent_message_received_is_hidden_until_ack() {
    let mut h = TestHarness::with_manual_timeline_ack(vec![]);
    let source = crate::NotificationSource::ParentMessage {
        parent_session_id: "parent".into(),
        message_id: "message-1".into(),
        interrupt: false,
    };
    let version = crate::NotificationSourceVersion::Ordinal {
        value: crate::PARENT_MESSAGE_SOURCE_VERSION,
    };
    let payload = crate::NotificationPayloadRef {
        blake3: blake3::hash(b"shared body").to_hex().to_string(),
        bytes: 11,
    };
    let handle = h.handle.clone();
    let receive = tokio::spawn(async move {
        handle
            .receive_notification_durably("session-1".into(), source, version, payload)
            .await
    });

    let ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("Received persistence acknowledgement");
    assert!(
        !receive.is_finished(),
        "Received must not be reported before its durable acknowledgement"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(30), h.handle.get_conversation())
            .await
            .is_err(),
        "the actor must not expose a Surface while Received is awaiting its acknowledgement"
    );
    assert!(
        h.drain_events().is_empty(),
        "an uncommitted Received must not publish a Surface event"
    );

    ack.send(Ok(()))
        .expect("actor is awaiting the acknowledgement");
    let receipt = receive
        .await
        .expect("receive task completes")
        .expect("Received commits after acknowledgement");
    assert!(matches!(
        receipt.kind,
        crate::TimelineEventKind::Notification(crate::NotificationEvent::Received { .. })
    ));
    assert!(
        h.handle.get_conversation().await.is_empty(),
        "a Received receipt alone is not model-visible context"
    );
    let receipts = h.handle.parent_message_receipts().await.unwrap();
    assert_eq!(receipts.len(), 1);
}

#[tokio::test]
async fn durable_agent_message_receive_retries_lost_and_failed_ack_without_duplicate_receipts() {
    let mut h = TestHarness::with_manual_timeline_ack(vec![]);

    let receive_lost = {
        let handle = h.handle.clone();
        tokio::spawn(async move {
            handle
                .receive_notification_durably(
                    "session-1".into(),
                    crate::NotificationSource::ParentMessage {
                        parent_session_id: "parent".into(),
                        message_id: "lost-ack".into(),
                        interrupt: false,
                    },
                    crate::NotificationSourceVersion::Ordinal {
                        value: crate::PARENT_MESSAGE_SOURCE_VERSION,
                    },
                    crate::NotificationPayloadRef {
                        blake3: blake3::hash(b"lost").to_hex().to_string(),
                        bytes: 4,
                    },
                )
                .await
        })
    };
    let first_lost_ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("first Received acknowledgement");
    let first_lost_event = h
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("first Received persistence record");
    drop(first_lost_ack);
    let retry_lost_ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("retry Received acknowledgement after a lost acknowledgement");
    let retry_lost_event = h
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("retry Received persistence record");
    assert_eq!(
        serde_json::to_vec(&first_lost_event).unwrap(),
        serde_json::to_vec(&retry_lost_event).unwrap(),
        "a lost acknowledgement retries the exact immutable event"
    );
    retry_lost_ack
        .send(Ok(()))
        .expect("actor is awaiting the retry acknowledgement");
    let lost_receipt = receive_lost
        .await
        .expect("lost acknowledgement receive task completes")
        .expect("retry after a lost acknowledgement commits");

    let receive_failed = {
        let handle = h.handle.clone();
        tokio::spawn(async move {
            handle
                .receive_notification_durably(
                    "session-1".into(),
                    crate::NotificationSource::ParentMessage {
                        parent_session_id: "parent".into(),
                        message_id: "failed-ack".into(),
                        interrupt: false,
                    },
                    crate::NotificationSourceVersion::Ordinal {
                        value: crate::PARENT_MESSAGE_SOURCE_VERSION,
                    },
                    crate::NotificationPayloadRef {
                        blake3: blake3::hash(b"failed").to_hex().to_string(),
                        bytes: 6,
                    },
                )
                .await
        })
    };
    let first_failed_ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("first failed Received acknowledgement");
    let first_failed_event = h
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("first failed Received persistence record");
    first_failed_ack
        .send(Err(std::io::Error::other("simulated disk failure")))
        .expect("actor is awaiting the failed acknowledgement");
    let retry_failed_ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("retry Received acknowledgement after a failed acknowledgement");
    let retry_failed_event = h
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("retry failed Received persistence record");
    assert_eq!(
        serde_json::to_vec(&first_failed_event).unwrap(),
        serde_json::to_vec(&retry_failed_event).unwrap(),
        "a failed acknowledgement retries the exact immutable event"
    );
    retry_failed_ack
        .send(Ok(()))
        .expect("actor is awaiting the retry acknowledgement");
    let failed_receipt = receive_failed
        .await
        .expect("failed acknowledgement receive task completes")
        .expect("retry after a failed acknowledgement commits");

    assert_ne!(lost_receipt.seq, failed_receipt.seq);
    let receipts = h.handle.parent_message_receipts().await.unwrap();
    assert_eq!(
        receipts.len(),
        2,
        "retries must not append a second receipt for either source"
    );
    assert_eq!(h.handle.pending_notifications().await.unwrap().len(), 2);
}

#[tokio::test]
async fn source_tool_result_ack_loss_does_not_redeliver_received_message() {
    let send_call = ConversationItem::assistant_tool_calls(vec![sampling_types::ToolCall {
        id: "send-call".into(),
        name: "send_subagent_message".into(),
        arguments: serde_json::json!({
            "subagent_id": "target-session",
            "message": "shared body",
            "interrupt": false,
        })
        .to_string()
        .into(),
    }]);
    let mut source = TestHarness::with_manual_timeline_ack_after(vec![], 1);
    source.handle.push_assistant_response(send_call);
    let mut target = TestHarness::new();
    let source_session_id = "source-session";
    let owner_session_id = "target-session";
    let source_notification = crate::NotificationSource::ParentMessage {
        parent_session_id: source_session_id.into(),
        message_id: "send-call".into(),
        interrupt: false,
    };
    let source_version = crate::NotificationSourceVersion::Ordinal {
        value: crate::PARENT_MESSAGE_SOURCE_VERSION,
    };
    let payload = crate::NotificationPayloadRef {
        blake3: blake3::hash(b"shared body").to_hex().to_string(),
        bytes: 11,
    };

    // Admit the live send call before driving its result acknowledgement;
    // loading an incomplete historical call would correctly repair it first.
    assert_eq!(source.handle.get_conversation().await.len(), 1);
    assert_eq!(source.drain_persistence().len(), 1);

    let target_receipt = target
        .handle
        .receive_notification_durably(
            owner_session_id.into(),
            source_notification.clone(),
            source_version.clone(),
            payload.clone(),
        )
        .await
        .expect("target Received must be durable before source result handling");
    let target_receipt_id = match &target_receipt.kind {
        crate::TimelineEventKind::Notification(crate::NotificationEvent::Received {
            id, ..
        }) => id.clone(),
        _ => panic!("target must return a Received event"),
    };
    let target_records = target.drain_persistence();
    assert_eq!(
        target_records
            .iter()
            .filter(|record| matches!(record, PersistenceRecord::Timeline(_)))
            .count(),
        1,
        "the target must persist exactly one Received event"
    );

    let tool_result = ConversationItem::tool_result(
        "send-call",
        serde_json::json!({
            "id": "send-call",
            "status": "received",
            "receipt_id": target_receipt_id,
        })
        .to_string(),
    );
    let source_handle = source.handle.clone();
    let push =
        tokio::spawn(async move { source_handle.push_tool_result_durably(tool_result).await });

    let first_ack = source
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("source ToolResult persistence acknowledgement");
    assert!(
        !push.is_finished(),
        "source must not expose the ToolResult before its durable acknowledgement"
    );
    let first_event = source
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("first source ToolResult event");
    first_ack
        .send(Err(std::io::Error::other("source result write failed")))
        .expect("source actor is awaiting the first acknowledgement");

    let second_ack = source
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("source retries the same ToolResult after failure");
    let second_event = source
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("retried source ToolResult event");
    drop(second_ack);

    let third_ack = source
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("source retries after the lost acknowledgement");
    let third_event = source
        .drain_persistence()
        .into_iter()
        .find_map(|record| match record {
            PersistenceRecord::Timeline(event) => Some(event),
            PersistenceRecord::Flush => None,
        })
        .expect("second retried source ToolResult event");
    third_ack
        .send(Ok(()))
        .expect("source actor is awaiting the final acknowledgement");
    push.await
        .expect("source ToolResult task should finish")
        .expect("the exact result event should recover after retries");

    assert_eq!(
        serde_json::to_value(&first_event).unwrap(),
        serde_json::to_value(&second_event).unwrap(),
        "a failed source result write must retry the exact immutable event"
    );
    assert_eq!(
        serde_json::to_value(&second_event).unwrap(),
        serde_json::to_value(&third_event).unwrap(),
        "a lost source result acknowledgement must retry the exact immutable event"
    );

    let source_surface = source.handle.get_conversation().await;
    let ConversationItem::ToolResult(result) = &source_surface[1] else {
        panic!("source must expose the persisted send ToolResult after its final ACK");
    };
    let result_json: serde_json::Value =
        serde_json::from_str(result.content.as_ref()).expect("typed send result JSON");
    assert_eq!(result_json["status"], "received");
    assert_eq!(result_json["receipt_id"], target_receipt_id);

    let retry = target
        .handle
        .receive_notification_durably(
            owner_session_id.into(),
            source_notification,
            source_version,
            payload,
        )
        .await
        .expect("same target identity can recheck its existing receipt");
    assert_eq!(retry.seq, target_receipt.seq);
    let receipts = target
        .handle
        .parent_message_receipts()
        .await
        .expect("target receipt index");
    assert_eq!(receipts.len(), 1);
    assert!(
        matches!(
            &receipts[0].kind,
            crate::TimelineEventKind::Notification(crate::NotificationEvent::Received { id, .. })
                if id == &target_receipt_id
        ),
        "source result persistence must not create a second target receipt"
    );
    assert!(
        target.drain_persistence().is_empty(),
        "receipt recheck must not redeliver or append a second Received event"
    );
}

#[tokio::test]
async fn durable_agent_message_receive_rejects_body_and_interrupt_conflicts() {
    let h = TestHarness::new();
    let source = crate::NotificationSource::ParentMessage {
        parent_session_id: "parent".into(),
        message_id: "message-1".into(),
        interrupt: false,
    };
    let version = crate::NotificationSourceVersion::Ordinal {
        value: crate::PARENT_MESSAGE_SOURCE_VERSION,
    };
    let payload = crate::NotificationPayloadRef {
        blake3: blake3::hash(b"shared body").to_hex().to_string(),
        bytes: 11,
    };
    let first = h
        .handle
        .receive_notification_durably(
            "session-1".into(),
            source.clone(),
            version.clone(),
            payload.clone(),
        )
        .await
        .expect("first receipt commits");
    let duplicate = h
        .handle
        .receive_notification_durably(
            "session-1".into(),
            source.clone(),
            version.clone(),
            payload.clone(),
        )
        .await
        .expect("exact retry is idempotent");
    assert_eq!(duplicate.seq, first.seq);

    let body_conflict = h
        .handle
        .receive_notification_durably(
            "session-1".into(),
            source.clone(),
            version.clone(),
            crate::NotificationPayloadRef {
                blake3: blake3::hash(b"changed body").to_hex().to_string(),
                bytes: 12,
            },
        )
        .await
        .expect_err("same source and message id cannot change the body");
    assert!(matches!(
        body_conflict,
        crate::commands::TimelineWriteError::Invalid(crate::TimelineError::InvalidNotification)
    ));

    let interrupt_conflict = h
        .handle
        .receive_notification_durably(
            "session-1".into(),
            crate::NotificationSource::ParentMessage {
                parent_session_id: "parent".into(),
                message_id: "message-1".into(),
                interrupt: true,
            },
            version,
            payload,
        )
        .await
        .expect_err("same source and message id cannot change interrupt semantics");
    assert!(matches!(
        interrupt_conflict,
        crate::commands::TimelineWriteError::Invalid(crate::TimelineError::InvalidNotification)
    ));
    assert_eq!(h.handle.parent_message_receipts().await.unwrap().len(), 1);
    assert_eq!(h.handle.pending_notifications().await.unwrap().len(), 1);
}

#[tokio::test]
async fn durable_notification_receive_is_idempotent_and_rejects_payload_conflicts() {
    let h = TestHarness::new();
    let source = crate::NotificationSource::TaskCompleted {
        task_id: "task-1".into(),
        task_kind: crate::NotificationTaskKind::Task,
        owner: crate::NotificationOwner::Goal {
            goal_id: "goal-1".into(),
            definition_revision: 1,
        },
    };
    let version = crate::NotificationSourceVersion::Ordinal { value: 1 };
    let payload = crate::NotificationPayloadRef {
        blake3: blake3::hash(b"done").to_hex().to_string(),
        bytes: 4,
    };
    assert_eq!(
        h.handle
            .received_notification_id(source.clone(), version.clone())
            .await,
        Some(None)
    );
    let first = h
        .handle
        .receive_notification_durably(
            "session-1".into(),
            source.clone(),
            version.clone(),
            payload.clone(),
        )
        .await
        .expect("first receipt commits");
    let first_id = match &first.kind {
        crate::TimelineEventKind::Notification(crate::NotificationEvent::Received {
            id, ..
        }) => id.clone(),
        _ => panic!("expected notification receipt"),
    };
    assert_eq!(
        h.handle
            .received_notification_id(source.clone(), version.clone())
            .await,
        Some(Some(first_id))
    );
    let duplicate = h
        .handle
        .receive_notification_durably("session-1".into(), source.clone(), version.clone(), payload)
        .await
        .expect("exact retry is idempotent");
    assert_eq!(duplicate.seq, first.seq);
    let ownerless_retry = h
        .handle
        .receive_notification_durably(
            "session-1".into(),
            crate::NotificationSource::TaskCompleted {
                task_id: "task-1".into(),
                task_kind: crate::NotificationTaskKind::Task,
                owner: crate::NotificationOwner::Session,
            },
            version.clone(),
            crate::NotificationPayloadRef {
                blake3: blake3::hash(b"done").to_hex().to_string(),
                bytes: 4,
            },
        )
        .await
        .expect("retry after owner memory loss resolves to the original receipt");
    assert_eq!(ownerless_retry.seq, first.seq);
    assert_eq!(
        h.handle.pending_notifications().await.unwrap().len(),
        1,
        "an exact retry must not append a second receipt"
    );

    let conflict = h
        .handle
        .receive_notification_durably(
            "session-1".into(),
            source,
            version,
            crate::NotificationPayloadRef {
                blake3: blake3::hash(b"different").to_hex().to_string(),
                bytes: 9,
            },
        )
        .await
        .expect_err("same source version cannot change payload");
    assert!(matches!(
        conflict,
        crate::commands::TimelineWriteError::Invalid(crate::TimelineError::InvalidNotification)
    ));
}

#[tokio::test]
async fn durable_input_submission_is_idempotent_and_rejects_payload_conflicts() {
    let h = TestHarness::new();
    let payload = crate::InputPayloadRef {
        blake3: blake3::hash(b"hello").to_hex().to_string(),
        bytes: 5,
    };
    let first = h
        .handle
        .submit_input_durably(
            "input-1".into(),
            crate::InputIntent::Prompt,
            payload.clone(),
        )
        .await
        .expect("first input submission commits");
    let duplicate = h
        .handle
        .submit_input_durably(
            "input-1".into(),
            crate::InputIntent::Prompt,
            payload.clone(),
        )
        .await
        .expect("exact retry is idempotent");
    assert!(first.created);
    assert!(!duplicate.created);
    assert_eq!(duplicate.event.seq, first.event.seq);
    assert_eq!(
        h.handle
            .timeline_events()
            .await
            .unwrap()
            .iter()
            .filter(|event| matches!(
                &event.kind,
                crate::TimelineEventKind::Input(crate::InputEvent::Submitted { .. })
            ))
            .count(),
        1
    );

    let conflict = h
        .handle
        .submit_input_durably(
            "input-1".into(),
            crate::InputIntent::Prompt,
            crate::InputPayloadRef {
                blake3: blake3::hash(b"changed").to_hex().to_string(),
                bytes: 7,
            },
        )
        .await
        .expect_err("an input ID cannot change payload");
    assert!(matches!(
        conflict,
        crate::commands::TimelineWriteError::Invalid(crate::TimelineError::InvalidInput)
    ));
}

#[tokio::test]
async fn live_tool_images_are_budgeted_without_changing_timeline_evidence() {
    for backend in [
        sampling_types::ApiBackend::ChatCompletions,
        sampling_types::ApiBackend::Responses,
        sampling_types::ApiBackend::Messages,
    ] {
        assert_live_tool_image_budget(backend).await;
    }
}

async fn assert_live_tool_image_budget(backend: sampling_types::ApiBackend) {
    use sampling_types::{ContentPart, NativeContinuationFragment, ToolCall};
    let mut config = test_config();
    config.api_backend = backend.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn(
        vec![
            ConversationItem::system("test"),
            ConversationItem::user("read images"),
        ],
        config,
        Box::new(crate::NullTimelinePersistence),
        tx,
        Default::default(),
    );
    handle
        .build_request("image-budget", vec![], None, None, None)
        .await
        .unwrap();
    let url: std::sync::Arc<str> =
        format!("data:image/png;base64,{}", "A".repeat(15 * 1024 * 1024 / 2)).into();
    let mut saw_eviction = false;
    for index in 0..5 {
        let id = format!("call_{index}");
        let assistant = ConversationItem::assistant_tool_calls(vec![ToolCall {
            id: id.clone().into(),
            name: "read_file".into(),
            arguments: "{}".into(),
        }]);
        let fragment = match &backend {
            sampling_types::ApiBackend::ChatCompletions => NativeContinuationFragment::ChatCompletions(
                sampling_types::conversation_item_to_chat_message(assistant.clone()),
            ),
            sampling_types::ApiBackend::Responses => NativeContinuationFragment::Responses(vec![
                serde_json::from_value(serde_json::json!({"type":"function_call", "call_id":id, "name":"read_file", "arguments":"{}"})).unwrap(),
            ]),
            sampling_types::ApiBackend::Messages => NativeContinuationFragment::Messages(vec![
                serde_json::from_value(serde_json::json!({"type":"tool_use", "id":id, "name":"read_file", "input":{}})).unwrap(),
            ]),
        };
        handle
            .push_response_durably(vec![assistant], Some(fragment))
            .await
            .unwrap();
        let marker = format!("image{index:03}");
        handle.push_tool_result(ConversationItem::tool_result_with_images(
            id,
            "Read image.",
            ["a", "b"]
                .into_iter()
                .map(|part| ContentPart::Image {
                    description: None,
                    url: format!("{url}{marker}{part}").into(),
                })
                .collect(),
        ));
        let request = handle
            .build_request("image-budget", vec![], None, None, None)
            .await
            .unwrap();
        if index == 0 {
            assert!(
                crate::estimate_request_input_tokens(&request)
                    >= token_estimation::IMAGE_TOKEN_ESTIMATE
            );
        }
        let wire = wire_request_json(&request, &backend);
        let wire_bytes = serde_json::to_vec(&wire).unwrap();
        assert!(wire_bytes.len() <= sampling_types::MAX_REQUEST_BODY_BYTES);
        fn contains_image(value: &serde_json::Value, marker: &str) -> bool {
            match value {
                serde_json::Value::String(text) => {
                    text.len() > 7 * 1024 * 1024 && text.ends_with(marker)
                }
                serde_json::Value::Array(values) => {
                    values.iter().any(|value| contains_image(value, marker))
                }
                serde_json::Value::Object(values) => {
                    values.values().any(|value| contains_image(value, marker))
                }
                _ => false,
            }
        }
        assert!(
            contains_image(&wire, &format!("{marker}a"))
                && contains_image(&wire, &format!("{marker}b")),
            "newest image must survive a native epoch reset on {backend:?}"
        );
        if index == 4 {
            assert!(
                contains_image(&wire, "image003a") && contains_image(&wire, "image003b"),
                "a ToolResult image selected at the first boundary must survive append on {backend:?}"
            );
        }
        assert!(
            crate::estimate_request_input_tokens(&request)
                >= token_estimation::IMAGE_TOKEN_ESTIMATE
        );
        while let Ok(event) = rx.try_recv() {
            if let ChatStateEvent::ImageBudget {
                inline_images,
                evicted,
                ..
            } = event
            {
                assert!(inline_images > 0);
                saw_eviction |= evicted > 0;
            }
        }
    }
    assert!(saw_eviction);
    let images = handle.get_conversation().await.iter().filter(|item| matches!(item,
        ConversationItem::ToolResult(result) if result.images.iter().any(|part| matches!(part, ContentPart::Image { .. }))
    )).count();
    assert_eq!(
        images, 5,
        "request projection must preserve original Timeline images"
    );
}

async fn request_runtime_suspension(handle: &crate::handle::ChatStateHandle, operation_id: &str) {
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::SuspendRequested {
                operation_id: operation_id.into(),
                root_session_id: "test-session".into(),
            },
        ))
        .await
        .unwrap();
}

async fn settle_runtime_commands(handle: &crate::handle::ChatStateHandle, operation_id: &str) {
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::CommandsSettled {
                operation_id: operation_id.into(),
                commands: Vec::new(),
            },
        ))
        .await
        .unwrap();
}

async fn root_checkpoint_with_manual_tree_ack()
-> (TestHarness, crate::session_runtime::RuntimeNodeRef) {
    // The first three live facts are acknowledged automatically: request,
    // empty command settlement, and checkpoint. TreeSuspended is the first
    // write whose durable ACK the caller controls.
    let h = TestHarness::with_manual_timeline_ack_after(vec![], 3);
    request_runtime_suspension(&h.handle, "tree-ack-operation").await;
    settle_runtime_commands(&h.handle, "tree-ack-operation").await;
    let checkpoint = h
        .handle
        .checkpoint_runtime(
            "tree-ack-operation".into(),
            "a".repeat(64),
            None,
            None,
            tokio::time::Instant::now() + Duration::from_secs(5),
        )
        .await
        .expect("root checkpoint is durable before the tree barrier");
    (
        h,
        crate::session_runtime::RuntimeNodeRef {
            session_id: "test-session".into(),
            checkpoint_seq: checkpoint.seq,
        },
    )
}

fn root_tree_suspended_kind(
    node: crate::session_runtime::RuntimeNodeRef,
) -> crate::TimelineEventKind {
    crate::TimelineEventKind::Runtime(crate::session_runtime::SessionRuntimeEvent::TreeSuspended {
        operation_id: "tree-ack-operation".into(),
        nodes: vec![node],
    })
}

#[tokio::test]
async fn root_tree_suspended_lost_ack_retries_one_exact_barrier_fact() {
    let (mut h, node) = root_checkpoint_with_manual_tree_ack().await;
    let _ = h.drain_persistence();
    let handle = h.handle.clone();
    let kind = root_tree_suspended_kind(node.clone());
    let write = tokio::spawn(async move { handle.record_timeline_event_durably(kind).await });

    let lost_ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("first root tree barrier persistence attempt");
    drop(lost_ack);
    let retry_ack = h
        .persistence_rx
        .next_timeline_ack()
        .await
        .expect("same root tree barrier is retried after the ACK is lost");
    let pending = h
        .handle
        .pending_timeline_commit()
        .expect("the retry retains the immutable pending event");
    assert!(matches!(
        &pending.event.kind,
        crate::TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::TreeSuspended { operation_id, nodes }
        ) if operation_id == "tree-ack-operation" && nodes == &vec![node.clone()]
    ));
    retry_ack.send(Ok(())).unwrap();
    write.await.unwrap().unwrap();

    let persisted_barriers = h
        .drain_persistence()
        .into_iter()
        .filter_map(|record| match record {
            PersistenceRecord::Timeline(event)
                if matches!(
                    event.kind,
                    crate::TimelineEventKind::Runtime(
                        crate::session_runtime::SessionRuntimeEvent::TreeSuspended { .. }
                    )
                ) =>
            {
                Some(event)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        persisted_barriers.len(),
        2,
        "one ACK-lost attempt and its exact retry"
    );
    assert_eq!(
        serde_json::to_value(&persisted_barriers[0]).unwrap(),
        serde_json::to_value(&persisted_barriers[1]).unwrap(),
        "retry must preserve operation, membership, sequence, and event identity"
    );
    let duplicate = h
        .handle
        .record_timeline_event_durably(root_tree_suspended_kind(node.clone()))
        .await;
    assert!(matches!(
        duplicate,
        Err(crate::commands::TimelineWriteError::Invalid(
            crate::TimelineError::InvalidRuntime(_)
        ))
    ));
    let timeline = crate::Timeline::from_events(h.handle.timeline_events().await.unwrap()).unwrap();
    assert_eq!(timeline.runtime_lifecycle().tree, vec![node]);
    assert_eq!(
        timeline
            .events()
            .iter()
            .filter(|event| matches!(
                event.kind,
                crate::TimelineEventKind::Runtime(
                    crate::session_runtime::SessionRuntimeEvent::TreeSuspended { .. }
                )
            ))
            .count(),
        1,
        "one logical root tree barrier is accepted"
    );
}

#[tokio::test]
async fn blocked_or_full_tree_barrier_does_not_claim_success_or_replace_writer() {
    for storage_full in [false, true] {
        let (mut h, node) = root_checkpoint_with_manual_tree_ack().await;
        let _ = h.drain_persistence();
        let kind = root_tree_suspended_kind(node);
        let handle = h.handle.clone();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(150);
        let write = tokio::spawn(async move {
            handle
                .record_timeline_event_durably_before(kind, deadline)
                .await
        });
        let ack = h
            .persistence_rx
            .next_timeline_ack()
            .await
            .expect("root tree barrier reaches the durable writer");
        let competing_handle = h.handle.clone();
        let competing = tokio::spawn(async move {
            competing_handle
                .record_timeline_event_durably(crate::TimelineEventKind::Observation(
                    crate::ObservationEvent {
                        scope: "test".into(),
                        name: "must_not_steal_writer".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .await
        });
        let blocked_ack = if storage_full {
            ack.send(Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "tree barrier storage full",
            )))
            .unwrap();
            None
        } else {
            Some(ack)
        };
        let result = tokio::time::timeout(Duration::from_secs(2), write)
            .await
            .expect("tree barrier resolves within its bounded persistence deadline")
            .unwrap();
        if storage_full {
            assert!(matches!(
                result,
                Err(crate::commands::TimelineWriteError::Persistence(ref error))
                    if error.kind() == std::io::ErrorKind::StorageFull
            ));
        } else {
            assert!(matches!(
                result,
                Err(crate::commands::TimelineWriteError::DeadlineExceeded)
            ));
            drop(blocked_ack);
        }
        let pending = h
            .handle
            .pending_timeline_commit()
            .expect("failed barrier retains its original pending commit");
        assert!(matches!(
            pending.event.kind,
            crate::TimelineEventKind::Runtime(
                crate::session_runtime::SessionRuntimeEvent::TreeSuspended { .. }
            )
        ));
        assert_eq!(
            pending.error_kind,
            storage_full.then_some(std::io::ErrorKind::StorageFull)
        );
        assert_eq!(pending.acknowledgement_lost, !storage_full);
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), competing)
                .await
                .expect("poisoned writer closes instead of taking a queued write")
                .unwrap(),
            Err(crate::commands::TimelineWriteError::AcknowledgementLost)
        ));
        assert!(
            h.handle.is_closed(),
            "a failed writer epoch cannot admit a replacement write"
        );
        assert!(
            h.persistence_rx.drain().iter().all(|record| match record {
                PersistenceRecord::Timeline(event) => matches!(
                    event.kind,
                    crate::TimelineEventKind::Runtime(
                        crate::session_runtime::SessionRuntimeEvent::TreeSuspended { .. }
                    )
                ),
                PersistenceRecord::Flush => true,
            }),
            "the queued observation must never reach persistence"
        );
    }
}

async fn suspended_open_turn_events() -> (Vec<crate::TimelineEvent>, crate::TurnId) {
    let h = TestHarness::new();
    let turn = crate::TurnId(990);
    let step = crate::StepId { turn, index: 0 };
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Turn(crate::TurnEvent::Started {
            prompt_id: "cold-runtime-prompt".into(),
            id: turn,
            input_ids: Vec::new(),
            identity: test_turn_identity(),
            model_id: "test-model".into(),
            input_message_count: 0,
            prompt_index: 0,
            prompt_text: "original in-flight turn".into(),
            input_kind: crate::TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Step(crate::StepEvent::Started {
            id: step,
        }))
        .await
        .unwrap();
    request_runtime_suspension(&h.handle, "cold-prefix-operation").await;
    settle_runtime_commands(&h.handle, "cold-prefix-operation").await;
    let continuation = crate::session_runtime::TurnContinuation {
        turn_id: turn,
        prompt_id: "cold-runtime-prompt".into(),
        prompt_index: 0,
        origin: "user".into(),
        next_step_index: 1,
        tool_turns_used: 0,
        tool_calls_used: 0,
        stop_continuations_used: 0,
        output_schema: None,
        resume_point: crate::session_runtime::TurnResumePoint::Model,
        task_output_usage: None,
        accepted_response: None,
        structured_output_retries: 0,
        sampling_attempts_used: 0,
        sampling_max_attempts: Some(3),
        sampling_replay_safe: true,
        sampling_deadline_ms: None,
        operations: Vec::new(),
    };
    let checkpoint = h
        .handle
        .checkpoint_runtime(
            "cold-prefix-operation".into(),
            "b".repeat(64),
            Some(continuation),
            Some(step),
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::TreeSuspended {
                operation_id: "cold-prefix-operation".into(),
                nodes: vec![crate::session_runtime::RuntimeNodeRef {
                    session_id: "test-session".into(),
                    checkpoint_seq: checkpoint.seq,
                }],
            },
        ))
        .await
        .unwrap();
    (h.handle.timeline_events().await.unwrap(), turn)
}

async fn cold_actor_from_events(
    events: Vec<crate::TimelineEvent>,
) -> (crate::handle::ChatStateHandle, MockPersistenceReceiver) {
    let (persistence, persistence_rx) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let handle = ChatStateActor::spawn_from_timeline(
        events,
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("cold load accepts the validated Timeline prefix");
    (handle, persistence_rx)
}

fn assert_original_turn_is_preserved(events: &[crate::TimelineEvent], turn: crate::TurnId) {
    let timeline = crate::Timeline::from_events(events.to_vec()).unwrap();
    assert_eq!(timeline.active_turn(), Some(turn));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.kind,
                crate::TimelineEventKind::Turn(crate::TurnEvent::Started { id, .. }) if id == turn
            ))
            .count(),
        1,
        "cold restoration must not create a replacement Turn"
    );
    assert!(
        !events.iter().any(|event| matches!(event.kind,
            crate::TimelineEventKind::Turn(crate::TurnEvent::Ended { id, .. }) if id == turn
        )),
        "restore preparation/commit cannot create an interruption terminal before release"
    );
}

#[tokio::test]
async fn runtime_cold_load_prefixes_preserve_turn_until_execution_release() {
    let (suspended_events, turn) = suspended_open_turn_events().await;
    let (suspended, _suspended_persistence) = cold_actor_from_events(suspended_events).await;
    assert_original_turn_is_preserved(&suspended.timeline_events().await.unwrap(), turn);
    let suspended_state = suspended.runtime_state().await.unwrap();
    assert_eq!(
        suspended_state.phase,
        crate::session_runtime::RuntimePhase::Suspended
    );
    let original_checkpoint = suspended_state
        .checkpoint
        .clone()
        .expect("suspended prefix retains its checkpoint");
    assert!(
        suspended
            .recover_interrupted_durably()
            .await
            .unwrap()
            .is_empty()
    );
    let prepared_event = suspended
        .record_timeline_event_durably(crate::TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::RestorePrepared {
                operation_id: "cold-prefix-operation".into(),
                instance_id: "restore-instance".into(),
            },
        ))
        .await
        .unwrap();
    let prepared_events = suspended.timeline_events().await.unwrap();
    assert!(matches!(
        prepared_event.kind,
        crate::TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::RestorePrepared { .. }
        )
    ));
    drop(suspended);

    let (prepared, _prepared_persistence) = cold_actor_from_events(prepared_events).await;
    let prepared_state = prepared.runtime_state().await.unwrap();
    assert_eq!(
        prepared_state.phase,
        crate::session_runtime::RuntimePhase::Restoring
    );
    assert!(!prepared_state.resume_committed);
    assert_eq!(prepared_state.checkpoint, Some(original_checkpoint.clone()));
    assert_original_turn_is_preserved(&prepared.timeline_events().await.unwrap(), turn);
    assert!(
        prepared
            .recover_interrupted_durably()
            .await
            .unwrap()
            .is_empty()
    );
    prepared
        .record_timeline_event_durably(crate::TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::Resumed {
                operation_id: "cold-prefix-operation".into(),
                instance_id: "restore-instance".into(),
            },
        ))
        .await
        .unwrap();
    let resumed_events = prepared.timeline_events().await.unwrap();
    drop(prepared);

    let (resumed, _resumed_persistence) = cold_actor_from_events(resumed_events).await;
    let resumed_state = resumed.runtime_state().await.unwrap();
    assert_eq!(
        resumed_state.phase,
        crate::session_runtime::RuntimePhase::Restoring
    );
    assert!(resumed_state.resume_committed);
    assert_eq!(resumed_state.checkpoint, Some(original_checkpoint.clone()));
    assert_original_turn_is_preserved(&resumed.timeline_events().await.unwrap(), turn);
    assert!(
        resumed
            .recover_interrupted_durably()
            .await
            .unwrap()
            .is_empty()
    );
    resumed
        .record_timeline_event_durably(crate::TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::ExecutionReleased {
                operation_id: "cold-prefix-operation".into(),
                instance_id: "restore-instance".into(),
            },
        ))
        .await
        .unwrap();
    let released_events = resumed.timeline_events().await.unwrap();
    drop(resumed);

    let (released, _released_persistence) = cold_actor_from_events(released_events).await;
    let released_state = released.runtime_state().await.unwrap();
    assert_eq!(
        released_state.phase,
        crate::session_runtime::RuntimePhase::Running
    );
    assert_eq!(
        released_state.checkpoint,
        Some(original_checkpoint),
        "checkpoint remains audit history"
    );
    assert_eq!(
        released
            .timeline_events()
            .await
            .unwrap()
            .iter()
            .filter(|event| matches!(
                &event.kind,
                crate::TimelineEventKind::Turn(crate::TurnEvent::Ended { id, outcome, .. })
                    if *id == turn && outcome == "interrupted"
            ))
            .count(),
        1,
        "after release, a cold crash follows ordinary interruption recovery"
    );
    assert_eq!(
        released.runtime_state().await.unwrap().phase,
        crate::session_runtime::RuntimePhase::Running
    );
}

fn test_turn_identity() -> crate::TurnIdentity {
    crate::TurnIdentity {
        origin: "user".into(),
        turn_kind: "internal".into(),
        goal_id: None,
        goal_definition_revision: None,
        stage_id: None,
    }
}

async fn seed_admitted_prompt_for_turn(
    handle: &crate::handle::ChatStateHandle,
    input_id: &str,
    turn: crate::TurnId,
) {
    handle
        .submit_input_durably(
            input_id.into(),
            crate::InputIntent::Prompt,
            crate::InputPayloadRef {
                blake3: blake3::hash(input_id.as_bytes()).to_hex().to_string(),
                bytes: input_id.len() as u64,
            },
        )
        .await
        .unwrap();
    let hook_occurrence_id = format!("hook-{input_id}");
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Hook(
            crate::HookEvent::Triggered {
                occurrence_id: hook_occurrence_id.clone(),
                event: crate::HookEventType::UserPromptSubmit,
                gate: crate::HookGateKind::Prompt,
                cause: crate::HookCause::Input {
                    input_id: input_id.into(),
                },
                config_generation: 1,
                handlers: Vec::new(),
            },
        ))
        .await
        .unwrap();
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Hook(
            crate::HookEvent::Completed {
                occurrence_id: hook_occurrence_id,
                decision: crate::HookAggregateDecision::Prompt {
                    decision: crate::HookGateDecision::Allow,
                },
            },
        ))
        .await
        .unwrap();
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Input(
            crate::InputEvent::AdmissionResolved {
                input_id: input_id.into(),
                decision: crate::InputAdmissionDecision::Allow,
                route: Some(crate::InputRoute::Fifo),
                supersedes: Vec::new(),
            },
        ))
        .await
        .unwrap();
    let mut identity = test_turn_identity();
    identity.turn_kind = "user".into();
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Turn(crate::TurnEvent::Started {
            prompt_id: "fixture-prompt".into(),
            id: turn,
            input_ids: vec![input_id.into()],
            identity,
            model_id: "test-model".into(),
            input_message_count: handle.get_conversation_len().await,
            prompt_index: 0,
            prompt_text: "skill input".into(),
            input_kind: crate::TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .await
        .unwrap();
}

async fn seed_admitted_load_skill_call(
    handle: &crate::handle::ChatStateHandle,
    turn: crate::TurnId,
    call_id: &str,
    arguments: &str,
) {
    seed_admitted_tool_batch(
        handle,
        turn,
        call_id,
        vec![sampling_types::ToolCall {
            id: call_id.into(),
            name: sampling_types::LOAD_SKILL_TOOL_NAME.into(),
            arguments: arguments.into(),
        }],
    )
    .await;
}

async fn seed_admitted_tool_batch(
    handle: &crate::handle::ChatStateHandle,
    turn: crate::TurnId,
    call_id: &str,
    calls: Vec<sampling_types::ToolCall>,
) {
    let step = crate::StepId { turn, index: 0 };
    let request_id = format!("request-{call_id}");
    let input_message_count = handle.get_conversation_len().await;
    let prompt_index = handle.get_prompt_index().await;
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Turn(crate::TurnEvent::Started {
            prompt_id: "fixture-prompt".into(),
            id: turn,
            input_ids: Vec::new(),
            identity: test_turn_identity(),
            model_id: "test-model".into(),
            input_message_count,
            prompt_index,
            prompt_text: "model load_skill call".into(),
            input_kind: crate::TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .await
        .unwrap();
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Step(crate::StepEvent::Started {
            id: step,
        }))
        .await
        .unwrap();
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Request(
            crate::RequestEvent::Started {
                id: request_id.clone(),
                turn,
                step,
                model_id: "test-model".into(),
                input_message_count,
                tool_count: 1,
            },
        ))
        .await
        .unwrap();
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Request(
            crate::RequestEvent::Completed {
                id: request_id.clone(),
                duration_ms: 1,
                time_to_first_token_ms: None,
                usage: crate::RequestUsage::default(),
                response_message_count: 1,
                attempt: 1,
                provider_terminal: None,
            },
        ))
        .await
        .unwrap();
    handle
        .record_timeline_event_durably(crate::TimelineEventKind::Messages(crate::MessageEvent {
            cause: crate::MessageCause::Assistant,
            items: vec![ConversationItem::assistant_tool_calls(calls)],
            surface: crate::SurfaceOp::Append,
            response_admission: Some(crate::ResponseAdmission {
                identity: crate::ResponseAdmissionIdentity {
                    request_id,
                    attempt: 1,
                },
                quarantined_tool_exchanges: 0,
            }),
        }))
        .await
        .unwrap();
}

fn user_skill_load(
    input_id: &str,
    reference_index: usize,
    call_id: &str,
    name: &str,
    body: &str,
) -> sampling_types::SkillLoadItem {
    sampling_types::SkillLoadItem {
        input_id: input_id.into(),
        reference_index,
        call_id: call_id.into(),
        skill_name: name.into(),
        source: sampling_types::SkillLoadSource::User,
        args: String::new(),
        prepared_body: std::sync::Arc::from(body),
        prompt_index: Some(0),
    }
}

fn user_display_echo(message_id: &str, prompt_index: usize, text: &str) -> crate::UserDisplayEcho {
    let mut blocks = serde_json::json!([{"type": "text", "text": text}]);
    blocks.sort_all_objects();
    crate::UserDisplayEcho {
        message_id: message_id.into(),
        prompt_index,
        block_count: 1,
        blake3: blake3::hash(&serde_json::to_vec(&blocks).unwrap())
            .to_hex()
            .to_string(),
    }
}

#[tokio::test]
async fn checkpoint_runtime_selects_frontier_after_queued_observation() {
    let h = TestHarness::new();
    request_runtime_suspension(&h.handle, "suspend-queued-observation").await;
    settle_runtime_commands(&h.handle, "suspend-queued-observation").await;
    h.handle
        .record_timeline_event(crate::TimelineEventKind::Observation(
            crate::ObservationEvent {
                scope: "test".into(),
                name: "queued_before_checkpoint".into(),
                turn: None,
                step: None,
                data: None,
            },
        ));

    let checkpoint_event = h
        .handle
        .checkpoint_runtime(
            "suspend-queued-observation".into(),
            "a".repeat(64),
            None,
            None,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
    let crate::TimelineEventKind::Runtime(
        crate::session_runtime::SessionRuntimeEvent::Checkpoint { checkpoint, .. },
    ) = checkpoint_event.kind
    else {
        panic!("checkpoint command must commit a runtime checkpoint");
    };
    assert_eq!(checkpoint.frontier.get() + 1, checkpoint_event.seq.get());
    let events = h.handle.timeline_events().await.unwrap();
    assert!(matches!(
        events
            .get(checkpoint.frontier.get() as usize)
            .map(|event| &event.kind),
        Some(crate::TimelineEventKind::Observation(observation))
            if observation.name == "queued_before_checkpoint"
    ));
}

#[tokio::test]
async fn suspension_hook_settlement_preserves_turn_input_and_unknown_outcome() {
    let h = TestHarness::new();
    let turn = crate::TurnId(952);
    let input_id = "suspension-hook-input";
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Turn(crate::TurnEvent::Started {
            prompt_id: "suspension-hook-prompt".into(),
            id: turn,
            input_ids: Vec::new(),
            identity: test_turn_identity(),
            model_id: "test-model".into(),
            input_message_count: 0,
            prompt_index: 0,
            prompt_text: "ongoing turn".into(),
            input_kind: crate::TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .await
        .unwrap();
    h.handle
        .submit_input_durably(
            input_id.into(),
            crate::InputIntent::Prompt,
            crate::InputPayloadRef {
                blake3: blake3::hash(input_id.as_bytes()).to_hex().to_string(),
                bytes: input_id.len() as u64,
            },
        )
        .await
        .unwrap();
    let occurrence_id = "suspension-hook-occurrence";
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Hook(
            crate::HookEvent::Triggered {
                occurrence_id: occurrence_id.into(),
                event: crate::HookEventType::UserPromptSubmit,
                gate: crate::HookGateKind::Prompt,
                cause: crate::HookCause::Input {
                    input_id: input_id.into(),
                },
                config_generation: 1,
                handlers: vec![crate::HookHandlerPlan {
                    index: 0,
                    run_id: "suspension-hook-run".into(),
                    name: "prompt hook".into(),
                    provenance: crate::HookHandlerProvenance::UserConfig,
                    kind: crate::HookHandlerKind::Command,
                    failure_policy: crate::HookFailurePolicy::Allow,
                    action: crate::HookHandlerPlanAction::Execute,
                }],
            },
        ))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Hook(
            crate::HookEvent::RunStarted {
                occurrence_id: occurrence_id.into(),
                run_id: "suspension-hook-run".into(),
                handler_index: 0,
            },
        ))
        .await
        .unwrap();
    request_runtime_suspension(&h.handle, "suspend-hook-operation").await;
    settle_runtime_commands(&h.handle, "suspend-hook-operation").await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let settled = h
        .handle
        .settle_hooks_for_suspension("suspend-hook-operation".into(), deadline)
        .await
        .unwrap();
    assert!(settled.iter().any(|event| matches!(
        &event.kind,
        crate::TimelineEventKind::Hook(crate::HookEvent::RunFinished {
            outcome: crate::HookRunOutcome::InterruptedOutcomeUnknown,
            ..
        })
    )));
    assert!(settled.iter().any(|event| matches!(
        &event.kind,
        crate::TimelineEventKind::Hook(crate::HookEvent::Completed { .. })
    )));

    let events = h.handle.timeline_events().await.unwrap();
    let timeline = crate::Timeline::from_events(events.clone()).unwrap();
    assert_eq!(timeline.active_turn(), Some(turn));
    assert_eq!(
        timeline.input_state(input_id).unwrap().state,
        crate::InputState::Submitted
    );
    assert!(!events.iter().any(|event| matches!(
        &event.kind,
        crate::TimelineEventKind::Turn(crate::TurnEvent::Ended { id, .. }) if *id == turn
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.kind,
        crate::TimelineEventKind::Input(crate::InputEvent::AdmissionResolved { input_id: id, .. })
            if id == input_id
    )));
    assert!(
        h.handle
            .settle_hooks_for_suspension("suspend-hook-operation".into(), deadline)
            .await
            .unwrap()
            .is_empty()
    );

    let continuation = crate::session_runtime::TurnContinuation {
        turn_id: turn,
        prompt_id: "suspension-hook-prompt".into(),
        prompt_index: 0,
        origin: "user".into(),
        next_step_index: 0,
        tool_turns_used: 0,
        tool_calls_used: 0,
        stop_continuations_used: 0,
        output_schema: None,
        resume_point: crate::session_runtime::TurnResumePoint::Model,
        task_output_usage: None,
        accepted_response: None,
        structured_output_retries: 0,
        sampling_attempts_used: 0,
        sampling_max_attempts: Some(3),
        sampling_replay_safe: true,
        sampling_deadline_ms: None,
        operations: Vec::new(),
    };
    h.handle
        .checkpoint_runtime(
            "suspend-hook-operation".into(),
            "a".repeat(64),
            Some(continuation),
            None,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
    assert!(
        h.handle
            .recover_interrupted_durably()
            .await
            .unwrap()
            .is_empty()
    );

    let events = h.handle.timeline_events().await.unwrap();
    let timeline = crate::Timeline::from_events(events.clone()).unwrap();
    assert_eq!(timeline.active_turn(), Some(turn));
    assert_eq!(
        timeline.input_state(input_id).unwrap().state,
        crate::InputState::Submitted
    );
}

#[tokio::test]
async fn rewind_rejects_an_unfinished_runtime_checkpoint_without_changing_surface() {
    let h = TestHarness::new();
    h.handle
        .push_user_message_durably(ConversationItem::user("retained prompt"))
        .await
        .unwrap();
    request_runtime_suspension(&h.handle, "suspend-rewind-source").await;
    settle_runtime_commands(&h.handle, "suspend-rewind-source").await;
    h.handle
        .checkpoint_runtime(
            "suspend-rewind-source".into(),
            "a".repeat(64),
            None,
            None,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();

    let before = h.handle.get_conversation().await;
    let events_before = h.handle.timeline_events().await.unwrap();
    let result = h.handle.rewind_durably(0).await;
    assert!(matches!(
        result,
        Err(crate::commands::TimelineWriteError::Invalid(
            crate::TimelineError::InvalidRuntime(_)
        ))
    ));
    assert_eq!(
        serde_json::to_value(h.handle.get_conversation().await).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(
        serde_json::to_value(h.handle.timeline_events().await.unwrap()).unwrap(),
        serde_json::to_value(events_before).unwrap()
    );
}

#[tokio::test]
async fn checkpoint_runtime_rejects_open_request_without_installing_checkpoint() {
    let h = TestHarness::new();
    let turn = crate::TurnId(930);
    let step = crate::StepId { turn, index: 0 };
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Turn(crate::TurnEvent::Started {
            prompt_id: "prompt-open-request".into(),
            id: turn,
            input_ids: Vec::new(),
            identity: test_turn_identity(),
            model_id: "test-model".into(),
            input_message_count: 0,
            prompt_index: 0,
            prompt_text: "open request".into(),
            input_kind: crate::TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Step(crate::StepEvent::Started {
            id: step,
        }))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Request(
            crate::RequestEvent::Started {
                id: "request-still-open".into(),
                turn,
                step,
                model_id: "test-model".into(),
                input_message_count: 0,
                tool_count: 0,
            },
        ))
        .await
        .unwrap();
    request_runtime_suspension(&h.handle, "suspend-open-request").await;
    settle_runtime_commands(&h.handle, "suspend-open-request").await;

    let continuation = crate::session_runtime::TurnContinuation {
        turn_id: turn,
        prompt_id: "fixture-prompt".into(),
        prompt_index: 0,
        origin: "user".into(),
        next_step_index: 1,
        tool_turns_used: 0,
        tool_calls_used: 0,
        stop_continuations_used: 0,
        output_schema: None,
        resume_point: crate::session_runtime::TurnResumePoint::Model,
        task_output_usage: None,
        accepted_response: None,
        structured_output_retries: 0,
        sampling_attempts_used: 0,
        sampling_max_attempts: Some(3),
        sampling_replay_safe: true,
        sampling_deadline_ms: None,
        operations: Vec::new(),
    };
    assert!(matches!(
        h.handle
            .checkpoint_runtime(
                "suspend-open-request".into(),
                "a".repeat(64),
                Some(continuation),
                Some(step),
                tokio::time::Instant::now() + Duration::from_secs(1),
            )
            .await,
        Err(crate::commands::TimelineWriteError::Invalid(
            crate::TimelineError::InvalidRuntime(_)
        ))
    ));
    let runtime = h.handle.runtime_state().await.unwrap();
    assert_eq!(
        runtime.phase,
        crate::session_runtime::RuntimePhase::Suspending
    );
    assert!(runtime.checkpoint.is_none());
    assert!(
        h.handle
            .timeline_events()
            .await
            .unwrap()
            .iter()
            .all(|event| !matches!(
                &event.kind,
                crate::TimelineEventKind::Runtime(
                    crate::session_runtime::SessionRuntimeEvent::Checkpoint { .. }
                )
            ))
    );
}

#[tokio::test]
async fn checkpoint_runtime_preserves_settled_turn_through_interruption_recovery() {
    let h = TestHarness::new();
    let turn = crate::TurnId(931);
    let step = crate::StepId { turn, index: 0 };
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Turn(crate::TurnEvent::Started {
            prompt_id: "prompt-931".into(),
            id: turn,
            input_ids: Vec::new(),
            identity: test_turn_identity(),
            model_id: "test-model".into(),
            input_message_count: 0,
            prompt_index: 0,
            prompt_text: "settled turn".into(),
            input_kind: crate::TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Step(crate::StepEvent::Started {
            id: step,
        }))
        .await
        .unwrap();
    h.handle
        .push_response_durably(
            vec![ConversationItem::assistant_tool_calls(vec![
                sampling_types::ToolCall {
                    id: "original-pending-call".into(),
                    name: "spawn_subagent".into(),
                    arguments: "{}".into(),
                },
            ])],
            None,
        )
        .await
        .unwrap();
    request_runtime_suspension(&h.handle, "suspend-settled-turn").await;
    settle_runtime_commands(&h.handle, "suspend-settled-turn").await;
    let continuation = crate::session_runtime::TurnContinuation {
        turn_id: turn,
        prompt_id: "prompt-931".into(),
        prompt_index: 0,
        origin: "user".into(),
        next_step_index: 1,
        tool_turns_used: 0,
        tool_calls_used: 0,
        stop_continuations_used: 0,
        output_schema: None,
        resume_point: crate::session_runtime::TurnResumePoint::Model,
        task_output_usage: None,
        accepted_response: None,
        structured_output_retries: 0,
        sampling_attempts_used: 0,
        sampling_max_attempts: Some(3),
        sampling_replay_safe: true,
        sampling_deadline_ms: None,
        operations: Vec::new(),
    };
    h.handle
        .checkpoint_runtime(
            "suspend-settled-turn".into(),
            "a".repeat(64),
            Some(continuation),
            Some(step),
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();

    assert!(
        h.handle
            .recover_interrupted_durably()
            .await
            .unwrap()
            .is_empty()
    );
    let runtime = h.handle.runtime_state().await.unwrap();
    assert_eq!(
        runtime.phase,
        crate::session_runtime::RuntimePhase::Suspended
    );
    assert_eq!(runtime.checkpoint.unwrap().turn.unwrap().turn_id, turn);
    let events = h.handle.timeline_events().await.unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        crate::TimelineEventKind::Turn(crate::TurnEvent::Started { id, .. }) if *id == turn
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.kind,
        crate::TimelineEventKind::Turn(crate::TurnEvent::Ended { id, .. }) if *id == turn
    )));

    let original_surface = h.handle.get_conversation().await;
    let (persistence, _persistence_rx) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        events.clone(),
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("cold load must preserve the suspended continuation");
    assert_eq!(
        serde_json::to_value(restored.get_conversation().await).unwrap(),
        serde_json::to_value(original_surface).unwrap(),
        "a suspended unresolved call must not receive an interruption placeholder"
    );
    assert_eq!(
        restored.timeline_events().await.unwrap().len(),
        events.len()
    );
}

#[tokio::test]
async fn conditional_skill_input_publishes_user_and_ordered_loads_once() {
    let h = TestHarness::new();
    let input_id = "input-skills";
    let turn = crate::TurnId(940);
    seed_admitted_prompt_for_turn(&h.handle, input_id, turn).await;
    let mut user = ConversationItem::user("/review fix /format rust");
    user.set_prompt_index(0);
    let loads = vec![
        user_skill_load(input_id, 0, "skill-call-review", "review", "review body"),
        user_skill_load(input_id, 1, "skill-call-format", "format", "format body"),
    ];
    let revision = h.handle.get_surface_revision().await.unwrap();

    let outcome = h
        .handle
        .consume_input_group_conditionally(
            vec![input_id.into()],
            turn,
            user.clone(),
            Some(user_display_echo(input_id, 0, "/review fix /format rust")),
            loads.clone(),
            revision,
            128_000,
        )
        .await
        .unwrap();
    assert_eq!(outcome, crate::ConditionalToolResultOutcome::Accepted);
    assert_eq!(
        serde_json::to_value(h.handle.get_conversation().await).unwrap(),
        serde_json::to_value(vec![
            user.clone(),
            ConversationItem::SkillLoad(loads[0].clone()),
            ConversationItem::SkillLoad(loads[1].clone()),
        ])
        .unwrap()
    );
    let events = h.handle.timeline_events().await.unwrap();
    let consumed = events
        .iter()
        .filter_map(|event| match &event.kind {
            crate::TimelineEventKind::Input(crate::InputEvent::Consumed {
                input_ids,
                item,
                skill_loads,
                ..
            }) => Some((input_ids, item, skill_loads)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(consumed.len(), 1);
    assert_eq!(consumed[0].0, &[input_id.to_string()]);
    assert_eq!(
        serde_json::to_value(consumed[0].1).unwrap(),
        serde_json::to_value(&user).unwrap()
    );
    assert_eq!(consumed[0].2, &loads);

    let revision_after = h.handle.get_surface_revision().await.unwrap();
    assert!(matches!(
        h.handle
            .consume_input_group_conditionally(
                vec![input_id.into()],
                turn,
                user,
                None,
                loads,
                revision_after,
                128_000,
            )
            .await,
        Err(crate::commands::TimelineWriteError::Invalid(
            crate::TimelineError::InvalidInput
        ))
    ));
    assert_eq!(
        h.handle.timeline_events().await.unwrap().len(),
        events.len()
    );

    h.handle
        .settle_unconsumed_skill_loads()
        .await
        .unwrap()
        .expect("terminal settlement removes unconsumed user guidance");
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Turn(crate::TurnEvent::Ended {
            id: turn,
            outcome: "completed".into(),
            duration_ms: 1,
            tool_count: 0,
            terminal: crate::TurnTerminal {
                source: crate::TurnTerminalSource::Host,
                stop_reason: "end_turn".into(),
                completion_kind: "completed".into(),
            },
            cancellation_category: None,
            details: None,
        }))
        .await
        .unwrap();
    let settled_history = h.handle.timeline_events().await.unwrap();
    let (persistence, _persistence_records) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        settled_history.clone(),
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("settled user skill history cold-restores");
    let restored_surface = restored.get_conversation().await;
    assert!(
        restored_surface
            .iter()
            .all(|item| !matches!(item, ConversationItem::SkillLoad(_)))
    );
    assert!(
        restored_surface
            .iter()
            .all(|item| !item.text_content().contains("review body")
                && !item.text_content().contains("format body"))
    );
    assert!(settled_history.iter().any(|event| matches!(
        &event.kind,
        crate::TimelineEventKind::Input(crate::InputEvent::Consumed { skill_loads, .. })
            if skill_loads.len() == 2
    )));
}

#[tokio::test]
async fn conditional_skill_input_revision_conflict_leaves_admitted_input_unconsumed() {
    let h = TestHarness::new();
    let input_id = "input-stale-skills";
    let turn = crate::TurnId(941);
    seed_admitted_prompt_for_turn(&h.handle, input_id, turn).await;
    let mut user = ConversationItem::user("/review check");
    user.set_prompt_index(0);
    let loads = vec![user_skill_load(
        input_id,
        0,
        "stale-skill-call",
        "review",
        "must not publish",
    )];
    let before_revision = h.handle.get_surface_revision().await.unwrap();
    let before_events = h.handle.timeline_events().await.unwrap();

    let outcome = h
        .handle
        .consume_input_group_conditionally(
            vec![input_id.into()],
            turn,
            user,
            None,
            loads,
            before_revision + 1,
            128_000,
        )
        .await
        .unwrap();
    assert_eq!(
        outcome,
        crate::ConditionalToolResultOutcome::RejectedSurfaceChanged
    );
    assert!(h.handle.get_conversation().await.is_empty());
    let after_events = h.handle.timeline_events().await.unwrap();
    assert_eq!(after_events.len(), before_events.len());
    assert!(!after_events.iter().any(|event| matches!(
        &event.kind,
        crate::TimelineEventKind::Input(crate::InputEvent::Consumed { input_ids, .. })
            if input_ids.iter().any(|id| id == input_id)
    )));
}

#[tokio::test]
async fn model_skill_call_mismatch_fails_closed_and_stale_rejection_uses_only_original_id() {
    let h = TestHarness::new();
    let call_id = "model-skill-original-call";
    seed_admitted_load_skill_call(
        &h.handle,
        crate::TurnId(942),
        call_id,
        r#"{"name":"review","args":"actual args"}"#,
    )
    .await;
    let skill = sampling_types::SkillLoadItem {
        input_id: format!("model:{call_id}"),
        reference_index: 0,
        call_id: call_id.into(),
        skill_name: "review".into(),
        source: sampling_types::SkillLoadSource::Model,
        args: "mismatched args".into(),
        prepared_body: std::sync::Arc::from("must not publish"),
        prompt_index: Some(0),
    };
    let prepared_result =
        ConversationItem::tool_result(call_id, sampling_types::skill_load_result_content(&skill));
    let rejection = ConversationItem::tool_result(call_id, "Skill was not loaded: call mismatch");
    let revision = h.handle.get_surface_revision().await.unwrap();
    let before_events = h.handle.timeline_events().await.unwrap();

    assert!(matches!(
        h.handle
            .publish_model_skill_conditionally(
                prepared_result.clone(),
                skill.clone(),
                rejection.clone(),
                revision,
                128_000,
                2_048,
            )
            .await,
        Err(crate::commands::TimelineWriteError::Invalid(
            crate::TimelineError::InvalidModelSkillLoad
        ))
    ));
    assert_eq!(
        h.handle.timeline_events().await.unwrap().len(),
        before_events.len()
    );

    // The stale-revision branch publishes the caller's fallback result but
    // must keep it tied to the original model call identity.
    let outcome = h
        .handle
        .publish_model_skill_conditionally(
            prepared_result,
            skill,
            rejection,
            revision + 1,
            128_000,
            2_048,
        )
        .await
        .unwrap();
    assert_eq!(
        outcome,
        crate::ConditionalToolResultOutcome::RejectedSurfaceChanged
    );
    let surface = h.handle.get_conversation().await;
    let results = surface
        .iter()
        .filter_map(|item| match item {
            ConversationItem::ToolResult(result) => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].tool_call_id, call_id);
    assert_eq!(
        results[0].content.as_ref(),
        "Skill was not loaded: call mismatch"
    );
    assert!(
        !surface
            .iter()
            .any(|item| matches!(item, ConversationItem::SkillLoad(_)))
    );
}

#[tokio::test]
async fn unconsumed_model_skill_is_pinned_then_settled_once_without_branch_revival() {
    let h = TestHarness::with_conversation(vec![ConversationItem::system("system")]);
    let call_id = "settled-skill-call";
    let args = r#"{"name":"review","args":"focus"}"#;
    seed_admitted_load_skill_call(&h.handle, crate::TurnId(943), call_id, args).await;
    let step = crate::StepId {
        turn: crate::TurnId(943),
        index: 0,
    };
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Tool(crate::ToolEvent::Started {
            call_id: call_id.into(),
            turn: step.turn,
            step,
            name: sampling_types::LOAD_SKILL_TOOL_NAME.into(),
            input: Some(serde_json::from_str(args).unwrap()),
        }))
        .await
        .unwrap();
    let skill = sampling_types::SkillLoadItem {
        input_id: format!("model:{call_id}"),
        reference_index: 0,
        call_id: call_id.into(),
        skill_name: "review".into(),
        source: sampling_types::SkillLoadSource::Model,
        args: "focus".into(),
        prepared_body: std::sync::Arc::from("private unconsumed guidance"),
        prompt_index: Some(0),
    };
    let result =
        ConversationItem::tool_result(call_id, sampling_types::skill_load_result_content(&skill));
    let rejection = ConversationItem::tool_result(call_id, "Skill was not loaded");
    let revision = h.handle.get_surface_revision().await.unwrap();
    assert_eq!(
        h.handle
            .publish_model_skill_conditionally(result, skill, rejection, revision, 128_000, 2_048,)
            .await
            .unwrap(),
        crate::ConditionalToolResultOutcome::Accepted
    );
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Tool(
            crate::ToolEvent::Completed {
                call_id: call_id.into(),
                name: sampling_types::LOAD_SKILL_TOOL_NAME.into(),
                outcome: "completed".into(),
                duration_ms: 1,
                details: None,
            },
        ))
        .await
        .unwrap();

    let source_items = h.handle.get_conversation_len().await;
    let prompt_index = h.handle.get_prompt_index().await;
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Started {
                mode: crate::CompactionMode::Foreground,
                id: "skill-pin".into(),
                source_items,
                prompt_index,
            },
        ))
        .await
        .unwrap();
    let target = record_compaction_summary(&h.handle, "skill-pin").await;
    assert!(matches!(
        h.handle
            .replace_compaction_range(target, vec![ConversationItem::user_meta("summary")])
            .await,
        Err(crate::commands::TimelineWriteError::Invalid(
            crate::TimelineError::UnconsumedSkillLoadProtected
        ))
    ));
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Compaction(
            crate::CompactionEvent::Failed {
                id: "skill-pin".into(),
                duration_ms: 1,
                error: "unconsumed skill load is pinned".into(),
            },
        ))
        .await
        .unwrap();

    let turn = step.turn;
    let settled = h
        .handle
        .settle_unconsumed_skill_loads()
        .await
        .unwrap()
        .expect("terminal settlement writes one Surface replacement");
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Step(crate::StepEvent::Ended {
            id: step,
            outcome: "completed".into(),
            duration_ms: 1,
        }))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(crate::TimelineEventKind::Turn(crate::TurnEvent::Ended {
            id: turn,
            outcome: "completed".into(),
            duration_ms: 1,
            tool_count: 1,
            terminal: crate::TurnTerminal {
                source: crate::TurnTerminalSource::Host,
                stop_reason: "end_turn".into(),
                completion_kind: "completed".into(),
            },
            cancellation_category: None,
            details: None,
        }))
        .await
        .unwrap();
    assert!(matches!(
        settled.kind,
        crate::TimelineEventKind::Messages(crate::MessageEvent {
            cause: crate::MessageCause::SkillLoadSettlement,
            ..
        })
    ));
    assert!(
        h.handle
            .settle_unconsumed_skill_loads()
            .await
            .unwrap()
            .is_none()
    );
    let surface = h.handle.get_conversation().await;
    assert!(
        !surface
            .iter()
            .any(|item| matches!(item, ConversationItem::SkillLoad(_)))
    );
    let failure_result = surface.iter().find_map(|item| match item {
        ConversationItem::ToolResult(result) if result.tool_call_id == call_id => Some(result),
        _ => None,
    });
    assert_eq!(
        failure_result.map(|result| result.content.as_ref()),
        Some("Loaded skill content was not consumed before execution ended")
    );
    let branch = h
        .handle
        .materialize_branch_transcript("test-timeline".into())
        .await
        .unwrap();
    assert!(branch.transcript.iter().all(|item| {
        !matches!(item, ConversationItem::SkillLoad(_))
            && item.text_content() != "private unconsumed guidance"
    }));
    let history = h.handle.timeline_events().await.unwrap();
    assert!(history.iter().any(|event| matches!(
        &event.kind,
        crate::TimelineEventKind::Messages(crate::MessageEvent {
            cause: crate::MessageCause::SkillLoadModel,
            ..
        })
    )));
    assert!(history.iter().any(|event| matches!(
        &event.kind,
        crate::TimelineEventKind::Messages(crate::MessageEvent {
            cause: crate::MessageCause::SkillLoadSettlement,
            ..
        })
    )));

    let (persistence, _persistence_records) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        history,
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("cold restore should preserve settlement without reviving the skill");
    let restored_surface = restored.get_conversation().await;
    assert!(
        restored_surface
            .iter()
            .all(|item| !matches!(item, ConversationItem::SkillLoad(_)))
    );
    assert!(
        restored_surface
            .iter()
            .all(|item| item.text_content() != "private unconsumed guidance")
    );
    assert_eq!(
        restored_surface
            .iter()
            .filter_map(|item| match item {
                ConversationItem::ToolResult(result) if result.tool_call_id == call_id => {
                    Some(result.content.as_ref())
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["Loaded skill content was not consumed before execution ended"]
    );
}

#[tokio::test]
async fn model_skill_batch_preserves_success_through_request_and_timeline_replay() {
    use sampling_types::{ApiBackend, SkillLoadSource, ToolCall};
    let h = TestHarness::new();
    seed_admitted_tool_batch(
        &h.handle,
        crate::TurnId(959),
        "skill-a",
        vec![
            ToolCall {
                id: "skill-a".into(),
                name: "load_skill".into(),
                arguments: r#"{"name":"review","args":""}"#.into(),
            },
            ToolCall {
                id: "write-b".into(),
                name: "write_file".into(),
                arguments: "{}".into(),
            },
        ],
    )
    .await;
    let mut skill = user_skill_load("model:skill-a", 0, "skill-a", "review", "full skill body");
    skill.source = SkillLoadSource::Model;
    let result =
        ConversationItem::tool_result("skill-a", sampling_types::skill_load_result_content(&skill));
    let revision = h.handle.get_surface_revision().await.unwrap();
    assert_eq!(
        h.handle
            .publish_model_skill_conditionally(
                result.clone(),
                skill.clone(),
                ConversationItem::tool_result("skill-a", "not loaded"),
                revision,
                128_000,
                4_096,
            )
            .await
            .unwrap(),
        crate::ConditionalToolResultOutcome::Accepted
    );
    let published = h.handle.timeline_events().await.unwrap().len();
    let revision = h.handle.get_surface_revision().await.unwrap();
    assert!(
        h.handle
            .publish_model_skill_conditionally(
                result,
                skill,
                ConversationItem::tool_result("skill-a", "not loaded"),
                revision,
                128_000,
                4_096,
            )
            .await
            .is_err(),
        "duplicate model metadata must not enter the Timeline"
    );
    assert_eq!(h.handle.timeline_events().await.unwrap().len(), published);
    h.handle
        .push_tool_result_durably(ConversationItem::tool_result(
            "write-b",
            "write succeeded exactly once",
        ))
        .await
        .unwrap();
    assert!(!h.handle.has_dangling_tool_calls().await);
    let before = h.handle.timeline_events().await.unwrap();
    let mut historical = crate::Timeline::from_events(before.clone()).unwrap();
    let mut legacy = historical.surface().to_vec();
    crate::compaction_utils::repair_tool_pairing_with_reason(
        &mut legacy,
        sampling_types::DanglingToolCallReason::UserCancelled,
    );
    assert!(
        !serde_json::to_string(&legacy)
            .unwrap()
            .contains("write succeeded exactly once")
    );
    historical
        .replace_all(legacy, crate::MessageCause::IntegrityRepair)
        .unwrap();
    crate::Timeline::from_events(historical.events().to_vec()).unwrap();
    let mut forged = crate::Timeline::from_events(before.clone()).unwrap();
    let mut altered = forged.surface().to_vec();
    altered[3] = ConversationItem::tool_result("write-b", "fabricated success");
    assert!(
        forged
            .replace_all(altered, crate::MessageCause::IntegrityRepair)
            .is_err()
    );
    let request = h
        .handle
        .build_request("skill-batch", vec![], None, None, None)
        .await
        .unwrap();
    let wire = wire_request_json(&request, &ApiBackend::Responses);
    assert!(wire.to_string().contains("write succeeded exactly once"));
    assert!(!wire.to_string().contains("cancelled by the user"));
    let after = h.handle.timeline_events().await.unwrap();
    assert!(
        after.len() > before.len(),
        "metadata ordering is a durable repair"
    );
    assert_eq!(
        serde_json::to_value(&after[..before.len()]).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    let replayed = crate::Timeline::from_events(after.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(replayed.surface()).unwrap(),
        serde_json::to_value(h.handle.get_conversation().await).unwrap()
    );
    h.handle
        .build_request("skill-batch-again", vec![], None, None, None)
        .await
        .unwrap();
    assert_eq!(h.handle.timeline_events().await.unwrap().len(), after.len());
    let (persistence, _records) = MockTimelinePersistence::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let restored = ChatStateActor::spawn_from_timeline(
        after,
        test_config(),
        Box::new(persistence),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let cold = restored
        .build_request("cold-skill-batch", vec![], None, None, None)
        .await
        .unwrap();
    assert!(
        wire_request_json(&cold, &ApiBackend::Responses)
            .to_string()
            .contains("write succeeded exactly once")
    );
}

#[tokio::test]
async fn chat_runtime_only_reasoning_recovery_is_acknowledged_once() {
    use sampling_types::{AgentMessage, ApiBackend};
    for fact in [
        ConversationItem::SkillLoad(user_skill_load("input", 0, "skill", "review", "body")),
        ConversationItem::received_agent_message(AgentMessage {
            receipt_id: "receipt".into(),
            source_session_id: "source".into(),
            target_session_id: "target".into(),
            message_id: "message".into(),
            reply_to: None,
            message: "body".into(),
        }),
    ] {
        let config = SamplingConfig {
            api_backend: ApiBackend::ChatCompletions,
            ..test_config()
        };
        let h =
            TestHarness::with_config(vec![ConversationItem::user("start"), fact], config.clone());
        assert_eq!(
            h.handle
                .enable_portable_reasoning(ApiBackend::Responses)
                .await,
            Some(false)
        );
        assert_eq!(
            h.handle
                .enable_portable_reasoning(ApiBackend::ChatCompletions)
                .await,
            Some(true)
        );
        assert_eq!(
            h.handle
                .enable_portable_reasoning(ApiBackend::ChatCompletions)
                .await,
            Some(false)
        );
        let request = h
            .handle
            .build_request("runtime-only", vec![], None, None, None)
            .await
            .unwrap();
        let wire = wire_request_json(&request, &ApiBackend::ChatCompletions);
        let assistant = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .unwrap();
        assert_eq!(assistant["reasoning_content"], "");
        assert!(h.handle.reset_continuation().await);
        assert_eq!(
            h.handle
                .enable_portable_reasoning(ApiBackend::ChatCompletions)
                .await,
            Some(false)
        );
        h.handle.replace_sampling_route(config);
        assert_eq!(
            h.handle
                .enable_portable_reasoning(ApiBackend::ChatCompletions)
                .await,
            Some(true)
        );
    }
    for fact in [ConversationItem::agent_message(vec![]), {
        let mut skill = user_skill_load("model:skill", 0, "skill", "review", "body");
        skill.source = sampling_types::SkillLoadSource::Model;
        ConversationItem::SkillLoad(skill)
    }] {
        let h = TestHarness::with_config(
            vec![fact],
            SamplingConfig {
                api_backend: ApiBackend::ChatCompletions,
                ..test_config()
            },
        );
        assert_eq!(
            h.handle
                .enable_portable_reasoning(ApiBackend::ChatCompletions)
                .await,
            Some(false)
        );
    }
}

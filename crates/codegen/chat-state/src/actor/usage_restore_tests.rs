//! Restoration of the open logical Turn's prompt usage across suspension.
//!
//! These tests cover the chat-state behavior only: the active prompt ledger
//! survives suspend/resume of the same logical Turn, attribution follows the
//! stable owner captured at admission, and nothing is inherited without a
//! valid runtime continuation of that exact Turn.

use std::time::Duration;

use sampling_types::{SamplingConfig, TokenUsage};

use crate::events::ChatStateEvent;
use crate::persistence::MockTimelinePersistence;
use crate::{ChatStateActor, TimelineEventKind, TurnId, UsagePromptOwner};

fn test_config() -> SamplingConfig {
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
        context_window: std::num::NonZeroU64::new(128_000).unwrap(),
        reasoning_effort: None,
        stream_tool_calls: None,
    }
}

/// Actor harness: keeps the event channel alive, mock persistence auto-acks.
struct Harness {
    handle: crate::handle::ChatStateHandle,
    _event_rx: tokio::sync::mpsc::UnboundedReceiver<ChatStateEvent>,
    _persistence_rx: crate::persistence::MockPersistenceReceiver,
    _cancellation_token: tokio_util::sync::CancellationToken,
}

impl Harness {
    fn new() -> Self {
        let (mock, _persistence_rx) = MockTimelinePersistence::new();
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let token = tokio_util::sync::CancellationToken::new();
        let handle = ChatStateActor::spawn(
            Vec::new(),
            test_config(),
            Box::new(mock),
            event_tx,
            token.clone(),
        );
        Self {
            handle,
            _event_rx: event_rx,
            _persistence_rx,
            _cancellation_token: token,
        }
    }
}

async fn restore(events: Vec<crate::TimelineEvent>) -> Harness {
    restore_with_timeline(crate::Timeline::from_events(events).unwrap()).await
}

async fn restore_with_timeline(timeline: crate::Timeline) -> Harness {
    let (mock, _persistence_rx) = MockTimelinePersistence::new();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let token = tokio_util::sync::CancellationToken::new();
    let handle = ChatStateActor::spawn_from_validated_timeline(
        timeline,
        test_config(),
        None,
        Box::new(mock),
        event_tx,
        token.clone(),
    )
    .await
    .unwrap();
    Harness {
        handle,
        _event_rx: event_rx,
        _persistence_rx,
        _cancellation_token: token,
    }
}

fn turn_identity() -> crate::TurnIdentity {
    crate::TurnIdentity {
        origin: "user".into(),
        turn_kind: "internal".into(),
        goal_id: None,
        goal_definition_revision: None,
        stage_id: None,
    }
}

/// Start (and leave open) one logical Turn at the next prompt position.
async fn start_turn(h: &Harness, turn: TurnId, prompt_index: usize) -> UsagePromptOwner {
    let owner = UsagePromptOwner {
        turn_id: turn,
        prompt_id: format!("prompt-{turn:?}"),
    };
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Turn(crate::TurnEvent::Started {
            id: turn,
            prompt_id: owner.prompt_id.clone(),
            input_ids: Vec::new(),
            identity: turn_identity(),
            model_id: "test-model".into(),
            input_message_count: 0,
            prompt_index,
            prompt_text: "in-flight turn".into(),
            input_kind: crate::TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .await
        .unwrap();
    owner
}

fn usage(prompt: u32, completion: u32) -> TokenUsage {
    TokenUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: prompt + completion,
        ..Default::default()
    }
}

fn continuation(
    owner: &UsagePromptOwner,
    prompt_index: usize,
) -> crate::session_runtime::TurnContinuation {
    crate::session_runtime::TurnContinuation {
        turn_id: owner.turn_id,
        prompt_id: owner.prompt_id.clone(),
        prompt_index,
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
    }
}

/// Durably move the runtime into Suspended with a checkpoint continuation.
async fn suspend_turn(h: &Harness, owner: &UsagePromptOwner, prompt_index: usize) {
    let operation = format!("op-{turn:?}", turn = owner.turn_id);
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::SuspendRequested {
                operation_id: operation.clone(),
                root_session_id: "test-session".into(),
            },
        ))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::CommandsSettled {
                operation_id: operation.clone(),
                commands: Vec::new(),
            },
        ))
        .await
        .unwrap();
    h.handle
        .checkpoint_runtime(
            operation,
            "b".repeat(64),
            Some(continuation(owner, prompt_index)),
            None,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
}

async fn settle_main(
    h: &Harness,
    attempt: &str,
    prompt_index: usize,
    owner: &UsagePromptOwner,
    usage: TokenUsage,
) {
    h.handle
        .settle_model_attempt_usage(
            attempt.into(),
            prompt_index,
            Some(owner.clone()),
            "model-a".into(),
            Some(usage),
            None,
            None,
        )
        .await
        .unwrap();
}

async fn prompt_totals_of(h: &Harness) -> Option<(u64, bool)> {
    h.handle
        .try_get_prompt_usage()
        .await
        .unwrap()
        .map(|ledger| (ledger.totals.total_tokens(), ledger.incomplete))
}

#[tokio::test]
async fn same_turn_resume_keeps_settled_prompt_usage() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(10), 0).await;
    settle_main(&h, "attempt-1", 0, &owner, usage(90, 10)).await;
    suspend_turn(&h, &owner, 0).await;

    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;

    // The settled consumption survives the resume of the same logical Turn,
    // while the single-call calibration stays unknown until a real call.
    assert_eq!(prompt_totals_of(&restored).await, Some((100, false)));
    assert_eq!(
        restored.handle.current_usage_prompt_owner().await,
        Some((0, owner.clone()))
    );
    assert!(restored.handle.get_last_turn_usage().await.is_none());

    settle_main(&restored, "attempt-2", 0, &owner, usage(45, 5)).await;
    assert_eq!(prompt_totals_of(&restored).await, Some((150, false)));
    assert_eq!(
        restored
            .handle
            .try_get_session_usage()
            .await
            .unwrap()
            .totals
            .total_tokens(),
        150
    );
}

#[tokio::test]
async fn usage_checkpoint_cut_restores_active_prompt_and_folds_suffix_once() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(20), 0).await;
    settle_main(&h, "cut-attempt", 0, &owner, usage(90, 10)).await;
    suspend_turn(&h, &owner, 0).await;
    let events = h.handle.timeline_events().await.unwrap();

    // Rebuild the published accounting cut the way the journal index does:
    // fold the prefix that also contains the suspension, seal a frontier.
    let mut cut = crate::actor::state::UsageCheckpoint::default();
    for event in &events {
        cut.publish_fact(event, |_| Ok(None), |_| Ok(None), |_| Ok(true))
            .unwrap();
    }
    let last_seq = events.last().unwrap().seq.get();
    cut.set_frontier(crate::EventSeq::new(last_seq + 1));
    let roundtrip = serde_json::from_value::<crate::actor::state::UsageCheckpoint>(
        serde_json::to_value(&cut).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&roundtrip).unwrap(),
        serde_json::to_value(&cut).unwrap()
    );

    let mut timeline = crate::Timeline::from_events(events).unwrap();
    timeline.set_usage_checkpoint(cut);
    let restored = restore_with_timeline(timeline).await;
    assert_eq!(prompt_totals_of(&restored).await, Some((100, false)));

    settle_main(&restored, "post-cut-attempt", 0, &owner, usage(45, 5)).await;
    // The suffix after the cut settles on top of the carried ledger exactly
    // once; a re-folded prefix would report 250.
    assert_eq!(prompt_totals_of(&restored).await, Some((150, false)));
}

#[tokio::test]
async fn attribution_follows_owner_for_main_aux_and_child() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(30), 0).await;
    let stale = UsagePromptOwner {
        turn_id: TurnId(29),
        prompt_id: "prompt-older".into(),
    };

    // Main attempt for the current prompt.
    settle_main(&h, "main-current", 0, &owner, usage(50, 5)).await;
    // Explicit session-only main attempt: never enters the prompt ledger.
    h.handle
        .settle_model_attempt_usage(
            "main-session-only".into(),
            0,
            None,
            "model-a".into(),
            Some(usage(500, 50)),
            None,
            None,
        )
        .await
        .unwrap();

    // Auxiliary attempt attributed by its start.
    let sideband = uuid::Uuid::now_v7().to_string();
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Sideband(crate::SidebandSpawnEvent {
            sideband_id: sideband.clone(),
            purpose: crate::SidebandPurpose::PermissionJudgment,
            source_refs: Vec::new(),
        }))
        .await
        .unwrap();
    h.handle
        .begin_auxiliary_attempt_usage(
            sideband.clone(),
            1,
            "aux-model".into(),
            Some(0),
            Some(owner.clone()),
        )
        .await
        .unwrap();
    // Session-only auxiliary start (explicit null) settles but stays out.
    let sideband_none = uuid::Uuid::now_v7().to_string();
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Sideband(crate::SidebandSpawnEvent {
            sideband_id: sideband_none.clone(),
            purpose: crate::SidebandPurpose::PermissionJudgment,
            source_refs: Vec::new(),
        }))
        .await
        .unwrap();
    h.handle
        .begin_auxiliary_attempt_usage(sideband_none.clone(), 1, "aux-model".into(), None, None)
        .await
        .unwrap();
    h.handle
        .settle_auxiliary_attempt_usage(sideband_none, 1, Some(usage(70, 7)), None, None)
        .await
        .unwrap();

    // Child bills: one for the current owner, one pinned to an older Turn.
    for (child, bill_owner, tokens) in [
        ("child-current", Some(owner.clone()), 30u32),
        ("child-stale", Some(stale), 400),
    ] {
        h.handle
            .record_subagent_usage(
                child.into(),
                vec![(
                    "child-model".into(),
                    crate::usage::UsageTotals {
                        input_tokens: u64::from(tokens),
                        output_tokens: 3,
                        model_calls: 1,
                        ..Default::default()
                    },
                )],
                bill_owner,
                false,
            )
            .await
            .unwrap();
    }

    suspend_turn(&h, &owner, 0).await;
    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    // 55 main + 0 session-only + 33 child-current; the aux start for the
    // current owner settles after the restore below.
    assert_eq!(prompt_totals_of(&restored).await, Some((88, false)));

    restored
        .handle
        .settle_auxiliary_attempt_usage(sideband, 1, Some(usage(20, 2)), None, None)
        .await
        .unwrap();
    assert_eq!(prompt_totals_of(&restored).await, Some((110, false)));

    let session = restored.handle.try_get_session_usage().await.unwrap();
    // Every settled fact lands in lifetime once: 55 main + 550 session-only
    // main + 77 session-only aux + 22 owner aux + 33 + 403 child bills.
    assert_eq!(session.totals.total_tokens(), 55 + 550 + 77 + 22 + 33 + 403);
}

#[tokio::test]
async fn legacy_fact_without_owner_keeps_lifetime_and_lower_bounds_prompt() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(40), 0).await;

    // A settlement whose payload predates the `prompt_owner` field. Build the
    // JSON from a full settlement, then strip the field so the raw fact is
    // genuinely unattributed rather than session-only.
    let mut data = serde_json::to_value(crate::actor::state::AttemptUsageSettlement {
        attempt_key: "legacy-attempt".into(),
        model_id: "model-a".into(),
        captured_prompt_index: 0,
        prompt_owner: Some(owner.clone()),
        usage: Some(usage(90, 10)),
        cost_usd_ticks: None,
        api_duration_ms: None,
    })
    .unwrap();
    data.as_object_mut().unwrap().remove("prompt_owner");
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Observation(crate::ObservationEvent {
            scope: "sampling_usage".into(),
            name: "attempt_settled".into(),
            turn: None,
            step: None,
            data: Some(data),
        }))
        .await
        .unwrap();

    suspend_turn(&h, &owner, 0).await;
    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;

    // Unknown attribution keeps the consumption in lifetime and restores the
    // prompt ledger only as a lower bound: incomplete, without the tokens.
    assert_eq!(prompt_totals_of(&restored).await, Some((0, true)));
    assert_eq!(
        restored
            .handle
            .try_get_session_usage()
            .await
            .unwrap()
            .totals
            .total_tokens(),
        100
    );
}

#[tokio::test]
async fn replayed_settlement_after_restore_is_idempotent() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(50), 0).await;
    settle_main(&h, "attempt-replay", 0, &owner, usage(80, 20)).await;
    suspend_turn(&h, &owner, 0).await;

    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    assert_eq!(
        restored
            .handle
            .settle_model_attempt_usage(
                "attempt-replay".into(),
                0,
                Some(owner.clone()),
                "model-a".into(),
                Some(usage(80, 20)),
                None,
                None,
            )
            .await
            .unwrap(),
        false
    );
    assert_eq!(prompt_totals_of(&restored).await, Some((100, false)));
    assert_eq!(
        restored
            .handle
            .try_get_session_usage()
            .await
            .unwrap()
            .totals
            .total_tokens(),
        100
    );
}

#[tokio::test]
async fn rewind_reused_index_does_not_mix_turns() {
    let h = Harness::new();
    // Rewind selects a branch cut at a prompt marker, so the position needs a
    // marked user message on the Surface.
    let mut prompt = sampling_types::ConversationItem::user("rewind me");
    prompt.set_prompt_index(0);
    h.handle.push_user_message_durably(prompt).await.unwrap();
    let first = start_turn(&h, TurnId(60), 0).await;
    settle_main(&h, "rewound-attempt", 0, &first, usage(90, 10)).await;
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Turn(crate::TurnEvent::Ended {
            id: first.turn_id,
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
    h.handle.rewind_durably(0).await.unwrap();

    let second = start_turn(&h, TurnId(61), 0).await;
    settle_main(&h, "reused-attempt", 0, &second, usage(25, 5)).await;
    suspend_turn(&h, &second, 0).await;

    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    // Only the Turn that owns the reused position contributes; mixing the
    // rewound branch would report 130.
    assert_eq!(prompt_totals_of(&restored).await, Some((30, false)));
    assert_eq!(
        restored.handle.current_usage_prompt_owner().await,
        Some((0, second))
    );
}

#[tokio::test]
async fn without_matching_continuation_nothing_is_inherited() {
    // Open Turn with settled usage but no runtime continuation: an ordinary
    // restart starts fresh instead of inheriting the old ledger.
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(70), 0).await;
    settle_main(&h, "open-attempt", 0, &owner, usage(90, 10)).await;
    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    assert_eq!(prompt_totals_of(&restored).await, None);
    assert_eq!(restored.handle.current_usage_prompt_owner().await, None);

    // A continuation whose index no longer matches the active projection is
    // not a valid handoff for that ledger.
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(71), 0).await;
    settle_main(&h, "index-attempt", 0, &owner, usage(90, 10)).await;
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::SuspendRequested {
                operation_id: "op-index".into(),
                root_session_id: "test-session".into(),
            },
        ))
        .await
        .unwrap();
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Runtime(
            crate::session_runtime::SessionRuntimeEvent::CommandsSettled {
                operation_id: "op-index".into(),
                commands: Vec::new(),
            },
        ))
        .await
        .unwrap();
    h.handle
        .checkpoint_runtime(
            "op-index".into(),
            "b".repeat(64),
            Some(continuation(&owner, 99)),
            None,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    assert_eq!(prompt_totals_of(&restored).await, None);
}

#[tokio::test]
async fn cut_without_active_prompt_field_is_unknown_not_zero() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(80), 0).await;
    settle_main(&h, "cut-attempt", 0, &owner, usage(90, 10)).await;
    suspend_turn(&h, &owner, 0).await;
    let events = h.handle.timeline_events().await.unwrap();

    // A cut written before the active-prompt projection existed cannot prove
    // attribution; the restore must stay unknown instead of an exact zero.
    let mut cut = crate::actor::state::UsageCheckpoint::default();
    for event in &events {
        cut.publish_fact(event, |_| Ok(None), |_| Ok(None), |_| Ok(true))
            .unwrap();
    }
    let last_seq = events.last().unwrap().seq.get();
    cut.set_frontier(crate::EventSeq::new(last_seq + 1));
    let mut legacy = serde_json::to_value(&cut).unwrap();
    legacy.as_object_mut().unwrap().remove("active_prompt");
    let legacy: crate::actor::state::UsageCheckpoint = serde_json::from_value(legacy).unwrap();

    let mut timeline = crate::Timeline::from_events(events).unwrap();
    timeline.set_usage_checkpoint(legacy);
    let restored = restore_with_timeline(timeline).await;
    assert_eq!(prompt_totals_of(&restored).await, None);
}

#[tokio::test]
async fn resumed_prompt_settlement_keeps_last_turn_usage_semantics() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(90), 0).await;
    settle_main(&h, "calibration-attempt", 0, &owner, usage(90, 10)).await;
    assert_eq!(
        h.handle
            .get_last_turn_usage()
            .await
            .map(|usage| usage.total_tokens),
        Some(100)
    );
    suspend_turn(&h, &owner, 0).await;

    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    // last_turn_usage calibrates the next real model call; it is never
    // reconstructed from the restored ledger totals.
    assert!(restored.handle.get_last_turn_usage().await.is_none());

    settle_main(&restored, "next-attempt", 0, &owner, usage(30, 5)).await;
    assert_eq!(
        restored
            .handle
            .get_last_turn_usage()
            .await
            .map(|usage| usage.total_tokens),
        Some(35)
    );
}

#[tokio::test]
async fn prompt_incomplete_marker_survives_suspend_and_restore() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(100), 0).await;
    settle_main(&h, "fail-closed-attempt", 0, &owner, usage(90, 10)).await;
    assert!(h.handle.mark_usage_incomplete(true, true).await);
    assert_eq!(prompt_totals_of(&h).await, Some((100, true)));
    suspend_turn(&h, &owner, 0).await;

    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    // The failure mark comes from real unknown consumption, not a transient
    // pending child: it must stay durable across the suspension instead of
    // letting the restored ledger look complete and falsely exact.
    assert_eq!(prompt_totals_of(&restored).await, Some((100, true)));
    assert_eq!(
        restored.handle.current_usage_prompt_owner().await,
        Some((0, owner))
    );
    assert!(
        restored
            .handle
            .try_get_session_usage()
            .await
            .unwrap()
            .incomplete
    );
}

#[tokio::test]
async fn prompt_mark_after_early_session_incomplete_is_still_durable() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(101), 0).await;
    // A session-only stain lands first (e.g. an apply miss with no live
    // prompt owner match); it must not touch the prompt ledger.
    assert!(h.handle.mark_usage_incomplete(false, true).await);
    settle_main(&h, "later-attempt", 0, &owner, usage(90, 10)).await;
    assert_eq!(prompt_totals_of(&h).await, Some((100, false)));

    // The session already carries its lower bound; the prompt mark itself is
    // still a new fact and must commit its own owner-bound marker.
    assert!(h.handle.mark_usage_incomplete(true, true).await);
    assert_eq!(prompt_totals_of(&h).await, Some((100, true)));
    suspend_turn(&h, &owner, 0).await;

    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    assert_eq!(prompt_totals_of(&restored).await, Some((100, true)));
    assert!(
        restored
            .handle
            .try_get_session_usage()
            .await
            .unwrap()
            .incomplete
    );
}

#[tokio::test]
async fn marker_of_earlier_turn_is_not_inherited_by_same_position() {
    let h = Harness::new();
    // Rewind selects a branch cut at a prompt marker, so the position needs
    // a marked user message on the Surface.
    let mut prompt = sampling_types::ConversationItem::user("rewind me");
    prompt.set_prompt_index(0);
    h.handle.push_user_message_durably(prompt).await.unwrap();
    let first = start_turn(&h, TurnId(102), 0).await;
    settle_main(&h, "first-attempt", 0, &first, usage(90, 10)).await;
    assert!(h.handle.mark_usage_incomplete(true, true).await);
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Turn(crate::TurnEvent::Ended {
            id: first.turn_id,
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
    h.handle.rewind_durably(0).await.unwrap();

    let second = start_turn(&h, TurnId(103), 0).await;
    settle_main(&h, "second-attempt", 0, &second, usage(20, 5)).await;
    suspend_turn(&h, &second, 0).await;

    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    // The marker names the first Turn's owner; the Turn reusing the same
    // prompt position starts from its own facts only.
    assert_eq!(prompt_totals_of(&restored).await, Some((25, false)));
    assert!(
        restored
            .handle
            .try_get_session_usage()
            .await
            .unwrap()
            .incomplete
    );
}

#[tokio::test]
async fn session_only_marker_does_not_stain_active_prompt() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(104), 0).await;
    settle_main(&h, "clean-attempt", 0, &owner, usage(90, 10)).await;
    assert!(h.handle.mark_usage_incomplete(false, true).await);
    assert_eq!(prompt_totals_of(&h).await, Some((100, false)));
    suspend_turn(&h, &owner, 0).await;

    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    // An explicit session-only marker (`prompt_owner: null`) never adopts
    // the prompt that happens to be open at replay time.
    assert_eq!(prompt_totals_of(&restored).await, Some((100, false)));
    assert!(
        restored
            .handle
            .try_get_session_usage()
            .await
            .unwrap()
            .incomplete
    );
}

#[tokio::test]
async fn marker_folds_identically_through_checkpoint_cut_and_suffix() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(105), 0).await;
    settle_main(&h, "cut-attempt", 0, &owner, usage(90, 10)).await;
    assert!(h.handle.mark_usage_incomplete(true, true).await);
    suspend_turn(&h, &owner, 0).await;
    let events = h.handle.timeline_events().await.unwrap();

    // Restore from the raw stream: the marker replays as an ordinary fact.
    let plain = restore(events.clone()).await;
    assert_eq!(prompt_totals_of(&plain).await, Some((100, true)));

    // Seal the published cut just before the marker so the checkpoint
    // carries the settlement and the marker itself folds as suffix.
    let marker_seq = events
        .iter()
        .find(|event| {
            matches!(
                &event.kind,
                TimelineEventKind::Observation(observation)
                    if observation.scope == "session_usage"
                        && observation.name == "incomplete"
            )
        })
        .unwrap()
        .seq
        .get();
    let mut cut = crate::actor::state::UsageCheckpoint::default();
    for event in &events {
        if event.seq.get() >= marker_seq {
            break;
        }
        cut.publish_fact(event, |_| Ok(None), |_| Ok(None), |_| Ok(true))
            .unwrap();
    }
    cut.set_frontier(crate::EventSeq::new(marker_seq));
    let mut timeline = crate::Timeline::from_events(events).unwrap();
    timeline.set_usage_checkpoint(cut);
    let cut_restored = restore_with_timeline(timeline).await;
    assert_eq!(prompt_totals_of(&cut_restored).await, Some((100, true)));
}

#[tokio::test]
async fn legacy_payloadless_marker_keeps_lifetime_only_semantics() {
    let h = Harness::new();
    let owner = start_turn(&h, TurnId(106), 0).await;
    settle_main(&h, "legacy-attempt", 0, &owner, usage(90, 10)).await;
    // A marker written before the explicit payload existed.
    h.handle
        .record_timeline_event_durably(TimelineEventKind::Observation(crate::ObservationEvent {
            scope: "session_usage".into(),
            name: "incomplete".into(),
            turn: None,
            step: None,
            data: None,
        }))
        .await
        .unwrap();
    suspend_turn(&h, &owner, 0).await;

    let events = h.handle.timeline_events().await.unwrap();
    let restored = restore(events).await;
    // The payload-less marker only ever established the lifetime lower
    // bound; without an owner there is nothing to attribute to the open
    // prompt, and guessing from the current Turn is forbidden.
    assert_eq!(prompt_totals_of(&restored).await, Some((100, false)));
    assert!(
        restored
            .handle
            .try_get_session_usage()
            .await
            .unwrap()
            .incomplete
    );
}

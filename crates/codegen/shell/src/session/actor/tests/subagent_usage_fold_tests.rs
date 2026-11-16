//! Subagent usage attribution and incomplete-bill gates.
use super::support::*;
use super::*;

async fn make_actor() -> SessionActor {
    let (gateway_tx, _gateway_rx) =
        tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
    let (persistence_tx, _persistence_rx) =
        tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await
}

fn usage_rows() -> Vec<(String, chat_state::UsageTotals)> {
    vec![(
        "m".into(),
        chat_state::UsageTotals {
            input_tokens: 40,
            model_calls: 1,
            ..Default::default()
        },
    )]
}

/// A settlement stamp that never matches the live prompt owner: proves
/// attribution is bound to the captured Turn, not to a reused prompt id.
fn next_owner_stale(_live: &chat_state::UsagePromptOwner) -> chat_state::UsagePromptOwner {
    chat_state::UsagePromptOwner {
        turn_id: chat_state::TurnId(424_242),
        prompt_id: "stale-prompt-id".into(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn subagent_usage_fold_attribution_gate() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = make_actor().await;
            let usage = usage_rows();

            let owner = start_test_turn(&actor, "first prompt").await;
            *actor.current_prompt_id.lock().unwrap() = Some(owner.prompt_id.clone());
            assert_eq!(
                actor
                    .record_subagent_usage("sub-1", &usage, Some(owner.clone()), false)
                    .await,
                Ok(super::updates::SubagentUsageApply::AttributedToPrompt)
            );
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(prompt.totals.input_tokens, 40);
            assert_eq!(
                prompt.by_agent[&chat_state::UsageAgent::Subagent("sub-1".into())].input_tokens,
                40
            );

            end_test_turn(&actor, &owner).await;
            let next_owner = start_test_turn(&actor, "next prompt").await;
            for (child, stamped) in [("sub-2", Some(next_owner_stale(&owner))), ("sub-3", None)] {
                *actor.current_prompt_id.lock().unwrap() = Some(next_owner.prompt_id.clone());
                // Session-only: ledger apply ok, not attributed to live prompt.
                assert_eq!(
                    actor
                        .record_subagent_usage(child, &usage, stamped, false)
                        .await,
                    Ok(super::updates::SubagentUsageApply::SessionOnly)
                );
                assert!(
                    actor
                        .chat_state_handle
                        .try_get_prompt_usage()
                        .await
                        .ok()
                        .flatten()
                        .is_none()
                );
            }
            // Live owner becomes unavailable once the turn ends; an old stamp
            // must stay session-only instead of binding the ended turn.
            end_test_turn(&actor, &next_owner).await;
            *actor.current_prompt_id.lock().unwrap() = None;
            assert_eq!(
                actor
                    .record_subagent_usage("sub-4", &usage, Some(next_owner_stale(&owner)), false)
                    .await,
                Ok(super::updates::SubagentUsageApply::SessionOnly)
            );
            // A repeated terminal receipt is idempotent even after the live
            // prompt changes; it must not add another child's worth of usage.
            assert_eq!(
                actor
                    .record_subagent_usage("sub-1", &usage, Some(owner), false)
                    .await,
                Ok(super::updates::SubagentUsageApply::SessionOnly)
            );
            let session = actor
                .chat_state_handle
                .try_get_session_usage()
                .await
                .expect("chat-state actor alive");
            assert_eq!(session.totals.input_tokens, 160);
            for child in ["sub-1", "sub-2", "sub-3", "sub-4"] {
                assert_eq!(
                    session.by_agent[&chat_state::UsageAgent::Subagent(child.into())].input_tokens,
                    40
                );
            }
        })
        .await;
}

/// One matrix for the shared freeze/cancel outcome policy, including the
/// `usage_incomplete_from_reply` wrapper the error path uses.
#[test]
fn usage_drain_outcome_policy_matches_freeze_and_cancel() {
    use super::turn::UsageDrainOutcome;
    use tools::implementations::grow_build::task::types::SubagentOutstandingReply;

    let none = UsageDrainOutcome::from_outstanding_reply(None);
    assert!(none.fail_closed);
    assert!(none.report_incomplete());
    assert!(SessionActor::usage_incomplete_from_reply(None));

    let fg_reply = SubagentOutstandingReply {
        live_ids: vec!["s1".into()],
        background_live: false,
        subagent_usage_not_applied: false,
    };
    let fg = UsageDrainOutcome::from_outstanding_reply(Some(&fg_reply));
    assert!(fg.fail_closed);
    assert!(fg.report_incomplete());
    assert!(SessionActor::usage_incomplete_from_reply(Some(&fg_reply)));

    let sticky_reply = SubagentOutstandingReply {
        live_ids: vec![],
        background_live: false,
        subagent_usage_not_applied: true,
    };
    let sticky = UsageDrainOutcome::from_outstanding_reply(Some(&sticky_reply));
    assert!(!sticky.fail_closed, "sticky is report-only");
    assert!(sticky.sticky_report);
    assert!(sticky.report_incomplete());
    assert!(SessionActor::usage_incomplete_from_reply(Some(
        &sticky_reply
    )));

    let bg_reply = SubagentOutstandingReply {
        live_ids: vec![],
        background_live: true,
        subagent_usage_not_applied: false,
    };
    let bg = UsageDrainOutcome::from_outstanding_reply(Some(&bg_reply));
    assert!(!bg.fail_closed, "background is report-only");
    assert!(bg.background_live);
    assert!(bg.report_incomplete());
    assert!(SessionActor::usage_incomplete_from_reply(Some(&bg_reply)));

    let clean_reply = SubagentOutstandingReply {
        live_ids: vec![],
        background_live: false,
        subagent_usage_not_applied: false,
    };
    let clean = UsageDrainOutcome::from_outstanding_reply(Some(&clean_reply));
    assert!(!clean.fail_closed);
    assert!(!clean.report_incomplete());
    assert!(!SessionActor::usage_incomplete_from_reply(Some(
        &clean_reply
    )));
}

#[test]
fn project_from_ledger_never_drops_incomplete_flag() {
    use crate::extensions::notification::PromptUsage;

    assert!(PromptUsage::project_from_ledger(None, false).is_none());
    assert!(
        PromptUsage::project_from_ledger(None, true)
            .unwrap()
            .usage_is_incomplete
    );

    let mut ledger = chat_state::UsageLedger::default();
    ledger.record_main_loop_call(
        "m",
        &sampling_types::TokenUsage {
            prompt_tokens: 3,
            completion_tokens: 1,
            total_tokens: 4,
            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
            cache_read_known: true,
            cache_creation_prompt_tokens: 0,
            cache_write_known: true,
        },
        None,
        None,
    );
    ledger.record_auxiliary_call(
        "recap",
        &sampling_types::TokenUsage {
            prompt_tokens: 7,
            completion_tokens: 2,
            total_tokens: 9,
            ..Default::default()
        },
        None,
        None,
    );
    let complete = PromptUsage::project_from_ledger(Some(&ledger), false).unwrap();
    assert!(!complete.usage_is_incomplete);
    assert_eq!(complete.totals.input_tokens, 10);
    assert_eq!(complete.totals.output_tokens, 3);
    assert_eq!(complete.num_turns, 1);

    let marked = PromptUsage::project_from_ledger(Some(&ledger), true).unwrap();
    assert!(marked.usage_is_incomplete);
    assert_eq!(marked.totals.input_tokens, 10);
}

#[tokio::test(flavor = "current_thread")]
async fn nested_incomplete_fold_marks_parent_ledger() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = make_actor().await;
            let owner = start_test_turn(&actor, "live incomplete").await;
            *actor.current_prompt_id.lock().unwrap() = Some(owner.prompt_id.clone());
            let usage = usage_rows();
            assert_eq!(
                actor
                    .record_subagent_usage("sub-1", &usage, Some(owner), true)
                    .await,
                Ok(super::updates::SubagentUsageApply::AttributedToPrompt)
            );
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .expect("prompt ledger");
            assert!(prompt.incomplete);
            assert_eq!(prompt.totals.input_tokens, 40);
            assert!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .expect("chat-state actor alive")
                    .incomplete
            );
        })
        .await;
}

#[test]
fn for_error_path_shared_policy() {
    use crate::extensions::notification::PromptUsage;

    assert!(PromptUsage::for_error_path(None, false).is_none());
    assert!(
        PromptUsage::for_error_path(None, true)
            .unwrap()
            .usage_is_incomplete
    );

    let mut ledger = chat_state::UsageLedger::default();
    ledger.record_main_loop_call(
        "m",
        &sampling_types::TokenUsage {
            prompt_tokens: 5,
            completion_tokens: 1,
            total_tokens: 6,
            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
            cache_read_known: true,
            cache_creation_prompt_tokens: 0,
            cache_write_known: true,
        },
        None,
        None,
    );
    let with_ledger = PromptUsage::for_error_path(Some(&ledger), false).unwrap();
    assert!(with_ledger.usage_is_incomplete);
    assert_eq!(with_ledger.totals.input_tokens, 5);
}

#[tokio::test(flavor = "current_thread")]
async fn error_path_omits_usage_when_never_billed() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = make_actor().await;
            assert!(actor.error_path_usage_fallback("p-1").await.is_none());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn error_path_marks_incomplete_when_ledger_open() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = make_actor().await;
            actor.chat_state_handle.record_model_call_usage(
                Some("m".into()),
                sampling_types::TokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 2,
                    total_tokens: 12,
                    reasoning_tokens: 0,
                    cached_prompt_tokens: 0,
                    cache_read_known: true,
                    cache_creation_prompt_tokens: 0,
                    cache_write_known: true,
                },
                None,
                None,
            );
            let usage = actor.error_path_usage_fallback("p-1").await.unwrap();
            assert!(usage.usage_is_incomplete);
            assert_eq!(usage.totals.input_tokens, 10);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn session_only_incomplete_does_not_stain_live_open_prompt() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = make_actor().await;
            let owner = start_test_turn(&actor, "live open prompt").await;
            *actor.current_prompt_id.lock().unwrap() = Some(owner.prompt_id.clone());
            // Open a live prompt ledger via a main-loop call.
            actor.chat_state_handle.record_model_call_usage(
                Some("m".into()),
                sampling_types::TokenUsage {
                    prompt_tokens: 7,
                    completion_tokens: 1,
                    total_tokens: 8,
                    reasoning_tokens: 0,
                    cached_prompt_tokens: 0,
                    cache_read_known: true,
                    cache_creation_prompt_tokens: 0,
                    cache_write_known: true,
                },
                None,
                None,
            );
            let usage = usage_rows();
            // Stamped owner ≠ live owner → session-only.
            assert_eq!(
                actor
                    .record_subagent_usage("sub-1", &usage, Some(next_owner_stale(&owner)), true)
                    .await,
                Ok(super::updates::SubagentUsageApply::SessionOnly)
            );
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .expect("prompt ledger");
            assert!(
                !prompt.incomplete,
                "live open prompt must not inherit session-only incomplete"
            );
            assert_eq!(prompt.totals.input_tokens, 7);
            assert!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .expect("chat-state actor alive")
                    .incomplete
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn snapshot_ors_ledger_incomplete_even_when_reply_complete() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = make_actor().await;
            let owner = start_test_turn(&actor, "live incomplete").await;
            *actor.current_prompt_id.lock().unwrap() = Some(owner.prompt_id.clone());
            let usage = usage_rows();
            assert_eq!(
                actor
                    .record_subagent_usage("sub-1", &usage, Some(owner), true)
                    .await,
                Ok(super::updates::SubagentUsageApply::AttributedToPrompt)
            );
            // Orchestration says complete (no live/sticky); ledger still incomplete.
            let snap = actor.snapshot_prompt_usage_marked(false).await.unwrap();
            assert!(snap.usage_is_incomplete);
            assert_eq!(snap.totals.input_tokens, 40);
        })
        .await;
}

/// Restoring an already-billed suspended Turn must not need another sampling
/// request before Shell can freeze its complete result usage.
#[tokio::test(flavor = "current_thread")]
async fn restored_turn_freezes_main_aux_and_child_without_new_sampling() {
    use crate::extensions::notification::SessionUpdate;
    use chat_state::session_runtime::{SessionRuntimeEvent, TurnContinuation, TurnResumePoint};
    use chat_state::{TimelineEventKind, UsageAgent};

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = make_actor().await;
            let prompt_index = actor.chat_state_handle.get_prompt_index().await;
            let owner = start_test_turn(&actor, "suspend after all bills settled").await;
            let token_usage = |input, output| sampling_types::TokenUsage {
                prompt_tokens: input,
                completion_tokens: output,
                total_tokens: input + output,
                ..Default::default()
            };
            // Only the first bill belongs to this Turn. Explicit session-only
            // and stale-owner bills must survive in lifetime, outside its result.
            for (attempt, bill_owner, input, output) in [
                ("main-current", Some(owner.clone()), 50, 5),
                ("main-session-only", None, 500, 50),
            ] {
                actor
                    .chat_state_handle
                    .settle_model_attempt_usage(
                        attempt.into(),
                        prompt_index,
                        bill_owner,
                        "main-model".into(),
                        Some(token_usage(input, output)),
                        None,
                        None,
                    )
                    .await
                    .unwrap();
            }
            for (bill_owner, input, output) in [(Some(owner.clone()), 20, 2), (None, 70, 7)] {
                let sideband_id = uuid::Uuid::now_v7().to_string();
                actor
                    .chat_state_handle
                    .record_timeline_event_durably(TimelineEventKind::Sideband(
                        chat_state::SidebandSpawnEvent {
                            sideband_id: sideband_id.clone(),
                            purpose: chat_state::SidebandPurpose::PermissionJudgment,
                            source_refs: Vec::new(),
                        },
                    ))
                    .await
                    .unwrap();
                actor
                    .chat_state_handle
                    .begin_auxiliary_attempt_usage(
                        sideband_id.clone(),
                        1,
                        "aux-model".into(),
                        bill_owner.as_ref().map(|_| prompt_index),
                        bill_owner,
                    )
                    .await
                    .unwrap();
                actor
                    .chat_state_handle
                    .settle_auxiliary_attempt_usage(
                        sideband_id,
                        1,
                        Some(token_usage(input, output)),
                        None,
                        None,
                    )
                    .await
                    .unwrap();
            }
            for (child, bill_owner, input) in [
                ("child-current", owner.clone(), 30),
                ("child-stale", next_owner_stale(&owner), 400),
            ] {
                actor
                    .chat_state_handle
                    .record_subagent_usage(
                        child.into(),
                        vec![(
                            "child-model".into(),
                            chat_state::UsageTotals {
                                input_tokens: input,
                                output_tokens: 3,
                                model_calls: 1,
                                cost_missing_calls: 1,
                                ..Default::default()
                            },
                        )],
                        Some(bill_owner),
                        false,
                    )
                    .await
                    .unwrap();
            }
            let operation_id = "suspend-before-shell-freeze".to_string();
            for runtime_event in [
                SessionRuntimeEvent::SuspendRequested {
                    operation_id: operation_id.clone(),
                    root_session_id: actor.session_id_string(),
                },
                SessionRuntimeEvent::CommandsSettled {
                    operation_id: operation_id.clone(),
                    commands: Vec::new(),
                },
            ] {
                actor
                    .chat_state_handle
                    .record_timeline_event_durably(TimelineEventKind::Runtime(runtime_event))
                    .await
                    .unwrap();
            }
            actor
                .chat_state_handle
                .checkpoint_runtime(
                    operation_id,
                    "b".repeat(64),
                    Some(TurnContinuation {
                        turn_id: owner.turn_id,
                        prompt_id: owner.prompt_id.clone(),
                        prompt_index,
                        origin: "user".into(),
                        next_step_index: 0,
                        tool_turns_used: 0,
                        tool_calls_used: 0,
                        stop_continuations_used: 0,
                        output_schema: None,
                        resume_point: TurnResumePoint::Model,
                        task_output_usage: None,
                        accepted_response: None,
                        structured_output_retries: 0,
                        sampling_attempts_used: 0,
                        sampling_max_attempts: Some(3),
                        sampling_replay_safe: true,
                        sampling_deadline_ms: None,
                        operations: Vec::new(),
                    }),
                    None,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(1),
                )
                .await
                .unwrap();
            let lifetime = actor
                .chat_state_handle
                .try_get_session_usage()
                .await
                .unwrap();
            assert_eq!(
                lifetime.totals.total_tokens(),
                55 + 550 + 22 + 77 + 33 + 403
            );
            let events = actor.chat_state_handle.timeline_events().await.unwrap();
            let suspended_facts = serde_json::to_value(&events).unwrap();
            let config = actor.chat_state_handle.get_sampling_config().await.unwrap();
            let (persistence, _persistence_rx) =
                chat_state::persistence::MockTimelinePersistence::new();
            let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
            let cancellation_token = tokio_util::sync::CancellationToken::new();
            let restored = chat_state::ChatStateActor::spawn_from_validated_timeline(
                chat_state::Timeline::from_events(events).unwrap(),
                config,
                None,
                Box::new(persistence),
                event_tx,
                cancellation_token.clone(),
            )
            .await
            .unwrap();
            actor.chat_state_handle = restored;
            *actor.current_prompt_id.lock().unwrap() = Some(owner.prompt_id.clone());
            actor.tool_context.subagent_event_tx =
                Some(scripted_outstanding_responder(vec![Default::default()]));

            assert!(
                actor
                    .chat_state_handle
                    .get_last_turn_usage()
                    .await
                    .is_none()
            );
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .expect("the original suspended Turn retains its ledger");
            assert_eq!(prompt.totals.total_tokens(), 110);
            assert_eq!(prompt.by_agent[&UsageAgent::Owner].total_tokens(), 77);
            assert_eq!(
                prompt.by_agent[&UsageAgent::Subagent("child-current".into())].total_tokens(),
                33
            );
            assert_eq!(prompt.by_agent.len(), 2, "stale child stays session-only");
            assert_eq!(
                prompt.main_loop_model_calls, 1,
                "aux and child are not main rounds"
            );
            assert!(!prompt.incomplete);

            // Exercise the real Shell drain -> finalize -> prompt-ledger
            // projection, without recording any post-restore sampling or bill.
            let usage = actor
                .freeze_prompt_usage_bounded(
                    &owner.prompt_id,
                    std::time::Duration::from_millis(120),
                )
                .await
                .expect("restored consumption attaches without a new model response");
            assert_eq!(usage.totals.input_tokens, 100);
            assert_eq!(usage.totals.output_tokens, 10);
            assert_eq!(usage.totals.total_tokens, 110);
            assert_eq!(usage.totals.model_calls, 3);
            assert_eq!(usage.num_turns, 1);
            assert!(!usage.usage_is_incomplete);
            assert_eq!(usage.model_usage.len(), 3);
            for (model, total) in [("main-model", 55), ("aux-model", 22), ("child-model", 33)] {
                assert_eq!(usage.model_usage[model].total_tokens, total);
                assert!(usage.model_usage[model].cost_usd_ticks.is_none());
                assert!(!usage.model_usage[model].cost_is_partial);
            }
            assert!(
                usage.totals.cost_usd_ticks.is_none(),
                "unknown cost stays unknown"
            );
            assert!(!usage.totals.cost_is_partial);
            assert!(
                actor
                    .chat_state_handle
                    .get_last_turn_usage()
                    .await
                    .is_none()
            );
            assert_eq!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap(),
                lifetime,
                "restoration and result freeze must not charge lifetime again"
            );
            assert_eq!(
                serde_json::to_value(actor.chat_state_handle.timeline_events().await.unwrap())
                    .unwrap(),
                suspended_facts,
                "no new Request, settlement, or other Timeline fact is needed for freeze"
            );

            let completed = SessionUpdate::TurnCompleted {
                prompt_id: owner.prompt_id.clone(),
                identity: None,
                stop_reason: "cancelled".into(),
                agent_result: None,
                usage: Some(usage.clone()),
            };
            let wire = serde_json::to_value(&completed).unwrap();
            assert_eq!(wire["sessionUpdate"], "turn_completed");
            assert_eq!(wire["prompt_id"], owner.prompt_id);
            assert_eq!(wire["usage"], serde_json::to_value(&usage).unwrap());
            assert_eq!(wire["usage"]["totalTokens"], 110);
            assert!(wire["usage"].get("costUsdTicks").is_none());
            let roundtrip: SessionUpdate = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(roundtrip).unwrap(), wire);
        })
        .await;
}

/// Scripted coordinator stub: answers each `Outstanding` query with the next
/// queued reply, repeating the last one; other events are ignored.
fn scripted_outstanding_responder(
    replies: Vec<tools::implementations::grow_build::task::types::SubagentOutstandingReply>,
) -> tokio::sync::mpsc::UnboundedSender<
    tools::implementations::grow_build::task::types::SubagentEvent,
> {
    use tools::implementations::grow_build::task::types::SubagentEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SubagentEvent>();
    tokio::task::spawn_local(async move {
        let mut queue = replies.into_iter();
        let mut last = None;
        while let Some(event) = rx.recv().await {
            if let SubagentEvent::Outstanding(req) = event {
                let reply = queue.next().or_else(|| last.clone()).unwrap_or_default();
                last = Some(reply.clone());
                let _ = req.respond_to.send(reply);
            }
        }
    });
    tx
}

/// Drain timeout (wedged foreground child) fails closed: the report and both
/// ledgers are marked incomplete.
#[tokio::test(flavor = "current_thread")]
async fn freeze_timeout_marks_report_and_both_ledgers() {
    use tools::implementations::grow_build::task::types::SubagentOutstandingReply;
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = make_actor().await;
            actor.tool_context.subagent_event_tx = Some(scripted_outstanding_responder(vec![
                SubagentOutstandingReply {
                    live_ids: vec!["wedged".into()],
                    background_live: false,
                    subagent_usage_not_applied: false,
                },
            ]));
            // Production freezes run for an already-admitted Turn; the
            // fail-closed prompt mark binds to that Turn's owner.
            let owner = start_test_turn(&actor, "frozen prompt").await;
            let usage = actor
                .freeze_prompt_usage_bounded(
                    &owner.prompt_id,
                    std::time::Duration::from_millis(120),
                )
                .await
                .expect("incomplete usage always attaches");
            assert!(usage.usage_is_incomplete);
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .expect("fail-closed mark opens the prompt ledger");
            assert!(prompt.incomplete);
            assert!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .incomplete
            );
        })
        .await;
}

/// Regression for the freeze deadline-coverage gap: a receiver that accepts
/// the Outstanding request and keeps `respond_to` alive without replying must
/// not extend the freeze past its budget. The freeze ends fail-closed (report
/// and both ledgers incomplete). The guard timeout makes the old unbounded
/// ACK wait fail instead of hanging the suite.
#[tokio::test(flavor = "current_thread")]
async fn freeze_held_outstanding_ack_fails_closed_at_deadline() {
    use tools::implementations::grow_build::task::types::SubagentEvent;

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = make_actor().await;
            // Holds every request (and its respond_to sender) without replying.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SubagentEvent>();
            tokio::task::spawn_local(async move {
                let mut held = Vec::new();
                while let Some(event) = rx.recv().await {
                    if let SubagentEvent::Outstanding(req) = event {
                        held.push(req);
                    }
                }
            });
            actor.tool_context.subagent_event_tx = Some(tx);
            // Production freezes run for an already-admitted Turn; the
            // fail-closed prompt mark binds to that Turn's owner.
            let owner = start_test_turn(&actor, "frozen prompt").await;

            let usage = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                actor.freeze_prompt_usage_bounded(
                    &owner.prompt_id,
                    std::time::Duration::from_millis(120),
                ),
            )
            .await
            .expect("held ACK must end at the freeze deadline")
            .expect("incomplete usage always attaches");
            assert!(usage.usage_is_incomplete);
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .expect("fail-closed mark opens the prompt ledger");
            assert!(prompt.incomplete);
            assert!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .incomplete
            );
        })
        .await;
}

/// A responder that eventually replies but only after the budget has expired
/// must not hold the freeze until the late reply arrives (the old unbounded
/// ACK wait exited at the late-reply moment, past the deadline). The guard is
/// deliberately generous — it distinguishes deadline-bounded termination from
/// a ~400ms late-reply exit without asserting millisecond wall-clock timing.
#[tokio::test(flavor = "current_thread")]
async fn freeze_late_reply_fails_closed_at_deadline() {
    use tools::implementations::grow_build::task::types::SubagentEvent;

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = make_actor().await;
            // Replies to every Outstanding query, but only 400ms later.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SubagentEvent>();
            tokio::task::spawn_local(async move {
                while let Some(event) = rx.recv().await {
                    if let SubagentEvent::Outstanding(req) = event {
                        tokio::task::spawn_local(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                            let _ = req.respond_to.send(Default::default());
                        });
                    }
                }
            });
            actor.tool_context.subagent_event_tx = Some(tx);
            // Production freezes run for an already-admitted Turn; the
            // fail-closed prompt mark binds to that Turn's owner.
            let owner = start_test_turn(&actor, "frozen prompt").await;

            let usage = tokio::time::timeout(
                std::time::Duration::from_millis(300),
                actor.freeze_prompt_usage_bounded(
                    &owner.prompt_id,
                    std::time::Duration::from_millis(120),
                ),
            )
            .await
            .expect("late reply must not hold the freeze past its deadline")
            .expect("incomplete usage always attaches");
            assert!(usage.usage_is_incomplete);
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .expect("fail-closed mark opens the prompt ledger");
            assert!(prompt.incomplete);
            assert!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .incomplete
            );
        })
        .await;
}

/// A live background child flags only the report: no ledger is marked,
/// because its fold still lands on the session ledger at completion.
#[tokio::test(flavor = "current_thread")]
async fn freeze_background_only_flags_report_not_ledgers() {
    use tools::implementations::grow_build::task::types::SubagentOutstandingReply;
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = make_actor().await;
            actor.tool_context.subagent_event_tx = Some(scripted_outstanding_responder(vec![
                SubagentOutstandingReply {
                    live_ids: vec![],
                    background_live: true,
                    subagent_usage_not_applied: false,
                },
            ]));
            let usage = actor
                .freeze_prompt_usage_bounded("p-1", std::time::Duration::from_millis(120))
                .await
                .expect("incomplete usage always attaches");
            assert!(usage.usage_is_incomplete, "report is incomplete");
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap();
            assert!(
                prompt.is_none_or(|l| !l.incomplete),
                "background child must not mark the prompt ledger"
            );
            assert!(
                !actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .incomplete,
                "session ledger stays unflagged: the fold still lands there"
            );
        })
        .await;
}

/// Cancel/freeze share `finalize_usage_from_outcome`: bg-only is report incomplete only.
#[tokio::test(flavor = "current_thread")]
async fn finalize_background_only_flags_report_not_ledgers() {
    use super::turn::UsageDrainOutcome;
    use tools::implementations::grow_build::task::types::SubagentOutstandingReply;

    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = make_actor().await;
            actor.chat_state_handle.record_model_call_usage(
                Some("m".into()),
                sampling_types::TokenUsage {
                    prompt_tokens: 4,
                    completion_tokens: 1,
                    total_tokens: 5,
                    reasoning_tokens: 0,
                    cached_prompt_tokens: 0,
                    cache_read_known: true,
                    cache_creation_prompt_tokens: 0,
                    cache_write_known: true,
                },
                None,
                None,
            );
            let outcome =
                UsageDrainOutcome::from_outstanding_reply(Some(&SubagentOutstandingReply {
                    live_ids: vec![],
                    background_live: true,
                    subagent_usage_not_applied: false,
                }));
            assert!(!outcome.fail_closed);
            let usage = actor
                .finalize_usage_from_outcome("p-1", outcome)
                .await
                .expect("billed prompt attaches");
            assert!(usage.usage_is_incomplete);
            assert_eq!(usage.totals.input_tokens, 4);
            assert!(
                !actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .incomplete,
                "bg-only must not stain session ledger"
            );
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .expect("prompt ledger open");
            assert!(!prompt.incomplete, "bg-only must not stain prompt ledger");
        })
        .await;
}

/// Report-channel stub: acks `MarkUsageNotApplied` and echoes that sticky on
/// the next `Outstanding` query, so freeze reads the same channel the
/// coordinator would.
fn capturing_usage_report() -> (
    tokio::sync::mpsc::UnboundedSender<
        tools::implementations::grow_build::task::types::SubagentEvent,
    >,
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) {
    use tools::implementations::grow_build::task::types::{
        SubagentEvent, SubagentOutstandingReply,
    };
    let marks = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = std::sync::Arc::clone(&marks);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_local(async move {
        while let Some(event) = rx.recv().await {
            match event {
                SubagentEvent::MarkUsageNotApplied(req) => {
                    recorded.lock().unwrap().push(req.prompt_id);
                    let _ = req.respond_to.send(());
                }
                SubagentEvent::Outstanding(req) => {
                    let marked = recorded
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|id| id == &req.prompt_id);
                    let _ = req.respond_to.send(SubagentOutstandingReply {
                        live_ids: vec![],
                        background_live: false,
                        subagent_usage_not_applied: marked,
                    });
                }
                _ => {}
            }
        }
    });
    (tx, marks)
}

/// Owner-aware apply-miss: stamped owner ≠ live owner stains session only.
#[tokio::test(flavor = "current_thread")]
async fn apply_miss_mismatched_pin_does_not_stain_live_prompt() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = make_actor().await;
            let (tx, marks) = capturing_usage_report();
            actor.tool_context.subagent_event_tx = Some(tx);
            let owner = start_test_turn(&actor, "live open prompt").await;
            *actor.current_prompt_id.lock().unwrap() = Some(owner.prompt_id.clone());
            actor.chat_state_handle.record_model_call_usage(
                Some("m".into()),
                sampling_types::TokenUsage {
                    prompt_tokens: 11,
                    completion_tokens: 1,
                    total_tokens: 12,
                    reasoning_tokens: 0,
                    cached_prompt_tokens: 0,
                    cache_read_known: true,
                    cache_creation_prompt_tokens: 0,
                    cache_write_known: true,
                },
                None,
                None,
            );
            let stale = next_owner_stale(&owner);
            assert!(
                actor
                    .mark_apply_miss_incomplete(Some(&stale), Some("p-stamped"))
                    .await
            );
            assert!(
                actor.mark_apply_miss_incomplete(None, None).await,
                "absent owner still lower-bounds the session lifetime"
            );
            assert!(
                actor
                    .mark_apply_miss_incomplete(None, Some(&owner.prompt_id))
                    .await
            );
            assert!(
                marks.lock().unwrap().is_empty(),
                "stale and absent owners must not mark the live report pin"
            );
            let usage = actor
                .freeze_prompt_usage_bounded(
                    &owner.prompt_id,
                    std::time::Duration::from_millis(120),
                )
                .await
                .expect("billed prompt attaches");
            assert!(!usage.usage_is_incomplete);
            assert_eq!(usage.totals.input_tokens, 11);
            let live = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .expect("live prompt ledger");
            assert!(
                !live.incomplete,
                "mismatched pin must not stain the live open prompt"
            );
            assert_eq!(live.totals.input_tokens, 11);
            assert!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .incomplete
            );
        })
        .await;
}

/// Owner-aware apply-miss: matching owner stains both ledgers and the report.
#[tokio::test(flavor = "current_thread")]
async fn apply_miss_matching_pin_stains_prompt_and_session() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = make_actor().await;
            let (tx, marks) = capturing_usage_report();
            actor.tool_context.subagent_event_tx = Some(tx);
            let owner = start_test_turn(&actor, "live matched prompt").await;
            *actor.current_prompt_id.lock().unwrap() = Some(owner.prompt_id.clone());
            actor.chat_state_handle.record_model_call_usage(
                Some("m".into()),
                sampling_types::TokenUsage {
                    prompt_tokens: 3,
                    completion_tokens: 1,
                    total_tokens: 4,
                    reasoning_tokens: 0,
                    cached_prompt_tokens: 0,
                    cache_read_known: true,
                    cache_creation_prompt_tokens: 0,
                    cache_write_known: true,
                },
                None,
                None,
            );
            assert!(
                actor
                    .mark_apply_miss_incomplete(Some(&owner), Some(&owner.prompt_id))
                    .await
            );
            assert!(
                actor
                    .mark_apply_miss_incomplete(Some(&owner), Some("other-pin"))
                    .await,
                "ledger still lower-bounds when the report pin is refused"
            );
            assert_eq!(
                marks.lock().unwrap().as_slice(),
                [owner.prompt_id.as_str()],
                "one sticky mark, and not the divergent pin"
            );
            let usage = actor
                .freeze_prompt_usage_bounded(
                    &owner.prompt_id,
                    std::time::Duration::from_millis(120),
                )
                .await
                .expect("billed prompt attaches");
            assert!(usage.usage_is_incomplete);
            assert_eq!(usage.totals.input_tokens, 3);
            assert!(
                actor
                    .chat_state_handle
                    .try_get_prompt_usage()
                    .await
                    .unwrap()
                    .expect("prompt")
                    .incomplete
            );
            assert!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .incomplete
            );
        })
        .await;
}

/// A reused client prompt id under a new Turn must not let the old Turn's
/// failed fold stain the new live prompt ledger or its report sticky.
#[tokio::test(flavor = "current_thread")]
async fn apply_miss_with_reused_prompt_id_does_not_stain_new_turn() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = make_actor().await;
            let (tx, marks) = capturing_usage_report();
            actor.tool_context.subagent_event_tx = Some(tx);
            let first = start_test_turn(&actor, "first logical turn").await;
            end_test_turn(&actor, &first).await;
            let second = start_test_turn(&actor, "second logical turn").await;
            *actor.current_prompt_id.lock().unwrap() = Some(second.prompt_id.clone());
            actor.chat_state_handle.record_model_call_usage(
                Some("m".into()),
                sampling_types::TokenUsage {
                    prompt_tokens: 5,
                    completion_tokens: 1,
                    total_tokens: 6,
                    reasoning_tokens: 0,
                    cached_prompt_tokens: 0,
                    cache_read_known: true,
                    cache_creation_prompt_tokens: 0,
                    cache_write_known: true,
                },
                None,
                None,
            );
            // Same prompt id text, different logical Turn: the stale stamp
            // carries the first Turn's identity with the second's prompt id.
            let stale = chat_state::UsagePromptOwner {
                turn_id: first.turn_id,
                prompt_id: second.prompt_id.clone(),
            };
            assert!(
                actor
                    .mark_apply_miss_incomplete(Some(&stale), Some(&stale.prompt_id))
                    .await
            );
            assert!(
                marks.lock().unwrap().is_empty(),
                "old Turn must not mark the reused prompt id"
            );
            let usage = actor
                .freeze_prompt_usage_bounded(
                    &second.prompt_id,
                    std::time::Duration::from_millis(120),
                )
                .await
                .expect("billed prompt attaches");
            assert!(
                !usage.usage_is_incomplete,
                "reused prompt id must stay a complete report"
            );
            assert_eq!(usage.totals.input_tokens, 5);
            let live = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .expect("live prompt ledger");
            assert!(
                !live.incomplete,
                "a reused prompt id must not stain the new Turn's ledger"
            );
            assert_eq!(live.totals.input_tokens, 5);
            assert!(
                actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .incomplete
            );
        })
        .await;
}

/// Sticky (session-only) is report-only on freeze: session ledger stays complete.
#[tokio::test(flavor = "current_thread")]
async fn freeze_sticky_only_flags_report_not_ledgers() {
    use tools::implementations::grow_build::task::types::SubagentOutstandingReply;
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = make_actor().await;
            actor.chat_state_handle.record_model_call_usage(
                Some("m".into()),
                sampling_types::TokenUsage {
                    prompt_tokens: 9,
                    completion_tokens: 1,
                    total_tokens: 10,
                    reasoning_tokens: 0,
                    cached_prompt_tokens: 0,
                    cache_read_known: true,
                    cache_creation_prompt_tokens: 0,
                    cache_write_known: true,
                },
                None,
                None,
            );
            actor.tool_context.subagent_event_tx = Some(scripted_outstanding_responder(vec![
                SubagentOutstandingReply {
                    live_ids: vec![],
                    background_live: false,
                    subagent_usage_not_applied: true,
                },
            ]));
            let usage = actor
                .freeze_prompt_usage_bounded("p-1", std::time::Duration::from_millis(120))
                .await
                .expect("incomplete usage always attaches");
            assert!(usage.usage_is_incomplete);
            assert!(
                !actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .incomplete,
                "sticky session-only must not stain session ledger"
            );
            let prompt = actor
                .chat_state_handle
                .try_get_prompt_usage()
                .await
                .unwrap()
                .expect("prompt ledger open");
            assert!(
                !prompt.incomplete,
                "sticky session-only must not stain prompt ledger"
            );
        })
        .await;
}

/// A fold landing mid-drain completes cleanly: no incomplete flag anywhere.
#[tokio::test(flavor = "current_thread")]
async fn freeze_completes_when_fold_lands_mid_drain() {
    use tools::implementations::grow_build::task::types::SubagentOutstandingReply;
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = make_actor().await;
            actor.chat_state_handle.record_model_call_usage(
                Some("m".into()),
                sampling_types::TokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 2,
                    total_tokens: 12,
                    reasoning_tokens: 0,
                    cached_prompt_tokens: 0,
                    cache_read_known: true,
                    cache_creation_prompt_tokens: 0,
                    cache_write_known: true,
                },
                None,
                None,
            );
            actor.tool_context.subagent_event_tx = Some(scripted_outstanding_responder(vec![
                SubagentOutstandingReply {
                    live_ids: vec!["finishing".into()],
                    background_live: false,
                    subagent_usage_not_applied: false,
                },
                SubagentOutstandingReply::default(),
            ]));
            let usage = actor
                .freeze_prompt_usage_bounded("p-1", std::time::Duration::from_secs(5))
                .await
                .expect("billed prompt attaches usage");
            assert!(!usage.usage_is_incomplete);
            assert_eq!(usage.totals.input_tokens, 10);
            assert!(
                !actor
                    .chat_state_handle
                    .try_get_session_usage()
                    .await
                    .unwrap()
                    .incomplete
            );
        })
        .await;
}

//! The turn-end `Stop`/`SubagentStop` gate for `SessionActor`.

use super::hook_dispatch::HookDispatchPolicy;
use super::*;
use ::hooks::dispatcher;
use ::hooks::event::{
    self, BackgroundTaskType, StopBackgroundTask, StopSessionCron, clip_stop_entry_text,
};

pub const MAX_STOP_HOOK_CONTINUATIONS_PER_TURN: u32 = 8;

/// `command` is a shell-only field, so a monitor's watch command is carried in
/// `description` instead.
fn stop_entry_from_task(task: &tools::types::TaskSnapshot) -> StopBackgroundTask {
    let command_text =
        clip_stop_entry_text(task.display_command.as_deref().unwrap_or(&task.command));
    let (kind, command, description) = match task.kind {
        tools::computer::types::TaskKind::Bash => {
            (BackgroundTaskType::Shell, Some(command_text), None)
        }
        tools::computer::types::TaskKind::Monitor => {
            (BackgroundTaskType::Monitor, None, Some(command_text))
        }
    };
    StopBackgroundTask {
        id: task.task_id.clone(),
        r#type: kind,
        status: "running".to_string(),
        description,
        command,
        agent_type: None,
    }
}

fn stop_entry_from_subagent(
    summary: &tools::implementations::grow_build::task::types::ActiveSubagentSummary,
) -> StopBackgroundTask {
    StopBackgroundTask {
        id: summary.subagent_id.clone(),
        r#type: BackgroundTaskType::Subagent,
        status: "running".to_string(),
        description: Some(clip_stop_entry_text(&summary.description)),
        command: None,
        agent_type: Some(summary.subagent_type.clone()),
    }
}

fn stop_cron_from_scheduled(
    task: &tools::implementations::grow_build::scheduler::types::ScheduledTask,
) -> StopSessionCron {
    StopSessionCron {
        id: task.id.clone(),
        schedule: tools::implementations::grow_build::scheduler::interval::interval_to_human(
            task.interval_secs,
        ),
        recurring: task.recurring,
        prompt: clip_stop_entry_text(&task.prompt),
    }
}

const STOP_FEEDBACK_TEXT_MAX: usize = 10_000;

fn format_stop_feedback(blocks: &[dispatcher::StopBlock], additional_context: &[String]) -> String {
    use std::fmt::Write as _;
    let clip = |text: &str| event::clip_text(text, STOP_FEEDBACK_TEXT_MAX);
    let mut feedback = String::new();
    if !blocks.is_empty() {
        feedback.push_str("Stop hook feedback:\n");
        for block in blocks {
            let _ = writeln!(feedback, "- {}", clip(&block.reason));
        }
    }
    for context in additional_context {
        if !feedback.is_empty() {
            feedback.push('\n');
        }
        feedback.push_str(&clip(context));
    }
    feedback
}

fn restored_stop_gate_decision(
    events: &[chat_state::TimelineEvent],
    response_seq: chat_state::EventSeq,
    turn: chat_state::TurnId,
    expected_event: chat_state::HookEventType,
    expected_subagent_id: Option<&str>,
) -> Result<Option<StopGateDecision>, String> {
    let triggers = events
        .iter()
        .filter_map(|entry| {
            if entry.seq <= response_seq {
                return None;
            }
            let chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                occurrence_id,
                event,
                gate,
                cause,
                ..
            }) = &entry.kind
            else {
                return None;
            };
            let matching_cause = match (expected_event, cause) {
                (
                    chat_state::HookEventType::Stop,
                    chat_state::HookCause::Turn { turn: cause_turn },
                ) => *cause_turn == turn,
                (
                    chat_state::HookEventType::SubagentStop,
                    chat_state::HookCause::Subagent { subagent_id },
                ) => expected_subagent_id.is_some_and(|id| id == subagent_id),
                _ => false,
            };
            (*event == expected_event && *gate == chat_state::HookGateKind::Stop && matching_cause)
                .then(|| (entry.seq, occurrence_id.clone()))
        })
        .collect::<Vec<_>>();
    if triggers.is_empty() {
        // The accepted response may have been checkpointed before the Stop
        // gate started. In that case the normal gate still needs to run once.
        return Ok(None);
    }
    let [(trigger_seq, occurrence_id)] = triggers.as_slice() else {
        return Err(format!(
            "expected one completed {expected_event:?} occurrence after the accepted response, found {}",
            triggers.len()
        ));
    };

    let mut started = std::collections::BTreeSet::new();
    let mut completed_decisions = Vec::new();
    for entry in events.iter().filter(|entry| entry.seq > *trigger_seq) {
        let chat_state::TimelineEventKind::Hook(hook) = &entry.kind else {
            continue;
        };
        match hook {
            chat_state::HookEvent::RunStarted {
                occurrence_id: run_occurrence,
                run_id,
                handler_index,
            } if run_occurrence == occurrence_id => {
                started.insert((run_id.clone(), *handler_index));
            }
            chat_state::HookEvent::RunFinished {
                occurrence_id: run_occurrence,
                run_id,
                handler_index,
                outcome,
                ..
            } if run_occurrence == occurrence_id => {
                if *outcome == chat_state::HookRunOutcome::InterruptedOutcomeUnknown {
                    return Err(format!(
                        "{expected_event:?} handler outcome is unknown after suspension"
                    ));
                }
                started.remove(&(run_id.clone(), *handler_index));
            }
            chat_state::HookEvent::RunSkipped {
                occurrence_id: run_occurrence,
                run_id,
                handler_index,
                ..
            } if run_occurrence == occurrence_id => {
                started.remove(&(run_id.clone(), *handler_index));
            }
            chat_state::HookEvent::Completed {
                occurrence_id: completed_occurrence,
                decision,
            } if completed_occurrence == occurrence_id => {
                if !started.is_empty() {
                    return Err(format!(
                        "{expected_event:?} completed while a handler run remained unresolved"
                    ));
                }
                completed_decisions.push(decision.clone());
            }
            _ => {}
        }
    }
    let [decision] = completed_decisions.as_slice() else {
        return Err(format!(
            "{expected_event:?} occurrence has no unique durable completion"
        ));
    };
    match decision {
        chat_state::HookAggregateDecision::Stop {
            decision:
                chat_state::HookStopDecision::AllowStop | chat_state::HookStopDecision::ForceStop { .. },
        } => Ok(Some(StopGateDecision::AllowStop)),
        chat_state::HookAggregateDecision::Stop {
            decision:
                chat_state::HookStopDecision::KeepWorking {
                    reasons,
                    additional_context,
                },
        } => {
            let blocks = reasons
                .iter()
                .map(|reason| dispatcher::StopBlock {
                    hook_name: String::new(),
                    reason: reason.clone(),
                })
                .collect::<Vec<_>>();
            Ok(Some(StopGateDecision::KeepWorking {
                feedback: format_stop_feedback(&blocks, additional_context),
            }))
        }
        _ => Err(format!(
            "completed {expected_event:?} occurrence has a non-Stop decision"
        )),
    }
}

pub(super) fn validate_restored_stop_gate(
    events: &[chat_state::TimelineEvent],
    response_seq: chat_state::EventSeq,
    turn: chat_state::TurnId,
    expected_event: chat_state::HookEventType,
    expected_subagent_id: Option<&str>,
) -> Result<(), String> {
    restored_stop_gate_decision(
        events,
        response_seq,
        turn,
        expected_event,
        expected_subagent_id,
    )
    .map(|_| ())
}

impl SessionActor {
    pub(crate) async fn list_active_subagents(
        &self,
    ) -> Vec<tools::implementations::grow_build::task::types::ActiveSubagentSummary> {
        use tools::implementations::grow_build::task::types::{
            SubagentEvent, SubagentListActiveRequest,
        };
        let Some(ref event_tx) = self.tool_context.subagent_event_tx else {
            return Vec::new();
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        if event_tx
            .send(SubagentEvent::ListActive(SubagentListActiveRequest {
                parent_session_id: self.session_id_string(),
                respond_to: tx,
            }))
            .is_err()
        {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    /// Snapshot in-flight background work and scheduled wakeups for the Stop
    /// hook input (filtering out tasks owned by other sessions on the shared
    /// backend).
    async fn stop_gate_work_snapshot(&self) -> (Vec<StopBackgroundTask>, Vec<StopSessionCron>) {
        let bridge = self.tool_bridge_handle();
        let my_session = self.session_id_string();
        let mut tasks: Vec<StopBackgroundTask> = bridge
            .list_background_tasks()
            .await
            .iter()
            .filter(|t| t.is_outstanding())
            .filter(|t| {
                t.owner_session_id
                    .as_deref()
                    .is_none_or(|owner| owner == my_session)
            })
            .map(stop_entry_from_task)
            .collect();
        tasks.extend(
            self.list_active_subagents()
                .await
                .iter()
                .map(stop_entry_from_subagent),
        );

        let now = chrono::Utc::now();
        let crons = bridge
            .list_scheduled_tasks()
            .await
            .iter()
            .filter(|t| !t.is_expired(now))
            .map(stop_cron_from_scheduled)
            .collect();
        (tasks, crons)
    }

    async fn build_stop_payload(&self, stop_hook_active: bool) -> event::HookPayload {
        let last_assistant_message = self
            .chat_state_handle
            .get_last_assistant_text_in_turn()
            .await;
        if self.startup_hints.is_subagent {
            event::HookPayload::SubagentStop {
                phase: event::SubagentStopPhase::Gate,
                subagent_id: self.session_id_string(),
                subagent_type: self.subagent_type_label().unwrap_or_default(),
                stop_hook_active: Some(stop_hook_active),
                last_assistant_message,
            }
        } else {
            let (background_tasks, session_crons) = self.stop_gate_work_snapshot().await;
            event::HookPayload::Stop {
                reason: "end_turn".to_string(),
                stop_hook_active,
                last_assistant_message,
                background_tasks: Some(background_tasks),
                session_crons: Some(session_crons),
            }
        }
    }

    /// Run the turn-end `Stop`/`SubagentStop` hook gate and decide whether the
    /// agent may stop or must keep working. Hook failures fail open (the agent
    /// stops normally).
    pub(super) async fn run_stop_gate(
        &self,
        prompt_id: &str,
        continuations_this_turn: u32,
    ) -> StopGateDecision {
        let event = if self.startup_hints.is_subagent {
            event::HookEventName::SubagentStop
        } else {
            event::HookEventName::Stop
        };
        let payload = self.build_stop_payload(continuations_this_turn > 0).await;
        // Gate envelope via `make_hook_envelope`, not the observe-notify
        // `fire_hook`: client hooks get the awaited `grow/hooks/run` request
        // below, not a fire-and-forget event.
        let envelope = self.make_hook_envelope(event, Some(prompt_id.to_string()), payload);

        let Some(turn) = self.events.current_turn() else {
            tracing::error!(%event, "stop hook occurrence has no active causal turn");
            return StopGateDecision::AllowStop;
        };
        let cause = if event == event::HookEventName::SubagentStop {
            chat_state::HookCause::Subagent {
                subagent_id: self.session_id_string(),
            }
        } else {
            chat_state::HookCause::Turn { turn }
        };
        // The cap is itself a durable Stop policy outcome. Do not invoke or
        // notify external handlers, but do record the same complete occurrence
        // shape as every other Stop gate before exposing AllowStop.
        if continuations_this_turn >= MAX_STOP_HOOK_CONTINUATIONS_PER_TURN {
            tracing::warn!(
                continuations_this_turn,
                "stop hook continuation limit reached; ending the turn"
            );
            match self
                .dispatch_hook_occurrence(
                    event,
                    cause,
                    envelope,
                    event::GateKind::Stop,
                    HookDispatchPolicy::SkipAllPolicyDisabled,
                )
                .await
            {
                Ok(aggregate) => {
                    debug_assert!(!aggregate.into_stop_result().wants_continuation());
                }
                Err(error) => {
                    tracing::error!(%error, "capped stop hook lifecycle was not durable");
                }
            }
            return StopGateDecision::AllowStop;
        }
        let aggregate = match self
            .dispatch_hook_occurrence(
                event,
                cause,
                envelope,
                event::GateKind::Stop,
                HookDispatchPolicy::Execute,
            )
            .await
        {
            Ok(aggregate) => aggregate,
            Err(error) => {
                // The subsequent Turn::Ended durable write is the terminal
                // fail-closed barrier; never admit another Step after a hook
                // lifecycle write has failed.
                tracing::error!(%error, "stop hook lifecycle was not durable");
                return StopGateDecision::AllowStop;
            }
        };
        let mut result = aggregate.into_stop_result();

        if result.prevent_continuation.take().is_some() {
            return StopGateDecision::AllowStop;
        }

        if !result.wants_continuation() {
            return StopGateDecision::AllowStop;
        }

        StopGateDecision::KeepWorking {
            feedback: format_stop_feedback(&result.blocks, &result.additional_context),
        }
    }

    pub(super) async fn restore_completed_stop_gate(
        &self,
        identity: chat_state::ResponseAdmissionIdentity,
    ) -> Result<Option<StopGateDecision>, String> {
        let accepted = self
            .chat_state_handle
            .admitted_response(identity)
            .await
            .ok_or_else(|| "checkpoint response is absent from the canonical branch".to_owned())?;
        let turn = self
            .events
            .current_turn()
            .ok_or_else(|| "restored accepted response has no active Turn".to_owned())?;
        let expected_event = if self.startup_hints.is_subagent {
            chat_state::HookEventType::SubagentStop
        } else {
            chat_state::HookEventType::Stop
        };
        let events = self
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or_else(|| "could not read Timeline to verify restored Stop gate".to_owned())?;
        let subagent_id = self
            .startup_hints
            .is_subagent
            .then(|| self.session_id_string());
        restored_stop_gate_decision(
            &events,
            accepted.event_seq,
            turn,
            expected_event,
            subagent_id.as_deref(),
        )
    }
}

#[cfg(test)]
mod stop_gate_snapshot_tests {
    use super::*;

    fn task_snapshot(kind: tools::computer::types::TaskKind) -> tools::types::TaskSnapshot {
        tools::types::TaskSnapshot {
            goal_definition_revision: None,
            task_id: "task-1".into(),
            command: "sandbox-exec tail -f /var/log/syslog".into(),
            display_command: Some("tail -f /var/log/syslog".into()),
            cwd: "/tmp".into(),
            start_time: std::time::SystemTime::UNIX_EPOCH,
            end_time: None,
            output: String::new(),
            output_file: std::path::PathBuf::from("/tmp/out"),
            truncated: false,
            exit_code: None,
            signal: None,
            completed: false,
            kind,
            block_waited: false,
            explicitly_killed: false,
            owner_session_id: None,
            goal_id: None,
            description: None,
            is_backgrounded: false,
        }
    }

    #[test]
    fn task_snapshot_maps_to_stop_entry() {
        let shell = stop_entry_from_task(&task_snapshot(tools::computer::types::TaskKind::Bash));
        assert_eq!(shell.r#type, BackgroundTaskType::Shell);
        assert_eq!(shell.command.as_deref(), Some("tail -f /var/log/syslog"));
        assert!(shell.description.is_none());
        assert_eq!(shell.status, "running");
        assert!(shell.agent_type.is_none());

        let monitor =
            stop_entry_from_task(&task_snapshot(tools::computer::types::TaskKind::Monitor));
        assert_eq!(monitor.r#type, BackgroundTaskType::Monitor);
        assert!(monitor.command.is_none());
        assert_eq!(
            monitor.description.as_deref(),
            Some("tail -f /var/log/syslog")
        );
    }

    #[test]
    fn subagent_summary_maps_to_stop_entry() {
        let summary = tools::implementations::grow_build::task::types::ActiveSubagentSummary {
            subagent_id: "sub-1".into(),
            subagent_type: "explore".into(),
            description: "d".repeat(2000),
            elapsed_ms: 5,
        };
        let entry = stop_entry_from_subagent(&summary);
        assert_eq!(entry.r#type, BackgroundTaskType::Subagent);
        assert_eq!(entry.agent_type.as_deref(), Some("explore"));
        let description = entry.description.unwrap();
        assert!(description.ends_with("… [+1000 chars]"));
        assert!(entry.command.is_none());
    }

    #[test]
    fn format_stop_feedback_lists_blocks_then_appends_context() {
        let block = |reason: &str| dispatcher::StopBlock {
            hook_name: "h".into(),
            reason: reason.into(),
        };
        assert_eq!(
            format_stop_feedback(&[block("first"), block("second")], &[]),
            "Stop hook feedback:\n- first\n- second\n"
        );
        assert_eq!(
            format_stop_feedback(&[block("fix tests")], &["note".to_string()]),
            "Stop hook feedback:\n- fix tests\n\nnote"
        );
        assert_eq!(
            format_stop_feedback(&[], &["only context".to_string()]),
            "only context"
        );
    }

    #[test]
    fn scheduled_task_maps_to_stop_cron() {
        let task = tools::implementations::grow_build::scheduler::types::ScheduledTask::new(
            300,
            "check the build".into(),
            true,
            false,
        );
        let cron = stop_cron_from_scheduled(&task);
        assert_eq!(cron.schedule, "every 5 minutes");
        assert!(cron.recurring);
        assert_eq!(cron.prompt, "check the build");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn continuation_cap_records_policy_skipped_stop_occurrence_before_allowing_stop() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (gateway_tx, mut gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _persistence_rx) =
                    tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
                let actor = crate::session::actor::tests::support::create_test_actor(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                crate::session::actor::tests::support::begin_test_causal_turn(&actor).await;

                let mut client_hooks = crate::extensions::hooks::ClientHooks::new();
                client_hooks.insert(
                    ::hooks::event::HookEventName::Stop,
                    vec![crate::extensions::hooks::ClientHookGroup {
                        matcher: None,
                        callback_ids: vec!["must-not-run-at-cap".into()],
                        timeout: None,
                    }],
                );
                *actor.hooks.client_hooks.borrow_mut() = client_hooks;

                let reverse_requests = std::rc::Rc::new(std::cell::Cell::new(0u32));
                let observed_requests = std::rc::Rc::clone(&reverse_requests);
                tokio::task::spawn_local(async move {
                    while let Some(message) = gateway_rx.recv().await {
                        match message {
                            acp_transport::AcpClientMessage::ExtMethod(args) => {
                                observed_requests.set(observed_requests.get() + 1);
                                let response: std::sync::Arc<serde_json::value::RawValue> =
                                    serde_json::value::to_raw_value(&serde_json::json!({}))
                                        .unwrap()
                                        .into();
                                let _ = args.response_tx.send(Ok(acp::ExtResponse::new(response)));
                            }
                            acp_transport::AcpClientMessage::SessionNotification(args) => {
                                let _ = args.response_tx.send(Ok(()));
                            }
                            _ => {}
                        }
                    }
                });

                let before = actor
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .unwrap()
                    .len();
                let decision = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    actor.run_stop_gate("prompt-at-cap", MAX_STOP_HOOK_CONTINUATIONS_PER_TURN),
                )
                .await
                .expect("the capped Stop gate must not hang");
                assert!(matches!(decision, StopGateDecision::AllowStop));

                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                let cap_events = &events[before..];
                assert_eq!(
                    cap_events.len(),
                    3,
                    "one matching handler closes as Triggered, RunSkipped, Completed"
                );
                let (occurrence_id, run_id) = match &cap_events[0].kind {
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                        occurrence_id,
                        event: chat_state::HookEventType::Stop,
                        gate: chat_state::HookGateKind::Stop,
                        cause: chat_state::HookCause::Turn { .. },
                        handlers,
                        ..
                    }) => {
                        assert_eq!(handlers.len(), 1);
                        assert_eq!(
                            handlers[0].provenance,
                            chat_state::HookHandlerProvenance::Client
                        );
                        assert_eq!(
                            handlers[0].action,
                            chat_state::HookHandlerPlanAction::Skip {
                                reason: chat_state::HookRunSkipReason::PolicyDisabled,
                            }
                        );
                        (occurrence_id.clone(), handlers[0].run_id.clone())
                    }
                    other => panic!("expected capped Stop HookTriggered, got {other:?}"),
                };
                assert!(matches!(
                    &cap_events[1].kind,
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::RunSkipped {
                        occurrence_id: skipped_occurrence,
                        run_id: skipped_run,
                        handler_index: 0,
                        reason: chat_state::HookRunSkipReason::PolicyDisabled,
                    }) if skipped_occurrence == &occurrence_id && skipped_run == &run_id
                ));
                assert!(matches!(
                    &cap_events[2].kind,
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                        occurrence_id: completed_occurrence,
                        decision: chat_state::HookAggregateDecision::Stop {
                            decision: chat_state::HookStopDecision::AllowStop,
                        },
                    }) if completed_occurrence == &occurrence_id
                ));

                let projection = actor
                    .chat_state_handle
                    .hook_projection(occurrence_id)
                    .await
                    .expect("the capped Stop occurrence must be projectable");
                assert!(matches!(
                    projection.runs.as_slice(),
                    [chat_state::HookHandlerLifecycle::Skipped {
                        reason: chat_state::HookRunSkipReason::PolicyDisabled,
                    }]
                ));
                tokio::task::yield_now().await;
                assert_eq!(
                    reverse_requests.get(),
                    0,
                    "the capped policy must not invoke the skipped client Hook"
                );
            })
            .await;
    }
}

#[cfg(test)]
mod restored_stop_gate_tests {
    use super::*;

    fn hook_event(seq: u64, event: chat_state::HookEvent) -> chat_state::TimelineEvent {
        chat_state::TimelineEvent {
            version: 1,
            seq: chat_state::EventSeq::new(seq),
            at_ms: 0,
            kind: chat_state::TimelineEventKind::Hook(event),
        }
    }

    fn trigger(turn: chat_state::TurnId) -> chat_state::HookEvent {
        chat_state::HookEvent::Triggered {
            occurrence_id: "stop-occurrence".into(),
            event: chat_state::HookEventType::Stop,
            gate: chat_state::HookGateKind::Stop,
            cause: chat_state::HookCause::Turn { turn },
            config_generation: 1,
            handlers: Vec::new(),
        }
    }

    #[test]
    fn restored_stop_gate_reuses_completed_feedback() {
        let turn = chat_state::TurnId(9);
        let events = vec![
            hook_event(2, trigger(turn)),
            hook_event(
                3,
                chat_state::HookEvent::Completed {
                    occurrence_id: "stop-occurrence".into(),
                    decision: chat_state::HookAggregateDecision::Stop {
                        decision: chat_state::HookStopDecision::KeepWorking {
                            reasons: vec!["review this detail".into()],
                            additional_context: vec!["keep this context".into()],
                        },
                    },
                },
            ),
        ];
        let decision = restored_stop_gate_decision(
            &events,
            chat_state::EventSeq::new(1),
            turn,
            chat_state::HookEventType::Stop,
            None,
        )
        .unwrap()
        .expect("completed stop gate is present");
        let StopGateDecision::KeepWorking { feedback } = decision else {
            panic!("expected stored KeepWorking decision, got {decision:?}");
        };
        assert!(feedback.contains("review this detail"));
        assert!(feedback.contains("keep this context"));
    }

    #[test]
    fn restored_stop_gate_refuses_unknown_or_incomplete_hook_runs() {
        let turn = chat_state::TurnId(9);
        let events = vec![
            hook_event(2, trigger(turn)),
            hook_event(
                3,
                chat_state::HookEvent::RunStarted {
                    occurrence_id: "stop-occurrence".into(),
                    run_id: "handler-run".into(),
                    handler_index: 0,
                },
            ),
            hook_event(
                4,
                chat_state::HookEvent::RunFinished {
                    occurrence_id: "stop-occurrence".into(),
                    run_id: "handler-run".into(),
                    handler_index: 0,
                    elapsed_ms: 1,
                    outcome: chat_state::HookRunOutcome::InterruptedOutcomeUnknown,
                    control: chat_state::HookRunControl::None,
                },
            ),
            hook_event(
                5,
                chat_state::HookEvent::Completed {
                    occurrence_id: "stop-occurrence".into(),
                    decision: chat_state::HookAggregateDecision::Stop {
                        decision: chat_state::HookStopDecision::AllowStop,
                    },
                },
            ),
        ];
        assert!(
            restored_stop_gate_decision(
                &events,
                chat_state::EventSeq::new(1),
                turn,
                chat_state::HookEventType::Stop,
                None,
            )
            .is_err()
        );
        assert!(
            restored_stop_gate_decision(
                &events[..1],
                chat_state::EventSeq::new(1),
                turn,
                chat_state::HookEventType::Stop,
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn accepted_response_without_started_stop_gate_runs_normal_gate() {
        assert!(
            restored_stop_gate_decision(
                &[],
                chat_state::EventSeq::new(1),
                chat_state::TurnId(9),
                chat_state::HookEventType::Stop,
                None,
            )
            .unwrap()
            .is_none()
        );
    }
}

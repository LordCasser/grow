use super::super::SessionActor;
use super::support::build_actor;
use agent_client_protocol::schema::v1 as acp;
use tool_types::planning::{
    DeliveryReview, DependencyKind, PlanLink, PlanPriority, PlanStep, PlanStepStatus, PlanWait,
    ReviewDecision, WaitCondition, WaitDependency,
};
use tools::implementations::grow_build::todo::runtime::PlanningCommand;

#[derive(Clone)]
struct PlanningAckProbe {
    events: std::sync::Arc<std::sync::Mutex<Vec<chat_state::TimelineEvent>>>,
    sampling: sampling_types::SamplingConfig,
    target_id: String,
    target_progress: tool_types::ReplanProgress,
    armed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

struct PlanningAckPersistence {
    probe: PlanningAckProbe,
    cancellation: tokio_util::sync::CancellationToken,
}

impl chat_state::TimelinePersistence for PlanningAckPersistence {
    fn persist_timeline_event_and_ack(
        &mut self,
        event: &chat_state::TimelineEvent,
    ) -> tokio::sync::oneshot::Receiver<std::io::Result<()>> {
        let (respond_to, receiver) = tokio::sync::oneshot::channel();
        let mut events = self.probe.events.lock().expect("planning event store");
        if !events.iter().any(|existing| existing.seq == event.seq) {
            events.push(event.clone());
        }
        drop(events);

        let is_target = matches!(
            &event.kind,
            chat_state::TimelineEventKind::Planning(planning)
                if planning.plan.replans.iter().any(|change|
                    change.id == self.probe.target_id
                        && change.progress == self.probe.target_progress)
        );
        if is_target
            && self
                .probe
                .armed
                .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            // The event is in the durable prefix, but its ACK is lost and the
            // old writer is stopped before it can accept a projection.
            drop(respond_to);
            self.cancellation.cancel();
        } else {
            let _ = respond_to.send(Ok(()));
        }
        receiver
    }

    fn flush(&mut self) {}
}

async fn install_planning_ack_fault(
    actor: &mut SessionActor,
    target_id: &str,
    target_progress: tool_types::ReplanProgress,
) -> PlanningAckProbe {
    let events = actor
        .chat_state_handle
        .timeline_events()
        .await
        .expect("read current durable Timeline");
    let sampling = actor
        .chat_state_handle
        .get_sampling_config()
        .await
        .expect("read sampling config");
    let probe = PlanningAckProbe {
        events: std::sync::Arc::new(std::sync::Mutex::new(events.clone())),
        sampling: sampling.clone(),
        target_id: target_id.to_owned(),
        target_progress,
        armed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
    };
    let cancellation = tokio_util::sync::CancellationToken::new();
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    actor.chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(
        events,
        sampling,
        Box::new(PlanningAckPersistence {
            probe: probe.clone(),
            cancellation: cancellation.clone(),
        }),
        event_tx,
        cancellation,
    )
    .await
    .expect("attach a Timeline writer with controlled ACK");
    actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
    probe
}

async fn cold_restore_planning_actor(actor: &mut SessionActor, probe: &PlanningAckProbe) {
    let events = probe.events.lock().expect("planning event store").clone();
    // The fault deliberately stops the old writer, so its request inbox is
    // no longer an authority for reconstructing the cold actor's route.
    let sampling = probe.sampling.clone();
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    actor.chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(
        events,
        sampling,
        Box::new(PlanningAckPersistence {
            probe: probe.clone(),
            cancellation: tokio_util::sync::CancellationToken::new(),
        }),
        event_tx,
        tokio_util::sync::CancellationToken::new(),
    )
    .await
    .expect("cold Timeline reconstruction must validate");
    actor.events = crate::session::events::EventTracker::new(actor.chat_state_handle.clone());
}

#[tokio::test(flavor = "current_thread")]
async fn received_replan_ack_loss_cold_retry_reuses_receipt_and_consumed_input() {
    use crate::session::actor::tests::support::install_test_foreground;
    use tools::implementations::grow_build::todo::{TodoStatus, TodoUpdate, TodoWriteInput};

    tokio::task::LocalSet::new()
        .run_until(async {
            let (parent, _parent_gateway) = build_actor().await;
            let (mut child, _child_gateway) = build_actor().await;
            std::sync::Arc::get_mut(&mut child)
                .expect("test child is uniquely owned")
                .session_info
                .id = acp::SessionId::new("planning-ack-child");
            let child_id = child.session_id_string();

            let mut parent_plan = tool_types::LocalPlan::new(parent.session_id_string(), "work:test");
            parent_plan.steps.push(PlanStep {
                id: "step-ack".into(),
                content: "original work".into(),
                priority: PlanPriority::Normal,
                status: PlanStepStatus::InProgress,
                requirements: None,
                removed: false,
            });
            parent_plan.links.push(PlanLink {
                id: "link-ack".into(),
                parent_step_id: "step-ack".into(),
                child_session_id: child_id.clone(),
                terminal: false,
                required: true,
                pending_source_revision: None,
                review_required_revision: 0,
                result_id: None,
                dispatch_failure: None,
            });
            parent
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: None,
                        plan: Box::new(parent_plan),
                    },
                ))
                .await
                .expect("parent owns the source plan");
            install_test_foreground(&parent, "planning-ack-turn").await;
            install_test_foreground(&child, "planning-ack-turn").await;

            let (write_tx, write_rx) = tokio::sync::oneshot::channel();
            parent
                .handle_planning_command(PlanningCommand::Write {
                    prompt_id: "planning-ack-turn".into(),
                    input: TodoWriteInput {
                        merge: true,
                        todos: vec![TodoUpdate {
                            id: "step-ack".into(),
                            content: Some("revised work".into()),
                            status: Some(TodoStatus::InProgress),
                            priority: None,
                            requirements: None,
                            removed: None,
                        }],
                        actions: Vec::new(),
                    },
                    respond_to: write_tx,
                })
                .await;
            write_rx
                .await
                .expect("source update response")
                .expect("source update must commit");
            let source = parent
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap()
                .plan
                .replans[0]
                .clone();

            let probe = install_planning_ack_fault(
                std::sync::Arc::get_mut(&mut child).unwrap(),
                &source.id,
                tool_types::ReplanProgress::Received,
            )
            .await;
            assert!(child
                .receive_plan_guidance(parent.session_id_string(), source.clone())
                .await
                .is_err(), "a lost durable ACK must be reported to the delivery caller");
            let committed_prefix = probe.events.lock().unwrap().clone();
            let received = chat_state::Timeline::from_events(committed_prefix.clone())
                .expect("the persist-before-ACK prefix must be a valid cold timeline");
            assert_eq!(
                received
                    .planning_snapshot()
                    .expect("received planning state")
                    .plan
                    .replans
                    .iter()
                    .filter(|change| change.id == source.id)
                    .count(),
                1
            );
            cold_restore_planning_actor(
                std::sync::Arc::get_mut(&mut child).unwrap(),
                &probe,
            )
            .await;

            let pending_before = child
                .chat_state_handle
                .pending_notifications()
                .await
                .expect("restored notification inbox");
            assert_eq!(pending_before.len(), 1);
            let receipt_id = pending_before[0].id.clone();
            assert_eq!(
                child
                    .receive_plan_guidance(parent.session_id_string(), source.clone())
                    .await
                    .expect("exact delivery retry must be accepted"),
                receipt_id,
                "cold retry must reuse the durable receipt identity"
            );
            let after_retry = child.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(after_retry.len(), committed_prefix.len());
            assert_eq!(
                child
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .plan
                    .replans
                    .iter()
                    .filter(|change| change.id == source.id)
                    .count(),
                1,
                "retry must not create a second child responsibility"
            );

            crate::session::actor::tests::support::begin_test_active_causal_turn(&child).await;
            let turn = child.events.current_turn().expect("durable active test Turn");
            child
                .consume_notifications_durably(vec![receipt_id], turn, None)
                .await
                .expect("consumption must commit before model projection");
            assert_eq!(
                child
                    .chat_state_handle
                    .get_conversation()
                    .await
                    .iter()
                    .filter(|item| matches!(item, sampling_types::ConversationItem::AgentMessage(_)))
                    .count(),
                1,
                "one consumed parent message must enter the model surface once"
            );
            let consumed_events = child.chat_state_handle.timeline_events().await.unwrap();
            cold_restore_planning_actor(
                std::sync::Arc::get_mut(&mut child).unwrap(),
                &probe,
            )
            .await;
            assert!(child
                .chat_state_handle
                .pending_notifications()
                .await
                .unwrap()
                .is_empty());
            assert_eq!(
                child
                    .chat_state_handle
                    .get_conversation()
                    .await
                    .iter()
                    .filter(|item| matches!(item, sampling_types::ConversationItem::AgentMessage(_)))
                    .count(),
                1,
                "cold replay keeps the consumed message in history without re-injecting it"
            );
            assert!(
                child.chat_state_handle.timeline_events().await.unwrap().len() >= consumed_events.len(),
                "cold recovery may append interruption closure, but must not remove consumed history"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn handled_receipt_ack_loss_cold_retry_preserves_new_replan() {
    use crate::session::actor::tests::support::install_test_foreground;

    tokio::task::LocalSet::new()
        .run_until(async {
            let (mut parent, _gateway) = build_actor().await;
            std::sync::Arc::get_mut(&mut parent)
                .expect("parent is uniquely owned")
                .session_info
                .id = acp::SessionId::new("planning-ack-parent");
            let child_id = "planning-ack-child".to_owned();
            let mut plan = tool_types::LocalPlan::new(parent.session_id_string(), "work:test");
            plan.steps.push(PlanStep {
                id: "step-ack".into(),
                content: "original work".into(),
                priority: PlanPriority::Normal,
                status: PlanStepStatus::InProgress,
                requirements: None,
                removed: false,
            });
            plan.links.push(PlanLink {
                id: "link-ack".into(),
                parent_step_id: "step-ack".into(),
                child_session_id: child_id.clone(),
                terminal: false,
                required: true,
                pending_source_revision: None,
                review_required_revision: 0,
                result_id: None,
                dispatch_failure: None,
            });
            let make_step = |content: &str| PlanStep {
                id: "step-ack".into(),
                content: content.into(),
                priority: PlanPriority::Normal,
                status: PlanStepStatus::InProgress,
                requirements: None,
                removed: false,
            };
            plan.update_steps(
                tool_types::PlanUpdateMode::Merge,
                vec![make_step("first revised work")],
            )
            .expect("first source edit creates a replan");
            let first = plan.replans[0].clone();
            for progress in [
                tool_types::ReplanProgress::Dispatching,
                tool_types::ReplanProgress::Received,
                tool_types::ReplanProgress::Consumed,
            ] {
                plan.advance_replan(&first.id, progress).unwrap();
            }
            plan.update_steps(
                tool_types::PlanUpdateMode::Merge,
                vec![make_step("newer responsibility")],
            )
            .expect("second source edit creates a newer replan");
            let newer = plan
                .replans
                .iter()
                .find(|change| change.id != first.id)
                .expect("new source responsibility")
                .clone();
            parent
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: None,
                        plan: Box::new(plan),
                    },
                ))
                .await
                .expect("parent source plan");
            install_test_foreground(&parent, "planning-ack-turn").await;
            assert_eq!(newer.progress, tool_types::ReplanProgress::Pending);

            let outcome = tool_types::ReplanOutcome::Updated { plan_revision: 7 };
            let probe = install_planning_ack_fault(
                std::sync::Arc::get_mut(&mut parent).unwrap(),
                &first.id,
                tool_types::ReplanProgress::Handled {
                    outcome: outcome.clone(),
                },
            )
            .await;
            assert!(
                parent
                    .receive_replan_handled(&child_id, &first.id, outcome.clone())
                    .await
                    .is_err(),
                "the parent must report the lost Handled ACK"
            );
            let committed_prefix = probe.events.lock().unwrap().clone();
            let timeline = chat_state::Timeline::from_events(committed_prefix.clone())
                .expect("persisted Handled fact must cold-validate");
            let persisted = timeline.planning_snapshot().unwrap();
            assert!(persisted.plan.replans.iter().any(|change| {
                change.id == first.id
                    && change.progress
                        == (tool_types::ReplanProgress::Handled {
                            outcome: outcome.clone(),
                        })
            }));
            assert!(persisted.plan.replans.iter().any(|change| {
                change.id == newer.id && change.progress == tool_types::ReplanProgress::Pending
            }));

            cold_restore_planning_actor(std::sync::Arc::get_mut(&mut parent).unwrap(), &probe)
                .await;
            parent
                .receive_replan_handled(&child_id, &first.id, outcome.clone())
                .await
                .expect("exact receipt retry after cold reconstruction is idempotent");
            let after_retry = parent.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(after_retry.len(), committed_prefix.len());
            let plan = parent
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap()
                .plan;
            assert_eq!(
                plan.replans
                    .iter()
                    .filter(|change| change.id == first.id)
                    .count(),
                1,
                "retry must not duplicate the handled source receipt"
            );
            assert!(
                plan.replans.iter().any(|change| {
                    change.id == newer.id && change.progress == tool_types::ReplanProgress::Pending
                }),
                "accepting an older child receipt must preserve newer source work"
            );
        })
        .await;
}

async fn linked_actor() -> std::sync::Arc<SessionActor> {
    linked_actor_with_required(true).await
}

async fn linked_actor_with_required(required: bool) -> std::sync::Arc<SessionActor> {
    let (actor, _gateway_rx) = build_actor().await;
    let mut plan = tool_types::LocalPlan::new(actor.session_id_string(), "work:test");
    plan.steps.push(PlanStep {
        id: "step-1".into(),
        content: "Build the change".into(),
        priority: PlanPriority::Normal,
        status: PlanStepStatus::Pending,
        requirements: None,
        removed: false,
    });
    plan.links.push(PlanLink {
        id: "task-1".into(),
        parent_step_id: "step-1".into(),
        child_session_id: "task-1".into(),
        terminal: false,
        required,
        pending_source_revision: None,
        review_required_revision: 0,
        result_id: None,
        dispatch_failure: None,
    });
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
            chat_state::timeline::PlanningEvent {
                previous: None,
                plan: Box::new(plan),
            },
        ))
        .await
        .expect("initial local plan must be durable");
    actor
}

async fn add_wait(actor: &SessionActor, kind: DependencyKind, id: &str) {
    let current = actor
        .chat_state_handle
        .planning_state()
        .await
        .expect("read planning state")
        .expect("plan exists");
    let mut plan = current.plan;
    plan.waits.push(PlanWait {
        id: format!("wait-{id}"),
        step_ids: Vec::new(),
        dependencies: vec![WaitDependency {
            kind,
            id: id.to_string(),
        }],
        condition: WaitCondition::All,
    });
    plan.revision += 1;
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
            chat_state::timeline::PlanningEvent {
                previous: Some(current.seq),
                plan: Box::new(plan),
            },
        ))
        .await
        .expect("wait must be durable");
}

async fn write_wait(actor: &std::sync::Arc<SessionActor>, wait: PlanWait) -> Result<(), String> {
    use tools::implementations::grow_build::todo::{PlanningAction, TodoWriteInput};

    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle_planning_command(PlanningCommand::Write {
            prompt_id: "planning-wait-test".into(),
            input: TodoWriteInput {
                merge: true,
                todos: Vec::new(),
                actions: vec![PlanningAction::Wait { wait }],
            },
            respond_to,
        })
        .await;
    response
        .await
        .expect("planning actor must acknowledge")
        .map(|_| ())
}

#[derive(Default)]
struct PlanningTaskSnapshots(
    std::sync::Mutex<
        std::collections::HashMap<
            String,
            tools::implementations::grow_build::task::types::SubagentSnapshot,
        >,
    >,
);

#[async_trait::async_trait]
impl tools::implementations::grow_build::task::backend::SubagentBackend for PlanningTaskSnapshots {
    async fn spawn(
        &self,
        _request: tools::implementations::grow_build::task::types::SubagentRequest,
    ) -> Result<
        tools::implementations::grow_build::task::types::SubagentResult,
        tool_runtime::ToolError,
    > {
        Err(tool_runtime::ToolError::invalid_arguments(
            "planning wait tests do not spawn children",
        ))
    }

    async fn query(
        &self,
        id: &str,
        _block: bool,
        _timeout_ms: Option<u64>,
    ) -> Result<
        Option<tools::implementations::grow_build::task::types::SubagentSnapshot>,
        tools::implementations::grow_build::task::types::SubagentObservationError,
    > {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(id)
            .cloned())
    }

    async fn cancel(
        &self,
        _id: &str,
    ) -> Result<
        tools::implementations::grow_build::task::types::SubagentCancelOutcome,
        tools::implementations::grow_build::task::types::SubagentObservationError,
    > {
        Ok(tools::implementations::grow_build::task::types::SubagentCancelOutcome::NotFound)
    }

    async fn validate_type(
        &self,
        _subagent_type: &str,
        _parent_session_id: &str,
    ) -> tools::implementations::grow_build::task::types::SubagentValidateTypeOutcome {
        tools::implementations::grow_build::task::types::SubagentValidateTypeOutcome::Ok
    }
}

fn task_snapshot(
    id: &str,
    status: tools::implementations::grow_build::task::types::SubagentSnapshotStatus,
) -> tools::implementations::grow_build::task::types::SubagentSnapshot {
    tools::implementations::grow_build::task::types::SubagentSnapshot {
        subagent_id: id.into(),
        description: "planning wait fixture".into(),
        subagent_type: "test".into(),
        status,
        started_at_epoch_ms: 1,
        duration_ms: 1,
    }
}

async fn start_owned_command(
    actor: &std::sync::Arc<SessionActor>,
    command: &str,
    owner_session_id: &str,
) -> (
    std::sync::Arc<dyn tools::computer::types::TerminalBackend>,
    String,
) {
    use tools::computer::types::{TaskKind, TerminalRunRequest};
    use tools::types::resources::Terminal;

    let bridge = actor.agent.borrow().tool_bridge().clone();
    let toolset = bridge.toolset();
    let terminal = toolset
        .resources
        .lock()
        .await
        .get::<Terminal>()
        .expect("actor fixture must expose its terminal backend")
        .0
        .clone();
    let output_dir = actor.session_dir.join("planning-wait-terminal");
    std::fs::create_dir_all(&output_dir).expect("create test output directory");
    let tool_call_id = uuid::Uuid::now_v7().to_string();
    let handle = terminal
        .run_background(TerminalRunRequest {
            command: command.into(),
            working_directory: std::env::temp_dir(),
            env: std::collections::HashMap::new(),
            timeout: std::time::Duration::from_secs(30),
            output_byte_limit: 4_096,
            output_file: output_dir.join(format!("{tool_call_id}.log")),
            notification_handle: tools::notification::ToolNotificationHandle::noop(),
            tool_call_id: tool_call_id.clone(),
            display_command: Some(command.into()),
            auto_background_on_timeout: false,
            foreground_block_budget: None,
            kind: TaskKind::Bash,
            owner_session_id: Some(owner_session_id.into()),
            goal_id: None,
            goal_definition_revision: None,
            description: Some("planning wait test".into()),
        })
        .await
        .expect("test command must launch");
    (terminal, handle.task_id)
}

fn wait_for_dependencies(
    id: &str,
    condition: WaitCondition,
    dependencies: Vec<WaitDependency>,
) -> PlanWait {
    PlanWait {
        id: id.into(),
        step_ids: Vec::new(),
        dependencies,
        condition,
    }
}

async fn write_review(
    actor: &std::sync::Arc<SessionActor>,
    result_id: String,
    revision: u64,
) -> Result<(), String> {
    use tools::implementations::grow_build::todo::{PlanningAction, TodoWriteInput};

    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle_planning_command(PlanningCommand::Write {
            prompt_id: "planning-review-test".into(),
            input: TodoWriteInput {
                merge: true,
                todos: Vec::new(),
                actions: vec![PlanningAction::Review {
                    review: DeliveryReview {
                        link_id: "task-1".into(),
                        result_id: Some(result_id),
                        reviewed_source_revision: revision,
                        decision: ReviewDecision::Accepted,
                        reason: Some("verified required deliverable".into()),
                        verification_refs: Vec::new(),
                    },
                }],
            },
            respond_to,
        })
        .await;
    response
        .await
        .expect("planning actor must acknowledge")
        .map(|_| ())
}

async fn request_goal_completion(
    actor: &std::sync::Arc<SessionActor>,
    authority: tools::implementations::grow_build::update_goal::GoalMutationAuthority,
) -> Result<String, String> {
    use tools::implementations::grow_build::update_goal::{
        GoalCommand, GoalUpdateStatus, UpdateGoalInput,
    };

    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle_goal_command(GoalCommand::Update {
            input: UpdateGoalInput {
                status: GoalUpdateStatus::Complete,
                blocker: None,
                completion_report: Some("All requested work verified complete.".into()),
            },
            authority,
            respond_to,
        })
        .await;
    response.await.expect("Goal command must be acknowledged")
}

async fn record_undispatched(
    actor: &std::sync::Arc<SessionActor>,
    reason: &str,
) -> Result<(), String> {
    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle_planning_command(PlanningCommand::Undispatched {
            task_id: "task-1".into(),
            reason: reason.into(),
            respond_to,
        })
        .await;
    response
        .await
        .expect("planning actor must acknowledge")
        .map(|_| ())
}

#[tokio::test(flavor = "current_thread")]
async fn late_undispatched_ack_is_recorded_once_without_faking_a_result() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = std::sync::Arc::new(linked_actor().await);

            // There is no active prompt. A late, explicit coordinator rejection is
            // still durable evidence tied to the task link.
            record_undispatched(&actor, "root is frozen")
                .await
                .expect("late dispatch failure must be recorded");
            let first = actor
                .chat_state_handle
                .planning_state()
                .await
                .expect("read plan")
                .expect("plan exists");
            let link = &first.plan.links[0];
            assert_eq!(link.dispatch_failure.as_deref(), Some("root is frozen"));
            assert!(!link.terminal);
            assert!(link.result_id.is_none());

            record_undispatched(&actor, "root is frozen")
                .await
                .expect("same acknowledgement is idempotent");
            let second = actor
                .chat_state_handle
                .planning_state()
                .await
                .expect("read plan")
                .expect("plan exists");
            assert_eq!(second.seq, first.seq);

            let mut invalid_accept = second.plan.clone();
            invalid_accept.reviews.push(DeliveryReview {
                link_id: "task-1".into(),
                result_id: None,
                reviewed_source_revision: invalid_accept.revision,
                decision: ReviewDecision::Accepted,
                reason: Some("looks good".into()),
                verification_refs: Vec::new(),
            });
            assert!(invalid_accept.validate().is_err());

            let mut released = second.plan;
            released.reviews.push(DeliveryReview {
                link_id: "task-1".into(),
                result_id: None,
                reviewed_source_revision: released.revision,
                decision: ReviewDecision::Released,
                reason: Some("the required execution was rejected before dispatch".into()),
                verification_refs: Vec::new(),
            });
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: Some(first.seq),
                        plan: Box::new(released),
                    },
                ))
                .await
                .expect("explicit release must be durable");
            assert!(
                actor
                    .planning_completion_blockers()
                    .await
                    .expect("completion blockers")
                    .is_empty()
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn planning_result_requires_a_canonical_child_reference() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor().await;

            let missing = actor
                .accept_planning_task_result("task-1", Some("step-1"), None)
                .await;
            assert!(missing.unwrap_err().contains("canonical child reference"));

            let foreign = chat_state::TimelineRangeRef {
                timeline_id: "another-child".into(),
                first_seq: 1,
                last_seq: 1,
            };
            let wrong_child = actor
                .accept_planning_task_result("task-1", Some("step-1"), Some(&foreign))
                .await;
            assert!(wrong_child.unwrap_err().contains("different child session"));

            let snapshot = actor
                .chat_state_handle
                .planning_state()
                .await
                .expect("read planning state")
                .expect("planning state exists");
            assert!(!snapshot.plan.links[0].terminal);
            assert!(snapshot.plan.links[0].result_id.is_none());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn planning_result_requires_expected_owner_but_allows_unlinked_task() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (unlinked_actor, _gateway_rx) = build_actor().await;
            unlinked_actor
                .accept_planning_task_result("ordinary-task", None, None)
                .await
                .expect("an unlinked task has no planning receipt to publish");

            let missing_owner = unlinked_actor
                .accept_planning_task_result("task-1", Some("step-1"), None)
                .await;
            assert!(
                missing_owner
                    .unwrap_err()
                    .contains("expected local plan association is unavailable")
            );

            let actor = linked_actor().await;
            let missing_link = actor
                .accept_planning_task_result("different-task", Some("step-1"), None)
                .await;
            assert!(
                missing_link
                    .unwrap_err()
                    .contains("expected local plan task/step association is missing")
            );
            let wrong_step = actor
                .accept_planning_task_result("task-1", Some("different-step"), None)
                .await;
            assert!(
                wrong_step
                    .unwrap_err()
                    .contains("expected local plan task/step association is missing")
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn pending_fifo_yields_goal_wait_before_dependency_is_ready() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor().await;
            add_wait(&actor, DependencyKind::Question, "question-1").await;
            let before = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            let (respond_to, _response) = tokio::sync::oneshot::channel();
            let (persist_ack, _ack) = tokio::sync::oneshot::channel();
            actor
                .state
                .lock()
                .await
                .pending_inputs
                .push_back(super::super::InputItem {
                    input_ids: vec!["queued-user-input".into()],
                    prompt_id: "queued-user-input".into(),
                    turn_kind: crate::session::TurnKind::User,
                    prompt_blocks: vec![],
                    client_identifier: None,
                    screen_mode: None,
                    verbatim: false,
                    json_schema: None,
                    origin: crate::session::PromptOrigin::User,
                    host_command: None,
                    notification_ids: vec![],
                    respond_to,
                    persist_ack: Some(persist_ack),
                    queue_meta: None,
                });

            let outcome = actor
                .await_planned_dependencies(&crate::session::PromptOrigin::GoalContinuation {
                    goal_id: "goal-1".into(),
                    definition_revision: 1,
                })
                .await
                .expect("FIFO priority is a normal control outcome");
            assert!(matches!(
                outcome,
                crate::session::actor::planning::PlannedDependenciesOutcome::YieldGoalForFifo
            ));
            let after = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(after.plan.waits, before.plan.waits);
            assert_eq!(actor.state.lock().await.pending_inputs.len(), 1);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn question_and_permission_answers_wake_the_real_planning_wait() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for (kind, pending_kind, call_id) in [
                (
                    DependencyKind::Question,
                    crate::session::pending_interaction::PendingKind::Question,
                    "question-call",
                ),
                (
                    DependencyKind::Permission,
                    crate::session::pending_interaction::PendingKind::Permission,
                    "permission-call",
                ),
            ] {
                let actor = linked_actor().await;
                add_wait(&actor, kind, call_id).await;
                crate::session::actor::tests::support::begin_test_active_causal_turn(&actor).await;
                let budget_before = chat_state::session_runtime::TurnContinuation {
                    turn_id: chat_state::TurnId(1),
                    prompt_id: "waiting-turn".into(),
                    prompt_index: 0,
                    origin: serde_json::to_string(&crate::session::PromptOrigin::User).unwrap(),
                    next_step_index: 3,
                    tool_turns_used: 2,
                    tool_calls_used: 4,
                    stop_continuations_used: 1,
                    output_schema: None,
                    resume_point: chat_state::session_runtime::TurnResumePoint::Model,
                    task_output_usage: Some((1_024, true)),
                    accepted_response: None,
                    structured_output_retries: 2,
                    sampling_attempts_used: 3,
                    sampling_max_attempts: Some(6),
                    sampling_replay_safe: false,
                    sampling_deadline_ms: Some(123_456),
                    operations: Vec::new(),
                };
                *actor.turn_continuation.borrow_mut() = Some(budget_before.clone());
                let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                let guard = crate::session::pending_interaction::PendingInteractionGuard::new(
                    actor.pending_interactions.clone(),
                    acp_transport::AcpAgentGatewaySender::new(gateway_tx),
                    actor.session_info.id.clone(),
                    call_id.to_string(),
                    pending_kind,
                );
                let waiter = {
                    let actor = actor.clone();
                    tokio::task::spawn_local(async move {
                        actor
                            .await_planned_dependencies(&crate::session::PromptOrigin::User)
                            .await
                    })
                };
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                drop(guard);
                let outcome = tokio::time::timeout(std::time::Duration::from_millis(500), waiter)
                    .await
                    .expect("answer should wake without polling delay")
                    .expect("planning wait task should finish")
                    .expect("planning wait should resolve");
                assert!(matches!(
                    outcome,
                    crate::session::actor::planning::PlannedDependenciesOutcome::ContinueTurn
                ));
                assert!(
                    actor
                        .chat_state_handle
                        .planning_state()
                        .await
                        .unwrap()
                        .unwrap()
                        .plan
                        .waits
                        .is_empty()
                );
                assert_eq!(
                    actor.turn_continuation.borrow().as_ref(),
                    Some(&budget_before),
                    "dependency wake must resume the same cursor without replenishing its limits"
                );
                let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
                assert_eq!(
                    timeline
                        .iter()
                        .filter(|event| matches!(
                            event.kind,
                            chat_state::TimelineEventKind::Turn(
                                chat_state::TurnEvent::Started { .. }
                            )
                        ))
                        .count(),
                    1,
                    "wake must not admit a replacement turn"
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn task_and_command_waits_apply_any_all_terminal_and_identity_rules() {
    tokio::task::LocalSet::new()
        .run_until(async {
            use tools::implementations::grow_build::task::backend::SubagentBackendResource;
            use tools::implementations::grow_build::task::types::SubagentSnapshotStatus;

            let actor = linked_actor().await;
            crate::session::actor::tests::support::install_test_foreground(
                &actor,
                "planning-wait-test",
            )
            .await;
            let task_backend = std::sync::Arc::new(PlanningTaskSnapshots::default());
            task_backend.0.lock().unwrap().insert(
                "failed-task".into(),
                task_snapshot(
                    "failed-task",
                    SubagentSnapshotStatus::Failed {
                        error: "child command failed".into(),
                    },
                ),
            );
            actor
                .agent
                .borrow()
                .tool_bridge()
                .update_resource(SubagentBackendResource(task_backend.clone()))
                .await;

            let (terminal, running_command) =
                start_owned_command(&actor, "sleep 30", &actor.session_id_string()).await;
            let task_dependency = WaitDependency {
                kind: DependencyKind::Task,
                id: "failed-task".into(),
            };
            let command_dependency = WaitDependency {
                kind: DependencyKind::Command,
                id: running_command.clone(),
            };

            write_wait(
                &actor,
                wait_for_dependencies(
                    "wait-any",
                    WaitCondition::Any,
                    vec![task_dependency.clone(), command_dependency.clone()],
                ),
            )
            .await
            .expect("registered failed Task and running Command are valid wait sources");
            let outcome = actor
                .await_planned_dependencies(&crate::session::PromptOrigin::User)
                .await
                .expect("Any wait should resolve when its failed Task is terminal");
            assert!(matches!(
                outcome,
                crate::session::actor::planning::PlannedDependenciesOutcome::ContinueTurn
            ));
            assert!(
                actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .plan
                    .waits
                    .is_empty()
            );
            assert!(
                actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .plan
                    .links
                    .iter()
                    .all(|link| !link.terminal && link.result_id.is_none()),
                "a failed Task waking Any must not become a successful deliverable"
            );

            write_wait(
                &actor,
                wait_for_dependencies(
                    "wait-all",
                    WaitCondition::All,
                    vec![task_dependency.clone(), command_dependency],
                ),
            )
            .await
            .expect("the same sources may be joined with All semantics");
            let waiter = {
                let actor = actor.clone();
                tokio::task::spawn_local(async move {
                    actor
                        .await_planned_dependencies(&crate::session::PromptOrigin::User)
                        .await
                })
            };
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            assert!(!waiter.is_finished(), "All waits for the live Command");
            assert_eq!(
                actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .plan
                    .waits
                    .len(),
                1
            );

            assert!(matches!(
                terminal.kill_task(&running_command).await,
                tools::computer::types::KillOutcome::Killed
                    | tools::computer::types::KillOutcome::AlreadyExited
            ));
            assert!(
                terminal
                    .get_task(&running_command)
                    .await
                    .is_some_and(|snapshot| snapshot.completed)
            );
            let outcome = tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
                .await
                .expect("interrupted Command must wake the All waiter")
                .expect("waiter task must finish")
                .expect("failed Task plus interrupted Command is a decided outcome");
            assert!(matches!(
                outcome,
                crate::session::actor::planning::PlannedDependenciesOutcome::ContinueTurn
            ));
            assert!(
                actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .plan
                    .waits
                    .is_empty()
            );
            assert!(
                actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .plan
                    .links
                    .iter()
                    .all(|link| !link.terminal && link.result_id.is_none()),
                "an interrupted Command waking All must not become a successful deliverable"
            );

            let (terminal_again, completed_command) =
                start_owned_command(&actor, "exit 1", &actor.session_id_string()).await;
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    if terminal_again
                        .get_task(&completed_command)
                        .await
                        .is_some_and(|snapshot| snapshot.completed)
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("failed Command must reach a terminal snapshot before wait registration");
            assert!(
                terminal_again
                    .get_task(&completed_command)
                    .await
                    .is_some_and(|snapshot| snapshot.completed && snapshot.exit_code != Some(0))
            );
            write_wait(
                &actor,
                wait_for_dependencies(
                    "wait-already-completed",
                    WaitCondition::All,
                    vec![
                        task_dependency,
                        WaitDependency {
                            kind: DependencyKind::Command,
                            id: completed_command,
                        },
                    ],
                ),
            )
            .await
            .expect("already-terminal dependencies may be registered");
            assert!(matches!(
                actor
                    .await_planned_dependencies(&crate::session::PromptOrigin::User)
                    .await
                    .expect("precompleted dependencies should resolve immediately"),
                crate::session::actor::planning::PlannedDependenciesOutcome::ContinueTurn
            ));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_and_unavailable_task_outputs_wake_without_success_claim() {
    tokio::task::LocalSet::new()
        .run_until(async {
            use tools::implementations::grow_build::task::backend::SubagentBackendResource;
            use tools::implementations::grow_build::task::types::SubagentSnapshotStatus;

            for (task_id, status) in [
                (
                    "cancelled-task",
                    SubagentSnapshotStatus::Cancelled {
                        reason: Some("cancelled by owner".into()),
                    },
                ),
                (
                    "output-unavailable-task",
                    SubagentSnapshotStatus::CompletedOutputUnavailable {
                        error: "canonical output artifact is unavailable".into(),
                        tool_calls: 1,
                        turns: 1,
                        tokens_used: 0,
                        worktree_path: None,
                    },
                ),
                (
                    "unknown-outcome-task",
                    SubagentSnapshotStatus::Failed {
                        error: "execution outcome is unknown; inspect before retry".into(),
                    },
                ),
            ] {
                let actor = linked_actor().await;
                crate::session::actor::tests::support::install_test_foreground(
                    &actor,
                    "planning-wait-test",
                )
                .await;
                let backend = std::sync::Arc::new(PlanningTaskSnapshots::default());
                backend
                    .0
                    .lock()
                    .unwrap()
                    .insert(task_id.into(), task_snapshot(task_id, status));
                actor
                    .agent
                    .borrow()
                    .tool_bridge()
                    .update_resource(SubagentBackendResource(backend))
                    .await;
                write_wait(
                    &actor,
                    wait_for_dependencies(
                        "terminal-task-wait",
                        WaitCondition::All,
                        vec![WaitDependency {
                            kind: DependencyKind::Task,
                            id: task_id.into(),
                        }],
                    ),
                )
                .await
                .expect("terminal Task dependencies may be observed");

                assert!(matches!(
                    actor
                        .await_planned_dependencies(&crate::session::PromptOrigin::User)
                        .await
                        .expect("terminal dependency should wake the admitted turn"),
                    crate::session::actor::planning::PlannedDependenciesOutcome::ContinueTurn
                ));
                let state = actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap();
                assert!(state.plan.waits.is_empty());
                assert!(state
                    .plan
                    .links
                    .iter()
                    .all(|link| !link.terminal && link.result_id.is_none()));
                let conversation = actor.chat_state_handle.get_conversation().await;
                assert!(conversation.iter().any(|item| matches!(
                    item,
                    sampling_types::conversation::ConversationItem::User(user)
                        if user.synthetic_reason == Some(sampling_types::conversation::SyntheticReason::SystemReminder)
                            && user.content.iter().any(|part| matches!(
                                part,
                                sampling_types::conversation::ContentPart::Text { text }
                                    if text.contains("not success")
                            ))
                )), "terminal {:?} must wake with explicit non-success guidance", task_id);
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn planning_wait_rejects_missing_and_foreign_dependency_identities() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor().await;
            crate::session::actor::tests::support::install_test_foreground(
                &actor,
                "planning-wait-test",
            )
            .await;
            actor
                .agent
                .borrow()
                .tool_bridge()
                .update_resource(
                    tools::implementations::grow_build::task::backend::SubagentBackendResource(
                        std::sync::Arc::new(PlanningTaskSnapshots::default()),
                    ),
                )
                .await;

            let missing_task = write_wait(
                &actor,
                wait_for_dependencies(
                    "missing-task",
                    WaitCondition::All,
                    vec![WaitDependency {
                        kind: DependencyKind::Task,
                        id: "not-a-task".into(),
                    }],
                ),
            )
            .await
            .unwrap_err();
            assert!(missing_task.contains("task dependency not-a-task does not exist"));

            let missing_command = write_wait(
                &actor,
                wait_for_dependencies(
                    "missing-command",
                    WaitCondition::All,
                    vec![WaitDependency {
                        kind: DependencyKind::Command,
                        id: "not-a-command".into(),
                    }],
                ),
            )
            .await
            .unwrap_err();
            assert!(missing_command.contains("command dependency not-a-command does not exist"));

            let (terminal, foreign_command) =
                start_owned_command(&actor, "sleep 30", "another-session").await;
            let foreign_owner = write_wait(
                &actor,
                wait_for_dependencies(
                    "foreign-command-owner",
                    WaitCondition::All,
                    vec![WaitDependency {
                        kind: DependencyKind::Command,
                        id: foreign_command.clone(),
                    }],
                ),
            )
            .await
            .unwrap_err();
            assert!(foreign_owner.contains("command dependency belongs to another session"));
            let state = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert!(
                state.plan.waits.is_empty(),
                "invalid waits must not persist"
            );
            let _ = terminal.kill_task(&foreign_command).await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn required_link_result_stays_blocked_until_current_requirements_are_reviewed() {
    tokio::task::LocalSet::new()
        .run_until(async {
            use tools::implementations::grow_build::todo::{TodoUpdate, TodoWriteInput};

            let actor = linked_actor().await;
            crate::session::actor::tests::support::install_test_foreground(
                &actor,
                "planning-review-test",
            )
            .await;
            let initial = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            let result_id = serde_json::to_string(&chat_state::TimelineRangeRef {
                timeline_id: "task-1".into(),
                first_seq: 1,
                last_seq: 1,
            })
            .unwrap();
            // This case starts after the link owner has durably recorded its
            // terminal result reference. Canonical child-reference provenance
            // has its own actor tests above; this test exercises the review
            // gate and its source-revision invalidation through real commands.
            let mut result_arrived = initial.plan;
            result_arrived.revision += 1;
            result_arrived.links[0].result_id = Some(result_id.clone());
            result_arrived
                .mark_link_terminal("task-1", "child result arrived")
                .expect("fixture link should become terminal");
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: Some(initial.seq),
                        plan: Box::new(result_arrived),
                    },
                ))
                .await
                .expect("terminal result fact must be durable before review");
            assert!(
                actor
                    .planning_completion_blockers()
                    .await
                    .unwrap()
                    .iter()
                    .any(|blocker| blocker.contains("required deliverable task-1"))
            );

            let after_result = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            write_review(&actor, result_id.clone(), after_result.plan.revision)
                .await
                .expect("review of the exact terminal result must be accepted");
            assert!(
                actor
                    .planning_completion_blockers()
                    .await
                    .unwrap()
                    .is_empty()
            );

            let reviewed = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            let (respond_to, response) = tokio::sync::oneshot::channel();
            actor
                .handle_planning_command(PlanningCommand::Write {
                    prompt_id: "planning-review-test".into(),
                    input: TodoWriteInput {
                        merge: true,
                        todos: vec![TodoUpdate {
                            id: "step-1".into(),
                            content: None,
                            status: None,
                            priority: None,
                            requirements: Some(vec!["also verify rollback".into()]),
                            removed: None,
                        }],
                        actions: Vec::new(),
                    },
                    respond_to,
                })
                .await;
            response
                .await
                .expect("planning actor must acknowledge the requirement change")
                .expect("effective requirement change must persist");
            let changed = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert!(changed.plan.revision > reviewed.plan.revision);
            assert!(changed.plan.links[0].review_required_revision > reviewed.plan.revision);
            assert!(
                actor
                    .planning_completion_blockers()
                    .await
                    .unwrap()
                    .iter()
                    .any(|blocker| blocker.contains("required deliverable task-1"))
            );

            write_review(&actor, result_id, changed.plan.revision)
                .await
                .expect("the unchanged terminal result may be reviewed against new requirements");
            assert!(
                actor
                    .planning_completion_blockers()
                    .await
                    .unwrap()
                    .is_empty()
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn goal_complete_command_enforces_required_delivery_but_ignores_unrelated_todo() {
    tokio::task::LocalSet::new()
        .run_until(async {
            use tools::implementations::grow_build::update_goal::GoalMutationAuthority;

            let actor = linked_actor().await;
            crate::session::actor::tests::support::install_test_foreground(
                &actor,
                "planning-review-test",
            )
            .await;
            *actor.current_prompt_id.lock().unwrap() = Some("planning-review-test".into());
            actor
                .goal_tracker
                .lock()
                .create_goal(
                    "goal-planning-complete".into(),
                    "finish the required deliverable".into(),
                    None,
                    "2026-10-19T00:00:00Z".into(),
                )
                .unwrap();
            let goal = actor
                .goal_tracker
                .lock()
                .snapshot()
                .cloned()
                .expect("created Goal must have a durable identity");
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Goal);

            let initial = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            let mut plan = initial.plan;
            plan.steps.push(PlanStep {
                id: "optional-cleanup".into(),
                content: "Optional unrelated cleanup".into(),
                priority: PlanPriority::Low,
                status: PlanStepStatus::Pending,
                requirements: None,
                removed: false,
            });
            plan.revision += 1;
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: Some(initial.seq),
                        plan: Box::new(plan),
                    },
                ))
                .await
                .expect("optional TODO must be a durable planning fact");

            let authority = || GoalMutationAuthority {
                prompt_id: "planning-review-test".into(),
                prompt_index: 1,
                control_revision: actor
                    .control_revision
                    .load(std::sync::atomic::Ordering::SeqCst),
                goal: Some((goal.goal_id.clone(), goal.definition_revision)),
            };
            let rejected = request_goal_completion(&actor, authority()).await;
            let error = rejected.expect_err("unreviewed required delivery must block completion");
            assert!(error.contains("required deliverable task-1"), "{error}");
            assert_eq!(
                actor.goal_tracker.lock().status(),
                Some(crate::session::goal_tracker::GoalStatus::Active),
                "a rejected completion must leave the Goal active"
            );

            let current = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            let result_id = serde_json::to_string(&chat_state::TimelineRangeRef {
                timeline_id: "task-1".into(),
                first_seq: 1,
                last_seq: 1,
            })
            .unwrap();
            let mut result_arrived = current.plan;
            result_arrived.revision += 1;
            result_arrived.links[0].result_id = Some(result_id.clone());
            result_arrived
                .mark_link_terminal("task-1", "required child result arrived")
                .expect("the required link can accept its child result fact");
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: Some(current.seq),
                        plan: Box::new(result_arrived),
                    },
                ))
                .await
                .expect("required child result must be durable before review");
            let reviewed_revision = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap()
                .plan
                .revision;
            write_review(&actor, result_id, reviewed_revision)
                .await
                .expect("exact current requirements review must be accepted");

            assert_eq!(
                request_goal_completion(&actor, authority()).await.unwrap(),
                "Goal marked complete."
            );
            assert_eq!(
                actor.goal_tracker.lock().status(),
                Some(crate::session::goal_tracker::GoalStatus::Complete)
            );
            assert_eq!(
                actor.behavior.lock().behavior(),
                tool_types::BehaviorId::Normal
            );
            assert!(
                actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .plan
                    .steps
                    .iter()
                    .any(|step| step.id == "optional-cleanup" && !step.removed),
                "an unrelated pending TODO remains visible but is not a Goal completion blocker"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn task_link_and_parent_commit_exchange_order_without_losing_latest_requirements() {
    use super::support::install_test_foreground;
    use tools::implementations::grow_build::task::{TaskTool, backend::ChannelBackend, types::*};
    use tools::implementations::grow_build::todo::runtime::PlanningRuntimeHandle;
    use tools::implementations::grow_build::todo::{TodoUpdate, TodoWriteInput};
    use tools::types::{resources::Resources, tool_metadata::test_ctx};

    tokio::task::LocalSet::new()
        .run_until(async {
            for edit_first in [true, false] {
                let (parent, _gateway) = build_actor().await;
                install_test_foreground(&parent, "link-race-turn").await;
                let mut plan = tool_types::LocalPlan::new(parent.session_id_string(), "work:test");
                plan.steps.push(PlanStep {
                    id: "linked-step".into(),
                    content: "original requirement".into(),
                    priority: PlanPriority::Normal,
                    status: PlanStepStatus::InProgress,
                    requirements: None,
                    removed: false,
                });
                parent
                    .chat_state_handle
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                        chat_state::timeline::PlanningEvent {
                            previous: None,
                            plan: Box::new(plan),
                        },
                    ))
                    .await
                    .unwrap();

                let (planning_tx, mut planning_rx) = tokio::sync::mpsc::unbounded_channel();
                let (backend_tx, mut backend_rx) = tokio::sync::mpsc::unbounded_channel();
                let mut resources = Resources::new();
                resources.insert(PlanningRuntimeHandle(planning_tx));
                resources.insert(
                    ChannelBackend::for_session(backend_tx, parent.session_id_string())
                        .into_resource(),
                );
                resources.insert(SessionIdResource(parent.session_id_string()));
                resources.insert(CurrentPromptIdResource("link-race-turn".into()));
                let tool = tokio::task::spawn_local(async move {
                    tool_runtime::Tool::run(
                        &TaskTool,
                        test_ctx(resources.into_shared()),
                        serde_json::from_value(serde_json::json!({
                            "parent_step_id": "linked-step", "task_id": "linked-task",
                            "description": "race test", "prompt": "original task prompt",
                            "subagent_type": "explore", "run_in_background": true,
                        }))
                        .unwrap(),
                    )
                    .await
                });
                let SubagentEvent::ValidateType(validation) = backend_rx.recv().await.unwrap()
                else {
                    panic!("Task must validate before delegation");
                };
                validation
                    .respond_to
                    .send(SubagentValidateTypeOutcome::Ok)
                    .unwrap();
                let link_command = planning_rx.recv().await.unwrap();
                assert!(matches!(link_command, PlanningCommand::Link { .. }));
                let (write_tx, write_rx) = tokio::sync::oneshot::channel();
                let write_command = PlanningCommand::Write {
                    prompt_id: "link-race-turn".into(),
                    input: TodoWriteInput {
                        merge: true,
                        todos: vec![TodoUpdate {
                            id: "linked-step".into(),
                            content: Some("latest requirement".into()),
                            status: None,
                            priority: None,
                            requirements: None,
                            removed: None,
                        }],
                        actions: Vec::new(),
                    },
                    respond_to: write_tx,
                };
                // Queue both production actor operations at the actual fair
                // control gate, then exchange their durable commit order.
                let barrier = parent.step_control_gate.lock().await;
                let write = parent.handle_planning_command(write_command);
                let link = parent.handle_planning_command(link_command);
                tokio::pin!(write, link);
                if edit_first {
                    assert!(futures::poll!(write.as_mut()).is_pending());
                    assert!(futures::poll!(link.as_mut()).is_pending());
                } else {
                    assert!(futures::poll!(link.as_mut()).is_pending());
                    assert!(futures::poll!(write.as_mut()).is_pending());
                }
                drop(barrier);
                tokio::join!(write, link);
                write_rx.await.unwrap().unwrap();
                tool.await.unwrap().expect("Task delegation is admitted");
                let SubagentEvent::Spawn(spawn) = backend_rx.recv().await.unwrap() else {
                    panic!("Task must submit the linked child to its real backend boundary");
                };
                let (_, encoded) = spawn
                    .request
                    .prompt
                    .split_once("\n\nCurrent delegated parent step:\n")
                    .expect("child must receive the committed parent requirement");
                let guidance: serde_json::Value = serde_json::from_str(encoded).unwrap();
                assert_eq!(guidance["session_id"], parent.session_id_string());
                assert_eq!(guidance["scope_id"], "work:test");
                assert_eq!(guidance["step"]["id"], "linked-step");
                let committed = parent
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap();
                let binding = &committed.plan.links[0];
                assert_eq!(binding.child_session_id, "linked-task");
                assert_eq!(binding.parent_step_id, "linked-step");
                if edit_first {
                    assert_eq!(guidance["step"]["content"], "latest requirement");
                    assert_eq!(guidance["revision"], committed.plan.revision);
                    assert_eq!(binding.review_required_revision, committed.plan.revision);
                    assert!(committed.plan.replans.is_empty());
                } else {
                    assert_eq!(guidance["step"]["content"], "original requirement");
                    assert_eq!(guidance["revision"], 0);
                    assert_eq!(committed.plan.replans.len(), 1);
                    let responsibility = &committed.plan.replans[0];
                    assert_eq!(responsibility.link_id, "linked-task");
                    assert_eq!(responsibility.child_session_id, "linked-task");
                    assert_eq!(responsibility.content, "latest requirement");
                    assert_eq!(responsibility.source_revision, committed.plan.revision);
                    assert_eq!(
                        binding.pending_source_revision,
                        Some(responsibility.source_revision)
                    );
                }
                spawn
                    .result_tx
                    .send(SubagentResult {
                        success: true,
                        subagent_id: "linked-task".into(),
                        child_session_id: "linked-task".into(),
                        ..Default::default()
                    })
                    .unwrap();
                parent.background_service_shutdown.cancel();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn linked_parent_update_is_dispatched_received_handled_and_completion_is_not_replanned() {
    use crate::session::actor::tests::support::install_test_foreground;
    use tools::implementations::grow_build::task::backend::{
        ChannelBackend, SubagentBackendResource,
    };
    use tools::implementations::grow_build::task::interaction::{
        AgentInteraction, AgentInteractionOutput,
    };
    use tools::implementations::grow_build::task::types::SubagentEvent;
    use tools::implementations::grow_build::todo::runtime::PlanningAction;
    use tools::implementations::grow_build::todo::{TodoStatus, TodoUpdate, TodoWriteInput};

    tokio::task::LocalSet::new()
        .run_until(async {
            let (parent, _parent_gateway) = build_actor().await;
            let (mut child, _child_gateway) = build_actor().await;
            std::sync::Arc::get_mut(&mut child)
                .expect("test child has no other owners yet")
                .session_info
                .id = acp::SessionId::new("planning-direct-child");
            let child_id = child.session_id_string();

            // Seed the exact parent-step/task/child relation in one durable
            // owner fact, then install the same direct-owner backend boundary
            // used by production planning delivery.
            let mut plan = tool_types::LocalPlan::new(parent.session_id_string(), "work:test");
            plan.steps.push(PlanStep {
                id: "step-1".into(),
                content: "Build the change".into(),
                priority: PlanPriority::Normal,
                status: PlanStepStatus::InProgress,
                requirements: None,
                removed: false,
            });
            plan.links.push(PlanLink {
                id: "task-1".into(),
                parent_step_id: "step-1".into(),
                child_session_id: child_id.clone(),
                terminal: false,
                required: true,
                pending_source_revision: None,
                review_required_revision: 0,
                result_id: None,
                dispatch_failure: None,
            });
            parent
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: None,
                        plan: Box::new(plan),
                    },
                ))
                .await
                .expect("the exact direct-child association must be durable");

            let mut child_plan = tool_types::LocalPlan::new(child_id.clone(), "work:test");
            child_plan.steps.push(PlanStep {
                id: "child-step-1".into(),
                content: "Build the original change".into(),
                priority: PlanPriority::Normal,
                status: PlanStepStatus::InProgress,
                requirements: None,
                removed: false,
            });
            child
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: None,
                        plan: Box::new(child_plan),
                    },
                ))
                .await
                .expect("child must own its local plan before parent guidance arrives");

            let (parent_events_tx, mut parent_events_rx) = tokio::sync::mpsc::unbounded_channel();
            let (child_events_tx, mut child_events_rx) = tokio::sync::mpsc::unbounded_channel();
            let parent_bridge = parent.agent.borrow().tool_bridge().clone();
            let child_bridge = child.agent.borrow().tool_bridge().clone();
            parent_bridge
                .update_resource(SubagentBackendResource(std::sync::Arc::new(
                    ChannelBackend::for_session(parent_events_tx, parent.session_id_string()),
                )))
                .await;
            child_bridge
                .update_resource(SubagentBackendResource(std::sync::Arc::new(
                    ChannelBackend::for_session(child_events_tx, child_id.clone()),
                )))
                .await;

            install_test_foreground(&parent, "test-active-turn").await;
            crate::session::actor::tests::support::begin_test_active_causal_turn(&child).await;

            let (write_tx, write_rx) = tokio::sync::oneshot::channel();
            parent
                .handle_planning_command(PlanningCommand::Write {
                    prompt_id: "test-active-turn".into(),
                    input: TodoWriteInput {
                        merge: true,
                        todos: vec![TodoUpdate {
                            id: "step-1".into(),
                            content: Some("Build the revised change".into()),
                            status: Some(TodoStatus::InProgress),
                            priority: None,
                            requirements: None,
                            removed: None,
                        }],
                        actions: Vec::new(),
                    },
                    respond_to: write_tx,
                })
                .await;
            write_rx
                .await
                .expect("parent planning command must answer")
                .expect("effective parent update must commit");

            let event =
                tokio::time::timeout(std::time::Duration::from_secs(2), parent_events_rx.recv())
                    .await
                    .expect("durable parent update must dispatch to its direct child")
                    .expect("parent backend event must arrive");
            let SubagentEvent::Interact(request) = event else {
                panic!("planning delivery must use the owner interaction route");
            };
            let AgentInteraction::Replan { change } = request.action else {
                panic!("first direct-child interaction must carry the replan");
            };
            assert_eq!(change.child_session_id, child_id);
            assert_eq!(change.content, "Build the revised change");
            assert_eq!(change.progress, tool_types::ReplanProgress::Dispatching);

            let change_id = change.id.clone();
            let stable_request_id = request.id.clone();
            let mut retried_change = change;
            let mut final_request_sender = None;
            let mut final_request_id = String::new();
            request
                .respond_to
                .send(Ok(AgentInteractionOutput::message_unconfirmed(
                    request.id,
                    "test_unconfirmed",
                    "the initial delivery attempt was not acknowledged",
                )))
                .expect("initial delivery attempt must remain recoverable");
            for attempt in 0..4 {
                let retry = tokio::time::timeout(
                    std::time::Duration::from_secs(4),
                    parent_events_rx.recv(),
                )
                .await
                .expect("a durable Dispatching change must keep retrying")
                .expect("retry event must arrive");
                let SubagentEvent::Interact(next_request) = retry else {
                    panic!("planning retry must use the owner interaction route");
                };
                let next_request_id = next_request.id;
                let next_request_sender = next_request.respond_to;
                assert_eq!(
                    next_request_id, stable_request_id,
                    "retry must keep the idempotency key"
                );
                let AgentInteraction::Replan {
                    change: next_change,
                } = next_request.action
                else {
                    panic!("retry must carry the same replan");
                };
                assert_eq!(next_change.id, change_id);
                assert_eq!(
                    next_change.progress,
                    tool_types::ReplanProgress::Dispatching
                );
                retried_change = next_change;
                if attempt < 3 {
                    next_request_sender
                        .send(Ok(AgentInteractionOutput::message_unconfirmed(
                            next_request_id,
                            "test_unconfirmed",
                            "the delivery attempt was not acknowledged",
                        )))
                        .expect("intermediate retry attempt must remain recoverable");
                } else {
                    final_request_id = next_request_id;
                    final_request_sender = Some(next_request_sender);
                }
            }
            let receipt_id = child
                .receive_plan_guidance(parent.session_id_string(), retried_change)
                .await
                .expect("child owner must durably receive exact replan");
            final_request_sender
                .expect("fifth request must have a live responder")
                .send(Ok(AgentInteractionOutput::message_received(
                    final_request_id,
                    receipt_id,
                )))
                .expect("parent dispatch waiter must still be alive");

            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                loop {
                    let state = parent
                        .chat_state_handle
                        .planning_state()
                        .await
                        .unwrap()
                        .unwrap();
                    if state.plan.replans[0].progress == tool_types::ReplanProgress::Received {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("parent must persist the direct-child Received acknowledgement");
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while parent.planning_delivery.is_running() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("Received is a waiting state and must stop delivery retries");
            assert!(parent_events_rx.try_recv().is_err());

            let pending = child
                .chat_state_handle
                .pending_notifications()
                .await
                .expect("child inbox must be available");
            let child_turn = child
                .events
                .current_turn()
                .expect("child test turn must have an active tracker identity");
            let child_timeline = chat_state::Timeline::from_events(
                child
                    .chat_state_handle
                    .timeline_events()
                    .await
                    .expect("read the child causal timeline"),
            )
            .expect("child timeline must replay");
            assert_eq!(
                child_timeline.active_turn(),
                Some(child_turn),
                "notification consumption must use the active durable Turn, not a fixture id"
            );
            let child_step = child
                .events
                .active_step_id()
                .expect("child test turn must have an active Step");
            assert_eq!(
                child_timeline.active_step(),
                Some(child_step),
                "child Step must be durable before consuming its notification"
            );
            child
                .consume_notifications_durably(
                    pending.into_iter().map(|item| item.id).collect(),
                    child_turn,
                    None,
                )
                .await
                .expect("child must materialize the exact received owner message");
            let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
            child
                .handle_planning_command(PlanningCommand::Write {
                    prompt_id: "test-active-turn".into(),
                    input: TodoWriteInput {
                        merge: true,
                        todos: vec![TodoUpdate {
                            id: "child-step-1".into(),
                            content: Some("Build the revised change".into()),
                            status: Some(TodoStatus::InProgress),
                            priority: None,
                            requirements: None,
                            removed: None,
                        }],
                        actions: vec![PlanningAction::HandleReplans {
                            ids: vec![change_id.clone()],
                            no_change_reason: None,
                        }],
                    },
                    respond_to: handle_tx,
                })
                .await;
            handle_rx
                .await
                .expect("child handling command must answer")
                .expect("child must revise its local plan and durably handle consumed replan");
            let child_plan_after = child
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                child_plan_after.plan.steps[0].content,
                "Build the revised change"
            );
            assert!(matches!(
                &child_plan_after.plan.replans[0].progress,
                tool_types::ReplanProgress::Handled {
                    outcome: tool_types::ReplanOutcome::Updated { plan_revision }
                } if *plan_revision == child_plan_after.plan.revision
            ));

            let child_event =
                tokio::time::timeout(std::time::Duration::from_secs(2), child_events_rx.recv())
                    .await
                    .expect("handled child fact must dispatch its direct-parent receipt")
                    .expect("child backend event must arrive");
            let SubagentEvent::Interact(reply) = child_event else {
                panic!("handled replan must use the owner interaction route");
            };
            let AgentInteraction::ReplanHandled {
                change_id: replied_change,
                outcome,
                ..
            } = reply.action
            else {
                panic!("child must return a ReplanHandled receipt");
            };
            assert_eq!(replied_change, change_id);
            let stable_handled_request_id = reply.id.clone();
            let mut final_handled_sender = None;
            let mut final_handled_id = String::new();
            reply
                .respond_to
                .send(Ok(AgentInteractionOutput::message_unconfirmed(
                    reply.id,
                    "test_unconfirmed",
                    "the initial handling receipt attempt was not acknowledged",
                )))
                .expect("initial handled receipt must remain recoverable");
            for attempt in 0..4 {
                let retry =
                    tokio::time::timeout(std::time::Duration::from_secs(4), child_events_rx.recv())
                        .await
                        .expect("a durable Handled fact must keep retrying its parent receipt")
                        .expect("handled receipt retry must arrive");
                let SubagentEvent::Interact(next_reply) = retry else {
                    panic!("handled receipt retry must use the owner interaction route");
                };
                let next_reply_id = next_reply.id;
                let next_reply_sender = next_reply.respond_to;
                assert_eq!(next_reply_id, stable_handled_request_id);
                let AgentInteraction::ReplanHandled {
                    change_id: retried_change,
                    outcome: retried_outcome,
                    ..
                } = next_reply.action
                else {
                    panic!("retry must carry the same handled receipt");
                };
                assert_eq!(retried_change, change_id);
                assert_eq!(retried_outcome, outcome);
                if attempt < 3 {
                    next_reply_sender
                        .send(Ok(AgentInteractionOutput::message_unconfirmed(
                            next_reply_id,
                            "test_unconfirmed",
                            "the handling receipt attempt was not acknowledged",
                        )))
                        .expect("intermediate handled receipt must remain recoverable");
                } else {
                    final_handled_id = next_reply_id;
                    final_handled_sender = Some(next_reply_sender);
                }
            }
            parent
                .receive_replan_handled(&child_id, &replied_change, outcome)
                .await
                .expect("direct parent must durably accept the exact child receipt");
            final_handled_sender
                .expect("fifth handled receipt must have a live responder")
                .send(Ok(AgentInteractionOutput::message_received(
                    final_handled_id,
                    "handled-receipt",
                )))
                .expect("child receipt sender must still be alive");
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                while child.planning_delivery.is_running() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("a received Handled ACK must terminate this bounded retry run");
            assert!(child_events_rx.try_recv().is_err());

            let handled = parent
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                handled.plan.replans[0].progress,
                tool_types::ReplanProgress::Handled {
                    outcome: tool_types::ReplanOutcome::Updated {
                        plan_revision: child_plan_after.plan.revision,
                    }
                }
            );

            let (complete_tx, complete_rx) = tokio::sync::oneshot::channel();
            parent
                .handle_planning_command(PlanningCommand::Write {
                    prompt_id: "test-active-turn".into(),
                    input: TodoWriteInput {
                        merge: true,
                        todos: vec![TodoUpdate {
                            id: "step-1".into(),
                            content: None,
                            status: Some(TodoStatus::Completed),
                            priority: None,
                            requirements: None,
                            removed: None,
                        }],
                        actions: Vec::new(),
                    },
                    respond_to: complete_tx,
                })
                .await;
            complete_rx
                .await
                .expect("completion command must answer")
                .expect("completion status must commit");
            let after_completion = parent
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(after_completion.plan.replans.len(), 1);
            assert_eq!(
                after_completion.plan.replans[0].progress,
                handled.plan.replans[0].progress
            );
            assert!(
                tokio::time::timeout(
                    std::time::Duration::from_millis(50),
                    parent_events_rx.recv(),
                )
                .await
                .is_err(),
                "completion alone must not emit another replan"
            );
        })
        .await;
}

async fn select_planning_scope(
    actor: &std::sync::Arc<SessionActor>,
    prompt_id: &str,
) -> Result<(), String> {
    use tools::implementations::grow_build::todo::{PlanningAction, TodoWriteInput};

    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle_planning_command(PlanningCommand::Write {
            prompt_id: prompt_id.into(),
            input: TodoWriteInput {
                merge: true,
                todos: Vec::new(),
                actions: vec![PlanningAction::SelectScope {
                    scope_id: "work:next".into(),
                }],
            },
            respond_to,
        })
        .await;
    response
        .await
        .expect("scope selection must answer")
        .map(|_| ())
}

async fn release_undispatched_link(actor: &std::sync::Arc<SessionActor>, prompt_id: &str) {
    use tools::implementations::grow_build::todo::{PlanningAction, TodoWriteInput};

    let current = actor
        .chat_state_handle
        .planning_state()
        .await
        .expect("read planning state")
        .expect("plan exists");
    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle_planning_command(PlanningCommand::Write {
            prompt_id: prompt_id.into(),
            input: TodoWriteInput {
                merge: true,
                todos: Vec::new(),
                actions: vec![PlanningAction::Review {
                    review: DeliveryReview {
                        link_id: "task-1".into(),
                        result_id: None,
                        reviewed_source_revision: current.plan.revision,
                        decision: ReviewDecision::Released,
                        reason: Some("task was rejected before dispatch".into()),
                        verification_refs: Vec::new(),
                    },
                }],
            },
            respond_to,
        })
        .await;
    response
        .await
        .expect("review must answer")
        .expect("released review must commit");
}

#[tokio::test(flavor = "current_thread")]
async fn hydration_closes_legacy_replans_after_startup_failure_once() {
    use crate::session::actor::tests::support::install_test_foreground;

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut actor = linked_actor().await;
            let snapshot = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            let mut legacy = snapshot.plan;
            legacy
                .update_steps(
                    tool_types::PlanUpdateMode::Merge,
                    vec![PlanStep {
                        content: "changed after startup failure".into(),
                        ..legacy.steps[0].clone()
                    }],
                )
                .unwrap();
            // Older code generated this state after recording the failure.
            legacy.links[0].dispatch_failure = Some("cwd outside parent workspace".into());
            legacy.replans[0].progress = tool_types::ReplanProgress::Dispatching;
            actor
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                    chat_state::timeline::PlanningEvent {
                        previous: Some(snapshot.seq),
                        plan: Box::new(legacy.clone()),
                    },
                ))
                .await
                .unwrap();
            let history = actor.chat_state_handle.timeline_events().await.unwrap();
            let sampling = actor.chat_state_handle.get_sampling_config().await.unwrap();
            let (events, _events_rx) = tokio::sync::mpsc::unbounded_channel();
            std::sync::Arc::get_mut(&mut actor)
                .unwrap()
                .chat_state_handle = chat_state::ChatStateActor::spawn_from_timeline(
                history,
                sampling,
                Box::new(chat_state::NullTimelinePersistence),
                events,
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
            let restored = std::sync::Arc::get_mut(&mut actor).unwrap();
            restored.events =
                crate::session::events::EventTracker::new(restored.chat_state_handle.clone());

            actor.hydrate_local_plan().await.unwrap();
            let repaired = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert!(repaired.plan.links[0].pending_source_revision.is_none());
            assert!(!repaired.plan.links[0].terminal);
            assert!(repaired.plan.links[0].result_id.is_none());
            assert_eq!(
                repaired.plan.links[0].dispatch_failure,
                legacy.links[0].dispatch_failure
            );
            assert_eq!(repaired.plan.replans[0].id, legacy.replans[0].id);
            assert!(matches!(&repaired.plan.replans[0].progress,
                tool_types::ReplanProgress::Unapplied { reason }
                    if reason == "cwd outside parent workspace"));
            assert_eq!(
                actor.planning_completion_blockers().await.unwrap().len(),
                1,
                "the required delivery still needs explicit release"
            );
            actor.hydrate_local_plan().await.unwrap();
            assert_eq!(
                actor
                    .chat_state_handle
                    .planning_state()
                    .await
                    .unwrap()
                    .unwrap()
                    .seq,
                repaired.seq
            );

            install_test_foreground(&actor, "release-recovered-failure").await;
            release_undispatched_link(&actor, "release-recovered-failure").await;
            assert!(
                actor
                    .planning_completion_blockers()
                    .await
                    .unwrap()
                    .is_empty()
            );
            select_planning_scope(&actor, "release-recovered-failure")
                .await
                .unwrap();
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn required_undispatched_scope_switch_needs_released_review() {
    use crate::session::actor::tests::support::install_test_foreground;

    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor().await;
            install_test_foreground(&actor, "scope-required-unreleased").await;
            record_undispatched(&actor, "coordinator admission closed")
                .await
                .expect("dispatch failure must be recorded");

            let before = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            let error = select_planning_scope(&actor, "scope-required-unreleased")
                .await
                .expect_err("unreleased required delivery must block scope switch");
            assert!(error.contains("dispose old executions"));
            let after = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(after.seq, before.seq);
            assert_eq!(after.plan, before.plan);
            assert!(!after.plan.links[0].terminal);
            assert!(after.plan.links[0].result_id.is_none());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn optional_undispatched_scope_switch_succeeds_without_review() {
    use crate::session::actor::tests::support::install_test_foreground;

    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor_with_required(false).await;
            install_test_foreground(&actor, "scope-optional-undispatched").await;
            record_undispatched(&actor, "coordinator admission closed")
                .await
                .expect("dispatch failure must be recorded");

            select_planning_scope(&actor, "scope-optional-undispatched")
                .await
                .expect("optional explicit dispatch failure is disposed");
            let current = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(current.plan.scope_id, "work:next");
            assert!(current.plan.links.is_empty());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn released_required_undispatched_scope_switch_succeeds_without_faking_result() {
    use crate::session::actor::tests::support::install_test_foreground;

    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor().await;
            install_test_foreground(&actor, "scope-required-released").await;
            record_undispatched(&actor, "coordinator admission closed")
                .await
                .expect("dispatch failure must be recorded");
            release_undispatched_link(&actor, "scope-required-released").await;

            let released = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert!(
                actor
                    .planning_completion_blockers()
                    .await
                    .unwrap()
                    .is_empty()
            );
            let next = tool_types::LocalPlan::new(actor.session_id_string(), "work:next");
            next.validate_transition_from(&released.plan)
                .expect("LocalPlan accepts the explicitly released dispatch failure");
            let link = &released.plan.links[0];
            assert!(!link.terminal);
            assert!(link.result_id.is_none());
            assert_eq!(
                link.dispatch_failure.as_deref(),
                Some("coordinator admission closed")
            );

            select_planning_scope(&actor, "scope-required-released")
                .await
                .expect("actor must use LocalPlan transition rules");
            let current = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(current.plan.scope_id, "work:next");
            assert!(current.plan.links.is_empty());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn active_or_unknown_link_blocks_scope_switch_without_faking_terminal() {
    use crate::session::actor::tests::support::install_test_foreground;

    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor_with_required(false).await;
            install_test_foreground(&actor, "scope-live-link").await;
            release_undispatched_link(&actor, "scope-live-link").await;
            let before = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(before.plan.reviews.len(), 1);
            assert_eq!(before.plan.reviews[0].decision, ReviewDecision::Released);
            let error = select_planning_scope(&actor, "scope-live-link")
                .await
                .expect_err("an active link must block even after Released review");
            assert!(error.contains("dispose old executions"));
            let after = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(after.seq, before.seq);
            assert_eq!(after.plan, before.plan);
            let link = &after.plan.links[0];
            assert!(!link.terminal);
            assert!(link.result_id.is_none());
            assert!(link.dispatch_failure.is_none());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn optional_released_undispatched_scope_switch_succeeds() {
    use crate::session::actor::tests::support::install_test_foreground;

    tokio::task::LocalSet::new()
        .run_until(async {
            let actor = linked_actor_with_required(false).await;
            install_test_foreground(&actor, "scope-optional-released").await;
            record_undispatched(&actor, "coordinator admission closed")
                .await
                .expect("dispatch failure must be recorded");
            release_undispatched_link(&actor, "scope-optional-released").await;

            select_planning_scope(&actor, "scope-optional-released")
                .await
                .expect("optional explicit dispatch failure remains disposed after review");
            let current = actor
                .chat_state_handle
                .planning_state()
                .await
                .unwrap()
                .unwrap();
            assert_eq!(current.plan.scope_id, "work:next");
        })
        .await;
}

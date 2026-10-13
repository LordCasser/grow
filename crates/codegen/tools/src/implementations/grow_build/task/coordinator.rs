//! Single-writer subagent coordinator actor.
//!
//! The actor owns the command receiver, pending/active/completed state,
//! concrete blocking waiters, foreground deadlines, cancellation, and the
//! terminal delivery disposition. All hosts drive it through `ChannelBackend`;
//! only their `ChildRunner` implementations differ.
//!
//! Lifecycle state stays actor-owned. A runner's lifecycle futures may be
//! `Send` or non-`Send`; the resulting actor future inherits this property
//! naturally on stable Rust. Persisted-output reads use a bounded `Send`
//! worker queue so filesystem I/O never blocks command handling.

mod query;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use futures::FutureExt;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::{mpsc, oneshot};

use super::coordinator_state::{
    ActiveChild, BlockingWaiter, ChildRecord, CompletedChild, InternalEvent,
    MAX_FINALIZATION_OUTPUT_BYTES, MAX_FINALIZATIONS, OutputReadRequest, PendingChild, ReplyFuture,
    TaggedFuture, active_summary, background_at_deadline, background_if_caller_gone,
    completed_snapshot, sleep_until, workflow_outstanding,
};
use super::types::{
    SpawnedSubagentRef, SubagentCancelOutcome, SubagentCancelTarget, SubagentEvent,
    SubagentOutstandingReply, SubagentOwner, SubagentRegistryCounts, SubagentRequest,
    SubagentResult, SubagentValidateTypeOutcome,
};

pub use super::coordinator_state::{
    ChildCompletion, ChildControl, ChildReporter, ChildRunOutput, ChildRunRequest, ChildRunner,
    CompletionDisposition, CoordinatorConfig, LocalBoxFuture, MAX_COMPLETED_ENTRIES, SendBoxFuture,
    StartedChild, SubagentProgress,
};

/// Channel-owned subagent lifecycle actor.
pub struct SubagentCoordinator<R: ChildRunner> {
    commands: mpsc::UnboundedReceiver<SubagentEvent>,
    internal_tx: mpsc::UnboundedSender<InternalEvent<R::Control>>,
    internal_rx: mpsc::UnboundedReceiver<InternalEvent<R::Control>>,
    runner: R,
    config: CoordinatorConfig,
    pending: HashMap<String, PendingChild>,
    active: HashMap<String, ActiveChild<R::Control>>,
    completed: HashMap<String, CompletedChild>,
    completed_order: VecDeque<String>,
    finalizations: HashMap<String, PendingFinalization<R::CompletionData>>,
    reconciliations: FuturesUnordered<
        TaggedFuture<futures::future::CatchUnwind<std::panic::AssertUnwindSafe<R::RunFuture>>>,
    >,
    waiters: HashMap<String, Vec<BlockingWaiter>>,
    goal_cancel_waiters: Vec<GoalCancelWaiter>,
    session_cancel_waiters: Vec<GoalCancelWaiter>,
    workflow_cancel_waiters: HashMap<String, Vec<oneshot::Sender<SubagentCancelOutcome>>>,
    /// Parent sessions that received `ParentSession` cancel. Non-workflow spawns
    /// are rejected until [`SubagentEvent::OpenSpawnAdmission`] (next turn) or
    /// teardown, so a detached late `TaskTool` spawn cannot outrun Stop.
    spawn_blocked_sessions: HashSet<String>,
    /// Prompt scopes that have been cancelled. A late spawn from either the
    /// lifecycle root or its immediate parent is rejected until session teardown.
    cancelled_prompt_scopes: HashSet<PromptScope>,
    usage_not_applied_prompts: HashSet<PromptScope>,
    runs: FuturesUnordered<
        TaggedFuture<futures::future::CatchUnwind<std::panic::AssertUnwindSafe<R::RunFuture>>>,
    >,
    validations: FuturesUnordered<ReplyFuture<R::ValidateFuture, SubagentValidateTypeOutcome>>,
    output_reads: FuturesUnordered<
        TaggedFuture<
            SendBoxFuture<Result<Option<Arc<str>>, super::types::SubagentObservationError>>,
        >,
    >,
    output_read_requests: HashMap<String, OutputReadRequest>,
    output_read_permits: Arc<tokio::sync::Semaphore>,
}

struct PendingFinalization<D> {
    output: ChildRunOutput<D>,
    disposition: CompletionDisposition,
    attempts: usize,
    cycle_deadline: tokio::time::Instant,
    next_cycle_deadline: Option<tokio::time::Instant>,
    in_flight: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PromptScope {
    parent_session_id: String,
    prompt_id: String,
}

struct GoalCancelWaiter {
    /// Exact children that existed at cancellation admission. Later work for
    /// the same long-lived Goal is a new lifecycle generation and must not
    /// extend this barrier.
    remaining_ids: HashSet<String>,
    respond_to: oneshot::Sender<SubagentCancelOutcome>,
}

/// A communication route is deliberately classified before it reaches the
/// host runner. New messages can only use an active owned child. Replies may
/// use a previously known direct lineage so the host can perform its
/// read-only receipt authorization, but that route never grants admission to
/// a new inbound message by itself.
enum InteractionRoute {
    ActiveChild {
        target_session_id: String,
        subagent_task_name: String,
    },
    DirectParent {
        target_session_id: String,
        subagent_task_name: String,
    },
    Reply {
        target_session_id: String,
        subagent_task_name: String,
    },
    ReceiptOnly {
        target_session_id: String,
        subagent_task_name: String,
    },
}

impl InteractionRoute {
    fn target_session_id(&self) -> &str {
        match self {
            Self::ActiveChild {
                target_session_id, ..
            }
            | Self::DirectParent {
                target_session_id, ..
            } => target_session_id,
            Self::Reply {
                target_session_id, ..
            } => target_session_id,
            Self::ReceiptOnly {
                target_session_id, ..
            } => target_session_id,
        }
    }

    fn subagent_task_name(&self) -> &str {
        match self {
            Self::ActiveChild {
                subagent_task_name, ..
            }
            | Self::DirectParent {
                subagent_task_name, ..
            } => subagent_task_name,
            Self::Reply {
                subagent_task_name, ..
            } => subagent_task_name,
            Self::ReceiptOnly {
                subagent_task_name, ..
            } => subagent_task_name,
        }
    }

    fn receipt_only(&self) -> bool {
        matches!(self, Self::ReceiptOnly { .. })
    }
}

impl PromptScope {
    fn new(parent_session_id: String, prompt_id: String) -> Self {
        Self {
            parent_session_id,
            prompt_id,
        }
    }
}

impl<R: ChildRunner> SubagentCoordinator<R> {
    pub fn new(
        commands: mpsc::UnboundedReceiver<SubagentEvent>,
        runner: R,
        config: CoordinatorConfig,
    ) -> Self {
        let (internal_tx, internal_rx) = mpsc::unbounded_channel();
        Self {
            commands,
            internal_tx,
            internal_rx,
            runner,
            config,
            pending: HashMap::new(),
            active: HashMap::new(),
            completed: HashMap::new(),
            completed_order: VecDeque::new(),
            finalizations: HashMap::new(),
            reconciliations: FuturesUnordered::new(),
            waiters: HashMap::new(),
            goal_cancel_waiters: Vec::new(),
            session_cancel_waiters: Vec::new(),
            workflow_cancel_waiters: HashMap::new(),
            spawn_blocked_sessions: HashSet::new(),
            cancelled_prompt_scopes: HashSet::new(),
            usage_not_applied_prompts: HashSet::new(),
            runs: FuturesUnordered::new(),
            validations: FuturesUnordered::new(),
            output_reads: FuturesUnordered::new(),
            output_read_requests: HashMap::new(),
            output_read_permits: Arc::new(tokio::sync::Semaphore::new(8)),
        }
    }

    pub async fn run(mut self) {
        let mut commands_open = true;
        let mut recovery_tick = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            if !commands_open
                && self.runs.is_empty()
                && self.reconciliations.is_empty()
                && self.validations.is_empty()
                && self.output_reads.is_empty()
            {
                break;
            }

            let deadline = self.next_deadline();
            tokio::select! {
                biased;
                Some(event) = self.internal_rx.recv() => self.handle_internal(event),
                Some((id, output)) = self.runs.next(), if !self.runs.is_empty() => {
                    match output {
                        Ok(output) => self.finish_child(&id, output),
                        Err(_) => self.finish_panicked_child(&id),
                    }
                }
                Some((id, output)) = self.reconciliations.next(), if !self.reconciliations.is_empty() => {
                    self.finish_reconciliation(&id, output);
                }
                _ = recovery_tick.tick(), if !self.finalizations.is_empty() => self.advance_finalizations(),
                Some((respond_to, outcome)) = self.validations.next(), if !self.validations.is_empty() => {
                    let _ = respond_to.send(outcome);
                }
                Some((key, output)) = self.output_reads.next(), if !self.output_reads.is_empty() => {
                    self.finish_output_read(&key, output);
                }
                command = self.commands.recv(), if commands_open => {
                    match command {
                        Some(command) => {
                            self.reap_abandoned_callers();
                            self.handle_command(command);
                        }
                        None => commands_open = false,
                    }
                }
                _ = sleep_until(deadline), if deadline.is_some() => self.process_deadlines(),
            }
            while self.completed.len() > MAX_COMPLETED_ENTRIES {
                let Some(id) = self.completed_order.pop_front() else {
                    break;
                };
                self.completed.remove(&id);
            }
        }

        self.cancel_all_children();
    }

    fn handle_command(&mut self, command: SubagentEvent) {
        match command {
            SubagentEvent::Interact(mut request) => {
                let route = self.interaction_route(&request);
                match route {
                    Some(route) if route.receipt_only() || !request.cancellation.is_cancelled() => {
                        request.receipt_only = route.receipt_only();
                        self.runner.interact(
                            request,
                            route.target_session_id().to_owned(),
                            route.subagent_task_name().to_owned(),
                        )
                    }
                    _ => self.reject_interaction(
                        request,
                        "no_direct_route",
                        "No active direct parent-child route for this operation",
                    ),
                }
            }
            SubagentEvent::Spawn(command) => {
                let mut request = *command.request;
                let immediate_parent_session_id = request.parent_session_id.clone();
                let mut security_parent = None;
                if let Some((
                    root_parent,
                    loop_task_id,
                    spawner_cancelled,
                    spawner_owner,
                    spawner_security_context,
                )) = self
                    .active
                    .values()
                    .find(|child| child.child_session_id == request.parent_session_id)
                    .map(|child| {
                        (
                            child.request.parent_session_id.clone(),
                            child.request.runtime_overrides.loop_task_id.clone(),
                            child.cancellation.is_cancelled(),
                            child.request.owner.clone(),
                            child.control.security_context(),
                        )
                    })
                {
                    if spawner_cancelled {
                        // The parent subagent is being torn down, so its late
                        // child would be orphaned against the closed scope.
                        let id = request.id.clone();
                        let _ = command.result_tx.send(SubagentResult {
                            success: false,
                            cancelled: true,
                            error: Some("parent subagent is being torn down".to_owned()),
                            subagent_id: id.clone(),
                            child_session_id: id,
                            ..Default::default()
                        });
                        return;
                    }
                    security_parent = Some(spawner_security_context);
                    request.parent_session_id = root_parent;
                    request.surface_completion = false;
                    // Nested children keep workflow lineage after reparent so
                    // ParentSession Stop does not kill in-flight workflow work.
                    if !request.owner.is_workflow()
                        && let Some(run_id) = spawner_owner.workflow_run_id()
                    {
                        request.owner = SubagentOwner::workflow(run_id);
                    }
                    if request.runtime_overrides.loop_task_id.is_none() {
                        request.runtime_overrides.loop_task_id = loop_task_id;
                    }
                }
                if self.prompt_scope_cancelled(&request, &immediate_parent_session_id) {
                    let id = request.id.clone();
                    let _ = command.result_tx.send(SubagentResult {
                        success: false,
                        cancelled: true,
                        error: Some("parent prompt is cancelled".to_owned()),
                        subagent_id: id.clone(),
                        child_session_id: id,
                        ..Default::default()
                    });
                    return;
                }
                // Late Task spawn after user Stop (detached TaskTool background).
                if !request.owner.is_workflow()
                    && (self
                        .spawn_blocked_sessions
                        .contains(&request.parent_session_id)
                        || self
                            .spawn_blocked_sessions
                            .contains(&immediate_parent_session_id))
                {
                    let id = request.id.clone();
                    let _ = command.result_tx.send(SubagentResult {
                        success: false,
                        cancelled: true,
                        error: Some("parent session is stopped".to_owned()),
                        subagent_id: id.clone(),
                        child_session_id: id,
                        ..Default::default()
                    });
                    return;
                }
                let id = request.id.clone();
                if self.pending.contains_key(&id)
                    || self.active.contains_key(&id)
                    || self.completed.contains_key(&id)
                {
                    let _ = command.result_tx.send(SubagentResult {
                        success: false,
                        error: Some(format!("Subagent id '{id}' already exists")),
                        subagent_id: id.clone(),
                        child_session_id: id,
                        ..Default::default()
                    });
                    return;
                }
                if self.pending.len() + self.active.len() + self.finalizations.len()
                    >= MAX_FINALIZATIONS
                {
                    let _ = command.result_tx.send(SubagentResult {
                        error: Some("subagent recovery capacity exhausted; resolve pending finalizations before spawning".into()),
                        subagent_id: request.id.clone(), child_session_id: request.id.clone(),
                        ..Default::default()
                    });
                    return;
                }
                let cancellation = request.cancel_token.clone();
                let handle_only = request.run_in_background;
                let foreground_deadline = (!request.run_in_background
                    && !request.await_to_completion)
                    .then(|| tokio::time::Instant::now() + self.config.foreground_budget);
                self.pending.insert(
                    id.clone(),
                    PendingChild {
                        request: request.clone(),
                        immediate_parent_session_id: immediate_parent_session_id.clone(),
                        started_at: std::time::Instant::now(),
                        cancellation: cancellation.clone(),
                        spawn_reply: Some(command.result_tx),
                        foreground_deadline,
                        handle_only,
                        explicitly_killed: false,
                    },
                );
                self.running_count_changed();
                let reporter = ChildReporter {
                    subagent_id: id.clone(),
                    tx: self.internal_tx.clone(),
                };
                self.runs.push(TaggedFuture {
                    subagent_id: id,
                    future: Box::pin(
                        std::panic::AssertUnwindSafe(self.runner.run(ChildRunRequest {
                            request,
                            security_parent_session_id: immediate_parent_session_id,
                            security_parent,
                            cancellation,
                            reporter,
                        }))
                        .catch_unwind(),
                    ),
                });
            }
            SubagentEvent::Query(query) => {
                self.handle_query(
                    query.subagent_id,
                    query.parent_session_id,
                    query.block,
                    query.timeout_ms,
                    query.respond_to,
                );
            }
            SubagentEvent::Cancel(request) => match request.target {
                SubagentCancelTarget::SubagentId(id) => {
                    let known = self
                        .pending
                        .get(&id)
                        .map(|child| {
                            belongs_to_session(
                                &child.request,
                                &child.immediate_parent_session_id,
                                request.parent_session_id.as_deref(),
                            )
                        })
                        .or_else(|| {
                            self.active.get(&id).map(|child| {
                                belongs_to_session(
                                    &child.request,
                                    &child.immediate_parent_session_id,
                                    request.parent_session_id.as_deref(),
                                )
                            })
                        })
                        .or_else(|| {
                            self.completed.get(&id).map(|child| {
                                belongs_to_session(
                                    &child.request,
                                    &child.immediate_parent_session_id,
                                    request.parent_session_id.as_deref(),
                                )
                            })
                        });
                    let outcome = if known == Some(false)
                        || (known.is_none() && request.parent_session_id.is_some())
                    {
                        SubagentCancelOutcome::Unauthorized
                    } else {
                        self.cancel_one(&id, request.parent_session_id.as_deref(), true)
                    };
                    let _ = request.respond_to.send(outcome);
                }
                SubagentCancelTarget::ParentPromptId(prompt_id) => {
                    self.cancel_parent_prompt(&prompt_id, request.parent_session_id.as_deref());
                    let _ = request.respond_to.send(SubagentCancelOutcome::Cancelled);
                }
                SubagentCancelTarget::ParentSession => {
                    let remaining_ids =
                        self.session_child_ids(request.parent_session_id.as_deref());
                    let outcome = self.cancel_parent_session(request.parent_session_id.as_deref());
                    if remaining_ids.is_empty()
                        || matches!(outcome, SubagentCancelOutcome::NotFound)
                    {
                        let _ = request.respond_to.send(outcome);
                    } else {
                        self.session_cancel_waiters.push(GoalCancelWaiter {
                            remaining_ids,
                            respond_to: request.respond_to,
                        });
                    }
                }
                SubagentCancelTarget::Goal {
                    goal_id,
                    definition_revision,
                } => {
                    let remaining_ids = self.goal_child_ids(
                        &goal_id,
                        definition_revision,
                        request.parent_session_id.as_deref(),
                    );
                    self.cancel_goal_children(
                        &goal_id,
                        definition_revision,
                        request.parent_session_id.as_deref(),
                    );
                    if remaining_ids.is_empty() {
                        let _ = request.respond_to.send(SubagentCancelOutcome::Cancelled);
                    } else {
                        self.goal_cancel_waiters.push(GoalCancelWaiter {
                            remaining_ids,
                            respond_to: request.respond_to,
                        });
                    }
                }
                SubagentCancelTarget::WorkflowRunId(run_id) => {
                    self.cancel_workflow_children(&run_id, request.parent_session_id.as_deref());
                    if workflow_outstanding(&self.pending, &self.active, &run_id) == 0 {
                        let _ = request.respond_to.send(SubagentCancelOutcome::Cancelled);
                    } else {
                        self.workflow_cancel_waiters
                            .entry(run_id)
                            .or_default()
                            .push(request.respond_to);
                    }
                }
            },
            SubagentEvent::ListActive(request) => {
                let summaries = self
                    .active
                    .values()
                    .filter(|child| {
                        belongs_to_session(
                            &child.request,
                            &child.immediate_parent_session_id,
                            Some(&request.parent_session_id),
                        ) && !child.request.owner.is_workflow()
                    })
                    .map(active_summary)
                    .collect();
                let _ = request.respond_to.send(summaries);
            }
            SubagentEvent::ListRunning(request) => {
                self.handle_list_running(request.parent_session_id, request.respond_to);
            }
            SubagentEvent::TeardownSession { parent_session_id } => {
                self.spawn_blocked_sessions.remove(&parent_session_id);
                self.cancelled_prompt_scopes
                    .retain(|scope| scope.parent_session_id != parent_session_id);
                self.teardown_session_children(&parent_session_id);
            }
            SubagentEvent::OpenSpawnAdmission { parent_session_id } => {
                self.spawn_blocked_sessions.remove(&parent_session_id);
                // This is emitted after a new turn's durable admission barrier.
                // It supplies an explicit new recovery cycle, never reconnect.
                for (id, pending) in &mut self.finalizations {
                    if self
                        .completed
                        .get(id)
                        .is_some_and(|child| child.request.parent_session_id == parent_session_id)
                    {
                        let deadline =
                            tokio::time::Instant::now() + std::time::Duration::from_secs(30);
                        if pending.in_flight {
                            // The current owner cannot be displaced. Preserve
                            // this explicit admission so its result can open
                            // the next cycle after the in-flight attempt ends.
                            pending.next_cycle_deadline = Some(deadline);
                        } else {
                            pending.attempts = 0;
                            pending.cycle_deadline = deadline;
                        }
                    }
                }
            }
            SubagentEvent::Outstanding(request) => {
                // Reap again here so turn-freeze / Outstanding polls see
                // ParentGone even if no other command woke the actor first.
                self.reap_abandoned_callers();
                let mut live_ids: Vec<_> = self
                    .pending
                    .values()
                    .filter(|child| {
                        belongs_to_session(
                            &child.request,
                            &child.immediate_parent_session_id,
                            Some(&request.parent_session_id),
                        ) && child.request.parent_prompt_id.as_deref() == Some(&request.prompt_id)
                            && !child.request.owner.is_workflow()
                            && !child.handle_only
                    })
                    .map(|child| child.request.id.clone())
                    .chain(
                        self.active
                            .values()
                            .filter(|child| {
                                belongs_to_session(
                                    &child.request,
                                    &child.immediate_parent_session_id,
                                    Some(&request.parent_session_id),
                                )
                                    && child.request.parent_prompt_id.as_deref()
                                        == Some(&request.prompt_id)
                                    && !child.request.owner.is_workflow()
                                    // Definition-declared background children are
                                    // background for accounting even while the
                                    // spawning tool block-awaits them.
                                    && !child.handle_only
                                    && !child.definition_background
                            })
                            .map(|child| child.request.id.clone()),
                    )
                    .collect();
                live_ids.sort();
                let background_live = self.pending.values().any(|child| {
                    belongs_to_session(
                        &child.request,
                        &child.immediate_parent_session_id,
                        Some(&request.parent_session_id),
                    ) && child.request.parent_prompt_id.as_deref() == Some(&request.prompt_id)
                        && !child.request.owner.is_workflow()
                        && child.handle_only
                }) || self.active.values().any(|child| {
                    belongs_to_session(
                        &child.request,
                        &child.immediate_parent_session_id,
                        Some(&request.parent_session_id),
                    ) && child.request.parent_prompt_id.as_deref() == Some(&request.prompt_id)
                        && !child.request.owner.is_workflow()
                        && (child.handle_only || child.definition_background)
                });
                let scope =
                    PromptScope::new(request.parent_session_id.clone(), request.prompt_id.clone());
                let _ = request.respond_to.send(SubagentOutstandingReply {
                    live_ids,
                    background_live,
                    subagent_usage_not_applied: self.usage_not_applied_prompts.contains(&scope),
                });
            }
            SubagentEvent::ClearUsageNotApplied(request) => {
                self.usage_not_applied_prompts.remove(&PromptScope::new(
                    request.parent_session_id,
                    request.prompt_id,
                ));
            }
            SubagentEvent::MarkUsageNotApplied(request) => {
                self.usage_not_applied_prompts.insert(PromptScope::new(
                    request.parent_session_id,
                    request.prompt_id,
                ));
                let _ = request.respond_to.send(());
            }
            SubagentEvent::RegistryCounts(request) => {
                let _ = request.respond_to.send(SubagentRegistryCounts {
                    pending: self.pending.len(),
                    active: self.active.len(),
                    completed: self.completed.len(),
                });
            }
            SubagentEvent::Inspect(request) => {
                self.handle_inspect(
                    request.subagent_id,
                    request.parent_session_id,
                    request.respond_to,
                );
            }
            SubagentEvent::SpawnedRefs(request) => {
                let mut refs: Vec<_> = self
                    .active
                    .values()
                    .filter(|child| {
                        belongs_to_session(
                            &child.request,
                            &child.immediate_parent_session_id,
                            Some(&request.parent_session_id),
                        ) && child.request.parent_prompt_id.as_deref() == Some(&request.prompt_id)
                    })
                    .map(|child| SpawnedSubagentRef {
                        subagent_id: child.request.id.clone(),
                        child_session_id: child.child_session_id.clone(),
                        subagent_type: child.request.subagent_type.clone(),
                        description: child.request.description.clone(),
                        resumed_from: child.resumed_from.clone(),
                    })
                    .chain(
                        self.completed
                            .values()
                            .filter(|child| {
                                belongs_to_session(
                                    &child.request,
                                    &child.immediate_parent_session_id,
                                    Some(&request.parent_session_id),
                                ) && child.request.parent_prompt_id.as_deref()
                                    == Some(&request.prompt_id)
                            })
                            .map(|child| SpawnedSubagentRef {
                                subagent_id: child.request.id.clone(),
                                child_session_id: child.child_session_id.clone(),
                                subagent_type: child.request.subagent_type.clone(),
                                description: child.request.description.clone(),
                                resumed_from: child.resumed_from.clone(),
                            }),
                    )
                    .collect();
                refs.sort_by(|a, b| a.subagent_id.cmp(&b.subagent_id));
                let _ = request.respond_to.send(refs);
            }
            SubagentEvent::ValidateType(request) => {
                self.validations.push(ReplyFuture {
                    future: Box::pin(
                        self.runner
                            .validate_type(request.subagent_type, request.parent_session_id),
                    ),
                    respond_to: Some(request.respond_to),
                });
            }
            SubagentEvent::LoopUnitActive(request) => {
                let is_active = self.pending.values().any(|child| {
                    child.request.runtime_overrides.loop_task_id.as_deref()
                        == Some(&request.task_id)
                }) || self.active.values().any(|child| {
                    child.request.runtime_overrides.loop_task_id.as_deref()
                        == Some(&request.task_id)
                });
                let _ = request.respond_to.send(is_active);
            }
        }
    }

    fn interaction_route(
        &self,
        request: &super::interaction::AgentInteractionRequest,
    ) -> Option<InteractionRoute> {
        use super::interaction::AgentInteraction;

        match (&request.action, request.target_child_id.as_deref()) {
            (AgentInteraction::Ask { .. }, Some(id)) => self
                .active
                .get(id)
                .filter(|child| {
                    child.immediate_parent_session_id == request.source_session_id
                        && !child.cancellation.is_cancelled()
                        && !child.request.owner.is_workflow()
                })
                .map(|child| InteractionRoute::ActiveChild {
                    target_session_id: child.child_session_id.clone(),
                    subagent_task_name: task_name(&child.request),
                }),
            (AgentInteraction::Send { reply_to: None, .. }, Some(id)) => {
                self.send_to_child_route(&request.source_session_id, id)
            }
            (AgentInteraction::Ask { .. }, None) => self
                .active
                .values()
                .find(|child| {
                    child.child_session_id == request.source_session_id
                        && !child.cancellation.is_cancelled()
                        && !child.request.owner.is_workflow()
                })
                .map(|child| InteractionRoute::DirectParent {
                    target_session_id: child.immediate_parent_session_id.clone(),
                    subagent_task_name: task_name(&child.request),
                }),
            (
                AgentInteraction::Send {
                    reply_to: Some(reply_to),
                    ..
                },
                None,
            ) => self.reply_route(&request.source_session_id, reply_to),
            // A new send must identify an owned child. A reply must not carry
            // an independently selected target; the referenced source session
            // is only a candidate until the runtime validates its receipt.
            _ => None,
        }
    }

    fn send_to_child_route(
        &self,
        source_session_id: &str,
        subagent_id: &str,
    ) -> Option<InteractionRoute> {
        if let Some(child) = self.active.get(subagent_id).filter(|child| {
            child.immediate_parent_session_id == source_session_id
                && !child.request.owner.is_workflow()
        }) {
            let route = InteractionRoute::ActiveChild {
                target_session_id: child.child_session_id.clone(),
                subagent_task_name: task_name(&child.request),
            };
            return Some(if child.cancellation.is_cancelled() {
                InteractionRoute::ReceiptOnly {
                    target_session_id: child.child_session_id.clone(),
                    subagent_task_name: task_name(&child.request),
                }
            } else {
                route
            });
        }

        // Completed children retain the direct ownership edge so a retry can
        // ask the target runtime to inspect an already durable receipt. This
        // route is read-only and cannot admit a new message.
        self.completed
            .get(subagent_id)
            .filter(|child| {
                child.immediate_parent_session_id == source_session_id
                    && !child.request.owner.is_workflow()
            })
            .map(|child| InteractionRoute::ReceiptOnly {
                target_session_id: child.child_session_id.clone(),
                subagent_task_name: task_name(&child.request),
            })
    }

    fn reply_route(
        &self,
        source_session_id: &str,
        reply_to: &sampling_types::AgentMessageRef,
    ) -> Option<InteractionRoute> {
        let active = self.active.values().find(|child| {
            child.child_session_id == source_session_id
                && child.immediate_parent_session_id == reply_to.source_session_id
                && !child.request.owner.is_workflow()
        });
        if let Some(child) = active {
            return Some(if child.cancellation.is_cancelled() {
                InteractionRoute::ReceiptOnly {
                    target_session_id: reply_to.source_session_id.clone(),
                    subagent_task_name: task_name(&child.request),
                }
            } else {
                InteractionRoute::Reply {
                    target_session_id: reply_to.source_session_id.clone(),
                    subagent_task_name: task_name(&child.request),
                }
            });
        }

        // Completed sources are retained only to verify an existing receipt;
        // this route must never admit a new reply delivery.
        self.completed
            .values()
            .find(|child| {
                child.child_session_id == source_session_id
                    && child.immediate_parent_session_id == reply_to.source_session_id
                    && !child.request.owner.is_workflow()
            })
            .map(|child| InteractionRoute::ReceiptOnly {
                target_session_id: reply_to.source_session_id.clone(),
                subagent_task_name: task_name(&child.request),
            })
    }

    fn reject_interaction(
        &self,
        request: super::interaction::AgentInteractionRequest,
        code: &str,
        message: &str,
    ) {
        use super::interaction::AgentInteraction;

        let result = match request.action {
            AgentInteraction::Send { .. } => Ok(
                super::interaction::AgentInteractionOutput::message_rejected(
                    request.id, code, message,
                ),
            ),
            AgentInteraction::Ask { .. } => Err(message.to_owned()),
        };
        let _ = request.respond_to.send(result);
    }

    fn handle_internal(&mut self, event: InternalEvent<R::Control>) {
        match event {
            InternalEvent::ExecutionReturned {
                subagent_id,
                outcome,
            } => {
                if let Some(child) = self.active.get_mut(&subagent_id) {
                    child.execution_outcome.get_or_insert(outcome);
                    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
                    child.foreground_deadline = Some(
                        child
                            .foreground_deadline
                            .map_or(deadline, |previous| previous.min(deadline)),
                    );
                }
                self.running_count_changed();
            }
            InternalEvent::Started {
                subagent_id,
                child,
                respond_to,
            } => {
                let Some(pending) = self.pending.remove(&subagent_id) else {
                    let _ = respond_to.send(false);
                    return;
                };
                if pending.cancellation.is_cancelled() {
                    self.pending.insert(subagent_id, pending);
                    let _ = respond_to.send(false);
                    return;
                }
                self.active.insert(
                    subagent_id,
                    ActiveChild {
                        execution_outcome: None,
                        request: pending.request,
                        immediate_parent_session_id: pending.immediate_parent_session_id,
                        started_at: pending.started_at,
                        cancellation: pending.cancellation,
                        spawn_reply: pending.spawn_reply,
                        foreground_deadline: pending.foreground_deadline,
                        handle_only: pending.handle_only,
                        definition_background: child.definition_background,
                        explicitly_killed: pending.explicitly_killed,
                        child_session_id: child.child_session_id,
                        resumed_from: child.resumed_from,
                        control: child.control,
                    },
                );
                let _ = respond_to.send(true);
            }
            InternalEvent::SourceIsActive {
                source_id,
                parent_session_id,
                respond_to,
            } => {
                let source_is_active = self.pending.get(&source_id).is_some_and(|child| {
                    belongs_to_session(
                        &child.request,
                        &child.immediate_parent_session_id,
                        Some(&parent_session_id),
                    )
                }) || self.active.get(&source_id).is_some_and(|child| {
                    belongs_to_session(
                        &child.request,
                        &child.immediate_parent_session_id,
                        Some(&parent_session_id),
                    )
                });
                let _ = respond_to.send(source_is_active);
            }
        }
    }

    fn finish_child(&mut self, id: &str, mut output: ChildRunOutput<R::CompletionData>) {
        let record = if let Some(child) = self.active.remove(id) {
            ChildRecord::Active(child)
        } else if let Some(child) = self.pending.remove(id) {
            ChildRecord::Pending(child)
        } else {
            return;
        };

        let request = record.request().clone();
        let explicitly_killed = record.explicitly_killed();
        let immediate_parent_session_id = record.immediate_parent_session_id().to_owned();
        let (started_at, child_session_id, resumed_from, mut spawn_reply, mut handle_only) =
            match record {
                ChildRecord::Pending(child) => (
                    child.started_at,
                    output.result.child_session_id.clone(),
                    child.request.resume_from.clone(),
                    child.spawn_reply,
                    child.handle_only,
                ),
                ChildRecord::Active(child) => (
                    child.started_at,
                    child.child_session_id,
                    child.resumed_from,
                    child.spawn_reply,
                    child.handle_only,
                ),
            };

        let finalization_pending = !self.runner.terminal_committed(&output.completion_data);
        output.result.finalization_pending = finalization_pending;

        if finalization_pending {
            let held_bytes: usize = self
                .finalizations
                .values()
                .map(|entry| entry.output.result.output.len())
                .sum();
            if held_bytes.saturating_add(output.result.output.len()) > MAX_FINALIZATION_OUTPUT_BYTES
            {
                output.result.output = Arc::from("");
                output.result.output_unavailable = true;
            }
        }

        let persisted_output_ref = self.runner.persisted_output_ref(&output.completion_data);
        let mut completed = CompletedChild {
            request: request.clone(),
            immediate_parent_session_id,
            started_at,
            child_session_id,
            resumed_from,
            persisted_output_ref,
            finalization_error: self.runner.finalization_error(&output.completion_data).or_else(|| {
                output.result.output_unavailable.then(|| "original output is unavailable or exceeded the bounded recovery retention budget".into())
            }),
            result: output.result.clone(),
        };
        let snapshot = completed_snapshot(&completed, None, false);

        let mut waiter_delivered = false;
        for waiter in self.waiters.remove(id).unwrap_or_default() {
            waiter_delivered |= waiter.respond_to.send(Ok(Some(snapshot.clone()))).is_ok();
        }

        let mut foreground_delivered = false;
        if let Some(respond_to) = spawn_reply.take() {
            let sent = respond_to.send(output.result.clone()).is_ok();
            if !handle_only {
                foreground_delivered = sent;
                handle_only = !sent;
            }
        } else if !handle_only {
            handle_only = true;
        }

        if completed.persisted_output_ref.is_some() && !finalization_pending {
            completed.result.output = Arc::from("");
        }

        let should_surface = request.surface_completion
            && handle_only
            && !output.result.cancelled
            && !waiter_delivered
            && !explicitly_killed;
        let disposition = CompletionDisposition {
            foreground_delivered,
            backgrounded: handle_only,
            waiter_delivered,
            explicitly_killed,
            should_surface,
        };
        self.completed.insert(id.to_owned(), completed);
        self.running_count_changed();
        if finalization_pending {
            // A pending response is not delivery of a terminal tool result.
            // Late canonical completion is handed to the existing inbox.
            let mut disposition = disposition;
            disposition.foreground_delivered = false;
            disposition.waiter_delivered = false;
            disposition.should_surface =
                request.surface_completion && !explicitly_killed && !output.result.cancelled;
            self.finalizations.insert(
                id.to_owned(),
                PendingFinalization {
                    output,
                    disposition,
                    attempts: 0,
                    cycle_deadline: tokio::time::Instant::now()
                        + std::time::Duration::from_secs(30),
                    next_cycle_deadline: None,
                    in_flight: false,
                },
            );
            return;
        }
        self.completed_order.push_back(id.to_owned());
        let workflow_run_id = request.owner.workflow_run_id().map(str::to_owned);
        self.runner.on_completed(ChildCompletion {
            request,
            result: output.result,
            completion_data: output.completion_data,
            disposition,
        });
        self.resolve_goal_cancel_waiters(id);
        self.resolve_session_cancel_waiters(id);
        if let Some(run_id) = workflow_run_id {
            self.resolve_workflow_cancel_waiters(&run_id);
        }
    }

    fn advance_finalizations(&mut self) {
        for (id, pending) in &mut self.finalizations {
            if pending.in_flight
                || pending.attempts >= 3
                || tokio::time::Instant::now() >= pending.cycle_deadline
            {
                continue;
            }
            let Some(child) = self.completed.get(id) else {
                continue;
            };
            pending.attempts += 1;
            match self.runner.reconcile(
                child.request.clone(),
                pending.output.clone(),
                pending.cycle_deadline,
            ) {
                Ok(future) => {
                    pending.in_flight = true;
                    self.reconciliations.push(TaggedFuture {
                        subagent_id: id.clone(),
                        future: Box::pin(std::panic::AssertUnwindSafe(future).catch_unwind()),
                    });
                }
                Err(output) => {
                    pending.output = output;
                    pending.attempts = 3;
                }
            }
        }
    }

    fn finish_reconciliation(
        &mut self,
        id: &str,
        output: std::thread::Result<ChildRunOutput<R::CompletionData>>,
    ) {
        let Some(pending) = self.finalizations.get_mut(id) else {
            return;
        };
        pending.in_flight = false;
        let next_cycle_deadline = pending.next_cycle_deadline.take();
        let Ok(mut output) = output else {
            if let Some(deadline) = next_cycle_deadline {
                pending.attempts = 0;
                pending.cycle_deadline = deadline;
            } else {
                pending.attempts = 3;
            }
            return;
        };
        if !self.runner.terminal_committed(&output.completion_data) {
            if let Some(child) = self.completed.get_mut(id) {
                child.finalization_error = self.runner.finalization_error(&output.completion_data);
            }
            pending.output = output;
            if let Some(deadline) = next_cycle_deadline {
                pending.attempts = 0;
                pending.cycle_deadline = deadline;
            }
            return;
        }
        output.result.finalization_pending = false;
        let Some(pending) = self.finalizations.remove(id) else {
            return;
        };
        let Some(child) = self.completed.get_mut(id) else {
            return;
        };
        child.result = output.result.clone();
        child.finalization_error = None;
        child.persisted_output_ref = self.runner.persisted_output_ref(&output.completion_data);
        if child.persisted_output_ref.is_some() {
            child.result.output = Arc::from("");
        }
        let request = child.request.clone();
        let workflow_run_id = request.owner.workflow_run_id().map(str::to_owned);
        self.completed_order.push_back(id.to_owned());
        self.runner.on_completed(ChildCompletion {
            request,
            result: output.result,
            completion_data: output.completion_data,
            disposition: pending.disposition,
        });
        self.resolve_goal_cancel_waiters(id);
        self.resolve_session_cancel_waiters(id);
        if let Some(run_id) = workflow_run_id {
            self.resolve_workflow_cancel_waiters(&run_id);
        }
    }

    fn finish_panicked_child(&mut self, id: &str) {
        // Polling the runner can enqueue a phase fact and panic in the same
        // poll, after select already polled the internal receiver. Apply those
        // accepted facts before deriving the panic outcome or dropping control.
        while let Ok(event) = self.internal_rx.try_recv() {
            self.handle_internal(event);
        }
        let execution_outcome = self
            .active
            .get(id)
            .and_then(|child| child.execution_outcome.clone());
        let child_session_id = self
            .active
            .get(id)
            .map(|child| child.child_session_id.clone())
            .or_else(|| self.pending.get(id).map(|child| child.request.id.clone()));
        let request = self
            .active
            .get(id)
            .map(|child| child.request.clone())
            .or_else(|| self.pending.get(id).map(|child| child.request.clone()));
        let Some(request) = request else {
            return;
        };
        let control = self.active.get(id).map(|child| &child.control);
        let completion_data =
            self.runner
                .completion_after_panic(&request, control, execution_outcome.as_deref());
        if let Some(control) = control {
            control.runner_panicked();
        }
        tracing::error!(subagent_id = id, "subagent child runner panicked");
        self.finish_child(
            id,
            ChildRunOutput {
                result: SubagentResult {
                    success: execution_outcome.as_deref() == Some("completed"),
                    cancelled: execution_outcome.as_deref() == Some("cancelled"),
                    output_unavailable: execution_outcome.is_some(),
                    error: Some("Subagent runtime panicked".to_owned()),
                    subagent_id: request.id.clone(),
                    child_session_id: child_session_id.unwrap_or(request.id),
                    ..Default::default()
                },
                completion_data,
            },
        );
    }

    fn cancel_one(
        &mut self,
        id: &str,
        parent_session_id: Option<&str>,
        explicit: bool,
    ) -> SubagentCancelOutcome {
        if let Some(child) = self.active.get_mut(id)
            && !child.request.owner.is_workflow()
            && belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id,
            )
        {
            child.explicitly_killed |= explicit;
            child.cancellation.cancel();
            child.control.cancel();
            return SubagentCancelOutcome::Cancelled;
        }
        if let Some(child) = self.pending.get_mut(id)
            && !child.request.owner.is_workflow()
            && belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id,
            )
        {
            child.explicitly_killed |= explicit;
            child.cancellation.cancel();
            return SubagentCancelOutcome::Cancelled;
        }
        if let Some(child) = self.completed.get(id)
            && !child.request.owner.is_workflow()
            && belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                parent_session_id,
            )
        {
            return SubagentCancelOutcome::AlreadyFinished {
                status: child.result.status().to_owned(),
            };
        }
        SubagentCancelOutcome::NotFound
    }

    fn cancel_parent_prompt(&mut self, parent_prompt_id: &str, parent_session_id: Option<&str>) {
        if let Some(parent_session_id) = parent_session_id {
            self.cancelled_prompt_scopes.insert(PromptScope::new(
                parent_session_id.to_owned(),
                parent_prompt_id.to_owned(),
            ));
        }
        for child in self.active.values() {
            if child.request.parent_prompt_id.as_deref() == Some(parent_prompt_id)
                && belongs_to_session(
                    &child.request,
                    &child.immediate_parent_session_id,
                    parent_session_id,
                )
            {
                child.cancellation.cancel();
                child.control.cancel();
            }
        }
        for child in self.pending.values() {
            if child.request.parent_prompt_id.as_deref() == Some(parent_prompt_id)
                && belongs_to_session(
                    &child.request,
                    &child.immediate_parent_session_id,
                    parent_session_id,
                )
            {
                child.cancellation.cancel();
            }
        }
    }

    fn prompt_scope_cancelled(
        &self,
        request: &SubagentRequest,
        immediate_parent_session_id: &str,
    ) -> bool {
        let Some(parent_prompt_id) = request.parent_prompt_id.as_deref() else {
            return false;
        };
        self.cancelled_prompt_scopes.contains(&PromptScope::new(
            request.parent_session_id.clone(),
            parent_prompt_id.to_owned(),
        )) || self.cancelled_prompt_scopes.contains(&PromptScope::new(
            immediate_parent_session_id.to_owned(),
            parent_prompt_id.to_owned(),
        ))
    }

    fn teardown_session_children(&mut self, parent_session_id: &str) {
        let mut cancelled = 0;
        for child in self.active.values_mut() {
            if child.request.parent_session_id == parent_session_id {
                // Parent is gone: do not rebuffer this completion for a later
                // resume of the same session id.
                child.request.surface_completion = false;
                child.cancellation.cancel();
                child.control.cancel();
                cancelled += 1;
            }
        }
        for child in self.pending.values_mut() {
            if child.request.parent_session_id == parent_session_id {
                child.request.surface_completion = false;
                child.cancellation.cancel();
                cancelled += 1;
            }
        }
        if cancelled > 0 {
            tracing::info!(
                parent_session_id,
                cancelled,
                "cancelled subagents on session teardown"
            );
        }
    }

    /// All non-workflow children for the parent session (user Stop / Esc).
    ///
    /// Requires a concrete session id — unbound (`None`) is rejected so a
    /// wildcard cannot cancel every session on a shared coordinator.
    fn cancel_parent_session(&mut self, parent_session_id: Option<&str>) -> SubagentCancelOutcome {
        let Some(parent_session_id) = parent_session_id else {
            return SubagentCancelOutcome::NotFound;
        };
        self.spawn_blocked_sessions
            .insert(parent_session_id.to_owned());
        for child in self.active.values() {
            if belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                Some(parent_session_id),
            ) && !child.request.owner.is_workflow()
            {
                child.cancellation.cancel();
                child.control.cancel();
            }
        }
        for child in self.pending.values() {
            if belongs_to_session(
                &child.request,
                &child.immediate_parent_session_id,
                Some(parent_session_id),
            ) && !child.request.owner.is_workflow()
            {
                child.cancellation.cancel();
            }
        }
        SubagentCancelOutcome::Cancelled
    }

    fn cancel_workflow_children(&mut self, run_id: &str, parent_session_id: Option<&str>) {
        for child in self.active.values() {
            if child.request.owner.workflow_run_id() == Some(run_id)
                && belongs_to_session(
                    &child.request,
                    &child.immediate_parent_session_id,
                    parent_session_id,
                )
            {
                child.cancellation.cancel();
                child.control.cancel();
            }
        }
        for child in self.pending.values() {
            if child.request.owner.workflow_run_id() == Some(run_id)
                && belongs_to_session(
                    &child.request,
                    &child.immediate_parent_session_id,
                    parent_session_id,
                )
            {
                child.cancellation.cancel();
            }
        }
    }

    fn cancel_goal_children(
        &mut self,
        goal_id: &str,
        definition_revision: u64,
        parent_session_id: Option<&str>,
    ) {
        for child in self.active.values() {
            if child.request.owner.goal_id() == Some(goal_id)
                && child.request.owner.goal_definition_revision() == Some(definition_revision)
                && belongs_to_session(
                    &child.request,
                    &child.immediate_parent_session_id,
                    parent_session_id,
                )
            {
                child.cancellation.cancel();
                child.control.cancel();
            }
        }
        for child in self.pending.values() {
            if child.request.owner.goal_id() == Some(goal_id)
                && child.request.owner.goal_definition_revision() == Some(definition_revision)
                && belongs_to_session(
                    &child.request,
                    &child.immediate_parent_session_id,
                    parent_session_id,
                )
            {
                child.cancellation.cancel();
            }
        }
    }

    fn resolve_goal_cancel_waiters(&mut self, finished_id: &str) {
        let mut pending = Vec::with_capacity(self.goal_cancel_waiters.len());
        for mut waiter in std::mem::take(&mut self.goal_cancel_waiters) {
            waiter.remaining_ids.remove(finished_id);
            if waiter.remaining_ids.is_empty() {
                let _ = waiter.respond_to.send(SubagentCancelOutcome::Cancelled);
            } else {
                pending.push(waiter);
            }
        }
        self.goal_cancel_waiters = pending;
    }

    fn resolve_session_cancel_waiters(&mut self, finished_id: &str) {
        let mut pending = Vec::with_capacity(self.session_cancel_waiters.len());
        for mut waiter in std::mem::take(&mut self.session_cancel_waiters) {
            waiter.remaining_ids.remove(finished_id);
            if waiter.remaining_ids.is_empty() {
                let _ = waiter.respond_to.send(SubagentCancelOutcome::Cancelled);
            } else {
                pending.push(waiter);
            }
        }
        self.session_cancel_waiters = pending;
    }

    fn session_child_ids(&self, parent_session_id: Option<&str>) -> HashSet<String> {
        self.pending
            .values()
            .filter(|child| {
                !child.request.owner.is_workflow()
                    && belongs_to_session(
                        &child.request,
                        &child.immediate_parent_session_id,
                        parent_session_id,
                    )
            })
            .map(|child| child.request.id.clone())
            .chain(
                self.active
                    .values()
                    .filter(|child| {
                        !child.request.owner.is_workflow()
                            && belongs_to_session(
                                &child.request,
                                &child.immediate_parent_session_id,
                                parent_session_id,
                            )
                    })
                    .map(|child| child.request.id.clone()),
            )
            .collect()
    }

    fn goal_child_ids(
        &self,
        goal_id: &str,
        definition_revision: u64,
        parent_session_id: Option<&str>,
    ) -> HashSet<String> {
        self.pending
            .values()
            .filter(|child| {
                child.request.owner.goal_id() == Some(goal_id)
                    && child.request.owner.goal_definition_revision() == Some(definition_revision)
                    && belongs_to_session(
                        &child.request,
                        &child.immediate_parent_session_id,
                        parent_session_id,
                    )
            })
            .map(|child| child.request.id.clone())
            .chain(
                self.active
                    .values()
                    .filter(|child| {
                        child.request.owner.goal_id() == Some(goal_id)
                            && child.request.owner.goal_definition_revision()
                                == Some(definition_revision)
                            && belongs_to_session(
                                &child.request,
                                &child.immediate_parent_session_id,
                                parent_session_id,
                            )
                    })
                    .map(|child| child.request.id.clone()),
            )
            .collect()
    }

    fn resolve_workflow_cancel_waiters(&mut self, run_id: &str) {
        if workflow_outstanding(&self.pending, &self.active, run_id) != 0 {
            return;
        }
        for respond_to in self
            .workflow_cancel_waiters
            .remove(run_id)
            .unwrap_or_default()
        {
            let _ = respond_to.send(SubagentCancelOutcome::Cancelled);
        }
    }

    fn next_deadline(&self) -> Option<tokio::time::Instant> {
        self.pending
            .values()
            .filter_map(|child| child.foreground_deadline)
            .chain(
                self.active
                    .values()
                    .filter_map(|child| child.foreground_deadline),
            )
            .chain(
                self.waiters
                    .values()
                    .flatten()
                    .map(|waiter| waiter.deadline),
            )
            .min()
    }

    fn reap_abandoned_callers(&mut self) {
        for child in self.pending.values_mut() {
            background_if_caller_gone(child);
        }
        for child in self.active.values_mut() {
            background_if_caller_gone(child);
        }
    }

    fn process_deadlines(&mut self) {
        self.reap_abandoned_callers();
        let now = tokio::time::Instant::now();
        for child in self.pending.values_mut() {
            background_at_deadline(child, now, self.config.foreground_budget);
        }
        for child in self.active.values_mut() {
            background_at_deadline(child, now, self.config.foreground_budget);
        }

        let ids: Vec<_> = self.waiters.keys().cloned().collect();
        for id in ids {
            let waiters = self.waiters.remove(&id).unwrap_or_default();
            let (due, live): (Vec<_>, Vec<_>) = waiters
                .into_iter()
                .partition(|waiter| waiter.deadline <= now);
            if !live.is_empty() {
                self.waiters.insert(id.clone(), live);
            }
            for waiter in due {
                if waiter.respond_to.is_closed() {
                    continue;
                }
                if let Some(child) = self.active.get(&id) {
                    let result = if let Some(outcome) = &child.execution_outcome {
                        Ok(Some(query::active_finalization_snapshot(child, outcome)))
                    } else if child.request.owner.is_workflow() {
                        Err(super::types::SubagentObservationError::Unauthorized)
                    } else {
                        child
                            .control
                            .progress_snapshot()
                            .map(|progress| {
                                Some(
                                    super::coordinator_state::running_inspection(
                                        super::coordinator_state::running_seed(child),
                                        progress,
                                    )
                                    .snapshot,
                                )
                            })
                            .ok_or(super::types::SubagentObservationError::Unavailable)
                    };
                    let _ = waiter.respond_to.send(result);
                } else if self.pending.contains_key(&id) {
                    let _ = waiter.respond_to.send(
                        self.pending
                            .get(&id)
                            .map(|child| {
                                Ok(Some(super::coordinator_state::pending_snapshot(child)))
                            })
                            .unwrap_or(Ok(None)),
                    );
                } else {
                    let _ = waiter.respond_to.send(Ok(self.ready_snapshot(&id)));
                }
            }
        }
    }

    fn running_count_changed(&self) {
        self.runner.running_count_changed(
            self.pending.len()
                + self
                    .active
                    .values()
                    .filter(|child| child.execution_outcome.is_none())
                    .count(),
        );
    }

    fn cancel_all_children(&self) {
        for child in self.active.values() {
            child.cancellation.cancel();
            child.control.cancel();
        }
        for child in self.pending.values() {
            child.cancellation.cancel();
        }
    }
}

fn belongs_to_session(
    request: &SubagentRequest,
    immediate_parent_session_id: &str,
    parent_session_id: Option<&str>,
) -> bool {
    // Nested requests retain the lifecycle root in the request for teardown
    // and completion routing, while session-scoped APIs arrive from the
    // immediate parent session. Both are authorized owners of the child.
    parent_session_id
        .is_none_or(|id| request.parent_session_id == id || immediate_parent_session_id == id)
}

fn task_name(request: &SubagentRequest) -> String {
    let description = request.description.trim();
    if description.is_empty() {
        request.id.clone()
    } else {
        description.to_owned()
    }
}

impl<R: ChildRunner> Drop for SubagentCoordinator<R> {
    fn drop(&mut self) {
        self.cancel_all_children();
    }
}

#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod tests;

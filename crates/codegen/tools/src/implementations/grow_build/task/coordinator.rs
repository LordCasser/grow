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
    ChildCheckpoint, ChildCompletion, ChildControl, ChildReporter, ChildRunOutcome, ChildRunOutput,
    ChildRunRequest, ChildRunner, CompletionDisposition, CoordinatorConfig, LocalBoxFuture,
    MAX_COMPLETED_ENTRIES, SendBoxFuture, StartedChild, SubagentProgress,
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
    frozen_roots: HashMap<String, String>,
    suspended: HashMap<String, SuspendedChild<R::CompletionData>>,
    restored_trees: HashMap<String, RestoredTreeState>,
    completed: HashMap<String, CompletedChild>,
    retiring: HashMap<String, RetiringChild<R::CompletionData>>,
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
    output_reads: FuturesUnordered<OutputReadFuture>,
    output_read_requests: HashMap<String, OutputReadRequest>,
    output_read_permits: Arc<tokio::sync::Semaphore>,
}

type OutputReadFuture =
    TaggedFuture<SendBoxFuture<Result<Option<Arc<str>>, super::types::SubagentObservationError>>>;

struct SuspendedChild<D> {
    request: SubagentRequest,
    immediate_parent_session_id: String,
    checkpoint: ChildCheckpoint,
    started_at_epoch_ms: u64,
    duration_ms: u64,
    spawn_reply: Option<oneshot::Sender<SubagentResult>>,
    handle_only: bool,
    waiters: Vec<BlockingWaiter>,
    _completion_data: D,
}

struct RetiringChild<D> {
    request: SubagentRequest,
    completion_data: D,
}

struct RestoredTreeState {
    operation_id: String,
    pending: HashSet<String>,
    blocked: HashSet<String>,
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
            frozen_roots: HashMap::new(),
            suspended: HashMap::new(),
            restored_trees: HashMap::new(),
            completed: HashMap::new(),
            retiring: HashMap::new(),
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
                && self.retiring.is_empty()
            {
                break;
            }

            let deadline = self.next_deadline();
            tokio::select! {
                biased;
                Some(event) = self.internal_rx.recv() => self.handle_internal(event),
                Some((id, output)) = self.runs.next(), if !self.runs.is_empty() => {
                    match output {
                        Ok(ChildRunOutcome::Finished(output)) => self.finish_child(&id, output),
                        Ok(ChildRunOutcome::Suspended { checkpoint, completion_data }) => {
                            if let Some(expected) = self
                                .pending_restore_checkpoint(&id)
                                .or_else(|| self.blocked_restore_checkpoint(&id))
                            {
                                self.block_restored_child(
                                    &id,
                                    expected,
                                    "adopted child suspended before confirming startup".into(),
                                    completion_data,
                                );
                            } else {
                                self.suspend_child(&id, checkpoint, completion_data);
                            }
                        }
                        Ok(ChildRunOutcome::RestoreBlocked { checkpoint, error, completion_data }) => {
                            self.block_restored_child(&id, checkpoint, error, completion_data);
                        }
                        Err(_) => self.finish_panicked_child(&id),
                    }
                }
                Some((id, output)) = self.reconciliations.next(), if !self.reconciliations.is_empty() => {
                    self.finish_reconciliation(&id, output);
                }
                _ = recovery_tick.tick(), if !self.finalizations.is_empty() || !self.retiring.is_empty() => {
                    self.advance_finalizations();
                    self.sweep_retiring();
                },
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
            SubagentEvent::FreezeAdmission {
                root_session_id,
                operation_id,
                respond_to,
            } => {
                if root_session_id.trim().is_empty()
                    || operation_id.trim().is_empty()
                    || self
                        .frozen_roots
                        .get(&root_session_id)
                        .is_some_and(|current| current != &operation_id)
                {
                    let _ = respond_to.send(Err("conflicting suspension owner".into()));
                    return;
                }
                self.frozen_roots
                    .insert(root_session_id.clone(), operation_id);
                let mut nodes = Vec::new();
                let mut failure = None;
                for child in self
                    .active
                    .values()
                    .filter(|child| child.request.parent_session_id == root_session_id)
                {
                    if let Err(error) = child.control.freeze_admission() {
                        failure = Some(error);
                    }
                    nodes.push(super::types::FrozenChildRef {
                        task_id: child.request.id.clone(),
                        session_id: child.child_session_id.clone(),
                        security_parent_session_id: child.immediate_parent_session_id.clone(),
                        origin_tool_call_id: child.request.origin_tool_call_id.clone(),
                        initializing: false,
                    });
                }
                for child in self
                    .pending
                    .values()
                    .filter(|child| child.request.parent_session_id == root_session_id)
                {
                    nodes.push(super::types::FrozenChildRef {
                        task_id: child.request.id.clone(),
                        session_id: child.request.id.clone(),
                        security_parent_session_id: child.immediate_parent_session_id.clone(),
                        origin_tool_call_id: child.request.origin_tool_call_id.clone(),
                        initializing: true,
                    });
                }
                for child in self
                    .suspended
                    .values()
                    .filter(|child| child.request.parent_session_id == root_session_id)
                {
                    nodes.push(super::types::FrozenChildRef {
                        task_id: child.request.id.clone(),
                        session_id: child.checkpoint.session_id.clone(),
                        security_parent_session_id: child.immediate_parent_session_id.clone(),
                        origin_tool_call_id: child.request.origin_tool_call_id.clone(),
                        initializing: false,
                    });
                }
                let _ = respond_to.send(failure.map_or(Ok(nodes), Err));
            }
            SubagentEvent::AdoptSuspended {
                request,
                security_parent_session_id,
                checkpoint,
                respond_to,
            } => self.adopt_suspended(
                *request,
                &security_parent_session_id,
                checkpoint,
                respond_to,
            ),
            SubagentEvent::ReleaseRestoredTree {
                root_session_id,
                operation_id,
                respond_to,
            } => {
                let result = self.release_restored_tree(&root_session_id, &operation_id);
                let _ = respond_to.send(result);
            }
            SubagentEvent::CheckSuspendedRetirement {
                root_session_id,
                operation_id,
                task_ids,
                respond_to,
            } => {
                let result =
                    self.check_suspended_retirement(&root_session_id, &operation_id, &task_ids);
                let _ = respond_to.send(result);
            }
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
                            not_dispatched: true,
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
                    if spawner_owner.is_workflow() {
                        request.surface_completion = false;
                    }
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
                        not_dispatched: true,
                        cancelled: true,
                        error: Some("parent prompt is cancelled".to_owned()),
                        subagent_id: id.clone(),
                        child_session_id: id,
                        ..Default::default()
                    });
                    return;
                }
                if self.frozen_roots.contains_key(&request.parent_session_id) {
                    let id = request.id.clone();
                    let _ = command.result_tx.send(SubagentResult {
                        not_dispatched: true,
                        error: Some("session execution admission is suspended; this call was not dispatched".into()),
                        subagent_id: id.clone(), child_session_id: id, ..Default::default()
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
                        not_dispatched: true,
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
                    || self.suspended.contains_key(&id)
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
                        not_dispatched: true,
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
                        restore: None,
                        adoption_reply: None,
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
                            restore: None,
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

    fn check_suspended_retirement(
        &mut self,
        root_session_id: &str,
        operation_id: &str,
        task_ids: &[String],
    ) -> Result<bool, String> {
        if root_session_id.trim().is_empty()
            || operation_id.trim().is_empty()
            || self.frozen_roots.get(root_session_id).map(String::as_str) != Some(operation_id)
        {
            return Err("suspension owner is unknown or conflicting".into());
        }

        let mut seen = HashSet::with_capacity(task_ids.len());
        for id in task_ids {
            if id.trim().is_empty() || !seen.insert(id.as_str()) {
                return Err("suspension task identity is empty or duplicated".into());
            }

            if let Some(child) = self.pending.get(id) {
                if child.request.parent_session_id != root_session_id {
                    return Err("suspension task belongs to another root".into());
                }
                return Ok(false);
            }
            if let Some(child) = self.active.get(id) {
                if child.request.parent_session_id != root_session_id {
                    return Err("suspension task belongs to another root".into());
                }
                return Ok(false);
            }
            if let Some(child) = self.suspended.get_mut(id) {
                if child.request.parent_session_id != root_session_id
                    || child.checkpoint.operation_id != operation_id
                {
                    return Err("suspended task belongs to another root or operation".into());
                }
                if !self
                    .runner
                    .suspension_retired(&mut child._completion_data)?
                {
                    return Ok(false);
                }
                continue;
            }
            if let Some(child) = self.completed.get(id) {
                if child.request.parent_session_id != root_session_id {
                    return Err("completed task belongs to another root".into());
                }
                continue;
            }
            return Err(format!("suspension task {id:?} is unknown"));
        }

        let retiring_ids: Vec<String> = self
            .retiring
            .iter()
            .filter(|(_, child)| child.request.parent_session_id == root_session_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in retiring_ids {
            let child = self
                .retiring
                .get_mut(&id)
                .expect("retiring child identity was collected from this map");
            if child.request.parent_session_id != root_session_id {
                return Err("retiring task belongs to another root".into());
            }
            if !self.runner.suspension_retired(&mut child.completion_data)? {
                return Ok(false);
            }
            self.retiring.remove(&id);
        }
        Ok(true)
    }

    fn adopt_suspended(
        &mut self,
        request: SubagentRequest,
        security_parent_session_id: &str,
        checkpoint: ChildCheckpoint,
        respond_to: oneshot::Sender<Result<(), String>>,
    ) {
        macro_rules! reject {
            ($error:expr) => {{
                let _ = respond_to.send(Err($error));
                return;
            }};
        }

        let id = request.id.clone();
        let root_session_id = request.parent_session_id.clone();
        if id.trim().is_empty()
            || root_session_id.trim().is_empty()
            || security_parent_session_id.trim().is_empty()
            || checkpoint.operation_id.trim().is_empty()
            || checkpoint.session_id != id
            || request.cancel_token.is_cancelled()
        {
            reject!("adoption identity or request is invalid".to_owned());
        }

        if self.pending.contains_key(&id)
            || self.active.contains_key(&id)
            || self.completed.contains_key(&id)
            || self.finalizations.contains_key(&id)
            || self.retiring.contains_key(&id)
        {
            reject!(format!("subagent id {id:?} already has an owner"));
        }

        let security_parent = if security_parent_session_id == root_session_id {
            None
        } else {
            match self.active.values().find(|child| {
                child.child_session_id == security_parent_session_id
                    && child.request.parent_session_id == root_session_id
            }) {
                Some(child) => Some(child.control.security_context()),
                None => {
                    reject!("direct security parent is not an active child of this root".into())
                }
            }
        };

        if let Some(suspended) = self.suspended.get_mut(&id) {
            if suspended.request.parent_session_id != root_session_id
                || suspended.immediate_parent_session_id != security_parent_session_id
                || suspended.checkpoint != checkpoint
                || !adoption_request_matches(&suspended.request, &request)
            {
                reject!("existing suspended identity does not match adoption request".into());
            }
            if self
                .runner
                .has_physical_retirement_owner(&suspended._completion_data)
            {
                match self
                    .runner
                    .suspension_retired(&mut suspended._completion_data)
                {
                    Ok(true) => {}
                    Ok(false) => {
                        reject!("existing suspended child thread is not retired".into())
                    }
                    Err(error) => reject!(format!(
                        "cannot confirm suspended child retirement: {error}"
                    )),
                }
            }
        }

        if let Some(current) = self.frozen_roots.get(&root_session_id) {
            if current != &checkpoint.operation_id {
                reject!("root is frozen for another suspension operation".into());
            }
        } else {
            self.frozen_roots
                .insert(root_session_id.clone(), checkpoint.operation_id.clone());
            let mut freeze_error = None;
            for child in self
                .active
                .values()
                .filter(|child| child.request.parent_session_id == root_session_id)
            {
                if let Err(error) = child.control.freeze_admission() {
                    freeze_error = Some(error);
                }
            }
            if let Some(error) = freeze_error {
                reject!(format!(
                    "could not freeze existing child admission: {error}"
                ));
            }
        }

        {
            let tree = self
                .restored_trees
                .entry(root_session_id.clone())
                .or_insert_with(|| RestoredTreeState {
                    operation_id: checkpoint.operation_id.clone(),
                    pending: HashSet::new(),
                    blocked: HashSet::new(),
                });
            if tree.operation_id != checkpoint.operation_id || tree.pending.contains(&id) {
                reject!("restore tree has a conflicting or duplicate adoption".into());
            }
            tree.blocked.remove(&id);
            tree.pending.insert(id.clone());
        }

        let cancellation = request.cancel_token.clone();
        let runner_request = request.clone();
        self.pending.insert(
            id.clone(),
            PendingChild {
                request,
                immediate_parent_session_id: security_parent_session_id.to_owned(),
                started_at: std::time::Instant::now(),
                cancellation: cancellation.clone(),
                spawn_reply: None,
                foreground_deadline: None,
                handle_only: true,
                explicitly_killed: false,
                restore: Some(checkpoint.clone()),
                adoption_reply: Some(respond_to),
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
                    request: runner_request,
                    security_parent_session_id: security_parent_session_id.to_owned(),
                    security_parent,
                    cancellation,
                    restore: Some(checkpoint),
                    reporter,
                }))
                .catch_unwind(),
            ),
        });
    }

    fn release_restored_tree(
        &mut self,
        root_session_id: &str,
        operation_id: &str,
    ) -> Result<(), String> {
        if root_session_id.trim().is_empty()
            || operation_id.trim().is_empty()
            || self.frozen_roots.get(root_session_id).map(String::as_str) != Some(operation_id)
        {
            return Err("restore tree owner is unknown or conflicting".into());
        }
        if let Some(tree) = self.restored_trees.get(root_session_id) {
            if tree.operation_id != operation_id {
                return Err("restore tree operation is conflicting".into());
            }
            if !tree.pending.is_empty() || !tree.blocked.is_empty() {
                return Err("restore tree still has pending or blocked adoptions".into());
            }
        }
        if self.pending.values().any(|child| {
            child.request.parent_session_id == root_session_id
                && child
                    .restore
                    .as_ref()
                    .is_some_and(|checkpoint| checkpoint.operation_id == operation_id)
        }) || self.suspended.values().any(|child| {
            child.request.parent_session_id == root_session_id
                && child.checkpoint.operation_id == operation_id
        }) {
            return Err("restore tree still has pending or suspended children".into());
        }
        self.frozen_roots.remove(root_session_id);
        self.restored_trees.remove(root_session_id);
        Ok(())
    }

    fn restore_checkpoint(&self, id: &str) -> Option<ChildCheckpoint> {
        self.pending
            .get(id)
            .and_then(|child| child.restore.clone())
            .or_else(|| self.active.get(id).and_then(|child| child.restore.clone()))
    }

    fn pending_restore_checkpoint(&self, id: &str) -> Option<ChildCheckpoint> {
        self.pending.get(id).and_then(|child| child.restore.clone())
    }

    fn blocked_restore_checkpoint(&self, id: &str) -> Option<ChildCheckpoint> {
        let child = self.active.get(id)?;
        let checkpoint = child.restore.as_ref()?;
        self.restored_trees
            .get(&child.request.parent_session_id)
            .filter(|tree| {
                tree.operation_id == checkpoint.operation_id && tree.blocked.contains(id)
            })
            .map(|_| checkpoint.clone())
    }

    fn block_restored_child(
        &mut self,
        id: &str,
        reported_checkpoint: ChildCheckpoint,
        mut error: String,
        mut completion_data: R::CompletionData,
    ) {
        let Some(checkpoint) = self.restore_checkpoint(id) else {
            self.finish_child(
                id,
                ChildRunOutput {
                    result: SubagentResult {
                        success: false,
                        error: Some(format!(
                            "runner reported a blocked restore without a pending adoption: {error}"
                        )),
                        subagent_id: id.to_owned(),
                        child_session_id: id.to_owned(),
                        ..Default::default()
                    },
                    completion_data,
                },
            );
            return;
        };
        if reported_checkpoint != checkpoint {
            error = format!("runner checkpoint did not match the adopted checkpoint: {error}");
        }

        let control = self.active.get(id).map(|child| &child.control);
        self.runner
            .retain_execution_owner(&mut completion_data, control);
        let (request, immediate_parent_session_id, timing, spawn_reply, handle_only) =
            if let Some(child) = self.active.remove(id) {
                let timing = super::coordinator_state::running_seed(&child);
                (
                    child.request,
                    child.immediate_parent_session_id,
                    (timing.started_at_epoch_ms, timing.duration_ms),
                    child.spawn_reply,
                    child.handle_only,
                )
            } else if let Some(child) = self.pending.remove(id) {
                let timing = super::coordinator_state::pending_snapshot(&child);
                if let Some(reply) = child.adoption_reply {
                    let _ = reply.send(Err(error.clone()));
                }
                (
                    child.request,
                    child.immediate_parent_session_id,
                    (timing.started_at_epoch_ms, timing.duration_ms),
                    child.spawn_reply,
                    child.handle_only,
                )
            } else {
                return;
            };
        let mut waiters = self.waiters.remove(id).unwrap_or_default();
        if let Some(mut suspended) = self.suspended.remove(id) {
            waiters.extend(suspended.waiters);
            suspended.waiters = waiters;
            if suspended.spawn_reply.is_none() {
                suspended.spawn_reply = spawn_reply;
            }
            suspended.handle_only = handle_only;
            if self.runner.has_physical_retirement_owner(&completion_data) {
                suspended._completion_data = completion_data;
            }
            self.suspended.insert(id.to_owned(), suspended);
        } else {
            self.suspended.insert(
                id.to_owned(),
                SuspendedChild {
                    request,
                    immediate_parent_session_id,
                    checkpoint: checkpoint.clone(),
                    started_at_epoch_ms: timing.0,
                    duration_ms: timing.1,
                    spawn_reply,
                    handle_only,
                    waiters,
                    _completion_data: completion_data,
                },
            );
        }
        let root = self
            .suspended
            .get(id)
            .map(|child| child.request.parent_session_id.clone())
            .unwrap_or_default();
        let tree = self
            .restored_trees
            .entry(root)
            .or_insert_with(|| RestoredTreeState {
                operation_id: checkpoint.operation_id.clone(),
                pending: HashSet::new(),
                blocked: HashSet::new(),
            });
        tree.pending.remove(id);
        tree.blocked.insert(id.to_owned());
        self.running_count_changed();
        tracing::warn!(
            subagent_id = id,
            error,
            "suspended child restore remains blocked"
        );
    }

    fn sweep_retiring(&mut self) {
        let ids: Vec<String> = self.retiring.keys().cloned().collect();
        for id in ids {
            let retired = self.retiring.get_mut(&id).and_then(|child| {
                self.runner
                    .suspension_retired(&mut child.completion_data)
                    .ok()
            });
            if retired == Some(true) {
                self.retiring.remove(&id);
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
            (AgentInteraction::Replan { .. }, Some(id)) => {
                self.send_to_child_route(&request.source_session_id, id)
            }
            (AgentInteraction::ReplanHandled { reply_to, .. }, None) => {
                self.reply_route(&request.source_session_id, reply_to)
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
            AgentInteraction::Send { .. }
            | AgentInteraction::Replan { .. }
            | AgentInteraction::ReplanHandled { .. } => Ok(
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
                let admission_open = !self
                    .frozen_roots
                    .contains_key(&pending.request.parent_session_id);
                let restore = pending.restore.clone();
                let restore_requested = restore.is_some();
                let mut active = ActiveChild {
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
                    restore: restore.clone(),
                    control: child.control,
                };
                let admitted = if admission_open && restore.is_none() {
                    true
                } else {
                    active.control.freeze_admission().is_ok()
                };
                let restore_tree_matches = restore.as_ref().is_some_and(|checkpoint| {
                    self.restored_trees
                        .get(&active.request.parent_session_id)
                        .is_some_and(|tree| {
                            tree.operation_id == checkpoint.operation_id
                                && tree.pending.contains(&subagent_id)
                        })
                });
                let restore_started = admitted && (restore.is_none() || restore_tree_matches);
                if restore_started
                    && restore.is_some()
                    && let Some(suspended) = self.suspended.remove(&subagent_id)
                {
                    self.waiters
                        .entry(subagent_id.clone())
                        .or_default()
                        .extend(suspended.waiters);
                    active.spawn_reply = suspended.spawn_reply;
                    active.handle_only = suspended.handle_only;
                }
                self.active.insert(subagent_id.clone(), active);
                if let Some(checkpoint) = restore {
                    let root = self.active[&subagent_id].request.parent_session_id.clone();
                    let tree =
                        self.restored_trees
                            .entry(root)
                            .or_insert_with(|| RestoredTreeState {
                                operation_id: checkpoint.operation_id.clone(),
                                pending: HashSet::new(),
                                blocked: HashSet::new(),
                            });
                    tree.pending.remove(&subagent_id);
                    if !restore_started || tree.operation_id != checkpoint.operation_id {
                        tree.blocked.insert(subagent_id.clone());
                    }
                    if let Some(respond_to) = pending.adoption_reply {
                        let result =
                            if restore_started && tree.operation_id == checkpoint.operation_id {
                                Ok(())
                            } else {
                                Err(
                                "restored child was not installed with matching frozen admission"
                                    .into(),
                            )
                            };
                        let _ = respond_to.send(result);
                    }
                }
                // On freeze failure, retain the control handle in active state
                // and reject startup acknowledgement so the adapter tears down
                // its half-initialized runtime without opening admission.
                let _ = respond_to.send(admitted && (!restore_requested || restore_started));
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
        if let Some(checkpoint) = self
            .pending_restore_checkpoint(id)
            .or_else(|| self.blocked_restore_checkpoint(id))
        {
            let error = output
                .result
                .error
                .clone()
                .unwrap_or_else(|| "adopted child finished before confirming startup".into());
            self.block_restored_child(id, checkpoint, error, output.completion_data);
            return;
        }
        let control = self.active.get(id).map(|child| &child.control);
        self.runner
            .retain_execution_owner(&mut output.completion_data, control);
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

        if self
            .runner
            .has_physical_retirement_owner(&output.completion_data)
        {
            self.retiring.insert(
                id.to_owned(),
                RetiringChild {
                    request: request.clone(),
                    completion_data: output.completion_data.clone(),
                },
            );
        }

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
        self.sweep_retiring();
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
                    pending.output = *output;
                    pending.attempts = 3;
                }
            }
        }
    }

    fn finish_reconciliation(
        &mut self,
        id: &str,
        output: std::thread::Result<ChildRunOutcome<R::CompletionData>>,
    ) {
        let Some(pending) = self.finalizations.get_mut(id) else {
            return;
        };
        pending.in_flight = false;
        let next_cycle_deadline = pending.next_cycle_deadline.take();
        let Ok(output) = output else {
            if let Some(deadline) = next_cycle_deadline {
                pending.attempts = 0;
                pending.cycle_deadline = deadline;
            } else {
                pending.attempts = 3;
            }
            return;
        };
        let ChildRunOutcome::Finished(mut output) = output else {
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
            .chain(
                self.suspended
                    .values()
                    .flat_map(|child| child.waiters.iter().map(|waiter| waiter.deadline)),
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
        for child in self.suspended.values_mut() {
            if !child.handle_only && child.spawn_reply.as_ref().is_some_and(|tx| tx.is_closed()) {
                child.handle_only = true;
            }
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
        for child in self.suspended.values_mut() {
            let (due, live): (Vec<_>, Vec<_>) = std::mem::take(&mut child.waiters)
                .into_iter()
                .partition(|waiter| waiter.deadline <= now);
            child.waiters = live;
            for waiter in due {
                if !waiter.respond_to.is_closed() {
                    let _ = waiter
                        .respond_to
                        .send(Ok(Some(query::suspended_snapshot(child))));
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
            if self
                .frozen_roots
                .contains_key(&child.request.parent_session_id)
            {
                continue;
            }
            child.cancellation.cancel();
            child.control.cancel();
        }
        for child in self.pending.values() {
            if self
                .frozen_roots
                .contains_key(&child.request.parent_session_id)
            {
                continue;
            }
            child.cancellation.cancel();
        }
    }

    fn suspend_child(
        &mut self,
        id: &str,
        checkpoint: ChildCheckpoint,
        mut completion_data: R::CompletionData,
    ) {
        let control = self.active.get(id).map(|child| &child.control);
        self.runner
            .retain_execution_owner(&mut completion_data, control);
        let root = self
            .active
            .get(id)
            .map(|child| child.request.parent_session_id.clone())
            .or_else(|| {
                self.pending
                    .get(id)
                    .map(|child| child.request.parent_session_id.clone())
            });
        let Some(root) = root else { return };
        let frozen_operation = self.frozen_roots.get(&root).map(String::as_str);
        if self.active.get(id).is_some_and(|child| {
            suspension_matches(
                Some(root.as_str()),
                frozen_operation,
                &child.request.parent_session_id,
                &child.child_session_id,
                &checkpoint,
            )
        }) {
            let Some(child) = self.active.remove(id) else {
                return;
            };
            let timing = super::coordinator_state::running_seed(&child);
            self.suspended.insert(
                id.to_owned(),
                SuspendedChild {
                    request: child.request,
                    immediate_parent_session_id: child.immediate_parent_session_id,
                    checkpoint,
                    started_at_epoch_ms: timing.started_at_epoch_ms,
                    duration_ms: timing.duration_ms,
                    spawn_reply: child.spawn_reply,
                    handle_only: child.handle_only,
                    waiters: self.waiters.remove(id).unwrap_or_default(),
                    _completion_data: completion_data,
                },
            );
            self.running_count_changed();
            return;
        }

        // Before Started, the task id is the provisional child-session identity
        // exposed by the frozen inventory. Keep the pending owner if it differs.
        if self.pending.get(id).is_some_and(|child| {
            suspension_matches(
                Some(root.as_str()),
                frozen_operation,
                &child.request.parent_session_id,
                &child.request.id,
                &checkpoint,
            )
        }) {
            let Some(child) = self.pending.remove(id) else {
                return;
            };
            let timing = super::coordinator_state::pending_snapshot(&child);
            self.suspended.insert(
                id.to_owned(),
                SuspendedChild {
                    request: child.request,
                    immediate_parent_session_id: child.immediate_parent_session_id,
                    checkpoint,
                    started_at_epoch_ms: timing.started_at_epoch_ms,
                    duration_ms: timing.duration_ms,
                    spawn_reply: child.spawn_reply,
                    handle_only: child.handle_only,
                    waiters: self.waiters.remove(id).unwrap_or_default(),
                    _completion_data: completion_data,
                },
            );
            self.running_count_changed();
        }
    }
}

fn suspension_matches(
    frozen_root_session_id: Option<&str>,
    frozen_operation: Option<&str>,
    child_root_session_id: &str,
    expected_child_session_id: &str,
    checkpoint: &ChildCheckpoint,
) -> bool {
    frozen_root_session_id == Some(child_root_session_id)
        && frozen_operation == Some(checkpoint.operation_id.as_str())
        && expected_child_session_id == checkpoint.session_id.as_str()
}

fn adoption_request_matches(existing: &SubagentRequest, adopted: &SubagentRequest) -> bool {
    existing.id == adopted.id
        && existing.origin_tool_call_id == adopted.origin_tool_call_id
        && existing.prompt == adopted.prompt
        && existing.description == adopted.description
        && existing.subagent_type == adopted.subagent_type
        && existing.parent_session_id == adopted.parent_session_id
        && existing.parent_step_id == adopted.parent_step_id
        && existing.parent_prompt_id == adopted.parent_prompt_id
        && existing.resume_from == adopted.resume_from
        && existing.cwd == adopted.cwd
        && existing.execution_recipe() == adopted.execution_recipe()
        && existing.surface_completion == adopted.surface_completion
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

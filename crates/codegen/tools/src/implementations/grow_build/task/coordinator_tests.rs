use super::*;
use crate::implementations::grow_build::task::backend::{ChannelBackend, SubagentBackend};
use crate::implementations::grow_build::task::types::{
    SubagentCancelRequest, SubagentClearUsageNotAppliedRequest, SubagentListActiveRequest,
    SubagentLoopUnitActiveRequest, SubagentMarkUsageNotAppliedRequest, SubagentObservationError,
    SubagentOutstandingReply, SubagentOutstandingRequest, SubagentOwner, SubagentRegistryCounts,
    SubagentRequest, SubagentSnapshotStatus,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct TestControl {
    cancellation: CancellationToken,
    session_id: String,
    panic_notified: Arc<AtomicBool>,
}

impl ChildControl for TestControl {
    type SecurityContext = String;

    fn security_context(&self) -> Self::SecurityContext {
        self.session_id.clone()
    }

    fn progress_snapshot(&self) -> Option<SubagentProgress> {
        if self.session_id == "unknown-progress" {
            return None;
        }
        Some(SubagentProgress {
            turn_count: 2,
            tool_call_count: 3,
            tokens_used: 100,
            context_window_tokens: 1_000,
            context_usage_pct: 10,
            tools_used: vec!["read_file".to_owned()],
            error_count: 0,
        })
    }

    fn cancel(&self) {
        self.cancellation.cancel();
    }

    fn freeze_admission(&self) -> Result<(), String> {
        Ok(())
    }

    fn runner_panicked(&self) {
        self.panic_notified.store(true, Ordering::SeqCst);
        self.cancel();
    }
}

type PanicObservation = (String, bool, Option<String>, String);

#[tokio::test]
async fn running_query_returns_unavailable_until_a_signal_snapshot_exists() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("unknown-progress", true)).await }
    });
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("unknown-progress")
    );

    assert!(matches!(
        harness.backend.query("unknown-progress", false, None).await,
        Err(SubagentObservationError::Unavailable)
    ));

    let _ = harness.finish.send(());
    let _ = harness.completions.recv().await;
    let _ = spawn.await;
    harness.actor.abort();
}

#[tokio::test]
async fn execution_returned_query_reports_finalization_pending_without_waiting() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("execution-returned", true)).await }
    });
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("execution-returned")
    );
    loop {
        let running = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            harness.running_counts.recv(),
        )
        .await
        .expect("running gauge update")
        .expect("running gauge sender");
        if running == 0 {
            break;
        }
    }

    let snapshot = loop {
        if let Some(snapshot) = harness
            .backend
            .query("execution-returned", false, None)
            .await
            .expect("query is authorized")
            && matches!(
                snapshot.status,
                SubagentSnapshotStatus::FinalizationPending { .. }
            )
        {
            break snapshot;
        }
        tokio::task::yield_now().await;
    };
    assert!(matches!(
        snapshot.status,
        SubagentSnapshotStatus::FinalizationPending { outcome, error: None }
            if outcome == "completed"
    ));

    let _ = harness.finish.send(());
    let _ = harness.completions.recv().await;
    let _ = spawn.await;
    harness.actor.abort();
}

#[tokio::test]
async fn suspended_retirement_checks_the_original_child_and_rejects_unknown_ids() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move {
            backend
                .spawn(request("retired-suspended-child", true))
                .await
        }
    });
    assert_eq!(
        harness.requests.recv().await.unwrap().id,
        "retired-suspended-child"
    );

    let sender = harness.backend.sender();
    let (freeze_tx, freeze_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::FreezeAdmission {
            root_session_id: "parent".into(),
            operation_id: "suspend-retirement".into(),
            respond_to: freeze_tx,
        })
        .unwrap();
    let frozen = freeze_rx.await.unwrap().unwrap();
    assert_eq!(frozen.len(), 1);
    assert!(frozen[0].initializing);

    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("retired-suspended-child")
    );
    loop {
        if harness.running_counts.recv().await == Some(0) {
            break;
        }
    }

    let (retirement_tx, retirement_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::CheckSuspendedRetirement {
            root_session_id: "parent".into(),
            operation_id: "suspend-retirement".into(),
            task_ids: vec!["retired-suspended-child".into()],
            respond_to: retirement_tx,
        })
        .unwrap();
    assert_eq!(retirement_rx.await.unwrap(), Ok(true));

    let (unknown_tx, unknown_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::CheckSuspendedRetirement {
            root_session_id: "parent".into(),
            operation_id: "suspend-retirement".into(),
            task_ids: vec!["unknown-child".into()],
            respond_to: unknown_tx,
        })
        .unwrap();
    assert!(unknown_rx.await.unwrap().is_err());

    drop(spawn);
    harness.actor.abort();
}

#[tokio::test]
async fn completed_child_remains_a_retirement_blocker_until_its_owner_is_joined() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move {
            backend
                .spawn(request("retiring-completed-child", true))
                .await
        }
    });
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("retiring-completed-child")
    );
    let _ = harness.finish.send(());
    assert!(spawn.await.unwrap().unwrap().success);
    harness.completions.recv().await.unwrap();

    let sender = harness.backend.sender();
    let (freeze_tx, freeze_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::FreezeAdmission {
            root_session_id: "parent".into(),
            operation_id: "suspend-completed".into(),
            respond_to: freeze_tx,
        })
        .unwrap();
    assert!(freeze_rx.await.unwrap().unwrap().is_empty());

    let check = || {
        let sender = sender.clone();
        async move {
            let (respond_to, response) = tokio::sync::oneshot::channel();
            sender
                .send(SubagentEvent::CheckSuspendedRetirement {
                    root_session_id: "parent".into(),
                    operation_id: "suspend-completed".into(),
                    task_ids: Vec::new(),
                    respond_to,
                })
                .unwrap();
            response.await.unwrap()
        }
    };

    assert_eq!(check().await, Ok(false));
    harness.retirement_ready.store(true, Ordering::SeqCst);
    assert_eq!(check().await, Ok(true));
    assert_eq!(check().await, Ok(true));
    harness.actor.abort();
}

#[path = "coordinator_race_tests.rs"]
mod race_tests;

#[tokio::test]
async fn restored_child_adoption_ack_waits_for_started_and_frozen_control() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let sender = harness.backend.sender();
    let checkpoint = ChildCheckpoint {
        session_id: "adopted-child".into(),
        operation_id: "restore-op".into(),
        checkpoint_seq: 41,
    };
    let (ack_tx, mut ack_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::AdoptSuspended {
            request: Box::new(request("adopted-child", true)),
            security_parent_session_id: "parent".into(),
            checkpoint: checkpoint.clone(),
            respond_to: ack_tx,
        })
        .unwrap();
    assert_eq!(harness.requests.recv().await.unwrap().id, "adopted-child");
    assert!(matches!(
        ack_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));

    let (duplicate_tx, duplicate_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::AdoptSuspended {
            request: Box::new(request("adopted-child", true)),
            security_parent_session_id: "parent".into(),
            checkpoint: checkpoint.clone(),
            respond_to: duplicate_tx,
        })
        .unwrap();
    assert!(duplicate_rx.await.unwrap().is_err());

    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("adopted-child")
    );
    assert_eq!(ack_rx.await.unwrap(), Ok(()));

    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::ReleaseRestoredTree {
            root_session_id: "parent".into(),
            operation_id: "restore-op".into(),
            respond_to: release_tx,
        })
        .unwrap();
    assert_eq!(release_rx.await.unwrap(), Ok(()));

    let _ = harness.finish.send(());
    harness.completions.recv().await.unwrap();
    harness.actor.abort();
}

#[tokio::test]
async fn blocked_restore_without_retirement_owner_can_be_retried() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let sender = harness.backend.sender();
    let checkpoint = ChildCheckpoint {
        session_id: "restore-blocked".into(),
        operation_id: "restore-fail-op".into(),
        checkpoint_seq: 9,
    };
    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::AdoptSuspended {
            request: Box::new(request("restore-blocked", true)),
            security_parent_session_id: "parent".into(),
            checkpoint: checkpoint.clone(),
            respond_to: ack_tx,
        })
        .unwrap();
    assert_eq!(harness.requests.recv().await.unwrap().id, "restore-blocked");
    assert_eq!(
        ack_rx.await.unwrap(),
        Err("restore state is unavailable".into())
    );
    assert!(harness.completions.try_recv().is_err());

    let (retry_tx, retry_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::AdoptSuspended {
            request: Box::new(request("restore-blocked", true)),
            security_parent_session_id: "parent".into(),
            checkpoint: checkpoint.clone(),
            respond_to: retry_tx,
        })
        .unwrap();
    assert_eq!(
        retry_rx.await.unwrap(),
        Err("restore state is unavailable".into()),
        "a blocked restore without a physical thread owner must be retried"
    );
    assert_eq!(harness.requests.recv().await.unwrap().id, "restore-blocked");
    assert!(harness.completions.try_recv().is_err());

    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::ReleaseRestoredTree {
            root_session_id: "parent".into(),
            operation_id: "restore-fail-op".into(),
            respond_to: release_tx,
        })
        .unwrap();
    assert!(release_rx.await.unwrap().is_err());

    let (check_tx, check_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::CheckSuspendedRetirement {
            root_session_id: "parent".into(),
            operation_id: "restore-fail-op".into(),
            task_ids: vec!["restore-blocked".into()],
            respond_to: check_tx,
        })
        .unwrap();
    assert_eq!(check_rx.await.unwrap(), Ok(false));
    harness.actor.abort();
}

#[tokio::test]
async fn restored_child_rejects_a_nonactive_direct_security_parent() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let (respond_to, response) = tokio::sync::oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::AdoptSuspended {
            request: Box::new(request("bad-direct-parent", true)),
            security_parent_session_id: "sibling-child".into(),
            checkpoint: ChildCheckpoint {
                session_id: "bad-direct-parent".into(),
                operation_id: "direct-op".into(),
                checkpoint_seq: 2,
            },
            respond_to,
        })
        .unwrap();
    assert!(response.await.unwrap().is_err());
    assert!(harness.requests.try_recv().is_err());
    harness.actor.abort();
}

#[tokio::test]
async fn same_process_suspended_child_requires_exact_request_before_adoption() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move {
            backend
                .spawn(request("retired-suspended-child", true))
                .await
        }
    });
    assert_eq!(
        harness.requests.recv().await.unwrap().id,
        "retired-suspended-child"
    );
    let sender = harness.backend.sender();
    let (freeze_tx, freeze_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::FreezeAdmission {
            root_session_id: "parent".into(),
            operation_id: "suspend-retirement".into(),
            respond_to: freeze_tx,
        })
        .unwrap();
    freeze_rx.await.unwrap().unwrap();
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("retired-suspended-child")
    );
    loop {
        if harness.running_counts.recv().await == Some(0) {
            break;
        }
    }

    let checkpoint = ChildCheckpoint {
        session_id: "retired-suspended-child".into(),
        operation_id: "suspend-retirement".into(),
        checkpoint_seq: 1,
    };
    let mut mismatched_request = request("retired-suspended-child", true);
    mismatched_request.prompt = "different request body".into();
    let (mismatch_tx, mismatch_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::AdoptSuspended {
            request: Box::new(mismatched_request),
            security_parent_session_id: "parent".into(),
            checkpoint: checkpoint.clone(),
            respond_to: mismatch_tx,
        })
        .unwrap();
    assert!(mismatch_rx.await.unwrap().is_err());

    let (adopt_tx, adopt_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::AdoptSuspended {
            request: Box::new(request("retired-suspended-child", true)),
            security_parent_session_id: "parent".into(),
            checkpoint,
            respond_to: adopt_tx,
        })
        .unwrap();
    assert_eq!(
        harness.requests.recv().await.unwrap().id,
        "retired-suspended-child"
    );
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("retired-suspended-child")
    );
    assert_eq!(adopt_rx.await.unwrap(), Ok(()));
    assert!(harness.completions.try_recv().is_err());
    assert!(
        !spawn.is_finished(),
        "the original caller remains attached to the suspended child"
    );
    spawn.abort();
    assert!(spawn.await.is_err());
    harness.actor.abort();
}

struct TestRunner {
    wait_before_start: bool,
    wait_after_cancel: bool,
    start: tokio::sync::broadcast::Sender<()>,
    finish: tokio::sync::broadcast::Sender<()>,
    completions: mpsc::UnboundedSender<CompletionDisposition>,
    requests: mpsc::UnboundedSender<SubagentRequest>,
    security_parents: mpsc::UnboundedSender<Option<String>>,
    started: mpsc::UnboundedSender<String>,
    running_counts: mpsc::UnboundedSender<usize>,
    terminal_commit_ready: Arc<AtomicBool>,
    reconcile_mode: Arc<AtomicUsize>,
    reconcile_calls: Arc<AtomicUsize>,
    reconcile_release: Arc<tokio::sync::Notify>,
    panic_notified: Arc<AtomicBool>,
    panic_observations: Arc<parking_lot::Mutex<Vec<PanicObservation>>>,
    retirement_ready: Arc<AtomicBool>,
}

impl ChildRunner for TestRunner {
    fn interact(
        &self,
        request: super::super::interaction::AgentInteractionRequest,
        target: String,
        subagent_task_name: String,
    ) {
        let _ = request
            .respond_to
            .send(Ok(super::super::interaction::AgentInteractionOutput {
                subagent_task_name: None,
                target_session_id: None,
                id: request.id,
                status: if request.receipt_only {
                    "receipt_only"
                } else {
                    "answered"
                }
                .into(),
                receipt_id: None,
                answer: Some(format!(
                    "{}->{target}:{subagent_task_name}",
                    request.source_session_id
                )),
                error: None,
            }));
    }

    type Control = TestControl;
    type CompletionData = String;
    type RunFuture = SendBoxFuture<ChildRunOutcome<String>>;
    type ValidateFuture = SendBoxFuture<SubagentValidateTypeOutcome>;

    fn run(&self, run: ChildRunRequest<Self::Control>) -> Self::RunFuture {
        let wait_before_start = self.wait_before_start;
        let wait_after_cancel = self.wait_after_cancel;
        let mut start = self.start.subscribe();
        let mut finish = self.finish.subscribe();
        let requests = self.requests.clone();
        let security_parents = self.security_parents.clone();
        let started = self.started.clone();
        let panic_notified = self.panic_notified.clone();
        Box::pin(async move {
            let ChildRunRequest {
                request,
                security_parent_session_id: _,
                security_parent,
                cancellation,
                restore,
                reporter,
            } = run;
            let completion_data = request.id.clone();
            let _ = requests.send(request.clone());
            let _ = security_parents.send(security_parent);
            if request.id == "restore-blocked" {
                return ChildRunOutcome::RestoreBlocked {
                    checkpoint: restore.expect("adoption supplies checkpoint"),
                    error: "restore state is unavailable".into(),
                    completion_data,
                };
            }
            if wait_before_start {
                tokio::select! {
                    _ = cancellation.cancelled() => {
                        if wait_after_cancel {
                            let _ = finish.recv().await;
                        }
                        return ChildRunOutcome::Finished(ChildRunOutput {
                            result: cancelled_result(&request),
                            completion_data: completion_data.clone(),
                        });
                    }
                    _ = start.recv() => {}
                }
            }
            if !reporter
                .started(StartedChild {
                    child_session_id: request.id.clone(),
                    resumed_from: request.resume_from.clone(),
                    // Mock definition resolution: this type declares background.
                    definition_background: request.subagent_type == "background-default",
                    control: TestControl {
                        cancellation: cancellation.clone(),
                        session_id: request.id.clone(),
                        panic_notified,
                    },
                })
                .await
            {
                return ChildRunOutcome::Finished(ChildRunOutput {
                    result: cancelled_result(&request),
                    completion_data: completion_data.clone(),
                });
            }
            if request.id == "execution-returned" {
                reporter.execution_returned("completed");
            }
            let _ = started.send(request.id.clone());
            if request.id == "retired-suspended-child" {
                return ChildRunOutcome::Suspended {
                    checkpoint: ChildCheckpoint {
                        session_id: request.id.clone(),
                        operation_id: "suspend-retirement".into(),
                        checkpoint_seq: 1,
                    },
                    completion_data,
                };
            }
            if request.id == "panic-after-completed-outcome" {
                reporter.execution_returned("completed");
                panic!("injected runner panic after completed outcome was reported");
            }
            // Cancellation is authoritative once the coordinator has closed
            // the owner scope.  Bias it ahead of the synthetic `finish`
            // broadcast so a test that releases multiple children together
            // cannot randomly report success for an already-cancelled child.
            let result = tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    if wait_after_cancel {
                        let _ = finish.recv().await;
                    }
                    cancelled_result(&request)
                },
                _ = finish.recv() => SubagentResult {
                    success: true,
                    output: request.prompt.clone().into(),
                    subagent_id: request.id.clone(),
                    child_session_id: request.id.clone(),
                    tool_calls: 3,
                    turns: 2,
                    ..Default::default()
                },
            };
            ChildRunOutcome::Finished(ChildRunOutput {
                result,
                completion_data,
            })
        })
    }

    fn validate_type(
        &self,
        _subagent_type: String,
        _parent_session_id: String,
    ) -> Self::ValidateFuture {
        Box::pin(std::future::ready(SubagentValidateTypeOutcome::Ok))
    }

    fn on_completed(&self, completion: ChildCompletion<Self::CompletionData>) {
        let _ = self.completions.send(completion.disposition);
    }

    fn running_count_changed(&self, running: usize) {
        let _ = self.running_counts.send(running);
    }

    fn persisted_output_ref(&self, completion_data: &Self::CompletionData) -> Option<String> {
        (completion_data == "missing-output").then(|| "artifact:missing".into())
    }

    fn terminal_committed(&self, _completion_data: &Self::CompletionData) -> bool {
        self.terminal_commit_ready.load(Ordering::SeqCst)
    }

    fn suspension_retired(
        &self,
        completion_data: &mut Self::CompletionData,
    ) -> Result<bool, String> {
        Ok(completion_data == "retired-suspended-child"
            || (completion_data == "retiring-completed-child"
                && self.retirement_ready.load(Ordering::SeqCst)))
    }

    fn has_physical_retirement_owner(&self, completion_data: &Self::CompletionData) -> bool {
        completion_data == "retiring-completed-child"
    }

    fn completion_after_panic(
        &self,
        request: &SubagentRequest,
        control: Option<&Self::Control>,
        execution_outcome: Option<&str>,
    ) -> Self::CompletionData {
        let completion_data = request.id.clone();
        self.panic_observations.lock().push((
            request.id.clone(),
            control.is_some(),
            execution_outcome.map(str::to_owned),
            completion_data.clone(),
        ));
        completion_data
    }

    fn reconcile(
        &self,
        _request: SubagentRequest,
        output: ChildRunOutput<Self::CompletionData>,
        _deadline: tokio::time::Instant,
    ) -> Result<Self::RunFuture, Box<ChildRunOutput<Self::CompletionData>>> {
        self.reconcile_calls.fetch_add(1, Ordering::SeqCst);
        match self.reconcile_mode.load(Ordering::SeqCst) {
            0 => Err(Box::new(output)),
            1 => {
                let release = self.reconcile_release.clone();
                Ok(Box::pin(async move {
                    release.notified().await;
                    ChildRunOutcome::Finished(output)
                }))
            }
            _ => Ok(Box::pin(
                async move { panic!("injected reconciliation panic") },
            )),
        }
    }
}

fn cancelled_result(request: &SubagentRequest) -> SubagentResult {
    SubagentResult {
        success: false,
        cancelled: true,
        error: Some("cancelled".to_owned()),
        subagent_id: request.id.clone(),
        child_session_id: request.id.clone(),
        ..Default::default()
    }
}

fn request(id: &str, background: bool) -> SubagentRequest {
    SubagentRequest {
        id: id.to_owned(),

        origin_tool_call_id: None,
        prompt: "work".to_owned(),
        description: "test child".to_owned(),
        subagent_type: "explore".to_owned(),
        parent_session_id: "parent".to_owned(),
        parent_step_id: None,
        parent_prompt_id: Some("prompt".to_owned()),
        resume_from: None,
        cwd: None,
        runtime_overrides: Default::default(),
        run_in_background: background,
        surface_completion: true,
        await_to_completion: false,
        fork_context: false,
        owner: SubagentOwner::Task,
        goal_context: None,
        cancel_token: CancellationToken::new(),
    }
}

#[test]
fn source_liveness_does_not_cross_sibling_security_parents() {
    let mut source = request("source", true);
    source.parent_session_id = "root".into();
    // Nested children retain the lifecycle root for teardown, but resume
    // liveness is queried by the direct security parent. A sibling must not
    // observe this source merely because both share the root session.
    assert!(belongs_to_session(&source, "child-a", Some("root")));
    assert!(belongs_to_session(&source, "child-a", Some("child-a")));
    assert!(!belongs_to_session(&source, "child-a", Some("child-b")));
}

#[test]
fn suspension_checkpoint_requires_frozen_root_operation_and_exact_child_session() {
    let checkpoint = ChildCheckpoint {
        session_id: "child-session".into(),
        operation_id: "suspend-1".into(),
        checkpoint_seq: 7,
    };

    assert!(super::suspension_matches(
        Some("root"),
        Some("suspend-1"),
        "root",
        "child-session",
        &checkpoint,
    ));
    assert!(!super::suspension_matches(
        None,
        Some("suspend-1"),
        "root",
        "child-session",
        &checkpoint,
    ));
    assert!(!super::suspension_matches(
        Some("other-root"),
        Some("suspend-1"),
        "root",
        "child-session",
        &checkpoint,
    ));
    assert!(!super::suspension_matches(
        Some("root"),
        Some("different-operation"),
        "root",
        "child-session",
        &checkpoint,
    ));
    assert!(!super::suspension_matches(
        Some("root"),
        Some("suspend-1"),
        "root",
        "different-session",
        &checkpoint,
    ));
}

struct Harness {
    backend: ChannelBackend,
    start: tokio::sync::broadcast::Sender<()>,
    finish: tokio::sync::broadcast::Sender<()>,
    completions: mpsc::UnboundedReceiver<CompletionDisposition>,
    requests: mpsc::UnboundedReceiver<SubagentRequest>,
    security_parents: mpsc::UnboundedReceiver<Option<String>>,
    started: mpsc::UnboundedReceiver<String>,
    running_counts: mpsc::UnboundedReceiver<usize>,
    actor: tokio::task::JoinHandle<()>,
    terminal_commit_ready: Arc<AtomicBool>,
    reconcile_mode: Arc<AtomicUsize>,
    reconcile_calls: Arc<AtomicUsize>,
    reconcile_release: Arc<tokio::sync::Notify>,
    panic_notified: Arc<AtomicBool>,
    panic_observations: Arc<parking_lot::Mutex<Vec<PanicObservation>>>,
    retirement_ready: Arc<AtomicBool>,
}

fn harness(wait_before_start: bool, foreground_budget: std::time::Duration) -> Harness {
    harness_with_config(wait_before_start, CoordinatorConfig { foreground_budget })
}

fn harness_with_config(wait_before_start: bool, config: CoordinatorConfig) -> Harness {
    harness_with_options(wait_before_start, false, config)
}

fn harness_with_options(
    wait_before_start: bool,
    wait_after_cancel: bool,
    config: CoordinatorConfig,
) -> Harness {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (start, _) = tokio::sync::broadcast::channel(4);
    let (finish, _) = tokio::sync::broadcast::channel(4);
    let (completion_tx, completions) = mpsc::unbounded_channel();
    let (request_tx, requests) = mpsc::unbounded_channel();
    let (security_parent_tx, security_parents) = mpsc::unbounded_channel();
    let (started_tx, started) = mpsc::unbounded_channel();
    let (running_count_tx, running_counts) = mpsc::unbounded_channel();
    let terminal_commit_ready = Arc::new(AtomicBool::new(true));
    let reconcile_mode = Arc::new(AtomicUsize::new(0));
    let reconcile_calls = Arc::new(AtomicUsize::new(0));
    let reconcile_release = Arc::new(tokio::sync::Notify::new());
    let panic_notified = Arc::new(AtomicBool::new(false));
    let panic_observations = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let retirement_ready = Arc::new(AtomicBool::new(false));
    let actor = tokio::spawn(
        SubagentCoordinator::new(
            command_rx,
            TestRunner {
                wait_before_start,
                wait_after_cancel,
                start: start.clone(),
                finish: finish.clone(),
                completions: completion_tx,
                requests: request_tx,
                security_parents: security_parent_tx,
                started: started_tx,
                running_counts: running_count_tx,
                terminal_commit_ready: terminal_commit_ready.clone(),
                reconcile_mode: reconcile_mode.clone(),
                reconcile_calls: reconcile_calls.clone(),
                reconcile_release: reconcile_release.clone(),
                panic_notified: panic_notified.clone(),
                panic_observations: panic_observations.clone(),
                retirement_ready: retirement_ready.clone(),
            },
            config,
        )
        .run(),
    );
    Harness {
        // Unbound by default so tests can set request.parent_session_id
        // freely (e.g. nested reparent). ParentSession APIs must use
        // `parent_backend` so they stay session-scoped.
        backend: ChannelBackend::new(command_tx),
        start,
        finish,
        completions,
        requests,
        security_parents,
        started,
        running_counts,
        actor,
        terminal_commit_ready,
        reconcile_mode,
        reconcile_calls,
        reconcile_release,
        panic_notified,
        panic_observations,
        retirement_ready,
    }
}

/// Session-bound backend for ParentSession cancel / admission on the default
/// test parent (`"parent"`). Required because unbound cancel is rejected.
fn parent_backend(harness: &Harness) -> ChannelBackend {
    ChannelBackend::for_session(harness.backend.sender(), "parent")
}

async fn complete_background_child(harness: &mut Harness, id: &str) -> SubagentResult {
    let backend = harness.backend.clone();
    let id = id.to_owned();
    let spawn_id = id.clone();
    let spawn = tokio::spawn(async move { backend.spawn(request(&spawn_id, true)).await });
    let observed = harness.requests.recv().await.expect("spawn request");
    let observed_id = observed.id.clone();
    assert_eq!(observed_id, id);
    assert_eq!(harness.started.recv().await.as_deref(), Some(id.as_str()));
    harness.finish.send(()).expect("child finish receiver");
    spawn.await.expect("spawn task").expect("child result")
}

#[tokio::test]
async fn uncommitted_success_stays_finalization_pending_instead_of_becoming_failed() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    harness.terminal_commit_ready.store(false, Ordering::SeqCst);

    let result = complete_background_child(&mut harness, "pending-result").await;
    assert!(result.success);
    assert!(result.finalization_pending);
    assert!(!result.output_unavailable);

    let snapshot = harness
        .backend
        .query("pending-result", false, None)
        .await
        .unwrap()
        .expect("pending finalization remains queryable");
    assert!(matches!(
        snapshot.status,
        SubagentSnapshotStatus::FinalizationPending { .. }
    ));
    assert!(harness.completions.try_recv().is_err());
    harness.actor.abort();
}

#[tokio::test]
async fn pending_finalization_survives_ordinary_completed_cache_eviction() {
    use crate::implementations::grow_build::task::coordinator_state::MAX_COMPLETED_ENTRIES;

    let mut harness = harness(false, std::time::Duration::from_secs(60));
    harness.terminal_commit_ready.store(false, Ordering::SeqCst);
    let pending = complete_background_child(&mut harness, "retained-pending").await;
    assert!(pending.finalization_pending);
    harness.terminal_commit_ready.store(true, Ordering::SeqCst);

    for index in 0..=MAX_COMPLETED_ENTRIES {
        let id = format!("ordinary-{index}");
        let result = complete_background_child(&mut harness, &id).await;
        assert!(result.success);
        harness
            .completions
            .recv()
            .await
            .expect("ordinary completion callback");
    }

    let retained = harness
        .backend
        .query("retained-pending", false, None)
        .await
        .unwrap()
        .expect("pending responsibility is retained outside the completed LRU");
    assert!(matches!(
        retained.status,
        SubagentSnapshotStatus::FinalizationPending { .. }
    ));
    harness.actor.abort();
}

#[tokio::test]
async fn pending_finalizations_apply_recovery_capacity_backpressure() {
    use crate::implementations::grow_build::task::coordinator_state::MAX_FINALIZATIONS;

    let mut harness = harness(false, std::time::Duration::from_secs(60));
    harness.terminal_commit_ready.store(false, Ordering::SeqCst);
    let mut spawns = Vec::with_capacity(MAX_FINALIZATIONS);
    for index in 0..MAX_FINALIZATIONS {
        let backend = harness.backend.clone();
        let id = format!("pending-capacity-{index}");
        spawns.push(tokio::spawn(async move {
            backend.spawn(request(&id, true)).await
        }));
    }
    let mut observed_ids = std::collections::HashSet::with_capacity(MAX_FINALIZATIONS);
    for _ in 0..MAX_FINALIZATIONS {
        let requested = harness.requests.recv().await.expect("child spawn request");
        observed_ids.insert(requested.id);
        observed_ids.insert(harness.started.recv().await.expect("started child id"));
    }
    assert_eq!(observed_ids.len(), MAX_FINALIZATIONS);
    harness.finish.send(()).expect("active child receivers");
    for spawn in spawns {
        let result = spawn.await.expect("spawn task").expect("child result");
        assert!(result.success && result.finalization_pending);
    }

    let rejected = harness
        .backend
        .spawn(request("over-capacity", true))
        .await
        .expect("capacity rejection is returned as a lifecycle result");
    assert!(!rejected.success);
    assert!(
        rejected
            .error
            .as_deref()
            .is_some_and(|error| error.contains("recovery capacity exhausted"))
    );
    harness.actor.abort();
}

#[tokio::test]
async fn uncommitted_output_larger_than_the_recovery_budget_is_cleared() {
    use crate::implementations::grow_build::task::coordinator_state::MAX_FINALIZATION_OUTPUT_BYTES;

    let mut harness = harness(false, std::time::Duration::from_secs(60));
    harness.terminal_commit_ready.store(false, Ordering::SeqCst);
    let mut oversized = request("oversized-pending", true);
    oversized.prompt = "x".repeat(MAX_FINALIZATION_OUTPUT_BYTES + 1);
    let backend = harness.backend.clone();
    let spawn = tokio::spawn(async move { backend.spawn(oversized).await });
    let observed = harness.requests.recv().await.expect("spawn request");
    assert_eq!(observed.id, "oversized-pending");
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("oversized-pending")
    );
    harness.finish.send(()).expect("child finish receiver");
    let result = spawn.await.expect("spawn task").expect("child result");
    assert!(result.success && result.finalization_pending);
    assert!(result.output_unavailable);
    assert!(result.output.is_empty());
    harness.actor.abort();
}

#[tokio::test]
async fn late_reconciliation_delivers_completion_once() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    harness.terminal_commit_ready.store(false, Ordering::SeqCst);
    harness.reconcile_mode.store(1, Ordering::SeqCst);
    let initial = complete_background_child(&mut harness, "late-reconcile").await;
    assert!(initial.success && initial.finalization_pending);

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while harness.reconcile_calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("coordinator starts reconciliation");
    harness.terminal_commit_ready.store(true, Ordering::SeqCst);
    harness.reconcile_release.notify_one();

    let completion = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        harness.completions.recv(),
    )
    .await
    .expect("late canonical result is delivered")
    .expect("completion callback");
    assert!(completion.should_surface);
    tokio::task::yield_now().await;
    assert!(harness.completions.try_recv().is_err());
    assert_eq!(harness.reconcile_calls.load(Ordering::SeqCst), 1);
    let terminal = harness
        .backend
        .query("late-reconcile", false, None)
        .await
        .unwrap()
        .expect("reconciled child stays queryable");
    assert!(matches!(
        terminal.status,
        SubagentSnapshotStatus::Completed { .. }
    ));
    harness.actor.abort();
}

#[tokio::test(start_paused = true)]
async fn finalization_cycle_deadline_survives_queries_until_new_admission() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    harness.terminal_commit_ready.store(false, Ordering::SeqCst);
    harness.reconcile_mode.store(1, Ordering::SeqCst);
    let initial = complete_background_child(&mut harness, "deadline-reconcile").await;
    assert!(initial.success && initial.finalization_pending);

    for _ in 0..20 {
        if harness.reconcile_calls.load(Ordering::SeqCst) >= 1 {
            break;
        }
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        tokio::task::yield_now().await;
    }
    assert_eq!(
        harness.reconcile_calls.load(Ordering::SeqCst),
        1,
        "first reconciliation starts and remains in flight"
    );

    for _ in 0..4 {
        let snapshot = harness
            .backend
            .query("deadline-reconcile", false, None)
            .await
            .expect("query is authorized")
            .expect("finalization remains queryable");
        assert!(matches!(
            snapshot.status,
            SubagentSnapshotStatus::FinalizationPending { .. }
        ));
    }
    assert_eq!(harness.reconcile_calls.load(Ordering::SeqCst), 1);

    tokio::time::advance(std::time::Duration::from_secs(30)).await;
    let _ = harness
        .backend
        .query("deadline-reconcile", false, None)
        .await
        .expect("query is authorized");
    assert_eq!(
        harness.reconcile_calls.load(Ordering::SeqCst),
        1,
        "the in-flight reconciliation owns the expired cycle"
    );
    harness.reconcile_release.notify_one();

    // Let the first owner return an uncommitted result. The expired cycle may
    // remain queryable, but no periodic retry or query may extend it.
    let _ = harness
        .backend
        .query("deadline-reconcile", false, None)
        .await
        .expect("query is authorized");
    tokio::time::advance(std::time::Duration::from_secs(6)).await;
    for _ in 0..4 {
        let snapshot = harness
            .backend
            .query("deadline-reconcile", false, None)
            .await
            .expect("query is authorized")
            .expect("pending finalization remains queryable");
        assert!(matches!(
            snapshot.status,
            SubagentSnapshotStatus::FinalizationPending { .. }
        ));
    }
    assert_eq!(
        harness.reconcile_calls.load(Ordering::SeqCst),
        1,
        "queries and recovery ticks do not reset an expired cycle"
    );

    assert!(parent_backend(&harness).open_spawn_admission());
    let _ = harness
        .backend
        .query("deadline-reconcile", false, None)
        .await
        .expect("query is authorized and follows admission");
    for _ in 0..20 {
        if harness.reconcile_calls.load(Ordering::SeqCst) >= 2 {
            break;
        }
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        tokio::task::yield_now().await;
    }
    assert_eq!(
        harness.reconcile_calls.load(Ordering::SeqCst),
        2,
        "a new admitted turn opens a fresh recovery cycle"
    );

    // A second legal admission can arrive while the new cycle's owner is in
    // flight. It authorizes a later cycle but cannot start concurrent work.
    assert!(parent_backend(&harness).open_spawn_admission());
    let _ = harness
        .backend
        .query("deadline-reconcile", false, None)
        .await
        .expect("query is authorized and follows admission");
    assert_eq!(harness.reconcile_calls.load(Ordering::SeqCst), 2);
    harness.reconcile_release.notify_one();
    let _ = harness
        .backend
        .query("deadline-reconcile", false, None)
        .await
        .expect("query waits behind reconciliation completion");
    for _ in 0..20 {
        if harness.reconcile_calls.load(Ordering::SeqCst) >= 3 {
            break;
        }
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        tokio::task::yield_now().await;
    }
    assert_eq!(
        harness.reconcile_calls.load(Ordering::SeqCst),
        3,
        "admission received in flight is retained until its owner returns"
    );
    tokio::time::advance(std::time::Duration::from_secs(6)).await;
    let _ = harness
        .backend
        .query("deadline-reconcile", false, None)
        .await
        .expect("query is authorized");
    assert_eq!(
        harness.reconcile_calls.load(Ordering::SeqCst),
        3,
        "one child has only one reconciliation in flight"
    );
    harness.actor.abort();
}

#[tokio::test]
async fn reconciliation_panic_keeps_finalization_pending() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    harness.terminal_commit_ready.store(false, Ordering::SeqCst);
    harness.reconcile_mode.store(2, Ordering::SeqCst);
    let initial = complete_background_child(&mut harness, "panic-reconcile").await;
    assert!(initial.finalization_pending);

    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while harness.reconcile_calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("coordinator attempts reconciliation");
    tokio::task::yield_now().await;
    assert!(harness.completions.try_recv().is_err());
    let snapshot = harness
        .backend
        .query("panic-reconcile", false, None)
        .await
        .unwrap()
        .expect("panic does not discard recovery responsibility");
    assert!(matches!(
        snapshot.status,
        SubagentSnapshotStatus::FinalizationPending { .. }
    ));
    harness.actor.abort();
}

#[tokio::test]
async fn runner_panic_after_execution_returned_preserves_completion_owner_and_outcome() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    harness.terminal_commit_ready.store(false, Ordering::SeqCst);
    let backend = harness.backend.clone();
    let spawn = tokio::spawn(async move {
        backend
            .spawn(request("panic-after-completed-outcome", true))
            .await
    });

    let requested = harness.requests.recv().await.expect("one child request");
    assert_eq!(requested.id, "panic-after-completed-outcome");
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("panic-after-completed-outcome")
    );
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), spawn)
        .await
        .expect("panic is caught by the coordinator")
        .expect("spawn task")
        .expect("the original caller receives its execution outcome");

    assert!(result.success, "known completed outcome remains completed");
    assert!(
        !result.cancelled,
        "panic cancellation must not rewrite outcome"
    );
    assert!(result.finalization_pending);
    assert!(result.output_unavailable);
    assert!(
        result.output.is_empty(),
        "panic must not invent result text"
    );
    assert_eq!(result.status(), "finalization_pending");
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|message| message.contains("runtime panicked"))
    );
    assert!(
        harness.panic_notified.load(Ordering::SeqCst),
        "the active control is told that its runner panicked and is cancelled"
    );
    let panic_observations = harness.panic_observations.lock().clone();
    assert_eq!(panic_observations.len(), 1, "one panic owner is recorded");
    assert_eq!(panic_observations[0].0, "panic-after-completed-outcome");
    assert!(panic_observations[0].1, "active control reaches panic hook");
    assert_eq!(panic_observations[0].2.as_deref(), Some("completed"));
    assert_eq!(panic_observations[0].3, "panic-after-completed-outcome");
    assert_eq!(
        harness.backend.registry_counts().await,
        SubagentRegistryCounts {
            pending: 0,
            active: 0,
            completed: 1,
        },
        "the panic leaves one completed record under finalization ownership"
    );
    let snapshot = harness
        .backend
        .query("panic-after-completed-outcome", false, None)
        .await
        .unwrap()
        .expect("the original child remains queryable");
    assert!(matches!(
        snapshot.status,
        SubagentSnapshotStatus::FinalizationPending { outcome, .. }
            if outcome == "completed"
    ));
    assert!(harness.completions.try_recv().is_err());
    assert!(
        harness.requests.try_recv().is_err(),
        "panic starts no replacement child"
    );
    harness.actor.abort();
}

async fn loop_unit_active(backend: &ChannelBackend, task_id: &str) -> bool {
    let (respond_to, response_rx) = oneshot::channel();
    backend
        .sender()
        .send(SubagentEvent::LoopUnitActive(
            SubagentLoopUnitActiveRequest {
                task_id: task_id.to_owned(),
                respond_to,
            },
        ))
        .expect("actor command channel open");
    response_rx.await.expect("loop activity response")
}

async fn outstanding(backend: &ChannelBackend, prompt_id: &str) -> SubagentOutstandingReply {
    let (respond_to, response_rx) = oneshot::channel();
    backend
        .sender()
        .send(SubagentEvent::Outstanding(SubagentOutstandingRequest {
            parent_session_id: "parent".to_owned(),
            prompt_id: prompt_id.to_owned(),
            respond_to,
        }))
        .expect("actor command channel open");
    response_rx.await.expect("outstanding response")
}

#[tokio::test]
async fn foreground_completion_is_delivered_inline() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("inline", false)).await }
    });
    tokio::task::yield_now().await;
    let _ = harness.finish.send(());

    let result = spawn.await.unwrap().unwrap();
    assert!(result.success);
    let disposition = harness.completions.recv().await.unwrap();
    assert!(disposition.foreground_delivered);
    assert!(!disposition.should_surface);
    harness.actor.abort();
}

#[tokio::test(start_paused = true)]
async fn foreground_deadline_hands_off_without_stopping_child() {
    let mut harness = harness(false, std::time::Duration::from_secs(1));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("slow", false)).await }
    });
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    let interim = spawn.await.unwrap().unwrap();
    assert!(interim.backgrounded);
    // Interim handoff must not read as a completion (status() contract).
    assert!(!interim.success);
    assert_eq!(
        outstanding(&harness.backend, "prompt").await,
        SubagentOutstandingReply {
            live_ids: Vec::new(),
            background_live: true,
            subagent_usage_not_applied: false,
        }
    );
    assert_eq!(
        harness.backend.registry_counts().await,
        SubagentRegistryCounts {
            pending: 0,
            active: 1,
            completed: 0,
        }
    );

    let running = harness
        .backend
        .query("slow", false, None)
        .await
        .unwrap()
        .expect("active child should be queryable");
    assert!(running.is_running());
    match running.status {
        SubagentSnapshotStatus::Running {
            turn_count,
            tool_call_count,
            tokens_used,
            ..
        } => {
            assert_eq!(turn_count, 2);
            assert_eq!(tool_call_count, 3);
            assert_eq!(tokens_used, 100);
        }
        status => panic!("expected signal-backed running counters, got {status:?}"),
    }
    let _ = harness.finish.send(());
    let disposition = harness.completions.recv().await.unwrap();
    assert!(disposition.backgrounded);
    assert!(disposition.should_surface);
    harness.actor.abort();
}

#[tokio::test]
async fn live_blocking_waiter_suppresses_async_surface() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("waited", true)).await }
    });
    tokio::task::yield_now().await;
    let wait = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.query("waited", true, Some(60_000)).await.unwrap() }
    });
    tokio::task::yield_now().await;
    let _ = harness.finish.send(());

    assert!(wait.await.unwrap().unwrap().status.is_terminal());
    let disposition = harness.completions.recv().await.unwrap();
    assert!(disposition.waiter_delivered);
    assert!(!disposition.should_surface);
    assert!(spawn.await.unwrap().unwrap().success);
    harness.actor.abort();
}

#[tokio::test(start_paused = true)]
async fn timed_out_waiter_does_not_suppress_later_completion() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("timeout", true)).await }
    });
    tokio::task::yield_now().await;
    let snapshot = harness
        .backend
        .query("timeout", true, Some(1_000))
        .await
        .unwrap()
        .expect("timeout returns a running snapshot");
    assert!(snapshot.is_running());

    let _ = harness.finish.send(());
    let disposition = harness.completions.recv().await.unwrap();
    assert!(!disposition.waiter_delivered);
    assert!(disposition.should_surface);
    assert!(spawn.await.unwrap().unwrap().success);
    harness.actor.abort();
}

#[tokio::test(start_paused = true)]
async fn surviving_waiter_suppresses_after_peer_times_out() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("two-waiters", true)).await }
    });
    tokio::task::yield_now().await;
    let short = tokio::spawn({
        let backend = harness.backend.clone();
        async move {
            backend
                .query("two-waiters", true, Some(1_000))
                .await
                .unwrap()
                .expect("timed waiter receives running snapshot")
        }
    });
    let long = tokio::spawn({
        let backend = harness.backend.clone();
        async move {
            backend
                .query("two-waiters", true, Some(60_000))
                .await
                .unwrap()
                .expect("long waiter receives terminal snapshot")
        }
    });
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    assert!(short.await.unwrap().is_running());

    let _ = harness.finish.send(());
    assert!(long.await.unwrap().status.is_terminal());
    let disposition = harness.completions.recv().await.unwrap();
    assert!(disposition.waiter_delivered);
    assert!(!disposition.should_surface);
    assert!(spawn.await.unwrap().unwrap().success);
    harness.actor.abort();
}

#[tokio::test]
async fn dropped_waiter_does_not_suppress_completion() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("dropped-wait", true)).await }
    });
    tokio::task::yield_now().await;
    let wait = tokio::spawn({
        let backend = harness.backend.clone();
        async move {
            backend
                .query("dropped-wait", true, Some(60_000))
                .await
                .unwrap()
        }
    });
    tokio::task::yield_now().await;
    wait.abort();
    let _ = wait.await;

    let _ = harness.finish.send(());
    let disposition = harness.completions.recv().await.unwrap();
    assert!(!disposition.waiter_delivered);
    assert!(disposition.should_surface);
    assert!(spawn.await.unwrap().unwrap().success);
    harness.actor.abort();
}

#[tokio::test]
async fn pending_cancel_delivers_waiter_once() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("pending-cancel", true)).await }
    });
    tokio::task::yield_now().await;
    let wait = tokio::spawn({
        let backend = harness.backend.clone();
        async move {
            backend
                .query("pending-cancel", true, Some(60_000))
                .await
                .unwrap()
        }
    });
    tokio::task::yield_now().await;
    assert!(matches!(
        harness.backend.cancel("pending-cancel").await,
        Ok(SubagentCancelOutcome::Cancelled)
    ));
    let snapshot = wait.await.unwrap().unwrap();
    assert!(matches!(
        snapshot.status,
        SubagentSnapshotStatus::Cancelled { .. }
    ));
    let disposition = harness.completions.recv().await.unwrap();
    assert!(disposition.waiter_delivered);
    assert!(disposition.explicitly_killed);
    assert!(!disposition.should_surface);
    assert!(spawn.await.unwrap().unwrap().cancelled);
    harness.actor.abort();
}

#[tokio::test]
async fn caller_drop_during_initialization_does_not_drop_owned_run() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("owned", false)).await }
    });
    tokio::task::yield_now().await;
    spawn.abort();
    let _ = spawn.await;

    let initializing = harness
        .backend
        .query("owned", false, None)
        .await
        .unwrap()
        .expect("pending child should be queryable");
    assert!(matches!(
        initializing.status,
        SubagentSnapshotStatus::Initializing
    ));
    let _ = harness.start.send(());
    tokio::task::yield_now().await;
    let _ = harness.finish.send(());
    let disposition = harness.completions.recv().await.unwrap();
    assert!(
        disposition.should_surface,
        "dropped foreground receiver becomes handle-only"
    );
    let terminal = harness
        .backend
        .query("owned", false, None)
        .await
        .unwrap()
        .expect("completed child should be queryable");
    assert!(terminal.status.is_terminal());
    harness.actor.abort();
}

#[tokio::test]
async fn abandoned_foreground_caller_clears_outstanding() {
    // ParentGone parity: dropping the spawn await must leave Outstanding
    // (turn-freeze) without waiting for the foreground budget.
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("abandoned", false)).await }
    });
    tokio::task::yield_now().await;
    assert_eq!(
        outstanding(&harness.backend, "prompt").await.live_ids,
        vec!["abandoned".to_owned()],
        "live foreground child blocks the turn"
    );

    spawn.abort();
    let _ = spawn.await;
    assert_eq!(
        outstanding(&harness.backend, "prompt").await,
        SubagentOutstandingReply {
            live_ids: Vec::new(),
            background_live: true,
            subagent_usage_not_applied: false,
        },
        "caller-gone foreground is handle-only for Outstanding"
    );
    let running = harness
        .backend
        .query("abandoned", false, None)
        .await
        .unwrap()
        .expect("abandoned caller leaves its child queryable");
    assert!(running.is_running(), "child keeps running after ParentGone");

    let _ = harness.finish.send(());
    let disposition = harness.completions.recv().await.unwrap();
    assert!(disposition.backgrounded);
    assert!(disposition.should_surface);
    harness.actor.abort();
}

#[tokio::test]
async fn duplicate_subagent_id_is_rejected_without_replacing_live_child() {
    let harness = harness(false, std::time::Duration::from_secs(60));
    let first = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("duplicate", true)).await }
    });
    tokio::task::yield_now().await;

    let duplicate = harness
        .backend
        .spawn(request("duplicate", false))
        .await
        .expect("duplicate rejection is a lifecycle result");
    assert!(!duplicate.success);
    assert!(
        duplicate
            .error
            .as_deref()
            .is_some_and(|error| error.contains("already exists"))
    );

    let running = harness
        .backend
        .query("duplicate", false, None)
        .await
        .unwrap()
        .expect("original child remains queryable");
    assert!(running.is_running());
    let _ = harness.finish.send(());
    assert!(first.await.unwrap().unwrap().success);
    harness.actor.abort();
}

#[tokio::test]
async fn external_cancel_token_cancels_live_child() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let request = request("external-cancel", false);
    let cancel_token = request.cancel_token.clone();
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request).await }
    });
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("external-cancel")
    );

    cancel_token.cancel();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), spawn)
        .await
        .expect("external cancellation should finish")
        .unwrap()
        .unwrap();
    assert!(result.cancelled);
    let disposition = harness.completions.recv().await.unwrap();
    assert!(!disposition.explicitly_killed);
    harness.actor.abort();
}

#[tokio::test]
async fn dropping_coordinator_cancels_live_child() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let cancellation = CancellationToken::new();
    let mut request = request("owner-drop", true);
    request.cancel_token = cancellation.clone();
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request).await }
    });
    assert_eq!(harness.started.recv().await.as_deref(), Some("owner-drop"));

    harness.actor.abort();
    tokio::time::timeout(std::time::Duration::from_secs(1), cancellation.cancelled())
        .await
        .expect("coordinator drop should cancel child");
    assert!(spawn.await.unwrap().is_err());
}

#[tokio::test(start_paused = true)]
async fn await_to_completion_has_no_foreground_deadline() {
    let mut harness = harness(false, std::time::Duration::from_secs(1));
    let mut request = request("await-completion", false);
    request.await_to_completion = true;
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request).await }
    });
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("await-completion")
    );

    tokio::time::advance(std::time::Duration::from_secs(10)).await;
    assert!(!spawn.is_finished());
    let _ = harness.finish.send(());
    let result = spawn.await.unwrap().unwrap();
    assert!(result.success);
    assert!(!result.backgrounded);
    harness.actor.abort();
}

#[tokio::test]
async fn workflow_cancel_waits_for_drain_and_hides_owned_children() {
    let mut harness = harness_with_options(true, true, CoordinatorConfig::default());

    let mut active_request = request("workflow-active", false);
    active_request.await_to_completion = true;
    active_request.owner = SubagentOwner::workflow("workflow-run");
    let active_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(active_request).await }
    });
    assert_eq!(
        harness
            .requests
            .recv()
            .await
            .as_ref()
            .map(|request| request.id.as_str()),
        Some("workflow-active")
    );
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("workflow-active")
    );

    let mut pending_request = request("workflow-pending", false);
    pending_request.await_to_completion = true;
    pending_request.owner = SubagentOwner::workflow("workflow-run");
    let pending_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(pending_request).await }
    });
    assert_eq!(
        harness
            .requests
            .recv()
            .await
            .as_ref()
            .map(|request| request.id.as_str()),
        Some("workflow-pending")
    );

    assert!(matches!(
        harness.backend.query("workflow-active", false, None).await,
        Err(SubagentObservationError::Unauthorized)
    ));
    assert!(matches!(
        harness.backend.query("workflow-pending", false, None).await,
        Err(SubagentObservationError::Unauthorized)
    ));
    assert!(
        harness
            .backend
            .inspect("workflow-active")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        harness
            .backend
            .inspect("workflow-pending")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        harness
            .backend
            .list_running("parent")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        parent_backend(&harness)
            .interact(
                "workflow-inquiry".into(),
                Some("workflow-active".into()),
                super::super::interaction::AgentInteraction::Ask {
                    question: "status?".into(),
                },
                CancellationToken::new(),
            )
            .await
            .is_err(),
        "Workflow-owned children remain outside manual parent control"
    );
    let workflow_send = parent_backend(&harness)
        .interact(
            "workflow-intervention".into(),
            Some("workflow-active".into()),
            super::super::interaction::AgentInteraction::Send {
                message: "change direction".into(),
                interrupt: true,
                reply_to: None,
            },
            CancellationToken::new(),
        )
        .await
        .expect("send preflight rejection is typed");
    assert_eq!(workflow_send.status, "rejected");
    assert_eq!(
        workflow_send
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("no_direct_route")
    );
    let (list_respond_to, list_response_rx) = oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::ListActive(SubagentListActiveRequest {
            parent_session_id: "parent".to_owned(),
            respond_to: list_respond_to,
        }))
        .expect("actor command channel open");
    assert!(list_response_rx.await.unwrap().is_empty());

    let (child_cancel_tx, child_cancel_rx) = oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::Cancel(SubagentCancelRequest {
            parent_session_id: Some("parent".to_owned()),
            target: SubagentCancelTarget::SubagentId("workflow-active".to_owned()),
            respond_to: child_cancel_tx,
        }))
        .expect("actor command channel open");
    assert!(matches!(
        child_cancel_rx.await.unwrap(),
        SubagentCancelOutcome::NotFound
    ));
    assert!(
        harness
            .backend
            .inspect("workflow-active")
            .await
            .unwrap()
            .is_some(),
        "only the Workflow Run owner may cancel its hidden child"
    );

    let (cancel_respond_to, mut cancel_response_rx) = oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::Cancel(SubagentCancelRequest {
            parent_session_id: Some("parent".to_owned()),
            target: SubagentCancelTarget::WorkflowRunId("workflow-run".to_owned()),
            respond_to: cancel_respond_to,
        }))
        .expect("actor command channel open");
    assert!(
        harness
            .backend
            .inspect("workflow-active")
            .await
            .unwrap()
            .is_some()
    );
    assert!(matches!(
        cancel_response_rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));

    let _ = harness.finish.send(());
    assert!(matches!(
        cancel_response_rx.await.unwrap(),
        SubagentCancelOutcome::Cancelled
    ));
    assert!(active_spawn.await.unwrap().unwrap().cancelled);
    assert!(pending_spawn.await.unwrap().unwrap().cancelled);
    assert!(matches!(
        harness.backend.query("workflow-active", false, None).await,
        Err(SubagentObservationError::Unauthorized)
    ));
    assert!(
        harness
            .backend
            .inspect("workflow-active")
            .await
            .unwrap()
            .is_some()
    );

    harness.actor.abort();
}

#[tokio::test]
async fn goal_cancel_waits_for_scoped_owner_drain_without_touching_another_revision() {
    let mut harness = harness_with_options(true, true, CoordinatorConfig::default());

    let mut owned = request("goal-owned", false);
    owned.await_to_completion = true;
    owned.owner = SubagentOwner::goal("goal-epoch", 1);
    let owned_cancel = owned.cancel_token.clone();
    let owned_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(owned).await }
    });
    assert_eq!(harness.requests.recv().await.unwrap().id, "goal-owned");
    let _ = harness.start.send(());
    assert_eq!(harness.started.recv().await.as_deref(), Some("goal-owned"));

    let mut other = request("other-session-goal", false);
    // Same session and Goal id, but a newer definition revision: an old
    // cancellation must not kill this child after an edit race.
    other.parent_session_id = "parent".into();
    other.await_to_completion = true;
    other.owner = SubagentOwner::goal("goal-epoch", 2);
    let other_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(other).await }
    });
    assert_eq!(
        harness.requests.recv().await.unwrap().id,
        "other-session-goal"
    );
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("other-session-goal")
    );

    let (respond_to, mut response) = oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::Cancel(SubagentCancelRequest {
            parent_session_id: Some("parent".into()),
            target: SubagentCancelTarget::Goal {
                goal_id: "goal-epoch".into(),
                definition_revision: 1,
            },
            respond_to,
        }))
        .unwrap();
    assert!(matches!(
        response.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));

    // The cancel command and the synthetic completion broadcast are handled by
    // separate tasks. Wait for the scoped owner cancellation to become visible
    // before releasing both runners, otherwise the test races command delivery.
    owned_cancel.cancelled().await;

    // A new child admitted for the same long-lived Goal after the cancel
    // command is a successor generation. It must not extend the barrier that
    // captured only `goal-owned`.
    let mut successor = request("goal-successor", false);
    successor.await_to_completion = true;
    successor.owner = SubagentOwner::goal("goal-epoch", 1);
    let successor_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(successor).await }
    });
    assert_eq!(harness.requests.recv().await.unwrap().id, "goal-successor");

    let _ = harness.finish.send(());
    assert!(matches!(
        response.await.unwrap(),
        SubagentCancelOutcome::Cancelled
    ));
    assert!(owned_spawn.await.unwrap().unwrap().cancelled);
    assert!(other_spawn.await.unwrap().unwrap().success);
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("goal-successor")
    );
    assert!(successor_spawn.await.unwrap().unwrap().success);
    harness.actor.abort();
}

/// Spawn an await-to-completion child under `session` and consume its request
/// event, returning the join handle for the in-flight spawn.
async fn spawn_session_child(
    harness: &mut Harness,
    id: &str,
    session: &str,
) -> tokio::task::JoinHandle<Result<SubagentResult, tool_runtime::ToolError>> {
    let mut req = request(id, false);
    req.await_to_completion = true;
    req.parent_session_id = session.to_owned();
    let backend = harness.backend.clone();
    let handle = tokio::spawn(async move { backend.spawn(req).await });
    assert_eq!(
        harness
            .requests
            .recv()
            .await
            .as_ref()
            .map(|r| r.id.as_str()),
        Some(id)
    );
    handle
}

#[tokio::test]
async fn teardown_session_children_spares_other_sessions() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));

    // Two children under "parent" (one active, one pending) plus one under a
    // different session that must survive.
    let keep = spawn_session_child(&mut harness, "keep-active", "other").await;
    let kill_active = spawn_session_child(&mut harness, "kill-active", "parent").await;

    // Start the children spawned so far; kill-pending subscribes after start, so
    // it never receives it and stays pending.
    let _ = harness.start.send(());
    let mut started = std::collections::HashSet::new();
    started.insert(harness.started.recv().await.unwrap());
    started.insert(harness.started.recv().await.unwrap());
    assert!(started.contains("keep-active") && started.contains("kill-active"));

    let kill_pending = spawn_session_child(&mut harness, "kill-pending", "parent").await;

    harness
        .backend
        .sender()
        .send(SubagentEvent::TeardownSession {
            parent_session_id: "parent".to_owned(),
        })
        .expect("actor command channel open");

    assert!(kill_active.await.unwrap().unwrap().cancelled);
    assert!(kill_pending.await.unwrap().unwrap().cancelled);

    assert!(
        !keep.is_finished(),
        "a different session's child must not be cancelled"
    );
    let _ = harness.finish.send(());
    let keep = keep.await.unwrap().unwrap();
    assert!(keep.success && !keep.cancelled);
    harness.actor.abort();
}

#[tokio::test]
async fn teardown_cancels_background_child() {
    let mut harness = harness_with_config(true, CoordinatorConfig::default());

    // A background subagent that outlives its parent must be cancelled during
    // teardown; notification persistence is owned by the shell, not this actor.
    let mut req = request("bg", true);
    req.parent_session_id = "parent".to_owned();
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(req).await }
    });
    assert_eq!(
        harness
            .requests
            .recv()
            .await
            .as_ref()
            .map(|r| r.id.as_str()),
        Some("bg")
    );
    let _ = harness.start.send(());
    assert_eq!(harness.started.recv().await.as_deref(), Some("bg"));

    harness
        .backend
        .sender()
        .send(SubagentEvent::TeardownSession {
            parent_session_id: "parent".to_owned(),
        })
        .expect("actor command channel open");

    // Wait for the cancelled child to finish.
    let _ = harness.completions.recv().await;
    assert!(spawn.await.unwrap().unwrap().cancelled);
    harness.actor.abort();
}

#[tokio::test]
async fn teardown_rejects_spawn_from_cancelled_parent() {
    // wait_after_cancel keeps the cancelled parent in `active`, so its late
    // nested Spawn still finds it.
    let mut harness = harness_with_options(true, true, CoordinatorConfig::default());

    // A parent subagent whose child_session_id is "A".
    let parent = spawn_session_child(&mut harness, "A", "parent").await;
    let _ = harness.start.send(());
    assert_eq!(harness.started.recv().await.as_deref(), Some("A"));

    harness
        .backend
        .sender()
        .send(SubagentEvent::TeardownSession {
            parent_session_id: "parent".to_owned(),
        })
        .expect("actor command channel open");

    // A nested Spawn from the now-cancelled parent (parent_session_id = its
    // child_session_id) must be rejected, not reparented and left running.
    let mut nested = request("B", false);
    nested.await_to_completion = true;
    nested.parent_session_id = "A".to_owned();
    let outcome = harness.backend.spawn(nested).await.unwrap();
    assert!(outcome.cancelled && !outcome.success);

    let _ = harness.finish.send(());
    let _ = parent.await;
    harness.actor.abort();
}

#[tokio::test]
async fn usage_events_feed_sorted_outstanding_reply() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let mut spawns = Vec::new();
    for (id, is_background) in [
        ("z-foreground", false),
        ("a-foreground", false),
        ("background", true),
    ] {
        spawns.push(tokio::spawn({
            let backend = harness.backend.clone();
            async move { backend.spawn(request(id, is_background)).await }
        }));
        assert_eq!(
            harness
                .requests
                .recv()
                .await
                .as_ref()
                .map(|request| request.id.as_str()),
            Some(id)
        );
    }

    let (foreign_respond_to, foreign_response_rx) = oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::MarkUsageNotApplied(
            SubagentMarkUsageNotAppliedRequest {
                parent_session_id: "foreign".to_owned(),
                prompt_id: "prompt".to_owned(),
                respond_to: foreign_respond_to,
            },
        ))
        .expect("actor command channel open");
    foreign_response_rx.await.expect("mark acknowledgement");
    assert!(
        !outstanding(&harness.backend, "prompt")
            .await
            .subagent_usage_not_applied
    );

    let (mark_respond_to, mark_response_rx) = oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::MarkUsageNotApplied(
            SubagentMarkUsageNotAppliedRequest {
                parent_session_id: "parent".to_owned(),
                prompt_id: "prompt".to_owned(),
                respond_to: mark_respond_to,
            },
        ))
        .expect("actor command channel open");
    mark_response_rx.await.expect("mark acknowledgement");
    assert_eq!(
        outstanding(&harness.backend, "prompt").await,
        SubagentOutstandingReply {
            live_ids: vec!["a-foreground".to_owned(), "z-foreground".to_owned()],
            background_live: true,
            subagent_usage_not_applied: true,
        }
    );

    harness
        .backend
        .sender()
        .send(SubagentEvent::ClearUsageNotApplied(
            SubagentClearUsageNotAppliedRequest {
                parent_session_id: "parent".to_owned(),
                prompt_id: "prompt".to_owned(),
            },
        ))
        .expect("actor command channel open");
    assert_eq!(
        outstanding(&harness.backend, "prompt").await,
        SubagentOutstandingReply {
            live_ids: vec!["a-foreground".to_owned(), "z-foreground".to_owned()],
            background_live: true,
            subagent_usage_not_applied: false,
        }
    );

    assert!(matches!(
        harness.backend.cancel_parent_prompt("prompt").await,
        SubagentCancelOutcome::Cancelled
    ));
    for spawn in spawns {
        assert!(spawn.await.unwrap().unwrap().cancelled);
    }
    harness.actor.abort();
}

#[tokio::test]
async fn cancel_parent_prompt_rejects_late_spawn_without_starting_runner() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let bound = parent_backend(&harness);

    assert!(matches!(
        bound.cancel_parent_prompt("prompt").await,
        SubagentCancelOutcome::Cancelled
    ));

    let late = harness
        .backend
        .spawn(request("late-after-prompt-cancel", true))
        .await
        .unwrap();
    assert!(late.cancelled && !late.success);
    assert!(harness.requests.try_recv().is_err());
    assert!(harness.started.try_recv().is_err());
    harness.actor.abort();
}

/// Prior-turn background + current-turn children all die on ParentSession cancel (GBT-4942).
#[tokio::test]
async fn cancel_parent_session_kills_prior_turn_background() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let mut prior = request("prior-bg", true);
    prior.parent_prompt_id = Some("turn-1".into());
    let mut current = request("current", false);
    current.parent_prompt_id = Some("turn-2".into());
    let mut spawns = Vec::new();
    for req in [prior, current] {
        let id = req.id.clone();
        spawns.push(tokio::spawn({
            let backend = harness.backend.clone();
            async move { backend.spawn(req).await }
        }));
        assert_eq!(
            harness
                .requests
                .recv()
                .await
                .as_ref()
                .map(|r| r.id.as_str()),
            Some(id.as_str())
        );
        let _ = harness.start.send(());
        assert_eq!(harness.started.recv().await.as_deref(), Some(id.as_str()));
    }
    assert!(matches!(
        parent_backend(&harness).cancel_parent_session().await,
        SubagentCancelOutcome::Cancelled
    ));
    for spawn in spawns {
        assert!(
            spawn.await.unwrap().unwrap().cancelled,
            "ParentSession cancel must kill prior-turn and current-turn children"
        );
    }
    harness.actor.abort();
}

/// ParentSession cancellation waits for the exact non-workflow snapshot, but
/// must ACK while an unrelated workflow child remains pending/active.
#[tokio::test]
async fn cancel_parent_session_ack_ignores_pending_workflow_child() {
    let mut harness = harness_with_options(
        true,
        true,
        CoordinatorConfig {
            foreground_budget: std::time::Duration::from_secs(60),
        },
    );

    let non_workflow = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("session-cancel", true)).await }
    });
    let mut workflow_request = request("workflow-survivor", true);
    workflow_request.owner = SubagentOwner::workflow("workflow-run");
    let workflow = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(workflow_request).await }
    });

    let mut observed = vec![
        harness.requests.recv().await.unwrap().id,
        harness.requests.recv().await.unwrap().id,
    ];
    observed.sort();
    assert_eq!(observed, vec!["session-cancel", "workflow-survivor"]);

    // The workflow child is still waiting for start. The session child is
    // cancelled and waits for the finish signal, so this signal releases only
    // the non-workflow snapshot. If workflow IDs leaked into the waiter, the
    // timeout below would fire because workflow has no finish receiver yet.
    let cancel = tokio::spawn({
        let backend = parent_backend(&harness);
        async move { backend.cancel_parent_session().await }
    });
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    let _ = harness.finish.send(());
    let outcome = tokio::time::timeout(std::time::Duration::from_millis(500), cancel)
        .await
        .expect("workflow child must not block ParentSession ACK")
        .unwrap();
    assert!(matches!(outcome, SubagentCancelOutcome::Cancelled));
    assert!(non_workflow.await.unwrap().unwrap().cancelled);

    // The workflow child was not cancelled; allow it to start and finish so
    // the coordinator has no orphaned test future.
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("workflow-survivor")
    );
    let _ = harness.finish.send(());
    assert!(workflow.await.unwrap().unwrap().success);
    harness.actor.abort();
}

/// Another session's children must not die when this session Stop fires.
#[tokio::test]
async fn cancel_parent_session_does_not_touch_other_session() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let mut foreign = request("foreign-child", true);
    foreign.parent_session_id = "other-session".into();
    let foreign_spawn = tokio::spawn({
        let backend = ChannelBackend::for_session(harness.backend.sender(), "other-session");
        async move { backend.spawn(foreign).await }
    });
    assert_eq!(
        harness
            .requests
            .recv()
            .await
            .as_ref()
            .map(|r| r.id.as_str()),
        Some("foreign-child")
    );
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("foreign-child")
    );

    assert!(matches!(
        parent_backend(&harness).cancel_parent_session().await,
        SubagentCancelOutcome::Cancelled
    ));
    // Foreign child still running — finish it successfully.
    let _ = harness.finish.send(());
    let result = foreign_spawn.await.unwrap().unwrap();
    assert!(result.success && !result.cancelled);
    harness.actor.abort();
}

/// Unbound backend must not wildcard-cancel (rejects before send).
#[tokio::test]
async fn cancel_parent_session_unbound_backend_is_not_found() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let unbound = ChannelBackend::new(tx);
    assert!(matches!(
        unbound.cancel_parent_session().await,
        SubagentCancelOutcome::NotFound
    ));
}

/// Late Task spawn after ParentSession cancel is rejected until admission reopens.
#[tokio::test]
async fn cancel_parent_session_rejects_late_spawn_until_admission_reopens() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let bound = parent_backend(&harness);
    let prior = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("prior", true)).await }
    });
    assert_eq!(
        harness
            .requests
            .recv()
            .await
            .as_ref()
            .map(|r| r.id.as_str()),
        Some("prior")
    );
    let _ = harness.start.send(());
    assert_eq!(harness.started.recv().await.as_deref(), Some("prior"));

    assert!(matches!(
        bound.cancel_parent_session().await,
        SubagentCancelOutcome::Cancelled
    ));
    assert!(prior.await.unwrap().unwrap().cancelled);

    // Late Task spawn is rejected by the coordinator gate (request still carries
    // parent="parent" via unbound backend + request default).
    let late = harness
        .backend
        .spawn(request("late-after-stop", true))
        .await
        .unwrap();
    assert!(
        late.cancelled && !late.success,
        "late Task spawn after ParentSession must be rejected"
    );

    assert!(bound.open_spawn_admission());
    let allowed = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("after-reopen", true)).await }
    });
    assert_eq!(
        harness
            .requests
            .recv()
            .await
            .as_ref()
            .map(|r| r.id.as_str()),
        Some("after-reopen")
    );
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("after-reopen")
    );
    let _ = harness.finish.send(());
    assert!(allowed.await.unwrap().unwrap().success);
    harness.actor.abort();
}

/// Nested children of a workflow subagent keep workflow ownership after reparent
/// and survive ParentSession cancel (active + pending).
#[tokio::test]
async fn cancel_parent_session_spares_nested_workflow_children() {
    // wait_before_start only: keep one child in pending through ParentSession.
    // (wait_after_cancel not needed — workflow lineage is not cancelled.)
    let mut harness = harness(true, std::time::Duration::from_secs(60));

    // Workflow-owned parent child (child_session_id = "wf-child").
    let mut wf_parent = request("wf-child", true);
    wf_parent.owner = SubagentOwner::workflow("run-1");
    let wf_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(wf_parent).await }
    });
    assert_eq!(
        harness
            .requests
            .recv()
            .await
            .as_ref()
            .map(|r| r.id.as_str()),
        Some("wf-child")
    );
    let _ = harness.start.send(());
    assert_eq!(harness.started.recv().await.as_deref(), Some("wf-child"));

    // Nested spawns use a backend bound to the workflow child's session id
    // (production binds ChannelBackend::for_session to the child session).
    let child_backend = ChannelBackend::for_session(harness.backend.sender(), "wf-child");

    // Nested Task-owned spawn from the workflow child (reparented to root parent).
    let nested_active = request("nested-active", true);
    let nested_active_spawn = tokio::spawn({
        let backend = child_backend.clone();
        async move { backend.spawn(nested_active).await }
    });
    let observed = harness
        .requests
        .recv()
        .await
        .expect("nested active observed");
    assert_eq!(observed.parent_session_id, "parent");
    assert_eq!(
        observed.owner.workflow_run_id(),
        Some("run-1"),
        "reparent must copy workflow lineage"
    );
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("nested-active")
    );

    // Nested pending (not started yet) under the same workflow child.
    let nested_pending = request("nested-pending", true);
    let nested_pending_spawn = tokio::spawn({
        let backend = child_backend.clone();
        async move { backend.spawn(nested_pending).await }
    });
    let observed_pending = harness
        .requests
        .recv()
        .await
        .expect("nested pending observed");
    assert_eq!(observed_pending.owner.workflow_run_id(), Some("run-1"));

    assert!(matches!(
        parent_backend(&harness).cancel_parent_session().await,
        SubagentCancelOutcome::Cancelled
    ));

    // Promote pending → active, then finish all three (must wait until each is
    // subscribed on finish; broadcast does not buffer for late receivers).
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("nested-pending")
    );
    let _ = harness.finish.send(());
    assert!(
        nested_active_spawn.await.unwrap().unwrap().success,
        "active nested workflow child must survive ParentSession"
    );
    assert!(
        nested_pending_spawn.await.unwrap().unwrap().success,
        "pending nested workflow child must survive ParentSession"
    );
    assert!(
        wf_spawn.await.unwrap().unwrap().success,
        "workflow parent must survive ParentSession"
    );
    harness.actor.abort();
}

#[tokio::test]
async fn nested_spawn_keeps_immediate_security_parent_after_lifecycle_reparent() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let outer_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("outer-security-parent", true)).await }
    });
    let observed_outer = harness.requests.recv().await.unwrap();
    assert_eq!(observed_outer.parent_session_id, "parent");
    assert_eq!(harness.security_parents.recv().await.unwrap(), None);
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("outer-security-parent")
    );

    let nested_spawn = tokio::spawn({
        let backend =
            ChannelBackend::for_session(harness.backend.sender(), "outer-security-parent");
        async move { backend.spawn(request("nested-security-parent", true)).await }
    });
    let observed_nested = harness.requests.recv().await.unwrap();
    assert_eq!(
        observed_nested.parent_session_id, "parent",
        "lifecycle ownership remains reparented to the root session"
    );
    assert_eq!(
        harness.security_parents.recv().await.unwrap().as_deref(),
        Some("outer-security-parent"),
        "security policy must be inherited from the immediate spawning child"
    );

    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("nested-security-parent")
    );
    let _ = harness.finish.send(());
    assert!(outer_spawn.await.unwrap().unwrap().success);
    assert!(nested_spawn.await.unwrap().unwrap().success);
    harness.actor.abort();
}

#[tokio::test]
async fn nested_child_can_query_and_cancel_its_immediate_child() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let outer_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("outer", true)).await }
    });
    assert_eq!(harness.requests.recv().await.unwrap().id, "outer");
    let _ = harness.start.send(());
    assert_eq!(harness.started.recv().await.as_deref(), Some("outer"));

    let child_backend = ChannelBackend::for_session(harness.backend.sender(), "outer");
    let nested_spawn = tokio::spawn({
        let backend = child_backend.clone();
        async move { backend.spawn(request("nested", true)).await }
    });
    assert_eq!(harness.requests.recv().await.unwrap().id, "nested");
    let _ = harness.start.send(());
    assert_eq!(harness.started.recv().await.as_deref(), Some("nested"));

    let snapshot = child_backend
        .query("nested", false, None)
        .await
        .unwrap()
        .expect("immediate parent can query nested child");
    assert!(snapshot.is_running());
    assert!(matches!(
        child_backend.cancel("nested").await,
        Ok(SubagentCancelOutcome::Cancelled)
    ));
    assert!(nested_spawn.await.unwrap().unwrap().cancelled);

    let _ = harness.finish.send(());
    assert!(outer_spawn.await.unwrap().unwrap().success);
    harness.actor.abort();
}

#[tokio::test]
async fn nested_parent_session_stop_cancels_its_immediate_children() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let outer_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("outer-stop-owner", true)).await }
    });
    assert_eq!(
        harness.requests.recv().await.unwrap().id,
        "outer-stop-owner"
    );
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("outer-stop-owner")
    );

    let child_backend = ChannelBackend::for_session(harness.backend.sender(), "outer-stop-owner");
    let nested_spawn = tokio::spawn({
        let backend = child_backend.clone();
        async move { backend.spawn(request("nested-stop-target", true)).await }
    });
    assert_eq!(
        harness.requests.recv().await.unwrap().id,
        "nested-stop-target"
    );
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("nested-stop-target")
    );

    assert!(matches!(
        child_backend.cancel_parent_session().await,
        SubagentCancelOutcome::Cancelled
    ));
    assert!(nested_spawn.await.unwrap().unwrap().cancelled);

    let _ = harness.finish.send(());
    assert!(outer_spawn.await.unwrap().unwrap().success);
    harness.actor.abort();
}

#[tokio::test]
async fn nested_parent_session_stop_rejects_late_reparented_spawn() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let outer_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("outer-late-owner", true)).await }
    });
    assert_eq!(
        harness.requests.recv().await.unwrap().id,
        "outer-late-owner"
    );
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("outer-late-owner")
    );

    let child_backend = ChannelBackend::for_session(harness.backend.sender(), "outer-late-owner");
    assert!(matches!(
        child_backend.cancel_parent_session().await,
        SubagentCancelOutcome::Cancelled
    ));

    let late = child_backend
        .spawn(request("nested-after-stop", true))
        .await
        .unwrap();
    assert!(late.cancelled && !late.success);
    assert!(
        harness.requests.try_recv().is_err(),
        "a spawn rejected by the immediate-parent gate must not reach the runner"
    );

    let _ = harness.finish.send(());
    assert!(outer_spawn.await.unwrap().unwrap().success);
    harness.actor.abort();
}

#[tokio::test]
async fn loop_tracking_covers_pending_active_and_nested_reparenting() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let mut outer_request = request("outer", true);
    outer_request.runtime_overrides.loop_task_id = Some("loop-task".to_owned());
    let outer_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(outer_request).await }
    });
    let observed_outer = harness.requests.recv().await.unwrap();
    assert_eq!(observed_outer.parent_session_id, "parent");
    assert!(loop_unit_active(&harness.backend, "loop-task").await);

    let _ = harness.start.send(());
    assert_eq!(harness.started.recv().await.as_deref(), Some("outer"));
    let refs = harness
        .backend
        .spawned_refs_for_prompt("parent", "prompt")
        .await;
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].description, "test child");

    let mut nested_request = request("nested", true);
    nested_request.parent_session_id = "outer".to_owned();
    let nested_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(nested_request).await }
    });
    let observed_nested = harness.requests.recv().await.unwrap();
    assert_eq!(observed_nested.parent_session_id, "parent");
    assert!(observed_nested.surface_completion);
    assert_eq!(
        observed_nested.runtime_overrides.loop_task_id.as_deref(),
        Some("loop-task")
    );
    assert!(loop_unit_active(&harness.backend, "loop-task").await);

    let _ = harness.start.send(());
    assert_eq!(harness.started.recv().await.as_deref(), Some("nested"));
    let _ = harness.finish.send(());
    assert!(outer_spawn.await.unwrap().unwrap().success);
    assert!(nested_spawn.await.unwrap().unwrap().success);
    assert!(!loop_unit_active(&harness.backend, "loop-task").await);
    harness.actor.abort();
}

#[tokio::test]
async fn workflow_nested_task_keeps_completion_internal_after_reparenting() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let mut workflow_request = request("workflow-parent", true);
    workflow_request.owner = SubagentOwner::workflow("workflow-run");
    let workflow_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(workflow_request).await }
    });
    assert_eq!(harness.requests.recv().await.unwrap().id, "workflow-parent");
    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("workflow-parent")
    );

    let mut nested_request = request("workflow-nested", true);
    nested_request.parent_session_id = "workflow-parent".to_owned();
    let nested_spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(nested_request).await }
    });
    let observed_nested = harness.requests.recv().await.unwrap();
    assert_eq!(observed_nested.id, "workflow-nested");
    assert!(!observed_nested.surface_completion);
    assert_eq!(
        observed_nested.owner,
        SubagentOwner::workflow("workflow-run")
    );

    let _ = harness.start.send(());
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("workflow-nested")
    );
    let _ = harness.finish.send(());
    assert!(workflow_spawn.await.unwrap().unwrap().success);
    assert!(nested_spawn.await.unwrap().unwrap().success);
    harness.actor.abort();
}

#[tokio::test]
async fn missing_persisted_output_is_typed_as_unavailable() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("missing-output", true)).await }
    });
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("missing-output")
    );
    let _ = harness.finish.send(());
    assert!(spawn.await.unwrap().unwrap().success);
    let _ = harness.completions.recv().await;

    let snapshot = harness
        .backend
        .query("missing-output", false, None)
        .await
        .unwrap()
        .expect("terminal metadata remains valid without output bytes");
    assert!(matches!(
        snapshot.status,
        SubagentSnapshotStatus::CompletedOutputUnavailable { .. }
    ));
    let inspection = harness
        .backend
        .inspect("missing-output")
        .await
        .unwrap()
        .expect("inspection identity remains valid without output bytes");
    assert!(matches!(
        inspection.snapshot.status,
        SubagentSnapshotStatus::CompletedOutputUnavailable { .. }
    ));
    harness.actor.abort();
}

/// Regression (review): an agent definition with `background: true` spawned
/// with a BLOCKING tool call (`run_in_background: false`) is background for
/// Outstanding/freeze accounting — not turn-blocking — while the spawn caller
/// still receives the result inline.
#[tokio::test]
async fn definition_background_counts_as_background_for_outstanding() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let mut blocking_request = request("bg-def", false);
    blocking_request.subagent_type = "background-default".to_owned();
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(blocking_request).await }
    });
    assert_eq!(harness.started.recv().await.as_deref(), Some("bg-def"));

    // Started with definition background: live for the child itself but not
    // turn-blocking; the prompt sees it as background work.
    assert_eq!(
        outstanding(&harness.backend, "prompt").await,
        SubagentOutstandingReply {
            live_ids: Vec::new(),
            background_live: true,
            subagent_usage_not_applied: false,
        }
    );

    // The blocking caller still gets the completed result inline.
    let _ = harness.finish.send(());
    let result = spawn.await.unwrap().unwrap();
    assert!(result.success);
    assert!(!result.backgrounded);
    harness.actor.abort();
}

#[tokio::test]
async fn session_backend_cannot_query_or_cancel_foreign_child() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("scoped", true)).await }
    });
    assert_eq!(harness.started.recv().await.as_deref(), Some("scoped"));

    let foreign = ChannelBackend::for_session(harness.backend.sender(), "foreign-parent");
    assert!(matches!(
        foreign.query("scoped", false, None).await,
        Err(SubagentObservationError::Unauthorized)
    ));
    assert!(matches!(
        foreign.inspect("scoped").await,
        Err(SubagentObservationError::Unauthorized)
    ));
    assert!(matches!(
        foreign.cancel("scoped").await,
        Err(SubagentObservationError::Unauthorized)
    ));
    // A caller without source access receives the same classification for
    // unknown IDs; denial must not reveal another session's registration.
    assert!(matches!(
        foreign.query("absent", false, None).await,
        Err(SubagentObservationError::Unauthorized)
    ));
    assert!(matches!(
        foreign.inspect("absent").await,
        Err(SubagentObservationError::Unauthorized)
    ));
    assert!(matches!(
        foreign.cancel("absent").await,
        Err(SubagentObservationError::Unauthorized)
    ));
    assert!(harness.backend.inspect("absent").await.unwrap().is_none());

    assert!(matches!(
        harness.backend.cancel("scoped").await,
        Ok(SubagentCancelOutcome::Cancelled)
    ));
    assert!(spawn.await.unwrap().unwrap().cancelled);
    let _ = harness.completions.recv().await;
    harness.actor.abort();
}

#[tokio::test]
async fn completed_cache_evicts_oldest_entry_at_cap() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    for index in 0..=MAX_COMPLETED_ENTRIES {
        let id = format!("cache-{index:04}");
        let spawn = tokio::spawn({
            let backend = harness.backend.clone();
            let request = request(&id, true);
            async move { backend.spawn(request).await }
        });
        assert_eq!(harness.started.recv().await.as_deref(), Some(id.as_str()));
        let _ = harness.finish.send(());
        assert!(spawn.await.unwrap().unwrap().success);
    }

    assert!(
        harness
            .backend
            .query("cache-0000", false, None)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        harness
            .backend
            .query("cache-0001", false, None)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        harness
            .backend
            .query(&format!("cache-{MAX_COMPLETED_ENTRIES:04}"), false, None,)
            .await
            .unwrap()
            .is_some()
    );
    harness.actor.abort();
}

#[tokio::test]
async fn interactions_use_direct_lineage_and_do_not_block_foreground_spawns() {
    use super::super::interaction::AgentInteraction;
    let mut h = harness(false, std::time::Duration::from_secs(60));
    let parent = parent_backend(&h);
    let spawn_backend = parent.clone();
    let spawn = tokio::spawn(async move { spawn_backend.spawn(request("child", false)).await });
    assert_eq!(h.started.recv().await.as_deref(), Some("child"));
    let child = ChannelBackend::for_session(h.backend.sender(), "child");
    let nested_backend = child.clone();
    let mut nested_request = request("nested", true);
    nested_request.description.clear();
    let nested = tokio::spawn(async move { nested_backend.spawn(nested_request).await });
    assert_eq!(h.started.recv().await.as_deref(), Some("nested"));
    let nested_agent = ChannelBackend::for_session(h.backend.sender(), "nested");
    for (sender, target, expected) in [
        (&parent, Some("child"), "parent->child:test child"),
        (&child, None, "child->parent:test child"),
        (&nested_agent, None, "nested->child:nested"),
        (&child, Some("nested"), "child->nested:nested"),
    ] {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            sender.interact(
                "ask".into(),
                target.map(str::to_owned),
                AgentInteraction::Ask {
                    question: "clarify".into(),
                },
                CancellationToken::new(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.answer.as_deref(), Some(expected));
    }
    let reply = child
        .interact(
            "reply".into(),
            None,
            AgentInteraction::Send {
                message: "follow-up".into(),
                interrupt: false,
                reply_to: Some(sampling_types::AgentMessageRef {
                    source_session_id: "parent".into(),
                    message_id: "parent-call".into(),
                }),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(reply.answer.as_deref(), Some("child->parent:test child"));
    assert!(
        !spawn.is_finished(),
        "sideband routing must not need child completion"
    );
    for (sender, target) in [
        (&parent, Some("nested")),
        (&nested_agent, Some("child")),
        (&parent, None),
        (&child, Some("unknown")),
    ] {
        assert!(
            sender
                .interact(
                    "bad".into(),
                    target.map(str::to_owned),
                    AgentInteraction::Ask {
                        question: "no".into()
                    },
                    CancellationToken::new()
                )
                .await
                .is_err()
        );
    }
    let upward = child
        .interact(
            "upward".into(),
            None,
            AgentInteraction::Send {
                message: "no".into(),
                interrupt: true,
                reply_to: None,
            },
            CancellationToken::new(),
        )
        .await
        .expect("preflight rejection is a typed send outcome");
    assert_eq!(upward.status, "rejected");
    assert_eq!(
        upward.error.as_ref().map(|error| error.code.as_str()),
        Some("no_direct_route")
    );
    assert!(
        h.backend
            .interact(
                "unbound".into(),
                Some("child".into()),
                AgentInteraction::Ask {
                    question: "no".into()
                },
                CancellationToken::new()
            )
            .await
            .is_err()
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(
        parent
            .interact(
                "cancel".into(),
                Some("child".into()),
                AgentInteraction::Ask {
                    question: "no".into()
                },
                cancelled
            )
            .await
            .is_err()
    );
    parent
        .cancel("child")
        .await
        .expect("parent can request child cancellation");
    assert!(
        parent
            .interact(
                "closed".into(),
                Some("child".into()),
                AgentInteraction::Ask {
                    question: "no".into()
                },
                CancellationToken::new()
            )
            .await
            .is_err()
    );
    spawn.abort();
    nested.abort();
    h.actor.abort();
}

#[tokio::test]
async fn send_keeps_ack_receiver_alive_when_cancellation_is_ready() {
    use super::super::interaction::{AgentInteraction, AgentInteractionOutput};

    let (tx, mut events) = mpsc::unbounded_channel();
    let backend = ChannelBackend::for_session(tx, "parent");
    let cancellation = CancellationToken::new();
    let call = tokio::spawn({
        let backend = backend.clone();
        let cancellation = cancellation.clone();
        async move {
            backend
                .interact(
                    "send".into(),
                    Some("child".into()),
                    AgentInteraction::Send {
                        message: "message".into(),
                        interrupt: false,
                        reply_to: None,
                    },
                    cancellation,
                )
                .await
        }
    });

    let Some(SubagentEvent::Interact(request)) =
        tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
            .await
            .expect("bound send reaches coordinator")
    else {
        panic!("send should reach the coordinator before waiting for its ACK");
    };
    cancellation.cancel();
    request
        .respond_to
        .send(Ok(AgentInteractionOutput::message_received(
            "send",
            "receipt-1",
        )))
        .expect("caller should still be waiting for the send ACK");

    let output = call
        .await
        .expect("send task should finish")
        .expect("a ready durable ACK wins cancellation");
    assert_eq!(output.status, "received");
    assert_eq!(output.receipt_id.as_deref(), Some("receipt-1"));
}

#[tokio::test]
async fn completed_direct_send_uses_readonly_receipt_route() {
    let mut h = harness_with_options(
        false,
        true,
        CoordinatorConfig {
            foreground_budget: std::time::Duration::from_secs(60),
        },
    );
    let parent = parent_backend(&h);
    let spawn = tokio::spawn({
        let backend = parent.clone();
        async move { backend.spawn(request("child", true)).await }
    });
    assert_eq!(h.started.recv().await.as_deref(), Some("child"));

    assert!(matches!(
        parent.cancel("child").await,
        Ok(SubagentCancelOutcome::Cancelled)
    ));
    let _ = h.finish.send(());
    tokio::time::timeout(std::time::Duration::from_secs(1), spawn)
        .await
        .expect("cancelled child should finish")
        .expect("spawn task should join")
        .expect("child result should be delivered");

    let retry = parent
        .interact(
            "retry-old-send".into(),
            Some("child".into()),
            super::super::interaction::AgentInteraction::Send {
                message: "same operation".into(),
                interrupt: true,
                reply_to: None,
            },
            CancellationToken::new(),
        )
        .await
        .expect("known completed child should allow receipt-only lookup");
    assert_eq!(retry.status, "receipt_only");
    h.actor.abort();
}

#[tokio::test]
async fn completed_reply_uses_readonly_receipt_route() {
    use super::super::interaction::AgentInteraction;

    let mut h = harness(false, std::time::Duration::from_secs(60));
    let parent = parent_backend(&h);
    let spawn = tokio::spawn({
        let backend = parent.clone();
        async move { backend.spawn(request("child", true)).await }
    });
    assert_eq!(h.started.recv().await.as_deref(), Some("child"));
    let _ = h.finish.send(());
    tokio::time::timeout(std::time::Duration::from_secs(1), spawn)
        .await
        .expect("completed child should finish")
        .expect("spawn task should join")
        .expect("child result should be delivered");

    let child = ChannelBackend::for_session(h.backend.sender(), "child");
    let reply = child
        .interact(
            "reply-after-complete".into(),
            None,
            AgentInteraction::Send {
                message: "late opinion".into(),
                interrupt: false,
                reply_to: Some(sampling_types::AgentMessageRef {
                    source_session_id: "parent".into(),
                    message_id: "parent-call".into(),
                }),
            },
            CancellationToken::new(),
        )
        .await
        .expect("known completed child should allow receipt-only lookup");
    assert_eq!(reply.status, "receipt_only");
    h.actor.abort();
}

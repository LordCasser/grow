use super::*;
use std::future::Future;
use std::time::Duration;
use tool_types::planning::{
    LocalPlan, PlanDelegation, PlanLink, PlanPriority, PlanStep, PlanStepStatus,
    PlanningReadSnapshot,
};

type SpawnCall = tokio::task::JoinHandle<Result<SubagentResult, tool_runtime::ToolError>>;
type ReadCall =
    tokio::task::JoinHandle<Result<Option<PlanningReadSnapshot>, ParentPlanningReadError>>;

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(2), future)
        .await
        .expect("parent planning test handshake must finish")
}

fn distinct_child(id: &str, parent_step_id: Option<&str>) -> SubagentRequest {
    let mut child = request(id, true);
    child.subagent_type = "parent-plan-distinct-session".into();
    child.parent_step_id = parent_step_id.map(str::to_owned);
    child
}

fn session_backend(harness: &Harness, session_id: &str) -> ChannelBackend {
    ChannelBackend::for_session(harness.backend.sender(), session_id.to_owned())
}

async fn start_child(
    harness: &mut Harness,
    backend: ChannelBackend,
    child: SubagentRequest,
) -> (SpawnCall, SubagentRequest) {
    let id = child.id.clone();
    let spawn = tokio::spawn(async move { backend.spawn(child).await });
    let observed = bounded(harness.requests.recv())
        .await
        .expect("runner request");
    assert_eq!(observed.id, id);
    // The runner publishes this only after reporter.started has been ACKed.
    assert_eq!(bounded(harness.started.recv()).await, Some(id));
    (spawn, observed)
}

async fn forward_read(
    harness: &mut Harness,
    backend: ChannelBackend,
    cancellation: CancellationToken,
) -> (ReadCall, ParentPlanRead) {
    let read = tokio::spawn(async move { backend.read_parent_plan(cancellation).await });
    let forwarded = bounded(harness.parent_plan_reads.recv())
        .await
        .expect("active direct child reaches the host");
    assert!(!read.is_finished(), "host has not replied yet");
    (read, forwarded)
}

async fn stop_actor(actor: tokio::task::JoinHandle<()>) {
    actor.abort();
    assert!(bounded(actor).await.unwrap_err().is_cancelled());
}

async fn finish_children(harness: &mut Harness, spawns: Vec<SpawnCall>) {
    harness
        .finish
        .send(())
        .expect("children are awaiting finish");
    for spawn in spawns {
        assert!(bounded(spawn).await.unwrap().unwrap().success);
        bounded(harness.completions.recv())
            .await
            .expect("normal terminal disposition");
    }
}

fn snapshot(parent: &str, task: &str, child: &str, step: &str) -> PlanningReadSnapshot {
    let mut plan = LocalPlan::new(parent, "actual-parent-scope");
    plan.revision = 7;
    plan.steps.push(PlanStep {
        id: step.into(),
        content: "Read the current direct parent's requirements".into(),
        priority: PlanPriority::High,
        status: PlanStepStatus::InProgress,
        requirements: Some(vec!["Preserve the original plan and execution".into()]),
        removed: false,
    });
    plan.links.push(PlanLink {
        id: task.into(),
        parent_step_id: step.into(),
        child_session_id: child.into(),
        terminal: false,
        required: true,
        pending_source_revision: None,
        review_required_revision: 7,
        result_id: None,
        dispatch_failure: None,
    });
    PlanningReadSnapshot {
        timeline_seq: 29,
        delegation: Some(PlanDelegation {
            parent_session_id: parent.into(),
            parent_scope_id: plan.scope_id.clone(),
            parent_step_id: step.into(),
            task_id: task.into(),
            child_session_id: child.into(),
        }),
        plan,
    }
}

#[test]
fn parent_planning_errors_have_stable_codes_and_error_messages() {
    fn is_error(_: &dyn std::error::Error) {}
    for (error, code) in [
        (ParentPlanningReadError::NoParent, "no_parent"),
        (ParentPlanningReadError::Unavailable, "unavailable"),
        (ParentPlanningReadError::TimedOut, "timed_out"),
        (ParentPlanningReadError::InvalidEvidence, "invalid_evidence"),
    ] {
        is_error(&error);
        assert_eq!(error.code(), code);
        assert!(!error.to_string().is_empty());
    }
}

#[tokio::test]
async fn bound_root_has_no_parent_and_unbound_observer_is_unavailable() {
    let mut harness = harness(false, Duration::from_secs(60));
    let root = session_backend(&harness, "root");
    let counts = bounded(harness.backend.registry_counts()).await;
    assert_eq!(
        bounded(root.read_parent_plan(CancellationToken::new())).await,
        Err(ParentPlanningReadError::NoParent),
    );
    let observer = ChannelBackend::for_authorized_observer(harness.backend.sender())
        .with_history_query(|_| async { panic!("history cannot authorize parent reads") });
    assert_eq!(
        bounded(observer.read_parent_plan(CancellationToken::new())).await,
        Err(ParentPlanningReadError::Unavailable),
    );
    assert_eq!(bounded(harness.backend.registry_counts()).await, counts);
    assert!(harness.parent_plan_reads.try_recv().is_err());
    assert!(harness.requests.try_recv().is_err());
    assert!(harness.completions.try_recv().is_err());
    assert_eq!(harness.interaction_calls.load(Ordering::SeqCst), 0);
    stop_actor(harness.actor).await;
}

#[tokio::test]
async fn nested_child_reads_its_actual_direct_parent_and_preserves_snapshot_and_lifecycle() {
    let mut harness = harness(false, Duration::from_secs(60));
    let root = session_backend(&harness, "root");
    let child_a = distinct_child("parent-plan-a", Some("root-step"));
    let a_session = test_child_session_id(&child_a);
    let a_cancellation = child_a.cancel_token.clone();
    let (spawn_a, observed_a) = start_child(&mut harness, root.clone(), child_a).await;
    assert_eq!(observed_a.parent_session_id, "root");
    let a_backend = session_backend(&harness, &a_session);
    let child_b = distinct_child("parent-plan-b", Some("a-step"));
    let b_session = test_child_session_id(&child_b);
    let b_cancellation = child_b.cancel_token.clone();
    let (spawn_b, observed_b) = start_child(&mut harness, a_backend.clone(), child_b).await;
    assert_eq!(
        observed_b.parent_session_id, "root",
        "lifecycle is reparented"
    );
    assert_ne!(b_session, observed_b.id);
    let b_backend = session_backend(&harness, &b_session);
    let before = bounded(harness.backend.registry_counts()).await;
    assert!(
        bounded(root.query(&observed_b.id, false, None))
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        bounded(root.read_parent_plan(CancellationToken::new())).await,
        Err(ParentPlanningReadError::NoParent),
    );
    let original = snapshot(&a_session, &observed_b.id, &b_session, "a-step");
    for _ in 0..2 {
        let (read, (request, parent, task, step)) =
            forward_read(&mut harness, b_backend.clone(), CancellationToken::new()).await;
        assert_eq!(request.source_session_id, b_session);
        assert_eq!(parent, a_session);
        assert_ne!(parent, "root");
        assert_eq!(task, observed_b.id);
        assert_eq!(step.as_deref(), Some("a-step"));
        request.respond_to.send(Ok(Some(original.clone()))).unwrap();
        assert_eq!(bounded(read).await.unwrap(), Ok(Some(original.clone())));
    }
    let (read, (request, parent, task, step)) =
        forward_read(&mut harness, a_backend, CancellationToken::new()).await;
    assert_eq!(request.source_session_id, a_session);
    assert_eq!(parent, "root");
    assert_eq!(task, observed_a.id);
    assert_eq!(step.as_deref(), Some("root-step"));
    request.respond_to.send(Ok(None)).unwrap();
    assert_eq!(bounded(read).await.unwrap(), Ok(None));
    assert_eq!(bounded(harness.backend.registry_counts()).await, before);
    assert!(!a_cancellation.is_cancelled());
    assert!(!b_cancellation.is_cancelled());
    assert!(harness.requests.try_recv().is_err());
    assert!(harness.started.try_recv().is_err());
    assert!(harness.completions.try_recv().is_err());
    assert!(harness.model_controls.try_recv().is_err());
    assert_eq!(harness.interaction_calls.load(Ordering::SeqCst), 0);
    finish_children(&mut harness, vec![spawn_a, spawn_b]).await;
    stop_actor(harness.actor).await;
}

#[tokio::test]
async fn workflow_direct_edge_can_read_with_no_fabricated_step_binding() {
    let mut harness = harness(false, Duration::from_secs(60));
    let mut child = distinct_child("parent-plan-workflow", None);
    child.owner = SubagentOwner::workflow("workflow-run");
    let source = test_child_session_id(&child);
    let parent = parent_backend(&harness);
    let (spawn, _) = start_child(&mut harness, parent, child).await;
    let backend = session_backend(&harness, &source);
    let (read, (request, parent, task, step)) =
        forward_read(&mut harness, backend, CancellationToken::new()).await;
    assert_eq!(request.source_session_id, source);
    assert_eq!(parent, "parent");
    assert_eq!(task, "parent-plan-workflow");
    assert_eq!(step, None);
    let snapshot = PlanningReadSnapshot {
        timeline_seq: 5,
        plan: LocalPlan::new(parent, "internal-scope"),
        delegation: None,
    };
    request.respond_to.send(Ok(Some(snapshot.clone()))).unwrap();
    assert_eq!(bounded(read).await.unwrap(), Ok(Some(snapshot)));
    assert_eq!(harness.interaction_calls.load(Ordering::SeqCst), 0);
    finish_children(&mut harness, vec![spawn]).await;
    stop_actor(harness.actor).await;
}

#[tokio::test]
async fn pending_and_cancelled_active_sources_stay_unavailable_without_starting_reads() {
    let mut harness = harness_with_options(
        true,
        true,
        CoordinatorConfig {
            foreground_budget: Duration::from_secs(60),
        },
    );
    let child = distinct_child("parent-plan-cancelled", Some("step"));
    let source = test_child_session_id(&child);
    let child_cancellation = child.cancel_token.clone();
    let backend = parent_backend(&harness);
    let spawn = tokio::spawn(async move { backend.spawn(child).await });
    bounded(harness.requests.recv()).await.unwrap();
    let pending_backend = session_backend(&harness, "parent-plan-cancelled");
    assert_eq!(
        bounded(pending_backend.read_parent_plan(CancellationToken::new())).await,
        Err(ParentPlanningReadError::Unavailable),
    );
    assert!(!child_cancellation.is_cancelled());
    assert!(harness.started.try_recv().is_err());
    harness.start.send(()).unwrap();
    assert_eq!(
        bounded(harness.started.recv()).await.as_deref(),
        Some("parent-plan-cancelled"),
    );
    child_cancellation.cancel();
    let active_backend = session_backend(&harness, &source);
    assert_eq!(
        bounded(active_backend.read_parent_plan(CancellationToken::new())).await,
        Err(ParentPlanningReadError::Unavailable),
    );
    assert_eq!(bounded(harness.backend.registry_counts()).await.active, 1);
    assert!(harness.parent_plan_reads.try_recv().is_err());
    assert!(harness.requests.try_recv().is_err());
    harness.finish.send(()).unwrap();
    assert!(bounded(spawn).await.unwrap().unwrap().cancelled);
    stop_actor(harness.actor).await;
}

#[tokio::test]
async fn suspended_source_uses_checkpoint_session_and_does_not_restore_execution() {
    let mut harness = harness(true, Duration::from_secs(60));
    let child = distinct_child("parent-plan-suspended", Some("step"));
    let source = test_child_session_id(&child);
    let child_cancellation = child.cancel_token.clone();
    let backend = parent_backend(&harness);
    let spawn = tokio::spawn(async move { backend.spawn(child).await });
    bounded(harness.requests.recv()).await.unwrap();
    let (respond_to, response) = oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::FreezeAdmission {
            root_session_id: "parent".into(),
            operation_id: "suspend-retirement".into(),
            respond_to,
        })
        .unwrap();
    assert_eq!(bounded(response).await.unwrap().unwrap().len(), 1);
    harness.start.send(()).unwrap();
    bounded(harness.started.recv()).await.unwrap();
    bounded(async {
        loop {
            if harness
                .running_counts
                .recv()
                .await
                .expect("coordinator stays alive")
                == 0
            {
                break;
            }
        }
    })
    .await;
    let before = bounded(harness.backend.registry_counts()).await;
    let backend = session_backend(&harness, &source);
    assert_eq!(
        bounded(backend.read_parent_plan(CancellationToken::new())).await,
        Err(ParentPlanningReadError::Unavailable),
    );
    assert_eq!(bounded(harness.backend.registry_counts()).await, before);
    assert!(!child_cancellation.is_cancelled());
    assert!(!spawn.is_finished());
    assert!(harness.requests.try_recv().is_err());
    assert!(harness.parent_plan_reads.try_recv().is_err());
    spawn.abort();
    assert!(bounded(spawn).await.unwrap_err().is_cancelled());
    stop_actor(harness.actor).await;
}

#[tokio::test(start_paused = true)]
async fn execution_returned_completed_failed_and_finalization_sources_are_unavailable() {
    let mut harness = harness(false, Duration::from_secs(60));
    let backend = parent_backend(&harness);
    let (returned, _) = start_child(
        &mut harness,
        backend.clone(),
        request("execution-returned", true),
    )
    .await;
    let source = session_backend(&harness, "execution-returned");
    assert!(matches!(
        bounded(backend.query("execution-returned", false, None))
            .await
            .unwrap()
            .unwrap()
            .status,
        SubagentSnapshotStatus::FinalizationPending { .. },
    ));
    let before = bounded(harness.backend.registry_counts()).await;
    let reconciliations = harness.reconcile_calls.load(Ordering::SeqCst);
    assert_eq!(reconciliations, 0);
    assert_eq!(
        bounded(source.read_parent_plan(CancellationToken::new())).await,
        Err(ParentPlanningReadError::Unavailable),
    );
    assert_eq!(bounded(harness.backend.registry_counts()).await, before);
    assert_eq!(
        harness.reconcile_calls.load(Ordering::SeqCst),
        reconciliations
    );
    finish_children(&mut harness, vec![returned]).await;
    for (id, committed) in [
        ("parent-plan-completed", true),
        ("failed-status", true),
        ("parent-plan-finalization", false),
    ] {
        harness
            .terminal_commit_ready
            .store(committed, Ordering::SeqCst);
        let child = distinct_child(id, Some("step"));
        let source = test_child_session_id(&child);
        let (spawn, _) = start_child(&mut harness, backend.clone(), child).await;
        harness.finish.send(()).unwrap();
        let result = bounded(spawn).await.unwrap().unwrap();
        assert_eq!(result.child_session_id, source);
        assert_eq!(result.success, id != "failed-status");
        assert_eq!(result.finalization_pending, !committed);
        if committed {
            bounded(harness.completions.recv()).await.unwrap();
        }
        // The uncommitted terminal fixture enables the existing recovery
        // timer's first tick. Establish its mailbox-ordered baseline before
        // attributing any recovery or lifecycle change to this read.
        let before = bounded(harness.backend.registry_counts()).await;
        let reconciliations = harness.reconcile_calls.load(Ordering::SeqCst);
        assert_eq!(reconciliations, usize::from(!committed));
        let source = session_backend(&harness, &source);
        assert_eq!(
            bounded(source.read_parent_plan(CancellationToken::new())).await,
            Err(ParentPlanningReadError::Unavailable),
        );
        assert_eq!(bounded(harness.backend.registry_counts()).await, before);
        assert_eq!(
            harness.reconcile_calls.load(Ordering::SeqCst),
            reconciliations
        );
    }
    assert!(harness.parent_plan_reads.try_recv().is_err());
    assert!(harness.requests.try_recv().is_err());
    assert_eq!(harness.interaction_calls.load(Ordering::SeqCst), 0);
    stop_actor(harness.actor).await;
}

#[tokio::test]
async fn retiring_actual_session_stays_unavailable_after_completed_cache_eviction() {
    let mut harness = harness(false, Duration::from_secs(60));
    let backend = parent_backend(&harness);
    let child = distinct_child("parent-plan-retiring", Some("step"));
    let source = test_child_session_id(&child);
    let (spawn, _) = start_child(&mut harness, backend.clone(), child).await;
    finish_children(&mut harness, vec![spawn]).await;
    for index in 0..MAX_COMPLETED_ENTRIES {
        let child = request(&format!("parent-plan-eviction-{index}"), true);
        let (spawn, _) = start_child(&mut harness, backend.clone(), child).await;
        finish_children(&mut harness, vec![spawn]).await;
    }
    assert!(
        bounded(harness.backend.query("parent-plan-retiring", false, None))
            .await
            .unwrap()
            .is_none()
    );
    let before = bounded(harness.backend.registry_counts()).await;
    assert_eq!(before.completed, MAX_COMPLETED_ENTRIES);
    let source = session_backend(&harness, &source);
    assert_eq!(
        bounded(source.read_parent_plan(CancellationToken::new())).await,
        Err(ParentPlanningReadError::Unavailable),
    );
    assert_eq!(bounded(harness.backend.registry_counts()).await, before);
    assert_eq!(harness.retirement_joins.load(Ordering::SeqCst), 0);
    assert!(harness.parent_plan_reads.try_recv().is_err());
    assert!(harness.requests.try_recv().is_err());
    stop_actor(harness.actor).await;
}

#[tokio::test]
async fn host_read_can_wait_while_other_reads_queries_and_spawn_continue() {
    let mut harness = harness(false, Duration::from_secs(60));
    let parent = parent_backend(&harness);
    let child = distinct_child("parent-plan-held", Some("step"));
    let source = test_child_session_id(&child);
    let cancellation = child.cancel_token.clone();
    let (spawn, _) = start_child(&mut harness, parent.clone(), child).await;
    let source = session_backend(&harness, &source);
    let (held, (held_request, _, _, _)) =
        forward_read(&mut harness, source.clone(), CancellationToken::new()).await;
    let snapshot = bounded(parent.query("parent-plan-held", false, None))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        snapshot.status,
        SubagentSnapshotStatus::Running { .. }
    ));
    let (second, (request, _, _, _)) =
        forward_read(&mut harness, source, CancellationToken::new()).await;
    request.respond_to.send(Ok(None)).unwrap();
    assert_eq!(bounded(second).await.unwrap(), Ok(None));
    let (sibling, _) = start_child(
        &mut harness,
        parent,
        distinct_child("parent-plan-sibling", None),
    )
    .await;
    assert_eq!(bounded(harness.backend.registry_counts()).await.active, 2);
    assert!(!held.is_finished());
    assert!(!cancellation.is_cancelled());
    held_request.respond_to.send(Ok(None)).unwrap();
    assert_eq!(bounded(held).await.unwrap(), Ok(None));
    assert_eq!(harness.interaction_calls.load(Ordering::SeqCst), 0);
    finish_children(&mut harness, vec![spawn, sibling]).await;
    stop_actor(harness.actor).await;
}

#[tokio::test]
async fn cancelled_and_abandoned_query_requests_do_not_invoke_the_host_or_cancel_child() {
    let mut harness = harness(false, Duration::from_secs(60));
    let parent = parent_backend(&harness);
    let child = distinct_child("parent-plan-abandoned", Some("step"));
    let source = test_child_session_id(&child);
    let child_cancellation = child.cancel_token.clone();
    let (spawn, _) = start_child(&mut harness, parent, child).await;
    let backend = session_backend(&harness, &source);
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
        bounded(backend.read_parent_plan(cancellation.clone())).await,
        Err(ParentPlanningReadError::Unavailable),
    );
    let (respond_to, response) = oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::ReadParentPlanning(ParentPlanningRequest {
            source_session_id: source.clone(),
            cancellation,
            respond_to,
        }))
        .unwrap();
    assert_eq!(
        bounded(response).await.unwrap(),
        Err(ParentPlanningReadError::Unavailable)
    );
    let (respond_to, response) = oneshot::channel();
    drop(response);
    harness
        .backend
        .sender()
        .send(SubagentEvent::ReadParentPlanning(ParentPlanningRequest {
            source_session_id: source,
            cancellation: CancellationToken::new(),
            respond_to,
        }))
        .unwrap();
    assert_eq!(bounded(harness.backend.registry_counts()).await.active, 1);
    assert!(harness.parent_plan_reads.try_recv().is_err());
    assert!(!child_cancellation.is_cancelled());

    let cancellation = CancellationToken::new();
    let (read, (forwarded, _, _, _)) =
        forward_read(&mut harness, backend.clone(), cancellation.clone()).await;
    cancellation.cancel();
    assert_eq!(
        bounded(read).await.unwrap(),
        Err(ParentPlanningReadError::Unavailable)
    );
    assert!(forwarded.cancellation.is_cancelled());
    assert!(forwarded.respond_to.is_closed());
    let (abandoned, (forwarded, _, _, _)) =
        forward_read(&mut harness, backend, CancellationToken::new()).await;
    abandoned.abort();
    assert!(bounded(abandoned).await.unwrap_err().is_cancelled());
    assert!(forwarded.respond_to.is_closed());
    assert!(!forwarded.cancellation.is_cancelled());
    assert!(!child_cancellation.is_cancelled());
    assert!(harness.requests.try_recv().is_err());
    assert!(harness.completions.try_recv().is_err());
    finish_children(&mut harness, vec![spawn]).await;
    stop_actor(harness.actor).await;
}

#[tokio::test(start_paused = true)]
async fn host_timeout_is_five_seconds_and_dropped_reply_remains_unavailable() {
    let mut harness = harness(false, Duration::from_secs(60));
    let parent = parent_backend(&harness);
    let child = distinct_child("parent-plan-timeout", Some("step"));
    let source = test_child_session_id(&child);
    let child_cancellation = child.cancel_token.clone();
    let (spawn, _) = start_child(&mut harness, parent, child).await;
    let backend = session_backend(&harness, &source);
    let began = tokio::time::Instant::now();
    let (read, (forwarded, _, _, _)) =
        forward_read(&mut harness, backend.clone(), CancellationToken::new()).await;
    tokio::time::advance(Duration::from_secs(4)).await;
    assert!(!read.is_finished());
    assert!(!forwarded.respond_to.is_closed());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(
        bounded(read).await.unwrap(),
        Err(ParentPlanningReadError::TimedOut)
    );
    assert_eq!(tokio::time::Instant::now() - began, Duration::from_secs(5));
    assert!(forwarded.respond_to.is_closed());
    assert!(!forwarded.cancellation.is_cancelled());
    assert!(!child_cancellation.is_cancelled());
    let (read, (forwarded, _, _, _)) =
        forward_read(&mut harness, backend.clone(), CancellationToken::new()).await;
    drop(forwarded.respond_to);
    assert_eq!(
        bounded(read).await.unwrap(),
        Err(ParentPlanningReadError::Unavailable)
    );
    for result in [
        Ok(None),
        Err(ParentPlanningReadError::Unavailable),
        Err(ParentPlanningReadError::InvalidEvidence),
        Err(ParentPlanningReadError::TimedOut),
    ] {
        let (read, (forwarded, _, _, _)) =
            forward_read(&mut harness, backend.clone(), CancellationToken::new()).await;
        forwarded.respond_to.send(result.clone()).unwrap();
        assert_eq!(bounded(read).await.unwrap(), result);
    }
    assert_eq!(bounded(harness.backend.registry_counts()).await.active, 1);
    assert!(!child_cancellation.is_cancelled());
    assert!(harness.requests.try_recv().is_err());
    assert!(harness.completions.try_recv().is_err());
    assert_eq!(harness.interaction_calls.load(Ordering::SeqCst), 0);
    finish_children(&mut harness, vec![spawn]).await;
    stop_actor(harness.actor).await;
}

#[tokio::test]
async fn closed_coordinator_channel_is_unavailable() {
    let (sender, receiver) = mpsc::unbounded_channel();
    drop(receiver);
    let backend = ChannelBackend::for_session(sender, "actual-child-session");
    assert_eq!(
        bounded(backend.read_parent_plan(CancellationToken::new())).await,
        Err(ParentPlanningReadError::Unavailable),
    );
}

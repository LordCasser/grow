use super::*;
use crate::implementations::grow_build::task::types::SubagentObservationError;
use tool_types::SubagentListScope;

struct OutputReadControl {
    cancellation: CancellationToken,
}

impl ChildControl for OutputReadControl {
    type SecurityContext = String;

    fn progress_snapshot(&self) -> Option<SubagentProgress> {
        None
    }

    fn security_context(&self) -> Self::SecurityContext {
        "output-reader-child".into()
    }

    fn cancel(&self) {
        self.cancellation.cancel();
    }
}

struct OutputReadRunner {
    finish: tokio::sync::broadcast::Sender<()>,
    started: mpsc::UnboundedSender<String>,
    output_reads_started: mpsc::UnboundedSender<String>,
    output_read_gate: Arc<tokio::sync::Semaphore>,
}

impl ChildRunner for OutputReadRunner {
    type Control = OutputReadControl;
    type CompletionData = String;
    type RunFuture = SendBoxFuture<ChildRunOutcome<String>>;
    type ValidateFuture = SendBoxFuture<SubagentValidateTypeOutcome>;

    fn run(&self, run: ChildRunRequest<Self::Control>) -> Self::RunFuture {
        let finish = self.finish.subscribe();
        let started = self.started.clone();
        Box::pin(async move {
            let ChildRunRequest {
                request,
                cancellation,
                reporter,
                ..
            } = run;
            let id = request.id.clone();
            if !reporter
                .started(StartedChild {
                    child_session_id: id.clone(),
                    resumed_from: request.resume_from.clone(),
                    definition_background: false,
                    control: OutputReadControl {
                        cancellation: cancellation.clone(),
                    },
                })
                .await
            {
                return ChildRunOutcome::Finished(ChildRunOutput {
                    result: cancelled_result(&request),
                    completion_data: id,
                });
            }
            let _ = started.send(id.clone());
            let mut finish = finish;
            tokio::select! {
                _ = cancellation.cancelled() => ChildRunOutcome::Finished(ChildRunOutput {
                    result: cancelled_result(&request),
                    completion_data: id,
                }),
                _ = finish.recv() => ChildRunOutcome::Finished(ChildRunOutput {
                    result: SubagentResult {
                        success: true,
                        output: "child result".into(),
                        subagent_id: id.clone(),
                        child_session_id: id.clone(),
                        ..Default::default()
                    },
                    completion_data: id,
                }),
            }
        })
    }

    fn validate_type(
        &self,
        _subagent_type: String,
        _parent_session_id: String,
    ) -> Self::ValidateFuture {
        Box::pin(std::future::ready(SubagentValidateTypeOutcome::Ok))
    }

    fn on_completed(&self, _completion: ChildCompletion<Self::CompletionData>) {}

    fn persisted_output_ref(&self, _completion_data: &Self::CompletionData) -> Option<String> {
        Some("held-output".into())
    }

    fn load_persisted_output_async(
        &self,
        request: SubagentRequest,
        _child_session_id: String,
        _reference: String,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> SendBoxFuture<Result<Option<Arc<str>>, SubagentObservationError>> {
        let output_reads_started = self.output_reads_started.clone();
        let output_read_gate = self.output_read_gate.clone();
        Box::pin(async move {
            let id = request.id.clone();
            let _ = output_reads_started.send(id.clone());
            let _coordinator_permit = permit;
            if id == "panic-output" {
                let _reader_gate = output_read_gate
                    .acquire_owned()
                    .await
                    .expect("test output gate remains open");
                panic!("injected persisted-output worker panic");
            }
            let gate = output_read_gate
                .acquire_owned()
                .await
                .expect("test output gate remains open");
            drop(gate);
            Ok(Some(Arc::from("persisted child output")))
        })
    }
}

#[tokio::test]
async fn completed_output_readers_are_bounded_and_release_their_permits() {
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (finish, _) = tokio::sync::broadcast::channel(16);
    let (started_tx, mut started) = mpsc::unbounded_channel();
    let (reads_tx, mut reads_started) = mpsc::unbounded_channel();
    let output_read_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let actor = tokio::spawn(
        SubagentCoordinator::new(
            command_rx,
            OutputReadRunner {
                finish: finish.clone(),
                started: started_tx,
                output_reads_started: reads_tx,
                output_read_gate: output_read_gate.clone(),
            },
            CoordinatorConfig::default(),
        )
        .run(),
    );
    let backend = ChannelBackend::for_session(commands, "parent");

    let spawn_calls = (0..9)
        .map(|index| {
            let backend = backend.clone();
            tokio::spawn(async move {
                backend
                    .spawn(request(&format!("held-output-{index}"), true))
                    .await
            })
        })
        .collect::<Vec<_>>();
    let mut started_ids = std::collections::HashSet::new();
    while started_ids.len() < 9 {
        started_ids.insert(
            tokio::time::timeout(std::time::Duration::from_secs(5), started.recv())
                .await
                .expect("all nine children start")
                .expect("runner stays alive"),
        );
    }
    assert_eq!(started_ids.len(), 9);
    finish
        .send(())
        .expect("nine children are awaiting completion");
    for spawn in spawn_calls {
        assert!(
            spawn
                .await
                .expect("spawn task joins")
                .expect("child succeeds")
                .success
        );
    }

    let first_eight = (0..8)
        .map(|index| {
            let backend = backend.clone();
            tokio::spawn(async move {
                backend
                    .query(&format!("held-output-{index}"), false, None)
                    .await
            })
        })
        .collect::<Vec<_>>();
    let mut reading_ids = std::collections::HashSet::new();
    while reading_ids.len() < 8 {
        reading_ids.insert(
            tokio::time::timeout(std::time::Duration::from_secs(5), reads_started.recv())
                .await
                .expect("all eight owned readers start")
                .expect("runner stays alive"),
        );
    }
    assert_eq!(reading_ids.len(), 8);

    assert!(matches!(
        backend.query("held-output-8", false, None).await,
        Err(SubagentObservationError::Unavailable)
    ));
    assert!(
        reads_started.try_recv().is_err(),
        "the ninth request is rejected before a worker starts"
    );

    output_read_gate.add_permits(8);
    for query in first_eight {
        let snapshot = query
            .await
            .expect("query task joins")
            .expect("reader returns")
            .expect("completed child remains queryable");
        assert!(matches!(
            snapshot.status,
            SubagentSnapshotStatus::Completed { .. }
        ));
    }
    let ninth = tokio::spawn({
        let backend = backend.clone();
        async move { backend.query("held-output-8", false, None).await }
    });
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(5), reads_started.recv())
            .await
            .expect("retry starts after a worker permit is released")
            .as_deref(),
        Some("held-output-8")
    );
    output_read_gate.add_permits(1);
    let ninth = ninth
        .await
        .expect("retry task joins")
        .expect("released output capacity allows retry")
        .expect("ninth child remains queryable");
    assert!(matches!(
        ninth.status,
        SubagentSnapshotStatus::Completed { .. }
    ));
    actor.abort();
}

#[tokio::test]
async fn persisted_output_worker_panic_does_not_destroy_the_coordinator_owner() {
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (finish, _) = tokio::sync::broadcast::channel(2);
    let (started_tx, mut started) = mpsc::unbounded_channel();
    let (reads_tx, mut reads_started) = mpsc::unbounded_channel();
    let output_read_gate = Arc::new(tokio::sync::Semaphore::new(0));
    let actor = tokio::spawn(
        SubagentCoordinator::new(
            command_rx,
            OutputReadRunner {
                finish: finish.clone(),
                started: started_tx,
                output_reads_started: reads_tx,
                output_read_gate: output_read_gate.clone(),
            },
            CoordinatorConfig::default(),
        )
        .run(),
    );
    let backend = ChannelBackend::for_session(commands, "parent");
    let spawn = tokio::spawn({
        let backend = backend.clone();
        async move { backend.spawn(request("panic-output", true)).await }
    });
    assert_eq!(started.recv().await.as_deref(), Some("panic-output"));
    finish
        .send(())
        .expect("child is awaiting terminal completion");
    assert!(spawn.await.unwrap().unwrap().success);

    let query = tokio::spawn({
        let backend = backend.clone();
        async move { backend.query("panic-output", false, None).await }
    });
    assert_eq!(reads_started.recv().await.as_deref(), Some("panic-output"));
    let inspect = tokio::spawn({
        let backend = backend.clone();
        async move { backend.inspect("panic-output").await }
    });
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    output_read_gate.add_permits(1);
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(5), query)
            .await
            .expect("panic settles the public query")
            .expect("query task joins"),
        Err(SubagentObservationError::Unavailable)
    ));
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(5), inspect)
            .await
            .expect("deduplicated inspect is settled")
            .expect("inspect task joins"),
        Err(SubagentObservationError::Unavailable)
    ));
    assert!(
        matches!(
            backend.list_running("parent").await,
            Ok(inspections) if inspections.is_empty()
        ),
        "worker panic must settle this read without killing the coordinator owner"
    );

    let followup_spawns = (0..8)
        .map(|index| {
            let backend = backend.clone();
            tokio::spawn(async move {
                backend
                    .spawn(request(&format!("after-panic-{index}"), true))
                    .await
            })
        })
        .collect::<Vec<_>>();
    let mut followup_ids = std::collections::HashSet::new();
    while followup_ids.len() < 8 {
        followup_ids.insert(
            started
                .recv()
                .await
                .expect("coordinator still starts children"),
        );
    }
    finish
        .send(())
        .expect("follow-up children are waiting for completion");
    for spawn in followup_spawns {
        assert!(spawn.await.unwrap().unwrap().success);
    }
    let queries = (0..8)
        .map(|index| {
            let backend = backend.clone();
            tokio::spawn(async move {
                backend
                    .query(&format!("after-panic-{index}"), false, None)
                    .await
            })
        })
        .collect::<Vec<_>>();
    let mut reads_after_panic = std::collections::HashSet::new();
    while reads_after_panic.len() < 8 {
        reads_after_panic.insert(
            tokio::time::timeout(std::time::Duration::from_secs(5), reads_started.recv())
                .await
                .expect("all eight readers can acquire permits after panic")
                .expect("reader owner remains live"),
        );
    }
    assert_eq!(reads_after_panic.len(), 8);
    output_read_gate.add_permits(8);
    for query in queries {
        assert!(matches!(
            query.await.unwrap().unwrap().unwrap().status,
            SubagentSnapshotStatus::Completed { .. }
        ));
    }
    actor.abort();
}

async fn discover_active_ids(harness: &Harness) -> Vec<String> {
    use crate::implementations::grow_build::subagent::ListSubagentTool;
    use crate::implementations::grow_build::task::backend::SubagentBackendResource;
    use crate::types::resources::Resources;
    use crate::types::tool_metadata::test_ctx;
    use tool_types::{ListSubagentInput, SubagentListScope};

    let mut resources = Resources::new();
    resources.insert(SubagentBackendResource(Arc::new(parent_backend(harness))));
    tool_runtime::Tool::run(
        &ListSubagentTool,
        test_ctx(resources.into_shared()),
        ListSubagentInput {
            scope: SubagentListScope::Active,
        },
    )
    .await
    .expect("public active list succeeds")
    .subagents
    .into_iter()
    .map(|item| item.subagent_id)
    .collect()
}

#[tokio::test]
async fn public_active_discovery_distinguishes_forgotten_from_finished_children() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("model-forgot-id", true)).await }
    });
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("model-forgot-id")
    );
    assert_eq!(discover_active_ids(&harness).await, ["model-forgot-id"]);

    harness.finish.send(()).expect("child is still active");
    assert!(
        spawn
            .await
            .expect("spawn task joins")
            .expect("child succeeds")
            .success
    );
    let _ = harness.completions.recv().await;
    assert!(
        discover_active_ids(&harness).await.is_empty(),
        "finished child is no longer active"
    );
    assert!(matches!(
        parent_backend(&harness)
            .query("model-forgot-id", false, None)
            .await,
        Ok(Some(SubagentSnapshot {
            status: SubagentSnapshotStatus::Completed { .. },
            ..
        }))
    ));
    harness.actor.abort();
}

#[tokio::test]
async fn active_discovery_distinguishes_forgotten_id_retired_checkpoint_and_lost_owner() {
    let mut harness = harness(true, std::time::Duration::from_secs(60));
    let active = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(request("forgotten-active", true)).await }
    });
    let retired = tokio::spawn({
        let backend = harness.backend.clone();
        async move {
            backend
                .spawn(request("retired-suspended-child", true))
                .await
        }
    });
    let requested = [
        harness
            .requests
            .recv()
            .await
            .expect("active request admitted")
            .id,
        harness
            .requests
            .recv()
            .await
            .expect("retiring request admitted")
            .id,
    ];
    assert_eq!(
        requested
            .into_iter()
            .collect::<std::collections::HashSet<_>>(),
        ["forgotten-active", "retired-suspended-child"]
            .into_iter()
            .map(str::to_owned)
            .collect()
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
    assert_eq!(freeze_rx.await.unwrap().unwrap().len(), 2);
    harness
        .start
        .send(())
        .expect("both child runners are waiting");
    let started = [
        harness.started.recv().await.expect("active child starts"),
        harness.started.recv().await.expect("retiring child starts"),
    ];
    assert_eq!(
        started
            .into_iter()
            .collect::<std::collections::HashSet<_>>(),
        ["forgotten-active", "retired-suspended-child"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    // The suspended runner reports `started` immediately before returning its
    // suspension outcome. Wait for the coordinator's matching active-count
    // transition before asking the runtime whether it actually retired.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while harness
            .running_counts
            .recv()
            .await
            .expect("coordinator count channel remains open")
            != 2
        {}
        assert_eq!(harness.running_counts.recv().await, Some(1));
    })
    .await
    .expect("runner commits suspension before retirement observation");
    let (retirement_tx, retirement_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::CheckSuspendedRetirement {
            root_session_id: "parent".into(),
            operation_id: "suspend-retirement".into(),
            task_ids: vec!["retired-suspended-child".into()],
            respond_to: retirement_tx,
        })
        .unwrap();
    let confirmed_retirement = retirement_rx.await.unwrap();
    assert_eq!(confirmed_retirement, Ok(true));
    assert_eq!(
        discover_active_ids(&harness).await,
        ["forgotten-active", "retired-suspended-child"],
        "active discovery retains suspended checkpoints even after physical retirement"
    );
    assert!(
        matches!(
            parent_backend(&harness)
                .query("retired-suspended-child", false, None)
                .await,
            Ok(Some(SubagentSnapshot {
                status: SubagentSnapshotStatus::Suspended { .. },
                ..
            }))
        ),
        "retirement is physical; the coordinator still retains the suspended checkpoint"
    );

    harness
        .finish
        .send(())
        .expect("forgotten active child is live");
    assert!(active.await.unwrap().unwrap().success);
    let _ = harness.completions.recv().await;
    retired.abort();
    let backend = parent_backend(&harness);
    harness.actor.abort();
    let _ = (&mut harness.actor).await;
    assert!(
        matches!(
            backend
                .list_subagents(
                    SubagentListScope::Active,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(5)
                )
                .await,
            Err(SubagentObservationError::Unavailable)
        ),
        "loss of the registry owner must remain unknown, never an empty active list"
    );
}

use super::*;
use crate::implementations::grow_build::set_subagent_model::{
    SetSubagentModelInput, SetSubagentModelTool,
};
use crate::types::{resources::Resources, tool_metadata::test_ctx};
use sampling_types::ReasoningEffort;
use std::time::Duration;

#[tokio::test]
async fn model_control_native_forwards_exact_context_and_selection_without_blocking() {
    let mut harness = harness(false, Duration::from_secs(60));
    let child = request("model-control-context", true);
    let child_cancellation = child.cancel_token.clone();
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(child).await }
    });
    assert_eq!(
        harness.requests.recv().await.unwrap().id,
        "model-control-context"
    );
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("model-control-context")
    );

    let mut resources = Resources::new();
    resources.insert(parent_backend(&harness).into_resource());
    let resources = resources.into_shared();
    let mut last_target_cancellation = None;
    for (model, effort) in [
        (Some("catalog-model"), Some("high")),
        (Some("catalog-model"), None),
        (None, Some("none")),
        (None, Some("medium")),
    ] {
        let caller_cancellation = CancellationToken::new();
        let mut ctx = test_ctx(resources.clone());
        ctx.insert(tool_runtime::Cancellation(caller_cancellation.clone()));
        let call = tool_runtime::Tool::run(
            &SetSubagentModelTool,
            ctx,
            SetSubagentModelInput {
                subagent_id: "model-control-context".into(),
                model: model.map(str::to_owned),
                effort: effort.map(str::to_owned),
            },
        );
        tokio::pin!(call);
        let (request, target, target_cancellation) = tokio::select! {
            forwarded = harness.model_controls.recv() => forwarded.unwrap(),
            result = &mut call => panic!("actor receipt is still pending: {result:?}"),
        };
        assert_eq!(request.source_session_id, "parent");
        assert_eq!(request.subagent_id, "model-control-context");
        assert_eq!(request.model.as_deref(), model);
        assert_eq!(
            request.effort,
            effort.map(|effort| {
                serde_json::from_value::<ReasoningEffort>(serde_json::json!(effort)).unwrap()
            }),
        );
        assert_eq!(target, "exact-runtime-context");
        assert!(!target_cancellation.is_cancelled());
        assert!(!request.cancellation.is_cancelled());
        assert!(futures::poll!(call.as_mut()).is_pending());
        // A host waiting for the actor receipt must leave the mailbox usable.
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), harness.backend.registry_counts())
                .await
                .unwrap()
                .active,
            1,
        );
        caller_cancellation.cancel();
        assert!(request.cancellation.is_cancelled());
        // The host decides the receipt race, and reports the actual actor ACK.
        request.respond_to.send(Ok(())).unwrap();
        let output = call.await.unwrap();
        assert_eq!(
            serde_json::to_value(output).unwrap(),
            serde_json::json!({
                "subagent_id":"model-control-context", "status":"queued",
            })
        );
        last_target_cancellation = Some(target_cancellation);
    }
    assert!(!child_cancellation.is_cancelled());
    assert!(
        harness.requests.try_recv().is_err(),
        "control must not rerun the child"
    );
    child_cancellation.cancel();
    assert!(last_target_cancellation.unwrap().is_cancelled());
    let result = spawn.await.unwrap().unwrap();
    assert!(result.cancelled);
    harness.actor.abort();
}

#[tokio::test]
async fn model_control_rejects_self_sibling_grandchild_foreign_and_unknown() {
    let mut harness = harness(false, Duration::from_secs(60));
    let mut foreign = request("foreign-child", true);
    foreign.parent_session_id = "foreign-parent".into();
    let mut spawns = Vec::new();
    for child in [request("child-a", true), request("child-b", true), foreign] {
        let backend = harness.backend.clone();
        spawns.push(tokio::spawn(async move { backend.spawn(child).await }));
        harness.requests.recv().await.unwrap();
        harness.started.recv().await.unwrap();
    }
    let direct_parent = ChannelBackend::for_session(harness.backend.sender(), "child-a");
    spawns.push(tokio::spawn({
        let backend = direct_parent.clone();
        async move { backend.spawn(request("grandchild", true)).await }
    }));
    harness.requests.recv().await.unwrap();
    assert_eq!(harness.started.recv().await.as_deref(), Some("grandchild"));
    let root = parent_backend(&harness);
    assert!(
        root.query("grandchild", false, None)
            .await
            .unwrap()
            .is_some()
    );

    for (source, target) in [
        ("parent", "parent"),
        ("child-a", "child-a"),
        ("child-a", "child-b"),
        ("parent", "grandchild"),
        ("parent", "foreign-child"),
        ("foreign-parent", "child-a"),
        ("parent", "unknown"),
        ("parent", "test child"),
    ] {
        let backend = ChannelBackend::for_session(harness.backend.sender(), source);
        assert!(
            backend
                .set_model(
                    target.into(),
                    Some("catalog-model".into()),
                    None,
                    CancellationToken::new(),
                )
                .await
                .is_err(),
            "{source} must not control {target}"
        );
        assert!(harness.model_controls.try_recv().is_err());
    }
    // Root visibility does not displace the immutable direct parent's authority.
    let call = direct_parent.set_model(
        "grandchild".into(),
        None,
        Some(ReasoningEffort::None),
        CancellationToken::new(),
    );
    tokio::pin!(call);
    let (request, target, _) = tokio::select! {
        forwarded = harness.model_controls.recv() => forwarded.unwrap(),
        result = &mut call => panic!("direct parent must reach the host: {result:?}"),
    };
    assert_eq!(request.source_session_id, "child-a");
    assert_eq!(request.subagent_id, "grandchild");
    assert_eq!(target, "grandchild");
    request.respond_to.send(Ok(())).unwrap();
    call.await.unwrap();
    assert!(harness.requests.try_recv().is_err());
    assert_eq!(harness.backend.registry_counts().await.active, 4);
    harness.finish.send(()).unwrap();
    for spawn in spawns {
        assert!(spawn.await.unwrap().unwrap().success);
    }
    harness.actor.abort();
}

#[tokio::test]
async fn model_control_rejects_initializing_cancelled_and_abandoned_requests() {
    let mut harness = harness_with_options(
        true,
        true,
        CoordinatorConfig {
            foreground_budget: Duration::from_secs(60),
        },
    );
    let child = request("control-pending", true);
    let child_cancellation = child.cancel_token.clone();
    let spawn = tokio::spawn({
        let backend = harness.backend.clone();
        async move { backend.spawn(child).await }
    });
    harness.requests.recv().await.unwrap();
    let backend = parent_backend(&harness);
    assert!(
        backend
            .set_model(
                "control-pending".into(),
                Some("catalog-model".into()),
                None,
                CancellationToken::new(),
            )
            .await
            .is_err()
    );
    assert_eq!(harness.backend.registry_counts().await.pending, 1);
    assert!(!child_cancellation.is_cancelled());
    assert!(harness.started.try_recv().is_err());
    assert!(harness.model_controls.try_recv().is_err());

    harness.start.send(()).unwrap();
    harness.started.recv().await.unwrap();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let (respond_to, response) = tokio::sync::oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::SetModel(SubagentModelControlRequest {
            source_session_id: "parent".into(),
            subagent_id: "control-pending".into(),
            model: Some("catalog-model".into()),
            effort: None,
            cancellation,
            respond_to,
        }))
        .unwrap();
    assert!(response.await.unwrap().is_err());
    let (respond_to, response) = tokio::sync::oneshot::channel();
    drop(response);
    harness
        .backend
        .sender()
        .send(SubagentEvent::SetModel(SubagentModelControlRequest {
            source_session_id: "parent".into(),
            subagent_id: "control-pending".into(),
            model: Some("catalog-model".into()),
            effort: None,
            cancellation: CancellationToken::new(),
            respond_to,
        }))
        .unwrap();
    assert_eq!(harness.backend.registry_counts().await.active, 1);
    assert!(!child_cancellation.is_cancelled());
    assert!(harness.model_controls.try_recv().is_err());

    child_cancellation.cancel();
    assert!(
        backend
            .set_model(
                "control-pending".into(),
                Some("catalog-model".into()),
                None,
                CancellationToken::new(),
            )
            .await
            .is_err()
    );
    assert_eq!(harness.backend.registry_counts().await.active, 1);
    assert!(harness.model_controls.try_recv().is_err());
    assert!(harness.requests.try_recv().is_err());
    harness.finish.send(()).unwrap();
    assert!(spawn.await.unwrap().unwrap().cancelled);
    harness.actor.abort();
}

#[tokio::test]
async fn model_control_rejects_execution_returned_and_workflow() {
    let mut harness = harness(false, Duration::from_secs(60));
    let mut workflow = request("control-workflow", true);
    workflow.owner = SubagentOwner::workflow("workflow-run");
    let mut spawns = Vec::new();
    for child in [request("execution-returned", true), workflow] {
        let backend = harness.backend.clone();
        spawns.push(tokio::spawn(async move { backend.spawn(child).await }));
        harness.requests.recv().await.unwrap();
        harness.started.recv().await.unwrap();
    }
    let snapshot = harness
        .backend
        .query("execution-returned", false, None)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        snapshot.status,
        SubagentSnapshotStatus::FinalizationPending { .. }
    ));
    for target in ["execution-returned", "control-workflow"] {
        assert!(
            parent_backend(&harness)
                .set_model(
                    target.into(),
                    None,
                    Some(ReasoningEffort::High),
                    CancellationToken::new(),
                )
                .await
                .is_err()
        );
    }
    assert_eq!(harness.backend.registry_counts().await.active, 2);
    assert!(harness.model_controls.try_recv().is_err());
    assert!(harness.requests.try_recv().is_err());
    harness.finish.send(()).unwrap();
    for spawn in spawns {
        assert!(spawn.await.unwrap().unwrap().success);
    }
    harness.actor.abort();
}

#[tokio::test]
async fn model_control_rejects_suspended_completed_and_failed_without_restoring() {
    let mut harness = harness(true, Duration::from_secs(60));
    let mut spawns = Vec::new();
    for id in [
        "retired-suspended-child",
        "control-completed",
        "failed-status",
    ] {
        let backend = harness.backend.clone();
        spawns.push(tokio::spawn(async move {
            backend.spawn(request(id, true)).await
        }));
        harness.requests.recv().await.unwrap();
    }
    let (respond_to, response) = tokio::sync::oneshot::channel();
    harness
        .backend
        .sender()
        .send(SubagentEvent::FreezeAdmission {
            root_session_id: "parent".into(),
            operation_id: "suspend-retirement".into(),
            respond_to,
        })
        .unwrap();
    assert_eq!(response.await.unwrap().unwrap().len(), 3);
    harness.start.send(()).unwrap();
    for _ in 0..3 {
        harness.started.recv().await.unwrap();
    }
    harness.finish.send(()).unwrap();
    // A suspended spawn has no completion result; its lifecycle remains open.
    let suspended_spawn = spawns.remove(0);
    for spawn in spawns {
        spawn.await.unwrap().unwrap();
    }
    let suspended = harness
        .backend
        .query("retired-suspended-child", false, None)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        suspended.status,
        SubagentSnapshotStatus::Suspended { .. }
    ));
    assert!(matches!(
        harness
            .backend
            .query("control-completed", false, None)
            .await
            .unwrap()
            .unwrap()
            .status,
        SubagentSnapshotStatus::Completed { .. },
    ));
    assert!(matches!(
        harness
            .backend
            .query("failed-status", false, None)
            .await
            .unwrap()
            .unwrap()
            .status,
        SubagentSnapshotStatus::Failed { .. },
    ));
    for target in [
        "retired-suspended-child",
        "control-completed",
        "failed-status",
    ] {
        assert!(
            parent_backend(&harness)
                .set_model(
                    target.into(),
                    Some("catalog-model".into()),
                    None,
                    CancellationToken::new(),
                )
                .await
                .is_err()
        );
    }
    assert_eq!(harness.backend.registry_counts().await.active, 0);
    assert!(harness.model_controls.try_recv().is_err());
    assert!(harness.requests.try_recv().is_err());
    assert!(harness.started.try_recv().is_err());
    suspended_spawn.abort();
    harness.actor.abort();
}

#[tokio::test]
async fn model_control_rejects_uncommitted_terminal_finalization_without_rerun() {
    let mut harness = harness(false, Duration::from_secs(60));
    harness.terminal_commit_ready.store(false, Ordering::SeqCst);
    let result = complete_background_child(&mut harness, "control-finalization-pending").await;
    assert!(result.finalization_pending);
    assert!(
        parent_backend(&harness)
            .set_model(
                "control-finalization-pending".into(),
                None,
                Some(ReasoningEffort::High),
                CancellationToken::new(),
            )
            .await
            .is_err()
    );
    assert_eq!(harness.backend.registry_counts().await.active, 0);
    assert_eq!(harness.reconcile_calls.load(Ordering::SeqCst), 0);
    assert!(harness.model_controls.try_recv().is_err());
    assert!(harness.requests.try_recv().is_err());
    assert!(harness.started.try_recv().is_err());
    harness.actor.abort();
}

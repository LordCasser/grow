use super::*;
use crate::implementations::grow_build::task::types::SubagentEvent;

/// The coordinator freeze and the runner's terminal edge share one actor. A
/// child observed as open by freeze may finish before retirement is checked;
/// its owner must be joined exactly once, then the original frozen task is
/// considered retired without asking that terminal child for a checkpoint.
#[tokio::test]
async fn child_finishing_after_freeze_is_joined_once_before_retirement() {
    let mut harness = harness(false, std::time::Duration::from_secs(60));
    let backend = harness.backend.clone();
    let spawn =
        tokio::spawn(async move { backend.spawn(request("terminal-during-freeze", true)).await });
    assert_eq!(
        harness.requests.recv().await.unwrap().id,
        "terminal-during-freeze"
    );
    assert_eq!(
        harness.started.recv().await.as_deref(),
        Some("terminal-during-freeze")
    );

    let sender = harness.backend.sender();
    let (freeze_tx, freeze_rx) = tokio::sync::oneshot::channel();
    sender
        .send(SubagentEvent::FreezeAdmission {
            root_session_id: "parent".into(),
            operation_id: "freeze-child-terminal-race".into(),
            respond_to: freeze_tx,
        })
        .unwrap();
    let frozen = freeze_rx.await.unwrap().unwrap();
    assert_eq!(frozen.len(), 1);
    assert_eq!(frozen[0].task_id, "terminal-during-freeze");
    assert!(!frozen[0].initializing);

    // Exchange the admission order on the same coordinator: the original
    // child is in the frozen membership; a Task arriving after that ACK
    // cannot reach the runner, even if a normal turn tries to open spawning.
    assert!(parent_backend(&harness).open_spawn_admission());
    let rejected = harness
        .backend
        .spawn(request("late-task-after-freeze", true))
        .await
        .unwrap();
    assert!(rejected.not_dispatched);
    assert!(!rejected.success);
    assert!(harness.requests.try_recv().is_err());
    assert!(harness.started.try_recv().is_err());

    let check_retirement = || {
        let sender = sender.clone();
        async move {
            let (respond_to, response) = tokio::sync::oneshot::channel();
            sender
                .send(SubagentEvent::CheckSuspendedRetirement {
                    root_session_id: "parent".into(),
                    operation_id: "freeze-child-terminal-race".into(),
                    task_ids: vec!["terminal-during-freeze".into()],
                    respond_to,
                })
                .unwrap();
            response.await.unwrap()
        }
    };
    assert_eq!(check_retirement().await, Ok(false));

    harness.finish.send(()).unwrap();
    let result = spawn.await.unwrap().unwrap();
    assert!(result.success);
    assert_eq!(result.child_session_id, "terminal-during-freeze");
    assert!(harness.completions.recv().await.is_some());

    let terminal = harness
        .backend
        .query("terminal-during-freeze", false, None)
        .await
        .unwrap()
        .expect("completed child remains observable");
    assert!(matches!(
        terminal.status,
        SubagentSnapshotStatus::Completed { .. }
    ));
    assert_eq!(check_retirement().await, Ok(true));
    assert_eq!(check_retirement().await, Ok(true));
    assert!(
        harness.completions.try_recv().is_err(),
        "the child terminal callback is delivered once"
    );

    harness.actor.abort();
}

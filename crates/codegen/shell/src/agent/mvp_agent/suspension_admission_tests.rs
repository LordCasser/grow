//! Exercise tree ownership across failures before any node checkpoint.
use super::*;

async fn retry_freeze_after_failure(lost_ack: bool) {
    use chat_state::session_runtime::RuntimePhase;
    use tools::implementations::grow_build::task::types::{FrozenChildRef, SubagentEvent};
    let (events, _events_rx) = tokio::sync::mpsc::unbounded_channel();
    let config = serde_json::from_value(serde_json::json!({
        "base_url":"http://localhost", "model":"test", "context_window":256000
    }))
    .unwrap();
    let chat = chat_state::ChatStateActor::spawn(
        vec![],
        config,
        Box::new(chat_state::NullTimelinePersistence),
        events,
        tokio_util::sync::CancellationToken::new(),
    );
    let (mut handle, mut commands) = make_test_handle_with_receiver("test", None);
    handle.chat_state_handle = chat.clone();
    let root = handle.info.id.clone();
    let agent = build_minimal_agent_for_tests();
    agent.sessions.borrow_mut().insert(root.clone(), handle);
    let mut requests = agent.subagent_event_rx.borrow_mut().take().unwrap();
    let retrying = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let transport_retrying = retrying.clone();
    let durable_chat = chat.clone();
    let coordinator = tokio::spawn(async move {
        let mut owner = None;
        loop {
            let Some(SubagentEvent::FreezeAdmission {
                operation_id,
                respond_to,
                ..
            }) = requests.recv().await
            else {
                panic!("expected freeze");
            };
            let runtime = durable_chat.runtime_state().await.unwrap();
            assert_eq!(runtime.phase, RuntimePhase::Suspending);
            assert_eq!(
                runtime.operation_id.as_ref(),
                Some(&operation_id),
                "freeze must follow durable intent"
            );
            if let Some(first) = &owner {
                assert_eq!(first, &operation_id);
            } else {
                owner = Some(operation_id.clone());
            }
            if transport_retrying.load(std::sync::atomic::Ordering::Acquire) {
                respond_to.send(Ok(vec![])).unwrap();
                break operation_id;
            }
            if lost_ack {
                drop(respond_to);
            } else {
                let _ = respond_to.send(Ok(vec![FrozenChildRef {
                    task_id: "initializing-child".into(),
                    session_id: "initializing-child".into(),
                    security_parent_session_id: "test".into(),
                    origin_tool_call_id: None,
                    initializing: true,
                }]));
            }
        }
    });
    let first = agent
        .suspend_session_tree(
            &root,
            tokio::time::Instant::now() + std::time::Duration::from_millis(60),
        )
        .await
        .unwrap_err();
    assert!(
        first.contains(if lost_ack {
            "acknowledgement was lost"
        } else {
            "child initialization is still settling"
        }),
        "{first}"
    );
    let saved = chat.runtime_state().await.unwrap();
    assert_eq!(saved.phase, RuntimePhase::Suspending);
    retrying.store(true, std::sync::atomic::Ordering::Release);
    let node_owner = saved.operation_id.clone().unwrap();
    let node = tokio::spawn(async move {
        let Some(SessionCommand::SuspendRuntimeNode {
            operation_id,
            respond_to,
            ..
        }) = commands.recv().await
        else {
            panic!("retry must advance to node checkpoint");
        };
        assert_eq!(operation_id, node_owner);
        respond_to
            .send(Err("injected checkpoint stop".into()))
            .unwrap();
    });
    let second = agent
        .suspend_session_tree(
            &root,
            tokio::time::Instant::now() + std::time::Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    assert!(second.contains("injected checkpoint stop"), "{second}");
    assert_eq!(Some(coordinator.await.unwrap()), saved.operation_id);
    node.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn tree_freeze_initializer_timeout_retries_durable_owner() {
    retry_freeze_after_failure(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn tree_freeze_lost_ack_retries_durable_owner() {
    retry_freeze_after_failure(true).await;
}

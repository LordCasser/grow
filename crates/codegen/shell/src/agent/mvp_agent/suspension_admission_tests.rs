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

async fn drive_exit_scope(agent: &MvpAgent, scope: Option<(u64, Vec<&str>)>) -> serde_json::Value {
    use acp_transport::AcpAgentHandler as _;
    let mut params = serde_json::json!({"exit_id":uuid::Uuid::now_v7().to_string()});
    if let Some((client_id, ids)) = scope {
        params["leaderScope"] = serde_json::json!({"clientId":client_id,"sessionIds":ids});
    }
    let response = agent
        .ext_method(acp::ExtRequest::new(
            "grow/session/suspend_for_exit",
            serde_json::value::to_raw_value(&params).unwrap().into(),
        ))
        .await
        .unwrap();
    serde_json::from_str(response.0.get()).unwrap()
}

#[test]
fn exit_scopes_do_not_coalesce_and_wait_for_concurrent_close_writer() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("exit-close-owned");
        let (handle, _tx, mut commands) = make_live_session_handle(&sid, None);
        agent.sessions.borrow_mut().insert(sid.clone(), handle);
        let other = acp::SessionId::new("exit-peer");
        let (peer, _peer_tx, _peer_rx) = make_live_session_handle(&other, None);
        let peer_suspend = peer.runtime_suspend.clone();
        agent.sessions.borrow_mut().insert(other.clone(), peer);
        let (release, waiting) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let _ = waiting.recv();
        });
        agent.session_threads.borrow_mut().insert(
            sid.clone(),
            crate::session::SessionThread::from_handle(writer),
        );
        let mut closing = Box::pin(drive_close(&agent, sid.0.as_ref()));
        assert!(futures::poll!(closing.as_mut()).is_pending());
        assert!(matches!(
            commands.try_recv(),
            Ok(TestSessionCommand::Shutdown)
        ));
        assert!(!agent.sessions.borrow().contains_key(&sid));
        let mut first = Box::pin(drive_exit_scope(&agent, Some((11, vec![sid.0.as_ref()]))));
        assert!(futures::poll!(first.as_mut()).is_pending());
        tokio::task::yield_now().await;
        let second = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            drive_exit_scope(&agent, Some((22, vec![]))),
        )
        .await
        .expect("independent scope cannot wait for client 11");
        assert_eq!(second["status"], "suspended");
        assert!(!peer_suspend.load(std::sync::atomic::Ordering::Acquire));
        assert!(agent.session_threads.borrow().contains_key(&sid));
        let mut retry = Box::pin(drive_exit_scope(&agent, Some((11, vec![sid.0.as_ref()]))));
        assert!(futures::poll!(retry.as_mut()).is_pending());
        assert_eq!(
            agent.exit_suspend.borrow().len(),
            2,
            "same-scope retry reuses the pending owner"
        );
        release.send(()).unwrap();
        closing.await.expect("terminal close confirms writer drain");
        assert_eq!(first.await["status"], "suspended");
        assert_eq!(retry.await["status"], "suspended");
        assert!(!agent.session_threads.borrow().contains_key(&sid));
        assert!(agent.sessions.borrow().contains_key(&other));
    });
}

#[test]
fn standalone_exit_waits_for_slow_nonresident_writer() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("exit-draining");
        let (release, waiting) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let _ = waiting.recv();
        });
        agent.session_threads.borrow_mut().insert(
            sid.clone(),
            crate::session::SessionThread::from_handle(writer),
        );
        let mut exit = Box::pin(drive_exit_scope(&agent, None));
        assert!(futures::poll!(exit.as_mut()).is_pending());
        tokio::time::pause();
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(6)).await;
        tokio::task::yield_now().await;
        tokio::time::resume();
        assert!(
            futures::poll!(exit.as_mut()).is_pending(),
            "absent view/actor is not a drained writer"
        );
        release.send(()).unwrap();
        assert_eq!(exit.await["status"], "suspended");
        assert!(!agent.session_threads.borrow().contains_key(&sid));
    });
}

#[test]
fn exit_waits_for_lifecycle_owner_to_release() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let sid = acp::SessionId::new("exit-locked");
        let owner = agent.lock_session_lifecycle(&sid).await;
        let mut exit = Box::pin(drive_exit_scope(&agent, Some((31, vec![sid.0.as_ref()]))));
        assert!(futures::poll!(exit.as_mut()).is_pending());
        tokio::time::pause();
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(6)).await;
        tokio::task::yield_now().await;
        assert!(futures::poll!(exit.as_mut()).is_pending());
        tokio::time::resume();
        drop(owner);
        assert_eq!(exit.await["status"], "suspended");
    });
}

#[test]
fn exit_retry_polls_a_completed_owner_after_its_rpc_waiter_disappears() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let mut abandoned = Box::pin(drive_exit_scope(&agent, Some((41, vec![]))));
        assert!(futures::poll!(abandoned.as_mut()).is_pending());
        drop(abandoned);
        tokio::task::yield_now().await;
        let sid = acp::SessionId::new("exit-after-lost-waiter");
        let (release, waiting) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let _ = waiting.recv();
        });
        agent.session_threads.borrow_mut().insert(
            sid.clone(),
            crate::session::SessionThread::from_handle(writer),
        );
        let mut retry = Box::pin(drive_exit_scope(&agent, Some((41, vec![sid.0.as_ref()]))));
        assert!(
            futures::poll!(retry.as_mut()).is_pending(),
            "new exit cannot reuse a completed earlier scope"
        );
        tokio::task::yield_now().await;
        assert!(futures::poll!(retry.as_mut()).is_pending());
        release.send(()).unwrap();
        assert_eq!(retry.await["status"], "suspended");
        assert!(!agent.session_threads.borrow().contains_key(&sid));
    });
}

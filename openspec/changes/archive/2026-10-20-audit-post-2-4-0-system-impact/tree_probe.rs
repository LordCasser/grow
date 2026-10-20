// Audit probe: real caller; transport models a child still initializing at the deadline.
use super::*;
#[tokio::test(flavor = "current_thread")]
async fn audit_post_240_tree_freeze_retry_changes_uncommitted_identity() {
    use chat_state::session_runtime::RuntimePhase;
    use tools::implementations::grow_build::task::types::{FrozenChildRef, SubagentEvent};
    let (events, _events_rx) = tokio::sync::mpsc::unbounded_channel();
    let config = serde_json::from_value(
        serde_json::json!({"base_url":"http://localhost", "model":"test", "context_window":256000}),
    )
    .unwrap();
    let chat = chat_state::ChatStateActor::spawn(
        vec![],
        config,
        Box::new(chat_state::NullTimelinePersistence),
        events,
        tokio_util::sync::CancellationToken::new(),
    );
    let (mut handle, _commands) = make_test_handle_with_receiver("test", None);
    handle.chat_state_handle = chat.clone();
    let root = handle.info.id.clone();
    let agent = build_minimal_agent_for_tests();
    agent.sessions.borrow_mut().insert(root.clone(), handle);
    let mut requests = agent.subagent_event_rx.borrow_mut().take().unwrap();
    let coordinator = tokio::spawn(async move {
        let Some(SubagentEvent::FreezeAdmission {
            operation_id: first,
            respond_to,
            ..
        }) = requests.recv().await
        else {
            panic!("expected freeze");
        };
        let frozen = || {
            vec![FrozenChildRef {
                task_id: "initializing-child".into(),
                session_id: "initializing-child".into(),
                security_parent_session_id: "test".into(),
                origin_tool_call_id: None,
                initializing: true,
            }]
        };
        let _ = respond_to.send(Ok(frozen()));
        loop {
            let Some(SubagentEvent::FreezeAdmission {
                operation_id: second,
                respond_to,
                ..
            }) = requests.recv().await
            else {
                panic!("expected retry freeze");
            };
            if second == first {
                let _ = respond_to.send(Ok(frozen()));
                continue;
            }
            let _ = respond_to.send(Err("conflicting suspension owner".into()));
            break (first, second);
        }
    });
    let first = agent
        .suspend_session_tree(
            &root,
            tokio::time::Instant::now() + std::time::Duration::from_millis(50),
        )
        .await
        .unwrap_err();
    assert!(
        first.contains("child initialization is still settling"),
        "{first}"
    );
    assert_eq!(
        chat.runtime_state().await.unwrap().phase,
        RuntimePhase::Running
    );
    let second = agent
        .suspend_session_tree(
            &root,
            tokio::time::Instant::now() + std::time::Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    assert_eq!(second, "conflicting suspension owner");
    let ids = coordinator.await.unwrap();
    eprintln!(
        "AUDIT tree freeze: first={} retry={}, durable phase still Running",
        ids.0, ids.1
    );
}
